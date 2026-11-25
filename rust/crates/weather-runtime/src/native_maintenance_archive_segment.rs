//! Exact archive segment publication and durable checkpoint reconciliation.

use crate::{
    NativeMaintenanceGraphCrossLink, NativeMaintenanceGraphEntry, NativeMaintenanceGraphManifest,
    NativeMaintenancePublishedCas, NativeMaintenanceVerifiedFullGraph,
    build_native_maintenance_graph_manifest, native_maintenance_retirement_files_v2::format_digest,
};
use weather_edge::RevisionArchiveCanonicalCodec;

const MAXIMUM_MEMBERS: usize = 13_000;
const MAXIMUM_CROSS_LINKS: usize = 40_000;

/// One ordered source segment member, including its exact retained body.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeMaintenanceArchiveSegmentMember {
    pub kind: String,
    pub identity_sha256: [u8; 32],
    pub payload: Vec<u8>,
}

/// One complete segment whose leading member identifies the archive checkpoint.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeMaintenanceArchiveSegment {
    pub members: Vec<NativeMaintenanceArchiveSegmentMember>,
    pub cross_links: Vec<NativeMaintenanceGraphCrossLink>,
}

/// Source archive publication result state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMaintenanceArchivePublicationState {
    AlreadyComplete,
    Reconciled,
    Published,
}

/// Actual durable manifest identity, never a merely attempted publication.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NativeMaintenanceArchivePublication {
    pub manifest_object_sha256: [u8; 32],
    pub state: NativeMaintenanceArchivePublicationState,
}

/// Project the single source due registration and immutable completion.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct NativeMaintenanceArchiveDue {
    pub input_head_sha256: Option<[u8; 32]>,
    pub output_sha256: Option<[u8; 32]>,
}

/// Active archive lease obtained from the verified journal.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeMaintenanceArchiveLease {
    pub due_key: String,
    pub run_id: String,
    pub expires_at: String,
}

/// Verified journal head and current archive mutation lease.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct NativeMaintenanceArchiveJournalStatus {
    pub head_sha256: Option<[u8; 32]>,
    pub active_lease: Option<NativeMaintenanceArchiveLease>,
}

/// One immutable archive lease request derived from the admitted due and checkpoint.
pub struct NativeMaintenanceArchiveLeaseRequest<'a> {
    pub due_key: &'a str,
    pub run_id: &'a str,
    pub now: &'a str,
    pub input_head_sha256: [u8; 32],
}

/// Exact archive-scope journal lifecycle, implemented by the actual durable state owner.
pub trait NativeMaintenanceArchiveSegmentJournal {
    type Error;
    /// Read the registered due, if present.
    fn due(&mut self, key: &str) -> Result<NativeMaintenanceArchiveDue, Self::Error>;
    /// Read the verified head and active archive lease.
    fn status(&mut self) -> Result<NativeMaintenanceArchiveJournalStatus, Self::Error>;
    /// Acquire one exact archive-scoped lease.
    fn acquire(
        &mut self,
        request: NativeMaintenanceArchiveLeaseRequest<'_>,
    ) -> Result<(), Self::Error>;
    /// Complete the due only after the durable graph is visible.
    fn complete(&mut self, key: &str, now: &str, output: [u8; 32]) -> Result<(), Self::Error>;
    /// Release only the selected live archive lease.
    fn release(&mut self, key: &str, run_id: &str, now: &str) -> Result<(), Self::Error>;
    /// Reconcile an expired lease using the actual reachable checkpoint and durable head.
    fn reconcile_complete(
        &mut self,
        key: &str,
        run_id: &str,
        now: &str,
        output: [u8; 32],
        head: Option<[u8; 32]>,
    ) -> Result<(), Self::Error>;
}

/// Actual immutable archive owner, including verified current head and predecessor traversal.
pub trait NativeMaintenanceArchiveSegmentArchive {
    type Error;
    /// Read the exact durable head pointer.
    fn read_head(&mut self) -> Result<Option<[u8; 32]>, Self::Error>;
    /// Verify one complete graph against all of its retained objects.
    fn verify_full_graph(
        &mut self,
        head: [u8; 32],
    ) -> Result<NativeMaintenanceVerifiedFullGraph, Self::Error>;
    /// Find an exact checkpoint reachable in the actual retained graph chain.
    fn find_member(
        &mut self,
        head: Option<[u8; 32]>,
        checkpoint: [u8; 32],
        kind: &str,
    ) -> Result<Option<[u8; 32]>, Self::Error>;
    /// Publish and reread one immutable CAS pack.
    fn publish_cas_object(
        &mut self,
        segment: &NativeMaintenanceArchiveSegment,
    ) -> Result<NativeMaintenancePublishedCas, Self::Error>;
    /// Publish the verified source graph manifest.
    fn publish_graph_manifest(
        &mut self,
        manifest: &NativeMaintenanceGraphManifest,
    ) -> Result<[u8; 32], Self::Error>;
    /// Atomically rotate the verified durable head.
    fn update_head(&mut self, head: [u8; 32]) -> Result<(), Self::Error>;
}

/// Obtain the actual canonical UTC commit instant at each source boundary.
pub trait NativeMaintenanceArchiveSegmentClock {
    /// Read the current source wall instant.
    fn now(&mut self) -> Option<String>;
}

/// Closed source publication failures.
#[derive(Debug, Eq, PartialEq)]
pub enum NativeMaintenanceArchiveSegmentError<Archive, Journal> {
    Input,
    Clock,
    LeaseOccupied,
    ReconciliationRequired,
    CompletedSegmentUnavailable,
    MissingJournalHead,
    Graph,
    HeadDiffers,
    Archive(Archive),
    Journal(Journal),
}

/// Publish one source segment and complete its lease only after real graph visibility.
pub fn publish_native_maintenance_archive_segment<A, J, C>(
    archive: &mut A,
    journal: &mut J,
    clock: &mut C,
    due_key: &str,
    segment: &NativeMaintenanceArchiveSegment,
) -> Result<
    NativeMaintenanceArchivePublication,
    NativeMaintenanceArchiveSegmentError<A::Error, J::Error>,
>
where
    A: NativeMaintenanceArchiveSegmentArchive,
    J: NativeMaintenanceArchiveSegmentJournal,
    C: NativeMaintenanceArchiveSegmentClock,
{
    use NativeMaintenanceArchiveSegmentError as Error;
    // retain the source due grammar and actual bounded segment size
    if !valid_due_key(due_key)
        || segment.members.is_empty()
        || segment.members.len() > MAXIMUM_MEMBERS
        || segment.cross_links.len() > MAXIMUM_CROSS_LINKS
    {
        return Err(Error::Input);
    }
    let checkpoint = &segment.members[0];
    let identity = format_digest(checkpoint.identity_sha256);
    let run_id = format!("archive-segment-{identity}");
    let due = journal.due(due_key).map_err(Error::Journal)?;
    // reuse a completed due only if its actual verified graph contains the source checkpoint
    if let Some(output) = due.output_sha256 {
        let graph = archive.verify_full_graph(output).map_err(Error::Archive)?;
        if !graph
            .manifest
            .entries
            .iter()
            .any(|entry| entry.identity_sha256 == identity && entry.kind == checkpoint.kind)
        {
            return Err(Error::CompletedSegmentUnavailable);
        }
        return Ok(NativeMaintenanceArchivePublication {
            manifest_object_sha256: output,
            state: NativeMaintenanceArchivePublicationState::AlreadyComplete,
        });
    }
    let status = journal.status().map_err(Error::Journal)?;
    let now = canonical_now(clock).ok_or(Error::Clock)?;
    // never release or reconcile another producer's active archive lease
    if let Some(active) = status.active_lease {
        if active.due_key != due_key || active.run_id != run_id {
            return Err(Error::LeaseOccupied);
        }
        let expires = crate::NativeWirePrimitives
            .canonical_utc_milliseconds(&active.expires_at)
            .ok_or(Error::Clock)?;
        let instant = crate::NativeWirePrimitives
            .canonical_utc_milliseconds(&now)
            .ok_or(Error::Clock)?;
        if expires <= instant {
            let head = archive.read_head().map_err(Error::Archive)?;
            let existing = archive
                .find_member(head, checkpoint.identity_sha256, &checkpoint.kind)
                .map_err(Error::Archive)?
                .ok_or(Error::ReconciliationRequired)?;
            let head = archive.read_head().map_err(Error::Archive)?;
            journal
                .reconcile_complete(due_key, &run_id, &now, existing, head)
                .map_err(Error::Journal)?;
            return Ok(NativeMaintenanceArchivePublication {
                manifest_object_sha256: existing,
                state: NativeMaintenanceArchivePublicationState::Reconciled,
            });
        }
    }
    let input = status.head_sha256.ok_or(Error::MissingJournalHead)?;
    journal
        .acquire(NativeMaintenanceArchiveLeaseRequest {
            due_key,
            run_id: &run_id,
            now: &now,
            input_head_sha256: due.input_head_sha256.unwrap_or(input),
        })
        .map_err(Error::Journal)?;
    let result = (|| {
        let published = archive
            .publish_cas_object(segment)
            .map_err(Error::Archive)?;
        let object = format_digest(published.object_sha256);
        let entries = published
            .members
            .iter()
            .map(|member| NativeMaintenanceGraphEntry {
                kind: member.kind.clone(),
                identity_sha256: format_digest(member.identity_sha256),
                object_sha256: object.clone(),
                member_offset: member.member_offset,
                member_length: member.member_length,
                member_sha256: format_digest(member.member_sha256),
            })
            .collect();
        let predecessor = archive
            .read_head()
            .map_err(Error::Archive)?
            .map(format_digest);
        let manifest = build_native_maintenance_graph_manifest(
            predecessor,
            entries,
            segment.cross_links.clone(),
        )
        .ok_or(Error::Graph)?;
        let head = archive
            .publish_graph_manifest(&manifest)
            .map_err(Error::Archive)?;
        archive.verify_full_graph(head).map_err(Error::Archive)?;
        archive.update_head(head).map_err(Error::Archive)?;
        // do not free a source server slot before the exact new graph head is visible
        if archive.read_head().map_err(Error::Archive)? != Some(head) {
            return Err(Error::HeadDiffers);
        }
        journal
            .complete(due_key, &canonical_now(clock).ok_or(Error::Clock)?, head)
            .map_err(Error::Journal)?;
        journal
            .release(due_key, &run_id, &canonical_now(clock).ok_or(Error::Clock)?)
            .map_err(Error::Journal)?;
        Ok(NativeMaintenanceArchivePublication {
            manifest_object_sha256: head,
            state: NativeMaintenanceArchivePublicationState::Published,
        })
    })();
    // preserve the publication failure and release only this still-live exact lease
    if result.is_err()
        && let Some(now) = canonical_now(clock)
    {
        let _ = journal.release(due_key, &run_id, &now);
    }
    result
}

/// Require an actual canonical clock reading.
fn canonical_now(clock: &mut impl NativeMaintenanceArchiveSegmentClock) -> Option<String> {
    let now = clock.now()?;
    crate::NativeWirePrimitives.canonical_utc_milliseconds(&now)?;
    Some(now)
}

/// Admit precisely the original bounded segment due-key families.
fn valid_due_key(key: &str) -> bool {
    let Some(rest) = key.strip_prefix("archive/") else {
        return false;
    };
    let Some((kind, identity)) = rest.rsplit_once('/') else {
        return false;
    };
    let valid = matches!(
        kind,
        "revision-cold-page"
            | "revision-gap-page"
            | "revision-custody-pack"
            | "future-only-genesis"
            | "future-only-lineage"
            | "development-custody"
            | "confirmation-plan"
            | "confirmation-terminal-part"
            | "control-reference/rain"
            | "development-candidate/temperature"
            | "development-candidate/wind"
            | "development-candidate/rain"
    );
    valid && crate::maintenance_canonical_json::valid_hash(identity)
}

#[cfg(test)]
#[path = "native_maintenance_archive_segment_test.rs"]
mod tests;

/// Actual source wall clock sampled independently at every journal transaction boundary.
pub struct NativeMaintenanceArchiveWallClock;
impl NativeMaintenanceArchiveSegmentClock for NativeMaintenanceArchiveWallClock {
    /// Read the actual current system UTC millisecond instant.
    fn now(&mut self) -> Option<String> {
        use weather_edge::ArchiveWirePrimitives;
        let milliseconds = i64::try_from(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .ok()?
                .as_millis(),
        )
        .ok()?;
        crate::NativeWirePrimitives.format_utc_milliseconds(milliseconds)
    }
}

/// Own the actual archive, journal, and clock required for each source segment publication.
pub struct NativeMaintenanceArchivePublisher<'a, A, J, C> {
    pub archive: &'a mut A,
    pub journal: &'a mut J,
    pub clock: &'a mut C,
}

impl<A, J, C> crate::NativeMaintenanceGapPublicationPort
    for NativeMaintenanceArchivePublisher<'_, A, J, C>
where
    A: NativeMaintenanceArchiveSegmentArchive,
    J: NativeMaintenanceArchiveSegmentJournal,
    C: NativeMaintenanceArchiveSegmentClock,
{
    type Error = NativeMaintenanceArchiveSegmentError<A::Error, J::Error>;

    /// Bind the native gap drain to the real source graph publication and durable journal completion.
    fn publish(
        &mut self,
        due: &str,
        segment: &NativeMaintenanceArchiveSegment,
    ) -> Result<NativeMaintenanceArchivePublication, Self::Error> {
        publish_native_maintenance_archive_segment(
            self.archive,
            self.journal,
            self.clock,
            due,
            segment,
        )
    }
}
