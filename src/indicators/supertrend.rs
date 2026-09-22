use std::fmt;
use std::time::Duration;

use chrono::{DateTime, Utc};
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use super::average_true_range::AverageTrueRange;
use super::fixed_time_bucket::{BucketUpdate, FixedTimeBucket};
use crate::errors::{Result, TaError};
use crate::{Close, High, Low, Next, NextBatch, Reset};

/// Supertrend direction: which band is currently active.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum TrendDirection {
    Up,
    Down,
}

/// Supertrend output: the active band value and its direction.
#[derive(Debug, Clone, PartialEq)]
pub struct SupertrendOutput {
    pub value: f64,
    pub direction: TrendDirection,
}

/// Supertrend over timestamped OHLC bars.
///
/// Basic bands = HL2 ± multiplier × ATR. Final bands ratchet (the lower band
/// never moves down in an uptrend, the upper never moves up in a
/// downtrend); the trend flips when the close crosses the opposite band.
/// Starts in an uptrend from the first bar.
///
/// Path-dependent state (bands, direction) is snapshotted on every new
/// bucket. Revisions inside `bucket_width` restore the snapshot before
/// re-applying, so a live bar re-evaluates from the sealed state instead of
/// compounding onto its own earlier revision.
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Debug, Clone)]
pub struct Supertrend {
    multiplier: f64,
    buckets: FixedTimeBucket,
    atr: AverageTrueRange,
    upper: Option<f64>,
    lower: Option<f64>,
    direction: TrendDirection,
    snapshot: Option<(Option<f64>, Option<f64>, TrendDirection)>,
}

impl Supertrend {
    pub fn new(length: Duration, bucket_width: Duration, multiplier: f64) -> Result<Self> {
        if length.is_zero() || bucket_width.is_zero() || bucket_width > length {
            return Err(TaError::InvalidParameter);
        }
        if !multiplier.is_finite() || multiplier <= 0.0 {
            return Err(TaError::InvalidParameter);
        }
        Ok(Self {
            multiplier,
            buckets: FixedTimeBucket::new(bucket_width)?,
            atr: AverageTrueRange::new(length, bucket_width)?,
            upper: None,
            lower: None,
            direction: TrendDirection::Up,
            snapshot: None,
        })
    }
}

impl<T> Next<T> for Supertrend
where
    T: High + Low + Close,
{
    type Output = SupertrendOutput;

    fn next(&mut self, (timestamp, input): (DateTime<Utc>, T)) -> Self::Output {
        let update = self.buckets.update(timestamp);
        if update == BucketUpdate::Append {
            self.snapshot = Some((self.upper, self.lower, self.direction));
        } else if let Some((upper, lower, direction)) = self.snapshot {
            // Revision: the owned ATR already swapped its own contribution,
            // so only the path-dependent band state needs restoring.
            self.upper = upper;
            self.lower = lower;
            self.direction = direction;
        }
        let (high, low, close) = (input.high(), input.low(), input.close());
        let atr = self.atr.next((timestamp, input));
        let hl2 = (high + low) / 2.0;
        let basic_upper = hl2 + self.multiplier * atr;
        let basic_lower = hl2 - self.multiplier * atr;

        self.upper = Some(match self.upper {
            Some(prev) if self.direction == TrendDirection::Down => basic_upper.min(prev),
            _ => basic_upper,
        });
        self.lower = Some(match self.lower {
            Some(prev) if self.direction == TrendDirection::Up => basic_lower.max(prev),
            _ => basic_lower,
        });

        match self.direction {
            TrendDirection::Up => {
                if close < self.lower.unwrap_or(f64::NEG_INFINITY) {
                    self.direction = TrendDirection::Down;
                }
            }
            TrendDirection::Down => {
                if close > self.upper.unwrap_or(f64::INFINITY) {
                    self.direction = TrendDirection::Up;
                }
            }
        }

        let value = match self.direction {
            TrendDirection::Up => self.lower.unwrap_or(basic_lower),
            TrendDirection::Down => self.upper.unwrap_or(basic_upper),
        };
        SupertrendOutput {
            value,
            direction: self.direction,
        }
    }
}

impl<T> NextBatch<T> for Supertrend where T: Copy + High + Low + Close {}

impl Reset for Supertrend {
    fn reset(&mut self) {
        self.buckets.reset();
        self.atr.reset();
        self.upper = None;
        self.lower = None;
        self.direction = TrendDirection::Up;
        self.snapshot = None;
    }
}

impl Default for Supertrend {
    fn default() -> Self {
        Self::new(
            Duration::from_secs(10 * 86_400),
            Duration::from_secs(86_400),
            3.0,
        )
        .unwrap()
    }
}

impl fmt::Display for Supertrend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Supertrend(x{})", self.multiplier)
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
    fn crash_flips_to_downtrend() {
        let t = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        let mut st = Supertrend::new(
            Duration::from_secs(3 * 86_400),
            Duration::from_secs(86_400),
            3.0,
        )
        .unwrap();
        for i in 0..5 {
            let out = st.next((t + chrono::Duration::days(i), bar(11.0, 9.0, 10.0)));
            assert_eq!(out.direction, TrendDirection::Up);
        }
        // Flat bars park the lower band near 4; a close at 2 breaks it.
        let out = st.next((t + chrono::Duration::days(5), bar(3.0, 1.0, 2.0)));
        assert_eq!(out.direction, TrendDirection::Down);
    }

    #[test]
    fn revision_does_not_compound() {
        let t = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        let mut a = Supertrend::new(
            Duration::from_secs(3 * 86_400),
            Duration::from_secs(86_400),
            3.0,
        )
        .unwrap();
        let mut b = a.clone();
        for i in 0..3 {
            let input = (t + chrono::Duration::days(i), bar(11.0, 9.0, 10.0));
            a.next(input.clone());
            b.next(input);
        }
        let ts = t + chrono::Duration::days(3);
        let revised = a.next((ts, bar(12.0, 8.0, 11.0)));
        let fresh = b.next((ts, bar(12.0, 8.0, 11.0)));
        assert_eq!(revised.direction, fresh.direction);
        assert!((revised.value - fresh.value).abs() < 1e-9);
        // Evaluating the same bucket again must be idempotent.
        let again = a.next((ts, bar(12.0, 8.0, 11.0)));
        assert!((again.value - revised.value).abs() < 1e-9);
    }
}
