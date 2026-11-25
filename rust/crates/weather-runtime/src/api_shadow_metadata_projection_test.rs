//! Compact Node-oracle coverage for shadow prediction metadata projection.

use weather_contracts::JsonValue;
use weather_database::Sha256Digest;

use crate::api_maintenance_admission::ApiMaintenanceShadowEvidenceValues;
use crate::api_shadow_metadata_projection::shadow_prediction_metadata;
use crate::{ApiMaintenanceReceiptCanonicalValues, MaintenanceReceiptScalar};

const REGISTRATION: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const CANDIDATE: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const SOURCE: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const DUE_KEY: &str = "temperature:2026-11-24T00:00:00.000Z";
const ISSUED_AT: &str = "2026-11-24T00:00:00.000Z";
const FIRST_VALID_AT: &str = "2026-11-24T01:00:00.000Z";
const LAST_VALID_AT: &str = "2026-11-24T02:00:00.000Z";
const INPUT_SHA256: &str = "d419d5f151d681b716fe149cc1e48e7f747b37b674cb9c7c18fecb1f9437af96";
const FIRST_ROW_SHA256: &str = "fc8bfb694997335fa16c0ba563773fbeef34dd656743833c3a21f5c352e2851c";
const LAST_ROW_SHA256: &str = "99c8732faa0903945a3d85db4b19fb63b40f2586ec955b76573a4cb39d746cf2";
const SOURCE_RECEIPT_SHA256: &str =
    "73f5398e30ad8b46c96d0feeb84c8e791ef82e204e82e43f57723b0ad0714258";
const BODY_SHA256: &str = "36f996e74e7aa93c263a74678aca46bc1927607b32d14ebbda9e67b21e6b2d75";
const SCHEMA_SHA256: &str = "4255feacfd464adf2cbbf1139ecdf30d9d00b847775c556407367ad1449d9e63";
const PREDICTION_SHA256: &str = "f3a592ad322889eb555cd258b4b5c304f3b261b2f055408f3e6e95d45ef4c0bf";
const SOURCE_BYTES: &[u8] = b"source-canonical\n";
const BODY_BYTES: &[u8] = b"body-canonical\n";

struct NodeOracleValues;

impl ApiMaintenanceReceiptCanonicalValues for NodeOracleValues {
    /// Resolve only the compact fixture clocks.
    fn canonical_utc_epoch_milliseconds(&self, value: &str) -> Option<i64> {
        match value {
            ISSUED_AT => Some(0),
            FIRST_VALID_AT => Some(3_600_000),
            LAST_VALID_AT => Some(7_200_000),
            _ => None,
        }
    }

    /// Admit only preimages whose digests were produced by Node's crypto implementation.
    fn sha256(&self, bytes: &[u8]) -> Option<Sha256Digest> {
        let digest = match bytes {
            SOURCE_BYTES => INPUT_SHA256,
            BODY_BYTES => BODY_SHA256,
            b"adjustment-shadow-source-row/v1\ntemperature\n{\"validAt\":\"2026-11-24T01:00:00.000Z\",\"leadHours\":1}\n" => FIRST_ROW_SHA256,
            b"adjustment-shadow-source-row/v1\ntemperature\n{\"validAt\":\"2026-11-24T02:00:00.000Z\",\"leadHours\":2}\n" => LAST_ROW_SHA256,
            b"adjustment-shadow-source-receipt/v1\naaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\nbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\ncccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc\ntemperature:2026-11-24T00:00:00.000Z\n2026-11-24T00:00:00.000Z\n2026-11-24T01:00:00.000Z\n2026-11-24T02:00:00.000Z\n2\nd419d5f151d681b716fe149cc1e48e7f747b37b674cb9c7c18fecb1f9437af96" => SOURCE_RECEIPT_SHA256,
            b"adjustment-shadow-prediction/v3\naaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\nbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\ncccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc\ntemperature:2026-11-24T00:00:00.000Z\n2026-11-24T00:00:00.000Z\n2026-11-24T01:00:00.000Z\n2026-11-24T02:00:00.000Z\n73f5398e30ad8b46c96d0feeb84c8e791ef82e204e82e43f57723b0ad0714258\nd419d5f151d681b716fe149cc1e48e7f747b37b674cb9c7c18fecb1f9437af96\n36f996e74e7aa93c263a74678aca46bc1927607b32d14ebbda9e67b21e6b2d75\n4255feacfd464adf2cbbf1139ecdf30d9d00b847775c556407367ad1449d9e63\n2\n15" => PREDICTION_SHA256,
            _ => return None,
        };
        Some(Sha256Digest::admitted(digest))
    }

    /// No receipt JSON is encoded while projecting metadata.
    fn unsigned_receipt_sha256(
        &self,
        _: &[(&str, MaintenanceReceiptScalar<'_>)],
    ) -> Option<Sha256Digest> {
        unreachable!("metadata projection must not encode receipt JSON")
    }
}

impl ApiMaintenanceShadowEvidenceValues for NodeOracleValues {
    /// Decoding belongs to the preceding admission boundary.
    fn decode_shadow_json(&self, _: &[u8]) -> Option<JsonValue> {
        unreachable!("the fixture supplies already decoded documents")
    }

    /// Preserve the source row's original two-field order for the Node preimage.
    fn encode_shadow_ordered_json(&self, ordered_row: &JsonValue) -> Option<Vec<u8>> {
        let first = source_row(FIRST_VALID_AT, 1);
        let last = source_row(LAST_VALID_AT, 2);
        match ordered_row {
            value if value == &first => {
                Some(b"{\"validAt\":\"2026-11-24T01:00:00.000Z\",\"leadHours\":1}".to_vec())
            }
            value if value == &last => {
                Some(b"{\"validAt\":\"2026-11-24T02:00:00.000Z\",\"leadHours\":2}".to_vec())
            }
            _ => None,
        }
    }

    /// Metadata projection does not decode embedded comparator members.
    fn decode_shadow_comparator_base64(&self, _: &[u16]) -> Option<Vec<u8>> {
        unreachable!()
    }

    /// Metadata projection does not encode comparator receipts.
    fn encode_shadow_sorted_json(&self, _: &JsonValue) -> Option<Vec<u8>> {
        unreachable!()
    }

    /// This fixture supplies no decoded maintenance package.
    fn decode_shadow_maintenance_package(
        &self,
        _: &JsonValue,
    ) -> Option<weather_contracts::ForecastAdjustmentMaintenanceRuntimePackage> {
        unreachable!()
    }

    /// This fixture supplies no decoded portable rain model.
    fn decode_shadow_rain_artifact(
        &self,
        _: &JsonValue,
    ) -> Option<weather_contracts::RainHurdleWindPortableArtifact> {
        unreachable!()
    }

    /// This fixture never examines a general wind runtime string.
    fn check_legacy_wind_runtime_value(&self, _: &[u16]) -> Option<()> {
        unreachable!()
    }
}

/// Build one ordered dependency-free JSON object.
fn object(fields: Vec<(&str, JsonValue)>) -> JsonValue {
    JsonValue::Object(
        fields
            .into_iter()
            .map(|(name, value)| (name.encode_utf16().collect(), value))
            .collect(),
    )
}

/// Build one dependency-free JSON string.
fn string(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}

/// Build one compact decoded source row.
fn source_row(valid_at: &str, lead_hours: u32) -> JsonValue {
    object(vec![
        ("validAt", string(valid_at)),
        ("leadHours", JsonValue::Number(f64::from(lead_hours))),
    ])
}

/// Build one compact decoded prediction row.
fn body_row(valid_at: &str, lead_hours: u32, source_row_sha256: &str) -> JsonValue {
    object(vec![
        ("validAt", string(valid_at)),
        ("leadHours", JsonValue::Number(f64::from(lead_hours))),
        ("sourceRowSha256", string(source_row_sha256)),
    ])
}

/// Build the shared header in original source order.
fn source() -> JsonValue {
    object(vec![
        (
            "contractVersion",
            string("adjustment-shadow-source-projection/v1"),
        ),
        ("family", string("temperature")),
        ("registrationSha256", string(REGISTRATION)),
        ("candidateSha256", string(CANDIDATE)),
        ("sourceSha256", string(SOURCE)),
        ("dueKey", string(DUE_KEY)),
        ("issuedAt", string(ISSUED_AT)),
        ("rowCount", JsonValue::Number(2.0)),
        (
            "rows",
            JsonValue::Array(vec![
                source_row(FIRST_VALID_AT, 1),
                source_row(LAST_VALID_AT, 2),
            ]),
        ),
    ])
}

/// Build the shared header and exact source-row bindings in prediction order.
fn body(last_source_row_sha256: &str) -> JsonValue {
    object(vec![
        (
            "contractVersion",
            string("adjustment-shadow-prediction-values/v3"),
        ),
        ("family", string("temperature")),
        ("registrationSha256", string(REGISTRATION)),
        ("candidateSha256", string(CANDIDATE)),
        ("sourceSha256", string(SOURCE)),
        ("dueKey", string(DUE_KEY)),
        ("issuedAt", string(ISSUED_AT)),
        ("sourceReceiptSha256", string(SOURCE_RECEIPT_SHA256)),
        ("inputSha256", string(INPUT_SHA256)),
        ("rowCount", JsonValue::Number(2.0)),
        (
            "rows",
            JsonValue::Array(vec![
                body_row(FIRST_VALID_AT, 1, FIRST_ROW_SHA256),
                body_row(LAST_VALID_AT, 2, last_source_row_sha256),
            ]),
        ),
    ])
}

#[test]
fn metadata_matches_node_preimages_and_rejects_a_substituted_source_row() {
    let values = NodeOracleValues;
    let source = source();
    let matching_body = body(LAST_ROW_SHA256);
    let metadata =
        shadow_prediction_metadata(&values, &source, &matching_body, SOURCE_BYTES, BODY_BYTES)
            .expect("the exact Node-bound rows must project");

    assert_eq!(metadata.bodyByteCount, 15);
    assert_eq!(metadata.candidateSha256, CANDIDATE);
    assert_eq!(metadata.dueKey, DUE_KEY);
    assert_eq!(metadata.inputSha256, INPUT_SHA256);
    assert_eq!(metadata.issuedAt, ISSUED_AT);
    assert_eq!(metadata.minValidAt, FIRST_VALID_AT);
    assert_eq!(metadata.maxValidAt, LAST_VALID_AT);
    assert_eq!(metadata.predictionBodySha256, BODY_SHA256);
    assert_eq!(metadata.predictionSchemaSha256, SCHEMA_SHA256);
    assert_eq!(metadata.predictionSha256, PREDICTION_SHA256);
    assert_eq!(metadata.registrationSha256, REGISTRATION);
    assert_eq!(metadata.rowCount, 2);
    assert_eq!(metadata.sourceReceiptSha256, SOURCE_RECEIPT_SHA256);
    assert_eq!(metadata.sourceSha256, SOURCE);

    let substituted_body = body(FIRST_ROW_SHA256);
    assert!(
        shadow_prediction_metadata(
            &values,
            &source,
            &substituted_body,
            SOURCE_BYTES,
            BODY_BYTES,
        )
        .is_none()
    );
}
