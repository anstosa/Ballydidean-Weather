//! Standalone Tempest backfill over the genuine mounted configuration and database.

use std::io::Write;

use weather_contracts::TempestConfiguration;
use weather_database::{
    admit_supported_postgres_version_num, postgres_server_version_read_plan, to_pool_configuration,
};
use weather_worker::{
    HistoricalBackfillArguments, HistoricalBackfillCommand, HistoricalBackfillConfigurationFamily,
    HistoricalBackfillReport, HistoricalBackfillSourceConfiguration, TaskFailure,
    TempestBackfillDate, TempestBackfillSource, TempestScheduledSource,
    WorkerProcessEnvironmentPort, execute_tempest_bulk_backfill,
};

use crate::{
    DatabaseTempestBackfillRepository, HistoricalBackfillReportFileMode,
    NativeHistoricalBackfillOutput, NativePostgresPool, NativePublicBackfillTime,
    NativeRuntimeMaterial, NativeScheduledValues, NativeWorkerBackfillOwner,
    WorkerSourceMaterialIdentity,
    historical_backfill_process::finish_report,
    native_postgres_weather_rows::json,
    native_tide_backfill_process::{history_midnight, read_station_rows, text},
    native_worker_configuration_loader,
    worker_tempest_dispatch::project_tempest_runtime_configurations,
};

/// Execute one installed Tempest historical command with original provider and database ownership.
pub fn run_native_tempest_backfill(
    command: HistoricalBackfillCommand,
    output: impl Write,
) -> Result<u8, TaskFailure> {
    // require the exact parsed family before entering the actual process
    let HistoricalBackfillArguments::Tempest(arguments) = command.arguments else {
        return Err(failure("historical_backfill_configuration_mismatch"));
    };
    if command.source_configuration
        != HistoricalBackfillSourceConfiguration::Required(
            HistoricalBackfillConfigurationFamily::Tempest,
        )
    {
        return Err(failure("historical_backfill_configuration_mismatch"));
    }
    let configuration = native_worker_configuration_loader()
        .load_configuration()
        .map_err(|_| failure("worker_configuration_failed"))?;
    let catalog = configuration
        .tempest
        .as_ref()
        .ok_or_else(|| failure("tempest_configuration_required"))?;
    let credential = configuration
        .tempest_api_key
        .clone()
        .ok_or_else(|| failure("tempest_configuration_required"))?;
    let pool = NativePostgresPool::new(to_pool_configuration(&configuration.database))
        .map_err(|_| failure("tempest_backfill_pool_failed"))?;
    let mut repository =
        DatabaseTempestBackfillRepository::new(pool.clone(), NativeScheduledValues::new());
    // close the pool even when source admission, provider verification or report publication fails
    let result = (|| {
        assert_supported_database(&pool)?;
        let sources = resolve_sources(&pool, catalog, &arguments.station_ids)?;
        let mut provider = NativeWorkerBackfillOwner::new()
            .and_then(|owner| owner.tempest(credential))
            .map_err(|_| failure("tempest_backfill_provider_failed"))?;
        // validate every live ST device before the first committed historical chunk
        if !arguments.dry_run {
            // preserve sequential provider verification and refuse physical replacement
            for source in &sources {
                provider.verify_station(
                    source.configuration.location_id,
                    source.configuration.device_id,
                    &source.configuration.serial,
                )?;
            }
        }
        let report = execute_tempest_bulk_backfill(
            &arguments,
            &catalog.siteKey,
            &sources,
            &mut repository,
            &mut provider,
            &mut NativePublicBackfillTime::default(),
        )?;
        finish_report(
            HistoricalBackfillReport::Tempest(report),
            command.report_path.as_deref(),
            HistoricalBackfillReportFileMode::OwnerReadWrite,
            &mut NativeHistoricalBackfillOutput::new(output),
        )
    })();
    drop(repository);
    // a finally-close error overrides the operation outcome
    pool.close()
        .map_err(|_| failure("tempest_backfill_pool_close_failed"))?;
    result
}

/// Preserve the predecessor's PostgreSQL feature-floor read before history work.
pub(crate) fn assert_supported_database(pool: &NativePostgresPool) -> Result<(), TaskFailure> {
    let mut session = pool
        .checkout()
        .map_err(|_| failure("tempest_backfill_version_read"))?;
    let rows = session
        .query(postgres_server_version_read_plan().sql, &[])
        .map_err(|_| failure("tempest_backfill_version_read"))?;
    session
        .release()
        .map_err(|_| failure("tempest_backfill_version_read"))?;
    let version: Option<String> = rows
        .first()
        .map(|row| row.try_get("server_version_num"))
        .transpose()
        .map_err(|_| failure("tempest_backfill_version_read"))?;
    admit_supported_postgres_version_num(version.as_deref())
        .map(|_| ())
        .map_err(|_| failure("tempest_backfill_unsupported_postgres"))
}

/// Resolve all active station selectors against the original independent database relation.
fn resolve_sources(
    pool: &NativePostgresPool,
    catalog: &TempestConfiguration,
    requested: &[u64],
) -> Result<Vec<TempestBackfillSource>, TaskFailure> {
    let mut material = NativeRuntimeMaterial;
    let configured = project_tempest_runtime_configurations(catalog, &mut material)?
        .into_iter()
        .zip(&catalog.stations)
        .filter(|(_, station)| {
            station.active && (requested.is_empty() || requested.contains(&station.locationId))
        })
        .collect::<Vec<_>>();
    // require every explicit selector to resolve
    if !requested.is_empty() && configured.len() != requested.len() {
        return Err(failure("tempest_backfill_source_unavailable"));
    }
    let rows = read_station_rows(
        pool,
        &catalog.siteKey,
        configured
            .iter()
            .map(|(configuration, _)| configuration.source_key.clone())
            .collect(),
    )?;
    let mut result = Vec::with_capacity(configured.len());
    // preserve configuration order and compare every independent durable field
    for (configuration, station) in configured {
        let row = rows
            .iter()
            .find(|row| {
                text(row, "source_key").is_ok_and(|value| value == configuration.source_key)
            })
            .ok_or_else(|| failure("tempest_backfill_source_unavailable"))?;
        let stored = json(row, "material_provider_config")
            .map_err(|_| failure("tempest_backfill_source_identity"))?
            .ok_or_else(|| failure("tempest_backfill_source_identity"))?;
        let identity = material.material_identity(&stored)?;
        // use the original immutable provider, station, physical type and material checks
        if text(row, "source_kind")? != "physical_sensor"
            || text(row, "site_slug")? != configuration.site_slug
            || text(row, "station_slug")? != configuration.station_slug
            || text(row, "provider_key")? != configuration.provider_key
            || text(row, "source_config_fingerprint")? != configuration.source_config_fingerprint
            || text(row, "timezone")? != configuration.timezone
            || identity != configuration.material_config_identity
        {
            return Err(failure("tempest_backfill_source_identity"));
        }
        let source = TempestScheduledSource {
            active: true,
            cadence_seconds: station.cadenceSeconds,
            id: text(row, "source_id")?,
            material_config_identity: identity,
            provider_key: configuration.provider_key.clone(),
            site_slug: configuration.site_slug.clone(),
            source_config_fingerprint: configuration.source_config_fingerprint.clone(),
            source_key: configuration.source_key.clone(),
            source_kind: configuration.source_kind,
            station_slug: configuration.station_slug.clone(),
            timezone: configuration.timezone.clone(),
        };
        result.push(TempestBackfillSource {
            configuration,
            history_start: TempestBackfillDate::admitted(
                &station.historyStartDate,
                history_midnight(&station.historyStartDate)?,
            ),
            provider_active: true,
            station_active: true,
            storage_active: true,
            source,
        });
    }
    Ok(result)
}

/// Keep standalone diagnostics categorical and bounded.
fn failure(code: &'static str) -> TaskFailure {
    TaskFailure {
        attempts: 0,
        code: code.to_owned(),
    }
}
