//! Read the bounded revision-custody acknowledgement pair without granting action authority.

use std::fs;
use std::io::{ErrorKind, Read};
use std::os::unix::fs::MetadataExt;
use std::path::Path;

use weather_api::CurrentTimeResolver;
use weather_contracts::JsonValue;

use crate::api_maintenance_candidate_files::open_read_no_follow;
use crate::maintenance_anchor_status::{
    clock_field, hash_field, nullable_hash_field, uint64_field,
};
use crate::maintenance_canonical_json::{exact_keys, field, text};
use crate::shadow_metadata_custody::canonical_self_address;
use crate::{MaintenanceAnchorStatusError, MaintenanceAnchorStatusJson};

const ACK_V1_VERSION: &str = "adjustment-revision-custody-acknowledgement/v1";
pub(crate) const ACK_V2_VERSION: &str = "adjustment-revision-custody-acknowledgement/v2";
const MAXIMUM_ACK_BYTES: usize = 16 * 1024;
const CUSTODY_DIRECTORY: &str = "revision-custody-acknowledgements";
const REQUIRED_DIRECTORIES: &[&str] = &[
    "revision-commit-receipts",
    "revision-frontier-successors",
    "revision-projections",
    "shadow-revision-capsules",
    "rain-control-states",
    "rain-control-state-stage-receipts",
    CUSTODY_DIRECTORY,
    "revision-gaps",
    "revision-publish-receipts",
    "revision-spool-locks",
    "revision-stage-receipts",
    "shadow-revision-gaps",
    "shadow-revision-publish-receipts",
];
const RETIREMENT_KINDS: &[&str] = &[
    "revision_commit_receipt",
    "revision_frontier_successor",
    "revision_commit_group",
    "revision_projection",
    "revision_publish_receipt",
    "revision_stage_receipt",
    "rain_control_state",
    "rain_control_state_stage_receipt",
    "revision_terminal_gap",
    "shadow_revision_capsule",
    "shadow_revision_publish_receipt",
    "shadow_revision_terminal_gap",
];

/// Read the current and retained direct predecessor after complete fixed-root admission.
pub(crate) fn read_revision_custody_acknowledgements(
    root: &Path,
    codec: &mut impl MaintenanceAnchorStatusJson,
    time: &dyn CurrentTimeResolver,
) -> Result<[Option<JsonValue>; 2], MaintenanceAnchorStatusError> {
    require_private_directory(root, None)?;
    // require every original cold and custody child before reading state
    for directory in REQUIRED_DIRECTORIES {
        require_private_directory(&root.join(directory), Some(root))?;
    }
    let directory = root.join(CUSTODY_DIRECTORY);
    let mut names = fs::read_dir(&directory)
        .map_err(|_| MaintenanceAnchorStatusError::Root)?
        .map(|entry| {
            entry
                .map_err(|_| MaintenanceAnchorStatusError::Root)?
                .file_name()
                .into_string()
                .map_err(|_| MaintenanceAnchorStatusError::UnknownEntry)
        })
        .collect::<Result<Vec<_>, _>>()?;
    names.sort_unstable();
    // admit only the two fixed checkpoint names
    if names.len() > 2
        || names
            .iter()
            .any(|name| name != "current.json" && name != "previous.json")
    {
        return Err(MaintenanceAnchorStatusError::UnknownEntry);
    }
    let current = read_acknowledgement(&directory.join("current.json"), codec, time)?;
    let previous = read_acknowledgement(&directory.join("previous.json"), codec, time)?;
    // reject an orphan predecessor or a substituted direct predecessor
    if current.is_none() && previous.is_some()
        || current.as_ref().is_some_and(|current| {
            let expected = text(current, "previousAcknowledgementSha256");
            match previous.as_ref() {
                Some(previous) => expected != text(previous, "acknowledgementSha256"),
                None => expected.is_some(),
            }
        })
    {
        return Err(MaintenanceAnchorStatusError::Slot);
    }
    Ok([current, previous])
}

// require one literal canonical owner-private directory without owner inference
fn require_private_directory(
    path: &Path,
    expected_parent: Option<&Path>,
) -> Result<(), MaintenanceAnchorStatusError> {
    let metadata = fs::symlink_metadata(path).map_err(|_| MaintenanceAnchorStatusError::Root)?;
    let canonical = fs::canonicalize(path).map_err(|_| MaintenanceAnchorStatusError::Root)?;
    // retain the predecessor's exact real-directory and mode contract
    if !metadata.is_dir()
        || metadata.mode() & 0o777 != 0o700
        || canonical != path
        || expected_parent.is_some_and(|parent| canonical.parent() != Some(parent))
    {
        return Err(MaintenanceAnchorStatusError::Root);
    }
    Ok(())
}

// read one optional canonical acknowledgement without adding owner or link requirements
fn read_acknowledgement(
    path: &Path,
    codec: &mut impl MaintenanceAnchorStatusJson,
    time: &dyn CurrentTimeResolver,
) -> Result<Option<JsonValue>, MaintenanceAnchorStatusError> {
    let file = match open_read_no_follow(path) {
        Ok(file) => file,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(MaintenanceAnchorStatusError::Slot),
    };
    let metadata = file
        .metadata()
        .map_err(|_| MaintenanceAnchorStatusError::Slot)?;
    // match readRegularFile's regular, private and bounded admission
    if !metadata.is_file()
        || metadata.mode() & 0o777 != 0o600
        || metadata.size() > MAXIMUM_ACK_BYTES as u64
    {
        return Err(MaintenanceAnchorStatusError::Slot);
    }
    let mut bytes = Vec::new();
    file.take(MAXIMUM_ACK_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| MaintenanceAnchorStatusError::Slot)?;
    // reject growth beyond the fixed ceiling
    if bytes.len() > MAXIMUM_ACK_BYTES {
        return Err(MaintenanceAnchorStatusError::Slot);
    }
    let value = codec
        .parse_canonical_object(&bytes)
        .ok_or(MaintenanceAnchorStatusError::Slot)?;
    validate_acknowledgement(&value, codec, time).ok_or(MaintenanceAnchorStatusError::Slot)?;
    // prohibit alternate encodings at the retained checkpoint boundary
    if codec.encode_canonical_json(&value).as_deref() != Some(bytes.as_slice()) {
        return Err(MaintenanceAnchorStatusError::Slot);
    }
    Ok(Some(value))
}

// validate either frozen acknowledgement version and its complete retirement set
fn validate_acknowledgement(
    value: &JsonValue,
    codec: &mut impl MaintenanceAnchorStatusJson,
    time: &dyn CurrentTimeResolver,
) -> Option<()> {
    let version = text(value, "contractVersion")?;
    // select the one version-specific packed-custody identity field
    let identity = if version == ACK_V2_VERSION {
        "custodyCheckpointSha256"
    } else {
        "graphManifestSha256"
    };
    exact_keys(
        value,
        &[
            "acknowledgedAt",
            "acknowledgementSha256",
            "afterArchiveCommitOrdinal",
            "afterFrontierSha256",
            "authority",
            "contractVersion",
            identity,
            "memberRootSha256",
            "nextArchiveCommitOrdinal",
            "nextFrontierSha256",
            "pageSha256",
            "previousAcknowledgementSha256",
            "previousPageSha256",
            "retirementEntries",
            "startMemberSha256",
            "startSha256",
            "watermarkArchiveCommitOrdinal",
            "watermarkFrontierSha256",
        ],
    )
    .then_some(())?;
    clock_field(value, "acknowledgedAt", time)?;
    let after = uint64_field(value, "afterArchiveCommitOrdinal")?;
    let next = uint64_field(value, "nextArchiveCommitOrdinal")?;
    uint64_field(value, "watermarkArchiveCommitOrdinal")?;
    // bind every nonnullable identity
    [
        "acknowledgementSha256",
        "afterFrontierSha256",
        identity,
        "memberRootSha256",
        "nextFrontierSha256",
        "pageSha256",
        "previousPageSha256",
        "startSha256",
        "watermarkFrontierSha256",
    ]
    .iter()
    .all(|key| hash_field(value, key))
    .then_some(())?;
    // retain the two protocol nullable identity boundaries
    ["previousAcknowledgementSha256", "startMemberSha256"]
        .iter()
        .all(|key| nullable_hash_field(value, key))
        .then_some(())?;
    (text(value, "authority")?.as_str() == "cold_custody_only"
        && matches!(version.as_str(), ACK_V1_VERSION | ACK_V2_VERSION)
        && next > after)
        .then_some(())?;
    let JsonValue::Array(entries) = field(value, "retirementEntries")? else {
        return None;
    };
    ((4..=10).contains(&entries.len())).then_some(())?;
    let mut prior: Option<(String, String)> = None;
    // validate the exact stable kind and identity ordering
    for entry in entries {
        exact_keys(entry, &["fileSha256", "identitySha256", "kind"]).then_some(())?;
        (hash_field(entry, "fileSha256") && hash_field(entry, "identitySha256")).then_some(())?;
        let kind = text(entry, "kind")?;
        RETIREMENT_KINDS.contains(&kind.as_str()).then_some(())?;
        let identity = text(entry, "identitySha256")?;
        // reject only descending source order, preserving stable equals
        if prior
            .as_ref()
            .is_some_and(|prior| prior > &(kind.clone(), identity.clone()))
        {
            return None;
        }
        prior = Some((kind, identity));
    }
    canonical_self_address(value, "acknowledgementSha256", codec)
}
