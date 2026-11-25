//! Installed native worker CLI, release, and service-manager shutdown binding.

use std::{
    env,
    error::Error,
    fmt::{Display, Formatter},
    future, io,
};

use weather_edge::ForecastEvidenceMaterialCodec;
use weather_worker::{
    WorkerProcessArgumentsError, WorkerProcessOptions, parse_worker_process_arguments,
};

use crate::{
    NativeComposedWorkerError, NativeComposedWorkerResult, NativeProcessShutdown,
    NativeWorkerTaskLimits, run_composed_native_worker,
};

const WORKER_RELEASE_ENVIRONMENT: &str = "WEATHER_RELEASE";

/// Installed CLI, release selection, signal registration, or worker lifecycle failure.
#[derive(Debug)]
pub enum NativeInstalledWorkerProcessError {
    Arguments(WorkerProcessArgumentsError),
    Release,
    SignalRegistration(io::Error),
    Worker(NativeComposedWorkerError),
}

impl Display for NativeInstalledWorkerProcessError {
    /// Render only the source diagnostic or its bounded owning process phase.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Arguments(error) => Display::fmt(error, formatter),
            Self::Release => formatter.write_str("WEATHER_RELEASE must be non-empty and bounded"),
            Self::SignalRegistration(_) => {
                formatter.write_str("worker shutdown signal registration failed")
            }
            Self::Worker(NativeComposedWorkerError::Startup(error)) => {
                Display::fmt(error, formatter)
            }
            Self::Worker(NativeComposedWorkerError::Running(error)) => {
                Display::fmt(error, formatter)
            }
        }
    }
}

impl Error for NativeInstalledWorkerProcessError {
    /// Preserve typed local causes without exposing configuration or provider material.
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Arguments(error) => Some(error),
            Self::Release => None,
            Self::SignalRegistration(error) => Some(error),
            Self::Worker(NativeComposedWorkerError::Startup(error)) => Some(error),
            Self::Worker(NativeComposedWorkerError::Running(error)) => Some(error),
        }
    }
}

/// Run the current installed worker with one explicitly admitted evidence codec.
pub async fn run_native_installed_worker<Codec>(
    codec: Codec,
    limits: NativeWorkerTaskLimits,
) -> Result<NativeComposedWorkerResult, NativeInstalledWorkerProcessError>
where
    Codec: ForecastEvidenceMaterialCodec,
{
    let options = current_process_options()?;
    let release = current_process_release()?;
    // preserve one-shot startup without installing retained process signal handlers
    if options.once {
        return run_composed_native_worker(release, codec, limits, options, future::pending())
            .await
            .map_err(NativeInstalledWorkerProcessError::Worker);
    }
    // subscribe before provider, pool, or scheduler startup so service-manager shutdown is retained
    let shutdown = NativeProcessShutdown::register()
        .map_err(NativeInstalledWorkerProcessError::SignalRegistration)?;
    run_composed_native_worker(release, codec, limits, options, shutdown.wait())
        .await
        .map_err(NativeInstalledWorkerProcessError::Worker)
}

/// Decode the source entrypoint's current arguments through its existing exact parser.
fn current_process_options() -> Result<WorkerProcessOptions, NativeInstalledWorkerProcessError> {
    let arguments = env::args_os()
        .skip(1)
        .map(|argument| argument.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    parse_worker_process_arguments(&arguments).map_err(NativeInstalledWorkerProcessError::Arguments)
}

/// Select and admit the scheduler release exactly like the full process configuration.
fn current_process_release() -> Result<String, NativeInstalledWorkerProcessError> {
    let release =
        env::var_os(WORKER_RELEASE_ENVIRONMENT).map(|value| value.to_string_lossy().into_owned());
    admit_process_release(release)
}

/// Preserve the original default, trim admission, and UTF-16 heartbeat bound.
fn admit_process_release(
    release: Option<String>,
) -> Result<String, NativeInstalledWorkerProcessError> {
    crate::worker_configuration::admit_worker_release(release)
        .map_err(|_| NativeInstalledWorkerProcessError::Release)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reuse the source parser's absent, duplicate once, and unknown-control behavior.
    #[test]
    fn installed_arguments_preserve_original_once_grammar() {
        assert_eq!(
            parse_worker_process_arguments(&[]),
            Ok(WorkerProcessOptions { once: false })
        );
        assert_eq!(
            parse_worker_process_arguments(&["--once".to_owned(), "--once".to_owned()]),
            Ok(WorkerProcessOptions { once: true })
        );
        assert_eq!(
            parse_worker_process_arguments(&["--watch".to_owned()]),
            Err(WorkerProcessArgumentsError)
        );
    }

    /// Preserve the original default and exact UTF-16 release admission.
    #[test]
    fn installed_release_matches_worker_environment_contract() {
        assert_eq!(
            admit_process_release(None).expect("admit default worker release"),
            "development"
        );
        assert_eq!(
            admit_process_release(Some(" release-1 ".to_owned()))
                .expect("admit bounded configured release"),
            " release-1 "
        );
        assert!(matches!(
            admit_process_release(Some(" ".to_owned())),
            Err(NativeInstalledWorkerProcessError::Release)
        ));
        assert!(matches!(
            admit_process_release(Some("\u{feff}".to_owned())),
            Err(NativeInstalledWorkerProcessError::Release)
        ));
        assert!(matches!(
            admit_process_release(Some("x".repeat(129))),
            Err(NativeInstalledWorkerProcessError::Release)
        ));
    }
}
