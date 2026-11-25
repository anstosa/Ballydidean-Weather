//! native one-shot worker database readiness and durable heartbeat health command

use std::{io, process::ExitCode};

use weather_runtime::run_native_worker_health_process;

/// run one health command and keep stdout reserved for its json line
fn main() -> ExitCode {
    let output = io::stdout().lock();
    // preserve the original ready zero and unhealthy or failed one statuses
    match run_native_worker_health_process(output) {
        Ok(status) => ExitCode::from(status),
        Err(error) => {
            eprintln!("weather-worker-health: {error}");
            ExitCode::FAILURE
        }
    }
}
