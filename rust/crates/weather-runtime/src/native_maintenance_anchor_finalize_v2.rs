//! Legacy finalized-anchor transition with exact retained graph and retirement custody.

use std::{
    fmt::{Display, Formatter},
    io::Read,
};

use weather_api::CurrentTimeResolver;
use weather_contracts::JsonValue;
use weather_edge::RevisionArchiveCanonicalCodec;

use crate::{
    MaintenanceAnchorSlot, MaintenanceAnchorState, MaintenanceAnchorStatusJson,
    NativeMaintenanceAnchorV3InstallFiles, NativeMaintenanceAnchorV3InstallStore,
    NativeMaintenanceFinalizationProofV2, NativeMaintenanceRetirementExpectedV2Authority,
    maintenance_anchor_status::MAXIMUM_ANCHOR_BYTES,
    maintenance_anchor_status_v2::validate_v2_anchor,
    maintenance_canonical_json::{field, text, valid_hash},
    native_maintenance_anchor_install_v2::commit_maintenance_anchor_v2_pending,
    native_maintenance_finalization_proof_v2::{
        parse_native_maintenance_finalization_proof_v2,
        project_native_maintenance_finalized_anchor_v2,
    },
    native_maintenance_retirement_files_v2::retire_maintenance_hot_files_v2,
};

const COMMAND: &str = "maintenance-anchor-finalize-v2";

/// Original legacy proof, graph, transition and retirement owning stages.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMaintenanceAnchorFinalizeV2Error {
    Arguments,
    Input,
    Proof,
    Identity,
    Root,
    Slot,
    CurrentUnavailable,
    Collision,
    AnchorBinding,
    Transfer,
    ProducerPopulation,
    Frontier,
    Seal,
    RetirementSet,
    Pending,
    Retirement,
    Codec,
}

impl Display for NativeMaintenanceAnchorFinalizeV2Error {
    /// Render only the stable owning stage of the original legacy operation.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Arguments => "maintenance-anchor-finalize-v2 requires PROOF_SHA256",
            Self::Input => "maintenance finalization proof bytes are invalid",
            Self::Proof => "maintenance-anchor-finalize-v2 requires a v1 proof",
            Self::Identity => "maintenance finalization proof identity is invalid",
            Self::Root => "maintenance anchor root is unsafe",
            Self::Slot => "maintenance anchor slot is invalid",
            Self::CurrentUnavailable => "maintenance transferred anchor is unavailable",
            Self::Collision => "maintenance finalization proof collision",
            Self::AnchorBinding => "maintenance finalization anchor binding differs",
            Self::Transfer => "maintenance finalization transfer proof is unavailable",
            Self::ProducerPopulation => {
                "maintenance finalization producer population is incomplete"
            }
            Self::Frontier => "maintenance finalization revision frontier is incomplete",
            Self::Seal => "maintenance finalization input seal differs",
            Self::RetirementSet => "maintenance finalization retirement set differs",
            Self::Pending => "maintenance anchor pending transaction failed",
            Self::Retirement => "maintenance retirement failed",
            Self::Codec => "maintenance finalization serialization failed",
        })
    }
}

impl std::error::Error for NativeMaintenanceAnchorFinalizeV2Error {}

/// Source internal result retained for fixture verification; the direct v2 verb emits no body.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeMaintenanceAnchorFinalizationV2 {
    pub anchor_sha256: String,
    pub retired_files: usize,
    pub retirement_file_count: usize,
}

/// Execute only the exact legacy finalization verb and preserve its empty stdout.
pub fn finalize_maintenance_anchor_v2_command<Time>(
    command: (&str, &[&str]),
    input: &mut impl Read,
    files: &mut NativeMaintenanceAnchorV3InstallStore,
    expected: &mut impl NativeMaintenanceRetirementExpectedV2Authority,
    codec: &mut impl MaintenanceAnchorStatusJson,
    time: &Time,
) -> Result<Option<Vec<u8>>, NativeMaintenanceAnchorFinalizeV2Error>
where
    Time: CurrentTimeResolver + RevisionArchiveCanonicalCodec,
{
    use NativeMaintenanceAnchorFinalizeV2Error as Error;
    let (action, arguments) = command;
    // leave all unrelated commands to their original owners before observing input
    if action != COMMAND {
        return Ok(None);
    }
    let [expected_sha256] = arguments else {
        return Err(Error::Arguments);
    };
    // admit one exact hash operand before reading the bounded proof
    if !valid_hash(expected_sha256) {
        return Err(Error::Arguments);
    }
    let mut bytes = Vec::new();
    input
        .take(MAXIMUM_ANCHOR_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| Error::Input)?;
    let proof = parse_native_maintenance_finalization_proof_v2(&bytes, codec, time)
        .map_err(|_| Error::Proof)?;
    if proof.digest != *expected_sha256 {
        return Err(Error::Identity);
    }
    finalize_native_maintenance_anchor_v2(proof, files, expected, codec, time)?;
    Ok(Some(Vec::new()))
}

/// Bind the admitted proof to retained T, commit exact F, and retire only afterward.
fn finalize_native_maintenance_anchor_v2<Time>(
    proof: NativeMaintenanceFinalizationProofV2,
    files: &mut NativeMaintenanceAnchorV3InstallStore,
    expected: &mut impl NativeMaintenanceRetirementExpectedV2Authority,
    codec: &mut impl MaintenanceAnchorStatusJson,
    time: &Time,
) -> Result<NativeMaintenanceAnchorFinalizationV2, NativeMaintenanceAnchorFinalizeV2Error>
where
    Time: CurrentTimeResolver + RevisionArchiveCanonicalCodec,
{
    use NativeMaintenanceAnchorFinalizeV2Error as Error;
    files.ensure_root().map_err(|_| Error::Root)?;
    let current_bytes = files
        .read_anchor_slot(MaintenanceAnchorSlot::Current)
        .map_err(|_| Error::Slot)?
        .ok_or(Error::CurrentUnavailable)?;
    let current = codec
        .parse_canonical_object(&current_bytes)
        .ok_or(Error::Slot)?;
    let state = validate_v2_anchor(&current, codec, time).ok_or(Error::Slot)?;
    let current_hash = codec.sha256(&current_bytes).ok_or(Error::Codec)?;
    // exact committed F replay may only resume its original retirement proof
    if state == MaintenanceAnchorState::Finalized {
        if text(&current, "finalizationProofSha256").as_deref() != Some(proof.digest.as_str()) {
            return Err(Error::Collision);
        }
        return retire_after_commit(current_hash.as_str(), &proof, files, time);
    }

    let finalized =
        project_native_maintenance_finalized_anchor_v2(&proof, &current_bytes, codec, time)
            .map_err(|_| Error::AnchorBinding)?;
    let manifest = text(&current, "manifestSha256").ok_or(Error::Transfer)?;
    let graph = expected
        .read_expected(&manifest)
        .map_err(|_| Error::Transfer)?;
    // require the exact transferred cold graph before trusting any producer or member population
    if text(&current, "graphManifestSha256").as_deref() != Some(graph.graph_sha256.as_str()) {
        return Err(Error::Transfer);
    }
    if !graph.complete_producers {
        return Err(Error::ProducerPopulation);
    }
    let Some(JsonValue::Number(frontier_count)) = field(&proof.value, "frontierCount") else {
        return Err(Error::Frontier);
    };
    // every admitted proof's frontier is an integral source-bounded value
    if (*frontier_count as u64) < graph.revision_input_count {
        return Err(Error::Frontier);
    }
    if text(&proof.value, "inputSealSha256").as_deref() != Some(graph.input_seal_sha256.as_str())
        || text(&proof.value, "requiredInputRootSha256").as_deref()
            != Some(graph.required_input_root_sha256.as_str())
    {
        return Err(Error::Seal);
    }
    let expected_json =
        JsonValue::Array(graph.entries.iter().map(|entry| entry.to_json()).collect());
    let expected_bytes = codec
        .encode_canonical_json(&expected_json)
        .ok_or(Error::Codec)?;
    let expected_hash = codec.sha256(&expected_bytes).ok_or(Error::Codec)?;
    // bind the complete exact redundant member list to its independent canonical root
    if proof.entries != graph.entries
        || text(&proof.value, "retirementSetRootSha256").as_deref() != Some(expected_hash.as_str())
    {
        return Err(Error::RetirementSet);
    }
    // the legacy source creates the first pending F exclusively and commits before any hot-file removal
    files
        .create_pending(&finalized.bytes)
        .map_err(|_| Error::Pending)?;
    commit_maintenance_anchor_v2_pending(&finalized.digest, files, codec, time)
        .map_err(|_| Error::Pending)?;
    retire_after_commit(&finalized.digest, &proof, files, time)
}

/// Execute the source's exact retirement members only after durable F has been admitted.
fn retire_after_commit(
    anchor_hash: &str,
    proof: &NativeMaintenanceFinalizationProofV2,
    files: &NativeMaintenanceAnchorV3InstallStore,
    codec: &impl RevisionArchiveCanonicalCodec,
) -> Result<NativeMaintenanceAnchorFinalizationV2, NativeMaintenanceAnchorFinalizeV2Error> {
    let retired_files =
        retire_maintenance_hot_files_v2(files.evidence_root(), &proof.entries, codec)
            .map_err(|_| NativeMaintenanceAnchorFinalizeV2Error::Retirement)?;
    Ok(NativeMaintenanceAnchorFinalizationV2 {
        anchor_sha256: anchor_hash.to_owned(),
        retired_files,
        retirement_file_count: proof.entries.len(),
    })
}

#[cfg(test)]
#[path = "native_maintenance_anchor_finalize_v2_test.rs"]
mod tests;
