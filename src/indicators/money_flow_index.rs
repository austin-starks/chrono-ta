use std::collections::VecDeque;
use std::fmt;
use std::time::Duration;

use chrono::{DateTime, Utc};
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use super::fixed_time_bucket::{BucketUpdate, FixedTimeBucket};
use crate::errors::{Result, TaError};
use crate::{Close, High, Low, Next, NextBatch, Reset, Volume};

/// Money Flow Index over timestamped OHLCV bars.
///
/// Money flow = typical price × volume, signed positive when the typical
/// price rose from the previous bucket and negative when it fell (flat keeps
/// the previous sign bucket out of both sums). MFI = 100 − 100 / (1 + MR)
/// with MR = positive flow / negative flow over the window; zero negative
/// flow reads 100.0. Runs over whatever bars exist so it emits from the
/// first bar.
///
/// Revisions inside `bucket_width` revise the current bar.
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Debug, Clone)]
pub struct MoneyFlowIndex {
    window: Duration,
    buckets: FixedTimeBucket,
    bars: VecDeque<(DateTime<Utc>, f64, f64)>,
    /// Signed money flow per bar, parallel to `bars`: +tp×vol when the
    /// typical rose vs its in-window predecessor, −tp×vol when it fell, 0
    /// for the window front (nothing before it) and flat bars. The front
    /// entry is always 0 — expiry re-zeroes the new front.
    contrib: VecDeque<f64>,
    positive: f64,
    negative: f64,
    /// Bars with strictly negative flow. Zero ⟺ the rescan sum is exactly
    /// 0, which the running `negative` can miss by float dust.
    down_count: usize,
}

impl MoneyFlowIndex {
    pub fn new(window: Duration, bucket_width: Duration) -> Result<Self> {
        if window.is_zero() || bucket_width.is_zero() || bucket_width > window {
            return Err(TaError::InvalidParameter);
        }
        Ok(Self {
            window,
            buckets: FixedTimeBucket::new(bucket_width)?,
            bars: VecDeque::new(),
            contrib: VecDeque::new(),
            positive: 0.0,
            negative: 0.0,
            down_count: 0,
        })
    }

    fn add_counted(&mut self, c: f64) {
        self.add_contrib(c);
        if c < 0.0 {
            self.down_count += 1;
        }
    }

    fn remove_counted(&mut self, c: f64) {
        self.remove_contrib(c);
        if c < 0.0 {
            self.down_count = self.down_count.saturating_sub(1);
        }
    }

    fn cutoff_nanos(&self, now: DateTime<Utc>) -> i64 {
        let nanos = i64::try_from(self.window.as_nanos()).unwrap_or(i64::MAX);
        now.timestamp_nanos_opt()
            .unwrap_or(i64::MIN)
            .saturating_sub(nanos)
    }

    fn add_contrib(&mut self, c: f64) {
        if c > 0.0 {
            self.positive += c;
        } else if c < 0.0 {
            self.negative -= c;
        }
    }

    fn remove_contrib(&mut self, c: f64) {
        if c > 0.0 {
            self.positive -= c;
        } else if c < 0.0 {
            self.negative += c;
        }
    }

    fn classify(new_tp: f64, prev_tp: Option<f64>, volume: f64) -> f64 {
        match prev_tp {
            Some(prev) if new_tp > prev => new_tp * volume,
            Some(prev) if new_tp < prev => -(new_tp * volume),
            _ => 0.0,
        }
    }

    fn remove_expired(&mut self, now: DateTime<Utc>) {
        let cutoff = self.cutoff_nanos(now);
        while self
            .bars
            .front()
            .is_some_and(|(ts, _, _)| ts.timestamp_nanos_opt().unwrap_or(i64::MIN) <= cutoff)
        {
            self.bars.pop_front();
            if let Some(c) = self.contrib.pop_front() {
                self.remove_counted(c);
            }
        }
        // The expired front was the new front's classifier: it contributes 0 now.
        if let Some(c) = self.contrib.front().copied() {
            self.remove_counted(c);
            self.contrib[0] = 0.0;
        }
    }
}

impl<T> Next<T> for MoneyFlowIndex
where
    T: High + Low + Close + Volume,
{
    type Output = f64;

    fn next(&mut self, (timestamp, input): (DateTime<Utc>, T)) -> Self::Output {
        let update = self.buckets.update(timestamp);
        if update == BucketUpdate::Replace {
            self.bars.pop_back();
            if let Some(c) = self.contrib.pop_back() {
                self.remove_counted(c);
            }
        }
        let typical = (input.high() + input.low() + input.close()) / 3.0;
        let prev_tp = self.bars.back().map(|(_, tp, _)| *tp);
        let c = Self::classify(typical, prev_tp, input.volume());
        self.add_counted(c);
        self.bars.push_back((timestamp, typical, input.volume()));
        self.contrib.push_back(c);
        self.remove_expired(timestamp);

        // `down_count` is exact where the running sum can carry dust: no
        // down-flow bars ⟺ the definitional sum is exactly 0 ⟹ 100.0.
        // The clamps cover the mirror case (dust-negative sums beside real
        // flow), which the ratio's singularity would otherwise blow up.
        if self.down_count == 0 {
            100.0
        } else {
            let positive = self.positive.max(0.0);
            let negative = self.negative.max(0.0);
            if negative == 0.0 {
                100.0
            } else {
                100.0 - 100.0 / (1.0 + positive / negative)
            }
        }
    }
}

impl<T> NextBatch<T> for MoneyFlowIndex where T: Copy + High + Low + Close + Volume {}

impl Reset for MoneyFlowIndex {
    fn reset(&mut self) {
        self.buckets.reset();
        self.bars.clear();
        self.contrib.clear();
        self.positive = 0.0;
        self.negative = 0.0;
        self.down_count = 0;
    }
}

impl Default for MoneyFlowIndex {
    fn default() -> Self {
        Self::new(
            Duration::from_secs(14 * 86_400),
            Duration::from_secs(86_400),
        )
        .unwrap()
    }
}

impl fmt::Display for MoneyFlowIndex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "MFI({}s window, {}s buckets)",
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

    fn bar(tp: f64, vol: f64) -> DataItem {
        DataItem::builder()
            .open(tp)
            .high(tp + 0.5)
            .low(tp - 0.5)
            .close(tp)
            .volume(vol)
            .build()
            .unwrap()
    }

    #[test]
    fn all_up_flow_reads_100() {
        let t = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        let mut mfi = MoneyFlowIndex::new(
            Duration::from_secs(3 * 86_400),
            Duration::from_secs(86_400),
        )
        .unwrap();
        let mut out = 0.0;
        for (i, tp) in [10.0, 11.0, 12.0].iter().enumerate() {
            out = mfi.next((t + chrono::Duration::days(i as i64), bar(*tp, 100.0)));
        }
        assert_eq!(out, 100.0);
    }

    #[test]
    fn balanced_flow_reads_50() {
        let t = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        let mut mfi = MoneyFlowIndex::new(
            Duration::from_secs(3 * 86_400),
            Duration::from_secs(86_400),
        )
        .unwrap();
        let mut out = 0.0;
        // Up flow 11×100, down flow 10×100 → MR = 1.1 → 100 − 100/2.1.
        for (i, tp) in [10.0, 11.0, 10.0].iter().enumerate() {
            out = mfi.next((t + chrono::Duration::days(i as i64), bar(*tp, 100.0)));
        }
        assert!((out - 52.380_952_380_952_38).abs() < 1e-9, "mfi = {out}");
    }

    /// Incremental signed sums match the definitional rescan under mixed
    /// appends, same-bucket revisions, and evictions. Summation order differs,
    /// so this asserts approximate (1e-9) equality.
    #[test]
    fn equivalence_with_naive_under_appends_replaces_and_evictions() {
        let mut state: u64 = 0x77aa55cc33ee1100;
        let mut rng = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        let start = Utc.with_ymd_and_hms(2021, 6, 1, 0, 0, 0).unwrap();

        for cfg in 0..100 {
            let window_days = 2 + rng() % 30;
            let mut mfi = MoneyFlowIndex::new(
                Duration::from_secs(window_days * 86_400),
                Duration::from_secs(86_400),
            )
            .unwrap();
            let mut bars: Vec<(i64, f64, f64)> = Vec::new();
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
                let tp = 10.0 + (rng() % 10_000) as f64 / 100.0;
                let vol = 100.0 + (rng() % 9_900) as f64;
                let got = mfi.next((start + chrono::Duration::days(day), bar(tp, vol)));
                if roll < 3 && !bars.is_empty() {
                    *bars.last_mut().unwrap() = (day, tp, vol);
                } else {
                    bars.push((day, tp, vol));
                }
                bars.retain(|(d, _, _)| *d > day - window_days as i64);
                let mut pos = 0.0;
                let mut neg = 0.0;
                for pair in bars.iter().zip(bars.iter().skip(1)) {
                    let ((_, prev, _), (_, tp, v)) = (pair.0, pair.1);
                    if tp > prev {
                        pos += tp * v;
                    } else if tp < prev {
                        neg += tp * v;
                    }
                }
                let expected = if neg == 0.0 {
                    100.0
                } else {
                    100.0 - 100.0 / (1.0 + pos / neg)
                };
                assert!(
                    (got - expected).abs() < 1e-9,
                    "mismatch: cfg={cfg} day={day} bars={bars:?} got {got} vs {expected}"
                );
            }
        }
    }
}
