//! Immutable value-free shadow and projection dispositions.

use weather_contracts::{AdjustmentMaintenanceGapInput, AdjustmentRevisionGapInput, Nullable};

use crate::private_archive_service::{
    AdjustmentMaintenanceGapReceipt, AdjustmentRevisionGapReceipt, PrivateArchivePermanentGapStore,
    PrivateArchiveStorageError,
};
use crate::revision_archive_canonical::{
    RevisionArchiveCanonicalClock, RevisionArchiveCanonicalCodec, instant, is_payload_name,
    is_sha256, lower_hex,
};
use crate::revision_online_spool::RevisionOnlineSpoolError;

const MAXIMUM_FILES: usize = 4_096;
const MAXIMUM_BYTES: usize = 4_096;
const DISPOSITION: &str = "forever_unqualified";
type SpoolResult<T> = Result<T, RevisionOnlineSpoolError>;

/// Exact value-free metadata roots, separate from the two online value slots.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermanentArchiveGapKind {
    Shadow,
    Revision,
}

impl PermanentArchiveGapKind {
    /// Resolve one fixed private spool directory component.
    pub const fn directory(self) -> &'static str {
        match self {
            Self::Shadow => "shadow-revision-gaps",
            Self::Revision => "revision-gaps",
        }
    }

    /// Select the source-owned durable contract by metadata class.
    const fn contract(self) -> &'static str {
        match self {
            Self::Shadow => "adjustment-shadow-gap/v1",
            Self::Revision => "adjustment-revision-gap/v1",
        }
    }
}

/// Exact durable gap decode shared by the native scanner and immutable writer.
pub trait PermanentArchiveGapCodec: RevisionArchiveCanonicalCodec {
    /// Encode the source's canonical value-free locator document.
    fn encode_gap_key(&self, input: PermanentArchiveGapInput<'_>) -> Option<Vec<u8>>;

    /// Encode the exact signed receipt or its unsigned canonical preimage.
    fn encode_gap_receipt(
        &self,
        input: PermanentArchiveGapInput<'_>,
        recorded_at: &str,
        digest: Option<&str>,
    ) -> Option<Vec<u8>>;

    /// Decode and verify canonical revision gap bytes and their complete receipt identity.
    fn accepted_revision_gap(&self, bytes: &[u8]) -> Option<AdjustmentRevisionGapReceipt>;

    /// Decode and verify canonical shadow gap bytes and their complete receipt identity.
    fn accepted_shadow_gap(&self, bytes: &[u8]) -> Option<AdjustmentMaintenanceGapReceipt>;
}

/// A typed canonical gap address that cannot be supplied as an arbitrary path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermanentArchiveGapAddress {
    kind: PermanentArchiveGapKind,
    name: String,
}

impl PermanentArchiveGapAddress {
    /// Return the selected fixed directory and lower-hex basename.
    pub fn components(&self) -> (PermanentArchiveGapKind, &str) {
        (self.kind, &self.name)
    }
}

/// Shared revision lease and exact immutable file storage.
pub trait PermanentArchiveGapStorage {
    /// Verify the same deployed private custody root and fixed children.
    fn ensure_revision_root(&mut self) -> SpoolResult<()>;

    /// Enclose normal gap read, census and immutable write in the shared lease.
    fn with_permanent_gap_lease<T>(
        &mut self,
        operation: impl FnOnce(&mut Self) -> SpoolResult<T>,
    ) -> SpoolResult<T>;

    /// Read one existing bounded permanent disposition.
    fn read_permanent_gap(
        &mut self,
        address: &PermanentArchiveGapAddress,
        maximum_bytes: usize,
    ) -> SpoolResult<Option<Vec<u8>>>;

    /// Read enough ordinary census entries to determine the fixed ceiling.
    fn list_permanent_gap_names(
        &mut self,
        kind: PermanentArchiveGapKind,
        maximum: usize,
    ) -> SpoolResult<Vec<String>>;

    /// Publish one complete immutable disposition exclusively.
    fn write_permanent_gap(
        &mut self,
        address: &PermanentArchiveGapAddress,
        bytes: &[u8],
    ) -> SpoolResult<()>;
}

/// Real value-free permanent store over the canonical codec and native lease.
pub struct PermanentArchiveGapStore<Storage, Codec, Clock> {
    storage: Storage,
    codec: Codec,
    clock: Clock,
}

impl<Storage, Codec, Clock> PermanentArchiveGapStore<Storage, Codec, Clock>
where
    Storage: PermanentArchiveGapStorage,
    Codec: PermanentArchiveGapCodec,
    Clock: RevisionArchiveCanonicalClock,
{
    /// Bind one permanent metadata owner without assuming live deployment.
    pub const fn new(storage: Storage, codec: Codec, clock: Clock) -> Self {
        Self {
            storage,
            codec,
            clock,
        }
    }

    /// Validate and persist a value-free disposition under the one shared archive lease.
    fn record(&mut self, input: PermanentArchiveGapInput<'_>) -> SpoolResult<RetainedGap> {
        let kind = input.validated_kind()?;
        self.storage.ensure_revision_root()?;
        let key = bounded_gap_bytes(self.codec.encode_gap_key(input))?;
        let identity = lower_hex(self.codec.sha256(&key));
        let address = PermanentArchiveGapAddress {
            kind,
            name: format!("sha256-{identity}.json"),
        };
        let codec = &self.codec;
        let clock = &mut self.clock;
        self.storage.with_permanent_gap_lease(
            // keep the existing first receipt independent of newly reached capacity
            |storage| {
                if let Some(bytes) = storage.read_permanent_gap(&address, MAXIMUM_BYTES)? {
                    return input.retain_existing(codec, &bytes);
                }
                let names = storage.list_permanent_gap_names(kind, MAXIMUM_FILES)?;
                // preserve permanent metadata without replacement or ageing
                if names.len() >= MAXIMUM_FILES
                    || !names.iter().map(String::as_str).all(is_payload_name)
                {
                    return Err(RevisionOnlineSpoolError::Census);
                }
                let recorded_at = clock
                    .canonical_now()
                    .ok_or(RevisionOnlineSpoolError::Input)?;
                instant(codec, &recorded_at).ok_or(RevisionOnlineSpoolError::Input)?;
                let unsigned =
                    bounded_gap_bytes(codec.encode_gap_receipt(input, &recorded_at, None))?;
                let digest = lower_hex(codec.sha256(&unsigned));
                let signed = bounded_gap_bytes(codec.encode_gap_receipt(
                    input,
                    &recorded_at,
                    Some(&digest),
                ))?;
                storage.write_permanent_gap(&address, &signed)?;
                Ok(input.build_receipt(kind, recorded_at, digest))
            },
        )
    }
}

impl<Storage, Codec, Clock> PrivateArchivePermanentGapStore
    for PermanentArchiveGapStore<Storage, Codec, Clock>
where
    Storage: PermanentArchiveGapStorage,
    Codec: PermanentArchiveGapCodec,
    Clock: RevisionArchiveCanonicalClock,
{
    /// Retain one exact scheduled shadow disposition with the original first clock.
    fn record_shadow_gap(
        &mut self,
        input: AdjustmentMaintenanceGapInput,
    ) -> Result<AdjustmentMaintenanceGapReceipt, PrivateArchiveStorageError> {
        match self.record(PermanentArchiveGapInput::Shadow(&input)) {
            Ok(RetainedGap::Shadow(value)) => Ok(value),
            _ => Err(PrivateArchiveStorageError::Rejected),
        }
    }

    /// Retain one exact revision disposition with the original first clock.
    fn record_revision_gap(
        &mut self,
        input: AdjustmentRevisionGapInput,
    ) -> Result<AdjustmentRevisionGapReceipt, PrivateArchiveStorageError> {
        match self.record(PermanentArchiveGapInput::Revision(&input)) {
            Ok(RetainedGap::Revision(value)) => Ok(value),
            _ => Err(PrivateArchiveStorageError::Rejected),
        }
    }
}

/// One closed decoded disposition, used by the canonical JSON adapter.
#[derive(Clone, Copy)]
pub enum PermanentArchiveGapInput<'a> {
    Shadow(&'a AdjustmentMaintenanceGapInput),
    Revision(&'a AdjustmentRevisionGapInput),
}

enum RetainedGap {
    Shadow(AdjustmentMaintenanceGapReceipt),
    Revision(AdjustmentRevisionGapReceipt),
}

impl PermanentArchiveGapInput<'_> {
    /// Require the exact scheduler or revision identity before performing durable work.
    pub(crate) fn validated_kind(self) -> SpoolResult<PermanentArchiveGapKind> {
        match self {
            Self::Shadow(input) => {
                // preserve the exact frozen scheduler grammar and digest
                if !valid_shadow_due_key(&input.dueKey) || !is_sha256(&input.registrationSha256) {
                    return Err(RevisionOnlineSpoolError::Input);
                }
                Ok(PermanentArchiveGapKind::Shadow)
            }
            Self::Revision(input) => {
                let projection = optional_digest(&input.projectionIdentitySha256);
                // keep both nullable body hashes together under the same canonical identity
                if !is_sha256(&input.logicalKeySha256)
                    || projection != optional_digest(&input.projectionSha256)
                    || projection.is_some_and(
                        // admit the exact lowercase body digest when present
                        |digest| !is_sha256(digest),
                    )
                {
                    return Err(RevisionOnlineSpoolError::Input);
                }
                Ok(PermanentArchiveGapKind::Revision)
            }
        }
    }

    /// Return only the original canonical classification that matches the same input.
    fn retain_existing(
        self,
        codec: &impl PermanentArchiveGapCodec,
        bytes: &[u8],
    ) -> SpoolResult<RetainedGap> {
        // keep the actual decoder behind the ordinary metadata envelope
        if bytes.len() > MAXIMUM_BYTES {
            return Err(RevisionOnlineSpoolError::Input);
        }
        match self {
            Self::Shadow(input) => codec
                .accepted_shadow_gap(bytes)
                .filter(
                    // compare the complete categorical input, not just the filename key
                    |retained| &retained.input == input,
                )
                .map(RetainedGap::Shadow)
                .ok_or(RevisionOnlineSpoolError::IdentityConflict),
            Self::Revision(input) => codec
                .accepted_revision_gap(bytes)
                .filter(
                    // do not relabel an already permanent disposition
                    |retained| &retained.input == input,
                )
                .map(RetainedGap::Revision)
                .ok_or(RevisionOnlineSpoolError::IdentityConflict),
        }
    }

    /// Produce the requested transport shape only after successful immutable publication.
    fn build_receipt(
        self,
        kind: PermanentArchiveGapKind,
        recorded_at: String,
        digest: String,
    ) -> RetainedGap {
        let contract_version = kind.contract().to_owned();
        let qualification_disposition = DISPOSITION.to_owned();
        match self {
            Self::Shadow(input) => RetainedGap::Shadow(AdjustmentMaintenanceGapReceipt {
                contract_version,
                input: input.clone(),
                qualification_disposition,
                recorded_at,
                gap_sha256: digest,
            }),
            Self::Revision(input) => RetainedGap::Revision(AdjustmentRevisionGapReceipt {
                contract_version,
                input: input.clone(),
                qualification_disposition,
                recorded_at,
                gap_sha256: digest,
            }),
        }
    }
}

/// Keep each canonical encoder result inside the small source metadata envelope.
pub(crate) fn bounded_gap_bytes(bytes: Option<Vec<u8>>) -> SpoolResult<Vec<u8>> {
    bytes
        .filter(
            // require one complete bounded output before hashing or writing
            |value| !value.is_empty() && value.len() <= MAXIMUM_BYTES,
        )
        .ok_or(RevisionOnlineSpoolError::Input)
}

/// Verify the source regex's exact UTC schedule syntax without inventing calendar validation.
fn valid_shadow_due_key(value: &str) -> bool {
    let Some(time) = value.strip_prefix("capture/") else {
        return false;
    };
    let bytes = time.as_bytes();
    // accept only the four retained hourly slots and exact scheduled minute
    if bytes.len() != 24
        || !matches!(&bytes[11..13], b"00" | b"06" | b"12" | b"18")
        || &bytes[13..] != b":35:00.000Z"
    {
        return false;
    }
    bytes[..11].iter().zip(b"dddd-dd-ddT").all(
        // enforce the exact date prefix alphabet as Node's scheduler regex
        |(value, expected)| {
            if *expected == b'd' {
                value.is_ascii_digit()
            } else {
                value == expected
            }
        },
    )
}

/// Translate a generated nullable fixed-width digest without copying.
fn optional_digest(value: &Nullable<String>) -> Option<&str> {
    match value {
        Nullable::Null => None,
        Nullable::Value(value) => Some(value.as_str()),
    }
}
