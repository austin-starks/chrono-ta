use std::time::Duration;

use chrono::{Duration as ChronoDuration, TimeZone, Utc};
use chrono_ta::indicators::{AverageTrueRange, RollingVwap};
use chrono_ta::{DataItem, Next};

fn main() -> Result<(), chrono_ta::errors::TaError> {
    let start = Utc.with_ymd_and_hms(2026, 9, 20, 14, 30, 0).unwrap();
    let first = DataItem::builder()
        .open(100.0)
        .high(104.0)
        .low(99.0)
        .close(102.0)
        .volume(1_000.0)
        .build()?;
    let second = DataItem::builder()
        .open(102.0)
        .high(106.0)
        .low(101.0)
        .close(105.0)
        .volume(1_500.0)
        .build()?;

    let bucket = Duration::from_secs(60);
    let window = Duration::from_secs(15 * 60);
    let mut atr = AverageTrueRange::new(window, bucket)?;
    let mut vwap = RollingVwap::new(window, bucket)?;

    println!("ATR: {}", atr.next((start, first)));
    println!(
        "ATR: {}",
        atr.next((start + ChronoDuration::minutes(1), second))
    );
    println!("VWAP: {:?}", vwap.next((start, first)));
    println!(
        "VWAP: {:?}",
        vwap.next((start + ChronoDuration::minutes(1), second))
    );

    Ok(())
}
