//! Job Lifecycle & State Machine Tests (P2)
//!
//! JobState transitions must be correct for the API to report accurate
//! progress and for billing to work. These tests verify the state machine
//! and index strategy naming conventions.

use scrapix_core::{IndexStrategy, JobState, JobStatus};

// ============================================================================
// JobState creation
// ============================================================================

#[test]
fn test_job_state_initial_status_is_pending() {
    let state = JobState::new("job-1", "my-index");
    assert!(matches!(state.status, JobStatus::Pending));
    assert_eq!(state.job_id, "job-1");
    assert_eq!(state.index_uid, "my-index");
    assert!(state.started_at.is_none());
    assert!(state.completed_at.is_none());
    assert_eq!(state.pages_crawled, 0);
    assert_eq!(state.pages_indexed, 0);
    assert_eq!(state.errors, 0);
}

#[test]
fn test_job_state_with_account() {
    let state = JobState::with_account("job-2", "idx", "acct_billing");
    assert_eq!(state.account_id, Some("acct_billing".to_string()));
    assert!(matches!(state.status, JobStatus::Pending));
}

// ============================================================================
// State transitions
// ============================================================================

#[test]
fn test_job_state_start_sets_running() {
    let mut state = JobState::new("job-1", "idx");
    state.start();
    assert!(matches!(state.status, JobStatus::Running));
    assert!(state.started_at.is_some());
}

#[test]
fn test_job_state_complete_sets_completed() {
    let mut state = JobState::new("job-1", "idx");
    state.start();
    state.complete();
    assert!(matches!(state.status, JobStatus::Completed));
    assert!(state.completed_at.is_some());
}

#[test]
fn test_job_state_fail_sets_failed_with_message() {
    let mut state = JobState::new("job-1", "idx");
    state.start();
    state.fail("connection timeout");
    assert!(matches!(state.status, JobStatus::Failed));
    assert_eq!(state.error_message, Some("connection timeout".to_string()));
}

#[test]
fn test_job_state_duration_requires_start() {
    let state = JobState::new("job-1", "idx");
    assert!(
        state.duration_seconds().is_none(),
        "Duration should be None before start"
    );
}

#[test]
fn test_job_state_duration_after_start() {
    let mut state = JobState::new("job-1", "idx");
    state.start();
    // Duration should be available (>= 0) after start
    let dur = state.duration_seconds();
    assert!(dur.is_some());
    assert!(dur.unwrap() >= 0);
}

#[test]
fn test_job_state_duration_after_complete() {
    let mut state = JobState::new("job-1", "idx");
    state.start();
    state.complete();
    let dur = state.duration_seconds();
    assert!(dur.is_some());
    assert!(dur.unwrap() >= 0);
}

// ============================================================================
// Job counters
// ============================================================================

#[test]
fn test_job_state_counters_track_progress() {
    let mut state = JobState::new("job-1", "idx");
    state.start();

    state.pages_crawled = 100;
    state.pages_indexed = 95;
    state.documents_sent = 95;
    state.errors = 5;
    state.bytes_downloaded = 1_048_576; // 1MB

    assert_eq!(state.pages_crawled, 100);
    assert_eq!(state.pages_indexed, 95);
    assert_eq!(state.documents_sent, 95);
    assert_eq!(state.errors, 5);
    assert_eq!(state.bytes_downloaded, 1_048_576);
}

// ============================================================================
// Index strategy: temp index naming
// ============================================================================

#[test]
fn test_index_strategy_replace_temp_name_format() {
    // The temp index is named: {index_uid}_tmp_{job_id[..8]}
    let index_uid = "products";
    let job_id = "a1b2c3d4-e5f6-7890-abcd-ef1234567890";
    let temp = format!("{}_tmp_{}", index_uid, &job_id[..8]);
    assert_eq!(temp, "products_tmp_a1b2c3d4");
}

#[test]
fn test_index_strategy_replace_stores_temp_in_job_state() {
    let mut state = JobState::new("job-12345678-rest", "my-index");
    let temp_index = format!("{}_tmp_{}", state.index_uid, &state.job_id[..8]);
    state.swap_temp_index = Some(temp_index.clone());

    assert_eq!(
        state.swap_temp_index,
        Some("my-index_tmp_job-1234".to_string())
    );
}

#[test]
fn test_index_strategy_update_has_no_temp_index() {
    let state = JobState::new("job-1", "idx");
    assert!(
        state.swap_temp_index.is_none(),
        "Update strategy should not have a temp index"
    );
}

// ============================================================================
// IndexStrategy enum
// ============================================================================

#[test]
fn test_index_strategy_default_is_update() {
    assert!(matches!(IndexStrategy::default(), IndexStrategy::Update));
}

#[test]
fn test_index_strategy_serialization() {
    let update = serde_json::to_string(&IndexStrategy::Update).unwrap();
    let replace = serde_json::to_string(&IndexStrategy::Replace).unwrap();

    assert_eq!(update, "\"update\"");
    assert_eq!(replace, "\"replace\"");

    let d: IndexStrategy = serde_json::from_str("\"update\"").unwrap();
    assert!(matches!(d, IndexStrategy::Update));

    let d: IndexStrategy = serde_json::from_str("\"replace\"").unwrap();
    assert!(matches!(d, IndexStrategy::Replace));
}

// ============================================================================
// JobState serialization
// ============================================================================

#[test]
fn test_job_state_serialization_round_trip() {
    let mut state = JobState::new("job-abc", "idx-1");
    state.start();
    state.pages_crawled = 42;
    state.account_id = Some("acct_test".to_string());
    state.swap_temp_index = Some("idx-1_tmp_job-abc1".to_string());

    let json = serde_json::to_string(&state).unwrap();
    let d: JobState = serde_json::from_str(&json).unwrap();

    assert_eq!(d.job_id, "job-abc");
    assert_eq!(d.index_uid, "idx-1");
    assert_eq!(d.pages_crawled, 42);
    assert_eq!(d.account_id, Some("acct_test".to_string()));
    assert_eq!(d.swap_temp_index, Some("idx-1_tmp_job-abc1".to_string()));
    assert!(matches!(d.status, JobStatus::Running));
}

#[test]
fn test_job_status_all_variants_serialize() {
    let variants = vec![
        JobStatus::Pending,
        JobStatus::Running,
        JobStatus::Completed,
        JobStatus::Failed,
        JobStatus::Cancelled,
        JobStatus::Paused,
    ];

    for v in variants {
        let json = serde_json::to_string(&v).unwrap();
        let d: JobStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(
            serde_json::to_string(&d).unwrap(),
            json,
            "Round-trip failed for {json}"
        );
    }
}
