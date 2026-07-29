use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use rayon::prelude::*;

use crate::candidate;

use super::{BackendOutcome, ProgressReporter, SEARCH_CHUNK_SIZE, SearchConfig, SearchHit};

pub fn search(config: &SearchConfig, progress: &ProgressReporter) -> BackendOutcome {
    let chunk_count = config.max_index.div_ceil(SEARCH_CHUNK_SIZE);
    let processed = AtomicU64::new(0);
    let stopped = AtomicBool::new(false);

    let hit = (0..chunk_count).into_par_iter().find_map_any(|chunk| {
        if stopped.load(Ordering::Acquire) {
            return None;
        }

        let start = chunk * SEARCH_CHUNK_SIZE;
        let end = (start + SEARCH_CHUNK_SIZE).min(config.max_index);
        let mut local_processed = 0u64;

        for index in start..end {
            if stopped.load(Ordering::Acquire) {
                break;
            }

            local_processed += 1;
            let hash = candidate::digest(index, config.candidate);
            if candidate::matches_leading_zero_bits(
                candidate::digest_word(&hash),
                config.leading_zero_bits,
            ) {
                if !stopped.swap(true, Ordering::AcqRel) {
                    processed.fetch_add(local_processed, Ordering::AcqRel);
                    progress(local_processed);
                    return Some(SearchHit { index });
                }
                break;
            }
        }

        processed.fetch_add(local_processed, Ordering::AcqRel);
        progress(local_processed);
        None
    });

    BackendOutcome {
        hit,
        processed: processed.load(Ordering::Acquire),
        backend_name: "scalar".to_owned(),
        device_name: None,
        fallback_reasons: Vec::new(),
    }
}

#[cfg(test)]
mod test;
