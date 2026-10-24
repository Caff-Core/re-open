//! Wall-clock timestamps and monotonic deadlines.
//!
//! Stored timestamps are UTC (RFC 3339 on the wire and in storage) and are
//! displayed in the user's timezone by the interface. Local deadlines use a
//! monotonic clock so wall-clock changes cannot extend or cut short a timeout.

use std::fmt;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// A UTC point in time with whole-second precision.
///
/// Truncating to seconds keeps the stored RFC 3339 text fixed-width
/// (`YYYY-MM-DDTHH:MM:SSZ`), so ordering stored text matches ordering time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "jiff::Timestamp", into = "jiff::Timestamp")]
pub struct Timestamp(jiff::Timestamp);

impl From<jiff::Timestamp> for Timestamp {
    fn from(timestamp: jiff::Timestamp) -> Self {
        Self::from_jiff(timestamp)
    }
}

impl From<Timestamp> for jiff::Timestamp {
    fn from(timestamp: Timestamp) -> Self {
        timestamp.0
    }
}

impl Timestamp {
    /// Truncates to whole seconds.
    #[must_use]
    pub fn from_jiff(timestamp: jiff::Timestamp) -> Self {
        Self(jiff::Timestamp::from_second(timestamp.as_second()).unwrap_or(timestamp))
    }

    /// Whole seconds since the Unix epoch.
    #[must_use]
    pub fn as_second(&self) -> i64 {
        self.0.as_second()
    }

    /// This instant minus a duration, saturating at the earliest timestamp.
    #[must_use]
    pub fn saturating_sub(self, duration: Duration) -> Self {
        let seconds = i64::try_from(duration.as_secs()).unwrap_or(i64::MAX);
        Self::from_jiff(
            jiff::Timestamp::from_second(self.as_second().saturating_sub(seconds))
                .unwrap_or(jiff::Timestamp::MIN),
        )
    }

    #[must_use]
    pub const fn as_jiff(&self) -> jiff::Timestamp {
        self.0
    }

    /// Parses an RFC 3339 timestamp with an offset; the offset is normalised to UTC.
    ///
    /// # Errors
    ///
    /// Returns an error when the text is not a valid RFC 3339 timestamp.
    pub fn parse(text: &str) -> Result<Self, jiff::Error> {
        text.parse().map(Self::from_jiff)
    }
}

impl fmt::Display for Timestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

/// A reading of a monotonic clock, relative to that clock's own origin.
///
/// Readings from different clocks are not comparable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MonotonicInstant(Duration);

impl MonotonicInstant {
    #[must_use]
    pub const fn from_origin(elapsed: Duration) -> Self {
        Self(elapsed)
    }

    #[must_use]
    pub const fn since_origin(self) -> Duration {
        self.0
    }

    #[must_use]
    pub fn saturating_add(self, duration: Duration) -> Self {
        Self(self.0.saturating_add(duration))
    }

    #[must_use]
    pub fn saturating_duration_since(self, earlier: Self) -> Duration {
        self.0.saturating_sub(earlier.0)
    }
}

/// Source of wall-clock and monotonic time; fake it in tests.
pub trait Clock: Send + Sync {
    fn now_utc(&self) -> Timestamp;
    fn now_monotonic(&self) -> MonotonicInstant;
}

/// The real system clock.
#[derive(Debug, Clone, Copy)]
pub struct SystemClock {
    origin: Instant,
}

impl SystemClock {
    #[must_use]
    pub fn new() -> Self {
        Self {
            origin: Instant::now(),
        }
    }
}

impl Default for SystemClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for SystemClock {
    fn now_utc(&self) -> Timestamp {
        Timestamp::from_jiff(jiff::Timestamp::now())
    }

    fn now_monotonic(&self) -> MonotonicInstant {
        MonotonicInstant(self.origin.elapsed())
    }
}

/// A point on a monotonic clock after which work must stop.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Deadline {
    at: MonotonicInstant,
}

impl Deadline {
    #[must_use]
    pub fn after(now: MonotonicInstant, budget: Duration) -> Self {
        Self {
            at: now.saturating_add(budget),
        }
    }

    #[must_use]
    pub const fn at(self) -> MonotonicInstant {
        self.at
    }

    #[must_use]
    pub fn remaining(self, now: MonotonicInstant) -> Duration {
        self.at.saturating_duration_since(now)
    }

    #[must_use]
    pub fn is_expired(self, now: MonotonicInstant) -> bool {
        now >= self.at
    }

    /// The earlier of two deadlines, for a per-provider budget inside a cycle budget.
    #[must_use]
    pub fn min(self, other: Self) -> Self {
        if other.at < self.at { other } else { self }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(secs: u64) -> MonotonicInstant {
        MonotonicInstant::from_origin(Duration::from_secs(secs))
    }

    #[test]
    fn timestamps_are_utc_rfc3339() {
        let ts = Timestamp::parse("2026-10-23T14:00:00+02:00").unwrap();
        assert_eq!(ts.to_string(), "2026-10-23T12:00:00Z");
        assert_eq!(
            serde_json::to_string(&ts).unwrap(),
            "\"2026-10-23T12:00:00Z\""
        );
    }

    #[test]
    fn timestamps_are_whole_seconds_so_text_order_is_time_order() {
        let fractional = Timestamp::parse("2026-10-23T12:00:00.9Z").unwrap();
        assert_eq!(fractional.to_string(), "2026-10-23T12:00:00Z");
        let later = Timestamp::parse("2026-10-23T12:00:01Z").unwrap();
        assert!(fractional.to_string() < later.to_string());
        let from_json: Timestamp = serde_json::from_str("\"2026-10-23T12:00:00.5Z\"").unwrap();
        assert_eq!(from_json, fractional);
        assert_eq!(SystemClock::new().now_utc().to_string().len(), 20);
    }

    #[test]
    fn saturating_sub_moves_back_in_whole_seconds() {
        let ts = Timestamp::parse("2026-10-23T12:00:00Z").unwrap();
        assert_eq!(
            ts.saturating_sub(Duration::from_secs(86_400)).to_string(),
            "2026-10-22T12:00:00Z"
        );
    }

    #[test]
    fn timestamp_without_offset_is_rejected() {
        assert!(Timestamp::parse("2026-10-23T12:00:00").is_err());
    }

    #[test]
    fn deadline_counts_down_and_expires() {
        let deadline = Deadline::after(at(10), Duration::from_secs(5));
        assert_eq!(deadline.remaining(at(12)), Duration::from_secs(3));
        assert!(!deadline.is_expired(at(14)));
        assert!(deadline.is_expired(at(15)));
        assert_eq!(deadline.remaining(at(20)), Duration::ZERO);
    }

    #[test]
    fn provider_deadline_is_capped_by_cycle_deadline() {
        let cycle = Deadline::after(at(0), Duration::from_secs(10));
        let provider = Deadline::after(at(8), Duration::from_secs(5));
        assert_eq!(provider.min(cycle), cycle);
    }

    #[test]
    fn system_clock_is_monotonic() {
        let clock = SystemClock::new();
        let first = clock.now_monotonic();
        assert!(clock.now_monotonic() >= first);
    }
}
