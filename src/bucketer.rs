//! Group datetimes that are close together, so runs that started at about the
//! Same time are grouped together.
//!
//! GitHub reports runs that started together with timestamps that differ by a
//! few seconds.  Instead of grouping by the exact time, each time is rounded
//! to a window, and nearby times are merged into the first one seen.

use std::collections::HashMap;
use std::collections::HashSet;

use chrono::DateTime;
use chrono::Utc;

/// Round half to even, like Python's `round()`.
fn round_i(x: f64) -> i64 {
    let r = x.round() as i64; // half away from zero, then to an integer
    if x.fract().abs() == 0.5 && r % 2 != 0 {
        r - x.signum() as i64
    } else {
        r
    }
}

pub struct Bucketer {
    /// The size of the window, in seconds.
    window: f64,
    /// The datetimes we've seen.
    instants: HashSet<DateTime<Utc>>,
    /// Maps a rounded time to the instant it came from.
    rounds: HashMap<i64, DateTime<Utc>>,
}

impl Bucketer {
    pub fn new(window_secs: f64) -> Self {
        Self {
            window: window_secs,
            instants: HashSet::new(),
            rounds: HashMap::new(),
        }
    }

    /// The two rounded times for `dt`: with and without a half-window of jitter.
    fn roundings(&self, dt: DateTime<Utc>) -> [i64; 2] {
        let t = dt.timestamp_micros() as f64 / 1_000_000.0;
        [
            round_i(t / self.window),
            round_i((t + self.window / 2.0) / self.window),
        ]
    }

    /// Return the "real" time for a time that's close to one we've already seen.
    pub fn defuzz(&mut self, dt: DateTime<Utc>) -> DateTime<Utc> {
        if self.instants.contains(&dt) {
            return dt;
        }
        for rounded in self.roundings(dt) {
            if let Some(instant) = self.rounds.get(&rounded) {
                return *instant;
            }
        }
        self.instants.insert(dt);
        for rounded in self.roundings(dt) {
            self.rounds.insert(rounded, dt);
        }
        dt
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dt(s: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(s, 0).unwrap()
    }

    #[test]
    fn merges_times_within_the_window() {
        let mut b = Bucketer::new(5.0);
        let a = b.defuzz(dt(100));
        assert_eq!(a, dt(100));
        assert_eq!(b.defuzz(dt(101)), a);
        assert_eq!(b.defuzz(dt(102)), a);
    }

    #[test]
    fn keeps_distant_times_separate() {
        let mut b = Bucketer::new(5.0);
        let a = b.defuzz(dt(100));
        let d = b.defuzz(dt(103));
        assert_ne!(a, d);
        assert_eq!(b.defuzz(dt(104)), d);
    }

    #[test]
    fn merges_subsecond_times() {
        let mut b = Bucketer::new(5.0);
        let a = DateTime::from_timestamp_micros(100_000_123).unwrap();
        let c = DateTime::from_timestamp_micros(100_000_987).unwrap();
        assert_eq!(b.defuzz(a), a);
        assert_eq!(b.defuzz(c), a);
    }

    #[test]
    fn round_half_to_even() {
        assert_eq!(round_i(0.0), 0);
        assert_eq!(round_i(0.5), 0);
        assert_eq!(round_i(1.5), 2);
        assert_eq!(round_i(2.5), 2);
        assert_eq!(round_i(3.5), 4);
        assert_eq!(round_i(-2.5), -2);
        assert_eq!(round_i(2.4), 2);
        assert_eq!(round_i(2.6), 3);
    }
}
