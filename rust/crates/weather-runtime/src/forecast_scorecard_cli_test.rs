use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};

use weather_contracts::{ForecastAdjustmentPerformancePackageInputs, JsonValue};
use weather_database::Sha256Digest;

use super::*;
use crate::{NativeRainPerformanceResultWriter, RainPerformanceTimestampValues, ScorecardClock};

const TARGET_CUTOFF: &str = "2026-10-02T07:00:00.000Z";
const EVIDENCE_CUTOFF: &str = "2026-10-07T00:00:00.000Z";
const GENERATED_AT: &str = "2026-10-08T00:00:00.000Z";
const VALID_THROUGH: &str = "2026-10-15T00:00:00.000Z";
const TARGET_CUTOFF_MS: i64 = 1_000;
const EVIDENCE_CUTOFF_MS: i64 = 2_000;
const GENERATED_AT_MS: i64 = 3_000;
const VALID_THROUGH_MS: i64 = GENERATED_AT_MS + SEVEN_DAYS_MS;
const SOURCE_REVISION: &str = "cccccccccccccccccccccccccccccccccccccccc";
const FORECAST_HASH: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const ADJUSTMENT_HASH: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const WATERMARK_HASH: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const TEMPERATURE_HASH: &str = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
const WIND_HASH: &str = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
const RAIN_HASH: &str = "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";
const SCORECARD_HASH: &str = "9999999999999999999999999999999999999999999999999999999999999999";
const FORECAST_BYTES: &[u8] = b"fixture forecast manifest\n";
const ADJUSTMENT_BYTES: &[u8] = b"fixture adjustment manifest\n";
const TEMPERATURE_BYTES: &[u8] = b"fixture temperature report\n";
const WIND_BYTES: &[u8] = b"fixture wind report\n";
const RAIN_BYTES: &[u8] = b"fixture rain report\n";
const SCORECARD_BYTES: &[u8] = b"fixture canonical scorecard\n";
const PACKAGE_INPUTS_BYTES: &[u8] = b"fixture canonical package inputs\n";

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct FixtureDirectory(PathBuf);

impl FixtureDirectory {
    /// create one process-owned actual-files fixture.
    fn new() -> Self {
        let sequence = NEXT_DIRECTORY.fetch_add(1, AtomicOrdering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "weather-scorecard-cli-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir(&root).expect("fresh scorecard fixture root");
        let root = fs::canonicalize(root).expect("canonical scorecard fixture root");
        Self(root)
    }

    /// return the canonical fixture root.
    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for FixtureDirectory {
    /// remove only this process-owned fixture.
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct FixtureValues {
    documents: BTreeMap<Vec<u8>, JsonValue>,
    digests: BTreeMap<Vec<u8>, String>,
    package_inputs: JsonValue,
    encoded_scorecard: Option<JsonValue>,
}

impl FixtureValues {
    /// bind exact byte maps for the actual fixture files.
    fn new(package_inputs: JsonValue) -> Self {
        let mut documents = BTreeMap::new();
        documents.insert(FORECAST_BYTES.to_vec(), forecast_manifest());
        documents.insert(ADJUSTMENT_BYTES.to_vec(), adjustment_manifest());
        documents.insert(
            TEMPERATURE_BYTES.to_vec(),
            family_report("temperature", package_inputs.clone()),
        );
        documents.insert(
            WIND_BYTES.to_vec(),
            family_report("wind", package_inputs.clone()),
        );
        documents.insert(
            RAIN_BYTES.to_vec(),
            family_report("rain", package_inputs.clone()),
        );
        let digests = BTreeMap::from([
            (FORECAST_BYTES.to_vec(), FORECAST_HASH.to_owned()),
            (ADJUSTMENT_BYTES.to_vec(), ADJUSTMENT_HASH.to_owned()),
            (TEMPERATURE_BYTES.to_vec(), TEMPERATURE_HASH.to_owned()),
            (WIND_BYTES.to_vec(), WIND_HASH.to_owned()),
            (RAIN_BYTES.to_vec(), RAIN_HASH.to_owned()),
            (SCORECARD_BYTES.to_vec(), SCORECARD_HASH.to_owned()),
        ]);
        Self {
            documents,
            digests,
            package_inputs,
            encoded_scorecard: None,
        }
    }
}

impl ScorecardClock for FixtureValues {
    /// parse only fixture publication instants.
    fn utc_epoch_milliseconds(&self, value: &str) -> Option<i64> {
        match value {
            TARGET_CUTOFF => Some(TARGET_CUTOFF_MS),
            EVIDENCE_CUTOFF => Some(EVIDENCE_CUTOFF_MS),
            GENERATED_AT => Some(GENERATED_AT_MS),
            VALID_THROUGH => Some(VALID_THROUGH_MS),
            _ => None,
        }
    }

    /// format only fixture publication instants.
    fn canonical_utc(&self, milliseconds: i64) -> Option<String> {
        match milliseconds {
            TARGET_CUTOFF_MS => Some(TARGET_CUTOFF.to_owned()),
            EVIDENCE_CUTOFF_MS => Some(EVIDENCE_CUTOFF.to_owned()),
            GENERATED_AT_MS => Some(GENERATED_AT.to_owned()),
            VALID_THROUGH_MS => Some(VALID_THROUGH.to_owned()),
            _ => None,
        }
    }
}

impl ScorecardContractValues for FixtureValues {
    /// decode only exact fixture byte populations.
    fn decode_scorecard_json(&mut self, bytes: &[u8]) -> Result<JsonValue, ScorecardContractError> {
        // return the scorecard document captured during exact encoding
        if bytes == SCORECARD_BYTES {
            return self
                .encoded_scorecard
                .clone()
                .ok_or_else(|| ScorecardContractError::new("fixture scorecard is unavailable"));
        }
        self.documents
            .get(bytes)
            .cloned()
            .ok_or_else(|| ScorecardContractError::new("fixture json is unavailable"))
    }

    /// keep fixture documents free of private member names.
    fn scorecard_private_member_key(&self, _key: &[u16]) -> bool {
        false
    }
}

impl MaintenanceCanonicalJson for FixtureValues {
    /// decode one exact mapped fixture object.
    fn parse_canonical_object(&mut self, bytes: &[u8]) -> Option<JsonValue> {
        self.documents.get(bytes).cloned()
    }

    /// encode only the shared inputs and complete scorecard.
    fn encode_canonical_json(&mut self, value: &JsonValue) -> Option<Vec<u8>> {
        // preserve one canonical byte spelling for all common report inputs
        if value == &self.package_inputs {
            return Some(PACKAGE_INPUTS_BYTES.to_vec());
        }
        // capture the assembled scorecard behind its exact written bytes
        if is_wire(member(value, "contractVersion"), FORECAST_SCORECARD_V1) {
            self.encoded_scorecard = Some(value.clone());
            return Some(SCORECARD_BYTES.to_vec());
        }
        None
    }

    /// hash only exact source and output byte populations.
    fn sha256(&mut self, bytes: &[u8]) -> Option<Sha256Digest> {
        self.digests.get(bytes).cloned().map(Sha256Digest::admitted)
    }
}

impl RainPerformanceTimestampValues for FixtureValues {
    /// parse the normalized manifest cutoff only.
    fn parse_js_timestamp_ms(&mut self, value: &[u16]) -> Option<i64> {
        String::from_utf16(value)
            .ok()
            .and_then(|value| (value == TARGET_CUTOFF).then_some(TARGET_CUTOFF_MS))
    }

    /// format the normalized manifest cutoff only.
    fn format_utc_milliseconds(&mut self, epoch_ms: i64) -> Option<Vec<u16>> {
        (epoch_ms == TARGET_CUTOFF_MS).then(|| TARGET_CUTOFF.encode_utf16().collect())
    }
}

impl ScorecardCommandValues for FixtureValues {
    type Error = io::Error;

    /// capture one deterministic creation and verification clock.
    fn capture_scorecard_clock(&mut self) -> Result<i64, Self::Error> {
        Ok(GENERATED_AT_MS)
    }

    /// read each actual fixture path unchanged.
    fn read_scorecard_file(&mut self, raw: &str) -> Result<Vec<u8>, Self::Error> {
        fs::read(raw)
    }

    /// order fixed-width fixture dates like javascript labels.
    fn compare_scorecard_labels(&self, left: &str, right: &str) -> Ordering {
        left.encode_utf16().cmp(right.encode_utf16())
    }
}

/// create and verify one full v1 scorecard through actual files.
#[test]
fn v1_scorecard_cli_writes_and_verifies_exact_native_output() {
    let directory = FixtureDirectory::new();
    let root = directory.path();
    let forecast_path = write_fixture(root, "inputs/forecast.json", FORECAST_BYTES);
    let adjustment_path = write_fixture(root, "inputs/adjustment.json", ADJUSTMENT_BYTES);
    let temperature_path = write_fixture(root, "reports/temperature.json", TEMPERATURE_BYTES);
    let wind_path = write_fixture(root, "reports/wind.json", WIND_BYTES);
    let rain_path = write_fixture(root, "reports/rain.json", RAIN_BYTES);
    let output_path = root.join(".weather-data/scorecards/current.json");
    let package_inputs = fixture_package_inputs();
    let package_inputs_json = package_inputs_json(&package_inputs);
    let mut values = FixtureValues::new(package_inputs_json);
    let owner = fs::metadata(root).expect("fixture root metadata").uid();
    let mut writer = NativeRainPerformanceResultWriter::new(
        root.to_path_buf(),
        root.to_path_buf(),
        root.to_path_buf(),
        owner,
    )
    .expect("native scorecard writer");
    let create_argv = arguments(&[
        "scorecard",
        "--temperature-report",
        path_text(&temperature_path),
        "--wind-report",
        path_text(&wind_path),
        "--rain-report",
        path_text(&rain_path),
        "--forecast-manifest",
        path_text(&forecast_path),
        "--adjustment-manifest",
        path_text(&adjustment_path),
        "--output",
        path_text(&output_path),
    ]);
    let mut create_stdout = Vec::new();

    let create_status = run_forecast_performance_scorecard_cli(
        &create_argv,
        &mut values,
        &mut writer,
        &mut create_stdout,
    )
    .expect("create v1 scorecard");

    assert_eq!(create_status, 0);
    assert!(create_stdout.is_empty());
    let written = fs::read(&output_path).expect("actual scorecard output");
    assert_eq!(written, SCORECARD_BYTES);
    let scorecard = values
        .encoded_scorecard
        .clone()
        .expect("captured scorecard document");
    assert_eq!(
        text(&scorecard, "generatedAt").as_deref(),
        Some(GENERATED_AT)
    );
    assert_eq!(
        text(&scorecard, "validThrough").as_deref(),
        Some(VALID_THROUGH)
    );
    let inputs = field(&scorecard, "inputs").expect("published scorecard inputs");
    assert_eq!(text(inputs, "localDateFrom").as_deref(), Some("2026-10-01"));
    assert_eq!(text(inputs, "localDateTo").as_deref(), Some("2026-10-01"));
    assert_eq!(
        text(inputs, "targetCutoffAt").as_deref(),
        Some(TARGET_CUTOFF)
    );
    assert_eq!(
        text(inputs, "sourceRevision").as_deref(),
        Some(SOURCE_REVISION)
    );
    let report_hashes = field(inputs, "reportSha256s").expect("published raw report hashes");
    // bind each public family card to its exact original report bytes
    for (family, expected) in [
        ("temperature", TEMPERATURE_HASH),
        ("wind", WIND_HASH),
        ("rain", RAIN_HASH),
    ] {
        assert_eq!(
            text(report_hashes, family).as_deref(),
            Some(expected),
            "{family}"
        );
    }

    let verify_argv = arguments(&["verify-scorecard", "--input", path_text(&output_path)]);
    let mut verify_stdout = Vec::new();
    let verify_status = run_forecast_performance_scorecard_cli(
        &verify_argv,
        &mut values,
        &mut writer,
        &mut verify_stdout,
    )
    .expect("verify actual v1 scorecard bytes");

    assert_eq!(verify_status, 0);
    assert_eq!(verify_stdout, format!("{SCORECARD_HASH}\n").as_bytes());
}

/// build one reusable complete ordinary family card.
pub(crate) fn ordinary_family_card(family: &str) -> JsonValue {
    // select the original family metric unit
    let unit = if family == "temperature" {
        "celsius"
    } else if family == "wind" {
        "meters_per_second"
    } else {
        "millimeters_per_hour"
    };
    let first = family.as_bytes()[0];
    let identity = format!("{first:02x}").repeat(32);
    // keep wind's separately matched diagnostic structurally absent
    let best_match = if family == "wind" {
        JsonValue::Null
    } else {
        object(vec![
            ("bestMatchRawMae", JsonValue::Number(1.4)),
            ("dateCount", JsonValue::Number(30.0)),
            ("rowCount", JsonValue::Number(500.0)),
            ("sourceAdjustedMae", JsonValue::Number(1.0)),
            ("sourceRawMae", JsonValue::Number(1.5)),
            ("unit", wire(unit)),
        ])
    };
    // retain rain diagnostics only on the rain card
    let rain = if family == "rain" {
        rain_diagnostics()
    } else {
        JsonValue::Null
    };
    object(vec![
        ("bestMatchDiagnostic", best_match),
        ("comparisonState", wire("better")),
        ("evidenceClass", wire("development")),
        ("evidenceCutoffAt", wire(EVIDENCE_CUTOFF)),
        ("family", wire(family)),
        ("metrics", metric(unit)),
        ("qualificationState", wire("development_only")),
        ("rainDiagnostics", rain),
        ("recommendation", wire("retain")),
        ("servingIdentitySha256", wire(&identity)),
        ("servingState", wire("authorized_active")),
        (
            "slices",
            JsonValue::Array(vec![object(vec![
                ("dimension", wire("horizon")),
                ("label", wire("001-024")),
                ("metrics", metric(unit)),
                ("rowCount", JsonValue::Number(250.0)),
            ])]),
        ),
        ("support", support()),
        ("supportState", wire("sufficient")),
    ])
}

/// build one complete aggregate metric.
fn metric(unit: &str) -> JsonValue {
    object(vec![
        ("adjustedBias", JsonValue::Number(-0.1)),
        ("adjustedMae", JsonValue::Number(1.0)),
        ("adjustedP95", JsonValue::Number(2.5)),
        ("adjustedRmse", JsonValue::Number(1.25)),
        ("deltaMae", JsonValue::Number(-0.5)),
        ("rawBias", JsonValue::Number(0.25)),
        ("rawMae", JsonValue::Number(1.5)),
        ("rawP95", JsonValue::Number(3.0)),
        ("rawRmse", JsonValue::Number(1.75)),
        (
            "skillInterval95",
            object(vec![
                ("lower", JsonValue::Number(5.0)),
                ("upper", JsonValue::Number(20.0)),
            ]),
        ),
        ("skillPercent", JsonValue::Number(12.5)),
        ("unit", wire(unit)),
    ])
}

/// build one complete aggregate support summary.
fn support() -> JsonValue {
    object(vec![
        ("dateCount", JsonValue::Number(60.0)),
        ("effectiveWeightSum", JsonValue::Number(60.0)),
        ("eventCount", JsonValue::Number(1_000.0)),
        ("excludedCount", JsonValue::Number(2.0)),
        (
            "exclusionReasons",
            object(vec![("missing_target", JsonValue::Number(2.0))]),
        ),
        ("fallbackCount", JsonValue::Number(3.0)),
        (
            "fallbackReasons",
            object(vec![("source_stale", JsonValue::Number(3.0))]),
        ),
        ("gapCount", JsonValue::Number(4.0)),
        ("rowCount", JsonValue::Number(1_000.0)),
        ("targetRowCount", JsonValue::Number(1_000.0)),
        ("validHourCount", JsonValue::Number(240.0)),
        ("vintageCount", JsonValue::Number(120.0)),
        ("wetDateCount", JsonValue::Number(20.0)),
        ("wetRowCount", JsonValue::Number(100.0)),
    ])
}

/// build complete rain-only aggregate diagnostics.
fn rain_diagnostics() -> JsonValue {
    let reliability = reliability();
    let thresholds = [0.1, 1.0, 2.5]
        .into_iter()
        .map(|threshold| {
            object(vec![
                ("adjustedBrier", JsonValue::Number(0.1)),
                ("csi", JsonValue::Number(0.5)),
                ("falseAlarms", JsonValue::Number(2.0)),
                ("far", JsonValue::Number(0.2)),
                ("hits", JsonValue::Number(8.0)),
                ("misses", JsonValue::Number(1.0)),
                ("pod", JsonValue::Number(8.0 / 9.0)),
                ("rawBrier", JsonValue::Number(0.2)),
                ("reliability", JsonValue::Array(reliability.clone())),
                ("thresholdMmPerHour", JsonValue::Number(threshold)),
            ])
        })
        .collect();
    let accumulations = [6.0, 12.0, 23.0]
        .into_iter()
        .map(|hours| {
            object(vec![
                ("adjustedMae", JsonValue::Number(0.5)),
                ("completeWindows", JsonValue::Number(10.0)),
                ("hours", JsonValue::Number(hours)),
                ("rawMae", JsonValue::Number(0.75)),
            ])
        })
        .collect();
    object(vec![
        ("accumulations", JsonValue::Array(accumulations)),
        ("annualBalancedVolumeRatio", JsonValue::Number(1.1)),
        ("heavyAdjustedMae", JsonValue::Number(1.1)),
        ("heavyRawMae", JsonValue::Number(1.4)),
        ("probabilityOrderViolationCount", JsonValue::Number(0.0)),
        ("thresholds", JsonValue::Array(thresholds)),
        ("wetAdjustedMae", JsonValue::Number(0.6)),
        ("wetRawMae", JsonValue::Number(0.8)),
        ("winterBalancedVolumeRatio", JsonValue::Number(1.2)),
    ])
}

/// build ten explicit reliability bins.
fn reliability() -> Vec<JsonValue> {
    (0..10)
        .map(|index| {
            // keep the empty first bin explicitly unavailable
            if index == 0 {
                return object(vec![
                    ("count", JsonValue::Number(0.0)),
                    ("meanProbability", JsonValue::Null),
                    ("observedFrequency", JsonValue::Null),
                ]);
            }
            object(vec![
                ("count", JsonValue::Number(10.0)),
                (
                    "meanProbability",
                    JsonValue::Number((f64::from(index) + 0.5) / 10.0),
                ),
                (
                    "observedFrequency",
                    JsonValue::Number(f64::from(index) / 10.0),
                ),
            ])
        })
        .collect()
}

/// build one family report wrapper.
fn family_report(family: &str, inputs: JsonValue) -> JsonValue {
    object(vec![
        (
            "contractVersion",
            wire("forecast-adjustment-performance-report/v1"),
        ),
        (
            "developmentReference",
            JsonValue::Array(vec![wire("separate analysis")]),
        ),
        ("family", ordinary_family_card(family)),
        ("inputs", inputs),
        ("siteKey", wire("ballydidean")),
        ("sourceRevision", wire(SOURCE_REVISION)),
    ])
}

/// build the one forecast manifest cohort.
fn forecast_manifest() -> JsonValue {
    object(vec![
        ("createdAtUtc", wire(TARGET_CUTOFF)),
        ("fromLocalDate", wire("2026-10-01")),
        ("toLocalDate", wire("2026-10-01")),
    ])
}

/// build the matching adjustment manifest cohort.
fn adjustment_manifest() -> JsonValue {
    object(vec![
        (
            "edgeEvidence",
            object(vec![("watermarkSha256", wire(WATERMARK_HASH))]),
        ),
        ("fromLocalDate", wire("2026-10-01")),
        ("toLocalDate", wire("2026-10-01")),
    ])
}

/// build the exact package inputs shared by all reports.
fn fixture_package_inputs() -> ForecastAdjustmentPerformancePackageInputs {
    ForecastAdjustmentPerformancePackageInputs {
        adjustmentEvidenceManifestSha256: ADJUSTMENT_HASH.to_owned(),
        adjustmentEvidenceWatermarkSha256: WATERMARK_HASH.to_owned(),
        forecastTrainingManifestSha256: FORECAST_HASH.to_owned(),
        localDateFrom: "2026-10-01".to_owned(),
        localDateTo: "2026-10-01".to_owned(),
        targetCutoffAt: TARGET_CUTOFF.to_owned(),
    }
}

/// write one actual fixture file beneath its owned root.
fn write_fixture(root: &Path, relative: &str, bytes: &[u8]) -> PathBuf {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().expect("fixture parent")).expect("fixture directory");
    fs::write(&path, bytes).expect("fixture file");
    path
}

/// convert exact argument literals into owned values.
fn arguments(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

/// expose one UTF-8 fixture path for command arguments.
fn path_text(path: &Path) -> &str {
    path.to_str().expect("UTF-8 fixture path")
}
