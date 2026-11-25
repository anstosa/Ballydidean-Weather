//! Shared application routing for the original Open-Meteo historical backfill command.

use std::{cell::RefCell, rc::Rc};

use weather_contracts::{
    BackfillChunkIdentity, BackfillChunkReportStatusEnum, BackfillReport, JsonValue,
    NormalizedWeatherRecord, Nullable, SiteConfiguration, SiteConfigurationProvider,
    SiteConfigurationSite, SiteConfigurationSource, SiteConfigurationStation, SourceCapability,
    SourceKind, StationKind,
};
use weather_database::{
    DatabaseConfiguration, OpenMeteoBackfillSourceBind, OpenMeteoBackfillSourceQuery,
    OpenMeteoBackfillSourceReadPort, OpenMeteoBackfillSourceRow,
};
use weather_runtime::{
    OpenMeteoBackfillApplicationError, OpenMeteoBackfillApplicationPort,
    TemperatureCanaryKillSwitch, WorkerConfiguration, WorkerSourceMaterialIdentity,
    run_open_meteo_backfill_application,
};
use weather_worker::{
    ForecastAnchorAttemptPayload, ForecastAnchorBackfillChunkIdentity,
    ForecastAnchorBackfillCompletion, ForecastAnchorBackfillDate, ForecastAnchorBackfillDeadlines,
    ForecastAnchorBackfillOperationError, ForecastAnchorBackfillProvider,
    ForecastAnchorBackfillProviderFailure, ForecastAnchorBackfillRepository,
    ForecastAnchorBackfillRunStart, ForecastAnchorBackfillTimeAdapter,
    HistoricalBackfillCommandDate, HistoricalBackfillCommandTimeAdapter,
    OpenMeteoArchiveCleanupFailure, OpenMeteoArchiveCompletion, OpenMeteoArchiveInstant,
    OpenMeteoArchiveProvider, OpenMeteoArchiveRepository, OpenMeteoArchiveRequest,
    OpenMeteoArchiveRunStart, OpenMeteoArchiveTimeAdapter, ProviderBatch,
    ScheduledSourceMaterialComparison, TaskFailure, TideBackfillDeadlines,
    TideBackfillOperationError, TideBackfillProviderFailure,
};

const ARCHIVE_KEY: &str = "open-meteo-reanalysis-v1";
const PREVIOUS_RUNS_KEY: &str = "open-meteo-previous-runs-v1";
const DATE: &str = "2026-03-08";
const DATE_MS: i64 = 1_772_928_000_000;
const NEXT_DATE_MS: i64 = 1_773_014_400_000;
const TODAY_MS: i64 = 1_787_356_800_000;

type Trace = Rc<RefCell<Vec<String>>>;

struct CommandTime;

impl HistoricalBackfillCommandTimeAdapter for CommandTime {
    /// return the fixed date used by original past-range admission
    fn today_utc(&mut self) -> Result<HistoricalBackfillCommandDate, TaskFailure> {
        Ok(HistoricalBackfillCommandDate::admitted(
            "2026-08-22",
            TODAY_MS,
        ))
    }

    /// admit the single requested fixture date
    fn admit_utc_date(
        &mut self,
        label: &str,
    ) -> Result<HistoricalBackfillCommandDate, TaskFailure> {
        match label {
            DATE => Ok(HistoricalBackfillCommandDate::admitted(DATE, DATE_MS)),
            _ => Err(failure("fixture_date_unavailable")),
        }
    }

    /// keep unrelated command calendar arithmetic outside this fixture
    fn add_utc_days(
        &mut self,
        _: &HistoricalBackfillCommandDate,
        _: i64,
    ) -> Result<HistoricalBackfillCommandDate, TaskFailure> {
        unreachable!("the Open-Meteo argument parser does not add dates")
    }
}

struct ArchiveTime;

impl OpenMeteoArchiveTimeAdapter for ArchiveTime {
    /// add only the two local-calendar offsets used by one dry-run chunk
    fn add_calendar_days(&mut self, date: &str, days: u64) -> Result<String, TaskFailure> {
        match (date, days) {
            (DATE, 13) => Ok("2026-03-21".to_owned()),
            (DATE, 1) => Ok("2026-03-09".to_owned()),
            _ => Err(failure("fixture_archive_date_unavailable")),
        }
    }

    /// resolve the spring-transition Los Angeles interval boundaries
    fn local_midnight_utc(
        &mut self,
        date: &str,
        timezone: &str,
    ) -> Result<OpenMeteoArchiveInstant, TaskFailure> {
        assert_eq!(timezone, "America/Los_Angeles");
        match date {
            DATE => Ok(OpenMeteoArchiveInstant {
                epoch_milliseconds: 1_772_956_800_000,
                utc: "2026-03-08T08:00:00.000Z".to_owned(),
            }),
            "2026-03-09" => Ok(OpenMeteoArchiveInstant {
                epoch_milliseconds: 1_773_039_600_000,
                utc: "2026-03-09T07:00:00.000Z".to_owned(),
            }),
            _ => Err(failure("fixture_archive_midnight_unavailable")),
        }
    }

    /// reject deadlines because dry-run never starts provider work
    fn deadlines(&mut self) -> Result<TideBackfillDeadlines, TideBackfillOperationError> {
        unreachable!("dry-run does not request deadlines")
    }
}

struct PreviousRunsTime;

impl ForecastAnchorBackfillTimeAdapter for PreviousRunsTime {
    /// return the fixed current UTC day used by executor validation
    fn today_utc(&mut self) -> Result<ForecastAnchorBackfillDate, TaskFailure> {
        Ok(ForecastAnchorBackfillDate::admitted("2026-08-22", TODAY_MS))
    }

    /// add only the two UTC offsets used by one dry-run chunk
    fn add_utc_days(
        &mut self,
        date: &ForecastAnchorBackfillDate,
        days: i64,
    ) -> Result<ForecastAnchorBackfillDate, TaskFailure> {
        match (date.label(), days) {
            (DATE, 13) => Ok(ForecastAnchorBackfillDate::admitted(
                "2026-03-21",
                1_774_051_200_000,
            )),
            (DATE, 1) => Ok(ForecastAnchorBackfillDate::admitted(
                "2026-03-09",
                NEXT_DATE_MS,
            )),
            _ => Err(failure("fixture_previous_runs_date_unavailable")),
        }
    }

    /// reject deadlines because dry-run never starts provider work
    fn deadlines(
        &mut self,
    ) -> Result<ForecastAnchorBackfillDeadlines, ForecastAnchorBackfillOperationError> {
        unreachable!("dry-run does not request deadlines")
    }
}

struct ForbiddenArchiveProvider;

impl OpenMeteoArchiveProvider for ForbiddenArchiveProvider {
    /// reject Archive transport during dry-run
    fn fetch(
        &mut self,
        _: OpenMeteoArchiveRequest,
    ) -> Result<ProviderBatch<NormalizedWeatherRecord, JsonValue, ()>, TideBackfillOperationError>
    {
        unreachable!("dry-run does not fetch Archive data")
    }
}

struct ForbiddenPreviousRunsProvider;

impl ForecastAnchorBackfillProvider for ForbiddenPreviousRunsProvider {
    /// reject Previous Runs transport during dry-run
    fn fetch(
        &mut self,
        _: weather_worker::ForecastAnchorProviderRequest,
    ) -> Result<ForecastAnchorAttemptPayload, ForecastAnchorBackfillOperationError> {
        unreachable!("dry-run does not fetch Previous Runs data")
    }
}

struct NoopArchiveRepository;

impl OpenMeteoArchiveRepository for NoopArchiveRepository {
    type Session = ();

    /// reject resume reads when resume is disabled
    fn has_successful_chunk(&mut self, _: &BackfillChunkIdentity) -> Result<bool, TaskFailure> {
        unreachable!("fixture disables resume")
    }

    /// reject locks during dry-run
    fn acquire_source_session(&mut self, _: &str) -> Result<Option<Self::Session>, TaskFailure> {
        unreachable!("dry-run does not acquire a session")
    }

    /// reject durable cleanup during dry-run
    fn abandon_expired_runs(
        &mut self,
        _: &mut Self::Session,
        _: i64,
    ) -> Result<(), TideBackfillOperationError> {
        unreachable!("dry-run does not abandon runs")
    }

    /// reject durable starts during dry-run
    fn start_ingestion_run(
        &mut self,
        _: &mut Self::Session,
        _: OpenMeteoArchiveRunStart,
    ) -> Result<String, TideBackfillOperationError> {
        unreachable!("dry-run does not start runs")
    }

    /// reject durable completion during dry-run
    fn complete_backfill_ingestion(
        &mut self,
        _: &mut Self::Session,
        _: OpenMeteoArchiveCompletion,
    ) -> Result<(), TideBackfillOperationError> {
        unreachable!("dry-run does not complete runs")
    }

    /// reject durable failure during dry-run
    fn fail_ingestion_run(
        &mut self,
        _: &mut Self::Session,
        _: &str,
        _: &BackfillChunkIdentity,
        _: &TideBackfillProviderFailure,
    ) -> Result<(), OpenMeteoArchiveCleanupFailure> {
        unreachable!("dry-run does not fail runs")
    }

    /// reject release when no session was acquired
    fn release_source_session(
        &mut self,
        _: Self::Session,
    ) -> Result<(), OpenMeteoArchiveCleanupFailure> {
        unreachable!("dry-run does not release sessions")
    }
}

struct NoopPreviousRunsRepository;

impl ForecastAnchorBackfillRepository for NoopPreviousRunsRepository {
    type Session = ();

    /// reject resume reads when resume is disabled
    fn has_successful_chunk(
        &mut self,
        _: &ForecastAnchorBackfillChunkIdentity,
    ) -> Result<bool, TaskFailure> {
        unreachable!("fixture disables resume")
    }

    /// reject locks during dry-run
    fn acquire_source_session(&mut self, _: &str) -> Result<Option<Self::Session>, TaskFailure> {
        unreachable!("dry-run does not acquire a session")
    }

    /// reject durable cleanup during dry-run
    fn abandon_expired_runs(
        &mut self,
        _: &mut Self::Session,
        _: i64,
    ) -> Result<(), ForecastAnchorBackfillOperationError> {
        unreachable!("dry-run does not abandon runs")
    }

    /// reject durable starts during dry-run
    fn start_ingestion_run(
        &mut self,
        _: &mut Self::Session,
        _: ForecastAnchorBackfillRunStart,
    ) -> Result<String, ForecastAnchorBackfillOperationError> {
        unreachable!("dry-run does not start runs")
    }

    /// reject durable completion during dry-run
    fn complete_forecast_anchor_backfill_ingestion(
        &mut self,
        _: &mut Self::Session,
        _: ForecastAnchorBackfillCompletion,
    ) -> Result<(), ForecastAnchorBackfillOperationError> {
        unreachable!("dry-run does not complete runs")
    }

    /// reject durable failure during dry-run
    fn fail_ingestion_run(
        &mut self,
        _: &mut Self::Session,
        _: &str,
        _: &ForecastAnchorBackfillChunkIdentity,
        _: &ForecastAnchorBackfillProviderFailure,
    ) -> Result<(), OpenMeteoArchiveCleanupFailure> {
        unreachable!("dry-run does not fail runs")
    }

    /// reject release when no session was acquired
    fn release_source_session(
        &mut self,
        _: Self::Session,
    ) -> Result<(), OpenMeteoArchiveCleanupFailure> {
        unreachable!("dry-run does not release sessions")
    }
}

struct FixturePool {
    site: SiteConfiguration,
    trace: Trace,
}

impl OpenMeteoBackfillSourceReadPort for FixturePool {
    type Error = ();

    /// return the exact configured active row selected by the original two binds
    fn read_open_meteo_backfill_source(
        &mut self,
        query: &OpenMeteoBackfillSourceQuery,
    ) -> Result<Option<OpenMeteoBackfillSourceRow>, Self::Error> {
        let OpenMeteoBackfillSourceBind::Text(key) = &query.binds[1];
        self.trace.borrow_mut().push(format!("read:{key}"));
        let configured = self.site.sources.iter().find(|source| source.key == *key);
        Ok(configured.map(|source| source_row(&self.site, source)))
    }
}

struct FixturePort {
    configuration: WorkerConfiguration,
    file: Option<(String, Vec<u8>)>,
    serialized: Option<Vec<u8>>,
    stdout: Option<Vec<u8>>,
    trace: Trace,
}

impl FixturePort {
    /// construct one closed shared application fixture
    fn new() -> Self {
        let trace = Rc::new(RefCell::new(Vec::new()));
        Self {
            configuration: configuration(),
            file: None,
            serialized: None,
            stdout: None,
            trace,
        }
    }
}

impl OpenMeteoBackfillApplicationPort for FixturePort {
    type Pool = FixturePool;
    type ArchiveProvider = ForbiddenArchiveProvider;
    type PreviousRunsProvider = ForbiddenPreviousRunsProvider;
    type ArchiveTime = ArchiveTime;
    type PreviousRunsTime = PreviousRunsTime;
    type ArchiveRepository<'a>
        = NoopArchiveRepository
    where
        Self: 'a;
    type PreviousRunsRepository<'a>
        = NoopPreviousRunsRepository
    where
        Self: 'a;

    /// load the exact two-source worker catalog
    fn load_configuration(&mut self) -> Result<WorkerConfiguration, TaskFailure> {
        self.trace.borrow_mut().push("load".to_owned());
        Ok(self.configuration.clone())
    }

    /// open one process-owned source pool
    fn open_pool(
        &mut self,
        configuration: &WorkerConfiguration,
    ) -> Result<Self::Pool, TaskFailure> {
        self.trace.borrow_mut().push("pool".to_owned());
        Ok(FixturePool {
            site: configuration.site.clone(),
            trace: Rc::clone(&self.trace),
        })
    }

    /// create the Archive operation before readiness
    fn create_archive_provider(&mut self, _: &WorkerConfiguration) -> Self::ArchiveProvider {
        self.trace.borrow_mut().push("archive-provider".to_owned());
        ForbiddenArchiveProvider
    }

    /// create Previous Runs before readiness
    fn create_previous_runs_provider(
        &mut self,
        _: &WorkerConfiguration,
    ) -> Self::PreviousRunsProvider {
        self.trace
            .borrow_mut()
            .push("previous-runs-provider".to_owned());
        ForbiddenPreviousRunsProvider
    }

    /// create Archive time only on the selected default route
    fn create_archive_time(&mut self) -> Self::ArchiveTime {
        self.trace.borrow_mut().push("archive-time".to_owned());
        ArchiveTime
    }

    /// create UTC time only on the selected Previous Runs route
    fn create_previous_runs_time(&mut self) -> Self::PreviousRunsTime {
        self.trace
            .borrow_mut()
            .push("previous-runs-time".to_owned());
        PreviousRunsTime
    }

    /// retain readiness before the source read
    fn assert_supported_database(&mut self, _: &mut Self::Pool) -> Result<(), TaskFailure> {
        self.trace.borrow_mut().push("readiness".to_owned());
        Ok(())
    }

    /// classify the fixture's unavailable driver failure
    fn source_read_failure(&mut self, _: ()) -> TaskFailure {
        failure("fixture_source_read_failed")
    }

    /// bind only the selected Archive persistence family
    fn archive_repository<'a>(&'a mut self, _: &'a mut Self::Pool) -> Self::ArchiveRepository<'a> {
        self.trace
            .borrow_mut()
            .push("archive-repository".to_owned());
        NoopArchiveRepository
    }

    /// bind only the selected Previous Runs persistence family
    fn previous_runs_repository<'a>(
        &'a mut self,
        _: &'a mut Self::Pool,
    ) -> Self::PreviousRunsRepository<'a> {
        self.trace
            .borrow_mut()
            .push("previous-runs-repository".to_owned());
        NoopPreviousRunsRepository
    }

    /// project one generated report without implementing a JSON codec in the fixture
    fn serialize_report(&mut self, report: &BackfillReport) -> Result<Vec<u8>, TaskFailure> {
        let chunk = report
            .chunks
            .first()
            .expect("one requested date produces one chunk");
        let bytes = format!(
            "{}\t{}\t{}\t{}\n",
            report.source,
            status_name(chunk.status),
            chunk.identity.intervalStart,
            chunk.identity.intervalEndExclusive,
        )
        .into_bytes();
        self.trace
            .borrow_mut()
            .push(format!("serialize:{}", report.source));
        self.serialized = Some(bytes.clone());
        Ok(bytes)
    }

    /// retain the exact optional report bytes before stdout
    fn write_private_report(&mut self, path: &str, contents: &[u8]) -> Result<(), TaskFailure> {
        self.trace.borrow_mut().push(format!("file:{path}"));
        self.file = Some((path.to_owned(), contents.to_vec()));
        Ok(())
    }

    /// retain the same emitted report bytes
    fn write_stdout(&mut self, contents: &[u8]) -> Result<(), TaskFailure> {
        self.trace.borrow_mut().push("stdout".to_owned());
        self.stdout = Some(contents.to_vec());
        Ok(())
    }

    /// close the one process pool after output or failure
    fn close_pool(&mut self, _: Self::Pool) -> Result<(), TaskFailure> {
        self.trace.borrow_mut().push("close".to_owned());
        Ok(())
    }
}

struct Material;

impl ScheduledSourceMaterialComparison for Material {
    type Error = ();

    /// compare already-decoded fixture values directly
    fn values_equal(&mut self, left: &JsonValue, right: &JsonValue) -> Result<bool, Self::Error> {
        Ok(left == right)
    }

    /// compare one decoded value with its borrowed object projection
    fn value_and_object_equal(
        &mut self,
        value: &JsonValue,
        object: &[(Vec<u16>, JsonValue)],
    ) -> Result<bool, Self::Error> {
        Ok(matches!(value, JsonValue::Object(fields) if fields == object))
    }
}

impl WorkerSourceMaterialIdentity for Material {
    /// bind the selected exact adapter without serializing it
    fn material_identity(&mut self, value: &JsonValue) -> Result<String, TaskFailure> {
        if value == &previous_runs_adapter() {
            Ok("previous-runs-material".to_owned())
        } else {
            Err(failure("unexpected_material"))
        }
    }
}

/// prove omitted source selects only Archive and shares output/finally ownership
#[test]
fn omitted_source_runs_only_archive_and_emits_once() {
    let mut port = FixturePort::new();
    let exit = run_open_meteo_backfill_application(
        &argv(None),
        &mut port,
        &mut CommandTime,
        &mut Material,
    )
    .expect("default Archive dry-run succeeds");

    assert_eq!(exit, 0);
    assert_eq!(
        port.trace.borrow().as_slice(),
        [
            "load",
            "pool",
            "archive-provider",
            "previous-runs-provider",
            "readiness",
            "read:open-meteo-reanalysis-v1",
            "archive-time",
            "archive-repository",
            "serialize:open-meteo-reanalysis-v1",
            "file:report.tsv",
            "stdout",
            "close",
        ]
    );
    assert_emitted_once(&port, "2026-03-08T08:00:00.000Z\t2026-03-09T07:00:00.000Z");
}

/// prove explicit Previous Runs selects only its actual UTC executor
#[test]
fn explicit_previous_runs_routes_without_archive_fallback() {
    let mut port = FixturePort::new();
    let exit = run_open_meteo_backfill_application(
        &argv(Some(PREVIOUS_RUNS_KEY)),
        &mut port,
        &mut CommandTime,
        &mut Material,
    )
    .expect("explicit Previous Runs dry-run succeeds");

    assert_eq!(exit, 0);
    assert_eq!(
        port.trace.borrow().as_slice(),
        [
            "load",
            "pool",
            "archive-provider",
            "previous-runs-provider",
            "readiness",
            "read:open-meteo-previous-runs-v1",
            "previous-runs-time",
            "previous-runs-repository",
            "serialize:open-meteo-previous-runs-v1",
            "file:report.tsv",
            "stdout",
            "close",
        ]
    );
    assert_emitted_once(&port, "2026-03-08T00:00:00.000Z\t2026-03-09T00:00:00.000Z");
}

/// prove an unknown explicit source never reads a row or retries another backend
#[test]
fn unknown_source_closes_without_read_or_output() {
    let mut port = FixturePort::new();
    let result = run_open_meteo_backfill_application(
        &argv(Some("unknown-history")),
        &mut port,
        &mut CommandTime,
        &mut Material,
    );

    assert_eq!(
        result,
        Err(OpenMeteoBackfillApplicationError::SourceNotConfigured)
    );
    assert_eq!(
        port.trace.borrow().as_slice(),
        [
            "load",
            "pool",
            "archive-provider",
            "previous-runs-provider",
            "readiness",
            "close",
        ]
    );
    assert!(port.serialized.is_none());
    assert!(port.file.is_none());
    assert!(port.stdout.is_none());
}

/// require one serialization reused unchanged for file and stdout
fn assert_emitted_once(port: &FixturePort, interval: &str) {
    let serialized = port.serialized.as_ref().expect("report serialized once");
    let (path, file) = port.file.as_ref().expect("private report written");
    let stdout = port.stdout.as_ref().expect("stdout written");
    assert_eq!(path, "report.tsv");
    assert_eq!(file, serialized);
    assert_eq!(stdout, serialized);
    assert!(String::from_utf8_lossy(serialized).contains(interval));
    assert_eq!(serialized.last(), Some(&b'\n'));
}

/// construct original CLI argv with one optional explicit source
fn argv(source: Option<&str>) -> Vec<String> {
    let mut values = vec![
        "--site".to_owned(),
        "ballydidean".to_owned(),
        "--from".to_owned(),
        DATE.to_owned(),
        "--to".to_owned(),
        DATE.to_owned(),
        "--dry-run".to_owned(),
        "--report".to_owned(),
        "report.tsv".to_owned(),
    ];
    // retain the caller's exact explicit source only when supplied
    if let Some(source) = source {
        values.extend(["--source".to_owned(), source.to_owned()]);
    }
    values
}

/// construct the loaded two-source worker configuration
fn configuration() -> WorkerConfiguration {
    WorkerConfiguration {
        adjustment_maintenance_api_origin: None,
        database: DatabaseConfiguration {
            application_name: "fixture".to_owned(),
            database: "fixture".to_owned(),
            host: "fixture".to_owned(),
            lock_timeout_ms: 10_000,
            password: "fixture".to_owned(),
            port: 5432,
            ssl: false,
            statement_timeout_ms: 30_000,
            user: "fixture".to_owned(),
        },
        ecowitt: None,
        instance: "fixture".to_owned(),
        migration_authorization: None,
        migration_directory: "migrations".to_owned(),
        open_meteo_compatibility_origin: None,
        public_stations: None,
        rain_collection_enabled: false,
        site: site_configuration(),
        site_configuration_path: "site.json".to_owned(),
        tempest: None,
        tempest_api_key: None,
        temperature_canary_kill_switch: Some(TemperatureCanaryKillSwitch::Zero),
        tides: None,
        version: "fixture".to_owned(),
    }
}

/// construct the exact Archive and Previous Runs site entries
fn site_configuration() -> SiteConfiguration {
    SiteConfiguration {
        provider: SiteConfigurationProvider {
            active: true,
            attributionLabel: "Open-Meteo".to_owned(),
            attributionUrl: "https://open-meteo.com".to_owned(),
            displayName: "Open-Meteo".to_owned(),
            key: "open-meteo".to_owned(),
        },
        site: SiteConfigurationSite {
            active: true,
            displayName: "Ballydidean".to_owned(),
            key: "ballydidean".to_owned(),
            latitude: 47.950429954185445,
            longitude: -122.42797012608193,
            timezone: "America/Los_Angeles".to_owned(),
        },
        sources: vec![
            SiteConfigurationSource {
                active: true,
                adapterConfig: archive_adapter(),
                cadenceSeconds: Nullable::Null,
                capabilities: vec![SourceCapability::Historical],
                fingerprint: "a".repeat(64),
                key: ARCHIVE_KEY.to_owned(),
                sourceKind: SourceKind::Reanalysis,
            },
            SiteConfigurationSource {
                active: true,
                adapterConfig: previous_runs_adapter(),
                cadenceSeconds: Nullable::Null,
                capabilities: vec![SourceCapability::Historical],
                fingerprint: "b".repeat(64),
                key: PREVIOUS_RUNS_KEY.to_owned(),
                sourceKind: SourceKind::Forecast,
            },
        ],
        station: SiteConfigurationStation {
            active: true,
            displayName: "Open-Meteo virtual".to_owned(),
            key: "open-meteo-virtual".to_owned(),
            kind: StationKind::Virtual,
            latitude: 47.950429954185445,
            longitude: -122.42797012608193,
            model: Nullable::Null,
            serial: Nullable::Null,
            vendor: Nullable::Value("Open-Meteo".to_owned()),
        },
        version: 1,
    }
}

/// construct one matching active database source row
fn source_row(
    site: &SiteConfiguration,
    source: &SiteConfigurationSource,
) -> OpenMeteoBackfillSourceRow {
    OpenMeteoBackfillSourceRow {
        latitude: site.site.latitude,
        longitude: site.site.longitude,
        material_provider_config: source.adapterConfig.clone(),
        provider_key: site.provider.key.clone(),
        site_slug: site.site.key.clone(),
        source_config_fingerprint: source.fingerprint.clone(),
        source_id: format!("source-{}", source.key),
        source_key: source.key.clone(),
        source_kind: source.sourceKind,
        station_slug: site.station.key.clone(),
        timezone: site.site.timezone.clone(),
    }
}

/// encode one fixture string in the generated dependency-free JSON type
fn json_string(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}

/// construct the two fixed Archive adapter controls
fn archive_adapter() -> JsonValue {
    JsonValue::Object(vec![
        (
            "contractVersion".encode_utf16().collect(),
            json_string("archive-hourly/v1"),
        ),
        (
            "maximumChunkDays".encode_utf16().collect(),
            JsonValue::Number(14.0),
        ),
    ])
}

/// construct the frozen Previous Runs adapter controls
fn previous_runs_adapter() -> JsonValue {
    JsonValue::Object(vec![
        (
            "contractVersion".encode_utf16().collect(),
            json_string("previous-runs-hourly/v1"),
        ),
        (
            "contractEpoch".encode_utf16().collect(),
            json_string("open-meteo-previous-runs-best-match/2026-09"),
        ),
        ("model".encode_utf16().collect(), json_string("best_match")),
        (
            "maximumChunkDays".encode_utf16().collect(),
            JsonValue::Number(14.0),
        ),
        (
            "leadHours".encode_utf16().collect(),
            JsonValue::Array(
                [24, 48, 72, 96, 120, 144, 168]
                    .map(|value| JsonValue::Number(f64::from(value)))
                    .to_vec(),
            ),
        ),
        (
            "variables".encode_utf16().collect(),
            JsonValue::Array(
                [
                    "temperature",
                    "apparent_temperature",
                    "relative_humidity",
                    "precipitation",
                    "cloud_cover",
                    "wind_speed",
                    "wind_gust",
                    "wind_direction",
                    "surface_pressure",
                ]
                .map(json_string)
                .to_vec(),
            ),
        ),
    ])
}

/// render one generated report status literal
const fn status_name(status: BackfillChunkReportStatusEnum) -> &'static str {
    match status {
        BackfillChunkReportStatusEnum::Abandoned => "abandoned",
        BackfillChunkReportStatusEnum::Completed => "completed",
        BackfillChunkReportStatusEnum::Failed => "failed",
        BackfillChunkReportStatusEnum::Planned => "planned",
        BackfillChunkReportStatusEnum::Remaining => "remaining",
        BackfillChunkReportStatusEnum::Skipped => "skipped",
    }
}

/// construct one stable process failure
fn failure(code: &str) -> TaskFailure {
    TaskFailure {
        attempts: 0,
        code: code.to_owned(),
    }
}
