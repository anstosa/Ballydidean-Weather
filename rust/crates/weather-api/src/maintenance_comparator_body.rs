//! Prepare the private incumbent comparator without owning JSON, base64, or hashing.

use weather_contracts::{
    MaintenanceShadowComparatorAuthorityKind, MaintenanceShadowServingAuthority, Nullable,
    TemperatureMaintenanceIncumbentRow, WindMaintenanceIncumbentRow,
};
use weather_database::Sha256Digest;

use crate::maintenance_prediction_body::PreparedMaintenancePrediction;
use crate::maintenance_source_projection::{MaintenanceSourceValue, PreparedMaintenanceSource};

const COMPARATOR_VERSION: &str = "adjustment-shadow-incumbent-comparator/v1";
const MAXIMUM_COMPARATOR_BYTES: usize = 512 * 1_024;

/// One incumbent result family already evaluated against the prepared source.
#[derive(Clone, Copy)]
pub enum MaintenanceIncumbentRows<'a> {
    Wind(&'a [WindMaintenanceIncumbentRow]),
    Temperature(&'a [TemperatureMaintenanceIncumbentRow]),
}

/// One scalar in the closed canonical comparator grammar.
#[derive(Clone, Debug, PartialEq)]
pub enum MaintenanceComparatorValue {
    Text(String),
    Number(u64),
    Boolean(bool),
    Null,
}

/// One exact ordered comparator field.
pub type MaintenanceComparatorField = (&'static str, MaintenanceComparatorValue);

/// Family-closed comparator rows in the original TypeScript key order.
#[derive(Debug, PartialEq)]
pub enum MaintenanceComparatorRows {
    Wind(Vec<[MaintenanceComparatorField; 8]>),
    Temperature(Vec<[MaintenanceComparatorField; 6]>),
}

/// Complete comparator material passed only to the approved canonical encoder.
pub struct MaintenanceComparatorDocument<'a> {
    pub contract_version: &'static str,
    pub family: &'static str,
    pub registration_sha256: &'a str,
    pub candidate_sha256: &'a str,
    pub source_sha256: &'a str,
    pub due_key: &'a str,
    pub issued_at: &'a str,
    pub source_projection_sha256: Sha256Digest,
    pub prediction_body_sha256: &'a str,
    pub serving_authority: &'a MaintenanceShadowServingAuthority,
    pub row_count: usize,
    pub rows: MaintenanceComparatorRows,
}

/// Approved exact-byte, authority, and canonical JSON boundary.
pub trait MaintenanceComparatorCanonicalCodec {
    /// Digest one exact byte member without normalizing it.
    fn sha256(&mut self, bytes: &[u8]) -> Option<Sha256Digest>;

    /// Validate embedded base64, receipt, artifact, and family-specific authority material.
    fn validate_authority(
        &mut self,
        authority: &MaintenanceShadowServingAuthority,
        family: &str,
    ) -> bool;

    /// Encode top-level, authority, and row keys in their original reviewed order with one LF.
    fn encode_comparator(
        &mut self,
        document: &MaintenanceComparatorDocument<'_>,
    ) -> Option<Vec<u8>>;
}

/// Bounded comparator preparation failures without embedded private material.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MaintenanceComparatorError {
    Family,
    Binding,
    Authority,
    Row,
    Canonical,
}

/// Bind one incumbent vector to exact source and candidate evidence before serialization.
pub fn prepare_maintenance_comparator(
    source: &PreparedMaintenanceSource<'_>,
    prediction: &PreparedMaintenancePrediction,
    authority: Option<&MaintenanceShadowServingAuthority>,
    rows: MaintenanceIncumbentRows<'_>,
    codec: &mut impl MaintenanceComparatorCanonicalCodec,
) -> Result<Vec<u8>, MaintenanceComparatorError> {
    let authority = authority.ok_or(MaintenanceComparatorError::Authority)?;
    let family = rows.family();
    // prevent a result vector from crossing the source family boundary
    if source.projection.family() != family {
        return Err(MaintenanceComparatorError::Family);
    }
    validate_prediction_binding(source, prediction, family, codec)?;
    validate_authority_shape(authority, family, codec)?;
    let projected_rows = project_rows(source, rows)?;
    let header = source.projection.header();
    let source_projection_sha256 = codec
        .sha256(&source.bytes)
        .filter(|digest| valid_sha256(digest.as_str()))
        .ok_or(MaintenanceComparatorError::Canonical)?;
    let document = MaintenanceComparatorDocument {
        contract_version: COMPARATOR_VERSION,
        family,
        registration_sha256: header.registration_sha256,
        candidate_sha256: header.candidate_sha256,
        source_sha256: header.source_sha256,
        due_key: header.due_key,
        issued_at: &header.issued_at.utc,
        source_projection_sha256,
        prediction_body_sha256: &prediction.metadata.predictionBodySha256,
        serving_authority: authority,
        row_count: source.projection.row_count(),
        rows: projected_rows,
    };
    let encoded = codec
        .encode_comparator(&document)
        .ok_or(MaintenanceComparatorError::Canonical)?;
    // retain the exact Node member cap and one canonical trailing newline
    if encoded.len() > MAXIMUM_COMPARATOR_BYTES || !encoded.ends_with(b"}\n") {
        return Err(MaintenanceComparatorError::Canonical);
    }
    Ok(encoded)
}

impl MaintenanceIncumbentRows<'_> {
    /// Report the exact family owned by one incumbent vector.
    const fn family(self) -> &'static str {
        match self {
            Self::Wind(_) => "wind",
            Self::Temperature(_) => "temperature",
        }
    }
}

/// Bind the prepared prediction metadata to the exact source and body bytes.
fn validate_prediction_binding(
    source: &PreparedMaintenanceSource<'_>,
    prediction: &PreparedMaintenancePrediction,
    family: &str,
    codec: &mut impl MaintenanceComparatorCanonicalCodec,
) -> Result<(), MaintenanceComparatorError> {
    let header = source.projection.header();
    let metadata = &prediction.metadata;
    let row_count = u64::try_from(source.projection.row_count())
        .map_err(|_| MaintenanceComparatorError::Binding)?;
    let body_bytes =
        u64::try_from(prediction.bytes.len()).map_err(|_| MaintenanceComparatorError::Binding)?;
    let first = source_valid_at(source, 0)?;
    let last = source_valid_at(
        source,
        source
            .projection
            .row_count()
            .checked_sub(1)
            .ok_or(MaintenanceComparatorError::Binding)?,
    )?;
    // reject any source/body substitution before preparing incumbent rows
    if family != source.projection.family()
        || metadata.registrationSha256 != header.registration_sha256
        || metadata.candidateSha256 != header.candidate_sha256
        || metadata.sourceSha256 != header.source_sha256
        || metadata.dueKey != header.due_key
        || metadata.issuedAt != header.issued_at.utc
        || metadata.inputSha256 != source.identity.inputSha256
        || metadata.sourceReceiptSha256 != source.identity.sourceReceiptSha256
        || metadata.rowCount != row_count
        || metadata.bodyByteCount != body_bytes
        || metadata.minValidAt != first
        || metadata.maxValidAt != last
        || source.identity.sourceRowSha256.len() != source.projection.row_count()
        || !valid_sha256(&metadata.predictionBodySha256)
    {
        return Err(MaintenanceComparatorError::Binding);
    }
    let digest = codec
        .sha256(&prediction.bytes)
        .ok_or(MaintenanceComparatorError::Canonical)?;
    // bind the comparator to the actual prepared prediction bytes
    if digest.as_str() != metadata.predictionBodySha256 {
        return Err(MaintenanceComparatorError::Binding);
    }
    Ok(())
}

/// Enforce structural authority generation before the codec opens embedded members.
fn validate_authority_shape(
    authority: &MaintenanceShadowServingAuthority,
    family: &str,
    codec: &mut impl MaintenanceComparatorCanonicalCodec,
) -> Result<(), MaintenanceComparatorError> {
    let artifact_absent = matches!(&authority.artifactBase64, Nullable::Null)
        && matches!(&authority.artifactIdentitySha256, Nullable::Null)
        && matches!(&authority.artifactMemberSha256, Nullable::Null);
    let artifact_present = nullable_nonempty(&authority.artifactBase64)
        && nullable_hash(&authority.artifactIdentitySha256)
        && nullable_hash(&authority.artifactMemberSha256);
    let artifact_shape_matches = match authority.authorityKind {
        MaintenanceShadowComparatorAuthorityKind::PolicyRaw => artifact_absent,
        MaintenanceShadowComparatorAuthorityKind::LegacyActive
        | MaintenanceShadowComparatorAuthorityKind::MaintenanceQualified => artifact_present,
    };
    // policy-raw still requires one real admitted registry receipt
    if !artifact_shape_matches
        || authority.receiptBase64.is_empty()
        || !valid_sha256(&authority.receiptMemberSha256)
        || !codec.validate_authority(authority, family)
    {
        return Err(MaintenanceComparatorError::Authority);
    }
    Ok(())
}

/// Project the incumbent vector into one family-closed canonical row schema.
fn project_rows(
    source: &PreparedMaintenanceSource<'_>,
    rows: MaintenanceIncumbentRows<'_>,
) -> Result<MaintenanceComparatorRows, MaintenanceComparatorError> {
    match rows {
        MaintenanceIncumbentRows::Wind(rows) => {
            // require one serving result for every source row
            if rows.len() != source.projection.row_count() {
                return Err(MaintenanceComparatorError::Row);
            }
            rows.iter()
                .enumerate()
                .map(|(index, row)| project_wind_row(source, index, row))
                .collect::<Result<Vec<_>, _>>()
                .map(MaintenanceComparatorRows::Wind)
        }
        MaintenanceIncumbentRows::Temperature(rows) => {
            // require one serving result for every source row
            if rows.len() != source.projection.row_count() {
                return Err(MaintenanceComparatorError::Row);
            }
            rows.iter()
                .enumerate()
                .map(|(index, row)| project_temperature_row(source, index, row))
                .collect::<Result<Vec<_>, _>>()
                .map(MaintenanceComparatorRows::Temperature)
        }
    }
}

/// Project one wind incumbent row in the predecessor's exact field order.
fn project_wind_row(
    source: &PreparedMaintenanceSource<'_>,
    index: usize,
    row: &WindMaintenanceIncumbentRow,
) -> Result<[MaintenanceComparatorField; 8], MaintenanceComparatorError> {
    let valid_at = source_valid_at(source, index)?;
    // reject reordering independently from numerical output
    if row.validAt != valid_at || !valid_wind_reason(&row.reasonCode) {
        return Err(MaintenanceComparatorError::Row);
    }
    let lead = u64::try_from(index + 1).map_err(|_| MaintenanceComparatorError::Row)?;
    Ok([
        (
            "validAt",
            MaintenanceComparatorValue::Text(valid_at.to_owned()),
        ),
        ("leadHours", MaintenanceComparatorValue::Number(lead)),
        (
            "sourceRowSha256",
            MaintenanceComparatorValue::Text(source_row_sha256(source, index)?.to_owned()),
        ),
        (
            "incumbentSpeedMps64",
            MaintenanceComparatorValue::Text(binary64(row.incumbentSpeedMps)?),
        ),
        (
            "incumbentGustMps64",
            nullable_binary64(&row.incumbentGustMps)?,
        ),
        (
            "speedApplied",
            MaintenanceComparatorValue::Boolean(row.speedApplied),
        ),
        (
            "gustApplied",
            MaintenanceComparatorValue::Boolean(row.gustApplied),
        ),
        ("reasonCode", nullable_text(&row.reasonCode)),
    ])
}

/// Project one temperature incumbent row in the predecessor's exact field order.
fn project_temperature_row(
    source: &PreparedMaintenanceSource<'_>,
    index: usize,
    row: &TemperatureMaintenanceIncumbentRow,
) -> Result<[MaintenanceComparatorField; 6], MaintenanceComparatorError> {
    let valid_at = source_valid_at(source, index)?;
    // reject reordering independently from numerical output
    if row.validAt != valid_at || !valid_temperature_reason(&row.reasonCode) {
        return Err(MaintenanceComparatorError::Row);
    }
    let lead = u64::try_from(index + 1).map_err(|_| MaintenanceComparatorError::Row)?;
    Ok([
        (
            "validAt",
            MaintenanceComparatorValue::Text(valid_at.to_owned()),
        ),
        ("leadHours", MaintenanceComparatorValue::Number(lead)),
        (
            "sourceRowSha256",
            MaintenanceComparatorValue::Text(source_row_sha256(source, index)?.to_owned()),
        ),
        (
            "incumbentTemperatureC64",
            MaintenanceComparatorValue::Text(binary64(row.incumbentTemperatureC)?),
        ),
        ("applied", MaintenanceComparatorValue::Boolean(row.applied)),
        ("reasonCode", nullable_text(&row.reasonCode)),
    ])
}

/// Borrow one already checked valid clock from the prepared source row.
fn source_valid_at<'a>(
    source: &'a PreparedMaintenanceSource<'_>,
    index: usize,
) -> Result<&'a str, MaintenanceComparatorError> {
    match source.projection.row_fields(index).and_then(<[_]>::first) {
        Some(("validAt", MaintenanceSourceValue::Text(value))) => Ok(value),
        _ => Err(MaintenanceComparatorError::Binding),
    }
}

/// Borrow one exact prepared source-row identity.
fn source_row_sha256<'a>(
    source: &'a PreparedMaintenanceSource<'_>,
    index: usize,
) -> Result<&'a str, MaintenanceComparatorError> {
    source
        .identity
        .sourceRowSha256
        .get(index)
        .filter(|value| valid_sha256(value))
        .map(String::as_str)
        .ok_or(MaintenanceComparatorError::Binding)
}

/// Encode one finite number as exact network-order binary64 hexadecimal.
fn binary64(value: f64) -> Result<String, MaintenanceComparatorError> {
    // reject values that JavaScript evidence encoding cannot retain
    if !value.is_finite() {
        return Err(MaintenanceComparatorError::Row);
    }
    Ok(format!("{:016x}", value.to_bits()))
}

/// Preserve a nullable binary64 without inventing a numeric sentinel.
fn nullable_binary64(
    value: &Nullable<f64>,
) -> Result<MaintenanceComparatorValue, MaintenanceComparatorError> {
    match value {
        Nullable::Null => Ok(MaintenanceComparatorValue::Null),
        Nullable::Value(value) => binary64(*value).map(MaintenanceComparatorValue::Text),
    }
}

/// Preserve one nullable diagnostic reason exactly.
fn nullable_text(value: &Nullable<String>) -> MaintenanceComparatorValue {
    match value {
        Nullable::Null => MaintenanceComparatorValue::Null,
        Nullable::Value(value) => MaintenanceComparatorValue::Text(value.clone()),
    }
}

/// Admit the exact Node temperature comparator reason vocabulary.
fn valid_temperature_reason(value: &Nullable<String>) -> bool {
    nullable_reason(
        value,
        &[
            "bundle_invalid",
            "bundle_missing",
            "canary_expired",
            "canary_killed",
            "registry_inactive",
            "registry_invalid",
            "policy_raw",
            "unsupported",
            "missing_source_forecast",
            "outside_operational_window",
            "source_identity_mismatch",
            "source_not_available",
            "source_stale",
            "source_time_mismatch",
        ],
    )
}

/// Admit the exact Node wind comparator reason vocabulary.
fn valid_wind_reason(value: &Nullable<String>) -> bool {
    nullable_reason(
        value,
        &[
            "adjustment_error",
            "bundle_invalid",
            "bundle_missing",
            "canary_expired",
            "canary_killed",
            "coefficient_missing",
            "direction_calm",
            "identity_mismatch",
            "metric_not_enabled",
            "metric_out_of_bounds",
            "policy_raw",
            "registry_inactive",
            "registry_invalid",
            "runtime_fingerprint_mismatch",
            "unsupported_lead",
            "wrong_cohort",
        ],
    )
}

/// Match null or one exact bounded reason without normalization.
fn nullable_reason(value: &Nullable<String>, admitted: &[&str]) -> bool {
    match value {
        Nullable::Null => true,
        Nullable::Value(value) => admitted.contains(&value.as_str()),
    }
}

/// Require one nonempty optional base64 member before deep codec admission.
fn nullable_nonempty(value: &Nullable<String>) -> bool {
    matches!(value, Nullable::Value(value) if !value.is_empty())
}

/// Require one optional member to carry a lowercase SHA-256 identity.
fn nullable_hash(value: &Nullable<String>) -> bool {
    matches!(value, Nullable::Value(value) if valid_sha256(value))
}

/// Match one exact lowercase hexadecimal SHA-256 string.
fn valid_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|digit| digit.is_ascii_digit() || matches!(digit, b'a'..=b'f'))
}
