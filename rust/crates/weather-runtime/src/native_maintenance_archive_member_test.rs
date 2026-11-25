use super::*;
use crate::{
    NativeMaintenanceArchiveDue, NativeMaintenanceArchiveJournalStatus,
    NativeMaintenanceArchiveLease, NativeMaintenanceCasMemberInventory,
    NativeMaintenanceGraphManifest, NativeMaintenancePublishedCas,
    NativeMaintenanceVerifiedFullGraph,
};

#[derive(Default)]
struct Journal {
    due: NativeMaintenanceArchiveDue,
    events: Vec<&'static str>,
}

impl NativeMaintenanceArchiveSegmentJournal for Journal {
    type Error = ();
    /// Return the existing immutable receipt state.
    fn due(&mut self, _: &str) -> Result<NativeMaintenanceArchiveDue, ()> {
        Ok(self.due)
    }
    /// Return the genuine fixture journal head and no competing lease.
    fn status(&mut self) -> Result<NativeMaintenanceArchiveJournalStatus, ()> {
        Ok(NativeMaintenanceArchiveJournalStatus {
            head_sha256: Some([1; 32]),
            active_lease: None::<NativeMaintenanceArchiveLease>,
        })
    }
    /// Bind the source member-specific due and run id.
    fn acquire(&mut self, request: NativeMaintenanceArchiveLeaseRequest<'_>) -> Result<(), ()> {
        assert_eq!(
            request.due_key,
            format!("archive/member/{}", format_digest([3; 32]))
        );
        assert_eq!(
            request.run_id,
            format!("archive-member-{}", format_digest([3; 32]))
        );
        self.events.push("acquire");
        self.due.input_head_sha256 = Some(request.input_head_sha256);
        Ok(())
    }
    /// Complete only the graph made visible by the archive.
    fn complete(&mut self, _: &str, _: &str, output: [u8; 32]) -> Result<(), ()> {
        self.events.push("complete");
        self.due.output_sha256 = Some(output);
        Ok(())
    }
    /// Release the corresponding source member run.
    fn release(&mut self, _: &str, _: &str, _: &str) -> Result<(), ()> {
        self.events.push("release");
        Ok(())
    }
    /// No expired reconciliation is used by this source fixture.
    fn reconcile_complete(
        &mut self,
        _: &str,
        _: &str,
        _: &str,
        _: [u8; 32],
        _: Option<[u8; 32]>,
    ) -> Result<(), ()> {
        Err(())
    }
}

#[derive(Default)]
struct Archive {
    head: Option<[u8; 32]>,
    manifest: Option<NativeMaintenanceGraphManifest>,
    publications: usize,
}

impl NativeMaintenanceArchiveSegmentArchive for Archive {
    type Error = ();
    /// Read the actual visible head from the fixture.
    fn read_head(&mut self) -> Result<Option<[u8; 32]>, ()> {
        Ok(self.head)
    }
    /// Revalidate the retained canonical graph digest.
    fn verify_full_graph(
        &mut self,
        head: [u8; 32],
    ) -> Result<NativeMaintenanceVerifiedFullGraph, ()> {
        let manifest = self.manifest.clone().ok_or(())?;
        if NativeWirePrimitives.sha256(&manifest.canonical_bytes().ok_or(())?) != head {
            return Err(());
        }
        Ok(NativeMaintenanceVerifiedFullGraph {
            manifest,
            manifest_object_sha256: head,
            object_count: 1,
            member_count: 1,
            predecessor_count: 0,
        })
    }
    /// Locate the exact existing verified member, if any.
    fn find_member(
        &mut self,
        head: Option<[u8; 32]>,
        identity: [u8; 32],
        kind: &str,
    ) -> Result<Option<[u8; 32]>, ()> {
        let Some(head) = head else {
            return Ok(None);
        };
        let graph = self.verify_full_graph(head)?;
        Ok(graph
            .manifest
            .entries
            .iter()
            .any(|entry| entry.kind == kind && entry.identity_sha256 == format_digest(identity))
            .then_some(head))
    }
    /// Return one exact fixture CAS receipt with the original byte identity.
    fn publish_cas_object(
        &mut self,
        segment: &NativeMaintenanceArchiveSegment,
    ) -> Result<NativeMaintenancePublishedCas, ()> {
        assert_eq!(segment.members.len(), 1);
        let member = &segment.members[0];
        Ok(NativeMaintenancePublishedCas {
            created: self.publications == 0,
            object_sha256: [2; 32],
            members: vec![NativeMaintenanceCasMemberInventory {
                kind: member.kind.clone(),
                identity_sha256: member.identity_sha256,
                member_offset: 16,
                member_length: member.payload.len() as u64,
                member_sha256: NativeWirePrimitives.sha256(&member.payload),
            }],
        })
    }
    /// Store one actual canonical graph.
    fn publish_graph_manifest(
        &mut self,
        manifest: &NativeMaintenanceGraphManifest,
    ) -> Result<[u8; 32], ()> {
        self.publications += 1;
        self.manifest = Some(manifest.clone());
        Ok(NativeWirePrimitives.sha256(&manifest.canonical_bytes().ok_or(())?))
    }
    /// Rotate the visible pointer only after successful graph verification.
    fn update_head(&mut self, head: [u8; 32]) -> Result<(), ()> {
        self.verify_full_graph(head)?;
        self.head = Some(head);
        Ok(())
    }
}

struct Clock;
impl NativeMaintenanceArchiveSegmentClock for Clock {
    /// Return the fixed source test instant.
    fn now(&mut self) -> Option<String> {
        Some("2026-10-10T12:00:00.000Z".to_owned())
    }
}

/// A report CAS publication is reused with no new graph after a retained exact due completion.
#[test]
fn native_maintenance_archive_member_publishes_and_reuses_source_member_receipt() {
    let member = || NativeMaintenanceArchiveSegmentMember {
        kind: "forecast-adjustment-maintenance-attempt/v2".to_owned(),
        identity_sha256: [3; 32],
        payload: b"fixture\n".to_vec(),
    };
    let mut archive = Archive::default();
    let mut journal = Journal::default();
    let mut clock = Clock;
    let result =
        publish_native_maintenance_archive_member(&mut archive, &mut journal, &mut clock, member())
            .unwrap();
    assert_eq!(Some(result.manifest_object_sha256), archive.head);
    assert_eq!(
        result.member_sha256,
        format_digest(NativeWirePrimitives.sha256(b"fixture\n"))
    );
    assert_eq!(journal.events, ["acquire", "complete", "release"]);
    assert_eq!(
        publish_native_maintenance_archive_member(&mut archive, &mut journal, &mut clock, member())
            .unwrap(),
        result
    );
    assert_eq!(archive.publications, 1);
    // simulate a reachable graph without durable due completion, then reuse its exact member metadata
    journal.due.output_sha256 = None;
    assert_eq!(
        publish_native_maintenance_archive_member(&mut archive, &mut journal, &mut clock, member())
            .unwrap(),
        result
    );
    assert_eq!(archive.publications, 1);
    assert_eq!(
        journal.events,
        [
            "acquire", "complete", "release", "acquire", "complete", "release"
        ]
    );
}
