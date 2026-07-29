use std::sync::Arc;

use super::*;
use crate::candidate;

const CONFIG: CandidateConfig = CandidateConfig {
    commit: 0xeb366895,
    node_suffix: 0xebac62b9,
};
const ALTERNATE_CONFIG: CandidateConfig = CandidateConfig {
    commit: 0x01234567,
    node_suffix: 0x89abcdef,
};

#[cfg(target_os = "windows")]
const TEST_BACKENDS: &[WgpuBackend] = &[WgpuBackend::Vulkan];

#[cfg(target_os = "macos")]
const TEST_BACKENDS: &[WgpuBackend] = &[WgpuBackend::Metal];

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
const TEST_BACKENDS: &[WgpuBackend] = &[WgpuBackend::Vulkan];

fn context_or_skip(backend: WgpuBackend, config: SearchConfig) -> Option<GpuContext> {
    match create_context(backend, config) {
        Ok(context) => Some(context),
        Err(SearchError::Unavailable(reason)) => {
            eprintln!("Skipping {} GPU test: {reason}", backend.name());
            None
        }
        Err(error) => panic!("GPU initialization failed unexpectedly: {error}"),
    }
}

#[test]
fn gpu_hashes_match_scalar() {
    for &backend in TEST_BACKENDS {
        for candidate_config in [CONFIG, ALTERNATE_CONFIG] {
            let search_config = SearchConfig {
                candidate: candidate_config,
                max_index: 256,
                leading_zero_bits: 64,
            };
            let Some(context) = context_or_skip(backend, search_config) else {
                continue;
            };

            let bases = [0, 1 << 28, 1 << 30, 1 << 42, (1 << 58) - 256];
            for base in bases {
                let actual = context
                    .hash_first_words(base, 256)
                    .expect("GPU hash readback must succeed");
                assert_eq!(actual.len(), 256);
                for (offset, &actual_word) in actual.iter().enumerate() {
                    let index = base + offset as u64;
                    let expected =
                        candidate::digest_word(&candidate::digest(index, candidate_config));
                    assert_eq!(actual_word, expected, "{} index {index}", backend.name());
                }
            }
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

    for &backend in TEST_BACKENDS {
        let outcome = match search(&config, backend, &progress) {
            Ok(outcome) => outcome,
            Err(SearchError::Unavailable(reason)) => {
                eprintln!("Skipping {} GPU test: {reason}", backend.name());
                continue;
            }
            Err(error) => panic!("{} GPU search failed unexpectedly: {error}", backend.name()),
        };
        assert_eq!(outcome.backend_name, backend.name());
        let hit = outcome
            .hit
            .expect("8-bit predicate should hit in 65536 candidates");
        assert!(hit.index < config.max_index);
        assert!(outcome.processed > 0);
        assert!(outcome.processed <= config.max_index);
        let word = candidate::digest_word(&candidate::digest(hit.index, CONFIG));
        assert!(candidate::matches_leading_zero_bits(
            word,
            config.leading_zero_bits
        ));
    }
}
