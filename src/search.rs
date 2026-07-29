use clap::ValueEnum;

use crate::backend::{self, BackendOutcome, ProgressReporter, SearchConfig, SearchError};
use crate::candidate::MAX_UUID_INDEX;

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
#[value(rename_all = "kebab-case")]
pub enum BackendKind {
    Auto,
    Scalar,
    Avx2,
    WgpuVulkan,
    WgpuMetal,
}

impl std::fmt::Display for BackendKind {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Auto => "auto",
            Self::Scalar => "scalar",
            Self::Avx2 => "avx2",
            Self::WgpuVulkan => "wgpu-vulkan",
            Self::WgpuMetal => "wgpu-metal",
        })
    }
}

#[cfg(target_os = "windows")]
const AUTO_BACKENDS: &[BackendKind] = &[
    BackendKind::WgpuVulkan,
    BackendKind::Avx2,
    BackendKind::Scalar,
];

#[cfg(target_os = "macos")]
const AUTO_BACKENDS: &[BackendKind] = &[
    BackendKind::WgpuMetal,
    BackendKind::Avx2,
    BackendKind::Scalar,
];

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
const AUTO_BACKENDS: &[BackendKind] = &[
    BackendKind::WgpuVulkan,
    BackendKind::Avx2,
    BackendKind::Scalar,
];

pub fn search(
    config: &SearchConfig,
    backend: BackendKind,
    progress: ProgressReporter,
) -> Result<BackendOutcome, SearchError> {
    validate_config(config)?;

    match backend {
        BackendKind::Auto => search_auto(config, &progress),
        selected => {
            let mut outcome = run_backend(config, selected, &progress)?;
            outcome.fallback_reasons = Vec::new();
            Ok(outcome)
        }
    }
}

fn validate_config(config: &SearchConfig) -> Result<(), SearchError> {
    if config.max_index == 0 {
        return Err(SearchError::InvalidConfig(
            "max_index must be greater than zero".to_owned(),
        ));
    }
    if config.max_index > MAX_UUID_INDEX {
        return Err(SearchError::InvalidConfig(format!(
            "max_index must be at most {MAX_UUID_INDEX}"
        )));
    }
    if config.leading_zero_bits > 64 {
        return Err(SearchError::InvalidConfig(
            "leading_zero_bits must be between 0 and 64".to_owned(),
        ));
    }
    Ok(())
}

fn search_auto(
    config: &SearchConfig,
    progress: &ProgressReporter,
) -> Result<BackendOutcome, SearchError> {
    let mut fallback_reasons = Vec::new();
    for &backend in AUTO_BACKENDS {
        match run_backend(config, backend, progress) {
            Ok(mut outcome) => {
                outcome.fallback_reasons = fallback_reasons;
                return Ok(outcome);
            }
            Err(SearchError::Unavailable(reason)) => {
                fallback_reasons.push(format!("{backend}: {reason}"));
            }
            Err(error) => return Err(error),
        }
    }

    Err(SearchError::Runtime(
        "auto backend selection exhausted without a usable backend".to_owned(),
    ))
}

fn run_backend(
    config: &SearchConfig,
    backend: BackendKind,
    progress: &ProgressReporter,
) -> Result<BackendOutcome, SearchError> {
    match backend {
        BackendKind::Auto => unreachable!("auto is expanded before backend dispatch"),
        BackendKind::Scalar => Ok(backend::scalar::search(config, progress)),
        BackendKind::Avx2 => backend::avx2::search(config, progress),
        BackendKind::WgpuVulkan => {
            backend::wgpu::search(config, backend::wgpu::WgpuBackend::Vulkan, progress)
        }
        BackendKind::WgpuMetal => {
            backend::wgpu::search(config, backend::wgpu::WgpuBackend::Metal, progress)
        }
    }
}

#[cfg(test)]
mod test;
