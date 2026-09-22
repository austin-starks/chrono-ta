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

/// Ichimoku Cloud output. All five lines are evaluated on the *current*
/// bucket: no forward/backward displacement is applied here. Shifting
/// Senkou A/B ahead and Chikou behind is the caller's job (the
/// `Lag`/`ValueAgo` indicator does exactly that).
#[derive(Debug, Clone, PartialEq)]
pub struct IchimokuOutput {
    pub tenkan: f64,
    pub kijun: f64,
    pub senkou_a: f64,
    pub senkou_b: f64,
    pub chikou: f64,
}

/// Ichimoku Cloud over timestamped OHLC bars.
///
/// Tenkan = midpoint of the highest high / lowest low over the conversion
/// window; Kijun the same over the base window; Senkou A = (Tenkan +
/// Kijun) / 2; Senkou B = midpoint over the span window; Chikou = the
/// current close. Each midpoint runs over whatever bars exist so the cloud
/// emits from the first bar.
///
/// Revisions inside `bucket_width` revise the current bar.
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Debug, Clone)]
pub struct IchimokuCloud {
    conversion: Duration,
    base: Duration,
    span: Duration,
    buckets: FixedTimeBucket,
    bars: VecDeque<(DateTime<Utc>, f64, f64, f64)>,
    /// Per-line extrema over committed bars; the live bar folds in at query.
    /// Derived state, rebuilt from `bars` after deserialize via `ensure_built`.
    #[cfg_attr(feature = "serde", serde(skip))]
    ext_conversion: SlidingExtrema,
    #[cfg_attr(feature = "serde", serde(skip))]
    ext_base: SlidingExtrema,
    #[cfg_attr(feature = "serde", serde(skip))]
    ext_span: SlidingExtrema,
    #[cfg_attr(feature = "serde", serde(skip))]
    ext_built: bool,
}

impl IchimokuCloud {
    pub fn new(
        conversion: Duration,
        base: Duration,
        span: Duration,
        bucket_width: Duration,
    ) -> Result<Self> {
        if conversion.is_zero()
            || base.is_zero()
            || span.is_zero()
            || bucket_width.is_zero()
            || bucket_width > conversion
        {
            return Err(TaError::InvalidParameter);
        }
        Ok(Self {
            conversion,
            base,
            span,
            buckets: FixedTimeBucket::new(bucket_width)?,
            bars: VecDeque::new(),
            ext_conversion: SlidingExtrema::default(),
            ext_base: SlidingExtrema::default(),
            ext_span: SlidingExtrema::default(),
            ext_built: true,
        })
    }

    fn cutoff_nanos(window: Duration, now: DateTime<Utc>) -> i64 {
        let nanos = i64::try_from(window.as_nanos()).unwrap_or(i64::MAX);
        now.timestamp_nanos_opt()
            .unwrap_or(i64::MIN)
            .saturating_sub(nanos)
    }

    fn midpoint(ext: &SlidingExtrema, live_high: f64, live_low: f64) -> f64 {
        let (highest, lowest) = ext.extremes(live_high, live_low);
        (highest + lowest) / 2.0
    }

    fn remove_expired(&mut self, now: DateTime<Utc>) {
        let cutoff = Self::cutoff_nanos(self.span, now);
        while self
            .bars
            .front()
            .is_some_and(|(ts, _, _, _)| ts.timestamp_nanos_opt().unwrap_or(i64::MIN) <= cutoff)
        {
            self.bars.pop_front();
        }
        self.ext_conversion
            .expire_before(Self::cutoff_nanos(self.conversion, now));
        self.ext_base
            .expire_before(Self::cutoff_nanos(self.base, now));
        self.ext_span.expire_before(cutoff);
    }

    fn ensure_built(&mut self) {
        if self.ext_built {
            return;
        }
        self.ext_conversion.clear();
        self.ext_base.clear();
        self.ext_span.clear();
        let committed = self.bars.len().saturating_sub(1);
        for &(ts, h, l, _) in self.bars.iter().take(committed) {
            self.ext_conversion.commit(ts, h, l);
            self.ext_base.commit(ts, h, l);
            self.ext_span.commit(ts, h, l);
        }
        self.ext_built = true;
    }
}

impl<T> Next<T> for IchimokuCloud
where
    T: High + Low + Close,
{
    type Output = IchimokuOutput;

    fn next(&mut self, (timestamp, input): (DateTime<Utc>, T)) -> Self::Output {
        self.ensure_built();
        let update = self.buckets.update(timestamp);
        if update == BucketUpdate::Replace {
            self.bars.pop_back();
        } else if let Some(&(ts, h, l, _)) = self.bars.back() {
            self.ext_conversion.commit(ts, h, l);
            self.ext_base.commit(ts, h, l);
            self.ext_span.commit(ts, h, l);
        }
        let (high, low, close) = (input.high(), input.low(), input.close());
        self.bars.push_back((timestamp, high, low, close));
        self.remove_expired(timestamp);

        let tenkan = Self::midpoint(&self.ext_conversion, high, low);
        let kijun = Self::midpoint(&self.ext_base, high, low);
        let senkou_b = Self::midpoint(&self.ext_span, high, low);
        IchimokuOutput {
            tenkan,
            kijun,
            senkou_a: (tenkan + kijun) / 2.0,
            senkou_b,
            chikou: close,
        }
    }
}

impl<T> NextBatch<T> for IchimokuCloud where T: Copy + High + Low + Close {}

impl Reset for IchimokuCloud {
    fn reset(&mut self) {
        self.buckets.reset();
        self.bars.clear();
        self.ext_conversion.clear();
        self.ext_base.clear();
        self.ext_span.clear();
        self.ext_built = true;
    }
}

impl Default for IchimokuCloud {
    fn default() -> Self {
        Self::new(
            Duration::from_secs(9 * 86_400),
            Duration::from_secs(26 * 86_400),
            Duration::from_secs(52 * 86_400),
            Duration::from_secs(86_400),
        )
        .unwrap()
    }
}

impl fmt::Display for IchimokuCloud {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "IchimokuCloud(unshifted lines)")
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
    fn tenkan_reacts_faster_than_kijun() {
        let t = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        let mut ichi = IchimokuCloud::new(
            Duration::from_secs(3 * 86_400),
            Duration::from_secs(5 * 86_400),
            Duration::from_secs(5 * 86_400),
            Duration::from_secs(86_400),
        )
        .unwrap();
        let mut out = None;
        for i in 0..8 {
            // Five flat bars then three spike bars: tenkan (3d) sees only
            // the spike (20.0), kijun (5d) still drags the older low (9.0).
            let c = if i < 5 { 10.0 } else { 20.0 };
            out = Some(ichi.next((t + chrono::Duration::days(i), bar(c + 1.0, c - 1.0, c))));
        }
        let out = out.unwrap();
        assert_eq!(out.tenkan, 20.0);
        assert_eq!(out.kijun, 15.0);
        assert_eq!(out.senkou_a, (out.tenkan + out.kijun) / 2.0);
        assert_eq!(out.chikou, 20.0);
    }

    /// Incremental per-line extrema match the definitional rescan under
    /// mixed appends, same-bucket revisions, and evictions (exact).
    #[test]
    fn equivalence_with_naive_under_appends_replaces_and_evictions() {
        let mut state: u64 = 0x1c4d79a3e6b2f580;
        let mut rng = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        let start = Utc.with_ymd_and_hms(2021, 6, 1, 0, 0, 0).unwrap();

        for _ in 0..60 {
            let conv = 2 + rng() % 8;
            let base = conv + 1 + rng() % 20;
            let span = base + 1 + rng() % 30;
            let mut ichi = IchimokuCloud::new(
                Duration::from_secs(conv * 86_400),
                Duration::from_secs(base * 86_400),
                Duration::from_secs(span * 86_400),
                Duration::from_secs(86_400),
            )
            .unwrap();
            let mut bars: Vec<(i64, f64, f64, f64)> = Vec::new();
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
                let base_price = 10.0 + (rng() % 10_000) as f64 / 100.0;
                let (h, l, c) = (
                    base_price + (rng() % 500) as f64 / 100.0,
                    base_price - (rng() % 500) as f64 / 100.0,
                    base_price,
                );
                let got = ichi.next((start + chrono::Duration::days(day), bar(h, l, c)));
                if roll < 3 && !bars.is_empty() {
                    *bars.last_mut().unwrap() = (day, h, l, c);
                } else {
                    bars.push((day, h, l, c));
                }
                bars.retain(|(d, _, _, _)| *d > day - span as i64);
                let midpoint = |window: u64| {
                    let (mut hi, mut lo) = (f64::NEG_INFINITY, f64::INFINITY);
                    for (d, h, l, _) in bars.iter() {
                        if *d <= day - window as i64 {
                            continue;
                        }
                        hi = hi.max(*h);
                        lo = lo.min(*l);
                    }
                    (hi + lo) / 2.0
                };
                let tenkan = midpoint(conv);
                let kijun = midpoint(base);
                let senkou_b = midpoint(span);
                assert_eq!(got.tenkan, tenkan, "tenkan mismatch (day={day})");
                assert_eq!(got.kijun, kijun, "kijun mismatch (day={day})");
                assert_eq!(
                    got.senkou_a,
                    (tenkan + kijun) / 2.0,
                    "senkou_a mismatch (day={day})"
                );
                assert_eq!(got.senkou_b, senkou_b, "senkou_b mismatch (day={day})");
                assert_eq!(got.chikou, c, "chikou mismatch (day={day})");
            }
        }
    }
}
