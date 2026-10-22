use chrono::{DateTime, Utc};
use std::time::Duration;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// Represents the frequency mode for de-duplication
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Debug, Clone, PartialEq)]
pub enum DetectedFrequency {
    /// Still learning from initial data points (kept for backward compatibility only)
    Unknown,
    /// Daily mode: one observation per trading session
    DailyOHLC,
    /// Intraday mode: minute-level bucketing
    Intraday(Duration),
}

/// Handles time-based de-duplication logic for indicators
///
/// Uses a simple duration-based rule:
/// - Indicators with duration < 1 day: Use minute-level bucketing (replace within same minute)
/// - Indicators with duration >= 1 day: One observation per trading session
///
/// A daily indicator must give the same value at the same moment however often
/// it is fed: a live engine ticking every minute, a backtest feeding the open and
/// the close, and a process restarted mid-session all agree. Every update inside
/// a session revises that session's observation; the first update of the next
/// session appends. Sessions roll over at 06:00 UTC (01:00-02:00 US Eastern), so
/// pre-market, regular and after-hours updates of one US session share a bucket
/// in both daylight-saving states.
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Debug, Clone)]
pub struct AdaptiveTimeDetector {
    frequency: DetectedFrequency,
    /// Pre-computed bucket size in seconds for the `Intraday` mode. `0` for
    /// `DailyOHLC` / `Unknown`. Hoisted out of the per-call enum match so
    /// `should_replace` is a single integer divide on the hot path.
    #[cfg_attr(feature = "serde", serde(default))]
    intraday_bucket_seconds: i64,
    /// Current bucket: the minute (or second) index in `Intraday` mode, the
    /// session index in `DailyOHLC` mode.
    last_minute_bucket: i64,
    last_timestamp: Option<DateTime<Utc>>,
}

/// Seconds after UTC midnight at which one trading session ends and the next begins.
const SESSION_ROLLOVER_UTC_SECONDS: i64 = 6 * 3600;

#[inline]
fn session_day(timestamp: DateTime<Utc>) -> i64 {
    (timestamp.timestamp() - SESSION_ROLLOVER_UTC_SECONDS).div_euclid(86_400)
}

impl AdaptiveTimeDetector {
    /// Create a new detector for a specific indicator duration
    ///
    /// # Arguments
    /// * `duration` - The indicator's time window duration, used to determine bucketing strategy:
    ///   - Duration < 5 minutes: Uses second-level bucketing
    ///   - Duration < 1 day: Uses minute-level bucketing
    ///   - Duration >= 1 day: Uses daily bucketing with 3.4 hour gap enforcement
    pub fn new(duration: Duration) -> Self {
        let frequency = if duration < Duration::from_secs(5 * 60) {
            // Use second bucketing for very short-term indicators (< 5 minutes)
            DetectedFrequency::Intraday(Duration::from_secs(1))
        } else if duration < Duration::from_secs(86400) {
            // Use minute bucketing for short-term indicators (< 1 day)
            DetectedFrequency::Intraday(Duration::from_secs(60))
        } else {
            // Use daily bucketing with gap enforcement for long-term indicators
            DetectedFrequency::DailyOHLC
        };

        let intraday_bucket_seconds = match &frequency {
            DetectedFrequency::Intraday(d) => d.as_secs() as i64,
            _ => 0,
        };

        Self {
            frequency,
            intraday_bucket_seconds,
            last_minute_bucket: i64::MIN,
            last_timestamp: None,
        }
    }

    /// Create a new detector with custom detection samples (DEPRECATED - use new())
    #[deprecated(since = "1.0.0", note = "Use new(duration) instead")]
    pub fn with_samples(_detection_samples: usize, duration: Duration) -> Self {
        Self::new(duration)
    }

    /// Get the current frequency mode
    pub fn frequency(&self) -> &DetectedFrequency {
        &self.frequency
    }

    /// Process a new timestamp and determine if it should replace the previous value
    /// Returns true if this is a duplicate within the same time bucket (should replace)
    /// Returns false if this is a new time period (should append)
    #[inline]
    pub fn should_replace(&mut self, timestamp: DateTime<Utc>) -> bool {
        // Hot path: Intraday bucketing. Hoisted out of the enum match so the
        // common case is a single field load + integer divide. `#[inline]`
        // lets cross-crate callers (e.g. RSI/EMA in indicator hot loops)
        // inline this entire body, eliminating the call overhead.
        if self.intraday_bucket_seconds > 0 {
            let current_bucket = timestamp.timestamp() / self.intraday_bucket_seconds;
            let should_replace = current_bucket == self.last_minute_bucket;
            self.last_minute_bucket = current_bucket;
            return should_replace;
        }

        match &self.frequency {
            DetectedFrequency::Intraday(d) => {
                // Reachable for structs deserialized with the legacy schema
                // (no `intraday_bucket_seconds` field; serde fills it with 0
                // via `#[serde(default)]`). Recompute from the enum and
                // self-heal so subsequent calls take the fast path above.
                let bucket_seconds = (d.as_secs() as i64).max(1);
                self.intraday_bucket_seconds = bucket_seconds;
                let current_bucket = timestamp.timestamp() / bucket_seconds;
                let should_replace = current_bucket == self.last_minute_bucket;
                self.last_minute_bucket = current_bucket;
                should_replace
            }
            DetectedFrequency::DailyOHLC => {
                // State serialized before session bucketing carries only the
                // timestamp of its current observation.
                if self.last_minute_bucket == i64::MIN {
                    if let Some(last_ts) = self.last_timestamp {
                        self.last_minute_bucket = session_day(last_ts);
                    }
                }
                let session = session_day(timestamp);
                let should_replace = session == self.last_minute_bucket;
                self.last_minute_bucket = session;
                self.last_timestamp = Some(timestamp);
                should_replace
            }
            DetectedFrequency::Unknown => {
                // Shouldn't happen but default to not replacing
                self.last_timestamp = Some(timestamp);
                false
            }
        }
    }

    /// Reset the detector to initial state
    pub fn reset(&mut self) {
        self.last_minute_bucket = i64::MIN;
        self.last_timestamp = None;
        // Keep frequency as it was set based on duration
    }

    /// Check if frequency has been detected
    /// Always returns true since we determine mode immediately from duration
    pub fn is_detected(&self) -> bool {
        self.frequency != DetectedFrequency::Unknown
    }
}

// Default implementation removed - force explicit duration
// impl Default for AdaptiveTimeDetector {
//     fn default() -> Self {
//         Self::new()
//     }
// }

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn test_daily_indicator_one_observation_per_session() {
        let mut detector = AdaptiveTimeDetector::new(Duration::from_secs(2 * 86400)); // 2 days
        assert_eq!(detector.frequency(), &DetectedFrequency::DailyOHLC);

        // 2024-01-02 regular session: 14:30-21:00 UTC (09:30-16:00 EST).
        let open = Utc.with_ymd_and_hms(2024, 1, 2, 14, 30, 0).unwrap();
        assert!(!detector.should_replace(open));
        // Every later update of the session revises its observation: midday,
        // the close, and after-hours prints up to 01:00 UTC the next day.
        for offset in [
            chrono::Duration::minutes(1),
            chrono::Duration::hours(3) + chrono::Duration::minutes(25),
            chrono::Duration::hours(6) + chrono::Duration::minutes(30),
            chrono::Duration::hours(10) + chrono::Duration::minutes(30),
        ] {
            assert!(detector.should_replace(open + offset));
        }
        // Next session's pre-market and open append exactly once.
        let next_premarket = Utc.with_ymd_and_hms(2024, 1, 3, 9, 0, 0).unwrap();
        assert!(!detector.should_replace(next_premarket));
        assert!(detector.should_replace(Utc.with_ymd_and_hms(2024, 1, 3, 14, 30, 0).unwrap()));
    }

    #[test]
    fn test_half_day_trading() {
        let mut detector = AdaptiveTimeDetector::new(Duration::from_secs(3 * 86400)); // 3 days
                                                                                      // 2024-11-29 half day: 14:30-18:00 UTC. Open and close are one observation.
        let open = Utc.with_ymd_and_hms(2024, 11, 29, 14, 30, 0).unwrap();
        assert!(!detector.should_replace(open));
        assert!(detector
            .should_replace(open + chrono::Duration::hours(3) + chrono::Duration::minutes(30)));
        // The following Monday is a new session.
        assert!(!detector.should_replace(Utc.with_ymd_and_hms(2024, 12, 2, 14, 30, 0).unwrap()));
    }

    #[test]
    fn test_sessions_roll_over_in_both_daylight_saving_states() {
        // 20:00 ET after-hours is 00:00 UTC (EDT) or 01:00 UTC (EST); both stay
        // with their session, and 04:00 ET pre-market starts the next one.
        for (close_utc, next_premarket_utc) in [
            (
                Utc.with_ymd_and_hms(2024, 7, 2, 0, 0, 0).unwrap(),
                Utc.with_ymd_and_hms(2024, 7, 2, 8, 0, 0).unwrap(),
            ),
            (
                Utc.with_ymd_and_hms(2024, 1, 3, 1, 0, 0).unwrap(),
                Utc.with_ymd_and_hms(2024, 1, 3, 9, 0, 0).unwrap(),
            ),
        ] {
            let mut detector = AdaptiveTimeDetector::new(Duration::from_secs(14 * 86400));
            let open = close_utc - chrono::Duration::hours(10) - chrono::Duration::minutes(30);
            assert!(!detector.should_replace(open));
            assert!(detector.should_replace(close_utc));
            assert!(!detector.should_replace(next_premarket_utc));
        }
    }

    #[test]
    fn test_intraday_indicator() {
        let mut detector = AdaptiveTimeDetector::new(Duration::from_secs(15 * 60)); // 15 minutes
        assert!(matches!(
            detector.frequency(),
            DetectedFrequency::Intraday(d) if d.as_secs() == 60
        ));

        let base = Utc.ymd(2024, 1, 1).and_hms(9, 30, 0);

        // First data point
        assert!(!detector.should_replace(base));

        // Same minute - should replace
        assert!(detector.should_replace(base + chrono::Duration::seconds(30)));

        // Next minute - new slot
        assert!(!detector.should_replace(base + chrono::Duration::minutes(1)));

        // Within same minute - should replace
        assert!(detector
            .should_replace(base + chrono::Duration::minutes(1) + chrono::Duration::seconds(15)));

        // Next minute - new slot
        assert!(!detector.should_replace(base + chrono::Duration::minutes(2)));
    }

    #[test]
    fn test_full_trading_day_with_minutely_updates() {
        let mut detector = AdaptiveTimeDetector::new(Duration::from_secs(5 * 86400)); // 5 days
        let market_open = Utc.with_ymd_and_hms(2024, 1, 2, 14, 30, 0).unwrap();
        assert!(!detector.should_replace(market_open));
        for minutes in 1..=390 {
            assert!(
                detector.should_replace(market_open + chrono::Duration::minutes(minutes)),
                "minute {} after the open must revise the session",
                minutes
            );
        }
    }

    #[test]
    fn test_reset() {
        let mut detector = AdaptiveTimeDetector::new(Duration::from_secs(86400));
        let base = Utc.ymd(2024, 1, 1).and_hms(9, 30, 0);

        // Use detector
        detector.should_replace(base);
        detector.should_replace(base + chrono::Duration::hours(1));

        // Reset
        detector.reset();

        // Frequency should remain but state should be cleared
        assert_eq!(detector.frequency(), &DetectedFrequency::DailyOHLC);
        assert!(detector.last_timestamp.is_none());

        // Should work normally after reset
        assert!(!detector.should_replace(base + chrono::Duration::days(1)));
    }

    /// A struct serialized before `intraday_bucket_seconds` existed
    /// has no value for that field. With `#[serde(default)]` it loads
    /// as `0`, but `frequency` still says `Intraday(60s)`. The first
    /// `should_replace` call must self-heal — recompute the bucket
    /// from `frequency` AND store it — so subsequent calls take the
    /// fast path. Without self-heal, every call would re-enter the
    /// slow fallback, defeating the whole optimization for any
    /// persisted detector.
    #[cfg(feature = "serde")]
    #[test]
    fn test_serde_default_bucket_self_heals() {
        // Simulate a deserialized struct missing the cached field
        let mut detector = AdaptiveTimeDetector {
            frequency: DetectedFrequency::Intraday(Duration::from_secs(60)),
            intraday_bucket_seconds: 0, // <-- the bug: serde default
            last_minute_bucket: i64::MIN,
            last_timestamp: None,
        };

        let base = Utc.ymd(2024, 1, 1).and_hms(9, 30, 0);

        // First call goes through the fallback path
        assert!(!detector.should_replace(base));

        // The fallback must have written the bucket size back
        assert_eq!(
            detector.intraday_bucket_seconds, 60,
            "first should_replace should self-heal intraday_bucket_seconds"
        );

        // Same minute → replace (proves the bucket comparison is correct)
        assert!(detector.should_replace(base + chrono::Duration::seconds(30)));

        // Next minute → new slot
        assert!(!detector.should_replace(base + chrono::Duration::minutes(1)));
    }

    #[test]
    fn test_memory_footprint_for_trading_sessions() {
        // Five sessions of minutely updates (a half day among them) keep five points.
        let mut detector = AdaptiveTimeDetector::new(Duration::from_secs(10 * 86400));
        let mut kept = 0;
        for (day, minutes) in [(25, 390), (26, 390), (27, 390), (29, 210), (2, 390)] {
            let month = if day == 2 { 12 } else { 11 };
            let open = Utc.with_ymd_and_hms(2024, month, day, 14, 30, 0).unwrap();
            for minute in 0..=minutes {
                if !detector.should_replace(open + chrono::Duration::minutes(minute)) {
                    kept += 1;
                }
            }
        }
        assert_eq!(kept, 5, "one point per session");
    }

    /// A daily detector serialized before session bucketing has only
    /// `last_timestamp`. Its next update in the same session must still revise.
    #[test]
    fn test_daily_state_from_before_session_bucketing() {
        let mut detector = AdaptiveTimeDetector::new(Duration::from_secs(14 * 86400));
        detector.last_timestamp = Some(Utc.with_ymd_and_hms(2024, 1, 2, 14, 30, 0).unwrap());
        assert!(detector.should_replace(Utc.with_ymd_and_hms(2024, 1, 2, 20, 0, 0).unwrap()));
        assert!(!detector.should_replace(Utc.with_ymd_and_hms(2024, 1, 3, 14, 30, 0).unwrap()));
    }
}
