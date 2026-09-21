use crate::errors::*;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DataItem {
    open: f64,
    high: f64,
    low: f64,
    close: f64,
    volume: f64,
}

impl DataItem {
    pub fn builder() -> DataItemBuilder {
        DataItemBuilder::new()
    }

    pub fn open(&self) -> f64 {
        self.open
    }

    pub fn high(&self) -> f64 {
        self.high
    }

    pub fn low(&self) -> f64 {
        self.low
    }

    pub fn close(&self) -> f64 {
        self.close
    }

    pub fn volume(&self) -> f64 {
        self.volume
    }
}

/// Supplies an opening price to an indicator.
pub trait Open {
    fn open(&self) -> f64;
}

/// Supplies a high price to an indicator.
pub trait High {
    fn high(&self) -> f64;
}

/// Supplies a low price to an indicator.
pub trait Low {
    fn low(&self) -> f64;
}

/// Supplies a closing price to an indicator.
pub trait Close {
    fn close(&self) -> f64;
}

/// Supplies volume to an indicator.
pub trait Volume {
    fn volume(&self) -> f64;
}

impl Open for DataItem {
    fn open(&self) -> f64 {
        self.open
    }
}

impl High for DataItem {
    fn high(&self) -> f64 {
        self.high
    }
}

impl Low for DataItem {
    fn low(&self) -> f64 {
        self.low
    }
}

impl Close for DataItem {
    fn close(&self) -> f64 {
        self.close
    }
}

impl Volume for DataItem {
    fn volume(&self) -> f64 {
        self.volume
    }
}

impl<T: Open + ?Sized> Open for &T {
    fn open(&self) -> f64 {
        (*self).open()
    }
}

impl<T: High + ?Sized> High for &T {
    fn high(&self) -> f64 {
        (*self).high()
    }
}

impl<T: Low + ?Sized> Low for &T {
    fn low(&self) -> f64 {
        (*self).low()
    }
}

impl<T: Close + ?Sized> Close for &T {
    fn close(&self) -> f64 {
        (*self).close()
    }
}

impl<T: Volume + ?Sized> Volume for &T {
    fn volume(&self) -> f64 {
        (*self).volume()
    }
}

pub struct DataItemBuilder {
    open: Option<f64>,
    high: Option<f64>,
    low: Option<f64>,
    close: Option<f64>,
    volume: Option<f64>,
}

impl DataItemBuilder {
    pub fn new() -> Self {
        Self {
            open: None,
            high: None,
            low: None,
            close: None,
            volume: None,
        }
    }

    pub fn open(mut self, val: f64) -> Self {
        self.open = Some(val);
        self
    }

    pub fn high(mut self, val: f64) -> Self {
        self.high = Some(val);
        self
    }

    pub fn low(mut self, val: f64) -> Self {
        self.low = Some(val);
        self
    }

    pub fn close(mut self, val: f64) -> Self {
        self.close = Some(val);
        self
    }

    pub fn volume(mut self, val: f64) -> Self {
        self.volume = Some(val);
        self
    }

    pub fn build(self) -> Result<DataItem> {
        if let (Some(open), Some(high), Some(low), Some(close), Some(volume)) =
            (self.open, self.high, self.low, self.close, self.volume)
        {
            // validate
            if low <= open
                && low <= close
                && low <= high
                && high >= open
                && high >= close
                && volume >= 0.0
            {
                let item = DataItem {
                    open,
                    high,
                    low,
                    close,
                    volume,
                };
                Ok(item)
            } else {
                Err(TaError::DataItemInvalid)
            }
        } else {
            Err(TaError::DataItemIncomplete)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builder() {
        fn assert_valid((open, high, low, close, volume): (f64, f64, f64, f64, f64)) {
            let result = DataItem::builder()
                .open(open)
                .high(high)
                .low(low)
                .close(close)
                .volume(volume)
                .build();
            assert!(result.is_ok());
        }

        fn assert_invalid(record: (f64, f64, f64, f64, f64)) {
            let (open, high, low, close, volume) = record;
            let result = DataItem::builder()
                .open(open)
                .high(high)
                .low(low)
                .close(close)
                .volume(volume)
                .build();
            assert_eq!(result, Err(TaError::DataItemInvalid));
        }

        let valid_records = vec![
            // open, high, low , close, volume
            (20.0, 25.0, 15.0, 21.0, 7500.0),
            (10.0, 10.0, 10.0, 10.0, 10.0),
            (0.0, 0.0, 0.0, 0.0, 0.0),
        ];
        for record in valid_records {
            assert_valid(record)
        }

        let invalid_records = vec![
            // open, high, low , close, volume
            (-1.0, 25.0, 15.0, 21.0, 7500.0),
            (20.0, -1.0, 15.0, 21.0, 7500.0),
            (20.0, 25.0, 15.0, -1.0, 7500.0),
            (20.0, 25.0, 15.0, 21.0, -1.0),
            (14.9, 25.0, 15.0, 21.0, 7500.0),
            (25.1, 25.0, 15.0, 21.0, 7500.0),
            (20.0, 25.0, 15.0, 14.9, 7500.0),
            (20.0, 25.0, 15.0, 25.1, 7500.0),
            (20.0, 15.0, 25.0, 21.0, 7500.0),
        ];
        for record in invalid_records {
            assert_invalid(record)
        }
    }
}
