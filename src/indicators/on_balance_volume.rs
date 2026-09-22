use std::fmt;
use std::time::Duration;

use chrono::{DateTime, Utc};
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use super::fixed_time_bucket::{BucketUpdate, FixedTimeBucket};
use crate::errors::{Result, TaError};
use crate::{Close, Next, NextBatch, Reset, Volume};

/// On-Balance Volume over timestamped bars.
///
/// Adds the bucket's volume when its close exceeds the previous *sealed*
/// bucket's close, subtracts when below, carries on ties and on the very
/// first bucket. Cumulative, so there is no window: the value is the running
/// total since construction (or `reset`).
///
/// Revisions inside `bucket_width` swap the current bucket's contribution:
/// the included contribution is subtracted before the revised one is added,
/// so a live bar never double-counts, and the comparison close never
/// advances onto the bar being revised.
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Debug, Clone)]
pub struct OnBalanceVolume {
    buckets: FixedTimeBucket,
    previous_close: Option<f64>,
    current_close: Option<f64>,
    current_contribution: f64,
    value: f64,
}

impl OnBalanceVolume {
    pub fn new(bucket_width: Duration) -> Result<Self> {
        if bucket_width.is_zero() {
            return Err(TaError::InvalidParameter);
        }
        Ok(Self {
            buckets: FixedTimeBucket::new(bucket_width)?,
            previous_close: None,
            current_close: None,
            current_contribution: 0.0,
            value: 0.0,
        })
    }

    fn contribution(&self, close: f64, volume: f64) -> f64 {
        match self.previous_close {
            Some(prev) if close > prev => volume,
            Some(prev) if close < prev => -volume,
            _ => 0.0,
        }
    }
}

impl<T> Next<T> for OnBalanceVolume
where
    T: Close + Volume,
{
    type Output = f64;

    fn next(&mut self, (timestamp, input): (DateTime<Utc>, T)) -> Self::Output {
        let update = self.buckets.update(timestamp);
        let (close, volume) = (input.close(), input.volume());
        if update == BucketUpdate::Append {
            // The live bucket just sealed: its close becomes the comparison
            // close for the new bucket.
            self.previous_close = self.current_close;
            self.current_contribution = 0.0;
        } else {
            self.value -= self.current_contribution;
        }
        self.current_close = Some(close);
        self.current_contribution = self.contribution(close, volume);
        self.value += self.current_contribution;
        self.value
    }
}

impl<T> NextBatch<T> for OnBalanceVolume where T: Copy + Close + Volume {}

impl Reset for OnBalanceVolume {
    fn reset(&mut self) {
        self.buckets.reset();
        self.previous_close = None;
        self.current_close = None;
        self.current_contribution = 0.0;
        self.value = 0.0;
    }
}

impl Default for OnBalanceVolume {
    fn default() -> Self {
        Self::new(Duration::from_secs(86_400)).unwrap()
    }
}

impl fmt::Display for OnBalanceVolume {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "OBV({}s buckets)", self.buckets.width().as_secs())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DataItem;
    use chrono::TimeZone;

    fn bar(close: f64, volume: f64) -> DataItem {
        DataItem::builder()
            .open(close)
            .high(close + 1.0)
            .low(close - 1.0)
            .close(close)
            .volume(volume)
            .build()
            .unwrap()
    }

    #[test]
    fn accumulates_signed_volume() {
        let t = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        let mut obv = OnBalanceVolume::new(Duration::from_secs(86_400)).unwrap();
        assert_eq!(obv.next((t, bar(10.0, 100.0))), 0.0);
        assert_eq!(
            obv.next((t + chrono::Duration::days(1), bar(11.0, 50.0))),
            50.0
        );
        assert_eq!(
            obv.next((t + chrono::Duration::days(2), bar(9.0, 30.0))),
            20.0
        );
        assert_eq!(
            obv.next((t + chrono::Duration::days(3), bar(9.0, 999.0))),
            20.0
        );
    }

    #[test]
    fn revision_swaps_contribution() {
        let t = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        let mut obv = OnBalanceVolume::new(Duration::from_secs(86_400)).unwrap();
        obv.next((t, bar(10.0, 100.0)));
        assert_eq!(
            obv.next((t + chrono::Duration::days(1), bar(11.0, 50.0))),
            50.0
        );
        // Revise the live bucket down through the prior close: +50 → −50.
        assert_eq!(
            obv.next((t + chrono::Duration::days(1), bar(9.0, 50.0))),
            -50.0
        );
    }
}
