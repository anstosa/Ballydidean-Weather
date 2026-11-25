//! Immutable monthly rain-control state and abandonment under the shared archive spool lease.

use weather_contracts::{
    JsonValue, RainMaintenanceControlStateGapInput,
    RainMaintenanceControlStateGapInputReasonLiteral, RainMaintenanceControlStateStageReceipt,
    RainMaintenanceControlStateStageReceiptContractVersionLiteral,
    RainMaintenanceControlStateStageReceiptDurableLiteral,
};
use weather_domain::{RainMaintenanceControlPrimitives, admit_rain_maintenance_control_state};

use crate::private_archive_service::{
    PrivateArchiveRainControlStore, PrivateArchiveStorageError,
    RainMaintenanceControlStateGapReceipt,
};
use crate::revision_archive_canonical::{
    RevisionArchiveCanonicalClock, RevisionArchiveCanonicalCodec, instant, is_payload_name,
    is_sha256, lower_hex,
};
use crate::revision_online_spool::{
    REVISION_ONLINE_PAYLOAD_MAXIMUM_SLOTS, RevisionOnlinePayload, RevisionOnlinePayloadAddress,
    RevisionOnlinePayloadKind, RevisionOnlineSpoolError, RevisionOnlineSpoolStorage, admit_payload,
};

const STATE_KIND: RevisionOnlinePayloadKind = RevisionOnlinePayloadKind::RainControlState;
const PROJECTION_KIND: RevisionOnlinePayloadKind = RevisionOnlinePayloadKind::Projection;
const MAXIMUM_RECEIPT_BYTES: usize = 4_096;
const STAGE_VERSION: &str =
    RainMaintenanceControlStateStageReceiptContractVersionLiteral::WIRE_VALUE;
const GAP_VERSION: &str = "adjustment-rain-control-state-gap/v1";
const GAP_REASON: &str = RainMaintenanceControlStateGapInputReasonLiteral::WIRE_VALUE;

/// Exact result of parsing one admitted retained revision projection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RainControlProjectionBinding {
    Other,
    RainControl(String),
}

/// exact retained state and receipt codecs independent of projection admission.
pub trait RainControlStateWireCodec: RevisionArchiveCanonicalCodec {
    /// Decode one monthly control-state JSON document without changing keys or strings.
    fn decode_control_json(&self, bytes: &[u8]) -> Option<JsonValue>;

    /// Encode the exact producer field order with its terminating line feed.
    fn encode_control_ordered_json(&self, value: &JsonValue) -> Option<Vec<u8>>;

    /// Encode recursively sorted canonical JSON with its terminating line feed.
    fn encode_control_sorted_json(&self, value: &JsonValue) -> Option<Vec<u8>>;

    /// Decode the exact canonical stage-receipt wire shape.
    fn decode_stage_receipt(&self, bytes: &[u8])
    -> Option<RainMaintenanceControlStateStageReceipt>;

    /// Decode the exact canonical abandonment receipt shape.
    fn decode_gap_receipt(&self, bytes: &[u8]) -> Option<RainMaintenanceControlStateGapReceipt>;
}

/// full monthly state archive codec with producer-owned projection admission.
pub trait RainControlStateArchiveCodec: RainControlStateWireCodec {
    /// Parse one projection and identify its exact monthly state if applicable.
    fn decode_projection(&self, bytes: &[u8]) -> Option<RainControlProjectionBinding>;
}

/// Private adapter from the archive's retained codec to shared control-state primitives.
struct RainControlAdmissionPrimitives<'a, Codec: ?Sized> {
    codec: &'a Codec,
}

impl<Codec> RainMaintenanceControlPrimitives for RainControlAdmissionPrimitives<'_, Codec>
where
    Codec: RainControlStateWireCodec + ?Sized,
{
    /// Forward exact JSON decoding to the archive's already retained codec.
    fn decode_control_json(&self, bytes: &[u8]) -> Option<JsonValue> {
        self.codec.decode_control_json(bytes)
    }

    /// Forward source-order JSON encoding without changing the stored bytes.
    fn encode_control_ordered_json(&self, value: &JsonValue) -> Option<Vec<u8>> {
        self.codec.encode_control_ordered_json(value)
    }

    /// Forward sorted canonical encoding for self-address verification.
    fn encode_control_sorted_json(&self, value: &JsonValue) -> Option<Vec<u8>> {
        self.codec.encode_control_sorted_json(value)
    }

    /// Compare one expected digest using the archive's approved SHA-256 owner.
    fn control_sha256_matches(&self, bytes: &[u8], expected: &str) -> bool {
        lower_hex(self.codec.sha256(bytes)) == expected
    }

    /// Forward calendar admission without adding a second time implementation.
    fn control_utc_milliseconds(&self, utc: &str) -> Option<i64> {
        self.codec.canonical_utc_milliseconds(utc)
    }
}

/// Two small metadata kinds in separate source-owned directories.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RainControlArchiveMetadataKind {
    StageReceipt,
    GapReceipt,
}

impl RainControlArchiveMetadataKind {
    /// Resolve the retained directory under the explicit owner-private root.
    pub const fn directory(self) -> &'static str {
        match self {
            Self::StageReceipt => "rain-control-state-stage-receipts",
            Self::GapReceipt => "rain-control-state-gaps",
        }
    }
}

/// Durable outcome of one exclusive publication, before response creation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RainControlExclusiveWrite {
    Created,
    AlreadyPresent,
}

/// Existing bounded spool owner for typed state, receipts, and projection data.
pub trait RainControlStateArchiveStorage: RevisionOnlineSpoolStorage {
    /// Verify custody root and create only the fixed revision directories.
    fn ensure_revision_root(&mut self) -> Result<(), RevisionOnlineSpoolError>;

    /// Publish the exact state without overwriting a retained object.
    fn write_state_exclusive(
        &mut self,
        address: &RevisionOnlinePayloadAddress,
        bytes: &[u8],
    ) -> Result<(), RevisionOnlineSpoolError>;

    /// Read an optional bounded regular private receipt addressed by the admitted state digest.
    fn read_metadata(
        &mut self,
        kind: RainControlArchiveMetadataKind,
        state_sha256: &str,
        maximum_bytes: usize,
    ) -> Result<Option<Vec<u8>>, RevisionOnlineSpoolError>;

    /// Publish one complete receipt exclusively with data and parent directory sync.
    fn write_metadata_exclusive(
        &mut self,
        kind: RainControlArchiveMetadataKind,
        state_sha256: &str,
        bytes: &[u8],
    ) -> Result<RainControlExclusiveWrite, RevisionOnlineSpoolError>;

    /// Read one retained projection using its validated canonical basename and hard bound.
    fn read_projection(
        &mut self,
        name: &str,
        maximum_bytes: usize,
    ) -> Result<Vec<u8>, RevisionOnlineSpoolError>;
}

/// Real state/gap archive logic over the shared approved spool and codec.
pub struct RainControlStateArchive<Storage, Codec, Clock> {
    storage: Storage,
    codec: Codec,
    clock: Clock,
}

impl<Storage, Codec, Clock> RainControlStateArchive<Storage, Codec, Clock>
where
    Storage: RainControlStateArchiveStorage,
    Codec: RainControlStateArchiveCodec,
    Clock: RevisionArchiveCanonicalClock,
{
    /// Bind one archive to its custody, parsing, digest, and time owners.
    pub const fn new(storage: Storage, codec: Codec, clock: Clock) -> Self {
        Self {
            storage,
            codec,
            clock,
        }
    }

    /// Release the storage handle back to the archive process.
    pub fn into_storage(self) -> Storage {
        self.storage
    }

    /// Retain one parser-verified state and the first canonical durable receipt.
    pub fn stage(
        &mut self,
        bytes: &[u8],
    ) -> Result<RainMaintenanceControlStateStageReceipt, RevisionOnlineSpoolError> {
        let primitives = RainControlAdmissionPrimitives { codec: &self.codec };
        // reject invalid state shape before acquiring the custody root or spool lease
        if !(2..=STATE_KIND.maximum_bytes()).contains(&bytes.len())
            || admit_rain_maintenance_control_state(&primitives, bytes).is_none()
        {
            return Err(RevisionOnlineSpoolError::Input);
        }
        let state_digest = lower_hex(self.codec.sha256(bytes));
        let address = state_address(&state_digest)?;
        self.storage.ensure_revision_root()?;
        let codec = &self.codec;
        let clock = &mut self.clock;
        self.storage.with_revision_spool_lease(
            // preserve the tight state read and immutable retry ahead of census
            |storage| {
                match storage.read_existing_payload(&address, STATE_KIND.maximum_bytes())? {
                    Some(existing) if existing == bytes => {}
                    Some(_) => return Err(RevisionOnlineSpoolError::IdentityConflict),
                    None => {
                        let payload = RevisionOnlinePayload::new(&address, bytes)?;
                        admit_payload(storage, &payload)?;
                        storage.write_state_exclusive(&address, bytes)?;
                    }
                }
                stage_receipt(storage, codec, clock, &state_digest)
            },
        )
    }

    /// Permanently mark an exact retained state that never acquired a projection.
    pub fn record_gap(
        &mut self,
        input: &RainMaintenanceControlStateGapInput,
    ) -> Result<RainMaintenanceControlStateGapReceipt, RevisionOnlineSpoolError> {
        let address = state_address(&input.stateSha256)?;
        // require the admitted producer stage identity before any spool I/O
        if !is_sha256(&input.stageReceiptSha256) {
            return Err(RevisionOnlineSpoolError::Input);
        }
        self.storage.ensure_revision_root()?;
        let codec = &self.codec;
        let clock = &mut self.clock;
        self.storage.with_revision_spool_lease(
            // keep receipt binding and the no-projection disposition in one lease
            |storage| {
                let body = storage
                    .read_existing_payload(&address, STATE_KIND.maximum_bytes())?
                    .ok_or(RevisionOnlineSpoolError::Storage)?;
                let staged = read_stage_receipt(storage, codec, &input.stateSha256)?
                    .ok_or(RevisionOnlineSpoolError::Storage)?;
                // bind the request to both actual retained state bytes and the first receipt
                if lower_hex(codec.sha256(&body)) != input.stateSha256
                    || staged.stageReceiptSha256 != input.stageReceiptSha256
                {
                    return Err(RevisionOnlineSpoolError::IdentityConflict);
                }
                require_no_reference(storage, codec, &input.stateSha256)?;
                gap_receipt(storage, codec, clock, input)
            },
        )
    }
}

impl<Storage, Codec, Clock> PrivateArchiveRainControlStore
    for RainControlStateArchive<Storage, Codec, Clock>
where
    Storage: RainControlStateArchiveStorage,
    Codec: RainControlStateArchiveCodec,
    Clock: RevisionArchiveCanonicalClock,
{
    /// Bind the private stage route to the real state and capacity owner.
    fn stage_rain_control_state(
        &mut self,
        bytes: Vec<u8>,
    ) -> Result<RainMaintenanceControlStateStageReceipt, PrivateArchiveStorageError> {
        self.stage(&bytes).map_err(archive_error)
    }

    /// Bind the abandonment route to the exact retained receipt before emitting success.
    fn record_rain_control_state_gap(
        &mut self,
        input: RainMaintenanceControlStateGapInput,
    ) -> Result<RainMaintenanceControlStateGapReceipt, PrivateArchiveStorageError> {
        self.record_gap(&input).map_err(archive_error)
    }
}

/// Expose only the actual shared payload ceiling as a retryable spool refusal.
fn archive_error(error: RevisionOnlineSpoolError) -> PrivateArchiveStorageError {
    match error {
        RevisionOnlineSpoolError::Capacity => PrivateArchiveStorageError::RevisionSpoolRefused,
        _ => PrivateArchiveStorageError::Rejected,
    }
}

/// Reconstruct the fixed state path from a checked content digest.
fn state_address(digest: &str) -> Result<RevisionOnlinePayloadAddress, RevisionOnlineSpoolError> {
    RevisionOnlinePayloadAddress::from_sha256(STATE_KIND, digest)
        .ok_or(RevisionOnlineSpoolError::Input)
}

/// Ensure no admitted pending projection already committed to this state.
fn require_no_reference(
    storage: &mut impl RainControlStateArchiveStorage,
    codec: &impl RainControlStateArchiveCodec,
    digest: &str,
) -> Result<(), RevisionOnlineSpoolError> {
    let names = storage.list_payload_names(PROJECTION_KIND)?;
    // only the two bounded online slots may participate in reference resolution
    if names.len() > REVISION_ONLINE_PAYLOAD_MAXIMUM_SLOTS
        || !names.iter().map(String::as_str).all(is_payload_name)
    {
        return Err(RevisionOnlineSpoolError::Census);
    }
    // validate each exact retained projection before attempting permanence
    for name in names {
        let bytes = storage.read_projection(&name, PROJECTION_KIND.maximum_bytes())?;
        // keep bounded projection parsing even if the file adapter misreports a size
        if bytes.len() > PROJECTION_KIND.maximum_bytes() {
            return Err(RevisionOnlineSpoolError::Census);
        }
        let binding = codec
            .decode_projection(&bytes)
            .ok_or(RevisionOnlineSpoolError::Input)?;
        // leave referenced states intact for their already durable projection
        if matches!(binding, RainControlProjectionBinding::RainControl(used) if used == digest) {
            return Err(RevisionOnlineSpoolError::IdentityConflict);
        }
    }
    Ok(())
}

/// Return an existing exact stage receipt or publish a new first availability witness.
fn stage_receipt(
    storage: &mut impl RainControlStateArchiveStorage,
    codec: &impl RainControlStateWireCodec,
    clock: &mut impl RevisionArchiveCanonicalClock,
    state_sha256: &str,
) -> Result<RainMaintenanceControlStateStageReceipt, RevisionOnlineSpoolError> {
    // preserve the first clock without observing time on a normal retry
    if let Some(retained) = read_stage_receipt(storage, codec, state_sha256)? {
        return Ok(retained);
    }
    let time = read_archive_time(codec, clock)?;
    let stage_receipt_sha256 =
        lower_hex(codec.sha256(stage_bytes(&time, state_sha256, None).as_bytes()));
    let bytes = stage_bytes(&time, state_sha256, Some(&stage_receipt_sha256));
    match storage.write_metadata_exclusive(
        RainControlArchiveMetadataKind::StageReceipt,
        state_sha256,
        bytes.as_bytes(),
    )? {
        RainControlExclusiveWrite::Created => Ok(RainMaintenanceControlStateStageReceipt {
            contractVersion: RainMaintenanceControlStateStageReceiptContractVersionLiteral,
            durable: RainMaintenanceControlStateStageReceiptDurableLiteral,
            durableAt: time,
            stageReceiptSha256: stage_receipt_sha256,
            stateSha256: state_sha256.to_owned(),
        }),
        RainControlExclusiveWrite::AlreadyPresent => {
            read_stage_receipt(storage, codec, state_sha256)?
                .ok_or(RevisionOnlineSpoolError::Storage)
        }
    }
}

/// Decode and rebind one durable stage receipt to its immutable bytes.
fn read_stage_receipt(
    storage: &mut impl RainControlStateArchiveStorage,
    codec: &impl RainControlStateWireCodec,
    state_sha256: &str,
) -> Result<Option<RainMaintenanceControlStateStageReceipt>, RevisionOnlineSpoolError> {
    let Some(bytes) = storage.read_metadata(
        RainControlArchiveMetadataKind::StageReceipt,
        state_sha256,
        MAXIMUM_RECEIPT_BYTES,
    )?
    else {
        return Ok(None);
    };
    decode_canonical_rain_stage_receipt(codec, &bytes, state_sha256).map(Some)
}

/// Verify the exact stored stage receipt before another archive path uses it.
pub(crate) fn decode_canonical_rain_stage_receipt(
    codec: &impl RainControlStateWireCodec,
    bytes: &[u8],
    state_sha256: &str,
) -> Result<RainMaintenanceControlStateStageReceipt, RevisionOnlineSpoolError> {
    // reject an oversize stage receipt before calling the external decoder
    if bytes.len() > MAXIMUM_RECEIPT_BYTES {
        return Err(RevisionOnlineSpoolError::Input);
    }
    let value = codec
        .decode_stage_receipt(bytes)
        .ok_or(RevisionOnlineSpoolError::Input)?;
    // validate the exact retained timestamp and key before forming the hash preimage
    if value.stateSha256 != state_sha256
        || instant(codec, &value.durableAt).is_none()
        || !is_sha256(&value.stageReceiptSha256)
    {
        return Err(RevisionOnlineSpoolError::IdentityConflict);
    }
    let unsigned = stage_bytes(&value.durableAt, state_sha256, None);
    let digest = lower_hex(codec.sha256(unsigned.as_bytes()));
    // require the exact digest and canonical reencoding of the retained receipt
    if value.stageReceiptSha256 != digest
        || bytes != stage_bytes(&value.durableAt, state_sha256, Some(&digest)).as_bytes()
    {
        return Err(RevisionOnlineSpoolError::IdentityConflict);
    }
    Ok(value)
}

/// Retain the exact first gap after the state and reference bindings are verified.
fn gap_receipt(
    storage: &mut impl RainControlStateArchiveStorage,
    codec: &impl RainControlStateWireCodec,
    clock: &mut impl RevisionArchiveCanonicalClock,
    input: &RainMaintenanceControlStateGapInput,
) -> Result<RainMaintenanceControlStateGapReceipt, RevisionOnlineSpoolError> {
    let kind = RainControlArchiveMetadataKind::GapReceipt;
    // do not replace an existing immutable failure disposition
    if let Some(bytes) = storage.read_metadata(kind, &input.stateSha256, MAXIMUM_RECEIPT_BYTES)? {
        return decode_canonical_rain_gap_receipt(codec, input, &bytes);
    }
    let time = read_archive_time(codec, clock)?;
    let digest = lower_hex(codec.sha256(gap_bytes(&time, input, None).as_bytes()));
    let bytes = gap_bytes(&time, input, Some(&digest));
    // preserve the source's exact one-time write behavior for newly recorded gaps
    if storage.write_metadata_exclusive(kind, &input.stateSha256, bytes.as_bytes())?
        != RainControlExclusiveWrite::Created
    {
        return Err(RevisionOnlineSpoolError::Storage);
    }
    Ok(RainMaintenanceControlStateGapReceipt {
        contract_version: GAP_VERSION.to_owned(),
        gap_at: time,
        gap_sha256: digest,
        reason: RainMaintenanceControlStateGapInputReasonLiteral,
        stage_receipt_sha256: input.stageReceiptSha256.clone(),
        state_sha256: input.stateSha256.clone(),
    })
}

/// Decode and rebind one previously recorded abandonment.
pub(crate) fn decode_canonical_rain_gap_receipt(
    codec: &impl RainControlStateWireCodec,
    input: &RainMaintenanceControlStateGapInput,
    bytes: &[u8],
) -> Result<RainMaintenanceControlStateGapReceipt, RevisionOnlineSpoolError> {
    // keep bounded metadata ahead of external decoding
    if bytes.len() > MAXIMUM_RECEIPT_BYTES {
        return Err(RevisionOnlineSpoolError::Input);
    }
    let value = codec
        .decode_gap_receipt(bytes)
        .ok_or(RevisionOnlineSpoolError::Input)?;
    // bind the exact retained document to both state and stage receipt
    if value.contract_version != GAP_VERSION
        || value.state_sha256 != input.stateSha256
        || value.stage_receipt_sha256 != input.stageReceiptSha256
        || instant(codec, &value.gap_at).is_none()
        || !is_sha256(&value.gap_sha256)
    {
        return Err(RevisionOnlineSpoolError::IdentityConflict);
    }
    let digest = lower_hex(codec.sha256(gap_bytes(&value.gap_at, input, None).as_bytes()));
    // match both the receipt digest and newline-framed canonical wire bytes
    if value.gap_sha256 != digest
        || bytes != gap_bytes(&value.gap_at, input, Some(&digest)).as_bytes()
    {
        return Err(RevisionOnlineSpoolError::IdentityConflict);
    }
    Ok(value)
}

/// Capture one canonical archive instant only after durable inputs are ready.
fn read_archive_time(
    codec: &impl RainControlStateWireCodec,
    clock: &mut impl RevisionArchiveCanonicalClock,
) -> Result<String, RevisionOnlineSpoolError> {
    let time = clock
        .canonical_now()
        .ok_or(RevisionOnlineSpoolError::Input)?;
    instant(codec, &time).ok_or(RevisionOnlineSpoolError::Input)?;
    Ok(time)
}

/// Render the fixed-alphabet stage receipt or its unsigned hash preimage.
fn stage_bytes(time: &str, state_sha256: &str, digest: Option<&str>) -> String {
    let stage_digest = digest.map_or(
        String::new(),
        // insert the optional digest at its frozen canonical key position
        |value| format!("\"stageReceiptSha256\":\"{value}\","),
    );
    format!(
        "{{\"contractVersion\":\"{STAGE_VERSION}\",\"durable\":true,\"durableAt\":\"{time}\",{stage_digest}\"stateSha256\":\"{state_sha256}\"}}\n",
    )
}

/// Render the fixed-alphabet abandonment receipt or its unsigned hash preimage.
fn gap_bytes(
    time: &str,
    input: &RainMaintenanceControlStateGapInput,
    digest: Option<&str>,
) -> String {
    let gap_digest = digest.map_or(
        String::new(),
        // insert the optional digest at its frozen canonical key position
        |value| format!("\"gapSha256\":\"{value}\","),
    );
    format!(
        "{{\"contractVersion\":\"{GAP_VERSION}\",\"gapAt\":\"{time}\",{gap_digest}\"reason\":\"{GAP_REASON}\",\"stageReceiptSha256\":\"{}\",\"stateSha256\":\"{}\"}}\n",
        input.stageReceiptSha256, input.stateSha256,
    )
}
