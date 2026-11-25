//! Exact single-member receipt publication over the retained graph and lease owners.

use crate::{
    NativeMaintenanceArchiveLeaseRequest, NativeMaintenanceArchiveSegment,
    NativeMaintenanceArchiveSegmentArchive, NativeMaintenanceArchiveSegmentClock,
    NativeMaintenanceArchiveSegmentError, NativeMaintenanceArchiveSegmentJournal,
    NativeMaintenanceArchiveSegmentMember, NativeMaintenanceGraphEntry, NativeWirePrimitives,
    build_native_maintenance_graph_manifest, native_maintenance_retirement_files_v2::format_digest,
};
use weather_edge::RevisionArchiveCanonicalCodec;

/// One real durable graph and its archived member byte identity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeMaintenanceArchiveMemberPublication {
    pub manifest_object_sha256: [u8; 32],
    pub member_sha256: String,
}

/// Publish or reuse a single exact immutable member using its original archive-member lease.
pub fn publish_native_maintenance_archive_member<A, J, C>(
    archive: &mut A,
    journal: &mut J,
    clock: &mut C,
    member: NativeMaintenanceArchiveSegmentMember,
) -> Result<
    NativeMaintenanceArchiveMemberPublication,
    NativeMaintenanceArchiveSegmentError<A::Error, J::Error>,
>
where
    A: NativeMaintenanceArchiveSegmentArchive,
    J: NativeMaintenanceArchiveSegmentJournal,
    C: NativeMaintenanceArchiveSegmentClock,
{
    use NativeMaintenanceArchiveSegmentError as Error;
    // admit only a bounded exact source member kind and payload
    if !valid_kind(&member.kind)
        || member.payload.is_empty()
        || member.payload.len() > 8 * 1024 * 1024
    {
        return Err(Error::Input);
    }
    let identity = format_digest(member.identity_sha256);
    let due_key = format!("archive/member/{identity}");
    let run_id = format!("archive-member-{identity}");
    let due = journal.due(&due_key).map_err(Error::Journal)?;
    let existing_head = archive.read_head().map_err(Error::Archive)?;
    // completed membership is usable only when reachable in the real current graph chain
    if let Some(output) = due.output_sha256 {
        let graph = archive
            .find_member(existing_head, member.identity_sha256, &member.kind)
            .map_err(Error::Archive)?
            .filter(|graph| *graph == output)
            .ok_or(Error::CompletedSegmentUnavailable)?;
        let entry = read_graph_entry(archive, graph, &identity, &member.kind)
            .map_err(Error::Archive)?
            .ok_or(Error::CompletedSegmentUnavailable)?;
        return Ok(NativeMaintenanceArchiveMemberPublication {
            manifest_object_sha256: graph,
            member_sha256: entry.member_sha256,
        });
    }
    let status = journal.status().map_err(Error::Journal)?;
    let now = canonical_now(clock).ok_or(Error::Clock)?;
    // reconcile only matching expired membership already reachable from the current durable head
    if let Some(active) = status.active_lease {
        if active.due_key != due_key || active.run_id != run_id {
            return Err(Error::LeaseOccupied);
        }
        let expiry = NativeWirePrimitives
            .canonical_utc_milliseconds(&active.expires_at)
            .ok_or(Error::Clock)?;
        let instant = NativeWirePrimitives
            .canonical_utc_milliseconds(&now)
            .ok_or(Error::Clock)?;
        if expiry <= instant {
            let graph = archive
                .find_member(existing_head, member.identity_sha256, &member.kind)
                .map_err(Error::Archive)?
                .ok_or(Error::ReconciliationRequired)?;
            journal
                .reconcile_complete(&due_key, &run_id, &now, graph, existing_head)
                .map_err(Error::Journal)?;
        }
    }
    let refreshed = journal.status().map_err(Error::Journal)?;
    let input = refreshed.head_sha256.ok_or(Error::MissingJournalHead)?;
    let now = canonical_now(clock).ok_or(Error::Clock)?;
    journal
        .acquire(NativeMaintenanceArchiveLeaseRequest {
            due_key: &due_key,
            run_id: &run_id,
            now: &now,
            input_head_sha256: due.input_head_sha256.unwrap_or(input),
        })
        .map_err(Error::Journal)?;
    let result = (|| {
        let kind = member.kind.clone();
        let member_identity = member.identity_sha256;
        let segment = NativeMaintenanceArchiveSegment {
            members: vec![member],
            cross_links: Vec::new(),
        };
        let published = archive
            .publish_cas_object(&segment)
            .map_err(Error::Archive)?;
        let receipt = published
            .members
            .first()
            .filter(|_| published.members.len() == 1)
            .ok_or(Error::Graph)?;
        let expected_entry = NativeMaintenanceGraphEntry {
            kind: receipt.kind.clone(),
            identity_sha256: format_digest(receipt.identity_sha256),
            member_length: receipt.member_length,
            member_offset: receipt.member_offset,
            member_sha256: format_digest(receipt.member_sha256),
            object_sha256: format_digest(published.object_sha256),
        };
        // require exact CAS receipt binding before accepting existing graph membership
        if expected_entry.identity_sha256 != identity || expected_entry.kind != kind {
            return Err(Error::Graph);
        }
        let head = archive.read_head().map_err(Error::Archive)?;
        let graph = if let Some(existing) = archive
            .find_member(head, member_identity, &kind)
            .map_err(Error::Archive)?
        {
            let entry = read_graph_entry(archive, existing, &identity, &kind)
                .map_err(Error::Archive)?
                .ok_or(Error::Graph)?;
            if entry != expected_entry {
                return Err(Error::Graph);
            }
            existing
        } else {
            // create exactly one member graph after checking the real predecessor
            let manifest = build_native_maintenance_graph_manifest(
                head.map(format_digest),
                vec![expected_entry.clone()],
                Vec::new(),
            )
            .ok_or(Error::Graph)?;
            let graph = archive
                .publish_graph_manifest(&manifest)
                .map_err(Error::Archive)?;
            archive.verify_full_graph(graph).map_err(Error::Archive)?;
            archive.update_head(graph).map_err(Error::Archive)?;
            if archive.read_head().map_err(Error::Archive)? != Some(graph) {
                return Err(Error::HeadDiffers);
            }
            graph
        };
        journal
            .complete(&due_key, &canonical_now(clock).ok_or(Error::Clock)?, graph)
            .map_err(Error::Journal)?;
        journal
            .release(
                &due_key,
                &run_id,
                &canonical_now(clock).ok_or(Error::Clock)?,
            )
            .map_err(Error::Journal)?;
        Ok(NativeMaintenanceArchiveMemberPublication {
            manifest_object_sha256: graph,
            member_sha256: expected_entry.member_sha256,
        })
    })();
    // preserve the real publication failure while releasing only the same live lease
    if result.is_err()
        && let Some(now) = canonical_now(clock)
    {
        let _ = journal.release(&due_key, &run_id, &now);
    }
    result
}

// verify graph contents and return the exact graph membership receipt
fn read_graph_entry<A: NativeMaintenanceArchiveSegmentArchive>(
    archive: &mut A,
    graph: [u8; 32],
    identity: &str,
    kind: &str,
) -> Result<Option<NativeMaintenanceGraphEntry>, A::Error> {
    let verified = archive.verify_full_graph(graph)?;
    Ok(verified
        .manifest
        .entries
        .into_iter()
        .find(|entry| entry.identity_sha256 == identity && entry.kind == kind))
}

// check the original closed member spelling
fn valid_kind(kind: &str) -> bool {
    let bytes = kind.as_bytes();
    (1..=128).contains(&bytes.len())
        && bytes[0].is_ascii_alphanumeric()
        && bytes[1..]
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'/' | b'-'))
}

// sample and verify the source wall clock
fn canonical_now(clock: &mut impl NativeMaintenanceArchiveSegmentClock) -> Option<String> {
    let now = clock.now()?;
    NativeWirePrimitives.canonical_utc_milliseconds(&now)?;
    Some(now)
}

#[cfg(test)]
#[path = "native_maintenance_archive_member_test.rs"]
mod tests;
