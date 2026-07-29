use super::*;
use crate::backend::SearchHit;
use crate::candidate::CandidateConfig;

const CONFIG: CandidateConfig = CandidateConfig {
    commit: 0xeb366895,
    node_suffix: 0xebac62b9,
};

#[test]
fn rustcrypto_avx2_search_handles_scalar_tail() {
    if !is_available() {
        return;
    }

    let progress: ProgressReporter = std::sync::Arc::new(|_| {});
    let config = SearchConfig {
        candidate: CONFIG,
        max_index: 3,
        leading_zero_bits: 0,
    };
    let outcome = search(&config, &progress).expect("AVX2 must be available");
    assert_eq!(outcome.hit, Some(SearchHit { index: 0 }));
    assert_eq!(outcome.processed, 1);
    assert_eq!(outcome.backend_name, "rustcrypto-avx2");
}
