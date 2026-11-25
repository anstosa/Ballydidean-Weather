use std::path::PathBuf;
use std::process::Command;
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use weather_contracts::{
    JsonValue, Nullable, PublicStationAdapter, PublicStationConfiguration,
    PublicStationConfigurationStation, PublicStationSourceConfiguration, SourceCapability,
};
use weather_runtime::{
    PublicStationBackfillPlanningRepository, PublicStationStartupBackfill,
    PublicStationStartupConfiguration, PublicStationStartupDiagnostic,
    PublicStationStartupDiagnosticEvent, PublicStationStartupDiagnostics,
    PublicStationStartupHandle, PublicStationStartupInput, PublicStationStartupOutcome,
    PublicStationStartupPool, PublicStationStartupTime, SupplementalPublicStationBackfillProvider,
};
use weather_worker::{
    AmbientWeatherBackfillProvider, AmbientWeatherProviderCursor, AmbientWeatherRangeRequest,
    AmbientWeatherResponseMetadata, NormalizedAmbientWeatherRecord, ProviderBatch,
    PublicStationBackfillChunkIdentity, PublicStationBackfillCompletion, PublicStationBackfillDate,
    PublicStationBackfillDeadlines, PublicStationBackfillOperationError,
    PublicStationBackfillPlanningSource, PublicStationBackfillProviderFailure,
    PublicStationBackfillRepository, PublicStationBackfillRunStart,
    PublicStationBackfillTimeAdapter, SupplementalPublicStationBackfillRepository,
    SupplementalPublicStationBatch, SupplementalPublicStationIdentity,
    SupplementalPublicStationRecord, SupplementalPublicStationResponseMetadata,
    SupplementalPublicStationScheduledRequest, TaskFailure, WorkerProcessBackfillPort,
};

type Trace = Arc<Mutex<Vec<String>>>;
const OCTOBER_19_MS: i64 = 1_792_368_000_000;
const OCTOBER_20_MS: i64 = 1_792_454_400_000;
const PURPLE_SOURCE_ID: &str = "source-purpleair";

struct FixtureConfiguration(Option<PublicStationStartupInput>);

impl PublicStationStartupConfiguration for FixtureConfiguration {
    /// Share the configured startup archive with the background job.
    fn public_station_startup_input(&self) -> Option<PublicStationStartupInput> {
        self.0.clone()
    }
}

struct FixtureRepository {
    entered: SyncSender<()>,
    release: Receiver<()>,
    trace: Trace,
    planning: Vec<PublicStationBackfillPlanningSource>,
    fails: bool,
}

impl PublicStationBackfillRepository for FixtureRepository {
    type Session = String;

    /// Resume three sources and execute the final PurpleAir chunk.
    fn has_successful_chunk(
        &mut self,
        identity: &PublicStationBackfillChunkIdentity,
    ) -> Result<bool, TaskFailure> {
        self.trace
            .lock()
            .expect("trace")
            .push(format!("lookup:{}", identity.source_id));
        Ok(identity.source_id != PURPLE_SOURCE_ID)
    }

    /// Acquire only the one non-resumed source session.
    fn acquire_source_session(
        &mut self,
        source_id: &str,
    ) -> Result<Option<Self::Session>, TaskFailure> {
        assert_eq!(source_id, PURPLE_SOURCE_ID);
        self.trace
            .lock()
            .expect("trace")
            .push("acquire:purpleair".to_owned());
        Ok(Some(source_id.to_owned()))
    }

    /// Recover prior runs before starting the retained chunk.
    fn abandon_expired_runs(
        &mut self,
        session: &mut Self::Session,
        now_ms: i64,
    ) -> Result<(), PublicStationBackfillOperationError> {
        assert_eq!(session, PURPLE_SOURCE_ID);
        assert_eq!(now_ms, OCTOBER_20_MS + 1_000);
        self.trace
            .lock()
            .expect("trace")
            .push("abandon:purpleair".to_owned());
        Ok(())
    }

    /// Capture the same committed run metadata as the one-shot command.
    fn start_ingestion_run(
        &mut self,
        session: &mut Self::Session,
        start: PublicStationBackfillRunStart,
    ) -> Result<String, PublicStationBackfillOperationError> {
        assert_eq!(session, PURPLE_SOURCE_ID);
        assert_eq!(start.endpoint, "purpleair");
        assert_eq!(start.station_key, "purple-station");
        assert_eq!(start.requested_start_ms, OCTOBER_19_MS);
        assert_eq!(start.requested_end_exclusive_ms, OCTOBER_20_MS);
        self.trace
            .lock()
            .expect("trace")
            .push("start:purpleair".to_owned());
        Ok("run-purpleair".to_owned())
    }

    /// Reject an Ambient completion because that source resumed.
    fn complete_backfill_ingestion(
        &mut self,
        _: &mut Self::Session,
        _: PublicStationBackfillCompletion,
    ) -> Result<(), PublicStationBackfillOperationError> {
        unreachable!("Ambient source resumed")
    }

    /// Reject failure finalization from the successful fixture.
    fn fail_ingestion_run(
        &mut self,
        _: &mut Self::Session,
        _: &str,
        _: &PublicStationBackfillChunkIdentity,
        _: &PublicStationBackfillProviderFailure,
    ) -> Result<(), TaskFailure> {
        unreachable!("fixture provider succeeds")
    }

    /// Release the same source after its successful completion.
    fn release_source_session(&mut self, session: Self::Session) -> Result<(), TaskFailure> {
        assert_eq!(session, PURPLE_SOURCE_ID);
        self.trace
            .lock()
            .expect("trace")
            .push("release:purpleair".to_owned());
        Ok(())
    }
}

impl SupplementalPublicStationBackfillRepository for FixtureRepository {
    /// Commit the empty successful PurpleAir receipt.
    fn complete_supplemental_backfill_ingestion(
        &mut self,
        session: &mut Self::Session,
        completion: PublicStationBackfillCompletion<
            SupplementalPublicStationRecord,
            SupplementalPublicStationResponseMetadata,
        >,
    ) -> Result<(), PublicStationBackfillOperationError> {
        assert_eq!(session, PURPLE_SOURCE_ID);
        assert_eq!(completion.run_id, "run-purpleair");
        assert_eq!(completion.attempts, 1);
        assert!(completion.records.is_empty());
        assert_eq!(completion.response_metadata.provider, "purpleair");
        self.trace
            .lock()
            .expect("trace")
            .push("complete:purpleair".to_owned());
        Ok(())
    }
}

impl PublicStationBackfillPlanningRepository for FixtureRepository {
    /// Keep source resolution pending so startup remains demonstrably nonblocking.
    fn resolve_public_station_planning_sources(
        &mut self,
        configuration: &PublicStationConfiguration,
        selectors: &[String],
    ) -> Result<Vec<PublicStationBackfillPlanningSource>, TaskFailure> {
        assert_eq!(configuration.siteKey, "ballydidean");
        assert!(selectors.is_empty());
        self.trace.lock().expect("trace").push("resolve".to_owned());
        self.entered
            .send(())
            .expect("startup thread reached resolution");
        self.release
            .recv_timeout(Duration::from_secs(4))
            .expect("main process releases work");
        // contain an ordinary catalog read failure before choosing the cutoff
        if self.fails {
            return Err(TaskFailure {
                attempts: 0,
                code: "fixture database unavailable".to_owned(),
            });
        }
        Ok(self.planning.clone())
    }
}

struct FixturePool(Option<FixtureRepository>);

impl PublicStationStartupPool for FixturePool {
    type Repository = FixtureRepository;

    /// Return the one available handle from the worker's shared pool.
    fn share_startup_repository(&mut self) -> Result<Self::Repository, TaskFailure> {
        Ok(self.0.take().expect("pool shared only once"))
    }
}

struct AmbientFixture;

impl AmbientWeatherBackfillProvider for AmbientFixture {
    /// Reject Ambient traffic because its exact chunk resumed.
    fn fetch(
        &mut self,
        _: AmbientWeatherRangeRequest,
        _: i64,
    ) -> Result<
        ProviderBatch<
            NormalizedAmbientWeatherRecord,
            AmbientWeatherResponseMetadata,
            Option<AmbientWeatherProviderCursor>,
        >,
        PublicStationBackfillOperationError,
    > {
        unreachable!("Ambient source resumed")
    }
}

struct SupplementalFixture(Trace);

impl SupplementalPublicStationBackfillProvider for SupplementalFixture {
    /// Return one admitted empty PurpleAir batch after the committed run starts.
    fn fetch(
        &mut self,
        request: SupplementalPublicStationScheduledRequest,
        deadline_at_ms: i64,
    ) -> Result<SupplementalPublicStationBatch, PublicStationBackfillOperationError> {
        assert_eq!(request.source_id, PURPLE_SOURCE_ID);
        assert_eq!(request.timezone, "America/Los_Angeles");
        assert_eq!(request.window.start_ms, OCTOBER_19_MS);
        assert_eq!(request.window.end_exclusive_ms, OCTOBER_20_MS);
        assert_eq!(deadline_at_ms, OCTOBER_20_MS + 2_000);
        assert!(matches!(
            request.identity,
            SupplementalPublicStationIdentity::PurpleAir {
                map_version,
                sensor_index: 1234,
            } if map_version == "1.2.3"
        ));
        self.0
            .lock()
            .expect("trace")
            .push("fetch:purpleair".to_owned());
        Ok(ProviderBatch {
            attempts: 1,
            provider_cursor: None,
            records: Vec::new(),
            response_metadata: SupplementalPublicStationResponseMetadata {
                measurement_request_count: None,
                normalized_record_count: 0,
                provider: "purpleair",
                raw_observation_count: Some(0),
                resolution_seconds: Some(120),
            },
            upstream_response_checksum: "a".repeat(64),
        })
    }
}

struct FixtureTime(Trace);

impl PublicStationBackfillTimeAdapter for FixtureTime {
    /// Return the frozen current UTC date.
    fn today_utc(&mut self) -> Result<PublicStationBackfillDate, TaskFailure> {
        self.0.lock().expect("trace").push("today".to_owned());
        Ok(PublicStationBackfillDate::admitted(
            "2026-10-20",
            OCTOBER_20_MS,
        ))
    }

    /// Perform the exact two-day calendar transitions used by this archive.
    fn add_utc_days(
        &mut self,
        date: &PublicStationBackfillDate,
        days: i64,
    ) -> Result<PublicStationBackfillDate, TaskFailure> {
        let result_ms = date.start_ms() + days * 86_400_000;
        let label = match result_ms {
            OCTOBER_19_MS => "2026-10-19",
            OCTOBER_20_MS => "2026-10-20",
            _ => panic!("unexpected fixture calendar transition"),
        };
        // distinguish only the startup cutoff observation from chunk planning
        if (date.start_ms(), days) == (OCTOBER_20_MS, -1) {
            self.0.lock().expect("trace").push("yesterday".to_owned());
        }
        Ok(PublicStationBackfillDate::admitted(label, result_ms))
    }

    /// Return one ordered provider and durable finalization deadline plan.
    fn deadlines(
        &mut self,
    ) -> Result<PublicStationBackfillDeadlines, PublicStationBackfillOperationError> {
        Ok(PublicStationBackfillDeadlines {
            started_at_ms: OCTOBER_20_MS + 1_000,
            provider_deadline_at_ms: OCTOBER_20_MS + 2_000,
            run_deadline_at_ms: OCTOBER_20_MS + 3_000,
        })
    }
}

impl PublicStationStartupTime for FixtureTime {
    /// Feed deterministic elapsed time to the normal startup diagnostics.
    fn diagnostic_now(&mut self) -> i64 {
        let mut trace = self.0.lock().expect("trace");
        trace.push("diagnostic-clock".to_owned());
        i64::try_from(trace.len()).expect("bounded fixture trace") * 100
    }
}

struct FixtureLog(Trace);

impl PublicStationStartupDiagnostics for FixtureLog {
    /// Capture only redacted fields exposed by the worker startup owner.
    fn record(&self, diagnostic: PublicStationStartupDiagnostic<'_>) {
        assert_eq!(diagnostic.release, "fixture-release");
        assert!(diagnostic.duration_ms >= 0);
        let event = match diagnostic.event {
            PublicStationStartupDiagnosticEvent::SourceRun => "source_run",
            PublicStationStartupDiagnosticEvent::WorkerIteration => "worker_iteration",
        };
        self.0.lock().expect("trace").push(format!(
            "diagnostic:{event}:{}:{}:{}",
            diagnostic.source_id.unwrap_or("-"),
            diagnostic.count,
            diagnostic.error_code.unwrap_or("-")
        ));
    }
}

// construct four historical adapters from one checked generated contract
fn configuration() -> PublicStationConfiguration {
    let sources = [
        (
            "ambient",
            PublicStationAdapter::AmbientWeather,
            vec![
                text("deviceId", "0123456789abcdef01234567"),
                text("macAddress", "AA:BB:CC:DD:EE:FF"),
            ],
        ),
        (
            "wunderground",
            PublicStationAdapter::WeatherUnderground,
            vec![
                text("stationId", "KWAREHOUSE1"),
                text("publicApiKey", "fixture-key"),
            ],
        ),
        (
            "netatmo",
            PublicStationAdapter::Netatmo,
            vec![
                text("deviceId", "aa:bb:cc:dd:ee:ff"),
                text("outdoorModuleId", "11:22:33:44:55:66"),
                text("rainModuleId", "22:33:44:55:66:77"),
                text("windModuleId", "33:44:55:66:77:88"),
            ],
        ),
        (
            "purple",
            PublicStationAdapter::Purpleair,
            vec![
                text("mapVersion", "1.2.3"),
                (
                    "sensorIndex".encode_utf16().collect(),
                    JsonValue::Number(1234.0),
                ),
            ],
        ),
    ];
    PublicStationConfiguration {
        siteKey: "ballydidean".to_owned(),
        providers: Vec::new(),
        stations: sources
            .into_iter()
            .enumerate()
            .map(
                |(index, (name, adapter, adapter_config))| PublicStationConfigurationStation {
                    active: true,
                    displayName: name.to_owned(),
                    key: format!("{name}-station"),
                    latitude: 47.9,
                    longitude: -122.4,
                    model: Nullable::Null,
                    serial: Nullable::Null,
                    sources: vec![PublicStationSourceConfiguration {
                        active: true,
                        adapter,
                        adapterConfig: adapter_config,
                        cadenceSeconds: Nullable::Null,
                        capabilities: vec![SourceCapability::Historical],
                        fingerprint: format!("{:064x}", index + 1),
                        historyEndDate: Nullable::Null,
                        historyStartDate: Nullable::Value("2026-10-19".to_owned()),
                        key: format!("{name}-history"),
                        maximumChunkDays: 1,
                        providerKey: name.to_owned(),
                    }],
                    timezone: "America/Los_Angeles".to_owned(),
                    vendor: Nullable::Null,
                },
            )
            .collect(),
        version: 1,
    }
}

// retain ordered JSON string fields through generated UTF-16 keys and values
fn text(key: &str, value: &str) -> (Vec<u16>, JsonValue) {
    (
        key.encode_utf16().collect(),
        JsonValue::String(value.encode_utf16().collect()),
    )
}

// recover the generated wire value used by the original database fixture
fn adapter_wire_value(adapter: PublicStationAdapter) -> &'static str {
    PublicStationAdapter::WIRE_VALUES
        .iter()
        .find_map(|&(wire, value)| (value == adapter).then_some(wire))
        .expect("known public-station adapter")
}

// derive the database-resolved planning order from the checked catalog
fn planning_sources(
    configuration: &PublicStationConfiguration,
) -> Vec<PublicStationBackfillPlanningSource> {
    configuration
        .stations
        .iter()
        .map(|station| {
            let source = &station.sources[0];
            PublicStationBackfillPlanningSource {
                adapter: source.adapter,
                source_id: format!("source-{}", adapter_wire_value(source.adapter)),
                source_key: source.key.clone(),
                source_config_fingerprint: source.fingerprint.clone(),
                history_start: PublicStationBackfillDate::admitted("2026-10-19", OCTOBER_19_MS),
                history_end: None,
                maximum_chunk_days: 1,
            }
        })
        .collect()
}

#[test]
// prove nonblocking readiness, all-provider resume, graceful drain, and Node failure order
fn resumes_public_station_archives_in_the_background_after_source_resolution() {
    let oracle = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/public-station-startup-backfill-node-oracle.mjs");
    let execution = Command::new("node")
        .arg(oracle)
        .arg("--mixed")
        .output()
        .expect("Node startup oracle");
    assert!(
        execution.status.success(),
        "{}",
        String::from_utf8_lossy(&execution.stderr)
    );
    let oracle = String::from_utf8(execution.stdout).expect("Node trace");
    let expected_events = oracle
        .lines()
        .find_map(|line| line.strip_prefix("node-mixed-events="))
        .expect("Node mixed startup events");
    let expected_report = oracle
        .lines()
        .find_map(|line| line.strip_prefix("node-mixed-report="))
        .expect("Node mixed startup report");

    // exercise the active workflow and its contained repository failure independently
    for fails in [false, true] {
        let trace = Trace::default();
        let checked = configuration();
        let (entered, entered_wait) = sync_channel(1);
        let (release, release_wait) = sync_channel(1);
        let mut pool = FixturePool(Some(FixtureRepository {
            entered,
            release: release_wait,
            trace: Arc::clone(&trace),
            planning: planning_sources(&checked),
            fails,
        }));
        let mut backfill = PublicStationStartupBackfill::new(
            AmbientFixture,
            SupplementalFixture(Arc::clone(&trace)),
            FixtureTime(Arc::clone(&trace)),
            Arc::new(FixtureLog(Arc::clone(&trace))),
        );
        let disabled = backfill.start(&FixtureConfiguration(None), &mut pool);
        assert!(matches!(disabled, PublicStationStartupHandle::Inactive));
        assert!(pool.0.is_some());
        assert!(trace.lock().expect("trace").is_empty());

        let running = backfill.start(
            &FixtureConfiguration(Some(PublicStationStartupInput {
                configuration: checked,
                release: "fixture-release".to_owned(),
            })),
            &mut pool,
        );
        entered_wait
            .recv_timeout(Duration::from_secs(4))
            .expect("resolver starts in background");
        let PublicStationStartupHandle::Running(task) = &running else {
            panic!("expected one joinable startup job");
        };
        assert!(!task.is_finished());
        assert!(backfill.completed().is_none());
        release.send(()).expect("continue background work");
        <_ as WorkerProcessBackfillPort<FixtureConfiguration, FixturePool>>::drain(
            &mut backfill,
            running,
            &mut pool,
        );
        let trace = trace.lock().expect("trace");
        // compare the contained failure order against the original compiled worker
        if fails {
            assert_eq!(
                backfill.completed(),
                Some(&PublicStationStartupOutcome::Failed)
            );
            assert_eq!(
                &trace[..],
                [
                    "diagnostic-clock",
                    "resolve",
                    "diagnostic-clock",
                    "diagnostic:worker_iteration:-:0:public_station_backfill_failed",
                ]
            );
            assert!(oracle.contains("node-failure=diagnostic-clock,resolve,diagnostic-clock,diagnostic:worker_iteration:public_station_backfill_failed"));
            continue;
        }
        let Some(PublicStationStartupOutcome::Completed(report)) = backfill.completed() else {
            panic!("drain must retain the completed source report");
        };
        assert_eq!(report.site, "ballydidean");
        assert!(report.resume && !report.dry_run);
        let report_signature = report
            .sources
            .iter()
            // compare every original report field in checked catalog order
            .map(|source| {
                format!(
                    "{}|{}|{}|{}|{}|{}|{}|{}",
                    source.source,
                    source.from,
                    source.to,
                    source.planned_chunks,
                    source.skipped_chunks,
                    source.completed_chunks,
                    source.records,
                    source.exit_code,
                )
            })
            .collect::<Vec<_>>()
            .join(";");
        assert_eq!(report_signature, expected_report);
        assert!(
            report.sources[..3]
                .iter()
                .all(|source| source.skipped_chunks == 1)
        );
        assert_eq!(
            (
                report.sources[3].completed_chunks,
                report.sources[3].records
            ),
            (1, 0)
        );
        assert_eq!(
            trace
                .iter()
                .filter(|event| event.starts_with("lookup:"))
                .count(),
            4
        );
        let event_signature = trace
            .iter()
            // omit startup-only clock/cutoff observations from the Node operation comparison
            .filter(|event| {
                [
                    "lookup:",
                    "acquire:",
                    "abandon:",
                    "start:",
                    "fetch:",
                    "complete:",
                    "release:",
                    "diagnostic:source_run:",
                ]
                .iter()
                .any(|prefix| event.starts_with(prefix))
            })
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join(",");
        assert_eq!(event_signature, expected_events);
        assert!(trace.windows(5).any(|events| events
            == [
                "acquire:purpleair",
                "abandon:purpleair",
                "start:purpleair",
                "fetch:purpleair",
                "complete:purpleair",
            ]));
        assert!(trace.iter().any(|event| event == "release:purpleair"));
        assert!(oracle.contains(
            "node-startup=diagnostic-clock,resolve,today,execute:ballydidean:2026-10-19:true"
        ));
    }
}
