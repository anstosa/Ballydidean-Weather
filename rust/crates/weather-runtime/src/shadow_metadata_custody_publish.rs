//! Prepare and publish compact shadow metadata custody proof checkpoints.

use std::fs;
use std::io::ErrorKind;

use weather_api::CurrentTimeResolver;
use weather_contracts::JsonValue;
use weather_database::Sha256Digest;

use crate::maintenance_canonical_json::{text, valid_hash};
use crate::shadow_metadata_custody::{
    MAXIMUM_DOCUMENT_BYTES, NativeShadowMetadataCustodyFiles, ShadowMetadataCustodyError,
    ShadowMetadataCustodySlot, finish_consumption, read_document,
};
use crate::shadow_metadata_custody_mutation::ShadowMetadataCustodyWrites;
use crate::shadow_metadata_custody_proof::validate_shadow_custody_proof;
use crate::shadow_metadata_custody_write_files::{
    cleanup_private_temporary, sync_private_directory, write_private_temporary,
};
use crate::{MaintenanceAnchorStatusJson, MaintenanceCanonicalJson};

const CONTRACT_VERSION: &str = "adjustment-shadow-metadata-custody-proof/v1";
const AUTHORITY: &str = "shadow_metadata_custody_only";

/// Already-admitted acknowledgement and capsule-derived material for one v2 proof.
#[derive(Debug, Clone, PartialEq)]
pub struct ShadowMetadataCustodyPublishInput {
    pub acknowledged_at: String,
    pub acknowledgement_sha256: Sha256Digest,
    pub archive_commit_ordinal: u64,
    pub custody_checkpoint_sha256: Sha256Digest,
    pub entries: Vec<JsonValue>,
    pub frontier_sha256: Sha256Digest,
    pub member_root_sha256: Sha256Digest,
    pub page_sha256: Sha256Digest,
}

/// Prepare one capsule-derived compact proof before its custody ACK is published.
pub fn prepare_shadow_metadata_custody_proof(
    input: ShadowMetadataCustodyPublishInput,
    files: &mut NativeShadowMetadataCustodyFiles,
    codec: &mut impl MaintenanceAnchorStatusJson,
    time: &dyn CurrentTimeResolver,
) -> Result<Option<JsonValue>, ShadowMetadataCustodyError> {
    use ShadowMetadataCustodyError::{Checkpoint, Codec};
    use ShadowMetadataCustodySlot::{Current, Pending, Previous};

    finish_consumption(files, codec, time)?;
    files.ensure_root()?;
    let current = read_document(Current, files, codec, time)?;
    let pending = read_document(Pending, files, codec, time)?;
    // validate the retained predecessor even though preparation does not rotate it
    let _previous = read_document(Previous, files, codec, time)?;
    let acknowledgement = input.acknowledgement_sha256.as_str();
    // block a later acknowledgement until its already-current proof is consumed
    if current.as_ref().is_some_and(|proof| {
        text(proof, "acknowledgementSha256").as_deref() != Some(acknowledgement)
    }) {
        return Err(Checkpoint);
    }
    let entries = ordered_entries(input.entries).ok_or(Checkpoint)?;
    // represent no shadow capsules with no proof while refusing orphaned pending state
    if entries.is_empty() {
        // reject an unrelated incomplete preparation
        if pending.is_some() {
            return Err(Checkpoint);
        }
        return Ok(None);
    }
    let mut fields = vec![
        member("acknowledgementSha256", string(acknowledgement)),
        member(
            "archiveCommitOrdinal",
            string(&input.archive_commit_ordinal.to_string()),
        ),
        member("authority", string(AUTHORITY)),
        member("contractVersion", string(CONTRACT_VERSION)),
        member(
            "custodyCheckpointSha256",
            string(input.custody_checkpoint_sha256.as_str()),
        ),
        member("entries", JsonValue::Array(entries)),
        member("frontierSha256", string(input.frontier_sha256.as_str())),
        member(
            "memberRootSha256",
            string(input.member_root_sha256.as_str()),
        ),
        member("pageSha256", string(input.page_sha256.as_str())),
        member("preparedAt", string(&input.acknowledged_at)),
    ];
    let unsigned = encode_bounded(&JsonValue::Object(fields.clone()), codec)?;
    let proof_sha256 = codec.sha256(&unsigned).ok_or(Codec)?;
    fields.push(member("proofSha256", string(proof_sha256.as_str())));
    let proof = JsonValue::Object(fields);
    validate_shadow_custody_proof(&proof, codec, time).ok_or(Checkpoint)?;
    let bytes = encode_bounded(&proof, codec)?;
    // converge only the exact already-current proof fixed by its first ACK
    if let Some(current) = current {
        // compare complete canonical bytes rather than selected fields
        if encode_bounded(&current, codec)? != bytes {
            return Err(Checkpoint);
        }
        return Ok(Some(proof));
    }
    // preserve an exact prior preparation or publish the first proof exclusively
    if let Some(pending) = pending {
        // reject a different proof behind the same pending slot
        if encode_bounded(&pending, codec)? != bytes {
            return Err(Checkpoint);
        }
    } else if !create_pending_proof(files, &bytes)? {
        return Err(ShadowMetadataCustodyError::Write);
    }
    Ok(Some(proof))
}

/// Publish the proof bound to one now-durable custody acknowledgement.
pub fn commit_shadow_metadata_custody_proof(
    acknowledgement_sha256: &Sha256Digest,
    files: &mut NativeShadowMetadataCustodyFiles,
    codec: &mut impl MaintenanceAnchorStatusJson,
    time: &dyn CurrentTimeResolver,
) -> Result<Option<JsonValue>, ShadowMetadataCustodyError> {
    use ShadowMetadataCustodyError::{Checkpoint, Remove, Write};
    use ShadowMetadataCustodySlot::{Current, Pending};

    let acknowledgement = acknowledgement_sha256.as_str();
    // reject a caller-constructed digest before observing any proof slot
    if !valid_hash(acknowledgement) {
        return Err(Checkpoint);
    }
    let current = read_document(Current, files, codec, time)?;
    let pending = read_document(Pending, files, codec, time)?;
    // converge an already-current proof without changing retained history
    if let Some(current) = current {
        // bind idempotency to the exact custody acknowledgement
        if text(&current, "acknowledgementSha256").as_deref() != Some(acknowledgement) {
            return Err(Checkpoint);
        }
        return Ok(Some(current));
    }
    let Some(pending) = pending else {
        return Ok(None);
    };
    // publish only the proof prepared for this durable acknowledgement
    if text(&pending, "acknowledgementSha256").as_deref() != Some(acknowledgement) {
        return Err(Checkpoint);
    }
    let bytes = encode_bounded(&pending, codec)?;
    replace_current_proof(files, &bytes).map_err(|_| Write)?;
    fs::remove_file(files.proof_root.join(Pending.filename())).map_err(|_| Remove)?;
    sync_private_directory(&files.proof_root).map_err(|_| ShadowMetadataCustodyError::Sync)?;
    Ok(Some(pending))
}

/// Sort already-admitted compact entries by their original proof identity order.
fn ordered_entries(entries: Vec<JsonValue>) -> Option<Vec<JsonValue>> {
    // retain only the original two-entry successful page bound
    if entries.len() > 2 {
        return None;
    }
    let mut ordered = entries
        .into_iter()
        .map(|entry| {
            // retain both exact digest keys beside the unchanged compact entry
            Some((
                text(&entry, "registrationSha256")?,
                text(&entry, "predictionSha256")?,
                entry,
            ))
        })
        .collect::<Option<Vec<_>>>()?;
    ordered.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));
    Some(ordered.into_iter().map(|(_, _, entry)| entry).collect())
}

/// Encode one canonical proof document within the shared compact slot ceiling.
fn encode_bounded(
    value: &JsonValue,
    codec: &mut impl MaintenanceCanonicalJson,
) -> Result<Vec<u8>, ShadowMetadataCustodyError> {
    let bytes = codec
        .encode_canonical_json(value)
        .ok_or(ShadowMetadataCustodyError::Codec)?;
    // keep every durable proof in the same fixed reader allocation bound
    if !(2..=MAXIMUM_DOCUMENT_BYTES).contains(&bytes.len()) {
        return Err(ShadowMetadataCustodyError::Codec);
    }
    Ok(bytes)
}

/// Publish the first pending proof through an exclusive hard link and directory sync.
fn create_pending_proof(
    files: &NativeShadowMetadataCustodyFiles,
    bytes: &[u8],
) -> Result<bool, ShadowMetadataCustodyError> {
    let temporary = write_private_temporary(&files.proof_root, "proof", bytes)
        .map_err(|_| ShadowMetadataCustodyError::Write)?;
    let target = files
        .proof_root
        .join(ShadowMetadataCustodySlot::Pending.filename());
    // distinguish only another complete first proof at the final exclusive link
    match fs::hard_link(&temporary, target) {
        Ok(()) => {}
        Err(error) if error.kind() == ErrorKind::AlreadyExists => {
            cleanup_private_temporary(&temporary).map_err(|_| ShadowMetadataCustodyError::Write)?;
            return Ok(false);
        }
        Err(_) => {
            cleanup_private_temporary(&temporary).map_err(|_| ShadowMetadataCustodyError::Write)?;
            return Err(ShadowMetadataCustodyError::Write);
        }
    }
    cleanup_private_temporary(&temporary).map_err(|_| ShadowMetadataCustodyError::Write)?;
    sync_private_directory(&files.proof_root).map_err(|_| ShadowMetadataCustodyError::Write)?;
    Ok(true)
}

/// Atomically replace current with the complete pending proof bytes.
fn replace_current_proof(
    files: &NativeShadowMetadataCustodyFiles,
    bytes: &[u8],
) -> std::io::Result<()> {
    let temporary = write_private_temporary(&files.proof_root, "proof-current", bytes)?;
    // rename only the unique complete temporary over the fixed current slot
    if let Err(error) = fs::rename(
        &temporary,
        files
            .proof_root
            .join(ShadowMetadataCustodySlot::Current.filename()),
    ) {
        return match cleanup_private_temporary(&temporary) {
            Ok(()) => Err(error),
            Err(cleanup) => Err(cleanup),
        };
    }
    sync_private_directory(&files.proof_root)
}

/// Encode one generated-contract string without introducing a JSON implementation.
fn string(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}

/// Build one generated-contract object member.
fn member(name: &str, value: JsonValue) -> (Vec<u16>, JsonValue) {
    (name.encode_utf16().collect(), value)
}
