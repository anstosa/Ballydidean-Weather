//! Closed native archive registry for revision-gap segments and genesis ancestry.

use weather_contracts::{JsonValue, RainMaintenanceControlStateStageReceiptContractVersionLiteral};
use weather_edge::{
    ArchiveWireCodec, NativeRevisionGapPayloadKind, PermanentArchiveGapCodec,
    PrivateRevisionReceiptCodec, REVISION_GAP_PROJECTION_MAXIMUM_BYTES,
    REVISION_GAP_RAIN_CONTROL_STATE_MAXIMUM_BYTES, REVISION_GAP_RECEIPT_MAXIMUM_BYTES,
    REVISION_GAP_SHADOW_STAGE_MAXIMUM_BYTES, REVISION_ONLINE_PAYLOAD_MAXIMUM_SLOTS,
    RainControlStateWireCodec, RevisionArchiveCanonicalCodec,
};

use crate::maintenance_canonical_json::{exact_keys, field, text, valid_hash};
use crate::native_maintenance_archive_genesis::{
    NativeMaintenanceArchiveGenesisValidationSession, NativeMaintenanceArchiveGenesisValidators,
};
use crate::native_maintenance_cas_pack::{
    NativeMaintenanceCasMemberValidator, NativeMaintenanceCasStreamSession,
    NativeMaintenanceCasStreamValidator,
};
use crate::native_maintenance_retirement_files_v2::format_digest;
use crate::native_revision_gap_artifacts::{
    NativeRevisionGapArtifactCodec, admitted_gap_rain_state,
};
use crate::{MaintenanceCanonicalJson, NativeWirePrimitives};

const CHECKPOINT_KIND: &str = "adjustment-revision-gap-page-checkpoint/v1";
const REVISION_GAP_KIND: &str = "adjustment-revision-gap/v1";
const RAIN_GAP_KIND: &str = "adjustment-rain-control-state-gap/v1";
const SHADOW_GAP_KIND: &str = "adjustment-shadow-gap/v1";
const REVISION_STAGE_KIND: &str = "adjustment-revision-stage-receipt/v1";
const RAIN_STAGE_KIND: &str =
    RainMaintenanceControlStateStageReceiptContractVersionLiteral::WIRE_VALUE;
const SHADOW_STAGE_RECEIPT_V1: &str = "adjustment-shadow-stage-receipt/v1";
const SHADOW_STAGE_RECEIPT_V2: &str = "adjustment-shadow-stage-receipt/v2";
const EVIDENCE_MAXIMUM_BYTES: usize = 4_832 * 1_024 + 1;

const CHECKPOINT_KEYS: &[&str] = &[
    "contractVersion",
    "entries",
    "memberRootSha256",
    "pageSha256",
    "predecessorFrontierSha256",
    "startSha256",
];
const ENTRY_KEYS: &[&str] = &[
    "gapMemberSha256",
    "gapSha256",
    "payloadIdentitySha256",
    "payloadKind",
    "slotIdentitySha256",
    "stageReceiptMemberSha256",
    "stageReceiptSha256",
];
const RAIN_ENTRY_KEYS: &[&str] = &[
    "gapMemberSha256",
    "gapSha256",
    "payloadIdentitySha256",
    "payloadKind",
    "rainControlStateIdentitySha256",
    "rainControlStateStageReceiptMemberSha256",
    "rainControlStateStageReceiptSha256",
    "slotIdentitySha256",
    "stageReceiptMemberSha256",
    "stageReceiptSha256",
];

/// Exact mixed-history registry for gap graph members and genesis predecessors.
#[derive(Clone, Copy, Debug, Default)]
pub struct NativeMaintenanceGapArchiveValidators;

impl NativeMaintenanceGapArchiveValidators {
    /// Construct the value-free closed registry.
    pub const fn new() -> Self {
        Self
    }
}

impl NativeMaintenanceCasMemberValidator for NativeMaintenanceGapArchiveValidators {
    /// Validate one real gap member or delegate only to the exact genesis registry.
    fn validate(&mut self, kind: &str, payload: &[u8]) -> bool {
        // keep unknown kinds closed rather than treating canonical JSON as sufficient
        if gap_member_maximum(kind).is_some() {
            validate_gap_member(kind, payload)
        } else {
            NativeMaintenanceArchiveGenesisValidators::new().validate(kind, payload)
        }
    }
}

impl NativeMaintenanceCasStreamValidator for NativeMaintenanceGapArchiveValidators {
    type Session = NativeMaintenanceGapArchiveValidationSession;

    /// Open one bounded real gap or exact genesis validation session.
    fn begin(&mut self, kind: &str, length: u64) -> Option<Self::Session> {
        // select an actual gap ceiling before falling back to the closed genesis registry
        if let Some((kind, maximum)) = gap_member_maximum(kind) {
            if length > maximum as u64 {
                return None;
            }
            return Some(NativeMaintenanceGapArchiveValidationSession {
                inner: NativeMaintenanceGapArchiveValidationInner::Gap {
                    bytes: Vec::with_capacity(usize::try_from(length).ok()?),
                    kind,
                    maximum,
                },
            });
        }
        let session = NativeMaintenanceArchiveGenesisValidators::new().begin(kind, length)?;
        Some(NativeMaintenanceGapArchiveValidationSession {
            inner: NativeMaintenanceGapArchiveValidationInner::Genesis(session),
        })
    }
}

/// One public associated session with a private closed registry discriminator.
pub struct NativeMaintenanceGapArchiveValidationSession {
    inner: NativeMaintenanceGapArchiveValidationInner,
}

enum NativeMaintenanceGapArchiveValidationInner {
    Gap {
        bytes: Vec<u8>,
        kind: &'static str,
        maximum: usize,
    },
    Genesis(NativeMaintenanceArchiveGenesisValidationSession),
}

impl NativeMaintenanceCasStreamSession for NativeMaintenanceGapArchiveValidationSession {
    /// Retain one member only within its source-specific byte ceiling.
    fn write(&mut self, bytes: &[u8]) -> bool {
        // preserve the owning validator's independent bounded stream state
        match &mut self.inner {
            NativeMaintenanceGapArchiveValidationInner::Gap {
                bytes: retained,
                maximum,
                ..
            } => {
                let Some(next) = retained.len().checked_add(bytes.len()) else {
                    return false;
                };
                // reject overflow before extending the retained validator input
                if next > *maximum {
                    return false;
                }
                retained.extend_from_slice(bytes);
                true
            }
            NativeMaintenanceGapArchiveValidationInner::Genesis(session) => session.write(bytes),
        }
    }

    /// Finish through the exact selected source validator.
    fn finish(self) -> bool {
        // preserve the original schema owner after the complete stream arrives
        match self.inner {
            NativeMaintenanceGapArchiveValidationInner::Gap { bytes, kind, .. } => {
                validate_gap_member(kind, &bytes)
            }
            NativeMaintenanceGapArchiveValidationInner::Genesis(session) => session.finish(),
        }
    }
}

/// Validate one registered gap body through its original native artifact owner.
fn validate_gap_member(kind: &str, bytes: &[u8]) -> bool {
    let wire = ArchiveWireCodec::new(NativeWirePrimitives::new());
    let artifacts = NativeRevisionGapArtifactCodec::native();
    // close every emitted gap member kind without a generic canonical success path
    match kind {
        CHECKPOINT_KIND => validate_gap_checkpoint(bytes).is_some(),
        REVISION_GAP_KIND => wire.accepted_revision_gap(bytes).is_some(),
        RAIN_GAP_KIND => RainControlStateWireCodec::decode_gap_receipt(&wire, bytes).is_some(),
        SHADOW_GAP_KIND => wire.accepted_shadow_gap(bytes).is_some(),
        REVISION_STAGE_KIND => {
            canonical_kind(bytes, kind, REVISION_GAP_RECEIPT_MAXIMUM_BYTES).is_some()
                && PrivateRevisionReceiptCodec::decode_stage_receipt(&wire, bytes).is_some()
        }
        RAIN_STAGE_KIND => {
            canonical_kind(bytes, kind, REVISION_GAP_RECEIPT_MAXIMUM_BYTES).is_some()
                && RainControlStateWireCodec::decode_stage_receipt(&wire, bytes).is_some()
        }
        SHADOW_STAGE_RECEIPT_V1 | SHADOW_STAGE_RECEIPT_V2 => {
            canonical_kind(bytes, kind, REVISION_GAP_RECEIPT_MAXIMUM_BYTES)
                .and_then(|value| wire.decode_shadow_stage_value(&value))
                .is_some()
        }
        "rain-maintenance-control-state/v1" => {
            admitted_gap_rain_state(&NativeWirePrimitives, bytes, None)
        }
        payload_kind => {
            let Some(expected) = revision_payload_kind(payload_kind) else {
                return false;
            };
            let Some(value) = canonical_kind(bytes, payload_kind, payload_maximum(expected)) else {
                return false;
            };
            drop(value);
            // preserve separate projection and self-contained shadow-stage owners
            match expected {
                NativeRevisionGapPayloadKind::ShadowStageV1
                | NativeRevisionGapPayloadKind::ShadowStageV2 => artifacts
                    .gap_shadow_stage(bytes)
                    .is_some_and(|stage| stage.kind == expected),
                _ => artifacts
                    .gap_projection(bytes)
                    .is_some_and(|projection| projection.kind == expected),
            }
        }
    }
}

/// Return one exact stream ceiling only for a real emitted gap member kind.
fn gap_member_maximum(kind: &str) -> Option<(&'static str, usize)> {
    let maximum = match kind {
        CHECKPOINT_KIND => EVIDENCE_MAXIMUM_BYTES,
        REVISION_GAP_KIND
        | RAIN_GAP_KIND
        | SHADOW_GAP_KIND
        | REVISION_STAGE_KIND
        | RAIN_STAGE_KIND
        | SHADOW_STAGE_RECEIPT_V1
        | SHADOW_STAGE_RECEIPT_V2 => REVISION_GAP_RECEIPT_MAXIMUM_BYTES,
        "rain-maintenance-control-state/v1" => REVISION_GAP_RAIN_CONTROL_STATE_MAXIMUM_BYTES,
        value => payload_maximum(revision_payload_kind(value)?),
    };
    Some((known_gap_kind(kind)?, maximum))
}

/// Borrow the static spelling for one closed real gap member kind.
fn known_gap_kind(kind: &str) -> Option<&'static str> {
    // return only literals already selected by the closed registry
    match kind {
        CHECKPOINT_KIND => Some(CHECKPOINT_KIND),
        REVISION_GAP_KIND => Some(REVISION_GAP_KIND),
        RAIN_GAP_KIND => Some(RAIN_GAP_KIND),
        SHADOW_GAP_KIND => Some(SHADOW_GAP_KIND),
        REVISION_STAGE_KIND => Some(REVISION_STAGE_KIND),
        RAIN_STAGE_KIND => Some(RAIN_STAGE_KIND),
        SHADOW_STAGE_RECEIPT_V1 => Some(SHADOW_STAGE_RECEIPT_V1),
        SHADOW_STAGE_RECEIPT_V2 => Some(SHADOW_STAGE_RECEIPT_V2),
        "adjustment-revision-projection/v1" => Some("adjustment-revision-projection/v1"),
        "adjustment-revision-batch-projection/v2" => {
            Some("adjustment-revision-batch-projection/v2")
        }
        "adjustment-rain-gate-feature-projection/v2" => {
            Some("adjustment-rain-gate-feature-projection/v2")
        }
        "adjustment-rain-gate-control-projection/v3" => {
            Some("adjustment-rain-gate-control-projection/v3")
        }
        "adjustment-rain-fixed-gauge-target-projection/v1" => {
            Some("adjustment-rain-fixed-gauge-target-projection/v1")
        }
        "adjustment-temperature-native-source-projection/v2" => {
            Some("adjustment-temperature-native-source-projection/v2")
        }
        "rain-maintenance-control-state/v1" => Some("rain-maintenance-control-state/v1"),
        "adjustment-shadow-revision-stage/v1" => Some("adjustment-shadow-revision-stage/v1"),
        "adjustment-shadow-revision-stage/v2" => Some("adjustment-shadow-revision-stage/v2"),
        _ => None,
    }
}

/// Map one portable body contract to its exact typed artifact class.
fn revision_payload_kind(kind: &str) -> Option<NativeRevisionGapPayloadKind> {
    // retain only the nine payload classes emitted by the source gap builder
    match kind {
        "adjustment-revision-projection/v1" => Some(NativeRevisionGapPayloadKind::Projection),
        "adjustment-revision-batch-projection/v2" => {
            Some(NativeRevisionGapPayloadKind::BatchProjection)
        }
        "adjustment-rain-gate-feature-projection/v2" => {
            Some(NativeRevisionGapPayloadKind::RainGateFeature)
        }
        "adjustment-rain-gate-control-projection/v3" => {
            Some(NativeRevisionGapPayloadKind::RainGateControl)
        }
        "adjustment-rain-fixed-gauge-target-projection/v1" => {
            Some(NativeRevisionGapPayloadKind::RainFixedGaugeProjection)
        }
        "adjustment-temperature-native-source-projection/v2" => {
            Some(NativeRevisionGapPayloadKind::TemperatureNativeSourceProjection)
        }
        "rain-maintenance-control-state/v1" => Some(NativeRevisionGapPayloadKind::RainControlState),
        "adjustment-shadow-revision-stage/v1" => Some(NativeRevisionGapPayloadKind::ShadowStageV1),
        "adjustment-shadow-revision-stage/v2" => Some(NativeRevisionGapPayloadKind::ShadowStageV2),
        _ => None,
    }
}

/// Select the actual retained byte ceiling for one typed payload class.
const fn payload_maximum(kind: NativeRevisionGapPayloadKind) -> usize {
    // preserve projection, state, and self-contained shadow storage ceilings
    match kind {
        NativeRevisionGapPayloadKind::RainControlState => {
            REVISION_GAP_RAIN_CONTROL_STATE_MAXIMUM_BYTES
        }
        NativeRevisionGapPayloadKind::ShadowStageV1
        | NativeRevisionGapPayloadKind::ShadowStageV2 => REVISION_GAP_SHADOW_STAGE_MAXIMUM_BYTES,
        _ => REVISION_GAP_PROJECTION_MAXIMUM_BYTES,
    }
}

/// Parse one exact canonical source object with the selected contract kind.
fn canonical_kind(bytes: &[u8], kind: &str, maximum: usize) -> Option<JsonValue> {
    // bound the source document before invoking the canonical JSON owner
    if !(2..=maximum).contains(&bytes.len()) {
        return None;
    }
    let mut codec = NativeWirePrimitives::new();
    let value = codec.parse_canonical_object(bytes)?;
    (text(&value, "contractVersion").as_deref() == Some(kind)).then_some(value)
}

/// Port the exact compact gap-checkpoint grammar and member-root derivation.
fn validate_gap_checkpoint(bytes: &[u8]) -> Option<()> {
    let value = canonical_kind(bytes, CHECKPOINT_KIND, EVIDENCE_MAXIMUM_BYTES)?;
    let Some(JsonValue::Array(entries)) = field(&value, "entries") else {
        return None;
    };
    // require the closed nonempty online-slot population and top-level identities
    if !exact_keys(&value, CHECKPOINT_KEYS)
        || entries.is_empty()
        || entries.len() > REVISION_ONLINE_PAYLOAD_MAXIMUM_SLOTS
        || ![
            "memberRootSha256",
            "pageSha256",
            "predecessorFrontierSha256",
            "startSha256",
        ]
        .into_iter()
        .all(|key| text(&value, key).is_some_and(|hash| valid_hash(&hash)))
    {
        return None;
    }
    let mut tuples = Vec::with_capacity(entries.len());
    // validate and project every exact retained member tuple in page order
    for entry in entries {
        let payload_kind = text(entry, "payloadKind")?;
        let rain = payload_kind == "adjustment-rain-gate-control-projection/v3";
        if revision_payload_kind(&payload_kind).is_none()
            || !exact_keys(entry, if rain { RAIN_ENTRY_KEYS } else { ENTRY_KEYS })
        {
            return None;
        }
        let mut hashes = vec![
            "gapMemberSha256",
            "gapSha256",
            "payloadIdentitySha256",
            "slotIdentitySha256",
            "stageReceiptMemberSha256",
            "stageReceiptSha256",
        ];
        // include the three detached control addresses only for the control projection
        if rain {
            hashes.extend([
                "rainControlStateIdentitySha256",
                "rainControlStateStageReceiptMemberSha256",
                "rainControlStateStageReceiptSha256",
            ]);
        }
        // reject any malformed address retained by the compact checkpoint
        if !hashes
            .iter()
            .all(|source| text(entry, source).is_some_and(|hash| valid_hash(&hash)))
        {
            return None;
        }
        let mut tuple = vec![
            json_field("gapMemberSha256", text(entry, "gapMemberSha256")?),
            json_field("payloadMemberSha256", text(entry, "payloadIdentitySha256")?),
            json_field(
                "stageReceiptMemberSha256",
                text(entry, "stageReceiptMemberSha256")?,
            ),
        ];
        // reproduce the source's detached control tuple field names exactly
        if rain {
            tuple.extend([
                json_field(
                    "rainControlStateMemberSha256",
                    text(entry, "rainControlStateIdentitySha256")?,
                ),
                json_field(
                    "rainControlStateStageReceiptMemberSha256",
                    text(entry, "rainControlStateStageReceiptMemberSha256")?,
                ),
            ]);
        }
        tuples.push(JsonValue::Object(tuple));
    }
    let mut codec = NativeWirePrimitives::new();
    let tuple_bytes = codec.encode_canonical_json(&JsonValue::Array(tuples))?;
    let expected = format_digest(RevisionArchiveCanonicalCodec::sha256(&codec, &tuple_bytes));
    (text(&value, "memberRootSha256").as_deref() == Some(expected.as_str())).then_some(())
}

/// Construct one JSON string field for canonical checkpoint root projection.
fn json_field(key: &str, value: String) -> (Vec<u16>, JsonValue) {
    (
        key.encode_utf16().collect(),
        JsonValue::String(value.encode_utf16().collect()),
    )
}

#[cfg(test)]
#[path = "native_maintenance_gap_archive_validators_test.rs"]
mod tests;
