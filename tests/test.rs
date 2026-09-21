#[cfg(test)]
mod test {
    #[cfg(feature = "serde")]
    mod serde {
        use chrono::Utc;
        use chrono_ta::indicators::SimpleMovingAverage;
        use chrono_ta::Next;
        use std::time::Duration;

        // Simple smoke test that serde works (not sure if this is really necessary)
        #[test]
        fn test_serde() {
            let mut sma = SimpleMovingAverage::new(Duration::from_secs(20)).unwrap();
            let bytes = bincode::serialize(&sma).unwrap();
            let mut deserialized: SimpleMovingAverage = bincode::deserialize(&bytes).unwrap();

            let now = Utc::now();
            assert_eq!(deserialized.next((now, 2.0)), sma.next((now, 2.0)));
        }
    }
}
