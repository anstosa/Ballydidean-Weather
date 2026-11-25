use std::cell::RefCell;
use std::io;
use std::rc::Rc;

use weather_contracts::JsonValue;
use weather_database::Sha256Digest;
use weather_worker::{
    AdmittedRainEdgeEvidence, AdmittedRainHurdleWindCalendar,
    ForecastAdjustmentRainDiagnosticCalendar, ForecastAdjustmentScorecardCalendar,
    RainAdjustmentFeatureAdmission, RainAdjustmentTimeAdapter, RainPerformanceCalendarLabels,
    RainPerformanceEvidenceValues, RainPerformanceReportCalendar,
    RainPerformanceReportPackageInputs, RainPerformanceTarget, TaskFailure,
    TempestObservationEnvelope,
};

use super::*;
use crate::{
    ForecastTrainingPackageManifestValues, ForecastTrainingPackageRowTime,
    MaintenanceCanonicalJson, RainArchiveTargetIdentityValues, RainArchiveTargetPreimage,
    RainPerformanceEdgeRowsValues, RainPerformancePackageCompositionValues,
    RainPerformanceTimestampValues, WorkerRainAdjustmentCaptureValues, WorkerRainForecastPayload,
    WorkerRainGaugeReplayValues,
};

const GENERATED_AT: &str = "2026-09-16T00:00:00.000Z";
const REVISION: &str = "cccccccccccccccccccccccccccccccccccccccc";

#[derive(Default)]
pub(crate) struct FixtureEnvironment {
    events: Rc<RefCell<Vec<String>>>,
    last_report: Option<JsonValue>,
}

impl FixtureEnvironment {
    /// build one event-sharing command environment
    pub(crate) fn with_events(events: Rc<RefCell<Vec<String>>>) -> Self {
        Self {
            events,
            last_report: None,
        }
    }

    /// take the most recently encoded report document
    pub(crate) fn take_report(&mut self) -> Option<JsonValue> {
        self.last_report.take()
    }

    /// Record one ordered command effect.
    fn record(&self, event: impl Into<String>) {
        self.events.borrow_mut().push(event.into());
    }
}

impl MaintenanceCanonicalJson for FixtureEnvironment {
    /// Keep parsing closed because package admission already returned typed values.
    fn parse_canonical_object(&mut self, _: &[u8]) -> Option<JsonValue> {
        None
    }

    /// distinguish the final report serialization from composition preimages
    fn encode_canonical_json(&mut self, value: &JsonValue) -> Option<Vec<u8>> {
        // record the final report only when its fixed contract member is present
        if has_object_key(value, "contractVersion") {
            self.last_report = Some(value.clone());
            self.record("encode:report");
            return Some(b"native-rain-report\n".to_vec());
        }
        self.record("encode:composition");
        Some(b"composition-preimage\n".to_vec())
    }

    /// Return one immutable admitted digest for every fixture preimage.
    fn sha256(&mut self, _: &[u8]) -> Option<Sha256Digest> {
        self.record("sha256:composition");
        Some(Sha256Digest::admitted("a".repeat(64)))
    }
}

impl RainPerformancePackageCompositionValues for FixtureEnvironment {
    /// Preserve each already admitted fixture local date.
    fn rain_package_local_date(&mut self, record_valid_at: &str) -> Option<String> {
        record_valid_at.get(..10).map(str::to_owned)
    }
}

impl RainPerformanceCliEnvironment for FixtureEnvironment {
    type Error = &'static str;

    /// Capture one deterministic clock and expose its position.
    fn capture_generated_at(&mut self) -> String {
        self.record("clock");
        GENERATED_AT.to_owned()
    }

    /// Resolve one full immutable fixture HEAD.
    fn repository_head(&mut self) -> Result<String, Self::Error> {
        self.record("head");
        Ok(REVISION.to_owned())
    }
}

impl RainPerformanceLegacyCliEnvironment for FixtureEnvironment {
    /// Return one independently admitted empty typed package pair.
    fn load_verified_package_pair(
        &mut self,
        forecast_package: &str,
        adjustment_package: &str,
    ) -> Result<AdmittedRainPerformancePackagePair, Self::Error> {
        self.record(format!("load:{forecast_package}:{adjustment_package}"));
        let date = if forecast_package.ends_with("second") {
            "2026-09-15"
        } else {
            "2026-09-14"
        };
        Ok(fixture_pair(date))
    }
}

impl WorkerRainGaugeReplayValues for FixtureEnvironment {
    /// Keep unreachable station decoding closed for empty typed archives.
    fn decode_tempest(&mut self, _: &[u8]) -> Result<TempestObservationEnvelope, TaskFailure> {
        Err(failure("unexpected station decode"))
    }

    /// Keep unreachable replay clock parsing closed for empty typed archives.
    fn parse_utc_milliseconds(&mut self, _: &str) -> Result<i64, TaskFailure> {
        Err(failure("unexpected replay clock"))
    }

    /// Keep unreachable replay clock formatting closed for empty typed archives.
    fn format_utc_milliseconds(&mut self, _: i64) -> Result<String, TaskFailure> {
        Err(failure("unexpected replay clock"))
    }
}

impl WorkerRainAdjustmentCaptureValues for FixtureEnvironment {
    /// Keep unreachable forecast decoding closed for empty typed archives.
    fn decode_forecast(&mut self, _: &[u8]) -> Result<WorkerRainForecastPayload, TaskFailure> {
        Err(failure("unexpected forecast decode"))
    }

    /// Keep unreachable provider-clock formatting closed for empty typed archives.
    fn format_open_meteo_hour(&mut self, _: i64) -> Result<String, TaskFailure> {
        Err(failure("unexpected provider clock"))
    }
}

impl RainPerformanceEvidenceValues for FixtureEnvironment {
    /// Keep unreachable evidence calendar projection closed for an empty report.
    fn local_date(&mut self, _: &str) -> Option<String> {
        None
    }

    /// Keep unreachable targets closed for an empty report.
    fn rain_target(&mut self, _: &str) -> Option<RainPerformanceTarget> {
        None
    }

    /// Keep unreachable receipt clocks closed for an empty report.
    fn receipt_clock(&mut self, _: &str) -> Option<String> {
        None
    }

    /// Keep unreachable edge identities closed for an empty report.
    fn edge_row_identity(&mut self, _: &AdmittedRainEdgeEvidence, _: &str) -> Option<String> {
        None
    }

    /// Keep unreachable Best Match identities closed for an empty report.
    fn best_match_edge_row_identity(&mut self, _: &AdmittedRainEdgeEvidence) -> Option<String> {
        None
    }
}

impl RainArchiveTargetIdentityValues for FixtureEnvironment {
    /// Keep unreachable fixed-gauge identities closed for an empty archive.
    fn rain_target_identity_sha256(&mut self, _: &RainArchiveTargetPreimage) -> Option<String> {
        None
    }
}

impl ForecastAdjustmentScorecardCalendar for FixtureEnvironment {
    /// Keep row clock parsing closed for an empty report.
    fn canonical_utc_epoch_milliseconds(&mut self, _: &str) -> Option<i64> {
        None
    }

    /// Keep row date projection closed for an empty report.
    fn los_angeles_local_date(&mut self, _: &str, _: i64) -> Option<String> {
        None
    }
}

impl ForecastAdjustmentRainDiagnosticCalendar for FixtureEnvironment {
    /// Keep row season projection closed for an empty report.
    fn los_angeles_is_winter(&mut self, _: &str) -> Option<bool> {
        None
    }
}

impl RainPerformanceReportCalendar for FixtureEnvironment {
    /// Keep row slice labels closed for an empty report.
    fn los_angeles_report_labels(&mut self, _: &str) -> Option<RainPerformanceCalendarLabels> {
        None
    }
}

impl RainArchivePerformanceOwners for FixtureEnvironment {
    /// Keep unreachable body inflation closed for an empty archive.
    fn gunzip_bounded(&mut self, _: &[u8], _: usize) -> Result<Vec<u8>, TaskFailure> {
        Err(failure("unexpected archive body"))
    }

    /// Keep unreachable body hashing closed for an empty archive.
    fn body_sha256(&mut self, _: &[u8]) -> Result<String, TaskFailure> {
        Err(failure("unexpected archive body"))
    }
}

impl RainPerformanceTimestampValues for FixtureEnvironment {
    /// Keep package timestamp parsing unreachable after native verifier refusal.
    fn parse_js_timestamp_ms(&mut self, _: &[u16]) -> Option<i64> {
        None
    }

    /// Keep package timestamp formatting unreachable after native verifier refusal.
    fn format_utc_milliseconds(&mut self, _: i64) -> Option<Vec<u16>> {
        None
    }
}

impl RainPerformanceEdgeRowsValues for FixtureEnvironment {
    /// Keep package JSON decoding unreachable after native verifier refusal.
    fn decode_json(&mut self, _: &[u8]) -> Option<JsonValue> {
        None
    }

    /// Keep package edge inflation unreachable after native verifier refusal.
    fn gunzip_bounded(&mut self, _: &[u8], _: usize) -> Option<Vec<u8>> {
        None
    }
}

impl RainPerformancePackageLoaderValues for FixtureEnvironment {
    type Error = &'static str;

    /// Resolve one native package root while exposing argument order.
    fn resolve_package_root(&mut self, raw: &str) -> Result<String, Self::Error> {
        self.record(format!("native-resolve:{raw}"));
        Ok(format!("/resolved/{raw}"))
    }

    /// Snapshot the forecast manifest before the native verifier starts.
    fn read_package_file(&mut self, root: &str, member_path: &str) -> Result<Vec<u8>, Self::Error> {
        self.record(format!("native-read:{root}:{member_path}"));
        Ok(b"forecast-manifest".to_vec())
    }

    /// Keep forecast member inflation unreachable after native verifier refusal.
    fn gunzip_forecast_member(&mut self, _: &[u8]) -> Result<Vec<u8>, Self::Error> {
        Err("unexpected forecast member")
    }
}

impl ForecastTrainingPackageManifestValues for FixtureEnvironment {
    /// Keep native manifest date parsing unreachable after the primitive refusal.
    fn local_date_ordinal(&mut self, _: &str) -> Option<i64> {
        None
    }

    /// Keep native manifest clock parsing unreachable after the primitive refusal.
    fn utc_epoch_milliseconds(&mut self, _: &str) -> Option<i64> {
        None
    }
}

impl ForecastTrainingPackageRowTime for FixtureEnvironment {
    /// Keep native row clock parsing unreachable after the primitive refusal.
    fn utc_epoch_milliseconds(&self, _: &str) -> Option<i64> {
        None
    }

    /// Keep native row clock formatting unreachable after the primitive refusal.
    fn canonical_utc(&self, _: i64) -> Option<String> {
        None
    }

    /// Keep native row calendar projection unreachable after the primitive refusal.
    fn los_angeles_local_date(&self, _: i64) -> Option<String> {
        None
    }
}

impl ForecastTrainingPackageVerificationValues for FixtureEnvironment {
    type Error = &'static str;

    /// Expose entry into the complete native verifier after the manifest snapshot.
    fn absolute_path(&mut self, raw: &str) -> Result<String, Self::Error> {
        self.record(format!("native-verify-absolute:{raw}"));
        Err("native forecast verifier failed")
    }

    /// Keep native directory admission unreachable after the primitive refusal.
    fn canonical_package_path(&mut self, _: &str) -> Result<Option<String>, Self::Error> {
        Err("unexpected canonical directory")
    }

    /// Keep native regular file reads unreachable after the primitive refusal.
    fn read_regular_file(&mut self, _: &str, _: &str) -> Result<Option<Vec<u8>>, Self::Error> {
        Err("unexpected native read")
    }

    /// Keep native tree enumeration unreachable after the primitive refusal.
    fn collect_regular_tree_entries(
        &mut self,
        _: &str,
    ) -> Result<Option<Vec<String>>, Self::Error> {
        Err("unexpected native tree")
    }

    /// Keep native ordinary JSON decoding unreachable after the primitive refusal.
    fn decode_json(&mut self, _: &[u8]) -> Option<JsonValue> {
        None
    }

    /// Keep native member inflation unreachable after the primitive refusal.
    fn gunzip(&mut self, _: &[u8]) -> Result<Vec<u8>, Self::Error> {
        Err("unexpected native gunzip")
    }

    /// Keep native inclusive date enumeration unreachable after the primitive refusal.
    fn inclusive_local_dates(&mut self, _: &str, _: &str) -> Option<Vec<String>> {
        None
    }

    /// Keep native hourly enumeration unreachable after the primitive refusal.
    fn local_date_utc_hours(&mut self, _: &str) -> Option<Vec<i64>> {
        None
    }
}

pub(crate) struct FixtureAdmission;

impl RainAdjustmentFeatureAdmission for FixtureAdmission {
    /// Keep unreachable feature calendars closed for an empty archive.
    fn calendar(&mut self, _: i64) -> Result<AdmittedRainHurdleWindCalendar, TaskFailure> {
        Err(failure("unexpected feature calendar"))
    }

    /// Keep unreachable station geometry closed for an empty archive.
    fn station_weights(&mut self) -> Result<[f64; 12], TaskFailure> {
        Err(failure("unexpected station geometry"))
    }
}

pub(crate) struct FixtureReplayTime;

impl RainAdjustmentTimeAdapter for FixtureReplayTime {
    /// Keep unreachable replay formatting closed for an empty archive.
    fn utc_string(&mut self, _: i64) -> Result<String, TaskFailure> {
        Err(failure("unexpected replay format"))
    }
}

struct FixtureWriter {
    events: Rc<RefCell<Vec<String>>>,
    bytes: Vec<u8>,
}

impl FixtureWriter {
    /// Build one event-sharing exclusive writer fixture.
    fn with_events(events: Rc<RefCell<Vec<String>>>) -> Self {
        Self {
            events,
            bytes: Vec::new(),
        }
    }
}

impl RainPerformanceResultWriter for FixtureWriter {
    type Output = String;

    /// Admit the exact fixture path before serialization.
    fn admit_result_path(&mut self, raw: &str) -> io::Result<Self::Output> {
        self.events.borrow_mut().push(format!("admit:{raw}"));
        Ok(raw.to_owned())
    }

    /// Retain the exact canonical report bytes after admission.
    fn write_new_result(&mut self, target: Self::Output, bytes: &[u8]) -> io::Result<()> {
        self.events.borrow_mut().push(format!("write:{target}"));
        self.bytes.extend_from_slice(bytes);
        Ok(())
    }
}

/// Preserve parsing and dispatch failures before the command clock effect.
#[test]
fn rain_performance_cli_argument_errors_precede_clock_capture() {
    let cases = [
        (Vec::new(), RainPerformanceCliError::CommandRequired),
        (
            arguments(&["rain-evaluate", "--forecast-package"]),
            RainPerformanceCliError::OptionsInvalid,
        ),
        (
            arguments(&["other-command", "--unused", "value"]),
            RainPerformanceCliError::UnsupportedCommand("other-command".to_owned()),
        ),
    ];
    // prove each parser or dispatch class is effect free
    for (argv, expected) in cases {
        let (result, events, _) = run_fixture(&argv);
        assert_eq!(result, Err(expected));
        assert!(events.is_empty());
    }
}

/// Capture the clock before matched-pair validation and let explicit adjustment flags shadow alias.
#[test]
fn rain_performance_cli_matched_pair_error_preserves_alias_fallback_order() {
    let argv = arguments(&[
        "rain-evaluate",
        "--forecast-package",
        "forecast:first",
        "--adjustment-package",
        "adjustment:first",
        "--adjustment-package",
        "adjustment:second",
        "--evaluation-packages",
        "legacy:first",
    ]);
    let (result, events, _) = run_fixture(&argv);

    assert_eq!(
        result,
        Err(RainPerformanceCliError::MatchedPackagesRequired)
    );
    assert_eq!(events, ["clock"]);
}

/// Load the legacy alias before rejecting an explicit malformed source revision.
#[test]
fn rain_performance_cli_legacy_alias_loads_before_source_revision_validation() {
    let argv = arguments(&[
        "rain-evaluate",
        "--forecast-package",
        "forecast:first",
        "--evaluation-packages",
        "legacy:first",
        "--source-revision",
        "BAD",
        "--output",
        "/private/report.json",
    ]);
    let (result, events, _) = run_fixture(&argv);

    assert_eq!(result, Err(RainPerformanceCliError::SourceRevisionInvalid));
    assert_eq!(events.first().map(String::as_str), Some("clock"));
    assert_eq!(
        events.get(1).map(String::as_str),
        Some("load:forecast:first:legacy:first")
    );
    assert!(!events.iter().any(|event| event == "head"));
    assert!(!events.iter().any(|event| event.starts_with("admit:")));
}

/// Build the actual native report before applying the original output singleton check.
#[test]
fn rain_performance_cli_output_singleton_follows_report_construction() {
    let argv = arguments(&[
        "rain-evaluate",
        "--forecast-package",
        "forecast:first",
        "--evaluation-packages",
        "legacy:first",
        "--source-revision",
        REVISION,
    ]);
    let (result, events, _) = run_fixture(&argv);

    assert_eq!(
        result,
        Err(RainPerformanceCliError::OptionRequiredExactlyOnce(
            "--output"
        ))
    );
    assert!(
        events
            .iter()
            .any(|event| event == "load:forecast:first:legacy:first")
    );
    assert!(!events.iter().any(|event| event == "head"));
    assert!(!events.iter().any(|event| event.starts_with("admit:")));
}

/// Preserve repeated pair order and admit the private target before final serialization.
#[test]
fn rain_performance_cli_runs_full_typed_repeated_pair_command() {
    let argv = arguments(&[
        "rain-evaluate",
        "--forecast-package",
        "forecast:first",
        "--forecast-package",
        "forecast:second",
        "--adjustment-package",
        "adjustment:first",
        "--adjustment-package",
        "adjustment:second",
        "--output",
        "/private/report.json",
    ]);
    let (result, events, bytes) = run_fixture(&argv);

    assert_eq!(result, Ok(0));
    assert_eq!(
        &events[..3],
        [
            "clock",
            "load:forecast:first:adjustment:first",
            "load:forecast:second:adjustment:second",
        ]
    );
    let head = event_index(&events, "head");
    let admit = event_index(&events, "admit:/private/report.json");
    let encode = event_index(&events, "encode:report");
    let write = event_index(&events, "write:/private/report.json");
    assert!(head < admit && admit < encode && encode < write);
    assert_eq!(bytes, b"native-rain-report\n");
}

/// Run one fixture command while retaining all file-only effects.
fn run_fixture(argv: &[String]) -> (Result<u8, RainPerformanceCliError>, Vec<String>, Vec<u8>) {
    let events = Rc::new(RefCell::new(Vec::new()));
    let mut environment = FixtureEnvironment::with_events(Rc::clone(&events));
    let mut writer = FixtureWriter::with_events(Rc::clone(&events));
    let mut admission = FixtureAdmission;
    let mut replay_time = FixtureReplayTime;
    let result = run_forecast_adjustment_performance_cli(
        argv,
        &mut environment,
        &mut admission,
        &mut replay_time,
        &mut writer,
    );
    let recorded = events.borrow().clone();
    (result, recorded, writer.bytes)
}

/// Build one typed verified pair with no replay rows.
fn fixture_pair(date: &str) -> AdmittedRainPerformancePackagePair {
    AdmittedRainPerformancePackagePair {
        inputs: RainPerformanceReportPackageInputs {
            adjustment_evidence_manifest_sha256: "b".repeat(64),
            adjustment_evidence_watermark_sha256: "c".repeat(64),
            forecast_training_manifest_sha256: "d".repeat(64),
            local_date_from: date.to_owned(),
            local_date_to: date.to_owned(),
            target_cutoff_at: format!("{date}T23:59:59.999Z"),
        },
        bodies: Vec::new(),
        claims: Vec::new(),
        edge_rows: Vec::new(),
        normalized_adjustment_rows: Vec::new(),
        receipts: Vec::new(),
        runs: Vec::new(),
    }
}

/// Convert concise fixture literals into owned process arguments.
fn arguments(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

/// Locate one required ordered effect.
fn event_index(events: &[String], expected: &str) -> usize {
    events
        .iter()
        .position(|event| event == expected)
        .expect("expected event should be present")
}

/// Recognize one UTF-16 object member without depending on private report helpers.
fn has_object_key(value: &JsonValue, expected: &str) -> bool {
    let JsonValue::Object(fields) = value else {
        return false;
    };
    fields
        .iter()
        .any(|(key, _)| key.iter().copied().eq(expected.encode_utf16()))
}

/// Build one unreachable fixture task failure.
fn failure(code: &str) -> TaskFailure {
    TaskFailure {
        attempts: 0,
        code: code.to_owned(),
    }
}
