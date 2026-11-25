//! Native installed-file owner for the private forecast-adjustment scorecard.

use std::error::Error;
use std::fmt::{Display, Formatter};
use std::fs;
use std::io;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use crate::native_revision_spool_file::{read_optional_file, read_private_file};
use crate::{
    AdmittedForecastAdjustmentScorecardNow, AdmittedForecastAdjustmentScorecardSnapshot,
    ForecastAdjustmentScorecardCandidates, ForecastAdjustmentScorecardContractVersion,
    ForecastAdjustmentScorecardStore,
};

const POINTER_MAXIMUM_BYTES: usize = 128;
const SCORECARD_V1_MAXIMUM_BYTES: usize = 512 * 1024;
const SCORECARD_V2_MAXIMUM_BYTES: usize = 128 * 1024;
const PRIVATE_DIRECTORY_MODE: u32 = 0o700;
const ORDINARY_MODE_MASK: u32 = 0o777;

/// Existing JSON, hash, and time admission retained outside filesystem mechanics.
pub trait ForecastAdjustmentScorecardFileAdapter {
    type Error;

    /// Decode one exact hash-only pointer document.
    fn decode_pointer(&self, content: &[u8]) -> Result<String, Self::Error>;

    /// Hash the exact scorecard bytes as lowercase hexadecimal SHA-256.
    fn sha256_lower_hex(&self, content: &[u8]) -> Result<String, Self::Error>;

    /// Decode and validate one scorecard against the captured read clock.
    fn decode_scorecard(
        &self,
        expected_version: ForecastAdjustmentScorecardContractVersion,
        content: &[u8],
        now: &AdmittedForecastAdjustmentScorecardNow,
    ) -> Result<AdmittedForecastAdjustmentScorecardSnapshot, Self::Error>;
}

/// Ordinary installed-file or canonical-adapter failure.
#[derive(Debug)]
pub enum FileForecastAdjustmentScorecardError<AdapterError> {
    Io(io::Error),
    Adapter(AdapterError),
    InvalidPublication(&'static str),
}

impl<AdapterError: Display> Display for FileForecastAdjustmentScorecardError<AdapterError> {
    /// Render one contained private-store failure.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        // preserve the owning error without exposing it through the route response
        match self {
            Self::Io(error) => Display::fmt(error, formatter),
            Self::Adapter(error) => Display::fmt(error, formatter),
            Self::InvalidPublication(message) => formatter.write_str(message),
        }
    }
}

impl<AdapterError> Error for FileForecastAdjustmentScorecardError<AdapterError>
where
    AdapterError: Error + 'static,
{
    /// Retain an ordinary filesystem or canonical-adapter source.
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        // only wrapped errors have an underlying source
        match self {
            Self::Io(error) => Some(error),
            Self::Adapter(error) => Some(error),
            Self::InvalidPublication(_) => None,
        }
    }
}

impl<AdapterError> From<io::Error> for FileForecastAdjustmentScorecardError<AdapterError> {
    /// Convert one native filesystem failure without changing its kind.
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

/// Real local scorecard storage over one configured evidence root.
#[derive(Debug)]
pub struct FileForecastAdjustmentScorecardStore<Adapter> {
    adapter: Adapter,
    root: PathBuf,
}

impl<Adapter> FileForecastAdjustmentScorecardStore<Adapter> {
    /// Bind one evidence root without reading or creating private material.
    pub fn new(root: PathBuf, adapter: Adapter) -> Self {
        Self { adapter, root }
    }

    /// Borrow the configured evidence root.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Recover the configured root and canonical adapter.
    pub fn into_parts(self) -> (PathBuf, Adapter) {
        (self.root, self.adapter)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SelectedSlot {
    CurrentV2,
    Legacy,
    PendingV2,
}

impl SelectedSlot {
    /// Return the only admitted contract generation for this pointer slot.
    const fn contract_version(self) -> ForecastAdjustmentScorecardContractVersion {
        // bind every selected publication slot to its source grammar
        match self {
            Self::Legacy => ForecastAdjustmentScorecardContractVersion::V1,
            Self::CurrentV2 | Self::PendingV2 => ForecastAdjustmentScorecardContractVersion::V2,
        }
    }

    /// Return the exact source-owned object size ceiling.
    const fn maximum_bytes(self) -> usize {
        // keep v2 under its tighter publication envelope
        match self {
            Self::Legacy => SCORECARD_V1_MAXIMUM_BYTES,
            Self::CurrentV2 | Self::PendingV2 => SCORECARD_V2_MAXIMUM_BYTES,
        }
    }
}

impl<Adapter> ForecastAdjustmentScorecardStore for FileForecastAdjustmentScorecardStore<Adapter>
where
    Adapter: ForecastAdjustmentScorecardFileAdapter,
{
    type Error = FileForecastAdjustmentScorecardError<Adapter::Error>;

    /// Read fixed pointers and the one selected hash-addressed publication.
    fn read_candidates(
        &self,
        now: &AdmittedForecastAdjustmentScorecardNow,
    ) -> Result<ForecastAdjustmentScorecardCandidates, Self::Error> {
        let legacy_directory = self.root.join("scorecards");
        let canonical_root = require_private_root_and_child(&self.root, &legacy_directory)?;

        let legacy = read_pointer(&self.root.join("current.json"), &self.adapter)?;
        let current_v2 = read_pointer(&self.root.join("v2-current.json"), &self.adapter)?;
        let previous_v2 = read_pointer(&self.root.join("v2-previous.json"), &self.adapter)?;
        let pending_v2 = read_pointer(&self.root.join("v2-pending.json"), &self.adapter)?;

        // preserve the predecessor's forbidden legacy/current-v2 coexistence rule
        if legacy.is_some() && (current_v2.is_some() || previous_v2.is_some()) {
            return Err(FileForecastAdjustmentScorecardError::InvalidPublication(
                "scorecard publication slots are invalid",
            ));
        }

        // select exactly one source slot without validating an unselected object
        let (slot, digest) = if let Some(digest) = pending_v2 {
            (SelectedSlot::PendingV2, digest)
        } else if let Some(digest) = current_v2 {
            (SelectedSlot::CurrentV2, digest)
        } else if let Some(digest) = legacy {
            (SelectedSlot::Legacy, digest)
        } else {
            return Err(FileForecastAdjustmentScorecardError::InvalidPublication(
                "scorecard publication is absent",
            ));
        };

        let selected_directory = match slot {
            SelectedSlot::Legacy => legacy_directory,
            SelectedSlot::CurrentV2 | SelectedSlot::PendingV2 => {
                let directory = self.root.join("scorecards-v2");
                require_private_child_directory(&canonical_root, &directory)?;
                directory
            }
        };
        let object_path = selected_directory.join(format!("sha256-{digest}.json"));
        let content = read_private_file(&object_path, slot.maximum_bytes())?;
        let actual = self
            .adapter
            .sha256_lower_hex(&content)
            .map_err(FileForecastAdjustmentScorecardError::Adapter)?;

        // bind the pointer to the exact bytes passed into the canonical decoder
        if !is_sha256(&actual) || actual != digest {
            return Err(FileForecastAdjustmentScorecardError::InvalidPublication(
                "scorecard content hash is invalid",
            ));
        }
        let snapshot = self
            .adapter
            .decode_scorecard(slot.contract_version(), &content, now)
            .map_err(FileForecastAdjustmentScorecardError::Adapter)?;

        // reject an adapter result belonging to another publication generation
        if snapshot.contract_version() != slot.contract_version() {
            return Err(FileForecastAdjustmentScorecardError::InvalidPublication(
                "scorecard generation does not match its publication slot",
            ));
        }

        let mut candidates = ForecastAdjustmentScorecardCandidates {
            current_v2: None,
            legacy: None,
            pending_v2: None,
            previous_v2_present: previous_v2.is_some(),
        };
        // move the one decoded publication into only its selected slot
        match slot {
            SelectedSlot::CurrentV2 => candidates.current_v2 = Some(snapshot),
            SelectedSlot::Legacy => candidates.legacy = Some(snapshot),
            SelectedSlot::PendingV2 => candidates.pending_v2 = Some(snapshot),
        }
        Ok(candidates)
    }
}

/// Read one optional fixed pointer through the private descriptor owner.
fn read_pointer<Adapter>(
    path: &Path,
    adapter: &Adapter,
) -> Result<Option<String>, FileForecastAdjustmentScorecardError<Adapter::Error>>
where
    Adapter: ForecastAdjustmentScorecardFileAdapter,
{
    let Some(content) = read_optional_file(path, POINTER_MAXIMUM_BYTES)? else {
        return Ok(None);
    };
    let digest = adapter
        .decode_pointer(&content)
        .map_err(FileForecastAdjustmentScorecardError::Adapter)?;

    // retain a closed lowercase digest grammar after JSON admission
    if !is_sha256(&digest) {
        return Err(FileForecastAdjustmentScorecardError::InvalidPublication(
            "scorecard pointer is invalid",
        ));
    }
    Ok(Some(digest))
}

/// Require one real private root and return its canonical identity.
fn require_private_root_and_child<AdapterError>(
    root: &Path,
    child: &Path,
) -> Result<PathBuf, FileForecastAdjustmentScorecardError<AdapterError>> {
    let root_metadata = fs::symlink_metadata(root)?;
    let canonical_root = fs::canonicalize(root)?;

    // reject a linked or non-private configured evidence root
    if !root_metadata.is_dir()
        || root_metadata.file_type().is_symlink()
        || root_metadata.mode() & ORDINARY_MODE_MASK != PRIVATE_DIRECTORY_MODE
    {
        return Err(FileForecastAdjustmentScorecardError::InvalidPublication(
            "scorecard directory is invalid",
        ));
    }
    require_private_child_directory(&canonical_root, child)?;
    Ok(canonical_root)
}

/// Require one direct private child under an already admitted root.
fn require_private_child_directory<AdapterError>(
    canonical_root: &Path,
    child: &Path,
) -> Result<(), FileForecastAdjustmentScorecardError<AdapterError>> {
    let child_metadata = fs::symlink_metadata(child)?;
    let canonical_child = fs::canonicalize(child)?;

    // reject links, mode drift, and a child escaping the configured evidence root
    if !child_metadata.is_dir()
        || child_metadata.file_type().is_symlink()
        || child_metadata.mode() & ORDINARY_MODE_MASK != PRIVATE_DIRECTORY_MODE
        || canonical_child.parent() != Some(canonical_root)
    {
        return Err(FileForecastAdjustmentScorecardError::InvalidPublication(
            "scorecard directory is invalid",
        ));
    }
    Ok(())
}

/// Recognize one exact lowercase SHA-256 spelling.
fn is_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
