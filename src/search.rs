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

enum PreparedBackend {
    Scalar,
    Avx2,
    Wgpu {
        kind: BackendKind,
        session: Box<backend::wgpu::WgpuSearchSession>,
    },
}

pub struct PreparedSearch {
    backends: Vec<PreparedBackend>,
    fallback_reasons: Vec<String>,
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
    prepare(config, backend)?.search(config, &progress)
}

pub fn prepare(
    config: &SearchConfig,
    requested: BackendKind,
) -> Result<PreparedSearch, SearchError> {
    validate_config(config)?;

    if requested == BackendKind::Auto {
        return prepare_auto(config);
    }

    Ok(PreparedSearch {
        backends: vec![prepare_backend(config, requested)?],
        fallback_reasons: Vec::new(),
    })
}

impl PreparedSearch {
    pub fn search(
        &self,
        config: &SearchConfig,
        progress: &ProgressReporter,
    ) -> Result<BackendOutcome, SearchError> {
        validate_config(config)?;
        let mut fallback_reasons = self.fallback_reasons.clone();

        for (position, backend) in self.backends.iter().enumerate() {
            let outcome = match backend {
                PreparedBackend::Scalar => Ok(backend::scalar::search(config, progress)),
                PreparedBackend::Avx2 => backend::avx2::search(config, progress),
                PreparedBackend::Wgpu { session, .. } => session.search(config, progress),
            };

            match outcome {
                Ok(mut outcome) => {
                    outcome.fallback_reasons = fallback_reasons;
                    return Ok(outcome);
                }
                Err(SearchError::Unavailable(reason)) if position + 1 < self.backends.len() => {
                    fallback_reasons.push(format!("{}: {reason}", backend.kind()));
                }
                Err(error) => return Err(error),
            }
        }

        unreachable!("prepared search always contains at least one backend")
    }
}

impl PreparedBackend {
    fn kind(&self) -> BackendKind {
        match self {
            Self::Scalar => BackendKind::Scalar,
            Self::Avx2 => BackendKind::Avx2,
            Self::Wgpu { kind, .. } => *kind,
        }
    }
}

fn prepare_auto(config: &SearchConfig) -> Result<PreparedSearch, SearchError> {
    let mut fallback_reasons = Vec::new();
    let mut backends = Vec::new();
    for &backend in AUTO_BACKENDS {
        match prepare_backend(config, backend) {
            Ok(prepared) => backends.push(prepared),
            Err(SearchError::Unavailable(reason)) => {
                fallback_reasons.push(format!("{backend}: {reason}"));
            }
            Err(error) => return Err(error),
        }
    }

    if backends.is_empty() {
        return Err(SearchError::Runtime(
            "auto backend selection exhausted without a usable backend".to_owned(),
        ));
    }

    Ok(PreparedSearch {
        backends,
        fallback_reasons,
    })
}

fn prepare_backend(
    config: &SearchConfig,
    backend: BackendKind,
) -> Result<PreparedBackend, SearchError> {
    match backend {
        BackendKind::Auto => unreachable!("auto is expanded before backend preparation"),
        BackendKind::Scalar => {
            warm_up_cpu(config, BackendKind::Scalar)?;
            Ok(PreparedBackend::Scalar)
        }
        BackendKind::Avx2 => {
            if !backend::avx2::is_available() {
                return Err(SearchError::Unavailable(
                    "AVX2 is not supported by this CPU".to_owned(),
                ));
            }
            warm_up_cpu(config, BackendKind::Avx2)?;
            Ok(PreparedBackend::Avx2)
        }
        BackendKind::WgpuVulkan | BackendKind::WgpuMetal => {
            let selected = match backend {
                BackendKind::WgpuVulkan => backend::wgpu::WgpuBackend::Vulkan,
                BackendKind::WgpuMetal => backend::wgpu::WgpuBackend::Metal,
                _ => unreachable!("matched WGPU backend"),
            };
            let session = backend::wgpu::WgpuSearchSession::new(selected, *config)?;
            session.warm_up()?;
            Ok(PreparedBackend::Wgpu {
                kind: backend,
                session: Box::new(session),
            })
        }
    }
}

fn warm_up_cpu(config: &SearchConfig, backend: BackendKind) -> Result<(), SearchError> {
    let warm_up_config = SearchConfig {
        candidate: config.candidate,
        max_index: backend::SEARCH_CHUNK_SIZE,
        leading_zero_bits: 64,
    };
    let progress: ProgressReporter = std::sync::Arc::new(|_| {});

    match backend {
        BackendKind::Scalar => {
            backend::scalar::search(&warm_up_config, &progress);
            Ok(())
        }
        BackendKind::Avx2 => backend::avx2::search(&warm_up_config, &progress).map(|_| ()),
        _ => unreachable!("CPU warm-up only accepts CPU backends"),
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

#[cfg(test)]
mod test;
