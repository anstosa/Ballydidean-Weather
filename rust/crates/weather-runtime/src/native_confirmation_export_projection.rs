//! native framing of the retained confirmation availability and authorization exports.

use std::fmt;
use std::io::{self, Read};

use jiff::{Span, civil::Date};
use weather_contracts::JsonValue;
use weather_database::Sha256Digest;
use weather_edge::{ArchiveWirePrimitives, RevisionArchiveCanonicalCodec};

use crate::{
    AdjustmentEvaluationPackageManifestValues, MaintenanceCanonicalJson, NativeWirePrimitives,
    adjustment_evaluation_package_manifest::validate_database_manifest,
    maintenance_canonical_json::{exact_keys, field, text, valid_hash},
};

const MAXIMUM_DATABASE_RESPONSE_BYTES: usize = 768 * 1_024;
const MAXIMUM_AVAILABILITY_BYTES: usize = 512 * 1_024;
const MAXIMUM_AUTHORIZATION_BYTES: usize = 8 * 1_024;
const AVAILABILITY_ENVELOPE_VERSION: &str = "adjustment-confirmation-availability-envelope/v2";
const AUTHORIZATION_ENVELOPE_VERSION: &str =
    "adjustment-confirmation-export-authorization-envelope/v2";
const ENVELOPE_FIELDS: &[&str] = &[
    "contractVersion",
    "databaseManifest",
    "payload",
    "payloadSha256",
    "transaction",
];
const DATABASE_RESPONSE_FIELDS: &[&str] = &["databaseManifest", "payload", "transaction"];
const TRANSACTION_FIELDS: &[&str] = &[
    "created_at_utc",
    "idle_in_transaction_session_timeout",
    "isolation_level",
    "lock_timeout",
    "read_only",
    "statement_timeout",
];
const AVAILABILITY_FIELDS: &[&str] = &[
    "contractVersion",
    "expectedKeySetSha256",
    "family",
    "finalizedMetadataRootSha256",
    "finalizedPredictionCount",
    "finalizedThroughAt",
    "hotPredictionCount",
    "hotPredictions",
    "hotSetRootSha256",
    "intervalEndAt",
    "intervalStartAt",
    "metadataGeneration",
    "missingExpectedDueKeys",
    "missingExpectedDueKeysStatus",
    "registrationSha256",
    "targetCutoffAt",
];
const HOT_PREDICTION_FIELDS: &[&str] = &[
    "dueKey",
    "maxValidAt",
    "minValidAt",
    "predictionSha256",
    "rowCount",
];
const AUTHORIZATION_FIELDS: &[&str] = &[
    "accessSha256",
    "chunkCount",
    "chunkIndex",
    "contractVersion",
    "eligiblePredictionSetSha256",
    "expectedKeySetSha256",
    "family",
    "fromLocalDate",
    "metadataRootSha256",
    "registrationSha256",
    "revisionCatalogWatermarkSha256",
    "targetComparatorSnapshotRootSha256",
    "targetCutoffAt",
    "toLocalDateExclusive",
];

/// Admit the exact complete maintenance ledger with no function payload.
pub fn verify_native_adjustment_evaluation_v2_readiness(input: impl Read) -> io::Result<()> {
    let response = read_database_response(input)?;
    // retain the original metadata-only readiness contract
    if !matches!(response.payload, JsonValue::Null) {
        return Err(invalid_projection());
    }
    Ok(())
}

/// frame one value-free confirmation availability SQL response.
pub fn frame_native_adjustment_confirmation_availability_v2(
    input: impl Read,
    registration_sha256: &str,
) -> io::Result<Vec<u8>> {
    if !valid_hash(registration_sha256) {
        return Err(invalid_projection());
    }
    let response = read_database_response(input)?;
    validate_availability_payload(&response.payload, registration_sha256)?;
    frame_envelope(
        AVAILABILITY_ENVELOPE_VERSION,
        response.manifest,
        response.payload,
        response.transaction,
    )
}

/// frame one persisted-burn confirmation authorization SQL response.
pub fn frame_native_adjustment_confirmation_authorization_v2(
    input: impl Read,
    registration_sha256: &str,
    access_sha256: &str,
    chunk_index: u32,
) -> io::Result<Vec<u8>> {
    if !valid_hash(registration_sha256) || !valid_hash(access_sha256) || chunk_index > 26 {
        return Err(invalid_projection());
    }
    let response = read_database_response(input)?;
    validate_authorization_payload(
        &response.payload,
        registration_sha256,
        access_sha256,
        chunk_index,
    )?;
    frame_envelope(
        AUTHORIZATION_ENVELOPE_VERSION,
        response.manifest,
        response.payload,
        response.transaction,
    )
}

/// read one canonical authorization envelope and return its inclusive legacy date interval.
pub fn read_native_adjustment_confirmation_authorization_dates(
    bytes: &[u8],
) -> io::Result<(String, String)> {
    let envelope = admit_native_adjustment_confirmation_authorization_v2(bytes)?;
    let payload = field(&envelope, "payload").ok_or_else(invalid_projection)?;
    let from = strict_date(payload, "fromLocalDate")?;
    let exclusive = strict_date(payload, "toLocalDateExclusive")?;
    let inclusive = add_days(exclusive, -1)?;
    Ok((from.to_string(), inclusive.to_string()))
}

/// admit one exact canonical persisted-burn authorization envelope.
pub fn admit_native_adjustment_confirmation_authorization_v2(
    bytes: &[u8],
) -> io::Result<JsonValue> {
    if bytes.is_empty() || bytes.len() > MAXIMUM_DATABASE_RESPONSE_BYTES {
        return Err(invalid_projection());
    }
    let wire = NativeWirePrimitives;
    let envelope = wire.decode_json(bytes).ok_or_else(invalid_projection)?;
    if !exact_keys(&envelope, ENVELOPE_FIELDS)
        || text(&envelope, "contractVersion").as_deref() != Some(AUTHORIZATION_ENVELOPE_VERSION)
        || wire.encode_sorted_json(&envelope).as_deref() != Some(bytes)
    {
        return Err(invalid_projection());
    }
    let manifest = field(&envelope, "databaseManifest").ok_or_else(invalid_projection)?;
    validate_complete_manifest(manifest)?;
    let transaction = field(&envelope, "transaction").ok_or_else(invalid_projection)?;
    validate_transaction(transaction)?;
    let payload = field(&envelope, "payload").ok_or_else(invalid_projection)?;
    let registration = text(payload, "registrationSha256").ok_or_else(invalid_projection)?;
    let access = text(payload, "accessSha256").ok_or_else(invalid_projection)?;
    let chunk = unsigned_integer(payload, "chunkIndex").ok_or_else(invalid_projection)?;
    let chunk = u32::try_from(chunk).map_err(|_| invalid_projection())?;
    validate_authorization_payload(payload, &registration, &access, chunk)?;
    let payload_sha256 = text(&envelope, "payloadSha256").ok_or_else(invalid_projection)?;
    if payload_digest(payload)?.as_str() != payload_sha256 {
        return Err(invalid_projection());
    }
    Ok(envelope)
}

struct DatabaseResponse {
    manifest: JsonValue,
    payload: JsonValue,
    transaction: JsonValue,
}

/// decode the bounded function response after all bytes arrive.
fn read_database_response(input: impl Read) -> io::Result<DatabaseResponse> {
    let mut bytes = Vec::new();
    input
        .take((MAXIMUM_DATABASE_RESPONSE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.is_empty() || bytes.len() > MAXIMUM_DATABASE_RESPONSE_BYTES {
        return Err(invalid_projection());
    }
    let value = NativeWirePrimitives
        .decode_json(&bytes)
        .ok_or_else(invalid_projection)?;
    if !exact_keys(&value, DATABASE_RESPONSE_FIELDS) {
        return Err(invalid_projection());
    }
    let manifest = field(&value, "databaseManifest")
        .cloned()
        .ok_or_else(invalid_projection)?;
    validate_complete_manifest(&manifest)?;
    let transaction = field(&value, "transaction")
        .cloned()
        .ok_or_else(invalid_projection)?;
    validate_transaction(&transaction)?;
    let payload = field(&value, "payload")
        .cloned()
        .ok_or_else(invalid_projection)?;
    Ok(DatabaseResponse {
        manifest,
        payload,
        transaction,
    })
}

/// admit only reviewed migration prefixes ending at 0018 through 0023.
fn validate_complete_manifest(manifest: &JsonValue) -> io::Result<()> {
    let mut values = ConfirmationManifestValues;
    validate_database_manifest(manifest, &mut values).map_err(|_| invalid_projection())?;
    let count = match field(manifest, "migration_names") {
        Some(JsonValue::Array(names)) => names.len(),
        _ => return Err(invalid_projection()),
    };
    if !(18..=23).contains(&count) {
        return Err(invalid_projection());
    }
    Ok(())
}

/// validate the fixed read-only transaction controls and server timestamp.
fn validate_transaction(transaction: &JsonValue) -> io::Result<()> {
    if !exact_keys(transaction, TRANSACTION_FIELDS)
        || [
            ("idle_in_transaction_session_timeout", "30s"),
            ("isolation_level", "repeatable read"),
            ("lock_timeout", "5s"),
            ("read_only", "on"),
            ("statement_timeout", "5min"),
        ]
        .iter()
        .any(|(key, expected)| text(transaction, key).as_deref() != Some(*expected))
    {
        return Err(invalid_projection());
    }
    let created = text(transaction, "created_at_utc").ok_or_else(invalid_projection)?;
    if NativeWirePrimitives
        .parse_explicit_utc_milliseconds(&created)
        .is_none()
    {
        return Err(invalid_projection());
    }
    Ok(())
}

/// validate one value-free availability payload without admitting observation rows.
fn validate_availability_payload(payload: &JsonValue, registration: &str) -> io::Result<()> {
    if !exact_keys(payload, AVAILABILITY_FIELDS)
        || text(payload, "contractVersion").as_deref()
            != Some("adjustment-confirmation-availability/v2")
        || text(payload, "registrationSha256").as_deref() != Some(registration)
        || !matches!(
            field(payload, "missingExpectedDueKeys"),
            Some(JsonValue::Null)
        )
        || text(payload, "missingExpectedDueKeysStatus").as_deref()
            != Some("requires_anchored_cold_reconstruction")
    {
        return Err(invalid_projection());
    }
    let family = family(payload)?;
    for key in [
        "expectedKeySetSha256",
        "finalizedMetadataRootSha256",
        "hotSetRootSha256",
    ] {
        if text(payload, key)
            .as_deref()
            .is_none_or(|value| !valid_hash(value))
        {
            return Err(invalid_projection());
        }
    }
    unsigned_integer(payload, "finalizedPredictionCount").ok_or_else(invalid_projection)?;
    unsigned_integer(payload, "metadataGeneration").ok_or_else(invalid_projection)?;
    canonical_instant(payload, "intervalStartAt")?;
    canonical_instant(payload, "intervalEndAt")?;
    canonical_instant(payload, "targetCutoffAt")?;
    match field(payload, "finalizedThroughAt") {
        Some(JsonValue::Null) => {}
        Some(_) => {
            canonical_instant(payload, "finalizedThroughAt")?;
        }
        None => return Err(invalid_projection()),
    }
    let predictions = match field(payload, "hotPredictions") {
        Some(JsonValue::Array(values)) => values,
        _ => return Err(invalid_projection()),
    };
    let declared =
        unsigned_integer(payload, "hotPredictionCount").ok_or_else(invalid_projection)?;
    if declared > 8_192 || declared != predictions.len() as u64 {
        return Err(invalid_projection());
    }
    let mut previous = String::new();
    // preserve the database's strict prediction identity order
    for prediction in predictions {
        if !exact_keys(prediction, HOT_PREDICTION_FIELDS) {
            return Err(invalid_projection());
        }
        let identity = text(prediction, "predictionSha256").ok_or_else(invalid_projection)?;
        let row_count = unsigned_integer(prediction, "rowCount").ok_or_else(invalid_projection)?;
        let maximum_rows = match family {
            "wind" => 168,
            "rain" => 23,
            _ => 12,
        };
        if !valid_hash(&identity)
            || identity <= previous
            || row_count == 0
            || row_count > maximum_rows
            || !text(prediction, "dueKey")
                .as_deref()
                .is_some_and(valid_due_key)
        {
            return Err(invalid_projection());
        }
        canonical_instant(prediction, "minValidAt")?;
        canonical_instant(prediction, "maxValidAt")?;
        previous = identity;
    }
    if canonical_value_size(payload)? > MAXIMUM_AVAILABILITY_BYTES {
        return Err(invalid_projection());
    }
    Ok(())
}

/// validate one persisted-burn authorization payload and its partition geometry.
fn validate_authorization_payload(
    payload: &JsonValue,
    registration: &str,
    access: &str,
    chunk_index: u32,
) -> io::Result<()> {
    if !exact_keys(payload, AUTHORIZATION_FIELDS)
        || text(payload, "contractVersion").as_deref()
            != Some("adjustment-confirmation-export-authorization/v2")
        || text(payload, "registrationSha256").as_deref() != Some(registration)
        || text(payload, "accessSha256").as_deref() != Some(access)
        || unsigned_integer(payload, "chunkIndex") != Some(u64::from(chunk_index))
    {
        return Err(invalid_projection());
    }
    let family = family(payload)?;
    let expected_chunks = if family == "rain" { 24 } else { 27 };
    if unsigned_integer(payload, "chunkCount") != Some(expected_chunks)
        || u64::from(chunk_index) >= expected_chunks
    {
        return Err(invalid_projection());
    }
    for key in [
        "eligiblePredictionSetSha256",
        "expectedKeySetSha256",
        "metadataRootSha256",
        "revisionCatalogWatermarkSha256",
        "targetComparatorSnapshotRootSha256",
    ] {
        if text(payload, key)
            .as_deref()
            .is_none_or(|value| !valid_hash(value))
        {
            return Err(invalid_projection());
        }
    }
    canonical_instant(payload, "targetCutoffAt")?;
    let from = strict_date(payload, "fromLocalDate")?;
    let exclusive = strict_date(payload, "toLocalDateExclusive")?;
    if !date_span_is_admitted(from, exclusive)?
        || canonical_value_size(payload)? > MAXIMUM_AUTHORIZATION_BYTES
    {
        return Err(invalid_projection());
    }
    Ok(())
}

/// build one canonical envelope and bind its payload digest to exact canonical bytes.
fn frame_envelope(
    version: &str,
    manifest: JsonValue,
    payload: JsonValue,
    transaction: JsonValue,
) -> io::Result<Vec<u8>> {
    let digest = payload_digest(&payload)?;
    let envelope = JsonValue::Object(vec![
        member("contractVersion", string(version)),
        member("databaseManifest", manifest),
        member("payload", payload),
        member("payloadSha256", string(digest.as_str())),
        member("transaction", transaction),
    ]);
    NativeWirePrimitives
        .encode_sorted_json(&envelope)
        .ok_or_else(invalid_projection)
}

/// hash the exact canonical payload record including its terminating line feed.
fn payload_digest(payload: &JsonValue) -> io::Result<Sha256Digest> {
    let bytes = NativeWirePrimitives
        .encode_sorted_json(payload)
        .ok_or_else(invalid_projection)?;
    MaintenanceCanonicalJson::sha256(&mut NativeWirePrimitives, &bytes)
        .ok_or_else(invalid_projection)
}

/// measure canonical JSON value bytes without the record line feed.
fn canonical_value_size(value: &JsonValue) -> io::Result<usize> {
    NativeWirePrimitives
        .encode_sorted_json(value)
        .and_then(|bytes| bytes.len().checked_sub(1))
        .ok_or_else(invalid_projection)
}

/// decode one closed adjustment family.
fn family(value: &JsonValue) -> io::Result<&str> {
    let Some(JsonValue::String(units)) = field(value, "family") else {
        return Err(invalid_projection());
    };
    match units.as_slice() {
        [114, 97, 105, 110] => Ok("rain"),
        [116, 101, 109, 112, 101, 114, 97, 116, 117, 114, 101] => Ok("temperature"),
        [119, 105, 110, 100] => Ok("wind"),
        _ => Err(invalid_projection()),
    }
}

/// decode one nonnegative JavaScript-safe integer field.
fn unsigned_integer(value: &JsonValue, key: &str) -> Option<u64> {
    let JsonValue::Number(number) = field(value, key)? else {
        return None;
    };
    (number.is_finite()
        && number.fract() == 0.0
        && *number >= 0.0
        && *number <= 9_007_199_254_740_991.0)
        .then_some(*number as u64)
}

/// validate one canonical UTC millisecond instant.
fn canonical_instant(value: &JsonValue, key: &str) -> io::Result<i64> {
    let value = text(value, key).ok_or_else(invalid_projection)?;
    NativeWirePrimitives
        .canonical_utc_milliseconds(&value)
        .ok_or_else(invalid_projection)
}

/// validate one strict Gregorian calendar date.
fn strict_date(value: &JsonValue, key: &str) -> io::Result<Date> {
    let value = text(value, key).ok_or_else(invalid_projection)?;
    let date = value.parse::<Date>().map_err(|_| invalid_projection())?;
    if value.len() != 10 || date.to_string() != value {
        return Err(invalid_projection());
    }
    Ok(date)
}

/// require an exclusive date one through fourteen days after its start.
fn date_span_is_admitted(start: Date, exclusive: Date) -> io::Result<bool> {
    let mut cursor = start;
    // compare only the predecessor's closed fourteen-day interval
    for _ in 1..=14 {
        cursor = add_days(cursor, 1)?;
        if cursor == exclusive {
            return Ok(true);
        }
        if cursor > exclusive {
            return Ok(false);
        }
    }
    Ok(false)
}

/// add whole Gregorian days without assuming a local timezone duration.
fn add_days(date: Date, days: i64) -> io::Result<Date> {
    let span = Span::new()
        .try_days(days)
        .map_err(|_| invalid_projection())?;
    date.checked_add(span).map_err(|_| invalid_projection())
}

/// validate one fixed capture due-key spelling without normalizing it.
fn valid_due_key(value: &str) -> bool {
    let Some(instant) = value.strip_prefix("capture/") else {
        return false;
    };
    if NativeWirePrimitives
        .canonical_utc_milliseconds(instant)
        .is_none()
    {
        return false;
    }
    matches!(instant.get(11..13), Some("00" | "06" | "12" | "18"))
        && instant.get(13..) == Some(":35:00.000Z")
}

/// construct one generated JSON object member.
fn member(key: &str, value: JsonValue) -> (Vec<u16>, JsonValue) {
    (key.encode_utf16().collect(), value)
}

/// construct one generated JSON string.
fn string(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}

struct ConfirmationManifestValues;

impl AdjustmentEvaluationPackageManifestValues for ConfirmationManifestValues {
    type Error = ConfirmationManifestEffect;

    /// refuse filesystem access from the pure manifest validation adapter.
    fn read_bounded_regular_file(
        &mut self,
        _root: &str,
        _relative_path: &str,
        _maximum_bytes: u64,
        _description: &str,
    ) -> Result<Vec<u8>, Self::Error> {
        Err(ConfirmationManifestEffect)
    }

    /// decode JSON through the approved native wire owner.
    fn decode_json(&mut self, bytes: &[u8]) -> Result<JsonValue, Self::Error> {
        NativeWirePrimitives
            .decode_json(bytes)
            .ok_or(ConfirmationManifestEffect)
    }

    /// hash the exact migration ledger preimage through the approved native owner.
    fn sha256(&mut self, bytes: &[u8]) -> Option<Sha256Digest> {
        MaintenanceCanonicalJson::sha256(&mut NativeWirePrimitives, bytes)
    }

    /// parse only explicit instants if a future shared validator requests one.
    fn utc_epoch_milliseconds(&mut self, instant: &JsonValue) -> Option<i64> {
        let JsonValue::String(units) = instant else {
            return None;
        };
        NativeWirePrimitives.parse_explicit_utc_milliseconds(&String::from_utf16(units).ok()?)
    }

    /// refuse unused local-date projection from the manifest-only adapter.
    fn local_date_ordinal(&mut self, _date: &str) -> Option<i64> {
        None
    }

    /// refuse filesystem enumeration from the pure manifest validation adapter.
    fn collect_regular_files(&mut self, _root: &str) -> Result<Vec<String>, Self::Error> {
        Err(ConfirmationManifestEffect)
    }
}

#[derive(Debug)]
struct ConfirmationManifestEffect;

impl fmt::Display for ConfirmationManifestEffect {
    /// render the fixed unreachable effect diagnostic.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("confirmation manifest effect is unavailable")
    }
}

/// collapse malformed, over-bound, or cross-authority input to one projection refusal.
fn invalid_projection() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "confirmation export projection is invalid",
    )
}

#[cfg(test)]
#[path = "native_confirmation_export_projection_test.rs"]
mod tests;
