use std::collections::VecDeque;
use std::fmt;
use std::time::Duration;

use crate::errors::Result;
use crate::indicators::AdaptiveTimeDetector;
use crate::{Next, NextBatch, Reset};
use chrono::{DateTime, Utc};
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

const MAX_WINDOW_SIZE: usize = 500;
const KEEP_OLDEST: usize = 10;
const KEEP_RECENT: usize = 100;

#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Debug, Clone)]
pub struct Minimum {
    duration: Duration,
    window: VecDeque<(DateTime<Utc>, f64)>,
    /// Nanosecond timestamps mirroring `window`. This transient cache removes
    /// repeated `DateTime` conversion from the no-expiry hot path without
    /// changing the serialized indicator state.
    #[cfg_attr(feature = "serde", serde(skip))]
    window_nanos: VecDeque<i64>,
    detector: AdaptiveTimeDetector,
    /// Cached `chrono::Duration` form of `duration` (computed once on first use)
    /// so `next()` skips a `from_std` conversion every call. Not serialized;
    /// lazily recomputed after deserialization.
    #[cfg_attr(feature = "serde", serde(skip))]
    cached_window: Option<i64>,
    /// Monotonic-increasing candidate deque over the *sealed* points (every
    /// point except the current/newest): entries run in increasing time (front
    /// oldest) and strictly increasing value, so `front()` is the min over the
    /// sealed points. Combined with the current point it gives the window min in
    /// O(1) amortized instead of an O(window) scan. Transient — rebuilt after a
    /// thin and defensively after a deserialize. Never a persisted contract.
    #[cfg_attr(feature = "serde", serde(skip))]
    mono: VecDeque<(i64, f64)>,
}

impl Minimum {
    pub fn get_window(&self) -> VecDeque<(DateTime<Utc>, f64)> {
        self.window.clone()
    }

    pub fn new(duration: Duration) -> Result<Self> {
        // Change: Check for zero duration (std::time::Duration can't be negative)
        if duration.as_secs() == 0 && duration.subsec_nanos() == 0 {
            return Err(crate::errors::TaError::InvalidParameter);
        }
        Ok(Self {
            duration,
            window: VecDeque::new(),
            window_nanos: VecDeque::new(),
            detector: AdaptiveTimeDetector::new(duration),
            cached_window: None,
            mono: VecDeque::new(),
        })
    }

    /// Authoritative O(window) scan. Kept as the correctness oracle for the
    /// `debug_assert` in `next()`; compiled out of release builds.
    #[cfg_attr(not(debug_assertions), allow(dead_code))]
    fn find_min_value(&self) -> f64 {
        self.window
            .iter()
            .map(|&(_, val)| val)
            .fold(f64::INFINITY, f64::min)
    }

    /// Rebuild `mono` from the *sealed* points (`window[..len-1]`) in O(n).
    /// Called after a thin (which drops interior points) and defensively after
    /// a deserialize, where `mono` deserializes empty while `window` is populated.
    fn rebuild_transients(&mut self) {
        self.window_nanos.clear();
        self.mono.clear();
        let sealed_len = self.window.len().saturating_sub(1);
        for (index, &(timestamp, value)) in self.window.iter().enumerate() {
            let timestamp_nanos = timestamp.timestamp_nanos_opt().unwrap_or(i64::MIN);
            self.window_nanos.push_back(timestamp_nanos);
            if index >= sealed_len {
                continue;
            }
            while self.mono.back().map_or(false, |&(_, bv)| bv >= value) {
                self.mono.pop_back();
            }
            self.mono.push_back((timestamp_nanos, value));
        }
    }

    fn remove_old(&mut self, current_nanos: i64) {
        let dur_nanos = *self
            .cached_window
            .get_or_insert_with(|| self.duration.as_nanos() as i64);
        let cutoff_nanos = current_nanos - dur_nanos;
        while self
            .window_nanos
            .front()
            .is_some_and(|&timestamp_nanos| timestamp_nanos < cutoff_nanos)
        {
            self.window.pop_front();
            self.window_nanos.pop_front();
        }
        // Evict the same expired points from the candidate deque (identical
        // strict `<` predicate); `mono` is time-ordered front-oldest.
        while self
            .mono
            .front()
            .is_some_and(|&(timestamp_nanos, _)| timestamp_nanos < cutoff_nanos)
        {
            self.mono.pop_front();
        }
    }

    fn thin_window(&mut self) {
        if self.window.len() <= MAX_WINDOW_SIZE {
            return;
        }

        let len = self.window.len();
        let middle_start = KEEP_OLDEST;
        let middle_end = len.saturating_sub(KEEP_RECENT);

        if middle_end <= middle_start {
            return;
        }

        let mut new_window = VecDeque::with_capacity(MAX_WINDOW_SIZE);

        for i in 0..middle_start.min(len) {
            new_window.push_back(self.window[i]);
        }

        let mut keep = true;
        for i in middle_start..middle_end {
            if keep {
                new_window.push_back(self.window[i]);
            }
            keep = !keep;
        }

        for i in middle_end..len {
            new_window.push_back(self.window[i]);
        }

        self.window = new_window;
    }
}

impl Next<f64> for Minimum {
    type Output = f64;

    fn next(&mut self, (timestamp, value): (DateTime<Utc>, f64)) -> Self::Output {
        // Resync the transient candidate deque after a deserialize (window
        // populated from a snapshot, mono defaulted empty). Invariant otherwise:
        // mono is non-empty iff window has >= 2 points.
        if self.window_nanos.len() != self.window.len()
            || (self.mono.is_empty() && self.window.len() > 1)
        {
            self.rebuild_transients();
        }

        // Check if we should replace the last value (same time bucket)
        let should_replace = self.detector.should_replace(timestamp);

        // ALWAYS remove old data first, regardless of replace/add (evicts
        // expired points from both the window and the sealed-candidate deque).
        let timestamp_nanos = timestamp.timestamp_nanos_opt().unwrap_or(i64::MIN);
        self.remove_old(timestamp_nanos);

        if should_replace {
            // Same bucket: drop the current (newest) point. It is `window.back()`
            // and is deliberately NOT in `mono` (which holds only sealed points),
            // so there is no candidate-deque surgery to do — the O(1) hot path.
            if !self.window.is_empty() {
                self.window.pop_back();
                self.window_nanos.pop_back();
            }
        } else if let (Some(&(_, sealed_value)), Some(&sealed_nanos)) =
            (self.window.back(), self.window_nanos.back())
        {
            // New bucket: the point that was current becomes permanent. Seal it
            // into the monotonic deque now, dropping dominated tail candidates
            // (any tail value >= it can never again be the min while it is
            // in-window, since it is newer).
            while self
                .mono
                .back()
                .map_or(false, |&(_, bv)| bv >= sealed_value)
            {
                self.mono.pop_back();
            }
            self.mono.push_back((sealed_nanos, sealed_value));
        }

        // The new point becomes the current (newest) point. It stays OUT of
        // `mono` until a later new-bucket tick seals it.
        self.window.push_back((timestamp, value));
        self.window_nanos.push_back(timestamp_nanos);

        // Thin window if it exceeds max size (sparse sampling for memory
        // efficiency). Thinning drops interior points, so rebuild mono to match.
        let len_before = self.window.len();
        self.thin_window();
        if self.window.len() != len_before {
            self.rebuild_transients();
        }

        // O(1) min = min(best sealed candidate, current point). In debug builds,
        // cross-check against the authoritative scan so any desync fails loudly.
        let min = match (self.mono.front(), self.window.back()) {
            (Some(&(_, s)), Some(&(_, c))) => s.min(c),
            (None, Some(&(_, c))) => c,
            (Some(&(_, s)), None) => s,
            (None, None) => f64::INFINITY,
        };
        debug_assert_eq!(
            min,
            self.find_min_value(),
            "monotonic min desynced from window scan"
        );
        min
    }
}

impl NextBatch<f64> for Minimum {}

impl Reset for Minimum {
    fn reset(&mut self) {
        self.window.clear();
        self.window_nanos.clear();
        self.mono.clear();
        self.detector.reset();
    }
}

impl Default for Minimum {
    fn default() -> Self {
        // Change: Use Duration::from_secs for 14 days
        Self::new(Duration::from_secs(14 * 24 * 60 * 60)).unwrap()
    }
}

impl fmt::Display for Minimum {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        // Change: Calculate days from seconds
        let days = self.duration.as_secs() / 86400;
        write!(f, "MIN({} days)", days)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};

    // Helper function to create a DateTime<Utc> from a date string for testing
    fn datetime(s: &str) -> DateTime<Utc> {
        Utc.datetime_from_str(s, "%Y-%m-%d %H:%M:%S").unwrap()
    }

    #[test]
    fn test_new() {
        // Change: Use std::time::Duration constructors
        assert!(Minimum::new(Duration::from_secs(0)).is_err());
        assert!(Minimum::new(Duration::from_secs(86400)).is_ok()); // 1 day
    }

    #[test]
    fn test_next() {
        let duration = Duration::from_secs(2 * 86400); // 2 days
        let mut min = Minimum::new(duration).unwrap();

        assert_eq!(min.next((datetime("2023-01-01 00:00:00"), 4.0)), 4.0);
        assert_eq!(min.next((datetime("2023-01-02 00:00:00"), 1.2)), 1.2);
        assert_eq!(min.next((datetime("2023-01-03 00:00:00"), 5.0)), 1.2);
        assert_eq!(min.next((datetime("2023-01-04 00:00:00"), 3.0)), 1.2);
        assert_eq!(min.next((datetime("2023-01-05 00:00:00"), 4.0)), 3.0);
        assert_eq!(min.next((datetime("2023-01-06 00:00:00"), 6.0)), 3.0);
        assert_eq!(min.next((datetime("2023-01-07 00:00:00"), 7.0)), 4.0);
        assert_eq!(min.next((datetime("2023-01-08 00:00:00"), 8.0)), 6.0);
        assert_eq!(min.next((datetime("2023-01-09 00:00:00"), -9.0)), -9.0);
        assert_eq!(min.next((datetime("2023-01-10 00:00:00"), 0.0)), -9.0);
    }

    #[test]
    fn test_reset() {
        let duration = Duration::from_secs(10 * 86400); // 10 days
        let mut min = Minimum::new(duration).unwrap();

        assert_eq!(min.next((datetime("2023-01-01 00:00:00"), 5.0)), 5.0);
        assert_eq!(min.next((datetime("2023-01-02 00:00:00"), 7.0)), 5.0);

        min.reset();
        assert_eq!(min.next((datetime("2023-01-03 00:00:00"), 8.0)), 8.0);
    }

    #[test]
    fn test_default() {
        let _ = Minimum::default();
    }

    #[test]
    fn test_display() {
        let indicator = Minimum::new(Duration::from_secs(10 * 86400)).unwrap(); // 10 days
        assert_eq!(format!("{}", indicator), "MIN(10 days)");
    }

    // Deterministic LCG so the property tests are reproducible without a dev-dep.
    fn lcg(state: &mut u64) -> u64 {
        *state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        *state >> 33
    }

    /// The O(1) monotonic path must equal the O(window) scan for every call.
    /// `next()` already `debug_assert`s `min == find_min_value()` each step, so
    /// varied/adversarial sequences make that scan oracle validate the fast path
    /// across eviction, same-bucket replacement, thinning, equal values, runs.
    #[test]
    fn monotonic_matches_scan_over_random_sequences() {
        let mut state: u64 = 0x2545_F491_4F6C_DD1D;
        for &secs in &[2u64, 3600, 86_400, 7 * 86_400] {
            let mut min = Minimum::new(Duration::from_secs(secs)).unwrap();
            let mut t = Utc.ymd(2020, 1, 1).and_hms(0, 0, 0);
            for _ in 0..1500 {
                let step = (lcg(&mut state) % (secs * 2 + 1)) as i64;
                t = t + chrono::Duration::seconds(step);
                let v = (lcg(&mut state) % 20_000) as f64 / 100.0 - 100.0; // [-100,100)
                let _ = min.next((t, v)); // internal debug_assert is the oracle
            }
        }
    }

    /// Force >500 in-window points so `thin_window` fires and `mono` is rebuilt
    /// from the thinned window; the internal debug_assert validates the min
    /// stays identical to the (thinned) scan the original used. We bound rather
    /// than assert exactness: the thinned min can never be below the true min.
    #[test]
    fn monotonic_matches_scan_across_thinning() {
        let mut min = Minimum::new(Duration::from_secs(1_000_000 * 86_400)).unwrap();
        let start = Utc.ymd(2000, 1, 1).and_hms(0, 0, 0);
        let mut true_running_min = f64::INFINITY;
        for i in 0..900i64 {
            let v = ((i.wrapping_mul(2_654_435_761)) % 1000) as f64;
            true_running_min = true_running_min.min(v);
            let got = min.next((start + chrono::Duration::seconds(i), v));
            assert!(got.is_finite());
            assert!(got >= true_running_min, "thinned min fell below true min");
        }
    }

    #[cfg(feature = "serde")]
    #[test]
    fn transient_timestamp_cache_preserves_serialized_contract_and_resume() {
        #[derive(serde::Serialize)]
        struct LegacyMinimum<'a> {
            duration: &'a Duration,
            window: &'a VecDeque<(DateTime<Utc>, f64)>,
            detector: &'a AdaptiveTimeDetector,
        }

        let duration = Duration::from_secs(7 * 86_400);
        let start = Utc.with_ymd_and_hms(2024, 1, 2, 9, 30, 0).unwrap();
        let mut uninterrupted = Minimum::new(duration).unwrap();
        for (offset, value) in [(0, 100.0), (30, 95.0), (390, 101.0), (1_440, 102.0)] {
            uninterrupted.next((start + chrono::Duration::minutes(offset), value));
        }

        let legacy_bytes = bincode::serialize(&LegacyMinimum {
            duration: &uninterrupted.duration,
            window: &uninterrupted.window,
            detector: &uninterrupted.detector,
        })
        .unwrap();
        assert_eq!(
            bincode::serialize(&uninterrupted).unwrap(),
            legacy_bytes,
            "transient caches must not change persisted bytes"
        );

        let mut resumed: Minimum = bincode::deserialize(&legacy_bytes).unwrap();
        assert!(resumed.window_nanos.is_empty());
        assert!(resumed.mono.is_empty());
        let next = (start + chrono::Duration::minutes(1_500), 90.0);
        assert_eq!(resumed.next(next), uninterrupted.next(next));
        assert_eq!(resumed.get_window(), uninterrupted.get_window());
        assert_eq!(
            bincode::serialize(&resumed).unwrap(),
            bincode::serialize(&uninterrupted).unwrap()
        );
    }
}
