//! Pure native revision-gap wire projections over the approved archive primitives.

use weather_contracts::JsonValue;
use weather_edge::{
    AdmittedRevisionGapAckIdentity, AdmittedRevisionGapPageIdentity, ArchiveWirePrimitives,
    NativeRevisionGapMember, NativeRevisionGapPayloadKind, REVISION_GAP_ACK_CONTRACT_VERSION,
    REVISION_GAP_PAYLOAD_PAGE_CONTRACT_VERSION, REVISION_GAP_TRANSFER_START_CONTRACT_VERSION,
    RetainedRevisionGapEntry, RevisionGapPayloadAcknowledgement, RevisionGapPayloadPage,
    RevisionGapSha256, RevisionGapTransferStart, RevisionGapUnsignedAcknowledgement,
    RevisionGapUnsignedPage, RevisionGapUnsignedStart,
};

const REVISION_GAP_GENESIS_FRONTIER: &[u8] = b"adjustment-revision-gap-frontier/v1\n0\n";

/// Derive the fixed empty revision-gap frontier through the approved digest primitive.
pub(crate) fn genesis_frontier_sha256(
    primitives: &impl ArchiveWirePrimitives,
) -> RevisionGapSha256 {
    RevisionGapSha256::admitted(primitives.sha256(REVISION_GAP_GENESIS_FRONTIER))
}

/// Bind one compact unsigned transfer start to its canonical sorted JSON identity.
pub(crate) fn start_sha256(
    primitives: &impl ArchiveWirePrimitives,
    start: RevisionGapUnsignedStart,
) -> Option<RevisionGapSha256> {
    // admit only the source-owned start contract
    if start.contract_version != REVISION_GAP_TRANSFER_START_CONTRACT_VERSION {
        return None;
    }
    Some(sha256(
        primitives,
        &primitives.encode_sorted_json(&unsigned_start_value(start))?,
    ))
}

/// Derive the exact member root, page identity, and complete canonical wire size.
pub(crate) fn page_identity(
    primitives: &impl ArchiveWirePrimitives,
    page: RevisionGapUnsignedPage<'_, NativeRevisionGapMember>,
) -> Option<AdmittedRevisionGapPageIdentity> {
    let material = page_material(
        primitives,
        page.contract_version,
        page.entries,
        page.idle,
        page.predecessor_frontier_sha256,
        page.start_sha256,
    )?;
    Some(AdmittedRevisionGapPageIdentity {
        canonical_byte_count: material.complete_bytes.len(),
        member_root_sha256: material.member_root_sha256,
        page_sha256: material.page_sha256,
    })
}

/// Derive the frontier and acknowledgement identities from one exact unsigned checkpoint.
pub(crate) fn acknowledgement_identity(
    primitives: &impl ArchiveWirePrimitives,
    acknowledgement: RevisionGapUnsignedAcknowledgement<'_>,
) -> Option<AdmittedRevisionGapAckIdentity> {
    // admit only a canonical non-genesis acknowledgement
    if acknowledgement.contract_version != REVISION_GAP_ACK_CONTRACT_VERSION
        || acknowledgement.page_count == 0
        || primitives
            .canonical_utc_milliseconds(acknowledgement.acknowledged_at)
            .is_none()
    {
        return None;
    }
    let frontier_sha256 = acknowledgement_frontier_sha256(primitives, acknowledgement);
    let unsigned = unsigned_acknowledgement_value(acknowledgement, frontier_sha256);
    let acknowledgement_sha256 = sha256(primitives, &primitives.encode_sorted_json(&unsigned)?);
    Some(AdmittedRevisionGapAckIdentity {
        acknowledgement_sha256,
        frontier_sha256,
    })
}

/// Encode one complete transfer start after rederiving its canonical identity.
pub(crate) fn encode_start(
    primitives: &impl ArchiveWirePrimitives,
    start: &RevisionGapTransferStart,
) -> Option<Vec<u8>> {
    let unsigned = RevisionGapUnsignedStart {
        contract_version: start.contract_version,
        frontier_sha256: start.frontier_sha256,
        page_count: start.page_count,
    };
    // reject a caller-supplied identity that does not bind the exact start fields
    if start_sha256(primitives, unsigned)? != start.start_sha256 {
        return None;
    }
    primitives.encode_sorted_json(&JsonValue::Object(vec![
        member("contractVersion", string(start.contract_version)),
        member("frontierSha256", digest_value(start.frontier_sha256)),
        member("pageCount", string(&start.page_count.to_string())),
        member("startSha256", digest_value(start.start_sha256)),
    ]))
}

/// Encode one complete payload page after rederiving every exact member identity.
pub(crate) fn encode_page(
    primitives: &impl ArchiveWirePrimitives,
    page: &RevisionGapPayloadPage<NativeRevisionGapMember>,
) -> Option<Vec<u8>> {
    let material = page_material(
        primitives,
        page.contract_version,
        &page.entries,
        page.idle,
        page.predecessor_frontier_sha256,
        page.start_sha256,
    )?;
    // bind the retained page metadata to the bytes being emitted
    if material.complete_bytes.len() != page.canonical_byte_count
        || material.member_root_sha256 != page.member_root_sha256
        || material.page_sha256 != page.page_sha256
    {
        return None;
    }
    Some(material.complete_bytes)
}

/// Encode one complete acknowledgement after rederiving both checkpoint identities.
pub(crate) fn encode_acknowledgement(
    primitives: &impl ArchiveWirePrimitives,
    acknowledgement: &RevisionGapPayloadAcknowledgement,
) -> Option<Vec<u8>> {
    let unsigned = RevisionGapUnsignedAcknowledgement {
        acknowledged_at: &acknowledgement.acknowledged_at,
        contract_version: acknowledgement.contract_version,
        graph_manifest_sha256: acknowledgement.graph_manifest_sha256,
        member_root_sha256: acknowledgement.member_root_sha256,
        page_count: acknowledgement.page_count,
        page_sha256: acknowledgement.page_sha256,
        predecessor_frontier_sha256: acknowledgement.predecessor_frontier_sha256,
    };
    let identity = acknowledgement_identity(primitives, unsigned)?;
    // reject any relabelling of the derived frontier or acknowledgement
    if identity.frontier_sha256 != acknowledgement.frontier_sha256
        || identity.acknowledgement_sha256 != acknowledgement.acknowledgement_sha256
    {
        return None;
    }
    primitives.encode_sorted_json(&JsonValue::Object(vec![
        member("acknowledgedAt", string(&acknowledgement.acknowledged_at)),
        member(
            "acknowledgementSha256",
            digest_value(acknowledgement.acknowledgement_sha256),
        ),
        member("contractVersion", string(acknowledgement.contract_version)),
        member(
            "frontierSha256",
            digest_value(acknowledgement.frontier_sha256),
        ),
        member(
            "graphManifestSha256",
            digest_value(acknowledgement.graph_manifest_sha256),
        ),
        member(
            "memberRootSha256",
            digest_value(acknowledgement.member_root_sha256),
        ),
        member("pageCount", string(&acknowledgement.page_count.to_string())),
        member("pageSha256", digest_value(acknowledgement.page_sha256)),
        member(
            "predecessorFrontierSha256",
            digest_value(acknowledgement.predecessor_frontier_sha256),
        ),
    ]))
}

/// Canonical values and identities needed by both page sizing and final emission.
struct PageMaterial {
    complete_bytes: Vec<u8>,
    member_root_sha256: RevisionGapSha256,
    page_sha256: RevisionGapSha256,
}

/// Construct one exact page envelope from already admitted retained members.
fn page_material(
    primitives: &impl ArchiveWirePrimitives,
    contract_version: &str,
    entries: &[RetainedRevisionGapEntry<NativeRevisionGapMember>],
    idle: bool,
    predecessor_frontier_sha256: RevisionGapSha256,
    start_sha256: RevisionGapSha256,
) -> Option<PageMaterial> {
    // preserve the source contract and explicit idle invariant
    if contract_version != REVISION_GAP_PAYLOAD_PAGE_CONTRACT_VERSION || idle != entries.is_empty()
    {
        return None;
    }
    let member_root_sha256 = member_root_sha256(primitives, entries)?;
    let entries = entries
        .iter()
        .map(
            // map each retained member without normalizing its embedded documents
            |entry| entry_value(primitives, entry),
        )
        .collect::<Option<Vec<_>>>()?;
    let unsigned = JsonValue::Object(vec![
        member("contractVersion", string(contract_version)),
        member("entries", JsonValue::Array(entries)),
        member("idle", JsonValue::Boolean(idle)),
        member("memberRootSha256", digest_value(member_root_sha256)),
        member(
            "predecessorFrontierSha256",
            digest_value(predecessor_frontier_sha256),
        ),
        member("startSha256", digest_value(start_sha256)),
    ]);
    let page_sha256 = sha256(primitives, &primitives.encode_sorted_json(&unsigned)?);
    let JsonValue::Object(mut complete) = unsigned else {
        return None;
    };
    complete.push(member("pageSha256", digest_value(page_sha256)));
    let complete_bytes = primitives.encode_sorted_json(&JsonValue::Object(complete))?;
    Some(PageMaterial {
        complete_bytes,
        member_root_sha256,
        page_sha256,
    })
}

/// Map one exact retained member into the source page entry envelope.
fn entry_value(
    primitives: &impl ArchiveWirePrimitives,
    entry: &RetainedRevisionGapEntry<NativeRevisionGapMember>,
) -> Option<JsonValue> {
    let value = &entry.value;
    let gap = canonical_value(primitives, &value.gap_bytes)?;
    let stage_receipt = canonical_value(primitives, &value.stage_receipt_bytes)?;
    // bind typed member labels to their exact retained byte preimages
    if value.payload_sha256 != sha256(primitives, &value.payload_bytes)
        || value.gap_member_sha256 != sha256(primitives, &value.gap_bytes)
        || value.stage_receipt_member_sha256 != sha256(primitives, &value.stage_receipt_bytes)
        || text_field(&gap, "gapSha256")? != digest_text(entry.gap_sha256)
        || (value.kind == NativeRevisionGapPayloadKind::RainGateControl)
            != value.rain_control.is_some()
        || (!matches!(
            value.kind,
            NativeRevisionGapPayloadKind::ShadowStageV1
                | NativeRevisionGapPayloadKind::ShadowStageV2
        ) && value.slot_sha256 != value.payload_sha256)
    {
        return None;
    }
    let payload = JsonValue::Object(vec![
        member(
            "bytesBase64",
            string(&primitives.encode_standard_base64(&value.payload_bytes)?),
        ),
        member("identitySha256", digest_value(value.payload_sha256)),
        member("kind", string(value.kind.wire_value())),
    ]);
    let mut fields = vec![
        member("gap", gap),
        member("gapMemberSha256", digest_value(value.gap_member_sha256)),
        member("payload", payload),
    ];
    // retain the supplementary public state only for the control projection
    if let Some(rain) = &value.rain_control {
        let state_receipt = canonical_value(primitives, &rain.stage_receipt_bytes)?;
        // bind both supplementary member identities to their exact bytes
        if rain.state_sha256 != sha256(primitives, &rain.state_bytes)
            || rain.stage_receipt_member_sha256 != sha256(primitives, &rain.stage_receipt_bytes)
        {
            return None;
        }
        fields.push(member(
            "rainControlState",
            JsonValue::Object(vec![
                member(
                    "bytesBase64",
                    string(&primitives.encode_standard_base64(&rain.state_bytes)?),
                ),
                member("identitySha256", digest_value(rain.state_sha256)),
                member("kind", string("rain-maintenance-control-state/v1")),
                member("stateSha256", digest_value(rain.state_sha256)),
            ]),
        ));
        fields.push(member("rainControlStateStageReceipt", state_receipt));
        fields.push(member(
            "rainControlStateStageReceiptMemberSha256",
            digest_value(rain.stage_receipt_member_sha256),
        ));
    }
    fields.push(member(
        "slotIdentitySha256",
        digest_value(value.slot_sha256),
    ));
    fields.push(member("stageReceipt", stage_receipt));
    fields.push(member(
        "stageReceiptMemberSha256",
        digest_value(value.stage_receipt_member_sha256),
    ));
    Some(JsonValue::Object(fields))
}

/// Hash the exact stable graph-member identity vector for one page.
fn member_root_sha256(
    primitives: &impl ArchiveWirePrimitives,
    entries: &[RetainedRevisionGapEntry<NativeRevisionGapMember>],
) -> Option<RevisionGapSha256> {
    let identities = entries
        .iter()
        // retain only the member hashes used by the source root projection
        .map(member_identity_value)
        .collect::<Option<Vec<_>>>()?;
    Some(sha256(
        primitives,
        &primitives.encode_sorted_json(&JsonValue::Array(identities))?,
    ))
}

/// Project one retained entry into the exact source member-root shape.
fn member_identity_value(
    entry: &RetainedRevisionGapEntry<NativeRevisionGapMember>,
) -> Option<JsonValue> {
    let value = &entry.value;
    let mut fields = vec![
        member("gapMemberSha256", digest_value(value.gap_member_sha256)),
        member("payloadMemberSha256", digest_value(value.payload_sha256)),
        member(
            "stageReceiptMemberSha256",
            digest_value(value.stage_receipt_member_sha256),
        ),
    ];
    // include both control-state graph members only when the entry carries them
    if let Some(state) = &value.rain_control {
        fields.push(member(
            "rainControlStateMemberSha256",
            digest_value(state.state_sha256),
        ));
        fields.push(member(
            "rainControlStateStageReceiptMemberSha256",
            digest_value(state.stage_receipt_member_sha256),
        ));
    }
    Some(JsonValue::Object(fields))
}

/// Build the unsigned start value in the source's field vocabulary.
fn unsigned_start_value(start: RevisionGapUnsignedStart) -> JsonValue {
    JsonValue::Object(vec![
        member("contractVersion", string(start.contract_version)),
        member("frontierSha256", digest_value(start.frontier_sha256)),
        member("pageCount", string(&start.page_count.to_string())),
    ])
}

/// Derive the next non-ordinal frontier from the source newline preimage.
fn acknowledgement_frontier_sha256(
    primitives: &impl ArchiveWirePrimitives,
    acknowledgement: RevisionGapUnsignedAcknowledgement<'_>,
) -> RevisionGapSha256 {
    let preimage = format!(
        "{}\n{}\n{}\n{}\n{}",
        acknowledgement.contract_version,
        digest_text(acknowledgement.predecessor_frontier_sha256),
        digest_text(acknowledgement.page_sha256),
        digest_text(acknowledgement.graph_manifest_sha256),
        digest_text(acknowledgement.member_root_sha256),
    );
    sha256(primitives, preimage.as_bytes())
}

/// Build the unsigned acknowledgement value with its derived frontier.
fn unsigned_acknowledgement_value(
    acknowledgement: RevisionGapUnsignedAcknowledgement<'_>,
    frontier_sha256: RevisionGapSha256,
) -> JsonValue {
    JsonValue::Object(vec![
        member("acknowledgedAt", string(acknowledgement.acknowledged_at)),
        member("contractVersion", string(acknowledgement.contract_version)),
        member("frontierSha256", digest_value(frontier_sha256)),
        member(
            "graphManifestSha256",
            digest_value(acknowledgement.graph_manifest_sha256),
        ),
        member(
            "memberRootSha256",
            digest_value(acknowledgement.member_root_sha256),
        ),
        member("pageCount", string(&acknowledgement.page_count.to_string())),
        member("pageSha256", digest_value(acknowledgement.page_sha256)),
        member(
            "predecessorFrontierSha256",
            digest_value(acknowledgement.predecessor_frontier_sha256),
        ),
    ])
}

/// Decode one embedded document only when its original bytes are canonical sorted JSON.
fn canonical_value(primitives: &impl ArchiveWirePrimitives, bytes: &[u8]) -> Option<JsonValue> {
    let value = primitives.decode_json(bytes)?;
    (primitives.encode_sorted_json(&value)?.as_slice() == bytes).then_some(value)
}

/// Read one exact UTF-16 string field.
fn text_field(value: &JsonValue, name: &str) -> Option<String> {
    let JsonValue::String(value) = field(value, name)? else {
        return None;
    };
    String::from_utf16(value).ok()
}

/// Borrow one object field without changing its UTF-16 spelling.
fn field<'a>(value: &'a JsonValue, name: &str) -> Option<&'a JsonValue> {
    let JsonValue::Object(fields) = value else {
        return None;
    };
    fields
        .iter()
        .find_map(|(key, value)| key.iter().copied().eq(name.encode_utf16()).then_some(value))
}

/// Represent one exact object member through the generated JSON contract.
fn member(name: &str, value: JsonValue) -> (Vec<u16>, JsonValue) {
    (name.encode_utf16().collect(), value)
}

/// Represent one exact string through the generated JSON contract.
fn string(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}

/// Represent one fixed digest as its lowercase wire string.
fn digest_value(value: RevisionGapSha256) -> JsonValue {
    string(&digest_text(value))
}

/// Render one fixed digest in the exact lowercase hexadecimal alphabet.
fn digest_text(value: RevisionGapSha256) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut text = String::with_capacity(64);
    // emit both nibbles of every fixed digest byte
    for byte in value.as_bytes() {
        text.push(char::from(HEX[usize::from(byte >> 4)]));
        text.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    text
}

/// Hash exact bytes into the fixed revision-gap digest type.
fn sha256(primitives: &impl ArchiveWirePrimitives, bytes: &[u8]) -> RevisionGapSha256 {
    RevisionGapSha256::admitted(primitives.sha256(bytes))
}
