use std::time::Duration;

use chrono::{DateTime, Duration as ChronoDuration, TimeZone, Utc};
use chrono_ta::errors::TaError;
use chrono_ta::indicators::{CrossAbove, CrossBelow, ExponentialMovingAverage};
use chrono_ta::Next;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Decision {
    Buy,
    Sell,
    Hold,
}

struct EmaCrossStrategy {
    fast: ExponentialMovingAverage,
    slow: ExponentialMovingAverage,
    cross_above: CrossAbove,
    cross_below: CrossBelow,
}

impl EmaCrossStrategy {
    fn new() -> Result<Self, TaError> {
        Ok(Self {
            fast: ExponentialMovingAverage::new(Duration::from_secs(2 * 60))?,
            slow: ExponentialMovingAverage::new(Duration::from_secs(5 * 60))?,
            cross_above: CrossAbove::new(Duration::from_secs(60))?,
            cross_below: CrossBelow::new(Duration::from_secs(60))?,
        })
    }

    fn on_close(&mut self, timestamp: DateTime<Utc>, close: f64) -> Decision {
        let fast = self.fast.next((timestamp, close));
        let slow = self.slow.next((timestamp, close));
        let pair = (fast, slow);

        if self.cross_above.next((timestamp, pair)) {
            Decision::Buy
        } else if self.cross_below.next((timestamp, pair)) {
            Decision::Sell
        } else {
            Decision::Hold
        }
    }
}

fn main() -> Result<(), TaError> {
    let start = Utc.with_ymd_and_hms(2026, 9, 1, 13, 30, 0).unwrap();
    let closes = [100.0, 99.0, 98.0, 101.0, 105.0, 103.0, 97.0];
    let mut strategy = EmaCrossStrategy::new()?;

    for (minute, close) in closes.into_iter().enumerate() {
        let timestamp = start + ChronoDuration::minutes(minute as i64);
        let decision = strategy.on_close(timestamp, close);
        println!("{timestamp} close={close:.2} decision={decision:?}");
    }

    Ok(())
}
