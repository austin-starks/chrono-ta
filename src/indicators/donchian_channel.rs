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

/// Donchian channel output: highest high, lowest low, and midline.
#[derive(Debug, Clone, PartialEq)]
pub struct DonchianOutput {
    pub upper: f64,
    pub lower: f64,
    pub middle: f64,
}

/// Donchian channel over timestamped OHLC bars.
///
/// Upper = highest high and lower = lowest low over the lookback window;
/// middle is their midpoint. Runs over whatever bars exist so the channel
/// emits from the first bar.
///
/// Revisions inside `bucket_width` revise the current bar.
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Debug, Clone)]
pub struct DonchianChannel {
    window: Duration,
    buckets: FixedTimeBucket,
    bars: VecDeque<(DateTime<Utc>, f64, f64)>,
    /// Highest/lowest over committed bars; the live bar folds in at query.
    /// Derived state, rebuilt from `bars` after deserialize via `ensure_built`.
    #[cfg_attr(feature = "serde", serde(skip))]
    ext: SlidingExtrema,
    #[cfg_attr(feature = "serde", serde(skip))]
    ext_built: bool,
}

impl DonchianChannel {
    pub fn new(window: Duration, bucket_width: Duration) -> Result<Self> {
        if window.is_zero() || bucket_width.is_zero() || bucket_width > window {
            return Err(TaError::InvalidParameter);
        }
        Ok(Self {
            window,
            buckets: FixedTimeBucket::new(bucket_width)?,
            bars: VecDeque::new(),
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
            .is_some_and(|(ts, _, _)| ts.timestamp_nanos_opt().unwrap_or(i64::MIN) <= cutoff)
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
        for &(ts, h, l) in self.bars.iter().take(committed) {
            self.ext.commit(ts, h, l);
        }
        self.ext_built = true;
    }
}

impl<T> Next<T> for DonchianChannel
where
    T: High + Low + Close,
{
    type Output = DonchianOutput;

    fn next(&mut self, (timestamp, input): (DateTime<Utc>, T)) -> Self::Output {
        self.ensure_built();
        let update = self.buckets.update(timestamp);
        if update == BucketUpdate::Replace {
            self.bars.pop_back();
        } else if let Some(&(ts, h, l)) = self.bars.back() {
            self.ext.commit(ts, h, l);
        }
        let (high, low) = (input.high(), input.low());
        self.bars.push_back((timestamp, high, low));
        self.remove_expired(timestamp);

        let (upper, lower) = self.ext.extremes(high, low);
        DonchianOutput {
            upper,
            lower,
            middle: (upper + lower) / 2.0,
        }
    }
}

impl<T> NextBatch<T> for DonchianChannel where T: Copy + High + Low + Close {}

impl Reset for DonchianChannel {
    fn reset(&mut self) {
        self.buckets.reset();
        self.bars.clear();
        self.ext.clear();
        self.ext_built = true;
    }
}

impl Default for DonchianChannel {
    fn default() -> Self {
        Self::new(
            Duration::from_secs(20 * 86_400),
            Duration::from_secs(86_400),
        )
        .unwrap()
    }
}

impl fmt::Display for DonchianChannel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Donchian({}s window, {}s buckets)",
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

    fn bar(high: f64, low: f64) -> DataItem {
        DataItem::builder()
            .open(low)
            .high(high)
            .low(low)
            .close(low)
            .volume(1.0)
            .build()
            .unwrap()
    }

    #[test]
    fn tracks_highest_high_and_lowest_low() {
        let t = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        let mut dc = DonchianChannel::new(
            Duration::from_secs(3 * 86_400),
            Duration::from_secs(86_400),
        )
        .unwrap();
        dc.next((t, bar(10.0, 8.0)));
        dc.next((t + chrono::Duration::days(1), bar(12.0, 9.0)));
        let out = dc.next((t + chrono::Duration::days(2), bar(11.0, 7.0)));
        assert_eq!(out.upper, 12.0);
        assert_eq!(out.lower, 7.0);
        assert_eq!(out.middle, 9.5);
    }

    /// Incremental extrema match the definitional rescan under mixed
    /// appends, same-bucket revisions, and window evictions (exact).
    #[test]
    fn equivalence_with_naive_under_appends_replaces_and_evictions() {
        let mut state: u64 = 0xd07c1a2b4e5f6071;
        let mut rng = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        let start = Utc.with_ymd_and_hms(2021, 6, 1, 0, 0, 0).unwrap();

        for _ in 0..100 {
            let window_days = 2 + rng() % 30;
            let mut dc = DonchianChannel::new(
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
                let base = 10.0 + (rng() % 10_000) as f64 / 100.0;
                let (h, l) = (
                    base + (rng() % 500) as f64 / 100.0,
                    base - (rng() % 500) as f64 / 100.0,
                );
                let got = dc.next((start + chrono::Duration::days(day), bar(h, l)));
                if roll < 3 && !bars.is_empty() {
                    *bars.last_mut().unwrap() = (day, h, l);
                } else {
                    bars.push((day, h, l));
                }
                bars.retain(|(d, _, _)| *d > day - window_days as i64);
                let upper = bars
                    .iter()
                    .map(|(_, h, _)| *h)
                    .fold(f64::NEG_INFINITY, f64::max);
                let lower = bars
                    .iter()
                    .map(|(_, _, l)| *l)
                    .fold(f64::INFINITY, f64::min);
                assert_eq!(got.upper, upper, "upper mismatch (day={day})");
                assert_eq!(got.lower, lower, "lower mismatch (day={day})");
                assert_eq!(got.middle, (upper + lower) / 2.0, "middle mismatch (day={day})");
            }
        }
    }
}
