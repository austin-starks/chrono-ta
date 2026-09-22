use std::fmt;
use std::time::Duration;

use chrono::{DateTime, Utc};
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use super::fixed_time_bucket::{BucketUpdate, FixedTimeBucket};
use crate::errors::{Result, TaError};
use crate::{Close, High, Low, Next, NextBatch, Reset};

/// Parabolic SAR over timestamped OHLC bars.
///
/// Standard Wilder formulation: in an uptrend SAR trails below as
/// `sar += af × (ep − sar)` with the extreme point ratcheted to each new
/// high and the acceleration factor stepping up by `step` (capped at
/// `maximum`) per new extreme; mirrored in a downtrend. The trend reverses
/// when the price crosses the SAR, and the new SAR seeds at the prior
/// extreme point. Starts long from the first bar.
///
/// Path-dependent state (trend, extreme point, acceleration factor, SAR) is
/// snapshotted on every new bucket. Revisions inside `bucket_width` restore
/// the snapshot before re-applying, so a live bar re-evaluates from the
/// sealed state instead of compounding onto its own earlier revision.
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Debug, Clone)]
pub struct ParabolicSar {
    step: f64,
    maximum: f64,
    buckets: FixedTimeBucket,
    long: bool,
    extreme: Option<f64>,
    acceleration: f64,
    sar: Option<f64>,
    /// Highs/lows of the last two *sealed* buckets for Wilder's SAR clamp.
    /// The live bar seals into these on the next append; it never clamps
    /// itself. Restored with the snapshot on revision.
    highs: std::collections::VecDeque<f64>,
    lows: std::collections::VecDeque<f64>,
    live: Option<(f64, f64)>,
    snapshot: Option<SarSnapshot>,
}

/// Sealed-bucket state restored when a live-bar revision re-evaluates.
type SarSnapshot = (
    bool,
    Option<f64>,
    f64,
    Option<f64>,
    Vec<f64>,
    Vec<f64>,
);

impl ParabolicSar {
    pub fn new(bucket_width: Duration, step: f64, maximum: f64) -> Result<Self> {
        if bucket_width.is_zero() {
            return Err(TaError::InvalidParameter);
        }
        if !step.is_finite() || step <= 0.0 || !maximum.is_finite() || maximum < step {
            return Err(TaError::InvalidParameter);
        }
        Ok(Self {
            step,
            maximum,
            buckets: FixedTimeBucket::new(bucket_width)?,
            long: true,
            extreme: None,
            acceleration: step,
            sar: None,
            highs: std::collections::VecDeque::new(),
            lows: std::collections::VecDeque::new(),
            live: None,
            snapshot: None,
        })
    }
}

impl<T> Next<T> for ParabolicSar
where
    T: High + Low + Close,
{
    type Output = f64;

    fn next(&mut self, (timestamp, input): (DateTime<Utc>, T)) -> Self::Output {
        let update = self.buckets.update(timestamp);
        if update == BucketUpdate::Append {
            if let Some((live_high, live_low)) = self.live.take() {
                self.highs.push_back(live_high);
                self.lows.push_back(live_low);
                while self.highs.len() > 2 {
                    self.highs.pop_front();
                }
                while self.lows.len() > 2 {
                    self.lows.pop_front();
                }
            }
            self.snapshot = Some((
                self.long,
                self.extreme,
                self.acceleration,
                self.sar,
                self.highs.iter().copied().collect(),
                self.lows.iter().copied().collect(),
            ));
        } else if let Some((long, extreme, acceleration, sar, highs, lows)) =
            self.snapshot.clone()
        {
            self.long = long;
            self.extreme = extreme;
            self.acceleration = acceleration;
            self.sar = sar;
            self.highs = highs.into_iter().collect();
            self.lows = lows.into_iter().collect();
        }
        let (high, low) = (input.high(), input.low());
        self.live = Some((high, low));

        // Seed from the first bar: long with the low as the initial SAR.
        let mut sar = match self.sar {
            Some(sar) => sar,
            None => {
                self.extreme = Some(high);
                self.acceleration = self.step;
                self.sar = Some(low);
                return low;
            }
        };
        let mut extreme = self.extreme.unwrap_or(if self.long { high } else { low });

        if self.long {
            if low < sar {
                self.long = false;
                self.extreme = Some(low);
                self.acceleration = self.step;
                sar = extreme;
            } else {
                if high > extreme {
                    extreme = high;
                    self.acceleration = (self.acceleration + self.step).min(self.maximum);
                }
                sar += self.acceleration * (extreme - sar);
                // Wilder's clamp: never above the prior two sealed periods' lows.
                for prior in self.lows.iter() {
                    sar = sar.min(*prior);
                }
                self.extreme = Some(extreme);
            }
        } else {
            if high > sar {
                self.long = true;
                self.extreme = Some(high);
                self.acceleration = self.step;
                sar = extreme;
            } else {
                if low < extreme {
                    extreme = low;
                    self.acceleration = (self.acceleration + self.step).min(self.maximum);
                }
                sar += self.acceleration * (extreme - sar);
                // Wilder's clamp: never below the prior two sealed periods' highs.
                for prior in self.highs.iter() {
                    sar = sar.max(*prior);
                }
                self.extreme = Some(extreme);
            }
        }
        self.sar = Some(sar);
        sar
    }
}

impl<T> NextBatch<T> for ParabolicSar where T: Copy + High + Low + Close {}

impl Reset for ParabolicSar {
    fn reset(&mut self) {
        self.buckets.reset();
        self.long = true;
        self.extreme = None;
        self.acceleration = self.step;
        self.sar = None;
        self.highs.clear();
        self.lows.clear();
        self.live = None;
        self.snapshot = None;
    }
}

impl Default for ParabolicSar {
    fn default() -> Self {
        Self::new(Duration::from_secs(86_400), 0.02, 0.2).unwrap()
    }
}

impl fmt::Display for ParabolicSar {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ParabolicSAR(step={}, max={})", self.step, self.maximum)
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
    fn uptrend_sar_trails_below_price() {
        let t = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        let mut sar = ParabolicSar::default();
        let mut out = 0.0;
        for i in 0..6 {
            let c = 10.0 + i as f64;
            out = sar.next((t + chrono::Duration::days(i), bar(c + 0.5, c - 0.5, c)));
        }
        assert!(out < 15.0, "sar = {out}");
        assert!(sar.long);
    }

    #[test]
    fn crash_reverses_to_short() {
        let t = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        let mut sar = ParabolicSar::default();
        for i in 0..4 {
            let c = 10.0 + i as f64;
            sar.next((t + chrono::Duration::days(i), bar(c + 0.5, c - 0.5, c)));
        }
        sar.next((t + chrono::Duration::days(4), bar(9.0, 3.0, 4.0)));
        assert!(!sar.long);
    }
}
