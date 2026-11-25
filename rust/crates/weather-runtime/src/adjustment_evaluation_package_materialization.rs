//! materialize one strictly verified adjustment package for rain scoring.

use std::collections::BTreeMap;
use std::fmt::{self, Display};
use std::io::{Cursor, Read};

use weather_contracts::JsonValue;

use crate::{
    AdjustmentEvaluationPackageRowError, AdjustmentEvaluationPackageVerificationError,
    AdjustmentEvaluationPackageVerificationValues, AdjustmentEvaluationStreamError,
    AdmittedAdjustmentEvaluationPackageManifest, AdmittedRainArchiveBody,
    VerifiedRainPerformanceAdjustmentPackage, VerifiedRainPerformanceEdgeMember,
    admit_adjustment_evaluation_package_row, read_verified_adjustment_evaluation_member,
    validate_adjustment_evaluation_body, verify_adjustment_evaluation_package,
};

const ROW_MEMBER_PATH: &str = "members/rows.jsonl.gz";
const BODY_MEMBER_PREFIX: &str = "members/bodies/";
const EDGE_MEMBER_PREFIX: &str = "members/edge/";
const MAXIMUM_CANONICAL_BYTES: usize = 48 * 1024 * 1024;
const MAXIMUM_PACKAGE_MEMBER_BYTES: usize = 64 * 1024 * 1024;
const MAXIMUM_DECOMPRESSION_RATIO: usize = 64;

/// ordinary failures from post-verification package materialization.
#[derive(Debug, PartialEq, Eq)]
pub enum AdjustmentEvaluationPackageMaterializationError {
    Verification(AdjustmentEvaluationPackageVerificationError),
    Member(AdjustmentEvaluationStreamError),
    Row(AdjustmentEvaluationPackageRowError),
    MissingRowMember,
    RowDecompressionLimit,
    RowDecompressionRatio,
    RowCount,
    MissingBodyMember,
    EdgeCounts,
    IdentityChanged,
}

impl Display for AdjustmentEvaluationPackageMaterializationError {
    /// preserve the predecessor's stable package-loader diagnostics
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        // delegate verified phases and retain the materialization-specific text
        match self {
            Self::Verification(error) => Display::fmt(error, formatter),
            Self::Member(error) => Display::fmt(error, formatter),
            Self::Row(error) => Display::fmt(error, formatter),
            Self::MissingRowMember => formatter.write_str("package row member is missing"),
            Self::RowDecompressionLimit => {
                formatter.write_str("package row member exceeds the decompression limit")
            }
            Self::RowDecompressionRatio => {
                formatter.write_str("package row member exceeds the decompression ratio")
            }
            Self::RowCount => formatter.write_str("package row count differs"),
            Self::MissingBodyMember => {
                formatter.write_str("packaged export row body member is missing")
            }
            Self::EdgeCounts => formatter.write_str("edge evidence member counts differ"),
            Self::IdentityChanged => formatter.write_str("package identity changed during loading"),
        }
    }
}

impl std::error::Error for AdjustmentEvaluationPackageMaterializationError {}

/// strictly verify, materialize, and reverify one local adjustment package.
pub fn load_verified_adjustment_evaluation_package(
    root: &str,
    values: &mut impl AdjustmentEvaluationPackageVerificationValues,
) -> Result<VerifiedRainPerformanceAdjustmentPackage, AdjustmentEvaluationPackageMaterializationError>
{
    use AdjustmentEvaluationPackageMaterializationError as Error;

    let manifest =
        verify_adjustment_evaluation_package(root, values).map_err(Error::Verification)?;
    let manifest_sha256 = manifest.manifest_sha256.clone();
    let declared_member_count = manifest.members.len();
    let rows = materialize_rows(root, &manifest, values)?;
    let bodies = materialize_bodies(root, &manifest, values)?;
    let edge_members = materialize_edges(root, &manifest, values)?;

    // recheck every materialized row-to-body binding after all member reads
    verify_materialized_body_claims(&rows, &bodies, values)?;
    let final_manifest =
        verify_adjustment_evaluation_package(root, values).map_err(Error::Verification)?;

    // bind both strict snapshots and the declaration table cardinality
    if final_manifest.manifest_sha256 != manifest_sha256
        || final_manifest.members.len() != declared_member_count
    {
        return Err(Error::IdentityChanged);
    }

    Ok(VerifiedRainPerformanceAdjustmentPackage {
        bodies,
        edge_members,
        manifest: manifest.manifest,
        manifest_sha256,
        rows: rows
            .into_iter()
            .map(|row| materialized_row(row.original))
            .collect(),
    })
}

/// restore the raw row shape returned by the original packaged row reader
fn materialized_row(row: JsonValue) -> JsonValue {
    let JsonValue::Object(mut fields) = row else {
        unreachable!("admitted package rows are objects");
    };
    let body_index = fields
        .iter()
        .position(|(name, _)| name.iter().copied().eq("body_member_path".encode_utf16()))
        .expect("admitted package rows have an explicit body marker");
    let body = fields.remove(body_index);
    // restore the null raw marker before reappending the existing member path
    fields.push((
        "compressed_body_base64".encode_utf16().collect(),
        JsonValue::Null,
    ));
    fields.push(body);
    JsonValue::Object(fields)
}

/// rebind and materialize the canonical row population.
fn materialize_rows(
    root: &str,
    manifest: &AdmittedAdjustmentEvaluationPackageManifest,
    values: &mut impl AdjustmentEvaluationPackageVerificationValues,
) -> Result<
    Vec<crate::AdmittedAdjustmentEvaluationPackageRow>,
    AdjustmentEvaluationPackageMaterializationError,
> {
    use AdjustmentEvaluationPackageMaterializationError as Error;

    let member = manifest
        .member(ROW_MEMBER_PATH)
        .ok_or(Error::MissingRowMember)?;
    let compressed = read_verified_adjustment_evaluation_member(
        root,
        &member.path,
        member.size_bytes,
        &member.sha256,
        MAXIMUM_PACKAGE_MEMBER_BYTES,
        values,
    )
    .map_err(Error::Member)?;
    let mut inflated = values
        .gunzip_stream(Cursor::new(compressed.as_slice()))
        .map_err(AdjustmentEvaluationStreamError::from)
        .map_err(Error::Member)?;
    let mut bytes = Vec::new();
    (&mut inflated)
        .take((MAXIMUM_CANONICAL_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(AdjustmentEvaluationStreamError::from)
        .map_err(Error::Member)?;

    // enforce the fixed decoded output ceiling before applying string semantics
    if bytes.len() > MAXIMUM_CANONICAL_BYTES {
        return Err(Error::RowDecompressionLimit);
    }
    let decoded = String::from_utf8_lossy(&bytes);
    // match Buffer.byteLength after UTF-8 replacement and before trimEnd
    if decoded.len() > compressed.len().saturating_mul(MAXIMUM_DECOMPRESSION_RATIO) {
        return Err(Error::RowDecompressionRatio);
    }
    let mut rows = Vec::new();
    // preserve the predecessor's special empty-stream result
    if !decoded.is_empty() {
        let decoded = trim_end_ecmascript(&decoded);
        // materialize in the exact JSONL order after JavaScript trimEnd
        for line in decoded.split('\n') {
            rows.push(
                admit_adjustment_evaluation_package_row(line.as_bytes(), values)
                    .map_err(Error::Row)?,
            );
        }
    }
    // bind the materialized population to the strict manifest diagnostics
    if rows.len() as u64 != manifest.row_count {
        return Err(Error::RowCount);
    }
    Ok(rows)
}

/// materialize all compressed bodies in manifest declaration order.
fn materialize_bodies(
    root: &str,
    manifest: &AdmittedAdjustmentEvaluationPackageManifest,
    values: &mut impl AdjustmentEvaluationPackageVerificationValues,
) -> Result<Vec<AdmittedRainArchiveBody>, AdjustmentEvaluationPackageMaterializationError> {
    let mut bodies = Vec::new();
    // retain only the exact compressed bytes required by archive replay
    for member in manifest
        .members
        .iter()
        .filter(|member| member.path.starts_with(BODY_MEMBER_PREFIX))
    {
        let compressed = read_verified_adjustment_evaluation_member(
            root,
            &member.path,
            member.size_bytes,
            &member.sha256,
            MAXIMUM_PACKAGE_MEMBER_BYTES,
            values,
        )
        .map_err(AdjustmentEvaluationPackageMaterializationError::Member)?;
        bodies.push(AdmittedRainArchiveBody {
            member_path: member.path.clone(),
            compressed,
        });
    }
    Ok(bodies)
}

/// materialize all frozen edge evidence in manifest declaration order.
fn materialize_edges(
    root: &str,
    manifest: &AdmittedAdjustmentEvaluationPackageManifest,
    values: &mut impl AdjustmentEvaluationPackageVerificationValues,
) -> Result<Vec<VerifiedRainPerformanceEdgeMember>, AdjustmentEvaluationPackageMaterializationError>
{
    use AdjustmentEvaluationPackageMaterializationError as Error;

    let mut members = Vec::new();
    // retain raw verifier-bound bytes for the later rain projection
    for member in manifest
        .members
        .iter()
        .filter(|member| member.path.starts_with(EDGE_MEMBER_PREFIX))
    {
        let bytes = read_verified_adjustment_evaluation_member(
            root,
            &member.path,
            member.size_bytes,
            &member.sha256,
            MAXIMUM_PACKAGE_MEMBER_BYTES,
            values,
        )
        .map_err(Error::Member)?;
        members.push(VerifiedRainPerformanceEdgeMember {
            path: member.path.clone(),
            bytes,
        });
    }
    let object_count = members
        .iter()
        .filter(|member| member.path.contains("/objects/"))
        .count();
    let receipt_count = members
        .iter()
        .filter(|member| member.path.contains("/receipts/"))
        .count();

    // rebind materialized member groups to the exact manifest counters
    if object_count as f64 != manifest.edge_object_count
        || receipt_count as f64 != manifest.edge_receipt_count
    {
        return Err(Error::EdgeCounts);
    }
    Ok(members)
}

/// revalidate every materialized receipt-body relationship without retaining plaintext.
fn verify_materialized_body_claims(
    rows: &[crate::AdmittedAdjustmentEvaluationPackageRow],
    bodies: &[AdmittedRainArchiveBody],
    values: &mut impl AdjustmentEvaluationPackageVerificationValues,
) -> Result<(), AdjustmentEvaluationPackageMaterializationError> {
    use AdjustmentEvaluationPackageMaterializationError as Error;

    let body_indexes = bodies
        .iter()
        .enumerate()
        .map(|(index, body)| (body.member_path.as_str(), index))
        .collect::<BTreeMap<_, _>>();
    // preserve row order and recheck shared bodies for each referring row
    for claim in rows.iter().filter_map(|row| row.body.as_ref()) {
        let body = body_indexes
            .get(claim.member_path.as_str())
            .and_then(|index| bodies.get(*index))
            .ok_or(Error::MissingBodyMember)?;
        let inspection = values.begin_body_inspection();
        validate_adjustment_evaluation_body(
            Cursor::new(body.compressed.as_slice()),
            body.compressed.len() as u64,
            claim.plaintext_bytes.unwrap_or(f64::NAN),
            claim.body_sha256.as_deref(),
            values,
            inspection,
        )
        .map_err(Error::Member)?;
    }
    Ok(())
}

/// match ECMAScript TrimString's fixed whitespace and line-terminator set.
fn trim_end_ecmascript(value: &str) -> &str {
    value.trim_end_matches(|character| {
        matches!(
            character,
            '\u{0009}'
                | '\u{000A}'
                | '\u{000B}'
                | '\u{000C}'
                | '\u{000D}'
                | '\u{0020}'
                | '\u{00A0}'
                | '\u{1680}'
                | '\u{2000}'
                ..='\u{200A}'
                    | '\u{2028}'
                    | '\u{2029}'
                    | '\u{202F}'
                    | '\u{205F}'
                    | '\u{3000}'
                    | '\u{FEFF}'
        )
    })
}

#[cfg(test)]
mod tests {
    use super::{JsonValue, materialized_row, trim_end_ecmascript};

    /// preserve the full trailing ECMAScript whitespace set without trimming ordinary text
    #[test]
    fn materialization_trim_end_matches_ecmascript_whitespace() {
        let value = "row\u{2007}\u{2028}\u{FEFF}";
        assert_eq!(trim_end_ecmascript(value), "row");
        assert_eq!(trim_end_ecmascript("row\u{200B}"), "row\u{200B}");
    }

    /// preserve the nullable raw body value and reappend its verified path
    #[test]
    fn materialization_restores_the_original_raw_body_marker() {
        let name = |value: &str| value.encode_utf16().collect::<Vec<_>>();
        let body = JsonValue::String("members/bodies/example.json.gz".encode_utf16().collect());
        let kind = JsonValue::String("rain_receipt".encode_utf16().collect());
        let raw = JsonValue::Object(vec![
            (name("body_member_path"), body.clone()),
            (name("record_kind"), kind.clone()),
        ]);
        assert_eq!(
            materialized_row(raw),
            JsonValue::Object(vec![
                (name("record_kind"), kind),
                (name("compressed_body_base64"), JsonValue::Null),
                (name("body_member_path"), body),
            ]),
        );
    }
}
