use std::collections::VecDeque;
use std::fmt;
use std::time::Duration;

use chrono::{DateTime, Utc};
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use crate::indicators::AdaptiveTimeDetector;
use crate::{errors::Result, Next, NextBatch, Reset};

/// Returns the newest observation at or before `timestamp - duration`.
#[doc(alias = "ValueAgo")]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Debug, Clone)]
pub struct Lag {
    duration: Duration,
    history: VecDeque<(DateTime<Utc>, f64)>,
    detector: AdaptiveTimeDetector,
}

/// Discoverability alias for [`Lag`].
pub type ValueAgo = Lag;

impl Lag {
    pub fn new(duration: Duration) -> Result<Self> {
        if duration.is_zero() {
            return Err(crate::errors::TaError::InvalidParameter);
        }
        Ok(Self {
            duration,
            history: VecDeque::new(),
            detector: AdaptiveTimeDetector::new(duration),
        })
    }

    pub fn duration(&self) -> Duration {
        self.duration
    }

    fn target_nanos(&self, timestamp: DateTime<Utc>) -> i64 {
        let duration_nanos = i64::try_from(self.duration.as_nanos()).unwrap_or(i64::MAX);
        timestamp
            .timestamp_nanos_opt()
            .unwrap_or(i64::MIN)
            .saturating_sub(duration_nanos)
    }
}

impl Next<f64> for Lag {
    type Output = Option<f64>;

    fn next(&mut self, (timestamp, value): (DateTime<Utc>, f64)) -> Self::Output {
        let replace = self.detector.should_replace(timestamp);
        if replace {
            self.history.pop_back();
        }
        self.history.push_back((timestamp, value));

        let target = self.target_nanos(timestamp);
        while self.history.len() > 1
            && self.history.get(1).is_some_and(|(candidate, _)| {
                candidate.timestamp_nanos_opt().unwrap_or(i64::MAX) <= target
            })
        {
            self.history.pop_front();
        }

        self.history.front().and_then(|(candidate, value)| {
            (candidate.timestamp_nanos_opt().unwrap_or(i64::MAX) <= target).then_some(*value)
        })
    }
}

impl NextBatch<f64> for Lag {}

impl Reset for Lag {
    fn reset(&mut self) {
        self.history.clear();
        self.detector.reset();
    }
}

impl Default for Lag {
    fn default() -> Self {
        Self::new(Duration::from_secs(86_400)).unwrap()
    }
}

impl fmt::Display for Lag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Lag({}s)", self.duration.as_secs())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration as ChronoDuration, TimeZone};

    #[test]
    fn returns_latest_value_at_or_before_target() {
        let t = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        let mut lag = Lag::new(Duration::from_secs(10)).unwrap();
        assert_eq!(lag.next((t, 1.0)), None);
        assert_eq!(lag.next((t + ChronoDuration::seconds(7), 2.0)), None);
        assert_eq!(lag.next((t + ChronoDuration::seconds(10), 3.0)), Some(1.0));
        assert_eq!(lag.next((t + ChronoDuration::seconds(18), 4.0)), Some(2.0));
    }
}
