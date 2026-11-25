//! Project already validated shadow documents into their compact database metadata.

use weather_contracts::{JsonValue, MaintenanceShadowPredictionMetadata};
use weather_database::Sha256Digest;

use crate::api_maintenance_admission::ApiMaintenanceShadowEvidenceValues;
use crate::api_shadow_wire_validation::{rows, unsigned};
use crate::maintenance_canonical_json::{field, text};

const SOURCE_ROW_IDENTITY_VERSION: &str = "adjustment-shadow-source-row/v1";
const SOURCE_RECEIPT_VERSION: &str = "adjustment-shadow-source-receipt/v1";
const PREDICTION_IDENTITY_VERSION: &str = "adjustment-shadow-prediction/v3";
const RAIN_SCHEMA_SHA256: &str = "c7ae2f750f13970137b1b2d885f7cce81b8ecbc2720635c6716f005bc04a5e25";
const TEMPERATURE_SCHEMA_SHA256: &str =
    "4255feacfd464adf2cbbf1139ecdf30d9d00b847775c556407367ad1449d9e63";
const WIND_SCHEMA_SHA256: &str = "3f573c3e49ed3b97636674b0630e49508cf80533ff8edc8f31f6f0411f70d902";

/// Project one already canonical source/body pair and prove their complete identity binding.
pub(crate) fn shadow_prediction_metadata(
    values: &impl ApiMaintenanceShadowEvidenceValues,
    source: &JsonValue,
    body: &JsonValue,
    source_bytes: &[u8],
    body_bytes: &[u8],
) -> Option<MaintenanceShadowPredictionMetadata> {
    project_shadow_prediction_metadata(values, source, body, source_bytes, body_bytes, None)
}

/// Exact source addresses retained by one local producer without repeating row hashes.
pub(crate) struct PreparedShadowSourceIdentity<'a> {
    source: &'a JsonValue,
    bytes: &'a [u8],
    pub(crate) input_sha256: String,
    pub(crate) source_receipt_sha256: String,
    pub(crate) row_sha256: Vec<String>,
}

impl<'a> PreparedShadowSourceIdentity<'a> {
    /// Hash one canonical source and each row exactly once for local production.
    pub(crate) fn prepare(
        values: &impl ApiMaintenanceShadowEvidenceValues,
        source: &'a JsonValue,
        bytes: &'a [u8],
    ) -> Option<Self> {
        let input_sha256 = digest(values, bytes)?;
        let source_receipt_sha256 = source_receipt_address(values, source, &input_sha256)?;
        let family = text(source, "family")?;
        let mut preimage = Vec::new();
        let row_sha256 = rows(source, "rows")?
            .iter()
            .map(|row| {
                source_row_address(values, row, &family, &mut preimage)
                    .map(|hash| hash.as_str().to_owned())
            })
            .collect::<Option<_>>()?;
        Some(Self {
            source,
            bytes,
            input_sha256,
            source_receipt_sha256,
            row_sha256,
        })
    }

    /// Bind a completed prediction to the exact already-prepared source addresses.
    pub(crate) fn bind_prediction(
        &self,
        values: &impl ApiMaintenanceShadowEvidenceValues,
        body: &JsonValue,
        body_bytes: &[u8],
    ) -> Option<MaintenanceShadowPredictionMetadata> {
        project_shadow_prediction_metadata(
            values,
            self.source,
            body,
            self.bytes,
            body_bytes,
            Some(self),
        )
    }
}

/// Reuse local hashes when provided; incoming API requests prove all raw addresses here.
fn project_shadow_prediction_metadata(
    values: &impl ApiMaintenanceShadowEvidenceValues,
    source: &JsonValue,
    body: &JsonValue,
    source_bytes: &[u8],
    body_bytes: &[u8],
    prepared: Option<&PreparedShadowSourceIdentity<'_>>,
) -> Option<MaintenanceShadowPredictionMetadata> {
    let family = text(body, "family")?;
    let prediction_schema_sha256 = shadow_schema(&family)?;
    // bind both canonical documents to one closed family
    if text(source, "family")? != family {
        return None;
    }
    let source_rows = rows(source, "rows")?;
    let body_rows = rows(body, "rows")?;
    let source_row_count = unsigned(source, "rowCount")?;
    let body_row_count = unsigned(body, "rowCount")?;
    let source_rows_len = u64::try_from(source_rows.len()).ok()?;
    let body_rows_len = u64::try_from(body_rows.len()).ok()?;
    // retain the already validated geometry while rejecting truncated projections
    if source_row_count != source_rows_len
        || body_row_count != body_rows_len
        || source_row_count != body_row_count
        || source_rows.is_empty()
    {
        return None;
    }

    // bind all common source context before deriving value-free identities
    for key in [
        "registrationSha256",
        "candidateSha256",
        "sourceSha256",
        "dueKey",
        "issuedAt",
    ] {
        if field(source, key)? != field(body, key)? {
            return None;
        }
    }
    let registration_sha256 = text(body, "registrationSha256")?;
    let candidate_sha256 = text(body, "candidateSha256")?;
    let source_sha256 = text(body, "sourceSha256")?;
    let due_key = text(body, "dueKey")?;
    let issued_at = text(body, "issuedAt")?;
    let row_count_text = source_row_count.to_string();
    let (input_sha256, source_receipt_sha256) = match prepared {
        Some(identity) => (
            identity.input_sha256.clone(),
            identity.source_receipt_sha256.clone(),
        ),
        None => {
            let input = digest(values, source_bytes)?;
            let receipt = source_receipt_address(values, source, &input)?;
            (input, receipt)
        }
    };
    // require the public body to carry the identities of these exact source bytes
    if text(body, "inputSha256")? != input_sha256
        || text(body, "sourceReceiptSha256")? != source_receipt_sha256
    {
        return None;
    }

    let mut row_preimage = Vec::new();
    // bind every public lead and row digest to its corresponding private source row
    for (index, (source_row, body_row)) in source_rows.iter().zip(body_rows).enumerate() {
        if field(source_row, "validAt")? != field(body_row, "validAt")?
            || field(source_row, "leadHours")? != field(body_row, "leadHours")?
        {
            return None;
        }
        let matches = match prepared {
            Some(identity) => {
                text_equals(body_row, "sourceRowSha256", identity.row_sha256.get(index)?)?
            }
            None => text_equals(
                body_row,
                "sourceRowSha256",
                source_row_address(values, source_row, &family, &mut row_preimage)?.as_str(),
            )?,
        };
        // compare the digest in place for incoming relay bytes
        if !matches {
            return None;
        }
    }

    let min_valid_at = text(body_rows.first()?, "validAt")?;
    let max_valid_at = text(body_rows.last()?, "validAt")?;
    let prediction_body_sha256 = digest(values, body_bytes)?;
    let body_byte_count = u64::try_from(body_bytes.len()).ok()?;
    let body_byte_count_text = body_byte_count.to_string();
    let prediction_sha256 = digest_lines(
        values,
        &[
            PREDICTION_IDENTITY_VERSION,
            &registration_sha256,
            &candidate_sha256,
            &source_sha256,
            &due_key,
            &issued_at,
            &min_valid_at,
            &max_valid_at,
            &source_receipt_sha256,
            &input_sha256,
            &prediction_body_sha256,
            prediction_schema_sha256,
            &row_count_text,
            &body_byte_count_text,
        ],
    )?;
    Some(MaintenanceShadowPredictionMetadata {
        bodyByteCount: body_byte_count,
        candidateSha256: candidate_sha256,
        dueKey: due_key,
        inputSha256: input_sha256,
        issuedAt: issued_at,
        maxValidAt: max_valid_at,
        minValidAt: min_valid_at,
        predictionBodySha256: prediction_body_sha256,
        predictionSchemaSha256: prediction_schema_sha256.to_owned(),
        predictionSha256: prediction_sha256,
        registrationSha256: registration_sha256,
        rowCount: body_row_count,
        sourceReceiptSha256: source_receipt_sha256,
        sourceSha256: source_sha256,
    })
}

/// Build the source receipt from the original header and first/last target clocks.
fn source_receipt_address(
    values: &impl ApiMaintenanceShadowEvidenceValues,
    source: &JsonValue,
    input_sha256: &str,
) -> Option<String> {
    let rows = rows(source, "rows")?;
    digest_lines(
        values,
        &[
            SOURCE_RECEIPT_VERSION,
            &text(source, "registrationSha256")?,
            &text(source, "candidateSha256")?,
            &text(source, "sourceSha256")?,
            &text(source, "dueKey")?,
            &text(source, "issuedAt")?,
            &text(rows.first()?, "validAt")?,
            &text(rows.last()?, "validAt")?,
            &unsigned(source, "rowCount")?.to_string(),
            input_sha256,
        ],
    )
}

/// Reuse one buffer for the original family-delimited canonical source row domain.
fn source_row_address(
    values: &impl ApiMaintenanceShadowEvidenceValues,
    row: &JsonValue,
    family: &str,
    preimage: &mut Vec<u8>,
) -> Option<Sha256Digest> {
    let encoded = values.encode_shadow_ordered_json(row)?;
    preimage.clear();
    preimage.extend_from_slice(SOURCE_ROW_IDENTITY_VERSION.as_bytes());
    preimage.push(b'\n');
    preimage.extend_from_slice(family.as_bytes());
    preimage.push(b'\n');
    preimage.extend_from_slice(&encoded);
    preimage.push(b'\n');
    values.sha256(preimage)
}

/// Select one closed family schema digest from the original Node constants.
fn shadow_schema(family: &str) -> Option<&'static str> {
    match family {
        "rain" => Some(RAIN_SCHEMA_SHA256),
        "temperature" => Some(TEMPERATURE_SCHEMA_SHA256),
        "wind" => Some(WIND_SCHEMA_SHA256),
        _ => None,
    }
}

/// Compare one UTF-16 JSON string with an admitted UTF-8 identity without allocating.
fn text_equals(value: &JsonValue, key: &str, expected: &str) -> Option<bool> {
    let JsonValue::String(value) = field(value, key)? else {
        return None;
    };
    Some(value.iter().copied().eq(expected.encode_utf16()))
}

/// Hash one exact byte string with the admitted evidence owner.
fn digest(values: &impl ApiMaintenanceShadowEvidenceValues, bytes: &[u8]) -> Option<String> {
    values.sha256(bytes).map(|value| value.as_str().to_owned())
}

/// Hash one Node-style line-joined identity without an added trailing newline.
fn digest_lines(
    values: &impl ApiMaintenanceShadowEvidenceValues,
    parts: &[&str],
) -> Option<String> {
    let capacity =
        parts.iter().map(|part| part.len()).sum::<usize>() + parts.len().saturating_sub(1);
    let mut bytes = Vec::with_capacity(capacity);
    // preserve original field order and insert exactly one separator between values
    for (index, part) in parts.iter().enumerate() {
        if index > 0 {
            bytes.push(b'\n');
        }
        bytes.extend_from_slice(part.as_bytes());
    }
    digest(values, &bytes)
}
