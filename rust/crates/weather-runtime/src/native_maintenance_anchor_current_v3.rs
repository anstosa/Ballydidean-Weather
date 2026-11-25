//! Exact future-only maintenance anchor current read over already verified seal authority.

use std::fmt::{Display, Formatter};

use weather_api::CurrentTimeResolver;
use weather_contracts::JsonValue;

use crate::{
    MaintenanceAnchorSlot, MaintenanceAnchorStatusFiles, MaintenanceAnchorStatusJson,
    NativeFutureInputSealCurrent,
    maintenance_anchor_status::clock_field,
    maintenance_anchor_status_v3::validate_v3_anchor,
    maintenance_canonical_json::{field, text},
};

const COMMAND: &str = "maintenance-anchor-current-v3";
const ACTIVE_CONTROL: &str = "14";
const SLOT_NAMES: [&str; 3] = ["current.json", "pending.json", "previous.json"];
const SEAL_BINDINGS: [&str; 20] = [
    "archiveCommitOrdinal",
    "burnSha256",
    "candidateArtifactRootSha256",
    "candidateReportSha256",
    "captureEpochWitnessSha256",
    "confirmationAccessSha256",
    "confirmationChunkCount",
    "custodyCheckpointSha256",
    "dueKey",
    "family",
    "frontierSha256",
    "fullMemberRootSha256",
    "graphManifestSha256",
    "lifecycleLedgerRootSha256",
    "pageSha256",
    "policyReportSha256",
    "requiredInputRootSha256",
    "sequence",
    "sourceCommit",
    "workstationJournalHeadSha256",
];

/// Preserve the owning read-only command failure without exposing custody material.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMaintenanceAnchorCurrentV3Error {
    Arguments,
    Seal,
    Root,
    UnknownEntry,
    Slot,
    Control,
    SealBinding,
    Codec,
}

impl Display for NativeMaintenanceAnchorCurrentV3Error {
    /// Render the original stage-specific diagnostic.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Arguments => "maintenance-anchor-current-v3 takes no arguments",
            Self::Seal => "future-only maintenance anchor active seal unavailable",
            Self::Root => "maintenance anchor root is unsafe",
            Self::UnknownEntry => "maintenance anchor root contains an unknown entry",
            Self::Slot => "maintenance anchor slot is invalid",
            Self::Control => "future-only maintenance anchor active control differs",
            Self::SealBinding => "future-only maintenance anchor current seal differs",
            Self::Codec => "future-only maintenance anchor current serialization failed",
        })
    }
}

impl std::error::Error for NativeMaintenanceAnchorCurrentV3Error {}

/// Exact active control values selected independently of the observed anchor.
#[derive(Clone, Copy)]
pub struct NativeMaintenanceAnchorCurrentV3Control<'a> {
    pub version: Option<&'a str>,
    pub sha256: Option<&'a str>,
}

/// Execute the original command after acquiring the actual verified seal through its owner.
pub fn read_maintenance_anchor_current_v3_command<Codec, Time, SealError>(
    action: &str,
    arguments: &[&str],
    files: &mut impl MaintenanceAnchorStatusFiles,
    codec: &mut Codec,
    time: &Time,
    control: NativeMaintenanceAnchorCurrentV3Control<'_>,
    read_verified_seal: impl FnOnce(
        &mut Codec,
        &Time,
    ) -> Result<Option<NativeFutureInputSealCurrent>, SealError>,
) -> Result<Option<Vec<u8>>, NativeMaintenanceAnchorCurrentV3Error>
where
    Codec: MaintenanceAnchorStatusJson,
    Time: CurrentTimeResolver,
{
    use NativeMaintenanceAnchorCurrentV3Error as Error;
    // preserve unrelated commands and reject operands before observing either custody owner
    if action != COMMAND {
        return Ok(None);
    }
    if !arguments.is_empty() {
        return Err(Error::Arguments);
    }
    // the original always verifies the current seal before observing anchor genesis absence
    let seal = read_verified_seal(codec, time).map_err(|_| Error::Seal)?;
    let value = read_current(files, codec, time, control, seal)?;
    Ok(Some(
        codec.encode_canonical_json(&value).ok_or(Error::Codec)?,
    ))
}

/// Observe a single admitted current slot without rotating or mutating its retained state.
fn read_current(
    files: &mut impl MaintenanceAnchorStatusFiles,
    codec: &mut impl MaintenanceAnchorStatusJson,
    time: &dyn CurrentTimeResolver,
    control: NativeMaintenanceAnchorCurrentV3Control<'_>,
    seal: Option<NativeFutureInputSealCurrent>,
) -> Result<JsonValue, NativeMaintenanceAnchorCurrentV3Error> {
    use NativeMaintenanceAnchorCurrentV3Error as Error;
    let Some(names) = files.read_root().map_err(|_| Error::Root)? else {
        return Ok(JsonValue::Null);
    };
    // only the source's three transaction slots may exist in the admitted anchor root
    if names
        .iter()
        .any(|name| !SLOT_NAMES.contains(&name.as_str()))
    {
        return Err(Error::UnknownEntry);
    }
    let Some(bytes) = files
        .read_slot(MaintenanceAnchorSlot::Current)
        .map_err(|_| Error::Slot)?
    else {
        return Ok(JsonValue::Null);
    };
    let anchor = codec.parse_canonical_object(&bytes).ok_or(Error::Slot)?;
    // bind only the already validated v3 contract to the exact active v14 control
    if validate_v3_anchor(&anchor, codec, time).is_none() {
        return Err(Error::Slot);
    }
    if control.version != Some(ACTIVE_CONTROL)
        || text(&anchor, "controlVersion").as_deref() != control.version
        || control.sha256.is_none()
        || text(&anchor, "controlSha256").as_deref() != control.sha256
    {
        return Err(Error::Control);
    }
    let seal = seal.ok_or(Error::SealBinding)?;
    // compare every copied scalar identity and the exact immutable current seal digest
    if text(&anchor, "inputSealSha256").as_deref() != Some(seal.seal_sha256.as_str())
        || !SEAL_BINDINGS.iter().all(|key| {
            field(&anchor, key).is_some() && field(&anchor, key) == field(&seal.seal, key)
        })
        || clock_field(&anchor, "fullGraphVerifiedAt", time).ok_or(Error::SealBinding)?
            < clock_field(&seal.seal, "sealedAt", time).ok_or(Error::SealBinding)?
    {
        return Err(Error::SealBinding);
    }
    let digest = codec.sha256(&bytes).ok_or(Error::Codec)?;
    Ok(JsonValue::Object(vec![
        ("anchor".encode_utf16().collect(), anchor),
        (
            "anchorSha256".encode_utf16().collect(),
            JsonValue::String(digest.as_str().encode_utf16().collect()),
        ),
    ]))
}

#[cfg(test)]
#[path = "native_maintenance_anchor_current_v3_test.rs"]
mod tests;
