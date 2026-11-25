//! Initialize the original create-once future-only capture epoch at the operator boundary.

use std::fmt::{Display, Formatter};
use std::io::Read;

use weather_api::CurrentTimeResolver;
use weather_contracts::{
    AdjustmentRevisionCaptureEpochWitness,
    AdjustmentRevisionCaptureEpochWitnessActivationKindLiteral,
    AdjustmentRevisionCaptureEpochWitnessArchiveCommitOrdinalLiteral,
    AdjustmentRevisionCaptureEpochWitnessContractVersionLiteral, AdjustmentRevisionServingSnapshot,
    JsonValue,
};

use crate::capture_epoch_init_deployment::valid_capture_epoch_target_release;
use crate::capture_epoch_init_envelope::validate_capture_epoch_database_envelope;
use crate::capture_epoch_operator::verify_snapshot;
use crate::capture_epoch_witness::{
    GENESIS_FRONTIER_SHA256, verify_operator_capture_epoch_witness,
};
use crate::{CaptureEpochInitDeployment, CaptureEpochOperatorFile, MaintenanceCanonicalJson};

const MAXIMUM_DATABASE_INPUT_BYTES: usize = 64 * 1024;

/// Contain original operator failures before an authoritative witness is emitted.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CaptureEpochInitError {
    Arguments,
    Root,
    DatabaseInput,
    Ledger,
    Transaction,
    Snapshot,
    Deployment,
    ColdArchive,
    File,
    Witness,
    Mismatch,
    Collision,
    MissingSnapshot,
}

impl Display for CaptureEpochInitError {
    /// Report bounded operator diagnostics without propagating input or credential text.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Arguments | Self::Root => {
                "revision-capture-epoch-init-v1 requires root and TARGET_RELEASE"
            }
            Self::DatabaseInput => "adjustment revision capture epoch database envelope is invalid",
            Self::Ledger => "adjustment revision capture epoch database ledger differs",
            Self::Transaction => "adjustment revision capture epoch transaction differs",
            Self::Snapshot => "adjustment revision capture epoch is not zero frontier",
            Self::Deployment | Self::Mismatch => {
                "adjustment revision capture epoch deployment differs"
            }
            Self::ColdArchive => "adjustment revision cold archive initialization failed",
            Self::File => "adjustment revision capture epoch file is invalid",
            Self::Witness => "adjustment revision capture epoch witness is invalid",
            Self::Collision => "adjustment revision capture epoch collision",
            Self::MissingSnapshot => {
                "adjustment revision capture epoch zero snapshot is unavailable"
            }
        })
    }
}

impl std::error::Error for CaptureEpochInitError {}

/// Retain the selected JSON/SHA owner without requiring canonical stdin bytes.
pub trait CaptureEpochInitJson: MaintenanceCanonicalJson {
    /// Decode one bounded Node JSON.parse database envelope before exact field admission.
    fn parse_database_envelope(&mut self, bytes: &[u8]) -> Option<JsonValue>;
}

/// Existing host authority, fixed file and target-proof owners required by the init command.
pub trait CaptureEpochInitPort {
    /// Admit the original effective root-only operator authority before reading input.
    fn require_root(&mut self) -> Result<(), CaptureEpochInitError>;

    /// Derive the immutable target from its independently admitted release environment/proof.
    fn resolve_deployment(
        &mut self,
        target: &str,
    ) -> Result<CaptureEpochInitDeployment, CaptureEpochInitError>;

    /// Prepare exactly the retained cold archive directories before making a witness visible.
    fn initialize_cold_archive(&mut self) -> Result<(), CaptureEpochInitError>;

    /// Expose absence separately from invalid installed authority bytes.
    fn read_optional(
        &mut self,
        artifact: CaptureEpochOperatorFile,
    ) -> Result<Option<Vec<u8>>, CaptureEpochInitError>;

    /// Create one fixed authority file, sync it and its parent; an existing file is not replaced.
    fn create_once(
        &mut self,
        artifact: CaptureEpochOperatorFile,
        bytes: &[u8],
    ) -> Result<(), CaptureEpochInitError>;
}

/// Execute only the original init verb, retaining the first snapshot on retry or interruption.
pub fn initialize_capture_epoch_operator_command(
    action: &str,
    arguments: &[&str],
    database_input: impl Read,
    port: &mut impl CaptureEpochInitPort,
    codec: &mut impl CaptureEpochInitJson,
    time: &dyn CurrentTimeResolver,
) -> Result<Option<Vec<u8>>, CaptureEpochInitError> {
    use CaptureEpochInitError as Error;
    use CaptureEpochOperatorFile::{Snapshot, Witness};

    // let the original read and other retained command owners handle all other verbs
    if action != "revision-capture-epoch-init-v1" {
        return Ok(None);
    }
    let [target] = arguments else {
        return Err(Error::Arguments);
    };
    port.require_root()?;
    // admit the exact bounded database envelope before resolving an installed target
    let mut database_bytes = Vec::new();
    database_input
        .take(MAXIMUM_DATABASE_INPUT_BYTES as u64 + 1)
        .read_to_end(&mut database_bytes)
        .map_err(|_| Error::DatabaseInput)?;
    // keep the original sixty-four-kibibyte input ceiling before parsing
    if database_bytes.is_empty() || database_bytes.len() > MAXIMUM_DATABASE_INPUT_BYTES {
        return Err(Error::DatabaseInput);
    }
    let value = codec
        .parse_database_envelope(&database_bytes)
        .ok_or(Error::DatabaseInput)?;
    let database = validate_capture_epoch_database_envelope(&value, codec, time)?;
    // require the original immutable target grammar before consulting its environment
    if !valid_capture_epoch_target_release(target) {
        return Err(Error::Deployment);
    }
    let deployment = port.resolve_deployment(target)?;
    deployment.validate()?;
    // bind the exact requested release rather than accepting a separately valid release
    if deployment.source_release != *target {
        return Err(Error::Deployment);
    }
    port.initialize_cold_archive()?;

    // match original read order even when a first witness is already present
    let existing = port
        .read_optional(Witness)?
        .map(|bytes| read_witness(bytes, codec, time))
        .transpose()?;
    let retained_snapshot = port
        .read_optional(Snapshot)?
        .map(|bytes| verify_snapshot(&bytes, codec, time).map_err(|_| Error::Snapshot))
        .transpose()?;

    // converge an exact retry on the first durable server clock and paired snapshot
    if let Some((witness, bytes)) = existing {
        let matches = retained_snapshot.as_ref().is_some_and(|snapshot| {
            deployment_matches(&witness, &deployment) && snapshot_matches(&witness, snapshot)
        });
        return if matches {
            Ok(Some(bytes))
        } else {
            Err(Error::Mismatch)
        };
    }
    let snapshot = match retained_snapshot {
        Some(snapshot) => snapshot,
        // persist the zero snapshot first and reread the actual retained winner
        None => {
            let (_, bytes) = database.serving_snapshot.ok_or(Error::MissingSnapshot)?;
            port.create_once(Snapshot, &bytes)?;
            let retained = port.read_optional(Snapshot)?.ok_or(Error::File)?;
            verify_snapshot(&retained, codec, time).map_err(|_| Error::Snapshot)?
        }
    };
    let unsigned =
        unsigned_capture_epoch(&deployment, &database.migration_history_sha256, &snapshot);
    let preimage = codec
        .encode_canonical_json(&unsigned)
        .ok_or(Error::Witness)?;
    let digest = codec.sha256(&preimage).ok_or(Error::Witness)?;
    let JsonValue::Object(mut fields) = unsigned else {
        return Err(Error::Witness);
    };
    fields.push(entry("witnessSha256", digest.as_str()));
    let bytes = codec
        .encode_canonical_json(&JsonValue::Object(fields))
        .ok_or(Error::Witness)?;
    // verify the fully formed witness through the same original operator reader before persisting
    verify_operator_capture_epoch_witness(&bytes, codec, time).map_err(|_| Error::Witness)?;
    port.create_once(Witness, &bytes)?;
    let durable = port.read_optional(Witness)?.ok_or(Error::File)?;
    let (witness, durable) = read_witness(durable, codec, time)?;
    // retain a racing first witness only when both immutable identity and snapshot agree
    if !deployment_matches(&witness, &deployment) || !snapshot_matches(&witness, &snapshot) {
        return Err(Error::Collision);
    }
    Ok(Some(durable))
}

// preserve exact persisted witness bytes alongside their typed admission
fn read_witness(
    bytes: Vec<u8>,
    codec: &mut impl MaintenanceCanonicalJson,
    time: &dyn CurrentTimeResolver,
) -> Result<(AdjustmentRevisionCaptureEpochWitness, Vec<u8>), CaptureEpochInitError> {
    let witness = verify_operator_capture_epoch_witness(&bytes, codec, time)
        .map_err(|_| CaptureEpochInitError::Witness)?
        .witness;
    Ok((witness, bytes))
}

// compare only the six original immutable activation identity fields
fn deployment_matches(
    witness: &AdjustmentRevisionCaptureEpochWitness,
    deployment: &CaptureEpochInitDeployment,
) -> bool {
    witness.controlPlaneSha256 == deployment.control_plane_sha256
        && witness.controlPlaneVersion == match deployment.control_plane_version.as_str() {
            "14" => weather_contracts::AdjustmentRevisionCaptureEpochWitnessControlPlaneVersionEnum::Value14,
            "17" => weather_contracts::AdjustmentRevisionCaptureEpochWitnessControlPlaneVersionEnum::Value17,
            _ => return false,
        }
        && witness.sourceCommit == deployment.source_commit
        && witness.sourceRelease == deployment.source_release
        && witness.sourceServerImageDigest == deployment.source_server_image_digest
        && witness.sourceWebImageDigest == deployment.source_web_image_digest
}

// bind both the immutable snapshot hash and the actual starting database clock
fn snapshot_matches(
    witness: &AdjustmentRevisionCaptureEpochWitness,
    snapshot: &AdjustmentRevisionServingSnapshot,
) -> bool {
    witness.servingSnapshotSha256 == snapshot.snapshotSha256 && witness.epochAt == snapshot.cutoffAt
}

// construct the exact unsigned sorted property set from the retained genesis snapshot
fn unsigned_capture_epoch(
    deployment: &CaptureEpochInitDeployment,
    database_history: &str,
    snapshot: &AdjustmentRevisionServingSnapshot,
) -> JsonValue {
    JsonValue::Object(vec![
        entry(
            "activationKind",
            AdjustmentRevisionCaptureEpochWitnessActivationKindLiteral::WIRE_VALUE,
        ),
        entry(
            "archiveCommitOrdinal",
            AdjustmentRevisionCaptureEpochWitnessArchiveCommitOrdinalLiteral::WIRE_VALUE,
        ),
        entry("catalogFrontierSha256", GENESIS_FRONTIER_SHA256),
        entry(
            "contractVersion",
            AdjustmentRevisionCaptureEpochWitnessContractVersionLiteral::WIRE_VALUE,
        ),
        entry("controlPlaneSha256", &deployment.control_plane_sha256),
        entry("controlPlaneVersion", &deployment.control_plane_version),
        entry("databaseMigrationHistorySha256", database_history),
        entry("epochAt", &snapshot.cutoffAt),
        entry("servingSnapshotSha256", &snapshot.snapshotSha256),
        entry("sourceCommit", &deployment.source_commit),
        entry("sourceRelease", &deployment.source_release),
        entry(
            "sourceServerImageDigest",
            &deployment.source_server_image_digest,
        ),
        entry("sourceWebImageDigest", &deployment.source_web_image_digest),
    ])
}

// retain all original scalar fields as generated JSON UTF-16 strings
fn entry(key: &str, value: &str) -> (Vec<u16>, JsonValue) {
    (
        key.encode_utf16().collect(),
        JsonValue::String(value.encode_utf16().collect()),
    )
}
