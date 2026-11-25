use weather_contracts::JsonValue;
use weather_database::Sha256Digest;

use super::*;

const LINE: &[u8] = b"canonical packaged rain receipt";
const HASH: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

struct Values {
    row: JsonValue,
}

impl MaintenanceCanonicalJson for Values {
    /// reject unrelated canonical-object parsing
    fn parse_canonical_object(&mut self, _: &[u8]) -> Option<JsonValue> {
        None
    }

    /// return the sole canonical JSONL fixture bytes
    fn encode_canonical_json(&mut self, value: &JsonValue) -> Option<Vec<u8>> {
        // bind encoding to the exact decoded fixture
        (value == &self.row).then(|| [LINE, b"\n"].concat())
    }

    /// leave digest ownership outside row admission
    fn sha256(&mut self, _: &[u8]) -> Option<Sha256Digest> {
        None
    }
}

impl AdjustmentEvaluationPackageRowValues for Values {
    /// decode only the exact fixture token
    fn decode_json(&mut self, bytes: &[u8]) -> Option<JsonValue> {
        (bytes == LINE).then(|| self.row.clone())
    }

    /// accept the fixed canonical timestamps in this fixture
    fn js_timestamp_milliseconds(&mut self, value: &JsonValue) -> Option<i64> {
        // accept only string timestamps in this ordinary fixture
        let JsonValue::String(value) = value else {
            return None;
        };
        String::from_utf16(value)
            .ok()
            .filter(|value| value.ends_with('Z'))
            .map(|_| 0)
    }

    /// recognize the one canonical fixture date
    fn matches_canonical_local_date(&mut self, value: &JsonValue) -> bool {
        matches!(value, JsonValue::String(value) if String::from_utf16(value).ok().as_deref() == Some("2026-10-01"))
    }

    /// retain the ordinary fixture as private-material-free
    fn payload_contains_forbidden_private_material(&mut self, _: &JsonValue) -> bool {
        false
    }
}

/// admit one body-backed receipt and expose its streaming observation
#[test]
fn admits_canonical_rain_receipt_with_typed_body_claim() {
    let path = format!("members/bodies/sha256-{HASH}.json.gz");
    let row = object(vec![
        ("body_member_path", string(&path)),
        ("content_hash", string(HASH)),
        ("first_received_at", string("2026-10-01T06:03:00.000Z")),
        ("last_received_at", string("2026-10-01T06:04:00.000Z")),
        ("local_date", string("2026-10-01")),
        ("payload", receipt_payload()),
        ("record_kind", string("rain_receipt")),
        (
            "record_revision_identity",
            string("rain-capture-receipt:fixture"),
        ),
        ("reference_at", JsonValue::Null),
        ("revision_count", JsonValue::Number(2.0)),
        ("site_key", string("ballydidean")),
        ("source_identity", string("rain-capture:forecast:fixture")),
        ("valid_at", string("2026-10-01T07:00:00.000Z")),
    ]);
    let mut values = Values { row: row.clone() };

    let admitted = admit_adjustment_evaluation_package_row(LINE, &mut values)
        .expect("canonical packaged receipt should be admitted");
    assert_eq!(admitted.original, row);
    assert_eq!(
        admitted.record_kind,
        AdjustmentEvaluationRecordKind::RainReceipt
    );
    assert_eq!(admitted.revision_count, 2.0);
    assert_eq!(
        admitted.source_identity,
        "rain-capture:forecast:fixture"
            .encode_utf16()
            .collect::<Vec<_>>()
    );
    assert_eq!(
        admitted.last_received_at,
        string("2026-10-01T06:04:00.000Z")
    );
    assert_eq!(
        admitted.body,
        Some(AdjustmentEvaluationPackageBodyClaim {
            member_path: path,
            body_sha256: Some(HASH.to_owned()),
            plaintext_bytes: Some(41.0),
            compressed_bytes: Some(57.0),
        })
    );
}

/// build the exact rain-receipt payload key population
fn receipt_payload() -> JsonValue {
    object(vec![
        ("availableByDecision", JsonValue::Boolean(true)),
        ("bodyBytes", JsonValue::Number(41.0)),
        ("bodySha256", string(HASH)),
        ("claimId", string("fixture")),
        ("completedAt", string("2026-10-01T06:03:00.000Z")),
        ("compressedBytes", JsonValue::Number(57.0)),
        ("errorCode", JsonValue::Null),
        ("httpStatus", JsonValue::Number(200.0)),
        ("metadata", object(Vec::new())),
        ("outcome", string("valid")),
        ("parserVersion", string("rain-prospective-capture/v1")),
        ("recordedAt", string("2026-10-01T06:03:00.000Z")),
        ("rowCount", JsonValue::Number(49.0)),
        ("startedAt", string("2026-10-01T06:02:00.000Z")),
    ])
}

/// build one dependency-free JSON object
fn object(fields: Vec<(&str, JsonValue)>) -> JsonValue {
    JsonValue::Object(
        fields
            .into_iter()
            .map(|(key, value)| (key.encode_utf16().collect(), value))
            .collect(),
    )
}

/// build one dependency-free JSON string
fn string(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}
