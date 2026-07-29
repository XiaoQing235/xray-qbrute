use std::sync::Arc;

use super::*;
use crate::candidate::CandidateConfig;

const CONFIG: CandidateConfig = CandidateConfig {
    commit: 0xeb366895,
    node_suffix: 0xebac62b9,
};

fn config() -> SearchConfig {
    SearchConfig {
        candidate: CONFIG,
        max_index: 256,
        leading_zero_bits: 0,
    }
}

#[test]
fn scalar_selection_is_explicit() {
    let progress: ProgressReporter = Arc::new(|_| {});
    let outcome = search(&config(), BackendKind::Scalar, progress).expect("scalar works");
    assert_eq!(outcome.backend_name, "scalar");
    assert!(outcome.fallback_reasons.is_empty());
}

#[test]
fn invalid_range_is_rejected_before_dispatch() {
    let progress: ProgressReporter = Arc::new(|_| {});
    let mut invalid = config();
    invalid.max_index = MAX_UUID_INDEX + 1;
    let error = search(&invalid, BackendKind::Scalar, progress).expect_err("range is invalid");
    assert!(matches!(error, SearchError::InvalidConfig(_)));
}
