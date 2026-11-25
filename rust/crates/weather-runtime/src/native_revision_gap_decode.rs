//! Exact retained revision-gap page and acknowledgement decoding.

use weather_contracts::{JsonValue, Nullable};
use weather_edge::{
    ArchiveWireCodec, ArchiveWirePrimitives, NativeRevisionGapMember, NativeRevisionGapPayloadKind,
    NativeRevisionGapRainAuxiliary, PermanentArchiveGapCodec, REVISION_GAP_ACK_CONTRACT_VERSION,
    REVISION_GAP_ACK_MAXIMUM_BYTES, REVISION_GAP_PAGE_MAXIMUM_BYTES,
    REVISION_GAP_PAGE_MAXIMUM_ENTRIES, REVISION_GAP_PAYLOAD_PAGE_CONTRACT_VERSION,
    RainControlStateWireCodec, RetainedRevisionGapEntry, RevisionArchiveCanonicalCodec,
    RevisionGapPayloadAcknowledgement, RevisionGapPayloadPage, RevisionGapRainControlStateBytes,
    RevisionGapRetainedBytes, RevisionGapSha256,
};

use crate::{
    maintenance_canonical_json::{exact_keys, field, text},
    native_revision_gap_artifacts::{NativeRevisionGapArtifactCodec, admitted_gap_rain_state},
    native_revision_gap_identity::{encode_acknowledgement, encode_page},
};

const PAGE_KEYS: &[&str] = &[
    "contractVersion",
    "entries",
    "idle",
    "memberRootSha256",
    "pageSha256",
    "predecessorFrontierSha256",
    "startSha256",
];
const ACKNOWLEDGEMENT_KEYS: &[&str] = &[
    "acknowledgedAt",
    "acknowledgementSha256",
    "contractVersion",
    "frontierSha256",
    "graphManifestSha256",
    "memberRootSha256",
    "pageCount",
    "pageSha256",
    "predecessorFrontierSha256",
];
const ENTRY_KEYS: &[&str] = &[
    "gap",
    "gapMemberSha256",
    "payload",
    "slotIdentitySha256",
    "stageReceipt",
    "stageReceiptMemberSha256",
];
const CONTROL_ENTRY_KEYS: &[&str] = &[
    "gap",
    "gapMemberSha256",
    "payload",
    "rainControlState",
    "rainControlStateStageReceipt",
    "rainControlStateStageReceiptMemberSha256",
    "slotIdentitySha256",
    "stageReceipt",
    "stageReceiptMemberSha256",
];
const PAYLOAD_KEYS: &[&str] = &["bytesBase64", "identitySha256", "kind"];
const CONTROL_STATE_KEYS: &[&str] = &["bytesBase64", "identitySha256", "kind", "stateSha256"];

/// Decode one exact canonical pending page and re-admit every retained artifact.
pub(crate) fn decode_page(
    primitives: &impl ArchiveWirePrimitives,
    bytes: &[u8],
) -> Option<RevisionGapPayloadPage<NativeRevisionGapMember>> {
    // enforce the source page envelope before allocating decoded members
    if bytes.len() > REVISION_GAP_PAGE_MAXIMUM_BYTES {
        return None;
    }
    let value = primitives.decode_json(bytes)?;
    // require the closed source page object before reading identities
    if !exact_keys(&value, PAGE_KEYS)
        || text(&value, "contractVersion")? != REVISION_GAP_PAYLOAD_PAGE_CONTRACT_VERSION
    {
        return None;
    }
    let JsonValue::Array(values) = field(&value, "entries")? else {
        return None;
    };
    // retain only the reviewed online slot count
    if values.len() > REVISION_GAP_PAGE_MAXIMUM_ENTRIES {
        return None;
    }
    let entries = values
        .iter()
        .map(
            // decode each entry through the same typed artifact owners used by collection
            |value| decode_entry(primitives, value),
        )
        .collect::<Option<Vec<_>>>()?;
    let JsonValue::Boolean(idle) = field(&value, "idle")? else {
        return None;
    };
    // preserve the source's explicit idle invariant
    if *idle != entries.is_empty() {
        return None;
    }
    let page = RevisionGapPayloadPage {
        canonical_byte_count: bytes.len(),
        contract_version: REVISION_GAP_PAYLOAD_PAGE_CONTRACT_VERSION,
        entries,
        idle: *idle,
        member_root_sha256: digest(&value, "memberRootSha256")?,
        page_sha256: digest(&value, "pageSha256")?,
        predecessor_frontier_sha256: digest(&value, "predecessorFrontierSha256")?,
        start_sha256: digest(&value, "startSha256")?,
    };
    // the canonical encoder rederives identities and measured size in one pass
    if encode_page(primitives, &page)?.as_slice() != bytes {
        return None;
    }
    Some(page)
}

/// Decode one exact canonical durable acknowledgement and rederive both identities.
pub(crate) fn decode_acknowledgement(
    primitives: &impl ArchiveWirePrimitives,
    bytes: &[u8],
) -> Option<RevisionGapPayloadAcknowledgement> {
    // enforce the compact checkpoint ceiling before JSON allocation
    if bytes.len() > REVISION_GAP_ACK_MAXIMUM_BYTES {
        return None;
    }
    let value = primitives.decode_json(bytes)?;
    // require the closed source acknowledgement object and fixed contract
    if !exact_keys(&value, ACKNOWLEDGEMENT_KEYS)
        || text(&value, "contractVersion")? != REVISION_GAP_ACK_CONTRACT_VERSION
    {
        return None;
    }
    let acknowledgement = RevisionGapPayloadAcknowledgement {
        acknowledged_at: text(&value, "acknowledgedAt")?,
        acknowledgement_sha256: digest(&value, "acknowledgementSha256")?,
        contract_version: REVISION_GAP_ACK_CONTRACT_VERSION,
        frontier_sha256: digest(&value, "frontierSha256")?,
        graph_manifest_sha256: digest(&value, "graphManifestSha256")?,
        member_root_sha256: digest(&value, "memberRootSha256")?,
        page_count: uint64_text(&value, "pageCount")?,
        page_sha256: digest(&value, "pageSha256")?,
        predecessor_frontier_sha256: digest(&value, "predecessorFrontierSha256")?,
    };
    // the canonical encoder rederives both checkpoint identities before writing
    if encode_acknowledgement(primitives, &acknowledgement)?.as_slice() != bytes {
        return None;
    }
    Some(acknowledgement)
}

/// Decode one closed retained entry and recover its exact storage byte classes.
fn decode_entry(
    primitives: &impl ArchiveWirePrimitives,
    value: &JsonValue,
) -> Option<RetainedRevisionGapEntry<NativeRevisionGapMember>> {
    let payload = field(value, "payload")?;
    // read the payload discriminator only from its closed portable envelope
    if !exact_keys(payload, PAYLOAD_KEYS) {
        return None;
    }
    let kind = payload_kind(&text(payload, "kind")?)?;
    let control = kind == NativeRevisionGapPayloadKind::RainGateControl;
    // require exact presence or omission of the three control-state members
    if !exact_keys(
        value,
        if control {
            CONTROL_ENTRY_KEYS
        } else {
            ENTRY_KEYS
        },
    ) {
        return None;
    }
    let payload_bytes = canonical_base64(primitives, payload, "bytesBase64")?;
    let gap = field(value, "gap")?;
    let gap_bytes = canonical_member_bytes(primitives, gap)?;
    let stage_receipt_bytes = canonical_member_bytes(primitives, field(value, "stageReceipt")?)?;
    let rain_control = if control {
        Some(decode_control_auxiliary(primitives, value)?)
    } else {
        None
    };
    let member = NativeRevisionGapMember {
        kind,
        payload_bytes,
        payload_sha256: digest(payload, "identitySha256")?,
        slot_sha256: digest(value, "slotIdentitySha256")?,
        gap_bytes,
        gap_member_sha256: digest(value, "gapMemberSha256")?,
        stage_receipt_bytes,
        stage_receipt_member_sha256: digest(value, "stageReceiptMemberSha256")?,
        rain_control,
    };
    let gap_sha256 = digest(gap, "gapSha256")?;
    let retained_bytes = admit_member(primitives, gap_sha256, &member)?;
    Some(RetainedRevisionGapEntry {
        gap_sha256,
        retained_bytes,
        value: member,
    })
}

/// Decode the supplementary public control state and detached first receipt.
fn decode_control_auxiliary(
    primitives: &impl ArchiveWirePrimitives,
    entry: &JsonValue,
) -> Option<NativeRevisionGapRainAuxiliary> {
    let state = field(entry, "rainControlState")?;
    // require the complete source portable-state envelope
    if !exact_keys(state, CONTROL_STATE_KEYS)
        || text(state, "kind")? != "rain-maintenance-control-state/v1"
    {
        return None;
    }
    let state_bytes = canonical_base64(primitives, state, "bytesBase64")?;
    let state_sha256 = digest(state, "identitySha256")?;
    // bind both portable-state labels to the same exact raw member
    if digest(state, "stateSha256")? != state_sha256 {
        return None;
    }
    Some(NativeRevisionGapRainAuxiliary {
        state_bytes,
        stage_receipt_bytes: canonical_member_bytes(
            primitives,
            field(entry, "rainControlStateStageReceipt")?,
        )?,
        state_sha256,
        stage_receipt_member_sha256: digest(entry, "rainControlStateStageReceiptMemberSha256")?,
    })
}

/// Reuse the original typed artifact validators and bind every transported address.
fn admit_member(
    primitives: &impl ArchiveWirePrimitives,
    gap_sha256: RevisionGapSha256,
    member: &NativeRevisionGapMember,
) -> Option<RevisionGapRetainedBytes> {
    let wire = ArchiveWireCodec::new(BorrowedPrimitives(primitives));
    let artifacts = NativeRevisionGapArtifactCodec::new(BorrowedPrimitives(primitives));
    // reject any portable member label that does not hash its exact transported bytes
    if digest_bytes(primitives, &member.payload_bytes) != member.payload_sha256
        || digest_bytes(primitives, &member.gap_bytes) != member.gap_member_sha256
        || digest_bytes(primitives, &member.stage_receipt_bytes)
            != member.stage_receipt_member_sha256
    {
        return None;
    }
    match member.kind {
        NativeRevisionGapPayloadKind::Projection
        | NativeRevisionGapPayloadKind::BatchProjection
        | NativeRevisionGapPayloadKind::RainGateFeature
        | NativeRevisionGapPayloadKind::RainGateControl
        | NativeRevisionGapPayloadKind::RainFixedGaugeProjection
        | NativeRevisionGapPayloadKind::TemperatureNativeSourceProjection => {
            // ordinary retained projection bytes are opaque; only a control member carries a detached state binding
            let rain_binding = if member.kind == NativeRevisionGapPayloadKind::RainGateControl {
                let projection = artifacts.gap_projection(&member.payload_bytes)?;
                if projection.kind != member.kind {
                    return None;
                }
                projection.rain_control
            } else {
                None
            };
            let gap = wire.accepted_revision_gap(&member.gap_bytes)?;
            let payload_text = text_digest(member.payload_sha256);
            // bind the classified projection, permanent gap and stage receipt to one slot
            if member.slot_sha256 != member.payload_sha256
                || RevisionGapSha256::from_lower_hex(&gap.gap_sha256)? != gap_sha256
                || gap.input.projectionIdentitySha256 != Nullable::Value(payload_text.clone())
                || gap.input.projectionSha256 != Nullable::Value(payload_text)
                || !artifacts.accepted_projection_stage(
                    &member.stage_receipt_bytes,
                    member.slot_sha256,
                    gap.input.projectionKind,
                )
            {
                return None;
            }
            let rain_control_state = match (&rain_binding, &member.rain_control) {
                (None, None) => None,
                (Some(binding), Some(control)) => {
                    // bind the full public state and receipt to the control projection dependency
                    if binding.state_sha256 != control.state_sha256
                        || digest_bytes(primitives, &control.state_bytes) != control.state_sha256
                        || digest_bytes(primitives, &control.stage_receipt_bytes)
                            != control.stage_receipt_member_sha256
                        || !admitted_gap_rain_state(
                            primitives,
                            &control.state_bytes,
                            Some(&binding.ordinal_artifact_sha256),
                        )
                    {
                        return None;
                    }
                    let receipt = RainControlStateWireCodec::decode_stage_receipt(
                        &wire,
                        &control.stage_receipt_bytes,
                    )?;
                    // bind the detached receipt to both the state and projection labels
                    if RevisionGapSha256::from_lower_hex(&receipt.stateSha256)?
                        != control.state_sha256
                        || receipt.stageReceiptSha256 != binding.state_stage_receipt_sha256
                    {
                        return None;
                    }
                    Some(RevisionGapRainControlStateBytes {
                        state_bytes: control.state_bytes.len(),
                        stage_receipt_bytes: control.stage_receipt_bytes.len(),
                    })
                }
                _ => return None,
            };
            let retained = RevisionGapRetainedBytes::Projection {
                gap_bytes: member.gap_bytes.len(),
                payload_bytes: member.payload_bytes.len(),
                stage_receipt_bytes: member.stage_receipt_bytes.len(),
                rain_control_state,
            };
            bounded_retained(retained)
        }
        NativeRevisionGapPayloadKind::RainControlState => {
            let gap = RainControlStateWireCodec::decode_gap_receipt(&wire, &member.gap_bytes)?;
            let stage = RainControlStateWireCodec::decode_stage_receipt(
                &wire,
                &member.stage_receipt_bytes,
            )?;
            // bind the admitted state, stage and permanent abandonment to one exact slot
            if member.rain_control.is_some()
                || member.slot_sha256 != member.payload_sha256
                || !admitted_gap_rain_state(primitives, &member.payload_bytes, None)
                || RevisionGapSha256::from_lower_hex(&gap.gap_sha256)? != gap_sha256
                || RevisionGapSha256::from_lower_hex(&gap.state_sha256)? != member.slot_sha256
                || RevisionGapSha256::from_lower_hex(&stage.stateSha256)? != member.slot_sha256
                || gap.stage_receipt_sha256 != stage.stageReceiptSha256
            {
                return None;
            }
            bounded_retained(RevisionGapRetainedBytes::RainControlState {
                gap_bytes: member.gap_bytes.len(),
                payload_bytes: member.payload_bytes.len(),
                stage_receipt_bytes: member.stage_receipt_bytes.len(),
            })
        }
        NativeRevisionGapPayloadKind::ShadowStageV1
        | NativeRevisionGapPayloadKind::ShadowStageV2 => {
            let stage = artifacts.gap_shadow_stage(&member.payload_bytes)?;
            let gap = wire.accepted_shadow_gap(&member.gap_bytes)?;
            // bind the scheduled gap, stage, embedded receipt and portable entry labels
            if member.rain_control.is_some()
                || stage.kind != member.kind
                || stage.prediction_sha256 != member.slot_sha256
                || stage.stage_receipt_bytes != member.stage_receipt_bytes
                || gap.input.dueKey != stage.due_key
                || gap.input.registrationSha256 != stage.registration_sha256
                || RevisionGapSha256::from_lower_hex(&gap.gap_sha256)? != gap_sha256
            {
                return None;
            }
            bounded_retained(RevisionGapRetainedBytes::ShadowStage {
                gap_bytes: member.gap_bytes.len(),
                stage_bytes: member.payload_bytes.len(),
            })
        }
    }
}

/// Enforce the retained storage ceilings recorded by the typed page state machine.
fn bounded_retained(value: RevisionGapRetainedBytes) -> Option<RevisionGapRetainedBytes> {
    use weather_edge::{
        REVISION_GAP_PROJECTION_MAXIMUM_BYTES, REVISION_GAP_RAIN_CONTROL_STATE_MAXIMUM_BYTES,
        REVISION_GAP_RECEIPT_MAXIMUM_BYTES, REVISION_GAP_SHADOW_STAGE_MAXIMUM_BYTES,
    };

    let admitted = match value {
        RevisionGapRetainedBytes::Projection {
            gap_bytes,
            payload_bytes,
            stage_receipt_bytes,
            rain_control_state,
        } => {
            gap_bytes <= REVISION_GAP_RECEIPT_MAXIMUM_BYTES
                && payload_bytes <= REVISION_GAP_PROJECTION_MAXIMUM_BYTES
                && stage_receipt_bytes <= REVISION_GAP_RECEIPT_MAXIMUM_BYTES
                && rain_control_state.is_none_or(|state| {
                    state.state_bytes <= REVISION_GAP_RAIN_CONTROL_STATE_MAXIMUM_BYTES
                        && state.stage_receipt_bytes <= REVISION_GAP_RECEIPT_MAXIMUM_BYTES
                })
        }
        RevisionGapRetainedBytes::RainControlState {
            gap_bytes,
            payload_bytes,
            stage_receipt_bytes,
        } => {
            gap_bytes <= REVISION_GAP_RECEIPT_MAXIMUM_BYTES
                && payload_bytes <= REVISION_GAP_RAIN_CONTROL_STATE_MAXIMUM_BYTES
                && stage_receipt_bytes <= REVISION_GAP_RECEIPT_MAXIMUM_BYTES
        }
        RevisionGapRetainedBytes::ShadowStage {
            gap_bytes,
            stage_bytes,
        } => {
            gap_bytes <= REVISION_GAP_RECEIPT_MAXIMUM_BYTES
                && stage_bytes <= REVISION_GAP_SHADOW_STAGE_MAXIMUM_BYTES
        }
    };
    admitted.then_some(value)
}

/// Map one closed payload discriminator to its typed retained class.
fn payload_kind(value: &str) -> Option<NativeRevisionGapPayloadKind> {
    match value {
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

/// Recover one exact lowercase digest field.
fn digest(value: &JsonValue, key: &str) -> Option<RevisionGapSha256> {
    RevisionGapSha256::from_lower_hex(&text(value, key)?)
}

/// Hash one transported member through the shared primitive owner.
fn digest_bytes(primitives: &impl ArchiveWirePrimitives, bytes: &[u8]) -> RevisionGapSha256 {
    RevisionGapSha256::admitted(primitives.sha256(bytes))
}

/// Render one admitted digest for typed string-address comparisons.
fn text_digest(value: RevisionGapSha256) -> String {
    let mut text = String::with_capacity(64);
    // append exactly two lowercase digits for each admitted byte
    for byte in value.as_bytes() {
        use std::fmt::Write as _;
        write!(&mut text, "{byte:02x}").expect("writing to a string cannot fail");
    }
    text
}

/// Decode only the canonical standard padded base64 spelling.
fn canonical_base64(
    primitives: &impl ArchiveWirePrimitives,
    value: &JsonValue,
    key: &str,
) -> Option<Vec<u8>> {
    let encoded = text(value, key)?;
    let bytes = primitives.decode_standard_base64(&encoded)?;
    (primitives.encode_standard_base64(&bytes)? == encoded).then_some(bytes)
}

/// Reconstruct one embedded object with the exact canonical member encoding.
fn canonical_member_bytes(
    primitives: &impl ArchiveWirePrimitives,
    value: &JsonValue,
) -> Option<Vec<u8>> {
    // embedded gap and receipt members are closed JSON objects, never scalars
    if !matches!(value, JsonValue::Object(_)) {
        return None;
    }
    primitives.encode_sorted_json(value)
}

/// Decode one canonical uint64 decimal text without alternate spellings.
fn uint64_text(value: &JsonValue, key: &str) -> Option<u64> {
    let value = text(value, key)?;
    // reject signs, empty text and leading zeroes before parsing
    if value.is_empty()
        || (value.len() > 1 && value.starts_with('0'))
        || !value.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    value.parse().ok()
}

/// Borrow a primitive owner so the original typed archive codecs can validate members.
#[derive(Clone, Copy)]
struct BorrowedPrimitives<'a, P>(&'a P);

impl<P: ArchiveWirePrimitives> RevisionArchiveCanonicalCodec for BorrowedPrimitives<'_, P> {
    /// Forward exact canonical UTC admission to the selected primitive owner.
    fn canonical_utc_milliseconds(&self, text: &str) -> Option<i64> {
        self.0.canonical_utc_milliseconds(text)
    }

    /// Forward exact-byte SHA-256 to the selected primitive owner.
    fn sha256(&self, bytes: &[u8]) -> [u8; 32] {
        self.0.sha256(bytes)
    }
}

impl<P: ArchiveWirePrimitives> ArchiveWirePrimitives for BorrowedPrimitives<'_, P> {
    /// Forward canonical UTC rendering to the selected primitive owner.
    fn format_utc_milliseconds(&self, milliseconds: i64) -> Option<String> {
        self.0.format_utc_milliseconds(milliseconds)
    }

    /// Forward explicit instant parsing to the selected primitive owner.
    fn parse_explicit_utc_milliseconds(&self, text: &str) -> Option<i64> {
        self.0.parse_explicit_utc_milliseconds(text)
    }

    /// Forward complete JSON decoding to the selected primitive owner.
    fn decode_json(&self, bytes: &[u8]) -> Option<JsonValue> {
        self.0.decode_json(bytes)
    }

    /// Forward recursively sorted JSON encoding to the selected primitive owner.
    fn encode_sorted_json(&self, value: &JsonValue) -> Option<Vec<u8>> {
        self.0.encode_sorted_json(value)
    }

    /// Forward source-ordered JSON encoding to the selected primitive owner.
    fn encode_ordered_json(&self, value: &JsonValue) -> Option<Vec<u8>> {
        self.0.encode_ordered_json(value)
    }

    /// Forward standard base64 decoding to the selected primitive owner.
    fn decode_standard_base64(&self, text: &str) -> Option<Vec<u8>> {
        self.0.decode_standard_base64(text)
    }

    /// Forward standard padded base64 encoding to the selected primitive owner.
    fn encode_standard_base64(&self, bytes: &[u8]) -> Option<String> {
        self.0.encode_standard_base64(bytes)
    }
}
