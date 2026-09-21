use chrono::{Duration as ChronoDuration, TimeZone, Utc};
use chrono_ta::indicators::SimpleMovingAverage;
use chrono_ta::Next;
use std::time::Duration;

fn main() {
    let start = Utc.with_ymd_and_hms(2026, 9, 20, 14, 30, 0).unwrap();
    let mut average = SimpleMovingAverage::new(Duration::from_secs(3 * 60)).unwrap();

    for (offset_minutes, price) in [(0, 100.0), (1, 101.0), (2, 102.0), (4, 106.0)] {
        let timestamp = start + ChronoDuration::minutes(offset_minutes);
        println!("{timestamp}: {}", average.next((timestamp, price)));
    }
}
