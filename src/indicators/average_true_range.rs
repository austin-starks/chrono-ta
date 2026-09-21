use std::collections::VecDeque;
use std::fmt;
use std::time::Duration;

use chrono::{DateTime, Utc};
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use super::fixed_time_bucket::BucketUpdate;
use super::TrueRange;
use crate::{errors::Result, Close, High, Low, Next, NextBatch, Reset};

/// Arithmetic mean of true ranges in an elapsed-time window.
///
/// This is a duration-windowed ATR, not Wilder's observation-count recurrence.
/// `bucket_width` identifies revisions of the current OHLC bar.
#[doc(alias = "ATR")]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Debug, Clone)]
pub struct AverageTrueRange {
    duration: Duration,
    true_range: TrueRange,
    window: VecDeque<(DateTime<Utc>, f64)>,
    sum: f64,
}

impl AverageTrueRange {
    pub fn new(duration: Duration, bucket_width: Duration) -> Result<Self> {
        if duration.is_zero() || bucket_width > duration {
            return Err(crate::errors::TaError::InvalidParameter);
        }
        Ok(Self {
            duration,
            true_range: TrueRange::new(bucket_width)?,
            window: VecDeque::new(),
            sum: 0.0,
        })
    }

    pub fn duration(&self) -> Duration {
        self.duration
    }

    pub fn bucket_width(&self) -> Duration {
        self.true_range.bucket_width()
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

impl<T> Next<T> for AverageTrueRange
where
    T: High + Low + Close,
{
    type Output = f64;

    fn next(&mut self, (timestamp, input): (DateTime<Utc>, T)) -> Self::Output {
        let (true_range, update) = self.true_range.next_with_update(timestamp, &input);
        self.remove_expired(timestamp);

        if update == BucketUpdate::Replace {
            if let Some((_, old_value)) = self.window.pop_back() {
                self.sum -= old_value;
            }
        }

        self.window.push_back((timestamp, true_range));
        self.sum += true_range;
        self.sum / self.window.len() as f64
    }
}

impl<T> NextBatch<T> for AverageTrueRange where T: Copy + High + Low + Close {}

impl Reset for AverageTrueRange {
    fn reset(&mut self) {
        self.true_range.reset();
        self.window.clear();
        self.sum = 0.0;
    }
}

impl Default for AverageTrueRange {
    fn default() -> Self {
        Self::new(
            Duration::from_secs(14 * 86_400),
            Duration::from_secs(86_400),
        )
        .unwrap()
    }
}

impl fmt::Display for AverageTrueRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "AverageTrueRange({}s window, {}s buckets)",
            self.duration.as_secs(),
            self.bucket_width().as_secs()
        )
    }
}
