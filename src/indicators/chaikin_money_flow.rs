use std::collections::VecDeque;
use std::fmt;
use std::time::Duration;

use chrono::{DateTime, Utc};
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use super::fixed_time_bucket::{BucketUpdate, FixedTimeBucket};
use crate::errors::{Result, TaError};
use crate::{Close, High, Low, Next, NextBatch, Reset, Volume};

/// Chaikin Money Flow over timestamped OHLCV bars.
///
/// CMF = sum(money flow volume, N) / sum(volume, N), where money flow volume
/// is the close-location value × volume per bucket. Bounded in [−1, 1]; zero
/// window volume yields 0.0. Runs over whatever bars exist so it emits from
/// the first bar.
///
/// Revisions inside `bucket_width` revise the current bar.
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Debug, Clone)]
pub struct ChaikinMoneyFlow {
    window: Duration,
    buckets: FixedTimeBucket,
    bars: VecDeque<(DateTime<Utc>, f64, f64)>,
}

impl ChaikinMoneyFlow {
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

impl<T> Next<T> for ChaikinMoneyFlow
where
    T: High + Low + Close + Volume,
{
    type Output = f64;

    fn next(&mut self, (timestamp, input): (DateTime<Utc>, T)) -> Self::Output {
        let update = self.buckets.update(timestamp);
        self.remove_expired(timestamp);
        if update == BucketUpdate::Replace {
            self.bars.pop_back();
        }
        let (high, low, close, volume) =
            (input.high(), input.low(), input.close(), input.volume());
        let clv = if high > low {
            ((close - low) - (high - close)) / (high - low)
        } else {
            0.0
        };
        self.bars.push_back((timestamp, clv * volume, volume));

        let flow: f64 = self.bars.iter().map(|(_, mfv, _)| mfv).sum();
        let vol: f64 = self.bars.iter().map(|(_, _, v)| v).sum();
        if vol == 0.0 {
            0.0
        } else {
            flow / vol
        }
    }
}

impl<T> NextBatch<T> for ChaikinMoneyFlow where T: Copy + High + Low + Close + Volume {}

impl Reset for ChaikinMoneyFlow {
    fn reset(&mut self) {
        self.buckets.reset();
        self.bars.clear();
    }
}

impl Default for ChaikinMoneyFlow {
    fn default() -> Self {
        Self::new(
            Duration::from_secs(20 * 86_400),
            Duration::from_secs(86_400),
        )
        .unwrap()
    }
}

impl fmt::Display for ChaikinMoneyFlow {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "CMF({}s window, {}s buckets)",
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

    fn bar(high: f64, low: f64, close: f64, volume: f64) -> DataItem {
        DataItem::builder()
            .open(close)
            .high(high)
            .low(low)
            .close(close)
            .volume(volume)
            .build()
            .unwrap()
    }

    #[test]
    fn persistent_buying_pressure_reads_positive() {
        let t = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        let mut cmf = ChaikinMoneyFlow::new(
            Duration::from_secs(3 * 86_400),
            Duration::from_secs(86_400),
        )
        .unwrap();
        let mut out = 0.0;
        for i in 0..3 {
            // Close at the high every bar → CLV = +1 → CMF = +1.
            out = cmf.next((t + chrono::Duration::days(i), bar(12.0, 10.0, 12.0, 100.0)));
        }
        assert!((out - 1.0).abs() < 1e-9, "cmf = {out}");
    }
}
