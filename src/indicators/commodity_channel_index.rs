use std::collections::VecDeque;
use std::fmt;
use std::time::Duration;

use chrono::{DateTime, Utc};
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use super::fixed_time_bucket::{BucketUpdate, FixedTimeBucket};
use crate::errors::{Result, TaError};
use crate::{Close, High, Low, Next, NextBatch, Reset};

/// Commodity Channel Index over timestamped OHLC bars.
///
/// CCI = (typical price − SMA(typical)) / (constant × mean deviation), with
/// typical price = (high + low + close) / 3 over the lookback window and the
/// constant defaulting to 0.015. Averages run over whatever bars exist so the
/// indicator emits from the first bar. Zero deviation yields 0.0.
///
/// Revisions inside `bucket_width` revise the current bar.
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Debug, Clone)]
pub struct CommodityChannelIndex {
    window: Duration,
    buckets: FixedTimeBucket,
    bars: VecDeque<(DateTime<Utc>, f64, f64, f64)>,
    constant: f64,
}

impl CommodityChannelIndex {
    pub fn new(window: Duration, bucket_width: Duration, constant: f64) -> Result<Self> {
        if window.is_zero()
            || bucket_width.is_zero()
            || bucket_width > window
            || !constant.is_finite()
            || constant <= 0.0
        {
            return Err(TaError::InvalidParameter);
        }
        Ok(Self {
            window,
            buckets: FixedTimeBucket::new(bucket_width)?,
            bars: VecDeque::new(),
            constant,
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

impl<T> Next<T> for CommodityChannelIndex
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
        self.bars
            .push_back((timestamp, input.high(), input.low(), input.close()));

        let typical: Vec<f64> = self
            .bars
            .iter()
            .map(|(_, h, l, c)| (h + l + c) / 3.0)
            .collect();
        let mean = typical.iter().sum::<f64>() / typical.len() as f64;
        let deviation = typical.iter().map(|tp| (tp - mean).abs()).sum::<f64>()
            / typical.len() as f64;
        let current = *typical.last().unwrap_or(&mean);
        if deviation == 0.0 {
            0.0
        } else {
            (current - mean) / (self.constant * deviation)
        }
    }
}

impl<T> NextBatch<T> for CommodityChannelIndex where T: Copy + High + Low + Close {}

impl Reset for CommodityChannelIndex {
    fn reset(&mut self) {
        self.buckets.reset();
        self.bars.clear();
    }
}

impl Default for CommodityChannelIndex {
    fn default() -> Self {
        Self::new(
            Duration::from_secs(20 * 86_400),
            Duration::from_secs(86_400),
            0.015,
        )
        .unwrap()
    }
}

impl fmt::Display for CommodityChannelIndex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "CCI({}s window, {}s buckets)",
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
    fn strong_rally_reads_overbought() {
        let t = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        let mut cci = CommodityChannelIndex::new(
            Duration::from_secs(5 * 86_400),
            Duration::from_secs(86_400),
            0.015,
        )
        .unwrap();
        let mut out = 0.0;
        for (i, c) in [10.0, 10.0, 10.0, 10.0, 20.0].iter().enumerate() {
            out = cci.next((
                t + chrono::Duration::days(i as i64),
                bar(c + 0.5, c - 0.5, *c),
            ));
        }
        assert!(out > 100.0, "cci = {out}");
    }

    #[test]
    fn flat_market_reads_zero() {
        let t = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        let mut cci = CommodityChannelIndex::default();
        let out = cci.next((t, bar(10.0, 10.0, 10.0)));
        assert_eq!(out, 0.0);
    }
}
