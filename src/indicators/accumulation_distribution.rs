use std::fmt;
use std::time::Duration;

use chrono::{DateTime, Utc};
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use super::fixed_time_bucket::{BucketUpdate, FixedTimeBucket};
use crate::errors::{Result, TaError};
use crate::{Close, High, Low, Next, NextBatch, Reset, Volume};

/// Accumulation/Distribution Line over timestamped OHLCV bars.
///
/// Per bucket, money flow volume = close-location value × volume, where
/// CLV = ((close − low) − (high − close)) / (high − low) (0.0 on a flat
/// bucket). The line is the running total since construction (or `reset`).
///
/// Revisions inside `bucket_width` swap the current bucket's contribution,
/// exactly like [`super::OnBalanceVolume`].
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Debug, Clone)]
pub struct AccumulationDistribution {
    buckets: FixedTimeBucket,
    current_contribution: f64,
    value: f64,
}

impl AccumulationDistribution {
    pub fn new(bucket_width: Duration) -> Result<Self> {
        if bucket_width.is_zero() {
            return Err(TaError::InvalidParameter);
        }
        Ok(Self {
            buckets: FixedTimeBucket::new(bucket_width)?,
            current_contribution: 0.0,
            value: 0.0,
        })
    }
}

impl<T> Next<T> for AccumulationDistribution
where
    T: High + Low + Close + Volume,
{
    type Output = f64;

    fn next(&mut self, (timestamp, input): (DateTime<Utc>, T)) -> Self::Output {
        let update = self.buckets.update(timestamp);
        if update == BucketUpdate::Replace {
            self.value -= self.current_contribution;
        } else {
            self.current_contribution = 0.0;
        }
        let (high, low, close, volume) =
            (input.high(), input.low(), input.close(), input.volume());
        let clv = if high > low {
            ((close - low) - (high - close)) / (high - low)
        } else {
            0.0
        };
        self.current_contribution = clv * volume;
        self.value += self.current_contribution;
        self.value
    }
}

impl<T> NextBatch<T> for AccumulationDistribution where T: Copy + High + Low + Close + Volume {}

impl Reset for AccumulationDistribution {
    fn reset(&mut self) {
        self.buckets.reset();
        self.current_contribution = 0.0;
        self.value = 0.0;
    }
}

impl Default for AccumulationDistribution {
    fn default() -> Self {
        Self::new(Duration::from_secs(86_400)).unwrap()
    }
}

impl fmt::Display for AccumulationDistribution {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ADL({}s buckets)", self.buckets.width().as_secs())
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
    fn close_at_high_accumulates_full_volume() {
        let t = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        let mut adl = AccumulationDistribution::default();
        assert_eq!(adl.next((t, bar(12.0, 10.0, 12.0, 100.0))), 100.0);
        // CLV = ((11−10) − (12−11)) / 2 = 0 → line unchanged.
        assert_eq!(
            adl.next((t + chrono::Duration::days(1), bar(12.0, 10.0, 11.0, 100.0))),
            100.0
        );
    }
}
