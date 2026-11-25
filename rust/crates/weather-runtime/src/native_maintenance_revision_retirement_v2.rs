//! Reconstruct exact nonempty legacy revision retirement members without deleting hot metadata.

use std::{
    fmt::{self, Display, Formatter},
    fs,
    path::Path,
};

use weather_contracts::{
    AdjustmentRevisionCommitReceipt, AdjustmentRevisionCommitReceiptProjectionKindEnum,
    AdjustmentRevisionGapReason, AdjustmentRevisionProjectionKind, MaintenanceShadowGapReason,
    Nullable,
};
use weather_edge::{
    ArchiveWireCodec, ArchiveWirePrimitives, PermanentArchiveGapCodec, PermanentArchiveGapInput,
    PrivateRevisionReceiptCodec, PrivateShadowReceiptCodec, RevisionArchiveCanonicalCodec,
};

use crate::maintenance_canonical_json::valid_hash;
use crate::native_maintenance_retirement_files_v2::{
    NativeMaintenanceRetirementEntryV2, NativeMaintenanceRetirementFilesV2Error,
    NativeMaintenanceRetirementKindV2, format_digest, read_expected_maintenance_member_v2,
    read_maintenance_revision_commit_receipt_names_v2,
};

const GAP_POPULATION_MAXIMUM: usize = 4_096;

/// Bounded failures while reconstructing one exact nonempty legacy revision graph.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMaintenanceRevisionRetirementV2Error {
    Store,
    Receipt,
    Capsule,
    Publication,
    Gap,
    Entry,
}

impl Display for NativeMaintenanceRevisionRetirementV2Error {
    /// Render only the failed source class without disclosing private paths or values.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Store => "maintenance revision retirement store differs",
            Self::Receipt => "maintenance revision retirement receipt differs",
            Self::Capsule => "maintenance shadow revision capsule differs",
            Self::Publication => "maintenance revision publication differs",
            Self::Gap => "maintenance revision terminal gap differs",
            Self::Entry => "maintenance revision retirement entry differs",
        })
    }
}

impl std::error::Error for NativeMaintenanceRevisionRetirementV2Error {}

/// Collect every exact revision member made redundant by the legacy two-slot online spool.
pub fn collect_native_maintenance_revision_retirement_v2<P: ArchiveWirePrimitives>(
    root: &Path,
    codec: &ArchiveWireCodec<P>,
    gap_codec: &impl PermanentArchiveGapCodec,
) -> Result<Vec<NativeMaintenanceRetirementEntryV2>, NativeMaintenanceRevisionRetirementV2Error> {
    let names = read_maintenance_revision_commit_receipt_names_v2(root).map_err(store_error)?;
    let mut entries = Vec::with_capacity(names.len() * 5);
    // preserve the source's sorted two-slot traversal
    for name in names {
        let addressed_frontier =
            payload_identity(&name).ok_or(NativeMaintenanceRevisionRetirementV2Error::Receipt)?;
        let receipt_bytes = required_member(
            root,
            NativeMaintenanceRetirementKindV2::RevisionCommitReceipt,
            &addressed_frontier,
        )?;
        let receipt = decode_canonical_commit(codec, &receipt_bytes)?;
        // bind the canonical receipt to the exact frontier-selected filename
        if receipt.frontierSha256 != addressed_frontier {
            return Err(NativeMaintenanceRevisionRetirementV2Error::Receipt);
        }
        add_entry(
            &mut entries,
            NativeMaintenanceRetirementKindV2::RevisionCommitReceipt,
            receipt.frontierSha256.clone(),
            &receipt_bytes,
            codec,
        )?;
        let successor_bytes = required_member(
            root,
            NativeMaintenanceRetirementKindV2::RevisionFrontierSuccessor,
            &receipt.predecessorFrontierSha256,
        )?;
        add_entry(
            &mut entries,
            NativeMaintenanceRetirementKindV2::RevisionFrontierSuccessor,
            receipt.predecessorFrontierSha256.clone(),
            &successor_bytes,
            codec,
        )?;
        // select the source's distinct shadow capsule and ordinary projection branches
        if receipt.projectionKind
            == AdjustmentRevisionCommitReceiptProjectionKindEnum::ShadowPrediction
        {
            collect_shadow_members(root, codec, gap_codec, &receipt, &mut entries)?;
        } else {
            collect_projection_members(root, codec, gap_codec, &receipt, &mut entries)?;
        }
    }
    entries.sort_by(|left, right| {
        left.kind
            .as_str()
            .cmp(right.kind.as_str())
            .then_with(|| left.identity_sha256.cmp(&right.identity_sha256))
    });
    Ok(entries)
}

/// Decode one canonical database commit receipt and recompute both linked identities.
fn decode_canonical_commit<P: ArchiveWirePrimitives>(
    codec: &ArchiveWireCodec<P>,
    bytes: &[u8],
) -> Result<AdjustmentRevisionCommitReceipt, NativeMaintenanceRevisionRetirementV2Error> {
    let value = codec
        .primitives()
        .decode_json(bytes)
        .ok_or(NativeMaintenanceRevisionRetirementV2Error::Receipt)?;
    let canonical = codec
        .primitives()
        .encode_sorted_json(&value)
        .ok_or(NativeMaintenanceRevisionRetirementV2Error::Receipt)?;
    // retain only the original sorted newline-framed file representation
    if canonical.as_slice() != bytes {
        return Err(NativeMaintenanceRevisionRetirementV2Error::Receipt);
    }
    codec
        .decode_revision_commit_value(&value)
        .filter(|receipt| codec.validate_revision_commit_receipt_identity(receipt))
        .ok_or(NativeMaintenanceRevisionRetirementV2Error::Receipt)
}

/// Collect the ordinary projection, stage metadata, and terminal publication disposition.
fn collect_projection_members<P: ArchiveWirePrimitives>(
    root: &Path,
    codec: &ArchiveWireCodec<P>,
    gap_codec: &impl PermanentArchiveGapCodec,
    receipt: &AdjustmentRevisionCommitReceipt,
    entries: &mut Vec<NativeMaintenanceRetirementEntryV2>,
) -> Result<(), NativeMaintenanceRevisionRetirementV2Error> {
    let projection_bytes = required_member(
        root,
        NativeMaintenanceRetirementKindV2::RevisionProjection,
        &receipt.projectionIdentitySha256,
    )?;
    add_entry(
        entries,
        NativeMaintenanceRetirementKindV2::RevisionProjection,
        receipt.projectionIdentitySha256.clone(),
        &projection_bytes,
        codec,
    )?;
    let stage_bytes = required_member(
        root,
        NativeMaintenanceRetirementKindV2::RevisionStageReceipt,
        &receipt.projectionIdentitySha256,
    )?;
    add_entry(
        entries,
        NativeMaintenanceRetirementKindV2::RevisionStageReceipt,
        receipt.projectionIdentitySha256.clone(),
        &stage_bytes,
        codec,
    )?;
    let publication = read_expected_maintenance_member_v2(
        root,
        NativeMaintenanceRetirementKindV2::RevisionPublishReceipt,
        &receipt.projectionIdentitySha256,
    )
    .map_err(store_error)?;
    // retire either the exact authenticated publication or one permanent terminal gap
    if let Some(bytes) = publication {
        let published = PrivateRevisionReceiptCodec::decode_publish_receipt(codec, &bytes)
            .filter(|published| revision_publication_matches(published, receipt))
            .ok_or(NativeMaintenanceRevisionRetirementV2Error::Publication)?;
        add_entry(
            entries,
            NativeMaintenanceRetirementKindV2::RevisionPublishReceipt,
            published.projectionIdentitySha256,
            &bytes,
            codec,
        )?;
    } else {
        let kind = ordinary_projection_kind(receipt.projectionKind)
            .ok_or(NativeMaintenanceRevisionRetirementV2Error::Receipt)?;
        let (identity, bytes) =
            find_revision_terminal_gap(root, gap_codec, &receipt.projectionIdentitySha256, kind)?;
        add_entry(
            entries,
            NativeMaintenanceRetirementKindV2::RevisionTerminalGap,
            identity,
            &bytes,
            codec,
        )?;
    }
    Ok(())
}

/// Collect the complete shadow capsule and its terminal publication disposition.
fn collect_shadow_members<P: ArchiveWirePrimitives>(
    root: &Path,
    codec: &ArchiveWireCodec<P>,
    gap_codec: &impl PermanentArchiveGapCodec,
    receipt: &AdjustmentRevisionCommitReceipt,
    entries: &mut Vec<NativeMaintenanceRetirementEntryV2>,
) -> Result<(), NativeMaintenanceRevisionRetirementV2Error> {
    let capsule_bytes = required_member(
        root,
        NativeMaintenanceRetirementKindV2::ShadowRevisionCapsule,
        &receipt.receiptSha256,
    )?;
    let capsule = codec
        .decode_capsule_document(&capsule_bytes)
        .ok_or(NativeMaintenanceRevisionRetirementV2Error::Capsule)?;
    add_entry(
        entries,
        NativeMaintenanceRetirementKindV2::ShadowRevisionCapsule,
        receipt.receiptSha256.clone(),
        &capsule_bytes,
        codec,
    )?;
    let metadata = &capsule.stage_input.metadata;
    let publication = read_expected_maintenance_member_v2(
        root,
        NativeMaintenanceRetirementKindV2::ShadowRevisionPublishReceipt,
        &metadata.predictionSha256,
    )
    .map_err(store_error)?;
    // retire either the exact authenticated publication or one permanent terminal gap
    if let Some(bytes) = publication {
        let published = PrivateShadowReceiptCodec::decode_publish_receipt(codec, &bytes)
            .filter(|published| {
                published.predictionSha256 == metadata.predictionSha256
                    && published.comparatorSha256 == capsule.stage_receipt.comparatorSha256
            })
            .ok_or(NativeMaintenanceRevisionRetirementV2Error::Publication)?;
        add_entry(
            entries,
            NativeMaintenanceRetirementKindV2::ShadowRevisionPublishReceipt,
            published.predictionSha256,
            &bytes,
            codec,
        )?;
    } else {
        let (identity, bytes) = find_shadow_terminal_gap(
            root,
            gap_codec,
            &metadata.dueKey,
            &metadata.registrationSha256,
        )?;
        add_entry(
            entries,
            NativeMaintenanceRetirementKindV2::ShadowRevisionTerminalGap,
            identity,
            &bytes,
            codec,
        )?;
    }
    Ok(())
}

/// Require the exact ordinary publication cross-bindings already authenticated by its codec.
fn revision_publication_matches(
    published: &weather_contracts::AdjustmentRevisionPublishReceipt,
    receipt: &AdjustmentRevisionCommitReceipt,
) -> bool {
    ordinary_projection_kind(receipt.projectionKind).is_some_and(|kind| {
        published.projectionIdentitySha256 == receipt.projectionIdentitySha256
            && published.projectionKind == kind
            && published.projectionSha256 == receipt.projectionSha256
            && published.revisionReceiptSha256 == receipt.receiptSha256
    })
}

/// Map the database receipt's nonshadow subset to the publication and gap contract enum.
fn ordinary_projection_kind(
    kind: AdjustmentRevisionCommitReceiptProjectionKindEnum,
) -> Option<AdjustmentRevisionProjectionKind> {
    match kind {
        AdjustmentRevisionCommitReceiptProjectionKindEnum::ActualBestMatch => {
            Some(AdjustmentRevisionProjectionKind::ActualBestMatch)
        }
        AdjustmentRevisionCommitReceiptProjectionKindEnum::NativeSource => {
            Some(AdjustmentRevisionProjectionKind::NativeSource)
        }
        AdjustmentRevisionCommitReceiptProjectionKindEnum::RainGateInput => {
            Some(AdjustmentRevisionProjectionKind::RainGateInput)
        }
        AdjustmentRevisionCommitReceiptProjectionKindEnum::TargetRevision => {
            Some(AdjustmentRevisionProjectionKind::TargetRevision)
        }
        AdjustmentRevisionCommitReceiptProjectionKindEnum::ShadowPrediction => None,
    }
}

/// Find exactly one authenticated ordinary gap addressed by its canonical locator fields.
fn find_revision_terminal_gap(
    root: &Path,
    codec: &impl PermanentArchiveGapCodec,
    projection_identity: &str,
    projection_kind: AdjustmentRevisionProjectionKind,
) -> Result<(String, Vec<u8>), NativeMaintenanceRevisionRetirementV2Error> {
    let names = read_gap_names(root, NativeMaintenanceRetirementKindV2::RevisionTerminalGap)?;
    let mut matched = None;
    // validate every stored gap before filtering the requested failed publication
    for name in names {
        let addressed_identity =
            payload_identity(&name).ok_or(NativeMaintenanceRevisionRetirementV2Error::Gap)?;
        let bytes = required_member(
            root,
            NativeMaintenanceRetirementKindV2::RevisionTerminalGap,
            &addressed_identity,
        )?;
        let gap = codec
            .accepted_revision_gap(&bytes)
            .ok_or(NativeMaintenanceRevisionRetirementV2Error::Gap)?;
        let key = codec
            .encode_gap_key(PermanentArchiveGapInput::Revision(&gap.input))
            .ok_or(NativeMaintenanceRevisionRetirementV2Error::Gap)?;
        // bind every decoded gap to its exact value-free source address
        if format_digest(codec.sha256(&key)) != addressed_identity {
            return Err(NativeMaintenanceRevisionRetirementV2Error::Gap);
        }
        let is_match = gap.input.projectionIdentitySha256
            == Nullable::Value(projection_identity.to_owned())
            && gap.input.projectionKind == projection_kind
            && gap.input.reason == AdjustmentRevisionGapReason::ArchivePublishFailed;
        // require a unique actual terminal member rather than choosing one duplicate
        if is_match && matched.replace((addressed_identity, bytes)).is_some() {
            return Err(NativeMaintenanceRevisionRetirementV2Error::Gap);
        }
    }
    matched.ok_or(NativeMaintenanceRevisionRetirementV2Error::Gap)
}

/// Find exactly one authenticated shadow gap addressed by its canonical schedule fields.
fn find_shadow_terminal_gap(
    root: &Path,
    codec: &impl PermanentArchiveGapCodec,
    due_key: &str,
    registration_sha256: &str,
) -> Result<(String, Vec<u8>), NativeMaintenanceRevisionRetirementV2Error> {
    let names = read_gap_names(
        root,
        NativeMaintenanceRetirementKindV2::ShadowRevisionTerminalGap,
    )?;
    let mut matched = None;
    // validate every stored gap before filtering the requested failed publication
    for name in names {
        let addressed_identity =
            payload_identity(&name).ok_or(NativeMaintenanceRevisionRetirementV2Error::Gap)?;
        let bytes = required_member(
            root,
            NativeMaintenanceRetirementKindV2::ShadowRevisionTerminalGap,
            &addressed_identity,
        )?;
        let gap = codec
            .accepted_shadow_gap(&bytes)
            .ok_or(NativeMaintenanceRevisionRetirementV2Error::Gap)?;
        let key = codec
            .encode_gap_key(PermanentArchiveGapInput::Shadow(&gap.input))
            .ok_or(NativeMaintenanceRevisionRetirementV2Error::Gap)?;
        // bind every decoded gap to its exact value-free source address
        if format_digest(codec.sha256(&key)) != addressed_identity {
            return Err(NativeMaintenanceRevisionRetirementV2Error::Gap);
        }
        let is_match = gap.input.dueKey == due_key
            && gap.input.registrationSha256 == registration_sha256
            && gap.input.reason == MaintenanceShadowGapReason::ArchivePublishFailed;
        // require a unique actual terminal member rather than choosing one duplicate
        if is_match && matched.replace((addressed_identity, bytes)).is_some() {
            return Err(NativeMaintenanceRevisionRetirementV2Error::Gap);
        }
    }
    matched.ok_or(NativeMaintenanceRevisionRetirementV2Error::Gap)
}

/// Inventory one required permanent-gap directory under the original fixed population bound.
fn read_gap_names(
    root: &Path,
    kind: NativeMaintenanceRetirementKindV2,
) -> Result<Vec<String>, NativeMaintenanceRevisionRetirementV2Error> {
    let read = fs::read_dir(root.join(kind.directory()))
        .map_err(|_| NativeMaintenanceRevisionRetirementV2Error::Store)?;
    let mut names = Vec::with_capacity(16);
    // reject alternate filenames and populations beyond the permanent writer's fixed ceiling
    for entry in read {
        let name = entry
            .map_err(|_| NativeMaintenanceRevisionRetirementV2Error::Store)?
            .file_name()
            .into_string()
            .map_err(|_| NativeMaintenanceRevisionRetirementV2Error::Gap)?;
        if names.len() == GAP_POPULATION_MAXIMUM || payload_identity(&name).is_none() {
            return Err(NativeMaintenanceRevisionRetirementV2Error::Gap);
        }
        names.push(name);
    }
    names.sort_unstable();
    Ok(names)
}

/// Read one fixed-address member and reject absence after its owning graph selected it.
fn required_member(
    root: &Path,
    kind: NativeMaintenanceRetirementKindV2,
    identity: &str,
) -> Result<Vec<u8>, NativeMaintenanceRevisionRetirementV2Error> {
    read_expected_maintenance_member_v2(root, kind, identity)
        .map_err(store_error)?
        .ok_or(NativeMaintenanceRevisionRetirementV2Error::Store)
}

/// Add one exact source entry using the hash of the retained original bytes.
fn add_entry(
    entries: &mut Vec<NativeMaintenanceRetirementEntryV2>,
    kind: NativeMaintenanceRetirementKindV2,
    identity: String,
    bytes: &[u8],
    codec: &impl RevisionArchiveCanonicalCodec,
) -> Result<(), NativeMaintenanceRevisionRetirementV2Error> {
    let file_sha256 = format_digest(codec.sha256(bytes));
    let entry = NativeMaintenanceRetirementEntryV2::new(kind, identity, file_sha256)
        .ok_or(NativeMaintenanceRevisionRetirementV2Error::Entry)?;
    entries.push(entry);
    Ok(())
}

/// Parse only the exact lower-hex content-addressed source filename grammar.
fn payload_identity(name: &str) -> Option<String> {
    let identity = name.strip_prefix("sha256-")?.strip_suffix(".json")?;
    valid_hash(identity).then(|| identity.to_owned())
}

/// Collapse the existing bounded file owner's failures into this graph boundary.
fn store_error(
    _error: NativeMaintenanceRetirementFilesV2Error,
) -> NativeMaintenanceRevisionRetirementV2Error {
    NativeMaintenanceRevisionRetirementV2Error::Store
}

#[cfg(test)]
#[path = "native_maintenance_revision_retirement_v2_test.rs"]
mod tests;
