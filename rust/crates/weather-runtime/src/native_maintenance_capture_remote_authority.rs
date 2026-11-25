//! Forced remote source authorities used by native maintenance capture readiness.

use std::fmt::{self, Display, Formatter};

use weather_api::CurrentTimeResolver;
use weather_contracts::{
    AdjustmentRevisionCaptureEpochWitnessControlPlaneVersionEnum, JsDateOrString, JsonValue,
};
use weather_edge::ArchiveWirePrimitives;

use crate::capture_epoch_operator::verify_snapshot;
use crate::capture_epoch_witness::verify_operator_capture_epoch_witness;
use crate::maintenance_canonical_json::{exact_keys, field, text, valid_hash};
use crate::native_maintenance_capture_readiness::{
    NativeMaintenanceCaptureFamily, NativeMaintenanceCaptureSourceAuthority,
    VerifiedNativeMaintenanceCaptureEpochSnapshot, VerifiedNativeMaintenanceCaptureEpochWitness,
    VerifiedNativeMaintenanceFamilyCurrent, VerifiedNativeMaintenanceFamilyLineage,
};
use crate::{MaintenanceCanonicalJson, NativeWirePrimitives};

const REMOTE_RESPONSE_MAXIMUM_BYTES: usize = 4_832 * 1_024 + 1;
const FAMILY_CURRENT_KEYS: &[&str] = &[
    "activeInstalledReceiptSha256",
    "catalogSha256",
    "commit",
    "contractVersion",
    "controlInstalledReceiptSha256",
    "family",
    "release",
    "settingsSha256",
    "shadowInstalledReceiptSha256",
    "sourceServerImageDigest",
];
const FAMILY_LINEAGE_KEYS: &[&str] = &[
    "contractVersion",
    "controlSha256",
    "controlVersion",
    "currentActionSha256",
    "currentCommit",
    "currentRelease",
    "epochAncestorCommit",
    "epochWitnessSha256",
    "state",
    "verifiedAt",
];
const EMPTY_ARGUMENTS: &[&str] = &[];
const TEMPERATURE_ARGUMENT: &[&str] = &["temperature"];
const WIND_ARGUMENT: &[&str] = &["wind"];
const RAIN_ARGUMENT: &[&str] = &["rain"];

/// Every fixed forced-account read admitted by the capture-readiness boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMaintenanceCaptureRemoteOperation {
    FamilyCurrent(NativeMaintenanceCaptureFamily),
    FamilyLineage,
    CaptureEpochWitness,
    CaptureEpochSnapshot,
}

impl NativeMaintenanceCaptureRemoteOperation {
    /// Return the exact forced SSH verb selected by the predecessor controller.
    pub const fn verb(self) -> &'static str {
        // keep the operation set closed with no caller-selected command fallback
        match self {
            Self::FamilyCurrent(_) => "adjustment-family-release-current-v1",
            Self::FamilyLineage => "adjustment-family-release-lineage-v2",
            Self::CaptureEpochWitness => "adjustment-revision-capture-epoch-v1",
            Self::CaptureEpochSnapshot => "adjustment-revision-capture-epoch-snapshot-v1",
        }
    }

    /// Return only the fixed family operand required by the current-release verb.
    pub const fn arguments(self) -> &'static [&'static str] {
        // prohibit operands on every root-authenticated common document
        match self {
            Self::FamilyCurrent(NativeMaintenanceCaptureFamily::Temperature) => {
                TEMPERATURE_ARGUMENT
            }
            Self::FamilyCurrent(NativeMaintenanceCaptureFamily::Wind) => WIND_ARGUMENT,
            Self::FamilyCurrent(NativeMaintenanceCaptureFamily::Rain) => RAIN_ARGUMENT,
            Self::FamilyLineage | Self::CaptureEpochWitness | Self::CaptureEpochSnapshot => {
                EMPTY_ARGUMENTS
            }
        }
    }
}

/// Explicit forced-command byte reader shared with the installed SSH transport owner.
pub trait NativeMaintenanceCaptureRemoteCommandPort {
    type Error;

    /// Read one operation's already bounded stdout without selecting a fallback verb.
    fn read_forced(
        &mut self,
        operation: NativeMaintenanceCaptureRemoteOperation,
    ) -> Result<Vec<u8>, Self::Error>;
}

/// One transport or exact response-admission failure.
#[derive(Debug)]
pub enum NativeMaintenanceCaptureRemoteAuthorityError<Transport> {
    Transport(Transport),
    InvalidResponse,
}

impl<Transport: Display> Display for NativeMaintenanceCaptureRemoteAuthorityError<Transport> {
    /// Keep semantic refusal path-free while retaining the transport owner's diagnostic.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        // distinguish transport failure from a noncanonical or invalid producer document
        match self {
            Self::Transport(error) => Display::fmt(error, formatter),
            Self::InvalidResponse => formatter.write_str("capture remote response is invalid"),
        }
    }
}

impl<Transport: std::error::Error + 'static> std::error::Error
    for NativeMaintenanceCaptureRemoteAuthorityError<Transport>
{
    /// Preserve only an actual transport cause.
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        // semantic response rejection has no lower-level cause
        match self {
            Self::Transport(error) => Some(error),
            Self::InvalidResponse => None,
        }
    }
}

/// Borrowed authority that validates forced responses before readiness projection.
pub struct NativeMaintenanceCaptureRemoteAuthority<'a, Command> {
    command: &'a mut Command,
}

impl<'a, Command> NativeMaintenanceCaptureRemoteAuthority<'a, Command> {
    /// Bind one explicit command port without executing a remote operation.
    pub const fn new(command: &'a mut Command) -> Self {
        Self { command }
    }
}

impl<Command: NativeMaintenanceCaptureRemoteCommandPort>
    NativeMaintenanceCaptureRemoteAuthority<'_, Command>
{
    /// Read the full root-authenticated current family release for native maintenance actions.
    pub fn read_verified_family_current_full(
        &mut self,
        family: NativeMaintenanceCaptureFamily,
    ) -> Result<
        crate::NativeFamilyReleaseCurrent,
        NativeMaintenanceCaptureRemoteAuthorityError<Command::Error>,
    > {
        use NativeMaintenanceCaptureRemoteAuthorityError as Error;
        let bytes = self.read(NativeMaintenanceCaptureRemoteOperation::FamilyCurrent(
            family,
        ))?;
        let value = canonical_response(&bytes)?;
        let projected = project_family_current(&value, family).ok_or(Error::InvalidResponse)?;
        // retain the already validated receipt and root identities beyond readiness's narrow projection
        let nullable = |key: &str| match field(&value, key) {
            Some(JsonValue::Null) => Ok(None),
            Some(JsonValue::String(_)) => text(&value, key).map(Some).ok_or(Error::InvalidResponse),
            _ => Err(Error::InvalidResponse),
        };
        Ok(crate::NativeFamilyReleaseCurrent {
            active_installed_receipt_sha256: nullable("activeInstalledReceiptSha256")?,
            catalog_sha256: text(&value, "catalogSha256").ok_or(Error::InvalidResponse)?,
            commit: projected.commit,
            control_installed_receipt_sha256: nullable("controlInstalledReceiptSha256")?,
            contract_version: text(&value, "contractVersion").ok_or(Error::InvalidResponse)?,
            family: projected.family,
            release: projected.release,
            settings_sha256: text(&value, "settingsSha256").ok_or(Error::InvalidResponse)?,
            shadow_installed_receipt_sha256: nullable("shadowInstalledReceiptSha256")?,
            source_server_image_digest: projected.source_server_image_digest,
        })
    }

    /// Read and project the root-authenticated capture epoch witness.
    pub fn read_verified_capture_epoch_witness(
        &mut self,
    ) -> Result<
        VerifiedNativeMaintenanceCaptureEpochWitness,
        NativeMaintenanceCaptureRemoteAuthorityError<Command::Error>,
    > {
        self.read_verified_capture_epoch_witness_with_bytes()
            .map(|(witness, _)| witness)
    }

    /// Retain the complete canonical source witness alongside the verified projection.
    pub fn read_verified_capture_epoch_witness_with_bytes(
        &mut self,
    ) -> Result<
        (VerifiedNativeMaintenanceCaptureEpochWitness, Vec<u8>),
        NativeMaintenanceCaptureRemoteAuthorityError<Command::Error>,
    > {
        let bytes = self.read(NativeMaintenanceCaptureRemoteOperation::CaptureEpochWitness)?;
        let value = canonical_response(&bytes)?;
        let mut codec = NativeWirePrimitives::new();
        let canonical = codec
            .encode_canonical_json(&value)
            .ok_or(NativeMaintenanceCaptureRemoteAuthorityError::InvalidResponse)?;
        let time = NativeWirePrimitives::new();
        let admitted = verify_operator_capture_epoch_witness(&canonical, &mut codec, &time)
            .map_err(|_| NativeMaintenanceCaptureRemoteAuthorityError::InvalidResponse)?;
        let witness = admitted.witness;
        // retain only the original final-v14 rolling capture authority at this host boundary
        if witness.controlPlaneVersion
            != AdjustmentRevisionCaptureEpochWitnessControlPlaneVersionEnum::Value14
            || witness.databaseMigrationHistorySha256
                != "6de5c8c7efaa448aeb12bf1a9debe6fe7d4d4d1003ee0e21ab619ffa624c3424"
        {
            return Err(NativeMaintenanceCaptureRemoteAuthorityError::InvalidResponse);
        }
        // project the generated closed enum back to its exact readiness wire spelling
        let control_plane_version = match witness.controlPlaneVersion {
            AdjustmentRevisionCaptureEpochWitnessControlPlaneVersionEnum::Value14 => "14",
            AdjustmentRevisionCaptureEpochWitnessControlPlaneVersionEnum::Value17 => "17",
        };
        Ok((
            VerifiedNativeMaintenanceCaptureEpochWitness {
                catalog_frontier_sha256: witness.catalogFrontierSha256,
                control_plane_sha256: witness.controlPlaneSha256,
                control_plane_version: control_plane_version.to_owned(),
                database_migration_history_sha256: witness.databaseMigrationHistorySha256,
                epoch_at: witness.epochAt,
                serving_snapshot_sha256: witness.servingSnapshotSha256,
                source_commit: witness.sourceCommit,
                source_release: witness.sourceRelease,
                source_server_image_digest: witness.sourceServerImageDigest,
                witness_sha256: witness.witnessSha256,
            },
            canonical,
        ))
    }

    /// Read and project the exact retained zero-frontier epoch snapshot.
    pub fn read_verified_capture_epoch_snapshot(
        &mut self,
    ) -> Result<
        VerifiedNativeMaintenanceCaptureEpochSnapshot,
        NativeMaintenanceCaptureRemoteAuthorityError<Command::Error>,
    > {
        self.read_verified_capture_epoch_snapshot_with_bytes()
            .map(|(snapshot, _)| snapshot)
    }

    /// Retain the complete canonical source snapshot alongside the verified projection.
    pub fn read_verified_capture_epoch_snapshot_with_bytes(
        &mut self,
    ) -> Result<
        (VerifiedNativeMaintenanceCaptureEpochSnapshot, Vec<u8>),
        NativeMaintenanceCaptureRemoteAuthorityError<Command::Error>,
    > {
        let bytes = self.read(NativeMaintenanceCaptureRemoteOperation::CaptureEpochSnapshot)?;
        let value = canonical_response(&bytes)?;
        let mut codec = NativeWirePrimitives::new();
        let canonical = codec
            .encode_canonical_json(&value)
            .ok_or(NativeMaintenanceCaptureRemoteAuthorityError::InvalidResponse)?;
        let time = NativeWirePrimitives::new();
        let snapshot = verify_snapshot(&canonical, &mut codec, &time)
            .map_err(|_| NativeMaintenanceCaptureRemoteAuthorityError::InvalidResponse)?;
        Ok((
            VerifiedNativeMaintenanceCaptureEpochSnapshot {
                archive_commit_ordinal: snapshot.archiveCommitOrdinal,
                cutoff_at: snapshot.cutoffAt,
                entry_count: snapshot.entryCount,
                entries_empty: snapshot.entries.is_empty(),
                frontier_sha256: snapshot.frontierSha256,
                snapshot_sha256: snapshot.snapshotSha256,
            },
            canonical,
        ))
    }

    /// Read one bounded response through the typed fixed operation port.
    fn read(
        &mut self,
        operation: NativeMaintenanceCaptureRemoteOperation,
    ) -> Result<Vec<u8>, NativeMaintenanceCaptureRemoteAuthorityError<Command::Error>> {
        self.command
            .read_forced(operation)
            .map_err(NativeMaintenanceCaptureRemoteAuthorityError::Transport)
    }
}

impl<Command: NativeMaintenanceCaptureRemoteCommandPort> NativeMaintenanceCaptureSourceAuthority
    for NativeMaintenanceCaptureRemoteAuthority<'_, Command>
{
    type Error = NativeMaintenanceCaptureRemoteAuthorityError<Command::Error>;

    /// Verify one exact canonical current-family source projection.
    fn read_verified_family_current(
        &mut self,
        family: NativeMaintenanceCaptureFamily,
    ) -> Result<VerifiedNativeMaintenanceFamilyCurrent, Self::Error> {
        let bytes = self.read(NativeMaintenanceCaptureRemoteOperation::FamilyCurrent(
            family,
        ))?;
        let value = canonical_response(&bytes)?;
        project_family_current(&value, family)
            .ok_or(NativeMaintenanceCaptureRemoteAuthorityError::InvalidResponse)
    }

    /// Verify the exact canonical transitive lineage projection.
    fn read_verified_family_lineage(
        &mut self,
    ) -> Result<VerifiedNativeMaintenanceFamilyLineage, Self::Error> {
        let bytes = self.read(NativeMaintenanceCaptureRemoteOperation::FamilyLineage)?;
        let value = canonical_response(&bytes)?;
        project_family_lineage(&value)
            .ok_or(NativeMaintenanceCaptureRemoteAuthorityError::InvalidResponse)
    }
}

/// Decode only one nonempty bounded producer-order JSON.stringify line.
fn canonical_response<Transport>(
    bytes: &[u8],
) -> Result<JsonValue, NativeMaintenanceCaptureRemoteAuthorityError<Transport>> {
    // retain the source transport's exact nonempty maximum before JSON allocation
    if bytes.is_empty() || bytes.len() > REMOTE_RESPONSE_MAXIMUM_BYTES {
        return Err(NativeMaintenanceCaptureRemoteAuthorityError::InvalidResponse);
    }
    let codec = NativeWirePrimitives::new();
    let value = codec
        .decode_json(bytes)
        .ok_or(NativeMaintenanceCaptureRemoteAuthorityError::InvalidResponse)?;
    // match the source JSON.stringify framing without demanding sorted key insertion order
    if codec.encode_ordered_json(&value).as_deref() != Some(bytes) {
        return Err(NativeMaintenanceCaptureRemoteAuthorityError::InvalidResponse);
    }
    Ok(value)
}

/// Validate every closed current-authority field before narrowing its projection.
fn project_family_current(
    value: &JsonValue,
    family: NativeMaintenanceCaptureFamily,
) -> Option<VerifiedNativeMaintenanceFamilyCurrent> {
    // require the exact canonical producer object and every receipt/hash spelling
    if !exact_keys(value, FAMILY_CURRENT_KEYS)
        || !required_hash(value, "catalogSha256")
        || !required_hash(value, "settingsSha256")
        || !nullable_hash(value, "activeInstalledReceiptSha256")
        || !nullable_hash(value, "controlInstalledReceiptSha256")
        || !nullable_hash(value, "shadowInstalledReceiptSha256")
        || text(value, "contractVersion").as_deref()
            != Some("adjustment-family-release-current-status/v1")
        || text(value, "family").as_deref() != Some(family.wire())
    {
        return None;
    }
    // only rain can expose the independently installed control slot
    if family != NativeMaintenanceCaptureFamily::Rain
        && !matches!(
            field(value, "controlInstalledReceiptSha256"),
            Some(JsonValue::Null)
        )
    {
        return None;
    }
    let commit = text(value, "commit")?;
    let release = text(value, "release")?;
    let source_server_image_digest = text(value, "sourceServerImageDigest")?;
    // close immutable source identities without accepting normalized alternatives
    if !valid_commit(&commit)
        || !valid_family_release(&release)
        || !valid_image_digest(&source_server_image_digest)
    {
        return None;
    }
    Some(VerifiedNativeMaintenanceFamilyCurrent {
        commit,
        family: family.wire().to_owned(),
        release,
        source_server_image_digest,
    })
}

/// Validate every closed lineage field before narrowing its readiness projection.
fn project_family_lineage(value: &JsonValue) -> Option<VerifiedNativeMaintenanceFamilyLineage> {
    // preserve the reviewed v14 epoch-descendant response and its optional action identity
    if !exact_keys(value, FAMILY_LINEAGE_KEYS)
        || !required_hash(value, "controlSha256")
        || !nullable_hash(value, "currentActionSha256")
        || !required_hash(value, "epochWitnessSha256")
        || text(value, "contractVersion").as_deref() != Some("adjustment-family-release-lineage/v2")
        || text(value, "controlVersion").as_deref() != Some("14")
        || text(value, "state").as_deref() != Some("verified_epoch_descendant")
    {
        return None;
    }
    let current_commit = text(value, "currentCommit")?;
    let current_release = text(value, "currentRelease")?;
    let epoch_ancestor_commit = text(value, "epochAncestorCommit")?;
    let verified_at = text(value, "verifiedAt")?;
    // retain exact commit, release, and canonical millisecond clock grammars
    if !valid_commit(&current_commit)
        || !valid_commit(&epoch_ancestor_commit)
        || !valid_family_release(&current_release)
        || !valid_instant(&verified_at)
    {
        return None;
    }
    Some(VerifiedNativeMaintenanceFamilyLineage {
        contract_version: "adjustment-family-release-lineage/v2".to_owned(),
        control_sha256: text(value, "controlSha256")?,
        control_version: "14".to_owned(),
        current_commit,
        current_release,
        epoch_ancestor_commit,
        epoch_witness_sha256: text(value, "epochWitnessSha256")?,
        state: "verified_epoch_descendant".to_owned(),
        verified_at,
    })
}

/// Require one present lowercase SHA-256 string field.
fn required_hash(value: &JsonValue, key: &str) -> bool {
    text(value, key).is_some_and(|value| valid_hash(&value))
}

/// Require one null or lowercase SHA-256 string field.
fn nullable_hash(value: &JsonValue, key: &str) -> bool {
    // admit only the source's explicit null and exact hash alternatives
    match field(value, key) {
        Some(JsonValue::Null) => true,
        Some(JsonValue::String(units)) => String::from_utf16(units)
            .ok()
            .is_some_and(|value| valid_hash(&value)),
        _ => false,
    }
}

/// Match one immutable lowercase forty-character Git commit.
fn valid_commit(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// Match the controller's one-to-three digit release ordinal grammar.
fn valid_family_release(value: &str) -> bool {
    let bytes = value.as_bytes();
    // preserve fixed punctuation and reject a zero-leading release ordinal
    if !(12..=14).contains(&bytes.len())
        || bytes.get(4) != Some(&b'.')
        || bytes.get(7) != Some(&b'.')
        || bytes.get(10) != Some(&b'-')
        || !bytes[..4].iter().all(u8::is_ascii_digit)
        || !bytes[5..7].iter().all(u8::is_ascii_digit)
        || !bytes[8..10].iter().all(u8::is_ascii_digit)
        || !matches!(bytes.get(11), Some(b'1'..=b'9'))
    {
        return false;
    }
    bytes[12..].iter().all(u8::is_ascii_digit)
}

/// Match the exact immutable OCI digest spelling exposed by the source authority.
fn valid_image_digest(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(valid_hash)
}

/// Resolve one exact ordinary-year UTC millisecond instant without normalization.
fn valid_instant(value: &str) -> bool {
    let bytes = value.as_bytes();
    // preserve the controller's fixed four-digit UTC wire shape before calendar parsing
    if bytes.len() != 24
        || !bytes[..4].iter().all(u8::is_ascii_digit)
        || bytes.get(4) != Some(&b'-')
        || bytes.get(7) != Some(&b'-')
        || bytes.get(10) != Some(&b'T')
        || bytes.get(13) != Some(&b':')
        || bytes.get(16) != Some(&b':')
        || bytes.get(19) != Some(&b'.')
        || bytes.get(23) != Some(&b'Z')
        || ![5..7, 8..10, 11..13, 14..16, 17..19, 20..23]
            .iter()
            .all(|range| bytes[range.clone()].iter().all(u8::is_ascii_digit))
    {
        return false;
    }
    NativeWirePrimitives::new()
        .resolve(&JsDateOrString::String(value.encode_utf16().collect()))
        .is_ok_and(|instant| instant.utc == value)
}

#[cfg(test)]
#[path = "native_maintenance_capture_remote_authority_test.rs"]
mod tests;
