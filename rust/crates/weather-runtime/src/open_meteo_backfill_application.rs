//! Run the original shared Open-Meteo historical backfill command.

use std::fmt;
use std::fs::OpenOptions;
use std::io::{self, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

use weather_contracts::{
    BackfillArguments, BackfillReport, BackfillReportExitCodeEnum, BackfillSource, JsonValue,
    Nullable, SiteConfiguration, SiteConfigurationSource, SourceCapability, SourceKind,
};
use weather_database::{
    OpenMeteoBackfillSourceReadError, OpenMeteoBackfillSourceReadPort,
    resolve_open_meteo_backfill_source,
};
use weather_worker::{
    ForecastAnchorBackfillArguments, ForecastAnchorBackfillDate, ForecastAnchorBackfillProvider,
    ForecastAnchorBackfillRepository, ForecastAnchorBackfillSource,
    ForecastAnchorBackfillTimeAdapter, ForecastAnchorCapability,
    ForecastAnchorRuntimeConfiguration, ForecastAnchorSourceKind,
    HistoricalBackfillCommandTimeAdapter, OPEN_METEO_ARCHIVE_SOURCE_KEY, OpenMeteoArchiveProvider,
    OpenMeteoArchiveRepository, OpenMeteoArchiveSource, OpenMeteoArchiveTimeAdapter,
    OpenMeteoBackfillArgumentError, TaskFailure, execute_forecast_anchor_backfill,
    execute_open_meteo_archive_backfill, parse_open_meteo_backfill_arguments,
};

use crate::forecast_anchor_backfill::forecast_anchor_adapter_configuration;
use crate::{WorkerConfiguration, WorkerSourceMaterialIdentity};

const PREVIOUS_RUNS_SOURCE_KEY: &str = "open-meteo-previous-runs-v1";

/// Closed application refusal before or during the original shared command.
#[derive(Clone, Debug, PartialEq)]
pub enum OpenMeteoBackfillApplicationError {
    Arguments(OpenMeteoBackfillArgumentError),
    SiteMismatch {
        requested: String,
        configured: String,
    },
    SourceNotConfigured,
    SourceUnavailable,
    SourceIdentityMismatch,
    SourceContractMismatch,
    ArchiveContractMismatch,
    PreviousRunsContractMismatch,
    Process(TaskFailure),
}

impl fmt::Display for OpenMeteoBackfillApplicationError {
    /// Keep the original command diagnostics distinct before process-level redaction.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Arguments(error) => fmt::Display::fmt(error, formatter),
            Self::SiteMismatch { requested, configured } => write!(formatter,
                "requested site {requested} does not match configured site {configured}"),
            Self::SourceNotConfigured => formatter.write_str("no configured Open-Meteo historical source matches the request"),
            Self::SourceUnavailable => formatter.write_str("no active Open-Meteo historical source matches the request"),
            Self::SourceIdentityMismatch => formatter.write_str("source identity does not match configuration"),
            Self::SourceContractMismatch => formatter.write_str("selected source does not support historical ingestion"),
            Self::ArchiveContractMismatch => formatter.write_str("source adapter contract must be archive-hourly/v1 with 14-day chunks"),
            Self::PreviousRunsContractMismatch => formatter.write_str("source adapter contract must be previous-runs-hourly/v1 with fixed 24-168 hour leads and 14-day chunks"),
            Self::Process(error) => formatter.write_str(&error.code),
        }
    }
}

impl std::error::Error for OpenMeteoBackfillApplicationError {}

impl From<TaskFailure> for OpenMeteoBackfillApplicationError {
    /// Leave ordinary driver and output failures in the admitted process category.
    fn from(failure: TaskFailure) -> Self {
        Self::Process(failure)
    }
}

/// Original configuration, pool, provider and report owners for the one-shot process.
pub trait OpenMeteoBackfillApplicationPort {
    type Pool: OpenMeteoBackfillSourceReadPort;
    type ArchiveProvider: OpenMeteoArchiveProvider;
    type PreviousRunsProvider: ForecastAnchorBackfillProvider;
    type ArchiveTime: OpenMeteoArchiveTimeAdapter;
    type PreviousRunsTime: ForecastAnchorBackfillTimeAdapter;
    type ArchiveRepository<'a>: OpenMeteoArchiveRepository
    where
        Self: 'a;
    type PreviousRunsRepository<'a>: ForecastAnchorBackfillRepository
    where
        Self: 'a;

    /// Load the original worker catalogs and compatibility origin after argv admission.
    fn load_configuration(&mut self) -> Result<WorkerConfiguration, TaskFailure>;

    /// Open the process pool from the loaded worker database configuration.
    fn open_pool(&mut self, configuration: &WorkerConfiguration)
    -> Result<Self::Pool, TaskFailure>;

    /// Create the Archive operation with the original compatibility origin.
    fn create_archive_provider(
        &mut self,
        configuration: &WorkerConfiguration,
    ) -> Self::ArchiveProvider;

    /// Create Previous Runs with the same already admitted compatibility origin.
    fn create_previous_runs_provider(
        &mut self,
        configuration: &WorkerConfiguration,
    ) -> Self::PreviousRunsProvider;

    /// Create the admitted site-local calendar and shared deadline implementation.
    fn create_archive_time(&mut self) -> Self::ArchiveTime;

    /// Create the admitted UTC calendar and shared deadline implementation.
    fn create_previous_runs_time(&mut self) -> Self::PreviousRunsTime;

    /// Verify the original supported PostgreSQL condition before source resolution.
    fn assert_supported_database(&mut self, pool: &mut Self::Pool) -> Result<(), TaskFailure>;

    /// Classify one original two-bind source read failure with process diagnostics.
    fn source_read_failure(
        &mut self,
        error: <Self::Pool as OpenMeteoBackfillSourceReadPort>::Error,
    ) -> TaskFailure;

    /// Bind normalized weather persistence to the same process-owned pool.
    fn archive_repository<'a>(
        &'a mut self,
        pool: &'a mut Self::Pool,
    ) -> Self::ArchiveRepository<'a>;

    /// Bind fixed anchor persistence to the same process-owned pool.
    fn previous_runs_repository<'a>(
        &'a mut self,
        pool: &'a mut Self::Pool,
    ) -> Self::PreviousRunsRepository<'a>;

    /// Serialize the original pretty report with its final line feed.
    fn serialize_report(&mut self, report: &BackfillReport) -> Result<Vec<u8>, TaskFailure>;

    /// Persist the optional report with original owner read/write creation mode.
    fn write_private_report(&mut self, path: &str, contents: &[u8]) -> Result<(), TaskFailure> {
        write_open_meteo_backfill_report(path, contents)
            .map_err(|_| output_failure("open_meteo_backfill_report_write_failed"))
    }

    /// Emit the same report bytes to ordinary stdout before closing the pool.
    fn write_stdout(&mut self, contents: &[u8]) -> Result<(), TaskFailure> {
        io::stdout()
            .lock()
            .write_all(contents)
            .map_err(|_| output_failure("open_meteo_backfill_stdout_failed"))
    }

    /// Close the pool in the original finally boundary.
    fn close_pool(&mut self, pool: Self::Pool) -> Result<(), TaskFailure>;
}

/// Write already serialized report bytes using the original file creation mode.
pub fn write_open_meteo_backfill_report(path: impl AsRef<Path>, contents: &[u8]) -> io::Result<()> {
    // set the mode only when creating a report, as the existing command does
    OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?
        .write_all(contents)
}

/// Keep file and stdout errors distinct without publishing host paths.
fn output_failure(code: &str) -> TaskFailure {
    TaskFailure {
        attempts: 0,
        code: code.to_owned(),
    }
}

/// Dispatch the original command once, using one pool and one output/finally path.
pub fn run_open_meteo_backfill_application<Port, CommandTime, Material>(
    argv: &[String],
    port: &mut Port,
    command_time: &mut CommandTime,
    material: &mut Material,
) -> Result<u8, OpenMeteoBackfillApplicationError>
where
    Port: OpenMeteoBackfillApplicationPort,
    CommandTime: HistoricalBackfillCommandTimeAdapter,
    Material: WorkerSourceMaterialIdentity,
{
    use OpenMeteoBackfillApplicationError as Error;

    let (arguments, from, to) = parse_open_meteo_backfill_arguments(argv, command_time)
        .map_err(Error::Arguments)?
        .into_parts();
    let configuration = port.load_configuration()?;
    // preserve the original command site check before opening a pool
    if configuration.site.site.key != arguments.site {
        return Err(Error::SiteMismatch {
            requested: arguments.site,
            configured: configuration.site.site.key,
        });
    }
    let mut pool = port.open_pool(&configuration)?;
    // create both original operations before readiness while fetching only the selected backend
    let mut archive = port.create_archive_provider(&configuration);
    let mut previous_runs = port.create_previous_runs_provider(&configuration);
    let execution = (|| {
        port.assert_supported_database(&mut pool)?;
        let (row, configured) =
            resolve_source(&arguments, &configuration.site, &mut pool, material).map_err(
                |error| match error {
                    SourceError::Read(error) => Error::Process(port.source_read_failure(error)),
                    SourceError::Application(error) => error,
                },
            )?;
        // dispatch the confirmed contract exactly once and never retry with the other storage family
        let report = match (configured.key.as_str(), configured.sourceKind) {
            (OPEN_METEO_ARCHIVE_SOURCE_KEY, SourceKind::Reanalysis) => {
                let source = archive_source(row, configured)?;
                let mut time = port.create_archive_time();
                let mut repository = port.archive_repository(&mut pool);
                execute_open_meteo_archive_backfill(
                    &arguments,
                    &source,
                    &mut repository,
                    &mut archive,
                    &mut time,
                )?
            }
            (PREVIOUS_RUNS_SOURCE_KEY, SourceKind::Forecast) => {
                let (anchor_arguments, anchor_configuration, source) =
                    previous_runs_inputs(&arguments, (from, to), configured, row, material)?;
                let mut time = port.create_previous_runs_time();
                let mut repository = port.previous_runs_repository(&mut pool);
                execute_forecast_anchor_backfill(
                    &anchor_arguments,
                    &anchor_configuration,
                    &source,
                    &mut repository,
                    &mut previous_runs,
                    &mut time,
                )?
                .into_backfill_report()
            }
            _ => return Err(Error::SourceContractMismatch),
        };
        let bytes = port.serialize_report(&report)?;
        // keep absent and empty supplied report paths distinct before stdout
        if let Nullable::Value(path) = &arguments.reportPath {
            port.write_private_report(path, &bytes)?;
        }
        port.write_stdout(&bytes)?;
        Ok(match report.exitCode {
            BackfillReportExitCodeEnum::Value0 => 0,
            BackfillReportExitCodeEnum::Value1 => 1,
        })
    })();
    // retain the close failure precedence of the original finally
    port.close_pool(pool)?;
    execution
}

enum SourceError<Driver> {
    Read(Driver),
    Application(OpenMeteoBackfillApplicationError),
}

/// Select the original configured historical key and bind its active database row.
fn resolve_source<'a, Pool, Material>(
    arguments: &BackfillArguments,
    site: &'a SiteConfiguration,
    pool: &mut Pool,
    material: &mut Material,
) -> Result<(BackfillSource, &'a SiteConfigurationSource), SourceError<Pool::Error>>
where
    Pool: OpenMeteoBackfillSourceReadPort,
    Material: WorkerSourceMaterialIdentity,
{
    use OpenMeteoBackfillApplicationError as Error;

    let key = match &arguments.source {
        Nullable::Null => OPEN_METEO_ARCHIVE_SOURCE_KEY,
        Nullable::Value(value) => value,
    };
    let configured = site
        .sources
        .iter()
        .find(|source| {
            source.key == key && source.capabilities.contains(&SourceCapability::Historical)
        })
        .ok_or(SourceError::Application(Error::SourceNotConfigured))?;
    let row =
        resolve_open_meteo_backfill_source(pool, &site.site.key, key).map_err(
            |error| match error {
                OpenMeteoBackfillSourceReadError::Driver(error) => SourceError::Read(error),
                OpenMeteoBackfillSourceReadError::Missing => {
                    SourceError::Application(Error::SourceUnavailable)
                }
            },
        )?;
    // enforce exact canonical material, site, station and original persisted identity
    let matches = row.providerKey == site.provider.key
        && row.siteSlug == site.site.key
        && row.sourceConfigFingerprint == configured.fingerprint
        && row.sourceKey == key
        && row.sourceKind == configured.sourceKind
        && row.stationSlug == site.station.key
        && row.timezone == site.site.timezone
        && row.latitude == site.site.latitude
        && row.longitude == site.site.longitude
        && material
            .values_equal(&row.materialProviderConfig, &configured.adapterConfig)
            .unwrap_or(false);
    // distinguish the ineligible durable row from a changed adapter contract
    if !matches {
        return Err(SourceError::Application(Error::SourceIdentityMismatch));
    }
    // require the original unscheduled historical contract after source resolution
    if configured.cadenceSeconds != Nullable::Null
        || configured.capabilities != [SourceCapability::Historical]
    {
        return Err(SourceError::Application(Error::SourceContractMismatch));
    }
    Ok((row, configured))
}

/// Admit only the selected reanalysis source and exact loaded Archive controls.
fn archive_source(
    row: BackfillSource,
    configured: &SiteConfigurationSource,
) -> Result<OpenMeteoArchiveSource, OpenMeteoBackfillApplicationError> {
    use OpenMeteoBackfillApplicationError as Error;

    let (contract_version, maximum_chunk_days) =
        archive_adapter_contract(&configured.adapterConfig)
            .ok_or(Error::ArchiveContractMismatch)?;
    OpenMeteoArchiveSource::admit(row, &contract_version, maximum_chunk_days, true)
        .map_err(|_| Error::ArchiveContractMismatch)
}

/// Reuse the checked Previous Runs contract against the very same validated source row.
fn previous_runs_inputs(
    arguments: &BackfillArguments,
    dates: (ForecastAnchorBackfillDate, ForecastAnchorBackfillDate),
    configured: &SiteConfigurationSource,
    row: BackfillSource,
    material: &mut impl WorkerSourceMaterialIdentity,
) -> Result<
    (
        ForecastAnchorBackfillArguments,
        ForecastAnchorRuntimeConfiguration,
        ForecastAnchorBackfillSource,
    ),
    OpenMeteoBackfillApplicationError,
> {
    use OpenMeteoBackfillApplicationError as Error;

    let adapter = forecast_anchor_adapter_configuration(&configured.adapterConfig)
        .ok_or(Error::PreviousRunsContractMismatch)?;
    let identity = material.material_identity(&configured.adapterConfig)?;
    let configuration = ForecastAnchorRuntimeConfiguration {
        active: true,
        adapter,
        cadence_seconds: None,
        capabilities: [ForecastAnchorCapability::Historical].into_iter().collect(),
        fingerprint: configured.fingerprint.clone(),
        latitude: row.latitude,
        longitude: row.longitude,
        material_config_identity: identity.clone(),
        provider_key: row.providerKey.clone(),
        site_slug: row.siteSlug.clone(),
        source_key: configured.key.clone(),
        source_kind: ForecastAnchorSourceKind::Forecast,
        station_slug: row.stationSlug.clone(),
        timezone: row.timezone.clone(),
    };
    let source = ForecastAnchorBackfillSource {
        active: true,
        id: row.id,
        key: row.key,
        latitude: row.latitude,
        longitude: row.longitude,
        material_config_identity: identity,
        provider_key: row.providerKey,
        site_slug: row.siteSlug,
        source_config_fingerprint: row.sourceConfigFingerprint,
        source_key: row.sourceKey,
        source_kind: ForecastAnchorSourceKind::Forecast,
        station_slug: row.stationSlug,
        timezone: row.timezone,
    };
    let (from, to) = dates;
    let arguments = ForecastAnchorBackfillArguments {
        chunk_days: u16::try_from(arguments.chunkDays)
            .map_err(|_| Error::SourceContractMismatch)?,
        dry_run: arguments.dryRun,
        from,
        resume: arguments.resume,
        site: arguments.site.clone(),
        source: configured.key.clone(),
        to,
    };
    Ok((arguments, configuration, source))
}

/// Extract the two original fixed fields from already-decoded configuration.
fn archive_adapter_contract(material: &JsonValue) -> Option<(String, u64)> {
    let JsonValue::Object(fields) = material else {
        return None;
    };
    let get = |key: &str| {
        fields
            .iter()
            .find_map(|(name, value)| name.iter().copied().eq(key.encode_utf16()).then_some(value))
    };
    let JsonValue::String(contract_version) = get("contractVersion")? else {
        return None;
    };
    let JsonValue::Number(maximum_chunk_days) = get("maximumChunkDays")? else {
        return None;
    };
    // use the existing frozen ceiling rather than allowing a lossy numeric cast
    let maximum = weather_worker::OPEN_METEO_ARCHIVE_MAXIMUM_CHUNK_DAYS;
    (*maximum_chunk_days == maximum as f64).then(|| {
        String::from_utf16(contract_version)
            .ok()
            .map(|contract| (contract, maximum))
    })?
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    /// Keep the report file's original private creation and replacement behavior.
    #[test]
    fn writes_the_native_report_without_changing_existing_mode() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "weather-backfill-report-{}-{stamp}",
            std::process::id()
        ));
        fs::DirBuilder::new()
            .mode(0o700)
            .create(&root)
            .expect("private fixture directory");
        let fixture = ReportFixture(root);
        let report = fixture.0.join("report");
        write_open_meteo_backfill_report(&report, b"initial report\n").expect("create report");
        assert_eq!(
            fs::metadata(&report)
                .expect("report metadata")
                .permissions()
                .mode()
                & 0o077,
            0
        );
        fs::set_permissions(&report, fs::Permissions::from_mode(0o640))
            .expect("existing fixture mode");
        write_open_meteo_backfill_report(&report, b"next\n").expect("replace report");
        assert_eq!(fs::read(&report).expect("report bytes"), b"next\n");
        assert_eq!(
            fs::metadata(&report)
                .expect("report metadata")
                .permissions()
                .mode()
                & 0o777,
            0o640
        );
    }

    struct ReportFixture(std::path::PathBuf);

    impl Drop for ReportFixture {
        /// Clean up only the temporary directory owned by this test.
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
}
