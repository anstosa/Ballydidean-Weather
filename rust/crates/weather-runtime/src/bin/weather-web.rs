//! Candidate-only installed native weather edge process.
//!
//! The explicit listener and body limits are candidate admission bounds rather than a claim of
//! parity with Node's implicit socket limits. Production adoption remains gated on the separately
//! proved target-architecture gzip candidate.

use std::{process::ExitCode, time::Duration};

use weather_runtime::{
    NativeBackendRuntime, NativeForecastArchiveCodec, NativeHttpListenerConfiguration,
    NativeInstalledEdgeLimits, NativeProcessShutdown, native_node_gzip_candidate,
    run_native_installed_edge,
};

/// Start the installed edge resources and report only bounded process diagnostics.
fn main() -> ExitCode {
    // keep diagnostics distinct from every public or private response body
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("weather-web: {error}");
            ExitCode::FAILURE
        }
    }
}

/// Retain installed startup, shutdown, and finite candidate admission limits.
fn run() -> Result<(), String> {
    let runtime = NativeBackendRuntime::new().map_err(|error| error.to_string())?;
    // share the same bounded listener policy across the public and optional private sockets
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
    let limits = NativeInstalledEdgeLimits {
        public: listener(64)?,
        private: listener(8)?,
        public_body_deadline: Duration::from_secs(60),
        private_body_deadline: Duration::from_secs(300),
    };
    let codec =
        NativeForecastArchiveCodec::new(native_node_gzip_candidate as fn(&[u8]) -> Option<Vec<u8>>);
    runtime.block_on(async {
        // register before resource loading so a boot-time signal still starts an orderly drain
        let shutdown = NativeProcessShutdown::register().map_err(|error| error.to_string())?;
        // leave process arguments untouched because the predecessor web entrypoint had no modes
        run_native_installed_edge(codec, limits, shutdown.wait())
            .await
            .map_err(|error| error.to_string())?;
        Ok(())
    })
}
