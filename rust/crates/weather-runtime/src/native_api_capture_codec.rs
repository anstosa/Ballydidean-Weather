//! Exact native JSON encoders for source, public prediction, and comparator capture bytes.

use sha2::{Digest, Sha256};
use weather_api::{
    MAINTENANCE_SOURCE_PROJECTION_VERSION, MaintenanceComparatorCanonicalCodec,
    MaintenanceComparatorDocument, MaintenanceComparatorField, MaintenanceComparatorRows,
    MaintenanceComparatorValue, MaintenancePredictionBody, MaintenancePredictionCanonicalCodec,
    MaintenancePredictionField, MaintenancePredictionRow, MaintenancePredictionValue,
    MaintenanceSourceCanonicalCodec, MaintenanceSourceField, MaintenanceSourceProjection,
    MaintenanceSourceProjectionRows, MaintenanceSourceValue,
};
use weather_contracts::{
    EcmwfTemperatureCanaryRecentErrorState, EcmwfTemperatureCanaryRecentErrorStateCohortLiteral,
    JsonValue, MaintenanceShadowComparatorAuthorityKind, MaintenanceShadowServingAuthority,
    Nullable,
};
use weather_database::Sha256Digest;
use weather_edge::ArchiveWirePrimitives;

use crate::{
    NativeModelEvidenceValues, NativeWirePrimitives,
    api_shadow_comparator_authority::admit_shadow_comparator_authority,
    api_shadow_wire_validation::ShadowFamily,
};

const MAXIMUM_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

/// Zero-state capture canonical owner over the existing native wire and artifact admission.
#[derive(Clone, Copy, Default)]
pub struct NativeApiCaptureCodec;

impl MaintenanceSourceCanonicalCodec for NativeApiCaptureCodec {
    /// Preserve the exact source header, temperature causal state, and admitted row order.
    fn encode_source(&mut self, projection: &MaintenanceSourceProjection<'_>) -> Option<Vec<u8>> {
        let header = projection.header();
        let mut fields = common_header(
            MAINTENANCE_SOURCE_PROJECTION_VERSION,
            projection.family(),
            header.registration_sha256,
            header.candidate_sha256,
            header.source_sha256,
            header.due_key,
            &header.issued_at.utc,
        );
        // only the temperature source carries the causal rolling-error state
        let rows = match projection.rows() {
            MaintenanceSourceProjectionRows::Wind(rows) => rows
                .iter()
                .map(|row| source_row(row))
                .collect::<Option<Vec<_>>>()?,
            MaintenanceSourceProjectionRows::Temperature {
                recent_error_state,
                rows,
            } => {
                fields.push(("recentErrorState", recent_state(recent_error_state)?));
                rows.iter()
                    .map(|row| source_row(row))
                    .collect::<Option<Vec<_>>>()?
            }
        };
        fields.push(("rowCount", count(rows.len().try_into().ok()?)?));
        fields.push(("rows", JsonValue::Array(rows)));
        NativeWirePrimitives.encode_ordered_json(&object(fields))
    }

    /// Encode the original row bytes without the document's framing newline.
    fn encode_row(&mut self, fields: &[MaintenanceSourceField]) -> Option<Vec<u8>> {
        let mut bytes = NativeWirePrimitives.encode_ordered_json(&source_row(fields)?)?;
        (bytes.pop()? == b'\n').then_some(bytes)
    }

    /// Hash ordered byte segments in one streaming digest.
    fn sha256(&mut self, parts: &[&[u8]]) -> Option<Sha256Digest> {
        digest(parts)
    }
}

impl MaintenancePredictionCanonicalCodec for NativeApiCaptureCodec {
    /// Serialize the exact public body and the pre-admitted family row fields.
    fn encode_prediction(&mut self, body: &MaintenancePredictionBody<'_>) -> Option<Vec<u8>> {
        let mut fields = common_header(
            body.contract_version,
            body.family.as_str(),
            body.registration_sha256,
            body.candidate_sha256,
            body.source_sha256,
            body.due_key,
            body.issued_at,
        );
        let rows = body
            .rows
            .iter()
            .map(|row| match row {
                MaintenancePredictionRow::Temperature(row) => prediction_row(row),
                MaintenancePredictionRow::Wind(row) => prediction_row(row),
            })
            .collect::<Option<Vec<_>>>()?;
        fields.extend([
            ("sourceReceiptSha256", text(body.source_receipt_sha256)),
            ("inputSha256", text(body.input_sha256)),
            ("rowCount", count(body.row_count)?),
            ("rows", JsonValue::Array(rows)),
        ]);
        NativeWirePrimitives.encode_ordered_json(&object(fields))
    }

    /// Hash the original ordered public identity segments.
    fn sha256(&mut self, parts: &[&[u8]]) -> Option<Sha256Digest> {
        digest(parts)
    }
}

impl MaintenanceComparatorCanonicalCodec for NativeApiCaptureCodec {
    /// Compute the actual member identity over exact immutable bytes.
    fn sha256(&mut self, bytes: &[u8]) -> Option<Sha256Digest> {
        digest(&[bytes])
    }

    /// Reuse full retained serving authority admission instead of trusting a typed bundle.
    fn validate_authority(
        &mut self,
        authority: &MaintenanceShadowServingAuthority,
        family: &str,
    ) -> bool {
        let family = match family {
            "temperature" => ShadowFamily::Temperature,
            "wind" => ShadowFamily::Wind,
            "rain" => ShadowFamily::Rain,
            _ => return false,
        };
        admit_shadow_comparator_authority(
            &NativeModelEvidenceValues::new(),
            &authority_object(authority),
            family,
        )
        .is_some()
    }

    /// Encode the original comparator header, authority and row order with one LF.
    fn encode_comparator(
        &mut self,
        document: &MaintenanceComparatorDocument<'_>,
    ) -> Option<Vec<u8>> {
        let mut fields = common_header(
            document.contract_version,
            document.family,
            document.registration_sha256,
            document.candidate_sha256,
            document.source_sha256,
            document.due_key,
            document.issued_at,
        );
        let rows = match &document.rows {
            MaintenanceComparatorRows::Wind(rows) => rows
                .iter()
                .map(|row| comparator_row(row))
                .collect::<Option<Vec<_>>>()?,
            MaintenanceComparatorRows::Temperature(rows) => rows
                .iter()
                .map(|row| comparator_row(row))
                .collect::<Option<Vec<_>>>()?,
        };
        fields.extend([
            (
                "sourceProjectionSha256",
                text(document.source_projection_sha256.as_str()),
            ),
            (
                "predictionBodySha256",
                text(document.prediction_body_sha256),
            ),
            (
                "servingAuthority",
                authority_object(document.serving_authority),
            ),
            ("rowCount", count(document.row_count.try_into().ok()?)?),
            ("rows", JsonValue::Array(rows)),
        ]);
        NativeWirePrimitives.encode_ordered_json(&object(fields))
    }
}

/// Preserve the common TypeScript field order across the three original documents.
fn common_header(
    version: &str,
    family: &str,
    registration: &str,
    candidate: &str,
    source: &str,
    due: &str,
    issued: &str,
) -> Vec<(&'static str, JsonValue)> {
    vec![
        ("contractVersion", text(version)),
        ("family", text(family)),
        ("registrationSha256", text(registration)),
        ("candidateSha256", text(candidate)),
        ("sourceSha256", text(source)),
        ("dueKey", text(due)),
        ("issuedAt", text(issued)),
    ]
}

/// Preserve all ordered source fields including UTF-16 source text.
fn source_row(fields: &[MaintenanceSourceField]) -> Option<JsonValue> {
    let fields = fields
        .iter()
        .map(|(key, value)| {
            Some((
                *key,
                match value {
                    MaintenanceSourceValue::Text(value) => text(value),
                    MaintenanceSourceValue::Utf16(value) => JsonValue::String(value.clone()),
                    MaintenanceSourceValue::Number(value) => count(*value)?,
                    MaintenanceSourceValue::Null => JsonValue::Null,
                },
            ))
        })
        .collect::<Option<Vec<_>>>()?;
    Some(object(fields))
}

/// Preserve the closed public prediction field grammar.
fn prediction_row(fields: &[MaintenancePredictionField]) -> Option<JsonValue> {
    let fields = fields
        .iter()
        .map(|(key, value)| {
            Some((
                *key,
                match value {
                    MaintenancePredictionValue::Text(value) => text(value),
                    MaintenancePredictionValue::Number(value) => count(*value)?,
                    MaintenancePredictionValue::Boolean(value) => JsonValue::Boolean(*value),
                    MaintenancePredictionValue::Null => JsonValue::Null,
                },
            ))
        })
        .collect::<Option<Vec<_>>>()?;
    Some(object(fields))
}

/// Preserve the closed comparator field grammar.
fn comparator_row(fields: &[MaintenanceComparatorField]) -> Option<JsonValue> {
    let fields = fields
        .iter()
        .map(|(key, value)| {
            Some((
                *key,
                match value {
                    MaintenanceComparatorValue::Text(value) => text(value),
                    MaintenanceComparatorValue::Number(value) => count(*value)?,
                    MaintenanceComparatorValue::Boolean(value) => JsonValue::Boolean(*value),
                    MaintenanceComparatorValue::Null => JsonValue::Null,
                },
            ))
        })
        .collect::<Option<Vec<_>>>()?;
    Some(object(fields))
}

/// Materialize the exact original nested temperature state field order.
fn recent_state(state: &EcmwfTemperatureCanaryRecentErrorState) -> Option<JsonValue> {
    Some(object([
        ("b24C", nullable_number(&state.b24C)?),
        ("b72C", nullable_number(&state.b72C)?),
        (
            "cohort",
            text(EcmwfTemperatureCanaryRecentErrorStateCohortLiteral::WIRE_VALUE),
        ),
        ("localDates", count(state.localDates)?),
        ("mad72C", nullable_number(&state.mad72C)?),
        (
            "maximumSourceRunInitializedAt",
            nullable_text(&state.maximumSourceRunInitializedAt),
        ),
        (
            "maximumSourceValidAt",
            nullable_text(&state.maximumSourceValidAt),
        ),
        ("n24", count(state.n24)?),
        ("n72", count(state.n72)?),
        (
            "sourceKeys",
            JsonValue::Array(state.sourceKeys.iter().map(|value| text(value)).collect()),
        ),
        ("supported", JsonValue::Boolean(state.supported)),
        (
            "targetRunInitializedAt",
            text(&state.targetRunInitializedAt),
        ),
        ("windowEndValidAt", text(&state.windowEndValidAt)),
    ]))
}

/// Encode the exact original nullable authority members before admission or publication.
fn authority_object(authority: &MaintenanceShadowServingAuthority) -> JsonValue {
    let kind = match authority.authorityKind {
        MaintenanceShadowComparatorAuthorityKind::LegacyActive => "legacy_active",
        MaintenanceShadowComparatorAuthorityKind::MaintenanceQualified => "maintenance_qualified",
        MaintenanceShadowComparatorAuthorityKind::PolicyRaw => "policy_raw",
    };
    object([
        ("artifactBase64", nullable_text(&authority.artifactBase64)),
        (
            "artifactIdentitySha256",
            nullable_text(&authority.artifactIdentitySha256),
        ),
        (
            "artifactMemberSha256",
            nullable_text(&authority.artifactMemberSha256),
        ),
        ("authorityKind", text(kind)),
        ("receiptBase64", text(&authority.receiptBase64)),
        ("receiptMemberSha256", text(&authority.receiptMemberSha256)),
    ])
}

/// Preserve JavaScript safe unsigned integer semantics.
fn count(value: u64) -> Option<JsonValue> {
    (value <= MAXIMUM_SAFE_INTEGER).then_some(JsonValue::Number(value as f64))
}

/// Retain a nullable finite value without normalizing it into a different type.
fn nullable_number(value: &Nullable<f64>) -> Option<JsonValue> {
    match value {
        Nullable::Null => Some(JsonValue::Null),
        Nullable::Value(value) => value.is_finite().then_some(JsonValue::Number(*value)),
    }
}

/// Retain explicit null rather than replacing it with an empty string.
fn nullable_text(value: &Nullable<String>) -> JsonValue {
    match value {
        Nullable::Null => JsonValue::Null,
        Nullable::Value(value) => text(value),
    }
}

/// Keep source text in the existing UTF-16 wire representation.
fn text(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}

/// Construct the original insertion-ordered object with no key sorting.
fn object(fields: impl IntoIterator<Item = (&'static str, JsonValue)>) -> JsonValue {
    JsonValue::Object(
        fields
            .into_iter()
            .map(|(key, value)| (key.encode_utf16().collect(), value))
            .collect(),
    )
}

/// Hash the exact concatenation without allocating a second buffer.
fn digest(parts: &[&[u8]]) -> Option<Sha256Digest> {
    use std::fmt::Write as _;
    let mut digest = Sha256::new();
    // consume every byte segment in source order
    for part in parts {
        digest.update(part);
    }
    let mut hex = String::with_capacity(64);
    // preserve the original lowercase digest spelling
    for byte in digest.finalize() {
        write!(&mut hex, "{byte:02x}").ok()?;
    }
    Some(Sha256Digest::admitted(hex))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Verify the original row bytes and streaming identity without a database or network.
    #[test]
    fn native_api_capture_codec_preserves_row_order_and_hash() {
        let mut codec = NativeApiCaptureCodec;
        let fields = [
            (
                "validAt",
                MaintenanceSourceValue::Text("2026-11-24T12:00:00.000Z".to_owned()),
            ),
            ("leadHours", MaintenanceSourceValue::Number(1)),
            (
                "sourceKey",
                MaintenanceSourceValue::Utf16(vec![0x41, 0xD800]),
            ),
            ("quality", MaintenanceSourceValue::Null),
        ];
        let row = codec.encode_row(&fields).unwrap();
        assert_eq!(row, br#"{"validAt":"2026-11-24T12:00:00.000Z","leadHours":1,"sourceKey":"A\ud800","quality":null}"#);
        let digest_a =
            MaintenanceSourceCanonicalCodec::sha256(&mut codec, &[&row[..20], &row[20..]]).unwrap();
        let digest_b = MaintenanceComparatorCanonicalCodec::sha256(&mut codec, &row).unwrap();
        assert_eq!(digest_a, digest_b);
    }
}
