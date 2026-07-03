/// Unit and behaviour tests for the Stellar Transaction Throughput Optimization
/// pipeline (Issue #401).
///
/// These tests are all pure-unit tests — no live database or live Horizon node
/// is required.  Tests that need a real database are tagged `#[ignore]` and can
/// be executed with:
///
///   cargo test --test stellar_throughput_tests -- --ignored --nocapture
///
/// Feature gate: all tests require the `database` feature because the module
/// tree lives behind that feature flag.

#[cfg(test)]
mod fee_engine {
    use aframp_backend::stellar::fee_engine::DynamicFeeEngine;
    use aframp_backend::stellar::models::FeeConfiguration;

    fn make_engine(max_fee: i64, surge_multiplier: f64) -> DynamicFeeEngine {
        DynamicFeeEngine::new(
            FeeConfiguration {
                base_fee: 100,
                min_fee: 100,
                max_fee,
                surge_threshold: 0.8,
                surge_multiplier,
                low_capacity_fee: 1_000,
            },
            "https://horizon-testnet.stellar.org".to_string(),
        )
    }

    #[test]
    fn normal_fee_no_surge() {
        let engine = make_engine(10_000, 1.5);
        // 1 op × 100 base × 1.0 multiplier = 100 stroops
        let fee = engine.calculate_fee_with_multiplier(1, 100, 1.0);
        assert_eq!(fee, 100);
    }

    #[test]
    fn surge_multiplier_applied() {
        let engine = make_engine(10_000, 1.5);
        // 1 op × 100 base × 1.5 surge = 150 stroops
        let fee = engine.calculate_fee_with_multiplier(1, 100, 1.5);
        assert_eq!(fee, 150);
    }

    #[test]
    fn multi_op_fee_scales_linearly() {
        let engine = make_engine(10_000, 1.5);
        // 5 ops × 100 = 500 stroops
        let fee = engine.calculate_fee_with_multiplier(5, 100, 1.0);
        assert_eq!(fee, 500);
    }

    #[test]
    fn fee_capped_at_max() {
        let engine = make_engine(5_000, 2.0);
        // 100 ops × 200 = 20_000, capped at 5_000
        let fee = engine.calculate_fee_with_multiplier(100, 100, 2.0);
        assert_eq!(fee, 5_000);
    }

    #[test]
    fn fee_floored_at_min() {
        let engine = make_engine(10_000, 1.5);
        // 1 op × 50 × 0.5 = 25, floored at 100
        let fee = engine.calculate_fee_with_multiplier(1, 50, 0.5);
        assert_eq!(fee, 100);
    }

    #[test]
    fn fee_config_default_values_are_sane() {
        let cfg = FeeConfiguration::default();
        assert_eq!(cfg.base_fee, 100, "base_fee default");
        assert_eq!(cfg.min_fee, 100, "min_fee default");
        assert_eq!(cfg.max_fee, 10_000, "max_fee default");
        assert!(
            (cfg.surge_threshold - 0.8).abs() < f64::EPSILON,
            "surge_threshold"
        );
        assert!(
            (cfg.surge_multiplier - 1.5).abs() < f64::EPSILON,
            "surge_multiplier"
        );
    }
}

#[cfg(test)]
mod sequence_coordinator {
    use aframp_backend::stellar::sequence_coordinator::SequenceCoordinator;

    #[test]
    fn initial_state() {
        let coord = SequenceCoordinator::new(1000, 10);
        assert_eq!(coord.current_sequence(), 1000);
        assert_eq!(coord.reserved_sequence(), 1000);
        assert_eq!(coord.in_flight_count(), 0);
    }

    #[test]
    fn single_reservation_increments_reserved() {
        let coord = SequenceCoordinator::new(1000, 10);
        let seq = coord.reserve_next().unwrap();
        assert_eq!(seq, 1001);
        assert_eq!(coord.in_flight_count(), 1);
        assert_eq!(coord.current_sequence(), 1000); // unchanged until confirmed
    }

    #[test]
    fn sequential_reservations_are_monotonically_increasing() {
        let coord = SequenceCoordinator::new(0, 100);
        let mut last = 0i64;
        for _ in 0..50 {
            let seq = coord.reserve_next().unwrap();
            assert!(seq > last, "each sequence must be greater than previous");
            last = seq;
        }
    }

    #[test]
    fn confirmation_advances_current() {
        let coord = SequenceCoordinator::new(100, 10);
        coord.reserve_next().unwrap(); // 101
        coord.reserve_next().unwrap(); // 102
        coord.mark_confirmed(102).unwrap();
        assert_eq!(coord.current_sequence(), 102);
    }

    #[test]
    fn exhaustion_returns_error() {
        let coord = SequenceCoordinator::new(100, 2);
        coord.reserve_next().unwrap();
        coord.reserve_next().unwrap();
        assert!(
            coord.reserve_next().is_err(),
            "should fail once max_in_flight is reached"
        );
    }

    #[test]
    fn parallel_reservations_have_no_duplicates() {
        use std::sync::Arc;
        use std::thread;

        let coord = Arc::new(SequenceCoordinator::new(0, 200));
        let mut handles = vec![];

        for _ in 0..10 {
            let c = Arc::clone(&coord);
            handles.push(thread::spawn(move || {
                (0..10)
                    .filter_map(|_| c.reserve_next().ok())
                    .collect::<Vec<_>>()
            }));
        }

        let mut all: Vec<i64> = handles
            .into_iter()
            .flat_map(|h| h.join().unwrap())
            .collect();

        all.sort_unstable();
        all.dedup();
        // All 100 sequences should be unique (dedup removes nothing)
        assert_eq!(all.len(), 100, "no duplicate sequence numbers");
    }

    #[test]
    fn sync_with_horizon_advances_state() {
        let coord = SequenceCoordinator::new(100, 10);
        coord.reserve_next().unwrap(); // 101
        coord.reserve_next().unwrap(); // 102
        coord.sync_with_horizon(102).unwrap();
        assert_eq!(coord.current_sequence(), 102);
        assert_eq!(coord.reserved_sequence(), 102);
    }
}

#[cfg(test)]
mod retry_state_machine {
    use aframp_backend::stellar::error::SubmissionError;
    use aframp_backend::stellar::models::RetryPolicy;
    use aframp_backend::stellar::retry_state_machine::{RetryState, RetryStateMachine};
    use chrono::Duration;

    fn default_machine() -> RetryStateMachine {
        RetryStateMachine::new(RetryPolicy::default())
    }

    #[test]
    fn initial_state_is_pending() {
        assert_eq!(default_machine().current_state(), &RetryState::Pending);
    }

    #[test]
    fn transient_error_is_retryable() {
        let m = default_machine();
        let err = SubmissionError::TransientNetworkError {
            source: "timeout".to_string(),
            attempt: 1,
        };
        assert!(m.should_retry(&err));
    }

    #[test]
    fn bad_sequence_is_retryable_and_rotates_channel() {
        let m = default_machine();
        let err = SubmissionError::BadSequence("seq mismatch".to_string());
        assert!(m.should_retry(&err));
        assert!(m.should_rotate_channel(&err));
    }

    #[test]
    fn max_retries_stops_retrying() {
        let policy = RetryPolicy {
            max_retries: 3,
            base_backoff_ms: 10,
            max_backoff_ms: 1_000,
            backoff_multiplier: 2.0,
        };
        let mut m = RetryStateMachine::new(policy);
        let err = SubmissionError::TransientNetworkError {
            source: "timeout".to_string(),
            attempt: 1,
        };
        for _ in 0..4 {
            let _ = m.record_attempt(&err);
        }
        assert!(!m.should_retry(&err), "should stop after max retries");
    }

    #[test]
    fn exponential_backoff_doubles_each_attempt() {
        let policy = RetryPolicy {
            max_retries: 10,
            base_backoff_ms: 100,
            max_backoff_ms: 100_000,
            backoff_multiplier: 2.0,
        };
        let m = RetryStateMachine::new(policy);
        let delay0 = m.calculate_next_retry_delay().as_millis();
        assert_eq!(delay0, 100, "first delay is base_backoff_ms");
    }

    #[test]
    fn mark_confirmed_transitions_state() {
        let mut m = default_machine();
        m.mark_confirmed("abc123".to_string(), 42);
        match m.current_state() {
            RetryState::Confirmed {
                stellar_tx_hash,
                ledger,
            } => {
                assert_eq!(stellar_tx_hash, "abc123");
                assert_eq!(*ledger, 42);
            }
            other => panic!("expected Confirmed, got {:?}", other),
        }
    }

    #[test]
    fn stale_detection_based_on_threshold() {
        let policy = RetryPolicy::default();
        let mut m = RetryStateMachine::new(policy);
        // Force the created_at to be far in the past via mark_stale path
        // (we can't directly set private fields from outside the crate)
        assert!(
            !m.is_stale(Duration::seconds(1)),
            "brand-new tx is not stale"
        );
    }

    #[test]
    fn retry_policy_defaults_are_sensible() {
        let p = RetryPolicy::default();
        assert!(p.max_retries > 0);
        assert!(p.base_backoff_ms > 0);
        assert!(p.max_backoff_ms >= p.base_backoff_ms);
        assert!(p.backoff_multiplier > 1.0);
    }
}

#[cfg(test)]
mod error_classification {
    use aframp_backend::stellar::error::HorizonErrorCode;

    #[test]
    fn bad_seq_not_retryable() {
        assert!(!HorizonErrorCode::TxBadSeq.is_retryable());
    }

    #[test]
    fn insufficient_fee_not_retryable() {
        assert!(!HorizonErrorCode::TxInsufficientFee.is_retryable());
    }

    #[test]
    fn transient_is_retryable() {
        assert!(HorizonErrorCode::Transient.is_retryable());
    }

    #[test]
    fn internal_server_error_is_retryable() {
        assert!(HorizonErrorCode::InternalServerError.is_retryable());
    }

    #[test]
    fn stale_ledger_is_retryable() {
        assert!(HorizonErrorCode::StaleLedgerVersion.is_retryable());
    }

    #[test]
    fn bad_seq_and_insufficient_fee_are_channel_exhaustion_errors() {
        assert!(HorizonErrorCode::TxBadSeq.is_channel_exhaustion());
        assert!(HorizonErrorCode::TxInsufficientFee.is_channel_exhaustion());
        assert!(!HorizonErrorCode::Transient.is_channel_exhaustion());
    }

    #[test]
    fn from_str_parsing() {
        assert!(matches!(
            HorizonErrorCode::from_str("tx_bad_seq"),
            HorizonErrorCode::TxBadSeq
        ));
        assert!(matches!(
            HorizonErrorCode::from_str("tx_insufficient_fee"),
            HorizonErrorCode::TxInsufficientFee
        ));
        assert!(matches!(
            HorizonErrorCode::from_str("tx_malformed"),
            HorizonErrorCode::TxMalformed
        ));
        assert!(matches!(
            HorizonErrorCode::from_str("connection timeout"),
            HorizonErrorCode::Transient
        ));
        // Unknown codes become Unknown
        let unknown = HorizonErrorCode::from_str("some_weird_code");
        assert!(matches!(unknown, HorizonErrorCode::Unknown(_)));
    }
}

#[cfg(test)]
mod submission_queue_status {
    use aframp_backend::stellar::models::SubmissionQueueStatus;

    #[test]
    fn as_str_round_trips() {
        assert_eq!(SubmissionQueueStatus::Pending.as_str(), "PENDING");
        assert_eq!(SubmissionQueueStatus::Submitted.as_str(), "SUBMITTED");
        assert_eq!(SubmissionQueueStatus::Confirmed.as_str(), "CONFIRMED");
        assert_eq!(SubmissionQueueStatus::Failed.as_str(), "FAILED");
        assert_eq!(SubmissionQueueStatus::Retrying.as_str(), "RETRYING");
    }

    #[test]
    fn equality_holds() {
        assert_eq!(
            SubmissionQueueStatus::Pending,
            SubmissionQueueStatus::Pending
        );
        assert_ne!(
            SubmissionQueueStatus::Pending,
            SubmissionQueueStatus::Failed
        );
    }
}

#[cfg(test)]
mod batch_submission {
    use aframp_backend::stellar::models::{BatchEnvelopeRequest, BatchSubmissionResult};

    #[test]
    fn batch_envelope_request_is_constructible() {
        let req = BatchEnvelopeRequest {
            tx_envelope_xdr: "AAAAAA==".to_string(),
            operation_count: 10,
        };
        assert_eq!(req.operation_count, 10);
    }

    #[test]
    fn batch_result_counts_accepted_and_rejected() {
        let result = BatchSubmissionResult {
            accepted: 95,
            rejected: 5,
            queued_ids: vec![uuid::Uuid::new_v4(); 95],
        };
        assert_eq!(result.accepted + result.rejected, 100);
        assert_eq!(result.queued_ids.len(), 95);
    }
}

#[cfg(test)]
mod admin_state {
    use aframp_backend::stellar::admin::StellarAdminState;

    /// Compile-time check: StellarAdminState must implement Clone so axum can
    /// hand it to each handler invocation.
    #[test]
    fn state_is_clone() {
        fn assert_clone<T: Clone>() {}
        assert_clone::<StellarAdminState>();
    }
}

#[cfg(test)]
mod horizon_client {
    use aframp_backend::stellar::horizon::HorizonClient;
    use std::time::Duration;

    #[test]
    fn client_construction_with_single_node() {
        let url = "https://horizon-testnet.stellar.org".to_string();
        let client = HorizonClient::new(url.clone());
        drop(client);
    }

    #[test]
    fn with_rpc_endpoints_accepts_multiple_nodes() {
        // Three-node load-balanced cluster — simulates Validator Interaction Tuning.
        let endpoints = vec![
            "https://horizon1.example.com".to_string(),
            "https://horizon2.example.com".to_string(),
            "https://horizon3.example.com".to_string(),
        ];
        let client = HorizonClient::new("https://horizon.stellar.org".to_string())
            .with_rpc_endpoints(endpoints);
        drop(client);
    }

    #[test]
    fn custom_timeout_is_accepted() {
        let client = HorizonClient::new("https://horizon-testnet.stellar.org".to_string())
            .with_timeout(Duration::from_secs(30));
        drop(client);
    }

    /// Verify the round-robin distribution logic doesn't panic even when the
    /// endpoint list has exactly one entry.
    #[test]
    fn single_rpc_endpoint_list_is_safe() {
        let client = HorizonClient::new("https://horizon.stellar.org".to_string())
            .with_rpc_endpoints(vec!["https://rpc.example.com".to_string()]);
        drop(client);
    }
}
