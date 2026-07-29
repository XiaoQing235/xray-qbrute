use super::{BackendOutcome, ProgressReporter, SearchConfig, SearchError};

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
pub fn is_available() -> bool {
    is_x86_feature_detected!("avx2")
}

#[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
pub fn is_available() -> bool {
    false
}

pub fn search(
    config: &SearchConfig,
    progress: &ProgressReporter,
) -> Result<BackendOutcome, SearchError> {
    if !is_available() {
        return Err(SearchError::Unavailable(
            "AVX2 is not supported by this CPU".to_owned(),
        ));
    }

    let mut outcome = super::scalar::search(config, progress);
    outcome.backend_name = "rustcrypto-avx2".to_owned();
    Ok(outcome)
}

#[cfg(test)]
mod test;
