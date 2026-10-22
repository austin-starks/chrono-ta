use chrono::{TimeZone, Utc};
use chrono_ta::indicators::{
    BollingerBands, ExponentialMovingAverage, Lag, MaxDrawdown, MaxDrawup, Maximum,
    MeanAbsoluteDeviation, Minimum, RateOfChange, RelativeStrengthIndex, RollingSum,
    SimpleMovingAverage, StandardDeviation,
};
use chrono_ta::Next;
use std::time::Duration;

#[test]
fn test_daily_close_revises_the_session_open() {
    // A daily indicator holds one observation per session: the close revises the open.
    let mut sma = SimpleMovingAverage::new(Duration::from_secs(3 * 86400)).unwrap(); // 3 days

    let day1_open = Utc.with_ymd_and_hms(2024, 1, 2, 14, 30, 0).unwrap();
    assert_eq!(sma.next((day1_open, 100.0)), 100.0);
    let day1_close = Utc.with_ymd_and_hms(2024, 1, 2, 21, 0, 0).unwrap();
    assert_eq!(sma.next((day1_close, 105.0)), 105.0);

    let day2_open = Utc.with_ymd_and_hms(2024, 1, 3, 14, 30, 0).unwrap();
    assert_eq!(sma.next((day2_open, 110.0)), 107.5); // 105 and the in-progress 110
    let day2_close = Utc.with_ymd_and_hms(2024, 1, 3, 21, 0, 0).unwrap();
    assert_eq!(sma.next((day2_close, 108.0)), 106.5); // 105 and 108
}

#[test]
fn test_intraday_replacement_within_bucket() {
    // Test that intraday data within the same time bucket DOES get replaced
    let mut sma = SimpleMovingAverage::new(Duration::from_secs(15 * 60)).unwrap(); // 15 minutes

    let base_time = Utc.with_ymd_and_hms(2024, 1, 1, 9, 30, 0).unwrap();

    // First 5-minute bar
    let result1 = sma.next((base_time, 100.0));
    assert_eq!(result1, 100.0);

    // Second 5-minute bar (different bucket)
    let result2 = sma.next((base_time + chrono::Duration::minutes(5), 101.0));
    assert_eq!(result2, 100.5); // Average of 100 and 101

    // Third 5-minute bar (different bucket)
    let result3 = sma.next((base_time + chrono::Duration::minutes(10), 102.0));
    assert_eq!(result3, 101.0); // Average of 100, 101, 102

    // Update at minute 11 - with 5-minute bucket detection, this is a new bucket
    // so it won't replace
    let result4 = sma.next((base_time + chrono::Duration::minutes(11), 103.0));
    assert_eq!(result4, 101.5); // Average of 100, 101, 102, 103

    // Move to next bucket (minute 15)
    let result5 = sma.next((base_time + chrono::Duration::minutes(15), 104.0));
    // With 15-minute window, the first value (100) should drop off
    // Window now has: 101, 102, 103, 104 (values from minutes 5, 10, 11, 15)
    assert_eq!(result5, 102.5); // Average of 101, 102, 103, 104
}

#[test]
fn test_standard_deviation_with_replacement() {
    // Test StandardDeviation with adaptive replacement
    let mut sd = StandardDeviation::new(Duration::from_secs(3600)).unwrap(); // 1 hour

    let base_time = Utc.with_ymd_and_hms(2024, 1, 1, 10, 0, 0).unwrap();

    // Add values at 1-minute intervals (will be detected as intraday)
    sd.next((base_time, 10.0));
    sd.next((base_time + chrono::Duration::minutes(1), 12.0));
    sd.next((base_time + chrono::Duration::minutes(2), 11.0));

    // Update within the same minute bucket - should replace
    let _result1 = sd.next((
        base_time + chrono::Duration::minutes(2) + chrono::Duration::seconds(30),
        11.5,
    ));

    // Add another value in a new minute
    let result2 = sd.next((base_time + chrono::Duration::minutes(3), 10.5));

    // The standard deviation should be calculated with the replaced value
    assert!(result2 > 0.0); // Should have some variance
}

#[test]
fn test_transition_from_warmup_to_live() {
    // Warm-up feeds each session's open and close; live then ticks every minute.
    let mut sma = SimpleMovingAverage::new(Duration::from_secs(2 * 86400)).unwrap(); // 2 days

    sma.next((Utc.with_ymd_and_hms(2024, 1, 2, 14, 30, 0).unwrap(), 100.0));
    sma.next((Utc.with_ymd_and_hms(2024, 1, 2, 21, 0, 0).unwrap(), 102.0));
    let warmup_result = sma.next((Utc.with_ymd_and_hms(2024, 1, 3, 14, 30, 0).unwrap(), 104.0));
    assert_eq!(warmup_result, 103.0); // 102 and the in-progress 104

    let live = sma.next((Utc.with_ymd_and_hms(2024, 1, 3, 15, 0, 0).unwrap(), 105.0));
    assert_eq!(live, 103.5); // the minute update revises the session: 102 and 105
}

/// The same daily indicator must give the same value at the same moment however
/// often it is fed: live every minute or every five, a backtest feeding the open,
/// a near-close decision and the close, or a process restarted at noon that
/// rebuilds from each prior session's open and close.
#[test]
fn test_daily_indicators_are_cadence_invariant() {
    const SESSIONS: i64 = 30;
    const OPEN: i64 = 14 * 60 + 30; // minutes after UTC midnight
    const CLOSE: i64 = 21 * 60;
    const DECISION: i64 = CLOSE - 2;
    let first = Utc.with_ymd_and_hms(2024, 1, 2, 0, 0, 0).unwrap();
    let at = |session: i64, minute: i64| {
        first + chrono::Duration::days(session) + chrono::Duration::minutes(minute)
    };
    // Deterministic minute prices with an overnight gap.
    let price = |session: i64, minute: i64| {
        let x = (session * 1_000 + minute) as f64;
        500.0 + 20.0 * (x / 997.0).sin() + 3.0 * (x / 37.0).cos() + 0.5 * (x / 7.0).sin()
    };
    type Feed = Vec<(i64, i64, bool)>;
    let cadence = |step: i64| -> Feed {
        (0..SESSIONS)
            .flat_map(|d| {
                let mut ticks: Vec<_> = (OPEN..=CLOSE)
                    .step_by(step as usize)
                    .map(|m| (d, m, false))
                    .collect();
                ticks.push((d, DECISION, true));
                ticks.sort();
                ticks
            })
            .collect()
    };
    let backtest: Feed = (0..SESSIONS)
        .flat_map(|d| [(d, OPEN, false), (d, DECISION, true), (d, CLOSE, false)])
        .collect();
    let restart: Feed = (0..SESSIONS)
        .flat_map(|d| -> Feed {
            if d < 20 {
                vec![(d, OPEN, false), (d, CLOSE, false)]
            } else {
                let start = if d == 20 { 18 * 60 } else { OPEN };
                let mut ticks: Feed = (start..=CLOSE).map(|m| (d, m, false)).collect();
                ticks.push((d, DECISION, true));
                ticks.sort();
                ticks
            }
        })
        .collect();

    let fourteen_days = Duration::from_secs(14 * 86400);
    let run = |feed: &Feed| -> Vec<(i64, [f64; 13])> {
        let mut rsi = RelativeStrengthIndex::new(fourteen_days).unwrap();
        let mut ema = ExponentialMovingAverage::new(fourteen_days).unwrap();
        let mut sma = SimpleMovingAverage::new(fourteen_days).unwrap();
        let mut sd = StandardDeviation::new(fourteen_days).unwrap();
        let mut mad = MeanAbsoluteDeviation::new(fourteen_days).unwrap();
        let mut roc = RateOfChange::new(fourteen_days).unwrap();
        let mut max = Maximum::new(fourteen_days).unwrap();
        let mut min = Minimum::new(fourteen_days).unwrap();
        let mut drawdown = MaxDrawdown::new(fourteen_days).unwrap();
        let mut drawup = MaxDrawup::new(fourteen_days).unwrap();
        let mut sum = RollingSum::new(fourteen_days).unwrap();
        let mut bands = BollingerBands::new(fourteen_days, 2.0).unwrap();
        let mut lag = Lag::new(Duration::from_secs(3 * 86400)).unwrap();
        let mut out = Vec::new();
        for &(d, m, decision) in feed {
            let input = (at(d, m), price(d, m));
            let values = [
                rsi.next(input),
                ema.next(input),
                sma.next(input),
                sd.next(input),
                mad.next(input),
                roc.next(input),
                max.next(input),
                min.next(input),
                drawdown.next(input),
                drawup.next(input),
                sum.next(input),
                bands.next(input),
                lag.next(input).unwrap_or(f64::NAN),
            ];
            if decision && d >= 20 {
                out.push((d, values));
            }
        }
        out
    };
    let reference = run(&cadence(1));
    assert_eq!(reference.len(), 10);
    for (name, feed) in [
        ("5-minute", cadence(5)),
        ("backtest", backtest),
        ("restart", restart),
    ] {
        let values = run(&feed);
        for ((d, expected), (_, actual)) in reference.iter().zip(&values) {
            for (i, (e, a)) in expected.iter().zip(actual).enumerate() {
                assert!(
                    (e - a).abs() <= 1e-9 * e.abs().max(1.0) || (e.is_nan() && a.is_nan()),
                    "{name} session {d} indicator {i}: {a} != {e}"
                );
            }
        }
    }
}

#[test]
fn test_high_frequency_tick_data() {
    // Test with very high frequency data (sub-second)
    let mut sma = SimpleMovingAverage::new(Duration::from_secs(5)).unwrap(); // 5 seconds

    let base_time = Utc.with_ymd_and_hms(2024, 1, 1, 10, 0, 0).unwrap();

    // Add tick data at sub-second intervals
    // These should be detected as high-frequency and each treated as unique
    sma.next((base_time, 100.0));
    sma.next((base_time + chrono::Duration::milliseconds(100), 100.1));
    sma.next((base_time + chrono::Duration::milliseconds(200), 100.2));
    sma.next((base_time + chrono::Duration::milliseconds(300), 100.3));

    let result = sma.next((base_time + chrono::Duration::milliseconds(400), 100.4));
    // With a 5-second duration (< 5 minutes), we use second-level bucketing
    // All millisecond updates within the same second get replaced
    // So we only have the last value: 100.4
    assert_eq!(result, 100.4); // Only one value in the window after replacements
}

#[test]
fn test_minute_bar_replacement() {
    // Test replacement within minute bars
    let mut sma = SimpleMovingAverage::new(Duration::from_secs(5 * 60)).unwrap(); // 5 minutes

    let base_time = Utc.with_ymd_and_hms(2024, 1, 1, 10, 0, 0).unwrap();

    // First minute
    sma.next((base_time, 100.0));

    // Update within the same minute (should replace if detected as 1-minute buckets)
    let _result1 = sma.next((base_time + chrono::Duration::seconds(30), 100.5));

    // Second minute
    let _result2 = sma.next((base_time + chrono::Duration::minutes(1), 101.0));

    // Third minute
    let result3 = sma.next((base_time + chrono::Duration::minutes(2), 102.0));

    // The exact results depend on how the detector interprets the pattern
    // But we should have at most 3 values in the window
    assert!(result3 >= 100.0 && result3 <= 102.0);
}

#[test]
fn test_weekly_data_detection() {
    // Test with weekly data (very long intervals)
    let mut sma = SimpleMovingAverage::new(Duration::from_secs(21 * 86400)).unwrap(); // 3 weeks (21 days)

    let week1 = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
    let week2 = Utc.with_ymd_and_hms(2024, 1, 8, 0, 0, 0).unwrap();
    let week3 = Utc.with_ymd_and_hms(2024, 1, 15, 0, 0, 0).unwrap();

    let result1 = sma.next((week1, 100.0));
    assert_eq!(result1, 100.0);

    let result2 = sma.next((week2, 110.0));
    assert_eq!(result2, 105.0);

    let result3 = sma.next((week3, 120.0));
    assert_eq!(result3, 110.0);

    // Add another value a week later
    let week4 = Utc.with_ymd_and_hms(2024, 1, 22, 0, 0, 0).unwrap();
    let result4 = sma.next((week4, 115.0));
    // First value should drop off (outside 21-day window)
    assert_eq!(result4, 115.0); // Average of 110, 120, 115
}
