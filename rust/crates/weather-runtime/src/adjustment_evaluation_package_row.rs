//! admit one canonical adjustment-evaluation package row before member effects

use std::fmt::{self, Display};

use weather_contracts::JsonValue;

use crate::MaintenanceCanonicalJson;

const PACKAGED_ROW_KEYS: &[&str] = &[
    "body_member_path",
    "content_hash",
    "first_received_at",
    "last_received_at",
    "local_date",
    "payload",
    "record_kind",
    "record_revision_identity",
    "reference_at",
    "revision_count",
    "site_key",
    "source_identity",
    "valid_at",
];

/// approved JSON, calendar, and payload-inspection owners
pub trait AdjustmentEvaluationPackageRowValues: MaintenanceCanonicalJson {
    /// decode one JSON value with JavaScript object replacement and exact UTF-16 strings
    fn decode_json(&mut self, bytes: &[u8]) -> Option<JsonValue>;

    /// parse one timestamp with the original JavaScript Date behavior
    fn js_timestamp_milliseconds(&mut self, value: &JsonValue) -> Option<i64>;

    /// apply the predecessor's canonical local-date regular expression
    fn matches_canonical_local_date(&mut self, value: &JsonValue) -> bool;

    /// apply the predecessor's payload-only private-material rule
    fn payload_contains_forbidden_private_material(&mut self, payload: &JsonValue) -> bool;
}

/// closed record kinds from the adjustment evaluation V1 database contract
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdjustmentEvaluationRecordKind {
    TemperatureRun,
    TemperatureHour,
    RainClaim,
    RainReceipt,
    RainAdjustmentRun,
    TargetRevisionDiagnostic,
}

impl AdjustmentEvaluationRecordKind {
    /// decode one exact database record-kind spelling
    fn from_wire(value: &[u16]) -> Option<Self> {
        // retain the closed V1 set without aliases
        [
            ("temperature_run", Self::TemperatureRun),
            ("temperature_hour", Self::TemperatureHour),
            ("rain_claim", Self::RainClaim),
            ("rain_receipt", Self::RainReceipt),
            ("rain_adjustment_run", Self::RainAdjustmentRun),
            ("target_revision_diagnostic", Self::TargetRevisionDiagnostic),
        ]
        .into_iter()
        .find_map(|(wire, kind)| match_utf16(value, wire).then_some(kind))
    }

    /// return the exact payload-description prefix
    fn wire(self) -> &'static str {
        // keep diagnostics tied to the admitted wire identity
        match self {
            Self::TemperatureRun => "temperature_run",
            Self::TemperatureHour => "temperature_hour",
            Self::RainClaim => "rain_claim",
            Self::RainReceipt => "rain_receipt",
            Self::RainAdjustmentRun => "rain_adjustment_run",
            Self::TargetRevisionDiagnostic => "target_revision_diagnostic",
        }
    }
}

/// one optional retained provider-body claim before member lookup
#[derive(Debug, Clone, PartialEq)]
pub struct AdjustmentEvaluationPackageBodyClaim {
    pub member_path: String,
    pub body_sha256: Option<String>,
    pub plaintext_bytes: Option<f64>,
    pub compressed_bytes: Option<f64>,
}

/// one exact row plus the observation used by strict streaming
#[derive(Debug, Clone, PartialEq)]
pub struct AdmittedAdjustmentEvaluationPackageRow {
    pub original: JsonValue,
    pub record_kind: AdjustmentEvaluationRecordKind,
    pub source_identity: Vec<u16>,
    pub last_received_at: JsonValue,
    pub revision_count: f64,
    pub body: Option<AdjustmentEvaluationPackageBodyClaim>,
}

/// ordinary V1 row-admission failures in predecessor order
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdjustmentEvaluationPackageRowError {
    Json,
    Canonical,
    Object,
    Keys,
    Identity,
    PayloadObject(AdjustmentEvaluationRecordKind),
    PayloadKeys(AdjustmentEvaluationRecordKind),
    PayloadPrivateMaterial,
    BodyPath,
    BodyBinding,
}

impl Display for AdjustmentEvaluationPackageRowError {
    /// render the predecessor-compatible row diagnostic
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        // preserve stable text for every closed phase
        match self {
            Self::Json => formatter.write_str("packaged export row is invalid JSON"),
            Self::Canonical => formatter.write_str("packaged export row is not canonical"),
            Self::Object => formatter.write_str("packaged export row must be an object"),
            Self::Keys => formatter.write_str("packaged export row has unexpected keys"),
            Self::Identity => formatter.write_str("export row identity is invalid"),
            Self::PayloadObject(kind) => {
                write!(formatter, "{} payload must be an object", kind.wire())
            }
            Self::PayloadKeys(kind) => {
                write!(formatter, "{} payload has unexpected keys", kind.wire())
            }
            Self::PayloadPrivateMaterial => {
                formatter.write_str("export row payload contains forbidden private material")
            }
            Self::BodyPath => formatter.write_str("packaged export row body path is invalid"),
            Self::BodyBinding => formatter.write_str("packaged export row body binding is invalid"),
        }
    }
}

impl std::error::Error for AdjustmentEvaluationPackageRowError {}

/// admit one canonical JSONL row and retain its streaming observation
pub fn admit_adjustment_evaluation_package_row(
    line: &[u8],
    values: &mut impl AdjustmentEvaluationPackageRowValues,
) -> Result<AdmittedAdjustmentEvaluationPackageRow, AdjustmentEvaluationPackageRowError> {
    let row = values
        .decode_json(line)
        .ok_or(AdjustmentEvaluationPackageRowError::Json)?;
    let entries = object_entries(&row).ok_or(AdjustmentEvaluationPackageRowError::Object)?;
    // reject extensions and missing fields before typed reads
    if !exact_keys(entries, PACKAGED_ROW_KEYS) {
        return Err(AdjustmentEvaluationPackageRowError::Keys);
    }
    let mut expected = line.to_vec();
    expected.push(b'\n');
    let encoded = values
        .encode_canonical_json(&row)
        .ok_or(AdjustmentEvaluationPackageRowError::Canonical)?;
    // bind exact sorted JSONL bytes before interpreting claims
    if encoded != expected {
        return Err(AdjustmentEvaluationPackageRowError::Canonical);
    }

    // preserve the predecessor's left-to-right outer identity checks
    if !text_equals(entries, "site_key", "ballydidean") {
        return Err(AdjustmentEvaluationPackageRowError::Identity);
    }
    let kind = string_field(entries, "record_kind")
        .and_then(|value| AdjustmentEvaluationRecordKind::from_wire(value))
        .ok_or(AdjustmentEvaluationPackageRowError::Identity)?;
    let source_identity = string_field(entries, "source_identity")
        .filter(|value| value.len() <= 256)
        .cloned()
        .ok_or(AdjustmentEvaluationPackageRowError::Identity)?;
    let revision_identity = string_field(entries, "record_revision_identity")
        .filter(|value| value.len() <= 320)
        .ok_or(AdjustmentEvaluationPackageRowError::Identity)?;
    let local_date =
        field(entries, "local_date").ok_or(AdjustmentEvaluationPackageRowError::Identity)?;
    // preserve JavaScript regular-expression coercion at the owner boundary
    if !values.matches_canonical_local_date(local_date) {
        return Err(AdjustmentEvaluationPackageRowError::Identity);
    }
    let valid_at = timestamp_field(entries, "valid_at", values)?;
    // preserve nullable reference-time behavior
    if !nullable_timestamp_valid(entries, "reference_at", values) {
        return Err(AdjustmentEvaluationPackageRowError::Identity);
    }
    let first_received_at = timestamp_field(entries, "first_received_at", values)?;
    let last_received_at = timestamp_field(entries, "last_received_at", values)?.clone();
    let revision_count = integer_number_field(entries, "revision_count")
        .filter(|value| *value >= 0.0)
        .ok_or(AdjustmentEvaluationPackageRowError::Identity)?;
    // close the outer identity with the nullable hash claim
    if !nullable_hash_valid(entries, "content_hash") {
        return Err(AdjustmentEvaluationPackageRowError::Identity);
    }

    let payload_value = field(entries, "payload").expect("exact keys include payload");
    let payload = object_entries(payload_value)
        .ok_or(AdjustmentEvaluationPackageRowError::PayloadObject(kind))?;
    // bind the fixed record-kind-specific payload schema
    if !exact_keys(payload, payload_keys(kind)) {
        return Err(AdjustmentEvaluationPackageRowError::PayloadKeys(kind));
    }
    // retain the existing inspection through its process owner
    if values.payload_contains_forbidden_private_material(payload_value) {
        return Err(AdjustmentEvaluationPackageRowError::PayloadPrivateMaterial);
    }

    let body_path = nullable_string_field(entries, "body_member_path")
        .ok_or(AdjustmentEvaluationPackageRowError::BodyPath)?;
    // reject paths outside the sole content-addressed grammar
    if body_path
        .as_deref()
        .is_some_and(|path| !valid_body_member_path(path))
    {
        return Err(AdjustmentEvaluationPackageRowError::BodyPath);
    }
    let compressed_present = !matches!(field(payload, "compressedBytes"), Some(JsonValue::Null));
    // bind non-null bodies only to receipt payloads
    if (kind == AdjustmentEvaluationRecordKind::RainReceipt && compressed_present)
        != body_path.is_some()
    {
        return Err(AdjustmentEvaluationPackageRowError::BodyBinding);
    }

    let body = body_path.map(|member_path| AdjustmentEvaluationPackageBodyClaim {
        member_path,
        body_sha256: text_field(payload, "bodySha256"),
        plaintext_bytes: number_field(payload, "bodyBytes"),
        compressed_bytes: number_field(payload, "compressedBytes"),
    });
    let _ = (revision_identity, local_date, valid_at, first_received_at);
    Ok(AdmittedAdjustmentEvaluationPackageRow {
        original: row,
        record_kind: kind,
        source_identity,
        last_received_at,
        revision_count,
        body,
    })
}

/// borrow ordered entries from one JSON object
fn object_entries(value: &JsonValue) -> Option<&[(Vec<u16>, JsonValue)]> {
    // accept only JSON objects rather than arrays or null
    match value {
        JsonValue::Object(entries) => Some(entries),
        _ => None,
    }
}

/// borrow one named member with object replacement semantics
fn field<'a>(entries: &'a [(Vec<u16>, JsonValue)], key: &str) -> Option<&'a JsonValue> {
    entries
        .iter()
        .rev()
        .find(|(name, _)| match_utf16(name, key))
        .map(|(_, value)| value)
}

/// borrow one exact UTF-16 string field
fn string_field<'a>(entries: &'a [(Vec<u16>, JsonValue)], key: &str) -> Option<&'a Vec<u16>> {
    // retain lone surrogate code units like JavaScript strings
    match field(entries, key)? {
        JsonValue::String(value) => Some(value),
        _ => None,
    }
}

/// decode one UTF-16 string needed for a path or hash claim
fn text_field(entries: &[(Vec<u16>, JsonValue)], key: &str) -> Option<String> {
    String::from_utf16(string_field(entries, key)?).ok()
}

/// decode one required nullable string
fn nullable_string_field(entries: &[(Vec<u16>, JsonValue)], key: &str) -> Option<Option<String>> {
    // distinguish required null from malformed or missing data
    match field(entries, key)? {
        JsonValue::Null => Some(None),
        JsonValue::String(value) => String::from_utf16(value).ok().map(Some),
        _ => None,
    }
}

/// read one finite JSON number without narrowing its range
fn number_field(entries: &[(Vec<u16>, JsonValue)], key: &str) -> Option<f64> {
    // real JSON parsing excludes non-finite numbers
    match field(entries, key)? {
        JsonValue::Number(value) if value.is_finite() => Some(*value),
        _ => None,
    }
}

/// read one finite integral JSON number
fn integer_number_field(entries: &[(Vec<u16>, JsonValue)], key: &str) -> Option<f64> {
    number_field(entries, key).filter(|value| value.fract() == 0.0)
}

/// parse and borrow one required timestamp without normalizing its units
fn timestamp_field<'a>(
    entries: &'a [(Vec<u16>, JsonValue)],
    key: &str,
    values: &mut impl AdjustmentEvaluationPackageRowValues,
) -> Result<&'a JsonValue, AdjustmentEvaluationPackageRowError> {
    let value = field(entries, key).ok_or(AdjustmentEvaluationPackageRowError::Identity)?;
    // preserve the raw JSON value after Date.parse-compatible coercion
    if values.js_timestamp_milliseconds(value).is_none() {
        return Err(AdjustmentEvaluationPackageRowError::Identity);
    }
    Ok(value)
}

/// compare one text field with an exact ASCII value
fn text_equals(entries: &[(Vec<u16>, JsonValue)], key: &str, expected: &str) -> bool {
    string_field(entries, key).is_some_and(|value| match_utf16(value, expected))
}

/// require one exact object-key population
fn exact_keys(entries: &[(Vec<u16>, JsonValue)], expected: &[&str]) -> bool {
    entries.len() == expected.len()
        && expected
            .iter()
            .all(|key| entries.iter().any(|(name, _)| match_utf16(name, key)))
}

/// validate one nullable timestamp
fn nullable_timestamp_valid(
    entries: &[(Vec<u16>, JsonValue)],
    key: &str,
    values: &mut impl AdjustmentEvaluationPackageRowValues,
) -> bool {
    // preserve null while parsing every supplied string
    match field(entries, key) {
        Some(JsonValue::Null) => true,
        Some(value) => values.js_timestamp_milliseconds(value).is_some(),
        None => false,
    }
}

/// validate one nullable lowercase SHA-256 spelling
fn nullable_hash_valid(entries: &[(Vec<u16>, JsonValue)], key: &str) -> bool {
    // preserve null while rejecting malformed content hashes
    match field(entries, key) {
        Some(JsonValue::Null) => true,
        Some(JsonValue::String(value)) => valid_hash_units(value),
        _ => false,
    }
}

/// recognize one lowercase SHA-256 spelling
fn valid_hash_units(value: &[u16]) -> bool {
    value.len() == 64
        && value.iter().all(|unit| {
            u8::try_from(*unit)
                .is_ok_and(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        })
}

/// recognize the sole retained body member grammar
fn valid_body_member_path(path: &str) -> bool {
    let Some(hash) = path
        .strip_prefix("members/bodies/sha256-")
        .and_then(|value| value.strip_suffix(".json.gz"))
    else {
        return false;
    };
    valid_hash_units(&hash.encode_utf16().collect::<Vec<_>>())
}

/// compare raw UTF-16 units with one ASCII spelling
fn match_utf16(value: &[u16], expected: &str) -> bool {
    value.len() == expected.len()
        && value
            .iter()
            .copied()
            .zip(expected.bytes().map(u16::from))
            .all(|(left, right)| left == right)
}

/// return the fixed V1 payload schema for one record kind
fn payload_keys(kind: AdjustmentEvaluationRecordKind) -> &'static [&'static str] {
    // retain the original key populations without validating their values here
    match kind {
        AdjustmentEvaluationRecordKind::TemperatureRun => &[
            "adapterVersion",
            "contentHash",
            "firstReceivedAt",
            "id",
            "lastReceivedAt",
            "modelCycle",
            "providerResponseSha256",
            "recentErrorState",
            "recentErrorStateSha256",
            "revisionCount",
            "runInitializedAt",
            "stateReason",
            "stateStatus",
            "upstreamModel",
        ],
        AdjustmentEvaluationRecordKind::TemperatureHour => &[
            "contentHash",
            "modelLeadHours",
            "rawRelativeHumidityPercent",
            "rawTemperatureC",
            "rawWindSpeedMps",
            "runId",
            "validAt",
        ],
        AdjustmentEvaluationRecordKind::RainClaim => &[
            "attempt",
            "claimedAt",
            "id",
            "kind",
            "policy",
            "policySha256",
            "release",
            "runInitializedAt",
            "slotKey",
            "stationId",
            "windowEndExclusive",
            "windowStart",
        ],
        AdjustmentEvaluationRecordKind::RainReceipt => &[
            "availableByDecision",
            "bodyBytes",
            "bodySha256",
            "claimId",
            "completedAt",
            "compressedBytes",
            "errorCode",
            "httpStatus",
            "metadata",
            "outcome",
            "parserVersion",
            "recordedAt",
            "rowCount",
            "startedAt",
        ],
        AdjustmentEvaluationRecordKind::RainAdjustmentRun => &[
            "decisionAt",
            "firstReceivedAt",
            "forecastClaimId",
            "generatedAt",
            "hours",
            "inputSha256",
            "modelSha256",
            "runInitializedAt",
        ],
        AdjustmentEvaluationRecordKind::TargetRevisionDiagnostic => &[
            "adapterContract",
            "contributorContentHashesSha256",
            "contributorRevisionSha256",
            "firstReceivedAt",
            "maxRevisionCount",
            "maxSourceReceiptAt",
            "physicalStationKey",
            "providerKey",
            "recordCount",
            "revisedRecordCount",
            "sourceConfigFingerprint",
            "sourceId",
            "sourceKey",
            "sourceKeys",
            "validAt",
        ],
    }
}

#[cfg(test)]
#[path = "adjustment_evaluation_package_row_test.rs"]
mod tests;
