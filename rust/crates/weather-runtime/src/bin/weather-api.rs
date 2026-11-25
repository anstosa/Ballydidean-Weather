//! Native public and optional private weather API process.

use std::{env, process::ExitCode, time::Duration};

use weather_runtime::{
    NativeBackendRuntime, NativeHttpListenerConfiguration, NativeInstalledApiLimits,
    NativeInstalledApiProcessError, NativeProcessShutdown, run_native_api_schema_readiness,
    run_native_installed_api,
};

/// Start installed resources and consume exactly one service-manager shutdown signal.
fn main() -> ExitCode {
    // keep diagnostics distinct from any API response bytes
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("weather-api: {error}");
            ExitCode::FAILURE
        }
    }
}

/// Retain bounded candidate limits within the original service's 45-second stop grace.
fn run() -> Result<(), String> {
    let arguments = env::args_os().skip(1).collect::<Vec<_>>();
    // preserve ordinary zero-argument startup and admit only the explicit read-only readiness mode
    if arguments.len() == 1 && arguments[0].to_str() == Some("--readiness") {
        let receipt = run_native_api_schema_readiness().map_err(|error| error.to_string())?;
        // retain the predecessor's fixed success receipt without configured material
        let _migration_version = receipt.migration_version;
        println!("read-only migration readiness passed");
        return Ok(());
    }
    if !arguments.is_empty() {
        return Err("usage: weather-api [--readiness]".to_owned());
    }
    run_service()
}

/// Start the ordinary installed API process without changing its service lifecycle.
fn run_service() -> Result<(), String> {
    let runtime = NativeBackendRuntime::new().map_err(|error| error.to_string())?;
    let listener = |connections| {
        NativeHttpListenerConfiguration::new(
            connections,
            Duration::from_secs(60),
            Duration::from_secs(300),
            Duration::from_secs(35),
            64 * 1024,
            true,
        )
        .map_err(|error| error.to_string())
    };
    let limits = NativeInstalledApiLimits {
        public: listener(64)?,
        maintenance: listener(8)?,
        archive_timeout: Duration::from_secs(30),
        maintenance_body_deadline: Duration::from_secs(300),
    };
    runtime.block_on(async {
        // register before startup so a signal during boot is retained for orderly shutdown
        let shutdown = NativeProcessShutdown::register().map_err(|error| error.to_string())?;
        run_native_installed_api(limits, shutdown.wait())
            .await
            .map_err(|error| {
                // keep the owning process stage without printing configured material
                match error {
                    NativeInstalledApiProcessError::Models(_) => {
                        "installed model initialization failed"
                    }
                    NativeInstalledApiProcessError::Startup(_) => {
                        "API configuration or listener startup failed"
                    }
                    NativeInstalledApiProcessError::Serve(_) => "API listener or drain failed",
                }
                .to_owned()
            })?;
        Ok(())
    })
}
