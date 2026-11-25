//! Exact future-only maintenance anchor finalization without legacy hot-file retirement.

use std::{
    fmt::{Display, Formatter},
    io::Read,
};

use weather_api::CurrentTimeResolver;
use weather_contracts::JsonValue;

use crate::{
    MaintenanceAnchorSlot, MaintenanceAnchorState, MaintenanceAnchorStatusJson,
    maintenance_anchor_status::{MAXIMUM_ANCHOR_BYTES, clock_field},
    maintenance_anchor_status_v3::validate_v3_anchor,
    maintenance_canonical_json::{exact_keys, field, text, valid_hash},
};

const COMMAND: &str = "maintenance-anchor-finalize-v3";
const PROOF_VERSION: &str = "adjustment-maintenance-finalization-proof/v3";
const ANCHOR_VERSION: &str = "adjustment-maintenance-anchor/v3";
const RECEIPT_VERSION: &str = "adjustment-maintenance-anchor-finalization/v3";

const PROOF_BINDINGS: &[&str] = &[
    "actionSha256",
    "archiveCommitOrdinal",
    "burnSha256",
    "candidateArtifactRootSha256",
    "candidateReportSha256",
    "captureEpochWitnessSha256",
    "confirmationAccessSha256",
    "confirmationChunkCount",
    "controlSha256",
    "controlVersion",
    "custodyCheckpointSha256",
    "dueKey",
    "family",
    "frontierSha256",
    "fullGraphVerifiedAt",
    "fullMemberRootSha256",
    "graphManifestSha256",
    "inputSealSha256",
    "lifecycleLedgerRootSha256",
    "pageSha256",
    "policyReportSha256",
    "predecessorAnchorSha256",
    "publishedAt",
    "requiredInputRootSha256",
    "sequence",
    "sourceCommit",
    "workstationJournalHeadSha256",
];

/// Bound the command, proof, and retained transaction failures.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMaintenanceAnchorFinalizeV3Error {
    Arguments,
    Input,
    Proof,
    Identity,
    Root,
    Slot,
    CurrentUnavailable,
    Collision,
    AnchorMismatch,
    PendingCollision,
    PendingIdentity,
    Codec,
}

impl Display for NativeMaintenanceAnchorFinalizeV3Error {
    /// Report only a fixed original operation category.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Arguments => "maintenance-anchor-finalize-v3 requires PROOF_SHA256",
            Self::Input => "maintenance finalization proof bytes are invalid",
            Self::Proof => "future-only maintenance finalization proof is invalid",
            Self::Identity => "maintenance finalization proof identity is invalid",
            Self::Root => "maintenance anchor root is unsafe",
            Self::Slot => "maintenance anchor slot is unsafe",
            Self::CurrentUnavailable => "maintenance transferred anchor is unavailable",
            Self::Collision => "future-only maintenance finalization proof collision",
            Self::AnchorMismatch => "future-only maintenance finalization proof differs",
            Self::PendingCollision => "maintenance anchor pending collision",
            Self::PendingIdentity => "maintenance anchor pending identity is invalid",
            Self::Codec => "maintenance finalization serialization failed",
        })
    }
}

impl std::error::Error for NativeMaintenanceAnchorFinalizeV3Error {}

/// Exact admitted proof material needed for the source T-to-F transition.
struct FinalizationProof {
    value: JsonValue,
    digest: String,
}

/// Execute the source v3 command with the shared native anchor transaction owner.
pub fn finalize_maintenance_anchor_v3_command(
    command: (&str, &[&str]),
    input: &mut impl Read,
    files: &mut impl crate::NativeMaintenanceAnchorV3InstallFiles,
    codec: &mut impl MaintenanceAnchorStatusJson,
    time: &dyn CurrentTimeResolver,
) -> Result<Option<Vec<u8>>, NativeMaintenanceAnchorFinalizeV3Error> {
    use NativeMaintenanceAnchorFinalizeV3Error as Error;
    let (action, arguments) = command;
    // leave every other operator command untouched before observing input or filesystem
    if action != COMMAND {
        return Ok(None);
    }
    let [expected_digest] = arguments else {
        return Err(Error::Arguments);
    };
    // parse the exact source hash operand before consuming the bounded proof
    if !valid_hash(expected_digest) {
        return Err(Error::Arguments);
    }
    let mut bytes = Vec::new();
    input
        .take(MAXIMUM_ANCHOR_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| Error::Input)?;
    let proof = parse_proof(&bytes, codec, time)?;
    if proof.digest != *expected_digest {
        return Err(Error::Identity);
    }
    files.ensure_root().map_err(|_| Error::Root)?;
    let current = read_anchor(files, MaintenanceAnchorSlot::Current, codec, time)?
        .ok_or(Error::CurrentUnavailable)?;
    // converge only the exact already-finalized proof identity
    if current.state == MaintenanceAnchorState::Finalized {
        if text(&current.value, "finalizationProofSha256").as_deref() != Some(expected_digest) {
            return Err(Error::Collision);
        }
        return receipt(codec, &current.digest, expected_digest).map(Some);
    }
    // match the exact transferred byte digest and every original primitive authority
    if text(&proof.value, "transferredAnchorSha256").as_deref() != Some(current.digest.as_str())
        || !PROOF_BINDINGS
            .iter()
            .all(|key| equal_scalar(field(&proof.value, key), field(&current.value, key)))
    {
        return Err(Error::AnchorMismatch);
    }
    let mut finalized = current.value;
    let JsonValue::Object(fields) = &mut finalized else {
        return Err(Error::AnchorMismatch);
    };
    // extend the original T document with exactly the three finalized identities
    for (name, value) in fields.iter_mut() {
        if name.iter().copied().eq("ctfState".encode_utf16()) {
            *value = json_text("finalized");
            break;
        }
    }
    fields.push((
        "finalizationProofSha256".encode_utf16().collect(),
        json_text(expected_digest),
    ));
    fields.push((
        "finalizedAt".encode_utf16().collect(),
        field(&proof.value, "finalizedAt")
            .ok_or(Error::Proof)?
            .clone(),
    ));
    if validate_v3_anchor(&finalized, codec, time) != Some(MaintenanceAnchorState::Finalized) {
        return Err(Error::Proof);
    }
    let finalized_bytes = codec
        .encode_canonical_json(&finalized)
        .ok_or(Error::Codec)?;
    let finalized_digest = codec
        .sha256(&finalized_bytes)
        .ok_or(Error::Codec)?
        .as_str()
        .to_owned();

    // retain the same byte-identical pending transaction and never create a legacy retirement set
    match read_anchor(files, MaintenanceAnchorSlot::Pending, codec, time)? {
        Some(pending) if pending.bytes != finalized_bytes => return Err(Error::PendingCollision),
        Some(_) => {}
        None => files
            .create_pending(&finalized_bytes)
            .map_err(|_| Error::PendingCollision)?,
    }
    crate::native_maintenance_anchor_install_v3::commit_maintenance_anchor_v3_pending(
        &finalized_digest,
        files,
        codec,
        time,
    )
    .map_err(|error| {
        // keep an exact pending identity refusal separate from other transaction failures
        match error {
            crate::NativeMaintenanceAnchorInstallV3Error::Identity => Error::PendingIdentity,
            _ => Error::Slot,
        }
    })?;
    receipt(codec, &finalized_digest, expected_digest).map(Some)
}

/// Parse one exact canonical future-only proof and validate its projected transferred anchor.
fn parse_proof(
    bytes: &[u8],
    codec: &mut impl MaintenanceAnchorStatusJson,
    time: &dyn CurrentTimeResolver,
) -> Result<FinalizationProof, NativeMaintenanceAnchorFinalizeV3Error> {
    use NativeMaintenanceAnchorFinalizeV3Error as Error;
    if !(2..=MAXIMUM_ANCHOR_BYTES).contains(&bytes.len()) {
        return Err(Error::Input);
    }
    let value = codec.parse_canonical_object(bytes).ok_or(Error::Proof)?;
    let mut keys = PROOF_BINDINGS.to_vec();
    keys.extend(["contractVersion", "finalizedAt", "transferredAnchorSha256"]);
    if !exact_keys(&value, &keys)
        || text(&value, "contractVersion").as_deref() != Some(PROOF_VERSION)
        || !text(&value, "transferredAnchorSha256")
            .as_deref()
            .is_some_and(valid_hash)
    {
        return Err(Error::Proof);
    }
    let mut fields = PROOF_BINDINGS
        .iter()
        .map(|key| {
            let member = field(&value, key).ok_or(Error::Proof)?.clone();
            Ok((key.encode_utf16().collect(), member))
        })
        .collect::<Result<Vec<_>, Error>>()?;
    fields.push((
        "contractVersion".encode_utf16().collect(),
        json_text(ANCHOR_VERSION),
    ));
    fields.push((
        "ctfState".encode_utf16().collect(),
        json_text("transferred"),
    ));
    let anchor = JsonValue::Object(fields);
    if validate_v3_anchor(&anchor, codec, time) != Some(MaintenanceAnchorState::Transferred)
        || clock_field(&value, "finalizedAt", time).ok_or(Error::Proof)?
            < clock_field(&anchor, "publishedAt", time).ok_or(Error::Proof)?
    {
        return Err(Error::Proof);
    }
    let digest = codec.sha256(bytes).ok_or(Error::Codec)?.as_str().to_owned();
    Ok(FinalizationProof { value, digest })
}

struct RetainedAnchor {
    value: JsonValue,
    bytes: Vec<u8>,
    digest: String,
    state: MaintenanceAnchorState,
}

/// Read and admit one exact v3 anchor slot before changing transaction state.
fn read_anchor(
    files: &mut impl crate::NativeMaintenanceAnchorV3InstallFiles,
    slot: MaintenanceAnchorSlot,
    codec: &mut impl MaintenanceAnchorStatusJson,
    time: &dyn CurrentTimeResolver,
) -> Result<Option<RetainedAnchor>, NativeMaintenanceAnchorFinalizeV3Error> {
    use NativeMaintenanceAnchorFinalizeV3Error as Error;
    let Some(bytes) = files.read_anchor_slot(slot).map_err(|_| Error::Slot)? else {
        return Ok(None);
    };
    let value = codec.parse_canonical_object(&bytes).ok_or(Error::Slot)?;
    let state = validate_v3_anchor(&value, codec, time).ok_or(Error::Slot)?;
    let digest = codec
        .sha256(&bytes)
        .ok_or(Error::Codec)?
        .as_str()
        .to_owned();
    Ok(Some(RetainedAnchor {
        value,
        bytes,
        digest,
        state,
    }))
}

/// Preserve JavaScript's strict primitive equality across independently parsed documents.
fn equal_scalar(left: Option<&JsonValue>, right: Option<&JsonValue>) -> bool {
    matches!(
        left,
        Some(JsonValue::Null | JsonValue::Boolean(_) | JsonValue::Number(_) | JsonValue::String(_))
    ) && left == right
}

/// Encode a source primitive string as its exact UTF-16 value.
fn json_text(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}

/// Materialize the original exact finalization envelope with no legacy retired file fields.
fn receipt(
    codec: &mut impl MaintenanceAnchorStatusJson,
    anchor_digest: &str,
    proof_digest: &str,
) -> Result<Vec<u8>, NativeMaintenanceAnchorFinalizeV3Error> {
    let value = JsonValue::Object(
        [
            ("anchorSha256", anchor_digest),
            ("contractVersion", RECEIPT_VERSION),
            ("finalizationProofSha256", proof_digest),
            ("state", "finalized"),
        ]
        .into_iter()
        .map(|(key, value)| (key.encode_utf16().collect(), json_text(value)))
        .collect(),
    );
    codec
        .encode_canonical_json(&value)
        .ok_or(NativeMaintenanceAnchorFinalizeV3Error::Codec)
}

#[cfg(test)]
#[path = "native_maintenance_anchor_finalize_v3_test.rs"]
mod tests;
