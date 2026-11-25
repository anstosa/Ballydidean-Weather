//! Project package-verifier-bound edge members into typed rain performance evidence.

use std::collections::BTreeMap;
use std::fmt::{self, Display};

use weather_contracts::{JsonValue, Nullable};
use weather_worker::{AdmittedRainEdgeEvidence, AdmittedRainEdgeSource};

use crate::{
    AdmittedRainPerformancePackageEdge, MaintenanceCanonicalJson,
    RainPerformancePackageEdgeEvidence,
    maintenance_canonical_json::{field, valid_hash},
};

const MAXIMUM_EDGE_OBJECT_BYTES: usize = 512 * 1024;

/// One raw edge member whose path and bytes were already bound by the package verifier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedRainPerformanceEdgeMember {
    pub path: String,
    pub bytes: Vec<u8>,
}

/// One generic joined edge row retained until its package interval is selected.
#[derive(Debug, Clone, PartialEq)]
pub struct JoinedRainPerformanceEdgeRow {
    pub bundle_identities: JsonValue,
    pub first_edge_committed_at: String,
    pub row: JsonValue,
    pub settings_sha256: String,
    pub source_receipt_at: String,
}

/// Approved JSON and bounded-gzip owners used after package verification.
pub trait RainPerformanceEdgeRowsValues: MaintenanceCanonicalJson {
    /// Decode one verifier-bound JSON document without changing its values.
    fn decode_json(&mut self, bytes: &[u8]) -> Option<JsonValue>;

    /// Inflate one verifier-bound edge object under the original output ceiling.
    fn gunzip_bounded(&mut self, compressed: &[u8], maximum_output_bytes: usize)
    -> Option<Vec<u8>>;
}

/// Closed failures from the post-verification edge row projection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RainPerformanceEdgeRowsError {
    Availability,
    AvailabilityIndex,
    CanonicalReceiptRow,
    EdgeObjectGzip,
    EdgeObjectJson,
    EdgeObjectPath,
    EdgeObjectUnavailable,
    EdgeReceiptJson,
    RainProjection,
}

impl Display for RainPerformanceEdgeRowsError {
    /// Render the predecessor's stable edge-row diagnostics.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        // retain a fixed diagnostic for every closed failure
        formatter.write_str(match self {
            Self::Availability => "edge receipt availability is invalid",
            Self::AvailabilityIndex => "edge receipt availability index is invalid",
            Self::CanonicalReceiptRow => "edge receipt row serialization failed",
            Self::EdgeObjectGzip => "edge object decompression failed",
            Self::EdgeObjectJson => "edge object must be an object",
            Self::EdgeObjectPath => "edge object pathname is invalid",
            Self::EdgeObjectUnavailable => "edge receipt object is unavailable",
            Self::EdgeReceiptJson => "edge receipt must be an object",
            Self::RainProjection => "edge rain row is invalid",
        })
    }
}

impl std::error::Error for RainPerformanceEdgeRowsError {}

/// Join verified receipts to ordered object rows and admit the rain scoring projection.
pub fn admit_verified_rain_performance_edge_rows(
    members: Vec<VerifiedRainPerformanceEdgeMember>,
    values: &mut impl RainPerformanceEdgeRowsValues,
) -> Result<Vec<AdmittedRainPerformancePackageEdge>, RainPerformanceEdgeRowsError> {
    let mut receipts = Vec::new();
    let mut objects = BTreeMap::<String, JsonValue>::new();

    // decode only the verifier-bound edge members
    for member in members {
        // retain receipt bytes as plain JSON
        if member.path.contains("/receipts/") {
            let receipt = values
                .decode_json(&member.bytes)
                .filter(is_object)
                .ok_or(RainPerformanceEdgeRowsError::EdgeReceiptJson)?;
            receipts.push(receipt);
        // retain bounded object bytes after delegated decompression
        } else if member.path.contains("/objects/") {
            let plaintext = values
                .gunzip_bounded(&member.bytes, MAXIMUM_EDGE_OBJECT_BYTES)
                .ok_or(RainPerformanceEdgeRowsError::EdgeObjectGzip)?;
            let object = values
                .decode_json(&plaintext)
                .filter(is_object)
                .ok_or(RainPerformanceEdgeRowsError::EdgeObjectJson)?;
            let object_sha256 = object_sha256_from_path(&member.path)
                .ok_or(RainPerformanceEdgeRowsError::EdgeObjectPath)?;
            objects.insert(object_sha256, object);
        }
    }

    let mut admitted = Vec::new();
    // join each exclusive receipt to its exact ordered object rows
    for receipt in receipts {
        admit_receipt_rows(&receipt, &objects, values, &mut admitted)?;
    }
    Ok(admitted)
}

/// Join one receipt's availability indexes to its immutable object rows.
fn admit_receipt_rows(
    receipt: &JsonValue,
    objects: &BTreeMap<String, JsonValue>,
    values: &mut impl RainPerformanceEdgeRowsValues,
    admitted: &mut Vec<AdmittedRainPerformancePackageEdge>,
) -> Result<(), RainPerformanceEdgeRowsError> {
    let edge_receipt_identity_sha256 = required_hash(receipt, "edgeReceiptIdentitySha256")?;
    let first_edge_committed_at = required_text(receipt, "firstEdgeCommittedAt")?;
    let object_sha256 = required_hash(receipt, "objectSha256")?;
    let object = objects
        .get(&object_sha256)
        .ok_or(RainPerformanceEdgeRowsError::EdgeObjectUnavailable)?;
    let rows =
        array_field(object, "rows").ok_or(RainPerformanceEdgeRowsError::EdgeObjectUnavailable)?;
    let availability = field(receipt, "availability")
        .filter(|value| is_object(value))
        .ok_or(RainPerformanceEdgeRowsError::Availability)?;
    let timestamps = array_field(availability, "timestamps")
        .ok_or(RainPerformanceEdgeRowsError::Availability)?;
    let timestamp_indexes = array_field(availability, "rowTimestampIndexes")
        .ok_or(RainPerformanceEdgeRowsError::Availability)?;

    // require one availability index for every ordered object row
    if timestamp_indexes.len() != rows.len() {
        return Err(RainPerformanceEdgeRowsError::Availability);
    }

    let bundle_identities = field(object, "bundleIdentities")
        .filter(|value| is_object(value))
        .ok_or(RainPerformanceEdgeRowsError::RainProjection)?;
    let settings_sha256 = required_hash(object, "settingsSha256")?;

    // retain one causal availability timestamp per ordered row
    for (row_index, row) in rows.iter().enumerate() {
        let timestamp_index = json_index(&timestamp_indexes[row_index])
            .ok_or(RainPerformanceEdgeRowsError::AvailabilityIndex)?;
        let source_receipt_at = timestamps
            .get(timestamp_index)
            .and_then(json_text)
            .ok_or(RainPerformanceEdgeRowsError::AvailabilityIndex)?;
        let row_index =
            u64::try_from(row_index).map_err(|_| RainPerformanceEdgeRowsError::RainProjection)?;
        admitted.push(join_verified_edge_row(
            bundle_identities,
            &edge_receipt_identity_sha256,
            &first_edge_committed_at,
            &object_sha256,
            row,
            row_index,
            &settings_sha256,
            &source_receipt_at,
            values,
        )?);
    }
    Ok(())
}

/// Retain one generic joined row and its canonical collision bytes before date filtering.
#[allow(clippy::too_many_arguments)]
fn join_verified_edge_row(
    bundle_identities: &JsonValue,
    edge_receipt_identity_sha256: &str,
    first_edge_committed_at: &str,
    object_sha256: &str,
    row: &JsonValue,
    row_index: u64,
    settings_sha256: &str,
    source_receipt_at: &str,
    values: &mut impl RainPerformanceEdgeRowsValues,
) -> Result<AdmittedRainPerformancePackageEdge, RainPerformanceEdgeRowsError> {
    let record = field(row, "record")
        .filter(|value| is_object(value))
        .ok_or(RainPerformanceEdgeRowsError::RainProjection)?;
    let record_valid_at = required_text(record, "validAt")?;
    let canonical_receipt_row_value = json_object(vec![
        ("bundleIdentities", bundle_identities.clone()),
        (
            "edgeReceiptIdentitySha256",
            json_string(edge_receipt_identity_sha256),
        ),
        ("firstEdgeCommittedAt", json_string(first_edge_committed_at)),
        ("objectSha256", json_string(object_sha256)),
        ("row", row.clone()),
        ("rowIndex", JsonValue::Number(row_index as f64)),
        ("settingsSha256", json_string(settings_sha256)),
        ("sourceReceiptAt", json_string(source_receipt_at)),
    ]);
    let canonical_receipt_row = values
        .encode_canonical_json(&canonical_receipt_row_value)
        .ok_or(RainPerformanceEdgeRowsError::CanonicalReceiptRow)?;
    let joined = JoinedRainPerformanceEdgeRow {
        bundle_identities: bundle_identities.clone(),
        first_edge_committed_at: first_edge_committed_at.to_owned(),
        row: row.clone(),
        settings_sha256: settings_sha256.to_owned(),
        source_receipt_at: source_receipt_at.to_owned(),
    };

    Ok(AdmittedRainPerformancePackageEdge {
        edge_receipt_identity_sha256: edge_receipt_identity_sha256.to_owned(),
        row_index,
        record_valid_at,
        canonical_receipt_row,
        evidence: RainPerformancePackageEdgeEvidence::Joined(Box::new(joined)),
    })
}

/// Project one interval-selected generic edge row into the exact rain evidence fields.
pub fn project_joined_rain_performance_edge_row(
    joined: &JoinedRainPerformanceEdgeRow,
) -> Result<AdmittedRainEdgeEvidence, RainPerformanceEdgeRowsError> {
    let record = field(&joined.row, "record")
        .filter(|value| is_object(value))
        .ok_or(RainPerformanceEdgeRowsError::RainProjection)?;
    let rain_adjustment = field(&joined.row, "rainAdjustment")
        .filter(|value| is_object(value))
        .ok_or(RainPerformanceEdgeRowsError::RainProjection)?;
    let rain_bundle = field(&joined.bundle_identities, "rain")
        .filter(|value| is_object(value))
        .ok_or(RainPerformanceEdgeRowsError::RainProjection)?;
    let state = required_text(rain_adjustment, "state")?;

    // reject values outside the verifier's closed rain decision states
    if !matches!(state.as_str(), "active" | "disabled" | "raw_fallback") {
        return Err(RainPerformanceEdgeRowsError::RainProjection);
    }

    let identity_context = json_object(vec![
        ("bundle", rain_bundle.clone()),
        ("record", record.clone()),
        ("settingsSha256", json_string(&joined.settings_sha256)),
    ]);
    let source_forecast = optional_source_forecast(rain_adjustment)?;
    Ok(AdmittedRainEdgeEvidence {
        active_bundle: nullable_hash(rain_bundle, "activeBundle")?,
        adjustment_bundle_sha256: nullable_hash(rain_adjustment, "bundleSha256")?,
        captured_applied: state == "active",
        corrected_precipitation_mm: nullable_finite(rain_adjustment, "correctedPrecipitationMm")?,
        first_edge_committed_at: joined.first_edge_committed_at.clone(),
        identity_context,
        provenance_complete: required_boolean(&joined.row, "provenanceComplete")?,
        raw_best_match_precipitation_mm: nullable_finite(
            rain_adjustment,
            "rawBestMatchPrecipitationMm",
        )?,
        reason_code: nullable_text(rain_adjustment, "reasonCode")?,
        source_receipt_at: joined.source_receipt_at.clone(),
        source_forecast,
    })
}

/// Admit an optional closed rain source forecast without conflating null with defaults.
fn optional_source_forecast(
    rain_adjustment: &JsonValue,
) -> Result<Option<AdmittedRainEdgeSource>, RainPerformanceEdgeRowsError> {
    let source = field(rain_adjustment, "sourceForecast")
        .ok_or(RainPerformanceEdgeRowsError::RainProjection)?;
    // preserve the verifier-permitted null source
    if matches!(source, JsonValue::Null) {
        return Ok(None);
    }
    // require the complete source projection otherwise
    if !is_object(source) {
        return Err(RainPerformanceEdgeRowsError::RainProjection);
    }
    let model_lead_hours = required_finite(source, "modelLeadHours")?;
    // retain the source contract's integral model lead
    if model_lead_hours.fract() != 0.0 {
        return Err(RainPerformanceEdgeRowsError::RainProjection);
    }
    Ok(Some(AdmittedRainEdgeSource {
        first_received_at: required_text(source, "firstReceivedAt")?,
        model_lead_hours,
        raw_precipitation_mm: required_finite(source, "rawPrecipitationMm")?,
        run_initialized_at: required_text(source, "runInitializedAt")?,
        valid_at: required_text(source, "validAt")?,
    }))
}

/// Extract the exact lowercase object digest from its verified path suffix.
fn object_sha256_from_path(path: &str) -> Option<String> {
    let file_name = path.rsplit('/').next()?;
    let digest = file_name
        .strip_prefix("sha256-")?
        .strip_suffix(".json.gz")?;
    valid_hash(digest).then(|| digest.to_owned())
}

/// Recognize the generated dependency-free JSON object representation.
fn is_object(value: &JsonValue) -> bool {
    matches!(value, JsonValue::Object(_))
}

/// Borrow one required array member.
fn array_field<'a>(value: &'a JsonValue, key: &str) -> Option<&'a [JsonValue]> {
    let JsonValue::Array(values) = field(value, key)? else {
        return None;
    };
    Some(values)
}

/// Decode one nonnegative integral JSON array index.
fn json_index(value: &JsonValue) -> Option<usize> {
    let JsonValue::Number(value) = value else {
        return None;
    };
    (*value >= 0.0 && value.is_finite() && value.fract() == 0.0 && *value <= usize::MAX as f64)
        .then_some(*value as usize)
}

/// Decode one required nonempty string member.
fn required_text(value: &JsonValue, key: &str) -> Result<String, RainPerformanceEdgeRowsError> {
    field(value, key)
        .and_then(json_text)
        .ok_or(RainPerformanceEdgeRowsError::RainProjection)
}

/// Decode one nonempty JSON string without replacing invalid UTF-16.
fn json_text(value: &JsonValue) -> Option<String> {
    let JsonValue::String(value) = value else {
        return None;
    };
    String::from_utf16(value)
        .ok()
        .filter(|value| !value.is_empty())
}

/// Decode one required lowercase SHA-256 member.
fn required_hash(value: &JsonValue, key: &str) -> Result<String, RainPerformanceEdgeRowsError> {
    let value = required_text(value, key)?;
    // retain only the verifier's exact digest spelling
    if !valid_hash(&value) {
        return Err(RainPerformanceEdgeRowsError::RainProjection);
    }
    Ok(value)
}

/// Decode one required finite number member.
fn required_finite(value: &JsonValue, key: &str) -> Result<f64, RainPerformanceEdgeRowsError> {
    let JsonValue::Number(value) =
        field(value, key).ok_or(RainPerformanceEdgeRowsError::RainProjection)?
    else {
        return Err(RainPerformanceEdgeRowsError::RainProjection);
    };
    // reject values JSON cannot faithfully carry
    if !value.is_finite() {
        return Err(RainPerformanceEdgeRowsError::RainProjection);
    }
    Ok(*value)
}

/// Decode one required boolean member.
fn required_boolean(value: &JsonValue, key: &str) -> Result<bool, RainPerformanceEdgeRowsError> {
    let JsonValue::Boolean(value) =
        field(value, key).ok_or(RainPerformanceEdgeRowsError::RainProjection)?
    else {
        return Err(RainPerformanceEdgeRowsError::RainProjection);
    };
    Ok(*value)
}

/// Decode one finite number or preserve the verifier-permitted null.
fn nullable_finite(
    value: &JsonValue,
    key: &str,
) -> Result<Nullable<f64>, RainPerformanceEdgeRowsError> {
    // preserve explicit semantic nulls
    match field(value, key).ok_or(RainPerformanceEdgeRowsError::RainProjection)? {
        JsonValue::Null => Ok(Nullable::Null),
        JsonValue::Number(value) if value.is_finite() => Ok(Nullable::Value(*value)),
        _ => Err(RainPerformanceEdgeRowsError::RainProjection),
    }
}

/// Decode one nonempty string or preserve the verifier-permitted null.
fn nullable_text(
    value: &JsonValue,
    key: &str,
) -> Result<Nullable<String>, RainPerformanceEdgeRowsError> {
    // preserve explicit semantic nulls
    match field(value, key).ok_or(RainPerformanceEdgeRowsError::RainProjection)? {
        JsonValue::Null => Ok(Nullable::Null),
        value => json_text(value)
            .map(Nullable::Value)
            .ok_or(RainPerformanceEdgeRowsError::RainProjection),
    }
}

/// Decode one nullable lowercase SHA-256 member.
fn nullable_hash(
    value: &JsonValue,
    key: &str,
) -> Result<Nullable<String>, RainPerformanceEdgeRowsError> {
    let value = nullable_text(value, key)?;
    // require exact hash spelling for every present identity
    if let Nullable::Value(hash) = &value
        && !valid_hash(hash)
    {
        return Err(RainPerformanceEdgeRowsError::RainProjection);
    }
    Ok(value)
}

/// Build one generated JSON string without delegating wire serialization.
fn json_string(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}

/// Build one generated JSON object with the declared semantic fields.
fn json_object(fields: Vec<(&str, JsonValue)>) -> JsonValue {
    JsonValue::Object(
        fields
            .into_iter()
            .map(|(name, value)| (name.encode_utf16().collect(), value))
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use weather_database::Sha256Digest;

    use super::*;

    const RECEIPT_BYTES: &[u8] = b"receipt-json";
    const COMPRESSED_OBJECT_BYTES: &[u8] = b"verified-gzip";
    const OBJECT_BYTES: &[u8] = b"object-json";

    #[derive(Default)]
    struct Values {
        receipt: Option<JsonValue>,
        object: Option<JsonValue>,
        encoded: Vec<JsonValue>,
        gunzip_limits: Vec<usize>,
    }

    impl MaintenanceCanonicalJson for Values {
        /// Leave canonical parsing outside this projector fixture.
        fn parse_canonical_object(&mut self, _: &[u8]) -> Option<JsonValue> {
            None
        }

        /// Record every complete joined edge row passed to the canonical owner.
        fn encode_canonical_json(&mut self, value: &JsonValue) -> Option<Vec<u8>> {
            self.encoded.push(value.clone());
            Some(vec![u8::try_from(self.encoded.len()).ok()?])
        }

        /// Leave hashing with the already completed package verifier.
        fn sha256(&mut self, _: &[u8]) -> Option<Sha256Digest> {
            None
        }
    }

    impl RainPerformanceEdgeRowsValues for Values {
        /// Return the verifier-bound fixture document selected by its opaque bytes.
        fn decode_json(&mut self, bytes: &[u8]) -> Option<JsonValue> {
            // preserve the fixture's plain receipt boundary
            if bytes == RECEIPT_BYTES {
                return self.receipt.clone();
            }
            // preserve the fixture's inflated object boundary
            if bytes == OBJECT_BYTES {
                return self.object.clone();
            }
            None
        }

        /// Record the bound and expose only the fixture's delegated object bytes.
        fn gunzip_bounded(
            &mut self,
            compressed: &[u8],
            maximum_output_bytes: usize,
        ) -> Option<Vec<u8>> {
            self.gunzip_limits.push(maximum_output_bytes);
            (compressed == COMPRESSED_OBJECT_BYTES).then(|| OBJECT_BYTES.to_vec())
        }
    }

    /// Build one repeated lowercase fixture digest.
    fn hash(character: char) -> String {
        std::iter::repeat_n(character, 64).collect()
    }

    /// Build one rain row with either a complete source or explicit semantic nulls.
    fn rain_row(valid_at: &str, with_source: bool) -> JsonValue {
        // select either one complete admitted source or the original semantic nulls
        let (source_forecast, bundle_sha256, corrected, state, record_id) = match with_source {
            true => (
                json_object(vec![
                    ("firstReceivedAt", json_string("2026-09-14T09:05:00.000Z")),
                    ("modelLeadHours", JsonValue::Number(9.0)),
                    ("rawPrecipitationMm", JsonValue::Number(1.25)),
                    ("runInitializedAt", json_string("2026-09-14T00:00:00.000Z")),
                    ("validAt", json_string(valid_at)),
                ]),
                json_string(&hash('d')),
                JsonValue::Number(1.0),
                "active",
                "first",
            ),
            false => (
                JsonValue::Null,
                JsonValue::Null,
                JsonValue::Null,
                "raw_fallback",
                "second",
            ),
        };
        json_object(vec![
            ("provenanceComplete", JsonValue::Boolean(with_source)),
            (
                "rainAdjustment",
                json_object(vec![
                    ("bundleSha256", bundle_sha256),
                    ("correctedPrecipitationMm", corrected),
                    ("rawBestMatchPrecipitationMm", JsonValue::Null),
                    ("reasonCode", JsonValue::Null),
                    ("sourceForecast", source_forecast),
                    ("state", json_string(state)),
                ]),
            ),
            (
                "record",
                json_object(vec![
                    ("id", json_string(record_id)),
                    ("revisionCount", JsonValue::Number(1.0)),
                    ("validAt", json_string(valid_at)),
                ]),
            ),
        ])
    }

    /// Build one verifier-compatible edge object for the supplied rows.
    fn edge_object(rows: Vec<JsonValue>) -> JsonValue {
        json_object(vec![
            (
                "bundleIdentities",
                json_object(vec![(
                    "rain",
                    json_object(vec![("activeBundle", JsonValue::Null)]),
                )]),
            ),
            ("rows", JsonValue::Array(rows)),
            ("settingsSha256", json_string(&hash('c'))),
        ])
    }

    /// Build one receipt with explicit timestamp dictionary and row indexes.
    fn edge_receipt(indexes: Vec<JsonValue>, timestamps: Vec<JsonValue>) -> JsonValue {
        json_object(vec![
            (
                "availability",
                json_object(vec![
                    ("rowTimestampIndexes", JsonValue::Array(indexes)),
                    ("timestamps", JsonValue::Array(timestamps)),
                ]),
            ),
            ("edgeReceiptIdentitySha256", json_string(&hash('b'))),
            (
                "firstEdgeCommittedAt",
                json_string("2026-09-14T10:00:00.000Z"),
            ),
            ("objectSha256", json_string(&hash('a'))),
        ])
    }

    /// Build the two verifier-bound edge members in their original receipt-first order.
    fn members(object_hash: &str) -> Vec<VerifiedRainPerformanceEdgeMember> {
        vec![
            VerifiedRainPerformanceEdgeMember {
                path: format!("members/edge/receipts/sha256-{}.json", hash('b')),
                bytes: RECEIPT_BYTES.to_vec(),
            },
            VerifiedRainPerformanceEdgeMember {
                path: format!("members/edge/objects/sha256-{object_hash}.json.gz"),
                bytes: COMPRESSED_OBJECT_BYTES.to_vec(),
            },
        ]
    }

    /// Recover the typed projection from either composition edge evidence state.
    fn projected_evidence(edge: &AdmittedRainPerformancePackageEdge) -> AdmittedRainEdgeEvidence {
        // keep the helper compatible with already projected composition fixtures
        match &edge.evidence {
            RainPerformancePackageEdgeEvidence::Projected(evidence) => evidence.as_ref().clone(),
            RainPerformancePackageEdgeEvidence::Joined(joined) => {
                project_joined_rain_performance_edge_row(joined)
                    .expect("selected fixture row should project")
            }
        }
    }

    /// Project ordered rows while preserving semantic nulls and all independent clocks.
    #[test]
    fn verified_edge_members_project_exact_rain_rows() {
        let first = rain_row("2026-09-14T17:00:00.000Z", true);
        let second = rain_row("2026-09-14T18:00:00.000Z", false);
        let bundle_identities = field(&edge_object(Vec::new()), "bundleIdentities")
            .expect("fixture bundle")
            .clone();
        let mut values = Values {
            receipt: Some(edge_receipt(
                vec![JsonValue::Number(1.0), JsonValue::Number(0.0)],
                vec![
                    json_string("2026-09-14T09:10:00.000Z"),
                    json_string("2026-09-14T09:20:00.000Z"),
                ],
            )),
            object: Some(edge_object(vec![first.clone(), second.clone()])),
            ..Values::default()
        };

        let admitted = admit_verified_rain_performance_edge_rows(members(&hash('a')), &mut values)
            .expect("verified rows should project");

        assert_eq!(values.gunzip_limits, vec![MAXIMUM_EDGE_OBJECT_BYTES]);
        assert_eq!(admitted.len(), 2);
        assert_eq!(admitted[0].row_index, 0);
        assert_eq!(admitted[0].canonical_receipt_row, vec![1]);
        assert_eq!(admitted[0].record_valid_at, "2026-09-14T17:00:00.000Z");
        let first_evidence = projected_evidence(&admitted[0]);
        let second_evidence = projected_evidence(&admitted[1]);
        assert_eq!(
            first_evidence.first_edge_committed_at,
            "2026-09-14T10:00:00.000Z"
        );
        assert_eq!(first_evidence.source_receipt_at, "2026-09-14T09:20:00.000Z");
        assert_eq!(
            first_evidence
                .source_forecast
                .as_ref()
                .expect("complete source")
                .first_received_at,
            "2026-09-14T09:05:00.000Z"
        );
        assert_eq!(first_evidence.active_bundle, Nullable::Null);
        assert_eq!(
            first_evidence.identity_context,
            json_object(vec![
                (
                    "bundle",
                    field(&bundle_identities, "rain")
                        .expect("fixture rain bundle")
                        .clone(),
                ),
                (
                    "record",
                    field(&first, "record").expect("fixture record").clone(),
                ),
                ("settingsSha256", json_string(&hash('c'))),
            ])
        );
        assert_eq!(admitted[1].canonical_receipt_row, vec![2]);
        assert_eq!(second_evidence.source_forecast, None);
        assert_eq!(second_evidence.adjustment_bundle_sha256, Nullable::Null);
        assert_eq!(second_evidence.corrected_precipitation_mm, Nullable::Null);
        assert_eq!(
            second_evidence.raw_best_match_precipitation_mm,
            Nullable::Null
        );
        assert_eq!(second_evidence.reason_code, Nullable::Null);
        assert_eq!(
            values.encoded[0],
            json_object(vec![
                ("bundleIdentities", bundle_identities),
                ("edgeReceiptIdentitySha256", json_string(&hash('b'))),
                (
                    "firstEdgeCommittedAt",
                    json_string("2026-09-14T10:00:00.000Z"),
                ),
                ("objectSha256", json_string(&hash('a'))),
                ("row", first),
                ("rowIndex", JsonValue::Number(0.0)),
                ("settingsSha256", json_string(&hash('c'))),
                ("sourceReceiptAt", json_string("2026-09-14T09:20:00.000Z"),),
            ])
        );
    }

    /// Refuse an object member whose digest suffix is not the original exact spelling.
    #[test]
    fn invalid_object_path_is_refused_after_bounded_decode() {
        let mut values = Values {
            object: Some(edge_object(Vec::new())),
            ..Values::default()
        };
        let error = admit_verified_rain_performance_edge_rows(
            vec![VerifiedRainPerformanceEdgeMember {
                path: "members/edge/objects/not-content-addressed.json.gz".to_owned(),
                bytes: COMPRESSED_OBJECT_BYTES.to_vec(),
            }],
            &mut values,
        )
        .expect_err("invalid suffix must fail");

        assert_eq!(error, RainPerformanceEdgeRowsError::EdgeObjectPath);
        assert_eq!(values.gunzip_limits, vec![MAXIMUM_EDGE_OBJECT_BYTES]);
    }

    /// Refuse misaligned and out-of-range receipt availability mappings distinctly.
    #[test]
    fn invalid_availability_mapping_is_refused() {
        let object = edge_object(vec![rain_row("2026-09-14T17:00:00.000Z", true)]);
        let mut misaligned = Values {
            receipt: Some(edge_receipt(Vec::new(), vec![json_string("clock")])),
            object: Some(object.clone()),
            ..Values::default()
        };
        let error = admit_verified_rain_performance_edge_rows(members(&hash('a')), &mut misaligned)
            .expect_err("misaligned mapping must fail");
        assert_eq!(error, RainPerformanceEdgeRowsError::Availability);

        let mut out_of_range = Values {
            receipt: Some(edge_receipt(
                vec![JsonValue::Number(1.0)],
                vec![json_string("clock")],
            )),
            object: Some(object),
            ..Values::default()
        };
        let error =
            admit_verified_rain_performance_edge_rows(members(&hash('a')), &mut out_of_range)
                .expect_err("out-of-range mapping must fail");
        assert_eq!(error, RainPerformanceEdgeRowsError::AvailabilityIndex);
    }

    /// Join a generic out-of-block row without inspecting rain-only fields prematurely.
    #[test]
    fn generic_row_without_rain_fields_is_joined_before_projection() {
        let generic_row = json_object(vec![(
            "record",
            json_object(vec![("validAt", json_string("2026-08-31T23:00:00.000Z"))]),
        )]);
        let mut values = Values {
            receipt: Some(edge_receipt(
                vec![JsonValue::Number(0.0)],
                vec![json_string("2026-08-31T22:00:00.000Z")],
            )),
            object: Some(edge_object(vec![generic_row])),
            ..Values::default()
        };

        let admitted = admit_verified_rain_performance_edge_rows(members(&hash('a')), &mut values)
            .expect("generic row should join before interval selection");
        let RainPerformancePackageEdgeEvidence::Joined(joined) = &admitted[0].evidence else {
            panic!("generic row must remain lazily joined");
        };

        assert_eq!(admitted[0].record_valid_at, "2026-08-31T23:00:00.000Z");
        assert_eq!(admitted[0].canonical_receipt_row, vec![1]);
        assert_eq!(
            project_joined_rain_performance_edge_row(joined),
            Err(RainPerformanceEdgeRowsError::RainProjection)
        );
    }
}
