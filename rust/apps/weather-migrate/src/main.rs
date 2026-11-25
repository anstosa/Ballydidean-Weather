//! native one-shot migration and generated catalog bootstrap entrypoint

use std::{env, io, process::ExitCode};

use weather_runtime::run_native_migration_process;

/// run one process command and keep stdout reserved for the completion receipt
fn main() -> ExitCode {
    let mut output = io::stdout().lock();
    // convert one typed process outcome into the ordinary command status
    match run_native_migration_process(env::args_os().skip(1), &mut output) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("weather-migrate: {error}");
            ExitCode::FAILURE
        }
    }
}
