use std::collections::VecDeque;
use std::fmt;
use std::time::Duration;

use chrono::{DateTime, Utc};
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use super::fixed_time_bucket::{BucketUpdate, FixedTimeBucket};
use crate::{errors::Result, Close, High, Low, Next, NextBatch, Reset, Volume};

#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Debug, Clone, Copy)]
struct VwapPoint {
    timestamp: DateTime<Utc>,
    price_volume: f64,
    volume: f64,
}

fn point<T>(timestamp: DateTime<Utc>, input: &T) -> VwapPoint
where
    T: High + Low + Close + Volume + ?Sized,
{
    let volume = input.volume();
    if volume <= 0.0 || !volume.is_finite() {
        return VwapPoint {
            timestamp,
            price_volume: 0.0,
            volume: 0.0,
        };
    }
    let typical_price = (input.high() + input.low() + input.close()) / 3.0;
    VwapPoint {
        timestamp,
        price_volume: typical_price * volume,
        volume,
    }
}

fn value(price_volume: f64, volume: f64) -> Option<f64> {
    (volume > 0.0).then_some(price_volume / volume)
}

/// Volume-weighted average typical price over an elapsed-time window.
#[doc(alias = "VWAP")]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Debug, Clone)]
pub struct RollingVwap {
    duration: Duration,
    buckets: FixedTimeBucket,
    window: VecDeque<VwapPoint>,
    price_volume: f64,
    volume: f64,
}

impl RollingVwap {
    pub fn new(duration: Duration, bucket_width: Duration) -> Result<Self> {
        if duration.is_zero() || bucket_width > duration {
            return Err(crate::errors::TaError::InvalidParameter);
        }
        Ok(Self {
            duration,
            buckets: FixedTimeBucket::new(bucket_width)?,
            window: VecDeque::new(),
            price_volume: 0.0,
            volume: 0.0,
        })
    }

    pub fn duration(&self) -> Duration {
        self.duration
    }

    pub fn bucket_width(&self) -> Duration {
        self.buckets.width()
    }

    fn remove_expired(&mut self, current_time: DateTime<Utc>) {
        let duration_nanos = i64::try_from(self.duration.as_nanos()).unwrap_or(i64::MAX);
        let cutoff = current_time
            .timestamp_nanos_opt()
            .unwrap_or(i64::MIN)
            .saturating_sub(duration_nanos);
        while self.window.front().is_some_and(|point| {
            point.timestamp.timestamp_nanos_opt().unwrap_or(i64::MIN) <= cutoff
        }) {
            if let Some(expired) = self.window.pop_front() {
                self.price_volume -= expired.price_volume;
                self.volume -= expired.volume;
            }
        }
    }
}

impl<T> Next<T> for RollingVwap
where
    T: High + Low + Close + Volume,
{
    type Output = Option<f64>;

    fn next(&mut self, (timestamp, input): (DateTime<Utc>, T)) -> Self::Output {
        let update = self.buckets.update(timestamp);
        self.remove_expired(timestamp);

        if update == BucketUpdate::Replace {
            if let Some(replaced) = self.window.pop_back() {
                self.price_volume -= replaced.price_volume;
                self.volume -= replaced.volume;
            }
        }

        let current = point(timestamp, &input);
        self.price_volume += current.price_volume;
        self.volume += current.volume;
        self.window.push_back(current);
        value(self.price_volume, self.volume)
    }
}

impl<T> NextBatch<T> for RollingVwap where T: Copy + High + Low + Close + Volume {}

impl Reset for RollingVwap {
    fn reset(&mut self) {
        self.buckets.reset();
        self.window.clear();
        self.price_volume = 0.0;
        self.volume = 0.0;
    }
}

impl Default for RollingVwap {
    fn default() -> Self {
        Self::new(Duration::from_secs(86_400), Duration::from_secs(60)).unwrap()
    }
}

impl fmt::Display for RollingVwap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "RollingVwap({}s window, {}s buckets)",
            self.duration.as_secs(),
            self.bucket_width().as_secs()
        )
    }
}

/// Volume-weighted average typical price accumulated until [`Reset::reset`].
#[doc(alias = "VWAP")]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Debug, Clone)]
pub struct AnchoredVwap {
    buckets: FixedTimeBucket,
    current: Option<VwapPoint>,
    price_volume: f64,
    volume: f64,
}

impl AnchoredVwap {
    pub fn new(bucket_width: Duration) -> Result<Self> {
        Ok(Self {
            buckets: FixedTimeBucket::new(bucket_width)?,
            current: None,
            price_volume: 0.0,
            volume: 0.0,
        })
    }

    pub fn bucket_width(&self) -> Duration {
        self.buckets.width()
    }
}

impl<T> Next<T> for AnchoredVwap
where
    T: High + Low + Close + Volume,
{
    type Output = Option<f64>;

    fn next(&mut self, (timestamp, input): (DateTime<Utc>, T)) -> Self::Output {
        let update = self.buckets.update(timestamp);
        if update == BucketUpdate::Replace {
            if let Some(replaced) = self.current {
                self.price_volume -= replaced.price_volume;
                self.volume -= replaced.volume;
            }
        }

        let current = point(timestamp, &input);
        self.price_volume += current.price_volume;
        self.volume += current.volume;
        self.current = Some(current);
        value(self.price_volume, self.volume)
    }
}

impl<T> NextBatch<T> for AnchoredVwap where T: Copy + High + Low + Close + Volume {}

impl Reset for AnchoredVwap {
    fn reset(&mut self) {
        self.buckets.reset();
        self.current = None;
        self.price_volume = 0.0;
        self.volume = 0.0;
    }
}

impl Default for AnchoredVwap {
    fn default() -> Self {
        Self::new(Duration::from_secs(60)).unwrap()
    }
}

impl fmt::Display for AnchoredVwap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "AnchoredVwap({}s buckets)",
            self.bucket_width().as_secs()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DataItem;
    use chrono::{Duration as ChronoDuration, TimeZone};

    fn bar(price: f64, volume: f64) -> DataItem {
        DataItem::builder()
            .open(price)
            .high(price)
            .low(price)
            .close(price)
            .volume(volume)
            .build()
            .unwrap()
    }

    #[test]
    fn anchored_vwap_replaces_current_bucket() {
        let t = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        let mut vwap = AnchoredVwap::new(Duration::from_secs(60)).unwrap();
        assert_eq!(vwap.next((t, bar(10.0, 2.0))), Some(10.0));
        assert_eq!(
            vwap.next((t + ChronoDuration::seconds(30), bar(20.0, 2.0))),
            Some(20.0)
        );
        assert_eq!(
            vwap.next((t + ChronoDuration::minutes(1), bar(10.0, 2.0))),
            Some(15.0)
        );
    }
}
