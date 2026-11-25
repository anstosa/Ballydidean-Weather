use std::cmp::Ordering;

use weather_contracts::JsonValue;
use weather_database::Sha256Digest;
use weather_worker::RainPerformanceReportPackageInputs;

use crate::{
    ForecastTrainingPackageRowTime, JoinedRainPerformanceEdgeRow, MaintenanceCanonicalJson,
    TemperaturePerformanceDataProjectionValues, TemperaturePerformanceTargetValues,
    project_temperature_performance_data,
};

const ACTIVE_BUNDLE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const SETTINGS: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const PROVIDER: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const VALID_AT: &str = "2026-10-08T13:00:00.000Z";
const RUN_INITIALIZED_AT: &str = "2026-10-08T06:00:00.000Z";

#[derive(Default)]
struct Values;

impl ForecastTrainingPackageRowTime for Values {
    /// Keep unused calendar conversion closed in this projection fixture.
    fn utc_epoch_milliseconds(&self, _value: &str) -> Option<i64> {
        None
    }

    /// Keep unused UTC formatting closed in this projection fixture.
    fn canonical_utc(&self, _epoch_ms: i64) -> Option<String> {
        None
    }

    /// Keep unused local-date conversion closed in this projection fixture.
    fn los_angeles_local_date(&self, _epoch_ms: i64) -> Option<String> {
        None
    }
}

impl MaintenanceCanonicalJson for Values {
    /// Keep parsing outside this already-decoded projection fixture.
    fn parse_canonical_object(&mut self, _bytes: &[u8]) -> Option<JsonValue> {
        None
    }

    /// Distinguish issued and fallback identity documents without implementing JSON.
    fn encode_canonical_json(&mut self, value: &JsonValue) -> Option<Vec<u8>> {
        // distinguish the two identity document shapes
        Some(if field(value, "recordId").is_some() {
            b"issued\n".to_vec()
        } else {
            b"fallback\n".to_vec()
        })
    }

    /// Return deterministic fixture identities for delegated canonical bytes.
    fn sha256(&mut self, bytes: &[u8]) -> Option<Sha256Digest> {
        // retain one digest per fixture identity shape
        Some(Sha256Digest::admitted(if bytes == b"issued\n" {
            "d".repeat(64)
        } else {
            "e".repeat(64)
        }))
    }
}

impl TemperaturePerformanceTargetValues for Values {
    /// Preserve ordinary JavaScript locale order for the ASCII fixture.
    fn compare_js_locale(&self, left: &str, right: &str) -> Ordering {
        left.cmp(right)
    }
}

impl TemperaturePerformanceDataProjectionValues for Values {
    /// Apply the fixture's required JavaScript string coercions.
    fn js_string(&mut self, value: &JsonValue) -> Option<Vec<u16>> {
        let value = match value {
            JsonValue::String(value) => return Some(value.clone()),
            JsonValue::Number(value) if value.is_finite() && value.fract() == 0.0 => {
                format!("{value:.0}")
            }
            _ => return None,
        };
        Some(value.encode_utf16().collect())
    }
}

/// Preserve exact issuance identity, runtime metadata, and fallback coalescence.
#[test]
fn projects_original_temperature_hour_and_earliest_serving_fallback() {
    let adjustment_rows = vec![temperature_run(), temperature_hour()];
    let edge_rows = vec![
        active_edge("decoy", 99.0, 0.0, "2026-10-08T06:08:00.000Z"),
        active_edge("issued", 9.5, -0.0, "2026-10-08T06:10:00.000Z"),
        fallback_edge("2026-10-08T06:20:00.000Z", "2026-10-08T06:09:00.000Z"),
        fallback_edge("2026-10-08T06:15:00.000Z", "2026-10-08T06:05:00.000Z"),
    ];
    let inputs = fixture_inputs();
    let projected = project_temperature_performance_data(
        &[],
        &adjustment_rows,
        &edge_rows,
        &inputs,
        &mut Values,
    )
    .expect("ordinary verified temperature material should project");

    let rows = array_field(&projected, "rows");
    assert_eq!(rows.len(), 1);
    let row = &rows[0];
    assert_eq!(field(row, "actualTemperatureC"), Some(&JsonValue::Null));
    assert_eq!(number_field(row, "bestMatchRawTemperatureC"), 8.0);
    assert_eq!(number_field(row, "modelLeadHours"), 7.0);
    assert_eq!(number_field(row, "operationalHorizonHours"), 1.0);
    assert_eq!(
        field(row, "rawRelativeHumidityPercent"),
        Some(&JsonValue::Null)
    );
    assert!(number_field(row, "rawWindSpeedMps").is_sign_negative());
    assert_eq!(field(row, "targetIdentity"), Some(&JsonValue::Null));
    let metadata = object_field(row, "scoredPairMetadata");
    assert_eq!(text_field(metadata, "evidenceClass"), "prospective_receipt");
    assert_eq!(text_field(metadata, "rowIdentity"), "d".repeat(64));
    assert_eq!(
        text_field(metadata, "targetKey"),
        "missing:2026-10-08T13:00:00.000Z"
    );
    let runtime = object_field(row, "recordedRuntimeResult");
    assert_eq!(number_field(runtime, "predictionTemperatureC"), 9.5);
    assert_eq!(field(runtime, "reasonCode"), Some(&JsonValue::Null));
    assert_eq!(
        text_field(object_field(runtime, "scoredPairMetadata"), "evidenceClass"),
        "as_issued"
    );

    let fallbacks = array_field(&projected, "actualServingFallbacks");
    assert_eq!(fallbacks.len(), 1);
    assert_eq!(
        text_field(&fallbacks[0], "firstEdgeCommittedAt"),
        "2026-10-08T06:15:00.000Z"
    );
    assert_eq!(
        text_field(&fallbacks[0], "sourceReceiptAt"),
        "2026-10-08T06:05:00.000Z"
    );
    assert_eq!(text_field(&fallbacks[0], "rowIdentity"), "e".repeat(64));
    assert_eq!(
        object_field(&projected, "inputs"),
        &package_inputs_json(&inputs)
    );
}

/// Build the immutable run fixture with a numeric JavaScript map identity.
fn temperature_run() -> JsonValue {
    object(vec![
        (
            "payload",
            object(vec![
                ("id", JsonValue::Number(1.0)),
                ("modelCycle", text("06z")),
                ("providerResponseSha256", text(PROVIDER)),
                (
                    "recentErrorState",
                    object(vec![("bias", JsonValue::Number(0.0))]),
                ),
                ("runInitializedAt", text(RUN_INITIALIZED_AT)),
                ("upstreamModel", text("ecmwf_ifs025")),
            ]),
        ),
        ("record_kind", text("temperature_run")),
    ])
}

/// Build one exported hour with null and signed-zero raw values.
fn temperature_hour() -> JsonValue {
    object(vec![
        ("first_received_at", text("2026-10-08T06:05:00.000Z")),
        ("local_date", text("2026-10-08")),
        (
            "payload",
            object(vec![
                ("modelLeadHours", JsonValue::Number(7.0)),
                ("rawRelativeHumidityPercent", JsonValue::Null),
                ("rawTemperatureC", JsonValue::Number(9.0)),
                ("rawWindSpeedMps", JsonValue::Number(-0.0)),
                ("runId", JsonValue::Number(1.0)),
                ("validAt", text(VALID_AT)),
            ]),
        ),
        ("record_kind", text("temperature_hour")),
        ("record_revision_identity", text("temperature-hour-1")),
    ])
}

/// Build one active edge whose wind value controls exact `Object.is` matching.
fn active_edge(
    record_id: &str,
    corrected_temperature_c: f64,
    raw_wind_speed_mps: f64,
    first_edge_committed_at: &str,
) -> JoinedRainPerformanceEdgeRow {
    JoinedRainPerformanceEdgeRow {
        bundle_identities: bundle_identities(),
        first_edge_committed_at: first_edge_committed_at.to_owned(),
        row: object(vec![
            ("provenanceComplete", JsonValue::Boolean(false)),
            (
                "record",
                object(vec![
                    ("id", text(record_id)),
                    ("revisionCount", JsonValue::Number(1.0)),
                    ("validAt", text(VALID_AT)),
                ]),
            ),
            (
                "temperatureAdjustment",
                object(vec![
                    ("bundleSha256", text(ACTIVE_BUNDLE)),
                    (
                        "correctedTemperatureC",
                        JsonValue::Number(corrected_temperature_c),
                    ),
                    ("rawBestMatchTemperatureC", JsonValue::Number(8.0)),
                    ("reasonCode", JsonValue::Null),
                    (
                        "sourceForecast",
                        object(vec![
                            ("firstReceivedAt", text("2026-10-08T06:05:00.000Z")),
                            ("modelCycle", text("06z")),
                            ("modelLeadHours", JsonValue::Number(7.0)),
                            ("providerResponseSha256", text(PROVIDER)),
                            ("rawRelativeHumidityPercent", JsonValue::Null),
                            ("rawTemperatureC", JsonValue::Number(9.0)),
                            ("rawWindSpeedMps", JsonValue::Number(raw_wind_speed_mps)),
                            ("runInitializedAt", text(RUN_INITIALIZED_AT)),
                            ("upstreamModel", text("ecmwf_ifs025")),
                            ("validAt", text(VALID_AT)),
                        ]),
                    ),
                    ("state", text("active")),
                ]),
            ),
        ]),
        settings_sha256: SETTINGS.to_owned(),
        source_receipt_at: "2026-10-08T06:05:00.000Z".to_owned(),
    }
}

/// Build one duplicate fallback identity with a selectable receipt window.
fn fallback_edge(
    first_edge_committed_at: &str,
    source_receipt_at: &str,
) -> JoinedRainPerformanceEdgeRow {
    JoinedRainPerformanceEdgeRow {
        bundle_identities: bundle_identities(),
        first_edge_committed_at: first_edge_committed_at.to_owned(),
        row: object(vec![
            ("provenanceComplete", JsonValue::Boolean(false)),
            (
                "record",
                object(vec![
                    ("id", text("fallback-edge")),
                    ("revisionCount", JsonValue::Number(1.0)),
                    ("validAt", text(VALID_AT)),
                ]),
            ),
            (
                "temperatureAdjustment",
                object(vec![
                    ("bundleSha256", text(ACTIVE_BUNDLE)),
                    ("correctedTemperatureC", JsonValue::Null),
                    ("rawBestMatchTemperatureC", JsonValue::Number(8.25)),
                    ("reasonCode", text("prediction_invalid")),
                    ("sourceForecast", JsonValue::Null),
                    ("state", text("raw_fallback")),
                ]),
            ),
        ]),
        settings_sha256: SETTINGS.to_owned(),
        source_receipt_at: source_receipt_at.to_owned(),
    }
}

/// Build the exact temperature bundle identity object.
fn bundle_identities() -> JsonValue {
    object(vec![(
        "temperature",
        object(vec![("activeBundle", text(ACTIVE_BUNDLE))]),
    )])
}

/// Build one typed package input fixture.
fn fixture_inputs() -> RainPerformanceReportPackageInputs {
    RainPerformanceReportPackageInputs {
        adjustment_evidence_manifest_sha256: "1".repeat(64),
        adjustment_evidence_watermark_sha256: "2".repeat(64),
        forecast_training_manifest_sha256: "3".repeat(64),
        local_date_from: "2026-10-08".to_owned(),
        local_date_to: "2026-10-08".to_owned(),
        target_cutoff_at: "2026-10-08T14:00:00.000Z".to_owned(),
    }
}

/// Encode package inputs for an exact output-shape assertion.
fn package_inputs_json(inputs: &RainPerformanceReportPackageInputs) -> JsonValue {
    object(vec![
        (
            "adjustmentEvidenceManifestSha256",
            text(&inputs.adjustment_evidence_manifest_sha256),
        ),
        (
            "adjustmentEvidenceWatermarkSha256",
            text(&inputs.adjustment_evidence_watermark_sha256),
        ),
        (
            "forecastTrainingManifestSha256",
            text(&inputs.forecast_training_manifest_sha256),
        ),
        ("localDateFrom", text(&inputs.local_date_from)),
        ("localDateTo", text(&inputs.local_date_to)),
        ("targetCutoffAt", text(&inputs.target_cutoff_at)),
    ])
}

/// Borrow one fixture object member.
fn field<'a>(value: &'a JsonValue, key: &str) -> Option<&'a JsonValue> {
    let JsonValue::Object(fields) = value else {
        return None;
    };
    fields
        .iter()
        .find_map(|(name, value)| name.iter().copied().eq(key.encode_utf16()).then_some(value))
}

/// Borrow one required fixture object.
fn object_field<'a>(value: &'a JsonValue, key: &str) -> &'a JsonValue {
    field(value, key).expect("fixture object member should exist")
}

/// Borrow one required fixture array.
fn array_field<'a>(value: &'a JsonValue, key: &str) -> &'a [JsonValue] {
    let JsonValue::Array(values) = field(value, key).expect("fixture array member should exist")
    else {
        panic!("fixture member should be an array");
    };
    values
}

/// Decode one required fixture number.
fn number_field(value: &JsonValue, key: &str) -> f64 {
    let JsonValue::Number(value) = field(value, key).expect("fixture number should exist") else {
        panic!("fixture member should be a number");
    };
    *value
}

/// Decode one required fixture text member.
fn text_field(value: &JsonValue, key: &str) -> String {
    let JsonValue::String(value) = field(value, key).expect("fixture text should exist") else {
        panic!("fixture member should be text");
    };
    String::from_utf16(value).expect("fixture text should be valid utf-16")
}

/// Build one JSON string fixture.
fn text(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}

/// Build one JSON object fixture.
fn object(fields: Vec<(&str, JsonValue)>) -> JsonValue {
    JsonValue::Object(
        fields
            .into_iter()
            .map(|(name, value)| (name.encode_utf16().collect(), value))
            .collect(),
    )
}
