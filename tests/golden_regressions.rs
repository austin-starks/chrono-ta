use std::time::Duration;

use chrono::{DateTime, Duration as ChronoDuration, TimeZone, Utc};
use chrono_ta::indicators::{
    AnchoredVwap, AverageTrueRange, CrossAbove, CrossBelow, Lag, RollingSum, RollingVwap, TrueRange,
};
use chrono_ta::{DataItem, Next, NextBatch, Reset};

fn start() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 1, 5, 14, 30, 0).unwrap()
}

fn bar(high: f64, low: f64, close: f64, volume: f64) -> DataItem {
    DataItem::builder()
        .open(close)
        .high(high)
        .low(low)
        .close(close)
        .volume(volume)
        .build()
        .unwrap()
}

fn flat_bar(price: f64, volume: f64) -> DataItem {
    bar(price, price, price, volume)
}

fn assert_close(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() < 1e-12,
        "expected {expected}, got {actual}"
    );
}

#[test]
fn golden_scalar_vectors() {
    let t = start();

    let mut sum = RollingSum::new(Duration::from_secs(3)).unwrap();
    let sum_outputs = [
        sum.next((t, 2.0)),
        sum.next((t + ChronoDuration::seconds(1), 2.0)),
        sum.next((t + ChronoDuration::seconds(2), 3.0)),
        sum.next((t + ChronoDuration::seconds(3), 4.0)),
    ];
    assert_eq!(sum_outputs, [2.0, 4.0, 7.0, 9.0]);

    let mut lag = Lag::new(Duration::from_secs(2)).unwrap();
    let lag_outputs = [
        lag.next((t, 10.0)),
        lag.next((t + ChronoDuration::seconds(1), 20.0)),
        lag.next((t + ChronoDuration::seconds(2), 30.0)),
        lag.next((t + ChronoDuration::seconds(4), 40.0)),
    ];
    assert_eq!(lag_outputs, [None, None, Some(10.0), Some(30.0)]);

    let mut above = CrossAbove::new(Duration::from_secs(60)).unwrap();
    let mut below = CrossBelow::new(Duration::from_secs(60)).unwrap();
    let pairs = [
        (t, (1.0, 2.0)),
        (t + ChronoDuration::minutes(1), (3.0, 2.0)),
        (t + ChronoDuration::minutes(2), (1.0, 2.0)),
    ];
    assert_eq!(above.next_batch(&pairs), [false, true, false]);
    assert_eq!(below.next_batch(&pairs), [false, false, true]);

    let bars = [
        (t, bar(10.0, 8.0, 9.0, 100.0)),
        (t + ChronoDuration::minutes(1), bar(15.0, 12.0, 14.0, 100.0)),
        (t + ChronoDuration::minutes(2), bar(13.0, 10.0, 11.0, 100.0)),
        (t + ChronoDuration::minutes(3), bar(12.0, 10.0, 10.0, 100.0)),
    ];
    let mut tr = TrueRange::new(Duration::from_secs(60)).unwrap();
    assert_eq!(tr.next_batch(&bars), [2.0, 6.0, 4.0, 2.0]);

    let mut atr =
        AverageTrueRange::new(Duration::from_secs(3 * 60), Duration::from_secs(60)).unwrap();
    assert_eq!(atr.next_batch(&bars), [2.0, 4.0, 4.0, 4.0]);

    let vwap_bars = [
        (t, flat_bar(10.0, 2.0)),
        (t + ChronoDuration::minutes(1), flat_bar(20.0, 1.0)),
        (t + ChronoDuration::minutes(2), flat_bar(30.0, 1.0)),
    ];
    let mut rolling = RollingVwap::new(Duration::from_secs(120), Duration::from_secs(60)).unwrap();
    let rolling_outputs = rolling.next_batch(&vwap_bars);
    assert_close(rolling_outputs[0].unwrap(), 10.0);
    assert_close(rolling_outputs[1].unwrap(), 40.0 / 3.0);
    assert_close(rolling_outputs[2].unwrap(), 25.0);

    let mut anchored = AnchoredVwap::new(Duration::from_secs(60)).unwrap();
    let anchored_outputs = anchored.next_batch(&vwap_bars);
    assert_close(anchored_outputs[2].unwrap(), 17.5);
    anchored.reset();
    assert_eq!(anchored.next((t, flat_bar(5.0, 1.0))), Some(5.0));
}

#[test]
fn batch_matches_scalar_for_every_new_indicator() {
    let t = start();
    let numbers = [
        (t, 1.0),
        (t + ChronoDuration::seconds(1), 3.0),
        (t + ChronoDuration::seconds(2), 2.0),
    ];
    let pairs = [
        (t, (1.0, 2.0)),
        (t + ChronoDuration::minutes(1), (3.0, 2.0)),
        (t + ChronoDuration::minutes(2), (1.0, 2.0)),
    ];
    let bars = [
        (t, bar(10.0, 8.0, 9.0, 2.0)),
        (t + ChronoDuration::minutes(1), bar(15.0, 12.0, 14.0, 1.0)),
        (t + ChronoDuration::minutes(2), bar(13.0, 10.0, 11.0, 1.0)),
    ];

    macro_rules! parity {
        ($scalar:expr, $batch:expr, $inputs:expr) => {{
            let mut scalar = $scalar;
            let expected: Vec<_> = $inputs
                .iter()
                .copied()
                .map(|input| scalar.next(input))
                .collect();
            let mut batch = $batch;
            assert_eq!(batch.next_batch(&$inputs), expected);
        }};
    }

    parity!(
        RollingSum::new(Duration::from_secs(3)).unwrap(),
        RollingSum::new(Duration::from_secs(3)).unwrap(),
        numbers
    );
    parity!(
        Lag::new(Duration::from_secs(2)).unwrap(),
        Lag::new(Duration::from_secs(2)).unwrap(),
        numbers
    );
    parity!(
        CrossAbove::new(Duration::from_secs(60)).unwrap(),
        CrossAbove::new(Duration::from_secs(60)).unwrap(),
        pairs
    );
    parity!(
        CrossBelow::new(Duration::from_secs(60)).unwrap(),
        CrossBelow::new(Duration::from_secs(60)).unwrap(),
        pairs
    );
    parity!(
        TrueRange::new(Duration::from_secs(60)).unwrap(),
        TrueRange::new(Duration::from_secs(60)).unwrap(),
        bars
    );
    parity!(
        AverageTrueRange::new(Duration::from_secs(180), Duration::from_secs(60)).unwrap(),
        AverageTrueRange::new(Duration::from_secs(180), Duration::from_secs(60)).unwrap(),
        bars
    );
    parity!(
        RollingVwap::new(Duration::from_secs(180), Duration::from_secs(60)).unwrap(),
        RollingVwap::new(Duration::from_secs(180), Duration::from_secs(60)).unwrap(),
        bars
    );
    parity!(
        AnchoredVwap::new(Duration::from_secs(60)).unwrap(),
        AnchoredVwap::new(Duration::from_secs(60)).unwrap(),
        bars
    );
}

#[cfg(feature = "serde")]
#[test]
fn serde_round_trip_continues_every_new_indicator() {
    fn round_trip<T>(value: &T) -> T
    where
        T: serde::Serialize + serde::de::DeserializeOwned,
    {
        bincode::deserialize(&bincode::serialize(value).unwrap()).unwrap()
    }

    let t = start();
    let bar0 = bar(10.0, 8.0, 9.0, 2.0);
    let bar1 = bar(15.0, 12.0, 14.0, 1.0);

    let mut sum = RollingSum::new(Duration::from_secs(10)).unwrap();
    sum.next((t, 1.0));
    let mut sum_restored = round_trip(&sum);
    assert_eq!(
        sum.next((t + ChronoDuration::seconds(1), 2.0)),
        sum_restored.next((t + ChronoDuration::seconds(1), 2.0))
    );

    let mut lag = Lag::new(Duration::from_secs(1)).unwrap();
    lag.next((t, 1.0));
    let mut lag_restored = round_trip(&lag);
    assert_eq!(
        lag.next((t + ChronoDuration::seconds(1), 2.0)),
        lag_restored.next((t + ChronoDuration::seconds(1), 2.0))
    );

    let mut above = CrossAbove::new(Duration::from_secs(60)).unwrap();
    above.next((t, (1.0, 2.0)));
    let mut above_restored = round_trip(&above);
    assert_eq!(
        above.next((t + ChronoDuration::minutes(1), (3.0, 2.0))),
        above_restored.next((t + ChronoDuration::minutes(1), (3.0, 2.0)))
    );

    let mut below = CrossBelow::new(Duration::from_secs(60)).unwrap();
    below.next((t, (2.0, 1.0)));
    let mut below_restored = round_trip(&below);
    assert_eq!(
        below.next((t + ChronoDuration::minutes(1), (1.0, 2.0))),
        below_restored.next((t + ChronoDuration::minutes(1), (1.0, 2.0)))
    );

    let mut tr = TrueRange::new(Duration::from_secs(60)).unwrap();
    tr.next((t, bar0));
    let mut tr_restored = round_trip(&tr);
    assert_eq!(
        tr.next((t + ChronoDuration::minutes(1), bar1)),
        tr_restored.next((t + ChronoDuration::minutes(1), bar1))
    );

    let mut atr = AverageTrueRange::new(Duration::from_secs(180), Duration::from_secs(60)).unwrap();
    atr.next((t, bar0));
    let mut atr_restored = round_trip(&atr);
    assert_eq!(
        atr.next((t + ChronoDuration::minutes(1), bar1)),
        atr_restored.next((t + ChronoDuration::minutes(1), bar1))
    );

    let mut rolling = RollingVwap::new(Duration::from_secs(180), Duration::from_secs(60)).unwrap();
    rolling.next((t, bar0));
    let mut rolling_restored = round_trip(&rolling);
    assert_eq!(
        rolling.next((t + ChronoDuration::minutes(1), bar1)),
        rolling_restored.next((t + ChronoDuration::minutes(1), bar1))
    );

    let mut anchored = AnchoredVwap::new(Duration::from_secs(60)).unwrap();
    anchored.next((t, bar0));
    let mut anchored_restored = round_trip(&anchored);
    assert_eq!(
        anchored.next((t + ChronoDuration::minutes(1), bar1)),
        anchored_restored.next((t + ChronoDuration::minutes(1), bar1))
    );
}
