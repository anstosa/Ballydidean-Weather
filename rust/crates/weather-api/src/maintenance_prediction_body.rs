//! Build the canonical public maintenance prediction and compact database metadata.

use weather_contracts::{
    MaintenanceShadowPredictionMetadata, TemperatureMaintenanceCandidateRow,
    TemperatureMaintenanceCandidateRowFallbackCodeEnum, WindMaintenanceCandidateRow,
};
use weather_database::Sha256Digest;

use crate::maintenance_source_projection::{
    MaintenanceSourceValue, PreparedMaintenanceSource, is_source_digest,
};

const PREDICTION_VALUES_VERSION: &str = "adjustment-shadow-prediction-values/v3";
const PREDICTION_IDENTITY_VERSION: &[u8] = b"adjustment-shadow-prediction/v3";
const TEMPERATURE_ROWS: usize = 12;
const WIND_ROWS: usize = 168;
const TEMPERATURE_BODY_BYTES: usize = 8_192;
const WIND_BODY_BYTES: usize = 65_536;
const TEMPERATURE_SCHEMA_SHA256: &str =
    "4255feacfd464adf2cbbf1139ecdf30d9d00b847775c556407367ad1449d9e63";
const WIND_SCHEMA_SHA256: &str = "3f573c3e49ed3b97636674b0630e49508cf80533ff8edc8f31f6f0411f70d902";

/// Candidate rows admitted for one exact source family.
pub enum MaintenanceCandidateRows<'a> {
    Wind(&'a [WindMaintenanceCandidateRow]),
    Temperature(&'a [TemperatureMaintenanceCandidateRow]),
}

/// Exact public family label supplied to the canonical codec.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MaintenancePredictionFamily {
    Temperature,
    Wind,
}

impl MaintenancePredictionFamily {
    /// Return the frozen public wire label.
    pub const fn as_str(self) -> &'static str {
        // keep the codec independent from generated enum formatting
        match self {
            Self::Temperature => "temperature",
            Self::Wind => "wind",
        }
    }
}

/// One scalar in an already ordered public prediction row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MaintenancePredictionValue {
    Text(String),
    Number(u64),
    Boolean(bool),
    Null,
}

/// One exact ordered field in a public prediction row.
pub type MaintenancePredictionField = (&'static str, MaintenancePredictionValue);

/// One closed family row supplied to the canonical codec.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MaintenancePredictionRow {
    Temperature([MaintenancePredictionField; 6]),
    Wind([MaintenancePredictionField; 7]),
}

/// Closed body header and rows in the original common-key order.
pub struct MaintenancePredictionBody<'a> {
    pub contract_version: &'static str,
    pub family: MaintenancePredictionFamily,
    pub registration_sha256: &'a str,
    pub candidate_sha256: &'a str,
    pub source_sha256: &'a str,
    pub due_key: &'a str,
    pub issued_at: &'a str,
    pub source_receipt_sha256: &'a str,
    pub input_sha256: &'a str,
    pub row_count: u64,
    pub rows: &'a [MaintenancePredictionRow],
}

/// Approved canonical JSON and SHA-256 owner for the public body.
pub trait MaintenancePredictionCanonicalCodec {
    /// Encode exact common-key and family-row order with one trailing LF.
    fn encode_prediction(&mut self, body: &MaintenancePredictionBody<'_>) -> Option<Vec<u8>>;

    /// Hash ordered byte parts without inserting separators.
    fn sha256(&mut self, parts: &[&[u8]]) -> Option<Sha256Digest>;
}

/// Canonical public bytes and their compact database identity.
pub struct PreparedMaintenancePrediction {
    pub bytes: Vec<u8>,
    pub metadata: MaintenanceShadowPredictionMetadata,
}

/// Bounded reasons a candidate cannot become durable prediction evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MaintenancePredictionError {
    Family,
    Rows,
    Geometry,
    Identity,
    Metric,
    Fallback,
    Canonical,
}

/// Bind candidate values to the prepared source and derive compact metadata.
pub fn prepare_maintenance_prediction(
    source: &PreparedMaintenanceSource<'_>,
    rows: MaintenanceCandidateRows<'_>,
    codec: &mut impl MaintenancePredictionCanonicalCodec,
) -> Result<PreparedMaintenancePrediction, MaintenancePredictionError> {
    let family = match (&rows, source.projection.family()) {
        (MaintenanceCandidateRows::Temperature(_), "temperature") => {
            MaintenancePredictionFamily::Temperature
        }
        (MaintenanceCandidateRows::Wind(_), "wind") => MaintenancePredictionFamily::Wind,
        _ => return Err(MaintenancePredictionError::Family),
    };
    let expected_rows = match family {
        MaintenancePredictionFamily::Temperature => TEMPERATURE_ROWS,
        MaintenancePredictionFamily::Wind => WIND_ROWS,
    };
    let candidate_count = match &rows {
        MaintenanceCandidateRows::Temperature(rows) => rows.len(),
        MaintenanceCandidateRows::Wind(rows) => rows.len(),
    };
    // require one candidate and one content identity for every causal source row
    if candidate_count != expected_rows
        || source.projection.row_count() != expected_rows
        || source.identity.sourceRowSha256.len() != expected_rows
    {
        return Err(MaintenancePredictionError::Rows);
    }
    validate_source_identity(source)?;

    let projected_rows = match rows {
        MaintenanceCandidateRows::Temperature(rows) => project_temperature_rows(source, rows)?,
        MaintenanceCandidateRows::Wind(rows) => project_wind_rows(source, rows)?,
    };
    let header = source.projection.header();
    let row_count =
        u64::try_from(projected_rows.len()).map_err(|_| MaintenancePredictionError::Canonical)?;
    let body = MaintenancePredictionBody {
        contract_version: PREDICTION_VALUES_VERSION,
        family,
        registration_sha256: header.registration_sha256,
        candidate_sha256: header.candidate_sha256,
        source_sha256: header.source_sha256,
        due_key: header.due_key,
        issued_at: &header.issued_at.utc,
        source_receipt_sha256: &source.identity.sourceReceiptSha256,
        input_sha256: &source.identity.inputSha256,
        row_count,
        rows: &projected_rows,
    };
    let bytes = codec
        .encode_prediction(&body)
        .ok_or(MaintenancePredictionError::Canonical)?;
    let byte_limit = match family {
        MaintenancePredictionFamily::Temperature => TEMPERATURE_BODY_BYTES,
        MaintenancePredictionFamily::Wind => WIND_BODY_BYTES,
    };
    // reject oversized or alternate non-LF body representations before hashing
    if bytes.len() > byte_limit || !bytes.ends_with(b"}\n") {
        return Err(MaintenancePredictionError::Canonical);
    }

    let prediction_body_sha256 = prediction_sha256(codec, &[&bytes])?;
    let prediction_schema_sha256 = match family {
        MaintenancePredictionFamily::Temperature => TEMPERATURE_SCHEMA_SHA256,
        MaintenancePredictionFamily::Wind => WIND_SCHEMA_SHA256,
    };
    let min_valid_at = source_valid_at(source, 0)?.to_owned();
    let max_valid_at = source_valid_at(source, expected_rows - 1)?.to_owned();
    let body_byte_count =
        u64::try_from(bytes.len()).map_err(|_| MaintenancePredictionError::Canonical)?;
    let row_count_text = row_count.to_string();
    let byte_count_text = body_byte_count.to_string();
    let prediction_sha256 = prediction_sha256(
        codec,
        &[
            PREDICTION_IDENTITY_VERSION,
            b"\n",
            header.registration_sha256.as_bytes(),
            b"\n",
            header.candidate_sha256.as_bytes(),
            b"\n",
            header.source_sha256.as_bytes(),
            b"\n",
            header.due_key.as_bytes(),
            b"\n",
            header.issued_at.utc.as_bytes(),
            b"\n",
            min_valid_at.as_bytes(),
            b"\n",
            max_valid_at.as_bytes(),
            b"\n",
            source.identity.sourceReceiptSha256.as_bytes(),
            b"\n",
            source.identity.inputSha256.as_bytes(),
            b"\n",
            prediction_body_sha256.as_bytes(),
            b"\n",
            prediction_schema_sha256.as_bytes(),
            b"\n",
            row_count_text.as_bytes(),
            b"\n",
            byte_count_text.as_bytes(),
        ],
    )?;
    Ok(PreparedMaintenancePrediction {
        bytes,
        metadata: MaintenanceShadowPredictionMetadata {
            bodyByteCount: body_byte_count,
            candidateSha256: header.candidate_sha256.to_owned(),
            dueKey: header.due_key.to_owned(),
            inputSha256: source.identity.inputSha256.clone(),
            issuedAt: header.issued_at.utc.clone(),
            maxValidAt: max_valid_at,
            minValidAt: min_valid_at,
            predictionBodySha256: prediction_body_sha256,
            predictionSchemaSha256: prediction_schema_sha256.to_owned(),
            predictionSha256: prediction_sha256,
            registrationSha256: header.registration_sha256.to_owned(),
            rowCount: row_count,
            sourceReceiptSha256: source.identity.sourceReceiptSha256.clone(),
            sourceSha256: header.source_sha256.to_owned(),
        },
    })
}

/// Project exact temperature rows with frozen fallback semantics.
fn project_temperature_rows(
    source: &PreparedMaintenanceSource<'_>,
    rows: &[TemperatureMaintenanceCandidateRow],
) -> Result<Vec<MaintenancePredictionRow>, MaintenancePredictionError> {
    let header = source.projection.header();
    let issued_at = header.issued_at.utc.as_str();
    let mut projected = Vec::with_capacity(rows.len());
    // bind each candidate to source geometry rather than trusting evaluator clocks
    for (index, candidate) in rows.iter().enumerate() {
        let valid_at = source_valid_at(source, index)?;
        // candidate substitution and nonprospective rows cannot become evidence
        if candidate.validAt != valid_at || valid_at <= issued_at {
            return Err(MaintenancePredictionError::Geometry);
        }
        let fallback_code = temperature_fallback_wire(candidate.fallbackCode);
        // application and fallback classification must describe the same decision
        if candidate.wouldApply
            != (candidate.fallbackCode == TemperatureMaintenanceCandidateRowFallbackCodeEnum::None)
        {
            return Err(MaintenancePredictionError::Fallback);
        }
        let candidate_temperature =
            prediction_binary64(candidate.candidateTemperatureC, -100.0, 70.0)?;
        projected.push(MaintenancePredictionRow::Temperature([
            (
                "validAt",
                MaintenancePredictionValue::Text(valid_at.to_owned()),
            ),
            (
                "leadHours",
                MaintenancePredictionValue::Number((index + 1) as u64),
            ),
            (
                "sourceRowSha256",
                MaintenancePredictionValue::Text(source.identity.sourceRowSha256[index].clone()),
            ),
            (
                "candidateTemperatureC64",
                MaintenancePredictionValue::Text(candidate_temperature),
            ),
            (
                "wouldApply",
                MaintenancePredictionValue::Boolean(candidate.wouldApply),
            ),
            (
                "fallbackCode",
                MaintenancePredictionValue::Text(fallback_code.to_owned()),
            ),
        ]));
    }
    Ok(projected)
}

/// Project exact wind rows including the disabled gust horizon.
fn project_wind_rows(
    source: &PreparedMaintenanceSource<'_>,
    rows: &[WindMaintenanceCandidateRow],
) -> Result<Vec<MaintenancePredictionRow>, MaintenancePredictionError> {
    let header = source.projection.header();
    let issued_at = header.issued_at.utc.as_str();
    let mut projected = Vec::with_capacity(rows.len());
    // bind each candidate to source geometry and metric caps
    for (index, candidate) in rows.iter().enumerate() {
        let valid_at = source_valid_at(source, index)?;
        // candidate substitution and nonprospective rows cannot become evidence
        if candidate.validAt != valid_at || valid_at <= issued_at {
            return Err(MaintenancePredictionError::Geometry);
        }
        let speed = prediction_binary64(candidate.candidateSpeedMps, 0.0, 150.0)?;
        let gust_suppressed = (48..72).contains(&index);
        let gust = match (&candidate.candidateGustMps, gust_suppressed) {
            (weather_contracts::Nullable::Null, true) if !candidate.gustWouldApply => {
                MaintenancePredictionValue::Null
            }
            (weather_contracts::Nullable::Value(value), false) => {
                MaintenancePredictionValue::Text(prediction_binary64(*value, 0.0, 150.0)?)
            }
            _ => return Err(MaintenancePredictionError::Metric),
        };
        projected.push(MaintenancePredictionRow::Wind([
            (
                "validAt",
                MaintenancePredictionValue::Text(valid_at.to_owned()),
            ),
            (
                "leadHours",
                MaintenancePredictionValue::Number((index + 1) as u64),
            ),
            (
                "sourceRowSha256",
                MaintenancePredictionValue::Text(source.identity.sourceRowSha256[index].clone()),
            ),
            (
                "candidateSpeedMps64",
                MaintenancePredictionValue::Text(speed),
            ),
            ("candidateGustMps64", gust),
            (
                "speedWouldApply",
                MaintenancePredictionValue::Boolean(candidate.speedWouldApply),
            ),
            (
                "gustWouldApply",
                MaintenancePredictionValue::Boolean(candidate.gustWouldApply),
            ),
        ]));
    }
    Ok(projected)
}

/// Verify all portable identities inherited from the prepared private source.
fn validate_source_identity(
    source: &PreparedMaintenanceSource<'_>,
) -> Result<(), MaintenancePredictionError> {
    let header = source.projection.header();
    // reject any identity that cannot cross the database boundary unchanged
    if ![
        header.registration_sha256,
        header.candidate_sha256,
        header.source_sha256,
        &source.identity.inputSha256,
        &source.identity.sourceReceiptSha256,
    ]
    .into_iter()
    .all(is_source_digest)
        || !source
            .identity
            .sourceRowSha256
            .iter()
            .all(|digest| is_source_digest(digest))
    {
        return Err(MaintenancePredictionError::Identity);
    }
    Ok(())
}

/// Borrow the already validated source clock from its exact first row field.
fn source_valid_at<'a>(
    source: &'a PreparedMaintenanceSource<'_>,
    index: usize,
) -> Result<&'a str, MaintenancePredictionError> {
    // only the frozen first-field text representation carries the source clock
    match source.projection.row_fields(index).and_then(<[_]>::first) {
        Some(("validAt", MaintenanceSourceValue::Text(value))) => Ok(value),
        _ => Err(MaintenancePredictionError::Geometry),
    }
}

/// Encode one finite, physically bounded value as canonical binary64 hex.
fn prediction_binary64(
    value: f64,
    minimum: f64,
    maximum: f64,
) -> Result<String, MaintenancePredictionError> {
    // nonfinite and out-of-domain values cannot become public evidence
    if !value.is_finite() || !(minimum..=maximum).contains(&value) {
        return Err(MaintenancePredictionError::Metric);
    }
    Ok(format!("{:016x}", value.to_bits()))
}

/// Resolve the generated temperature fallback label without string heuristics.
fn temperature_fallback_wire(
    code: TemperatureMaintenanceCandidateRowFallbackCodeEnum,
) -> &'static str {
    TemperatureMaintenanceCandidateRowFallbackCodeEnum::WIRE_VALUES
        .iter()
        .find_map(|(wire, value)| (*value == code).then_some(*wire))
        .expect("generated temperature fallback must have a wire value")
}

/// Hash one exact ordered preimage through the approved codec owner.
fn prediction_sha256(
    codec: &mut impl MaintenancePredictionCanonicalCodec,
    parts: &[&[u8]],
) -> Result<String, MaintenancePredictionError> {
    codec
        .sha256(parts)
        .filter(|digest| is_source_digest(digest.as_str()))
        .map(|digest| digest.as_str().to_owned())
        .ok_or(MaintenancePredictionError::Canonical)
}
