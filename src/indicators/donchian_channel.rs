use std::collections::VecDeque;
use std::fmt;
use std::time::Duration;

use chrono::{DateTime, Utc};
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use super::fixed_time_bucket::{BucketUpdate, FixedTimeBucket};
use crate::errors::{Result, TaError};
use crate::{Close, High, Low, Next, NextBatch, Reset};

/// Donchian channel output: highest high, lowest low, and midline.
#[derive(Debug, Clone, PartialEq)]
pub struct DonchianOutput {
    pub upper: f64,
    pub lower: f64,
    pub middle: f64,
}

/// Donchian channel over timestamped OHLC bars.
///
/// Upper = highest high and lower = lowest low over the lookback window;
/// middle is their midpoint. Runs over whatever bars exist so the channel
/// emits from the first bar.
///
/// Revisions inside `bucket_width` revise the current bar.
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Debug, Clone)]
pub struct DonchianChannel {
    window: Duration,
    buckets: FixedTimeBucket,
    bars: VecDeque<(DateTime<Utc>, f64, f64)>,
}

impl DonchianChannel {
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
            .is_some_and(|(ts, _, _)| ts.timestamp_nanos_opt().unwrap_or(i64::MIN) <= cutoff)
        {
            self.bars.pop_front();
        }
    }
}

impl<T> Next<T> for DonchianChannel
where
    T: High + Low + Close,
{
    type Output = DonchianOutput;

    fn next(&mut self, (timestamp, input): (DateTime<Utc>, T)) -> Self::Output {
        let update = self.buckets.update(timestamp);
        self.remove_expired(timestamp);
        if update == BucketUpdate::Replace {
            self.bars.pop_back();
        }
        self.bars.push_back((timestamp, input.high(), input.low()));

        let upper = self
            .bars
            .iter()
            .map(|(_, h, _)| *h)
            .fold(f64::NEG_INFINITY, f64::max);
        let lower = self
            .bars
            .iter()
            .map(|(_, _, l)| *l)
            .fold(f64::INFINITY, f64::min);
        DonchianOutput {
            upper,
            lower,
            middle: (upper + lower) / 2.0,
        }
    }
}

impl<T> NextBatch<T> for DonchianChannel where T: Copy + High + Low + Close {}

impl Reset for DonchianChannel {
    fn reset(&mut self) {
        self.buckets.reset();
        self.bars.clear();
    }
}

impl Default for DonchianChannel {
    fn default() -> Self {
        Self::new(
            Duration::from_secs(20 * 86_400),
            Duration::from_secs(86_400),
        )
        .unwrap()
    }
}

impl fmt::Display for DonchianChannel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Donchian({}s window, {}s buckets)",
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

    fn bar(high: f64, low: f64) -> DataItem {
        DataItem::builder()
            .open(low)
            .high(high)
            .low(low)
            .close(low)
            .volume(1.0)
            .build()
            .unwrap()
    }

    #[test]
    fn tracks_highest_high_and_lowest_low() {
        let t = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        let mut dc = DonchianChannel::new(
            Duration::from_secs(3 * 86_400),
            Duration::from_secs(86_400),
        )
        .unwrap();
        dc.next((t, bar(10.0, 8.0)));
        dc.next((t + chrono::Duration::days(1), bar(12.0, 9.0)));
        let out = dc.next((t + chrono::Duration::days(2), bar(11.0, 7.0)));
        assert_eq!(out.upper, 12.0);
        assert_eq!(out.lower, 7.0);
        assert_eq!(out.middle, 9.5);
    }
}
