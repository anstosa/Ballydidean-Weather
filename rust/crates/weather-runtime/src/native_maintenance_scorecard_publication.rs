//! Exact native projection of the controller's bounded v2 scorecard publication material.

use weather_contracts::JsonValue;
use weather_edge::{ArchiveWirePrimitives, RevisionArchiveCanonicalCodec};

use crate::{
    NativeWirePrimitives, forecast_scorecard_contract::FORECAST_SCORECARD_V2_MAX_BYTES,
    maintenance_canonical_json::valid_hash, native_maintenance_retirement_files_v2::format_digest,
    validate_forecast_adjustment_scorecard,
};

const SEVEN_DAYS_MS: i64 = 7 * 86_400_000;

/// Source values consumed by `buildAdjustmentMaintenanceScorecardV2`.
pub struct NativeMaintenanceScorecardPublicationInput<'a> {
    pub attempt_sha256: &'a str,
    pub families: &'a JsonValue,
    pub generated_at: &'a str,
    pub identities: &'a JsonValue,
    pub inputs: &'a JsonValue,
    pub job: &'a JsonValue,
    pub operator_state: &'a str,
    pub progress: &'a JsonValue,
    pub source_revision: &'a str,
    pub warnings: &'a JsonValue,
}

/// Canonical scorecard value, exact publication bytes, and their immutable identity.
#[derive(Clone, Debug, PartialEq)]
pub struct NativeMaintenanceScorecardPublication {
    pub bytes: Vec<u8>,
    pub scorecard: JsonValue,
    pub scorecard_sha256: String,
}

/// Closed refusal phases for scorecard material construction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMaintenanceScorecardPublicationError {
    InvalidInput,
    Encoding,
    TooLarge,
}

/// Build the source controller's pending/no-action v2 publication projection.
pub fn build_native_maintenance_scorecard_publication(
    input: NativeMaintenanceScorecardPublicationInput<'_>,
) -> Result<NativeMaintenanceScorecardPublication, NativeMaintenanceScorecardPublicationError> {
    use NativeMaintenanceScorecardPublicationError as Error;

    // reject the same immutable attempt identity before deriving any descendant hashes
    if !valid_hash(input.attempt_sha256) {
        return Err(Error::InvalidInput);
    }
    let wire = NativeWirePrimitives;
    let generated_ms = wire
        .canonical_utc_milliseconds(input.generated_at)
        .ok_or(Error::InvalidInput)?;
    let valid_through = generated_ms
        .checked_add(SEVEN_DAYS_MS)
        .and_then(|instant| wire.format_utc_milliseconds(instant))
        .ok_or(Error::InvalidInput)?;
    let no_action_identity = object(vec![
        ("attemptSha256", string(input.attempt_sha256)),
        (
            "contractVersion",
            string("forecast-adjustment-no-action-lineage/v1"),
        ),
        ("state", string("none")),
    ]);
    let action_sha256 = digest_json(&wire, &no_action_identity).ok_or(Error::Encoding)?;
    let action_lineage = object(vec![
        ("actionSha256", string(&action_sha256)),
        ("attemptSha256", string(input.attempt_sha256)),
        ("predecessorActionSha256", JsonValue::Null),
        ("releaseManifestSha256", JsonValue::Null),
        ("sourceRevision", string(input.source_revision)),
    ]);
    let action_projection = object(vec![
        ("actionLineage", action_lineage.clone()),
        ("actionState", string("none")),
        (
            "contractVersion",
            string("forecast-adjustment-action-projection/v2"),
        ),
        ("policyDecision", string("pending")),
    ]);
    let action_projection_sha256 = digest_json(&wire, &action_projection).ok_or(Error::Encoding)?;
    let action_lineage_sha256 = digest_json(&wire, &action_lineage).ok_or(Error::Encoding)?;
    let job =
        job_with_operator_state(input.job, input.operator_state).ok_or(Error::InvalidInput)?;
    let scorecard = object(vec![
        ("actionLineage", action_lineage),
        ("actionProjectionSha256", string(&action_projection_sha256)),
        ("actionState", string("none")),
        (
            "contractVersion",
            string("forecast-adjustment-scorecard/v2"),
        ),
        ("families", input.families.clone()),
        ("generatedAt", string(input.generated_at)),
        (
            "history",
            JsonValue::Array(vec![object(vec![
                ("actionLineageSha256", string(&action_lineage_sha256)),
                ("actionProjectionSha256", string(&action_projection_sha256)),
                ("actionState", string("none")),
                ("attemptSha256", string(input.attempt_sha256)),
                ("occurredAt", string(input.generated_at)),
                ("policyDecision", string("pending")),
                ("releaseManifestSha256", JsonValue::Null),
            ])]),
        ),
        ("identities", input.identities.clone()),
        ("inputs", input.inputs.clone()),
        ("job", job),
        ("policyDecision", string("pending")),
        ("progress", input.progress.clone()),
        ("siteKey", string("ballydidean")),
        ("validThrough", string(&valid_through)),
        ("warnings", input.warnings.clone()),
    ]);
    validate_forecast_adjustment_scorecard(&scorecard, Some(generated_ms), &wire)
        .map_err(|_| Error::InvalidInput)?;
    let bytes = wire.encode_sorted_json(&scorecard).ok_or(Error::Encoding)?;

    // enforce the source's tighter hot-publication envelope after canonical encoding
    if bytes.len() > FORECAST_SCORECARD_V2_MAX_BYTES {
        return Err(Error::TooLarge);
    }
    let scorecard_sha256 = format_digest(wire.sha256(&bytes));
    Ok(NativeMaintenanceScorecardPublication {
        bytes,
        scorecard,
        scorecard_sha256,
    })
}

/// Reproduce object spread followed by the source's final `operatorState` assignment.
fn job_with_operator_state(job: &JsonValue, operator_state: &str) -> Option<JsonValue> {
    let JsonValue::Object(mut fields) = job.clone() else {
        return None;
    };
    let operator_key: Vec<u16> = "operatorState".encode_utf16().collect();

    // preserve the original property position when object spread overwrites an existing key
    if let Some((_, value)) = fields.iter_mut().find(|(key, _)| *key == operator_key) {
        *value = string(operator_state);
    } else {
        fields.push((operator_key, string(operator_state)));
    }
    Some(JsonValue::Object(fields))
}

/// Hash one exact canonical source document.
fn digest_json(wire: &NativeWirePrimitives, value: &JsonValue) -> Option<String> {
    Some(format_digest(wire.sha256(&wire.encode_sorted_json(value)?)))
}

/// Construct one source-shaped JSON object without changing field values.
fn object(fields: Vec<(&str, JsonValue)>) -> JsonValue {
    JsonValue::Object(
        fields
            .into_iter()
            .map(|(key, value)| (key.encode_utf16().collect(), value))
            .collect(),
    )
}

/// Construct one exact generated JSON string.
fn string(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}

#[cfg(test)]
#[path = "native_maintenance_scorecard_publication_test.rs"]
mod tests;
