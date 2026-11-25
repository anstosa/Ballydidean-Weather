//! Native transferred installation for the retained legacy v2 maintenance anchor.

use std::fmt::{Display, Formatter};
use std::io::Read;

use weather_api::CurrentTimeResolver;
use weather_contracts::JsonValue;
use weather_edge::{
    ForecastCycleFinalAckCodec, ForecastCycleFinalManifest, ForecastCycleFinalManifestCodec,
    ForecastCycleSha256, NativeForecastCycleFinalAcknowledgements,
    NativeForecastCycleFinalManifests,
};

use crate::maintenance_anchor_status::{MAXIMUM_ANCHOR_BYTES, MaintenanceAnchorState, clock_field};
use crate::maintenance_anchor_status_v2::validate_v2_anchor;
use crate::maintenance_anchor_status_v3::validate_v3_anchor;
use crate::maintenance_canonical_json::{field, text, valid_hash};
use crate::native_maintenance_anchor_install_v3::{
    NativeMaintenanceAnchorInstallV3Error, NativeMaintenanceAnchorV3InstallFiles,
};
use crate::{MaintenanceAnchorSlot, MaintenanceAnchorStatusJson};

const COMMAND: &str = "maintenance-anchor-install-v2";
const V3_VERSION: &str = "adjustment-maintenance-anchor/v3";

/// Bounded failures from the legacy transferred-anchor installation boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMaintenanceAnchorInstallV2Error {
    Arguments,
    Input,
    Identity,
    Control,
    Transfer,
    Root,
    UnknownEntry,
    Slot,
    PendingCollision,
    Predecessor,
    Codec,
}

impl Display for NativeMaintenanceAnchorInstallV2Error {
    /// Render one stable operator-facing failure category without retained values.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Arguments => "maintenance-anchor-install-v2 requires ANCHOR_SHA256",
            Self::Input => "maintenance-anchor-install-v2 requires a v2 anchor",
            Self::Identity => "maintenance anchor identity is invalid",
            Self::Control => "maintenance anchor control identity is invalid",
            Self::Transfer => "maintenance anchor transfer binding is invalid",
            Self::Root => "maintenance anchor root is unsafe",
            Self::UnknownEntry => "maintenance anchor root contains an unknown entry",
            Self::Slot => "maintenance anchor slot is unsafe",
            Self::PendingCollision => "maintenance anchor pending collision",
            Self::Predecessor => "maintenance anchor predecessor is invalid",
            Self::Codec => "maintenance anchor serialization failed",
        })
    }
}

impl std::error::Error for NativeMaintenanceAnchorInstallV2Error {}

/// Independently selected optional active-control constraints.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NativeMaintenanceAnchorInstallV2Control<'a> {
    pub version: Option<&'a str>,
    pub sha256: Option<&'a str>,
}

/// Fully validated legacy final-manifest and cold-graph transfer projection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeMaintenanceAnchorV2Transfer {
    pub due_key: String,
    pub generation: String,
    pub graph_sha256: String,
    pub acknowledged_at: String,
}

/// Read-only authority for the exact final manifest and its immutable acknowledgement.
pub trait NativeMaintenanceAnchorV2TransferAuthority {
    /// Return the joined final closure for one content-addressed manifest identity.
    fn read_transfer(
        &mut self,
        manifest_sha256: &str,
    ) -> Result<Option<NativeMaintenanceAnchorV2Transfer>, NativeMaintenanceAnchorInstallV2Error>;
}

/// Actual typed native final-manifest and acknowledgement stores supplied by startup.
pub struct NativeMaintenanceAnchorV2NativeTransferAuthority<
    ManifestCodec,
    ManifestStatfs,
    AckCodec,
    AckStatfs,
> {
    manifests: NativeForecastCycleFinalManifests<ManifestCodec, ManifestStatfs>,
    acknowledgements: NativeForecastCycleFinalAcknowledgements<AckCodec, AckStatfs>,
}

impl<ManifestCodec, ManifestStatfs, AckCodec, AckStatfs>
    NativeMaintenanceAnchorV2NativeTransferAuthority<
        ManifestCodec,
        ManifestStatfs,
        AckCodec,
        AckStatfs,
    >
{
    /// Retain already admitted native stores without initializing or creating directories.
    pub const fn new(
        manifests: NativeForecastCycleFinalManifests<ManifestCodec, ManifestStatfs>,
        acknowledgements: NativeForecastCycleFinalAcknowledgements<AckCodec, AckStatfs>,
    ) -> Self {
        Self {
            manifests,
            acknowledgements,
        }
    }
}

impl<ManifestCodec, ManifestStatfs, AckCodec, AckStatfs> NativeMaintenanceAnchorV2TransferAuthority
    for NativeMaintenanceAnchorV2NativeTransferAuthority<
        ManifestCodec,
        ManifestStatfs,
        AckCodec,
        AckStatfs,
    >
where
    ManifestCodec: ForecastCycleFinalManifestCodec<Manifest = ForecastCycleFinalManifest>,
    AckCodec: ForecastCycleFinalAckCodec,
{
    /// Join the exact typed records without adding a second JSON parser or storage owner.
    fn read_transfer(
        &mut self,
        manifest_sha256: &str,
    ) -> Result<Option<NativeMaintenanceAnchorV2Transfer>, NativeMaintenanceAnchorInstallV2Error>
    {
        let digest =
            parse_digest(manifest_sha256).ok_or(NativeMaintenanceAnchorInstallV2Error::Transfer)?;
        let Some(manifest) = self
            .manifests
            .read_final_manifest(digest)
            .map_err(|_| NativeMaintenanceAnchorInstallV2Error::Transfer)?
        else {
            return Ok(None);
        };
        let Some(acknowledgement) = self
            .acknowledgements
            .read_final_acknowledgement(digest)
            .map_err(|_| NativeMaintenanceAnchorInstallV2Error::Transfer)?
        else {
            return Ok(None);
        };
        let document = manifest.manifest.document();
        let due_key =
            text(document, "dueKey").ok_or(NativeMaintenanceAnchorInstallV2Error::Transfer)?;
        let generation =
            text(document, "generation").ok_or(NativeMaintenanceAnchorInstallV2Error::Transfer)?;
        Ok(Some(NativeMaintenanceAnchorV2Transfer {
            due_key,
            generation,
            graph_sha256: lower_hex(acknowledgement.graph_sha256.as_bytes()),
            acknowledged_at: acknowledgement.acknowledged_at,
        }))
    }
}

/// Execute the exact legacy v2 transferred-anchor install verb with no response body.
pub fn install_maintenance_anchor_v2_command(
    command: (&str, &[&str]),
    input: &mut impl Read,
    files: &mut impl NativeMaintenanceAnchorV3InstallFiles,
    transfer_authority: &mut impl NativeMaintenanceAnchorV2TransferAuthority,
    codec: &mut impl MaintenanceAnchorStatusJson,
    time: &dyn CurrentTimeResolver,
    control: NativeMaintenanceAnchorInstallV2Control<'_>,
) -> Result<Option<Vec<u8>>, NativeMaintenanceAnchorInstallV2Error> {
    use NativeMaintenanceAnchorInstallV2Error as Error;

    let (action, arguments) = command;
    // leave every unrelated evidence command to its existing owner
    if action != COMMAND {
        return Ok(None);
    }
    let [expected_sha256] = arguments else {
        return Err(Error::Arguments);
    };
    // reject a malformed identity operand before consuming stdin
    if !valid_hash(expected_sha256) {
        return Err(Error::Arguments);
    }
    let mut bytes = Vec::new();
    input
        .take(MAXIMUM_ANCHOR_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| Error::Input)?;
    // preserve the exact bounded canonical document contract
    if !(2..=MAXIMUM_ANCHOR_BYTES).contains(&bytes.len()) {
        return Err(Error::Input);
    }
    let anchor = parse_transferred_v2(&bytes, codec, time)?;
    let anchor_sha256 = codec.sha256(&bytes).ok_or(Error::Codec)?;
    // bind the operand to the exact canonical transferred payload
    if anchor_sha256.as_str() != *expected_sha256 {
        return Err(Error::Identity);
    }
    // apply only independently configured control values, preserving source optionality
    if control
        .sha256
        .is_some_and(|expected| text(&anchor, "controlSha256").as_deref() != Some(expected))
        || control
            .version
            .is_some_and(|expected| text(&anchor, "controlVersion").as_deref() != Some(expected))
    {
        return Err(Error::Control);
    }
    let manifest_sha256 = text(&anchor, "manifestSha256").ok_or(Error::Input)?;
    let transfer = transfer_authority
        .read_transfer(&manifest_sha256)?
        .ok_or(Error::Transfer)?;
    let verified_at = clock_field(&anchor, "fullGraphVerifiedAt", time).ok_or(Error::Transfer)?;
    let published_at = clock_field(&anchor, "publishedAt", time).ok_or(Error::Transfer)?;
    let acknowledged_at = crate::capture_epoch_witness::resolve_operator_capture_clock(
        &transfer.acknowledged_at,
        time,
    )
    .ok_or(Error::Transfer)?
    .epoch_milliseconds;
    // bind the sealed cycle identity and complete cold-graph transfer chronology
    if text(&anchor, "dueKey").as_deref() != Some(transfer.due_key.as_str())
        || text(&anchor, "generation").as_deref() != Some(transfer.generation.as_str())
        || text(&anchor, "graphManifestSha256").as_deref() != Some(transfer.graph_sha256.as_str())
        || verified_at > acknowledged_at
        || published_at < acknowledged_at
    {
        return Err(Error::Transfer);
    }

    files.ensure_root().map_err(map_shared_error)?;
    require_fixed_inventory(files)?;
    install_anchor(&anchor, &bytes, expected_sha256, files, codec, time)?;
    Ok(Some(Vec::new()))
}

/// Parse one exact canonical transferred v2 anchor and reject every v3 document.
fn parse_transferred_v2(
    bytes: &[u8],
    codec: &mut impl MaintenanceAnchorStatusJson,
    time: &dyn CurrentTimeResolver,
) -> Result<JsonValue, NativeMaintenanceAnchorInstallV2Error> {
    let anchor = codec
        .parse_canonical_object(bytes)
        .ok_or(NativeMaintenanceAnchorInstallV2Error::Input)?;
    // require the exact legacy grammar, transferred state, and canonical bytes
    if validate_v2_anchor(&anchor, codec, time) != Some(MaintenanceAnchorState::Transferred)
        || codec.encode_canonical_json(&anchor).as_deref() != Some(bytes)
    {
        return Err(NativeMaintenanceAnchorInstallV2Error::Input);
    }
    Ok(anchor)
}

/// Require the shared store's fixed three-name transaction namespace.
fn require_fixed_inventory(
    files: &mut impl NativeMaintenanceAnchorV3InstallFiles,
) -> Result<(), NativeMaintenanceAnchorInstallV2Error> {
    let names = files
        .read_root()
        .map_err(|error| match error {
            crate::MaintenanceAnchorStatusError::UnknownEntry => {
                NativeMaintenanceAnchorInstallV2Error::UnknownEntry
            }
            _ => NativeMaintenanceAnchorInstallV2Error::Root,
        })?
        .ok_or(NativeMaintenanceAnchorInstallV2Error::Root)?;
    // reject foreign, hidden, or temporary state before retained slot reads
    if names.iter().any(|name| {
        !matches!(
            name.as_str(),
            "current.json" | "pending.json" | "previous.json"
        )
    }) {
        return Err(NativeMaintenanceAnchorInstallV2Error::UnknownEntry);
    }
    Ok(())
}

/// Retain one parsed v2 or v3 slot with its exact bytes and identity.
struct AnchorSlot {
    anchor: JsonValue,
    anchor_sha256: String,
    bytes: Vec<u8>,
}

/// Read either genuine retained anchor version exactly as the shared source store does.
fn read_anchor_slot(
    slot: MaintenanceAnchorSlot,
    files: &mut impl NativeMaintenanceAnchorV3InstallFiles,
    codec: &mut impl MaintenanceAnchorStatusJson,
    time: &dyn CurrentTimeResolver,
) -> Result<Option<AnchorSlot>, NativeMaintenanceAnchorInstallV2Error> {
    files
        .read_anchor_slot(slot)
        .map_err(map_shared_error)?
        .map(|bytes| {
            let anchor = codec
                .parse_canonical_object(&bytes)
                .ok_or(NativeMaintenanceAnchorInstallV2Error::Slot)?;
            let state = if text(&anchor, "contractVersion").as_deref() == Some(V3_VERSION) {
                validate_v3_anchor(&anchor, codec, time)
            } else {
                validate_v2_anchor(&anchor, codec, time)
            };
            // require the complete selected schema and exact retained canonical bytes
            if state.is_none()
                || codec.encode_canonical_json(&anchor).as_deref() != Some(bytes.as_slice())
            {
                return Err(NativeMaintenanceAnchorInstallV2Error::Slot);
            }
            let anchor_sha256 = codec
                .sha256(&bytes)
                .ok_or(NativeMaintenanceAnchorInstallV2Error::Codec)?
                .as_str()
                .to_owned();
            Ok(AnchorSlot {
                anchor,
                anchor_sha256,
                bytes,
            })
        })
        .transpose()
}

/// Preserve legacy sequence, due ordering, and one byte-identical pending transaction.
fn install_anchor(
    anchor: &JsonValue,
    bytes: &[u8],
    expected_sha256: &str,
    files: &mut impl NativeMaintenanceAnchorV3InstallFiles,
    codec: &mut impl MaintenanceAnchorStatusJson,
    time: &dyn CurrentTimeResolver,
) -> Result<(), NativeMaintenanceAnchorInstallV2Error> {
    use NativeMaintenanceAnchorInstallV2Error as Error;

    let current = read_anchor_slot(MaintenanceAnchorSlot::Current, files, codec, time)?;
    let pending = read_anchor_slot(MaintenanceAnchorSlot::Pending, files, codec, time)?;
    read_anchor_slot(MaintenanceAnchorSlot::Previous, files, codec, time)?;
    // finish an exact already-current retry without advancing sequence
    if current.as_ref().is_some_and(|slot| slot.bytes == bytes) {
        // preserve a different pending transaction rather than hiding drift
        if let Some(pending) = pending {
            // retire only a byte-identical duplicate marker
            if pending.bytes != bytes {
                return Err(Error::PendingCollision);
            }
            files.remove_pending().map_err(map_shared_error)?;
            files.sync_root().map_err(map_shared_error)?;
        }
        return Ok(());
    }
    let (predecessor, sequence) = match current.as_ref() {
        Some(current) => {
            let sequence = anchor_sequence(&current.anchor)?
                .checked_add(1)
                .ok_or(Error::Predecessor)?;
            (Some(current.anchor_sha256.clone()), sequence)
        }
        None => (None, 0),
    };
    // retain the legacy strict due-key advance in addition to direct predecessor linkage
    if nullable_hash(anchor, "predecessorAnchorSha256")? != predecessor
        || anchor_sequence(anchor)? != sequence
        || current.as_ref().is_some_and(|current| {
            text(anchor, "dueKey").as_deref() <= text(&current.anchor, "dueKey").as_deref()
        })
    {
        return Err(Error::Predecessor);
    }
    // preserve only one byte-identical in-progress transaction
    if let Some(pending) = pending {
        // refuse replacement of an independently pending transaction
        if pending.bytes != bytes {
            return Err(Error::PendingCollision);
        }
    } else {
        files.create_pending(bytes).map_err(map_shared_error)?;
    }
    commit_maintenance_anchor_v2_pending(expected_sha256, files, codec, time)
}

/// Commit one exact pending anchor while retaining only its direct predecessor.
pub(crate) fn commit_maintenance_anchor_v2_pending(
    expected_sha256: &str,
    files: &mut impl NativeMaintenanceAnchorV3InstallFiles,
    codec: &mut impl MaintenanceAnchorStatusJson,
    time: &dyn CurrentTimeResolver,
) -> Result<(), NativeMaintenanceAnchorInstallV2Error> {
    use NativeMaintenanceAnchorInstallV2Error as Error;

    let pending = read_anchor_slot(MaintenanceAnchorSlot::Pending, files, codec, time)?;
    let current = read_anchor_slot(MaintenanceAnchorSlot::Current, files, codec, time)?;
    // converge only the already committed expected anchor after pending retirement
    let Some(pending) = pending else {
        return current
            .filter(|slot| slot.anchor_sha256 == expected_sha256)
            .map(|_| ())
            .ok_or(Error::Slot);
    };
    // refuse a pending transaction whose exact bytes have another identity
    if pending.anchor_sha256 != expected_sha256 {
        return Err(Error::Identity);
    }
    // retain only a genuinely different current anchor as direct history
    if let Some(current) = current
        && current.anchor_sha256 != expected_sha256
    {
        files
            .replace_slot(MaintenanceAnchorSlot::Previous, &current.bytes)
            .map_err(map_shared_error)?;
    }
    files
        .replace_slot(MaintenanceAnchorSlot::Current, &pending.bytes)
        .map_err(map_shared_error)?;
    files.remove_pending().map_err(map_shared_error)?;
    files.sync_root().map_err(map_shared_error)
}

/// Map the shared physical store's exact failures into the legacy command boundary.
fn map_shared_error(
    error: NativeMaintenanceAnchorInstallV3Error,
) -> NativeMaintenanceAnchorInstallV2Error {
    match error {
        NativeMaintenanceAnchorInstallV3Error::Root => NativeMaintenanceAnchorInstallV2Error::Root,
        NativeMaintenanceAnchorInstallV3Error::UnknownEntry => {
            NativeMaintenanceAnchorInstallV2Error::UnknownEntry
        }
        NativeMaintenanceAnchorInstallV3Error::PendingCollision => {
            NativeMaintenanceAnchorInstallV2Error::PendingCollision
        }
        NativeMaintenanceAnchorInstallV3Error::Codec => {
            NativeMaintenanceAnchorInstallV2Error::Codec
        }
        _ => NativeMaintenanceAnchorInstallV2Error::Slot,
    }
}

/// Decode one already validated anchor sequence.
fn anchor_sequence(anchor: &JsonValue) -> Result<u64, NativeMaintenanceAnchorInstallV2Error> {
    text(anchor, "sequence")
        .ok_or(NativeMaintenanceAnchorInstallV2Error::Predecessor)?
        .parse()
        .map_err(|_| NativeMaintenanceAnchorInstallV2Error::Predecessor)
}

/// Decode one already validated nullable predecessor identity.
fn nullable_hash(
    anchor: &JsonValue,
    key: &str,
) -> Result<Option<String>, NativeMaintenanceAnchorInstallV2Error> {
    match field(anchor, key) {
        Some(JsonValue::Null) => Ok(None),
        Some(JsonValue::String(units)) => String::from_utf16(units)
            .ok()
            .filter(|value| valid_hash(value))
            .map(Some)
            .ok_or(NativeMaintenanceAnchorInstallV2Error::Predecessor),
        _ => Err(NativeMaintenanceAnchorInstallV2Error::Predecessor),
    }
}

/// Decode one admitted lowercase SHA-256 address into the native typed digest.
pub(crate) fn parse_digest(value: &str) -> Option<ForecastCycleSha256> {
    valid_hash(value).then_some(())?;
    let mut bytes = [0_u8; 32];
    // decode every exact lowercase hexadecimal pair without accepting aliases
    for (index, pair) in value.as_bytes().chunks_exact(2).enumerate() {
        bytes[index] = (hex_nibble(pair[0])? << 4) | hex_nibble(pair[1])?;
    }
    Some(ForecastCycleSha256::admitted(bytes))
}

/// Decode one lowercase hexadecimal nibble.
fn hex_nibble(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        _ => None,
    }
}

/// Encode one typed native digest into its exact lowercase wire spelling.
fn lower_hex(bytes: &[u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(64);
    // emit exactly two lowercase hexadecimal characters per digest byte
    for byte in bytes {
        encoded.push(HEX[(byte >> 4) as usize] as char);
        encoded.push(HEX[(byte & 0x0f) as usize] as char);
    }
    encoded
}

#[cfg(test)]
#[path = "native_maintenance_anchor_install_v2_test.rs"]
mod tests;
