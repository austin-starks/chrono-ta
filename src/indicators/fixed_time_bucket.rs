use chrono::{DateTime, Utc};
use std::time::Duration;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BucketUpdate {
    First,
    Replace,
    Append,
}

/// Tracks fixed-width UTC buckets for indicators whose window does not imply a
/// sampling cadence, such as true range and crossovers.
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Debug, Clone)]
pub(crate) struct FixedTimeBucket {
    width: Duration,
    current: Option<i64>,
}

impl FixedTimeBucket {
    pub(crate) fn new(width: Duration) -> crate::errors::Result<Self> {
        if width.is_zero() {
            return Err(crate::errors::TaError::InvalidParameter);
        }
        Ok(Self {
            width,
            current: None,
        })
    }

    pub(crate) fn update(&mut self, timestamp: DateTime<Utc>) -> BucketUpdate {
        let width_nanos = i64::try_from(self.width.as_nanos()).unwrap_or(i64::MAX);
        let timestamp_nanos = timestamp.timestamp_nanos_opt().unwrap_or(i64::MIN);
        let bucket = timestamp_nanos.div_euclid(width_nanos);

        let update = match self.current {
            None => BucketUpdate::First,
            Some(current) if current == bucket => BucketUpdate::Replace,
            Some(current) => {
                debug_assert!(
                    bucket >= current,
                    "indicator timestamps must be nondecreasing"
                );
                BucketUpdate::Append
            }
        };
        self.current = Some(bucket);
        update
    }

    pub(crate) fn reset(&mut self) {
        self.current = None;
    }

    pub(crate) fn width(&self) -> Duration {
        self.width
    }
}
