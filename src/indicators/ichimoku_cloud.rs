use std::collections::VecDeque;
use std::fmt;
use std::time::Duration;

use chrono::{DateTime, Utc};
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use super::fixed_time_bucket::{BucketUpdate, FixedTimeBucket};
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
        })
    }

    fn midpoint(&self, window: Duration, now: DateTime<Utc>) -> f64 {
        let nanos = i64::try_from(window.as_nanos()).unwrap_or(i64::MAX);
        let cutoff = now
            .timestamp_nanos_opt()
            .unwrap_or(i64::MIN)
            .saturating_sub(nanos);
        let mut highest = f64::NEG_INFINITY;
        let mut lowest = f64::INFINITY;
        for (ts, h, l, _) in self.bars.iter() {
            if ts.timestamp_nanos_opt().unwrap_or(i64::MIN) <= cutoff {
                continue;
            }
            highest = highest.max(*h);
            lowest = lowest.min(*l);
        }
        if highest.is_finite() && lowest.is_finite() {
            (highest + lowest) / 2.0
        } else {
            0.0
        }
    }

    fn remove_expired(&mut self, now: DateTime<Utc>) {
        let nanos = i64::try_from(self.span.as_nanos()).unwrap_or(i64::MAX);
        let cutoff = now
            .timestamp_nanos_opt()
            .unwrap_or(i64::MIN)
            .saturating_sub(nanos);
        while self
            .bars
            .front()
            .is_some_and(|(ts, _, _, _)| ts.timestamp_nanos_opt().unwrap_or(i64::MIN) <= cutoff)
        {
            self.bars.pop_front();
        }
    }
}

impl<T> Next<T> for IchimokuCloud
where
    T: High + Low + Close,
{
    type Output = IchimokuOutput;

    fn next(&mut self, (timestamp, input): (DateTime<Utc>, T)) -> Self::Output {
        let update = self.buckets.update(timestamp);
        self.remove_expired(timestamp);
        if update == BucketUpdate::Replace {
            self.bars.pop_back();
        }
        let (high, low, close) = (input.high(), input.low(), input.close());
        self.bars.push_back((timestamp, high, low, close));

        let tenkan = self.midpoint(self.conversion, timestamp);
        let kijun = self.midpoint(self.base, timestamp);
        let senkou_b = self.midpoint(self.span, timestamp);
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
}
