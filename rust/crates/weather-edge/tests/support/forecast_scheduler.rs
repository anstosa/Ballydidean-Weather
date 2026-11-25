use std::cell::{Cell, RefCell};
use std::convert::Infallible;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::rc::Rc;
use std::time::Duration;

use weather_contracts::JsonValue;
use weather_edge::{
    AdmittedPublicApiOrigin, BorrowedForecastSchedulerPublicReadyStarter,
    BoundedForecastSchedulerSlotReader, ForecastAllocationStatfsPort,
    ForecastAllocationStatfsSample, ForecastEvidenceMaterial, ForecastEvidenceMaterialCodec,
    ForecastEvidenceOutcome, ForecastScheduleSettlement, ForecastSchedulerCalendarPort,
    ForecastSchedulerCaptureRequest, ForecastSchedulerConfiguration, ForecastSchedulerDayOffset,
    ForecastSchedulerPollDriver, ForecastSchedulerPollStep, ForecastSchedulerPollTimerPort,
    ForecastSchedulerPorts, ForecastSchedulerPublicReadyStarter, ForecastSchedulerStateCodec,
    ForecastSchedulerTick, ForecastSchedulerTriggerPort, ForecastSchedulerUtcBase,
    ForecastSchedulerUtcDay, NativeForecastScheduler, NativeForecastSchedulerPollTarget,
    PublicApiBodyReadOutcome, PublicApiHttpMethod, PublicApiHttpPort, PublicApiHttpRequest,
    PublicApiHttpResponse, PublicApiHttpResponseHead, PublicApiRedirectPolicy,
    RevisionArchiveCanonicalClock,
};

use super::{Codec, Fixture, node, read_typed};

const TERMINAL: &str = "2028-10-08T07:00:00.000Z";
const STEP: i64 = 86_400_000;

#[derive(Default)]
struct Timer(Vec<&'static str>);

impl ForecastSchedulerPollTimerPort for Timer {
    type Interval = u8;
    type Error = Infallible;

    /// Model deferred callback delivery without doing any work from readiness.
    fn schedule_immediate_callback(&mut self) {
        self.0.push("immediate");
    }

    /// Verify the actual scheduler's exact interval and track its allocated handle.
    fn install_unreferenced_interval(&mut self, cadence: Duration) -> Result<u8, Infallible> {
        assert_eq!(cadence, Duration::from_secs(30));
        self.0.push("interval");
        Ok(1)
    }

    /// Record the interval cleared when the native scheduler reaches terminal state.
    fn clear_unreferenced_interval(&mut self, interval: u8) {
        assert_eq!(interval, 1);
        self.0.push("clear");
    }
}

/// Read an exact field from the original Node result written as generated typed JSON.
fn field<'a>(value: &'a JsonValue, key: &str) -> &'a JsonValue {
    let JsonValue::Object(fields) = value else {
        panic!("expected object for {key}");
    };
    let key: Vec<u16> = key.encode_utf16().collect();
    &fields.iter().find(|(name, _)| name == &key).unwrap().1
}

/// Read a scalar without interpreting an application wire document.
fn string(value: &JsonValue) -> String {
    let JsonValue::String(value) = value else {
        panic!("expected a witness string");
    };
    String::from_utf16(value).unwrap()
}

/// Read original array order for triggering and state history.
fn array(value: &JsonValue) -> &[JsonValue] {
    let JsonValue::Array(values) = value else {
        panic!("expected witness array");
    };
    values
}

/// Reuse Node's limited test table without deriving civil dates in Rust.
struct Calendar<'a>(&'a Fixture);

impl ForecastSchedulerCalendarPort for Calendar<'_> {
    /// Find the four precomputed source instants complete with exact UTC spelling.
    fn fixed_utc_day(
        &self,
        epoch: i64,
        offset: ForecastSchedulerDayOffset,
    ) -> Option<ForecastSchedulerUtcDay> {
        let midnight = (epoch.div_euclid(STEP)
            - i64::from(offset == ForecastSchedulerDayOffset::Yesterday))
            * STEP;
        let times = [35, 395, 755, 1115].map(|minutes| {
            let epoch_milliseconds = midnight + minutes * 60_000;
            ForecastSchedulerUtcBase {
                canonical_at: self.canonical_utc(epoch_milliseconds).unwrap(),
                epoch_milliseconds,
            }
        });
        Some(ForecastSchedulerUtcDay {
            bases: times,
            midnight_epoch_milliseconds: midnight,
        })
    }

    /// Find a canonical spelling computed independently by the test's Node clock.
    fn canonical_utc(&self, epoch: i64) -> Option<String> {
        self.0
            .times
            .iter()
            .find_map(|(time, millis)| (*millis == epoch).then(|| time.clone()))
    }

    /// UTC has the fixed step of the source loop for these admitted fixture days.
    fn next_utc_day_observation(&self, epoch: i64) -> Option<i64> {
        epoch.checked_add(STEP)
    }
}

/// Capture only the disposable lifecycle test observations.
struct Clock(&'static str);

impl RevisionArchiveCanonicalClock for Clock {
    /// Return the explicitly staged observation to every clock consumer.
    fn canonical_now(&mut self) -> Option<String> {
        Some(self.0.to_owned())
    }
}

#[derive(Clone, Copy)]
struct StorageWitness;

impl ForecastAllocationStatfsPort for StorageWitness {
    /// Prove the scheduler never probes a non-disposable storage root in this test.
    fn approved_sample(&mut self, root: &Path) -> std::io::Result<ForecastAllocationStatfsSample> {
        assert!(
            root.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("weather-forecast-scheduler-rust-")
        );
        Ok(ForecastAllocationStatfsSample {
            available_blocks: 600_000,
            block_size: 4_096,
            free_inodes: 65_536,
        })
    }
}

/// Independent HTTP boundary records the request; no listener or socket is opened.
struct SlotsHttp {
    body: Vec<u8>,
    calls: Rc<RefCell<Vec<PublicApiHttpRequest>>>,
}

impl PublicApiHttpPort for SlotsHttp {
    type Error = ();
    type Response = SlotsBody;

    /// Supply the exact Node wire response after observing all real request fields.
    fn execute(&self, request: &PublicApiHttpRequest) -> Result<Self::Response, Self::Error> {
        self.calls.borrow_mut().push(request.clone());
        Ok(SlotsBody(self.body.clone()))
    }
}

struct SlotsBody(Vec<u8>);

impl PublicApiHttpResponse for SlotsBody {
    type Error = ();

    /// Present the real route's ordinary successful metadata.
    fn head(&self) -> PublicApiHttpResponseHead {
        PublicApiHttpResponseHead {
            content_length: Some(self.0.len()),
            content_type: Some("application/json".to_owned()),
            status_code: 200,
        }
    }

    /// Force the slot reader to concatenate multiple precise chunks.
    fn read_body(
        &mut self,
        receiver: &mut dyn FnMut(&[u8]) -> bool,
    ) -> Result<PublicApiBodyReadOutcome, Self::Error> {
        for chunk in self.0.chunks(64) {
            // respect the real reader's stop decision
            if !receiver(chunk) {
                return Ok(PublicApiBodyReadOutcome::ReceiverStopped);
            }
        }
        Ok(PublicApiBodyReadOutcome::Complete)
    }
}

#[derive(Default)]
struct Trigger(Vec<ForecastSchedulerCaptureRequest>);

impl ForecastSchedulerTriggerPort for Trigger {
    /// Accept first and catch-up captures, then return the exact injected failure.
    fn trigger_forecast_capture(
        &mut self,
        request: ForecastSchedulerCaptureRequest,
    ) -> Result<ForecastScheduleSettlement, Option<String>> {
        self.0.push(request);
        // only the last attempt receives the source's categorical failure witness
        if self.0.len() == 3 {
            return Err(Some("injected_failure".to_owned()));
        }
        Ok(ForecastScheduleSettlement::Evidence(
            ForecastEvidenceOutcome::Created,
        ))
    }
}

struct Root(PathBuf);

impl Root {
    /// Allocate only a fresh disposable private test root.
    fn create() -> Self {
        use std::os::unix::fs::DirBuilderExt;
        use std::time::{SystemTime, UNIX_EPOCH};
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().canonicalize().unwrap().join(format!(
            "weather-forecast-scheduler-rust-{}-{nonce}",
            std::process::id()
        ));
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&path)
            .unwrap();
        Self(path)
    }
}

impl Drop for Root {
    /// Remove only this disposable test's data.
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Compare a live Rust tick and its complete durable projection against the predecessor.
fn assert_stage(
    oracle: &JsonValue,
    stage: &str,
    tick: ForecastSchedulerTick,
    scheduler: &NativeForecastScheduler,
    codec: &impl ForecastSchedulerStateCodec,
    bytes: &impl ForecastEvidenceMaterialCodec,
) {
    let stage = field(oracle, stage);
    let result = field(stage, "result");
    assert_eq!(tick.status(), string(field(result, "status")));
    match &tick {
        ForecastSchedulerTick::Captured {
            attempt,
            scheduled_at,
        } => {
            assert_eq!(*scheduled_at, string(field(result, "scheduledAt")));
            assert_eq!(attempt.committed_at, string(field(result, "committedAt")));
            assert_eq!(attempt.due_key, string(field(result, "dueKey")));
        }
        ForecastSchedulerTick::CurrentAlreadyProcessed { due_key }
        | ForecastSchedulerTick::CaptureFailed { due_key } => {
            assert_eq!(*due_key, string(field(result, "dueKey")));
        }
        _ => {}
    }
    let mut expected = bytes.canonical_json(field(stage, "state")).unwrap();
    expected.push(b'\n');
    assert_eq!(codec.encode_state(scheduler.status()).unwrap(), expected);
}

/// Compare scheduler restart and terminal filesystem generations to the actual Node scheduler.
#[test]
fn scheduler_lifecycle_matches_node_with_real_native_restart_state() {
    let oracle_path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/forecast-scheduler-node-oracle.mjs");
    let output = Command::new("node").arg(oracle_path).output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let described = String::from_utf8(node(&["describe"], &output.stdout)).unwrap();
    let oracle = read_typed(&mut described.lines());
    let mut fixture = Fixture::load("legacy");
    let times = String::from_utf8(node(&["scheduler-clocks"], b"")).unwrap();
    // retain only the exact fixture epoch values generated by the original runtime
    for line in times.lines() {
        let (time, epoch) = line.split_once('=').unwrap();
        fixture
            .times
            .insert(time.to_owned(), epoch.parse().unwrap());
    }
    let gzip_calls = Cell::new(0);
    let bytes = || Codec {
        fixture: &fixture,
        gzip_calls: &gzip_calls,
    };
    let codec = ForecastEvidenceMaterial::new(bytes());
    let decoder = ForecastEvidenceMaterial::new(bytes());
    let slot_body = bytes()
        .canonical_json(field(&oracle, "scheduleSlots"))
        .unwrap();
    let calls = Rc::new(RefCell::new(Vec::new()));
    let http = SlotsHttp {
        body: slot_body,
        calls: Rc::clone(&calls),
    };
    let mut slots = BoundedForecastSchedulerSlotReader::new(
        Some(AdmittedPublicApiOrigin::admitted(
            "http://weather-fixture.invalid:9800",
        )),
        http,
        decoder,
    );
    let calendar = Calendar(&fixture);
    let mut filesystem = StorageWitness;
    let mut trigger = Trigger::default();
    let mut clock = Clock("2026-10-08T00:40:00.000Z");
    let mut ports = ForecastSchedulerPorts {
        codec: &codec,
        calendar: &calendar,
        clock: &mut clock,
        slots: &mut slots,
        filesystem: &mut filesystem,
        trigger: &mut trigger,
    };
    let root = Root::create();
    let configuration = ForecastSchedulerConfiguration {
        enabled: true,
        migration_ready: true,
    };
    {
        let mut scheduler =
            NativeForecastScheduler::initialize(root.0.clone(), configuration, &mut ports);
        let mut timer = Timer::default();
        let mut driver = ForecastSchedulerPollDriver::new();
        let mut target = NativeForecastSchedulerPollTarget::new(&mut scheduler, &mut ports);
        let calls_before_ready = calls.borrow().len();
        let mut starter =
            BorrowedForecastSchedulerPublicReadyStarter::new(&mut driver, &mut target, &mut timer);
        assert!(starter.start_on_public_ready().unwrap());
        assert!(!starter.start_on_public_ready().unwrap());
        assert_eq!(calls.borrow().len(), calls_before_ready);
        assert_eq!(timer.0, ["immediate", "interval"]);
        let ForecastSchedulerPollStep::Completed(Ok(tick)) =
            driver.immediate_elapsed(&mut target, &mut timer)
        else {
            panic!("expected native scheduler tick from the detached callback");
        };
        assert!(matches!(
            driver.immediate_elapsed(&mut target, &mut timer),
            ForecastSchedulerPollStep::NotPolling
        ));
        driver.stop(&mut target, &mut timer);
        assert_eq!(timer.0, ["immediate", "interval", "clear"]);
        assert_stage(&oracle, "firstCatchup", tick, &scheduler, &codec, &bytes());
    }
    let mut scheduler =
        NativeForecastScheduler::initialize(root.0.clone(), configuration, &mut ports);
    let mut timer = Timer::default();
    let mut driver = ForecastSchedulerPollDriver::new();
    assert!(
        driver
            .start_on_public_ready(
                &mut NativeForecastSchedulerPollTarget::new(&mut scheduler, &mut ports),
                &mut timer,
            )
            .unwrap()
    );
    // compare exact phase results after actual native process-local recreation
    for (time, stage) in [
        ("2026-10-08T00:40:00.000Z", "repeatedAfterRestart"),
        ("2026-10-08T18:40:00.000Z", "missedCatchup"),
        ("2026-10-08T12:40:00.000Z", "rollback"),
        ("2026-10-09T00:40:00.000Z", "failedTrigger"),
        (TERMINAL, "terminal"),
    ] {
        ports.clock.0 = time;
        let mut target = NativeForecastSchedulerPollTarget::new(&mut scheduler, &mut ports);
        let result = if driver.has_pending_immediate() {
            driver.immediate_elapsed(&mut target, &mut timer)
        } else {
            driver.interval_elapsed(&mut target, &mut timer)
        };
        let ForecastSchedulerPollStep::Completed(Ok(tick)) = result else {
            panic!("expected native scheduler tick for {stage}");
        };
        assert_stage(&oracle, stage, tick, &scheduler, &codec, &bytes());
    }
    assert!(!scheduler.is_polling());
    assert!(!driver.has_interval());
    assert_eq!(timer.0, ["immediate", "interval", "clear"]);
    for (name, oracle_key) in [
        ("current.json", "currentStateUtf8"),
        ("previous.json", "previousStateUtf8"),
    ] {
        assert_eq!(
            std::fs::read(root.0.join("scheduler").join(name)).unwrap(),
            string(field(&oracle, oracle_key)).as_bytes()
        );
    }
    let expected = array(field(&oracle, "triggers"));
    assert_eq!(trigger.0.len(), expected.len());
    // prove no extra retry or skipped capture was emitted after a warm restart
    for (actual, expected) in trigger.0.iter().zip(expected) {
        assert_eq!(actual.commit_at, string(field(expected, "commitAt")));
        assert_eq!(actual.slot.due_key(), string(field(expected, "dueKey")));
        assert_eq!(actual.path, string(field(expected, "path")));
        assert_eq!(
            actual.channel,
            weather_edge::ForecastCaptureChannel::SchedulerRequest
        );
    }
    assert_eq!(calls.borrow().len(), 8);
    // bind the exact bounded HTTP method, origin, route, timeout, and content negotiation
    for call in calls.borrow().iter() {
        assert_eq!(call.accept, "application/json");
        assert_eq!(call.method, PublicApiHttpMethod::Get);
        assert_eq!(call.origin.as_str(), "http://weather-fixture.invalid:9800");
        assert_eq!(
            call.pathname,
            "/internal/adjustment-maintenance/schedule-slots"
        );
        assert_eq!(call.search, "");
        assert_eq!(call.redirect, PublicApiRedirectPolicy::Manual);
        assert_eq!(call.timeout_ms, 5_000);
    }
}

impl weather_edge::ForecastCycleHashPair for Codec<'_> {
    /// Use the same Node SHA-256 owner over the exact concatenated fixture segments.
    fn sha256_pair(&self, header: &[u8], payload: &[u8]) -> [u8; 32] {
        let mut bytes = Vec::with_capacity(header.len() + payload.len());
        bytes.extend_from_slice(header);
        bytes.extend_from_slice(payload);
        weather_edge::RevisionArchiveCanonicalCodec::sha256(self, &bytes)
    }
}

/// Reproduce a real predecessor page from native committed evidence and verify its persisted slot.
#[test]
fn scheduler_archive_slot_matches_the_node_page() {
    use weather_edge::{
        BoundForecastArchiveLockClock, BoundForecastArchiveLockCodec, BoundForecastCyclePageCodec,
        ForecastArchiveAppendOutcome, ForecastCaptureChannel, ForecastCaptureSource,
        ForecastCaptureVersion, ForecastCyclePageAppendInput, ForecastCyclePageSlotKind,
        ForecastCyclePageStateInput, ForecastCycleProjection, ForecastCycleProjectionChannel,
        ForecastCycleSha256, ForecastEvidenceWindow, ForecastResponseEvidenceStore,
        ForecastScheduledEvidenceArchive, NativeForecastCaptureStore, NativeForecastCycleDiskSlots,
        NativeForecastSchedulerArchive, append_forecast_cycle_page,
        create_forecast_cycle_page_state, read_committed_scheduler_payload,
        with_native_forecast_archive_lease,
    };

    const COMMITTED_AT: &str = "2026-10-08T00:40:00.123Z";
    const EXPIRES_AT: &str = "2026-10-08T00:45:00.123Z";
    let oracle_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/forecast-scheduler-archive-node-oracle.mjs");
    let output = Command::new("node").arg(oracle_path).output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let described = String::from_utf8(node(&["describe"], &output.stdout)).unwrap();
    let oracle = read_typed(&mut described.lines());
    let expected = field(&oracle, "payload");
    let base64 = string(field(expected, "base64"));
    let expected = node(&["base64-decode"], base64.as_bytes());
    let slot_witness = field(&oracle, "currentSlot");
    let slot_base64 = string(field(slot_witness, "base64"));
    let expected_slot = node(&["base64-decode"], slot_base64.as_bytes());
    let page = field(&oracle, "page");
    let header = field(page, "header");
    let expected_page_sha256 = string(field(page, "pageSha256"));

    let mut fixture = Fixture::load("extended");
    fixture
        .times
        .insert(COMMITTED_AT.to_owned(), 1_791_420_000_123);
    fixture
        .times
        .insert(EXPIRES_AT.to_owned(), 1_791_420_300_123);
    fixture
        .times
        .insert("2026-10-08T00:35:00.000Z".to_owned(), 1_791_419_700_000);
    let gzip_calls = Cell::new(0);
    let bytes = || Codec {
        fixture: &fixture,
        gzip_calls: &gzip_calls,
    };
    let root = Root::create();
    let mut store = NativeForecastCaptureStore::initialize(
        root.0.clone(),
        bytes(),
        StorageWitness,
        &mut Clock(COMMITTED_AT),
    );
    assert!(store.initialized());
    let prepared = store
        .prepare(
            ForecastCaptureSource::Projected(&fixture.forecast),
            Some(ForecastEvidenceWindow::TenDays),
            ForecastCaptureVersion::ChannelBound(ForecastCaptureChannel::SchedulerRequest),
        )
        .unwrap();
    assert_eq!(
        store.commit(&prepared, COMMITTED_AT).unwrap(),
        ForecastEvidenceOutcome::Created
    );
    let material = ForecastEvidenceMaterial::new(bytes());
    let payload = read_committed_scheduler_payload(&root.0, &prepared, &material).unwrap();
    assert_eq!(payload, expected);

    let state = create_forecast_cycle_page_state(ForecastCyclePageStateInput {
        daily_page_count: 0,
        due_key: string(field(header, "dueKey")),
        generation: string(field(header, "generation")).parse().unwrap(),
        local_date: "2026-10-08".to_owned(),
    })
    .unwrap();
    let projections = [ForecastCycleProjection {
        identity_sha256: ForecastCycleSha256::admitted(prepared.edge_receipt_identity_sha256()),
        channel: ForecastCycleProjectionChannel::SchedulerRequest,
    }];
    let bytes = bytes();
    let page_codec = BoundForecastCyclePageCodec::new(&bytes);
    let calendar = Calendar(&fixture);
    let make_archive = || {
        NativeForecastSchedulerArchive::initialize(
            root.0.clone(),
            &material,
            &calendar,
            Clock(COMMITTED_AT),
            StorageWitness,
        )
        .unwrap()
    };
    let mut archive = make_archive();
    assert_eq!(
        archive.append_committed(&state.due_key, &prepared).unwrap(),
        ForecastArchiveAppendOutcome::Settled,
    );
    let physical = root.0.join("online-pages/slots/current.page");
    assert_eq!(std::fs::read(&physical).unwrap(), expected_slot);
    drop(archive);
    // ensure a fresh process converges on the same immutable physical slot
    assert_eq!(
        make_archive()
            .append_committed(&state.due_key, &prepared)
            .unwrap(),
        ForecastArchiveAppendOutcome::Settled,
    );
    assert_eq!(std::fs::read(&physical).unwrap(), expected_slot);
    assert!(
        std::fs::read_dir(root.0.join("online-pages/gaps"))
            .unwrap()
            .next()
            .is_none()
    );

    let lock_codec = BoundForecastArchiveLockCodec::new(&bytes, &calendar);
    let mut clock = Clock(COMMITTED_AT);
    let mut lease_clock = BoundForecastArchiveLockClock::new(&mut clock, &bytes);
    let mut disk = NativeForecastCycleDiskSlots::initialize(
        root.0.clone(),
        BoundForecastCyclePageCodec::new(&bytes),
        StorageWitness,
    )
    .unwrap();
    let lock_path = root.0.join("online-pages/locks/transport.lock");
    let published =
        with_native_forecast_archive_lease(&root.0, &lock_codec, &mut lease_clock, || {
            // the existing process lease must cover the physical slot publication
            let lock = std::fs::read(&lock_path).unwrap();
            let frame = String::from_utf8(node(&["describe"], &lock)).unwrap();
            let lock = read_typed(&mut frame.lines());
            assert_eq!(
                string(field(&lock, "contractVersion")),
                "adjustment-archive-lock/v1"
            );
            assert_eq!(string(field(&lock, "expiresAt")), EXPIRES_AT);
            append_forecast_cycle_page(
                &state,
                ForecastCyclePageAppendInput {
                    payload: &payload,
                    projections: &projections,
                },
                &page_codec,
                &mut disk,
            )
        })
        .unwrap();
    assert!(!lock_path.exists());
    assert_eq!(published.pages.len(), 1);
    assert_eq!(published.slots[0].slot, ForecastCyclePageSlotKind::Current);
    assert_eq!(
        std::fs::read(root.0.join("online-pages/slots/current.page")).unwrap(),
        expected_slot
    );
    let reread = disk
        .read_slot(ForecastCyclePageSlotKind::Current)
        .unwrap()
        .unwrap();
    assert_eq!(reread.header.due_key, state.due_key);
    assert_eq!(reread.header.generation, state.generation);
    let digest: String = reread
        .page_sha256
        .as_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    assert_eq!(digest, expected_page_sha256);
    assert_eq!(reread.projections, projections);
    assert_eq!(reread.payload, payload);
}

/// Decode the exact bytes emitted from one independently published Node file.
fn witnessed_bytes(oracle: &JsonValue, key: &str) -> Vec<u8> {
    let base64 = string(field(field(oracle, key), "base64"));
    node(&["base64-decode"], base64.as_bytes())
}

/// Compare native physical acknowledgement and promotion to the real Node disk ports.
#[test]
fn cycle_ack_promotion_and_recovery_match_node() {
    use weather_edge::{
        BoundForecastArchiveLockClock, BoundForecastArchiveLockCodec, BoundForecastCyclePageCodec,
        FORECAST_CYCLE_TRANSFER_CHECKPOINT_CONTRACT_VERSION, ForecastCycleGapOutcome,
        ForecastCycleGapReason, ForecastCycleGapRecord, ForecastCyclePageAckInput,
        ForecastCyclePageAppendInput, ForecastCyclePageSlotKind, ForecastCyclePageStateInput,
        ForecastCycleProjection, ForecastCycleProjectionChannel, ForecastCycleReconcileStatus,
        ForecastCycleSha256, ForecastCycleTransferCheckpoint,
        NativeForecastCycleDiskAcknowledgements, NativeForecastCycleDiskSlots,
        NativeForecastCycleRecoveryError, NativeForecastCycleTransferCheckpoints,
        ack_forecast_cycle_page, append_forecast_cycle_page, create_forecast_cycle_page_state,
        recover_current_forecast_cycle_slot, recover_native_forecast_cycle,
        with_native_forecast_archive_lease,
    };

    const ACK_AT: &str = "2026-10-08T00:41:00.456Z";
    const ACK_LEASE_END: &str = "2026-10-08T00:46:00.456Z";
    const GAP_AT: &str = "2026-10-08T00:42:00.789Z";
    let output = Command::new("node")
        .arg(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests/forecast-cycle-ack-node-oracle.mjs"),
        )
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let tree = String::from_utf8(node(&["describe"], &output.stdout)).unwrap();
    let oracle = read_typed(&mut tree.lines());
    let state_witness = field(&oracle, "finalState");
    let initial = create_forecast_cycle_page_state(ForecastCyclePageStateInput {
        daily_page_count: 0,
        due_key: string(field(state_witness, "dueKey")),
        generation: string(field(state_witness, "generation")).parse().unwrap(),
        local_date: string(field(state_witness, "localDate")),
    })
    .unwrap();
    let mut fixture = Fixture::load("legacy");
    fixture.times.insert(ACK_AT.to_owned(), 1_791_420_060_456);
    fixture
        .times
        .insert(ACK_LEASE_END.to_owned(), 1_791_420_360_456);
    fixture.times.insert(GAP_AT.to_owned(), 1_791_420_120_789);
    let gzip_calls = Cell::new(0);
    let bytes = Codec {
        fixture: &fixture,
        gzip_calls: &gzip_calls,
    };
    let codec = BoundForecastCyclePageCodec::new(&bytes);
    let root = Root::create();
    let make_slots = || {
        NativeForecastCycleDiskSlots::initialize(
            root.0.clone(),
            BoundForecastCyclePageCodec::new(&bytes),
            StorageWitness,
        )
        .unwrap()
    };
    let mut slots = make_slots();
    let mut state = initial.clone();
    // fill the two exact physical slots with the reference's ordered page payloads
    for (payload, channel, marker) in [
        (
            b"scheduler-page-one".as_slice(),
            ForecastCycleProjectionChannel::SchedulerRequest,
            0x11,
        ),
        (
            b"organic-page-two".as_slice(),
            ForecastCycleProjectionChannel::Organic,
            0x22,
        ),
    ] {
        let projections = [ForecastCycleProjection {
            channel,
            identity_sha256: ForecastCycleSha256::admitted([marker; 32]),
        }];
        state = append_forecast_cycle_page(
            &state,
            ForecastCyclePageAppendInput {
                payload,
                projections: &projections,
            },
            &codec,
            &mut slots,
        )
        .unwrap();
    }
    let slot_dir = root.0.join("online-pages/slots");
    assert_eq!(
        std::fs::read(slot_dir.join("current.page")).unwrap(),
        witnessed_bytes(&oracle, "currentBefore")
    );
    assert_eq!(
        std::fs::read(slot_dir.join("next.page")).unwrap(),
        witnessed_bytes(&oracle, "nextBefore")
    );
    let first = slots
        .read_slot(ForecastCyclePageSlotKind::Current)
        .unwrap()
        .unwrap();
    let first_sha = first.page_sha256;
    let make_transfers = || {
        NativeForecastCycleTransferCheckpoints::initialize(
            root.0.clone(),
            BoundForecastCyclePageCodec::new(&bytes),
            StorageWitness,
        )
        .unwrap()
    };
    let mut checkpoints = make_transfers();
    let checkpoint = ForecastCycleTransferCheckpoint {
        acknowledged_at: ACK_AT.to_owned(),
        checkpoint_sha256: ForecastCycleSha256::admitted([0xaa; 32]),
        contract_version: FORECAST_CYCLE_TRANSFER_CHECKPOINT_CONTRACT_VERSION,
        header: first.header,
        page_sha256: first_sha,
        payload_length: first.payload.len(),
        projections: first.projections,
    };
    let proof = checkpoints
        .persist_transfer_acknowledgement(&checkpoint)
        .unwrap();
    assert!(proof.fsynced);
    assert_eq!(proof.page_sha256, first_sha);
    let hash: String = first_sha
        .as_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let transfer_path = root
        .0
        .join("online-pages/archive-acknowledgements")
        .join(format!("sha256-{hash}.json"));
    assert_eq!(
        std::fs::read(transfer_path).unwrap(),
        witnessed_bytes(&oracle, "transferCheckpoint")
    );
    assert_eq!(
        recover_native_forecast_cycle(&initial.due_key, &slots, &checkpoints, &codec),
        Err(NativeForecastCycleRecoveryError::IncompleteAcknowledgement)
    );
    drop(checkpoints);
    let mut durable = NativeForecastCycleDiskAcknowledgements::new(
        slots,
        BoundForecastCyclePageCodec::new(&bytes),
        StorageWitness,
    );
    let promoted = ack_forecast_cycle_page(
        &state,
        &ForecastCyclePageAckInput {
            acknowledged_at: ACK_AT.to_owned(),
            page_sha256: first_sha,
        },
        &codec,
        &mut durable,
    )
    .unwrap();
    let ack_witness = field(&oracle, "acknowledgement");
    let ack_path = root.0.join("online-pages/acknowledgements").join(format!(
        "sha256-{}.json",
        string(field(ack_witness, "sha256"))
    ));
    assert_eq!(
        std::fs::read(ack_path).unwrap(),
        witnessed_bytes(&oracle, "acknowledgement")
    );
    assert_eq!(
        std::fs::read(slot_dir.join("current.page")).unwrap(),
        witnessed_bytes(&oracle, "promotedCurrent")
    );
    assert!(!slot_dir.join("next.page").exists());
    drop(durable);

    let mut restarted = make_slots();
    let checkpoints = make_transfers();
    // re-create an interrupted next promotion using only this disposable slot tree
    std::fs::rename(slot_dir.join("current.page"), slot_dir.join("next.page")).unwrap();
    let calendar = Calendar(&fixture);
    let lease_codec = BoundForecastArchiveLockCodec::new(&bytes, &calendar);
    let mut clock = Clock(ACK_AT);
    let mut lease_clock = BoundForecastArchiveLockClock::new(&mut clock, &bytes);
    let promoted_slot =
        with_native_forecast_archive_lease(&root.0, &lease_codec, &mut lease_clock, || {
            recover_current_forecast_cycle_slot(&restarted, &checkpoints)
        })
        .unwrap()
        .unwrap();
    assert_eq!(promoted_slot.header.page_index, 1);
    assert!(!slot_dir.join("next.page").exists());
    let recovered =
        recover_native_forecast_cycle(&initial.due_key, &restarted, &checkpoints, &codec).unwrap();
    assert_eq!(recovered.cycle.state, promoted);
    assert_eq!(
        recovered.cycle.status,
        ForecastCycleReconcileStatus::AwaitingAcknowledgement
    );
    assert!(recovered.gaps.is_empty());

    let gap = ForecastCycleGapRecord {
        due_key: initial.due_key.clone(),
        generation: initial.generation,
        projection_identity_sha256: ForecastCycleSha256::admitted([0x33; 32]),
        reason: ForecastCycleGapReason::CapacityRefused,
        occurred_at: GAP_AT.to_owned(),
    };
    let ForecastCycleGapOutcome::Recorded(first) = restarted.record_cycle_gap(gap.clone()).unwrap()
    else {
        panic!("first gap must become durable");
    };
    let gap_sha = string(field(field(&oracle, "gap"), "sha256"));
    let gap_path = root
        .0
        .join("online-pages/gaps")
        .join(format!("sha256-{gap_sha}.json"));
    assert_eq!(
        std::fs::read(gap_path).unwrap(),
        witnessed_bytes(&oracle, "gap")
    );
    let retry = ForecastCycleGapRecord {
        occurred_at: ACK_AT.to_owned(),
        reason: ForecastCycleGapReason::ArchiveRefused,
        ..gap
    };
    assert_eq!(
        restarted.record_cycle_gap(retry).unwrap(),
        ForecastCycleGapOutcome::Retained(first.clone())
    );
    assert_eq!(
        restarted
            .read_cycle_gaps(&initial.due_key, initial.generation)
            .unwrap(),
        vec![first]
    );
}

/// Build the exact required producer population independently of the final page root.
fn producer_root(
    bytes: &Codec<'_>,
    members: &[char],
) -> weather_edge::ForecastCycleRequiredInputRoot {
    let values: Vec<_> = members
        .iter()
        .map(|member| JsonValue::String(member.to_string().repeat(64).encode_utf16().collect()))
        .collect();
    let mut canonical = bytes.canonical_json(&JsonValue::Array(values)).unwrap();
    canonical.push(b'\n');
    let digest = weather_edge::RevisionArchiveCanonicalCodec::sha256(bytes, &canonical);
    weather_edge::ForecastCycleRequiredInputRoot {
        count: members.len(),
        root_sha256: weather_edge::ForecastCycleSha256::admitted(digest),
    }
}

/// Execute the real local command dispatcher with the same stdout contract as Node.
fn archive_fixture_command(
    application: &mut impl weather_edge::ForecastArchiveOperatorApplication<
        Error = weather_edge::NativeForecastArchiveOperatorError,
    >,
    action: &str,
    operands: &[&str],
) -> Vec<u8> {
    let command = weather_edge::parse_forecast_archive_operator_command(action, operands)
        .unwrap()
        .unwrap();
    let mut stdout = Vec::new();
    weather_edge::run_forecast_archive_operator_command(command, application, &mut stdout).unwrap();
    stdout
}

/// Build and recover a native sealed archive closure against Node's real persisted files.
#[test]
fn cycle_input_seal_and_final_manifest_match_node() {
    use weather_edge::{
        BoundForecastArchiveLockClock, BoundForecastArchiveLockCodec, BoundForecastCyclePageCodec,
        FORECAST_CYCLE_TRANSFER_CHECKPOINT_CONTRACT_VERSION, ForecastCycleOnlineNext,
        ForecastCycleOnlineSealRequest, ForecastCyclePageAckInput, ForecastCyclePageAppendInput,
        ForecastCyclePageError, ForecastCyclePageRefusal, ForecastCyclePageSlotKind,
        ForecastCyclePageStateInput, ForecastCycleProjection, ForecastCycleProjectionChannel,
        ForecastCycleRequiredInputs, ForecastCycleSha256, ForecastCycleTransferCheckpoint,
        NativeForecastArchiveOperator, NativeForecastCycleDiskAcknowledgements,
        NativeForecastCycleDiskSlots, NativeForecastCycleFinalAcknowledgements,
        NativeForecastCycleFinalManifests, NativeForecastCycleFinalizationError,
        NativeForecastCycleFinalizationOutcome, NativeForecastCycleInputSeals,
        NativeForecastCycleOnlineTransfer, NativeForecastCycleOnlineTransferError,
        NativeForecastCycleTransferCheckpoints, ack_forecast_cycle_page,
        append_forecast_cycle_page, create_forecast_cycle_page_state,
        encode_forecast_cycle_capsule, finalize_native_forecast_cycle,
        prepare_forecast_cycle_input_seal, recover_native_forecast_cycle,
        validate_forecast_cycle_capsule,
    };

    const FIRST_ACK: &str = "2026-10-08T06:41:00.123Z";
    const SECOND_ACK: &str = "2026-10-08T06:42:00.456Z";
    const SEALED_AT: &str = "2026-10-08T06:43:00.789Z";
    const LOCK_EXPIRES_AT: &str = "2026-10-08T06:48:00.789Z";
    const FINAL_ACK_AT: &str = "2026-10-08T06:45:00.246Z";
    const ACK_LEASE_EXPIRES_AT: &str = "2026-10-08T06:50:00.246Z";
    let output = Command::new("node")
        .arg(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests/forecast-cycle-final-node-oracle.mjs"),
        )
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let tree = String::from_utf8(node(&["describe"], &output.stdout)).unwrap();
    let oracle = read_typed(&mut tree.lines());
    let identity = field(&oracle, "identity");
    let due_key = string(field(identity, "dueKey"));
    let generation = string(field(identity, "generation")).parse().unwrap();

    let mut fixture = Fixture::load("legacy");
    fixture.times.extend([
        (FIRST_ACK.to_owned(), 1_791_441_660_123),
        (SECOND_ACK.to_owned(), 1_791_441_720_456),
        (SEALED_AT.to_owned(), 1_791_441_780_789),
        (LOCK_EXPIRES_AT.to_owned(), 1_791_442_080_789),
        (FINAL_ACK_AT.to_owned(), 1_791_441_900_246),
        (ACK_LEASE_EXPIRES_AT.to_owned(), 1_791_442_200_246),
    ]);
    let gzip_calls = Cell::new(0);
    let bytes = Codec {
        fixture: &fixture,
        gzip_calls: &gzip_calls,
    };
    let codec = BoundForecastCyclePageCodec::new(&bytes);
    let root = Root::create();
    let make_slots = || {
        NativeForecastCycleDiskSlots::initialize(
            root.0.clone(),
            BoundForecastCyclePageCodec::new(&bytes),
            StorageWitness,
        )
        .unwrap()
    };
    let make_transfers = || {
        NativeForecastCycleTransferCheckpoints::initialize(
            root.0.clone(),
            BoundForecastCyclePageCodec::new(&bytes),
            StorageWitness,
        )
        .unwrap()
    };
    let mut slots = make_slots();
    let mut state = create_forecast_cycle_page_state(ForecastCyclePageStateInput {
        daily_page_count: 1,
        due_key: due_key.clone(),
        generation,
        local_date: "2026-10-08".to_owned(),
    })
    .unwrap();
    // write the reference's exact two pages and stable projected identities
    for (payload, channel, marker) in [
        (
            b"scheduler-final-page-one".as_slice(),
            ForecastCycleProjectionChannel::SchedulerRequest,
            0x11,
        ),
        (
            b"organic-final-page-two".as_slice(),
            ForecastCycleProjectionChannel::Organic,
            0x22,
        ),
    ] {
        let projections = [ForecastCycleProjection {
            channel,
            identity_sha256: ForecastCycleSha256::admitted([marker; 32]),
        }];
        state = append_forecast_cycle_page(
            &state,
            ForecastCyclePageAppendInput {
                payload,
                projections: &projections,
            },
            &codec,
            &mut slots,
        )
        .unwrap();
    }
    let mut transfer = make_transfers();
    let mut ack = NativeForecastCycleDiskAcknowledgements::new(
        slots,
        BoundForecastCyclePageCodec::new(&bytes),
        StorageWitness,
    );
    // commit the workstation checkpoint before releasing either page through the local ACK protocol
    for clock in [FIRST_ACK, SECOND_ACK] {
        let page = ack
            .slots()
            .read_slot(ForecastCyclePageSlotKind::Current)
            .unwrap()
            .unwrap();
        let page_sha256 = page.page_sha256;
        transfer
            .persist_transfer_acknowledgement(&ForecastCycleTransferCheckpoint {
                acknowledged_at: clock.to_owned(),
                checkpoint_sha256: ForecastCycleSha256::admitted([0xaa; 32]),
                contract_version: FORECAST_CYCLE_TRANSFER_CHECKPOINT_CONTRACT_VERSION,
                header: page.header,
                page_sha256,
                payload_length: page.payload.len(),
                projections: page.projections,
            })
            .unwrap();
        state = ack_forecast_cycle_page(
            &state,
            &ForecastCyclePageAckInput {
                acknowledged_at: clock.to_owned(),
                page_sha256,
            },
            &codec,
            &mut ack,
        )
        .unwrap();
    }
    assert!(state.slots.is_empty());
    drop((state, ack, transfer));

    let mut slots = make_slots();
    let state = recover_native_forecast_cycle(&due_key, &slots, &make_transfers(), &codec)
        .unwrap()
        .into_page_state();
    let seals = NativeForecastCycleInputSeals::initialize(
        root.0.clone(),
        BoundForecastCyclePageCodec::new(&bytes),
        StorageWitness,
    )
    .unwrap();
    let mut manifests = NativeForecastCycleFinalManifests::new(make_slots()).unwrap();
    assert_eq!(
        finalize_native_forecast_cycle(state.clone(), &seals, &mut manifests, &codec),
        Err(NativeForecastCycleFinalizationError::MissingInputSeal)
    );

    let empty = producer_root(&bytes, &[]);
    let required_inputs = ForecastCycleRequiredInputs {
        actual_best_match: empty,
        native_source: producer_root(&bytes, &['3']),
        rain_gate_input: empty,
        serving_evidence: producer_root(&bytes, &['1', '2']),
        shadow_body: producer_root(&bytes, &['4']),
        shadow_metadata: producer_root(&bytes, &['5']),
        target_revision: producer_root(&bytes, &['6']),
    };
    let expected =
        prepare_forecast_cycle_input_seal(&state, required_inputs, SEALED_AT, &codec).unwrap();
    let calendar = Calendar(&fixture);
    let make_operator = |instant| {
        NativeForecastArchiveOperator::initialize(
            root.0.clone(),
            &bytes,
            &calendar,
            Clock(instant),
            StorageWitness,
        )
        .unwrap()
    };
    let seal_request = ForecastCycleOnlineSealRequest {
        due_key: due_key.clone(),
        generation,
        required_inputs,
    };
    let seal = make_operator(SEALED_AT)
        .seal_inputs(seal_request.clone())
        .unwrap();
    assert_eq!(seal, expected);
    assert_eq!(
        make_operator(FINAL_ACK_AT)
            .seal_inputs(seal_request)
            .unwrap(),
        seal
    );
    let first_manifest = manifests
        .find_final_manifest(&due_key, generation)
        .unwrap()
        .unwrap();
    assert_eq!(
        string(field(first_manifest.manifest.document(), "finalizedAt")),
        SEALED_AT
    );
    let seal_name = format!("sha256-{}.json", string(field(identity, "inputSealSha256")));
    assert_eq!(
        std::fs::read(root.0.join("online-pages/input-seals").join(seal_name)).unwrap(),
        witnessed_bytes(&oracle, "seal")
    );

    // rebuild the same closure after an interrupted final publication on this disposable root
    let witness = field(&oracle, "manifest");
    let name = format!("sha256-{}.json", string(field(witness, "sha256")));
    let original = root.0.join("online-pages/final-manifests").join(&name);
    let before_recovery = root.0.join("seal-final-fixture.json");
    std::fs::rename(&original, &before_recovery).unwrap();
    let NativeForecastCycleFinalizationOutcome::Finalized(closed) =
        finalize_native_forecast_cycle(state.clone(), &seals, &mut manifests, &codec).unwrap()
    else {
        panic!("first sealed cycle must finalize");
    };
    assert!(closed.is_finalized());
    drop(manifests);
    let mut manifests = NativeForecastCycleFinalManifests::new(make_slots()).unwrap();
    assert_eq!(
        std::fs::read(&original).unwrap(),
        std::fs::read(&before_recovery).unwrap()
    );
    assert_eq!(
        std::fs::read(root.0.join("online-pages/final-manifests").join(&name)).unwrap(),
        witnessed_bytes(&oracle, "manifest")
    );
    let saved = manifests
        .find_final_manifest(&due_key, generation)
        .unwrap()
        .unwrap();
    assert_eq!(
        bytes.canonical_json(saved.manifest.document()),
        bytes.canonical_json(field(witness, "value"))
    );
    assert_eq!(
        string(field(saved.manifest.document(), "finalizedAt")),
        SEALED_AT
    );
    let mut stale = state;
    stale.pages.clear();
    assert_eq!(
        finalize_native_forecast_cycle(stale, &seals, &mut manifests, &codec).unwrap(),
        NativeForecastCycleFinalizationOutcome::AlreadyFinalized(saved),
    );

    let late = [ForecastCycleProjection {
        channel: ForecastCycleProjectionChannel::Organic,
        identity_sha256: ForecastCycleSha256::admitted([0x77; 32]),
    }];
    assert!(matches!(
        append_forecast_cycle_page(
            &closed,
            ForecastCyclePageAppendInput {
                payload: b"late-page",
                projections: &late
            },
            &codec,
            &mut slots
        ),
        Err(ForecastCyclePageError::Refused(
            ForecastCyclePageRefusal::CycleAlreadyFinalized
        ))
    ));

    // exercise actual transfer finalization recovery with a separate due pending
    let priority = field(&oracle, "transferPriority");
    let command_output = field(&oracle, "commandStdout");
    assert!(string(field(command_output, "finalAcknowledgement")).is_empty());
    assert!(string(field(command_output, "pageAcknowledgement")).is_empty());
    let next_oracle = field(priority, "pageAfterFinalAcknowledgement");
    let header_oracle = field(next_oracle, "header");
    let next_state = create_forecast_cycle_page_state(ForecastCyclePageStateInput {
        daily_page_count: 2,
        due_key: string(field(header_oracle, "dueKey")),
        generation: string(field(header_oracle, "generation")).parse().unwrap(),
        local_date: "2026-10-08".to_owned(),
    })
    .unwrap();
    let later_projections = [ForecastCycleProjection {
        channel: ForecastCycleProjectionChannel::SchedulerRequest,
        identity_sha256: ForecastCycleSha256::admitted([0x88; 32]),
    }];
    let next_state = append_forecast_cycle_page(
        &next_state,
        ForecastCyclePageAppendInput {
            payload: b"next-due-pending-page",
            projections: &later_projections,
        },
        &codec,
        &mut slots,
    )
    .unwrap();
    let existing = root.0.join("online-pages/final-manifests").join(&name);
    // simulate interrupted final publication while keeping the original bytes for comparison
    let preserved = root.0.join("first-final-fixture.json");
    std::fs::rename(&existing, &preserved).unwrap();

    let checkpoints = make_transfers();
    let make_acks = || {
        NativeForecastCycleFinalAcknowledgements::initialize(
            root.0.clone(),
            BoundForecastCyclePageCodec::new(&bytes),
            StorageWitness,
        )
        .unwrap()
    };
    let mut final_acks = make_acks();
    let lock_codec = BoundForecastArchiveLockCodec::new(&bytes, &calendar);
    let final_stdout = archive_fixture_command(&mut make_operator(SEALED_AT), "archive-next", &[]);
    assert_eq!(
        final_stdout,
        node(
            &["base64-decode"],
            string(field(command_output, "final")).as_bytes()
        )
    );

    let mut clock = Clock(SEALED_AT);
    let mut lock_clock = BoundForecastArchiveLockClock::new(&mut clock, &bytes);
    let mut online = NativeForecastCycleOnlineTransfer::new(
        &slots,
        &checkpoints,
        &seals,
        &mut manifests,
        &mut final_acks,
        &codec,
    )
    .unwrap();
    let ForecastCycleOnlineNext::Final {
        manifest,
        manifest_sha256,
        ..
    } = online.next(&lock_codec, &mut lock_clock).unwrap()
    else {
        panic!("recovered sealed final must precede the later page");
    };
    assert_eq!(
        std::fs::read(existing).unwrap(),
        std::fs::read(preserved).unwrap()
    );

    let page_payloads = [
        b"scheduler-final-page-one".as_slice(),
        b"organic-final-page-two".as_slice(),
    ];
    let capsule =
        encode_forecast_cycle_capsule(&manifest, manifest_sha256, &page_payloads, &codec).unwrap();
    assert_eq!(capsule, witnessed_bytes(&oracle, "capsule"));
    let reopened = validate_forecast_cycle_capsule(&capsule, &codec).unwrap();
    assert_eq!(reopened.page_payloads, page_payloads);
    assert_eq!(reopened.manifest_sha256, manifest_sha256);

    let graph = ForecastCycleSha256::admitted([0x99; 32]);
    let manifest_hex = string(field(witness, "sha256"));
    let graph_hex = "9".repeat(64);
    assert!(
        archive_fixture_command(
            &mut make_operator(FINAL_ACK_AT),
            "archive-ack-final",
            &[&manifest_hex, &graph_hex],
        )
        .is_empty()
    );
    let ack = online
        .acknowledge_final(manifest_sha256, graph, &lock_codec, &mut Clock(SEALED_AT))
        .unwrap();
    assert_eq!(ack.graph_sha256, graph);
    assert_eq!(ack.acknowledged_at, FINAL_ACK_AT);
    let ack_file = root
        .0
        .join("online-pages/final-acknowledgements")
        .join(&name);
    assert_eq!(
        std::fs::read(ack_file).unwrap(),
        witnessed_bytes(&oracle, "coldGraphAcknowledgement")
    );
    let mut final_acks = make_acks();
    let mut online = NativeForecastCycleOnlineTransfer::new(
        &slots,
        &checkpoints,
        &seals,
        &mut manifests,
        &mut final_acks,
        &codec,
    )
    .unwrap();
    assert_eq!(
        online
            .acknowledge_final(manifest_sha256, graph, &lock_codec, &mut Clock(SEALED_AT))
            .unwrap(),
        ack
    );
    assert!(matches!(
        online.acknowledge_final(
            manifest_sha256,
            ForecastCycleSha256::admitted([0; 32]),
            &lock_codec,
            &mut Clock(FINAL_ACK_AT)
        ),
        Err(NativeForecastCycleOnlineTransferError::GraphCollision),
    ));
    let ForecastCycleOnlineNext::Page {
        header,
        page_sha256,
        projections,
        payload,
        ..
    } = online.next(&lock_codec, &mut lock_clock).unwrap()
    else {
        panic!("later page must resume after the final graph is durable");
    };
    assert_eq!(header.due_key, next_state.due_key);
    assert_eq!(page_sha256, next_state.pages[0].page_sha256);
    assert_eq!(
        payload,
        node(
            &["base64-decode"],
            string(field(next_oracle, "payloadBase64")).as_bytes()
        )
    );
    let page_stdout = archive_fixture_command(&mut make_operator(SEALED_AT), "archive-next", &[]);
    assert_eq!(
        page_stdout,
        node(
            &["base64-decode"],
            string(field(command_output, "page")).as_bytes()
        )
    );
    let page_hex = string(field(next_oracle, "pageSha256"));
    let checkpoint_hex = "b".repeat(64);
    assert!(
        archive_fixture_command(
            &mut make_operator(FINAL_ACK_AT),
            "archive-ack",
            &[&page_hex, &checkpoint_hex],
        )
        .is_empty()
    );
    assert!(
        archive_fixture_command(
            &mut make_operator(SEALED_AT),
            "archive-ack",
            &[&page_hex, &checkpoint_hex],
        )
        .is_empty()
    );
    let stored = checkpoints
        .read_transfer_acknowledgement(page_sha256)
        .unwrap()
        .unwrap();
    assert_eq!(stored.header, header);
    assert_eq!(stored.projections, projections);
    assert_eq!(stored.payload_length, payload.len());
    assert_eq!(stored.acknowledged_at, FINAL_ACK_AT);
    let receipt_witness = field(&oracle, "pageDurability");
    // compare both first-writer receipts using the exact Node filenames and bytes
    for (kind, directory) in [
        ("checkpoint", "online-pages/archive-acknowledgements"),
        ("protocolAcknowledgement", "online-pages/acknowledgements"),
    ] {
        let witness = field(receipt_witness, kind);
        let filename = string(field(witness, "filename"));
        assert_eq!(
            std::fs::read(root.0.join(directory).join(filename)).unwrap(),
            witnessed_bytes(receipt_witness, kind)
        );
    }

    let idle_stdout = archive_fixture_command(&mut make_operator(SEALED_AT), "archive-next", &[]);
    assert_eq!(
        idle_stdout,
        node(
            &["base64-decode"],
            string(field(command_output, "idle")).as_bytes()
        )
    );
    assert!(matches!(
        online.next(&lock_codec, &mut lock_clock).unwrap(),
        ForecastCycleOnlineNext::Idle { .. }
    ));
}
