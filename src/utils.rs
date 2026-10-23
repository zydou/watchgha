//! Small helpers: errors, datetime formatting, sorting keys.

use std::sync::OnceLock;
use std::time::Duration;
use std::time::Instant;

use chrono::DateTime;
use chrono::Datelike;
use chrono::Local;
use chrono::Utc;
use regex::Regex;

/// Every error in this program is shown to the user, so there's no need for
/// kinds of errors: just the message.
#[derive(Debug, Clone)]
pub struct Error(pub String);

impl Error {
    pub fn msg<S: Into<String>>(s: S) -> Self {
        Self(s.into())
    }

    pub fn fmt<S: std::fmt::Display>(s: S) -> Self {
        Self(s.to_string())
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Self(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, Error>;

/// Format a datetime the way GitHub's UI does: no date if it's today, just the
/// month and day if it's earlier this year, the full date otherwise.
pub fn nice_time(dt: DateTime<Utc>, now: DateTime<Local>) -> String {
    let dt = dt.with_timezone(&Local);
    let fmt = if dt.date_naive() != now.date_naive() {
        if dt.year() != now.year() {
            "%Y-%m-%d %I:%M%p"
        } else {
            "%m-%d %I:%M%p"
        }
    } else {
        "%I:%M%p"
    };
    dt.format(fmt).to_string().to_lowercase()
}

/// Parse a GitHub datetime, like "2026-01-18T16:55:30Z".
pub fn parse_dt(s: &str) -> Result<DateTime<Utc>> {
    // Some Pythons don't accept Z, so it's replaced.  Rust doesn't need that,
    // but GitHub sends it, and %f needs the offset.
    let s = s.replace('Z', "+00:00");
    match DateTime::parse_from_rfc3339(&s) {
        Ok(dt) => Ok(dt.with_timezone(&Utc)),
        Err(e) => Err(Error::fmt(format!("Couldn't parse datetime {s:?}: {e}"))),
    }
}

/// One piece of a natural sort key.  Numbers sort before strings.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum KeyPart {
    Num(i64),
    Str(String),
}

/// A value that sorts the way humans expect: numbers by value, not by char.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct HumanKey(pub Vec<KeyPart>, pub String);

/// Turn a string into a `HumanKey`.
///
/// "z23A" -> (["z", 23, "a"], "z23A")
///
/// The original string is appended as a last value to ensure the key is
/// unique enough so that "x1y" and "x001y" can be distinguished.
pub fn human_key(s: &str) -> HumanKey {
    let folded = s.to_lowercase();
    let mut parts = Vec::new();
    let mut last = 0;
    for m in split_re().find_iter(&folded) {
        parts.push(KeyPart::Str(folded[last..m.start()].to_string()));
        // `(\d+)` guarantees digits, so this only fails on overflow.
        let n = m.as_str().parse::<i64>().unwrap_or(i64::MAX);
        parts.push(KeyPart::Num(n));
        last = m.end();
    }
    parts.push(KeyPart::Str(folded[last..].to_string()));
    HumanKey(parts, s.to_string())
}

fn split_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(\d+)").unwrap())
}

/// Wait for an interval of time.
///
/// Better than `thread::sleep` because it accounts for time spent doing things
/// other than sleeping.
pub struct Interval {
    secs: u64,
    last: Instant,
}

impl Interval {
    pub fn new(secs: u64) -> Self {
        Self {
            secs,
            last: Instant::now(),
        }
    }

    /// How much of the current interval is left.
    pub fn remaining(&self) -> Duration {
        Duration::from_secs(self.secs).saturating_sub(self.last.elapsed())
    }

    /// Start the next interval now.
    pub fn reset(&mut self) {
        self.last = Instant::now();
    }
}

/// Pad `s` to at least `width` characters, like Python's f"{s:16}".
pub fn pad(s: &str, width: usize) -> String {
    let n = s.chars().count();
    if n >= width {
        s.to_string()
    } else {
        format!("{s}{}", " ".repeat(width - n))
    }
}

/// Remove control characters, to prevent terminal escape attacks.
pub fn scrub(s: &str) -> String {
    s.chars()
        .filter(|c| {
            let n = *c as u32;
            !(n <= 0x1f || (0x7f..=0x9f).contains(&n))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pad_pads_by_characters() {
        assert_eq!(pad("abc", 5), "abc  ");
        assert_eq!(pad("abcdef", 5), "abcdef");
        assert_eq!(pad("é", 3), "é  ");
        assert_eq!(pad("", 3), "   ");
    }

    #[test]
    fn scrub_removes_control_chars() {
        assert_eq!(scrub("a\x1b[1mMALICIOUS\x1b[0mb"), "a[1mMALICIOUS[0mb");
        assert_eq!(scrub("ok"), "ok");
        assert_eq!(scrub("a\tb"), "ab");
    }

    #[test]
    fn human_keys_sort_numerically() {
        let mut keys = ["py3.10", "py3.9", "py3.8", "x1y", "x001y"]
            .iter()
            .map(|s| (*s, human_key(s)))
            .collect::<Vec<_>>();
        keys.sort_by(|a, b| a.1.cmp(&b.1));
        assert_eq!(
            keys.iter().map(|(s, _)| *s).collect::<Vec<_>>(),
            ["py3.8", "py3.9", "py3.10", "x001y", "x1y"]
        );
    }

    #[test]
    fn numbers_sort_before_strings() {
        // Python raises TypeError here; a natural sort needs an answer.
        assert!(human_key("a1b") < human_key("abb"));
    }

    #[test]
    fn nice_time_formats() {
        let now = Local::now();
        let same = nice_time(now.with_timezone(&Utc), now);
        assert_eq!(same.len(), 7, "{same}");
        assert!(same.ends_with("am") || same.ends_with("pm"), "{same}");

        let yesterday = nice_time((now - chrono::Duration::days(1)).with_timezone(&Utc), now);
        assert_eq!(yesterday.len(), 13, "{yesterday}");

        let last_year = nice_time((now - chrono::Duration::days(400)).with_timezone(&Utc), now);
        assert_eq!(last_year.len(), 18, "{last_year}");
    }

    #[test]
    fn interval_accounts_for_work() {
        let mut i = Interval::new(1);
        // A bit of work.
        let mut n = 0u64;
        while n < 1_000_000 {
            n += 1;
        }
        assert!(i.remaining() < Duration::from_secs(1));
        i.reset();
        assert!(i.remaining() > Duration::from_millis(500));
    }
}
