use super::*;
use crate::{NativeMaintenanceCasMemberInventory, NativeWirePrimitives};

const NOW: &str = "2026-10-10T12:00:00.000Z";
struct Clock;
impl NativeMaintenanceArchiveSegmentClock for Clock {
    /// Supply the fixed fixture-only commit clock.
    fn now(&mut self) -> Option<String> {
        Some(NOW.to_owned())
    }
}
#[derive(Default)]
struct Journal {
    due: NativeMaintenanceArchiveDue,
    status: NativeMaintenanceArchiveJournalStatus,
    events: Vec<&'static str>,
}
impl NativeMaintenanceArchiveSegmentJournal for Journal {
    type Error = ();
    /// Read the fixture's exact persisted due.
    fn due(&mut self, _: &str) -> Result<NativeMaintenanceArchiveDue, ()> {
        Ok(self.due)
    }
    /// Report the fixture's actual retained lease.
    fn status(&mut self) -> Result<NativeMaintenanceArchiveJournalStatus, ()> {
        Ok(self.status.clone())
    }
    /// Record the exact current lease request.
    fn acquire(&mut self, req: NativeMaintenanceArchiveLeaseRequest<'_>) -> Result<(), ()> {
        assert_eq!(req.input_head_sha256, [1; 32]);
        self.events.push("acquire");
        Ok(())
    }
    /// Retain the actual published fixture graph identity.
    fn complete(&mut self, _: &str, _: &str, output: [u8; 32]) -> Result<(), ()> {
        self.due.output_sha256 = Some(output);
        self.events.push("complete");
        Ok(())
    }
    /// Record only the source-selected lease release.
    fn release(&mut self, _: &str, _: &str, _: &str) -> Result<(), ()> {
        self.events.push("release");
        Ok(())
    }
    /// Retain the reconciled fixture checkpoint.
    fn reconcile_complete(
        &mut self,
        _: &str,
        _: &str,
        _: &str,
        output: [u8; 32],
        head: Option<[u8; 32]>,
    ) -> Result<(), ()> {
        assert_eq!(Some(output), head);
        self.events.push("reconcile");
        Ok(())
    }
}
#[derive(Default)]
struct Archive {
    head: Option<[u8; 32]>,
    manifest: Option<NativeMaintenanceGraphManifest>,
    events: Vec<&'static str>,
}
impl NativeMaintenanceArchiveSegmentArchive for Archive {
    type Error = ();
    /// Read only the actual fixture's visible head.
    fn read_head(&mut self) -> Result<Option<[u8; 32]>, ()> {
        Ok(self.head)
    }
    /// Verify the retained fixture manifest and its stable identity.
    fn verify_full_graph(
        &mut self,
        head: [u8; 32],
    ) -> Result<NativeMaintenanceVerifiedFullGraph, ()> {
        let manifest = self.manifest.clone().ok_or(())?;
        self.events.push("verify");
        Ok(NativeMaintenanceVerifiedFullGraph {
            manifest,
            manifest_object_sha256: head,
            object_count: 1,
            member_count: 1,
            predecessor_count: 0,
        })
    }
    /// Find the exact checkpoint in the retained fixture graph.
    fn find_member(
        &mut self,
        head: Option<[u8; 32]>,
        checkpoint: [u8; 32],
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
            .any(|entry| entry.identity_sha256 == format_digest(checkpoint) && entry.kind == kind)
            .then_some(head))
    }
    /// Build genuine content identities for the source fixture CAS member.
    fn publish_cas_object(
        &mut self,
        segment: &NativeMaintenanceArchiveSegment,
    ) -> Result<NativeMaintenancePublishedCas, ()> {
        let member = &segment.members[0];
        self.events.push("publish_cas");
        Ok(NativeMaintenancePublishedCas {
            created: true,
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
    /// Retain the actual native builder's immutable manifest.
    fn publish_graph_manifest(
        &mut self,
        manifest: &NativeMaintenanceGraphManifest,
    ) -> Result<[u8; 32], ()> {
        self.manifest = Some(manifest.clone());
        self.events.push("publish_manifest");
        Ok(NativeWirePrimitives.sha256(&manifest.canonical_bytes().unwrap()))
    }
    /// Make only the verified fixture head visible.
    fn update_head(&mut self, head: [u8; 32]) -> Result<(), ()> {
        self.events.push("head");
        self.head = Some(head);
        Ok(())
    }
}

/// Publish, reuse and reconcile the same real native checkpoint in source order.
#[test]
fn native_archive_segment_preserves_publication_and_retry_order() {
    let segment = NativeMaintenanceArchiveSegment {
        members: vec![NativeMaintenanceArchiveSegmentMember {
            kind: "adjustment-fixture/v1".into(),
            identity_sha256: [3; 32],
            payload: b"payload".to_vec(),
        }],
        cross_links: vec![],
    };
    let due = format!("archive/revision-gap-page/{}", format_digest([3; 32]));
    let mut archive = Archive::default();
    let mut journal = Journal::default();
    journal.status.head_sha256 = Some([1; 32]);
    let result = publish_native_maintenance_archive_segment(
        &mut archive,
        &mut journal,
        &mut Clock,
        &due,
        &segment,
    )
    .unwrap();
    assert_eq!(
        result.state,
        NativeMaintenanceArchivePublicationState::Published
    );
    assert_eq!(journal.events, ["acquire", "complete", "release"]);
    assert_eq!(
        archive.events,
        ["publish_cas", "publish_manifest", "verify", "head"]
    );
    let retry = publish_native_maintenance_archive_segment(
        &mut archive,
        &mut journal,
        &mut Clock,
        &due,
        &segment,
    )
    .unwrap();
    assert_eq!(
        retry.state,
        NativeMaintenanceArchivePublicationState::AlreadyComplete
    );
    assert_eq!(retry.manifest_object_sha256, result.manifest_object_sha256);
    journal.due.output_sha256 = None;
    journal.status.active_lease = Some(NativeMaintenanceArchiveLease {
        due_key: due.clone(),
        run_id: format!("archive-segment-{}", format_digest([3; 32])),
        expires_at: NOW.into(),
    });
    let retry = publish_native_maintenance_archive_segment(
        &mut archive,
        &mut journal,
        &mut Clock,
        &due,
        &segment,
    )
    .unwrap();
    assert_eq!(
        retry.state,
        NativeMaintenanceArchivePublicationState::Reconciled
    );
    assert_eq!(journal.events.last(), Some(&"reconcile"));
}
