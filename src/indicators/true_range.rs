use std::fmt;
use std::time::Duration;

use chrono::{DateTime, Utc};
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use super::fixed_time_bucket::{BucketUpdate, FixedTimeBucket};
use crate::{errors::Result, Close, High, Low, Next, NextBatch, Reset};

/// True range for timestamped OHLC bars.
///
/// Revisions inside `bucket_width` replace the current bar. The previous close
/// advances only when a new bucket begins, so revising a live bar cannot turn
/// its own close into the comparison close.
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Debug, Clone)]
pub struct TrueRange {
    buckets: FixedTimeBucket,
    previous_close: Option<f64>,
    current_close: Option<f64>,
}

impl TrueRange {
    pub fn new(bucket_width: Duration) -> Result<Self> {
        Ok(Self {
            buckets: FixedTimeBucket::new(bucket_width)?,
            previous_close: None,
            current_close: None,
        })
    }

    pub fn bucket_width(&self) -> Duration {
        self.buckets.width()
    }

    pub(crate) fn next_with_update<T>(
        &mut self,
        timestamp: DateTime<Utc>,
        input: &T,
    ) -> (f64, BucketUpdate)
    where
        T: High + Low + Close + ?Sized,
    {
        let update = self.buckets.update(timestamp);
        if update == BucketUpdate::Append {
            self.previous_close = self.current_close;
        }

        let high = input.high();
        let low = input.low();
        self.current_close = Some(input.close());

        let intrabar = high - low;
        let value = match self.previous_close {
            Some(previous_close) => intrabar
                .max((high - previous_close).abs())
                .max((low - previous_close).abs()),
            None => intrabar,
        };
        (value, update)
    }
}

impl<T> Next<T> for TrueRange
where
    T: High + Low + Close,
{
    type Output = f64;

    fn next(&mut self, (timestamp, input): (DateTime<Utc>, T)) -> Self::Output {
        self.next_with_update(timestamp, &input).0
    }
}

impl<T> NextBatch<T> for TrueRange where T: Copy + High + Low + Close {}

impl Reset for TrueRange {
    fn reset(&mut self) {
        self.buckets.reset();
        self.previous_close = None;
        self.current_close = None;
    }
}

impl Default for TrueRange {
    fn default() -> Self {
        Self::new(Duration::from_secs(60)).unwrap()
    }
}

impl fmt::Display for TrueRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "TrueRange({}s buckets)", self.bucket_width().as_secs())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DataItem;
    use chrono::{Duration as ChronoDuration, TimeZone};

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
    fn preserves_previous_bucket_close_during_revision() {
        let t = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        let mut tr = TrueRange::new(Duration::from_secs(60)).unwrap();
        assert_eq!(tr.next((t, bar(10.0, 8.0, 9.0))), 2.0);
        assert_eq!(
            tr.next((t + ChronoDuration::minutes(1), bar(15.0, 12.0, 14.0))),
            6.0
        );
        assert_eq!(
            tr.next((
                t + ChronoDuration::minutes(1) + ChronoDuration::seconds(30),
                bar(16.0, 11.0, 15.0),
            )),
            7.0
        );
    }
}
