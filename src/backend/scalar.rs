use rayon::prelude::*;

use crate::candidate;

use super::{BackendOutcome, ProgressReporter, SEARCH_CHUNK_SIZE, SearchConfig, SearchHit};

pub fn search(config: &SearchConfig, progress: &ProgressReporter) -> BackendOutcome {
    let chunk_count = config.max_index.div_ceil(SEARCH_CHUNK_SIZE);
    let mut evaluated = 0u64;

    for chunk in 0..chunk_count {
        let start = chunk * SEARCH_CHUNK_SIZE;
        let end = (start + SEARCH_CHUNK_SIZE).min(config.max_index);
        let chunk_size = end - start;

        let min_index: Option<u64> = (start..end)
            .into_par_iter()
            .filter_map(|i| {
                let hash = candidate::digest(i, config.candidate);
                if candidate::matches_leading_zero_bits(
                    candidate::digest_word(&hash),
                    config.leading_zero_bits,
                ) {
                    Some(i)
                } else {
                    None
                }
            })
            .min();

        evaluated += chunk_size;

        if let Some(index) = min_index {
            let processed = index + 1;
            progress(processed - start);
            return BackendOutcome {
                hit: Some(SearchHit { index }),
                processed,
                evaluated,
                backend_name: "scalar".to_owned(),
                device_name: None,
                fallback_reasons: Vec::new(),
                search_elapsed: None,
                kernel_config: None,
                tuning_elapsed: None,
            };
        }

        progress(chunk_size);
    }

    BackendOutcome {
        hit: None,
        processed: config.max_index,
        evaluated,
        backend_name: "scalar".to_owned(),
        device_name: None,
        fallback_reasons: Vec::new(),
        search_elapsed: None,
        kernel_config: None,
        tuning_elapsed: None,
    }
}

#[cfg(test)]
mod test;
