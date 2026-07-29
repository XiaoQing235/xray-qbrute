use std::sync::Arc;

use super::*;
use crate::candidate;

const CONFIG: CandidateConfig = CandidateConfig {
    commit: 0xeb366895,
    node_suffix: 0xebac62b9,
};

fn context_or_skip() -> Option<GpuContext> {
    match create_context() {
        Ok(context) => Some(context),
        Err(SearchError::Unavailable(reason)) => {
            eprintln!("Skipping GPU test: {reason}");
            None
        }
        Err(error) => panic!("GPU initialization failed unexpectedly: {error}"),
    }
}

#[test]
fn gpu_hashes_match_scalar() {
    let Some(context) = context_or_skip() else {
        return;
    };

    let bases = [0, 1 << 28, 1 << 30, 1 << 42, (1 << 58) - 256];
    for base in bases {
        let actual = context
            .hash_first_words(base, 256, CONFIG)
            .expect("GPU hash readback must succeed");
        assert_eq!(actual.len(), 256);
        for (offset, &actual_word) in actual.iter().enumerate() {
            let index = base + offset as u64;
            let expected = candidate::digest_word(&candidate::digest(index, CONFIG));
            assert_eq!(actual_word, expected, "index {index}");
        }
    }
}

#[test]
fn gpu_search_finds_valid_hit() {
    let progress: ProgressReporter = Arc::new(|_| {});
    let config = SearchConfig {
        candidate: CONFIG,
        max_index: 65_536,
        leading_zero_bits: 8,
    };

    let outcome = match search(&config, &progress) {
        Ok(outcome) => outcome,
        Err(SearchError::Unavailable(reason)) => {
            eprintln!("Skipping GPU test: {reason}");
            return;
        }
        Err(error) => panic!("GPU search failed unexpectedly: {error}"),
    };
    let hit = outcome
        .hit
        .expect("8-bit predicate should hit in 65536 candidates");
    assert!(hit.index < config.max_index);
    let word = candidate::digest_word(&candidate::digest(hit.index, CONFIG));
    assert!(candidate::matches_leading_zero_bits(
        word,
        config.leading_zero_bits
    ));
}
