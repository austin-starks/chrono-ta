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
        })
    }

    fn remove_expired(&mut self, now: DateTime<Utc>) {
        let nanos = i64::try_from(self.window.as_nanos()).unwrap_or(i64::MAX);
        let cutoff = now
            .timestamp_nanos_opt()
            .unwrap_or(i64::MIN)
            .saturating_sub(nanos);
        while self
            .bars
            .front()
            .is_some_and(|(ts, _, _)| ts.timestamp_nanos_opt().unwrap_or(i64::MIN) <= cutoff)
        {
            self.bars.pop_front();
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
        self.remove_expired(timestamp);
        if update == BucketUpdate::Replace {
            self.bars.pop_back();
        }
        let typical = (input.high() + input.low() + input.close()) / 3.0;
        self.bars.push_back((timestamp, typical, input.volume()));

        let mut positive = 0.0;
        let mut negative = 0.0;
        for pair in self.bars.iter().zip(self.bars.iter().skip(1)) {
            let ((_, prev_tp, _), (_, tp, vol)) = (pair.0, pair.1);
            if tp > prev_tp {
                positive += tp * vol;
            } else if tp < prev_tp {
                negative += tp * vol;
            }
        }
        if negative == 0.0 {
            100.0
        } else {
            100.0 - 100.0 / (1.0 + positive / negative)
        }
    }
}

impl<T> NextBatch<T> for MoneyFlowIndex where T: Copy + High + Low + Close + Volume {}

impl Reset for MoneyFlowIndex {
    fn reset(&mut self) {
        self.buckets.reset();
        self.bars.clear();
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
}
