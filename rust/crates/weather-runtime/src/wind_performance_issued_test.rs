use std::cmp::Ordering;

use weather_contracts::{
    ForecastAdjustmentEvidenceClass, ForecastAdjustmentMetric, ForecastLeadBandKey, JsonValue,
    Nullable,
};
use weather_database::Sha256Digest;

use super::{WindPerformanceBand, collect_issued_wind_pairs};
use crate::forecast_training_package_row::ForecastTrainingPackageRowTime;
use crate::maintenance_canonical_json::{MaintenanceCanonicalJson, field, text};
use crate::rain_performance_edge_rows::JoinedRainPerformanceEdgeRow;
use crate::temperature_performance_target::TemperaturePerformanceTargetValues;

const BUNDLE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const SETTINGS: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const ROW_IDENTITY: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const PRODUCT_RUN_AT: &str = "2026-10-08T06:30:00.000Z";
const VALID_AT: &str = "2026-10-08T13:00:00.000Z";

#[derive(Default)]
struct Values {
    encoded: Vec<JsonValue>,
}

impl ForecastTrainingPackageRowTime for Values {
    /// parse the two ordinary issued-row instants
    fn utc_epoch_milliseconds(&self, value: &str) -> Option<i64> {
        match value {
            PRODUCT_RUN_AT => Some(0),
            VALID_AT => Some(23_400_000),
            _ => None,
        }
    }

    /// leave unused utc formatting outside this cohort fixture
    fn canonical_utc(&self, _: i64) -> Option<String> {
        None
    }

    /// return the pinned local date for the issued valid instant
    fn los_angeles_local_date(&self, epoch_ms: i64) -> Option<String> {
        (epoch_ms == 23_400_000).then(|| "2026-10-08".to_owned())
    }
}

impl MaintenanceCanonicalJson for Values {
    /// leave parsing outside this already-decoded fixture
    fn parse_canonical_object(&mut self, _: &[u8]) -> Option<JsonValue> {
        None
    }

    /// capture each issued row identity document
    fn encode_canonical_json(&mut self, value: &JsonValue) -> Option<Vec<u8>> {
        self.encoded.push(value.clone());
        Some(b"issued\n".to_vec())
    }

    /// assign one deterministic digest to ordinary issued identity bytes
    fn sha256(&mut self, bytes: &[u8]) -> Option<Sha256Digest> {
        (bytes == b"issued\n").then(|| Sha256Digest::admitted(ROW_IDENTITY.to_owned()))
    }
}

impl TemperaturePerformanceTargetValues for Values {
    /// preserve ascii fixture ordering for javascript locale comparisons
    fn compare_js_locale(&self, left: &str, right: &str) -> Ordering {
        left.cmp(right)
    }
}

/// retain active issued pairs, raw fallback, empty bands, and nonactive counts
#[test]
fn collects_receipt_backed_pairs_in_the_enabled_mask() {
    let mask = vec![
        WindPerformanceBand {
            metric: ForecastAdjustmentMetric::WindSpeedMps,
            lead_band: ForecastLeadBandKey::Value001024,
        },
        WindPerformanceBand {
            metric: ForecastAdjustmentMetric::WindGustMps,
            lead_band: ForecastLeadBandKey::Value001024,
        },
        WindPerformanceBand {
            metric: ForecastAdjustmentMetric::WindSpeedMps,
            lead_band: ForecastLeadBandKey::Value025048,
        },
    ];
    let edges = vec![
        active_edge(),
        nonactive_edge("disabled", JsonValue::Number(7.0), JsonValue::Number(9.0)),
        nonactive_edge("not_applicable", JsonValue::Number(6.0), JsonValue::Null),
    ];
    let mut values = Values::default();

    let result = collect_issued_wind_pairs(&[], &[], &edges, BUNDLE, &mask, &mut values)
        .expect("ordinary issued cohort");

    assert_eq!(result.disabled_count, 2);
    assert_eq!(result.not_applicable_count, 1);
    assert_eq!(result.pairs.len(), 3);
    assert!(result.pairs["windSpeedMps:025-048"].is_empty());

    let speed = &result.pairs["windSpeedMps:001-024"][0];
    assert_eq!(speed.adjustedPrediction, 8.5);
    assert_eq!(
        speed.evidenceClass,
        ForecastAdjustmentEvidenceClass::AsIssued
    );
    assert!(!speed.fallback);
    assert_eq!(speed.horizonHours, 7.0);
    assert_eq!(
        speed.key,
        format!("windSpeedMps:001-024:{PRODUCT_RUN_AT}:{VALID_AT}")
    );
    assert_eq!(speed.localDate, "2026-10-08");
    assert!(speed.provenanceComplete);
    assert_eq!(speed.rawPrediction, 8.0);
    assert_eq!(speed.rowIdentity, ROW_IDENTITY);
    assert_eq!(
        speed.firstEdgeCommittedAt,
        Nullable::Value("2026-10-08T13:01:00.000Z".to_owned())
    );
    assert_eq!(
        speed.sourceReceiptAt,
        Nullable::Value("2026-10-08T12:59:00.000Z".to_owned())
    );
    assert_eq!(speed.target, Nullable::Null);
    assert_eq!(speed.targetKey, format!("missing:{VALID_AT}:windSpeedMps"));
    assert_eq!(speed.validAt, VALID_AT);
    assert_eq!(speed.vintageKey, PRODUCT_RUN_AT);

    let gust = &result.pairs["windGustMps:001-024"][0];
    assert_eq!(gust.adjustedPrediction, 10.0);
    assert!(gust.fallback);
    assert_eq!(gust.rawPrediction, 10.0);
    assert_eq!(gust.target, Nullable::Null);
    assert_eq!(gust.targetKey, format!("missing:{VALID_AT}:windGustMps"));

    assert_eq!(values.encoded.len(), 2);
    assert_eq!(
        text(&values.encoded[0], "activeBundle").as_deref(),
        Some(BUNDLE)
    );
    assert_eq!(
        text(&values.encoded[0], "recordId").as_deref(),
        Some("record-1")
    );
    let definition = field(&values.encoded[0], "definition").expect("issued definition");
    assert_eq!(text(definition, "metric").as_deref(), Some("windSpeedMps"));
    assert_eq!(text(definition, "leadBand").as_deref(), Some("001-024"));
    assert_eq!(
        field(&values.encoded[0], "recordRevision"),
        Some(&JsonValue::Number(4.0))
    );
    assert_eq!(
        text(&values.encoded[0], "settingsSha256").as_deref(),
        Some(SETTINGS)
    );
}

/// build one ordinary active captured wind edge
fn active_edge() -> JoinedRainPerformanceEdgeRow {
    JoinedRainPerformanceEdgeRow {
        bundle_identities: object(vec![(
            "wind",
            object(vec![("activeBundle", string(BUNDLE))]),
        )]),
        first_edge_committed_at: "2026-10-08T13:01:00.000Z".to_owned(),
        row: object(vec![
            (
                "record",
                object(vec![
                    ("id", string("record-1")),
                    ("productRunAt", string(PRODUCT_RUN_AT)),
                    ("revisionCount", JsonValue::Number(4.0)),
                    ("validAt", string(VALID_AT)),
                ]),
            ),
            (
                "windAdjustment",
                object(vec![
                    (
                        "adjustedMetrics",
                        object(vec![
                            ("windGustMps", JsonValue::Number(10.5)),
                            ("windSpeedMps", JsonValue::Number(8.5)),
                        ]),
                    ),
                    (
                        "appliedMetrics",
                        JsonValue::Array(vec![string("windSpeedMps")]),
                    ),
                    ("leadBand", string("001-024")),
                    ("state", string("active")),
                ]),
            ),
            (
                "raw",
                object(vec![
                    ("windGustMps", JsonValue::Number(10.0)),
                    ("windSpeedMps", JsonValue::Number(8.0)),
                ]),
            ),
            ("provenanceComplete", JsonValue::Boolean(true)),
        ]),
        settings_sha256: SETTINGS.to_owned(),
        source_receipt_at: "2026-10-08T12:59:00.000Z".to_owned(),
    }
}

/// build one ordinary nonactive captured wind edge
fn nonactive_edge(
    state: &str,
    wind_speed: JsonValue,
    wind_gust: JsonValue,
) -> JoinedRainPerformanceEdgeRow {
    JoinedRainPerformanceEdgeRow {
        bundle_identities: object(vec![(
            "wind",
            object(vec![("activeBundle", string(BUNDLE))]),
        )]),
        first_edge_committed_at: "2026-10-08T13:01:00.000Z".to_owned(),
        row: object(vec![
            ("record", object(vec![("validAt", string(VALID_AT))])),
            (
                "windAdjustment",
                object(vec![
                    ("adjustedMetrics", object(Vec::new())),
                    ("appliedMetrics", JsonValue::Array(Vec::new())),
                    ("state", string(state)),
                ]),
            ),
            (
                "raw",
                object(vec![
                    ("windGustMps", wind_gust),
                    ("windSpeedMps", wind_speed),
                ]),
            ),
        ]),
        settings_sha256: SETTINGS.to_owned(),
        source_receipt_at: "2026-10-08T12:59:00.000Z".to_owned(),
    }
}

/// build one json string
fn string(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}

/// build one json object from utf-eight field names
fn object(fields: Vec<(&str, JsonValue)>) -> JsonValue {
    JsonValue::Object(
        fields
            .into_iter()
            .map(|(key, value)| (key.encode_utf16().collect(), value))
            .collect(),
    )
}
