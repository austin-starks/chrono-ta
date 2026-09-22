use std::collections::VecDeque;
use std::fmt;
use std::time::Duration;

use chrono::{DateTime, Utc};
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use super::fixed_time_bucket::{BucketUpdate, FixedTimeBucket};
use crate::errors::{Result, TaError};
use crate::{Close, High, Low, Next, NextBatch, Reset};

/// Williams %R over timestamped OHLC bars.
///
/// %R = −100 × (highest high − close) / (highest high − lowest low) over the
/// lookback window, so the range is [−100, 0]. A flat window (highest ==
/// lowest) yields −50.0 (neutral) rather than a divide-by-zero.
///
/// Revisions inside `bucket_width` revise the current bar.
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Debug, Clone)]
pub struct WilliamsR {
    window: Duration,
    buckets: FixedTimeBucket,
    bars: VecDeque<(DateTime<Utc>, f64, f64, f64)>,
}

impl WilliamsR {
    pub fn new(window: Duration, bucket_width: Duration) -> Result<Self> {
        if window.is_zero() || bucket_width.is_zero() || bucket_width > window {
            return Err(TaError::InvalidParameter);
        }
        Ok(Self {
            window,
            buckets: FixedTimeBucket::new(bucket_width)?,
            bars: VecDeque::new(),
        })
    }

    fn remove_expired(&mut self, now: DateTime<Utc>) {
        let nanos = i64::try_from(self.window.as_nanos()).unwrap_or(i64::MAX);
        let cutoff = now
            .timestamp_nanos_opt()
            .unwrap_or(i64::MIN)
            .saturating_sub(nanos);
        while self
            .bars
            .front()
            .is_some_and(|(ts, _, _, _)| ts.timestamp_nanos_opt().unwrap_or(i64::MIN) <= cutoff)
        {
            self.bars.pop_front();
        }
    }
}

impl<T> Next<T> for WilliamsR
where
    T: High + Low + Close,
{
    type Output = f64;

    fn next(&mut self, (timestamp, input): (DateTime<Utc>, T)) -> Self::Output {
        let update = self.buckets.update(timestamp);
        self.remove_expired(timestamp);
        if update == BucketUpdate::Replace {
            self.bars.pop_back();
        }
        let (high, low, close) = (input.high(), input.low(), input.close());
        self.bars.push_back((timestamp, high, low, close));

        let highest = self
            .bars
            .iter()
            .map(|(_, h, _, _)| *h)
            .fold(f64::NEG_INFINITY, f64::max);
        let lowest = self
            .bars
            .iter()
            .map(|(_, _, l, _)| *l)
            .fold(f64::INFINITY, f64::min);
        if highest > lowest {
            -100.0 * (highest - close) / (highest - lowest)
        } else {
            -50.0
        }
    }
}

impl<T> NextBatch<T> for WilliamsR where T: Copy + High + Low + Close {}

impl Reset for WilliamsR {
    fn reset(&mut self) {
        self.buckets.reset();
        self.bars.clear();
    }
}

impl Default for WilliamsR {
    fn default() -> Self {
        Self::new(
            Duration::from_secs(14 * 86_400),
            Duration::from_secs(86_400),
        )
        .unwrap()
    }
}

impl fmt::Display for WilliamsR {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "WilliamsR({}s window, {}s buckets)",
            self.window.as_secs(),
            self.buckets.width().as_secs(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DataItem;
    use chrono::TimeZone;

    fn bar(high: f64, low: f64, close: f64) -> DataItem {
        DataItem::builder()
            .open(close)
            .high(high)
            .low(low)
            .close(close)
            .volume(1.0)
            .build()
            .unwrap()
    }

    #[test]
    fn close_at_high_reads_zero() {
        let t = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        let mut w = WilliamsR::new(
            Duration::from_secs(3 * 86_400),
            Duration::from_secs(86_400),
        )
        .unwrap();
        let mut out = 0.0;
        for (i, c) in [8.0, 9.0, 10.0].iter().enumerate() {
            out = w.next((
                t + chrono::Duration::days(i as i64),
                bar(c + 1.0, c - 1.0, *c),
            ));
        }
        // Highest = 11, close = 10 → −100 × 1/4 = −25.
        assert!((out + 25.0).abs() < 1e-9, "williams = {out}");
    }
}
