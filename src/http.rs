//! Get data from URLs.
//!
//! Uses the GITHUB_TOKEN environment variable (if set) as authentication,
//! falling back to an entry in ~/.netrc.
//!
//! Set SAVE_DATA=1 in the environment to save retrieved data in get_* files.

use std::future::Future;
use std::io::{BufReader, Write};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose;
use netrc::Netrc;
use reqwest::StatusCode;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, USER_AGENT};
use serde_json::{Map, Value};

use crate::data::Fetcher;
use crate::utils::Error;
use crate::utils::Result;

/// The responses to retry.
const RETRY_STATUS: [u16; 1] = [502];

/// How many seconds to wait for a response.
const TIMEOUT: Duration = Duration::from_secs(30);

pub struct Http {
    client: reqwest::Client,
    headers: reqwest::header::HeaderMap,
    netrc: Option<Netrc>,
    save: bool,
    count: AtomicUsize,
}

impl Http {
    pub fn new(token: Option<String>, save: bool) -> Result<Self> {
        // SAVE_DATA records everything we fetch, for debugging the API.
        if save {
            std::fs::write("get_index.txt", "# URLs fetched:\n")?;
        }

        let mut headers = reqwest::header::HeaderMap::new();
        let ua = format!("watchgha/{}", env!("CARGO_PKG_VERSION"));
        headers.insert(USER_AGENT, ua.parse().expect("valid user agent"));
        let netrc = match token {
            Some(token) => {
                let value = format!("Bearer {token}")
                    .parse()
                    .map_err(|e| Error::fmt(format!("Bad GITHUB_TOKEN: {e}")))?;
                headers.insert(AUTHORIZATION, value);
                None
            }
            None => netrc_file()?,
        };

        let client = reqwest::Client::builder()
            .timeout(TIMEOUT)
            .build()
            .map_err(|e| Error::fmt(format!("Couldn't build the HTTP client: {e}")))?;

        Ok(Self {
            client,
            headers,
            netrc,
            save,
            count: AtomicUsize::new(0),
        })
    }

    /// Get `url` three times, with retry.
    pub async fn get(&self, url: &str) -> Result<String> {
        let mut error = None;
        for ntry in 0..3u32 {
            match self.get_data(url).await {
                Ok(data) => return Ok(data),
                Err(e) => {
                    error = Some(e);
                    if ntry < 2 {
                        tokio::time::sleep(backoff(ntry)).await;
                    }
                }
            }
        }
        Err(error.expect("always set"))
    }

    async fn get_data(&self, url: &str) -> Result<String> {
        let mut ntry = 0u32;
        let resp = loop {
            let resp = self.request(url).await?;
            if !RETRY_STATUS.contains(&resp.status().as_u16()) || ntry == 2 {
                break resp;
            }
            tokio::time::sleep(backoff(ntry)).await;
            ntry += 1;
        };

        let status = resp.status();
        let content_type = resp
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        let body = resp
            .text()
            .await
            .map_err(|e| Error::fmt(format!("Couldn't read {url}: {e}")))?;
        if !status.is_success() {
            return Err(status_error(url, status, &body));
        }
        if self.save {
            self.save_response(url, content_type.as_deref(), &body)?;
        }
        Ok(body)
    }

    async fn request(&self, url: &str) -> Result<reqwest::Response> {
        let mut req = self.client.get(url).headers(self.headers.clone());
        if let Some(auth) = self.netrc_auth(url) {
            req = req.header(AUTHORIZATION, auth);
        }
        req.send()
            .await
            .map_err(|e| Error::msg(format!("Couldn't get {url:?}: {e}")))
    }

    /// The Basic auth header for a ~/.netrc entry for `url`, if there is one.
    fn netrc_auth(&self, url: &str) -> Option<reqwest::header::HeaderValue> {
        let host = url.parse::<url::Url>().ok()?.host_str()?.to_string();
        let machine = &self
            .netrc
            .as_ref()?
            .hosts
            .iter()
            .find(|(h, _)| *h == host)?
            .1;
        let login = unquote(machine.login.as_str());
        let password = unquote(machine.password.as_deref().unwrap_or_default());
        let token = general_purpose::STANDARD.encode(format!("{login}:{password}"));
        // Base64 output is always a valid header value.
        format!("Basic {token}").parse().ok()
    }

    fn save_response(&self, url: &str, content_type: Option<&str>, body: &str) -> Result<()> {
        let ext = content_type
            .map(|ct| ct.split(';').next().unwrap_or("").trim())
            .map_or("", extension)
            .to_string();
        let n = self.count.fetch_add(1, Ordering::Relaxed);
        let filename = format!("get_{n:03}{ext}");
        std::fs::write(&filename, body)?;
        let mut index = std::fs::OpenOptions::new()
            .append(true)
            .open("get_index.txt")?;
        writeln!(index, "{filename}: {url}")?;
        Ok(())
    }
}

impl Fetcher for Http {
    fn fetch<'a>(&'a self, url: &'a str) -> impl Future<Output = Result<String>> + Send + 'a {
        self.get(url)
    }
}

/// Read the ~/.netrc file, if there is one.
fn netrc_file() -> Result<Option<Netrc>> {
    let Some(path) = std::env::home_dir().map(|dir| dir.join(".netrc")) else {
        return Ok(None);
    };
    let Ok(file) = std::fs::File::open(&path) else {
        return Ok(None);
    };
    Netrc::parse(BufReader::new(file))
        .map(Some)
        .map_err(|e| Error::msg(format!("Couldn't parse {}: {e:?}", path.display())))
}

/// Strip the double quotes that RFC 1961 allows around a .netrc value.
fn unquote(s: &str) -> &str {
    let s = s.strip_prefix('"').unwrap_or(s);
    s.strip_suffix('"').unwrap_or(s)
}

fn backoff(ntry: u32) -> Duration {
    Duration::from_millis(50u64 * 2u64.pow(ntry))
}

fn status_error(url: &str, status: StatusCode, body: &str) -> Error {
    let mut msg = format!("HTTP {status} for url {url}");
    match serde_json::from_str::<Map<String, Value>>(body) {
        // GitHub's errors are JSON objects, one line per field.
        Ok(fields) => {
            for (label, value) in fields {
                msg.push_str(&format!("\n{label}: {value}"));
            }
        }
        Err(_) => msg.push_str(&format!("\n{body}")),
    }
    Error::msg(msg)
}
/// The file extension for a content type, like `mimetypes.guess_extension`.
fn extension(content_type: &str) -> &str {
    match content_type {
        "application/json" => ".json",
        "application/xml" | "text/xml" => ".xml",
        "text/html" => ".html",
        "text/plain" => ".txt",
        _ => "",
    }
}
