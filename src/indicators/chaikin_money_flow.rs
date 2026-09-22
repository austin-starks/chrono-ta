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
    flow_sum: f64,
    vol_sum: f64,
    /// Bars with exactly zero volume. All-zero ⟺ the rescan volume sum is
    /// exactly 0, which the running `vol_sum` can miss by float dust.
    zero_vol: usize,
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
            flow_sum: 0.0,
            vol_sum: 0.0,
            zero_vol: 0,
        })
    }

    fn cutoff_nanos(&self, now: DateTime<Utc>) -> i64 {
        let nanos = i64::try_from(self.window.as_nanos()).unwrap_or(i64::MAX);
        now.timestamp_nanos_opt()
            .unwrap_or(i64::MIN)
            .saturating_sub(nanos)
    }

    fn add_bar(&mut self, mfv: f64, vol: f64) {
        self.flow_sum += mfv;
        self.vol_sum += vol;
        if vol == 0.0 {
            self.zero_vol += 1;
        }
    }

    fn remove_bar(&mut self, mfv: f64, vol: f64) {
        self.flow_sum -= mfv;
        self.vol_sum -= vol;
        if vol == 0.0 {
            self.zero_vol = self.zero_vol.saturating_sub(1);
        }
    }

    fn remove_expired(&mut self, now: DateTime<Utc>) {
        let cutoff = self.cutoff_nanos(now);
        while self
            .bars
            .front()
            .is_some_and(|(ts, _, _)| ts.timestamp_nanos_opt().unwrap_or(i64::MIN) <= cutoff)
        {
            if let Some((_, mfv, vol)) = self.bars.pop_front() {
                self.remove_bar(mfv, vol);
            }
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
        if update == BucketUpdate::Replace {
            if let Some((_, mfv, vol)) = self.bars.pop_back() {
                self.remove_bar(mfv, vol);
            }
        }
        let (high, low, close, volume) =
            (input.high(), input.low(), input.close(), input.volume());
        let clv = if high > low {
            ((close - low) - (high - close)) / (high - low)
        } else {
            0.0
        };
        let (mfv, vol) = (clv * volume, volume);
        self.add_bar(mfv, vol);
        self.bars.push_back((timestamp, mfv, vol));
        self.remove_expired(timestamp);

        if self.zero_vol == self.bars.len() {
            0.0
        } else {
            self.flow_sum / self.vol_sum
        }
    }
}

impl<T> NextBatch<T> for ChaikinMoneyFlow where T: Copy + High + Low + Close + Volume {}

impl Reset for ChaikinMoneyFlow {
    fn reset(&mut self) {
        self.buckets.reset();
        self.bars.clear();
        self.flow_sum = 0.0;
        self.vol_sum = 0.0;
        self.zero_vol = 0;
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

    /// Incremental sums match the definitional rescan under mixed appends,
    /// same-bucket revisions, and evictions. Summation order differs, so this
    /// asserts approximate (1e-9) equality.
    #[test]
    fn equivalence_with_naive_under_appends_replaces_and_evictions() {
        let mut state: u64 = 0x3c1e5a7b9d0f2244;
        let mut rng = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        let start = Utc.with_ymd_and_hms(2021, 6, 1, 0, 0, 0).unwrap();

        for _ in 0..100 {
            let window_days = 2 + rng() % 30;
            let mut cmf = ChaikinMoneyFlow::new(
                Duration::from_secs(window_days * 86_400),
                Duration::from_secs(86_400),
            )
            .unwrap();
            let mut bars: Vec<(i64, f64, f64, f64, f64)> = Vec::new();
            let mut day: i64 = 0;
            for _ in 0..400 {
                let roll = rng() % 10;
                if roll < 3 && !bars.is_empty() {
                    // Same bucket: revise the live bar.
                } else if roll < 4 {
                    day += 1 + (rng() % 40) as i64;
                } else {
                    day += 1;
                }
                let base = 10.0 + (rng() % 10_000) as f64 / 100.0;
                let h = base + (rng() % 500) as f64 / 100.0;
                let l = base - (rng() % 500) as f64 / 100.0;
                let v = 100.0 + (rng() % 9_900) as f64;
                let got = cmf.next((start + chrono::Duration::days(day), bar(h, l, base, v)));
                if roll < 3 && !bars.is_empty() {
                    *bars.last_mut().unwrap() = (day, h, l, base, v);
                } else {
                    bars.push((day, h, l, base, v));
                }
                bars.retain(|(d, _, _, _, _)| *d > day - window_days as i64);
                let mut flow = 0.0;
                let mut vol = 0.0;
                for (_, h, l, c, v) in bars.iter() {
                    let clv = if h > l {
                        ((c - l) - (h - c)) / (h - l)
                    } else {
                        0.0
                    };
                    flow += clv * v;
                    vol += v;
                }
                let expected = if vol == 0.0 { 0.0 } else { flow / vol };
                assert!(
                    (got - expected).abs() < 1e-9,
                    "mismatch: got {got} vs {expected} (day={day})"
                );
            }
        }
    }
}
