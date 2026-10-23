//! Fetch the workflow runs and jobs from the GitHub API.
//!
//! A workflow run has jobs, and each job has steps.  Runs that started at
//! about the same time are grouped into an "event".

use std::collections::HashSet;
use std::future::Future;

use chrono::DateTime;
use chrono::Utc;
use futures::future::join_all;
use serde::Deserialize;

use crate::bucketer::Bucketer;
use crate::utils::Error;
use crate::utils::HumanKey;
use crate::utils::Result;
use crate::utils::human_key;
use crate::utils::parse_dt;

/// Get data from a URL.
pub trait Fetcher: Send + Sync {
    fn fetch<'a>(&'a self, url: &'a str) -> impl Future<Output = Result<String>> + Send + 'a;
}

/// Deserialize a string field, which GitHub may leave as `null`.
fn field<'de, D>(d: D) -> std::result::Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Option::<String>::deserialize(d)?.unwrap_or_default())
}

/// Deserialize a list field, which GitHub may leave as `null`.
fn field_vec<'de, D, T>(d: D) -> std::result::Result<Vec<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Ok(Option::<Vec<T>>::deserialize(d)?.unwrap_or_default())
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct RunList {
    workflow_runs: Vec<Run>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct JobList {
    jobs: Vec<Job>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct Run {
    pub id: u64,
    #[serde(deserialize_with = "field")]
    pub name: String,
    #[serde(deserialize_with = "field")]
    pub status: String,
    #[serde(deserialize_with = "field")]
    pub conclusion: String,
    #[serde(deserialize_with = "field")]
    pub html_url: String,
    #[serde(deserialize_with = "field")]
    pub display_title: String,
    #[serde(deserialize_with = "field")]
    pub head_branch: String,
    #[serde(deserialize_with = "field")]
    pub head_sha: String,
    #[serde(deserialize_with = "field")]
    pub event: String,
    #[serde(deserialize_with = "field")]
    pub run_started_at: String,
    #[serde(deserialize_with = "field")]
    pub jobs_url: String,
    #[serde(skip)]
    pub started_dt: DateTime<Utc>,
    #[serde(skip)]
    pub jobs: Vec<Job>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct Job {
    #[serde(deserialize_with = "field")]
    pub name: String,
    #[serde(deserialize_with = "field")]
    pub status: String,
    #[serde(deserialize_with = "field")]
    pub conclusion: String,
    #[serde(deserialize_with = "field")]
    pub created_at: String,
    #[serde(deserialize_with = "field_vec")]
    pub steps: Vec<Step>,
    #[serde(skip)]
    pub created_dt: DateTime<Utc>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct Step {
    #[serde(deserialize_with = "field")]
    pub name: String,
    #[serde(deserialize_with = "field")]
    pub status: String,
    #[serde(deserialize_with = "field")]
    pub conclusion: String,
}

/// Sort runs newest first.  Keys are computed before sorting, since `defuzz`
/// remembers the times it has seen.
type RunKey = (DateTime<Utc>, String, String, String);

type JobKey = (DateTime<Utc>, HumanKey);

/// Fetch all the workflow runs for `urls`, grouped into events, with their
/// jobs fetched.
pub async fn get_events<F: Fetcher>(
    now: DateTime<Utc>,
    urls: &[String],
    fetch: &F,
    only: Option<&[String]>,
    bucketer: &mut Bucketer,
) -> Result<Vec<Vec<Run>>> {
    // Fetch all the run lists at the same time.
    let mut runs = Vec::new();
    for (url, body) in urls
        .iter()
        .zip(join_all(urls.iter().map(|url| fetch.fetch(url))).await)
    {
        let body = body?;
        let list: RunList = serde_json::from_str(&body)
            .map_err(|e| Error::msg(format!("Bad JSON from {url}: {e}")))?;
        runs.extend(list.workflow_runs);
    }

    // In odd situations (duplicate remotes) we can get the same run more than
    // once.  De-duplicate them, keeping the first copy in the order we saw it.
    let mut seen = HashSet::new();
    runs.retain(|run| seen.insert(run.id));

    for run in &mut runs {
        run.started_dt = parse_dt(&run.run_started_at)?;
    }

    // Sort newest first, the way GitHub's UI shows the runs.
    let mut sorted: Vec<(RunKey, Run)> = runs
        .into_iter()
        .map(|run| {
            let key = (
                bucketer.defuzz(run.started_dt),
                run.head_sha.clone(),
                run.event.clone(),
                run.name.clone(),
            );
            (key, run)
        })
        .collect();
    sorted.sort_by(|a, b| b.0.cmp(&a.0));

    // Group runs that started at about the same time into events.
    let mut groups: Vec<Vec<Run>> = Vec::new();
    for run in sorted.into_iter().map(|(_, run)| run) {
        match groups.last_mut() {
            Some(last) if same_group(last, &run, bucketer) => last.push(run),
            _ => groups.push(vec![run]),
        }
    }

    // Keep only the newest event for each workflow, and only recent ones.
    let mut events = Vec::new();
    let mut run_names_seen = HashSet::from(["Cancel".to_string()]);
    for mut group in groups {
        // If the .yml file couldn't even be parsed, the run name is the name
        // of the .yml file.  Exclude those, or a bad parse will pollute the run list.
        let names: HashSet<String> = group
            .iter()
            .map(|r| r.name.clone())
            .filter(|n| !n.starts_with(".github/"))
            .collect();
        if names.is_subset(&run_names_seen) {
            continue;
        }
        if (now - group[0].started_dt).num_days() > 7 {
            continue;
        }
        if let Some(words) = only {
            group.retain(|r| words.iter().any(|w| r.name.to_lowercase().contains(w)));
            if group.is_empty() {
                continue;
            }
        }
        events.push(group);
        run_names_seen.extend(names);
    }

    // Fetch the jobs for every run we'll show.
    let mut job_urls: Vec<(usize, usize, String)> = Vec::new();
    for (ei, event) in events.iter().enumerate() {
        for (ri, run) in event.iter().enumerate() {
            // GitHub shows only 30 jobs per page by default.
            job_urls.push((ei, ri, format!("{}?per_page=100", run.jobs_url)));
        }
    }
    let bodies = join_all(job_urls.iter().map(|(_, _, url)| fetch.fetch(url))).await;
    let mut first_error = None;
    for ((ei, ri, url), body) in job_urls.into_iter().zip(bodies) {
        let body = match body {
            Ok(body) => body,
            Err(e) => {
                first_error.get_or_insert(e);
                continue;
            }
        };
        match fetch_jobs(&body, &url, bucketer) {
            Ok(jobs) => {
                events[ei][ri].jobs = jobs;
            }
            Err(e) => {
                first_error.get_or_insert(e);
            }
        }
    }
    if let Some(e) = first_error {
        return Err(e);
    }
    Ok(events)
}

/// Whether two runs belong to the same event.
fn same_group(last: &[Run], run: &Run, bucketer: &mut Bucketer) -> bool {
    // `defuzz` is idempotent for times it has already seen.
    bucketer.defuzz(run.started_dt) == bucketer.defuzz(last[0].started_dt)
        && run.head_sha == last[0].head_sha
        && run.event == last[0].event
}

fn fetch_jobs(body: &str, url: &str, bucketer: &mut Bucketer) -> Result<Vec<Job>> {
    let list: JobList =
        serde_json::from_str(body).map_err(|e| Error::msg(format!("Bad JSON from {url}: {e}")))?;
    let mut keyed: Vec<(JobKey, Job)> = Vec::new();
    for mut job in list.jobs {
        job.created_dt = parse_dt(&job.created_at)?;
        keyed.push(((bucketer.defuzz(job.created_dt), human_key(&job.name)), job));
    }
    keyed.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(keyed.into_iter().map(|(_, job)| job).collect())
}
