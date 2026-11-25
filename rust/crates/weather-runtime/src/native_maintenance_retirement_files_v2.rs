//! Exact fixed-address legacy maintenance retirement over admitted finalized authority.

use std::{
    fmt::{self, Display, Formatter},
    fs,
    io::{ErrorKind, Read},
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
};

use weather_contracts::JsonValue;
use weather_edge::RevisionArchiveCanonicalCodec;

use crate::{
    api_maintenance_candidate_files::open_read_no_follow,
    maintenance_canonical_json::{exact_keys, text, valid_hash},
    shadow_metadata_custody_write_files::sync_private_directory,
};

/// Closed source metadata families that can become redundant after durable finalization.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMaintenanceRetirementKindV2 {
    ArchiveAcknowledgement,
    CycleAcknowledgement,
    FinalAcknowledgement,
    FinalManifest,
    InputSeal,
    RevisionCommitReceipt,
    RevisionFrontierSuccessor,
    RevisionProjection,
    RevisionPublishReceipt,
    RevisionStageReceipt,
    RevisionTerminalGap,
    ShadowRevisionCapsule,
    ShadowRevisionPublishReceipt,
    ShadowRevisionTerminalGap,
}

impl NativeMaintenanceRetirementKindV2 {
    /// Parse only the legacy source's fixed retirement families.
    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "archive_acknowledgement" => Self::ArchiveAcknowledgement,
            "cycle_acknowledgement" => Self::CycleAcknowledgement,
            "final_acknowledgement" => Self::FinalAcknowledgement,
            "final_manifest" => Self::FinalManifest,
            "input_seal" => Self::InputSeal,
            "revision_commit_receipt" => Self::RevisionCommitReceipt,
            "revision_frontier_successor" => Self::RevisionFrontierSuccessor,
            "revision_projection" => Self::RevisionProjection,
            "revision_publish_receipt" => Self::RevisionPublishReceipt,
            "revision_stage_receipt" => Self::RevisionStageReceipt,
            "revision_terminal_gap" => Self::RevisionTerminalGap,
            "shadow_revision_capsule" => Self::ShadowRevisionCapsule,
            "shadow_revision_publish_receipt" => Self::ShadowRevisionPublishReceipt,
            "shadow_revision_terminal_gap" => Self::ShadowRevisionTerminalGap,
            _ => return None,
        })
    }

    /// Retain the exact source wire name and ordering key.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ArchiveAcknowledgement => "archive_acknowledgement",
            Self::CycleAcknowledgement => "cycle_acknowledgement",
            Self::FinalAcknowledgement => "final_acknowledgement",
            Self::FinalManifest => "final_manifest",
            Self::InputSeal => "input_seal",
            Self::RevisionCommitReceipt => "revision_commit_receipt",
            Self::RevisionFrontierSuccessor => "revision_frontier_successor",
            Self::RevisionProjection => "revision_projection",
            Self::RevisionPublishReceipt => "revision_publish_receipt",
            Self::RevisionStageReceipt => "revision_stage_receipt",
            Self::RevisionTerminalGap => "revision_terminal_gap",
            Self::ShadowRevisionCapsule => "shadow_revision_capsule",
            Self::ShadowRevisionPublishReceipt => "shadow_revision_publish_receipt",
            Self::ShadowRevisionTerminalGap => "shadow_revision_terminal_gap",
        }
    }

    /// Map each admitted family to the source's fixed relative storage directory.
    pub const fn directory(self) -> &'static str {
        match self {
            Self::ArchiveAcknowledgement => "online-pages/archive-acknowledgements",
            Self::CycleAcknowledgement => "online-pages/acknowledgements",
            Self::FinalAcknowledgement => "online-pages/final-acknowledgements",
            Self::FinalManifest => "online-pages/final-manifests",
            Self::InputSeal => "online-pages/input-seals",
            Self::RevisionCommitReceipt => "revision-commit-receipts",
            Self::RevisionFrontierSuccessor => "revision-frontier-successors",
            Self::RevisionProjection => "revision-projections",
            Self::RevisionPublishReceipt => "revision-publish-receipts",
            Self::RevisionStageReceipt => "revision-stage-receipts",
            Self::RevisionTerminalGap => "revision-gaps",
            Self::ShadowRevisionCapsule => "shadow-revision-capsules",
            Self::ShadowRevisionPublishReceipt => "shadow-revision-publish-receipts",
            Self::ShadowRevisionTerminalGap => "shadow-revision-gaps",
        }
    }

    /// Retain the original per-family reread bound at retirement.
    pub const fn retirement_maximum_bytes(self) -> usize {
        match self {
            Self::FinalManifest => 32 * 1024,
            Self::RevisionProjection => 768 * 1024,
            Self::ShadowRevisionCapsule => 2 * 1024 * 1024,
            _ => 4096,
        }
    }

    /// Retain the source collection bound for cycle metadata identities.
    pub const fn collection_maximum_bytes(self) -> usize {
        match self {
            Self::ArchiveAcknowledgement
            | Self::CycleAcknowledgement
            | Self::FinalAcknowledgement
            | Self::InputSeal => 2048,
            _ => self.retirement_maximum_bytes(),
        }
    }
}

/// One exact metadata identity whose cold member retains the same original bytes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeMaintenanceRetirementEntryV2 {
    pub kind: NativeMaintenanceRetirementKindV2,
    pub identity_sha256: String,
    pub file_sha256: String,
}

impl NativeMaintenanceRetirementEntryV2 {
    /// Admit the fixed lowercase identity and content hash before constructing a path.
    pub fn new(
        kind: NativeMaintenanceRetirementKindV2,
        identity_sha256: String,
        file_sha256: String,
    ) -> Option<Self> {
        (valid_hash(&identity_sha256) && valid_hash(&file_sha256)).then_some(Self {
            kind,
            identity_sha256,
            file_sha256,
        })
    }

    /// Render exactly the four source retirement fields with identical cold and file hashes.
    pub fn to_json(&self) -> JsonValue {
        JsonValue::Object(
            [
                ("coldMemberSha256", self.file_sha256.as_str()),
                ("fileSha256", self.file_sha256.as_str()),
                ("identitySha256", self.identity_sha256.as_str()),
                ("kind", self.kind.as_str()),
            ]
            .into_iter()
            .map(|(key, value)| {
                (
                    key.encode_utf16().collect(),
                    JsonValue::String(value.encode_utf16().collect()),
                )
            })
            .collect(),
        )
    }

    /// Produce the exact fixed-address member path under an already admitted evidence root.
    pub fn path(&self, root: &Path) -> Option<PathBuf> {
        (valid_hash(&self.identity_sha256) && valid_hash(&self.file_sha256)).then(|| {
            root.join(self.kind.directory())
                .join(format!("sha256-{}.json", self.identity_sha256))
        })
    }
}

/// Parse one exact source entry and keep only members with redundant identical bytes.
pub fn parse_maintenance_retirement_entry_v2(
    value: &JsonValue,
) -> Option<NativeMaintenanceRetirementEntryV2> {
    if !exact_keys(
        value,
        &["coldMemberSha256", "fileSha256", "identitySha256", "kind"],
    ) {
        return None;
    }
    let file_sha256 = text(value, "fileSha256")?;
    // require the cold graph to retain the exact hot member bytes
    if text(value, "coldMemberSha256")? != file_sha256 {
        return None;
    }
    NativeMaintenanceRetirementEntryV2::new(
        NativeMaintenanceRetirementKindV2::parse(&text(value, "kind")?)?,
        text(value, "identitySha256")?,
        file_sha256,
    )
}

/// Source-shaped bounded file retirement failures.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMaintenanceRetirementFilesV2Error {
    Root,
    Entry,
    File,
    Identity,
    Remove,
    Sync,
}

impl Display for NativeMaintenanceRetirementFilesV2Error {
    /// Render the fixed owning operation rather than disclosing retained paths.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Root => "maintenance evidence root is invalid",
            Self::Entry => "maintenance retirement entry is invalid",
            Self::File => "adjustment evidence file is invalid",
            Self::Identity => "maintenance retirement file differs",
            Self::Remove => "maintenance retirement unlink failed",
            Self::Sync => "maintenance retirement directory sync failed",
        })
    }
}

impl std::error::Error for NativeMaintenanceRetirementFilesV2Error {}

/// Read one exact bounded regular metadata file, preserving only genuine absence.
pub(crate) fn read_optional_maintenance_retirement_file_v2(
    path: &Path,
    maximum: usize,
) -> Result<Option<Vec<u8>>, NativeMaintenanceRetirementFilesV2Error> {
    use NativeMaintenanceRetirementFilesV2Error as Error;
    let mut file = match open_read_no_follow(path) {
        Ok(file) => file,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(Error::File),
    };
    let before = file.metadata().map_err(|_| Error::File)?;
    // retain the source no-follow regular-file, exact mode, and size admission
    if !before.is_file()
        || before.permissions().mode() & 0o777 != 0o600
        || before.size() > maximum as u64
    {
        return Err(Error::File);
    }
    let mut bytes = Vec::with_capacity(before.size() as usize);
    (&mut file)
        .take(maximum as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| Error::File)?;
    let after = file.metadata().map_err(|_| Error::File)?;
    // keep stable descriptor ownership throughout the bounded read
    if bytes.len() > maximum
        || bytes.len() as u64 != before.size()
        || before.dev() != after.dev()
        || before.ino() != after.ino()
        || before.size() != after.size()
        || before.mtime() != after.mtime()
        || before.mtime_nsec() != after.mtime_nsec()
        || before.ctime() != after.ctime()
        || before.ctime_nsec() != after.ctime_nsec()
    {
        return Err(Error::File);
    }
    Ok(Some(bytes))
}

/// Read the source-bound fixed collection member without changing retained files.
pub(crate) fn read_expected_maintenance_member_v2(
    root: &Path,
    kind: NativeMaintenanceRetirementKindV2,
    identity: &str,
) -> Result<Option<Vec<u8>>, NativeMaintenanceRetirementFilesV2Error> {
    // require an admitted immutable address before constructing the fixed path
    if !valid_hash(identity) {
        return Err(NativeMaintenanceRetirementFilesV2Error::Entry);
    }
    read_optional_maintenance_retirement_file_v2(
        &root
            .join(kind.directory())
            .join(format!("sha256-{identity}.json")),
        kind.collection_maximum_bytes(),
    )
}

/// Return only the source's two-slot exact revision receipt inventory in sorted order.
pub(crate) fn read_maintenance_revision_commit_receipt_names_v2(
    root: &Path,
) -> Result<Vec<String>, NativeMaintenanceRetirementFilesV2Error> {
    read_fixed_maintenance_names(
        root,
        NativeMaintenanceRetirementKindV2::RevisionCommitReceipt,
        2,
    )
}

/// Return the source's finite acknowledgement namespace sorted by immutable address.
pub(crate) fn read_maintenance_cycle_acknowledgement_names_v2(
    root: &Path,
) -> Result<Vec<String>, NativeMaintenanceRetirementFilesV2Error> {
    read_fixed_maintenance_names(
        root,
        NativeMaintenanceRetirementKindV2::CycleAcknowledgement,
        8192,
    )
}

/// Inventory one already selected fixed metadata directory under its source population bound.
fn read_fixed_maintenance_names(
    root: &Path,
    kind: NativeMaintenanceRetirementKindV2,
    maximum: usize,
) -> Result<Vec<String>, NativeMaintenanceRetirementFilesV2Error> {
    use NativeMaintenanceRetirementFilesV2Error as Error;
    let directory = root.join(kind.directory());
    let read = match fs::read_dir(directory) {
        Ok(read) => read,
        // the original permits a legacy cycle with no revision spool
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(_) => return Err(Error::File),
    };
    let mut names = Vec::with_capacity(maximum.min(16));
    for entry in read {
        let name = entry
            .map_err(|_| Error::File)?
            .file_name()
            .into_string()
            .map_err(|_| Error::Entry)?;
        let identity = name
            .strip_prefix("sha256-")
            .and_then(|value| value.strip_suffix(".json"));
        // refuse an excessive receipt population or an alternate source filename
        if names.len() == maximum || !identity.is_some_and(valid_hash) {
            return Err(Error::Entry);
        }
        names.push(name);
    }
    names.sort_unstable();
    Ok(names)
}

/// Retire only the already authorized fixed members after the caller commits exact F.
pub(crate) fn retire_maintenance_hot_files_v2(
    root: &Path,
    entries: &[NativeMaintenanceRetirementEntryV2],
    codec: &impl RevisionArchiveCanonicalCodec,
) -> Result<usize, NativeMaintenanceRetirementFilesV2Error> {
    use NativeMaintenanceRetirementFilesV2Error as Error;
    let metadata = fs::symlink_metadata(root).map_err(|_| Error::Root)?;
    // accept only the canonical private evidence directory selected by the installed owner
    if !metadata.is_dir()
        || metadata.permissions().mode() & 0o777 != 0o700
        || fs::canonicalize(root).map_err(|_| Error::Root)? != root
    {
        return Err(Error::Root);
    }
    let mut retired = 0;
    let mut directories: Vec<&str> = Vec::new();
    for entry in entries {
        let path = entry.path(root).ok_or(Error::Entry)?;
        let Some(bytes) = read_optional_maintenance_retirement_file_v2(
            &path,
            entry.kind.retirement_maximum_bytes(),
        )?
        else {
            // exact retry after previous retirement leaves a genuine absent member
            continue;
        };
        // compare only the immutable content identity immediately before the exact unlink
        if format_digest(codec.sha256(&bytes)) != entry.file_sha256 {
            return Err(Error::Identity);
        }
        fs::remove_file(path).map_err(|_| Error::Remove)?;
        retired += 1;
        let directory = entry.kind.directory();
        if !directories.contains(&directory) {
            directories.push(directory);
        }
    }
    // make every completed exact unlink durable before reporting success
    for directory in directories {
        sync_private_directory(&root.join(directory)).map_err(|_| Error::Sync)?;
    }
    Ok(retired)
}

/// Encode the selected native SHA-256 bytes as the source's lowercase hex identity.
pub(crate) fn format_digest(bytes: [u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(64);
    // retain exact lowercase wire spelling without a new dependency
    for byte in bytes {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 15)]));
    }
    output
}

#[cfg(test)]
#[path = "native_maintenance_retirement_files_v2_test.rs"]
mod tests;
