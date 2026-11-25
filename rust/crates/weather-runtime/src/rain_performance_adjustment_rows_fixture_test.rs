use weather_contracts::{JsonValue, Nullable, RainAdjustmentCaptureKindEnum};

use super::*;

const BASE: i64 = 1_000;
const CEILING: i64 = 1_001;
const ROLLOVER: i64 = 1_999;
const ROLLED_OVER: i64 = 2_000;

#[derive(Default)]
struct Values;

impl RainPerformanceTimestampValues for Values {
    /// map only the closed timestamp spellings used by this fixture.
    fn parse_js_timestamp_ms(&mut self, value: &[u16]) -> Option<i64> {
        match String::from_utf16(value).ok()?.as_str() {
            "2026-09-14T00:00:00.123456Z" | "2026-09-14T00:00:00.123000Z" => Some(BASE),
            "2026-09-14T00:00:00.999999Z" => Some(ROLLOVER),
            _ => None,
        }
    }

    /// render the exact millisecond outputs expected from the original helper.
    fn format_utc_milliseconds(&mut self, epoch_ms: i64) -> Option<Vec<u16>> {
        let value = match epoch_ms {
            BASE => "2026-09-14T00:00:00.123Z",
            CEILING => "2026-09-14T00:00:00.124Z",
            ROLLED_OVER => "2026-09-14T00:00:01.000Z",
            _ => return None,
        };
        Some(utf16(value))
    }
}

/// preserve the exact original truncation, availability ceiling, and array key reset.
#[test]
fn normalizes_verified_timestamps_with_original_field_rules() {
    let source = object(vec![
        ("valid_at", string("2026-09-14T00:00:00.123456Z")),
        ("firstReceivedAt", string("2026-09-14T00:00:00.123456Z")),
        ("sourceReceiptAt", string("2026-09-14T00:00:00.999999Z")),
        ("createdAtUtc", string("2026-09-14T00:00:00.123000Z")),
        (
            "validAt",
            JsonValue::Array(vec![string("2026-09-14T00:00:00.123456Z")]),
        ),
        ("localDate", string("2026-09-14")),
    ]);

    let normalized = normalize_verified_performance_timestamps(source, &mut Values).unwrap();
    assert_eq!(
        normalized,
        object(vec![
            ("valid_at", string("2026-09-14T00:00:00.123Z")),
            ("firstReceivedAt", string("2026-09-14T00:00:00.124Z")),
            ("sourceReceiptAt", string("2026-09-14T00:00:01.000Z")),
            ("createdAtUtc", string("2026-09-14T00:00:00.123Z")),
            (
                "validAt",
                JsonValue::Array(vec![string("2026-09-14T00:00:00.123456Z")]),
            ),
            ("localDate", string("2026-09-14")),
        ])
    );
}

/// retain typed claims, valid receipts, and ordered runs while ignoring invalid outcomes.
#[test]
fn projects_relevant_rain_rows_after_normalization() {
    let rows = vec![
        object(vec![
            ("record_kind", string("rain_claim")),
            (
                "payload",
                object(vec![
                    ("id", string("forecast-1")),
                    ("kind", string("forecast")),
                    ("runInitializedAt", string("2026-09-14T00:00:00.123456Z")),
                    ("stationId", JsonValue::Null),
                    ("windowEndExclusive", JsonValue::Null),
                    ("windowStart", JsonValue::Null),
                ]),
            ),
        ]),
        object(vec![
            ("record_kind", string("rain_receipt")),
            ("payload", object(vec![("outcome", string("invalid"))])),
        ]),
        object(vec![
            ("record_kind", string("rain_receipt")),
            ("body_member_path", string("members/body.gz")),
            (
                "payload",
                object(vec![
                    ("bodySha256", string("body-hash")),
                    ("claimId", string("forecast-1")),
                    ("completedAt", string("2026-09-14T00:00:00.123456Z")),
                    ("outcome", string("valid")),
                ]),
            ),
        ]),
        object(vec![
            ("record_kind", string("rain_adjustment_run")),
            ("record_revision_identity", string("run-revision")),
            (
                "payload",
                object(vec![
                    ("runInitializedAt", string("2026-09-14T00:00:00.123456Z")),
                    ("modelSha256", string("model")),
                    ("inputSha256", string("input")),
                    ("forecastClaimId", string("forecast-1")),
                    ("firstReceivedAt", string("2026-09-14T00:00:00.123456Z")),
                    ("decisionAt", string("2026-09-14T00:00:00.123456Z")),
                    ("generatedAt", string("2026-09-14T00:00:00.123456Z")),
                    (
                        "hours",
                        JsonValue::Array(vec![object(vec![
                            ("validAt", string("2026-09-14T00:00:00.123456Z")),
                            ("modelLeadHours", JsonValue::Number(9.0)),
                            ("rawPrecipitationMm", JsonValue::Number(1.0)),
                            ("correctedPrecipitationMm", JsonValue::Number(0.5)),
                            ("applied", JsonValue::Boolean(true)),
                            ("reasonCode", JsonValue::Null),
                        ])]),
                    ),
                ]),
            ),
        ]),
    ];

    let projected = project_admitted_rain_performance_adjustment_rows(rows, &mut Values).unwrap();
    assert_eq!(projected.claims.len(), 1);
    assert_eq!(
        projected.claims[0].kind,
        RainAdjustmentCaptureKindEnum::Forecast
    );
    assert_eq!(
        projected.claims[0].run_initialized_at,
        Nullable::Value("2026-09-14T00:00:00.123Z".to_owned())
    );
    assert_eq!(projected.receipts.len(), 1);
    assert_eq!(
        projected.receipts[0].completed_at,
        "2026-09-14T00:00:00.123Z"
    );
    assert_eq!(projected.runs.len(), 1);
    assert_eq!(
        projected.runs[0].stored.firstReceivedAt,
        "2026-09-14T00:00:00.124Z"
    );
    assert_eq!(projected.runs[0].stored.hours[0].modelLeadHours, 9);
    assert_eq!(projected.runs[0].stored.hours[0].reasonCode, Nullable::Null);
}

/// reject a scalar package row at the original object boundary.
#[test]
fn rejects_non_object_adjustment_rows() {
    assert_eq!(
        project_admitted_rain_performance_adjustment_rows(
            vec![JsonValue::Boolean(false)],
            &mut Values,
        ),
        Err(RainPerformanceAdjustmentRowsError::InvalidRow)
    );
}

/// retain generic normalized rows until the later typed archive materialization boundary.
#[test]
fn separates_normalization_from_typed_rain_projection() {
    let source = object(vec![
        ("record_kind", string("rain_claim")),
        ("payload", JsonValue::Boolean(false)),
    ]);
    let normalized =
        normalize_admitted_rain_performance_adjustment_rows(vec![source.clone()], &mut Values)
            .expect("generic verified rows should normalize before typed selection");

    assert_eq!(normalized, vec![source]);
    assert_eq!(
        project_normalized_rain_performance_adjustment_rows(normalized),
        Err(RainPerformanceAdjustmentRowsError::InvalidClaim)
    );
}

/// build one ordered fixture object.
fn object(fields: Vec<(&str, JsonValue)>) -> JsonValue {
    JsonValue::Object(
        fields
            .into_iter()
            .map(|(key, value)| (utf16(key), value))
            .collect(),
    )
}

/// build one fixture string.
fn string(value: &str) -> JsonValue {
    JsonValue::String(utf16(value))
}

/// encode one fixture string without a JSON parser.
fn utf16(value: &str) -> Vec<u16> {
    value.encode_utf16().collect()
}
