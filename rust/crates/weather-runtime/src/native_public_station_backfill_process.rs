//! Standalone installed public-station backfill over the genuine worker configuration and database.

use std::{io::Write, sync::Arc};

use weather_database::to_pool_configuration;
use weather_worker::{
    HistoricalBackfillArguments, HistoricalBackfillCommand, HistoricalBackfillConfigurationFamily,
    HistoricalBackfillReport, HistoricalBackfillSourceConfiguration, TaskFailure,
    WorkerProcessEnvironmentPort, execute_public_station_archive_backfill,
    plan_public_station_backfill_dry_run,
};

use crate::{
    DatabaseAmbientBackfillRepository, HistoricalBackfillReportFileMode,
    NativeAmbientWeatherTransport, NativeHistoricalBackfillOutput, NativeOutboundHttp,
    NativePostgresPool, NativePublicBackfillTime, NativeScheduledValues, NativeWorkerBackfillOwner,
    PublicStationBackfillPlanningRepository, historical_backfill_process::finish_report,
    native_worker_configuration_loader,
    public_station_archive_binding::bind_public_station_archive,
};

/// Execute one admitted native public-station historical command with real process ownership.
pub fn run_native_public_station_backfill(
    command: HistoricalBackfillCommand,
    output: impl Write,
) -> Result<u8, TaskFailure> {
    // retain the exact parsed family instead of accepting another backfill's configuration
    let HistoricalBackfillArguments::PublicStations(arguments) = command.arguments else {
        return Err(failure("historical_backfill_configuration_mismatch"));
    };
    if command.source_configuration
        != HistoricalBackfillSourceConfiguration::Required(
            HistoricalBackfillConfigurationFamily::PublicStations,
        )
    {
        return Err(failure("historical_backfill_configuration_mismatch"));
    }
    let configuration = native_worker_configuration_loader()
        .load_configuration()
        .map_err(|_| failure("worker_configuration_failed"))?;
    let catalog = configuration
        .public_stations
        .as_ref()
        .ok_or_else(|| failure("public_station_configuration_required"))?;
    let pool = NativePostgresPool::new(to_pool_configuration(&configuration.database))
        .map_err(|_| failure("public_station_backfill_pool_failed"))?;
    let mut repository = DatabaseAmbientBackfillRepository::new(pool, NativeScheduledValues::new());
    let mut output = NativeHistoricalBackfillOutput::new(output);
    // retain the process pool through planning, execution and optional report publication
    let execution = (|| {
        let planning =
            repository.resolve_public_station_planning_sources(catalog, &arguments.source_keys)?;
        let mut time = NativePublicBackfillTime::default();
        // dry runs never construct an outbound HTTP owner or acquire a source session
        let report = if arguments.dry_run {
            plan_public_station_backfill_dry_run(
                &arguments,
                &catalog.siteKey,
                &planning,
                &mut repository,
                &mut time,
            )?
        } else {
            let client = Arc::new(
                NativeOutboundHttp::new()
                    .map_err(|_| failure("public_station_backfill_provider_failed"))?,
            );
            let ambient = NativeAmbientWeatherTransport::with_client(Arc::clone(&client))
                .map_err(|_| failure("public_station_backfill_provider_failed"))?;
            let supplemental = NativeWorkerBackfillOwner::with_client(client)
                .supplemental_public_station()
                .map_err(|_| failure("public_station_backfill_provider_failed"))?;
            let (sources, mut provider) = bind_public_station_archive(
                catalog,
                planning,
                ambient,
                supplemental,
                |delay_ms| std::thread::sleep(std::time::Duration::from_millis(delay_ms)),
            )?;
            execute_public_station_archive_backfill(
                &arguments,
                &catalog.siteKey,
                &sources,
                &mut repository,
                &mut provider,
                &mut time,
            )?
        };
        finish_report(
            HistoricalBackfillReport::PublicStations(report),
            command.report_path.as_deref(),
            HistoricalBackfillReportFileMode::OwnerReadWrite,
            &mut output,
        )
    })();
    let (pool, _) = repository.into_parts();
    // the original finally-close failure overrides the retained operation result
    pool.close()
        .map_err(|_| failure("public_station_backfill_pool_close_failed"))?;
    execution
}

/// Keep process diagnostics categorical and free of provider material.
fn failure(code: &'static str) -> TaskFailure {
    TaskFailure {
        attempts: 0,
        code: code.to_owned(),
    }
}
