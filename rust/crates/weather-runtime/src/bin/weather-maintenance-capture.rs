//! Candidate installed entrypoint for the custody-only maintenance capture job.

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod linux {
    use std::env;
    use std::io::{self, Write};
    use std::process::ExitCode;

    use weather_runtime::{
        NativeBackendRuntime, NativeMaintenanceCaptureCycleHost,
        NativeMaintenanceCaptureJobCommand, NativeMaintenanceControllerHost,
        NativeMaintenanceControllerHostError, NativeMaintenanceDueMode,
        NativeMaintenanceInstalledActions, NativeMaintenanceInstalledDuePorts,
        encode_native_maintenance_capture_readiness, encode_native_maintenance_controller_run,
        inspect_native_maintenance_installed_host_readiness,
        parse_native_maintenance_capture_job_arguments, parse_native_maintenance_controller_mode,
        run_native_maintenance_capture_job, run_native_maintenance_controller,
    };

    /// Dispatch only the predecessor's normal service or exact readiness inspection mode.
    pub fn main() -> ExitCode {
        // keep diagnostics separate from the readiness JSON and all retained evidence bytes
        match run() {
            Ok(code) => code,
            Err(error) => {
                eprintln!("weather-maintenance-capture: {error}");
                ExitCode::FAILURE
            }
        }
    }

    /// Parse the closed argument set before selecting a read-only or mutating owner.
    fn run() -> Result<ExitCode, String> {
        let arguments = env::args_os().skip(1).collect::<Vec<_>>();
        // preserve the separate predecessor one-shot daily/monthly controller modes in the admitted binary
        if arguments.len() == 1
            && let Some(mode) = arguments[0].to_str()
            && matches!(mode, "--daily" | "--monthly")
        {
            let mode = parse_native_maintenance_controller_mode(&[mode.to_owned()])
                .map_err(|_| "maintenance controller requires exactly one mode".to_owned())?;
            return run_controller(mode);
        }
        let command = parse_native_maintenance_capture_job_arguments(&arguments)
            .map_err(|error| error.to_string())?;
        // preserve readiness as a read-only fail-closed inspection independent of service admission
        match command {
            NativeMaintenanceCaptureJobCommand::Readiness => run_readiness(),
            NativeMaintenanceCaptureJobCommand::Run => run_service(),
        }
    }

    /// Emit the exact predecessor JSON response and its distinct not-ready exit status.
    fn run_readiness() -> Result<ExitCode, String> {
        let readiness = inspect_native_maintenance_installed_host_readiness();
        let bytes = encode_native_maintenance_capture_readiness(&readiness)
            .ok_or_else(|| "maintenance capture readiness encoding failed".to_owned())?;
        io::stdout()
            .lock()
            .write_all(&bytes)
            .map_err(|_| "maintenance capture readiness output failed".to_owned())?;
        // match the source's exit 3 installation gate without changing the response document
        Ok(if readiness.ready {
            ExitCode::SUCCESS
        } else {
            ExitCode::from(3)
        })
    }

    /// Execute the native one-shot controller with its actual fixed installed action owners.
    fn run_controller(mode: NativeMaintenanceDueMode) -> Result<ExitCode, String> {
        let mut host =
            NativeMaintenanceControllerHost::for_current_installed_process(|root, commit| {
                let actions = NativeMaintenanceInstalledActions::bind_installed(root, commit)
                    .map_err(|_| NativeMaintenanceControllerHostError::Installation)?;
                NativeMaintenanceInstalledDuePorts::bind_installed(root, commit, actions)
                    .map_err(|_| NativeMaintenanceControllerHostError::Installation)
            })
            .map_err(|error| format!("maintenance controller installation: {error:?}"))?;
        let runtime = NativeBackendRuntime::new().map_err(|error| error.to_string())?;
        runtime.block_on(async {
            let result = run_native_maintenance_controller(&mut host, mode)
                .await
                .map_err(|error| format!("maintenance controller: {error:?}"))?;
            let bytes = encode_native_maintenance_controller_run(&result)
                .ok_or_else(|| "maintenance controller run encoding failed".to_owned())?;
            io::stdout()
                .lock()
                .write_all(&bytes)
                .map_err(|_| "maintenance controller output failed".to_owned())?;
            Ok(ExitCode::SUCCESS)
        })
    }

    /// Admit the literal installed process before registering signals or entering the capture loop.
    fn run_service() -> Result<ExitCode, String> {
        let mut host = current_installed_owner()?;
        let runtime = NativeBackendRuntime::new().map_err(|error| error.to_string())?;
        runtime.block_on(async {
            // the retained job owns SIGINT/SIGTERM and never cancels an admitted locked cycle
            run_native_maintenance_capture_job(&mut host)
                .await
                .map_err(|error| error.to_string())?;
            Ok(ExitCode::SUCCESS)
        })
    }

    /// Construct only the fixed current installed owner, never a caller-selected release path.
    fn current_installed_owner() -> Result<NativeMaintenanceCaptureCycleHost, String> {
        NativeMaintenanceCaptureCycleHost::for_current_installed_process()
            .map_err(|error| error.to_string())
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// Refuse an ordinary test binary before any service signal or host mutation can begin.
        #[test]
        fn non_installed_candidate_cannot_construct_service_owner() {
            assert!(current_installed_owner().is_err());
        }

        /// The exact installed controller entrypoint refuses an ordinary build before any owner mutation.
        #[test]
        fn non_installed_candidate_cannot_construct_controller_owner() {
            assert!(run_controller(NativeMaintenanceDueMode::Daily).is_err());
            assert!(run_controller(NativeMaintenanceDueMode::Monthly).is_err());
        }
    }
}

/// Dispatch the installed native Linux capture owner.
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
fn main() -> std::process::ExitCode {
    linux::main()
}

/// Refuse unsupported native targets explicitly.
#[cfg(not(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
)))]
fn main() -> std::process::ExitCode {
    eprintln!("weather-maintenance-capture: unavailable on this target");
    std::process::ExitCode::FAILURE
}
