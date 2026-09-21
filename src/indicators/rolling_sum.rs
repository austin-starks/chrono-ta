use std::collections::VecDeque;
use std::fmt;
use std::time::Duration;

use chrono::{DateTime, Utc};
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use crate::indicators::AdaptiveTimeDetector;
use crate::{errors::Result, Next, NextBatch, Reset};

/// Sum of observations retained in an elapsed-time window.
///
/// Revisions in the current adaptive time bucket replace the prior observation
/// before the sum is calculated. Unlike NexusTrade's historical `TrailingSum`,
/// equal consecutive values are still distinct observations.
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Debug, Clone)]
pub struct RollingSum {
    duration: Duration,
    window: VecDeque<(DateTime<Utc>, f64)>,
    sum: f64,
    detector: AdaptiveTimeDetector,
}

impl RollingSum {
    pub fn new(duration: Duration) -> Result<Self> {
        if duration.is_zero() {
            return Err(crate::errors::TaError::InvalidParameter);
        }
        Ok(Self {
            duration,
            window: VecDeque::new(),
            sum: 0.0,
            detector: AdaptiveTimeDetector::new(duration),
        })
    }

    pub fn duration(&self) -> Duration {
        self.duration
    }

    fn remove_expired(&mut self, current_time: DateTime<Utc>) {
        let duration_nanos = i64::try_from(self.duration.as_nanos()).unwrap_or(i64::MAX);
        let cutoff = current_time
            .timestamp_nanos_opt()
            .unwrap_or(i64::MIN)
            .saturating_sub(duration_nanos);
        while self.window.front().is_some_and(|(timestamp, _)| {
            timestamp.timestamp_nanos_opt().unwrap_or(i64::MIN) <= cutoff
        }) {
            if let Some((_, value)) = self.window.pop_front() {
                self.sum -= value;
            }
        }
    }
}

impl Next<f64> for RollingSum {
    type Output = f64;

    fn next(&mut self, (timestamp, value): (DateTime<Utc>, f64)) -> Self::Output {
        let replace = self.detector.should_replace(timestamp);
        self.remove_expired(timestamp);

        if replace {
            if let Some((_, old_value)) = self.window.pop_back() {
                self.sum -= old_value;
            }
        }

        self.window.push_back((timestamp, value));
        self.sum += value;
        self.sum
    }
}

impl NextBatch<f64> for RollingSum {}

impl Reset for RollingSum {
    fn reset(&mut self) {
        self.window.clear();
        self.sum = 0.0;
        self.detector.reset();
    }
}

impl Default for RollingSum {
    fn default() -> Self {
        Self::new(Duration::from_secs(14 * 86_400)).unwrap()
    }
}

impl fmt::Display for RollingSum {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "RollingSum({}s)", self.duration.as_secs())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration as ChronoDuration, TimeZone};

    fn start() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap()
    }

    #[test]
    fn expires_exact_boundary_and_keeps_equal_values() {
        let mut sum = RollingSum::new(Duration::from_secs(3)).unwrap();
        let t = start();
        assert_eq!(sum.next((t, 2.0)), 2.0);
        assert_eq!(sum.next((t + ChronoDuration::seconds(1), 2.0)), 4.0);
        assert_eq!(sum.next((t + ChronoDuration::seconds(2), 3.0)), 7.0);
        assert_eq!(sum.next((t + ChronoDuration::seconds(3), 4.0)), 9.0);
    }

    #[test]
    fn replaces_current_bucket() {
        let mut sum = RollingSum::new(Duration::from_secs(60)).unwrap();
        let t = start();
        assert_eq!(sum.next((t, 2.0)), 2.0);
        assert_eq!(sum.next((t + ChronoDuration::milliseconds(500), 7.0)), 7.0);
    }
}
