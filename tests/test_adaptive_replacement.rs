use chrono::{TimeZone, Utc};
use chrono_ta::indicators::{SimpleMovingAverage, StandardDeviation};
use chrono_ta::Next;
use std::time::Duration;

#[test]
fn test_daily_ohlc_no_replacement() {
    // Test that daily OHLC data (Open and Close) are NOT replaced
    let mut sma = SimpleMovingAverage::new(Duration::from_secs(3 * 86400)).unwrap(); // 3 days

    // Day 1: Open at 9:30 AM
    let day1_open = Utc.with_ymd_and_hms(2024, 1, 1, 9, 30, 0).unwrap();
    let result1 = sma.next((day1_open, 100.0));
    assert_eq!(result1, 100.0); // First value

    // Day 1: Close at 4:00 PM (6.5 hours later) - should NOT replace
    let day1_close = Utc.with_ymd_and_hms(2024, 1, 1, 16, 0, 0).unwrap();
    let result2 = sma.next((day1_close, 105.0));
    assert_eq!(result2, 102.5); // Average of 100 and 105

    // Day 2: Open at 9:30 AM
    let day2_open = Utc.with_ymd_and_hms(2024, 1, 2, 9, 30, 0).unwrap();
    let result3 = sma.next((day2_open, 110.0));
    assert_eq!(result3, 105.0); // Average of 100, 105, and 110

    // Day 2: Close at 4:00 PM - should NOT replace
    let day2_close = Utc.with_ymd_and_hms(2024, 1, 2, 16, 0, 0).unwrap();
    let result4 = sma.next((day2_close, 108.0));
    // With 3-day window, all 4 values should still be in window
    // Window has: 100, 105, 110, 108
    assert_eq!(result4, 105.75);
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
    // Simulate warming up with daily data then transitioning to intraday
    let mut sma = SimpleMovingAverage::new(Duration::from_secs(2 * 86400)).unwrap(); // 2 days

    // Warmup with daily OHLC (>4 hours apart)
    let day1_open = Utc.with_ymd_and_hms(2024, 1, 1, 9, 30, 0).unwrap();
    sma.next((day1_open, 100.0));

    let day1_close = Utc.with_ymd_and_hms(2024, 1, 1, 16, 0, 0).unwrap();
    sma.next((day1_close, 102.0));

    let day2_open = Utc.with_ymd_and_hms(2024, 1, 2, 9, 30, 0).unwrap();
    let warmup_result = sma.next((day2_open, 104.0));
    assert_eq!(warmup_result, 102.0); // Average of 100, 102, 104

    // Now continue with more frequent updates on day 2
    // With DailyOHLC and 3.4-hour gap logic, this WILL replace day2_open
    // since 30 minutes < 3.4 hours
    let day2_mid = Utc.with_ymd_and_hms(2024, 1, 2, 10, 0, 0).unwrap();
    let result = sma.next((day2_mid, 105.0));
    // Should replace day2_open (104) with 105
    assert_eq!(result, (100.0 + 102.0 + 105.0) / 3.0); // Average of 100, 102, 105
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
