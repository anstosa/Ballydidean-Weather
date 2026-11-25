//! Prepare and append immutable rain-capture receipt evidence through injected canonical owners.

use weather_contracts::{
    Nullable, RainCaptureOutcome, RainCaptureReceipt, RainCaptureReceiptMetadataValue,
    RainCaptureReceiptParserVersionLiteral,
};

use crate::{SerializedJson, Sha256Digest};

pub const RAIN_CAPTURE_RECEIPT_CLAIM_SQL: &str =
    "SELECT id, kind, claimed_at, run_initialized_at FROM rain_capture_claims WHERE id = $1";
pub const INSERT_RAIN_CAPTURE_RECEIPT_SQL: &str = r#"
    INSERT INTO rain_capture_receipts (
      claim_id, started_at, completed_at, http_status, outcome, error_code,
      compressed_body, body_sha256, body_bytes, compressed_bytes,
      parser_version, row_count, available_by_decision, metadata
    ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14::jsonb)
"#;

const DECISION_DELAY_MILLISECONDS: i64 = 8 * 3_600_000;
const MAXIMUM_BODY_BYTES: usize = 2_000_000;
const MAXIMUM_COMPRESSED_BODY_BYTES: usize = 2_100_000;
const MAXIMUM_METADATA_JSON_UNITS: usize = 4_096;
const MAXIMUM_ROW_COUNT: u64 = 100_000;
const MAXIMUM_RETRY_AFTER_SECONDS: f64 = 604_800.0;
const BLOCKED_METADATA_KEY_PARTS: &[&[u8]] = &[
    b"secret",
    b"token",
    b"authorization",
    b"apikey",
    b"password",
    b"url",
];
const BLOCKED_METADATA_STRING_PARTS: &[&[u8]] = &[b"http://", b"https://", b"api_key="];

/// claim family retained from the prior durable request row
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RainCaptureReceiptClaimKind {
    Forecast,
    Station,
}

/// prior claim identity required before receipt evidence may be appended
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RainCaptureClaimRow {
    pub id: String,
    pub kind: RainCaptureReceiptClaimKind,
    pub claimed_at_ms: i64,
    pub run_initialized_at_ms: Option<i64>,
}

/// exact prepared JSON plus its JavaScript string-length witness
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedRainCaptureMetadataJson {
    pub json: SerializedJson,
    pub js_string_units: usize,
}

/// receipt row prepared without changing caller evidence values
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedRainCaptureReceipt {
    pub available_by_decision: Option<bool>,
    pub body_bytes: Option<u64>,
    pub body_sha256: Option<Sha256Digest>,
    pub claim_id: String,
    pub completed_at: String,
    pub compressed_body: Option<Vec<u8>>,
    pub compressed_bytes: Option<u64>,
    pub error_code: Option<String>,
    pub http_status: Option<u16>,
    pub metadata_json: SerializedJson,
    pub outcome: RainCaptureOutcome,
    pub parser_version: &'static str,
    pub row_count: u64,
    pub started_at: String,
}

/// database reads and writes for the source's two-statement append path
pub trait RainCaptureReceiptDatabase {
    type Error;

    /// read the exact prior claim selected by its immutable identifier
    fn select_claim(
        &mut self,
        statement: &'static str,
        claim_id: &str,
    ) -> Result<Option<RainCaptureClaimRow>, Self::Error>;

    /// append the fully prepared immutable receipt row
    fn insert_receipt(
        &mut self,
        statement: &'static str,
        receipt: &PreparedRainCaptureReceipt,
    ) -> Result<(), Self::Error>;
}

/// approved time, hash, compression, and JavaScript JSON boundaries
pub trait RainCaptureReceiptCanonicalValues {
    type Error;

    /// parse one exact canonical UTC instant to epoch milliseconds
    fn utc_epoch_milliseconds(&mut self, value: &str) -> Result<i64, Self::Error>;

    /// digest the exact unmodified provider bytes
    fn sha256(&mut self, bytes: &[u8]) -> Result<Sha256Digest, Self::Error>;

    /// gzip the exact unmodified provider bytes at compression level nine
    fn gzip_level_9(&mut self, bytes: &[u8]) -> Result<Vec<u8>, Self::Error>;

    /// stringify the ordered generated metadata with JavaScript semantics
    fn metadata_json(
        &mut self,
        metadata: &[(Vec<u16>, RainCaptureReceiptMetadataValue)],
    ) -> Result<PreparedRainCaptureMetadataJson, Self::Error>;
}

/// source-compatible receipt admission failures before or between database statements
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RainCaptureReceiptAdmissionError {
    InvalidClaimId,
    InvalidTimeOrParser,
    BodyTooLarge,
    BodyHashMismatch,
    CompressedBodyTooLarge,
    InvalidReceiptFields,
    InvalidMetadata,
    MissingPriorClaim,
    AvailabilityMismatch,
}

/// typed owner failure retaining database and canonical preparation boundaries
#[derive(Debug, PartialEq, Eq)]
pub enum RainCaptureReceiptError<DatabaseError, CanonicalError> {
    Admission(RainCaptureReceiptAdmissionError),
    Database(DatabaseError),
    Canonical(CanonicalError),
}

/// validate, prepare, and append one immutable rain-capture receipt
pub fn append_rain_capture_receipt<Database, Values>(
    database: &mut Database,
    canonical_values: &mut Values,
    claim_id: &str,
    receipt: &RainCaptureReceipt,
) -> Result<(), RainCaptureReceiptError<Database::Error, Values::Error>>
where
    Database: RainCaptureReceiptDatabase,
    Values: RainCaptureReceiptCanonicalValues,
{
    // reject malformed claim identities before canonical work or database reads
    if !valid_claim_id(claim_id) {
        return Err(admission(RainCaptureReceiptAdmissionError::InvalidClaimId));
    }
    let started_at_ms = canonical_values
        .utc_epoch_milliseconds(&receipt.startedAt)
        .map_err(RainCaptureReceiptError::Canonical)?;
    let completed_at_ms = canonical_values
        .utc_epoch_milliseconds(&receipt.completedAt)
        .map_err(RainCaptureReceiptError::Canonical)?;
    // preserve chronological transport evidence and the fixed parser contract
    if completed_at_ms < started_at_ms {
        return Err(admission(
            RainCaptureReceiptAdmissionError::InvalidTimeOrParser,
        ));
    }

    let body = nullable_ref(&receipt.body);
    // keep raw provider bytes bounded before hash or gzip allocation
    if body.is_some_and(|body| body.len() > MAXIMUM_BODY_BYTES) {
        return Err(admission(RainCaptureReceiptAdmissionError::BodyTooLarge));
    }
    let digest = body
        .map(|body| {
            canonical_values
                .sha256(body)
                .map_err(RainCaptureReceiptError::Canonical)
        })
        .transpose()?;
    let expected_digest = nullable_ref(&receipt.bodySha256).map(String::as_str);
    // require the exact caller checksum even for a present empty body
    if digest.as_ref().map(Sha256Digest::as_str) != expected_digest
        || digest
            .as_ref()
            .is_some_and(|digest| !valid_sha256(digest.as_str()))
    {
        return Err(admission(
            RainCaptureReceiptAdmissionError::BodyHashMismatch,
        ));
    }
    let compressed_body = body
        .map(|body| {
            canonical_values
                .gzip_level_9(body)
                .map_err(RainCaptureReceiptError::Canonical)
        })
        .transpose()?;
    // keep compressed custody independently bounded from source bytes
    if compressed_body
        .as_ref()
        .is_some_and(|compressed| compressed.len() > MAXIMUM_COMPRESSED_BODY_BYTES)
    {
        return Err(admission(
            RainCaptureReceiptAdmissionError::CompressedBodyTooLarge,
        ));
    }
    validate_receipt_fields(receipt, body.is_some()).map_err(admission)?;
    let prepared_metadata = canonical_values
        .metadata_json(&receipt.metadata)
        .map_err(RainCaptureReceiptError::Canonical)?;
    // apply the JavaScript string-unit budget after exact metadata serialization
    if prepared_metadata.js_string_units > MAXIMUM_METADATA_JSON_UNITS {
        return Err(admission(RainCaptureReceiptAdmissionError::InvalidMetadata));
    }
    validate_metadata(&receipt.metadata, receipt.outcome).map_err(admission)?;
    // constrain diagnostic codes only after metadata admission, matching the source order
    if nullable_ref(&receipt.errorCode).is_some_and(|code| !valid_error_code(code)) {
        return Err(admission(
            RainCaptureReceiptAdmissionError::InvalidReceiptFields,
        ));
    }

    let claim = database
        .select_claim(RAIN_CAPTURE_RECEIPT_CLAIM_SQL, claim_id)
        .map_err(RainCaptureReceiptError::Database)?
        .ok_or_else(|| admission(RainCaptureReceiptAdmissionError::MissingPriorClaim))?;
    // require the durable claim to predate the provider attempt
    if started_at_ms < claim.claimed_at_ms {
        return Err(admission(
            RainCaptureReceiptAdmissionError::MissingPriorClaim,
        ));
    }
    let available_by_decision = match claim.kind {
        RainCaptureReceiptClaimKind::Forecast => {
            receipt.outcome == RainCaptureOutcome::Valid
                && claim
                    .run_initialized_at_ms
                    .is_some_and(|initialized_at_ms| {
                        initialized_at_ms
                            .checked_add(DECISION_DELAY_MILLISECONDS)
                            .is_some_and(|deadline_ms| completed_at_ms <= deadline_ms)
                    })
        }
        RainCaptureReceiptClaimKind::Station => false,
    };
    let available_by_decision = match claim.kind {
        RainCaptureReceiptClaimKind::Forecast => Some(available_by_decision),
        RainCaptureReceiptClaimKind::Station => None,
    };
    // prevent promotion of a late or invalid forecast response
    if nullable_copy(&receipt.availableByDecision) != available_by_decision {
        return Err(admission(
            RainCaptureReceiptAdmissionError::AvailabilityMismatch,
        ));
    }

    let prepared = PreparedRainCaptureReceipt {
        available_by_decision,
        body_bytes: body.map(|body| body.len() as u64),
        body_sha256: digest,
        claim_id: claim_id.to_owned(),
        completed_at: receipt.completedAt.clone(),
        compressed_bytes: compressed_body
            .as_ref()
            .map(|compressed| compressed.len() as u64),
        compressed_body,
        error_code: nullable_ref(&receipt.errorCode).cloned(),
        http_status: nullable_copy(&receipt.httpStatus).map(|status| status as u16),
        metadata_json: prepared_metadata.json,
        outcome: receipt.outcome,
        parser_version: RainCaptureReceiptParserVersionLiteral::WIRE_VALUE,
        row_count: receipt.rowCount,
        started_at: receipt.startedAt.clone(),
    };
    database
        .insert_receipt(INSERT_RAIN_CAPTURE_RECEIPT_SQL, &prepared)
        .map_err(RainCaptureReceiptError::Database)
}

/// validate exact receipt status, row-count, and diagnostic-code policies
fn validate_receipt_fields(
    receipt: &RainCaptureReceipt,
    has_body: bool,
) -> Result<(), RainCaptureReceiptAdmissionError> {
    let status = nullable_copy(&receipt.httpStatus);
    let error_code = nullable_ref(&receipt.errorCode);
    let status_valid = status.is_none_or(|status| (100..=599).contains(&status));
    let outcome_valid = match receipt.outcome {
        RainCaptureOutcome::Valid => status == Some(200) && has_body && error_code.is_none(),
        RainCaptureOutcome::RateLimited => status == Some(429),
        RainCaptureOutcome::Unauthorized => matches!(status, Some(401 | 403)),
        RainCaptureOutcome::TransportError => status.is_none() && error_code.is_some(),
        RainCaptureOutcome::Invalid => status.is_some() && !matches!(status, Some(401 | 403 | 429)),
    };
    // retain the Node row, status, and outcome invariants before metadata serialization
    if receipt.rowCount > MAXIMUM_ROW_COUNT || !status_valid || !outcome_valid {
        return Err(RainCaptureReceiptAdmissionError::InvalidReceiptFields);
    }
    Ok(())
}

/// validate scalar metadata keys, values, and retry-after coupling
fn validate_metadata(
    metadata: &[(Vec<u16>, RainCaptureReceiptMetadataValue)],
    outcome: RainCaptureOutcome,
) -> Result<(), RainCaptureReceiptAdmissionError> {
    // reject any key or scalar value outside the small credential-free envelope
    if metadata
        .iter()
        .any(|(key, value)| !valid_metadata_entry(key, value))
    {
        return Err(RainCaptureReceiptAdmissionError::InvalidMetadata);
    }
    let retry_seconds = metadata_value(metadata, "retryAfterSeconds");
    let manual_resume = metadata_value(metadata, "retryAfterRequiresManualResume");
    let retry_seconds_valid = match retry_seconds {
        None | Some(RainCaptureReceiptMetadataValue::Null) => true,
        Some(RainCaptureReceiptMetadataValue::Number(value)) => {
            value.fract() == 0.0 && (0.0..=MAXIMUM_RETRY_AFTER_SECONDS).contains(value)
        }
        _ => false,
    };
    let manual_resume_valid = matches!(
        manual_resume,
        None | Some(RainCaptureReceiptMetadataValue::Boolean(_))
    );
    let retry_fields_absent = retry_seconds.is_none() && manual_resume.is_none();
    // bind retry metadata exclusively to rate-limited outcomes
    if !retry_seconds_valid
        || !manual_resume_valid
        || (!retry_fields_absent && outcome != RainCaptureOutcome::RateLimited)
    {
        return Err(RainCaptureReceiptAdmissionError::InvalidMetadata);
    }
    Ok(())
}

/// validate one metadata key and scalar without decoding or reserializing it
fn valid_metadata_entry(key: &[u16], value: &RainCaptureReceiptMetadataValue) -> bool {
    let key_valid = (1..=80).contains(&key.len())
        && is_ascii_lowercase_u16(key[0])
        && key[1..].iter().all(|unit| is_ascii_alphanumeric_u16(*unit))
        && !BLOCKED_METADATA_KEY_PARTS
            .iter()
            .any(|needle| contains_ascii_case_insensitive(key, needle));
    // stop before inspecting a scalar whose key is not admitted
    if !key_valid {
        return false;
    }
    match value {
        RainCaptureReceiptMetadataValue::Null | RainCaptureReceiptMetadataValue::Boolean(_) => true,
        RainCaptureReceiptMetadataValue::Number(value) => value.is_finite(),
        RainCaptureReceiptMetadataValue::String(value) => {
            value.len() <= 256
                && !BLOCKED_METADATA_STRING_PARTS
                    .iter()
                    .any(|needle| contains_ascii_case_insensitive(value, needle))
        }
    }
}

/// find one exact ASCII metadata key in the generated ordered object
fn metadata_value<'a>(
    metadata: &'a [(Vec<u16>, RainCaptureReceiptMetadataValue)],
    key: &str,
) -> Option<&'a RainCaptureReceiptMetadataValue> {
    let key = key.encode_utf16().collect::<Vec<_>>();
    metadata
        .iter()
        .find_map(|(candidate, value)| (candidate == &key).then_some(value))
}

/// compare an ASCII policy token against UTF-16 without adding a string codec
fn contains_ascii_case_insensitive(value: &[u16], needle: &[u8]) -> bool {
    value.windows(needle.len()).any(|window| {
        window
            .iter()
            .zip(needle)
            .all(|(left, right)| lower_ascii(*left) == u16::from(*right))
    })
}

/// lowercase one ASCII UTF-16 code unit while leaving all others unchanged
fn lower_ascii(value: u16) -> u16 {
    if (u16::from(b'A')..=u16::from(b'Z')).contains(&value) {
        value + u16::from(b'a' - b'A')
    } else {
        value
    }
}

/// report whether one UTF-16 unit is an ASCII lowercase letter
fn is_ascii_lowercase_u16(value: u16) -> bool {
    (u16::from(b'a')..=u16::from(b'z')).contains(&value)
}

/// report whether one UTF-16 unit is an ASCII letter or digit
fn is_ascii_alphanumeric_u16(value: u16) -> bool {
    (u16::from(b'0')..=u16::from(b'9')).contains(&value)
        || (u16::from(b'A')..=u16::from(b'Z')).contains(&value)
        || is_ascii_lowercase_u16(value)
}

/// match Node's broad case-insensitive 36-character claim grammar
fn valid_claim_id(value: &str) -> bool {
    value.len() == 36
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() || byte == b'-')
}

/// require one exact lowercase hexadecimal SHA-256 representation
fn valid_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

/// require one bounded snake-case diagnostic code
fn valid_error_code(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 80
        && bytes[0].is_ascii_lowercase()
        && bytes[1..]
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_')
}

/// borrow one generated nullable value without adding a sentinel
fn nullable_ref<T>(value: &Nullable<T>) -> Option<&T> {
    match value {
        Nullable::Null => None,
        Nullable::Value(value) => Some(value),
    }
}

/// copy one generated nullable scalar without adding a sentinel
fn nullable_copy<T: Copy>(value: &Nullable<T>) -> Option<T> {
    match value {
        Nullable::Null => None,
        Nullable::Value(value) => Some(*value),
    }
}

/// construct one typed admission failure independent of owner error types
const fn admission<DatabaseError, CanonicalError>(
    error: RainCaptureReceiptAdmissionError,
) -> RainCaptureReceiptError<DatabaseError, CanonicalError> {
    RainCaptureReceiptError::Admission(error)
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use super::*;

    const CLAIM_ID: &str = "01234567-89ab-cdef-0123-456789abcdef";
    const BODY_SHA256: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    #[derive(Default)]
    struct CanonicalValues {
        gzip_inputs: Vec<Vec<u8>>,
        hash_inputs: Vec<Vec<u8>>,
        metadata_inputs: Vec<Vec<(Vec<u16>, RainCaptureReceiptMetadataValue)>>,
        times: VecDeque<i64>,
    }

    impl RainCaptureReceiptCanonicalValues for CanonicalValues {
        type Error = &'static str;

        /// return the fixture's exact admitted UTC epochs
        fn utc_epoch_milliseconds(&mut self, _value: &str) -> Result<i64, Self::Error> {
            self.times.pop_front().ok_or("missing fixture time")
        }

        /// retain the exact raw body passed to the digest owner
        fn sha256(&mut self, bytes: &[u8]) -> Result<Sha256Digest, Self::Error> {
            self.hash_inputs.push(bytes.to_vec());
            Ok(Sha256Digest::admitted(BODY_SHA256))
        }

        /// retain the exact raw body and return deterministic level-nine output
        fn gzip_level_9(&mut self, bytes: &[u8]) -> Result<Vec<u8>, Self::Error> {
            self.gzip_inputs.push(bytes.to_vec());
            Ok([&[9_u8][..], bytes].concat())
        }

        /// retain ordered metadata and return its admitted JavaScript JSON form
        fn metadata_json(
            &mut self,
            metadata: &[(Vec<u16>, RainCaptureReceiptMetadataValue)],
        ) -> Result<PreparedRainCaptureMetadataJson, Self::Error> {
            self.metadata_inputs.push(metadata.to_vec());
            Ok(PreparedRainCaptureMetadataJson {
                json: SerializedJson::admitted(r#"{"provider":"fixture"}"#),
                js_string_units: 22,
            })
        }
    }

    struct Database {
        claim: Option<RainCaptureClaimRow>,
        events: Vec<&'static str>,
        inserted: Option<PreparedRainCaptureReceipt>,
    }

    impl RainCaptureReceiptDatabase for Database {
        type Error = &'static str;

        /// return the exact prior claim after checking statement identity
        fn select_claim(
            &mut self,
            statement: &'static str,
            claim_id: &str,
        ) -> Result<Option<RainCaptureClaimRow>, Self::Error> {
            assert_eq!(statement, RAIN_CAPTURE_RECEIPT_CLAIM_SQL);
            assert_eq!(claim_id, CLAIM_ID);
            self.events.push("select");
            Ok(self.claim.clone())
        }

        /// retain the prepared immutable insert after checking statement identity
        fn insert_receipt(
            &mut self,
            statement: &'static str,
            receipt: &PreparedRainCaptureReceipt,
        ) -> Result<(), Self::Error> {
            assert_eq!(statement, INSERT_RAIN_CAPTURE_RECEIPT_SQL);
            self.events.push("insert");
            self.inserted = Some(receipt.clone());
            Ok(())
        }
    }

    /// preserve exact raw evidence and append one timely forecast receipt
    #[test]
    fn appends_unmodified_evidence_after_prior_claim() {
        let body = vec![0, 1, 2, 255];
        let receipt = valid_receipt(body.clone());
        let mut values = CanonicalValues {
            times: VecDeque::from([2_000, 3_000]),
            ..CanonicalValues::default()
        };
        let mut database = Database {
            claim: Some(RainCaptureClaimRow {
                id: CLAIM_ID.to_owned(),
                kind: RainCaptureReceiptClaimKind::Forecast,
                claimed_at_ms: 1_000,
                run_initialized_at_ms: Some(2_500),
            }),
            events: Vec::new(),
            inserted: None,
        };

        append_rain_capture_receipt(&mut database, &mut values, CLAIM_ID, &receipt)
            .expect("a timely exact receipt should append");

        assert_eq!(database.events, ["select", "insert"]);
        assert_eq!(values.hash_inputs, std::slice::from_ref(&body));
        assert_eq!(values.gzip_inputs, std::slice::from_ref(&body));
        assert_eq!(
            values.metadata_inputs,
            std::slice::from_ref(&receipt.metadata)
        );
        let inserted = database.inserted.expect("inserted receipt should exist");
        assert_eq!(inserted.started_at, receipt.startedAt);
        assert_eq!(inserted.completed_at, receipt.completedAt);
        assert_eq!(
            inserted.compressed_body,
            Some([&[9_u8][..], &body].concat())
        );
        assert_eq!(inserted.body_bytes, Some(4));
        assert_eq!(inserted.compressed_bytes, Some(5));
        assert_eq!(inserted.body_sha256.unwrap().as_str(), BODY_SHA256);
        assert_eq!(inserted.available_by_decision, Some(true));
        assert_eq!(inserted.http_status, Some(200));
        assert_eq!(inserted.outcome, RainCaptureOutcome::Valid);
        assert_eq!(inserted.metadata_json.as_str(), r#"{"provider":"fixture"}"#);
    }

    /// reject a caller availability promotion that differs from the prior claim
    #[test]
    fn rejects_late_forecast_availability_before_insert() {
        let receipt = valid_receipt(vec![1]);
        let mut values = CanonicalValues {
            times: VecDeque::from([2_000, 40_000_000]),
            ..CanonicalValues::default()
        };
        let mut database = Database {
            claim: Some(RainCaptureClaimRow {
                id: CLAIM_ID.to_owned(),
                kind: RainCaptureReceiptClaimKind::Forecast,
                claimed_at_ms: 1_000,
                run_initialized_at_ms: Some(2_500),
            }),
            events: Vec::new(),
            inserted: None,
        };

        let error = append_rain_capture_receipt(&mut database, &mut values, CLAIM_ID, &receipt)
            .expect_err("late evidence cannot retain a true availability flag");

        assert_eq!(
            error,
            RainCaptureReceiptError::Admission(
                RainCaptureReceiptAdmissionError::AvailabilityMismatch
            )
        );
        assert_eq!(database.events, ["select"]);
        assert!(database.inserted.is_none());
    }

    /// reject inconsistent outcome status before reading a claim
    #[test]
    fn rejects_invalid_status_policy_before_database_access() {
        let mut receipt = valid_receipt(vec![1]);
        receipt.httpStatus = Nullable::Value(201);
        let mut values = CanonicalValues {
            times: VecDeque::from([2_000, 3_000]),
            ..CanonicalValues::default()
        };
        let mut database = Database {
            claim: None,
            events: Vec::new(),
            inserted: None,
        };

        let error = append_rain_capture_receipt(&mut database, &mut values, CLAIM_ID, &receipt)
            .expect_err("a valid outcome requires HTTP 200");

        assert_eq!(
            error,
            RainCaptureReceiptError::Admission(
                RainCaptureReceiptAdmissionError::InvalidReceiptFields
            )
        );
        assert!(database.events.is_empty());
    }

    /// build one canonical valid forecast receipt
    fn valid_receipt(body: Vec<u8>) -> RainCaptureReceipt {
        RainCaptureReceipt {
            startedAt: "2026-11-23T00:00:02.000Z".to_owned(),
            completedAt: "2026-11-23T00:00:03.000Z".to_owned(),
            httpStatus: Nullable::Value(200),
            body: Nullable::Value(body),
            bodySha256: Nullable::Value(BODY_SHA256.to_owned()),
            outcome: RainCaptureOutcome::Valid,
            errorCode: Nullable::Null,
            parserVersion: RainCaptureReceiptParserVersionLiteral,
            rowCount: 1,
            availableByDecision: Nullable::Value(true),
            metadata: vec![(
                "provider".encode_utf16().collect(),
                RainCaptureReceiptMetadataValue::String("fixture".encode_utf16().collect()),
            )],
        }
    }
}
