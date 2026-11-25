//! Compiled-Node parity for Open-Meteo Archive planning and default lifecycle execution.

use std::{cell::RefCell, path::PathBuf, process::Command, rc::Rc};

use weather_contracts::{
    BackfillArguments, BackfillChunkIdentity, BackfillChunkReportStatusEnum, BackfillReport,
    BackfillReportExitCodeEnum, BackfillSource, CanonicalWeatherMetrics, JsonValue,
    NormalizedWeatherRecord, Nullable, SourceKind, WeatherRecordMetadata,
};
use weather_worker::{
    OpenMeteoArchiveCleanupFailure, OpenMeteoArchiveCompletion, OpenMeteoArchiveInstant,
    OpenMeteoArchiveProvider, OpenMeteoArchiveRepository, OpenMeteoArchiveRequest,
    OpenMeteoArchiveRunStart, OpenMeteoArchiveSource, OpenMeteoArchiveTimeAdapter, ProviderBatch,
    TaskFailure, TideBackfillDeadlines, TideBackfillOperationError, TideBackfillProviderFailure,
    execute_open_meteo_archive_backfill, plan_open_meteo_archive_chunks,
};

const FINGERPRINT: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const SOURCE_ID: &str = "00000000-0000-4000-8000-000000000001";
const STARTED_AT_MS: i64 = 1_787_376_000_000;
const PROVIDER_DEADLINE_MS: i64 = 1_787_376_090_000;
const RUN_DEADLINE_MS: i64 = 1_787_376_120_000;

type Events = Rc<RefCell<Vec<String>>>;

#[derive(Default)]
struct FixtureTime;

impl OpenMeteoArchiveTimeAdapter for FixtureTime {
    /// add only the explicit local dates exercised by the oracle corpus
    fn add_calendar_days(&mut self, date: &str, days: u64) -> Result<String, TaskFailure> {
        let value = match (date, days) {
            ("2026-03-01", 13) => "2026-03-14",
            ("2026-03-08", 0) => "2026-03-08",
            ("2026-03-08", 1) => "2026-03-09",
            ("2026-03-14", 1) => "2026-03-15",
            ("2026-03-15", 13) => "2026-03-28",
            ("2026-03-15", 1) => "2026-03-16",
            ("2026-03-28", 1) => "2026-03-29",
            ("2026-03-29", 13) => "2026-04-11",
            ("2026-04-11", 1) => "2026-04-12",
            ("2026-04-12", 13) => "2026-04-25",
            ("2026-04-12", 1) => "2026-04-13",
            _ => return Err(failure("fixture_calendar_unavailable")),
        };
        Ok(value.to_owned())
    }

    /// resolve only the fixture's admitted Los Angeles midnights
    fn local_midnight_utc(
        &mut self,
        date: &str,
        timezone: &str,
    ) -> Result<OpenMeteoArchiveInstant, TaskFailure> {
        assert_eq!(timezone, "America/Los_Angeles");
        let (epoch_milliseconds, utc) = match date {
            "2026-03-01" => (1_772_352_000_000, "2026-03-01T08:00:00.000Z"),
            "2026-03-08" => (1_772_956_800_000, "2026-03-08T08:00:00.000Z"),
            "2026-03-09" => (1_773_039_600_000, "2026-03-09T07:00:00.000Z"),
            "2026-03-15" => (1_773_558_000_000, "2026-03-15T07:00:00.000Z"),
            "2026-03-16" => (1_773_644_400_000, "2026-03-16T07:00:00.000Z"),
            "2026-03-29" => (1_774_767_600_000, "2026-03-29T07:00:00.000Z"),
            "2026-04-12" => (1_775_977_200_000, "2026-04-12T07:00:00.000Z"),
            "2026-04-13" => (1_776_063_600_000, "2026-04-13T07:00:00.000Z"),
            _ => return Err(failure("fixture_midnight_unavailable")),
        };
        Ok(OpenMeteoArchiveInstant {
            epoch_milliseconds,
            utc: utc.to_owned(),
        })
    }

    /// return the original fixed default provider and run deadlines
    fn deadlines(&mut self) -> Result<TideBackfillDeadlines, TideBackfillOperationError> {
        Ok(TideBackfillDeadlines {
            provider_deadline_at_ms: PROVIDER_DEADLINE_MS,
            run_deadline_at_ms: RUN_DEADLINE_MS,
            started_at_ms: STARTED_AT_MS,
        })
    }
}

struct FixtureSession;

#[derive(Default)]
struct DryRepository {
    resume_events: Vec<String>,
}

impl OpenMeteoArchiveRepository for DryRepository {
    type Session = FixtureSession;

    /// retain each resume read before dry-run planning
    fn has_successful_chunk(
        &mut self,
        identity: &BackfillChunkIdentity,
    ) -> Result<bool, TaskFailure> {
        self.resume_events
            .push(format!("resume:{}", identity.intervalStart));
        Ok(false)
    }

    /// reject source locks during dry-run
    fn acquire_source_session(&mut self, _: &str) -> Result<Option<Self::Session>, TaskFailure> {
        panic!("dry-run acquired a source session")
    }

    /// reject durable cleanup during dry-run
    fn abandon_expired_runs(
        &mut self,
        _: &mut Self::Session,
        _: i64,
    ) -> Result<(), TideBackfillOperationError> {
        panic!("dry-run abandoned runs")
    }

    /// reject durable starts during dry-run
    fn start_ingestion_run(
        &mut self,
        _: &mut Self::Session,
        _: OpenMeteoArchiveRunStart,
    ) -> Result<String, TideBackfillOperationError> {
        panic!("dry-run started an ingestion run")
    }

    /// reject durable completion during dry-run
    fn complete_backfill_ingestion(
        &mut self,
        _: &mut Self::Session,
        _: OpenMeteoArchiveCompletion,
    ) -> Result<(), TideBackfillOperationError> {
        panic!("dry-run completed an ingestion run")
    }

    /// reject durable failure during dry-run
    fn fail_ingestion_run(
        &mut self,
        _: &mut Self::Session,
        _: &str,
        _: &BackfillChunkIdentity,
        _: &TideBackfillProviderFailure,
    ) -> Result<(), OpenMeteoArchiveCleanupFailure> {
        panic!("dry-run finalized an ingestion run")
    }

    /// reject session release when no dry-run lock exists
    fn release_source_session(
        &mut self,
        _: Self::Session,
    ) -> Result<(), OpenMeteoArchiveCleanupFailure> {
        panic!("dry-run released a source session")
    }
}

struct ForbiddenProvider;

impl OpenMeteoArchiveProvider for ForbiddenProvider {
    /// reject provider traffic during dry-run
    fn fetch(
        &mut self,
        _: OpenMeteoArchiveRequest,
    ) -> Result<ProviderBatch<NormalizedWeatherRecord, JsonValue, ()>, TideBackfillOperationError>
    {
        panic!("dry-run fetched Archive data")
    }
}

struct LifecycleRepository {
    events: Events,
    resume_reads: usize,
    runs: usize,
}

impl OpenMeteoArchiveRepository for LifecycleRepository {
    type Session = FixtureSession;

    /// resume only the first exact planned identity
    fn has_successful_chunk(
        &mut self,
        identity: &BackfillChunkIdentity,
    ) -> Result<bool, TaskFailure> {
        self.resume_reads += 1;
        self.events
            .borrow_mut()
            .push(format!("resume:{}", identity.intervalStart));
        Ok(self.resume_reads == 1)
    }

    /// retain one source-scoped lock for each attempted chunk
    fn acquire_source_session(
        &mut self,
        source_id: &str,
    ) -> Result<Option<Self::Session>, TaskFailure> {
        self.events.borrow_mut().push(format!("lock:{source_id}"));
        Ok(Some(FixtureSession))
    }

    /// retain expired-run cleanup before each committed start
    fn abandon_expired_runs(
        &mut self,
        _: &mut Self::Session,
        started_at_ms: i64,
    ) -> Result<(), TideBackfillOperationError> {
        self.events
            .borrow_mut()
            .push(format!("abandon:{started_at_ms}"));
        Ok(())
    }

    /// retain global UTC bounds beside the per-chunk local dates
    fn start_ingestion_run(
        &mut self,
        _: &mut Self::Session,
        start: OpenMeteoArchiveRunStart,
    ) -> Result<String, TideBackfillOperationError> {
        self.runs += 1;
        self.events.borrow_mut().push(format!(
            "start:{}:{}:{}:{}:{}:{}:{}:{}",
            start.requested_start,
            start.requested_end_exclusive,
            start.start_date,
            start.end_date,
            start.endpoint,
            start.adapter_version,
            start.chunk_plan_version,
            start.deadline_at_ms,
        ));
        Ok(format!("run-{}", self.runs))
    }

    /// retain successful provider attempts and exact identity
    fn complete_backfill_ingestion(
        &mut self,
        _: &mut Self::Session,
        completion: OpenMeteoArchiveCompletion,
    ) -> Result<(), TideBackfillOperationError> {
        self.events.borrow_mut().push(format!(
            "complete:{}:{}:{}:{}",
            completion.attempts,
            completion.identity.intervalStart,
            completion.run_id,
            completion.upstream_response_checksum,
        ));
        Ok(())
    }

    /// retain the primary failed provider attempts and exact identity
    fn fail_ingestion_run(
        &mut self,
        _: &mut Self::Session,
        run_id: &str,
        identity: &BackfillChunkIdentity,
        failure: &TideBackfillProviderFailure,
    ) -> Result<(), OpenMeteoArchiveCleanupFailure> {
        self.events.borrow_mut().push(format!(
            "fail:{}:{}:{}:{}:{}",
            failure.attempts,
            identity.intervalStart,
            failure.error.code,
            run_id,
            failure.error.message,
        ));
        Err(OpenMeteoArchiveCleanupFailure::admitted(
            "finalization timed out".to_owned(),
        ))
    }

    /// retain release after both completion and failure
    fn release_source_session(
        &mut self,
        _: Self::Session,
    ) -> Result<(), OpenMeteoArchiveCleanupFailure> {
        self.events.borrow_mut().push("release".to_owned());
        Err(OpenMeteoArchiveCleanupFailure::admitted(
            "cleanup delayed".to_owned(),
        ))
    }
}

struct LifecycleProvider {
    calls: usize,
    events: Events,
}

impl OpenMeteoArchiveProvider for LifecycleProvider {
    /// complete the first attempted chunk then fail the second
    fn fetch(
        &mut self,
        request: OpenMeteoArchiveRequest,
    ) -> Result<ProviderBatch<NormalizedWeatherRecord, JsonValue, ()>, TideBackfillOperationError>
    {
        self.calls += 1;
        self.events.borrow_mut().push(format!(
            "fetch:{}:{}:{}:{}:{}:{}",
            self.calls,
            request.start_date,
            request.end_date,
            request.timezone,
            request.source_id,
            request.deadline_at_ms,
        ));
        // return one original-shaped record for the successful chunk
        if self.calls == 1 {
            return Ok(ProviderBatch {
                attempts: 2,
                provider_cursor: (),
                records: vec![weather_record("2026-03-15T07:00:00.000Z")],
                response_metadata: JsonValue::Object(vec![(
                    "fixture".encode_utf16().collect(),
                    JsonValue::String("archive".encode_utf16().collect()),
                )]),
                upstream_response_checksum: "d".repeat(64),
            });
        }
        // preserve all three attempts when a returned but unusable batch reaches the core guard
        Ok(ProviderBatch {
            attempts: 3,
            provider_cursor: (),
            records: Vec::new(),
            response_metadata: JsonValue::Object(Vec::new()),
            upstream_response_checksum: "d".repeat(64),
        })
    }
}

/// compare Rust planning, dry-run, and default lifecycle to compiled original Node
#[test]
fn matches_compiled_archive_planning_and_lifecycle() {
    let oracle = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/open_meteo_archive_backfill_node_oracle.mjs");
    let output = Command::new("node")
        .arg(oracle)
        .output()
        .expect("compiled Node Archive oracle starts");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let expected = String::from_utf8(output.stdout).expect("oracle output is UTF-8");
    let actual = rust_projection().join("\n") + "\n";
    assert_eq!(actual, expected);
}

/// project the typed Rust results into the oracle's fixed tab rows
fn rust_projection() -> Vec<String> {
    let source = source();
    let mut rows = Vec::new();
    let planned = plan_open_meteo_archive_chunks(
        &arguments("2026-03-01", "2026-03-15", true, false),
        &source,
        &mut FixtureTime,
    )
    .expect("spring plan succeeds");
    // retain each planned original chunk in order
    for (index, chunk) in planned.iter().enumerate() {
        rows.push(row(
            "PLAN",
            vec![
                index.to_string(),
                chunk.start_date.clone(),
                chunk.end_date.clone(),
            ]
            .into_iter()
            .chain(identity_fields(&chunk.identity)),
        ));
    }
    let spring_day = plan_open_meteo_archive_chunks(
        &BackfillArguments {
            chunkDays: 1,
            ..arguments("2026-03-08", "2026-03-08", false, false)
        },
        &source,
        &mut FixtureTime,
    )
    .expect("spring transition day plans")
    .remove(0);
    rows.push(row(
        "SPRING_DAY",
        vec![spring_day.start_date, spring_day.end_date]
            .into_iter()
            .chain(identity_fields(&spring_day.identity)),
    ));

    let mut dry_repository = DryRepository::default();
    let dry_report = execute_open_meteo_archive_backfill(
        &arguments("2026-03-01", "2026-03-15", true, true),
        &source,
        &mut dry_repository,
        &mut ForbiddenProvider,
        &mut FixtureTime,
    )
    .expect("dry-run succeeds");
    rows.push(report_row("DRY_REPORT", &dry_report, false));
    rows.push(row("DRY_EVENTS", [dry_repository.resume_events.join("|")]));

    let events = Rc::new(RefCell::new(Vec::new()));
    let mut repository = LifecycleRepository {
        events: Rc::clone(&events),
        resume_reads: 0,
        runs: 0,
    };
    let mut provider = LifecycleProvider {
        calls: 0,
        events: Rc::clone(&events),
    };
    let lifecycle = execute_open_meteo_archive_backfill(
        &arguments("2026-03-01", "2026-04-12", true, false),
        &source,
        &mut repository,
        &mut provider,
        &mut FixtureTime,
    )
    .expect("mixed lifecycle reports its provider failure");
    rows.push(report_row("LIFECYCLE_REPORT", &lifecycle, true));
    // retain all attempted and untouched report identities in plan order
    for (index, chunk) in lifecycle.chunks.iter().enumerate() {
        rows.push(row(
            "LIFECYCLE_CHUNK",
            vec![
                index.to_string(),
                status_name(chunk.status).to_owned(),
                nullable_text(&chunk.errorCode).to_owned(),
            ]
            .into_iter()
            .chain(identity_fields(&chunk.identity)),
        ));
    }
    rows.push(row("LIFECYCLE_EVENTS", [events.borrow().join("|")]));
    rows
}

/// project one complete generated report row
fn report_row(label: &str, report: &BackfillReport, include_errors: bool) -> String {
    let mut fields = vec![
        exit_code(report.exitCode).to_owned(),
        report.dryRun.to_string(),
        report.resume.to_string(),
        report.from.clone(),
        report.to.clone(),
        report.site.clone(),
        report.source.clone(),
        report
            .chunks
            .iter()
            .map(|chunk| status_name(chunk.status))
            .collect::<Vec<_>>()
            .join(","),
    ];
    // include the lifecycle diagnostics omitted by the compact dry-run row
    if include_errors {
        fields.push(
            report
                .chunks
                .iter()
                .map(|chunk| nullable_text(&chunk.errorCode))
                .collect::<Vec<_>>()
                .join(","),
        );
        fields.push(
            report
                .chunks
                .iter()
                .map(|chunk| nullable_text(&chunk.secondaryError))
                .collect::<Vec<_>>()
                .join(","),
        );
    }
    row(label, fields)
}

/// project one generated chunk identity in original field order
fn identity_fields(identity: &BackfillChunkIdentity) -> impl Iterator<Item = String> {
    [
        identity.adapterVersion.clone(),
        identity.chunkPlanVersion.clone(),
        identity.intervalStart.clone(),
        identity.intervalEndExclusive.clone(),
        identity.requestedFromDate.clone(),
        identity.requestedToDate.clone(),
        identity.sourceConfigFingerprint.clone(),
        identity.sourceId.clone(),
    ]
    .into_iter()
}

/// retain one stable tab-separated oracle row
fn row(label: &str, fields: impl IntoIterator<Item = String>) -> String {
    std::iter::once(label.to_owned())
        .chain(fields)
        .collect::<Vec<_>>()
        .join("\t")
}

/// construct the exact generated source admitted by both implementations
fn source() -> OpenMeteoArchiveSource {
    OpenMeteoArchiveSource::admit(
        BackfillSource {
            materialProviderConfig: JsonValue::Null,
            providerKey: "open-meteo".to_owned(),
            siteSlug: "ballydidean".to_owned(),
            sourceConfigFingerprint: FINGERPRINT.to_owned(),
            sourceKey: "open-meteo-reanalysis-v1".to_owned(),
            sourceKind: SourceKind::Reanalysis,
            stationSlug: "open-meteo-virtual".to_owned(),
            timezone: "America/Los_Angeles".to_owned(),
            id: SOURCE_ID.to_owned(),
            key: "open-meteo-reanalysis-v1".to_owned(),
            latitude: 47.950429954185445,
            longitude: -122.42797012608193,
        },
        "archive-hourly/v1",
        14,
        true,
    )
    .expect("fixture source is admitted")
}

/// construct one original-shaped command with explicit range and controls
fn arguments(from: &str, to: &str, resume: bool, dry_run: bool) -> BackfillArguments {
    BackfillArguments {
        chunkDays: 14,
        dryRun: dry_run,
        from: from.to_owned(),
        reportPath: Nullable::Null,
        resume,
        site: "ballydidean".to_owned(),
        source: Nullable::Null,
        to: to.to_owned(),
    }
}

/// construct one sufficient normalized provider row inside the successful chunk
fn weather_record(valid_at: &str) -> NormalizedWeatherRecord {
    NormalizedWeatherRecord {
        metadata: WeatherRecordMetadata {
            device: Nullable::Null,
            model: Nullable::Null,
            provider: Nullable::Null,
            quality: Nullable::Null,
            upstreamTimezone: "America/Los_Angeles".to_owned(),
        },
        metrics: empty_metrics(),
        productRunAt: Nullable::Null,
        receivedAt: "2026-08-22T05:20:00.000Z".to_owned(),
        sourceId: SOURCE_ID.to_owned(),
        sourceKind: SourceKind::Reanalysis,
        validAt: valid_at.to_owned(),
    }
}

/// construct generated metrics with every optional reading absent
fn empty_metrics() -> CanonicalWeatherMetrics {
    CanonicalWeatherMetrics {
        apparentTemperatureC: Nullable::Null,
        blackGlobeTemperatureC: Nullable::Null,
        cloudCoverPercent: Nullable::Null,
        pm25MicrogramsPerCubicMeter: Nullable::Null,
        precipitationMm: Nullable::Null,
        precipitationRateMmPerHour: Nullable::Null,
        pressureHpa: Nullable::Null,
        relativeHumidityPercent: Nullable::Null,
        soilElectricalConductivityMicrosiemensPerCm: Nullable::Null,
        soilMoisturePercent: Nullable::Null,
        solarRadiationWm2: Nullable::Null,
        temperatureC: Nullable::Null,
        uvIndex: Nullable::Null,
        waterLevelM: Nullable::Null,
        windDirectionDegrees: Nullable::Null,
        windGustMps: Nullable::Null,
        windSpeedMps: Nullable::Null,
        wetBulbGlobeTemperatureC: Nullable::Null,
    }
}

/// render the original chunk status literal
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

/// render the generated report exit value
const fn exit_code(code: BackfillReportExitCodeEnum) -> &'static str {
    match code {
        BackfillReportExitCodeEnum::Value0 => "0",
        BackfillReportExitCodeEnum::Value1 => "1",
    }
}

/// project generated nullability without a document codec
fn nullable_text(value: &Nullable<String>) -> &str {
    match value {
        Nullable::Null => "<null>",
        Nullable::Value(value) => value,
    }
}

/// construct one stable fixture boundary failure
fn failure(code: &str) -> TaskFailure {
    TaskFailure {
        attempts: 0,
        code: code.to_owned(),
    }
}
