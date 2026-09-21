use chrono::{Duration as ChronoDuration, TimeZone, Utc};
use chrono_ta::indicators::RelativeStrengthIndex;
use chrono_ta::NextBatch;
use std::time::Duration;

fn main() {
    let start = Utc.with_ymd_and_hms(2026, 9, 20, 0, 0, 0).unwrap();
    let inputs = vec![
        (start, 100.0),
        (start + ChronoDuration::days(1), 102.0),
        (start + ChronoDuration::days(2), 101.0),
    ];

    let mut rsi = RelativeStrengthIndex::new(Duration::from_secs(14 * 86_400)).unwrap();
    let values = rsi.next_batch(&inputs);
    println!("{values:?}");
}
