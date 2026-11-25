//! Exact future-only epoch genesis segment and its closed CAS member registry.

use std::fmt::{self, Display, Formatter};

use weather_api::CurrentTimeResolver;
use weather_contracts::{
    AdjustmentRevisionCaptureEpochWitnessContractVersionLiteral,
    AdjustmentRevisionServingSnapshotContractVersionLiteral,
};

use crate::capture_epoch_operator::verify_snapshot;
use crate::capture_epoch_witness::verify_capture_epoch_witness;
use crate::native_maintenance_archive_segment::{
    NativeMaintenanceArchiveSegment, NativeMaintenanceArchiveSegmentMember,
};
use crate::native_maintenance_cas_pack::{
    NativeMaintenanceCasMemberValidator, NativeMaintenanceCasStreamSession,
    NativeMaintenanceCasStreamValidator,
};
use crate::{
    CAPTURE_EPOCH_WITNESS_MAX_BYTES, MaintenanceCanonicalJson, NativeMaintenanceGraphCrossLink,
    NativeWirePrimitives,
};

const WITNESS_KIND: &str = AdjustmentRevisionCaptureEpochWitnessContractVersionLiteral::WIRE_VALUE;
const SNAPSHOT_KIND: &str = AdjustmentRevisionServingSnapshotContractVersionLiteral::WIRE_VALUE;
const ZERO_FRONTIER_RELATION: &str = "binds_zero_frontier_snapshot";

/// Exact source due identity and typed future-only genesis segment.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeMaintenanceArchiveFutureOnlyGenesis {
    pub due_key: String,
    pub segment: NativeMaintenanceArchiveSegment,
}

/// Closed genesis source-byte, identity, and cross-binding failures.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMaintenanceArchiveGenesisError {
    Witness,
    Snapshot,
    Binding,
}

impl Display for NativeMaintenanceArchiveGenesisError {
    /// Render one path-free genesis composition failure category.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        // preserve only the owning validation boundary
        formatter.write_str(match self {
            Self::Witness => "maintenance archive genesis witness is invalid",
            Self::Snapshot => "maintenance archive genesis snapshot is invalid",
            Self::Binding => "maintenance archive genesis witness and snapshot differ",
        })
    }
}

impl std::error::Error for NativeMaintenanceArchiveGenesisError {}

/// Reverify exact retained witness and snapshot bytes and build the source segment.
pub fn build_native_maintenance_archive_future_only_genesis(
    witness_bytes: &[u8],
    snapshot_bytes: &[u8],
    codec: &mut impl MaintenanceCanonicalJson,
    time: &dyn CurrentTimeResolver,
) -> Result<NativeMaintenanceArchiveFutureOnlyGenesis, NativeMaintenanceArchiveGenesisError> {
    use NativeMaintenanceArchiveGenesisError as Error;

    let witness = verify_capture_epoch_witness(witness_bytes, codec, time)
        .map_err(|_| Error::Witness)?
        .witness;
    let snapshot = verify_snapshot(snapshot_bytes, codec, time).map_err(|_| Error::Snapshot)?;
    // require the exact retained zero-frontier snapshot sealed by this witness
    if snapshot.snapshotSha256 != witness.servingSnapshotSha256
        || snapshot.cutoffAt != witness.epochAt
        || snapshot.archiveCommitOrdinal != "0"
        || snapshot.frontierSha256 != witness.catalogFrontierSha256
    {
        return Err(Error::Binding);
    }
    let witness_identity = parse_digest(&witness.witnessSha256).ok_or(Error::Witness)?;
    let snapshot_identity = parse_digest(&snapshot.snapshotSha256).ok_or(Error::Snapshot)?;
    let due_key = format!("archive/future-only-genesis/{}", witness.witnessSha256);
    Ok(NativeMaintenanceArchiveFutureOnlyGenesis {
        due_key,
        segment: NativeMaintenanceArchiveSegment {
            members: vec![
                NativeMaintenanceArchiveSegmentMember {
                    identity_sha256: witness_identity,
                    kind: WITNESS_KIND.to_owned(),
                    payload: witness_bytes.to_vec(),
                },
                NativeMaintenanceArchiveSegmentMember {
                    identity_sha256: snapshot_identity,
                    kind: SNAPSHOT_KIND.to_owned(),
                    payload: snapshot_bytes.to_vec(),
                },
            ],
            cross_links: vec![NativeMaintenanceGraphCrossLink {
                from_identity_sha256: witness.witnessSha256,
                relation: ZERO_FRONTIER_RELATION.to_owned(),
                to_identity_sha256: snapshot.snapshotSha256,
            }],
        },
    })
}

/// Closed production registry for the two exact future-only genesis schemas.
#[derive(Clone, Copy, Debug, Default)]
pub struct NativeMaintenanceArchiveGenesisValidators;

impl NativeMaintenanceArchiveGenesisValidators {
    /// Construct the value-free exact genesis registry.
    pub const fn new() -> Self {
        Self
    }
}

impl NativeMaintenanceCasMemberValidator for NativeMaintenanceArchiveGenesisValidators {
    /// Validate only one exact canonical witness or zero-frontier snapshot body.
    fn validate(&mut self, kind: &str, payload: &[u8]) -> bool {
        validate_genesis_member(kind, payload)
    }
}

impl NativeMaintenanceCasStreamValidator for NativeMaintenanceArchiveGenesisValidators {
    type Session = NativeMaintenanceArchiveGenesisValidationSession;

    /// Open one bounded validator only for a registered genesis member kind.
    fn begin(&mut self, kind: &str, length: u64) -> Option<Self::Session> {
        let kind = match kind {
            WITNESS_KIND => NativeMaintenanceArchiveGenesisMemberKind::Witness,
            SNAPSHOT_KIND => NativeMaintenanceArchiveGenesisMemberKind::Snapshot,
            _ => return None,
        };
        // refuse advertised bodies beyond the shared retained authority ceiling
        if length > CAPTURE_EPOCH_WITNESS_MAX_BYTES as u64 {
            return None;
        }
        Some(NativeMaintenanceArchiveGenesisValidationSession {
            bytes: Vec::with_capacity(usize::try_from(length).ok()?),
            kind,
        })
    }
}

/// One bounded retained member session used by full-graph streaming verification.
pub struct NativeMaintenanceArchiveGenesisValidationSession {
    bytes: Vec<u8>,
    kind: NativeMaintenanceArchiveGenesisMemberKind,
}

impl NativeMaintenanceCasStreamSession for NativeMaintenanceArchiveGenesisValidationSession {
    /// Retain no more than the original authority-file byte ceiling.
    fn write(&mut self, bytes: &[u8]) -> bool {
        let Some(next) = self.bytes.len().checked_add(bytes.len()) else {
            return false;
        };
        // reject overflow before extending the bounded validation buffer
        if next > CAPTURE_EPOCH_WITNESS_MAX_BYTES {
            return false;
        }
        self.bytes.extend_from_slice(bytes);
        true
    }

    /// Reverify the complete exact canonical member through its existing owner.
    fn finish(self) -> bool {
        validate_genesis_member(self.kind.as_str(), &self.bytes)
    }
}

#[derive(Clone, Copy)]
enum NativeMaintenanceArchiveGenesisMemberKind {
    Witness,
    Snapshot,
}

impl NativeMaintenanceArchiveGenesisMemberKind {
    /// Return the exact registered source kind.
    const fn as_str(self) -> &'static str {
        // retain only the two closed future-only genesis schemas
        match self {
            Self::Witness => WITNESS_KIND,
            Self::Snapshot => SNAPSHOT_KIND,
        }
    }
}

/// Invoke the owning canonical parser for one closed registered member kind.
fn validate_genesis_member(kind: &str, payload: &[u8]) -> bool {
    let mut codec = NativeWirePrimitives::new();
    let time = NativeWirePrimitives::new();
    // keep each schema under its existing exact canonical and identity validator
    match kind {
        WITNESS_KIND => verify_capture_epoch_witness(payload, &mut codec, &time).is_ok(),
        SNAPSHOT_KIND => verify_snapshot(payload, &mut codec, &time).is_ok(),
        _ => false,
    }
}

/// Decode one exact lowercase digest without introducing a codec dependency.
fn parse_digest(value: &str) -> Option<[u8; 32]> {
    Some(*weather_edge::RevisionGapSha256::from_lower_hex(value)?.as_bytes())
}
