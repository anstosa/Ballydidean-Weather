//! Candidate-only installed native weather worker process.
//!
//! The installed owner retains the predecessor's `--once` grammar and signal/drain lifecycle.
//! The response ceiling is an explicit candidate transport bound rather than a final parity claim;
//! production adoption remains gated on the separately proved target-architecture gzip candidate.

use std::{env, io, process::ExitCode, time::Duration};

use weather_runtime::{
    NativeBackendRuntime, NativeForecastArchiveCodec, NativeWorkerTaskLimits,
    native_node_gzip_candidate, run_native_installed_worker,
};

/// Start the installed worker and report only its bounded owning diagnostic.
fn main() -> ExitCode {
    // keep diagnostics separate from retained task evidence and receipts
    match run() {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            eprintln!("weather-worker: {error}");
            ExitCode::FAILURE
        }
    }
}

/// Bind the proved candidate codec to the actual installed worker composition.
fn run() -> Result<u8, String> {
    let arguments = env::args().skip(1).collect::<Vec<_>>();
    // retain the original archive and forecast-anchor one-shot parser
    if arguments
        .first()
        .is_some_and(|argument| argument == "backfill")
    {
        return weather_runtime::run_native_open_meteo_backfill(&arguments[1..])
            .map_err(|error| error.to_string());
    }
    // dispatch standalone historical commands before the scheduled owner starts
    let kind = match arguments.first().map(String::as_str) {
        Some("public-stations-backfill") => {
            Some(weather_worker::HistoricalBackfillCommandKind::PublicStations)
        }
        Some("tide-backfill") => Some(weather_worker::HistoricalBackfillCommandKind::Tide),
        Some("tempest-backfill") => Some(weather_worker::HistoricalBackfillCommandKind::Tempest),
        _ => None,
    };
    // preserve each command's original parser and native process owner
    if let Some(kind) = kind {
        let command = weather_worker::parse_historical_backfill_command(
            kind,
            &arguments[1..],
            &mut weather_runtime::NativePublicBackfillTime::default(),
        )
        .map_err(|error| format!("{error:?}"))?;
        let result = match kind {
            weather_worker::HistoricalBackfillCommandKind::PublicStations => {
                weather_runtime::run_native_public_station_backfill(command, io::stdout().lock())
            }
            weather_worker::HistoricalBackfillCommandKind::Tide => {
                weather_runtime::run_native_tide_backfill(command, io::stdout().lock())
            }
            weather_worker::HistoricalBackfillCommandKind::Tempest => {
                weather_runtime::run_native_tempest_backfill(command, io::stdout().lock())
            }
        };
        return result.map_err(|failure| failure.code);
    }
    let runtime = NativeBackendRuntime::new().map_err(|error| error.to_string())?;
    let codec =
        NativeForecastArchiveCodec::new(native_node_gzip_candidate as fn(&[u8]) -> Option<Vec<u8>>);
    let limits = NativeWorkerTaskLimits {
        archive_timeout: Duration::from_secs(30),
        maintenance_response_body_bytes: 64 * 1024,
    };
    runtime.block_on(async {
        // the installed owner selects one-shot versus signal-aware retained lifecycle exactly once
        run_native_installed_worker(codec, limits)
            .await
            .map_err(|error| error.to_string())?;
        Ok(0)
    })
}
