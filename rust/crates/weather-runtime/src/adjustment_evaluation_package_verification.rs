//! Strictly verify one v1 adjustment package before materialization.

use std::collections::BTreeMap;
use std::fmt::{self, Display};
use std::io;

use weather_contracts::JsonValue;
use weather_database::Sha256Digest;

use crate::adjustment_evaluation_package_edge_verification::{
    AdjustmentEvaluationEdgeMember, AdjustmentEvaluationEdgeSummary,
    AdjustmentEvaluationEdgeVerificationError, AdjustmentEvaluationEdgeVerificationValues,
    verify_adjustment_evaluation_edge_bundle,
};
use crate::adjustment_evaluation_package_manifest::{
    AdjustmentEvaluationPackageManifestError, AdjustmentEvaluationPackageManifestValues,
    AdjustmentEvaluationPackageMember, AdmittedAdjustmentEvaluationPackageManifest,
    admit_adjustment_evaluation_package_manifest, read_adjustment_evaluation_package_manifest,
};
use crate::adjustment_evaluation_package_row::{
    AdjustmentEvaluationPackageBodyClaim, AdjustmentEvaluationPackageRowValues,
    AdmittedAdjustmentEvaluationPackageRow, admit_adjustment_evaluation_package_row,
};
use crate::adjustment_evaluation_package_stream::{
    AdjustmentEvaluationBodyInspection, AdjustmentEvaluationOpenedMember,
    AdjustmentEvaluationRowObservation, AdjustmentEvaluationRowStreamSummary,
    AdjustmentEvaluationRowsConsumer, AdjustmentEvaluationStreamError,
    AdjustmentEvaluationStreamingValues, consume_adjustment_evaluation_member,
    consume_adjustment_evaluation_row_stream, validate_adjustment_evaluation_body,
};

const ROW_MEMBER: &str = "members/rows.jsonl.gz";
const BODY_PREFIX: &str = "members/bodies/";
const EDGE_OBJECT_PREFIX: &str = "members/edge/objects/";
const EDGE_RECEIPT_PREFIX: &str = "members/edge/receipts/";

/// Approved effects needed to verify the complete extracted package.
pub trait AdjustmentEvaluationPackageVerificationValues:
    AdjustmentEvaluationPackageManifestValues
    + AdjustmentEvaluationPackageRowValues
    + AdjustmentEvaluationStreamingValues
    + AdjustmentEvaluationEdgeVerificationValues
{
    type OpenedMember: AdjustmentEvaluationOpenedMember;
    type BodyInspection: AdjustmentEvaluationBodyInspection;

    /// open one admitted local member without consuming its stream
    fn open_declared_member(
        &mut self,
        root: &str,
        member_path: &str,
    ) -> io::Result<Self::OpenedMember>;

    /// start the existing private-material inspection for one decompressed body
    fn begin_body_inspection(&mut self) -> Self::BodyInspection;

    /// retain javascript comparison outside the ordinary timestamp string domain
    fn receipt_is_later(&mut self, next: &JsonValue, prior: &JsonValue) -> bool;
}

/// Closed failures returned by the strict verification boundary.
#[derive(Debug, PartialEq, Eq)]
pub enum AdjustmentEvaluationPackageVerificationError {
    Manifest(AdjustmentEvaluationPackageManifestError),
    Stream(AdjustmentEvaluationStreamError),
    Edge(AdjustmentEvaluationEdgeVerificationError),
    MissingRowMember,
    RowDiagnostics,
    BodyAllocation,
    UnverifiedMember,
    ManifestChanged,
}

impl Display for AdjustmentEvaluationPackageVerificationError {
    /// render the retained predecessor diagnostic
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        // preserve the exact phase text or its underlying effect
        match self {
            Self::Manifest(error) => Display::fmt(error, formatter),
            Self::Stream(error) => Display::fmt(error, formatter),
            Self::Edge(error) => Display::fmt(error, formatter),
            Self::MissingRowMember => formatter.write_str("package row member is missing"),
            Self::RowDiagnostics => formatter.write_str("package row diagnostics differ"),
            Self::BodyAllocation => {
                formatter.write_str("package compressed body allocation differs")
            }
            Self::UnverifiedMember => formatter.write_str("package contains an unverified member"),
            Self::ManifestChanged => {
                formatter.write_str("package manifest changed during verification")
            }
        }
    }
}

impl std::error::Error for AdjustmentEvaluationPackageVerificationError {}

/// Verify the declared inventory, streamed rows, bodies, edge bundle and final controls.
pub fn verify_adjustment_evaluation_package(
    root: &str,
    values: &mut impl AdjustmentEvaluationPackageVerificationValues,
) -> Result<AdmittedAdjustmentEvaluationPackageManifest, AdjustmentEvaluationPackageVerificationError>
{
    use AdjustmentEvaluationPackageVerificationError as Error;

    let manifest =
        admit_adjustment_evaluation_package_manifest(root, values).map_err(Error::Manifest)?;
    let row_member = manifest.member(ROW_MEMBER).ok_or(Error::MissingRowMember)?;
    let opened = values
        .open_declared_member(root, ROW_MEMBER)
        .map_err(|error| Error::Stream(error.into()))?;
    let digest = values.begin_sha256();
    let mut rows = StrictRows {
        root,
        manifest: &manifest,
        values,
        body_bindings: BTreeMap::new(),
    };

    let summary = consume_adjustment_evaluation_member(
        opened,
        row_member.size_bytes,
        &row_member.sha256,
        digest,
        |compressed| {
            let decoded = rows.values.gunzip_stream(compressed)?;
            consume_adjustment_evaluation_row_stream(decoded, row_member.size_bytes, &mut rows)
        },
    )
    .map_err(Error::Stream)?;
    require_row_diagnostics(&manifest, &summary)?;
    let body_count = verify_remaining_bodies(&mut rows)?;
    let edge_count = verify_declared_edges(root, &manifest, values)?;

    // bind every member to exactly one completed verification phase
    if 1 + body_count + edge_count != manifest.members.len() {
        return Err(Error::UnverifiedMember);
    }
    let final_manifest =
        read_adjustment_evaluation_package_manifest(root, values).map_err(Error::Manifest)?;

    // compare exact controls after verifying all member populations
    if final_manifest.manifest_sha256 != manifest.manifest_sha256
        || final_manifest.manifest_bytes != manifest.manifest_bytes
    {
        return Err(Error::ManifestChanged);
    }
    Ok(manifest)
}

/// Read and rebind one small member through the same opened stream boundary.
pub fn read_verified_adjustment_evaluation_member(
    root: &str,
    member_path: &str,
    size_bytes: u64,
    sha256: &Sha256Digest,
    maximum_bytes: usize,
    values: &mut impl AdjustmentEvaluationPackageVerificationValues,
) -> Result<Vec<u8>, AdjustmentEvaluationStreamError> {
    // protect narrow consumers at their original fixed member size
    if size_bytes > maximum_bytes as u64 {
        return Err(AdjustmentEvaluationStreamError::Effect(
            "package member exceeds its fixed size limit".to_owned(),
        ));
    }
    let member = values.open_declared_member(root, member_path)?;
    let digest = values.begin_sha256();
    consume_adjustment_evaluation_member(member, size_bytes, sha256, digest, |reader| {
        let mut bytes = Vec::with_capacity(size_bytes as usize);
        reader.read_to_end(&mut bytes)?;
        Ok(bytes)
    })
}

struct StrictRows<'a, Values> {
    root: &'a str,
    manifest: &'a AdmittedAdjustmentEvaluationPackageManifest,
    values: &'a mut Values,
    body_bindings: BTreeMap<String, VerifiedBodyBinding>,
}

#[derive(PartialEq)]
struct VerifiedBodyBinding {
    plaintext_bytes: Option<f64>,
    compressed_bytes: u64,
    plaintext_sha256: String,
}

impl<Values: AdjustmentEvaluationPackageVerificationValues> AdjustmentEvaluationRowsConsumer
    for StrictRows<'_, Values>
{
    type Row = AdmittedAdjustmentEvaluationPackageRow;

    /// admit one original canonical row before opening its body member
    fn admit_row(&mut self, line: &str) -> Result<Self::Row, AdjustmentEvaluationStreamError> {
        admit_adjustment_evaluation_package_row(line.as_bytes(), self.values)
            .map_err(|error| AdjustmentEvaluationStreamError::Effect(error.to_string()))
    }

    /// project the exact summary values from the retained canonical row
    fn observe_row(row: &Self::Row) -> AdjustmentEvaluationRowObservation<'_> {
        AdjustmentEvaluationRowObservation {
            last_received_at: &row.last_received_at,
            revision_count: row.revision_count,
            source_identity: &row.source_identity,
        }
    }

    /// delegate original ordering only for non-string receipts
    fn receipt_is_later(&mut self, next: &JsonValue, prior: &JsonValue) -> bool {
        self.values.receipt_is_later(next, prior)
    }

    /// verify a referenced body only once and drop the complete verified row
    fn accept_row(&mut self, row: Self::Row) -> Result<(), AdjustmentEvaluationStreamError> {
        // rows without bodies have no additional verification effect
        if let Some(body) = row.body {
            verify_bound_body(self, body)?;
        }
        Ok(())
    }
}

/// Bind the row stream diagnostics after the compressed bytes themselves pass.
fn require_row_diagnostics(
    manifest: &AdmittedAdjustmentEvaluationPackageManifest,
    summary: &AdjustmentEvaluationRowStreamSummary,
) -> Result<(), AdjustmentEvaluationPackageVerificationError> {
    let max_receipt = summary
        .max_source_receipt_at
        .as_ref()
        .unwrap_or(&JsonValue::Null);
    // retain exact row populations and actual decoded byte framing
    if (summary.canonical_stream_bytes > 0 && !summary.ends_with_line_feed)
        || summary.canonical_stream_bytes != manifest.canonical_stream_bytes
        || summary.row_count != manifest.row_count
        || summary.revised_row_count != manifest.revised_row_count
        || summary.max_revision_count != manifest.max_revision_count
        || !same_receipt_value(
            max_receipt,
            manifest
                .max_source_receipt_at
                .as_ref()
                .unwrap_or(&JsonValue::Null),
        )
        || summary
            .source_identities
            .iter()
            .ne(&manifest.source_identities)
    {
        return Err(AdjustmentEvaluationPackageVerificationError::RowDiagnostics);
    }
    Ok(())
}

/// compare separately parsed receipt values with javascript strict equality
fn same_receipt_value(left: &JsonValue, right: &JsonValue) -> bool {
    !matches!(left, JsonValue::Array(_) | JsonValue::Object(_)) && left == right
}

/// Bind a body declaration, then consume its actual compressed and plaintext bytes.
fn verify_bound_body(
    rows: &mut StrictRows<'_, impl AdjustmentEvaluationPackageVerificationValues>,
    claim: AdjustmentEvaluationPackageBodyClaim,
) -> Result<(), AdjustmentEvaluationStreamError> {
    // require the declared body length and content address for this receipt
    let Some((member, plaintext_sha256)) = rows
        .manifest
        .member(&claim.member_path)
        .zip(claim.body_sha256)
        .filter(|(member, digest)| {
            Some(member.size_bytes as f64) == claim.compressed_bytes
                && claim.member_path == format!("{BODY_PREFIX}sha256-{digest}.json.gz")
        })
    else {
        return Err(AdjustmentEvaluationStreamError::Effect(
            "packaged export row body member is missing".to_owned(),
        ));
    };
    let binding = VerifiedBodyBinding {
        plaintext_bytes: claim.plaintext_bytes,
        compressed_bytes: member.size_bytes,
        plaintext_sha256,
    };

    // require repeated receipts to bind the very same body metadata
    if let Some(prior) = rows.body_bindings.get(&claim.member_path) {
        return if *prior == binding {
            Ok(())
        } else {
            Err(AdjustmentEvaluationStreamError::Effect(
                "packaged export row body binding differs".to_owned(),
            ))
        };
    }

    let opened = rows.values.open_declared_member(rows.root, &member.path)?;
    let digest = rows.values.begin_sha256();
    consume_adjustment_evaluation_member(
        opened,
        member.size_bytes,
        &member.sha256,
        digest,
        |compressed| {
            let inspection = rows.values.begin_body_inspection();
            validate_adjustment_evaluation_body(
                compressed,
                member.size_bytes,
                binding.plaintext_bytes.unwrap_or(f64::NAN),
                Some(&binding.plaintext_sha256),
                rows.values,
                inspection,
            )
        },
    )?;
    rows.body_bindings.insert(claim.member_path, binding);
    Ok(())
}

/// Verify the allocation and consume unmatched bodies without retaining them.
fn verify_remaining_bodies(
    rows: &mut StrictRows<'_, impl AdjustmentEvaluationPackageVerificationValues>,
) -> Result<usize, AdjustmentEvaluationPackageVerificationError> {
    use AdjustmentEvaluationPackageVerificationError as Error;

    let body_members = rows
        .manifest
        .members
        .iter()
        .filter(|member| member.path.starts_with(BODY_PREFIX));
    let declared_bytes = body_members
        .clone()
        .map(|member| member.size_bytes)
        .sum::<u64>();
    // bind all compressed bodies before opening an unreferenced member
    if declared_bytes != rows.manifest.compressed_body_bytes {
        return Err(Error::BodyAllocation);
    }
    let mut count = 0;
    // verify otherwise unreferenced compressed members in declaration order
    for member in body_members {
        count += 1;
        // do not reread a shared body already verified by a referring row
        if !rows.body_bindings.contains_key(&member.path) {
            let opened = rows
                .values
                .open_declared_member(rows.root, &member.path)
                .map_err(|error| Error::Stream(error.into()))?;
            let digest = rows.values.begin_sha256();
            consume_adjustment_evaluation_member(
                opened,
                member.size_bytes,
                &member.sha256,
                digest,
                |reader| {
                    io::copy(reader, &mut io::sink())?;
                    Ok(())
                },
            )
            .map_err(Error::Stream)?;
        }
    }
    Ok(count)
}

/// Verify frozen receipts and objects in their declared stream order.
fn verify_declared_edges(
    root: &str,
    manifest: &AdmittedAdjustmentEvaluationPackageManifest,
    values: &mut impl AdjustmentEvaluationPackageVerificationValues,
) -> Result<usize, AdjustmentEvaluationPackageVerificationError> {
    let summary = AdjustmentEvaluationEdgeSummary {
        // a saturated count is still above the bounded package's possible members
        object_count: manifest.edge_object_count as u64,
        receipt_count: manifest.edge_receipt_count as u64,
        watermark_sha256: Sha256Digest::admitted(manifest.edge_watermark_sha256.clone()),
    };
    let objects = edge_members(&manifest.members, EDGE_OBJECT_PREFIX);
    let receipts = edge_members(&manifest.members, EDGE_RECEIPT_PREFIX);
    verify_adjustment_evaluation_edge_bundle(root, &summary, &objects, &receipts, values)
        .map(|paths| paths.len())
        .map_err(AdjustmentEvaluationPackageVerificationError::Edge)
}

#[cfg(test)]
#[path = "adjustment_evaluation_package_verification_test.rs"]
pub(crate) mod tests;

/// Keep only narrow edge descriptors and preserve manifest declaration order.
fn edge_members(
    members: &[AdjustmentEvaluationPackageMember],
    prefix: &str,
) -> Vec<AdjustmentEvaluationEdgeMember> {
    members
        .iter()
        .filter(|member| member.path.starts_with(prefix))
        .map(|member| AdjustmentEvaluationEdgeMember {
            path: member.path.clone(),
            sha256: member.sha256.clone(),
            size_bytes: member.size_bytes,
        })
        .collect()
}
