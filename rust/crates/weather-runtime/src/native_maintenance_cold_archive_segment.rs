//! Exact source-native archive segment for one verified revision cold page.

use std::fmt::{self, Display, Formatter};

use weather_contracts::{FieldPresence, JsonValue};
use weather_edge::{
    ArchiveWireCodec, ArchiveWirePrimitives, PrivateShadowArchiveVersion,
    PrivateShadowReceiptCodec, RevisionArchiveCanonicalCodec, RevisionColdAckPage,
    RevisionGapSha256, derive_revision_cold_ack_page,
};

use crate::NativeWirePrimitives;
use crate::maintenance_canonical_json::{field, text};
use crate::native_maintenance_archive_segment::{
    NativeMaintenanceArchiveSegment, NativeMaintenanceArchiveSegmentMember,
};
use crate::native_maintenance_graph_manifest::NativeMaintenanceGraphCrossLink;
use crate::native_maintenance_retirement_files_v2::format_digest;
use crate::native_maintenance_revision_cold_drain::{
    NativeRevisionColdPublicationDisposition, VerifiedNativeRevisionColdPage,
    VerifiedNativeRevisionColdStart,
};

const CHECKPOINT_KIND: &str = "adjustment-revision-cold-page-checkpoint/v1";
const START_KIND: &str = "adjustment-revision-cold-transfer-start/v1";
const COMPARATOR_KIND: &str = "adjustment-shadow-incumbent-comparator/v1";

/// Exact source due identity, checkpoint proof, and immutable member closure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeMaintenanceColdArchiveSegment {
    pub due_key: String,
    pub member_root_sha256: String,
    pub start_member_sha256: Option<String>,
    pub segment: NativeMaintenanceArchiveSegment,
}

/// Closed segment-composition refusal categories with no retained evidence reflection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMaintenanceColdArchiveSegmentError {
    Page,
    Start,
    Binding,
    Encoding,
}

impl Display for NativeMaintenanceColdArchiveSegmentError {
    /// Render one stable composition boundary.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        // keep malformed evidence details out of operational diagnostics
        formatter.write_str(match self {
            Self::Page => "maintenance cold archive page is invalid",
            Self::Start => "maintenance cold archive start is invalid",
            Self::Binding => "maintenance cold archive binding differs",
            Self::Encoding => "maintenance cold archive encoding failed",
        })
    }
}

impl std::error::Error for NativeMaintenanceColdArchiveSegmentError {}

struct ColdEntrySegment {
    checkpoint: JsonValue,
    root: JsonValue,
    members: Vec<NativeMaintenanceArchiveSegmentMember>,
    cross_links: Vec<NativeMaintenanceGraphCrossLink>,
}

/// Reverify one admitted page/start pair and reproduce the predecessor graph segment exactly.
pub fn build_native_maintenance_cold_archive_segment(
    page: &VerifiedNativeRevisionColdPage,
    start: Option<&VerifiedNativeRevisionColdStart>,
) -> Result<NativeMaintenanceColdArchiveSegment, NativeMaintenanceColdArchiveSegmentError> {
    use NativeMaintenanceColdArchiveSegmentError as Error;

    let wire = NativeWirePrimitives::new();
    verify_page_projection(page, &wire)?;
    let first_page = page.request.previous_page_sha256 == page.request.start_sha256;
    // retain the start member only at the exact first-page boundary
    let start_member_sha256 = match (first_page, start) {
        (true, Some(start)) => Some(verify_start_projection(page, start, &wire)?),
        (false, None) => None,
        (true, None) => return Err(Error::Start),
        (false, Some(_)) => return Err(Error::Binding),
    };
    let codec = ArchiveWireCodec::new(wire);
    let admitted = derive_revision_cold_ack_page(
        &page.canonical_bytes,
        start_member_sha256
            .as_deref()
            .and_then(RevisionGapSha256::from_lower_hex),
        &codec,
    )
    .map_err(|_| Error::Page)?;
    verify_admitted_projection(page, &admitted)?;

    let page_identity = parse_digest(&page.page_sha256).ok_or(Error::Page)?;
    let mut checkpoint_entries = Vec::with_capacity(page.entries.len());
    let mut root_entries = Vec::with_capacity(page.entries.len());
    let mut members = Vec::new();
    let mut cross_links = Vec::new();
    // retain source entry order because it is the database successor order
    for entry in &page.entries {
        let projected = build_entry_segment(&entry.value, &page.page_sha256, &codec)?;
        checkpoint_entries.push(projected.checkpoint);
        root_entries.push(projected.root);
        members.extend(projected.members);
        cross_links.extend(projected.cross_links);
    }
    let root_value = object(vec![
        ("entries", JsonValue::Array(root_entries)),
        (
            "startMemberSha256",
            nullable_string(start_member_sha256.as_deref()),
        ),
    ]);
    let root_bytes = encode(&wire, &root_value)?;
    let member_root_sha256 = digest(&wire, &root_bytes);
    // require the independent source-backed page derivation to agree on the complete member root
    if member_root_sha256 != format_digest(*admitted.member_root_sha256.as_bytes()) {
        return Err(Error::Binding);
    }
    let checkpoint = object(vec![
        (
            "afterArchiveCommitOrdinal",
            string(&page.request.after_archive_commit_ordinal),
        ),
        (
            "afterFrontierSha256",
            string(&page.request.after_frontier_sha256),
        ),
        ("contractVersion", string(CHECKPOINT_KIND)),
        ("entries", JsonValue::Array(checkpoint_entries)),
        ("eof", JsonValue::Boolean(page.eof)),
        ("memberRootSha256", string(&member_root_sha256)),
        (
            "nextArchiveCommitOrdinal",
            string(&page.next_archive_commit_ordinal),
        ),
        ("nextFrontierSha256", string(&page.next_frontier_sha256)),
        ("pageSha256", string(&page.page_sha256)),
        (
            "previousPageSha256",
            string(&page.request.previous_page_sha256),
        ),
        (
            "startMemberSha256",
            nullable_string(start_member_sha256.as_deref()),
        ),
        ("startSha256", string(&page.request.start_sha256)),
        (
            "watermarkArchiveCommitOrdinal",
            string(&page.request.watermark_archive_commit_ordinal),
        ),
        (
            "watermarkFrontierSha256",
            string(&page.request.watermark_frontier_sha256),
        ),
    ]);
    let checkpoint_bytes = encode(&wire, &checkpoint)?;
    let mut leading_members = vec![NativeMaintenanceArchiveSegmentMember {
        kind: CHECKPOINT_KIND.to_owned(),
        identity_sha256: page_identity,
        payload: checkpoint_bytes,
    }];
    // include the transfer snapshot exactly once at the first page boundary
    if let Some(start) = start {
        leading_members.push(NativeMaintenanceArchiveSegmentMember {
            kind: START_KIND.to_owned(),
            identity_sha256: parse_digest(&start.start_sha256).ok_or(Error::Start)?,
            payload: start.canonical_bytes.clone(),
        });
        cross_links.insert(
            0,
            link(
                &page.page_sha256,
                "binds_transfer_start",
                &start.start_sha256,
            ),
        );
    }
    leading_members.extend(members);
    Ok(NativeMaintenanceColdArchiveSegment {
        due_key: format!("archive/revision-cold-page/{}", page.page_sha256),
        member_root_sha256,
        start_member_sha256,
        segment: NativeMaintenanceArchiveSegment {
            members: leading_members,
            cross_links,
        },
    })
}

/// Bind the retained typed page projection back to its exact canonical value.
fn verify_page_projection(
    page: &VerifiedNativeRevisionColdPage,
    wire: &NativeWirePrimitives,
) -> Result<(), NativeMaintenanceColdArchiveSegmentError> {
    use NativeMaintenanceColdArchiveSegmentError as Error;

    let entries = array(&page.value, "entries").ok_or(Error::Page)?;
    // prevent a caller from swapping typed coordinates around an otherwise valid page value
    if encode(wire, &page.value)? != page.canonical_bytes
        || text(&page.value, "afterArchiveCommitOrdinal").as_deref()
            != Some(page.request.after_archive_commit_ordinal.as_str())
        || text(&page.value, "afterFrontierSha256").as_deref()
            != Some(page.request.after_frontier_sha256.as_str())
        || text(&page.value, "previousPageSha256").as_deref()
            != Some(page.request.previous_page_sha256.as_str())
        || text(&page.value, "startSha256").as_deref() != Some(page.request.start_sha256.as_str())
        || text(&page.value, "watermarkArchiveCommitOrdinal").as_deref()
            != Some(page.request.watermark_archive_commit_ordinal.as_str())
        || text(&page.value, "watermarkFrontierSha256").as_deref()
            != Some(page.request.watermark_frontier_sha256.as_str())
        || text(&page.value, "nextArchiveCommitOrdinal").as_deref()
            != Some(page.next_archive_commit_ordinal.as_str())
        || text(&page.value, "nextFrontierSha256").as_deref()
            != Some(page.next_frontier_sha256.as_str())
        || text(&page.value, "pageSha256").as_deref() != Some(page.page_sha256.as_str())
        || boolean(&page.value, "eof") != Some(page.eof)
        || entries.len() != page.entries.len()
    {
        return Err(Error::Binding);
    }
    // bind every retained semantic disposition to the same complete entry value
    for (value, entry) in entries.iter().zip(&page.entries) {
        if value != &entry.value
            || text(
                field(value, "publication").ok_or(Error::Page)?,
                "disposition",
            )
            .as_deref()
                != Some(entry.publication_disposition.as_str())
        {
            return Err(Error::Binding);
        }
    }
    Ok(())
}

/// Bind a first-page start member to the page watermark and exact retained bytes.
fn verify_start_projection(
    page: &VerifiedNativeRevisionColdPage,
    start: &VerifiedNativeRevisionColdStart,
    wire: &NativeWirePrimitives,
) -> Result<String, NativeMaintenanceColdArchiveSegmentError> {
    use NativeMaintenanceColdArchiveSegmentError as Error;

    // retain the independently verified start only when every typed projection remains exact
    if encode(wire, &start.value)? != start.canonical_bytes
        || text(&start.value, "contractVersion").as_deref() != Some(START_KIND)
        || text(&start.value, "startSha256").as_deref() != Some(start.start_sha256.as_str())
        || text(&start.value, "watermarkArchiveCommitOrdinal").as_deref()
            != Some(start.watermark_archive_commit_ordinal.as_str())
        || text(&start.value, "watermarkFrontierSha256").as_deref()
            != Some(start.watermark_frontier_sha256.as_str())
        || start.start_sha256 != page.request.start_sha256
        || start.watermark_archive_commit_ordinal != page.request.watermark_archive_commit_ordinal
        || start.watermark_frontier_sha256 != page.request.watermark_frontier_sha256
    {
        return Err(Error::Binding);
    }
    Ok(digest(wire, &start.canonical_bytes))
}

/// Compare the independent weather-edge page proof with the caller's typed coordinates.
fn verify_admitted_projection(
    page: &VerifiedNativeRevisionColdPage,
    admitted: &RevisionColdAckPage,
) -> Result<(), NativeMaintenanceColdArchiveSegmentError> {
    use NativeMaintenanceColdArchiveSegmentError as Error;

    // reject any typed wrapper that differs from the independently decoded page proof
    if admitted.entry_count != page.entries.len()
        || admitted.request.watermark.ordinal.to_string()
            != page.request.watermark_archive_commit_ordinal
        || format_digest(*admitted.request.watermark.frontier.as_bytes())
            != page.request.watermark_frontier_sha256
        || format_digest(*admitted.request.start.as_bytes()) != page.request.start_sha256
        || admitted.request.after.ordinal.to_string() != page.request.after_archive_commit_ordinal
        || format_digest(*admitted.request.after.frontier.as_bytes())
            != page.request.after_frontier_sha256
        || format_digest(*admitted.request.previous.as_bytes()) != page.request.previous_page_sha256
        || admitted.next.ordinal.to_string() != page.next_archive_commit_ordinal
        || format_digest(*admitted.next.frontier.as_bytes()) != page.next_frontier_sha256
        || format_digest(*admitted.page_sha256.as_bytes()) != page.page_sha256
    {
        return Err(Error::Binding);
    }
    Ok(())
}

/// Reproduce one source entry's checkpoint, members, and receipt crosslinks.
fn build_entry_segment(
    entry: &JsonValue,
    page_sha256: &str,
    codec: &ArchiveWireCodec<NativeWirePrimitives>,
) -> Result<ColdEntrySegment, NativeMaintenanceColdArchiveSegmentError> {
    use NativeMaintenanceColdArchiveSegmentError as Error;

    let wire = codec.primitives();
    let payload = field(entry, "payload").ok_or(Error::Page)?;
    let payload_kind = text(payload, "kind").ok_or(Error::Page)?;
    let payload_identity = text(payload, "identitySha256").ok_or(Error::Page)?;
    let payload_bytes = decode_body(wire, payload)?;
    let stage = field(entry, "stageReceipt").ok_or(Error::Page)?;
    let stage_bytes = encode(wire, stage)?;
    let stage_identity = text(stage, "stageReceiptSha256").ok_or(Error::Page)?;
    let stage_kind = text(stage, "contractVersion").ok_or(Error::Page)?;
    let publication = field(entry, "publication").ok_or(Error::Page)?;
    let publication_value = field(publication, "value").ok_or(Error::Page)?;
    let publication_bytes = encode(wire, publication_value)?;
    let disposition = text(publication, "disposition").ok_or(Error::Page)?;
    // select the producer-issued identity for the two closed publication outcomes
    let publication_identity = match disposition.as_str() {
        "published" => text(publication_value, "publishReceiptSha256"),
        "committed_unpublished_gap" => text(publication_value, "gapSha256"),
        _ => None,
    }
    .ok_or(Error::Page)?;
    let publication_kind = text(publication_value, "contractVersion").ok_or(Error::Page)?;
    let batch = field(entry, "receipts").is_some();
    // preserve the source's singular versus grouped receipt shape
    let receipts = if batch {
        array(entry, "receipts").ok_or(Error::Page)?.to_vec()
    } else {
        vec![field(entry, "receipt").ok_or(Error::Page)?.clone()]
    };
    // preserve the corresponding singular versus grouped successor shape
    let successors = if batch {
        array(entry, "successors").ok_or(Error::Page)?.to_vec()
    } else {
        vec![field(entry, "successor").ok_or(Error::Page)?.clone()]
    };
    // preserve the validated one-to-one receipt and successor population
    if receipts.is_empty() || receipts.len() != successors.len() {
        return Err(Error::Page);
    }

    let comparator = comparator_material(&payload_kind, &payload_bytes, codec)?;
    // project control-state auxiliaries only for the exact control payload kind
    let rain = if payload_kind == "adjustment-rain-gate-control-projection/v3" {
        Some(rain_material(entry, wire)?)
    } else {
        None
    };
    let mut checkpoint_fields = Vec::new();
    let mut root_fields = Vec::new();
    // add the v2 comparator without changing the legacy checkpoint shape
    if let Some((identity, _)) = &comparator {
        checkpoint_fields.push(("comparatorMemberSha256", string(identity)));
        root_fields.push(("comparatorMemberSha256", string(identity)));
    }
    checkpoint_fields.extend([
        ("payloadIdentitySha256", string(&payload_identity)),
        ("payloadKind", string(&payload_kind)),
        ("publicationDisposition", string(&disposition)),
        ("publicationIdentitySha256", string(&publication_identity)),
        (
            "publicationMemberSha256",
            string(&digest(wire, &publication_bytes)),
        ),
        (
            "stageReceiptMemberSha256",
            string(&digest(wire, &stage_bytes)),
        ),
        ("stageReceiptSha256", string(&stage_identity)),
    ]);
    root_fields.extend([
        ("payloadMemberSha256", string(&payload_identity)),
        ("publicationDisposition", string(&disposition)),
        ("publicationIdentitySha256", string(&publication_identity)),
        (
            "publicationMemberSha256",
            string(&digest(wire, &publication_bytes)),
        ),
    ]);
    // add the two control-state members as one inseparable auxiliary pair
    if let Some(rain) = &rain {
        checkpoint_fields.extend([
            (
                "rainControlStateIdentitySha256",
                string(&rain.identity_sha256),
            ),
            ("rainControlStateSha256", string(&rain.state_sha256)),
            (
                "rainControlStateStageReceiptMemberSha256",
                string(&digest(wire, &rain.stage_bytes)),
            ),
            (
                "rainControlStateStageReceiptSha256",
                string(&rain.stage_identity_sha256),
            ),
        ]);
        root_fields.extend([
            (
                "rainControlStateMemberSha256",
                string(&rain.identity_sha256),
            ),
            (
                "rainControlStateStageReceiptMemberSha256",
                string(&digest(wire, &rain.stage_bytes)),
            ),
        ]);
    }

    let receipt_bytes = receipts
        .iter()
        .map(|value| encode(wire, value))
        .collect::<Result<Vec<_>, _>>()?;
    let successor_bytes = successors
        .iter()
        .map(|value| encode(wire, value))
        .collect::<Result<Vec<_>, _>>()?;
    let receipt_member_hashes: Vec<_> = receipt_bytes
        .iter()
        .map(|bytes| digest(wire, bytes))
        .collect();
    let receipt_identities = receipts
        .iter()
        .map(|value| text(value, "receiptSha256").ok_or(Error::Page))
        .collect::<Result<Vec<_>, _>>()?;
    let successor_hashes: Vec<_> = successor_bytes
        .iter()
        .map(|bytes| digest(wire, bytes))
        .collect();
    // keep the original singular versus grouped checkpoint grammar unchanged
    if batch {
        checkpoint_fields.extend([
            ("receiptMemberSha256s", string_array(&receipt_member_hashes)),
            ("receiptSha256s", string_array(&receipt_identities)),
            ("successorMemberSha256s", string_array(&successor_hashes)),
            ("successorSha256s", string_array(&successor_hashes)),
        ]);
        root_fields.extend([
            ("receiptMemberSha256s", string_array(&receipt_member_hashes)),
            (
                "stageReceiptMemberSha256",
                string(&digest(wire, &stage_bytes)),
            ),
            ("successorMemberSha256s", string_array(&successor_hashes)),
        ]);
    } else {
        checkpoint_fields.extend([
            ("receiptMemberSha256", string(&receipt_member_hashes[0])),
            ("receiptSha256", string(&receipt_identities[0])),
            ("successorMemberSha256", string(&successor_hashes[0])),
            ("successorSha256", string(&successor_hashes[0])),
        ]);
        root_fields.extend([
            ("receiptMemberSha256", string(&receipt_member_hashes[0])),
            (
                "stageReceiptMemberSha256",
                string(&digest(wire, &stage_bytes)),
            ),
            ("successorMemberSha256", string(&successor_hashes[0])),
        ]);
    }

    let mut members = vec![
        member(&payload_kind, &payload_identity, payload_bytes)?,
        member(&stage_kind, &stage_identity, stage_bytes)?,
        member(&publication_kind, &publication_identity, publication_bytes)?,
    ];
    // append rain-control state bytes in the same source order
    if let Some(rain) = &rain {
        members.push(member(
            &rain.kind,
            &rain.identity_sha256,
            rain.state_bytes.clone(),
        )?);
        members.push(member(
            &rain.stage_kind,
            &rain.stage_identity_sha256,
            rain.stage_bytes.clone(),
        )?);
    }
    // append the decoded comparator only for an admitted v2 capsule
    if let Some((identity, bytes)) = &comparator {
        members.push(member(COMPARATOR_KIND, identity, bytes.clone())?);
    }
    let mut cross_links = Vec::new();
    // bind every receipt to the shared body and its complete auxiliary closure
    for (((receipt, receipt_bytes), successor_bytes), successor_sha256) in receipts
        .iter()
        .zip(receipt_bytes)
        .zip(successor_bytes)
        .zip(successor_hashes)
    {
        let receipt_identity = text(receipt, "receiptSha256").ok_or(Error::Page)?;
        let receipt_kind = text(receipt, "contractVersion").ok_or(Error::Page)?;
        let successor = wire.decode_json(&successor_bytes).ok_or(Error::Page)?;
        let successor_kind = text(&successor, "contractVersion").ok_or(Error::Page)?;
        members.push(member(&receipt_kind, &receipt_identity, receipt_bytes)?);
        members.push(member(&successor_kind, &successor_sha256, successor_bytes)?);
        cross_links.extend([
            link(page_sha256, "contains_receipt", &receipt_identity),
            link(&receipt_identity, "binds_payload", &payload_identity),
            link(&receipt_identity, "binds_stage_receipt", &stage_identity),
            link(
                &receipt_identity,
                // keep the source relation spelling tied to the admitted disposition
                if disposition == NativeRevisionColdPublicationDisposition::Published.as_str() {
                    "binds_publish_receipt"
                } else {
                    "binds_terminal_gap"
                },
                &publication_identity,
            ),
            link(&receipt_identity, "binds_successor", &successor_sha256),
        ]);
        // bind the additive v2 comparator to every receipt in its capsule group
        if let Some((identity, _)) = &comparator {
            cross_links.push(link(
                &receipt_identity,
                "binds_incumbent_comparator",
                identity,
            ));
        }
        // bind an admitted control receipt to both public state members
        if let Some(rain) = &rain {
            cross_links.extend([
                link(
                    &receipt_identity,
                    "binds_rain_control_state",
                    &rain.identity_sha256,
                ),
                link(
                    &receipt_identity,
                    "binds_rain_control_state_stage_receipt",
                    &rain.stage_identity_sha256,
                ),
            ]);
        }
    }
    Ok(ColdEntrySegment {
        checkpoint: object(checkpoint_fields),
        root: object(root_fields),
        members,
        cross_links,
    })
}

struct RainMaterial {
    identity_sha256: String,
    kind: String,
    stage_bytes: Vec<u8>,
    stage_identity_sha256: String,
    stage_kind: String,
    state_bytes: Vec<u8>,
    state_sha256: String,
}

/// Project the exact already-admitted control state and stage member bytes.
fn rain_material(
    entry: &JsonValue,
    wire: &NativeWirePrimitives,
) -> Result<RainMaterial, NativeMaintenanceColdArchiveSegmentError> {
    use NativeMaintenanceColdArchiveSegmentError as Error;

    let state = field(entry, "rainControlState").ok_or(Error::Page)?;
    let stage = field(entry, "rainControlStateStageReceipt").ok_or(Error::Page)?;
    Ok(RainMaterial {
        identity_sha256: text(state, "identitySha256").ok_or(Error::Page)?,
        kind: text(state, "kind").ok_or(Error::Page)?,
        stage_bytes: encode(wire, stage)?,
        stage_identity_sha256: text(stage, "stageReceiptSha256").ok_or(Error::Page)?,
        stage_kind: text(stage, "contractVersion").ok_or(Error::Page)?,
        state_bytes: decode_body(wire, state)?,
        state_sha256: text(state, "stateSha256").ok_or(Error::Page)?,
    })
}

/// Decode the separate comparator member from an exact v2 shadow capsule.
fn comparator_material(
    payload_kind: &str,
    payload_bytes: &[u8],
    codec: &ArchiveWireCodec<NativeWirePrimitives>,
) -> Result<Option<(String, Vec<u8>)>, NativeMaintenanceColdArchiveSegmentError> {
    use NativeMaintenanceColdArchiveSegmentError as Error;

    // preserve comparator membership only for the additive v2 shadow grammar
    if payload_kind != "adjustment-shadow-revision-capsule/v2" {
        return Ok(None);
    }
    let capsule = codec
        .decode_capsule_document(payload_bytes)
        .ok_or(Error::Page)?;
    let FieldPresence::Value(bytes) = capsule.stage_input.comparator else {
        return Err(Error::Page);
    };
    // reject any decoder result that does not retain the additive capsule version
    if capsule.version != PrivateShadowArchiveVersion::V2 {
        return Err(Error::Page);
    }
    Ok(Some((digest(codec.primitives(), &bytes), bytes)))
}

/// Build one typed graph member from a lowercase source identity.
fn member(
    kind: &str,
    identity: &str,
    payload: Vec<u8>,
) -> Result<NativeMaintenanceArchiveSegmentMember, NativeMaintenanceColdArchiveSegmentError> {
    Ok(NativeMaintenanceArchiveSegmentMember {
        kind: kind.to_owned(),
        identity_sha256: parse_digest(identity)
            .ok_or(NativeMaintenanceColdArchiveSegmentError::Page)?,
        payload,
    })
}

/// Build one exact semantic graph relation without normalizing its identities.
fn link(from: &str, relation: &str, to: &str) -> NativeMaintenanceGraphCrossLink {
    NativeMaintenanceGraphCrossLink {
        from_identity_sha256: from.to_owned(),
        relation: relation.to_owned(),
        to_identity_sha256: to.to_owned(),
    }
}

/// Decode one standard base64 body and require its canonical spelling.
fn decode_body(
    wire: &NativeWirePrimitives,
    value: &JsonValue,
) -> Result<Vec<u8>, NativeMaintenanceColdArchiveSegmentError> {
    use NativeMaintenanceColdArchiveSegmentError as Error;

    let encoded = text(value, "bytesBase64").ok_or(Error::Page)?;
    let bytes = wire.decode_standard_base64(&encoded).ok_or(Error::Page)?;
    // reject alternate standard-base64 spellings even after semantic page validation
    if wire.encode_standard_base64(&bytes).as_deref() != Some(encoded.as_str()) {
        return Err(Error::Page);
    }
    Ok(bytes)
}

/// Encode one recursively sorted LF-framed source document.
fn encode(
    wire: &NativeWirePrimitives,
    value: &JsonValue,
) -> Result<Vec<u8>, NativeMaintenanceColdArchiveSegmentError> {
    wire.encode_sorted_json(value)
        .ok_or(NativeMaintenanceColdArchiveSegmentError::Encoding)
}

/// Hash exact retained bytes into lowercase source spelling.
fn digest(wire: &impl RevisionArchiveCanonicalCodec, bytes: &[u8]) -> String {
    format_digest(wire.sha256(bytes))
}

/// Decode one exact lowercase digest for typed archive storage.
fn parse_digest(value: &str) -> Option<[u8; 32]> {
    Some(*RevisionGapSha256::from_lower_hex(value)?.as_bytes())
}

/// Borrow one exact JSON array without scalar coercion.
fn array<'a>(value: &'a JsonValue, name: &str) -> Option<&'a [JsonValue]> {
    // refuse scalar coercion at the retained JSON boundary
    match field(value, name)? {
        JsonValue::Array(values) => Some(values),
        _ => None,
    }
}

/// Borrow one exact JSON boolean without truthy coercion.
fn boolean(value: &JsonValue, name: &str) -> Option<bool> {
    // refuse truthy coercion at the retained JSON boundary
    match field(value, name)? {
        JsonValue::Boolean(value) => Some(*value),
        _ => None,
    }
}

/// Construct one source-shaped JSON object using unnormalized UTF-16 keys.
fn object(fields: Vec<(&str, JsonValue)>) -> JsonValue {
    JsonValue::Object(
        fields
            .into_iter()
            .map(|(name, value)| (name.encode_utf16().collect(), value))
            .collect(),
    )
}

/// Construct one generated JSON string without an intermediary parser.
fn string(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}

/// Preserve source null semantics for the first-page-only start member.
fn nullable_string(value: Option<&str>) -> JsonValue {
    value.map_or(JsonValue::Null, string)
}

/// Project an ordered digest vector without changing receipt order.
fn string_array(values: &[String]) -> JsonValue {
    JsonValue::Array(values.iter().map(|value| string(value)).collect())
}

#[cfg(test)]
#[path = "native_maintenance_cold_archive_segment_test.rs"]
mod tests;
