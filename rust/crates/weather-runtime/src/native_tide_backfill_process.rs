//! Standalone station archive backfills over the native configuration and PostgreSQL owners.

use std::io::Write;

use weather_contracts::TideConfiguration;
use weather_database::to_pool_configuration;
use weather_worker::{
    HistoricalBackfillArguments, HistoricalBackfillCommand, HistoricalBackfillConfigurationFamily,
    HistoricalBackfillReport, HistoricalBackfillSourceConfiguration, NoaaTideScheduledSource,
    NoaaTideSourceKind, TaskFailure, TideBackfillDate, TideBackfillSource,
    WorkerProcessEnvironmentPort, execute_tide_backfill,
};

use crate::{
    DatabaseTideBackfillRepository, HistoricalBackfillReportFileMode,
    NativeHistoricalBackfillOutput, NativePostgresParameter, NativePostgresPool,
    NativePublicBackfillTime, NativeRuntimeMaterial, NativeScheduledValues,
    NativeWorkerBackfillOwner, WorkerSourceMaterialIdentity,
    historical_backfill_process::finish_report, native_postgres_weather_rows::json,
    native_worker_configuration_loader,
    worker_noaa_tide_dispatch::project_noaa_tide_runtime_configurations,
};

/// Run one original standalone tide import with the genuine configuration and pool.
pub fn run_native_tide_backfill(
    command: HistoricalBackfillCommand,
    output: impl Write,
) -> Result<u8, TaskFailure> {
    // preserve the closed command and source-family relationship
    let HistoricalBackfillArguments::Tide(arguments) = command.arguments else {
        return Err(failure("historical_backfill_configuration_mismatch"));
    };
    if command.source_configuration
        != HistoricalBackfillSourceConfiguration::Required(
            HistoricalBackfillConfigurationFamily::Tide,
        )
    {
        return Err(failure("historical_backfill_configuration_mismatch"));
    }
    let configuration = native_worker_configuration_loader()
        .load_configuration()
        .map_err(|_| failure("worker_configuration_failed"))?;
    let catalog = configuration
        .tides
        .as_ref()
        .ok_or_else(|| failure("tide_configuration_required"))?;
    let pool = NativePostgresPool::new(to_pool_configuration(&configuration.database))
        .map_err(|_| failure("tide_backfill_pool_failed"))?;
    let mut repository =
        DatabaseTideBackfillRepository::new(pool.clone(), NativeScheduledValues::new());
    // keep the pool alive through committed chunks and report publication
    let result = (|| {
        let sources = resolve_sources(&pool, catalog, &arguments.source_keys)?;
        let mut provider = NativeWorkerBackfillOwner::new()
            .and_then(|owner| owner.noaa())
            .map_err(|_| failure("tide_backfill_provider_failed"))?;
        let report = execute_tide_backfill(
            &arguments,
            &catalog.siteKey,
            &sources,
            &mut repository,
            &mut provider,
            &mut NativePublicBackfillTime::default(),
        )?;
        finish_report(
            HistoricalBackfillReport::Tide(report),
            command.report_path.as_deref(),
            HistoricalBackfillReportFileMode::ProcessDefault,
            &mut NativeHistoricalBackfillOutput::new(output),
        )
    })();
    // retain the original finally-close precedence
    drop(repository);
    pool.close()
        .map_err(|_| failure("tide_backfill_pool_close_failed"))?;
    result
}

/// Resolve the original active configured source identities before doing provider work.
fn resolve_sources(
    pool: &NativePostgresPool,
    catalog: &TideConfiguration,
    requested: &[String],
) -> Result<Vec<TideBackfillSource>, TaskFailure> {
    let mut material = NativeRuntimeMaterial;
    let configurations = project_noaa_tide_runtime_configurations(catalog, &mut material)?;
    let configured = configurations
        .into_iter()
        .zip(&catalog.stations)
        .filter(|(configuration, _)| {
            configuration.station_active
                && configuration.source_active
                && (requested.is_empty() || requested.contains(&configuration.source_key))
        })
        .collect::<Vec<_>>();
    // require each explicit selector to resolve to a configured active source
    if !requested.is_empty() && configured.len() != requested.len() {
        return Err(failure("tide_backfill_source_unavailable"));
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
    // preserve catalog ordering, checking all independent durable identity members
    for (configuration, station) in configured {
        let row = rows
            .iter()
            .find(|row| {
                text(row, "source_key").is_ok_and(|value| value == configuration.source_key)
            })
            .ok_or_else(|| failure("tide_backfill_source_unavailable"))?;
        let kind = match configuration.source_kind {
            NoaaTideSourceKind::Observation => "tide_observation",
            NoaaTideSourceKind::Prediction => "tide_prediction",
        };
        let stored = json(row, "material_provider_config")
            .map_err(|_| failure("tide_backfill_source_identity"))?
            .ok_or_else(|| failure("tide_backfill_source_identity"))?;
        let identity = material.material_identity(&stored)?;
        // verify source, provider, station, timezone and exact canonical material
        if text(row, "source_kind")? != kind
            || text(row, "site_slug")? != configuration.site_slug
            || text(row, "station_slug")? != configuration.station_slug
            || text(row, "provider_key")? != configuration.provider_key
            || text(row, "source_config_fingerprint")? != configuration.source_config_fingerprint
            || text(row, "timezone")? != configuration.timezone
            || identity != configuration.material_config_identity
        {
            return Err(failure("tide_backfill_source_identity"));
        }
        let label = &station.source.historyStartDate;
        let midnight = history_midnight(label)?;
        let source = NoaaTideScheduledSource {
            active: true,
            cadence_seconds: station.source.cadenceSeconds,
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
        result.push(TideBackfillSource {
            configuration,
            history_start: TideBackfillDate::admitted(label, midnight),
            source,
            storage_active: true,
        });
    }
    Ok(result)
}

/// Read the identical active station-source relation for NOAA and Tempest one-shots.
pub(crate) fn read_station_rows(
    pool: &NativePostgresPool,
    site: &str,
    keys: Vec<String>,
) -> Result<Vec<tokio_postgres::Row>, TaskFailure> {
    let mut session = pool
        .checkout()
        .map_err(|_| failure("historical_backfill_source_read"))?;
    let rows = session.query(
        "SELECT s.material_provider_config, p.provider_key, si.slug AS site_slug, s.source_config_fingerprint, \
         s.id::text AS source_id, s.source_key, s.source_kind::text AS source_kind, st.slug AS station_slug, si.timezone \
         FROM sources s JOIN stations st ON st.id=s.station_id JOIN sites si ON si.id=st.site_id \
         JOIN providers p ON p.id=s.provider_id WHERE si.slug=$1 AND s.source_key=ANY($2::text[]) \
         AND s.active AND st.active AND si.active AND p.active ORDER BY s.source_key",
        &[NativePostgresParameter::Text(site.to_owned()), NativePostgresParameter::TextArray(
            keys)],
    ).map_err(|_| failure("historical_backfill_source_read"))?;
    session
        .release()
        .map_err(|_| failure("historical_backfill_source_read"))?;
    Ok(rows)
}

/// Decode the exact typed database identity field.
pub(crate) fn text(row: &tokio_postgres::Row, name: &str) -> Result<String, TaskFailure> {
    row.try_get(name)
        .map_err(|_| failure("historical_backfill_source_identity"))
}

/// Admit one exact Gregorian history date and UTC midnight.
pub(crate) fn history_midnight(label: &str) -> Result<i64, TaskFailure> {
    let date = label
        .parse::<jiff::civil::Date>()
        .map_err(|_| failure("historical_backfill_date_invalid"))?;
    // reject normalized or expanded date spellings
    if date.to_string() != label {
        return Err(failure("historical_backfill_date_invalid"));
    }
    jiff::tz::TimeZone::UTC
        .to_timestamp(date.at(0, 0, 0, 0))
        .map(|timestamp| timestamp.as_millisecond())
        .map_err(|_| failure("historical_backfill_date_invalid"))
}

/// Keep the native process diagnostics bounded.
fn failure(code: &'static str) -> TaskFailure {
    TaskFailure {
        attempts: 0,
        code: code.to_owned(),
    }
}
