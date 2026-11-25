//! Native workstation gap checkpoint construction and verified publication-before-ack drain.

use crate::{
    NativeMaintenanceArchivePublication, NativeMaintenanceArchiveSegment,
    NativeMaintenanceArchiveSegmentMember, NativeMaintenanceGraphCrossLink, NativeWirePrimitives,
    maintenance_canonical_json::{exact_keys, text},
    native_maintenance_retirement_files_v2::format_digest,
};
use weather_contracts::JsonValue;
use weather_edge::{
    ArchiveWirePrimitives, NativeRevisionGapMember, REVISION_GAP_TRANSFER_START_CONTRACT_VERSION,
    RevisionGapPayloadAcknowledgement, RevisionGapPayloadPage, RevisionGapSha256,
    RevisionGapTransferStart,
};

const CHECKPOINT: &str = "adjustment-revision-gap-page-checkpoint/v1";

/// Read a compact source start only after the exact predecessor hash is rederived.
pub fn read_native_maintenance_gap_start(bytes: &[u8]) -> Option<RevisionGapTransferStart> {
    let wire = NativeWirePrimitives;
    let value = wire.decode_json(bytes)?;
    if !exact_keys(
        &value,
        &[
            "contractVersion",
            "frontierSha256",
            "pageCount",
            "startSha256",
        ],
    ) || text(&value, "contractVersion")? != REVISION_GAP_TRANSFER_START_CONTRACT_VERSION
    {
        return None;
    }
    let count = text(&value, "pageCount")?;
    let page_count = count.parse::<u64>().ok()?;
    if page_count.to_string() != count {
        return None;
    }
    let start = RevisionGapTransferStart {
        contract_version: REVISION_GAP_TRANSFER_START_CONTRACT_VERSION,
        frontier_sha256: RevisionGapSha256::from_lower_hex(&text(&value, "frontierSha256")?)?,
        page_count,
        start_sha256: RevisionGapSha256::from_lower_hex(&text(&value, "startSha256")?)?,
    };
    (crate::native_revision_gap_identity::encode_start(&wire, &start)?.as_slice() == bytes)
        .then_some(start)
}

/// Build every exact source gap checkpoint, body, stage receipt and auxiliary member.
pub fn build_native_maintenance_gap_segment(
    page_bytes: &[u8],
) -> Option<NativeMaintenanceArchiveSegment> {
    let wire = NativeWirePrimitives;
    let page = crate::native_revision_gap_decode::decode_page(&wire, page_bytes)?;
    // idle transfer pages never create a graph or acknowledgement
    if page.idle {
        return None;
    }
    let mut rows = Vec::with_capacity(page.entries.len());
    let mut members = Vec::with_capacity(1 + page.entries.len() * 5);
    let mut links = Vec::new();
    let page_digest = *page.page_sha256.as_bytes();
    for entry in &page.entries {
        let value = &entry.value;
        let gap = wire.decode_json(&value.gap_bytes)?;
        let stage = wire.decode_json(&value.stage_receipt_bytes)?;
        let gap_id = *entry.gap_sha256.as_bytes();
        let payload_id = *value.payload_sha256.as_bytes();
        let stage_id = identity(&stage, "stageReceiptSha256")?;
        let mut row = vec![
            member("gapMemberSha256", digest(value.gap_member_sha256)),
            member("gapSha256", digest(entry.gap_sha256)),
            member("payloadIdentitySha256", digest(value.payload_sha256)),
            member("payloadKind", string(value.kind.wire_value())),
            member("slotIdentitySha256", digest(value.slot_sha256)),
            member(
                "stageReceiptMemberSha256",
                digest(value.stage_receipt_member_sha256),
            ),
            member("stageReceiptSha256", string(&format_digest(stage_id))),
        ];
        members.extend([
            segment_member(
                text(&gap, "contractVersion")?,
                gap_id,
                value.gap_bytes.clone(),
            ),
            segment_member(
                value.kind.wire_value().to_owned(),
                payload_id,
                value.payload_bytes.clone(),
            ),
            segment_member(
                text(&stage, "contractVersion")?,
                stage_id,
                value.stage_receipt_bytes.clone(),
            ),
        ]);
        links.extend([
            link(page_digest, "contains_gap", gap_id),
            link(gap_id, "binds_unqualified_payload", payload_id),
            link(gap_id, "binds_stage_receipt", stage_id),
        ]);
        // preserve the public monthly control state and its separate exact receipt for this payload class
        if let Some(rain) = &value.rain_control {
            let receipt = wire.decode_json(&rain.stage_receipt_bytes)?;
            let receipt_id = identity(&receipt, "stageReceiptSha256")?;
            let state_id = *rain.state_sha256.as_bytes();
            row.extend([
                member("rainControlStateIdentitySha256", digest(rain.state_sha256)),
                member(
                    "rainControlStateStageReceiptMemberSha256",
                    digest(rain.stage_receipt_member_sha256),
                ),
                member(
                    "rainControlStateStageReceiptSha256",
                    string(&format_digest(receipt_id)),
                ),
            ]);
            members.extend([
                segment_member(
                    "rain-maintenance-control-state/v1".into(),
                    state_id,
                    rain.state_bytes.clone(),
                ),
                segment_member(
                    text(&receipt, "contractVersion")?,
                    receipt_id,
                    rain.stage_receipt_bytes.clone(),
                ),
            ]);
            links.extend([
                link(gap_id, "binds_rain_control_state", state_id),
                link(gap_id, "binds_rain_control_state_stage_receipt", receipt_id),
            ]);
        }
        rows.push(JsonValue::Object(row));
    }
    let checkpoint = JsonValue::Object(vec![
        member("contractVersion", string(CHECKPOINT)),
        member("entries", JsonValue::Array(rows)),
        member("memberRootSha256", digest(page.member_root_sha256)),
        member("pageSha256", digest(page.page_sha256)),
        member(
            "predecessorFrontierSha256",
            digest(page.predecessor_frontier_sha256),
        ),
        member("startSha256", digest(page.start_sha256)),
    ]);
    // the validated page already rederived the same ordered member root from the retained bodies
    members.insert(
        0,
        segment_member(
            CHECKPOINT.into(),
            page_digest,
            wire.encode_sorted_json(&checkpoint)?,
        ),
    );
    Some(NativeMaintenanceArchiveSegment {
        members,
        cross_links: links,
    })
}

/// Actual bounded forced remote gap operations.
pub trait NativeMaintenanceGapRemotePort {
    type Error;
    /// Obtain the retained compact start response.
    fn fetch_start(&mut self) -> Result<Vec<u8>, Self::Error>;
    /// Fetch exactly one frozen page for the admitted start cursor and frontier.
    fn fetch_page(&mut self, start: RevisionGapTransferStart) -> Result<Vec<u8>, Self::Error>;
    /// Acknowledge only the exact page and its verified durable graph.
    fn acknowledge(
        &mut self,
        page: RevisionGapSha256,
        graph: [u8; 32],
    ) -> Result<Vec<u8>, Self::Error>;
}

/// Concrete durable publication boundary used before the original remote acknowledgement.
pub trait NativeMaintenanceGapPublicationPort {
    type Error;
    /// Publish and verify the segment using its exact original due key.
    fn publish(
        &mut self,
        due: &str,
        segment: &NativeMaintenanceArchiveSegment,
    ) -> Result<NativeMaintenanceArchivePublication, Self::Error>;
}

/// Verified result of one source gap drain.
#[derive(Debug, Eq, PartialEq)]
pub enum NativeMaintenanceGapDrainResult {
    Idle {
        page_sha256: RevisionGapSha256,
    },
    Captured {
        publication: NativeMaintenanceArchivePublication,
        acknowledgement: Box<RevisionGapPayloadAcknowledgement>,
    },
}

/// Closed malformed wire or actual port failure.
#[derive(Debug, Eq, PartialEq)]
pub enum NativeMaintenanceGapDrainError<Remote, Archive> {
    Start,
    Page,
    Segment,
    Acknowledgement,
    Remote(Remote),
    Archive(Archive),
}

/// Drain exactly one page; never acknowledge until the actual graph publication has completed.
pub fn drain_native_maintenance_revision_gap<
    R: NativeMaintenanceGapRemotePort,
    A: NativeMaintenanceGapPublicationPort,
>(
    remote: &mut R,
    archive: &mut A,
) -> Result<NativeMaintenanceGapDrainResult, NativeMaintenanceGapDrainError<R::Error, A::Error>> {
    use NativeMaintenanceGapDrainError as Error;
    let start = read_native_maintenance_gap_start(&remote.fetch_start().map_err(Error::Remote)?)
        .ok_or(Error::Start)?;
    let bytes = remote.fetch_page(start).map_err(Error::Remote)?;
    let page = crate::native_revision_gap_decode::decode_page(&NativeWirePrimitives, &bytes)
        .ok_or(Error::Page)?;
    // bind the response to the frozen source cursor before selecting the idle or archive branch
    if page.start_sha256 != start.start_sha256
        || page.predecessor_frontier_sha256 != start.frontier_sha256
    {
        return Err(Error::Page);
    }
    if page.idle {
        return Ok(NativeMaintenanceGapDrainResult::Idle {
            page_sha256: page.page_sha256,
        });
    }
    let segment = build_native_maintenance_gap_segment(&bytes).ok_or(Error::Segment)?;
    let due = format!(
        "archive/revision-gap-page/{}",
        format_digest(*page.page_sha256.as_bytes())
    );
    let publication = archive.publish(&due, &segment).map_err(Error::Archive)?;
    let ack_bytes = remote
        .acknowledge(page.page_sha256, publication.manifest_object_sha256)
        .map_err(Error::Remote)?;
    let acknowledgement = crate::native_revision_gap_decode::decode_acknowledgement(
        &NativeWirePrimitives,
        &ack_bytes,
    )
    .ok_or(Error::Acknowledgement)?;
    // require every retained source acknowledgement identity before reporting the captured graph
    if !matches_ack(&page, &publication, &acknowledgement) {
        return Err(Error::Acknowledgement);
    }
    Ok(NativeMaintenanceGapDrainResult::Captured {
        publication,
        acknowledgement: Box::new(acknowledgement),
    })
}

/// Cross-bind the server's durable acknowledgement to every exact source publication field.
fn matches_ack(
    page: &RevisionGapPayloadPage<NativeRevisionGapMember>,
    publication: &NativeMaintenanceArchivePublication,
    ack: &RevisionGapPayloadAcknowledgement,
) -> bool {
    ack.page_sha256 == page.page_sha256
        && *ack.graph_manifest_sha256.as_bytes() == publication.manifest_object_sha256
        && ack.member_root_sha256 == page.member_root_sha256
        && ack.predecessor_frontier_sha256 == page.predecessor_frontier_sha256
}

/// Construct one exact member address.
fn identity(value: &JsonValue, key: &str) -> Option<[u8; 32]> {
    Some(*RevisionGapSha256::from_lower_hex(&text(value, key)?)?.as_bytes())
}
/// Preserve a source segment member's exact body.
fn segment_member(
    kind: String,
    identity_sha256: [u8; 32],
    payload: Vec<u8>,
) -> NativeMaintenanceArchiveSegmentMember {
    NativeMaintenanceArchiveSegmentMember {
        kind,
        identity_sha256,
        payload,
    }
}
/// Convert one source crosslink into the path-free graph shape.
fn link(from: [u8; 32], relation: &str, to: [u8; 32]) -> NativeMaintenanceGraphCrossLink {
    NativeMaintenanceGraphCrossLink {
        from_identity_sha256: format_digest(from),
        relation: relation.into(),
        to_identity_sha256: format_digest(to),
    }
}
/// Bind a source JSON member.
fn member(key: &str, value: JsonValue) -> (Vec<u16>, JsonValue) {
    (key.encode_utf16().collect(), value)
}
/// Encode an exact digest scalar.
fn digest(value: RevisionGapSha256) -> JsonValue {
    string(&format_digest(*value.as_bytes()))
}
/// Encode a source JSON string.
fn string(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}

#[cfg(test)]
#[path = "native_maintenance_revision_gap_drain_test.rs"]
mod tests;
