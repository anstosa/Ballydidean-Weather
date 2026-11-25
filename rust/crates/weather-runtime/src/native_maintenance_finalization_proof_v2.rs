//! Pure legacy v1 finalization-proof admission and v2 anchor projection.

use std::fmt::{Display, Formatter};

use weather_api::CurrentTimeResolver;
use weather_contracts::JsonValue;

use crate::maintenance_anchor_status::{
    MAXIMUM_ANCHOR_BYTES, clock_field, hash_field, regex_text, uint64_field, valid_control_version,
    valid_source_commit,
};
use crate::maintenance_anchor_status_v2::validate_v2_anchor;
use crate::maintenance_canonical_json::{exact_keys, field, text};
use crate::native_maintenance_retirement_files_v2::{
    NativeMaintenanceRetirementEntryV2, parse_maintenance_retirement_entry_v2,
};
use crate::{MaintenanceAnchorState, MaintenanceAnchorStatusJson};

const PROOF_VERSION: &str = "adjustment-maintenance-finalization-proof/v1";
const PROOF_KEYS: &[&str] = &[
    "actionSha256",
    "archiveCommitOrdinal",
    "burnSha256",
    "catalogGeneration",
    "catalogRevisionReceiptRootSha256",
    "catalogRootSha256",
    "catalogWatermarkSha256",
    "confirmationChunkCount",
    "contractVersion",
    "controlSha256",
    "controlVersion",
    "finalizedAt",
    "frontierCount",
    "frontierRootSha256",
    "fullMemberRootSha256",
    "graphManifestSha256",
    "inputSealSha256",
    "lifecycleLedgerRootSha256",
    "manifestSha256",
    "reportSha256",
    "requiredInputRootSha256",
    "retirementEntries",
    "retirementSetRootSha256",
    "revisionSnapshotRootSha256",
    "sourceCommit",
    "transferredAnchorSha256",
    "workstationJournalHeadSha256",
];
const PROOF_HASH_FIELDS: &[&str] = &[
    "actionSha256",
    "burnSha256",
    "catalogRevisionReceiptRootSha256",
    "catalogRootSha256",
    "catalogWatermarkSha256",
    "controlSha256",
    "frontierRootSha256",
    "fullMemberRootSha256",
    "graphManifestSha256",
    "inputSealSha256",
    "lifecycleLedgerRootSha256",
    "manifestSha256",
    "reportSha256",
    "requiredInputRootSha256",
    "retirementSetRootSha256",
    "revisionSnapshotRootSha256",
    "transferredAnchorSha256",
    "workstationJournalHeadSha256",
];
const ANCHOR_BINDINGS: &[&str] = &[
    "actionSha256",
    "catalogGeneration",
    "catalogRootSha256",
    "catalogWatermarkSha256",
    "controlSha256",
    "controlVersion",
    "frontierCount",
    "frontierRootSha256",
    "fullMemberRootSha256",
    "graphManifestSha256",
    "lifecycleLedgerRootSha256",
    "manifestSha256",
    "reportSha256",
    "revisionSnapshotRootSha256",
    "sourceCommit",
    "workstationJournalHeadSha256",
];
const FINALIZED_PROOF_FIELDS: &[&str] = &[
    "archiveCommitOrdinal",
    "burnSha256",
    "catalogRevisionReceiptRootSha256",
    "confirmationChunkCount",
    "finalizedAt",
    "inputSealSha256",
    "requiredInputRootSha256",
    "retirementSetRootSha256",
];

/// Value-free legacy proof and projection admission failures.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMaintenanceFinalizationProofV2Error {
    Input,
    Proof,
    Anchor,
    Binding,
    Projection,
    Codec,
}

impl Display for NativeMaintenanceFinalizationProofV2Error {
    /// Report only the bounded source operation category.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Input => "maintenance finalization proof bytes are invalid",
            Self::Proof => "maintenance finalization proof is invalid",
            Self::Anchor => "maintenance transferred anchor is invalid",
            Self::Binding => "maintenance finalization anchor binding differs",
            Self::Projection => "maintenance finalized anchor projection is invalid",
            Self::Codec => "maintenance finalization serialization failed",
        })
    }
}

impl std::error::Error for NativeMaintenanceFinalizationProofV2Error {}

/// Exact canonical legacy proof plus its typed retirement population and identity.
#[derive(Clone, Debug, PartialEq)]
pub struct NativeMaintenanceFinalizationProofV2 {
    pub value: JsonValue,
    pub bytes: Vec<u8>,
    pub digest: String,
    pub entries: Vec<NativeMaintenanceRetirementEntryV2>,
}

/// Exact finalized v2 anchor projection ready for the retained slot transaction owner.
#[derive(Clone, Debug, PartialEq)]
pub struct NativeMaintenanceFinalizedAnchorV2 {
    pub value: JsonValue,
    pub bytes: Vec<u8>,
    pub digest: String,
    pub finalization_proof_sha256: String,
}

/// Parse one bounded canonical v1 proof without reading or retiring any live metadata.
pub fn parse_native_maintenance_finalization_proof_v2(
    bytes: &[u8],
    codec: &mut impl MaintenanceAnchorStatusJson,
    time: &dyn CurrentTimeResolver,
) -> Result<NativeMaintenanceFinalizationProofV2, NativeMaintenanceFinalizationProofV2Error> {
    use NativeMaintenanceFinalizationProofV2Error as Error;

    // retain the source anchor-document byte bounds
    if !(2..=MAXIMUM_ANCHOR_BYTES).contains(&bytes.len()) {
        return Err(Error::Input);
    }
    let document = codec.parse_canonical_object(bytes).ok_or(Error::Proof)?;
    let control_version = regex_text(&document, "controlVersion", codec).ok_or(Error::Proof)?;
    let source_commit = regex_text(&document, "sourceCommit", codec).ok_or(Error::Proof)?;
    // close the legacy proof contract before interpreting its retirement population
    if !exact_keys(&document, PROOF_KEYS)
        || text(&document, "contractVersion").as_deref() != Some(PROOF_VERSION)
        || !matches!(
            field(&document, "confirmationChunkCount"),
            Some(JsonValue::Number(value)) if *value == 24.0 || *value == 27.0
        )
        || !valid_control_version(&control_version)
        || !valid_source_commit(&source_commit)
    {
        return Err(Error::Proof);
    }
    uint64_field(&document, "archiveCommitOrdinal").ok_or(Error::Proof)?;
    uint64_field(&document, "catalogGeneration").ok_or(Error::Proof)?;
    clock_field(&document, "finalizedAt", time).ok_or(Error::Proof)?;
    // require every cross-system root before admitting any typed entry
    if !PROOF_HASH_FIELDS
        .iter()
        .all(|key| hash_field(&document, key))
    {
        return Err(Error::Proof);
    }
    frontier_count(&document).ok_or(Error::Proof)?;
    let JsonValue::Array(entries) = field(&document, "retirementEntries").ok_or(Error::Proof)?
    else {
        return Err(Error::Proof);
    };
    // retain the complete source population bounds
    if !(5..=51).contains(&entries.len()) {
        return Err(Error::Proof);
    }
    let mut retirement_entries = Vec::with_capacity(entries.len());
    let mut prior: Option<(String, String)> = None;
    // validate exact redundant identities and their canonical source order
    for value in entries {
        let entry = parse_maintenance_retirement_entry_v2(value).ok_or(Error::Proof)?;
        if entry.to_json() != value.clone()
            || prior.as_ref().is_some_and(|prior| {
                prior
                    > &(
                        entry.kind.as_str().to_owned(),
                        entry.identity_sha256.clone(),
                    )
            })
        {
            return Err(Error::Proof);
        }
        prior = Some((
            entry.kind.as_str().to_owned(),
            entry.identity_sha256.clone(),
        ));
        retirement_entries.push(entry);
    }
    // reject alternate object, number or array encodings after full validation
    if codec.encode_canonical_json(&document).as_deref() != Some(bytes) {
        return Err(Error::Proof);
    }
    let digest = codec.sha256(bytes).ok_or(Error::Codec)?.as_str().to_owned();
    Ok(NativeMaintenanceFinalizationProofV2 {
        value: document,
        bytes: bytes.to_vec(),
        digest,
        entries: retirement_entries,
    })
}

/// Crossbind one admitted proof and project the exact finalized v2 anchor document.
pub fn project_native_maintenance_finalized_anchor_v2(
    proof: &NativeMaintenanceFinalizationProofV2,
    transferred_anchor_bytes: &[u8],
    codec: &mut impl MaintenanceAnchorStatusJson,
    time: &dyn CurrentTimeResolver,
) -> Result<NativeMaintenanceFinalizedAnchorV2, NativeMaintenanceFinalizationProofV2Error> {
    use NativeMaintenanceFinalizationProofV2Error as Error;

    // retain the installed anchor's source byte and canonical encoding boundary
    if !(2..=MAXIMUM_ANCHOR_BYTES).contains(&transferred_anchor_bytes.len()) {
        return Err(Error::Anchor);
    }
    let anchor = codec
        .parse_canonical_object(transferred_anchor_bytes)
        .ok_or(Error::Anchor)?;
    if validate_v2_anchor(&anchor, codec, time) != Some(MaintenanceAnchorState::Transferred)
        || codec.encode_canonical_json(&anchor).as_deref() != Some(transferred_anchor_bytes)
    {
        return Err(Error::Anchor);
    }
    let transferred_sha256 = codec
        .sha256(transferred_anchor_bytes)
        .ok_or(Error::Codec)?
        .as_str()
        .to_owned();
    // require the exact anchor digest and every non-null transferred authority identity
    if text(&proof.value, "transferredAnchorSha256").as_deref() != Some(transferred_sha256.as_str())
        || !ANCHOR_BINDINGS
            .iter()
            .all(|key| binding_matches(&proof.value, &anchor, key))
        || uint64_field(&proof.value, "archiveCommitOrdinal").ok_or(Error::Proof)?
            < frontier_count(&proof.value).ok_or(Error::Proof)?
        || clock_field(&proof.value, "finalizedAt", time).ok_or(Error::Proof)?
            < clock_field(&anchor, "publishedAt", time).ok_or(Error::Anchor)?
    {
        return Err(Error::Binding);
    }

    let mut finalized = anchor;
    let JsonValue::Object(fields) = &mut finalized else {
        return Err(Error::Anchor);
    };
    // replace only the transferred state in the already admitted anchor
    let state = fields
        .iter_mut()
        .find(|(name, _)| name.iter().copied().eq("ctfState".encode_utf16()))
        .ok_or(Error::Anchor)?;
    state.1 = json_text("finalized");
    // append exactly the source proof-derived finalized projection fields
    for key in FINALIZED_PROOF_FIELDS {
        fields.push((
            key.encode_utf16().collect(),
            field(&proof.value, key).ok_or(Error::Proof)?.clone(),
        ));
    }
    fields.push((
        "finalizationProofSha256".encode_utf16().collect(),
        json_text(&proof.digest),
    ));
    if validate_v2_anchor(&finalized, codec, time) != Some(MaintenanceAnchorState::Finalized) {
        return Err(Error::Projection);
    }
    let bytes = codec
        .encode_canonical_json(&finalized)
        .ok_or(Error::Codec)?;
    let digest = codec
        .sha256(&bytes)
        .ok_or(Error::Codec)?
        .as_str()
        .to_owned();
    Ok(NativeMaintenanceFinalizedAnchorV2 {
        value: finalized,
        bytes,
        digest,
        finalization_proof_sha256: proof.digest.clone(),
    })
}

/// Decode the source's safe positive frontier count as an exact integer.
fn frontier_count(value: &JsonValue) -> Option<u64> {
    let JsonValue::Number(value) = field(value, "frontierCount")? else {
        return None;
    };
    (value.is_finite() && value.fract() == 0.0 && (1.0..=9_500_000.0).contains(value))
        .then_some(*value as u64)
}

/// Preserve JavaScript strict equality and reject unavailable anchor bindings.
fn binding_matches(proof: &JsonValue, anchor: &JsonValue, key: &str) -> bool {
    let Some(anchor_value) = field(anchor, key) else {
        return false;
    };
    !matches!(anchor_value, JsonValue::Null) && field(proof, key) == Some(anchor_value)
}

/// Encode one source primitive string without normalization.
fn json_text(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}

#[cfg(test)]
#[path = "native_maintenance_finalization_proof_v2_test.rs"]
pub(crate) mod tests;
