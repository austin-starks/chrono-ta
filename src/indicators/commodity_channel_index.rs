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
    bars: VecDeque<(DateTime<Utc>, f64)>,
    tp_sum: f64,
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
            tp_sum: 0.0,
            constant,
        })
    }

    fn cutoff_nanos(&self, now: DateTime<Utc>) -> i64 {
        let nanos = i64::try_from(self.window.as_nanos()).unwrap_or(i64::MAX);
        now.timestamp_nanos_opt()
            .unwrap_or(i64::MIN)
            .saturating_sub(nanos)
    }

    fn remove_expired(&mut self, now: DateTime<Utc>) {
        let cutoff = self.cutoff_nanos(now);
        while self
            .bars
            .front()
            .is_some_and(|(ts, _)| ts.timestamp_nanos_opt().unwrap_or(i64::MIN) <= cutoff)
        {
            if let Some((_, tp)) = self.bars.pop_front() {
                self.tp_sum -= tp;
            }
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
        if update == BucketUpdate::Replace {
            if let Some((_, tp)) = self.bars.pop_back() {
                self.tp_sum -= tp;
            }
        }
        let typical = (input.high() + input.low() + input.close()) / 3.0;
        self.tp_sum += typical;
        self.bars.push_back((timestamp, typical));
        self.remove_expired(timestamp);

        // The mean comes from the running sum; the mean deviation still needs
        // one pass — it is defined around the current mean, so it cannot be
        // maintained incrementally. The flat check is exact (highest == lowest)
        // rather than `deviation == 0.0`: float dust in the running mean would
        // otherwise miss a truly flat window and blow the ratio up.
        let n = self.bars.len() as f64;
        let mean = self.tp_sum / n;
        let mut highest = f64::NEG_INFINITY;
        let mut lowest = f64::INFINITY;
        let mut dev_sum = 0.0;
        for (_, tp) in self.bars.iter() {
            highest = highest.max(*tp);
            lowest = lowest.min(*tp);
            dev_sum += (tp - mean).abs();
        }
        let deviation = dev_sum / n;
        let current = self.bars.back().map(|(_, tp)| *tp).unwrap_or(mean);
        if highest == lowest {
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
        self.tp_sum = 0.0;
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

    /// Incremental mean matches the definitional rescan under mixed appends,
    /// same-bucket revisions, and evictions. Summation order differs, so this
    /// asserts approximate (1e-9) equality.
    #[test]
    fn equivalence_with_naive_under_appends_replaces_and_evictions() {
        let mut state: u64 = 0x8f2b4c6d1e3a5967;
        let mut rng = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        let start = Utc.with_ymd_and_hms(2021, 6, 1, 0, 0, 0).unwrap();

        for cfg in 0..100 {
            let window_days = 2 + rng() % 30;
            let mut cci = CommodityChannelIndex::new(
                Duration::from_secs(window_days * 86_400),
                Duration::from_secs(86_400),
                0.015,
            )
            .unwrap();
            let mut bars: Vec<(i64, f64)> = Vec::new();
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
                let got = cci.next((start + chrono::Duration::days(day), bar(h, l, base)));
                let tp = (h + l + base) / 3.0;
                if roll < 3 && !bars.is_empty() {
                    *bars.last_mut().unwrap() = (day, tp);
                } else {
                    bars.push((day, tp));
                }
                bars.retain(|(d, _)| *d > day - window_days as i64);
                let mean = bars.iter().map(|(_, tp)| tp).sum::<f64>() / bars.len() as f64;
                let dev =
                    bars.iter().map(|(_, tp)| (tp - mean).abs()).sum::<f64>() / bars.len() as f64;
                let current = bars.last().map(|(_, tp)| *tp).unwrap_or(mean);
                let expected = if dev == 0.0 {
                    0.0
                } else {
                    (current - mean) / (0.015 * dev)
                };
                // Relative tolerance: the running mean carries dust that the
                // ratio can amplify on large-magnitude outputs.
                let tol = 1e-9 * expected.abs().max(1.0);
                assert!(
                    (got - expected).abs() <= tol,
                    "mismatch: cfg={cfg} day={day} bars={bars:?} got {got} vs {expected}"
                );
            }
        }
    }
}
