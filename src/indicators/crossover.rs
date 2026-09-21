use std::fmt;
use std::time::Duration;

use chrono::{DateTime, Utc};
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use super::fixed_time_bucket::{BucketUpdate, FixedTimeBucket};
use crate::{errors::Result, Next, NextBatch, Reset};

#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Debug, Clone)]
struct CrossState {
    buckets: FixedTimeBucket,
    previous: Option<(f64, f64)>,
    current: Option<(f64, f64)>,
}

impl CrossState {
    fn new(bucket_width: Duration) -> Result<Self> {
        Ok(Self {
            buckets: FixedTimeBucket::new(bucket_width)?,
            previous: None,
            current: None,
        })
    }

    fn update(&mut self, timestamp: DateTime<Utc>, pair: (f64, f64)) {
        match self.buckets.update(timestamp) {
            BucketUpdate::First | BucketUpdate::Replace => self.current = Some(pair),
            BucketUpdate::Append => {
                self.previous = self.current;
                self.current = Some(pair);
            }
        }
    }

    fn reset(&mut self) {
        self.buckets.reset();
        self.previous = None;
        self.current = None;
    }
}

/// Detects a transition from `lhs <= rhs` to `lhs > rhs`.
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Debug, Clone)]
pub struct CrossAbove(CrossState);

impl CrossAbove {
    pub fn new(bucket_width: Duration) -> Result<Self> {
        CrossState::new(bucket_width).map(Self)
    }

    pub fn bucket_width(&self) -> Duration {
        self.0.buckets.width()
    }
}

impl Next<(f64, f64)> for CrossAbove {
    type Output = bool;

    fn next(&mut self, (timestamp, pair): (DateTime<Utc>, (f64, f64))) -> Self::Output {
        self.0.update(timestamp, pair);
        matches!(
            (self.0.previous, self.0.current),
            (Some((previous_lhs, previous_rhs)), Some((lhs, rhs)))
                if previous_lhs <= previous_rhs && lhs > rhs
        )
    }
}

impl NextBatch<(f64, f64)> for CrossAbove {}

impl Reset for CrossAbove {
    fn reset(&mut self) {
        self.0.reset();
    }
}

impl fmt::Display for CrossAbove {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "CrossAbove({}s)", self.bucket_width().as_secs())
    }
}

/// Detects a transition from `lhs >= rhs` to `lhs < rhs`.
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Debug, Clone)]
pub struct CrossBelow(CrossState);

impl CrossBelow {
    pub fn new(bucket_width: Duration) -> Result<Self> {
        CrossState::new(bucket_width).map(Self)
    }

    pub fn bucket_width(&self) -> Duration {
        self.0.buckets.width()
    }
}

impl Next<(f64, f64)> for CrossBelow {
    type Output = bool;

    fn next(&mut self, (timestamp, pair): (DateTime<Utc>, (f64, f64))) -> Self::Output {
        self.0.update(timestamp, pair);
        matches!(
            (self.0.previous, self.0.current),
            (Some((previous_lhs, previous_rhs)), Some((lhs, rhs)))
                if previous_lhs >= previous_rhs && lhs < rhs
        )
    }
}

impl NextBatch<(f64, f64)> for CrossBelow {}

impl Reset for CrossBelow {
    fn reset(&mut self) {
        self.0.reset();
    }
}

impl fmt::Display for CrossBelow {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "CrossBelow({}s)", self.bucket_width().as_secs())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration as ChronoDuration, TimeZone};

    #[test]
    fn replacement_recomputes_against_previous_bucket() {
        let t = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        let mut cross = CrossAbove::new(Duration::from_secs(60)).unwrap();
        assert!(!cross.next((t, (1.0, 2.0))));
        assert!(cross.next((t + ChronoDuration::minutes(1), (3.0, 2.0))));
        assert!(!cross.next((
            t + ChronoDuration::minutes(1) + ChronoDuration::seconds(30),
            (1.5, 2.0),
        )));
    }
}
