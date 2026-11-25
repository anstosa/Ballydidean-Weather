//! Shared capacity admission for the four value-bearing revision spool directories.

use crate::revision_archive_canonical::{is_payload_name, is_sha256};

pub const REVISION_ONLINE_PAYLOAD_MAXIMUM_SLOTS: usize = 2;
pub const REVISION_ONLINE_PAYLOAD_MAXIMUM_BYTES: usize = 4_832 * 1024;
const DIRECTORY_ORDER: [RevisionOnlinePayloadKind; 4] = [
    RevisionOnlinePayloadKind::Projection,
    RevisionOnlinePayloadKind::ShadowCapsule,
    RevisionOnlinePayloadKind::ShadowStage,
    RevisionOnlinePayloadKind::RainControlState,
];

/// One of the online archive payload classes charged against the shared ceiling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RevisionOnlinePayloadKind {
    Projection,
    ShadowCapsule,
    ShadowStage,
    RainControlState,
}

impl RevisionOnlinePayloadKind {
    /// Resolve only a fixed source-owned directory component.
    pub const fn directory(self) -> &'static str {
        match self {
            Self::Projection => "revision-projections",
            Self::ShadowCapsule => "shadow-revision-capsules",
            Self::ShadowStage => "shadow-revision-stages",
            Self::RainControlState => "rain-control-states",
        }
    }

    /// Keep each individual member bounded independently of the aggregate.
    pub const fn maximum_bytes(self) -> usize {
        match self {
            Self::Projection => 3_500 * 1024,
            Self::ShadowCapsule | Self::ShadowStage => 2 * 1024 * 1024,
            Self::RainControlState => 32 * 1024,
        }
    }
}

/// One content-addressed member, never a caller-supplied filesystem path.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct RevisionOnlinePayloadAddress {
    kind: RevisionOnlinePayloadKind,
    name: String,
}

impl RevisionOnlinePayloadAddress {
    /// Form the exact spool basename from an admitted lowercase digest.
    pub fn from_sha256(kind: RevisionOnlinePayloadKind, sha256: &str) -> Option<Self> {
        // admit only the fixed content-addressed alphabet
        if !is_sha256(sha256) {
            return None;
        }
        Some(Self {
            kind,
            name: format!("sha256-{sha256}.json"),
        })
    }

    /// Expose the closed directory kind and its member name.
    pub fn components(&self) -> (RevisionOnlinePayloadKind, &str) {
        (self.kind, &self.name)
    }
}

/// New immutable bytes following content parsing and typed archive validation.
pub struct RevisionOnlinePayload<'a> {
    target: &'a RevisionOnlinePayloadAddress,
    bytes: &'a [u8],
    replaced_stage: Option<&'a RevisionOnlinePayloadAddress>,
}

impl<'a> RevisionOnlinePayload<'a> {
    /// Bind one admitted payload to the existing per-kind bound.
    pub fn new(
        target: &'a RevisionOnlinePayloadAddress,
        bytes: &'a [u8],
    ) -> Result<Self, RevisionOnlineSpoolError> {
        // keep the bound before any storage or lease work
        if bytes.is_empty() || bytes.len() > target.kind.maximum_bytes() {
            return Err(RevisionOnlineSpoolError::Input);
        }
        Ok(Self {
            target,
            bytes,
            replaced_stage: None,
        })
    }

    /// Replace only a bound stage when publishing its complete shadow capsule.
    pub fn replacing_shadow_stage(
        mut self,
        stage: &'a RevisionOnlinePayloadAddress,
    ) -> Result<Self, RevisionOnlineSpoolError> {
        // no other kind can subtract an already retained payload from its charge
        if self.target.kind != RevisionOnlinePayloadKind::ShadowCapsule
            || stage.kind != RevisionOnlinePayloadKind::ShadowStage
        {
            return Err(RevisionOnlineSpoolError::Input);
        }
        self.replaced_stage = Some(stage);
        Ok(self)
    }
}

/// Exact fresh-admission versus byte-identical retry disposition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RevisionOnlinePayloadAdmission {
    Fresh,
    IdenticalRetry,
}

/// Retain ordinary archive errors separately from the explicit spool capacity response.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RevisionOnlineSpoolError {
    Input,
    Storage,
    Census,
    IdentityConflict,
    Capacity,
}

/// IO under the existing admitted revision spool root and cross-process lease.
pub trait RevisionOnlineSpoolStorage {
    /// Keep the slot census, receipt update, and immutable publication in one lease.
    fn with_revision_spool_lease<T>(
        &mut self,
        operation: impl FnOnce(&mut Self) -> Result<T, RevisionOnlineSpoolError>,
    ) -> Result<T, RevisionOnlineSpoolError>;

    /// Read the existing candidate bytes only when resolving exact retry identity.
    fn read_existing_payload(
        &mut self,
        address: &RevisionOnlinePayloadAddress,
        maximum_bytes: usize,
    ) -> Result<Option<Vec<u8>>, RevisionOnlineSpoolError>;

    /// Enumerate the admitted private directory in its native iteration order.
    fn list_payload_names(
        &mut self,
        kind: RevisionOnlinePayloadKind,
    ) -> Result<Vec<String>, RevisionOnlineSpoolError>;

    /// Get the checked size from the validated immutable file handle, without copying its bytes.
    fn payload_file_length(
        &mut self,
        address: &RevisionOnlinePayloadAddress,
        maximum_bytes: usize,
    ) -> Result<usize, RevisionOnlineSpoolError>;
}

/// Perform one domain-specific persistence operation only after its shared capacity admission.
pub fn with_revision_online_payload<S: RevisionOnlineSpoolStorage, T>(
    storage: &mut S,
    payload: &RevisionOnlinePayload<'_>,
    persist: impl FnOnce(&mut S, RevisionOnlinePayloadAdmission) -> Result<T, RevisionOnlineSpoolError>,
) -> Result<T, RevisionOnlineSpoolError> {
    storage.with_revision_spool_lease(
        // keep the persistence callback inside the same lease as admission
        |storage| {
            let admission = admit_payload(storage, payload)?;
            persist(storage, admission)
        },
    )
}

/// Charge every retained member except the exact completed capsule's predecessor stage.
pub(crate) fn admit_payload(
    storage: &mut impl RevisionOnlineSpoolStorage,
    payload: &RevisionOnlinePayload<'_>,
) -> Result<RevisionOnlinePayloadAdmission, RevisionOnlineSpoolError> {
    // retry the same immutable bytes before imposing new-write capacity
    if let Some(existing) = storage.read_existing_payload(
        payload.target,
        RevisionOnlinePayloadKind::Projection.maximum_bytes(),
    )? {
        return if existing == payload.bytes {
            Ok(RevisionOnlinePayloadAdmission::IdenticalRetry)
        } else {
            Err(RevisionOnlineSpoolError::IdentityConflict)
        };
    }
    let mut slots = 1_usize;
    let mut measured_bytes = payload.bytes.len();
    // preserve the source census order and per-directory closed filename grammar
    for kind in DIRECTORY_ORDER {
        let maximum_bytes = kind.maximum_bytes();
        let names = storage.list_payload_names(kind)?;
        // classify unusable census rather than treating foreign entries as available capacity
        if !names.iter().map(String::as_str).all(is_payload_name) {
            return Err(RevisionOnlineSpoolError::Census);
        }
        // validate every retained file even when the final slot cannot be admitted
        for name in names {
            let member = RevisionOnlinePayloadAddress { name, kind };
            let bytes = storage.payload_file_length(&member, maximum_bytes)?;
            // require the file bound before subtracting the one replaced stage
            if bytes > maximum_bytes {
                return Err(RevisionOnlineSpoolError::Census);
            }
            // validate the predecessor file but don't count its retired capacity
            if payload.replaced_stage == Some(&member) {
                continue;
            }
            slots = slots.saturating_add(1);
            measured_bytes = measured_bytes.saturating_add(bytes);
        }
    }
    // preserve the distinct retryable private spool-capacity error
    if slots > REVISION_ONLINE_PAYLOAD_MAXIMUM_SLOTS
        || measured_bytes > REVISION_ONLINE_PAYLOAD_MAXIMUM_BYTES
    {
        return Err(RevisionOnlineSpoolError::Capacity);
    }
    Ok(RevisionOnlinePayloadAdmission::Fresh)
}
