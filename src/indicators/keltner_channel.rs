use std::fmt;
use std::time::Duration;

use chrono::{DateTime, Utc};
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use super::average_true_range::AverageTrueRange;
use super::exponential_moving_average::ExponentialMovingAverage;
use crate::errors::{Result, TaError};
use crate::{Close, High, Low, Next, NextBatch, Reset};

/// Keltner channel output: EMA midline with ATR bands around it.
#[derive(Debug, Clone, PartialEq)]
pub struct KeltnerOutput {
    pub upper: f64,
    pub middle: f64,
    pub lower: f64,
}

/// Keltner channel over timestamped OHLC bars.
///
/// Middle = exponential moving average of closes over `length`; bands sit
/// `multiplier` average-true-ranges above and below. Both legs are
/// revision-safe, so revisions inside `bucket_width` revise the channel
/// rather than stacking duplicate observations.
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Debug, Clone)]
pub struct KeltnerChannel {
    length: Duration,
    multiplier: f64,
    ema: ExponentialMovingAverage,
    atr: AverageTrueRange,
}

impl KeltnerChannel {
    pub fn new(length: Duration, bucket_width: Duration, multiplier: f64) -> Result<Self> {
        if length.is_zero() || bucket_width.is_zero() || bucket_width > length {
            return Err(TaError::InvalidParameter);
        }
        if !multiplier.is_finite() || multiplier <= 0.0 {
            return Err(TaError::InvalidParameter);
        }
        Ok(Self {
            length,
            multiplier,
            ema: ExponentialMovingAverage::new(length)?,
            atr: AverageTrueRange::new(length, bucket_width)?,
        })
    }
}

impl<T> Next<T> for KeltnerChannel
where
    T: High + Low + Close,
{
    type Output = KeltnerOutput;

    fn next(&mut self, (timestamp, input): (DateTime<Utc>, T)) -> Self::Output {
        let middle = self.ema.next((timestamp, input.close()));
        let range = self.multiplier * self.atr.next((timestamp, input));
        KeltnerOutput {
            upper: middle + range,
            middle,
            lower: middle - range,
        }
    }
}

impl<T> NextBatch<T> for KeltnerChannel where T: Copy + High + Low + Close {}

impl Reset for KeltnerChannel {
    fn reset(&mut self) {
        self.ema.reset();
        self.atr.reset();
    }
}

impl Default for KeltnerChannel {
    fn default() -> Self {
        Self::new(
            Duration::from_secs(20 * 86_400),
            Duration::from_secs(86_400),
            2.0,
        )
        .unwrap()
    }
}

impl fmt::Display for KeltnerChannel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Keltner({}s length, x{})",
            self.length.as_secs(),
            self.multiplier,
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
    fn bands_bracket_the_midline_symmetrically() {
        let t = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        let mut kc = KeltnerChannel::new(
            Duration::from_secs(3 * 86_400),
            Duration::from_secs(86_400),
            2.0,
        )
        .unwrap();
        let mut out = None;
        for i in 0..5 {
            out = Some(kc.next((t + chrono::Duration::days(i), bar(11.0, 9.0, 10.0))));
        }
        let out = out.unwrap();
        assert!((out.upper - out.middle - (out.middle - out.lower)).abs() < 1e-9);
        assert!(out.upper >= out.middle && out.middle >= out.lower);
    }
}
