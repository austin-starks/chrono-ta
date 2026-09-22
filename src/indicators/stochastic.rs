use std::collections::VecDeque;
use std::fmt;
use std::time::Duration;

use chrono::{DateTime, Utc};
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use super::fixed_time_bucket::{BucketUpdate, FixedTimeBucket};
use super::window_aggregate::SlidingExtrema;
use crate::errors::{Result, TaError};
use crate::{Close, High, Low, Next, NextBatch, Reset};

/// Slow-stochastic output: smoothed %K and its %D signal average.
#[derive(Debug, Clone, PartialEq)]
pub struct StochasticOutput {
    pub k: f64,
    pub d: f64,
}

/// Stochastic oscillator over timestamped OHLC bars.
///
/// %K = 100 * (close − lowest low) / (highest high − lowest low) over the
/// lookback window, smoothed with a moving average over `smooth_k` values;
/// %D is the moving average of smoothed %K over `smooth_d` values. Averages
/// run over whatever values exist so the indicator emits from the first bar.
/// A flat window (highest == lowest) yields 50.0 (neutral) rather than a
/// divide-by-zero.
///
/// Revisions inside `bucket_width` revise the current bar: the back bar and
/// the back %K contribution are swapped, never double-counted.
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Debug, Clone)]
pub struct Stochastic {
    window: Duration,
    buckets: FixedTimeBucket,
    bars: VecDeque<(DateTime<Utc>, f64, f64, f64)>,
    smooth_k: usize,
    smooth_d: usize,
    raw_k: VecDeque<f64>,
    smooth_history: VecDeque<f64>,
    /// Highest/lowest over committed bars; the live bar folds in at query.
    /// Derived state, rebuilt from `bars` after deserialize via `ensure_built`.
    #[cfg_attr(feature = "serde", serde(skip))]
    ext: SlidingExtrema,
    #[cfg_attr(feature = "serde", serde(skip))]
    ext_built: bool,
}

impl Stochastic {
    pub fn new(
        window: Duration,
        bucket_width: Duration,
        smooth_k: usize,
        smooth_d: usize,
    ) -> Result<Self> {
        if window.is_zero() || bucket_width.is_zero() || bucket_width > window {
            return Err(TaError::InvalidParameter);
        }
        if smooth_k == 0 || smooth_d == 0 {
            return Err(TaError::InvalidParameter);
        }
        Ok(Self {
            window,
            buckets: FixedTimeBucket::new(bucket_width)?,
            bars: VecDeque::new(),
            smooth_k,
            smooth_d,
            raw_k: VecDeque::new(),
            smooth_history: VecDeque::new(),
            ext: SlidingExtrema::default(),
            ext_built: true,
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
            .is_some_and(|(ts, _, _, _)| ts.timestamp_nanos_opt().unwrap_or(i64::MIN) <= cutoff)
        {
            self.bars.pop_front();
        }
        self.ext.expire_before(cutoff);
    }

    fn ensure_built(&mut self) {
        if self.ext_built {
            return;
        }
        self.ext.clear();
        let committed = self.bars.len().saturating_sub(1);
        for &(ts, h, l, _) in self.bars.iter().take(committed) {
            self.ext.commit(ts, h, l);
        }
        self.ext_built = true;
    }

    fn average_last(values: &VecDeque<f64>, n: usize) -> f64 {
        let len = values.len().min(n).max(1);
        values.iter().rev().take(len).sum::<f64>() / len as f64
    }
}

impl<T> Next<T> for Stochastic
where
    T: High + Low + Close,
{
    type Output = StochasticOutput;

    fn next(&mut self, (timestamp, input): (DateTime<Utc>, T)) -> Self::Output {
        self.ensure_built();
        let update = self.buckets.update(timestamp);
        if update == BucketUpdate::Replace {
            self.bars.pop_back();
            self.raw_k.pop_back();
            self.smooth_history.pop_back();
        } else if let Some(&(ts, h, l, _)) = self.bars.back() {
            self.ext.commit(ts, h, l);
        }

        let (high, low, close) = (input.high(), input.low(), input.close());
        self.bars.push_back((timestamp, high, low, close));
        self.remove_expired(timestamp);

        let (highest, lowest) = self.ext.extremes(high, low);
        let raw = if highest > lowest {
            100.0 * (close - lowest) / (highest - lowest)
        } else {
            50.0
        };
        self.raw_k.push_back(raw);
        while self.raw_k.len() > self.smooth_k {
            self.raw_k.pop_front();
        }

        let k = Self::average_last(&self.raw_k, self.smooth_k);
        self.smooth_history.push_back(k);
        while self.smooth_history.len() > self.smooth_d {
            self.smooth_history.pop_front();
        }
        let d = Self::average_last(&self.smooth_history, self.smooth_d);
        StochasticOutput { k, d }
    }
}

impl<T> NextBatch<T> for Stochastic where T: Copy + High + Low + Close {}

impl Reset for Stochastic {
    fn reset(&mut self) {
        self.buckets.reset();
        self.bars.clear();
        self.raw_k.clear();
        self.smooth_history.clear();
        self.ext.clear();
        self.ext_built = true;
    }
}

impl Default for Stochastic {
    fn default() -> Self {
        Self::new(
            Duration::from_secs(14 * 86_400),
            Duration::from_secs(86_400),
            3,
            3,
        )
        .unwrap()
    }
}

impl fmt::Display for Stochastic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Stochastic({}s window, {}s buckets, smooth {}/{})",
            self.window.as_secs(),
            self.buckets.width().as_secs(),
            self.smooth_k,
            self.smooth_d,
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

    fn feed(stoch: &mut Stochastic, t: DateTime<Utc>, closes: &[f64]) -> StochasticOutput {
        let mut out = None;
        for (i, c) in closes.iter().enumerate() {
            out = Some(
                stoch.next((
                    t + chrono::Duration::days(i as i64),
                    bar(c + 1.0, c - 1.0, *c),
                )),
            );
        }
        out.unwrap()
    }

    #[test]
    fn rising_market_pushes_k_toward_100() {
        let t = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        let mut stoch = Stochastic::new(
            Duration::from_secs(5 * 86_400),
            Duration::from_secs(86_400),
            1,
            1,
        )
        .unwrap();
        let out = feed(&mut stoch, t, &[10.0, 11.0, 12.0, 13.0, 14.0]);
        // Final bar: HH = 15, LL = 9, close 14 → 100 × 5/6 ≈ 83.3.
        assert!((out.k - 83.333_333_333_333_33).abs() < 1e-9, "k = {}", out.k);
        assert!((out.k - out.d).abs() < 1e-9);
    }

    #[test]
    fn revision_replaces_instead_of_double_counting() {
        let t = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        let mut a = Stochastic::new(
            Duration::from_secs(5 * 86_400),
            Duration::from_secs(86_400),
            1,
            1,
        )
        .unwrap();
        let mut b = a.clone();
        feed(&mut a, t, &[10.0, 11.0]);
        feed(&mut b, t, &[10.0, 11.0]);
        let revised = a.next((t + chrono::Duration::days(1), bar(13.0, 9.0, 12.0)));
        let fresh = b.next((t + chrono::Duration::days(1), bar(13.0, 9.0, 12.0)));
        assert!((revised.k - fresh.k).abs() < 1e-9);
        // A second evaluation of the same bucket revises, not appends.
        let again = a.next((t + chrono::Duration::days(1), bar(13.0, 9.0, 12.0)));
        assert!((again.k - revised.k).abs() < 1e-9);
    }

    #[test]
    fn rejects_bad_params() {
        assert!(Stochastic::new(
            Duration::ZERO,
            Duration::from_secs(60),
            3,
            3
        )
        .is_err());
        assert!(Stochastic::new(
            Duration::from_secs(60),
            Duration::from_secs(60),
            0,
            3
        )
        .is_err());
    }

    /// Incremental extrema + bounded smoothing match the definitional rescan
    /// under mixed appends, same-bucket revisions, and window evictions.
    /// Min/max selection is bitwise exact, so this asserts exact equality.
    #[test]
    fn equivalence_with_naive_under_appends_replaces_and_evictions() {
        let mut state: u64 = 0x9e3779b97f4a7c15;
        let mut rng = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        let start = Utc.with_ymd_and_hms(2021, 6, 1, 0, 0, 0).unwrap();

        for _ in 0..100 {
            let window_days = 2 + rng() % 30;
            let smooth_k = 1 + rng() % 5;
            let smooth_d = 1 + rng() % 5;
            let mut stoch = Stochastic::new(
                Duration::from_secs(window_days * 86_400),
                Duration::from_secs(86_400),
                smooth_k as usize,
                smooth_d as usize,
            )
            .unwrap();
            // Naive log: Vec of (day, high, low, close); same-day revises last.
            let mut bars: Vec<(i64, f64, f64, f64)> = Vec::new();
            let mut raw_hist: Vec<f64> = Vec::new();
            let mut smooth_hist: Vec<f64> = Vec::new();
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
                let (h, l, c) = (
                    base + (rng() % 500) as f64 / 100.0,
                    base - (rng() % 500) as f64 / 100.0,
                    base,
                );
                let ts = start + chrono::Duration::days(day);
                let got = stoch.next((ts, bar(h, l, c)));
                if roll < 3 && !bars.is_empty() {
                    let last = bars.last_mut().unwrap();
                    *last = (day, h, l, c);
                    raw_hist.pop();
                    smooth_hist.pop();
                } else {
                    bars.push((day, h, l, c));
                }
                bars.retain(|(d, _, _, _)| *d > day - window_days as i64);
                let highest = bars
                    .iter()
                    .map(|(_, h, _, _)| *h)
                    .fold(f64::NEG_INFINITY, f64::max);
                let lowest = bars
                    .iter()
                    .map(|(_, _, l, _)| *l)
                    .fold(f64::INFINITY, f64::min);
                let raw = if highest > lowest {
                    100.0 * (c - lowest) / (highest - lowest)
                } else {
                    50.0
                };
                raw_hist.push(raw);
                while raw_hist.len() > smooth_k as usize {
                    raw_hist.remove(0);
                }
                // Newest-first, matching `average_last` summation order exactly.
                let k = raw_hist.iter().rev().sum::<f64>() / raw_hist.len() as f64;
                smooth_hist.push(k);
                while smooth_hist.len() > smooth_d as usize {
                    smooth_hist.remove(0);
                }
                let d = smooth_hist.iter().rev().sum::<f64>() / smooth_hist.len() as f64;
                assert_eq!(got.k, k, "k mismatch (day={day})");
                assert_eq!(got.d, d, "d mismatch (day={day})");
            }
        }
    }
}
