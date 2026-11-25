//! Actual local pack archive and remote evidence reconciliation effects.

use crate::{
    NativeMaintenanceArchiveJournal, NativeMaintenanceArchivePublisher,
    NativeMaintenanceArchiveSegmentArchive, NativeMaintenanceArchiveSegmentClock,
    NativeMaintenanceArchiveWallClock, NativeMaintenanceColdJournal,
    NativeMaintenanceDatabaseLedgerReader, NativeMaintenanceEvidenceReconciliationPort,
    NativeMaintenanceGapDrainResult, NativeMaintenanceJournalStore,
    NativeMaintenanceRevisionGapDrain, NativeRevisionColdArchivePort,
    NativeRevisionColdArchivePublication, NativeRevisionColdClock, NativeRevisionColdDrainResult,
    NativeWirePrimitives, VerifiedNativeRevisionColdPage, VerifiedNativeRevisionColdStart,
    drain_native_maintenance_revision_cold_page, drain_native_maintenance_revision_gap,
    maintenance_canonical_json::{exact_keys, field, text},
    native_maintenance_cold_pack_seal_journal::record_native_maintenance_cold_pack_seal,
    native_maintenance_open_pack::{NativeMaintenanceOpenPack, NativeMaintenanceOpenPackError},
    native_maintenance_open_pack_store::NativeMaintenanceOpenPackStore,
    native_maintenance_retirement_files_v2::format_digest,
    publish_native_maintenance_archive_segment,
};
use std::{future::Future, io, pin::Pin};
use weather_contracts::JsonValue;
use weather_edge::ArchiveWirePrimitives;

/// Actual private open-pack publication and seal boundary.
pub struct NativeMaintenanceColdArchiveHost<'a, Archive, Store, Clock> {
    pub pack: &'a mut NativeMaintenanceOpenPack<Store>,
    pub archive: &'a mut Archive,
    pub journal: &'a mut NativeMaintenanceJournalStore,
    pub clock: &'a mut Clock,
}

impl<Archive, Store, Clock> NativeMaintenanceColdArchiveHost<'_, Archive, Store, Clock>
where
    Archive: NativeMaintenanceArchiveSegmentArchive,
    Store: NativeMaintenanceOpenPackStore,
    Clock: NativeMaintenanceArchiveSegmentClock,
{
    /// Publish the source sealed segment and journal its mapping before removing open files.
    pub fn seal(&mut self, force: bool) -> io::Result<()> {
        let status = self.pack.inspect().map_err(|_| invalid())?;
        // wait for four pages unless the original due pipeline explicitly flushes
        if status.pages.is_empty() || (!force && status.pages.len() < 4) {
            return Ok(());
        }
        let sealed = self.pack.build_sealed_segment().map_err(|_| invalid())?;
        let due = format!("archive/revision-custody-pack/{}", sealed.checkpoint_sha256);
        let mut journal = NativeMaintenanceArchiveJournal::new(self.journal);
        let published = publish_native_maintenance_archive_segment(
            self.archive,
            &mut journal,
            self.clock,
            &due,
            &sealed.segment,
        )
        .map_err(|_| invalid())?;
        let now = self.clock.now().ok_or_else(invalid)?;
        let graph = format_digest(published.manifest_object_sha256);
        // persist the exact graph mapping before removing a single local raw byte
        record_native_maintenance_cold_pack_seal(
            self.journal,
            &sealed.checkpoint_sha256,
            &graph,
            &now,
            &sealed.records,
        )
        .map_err(|_| invalid())?;
        self.pack
            .remove_sealed(&sealed.checkpoint_sha256)
            .map_err(|_| invalid())
    }
}

impl<Archive, Store, Clock> NativeRevisionColdArchivePort
    for NativeMaintenanceColdArchiveHost<'_, Archive, Store, Clock>
where
    Archive: NativeMaintenanceArchiveSegmentArchive,
    Store: NativeMaintenanceOpenPackStore,
    Clock: NativeMaintenanceArchiveSegmentClock,
{
    type Error = io::Error;

    /// Append and reopen one complete page before its remote custody request.
    fn archive_verified_page<'a>(
        &'a mut self,
        page: &'a VerifiedNativeRevisionColdPage,
        start: &'a VerifiedNativeRevisionColdStart,
    ) -> Pin<Box<dyn Future<Output = Result<NativeRevisionColdArchivePublication, Self::Error>> + 'a>>
    {
        Box::pin(async move {
            let retained = match self.pack.append(page, Some(start)) {
                Ok(value) => value,
                // seal a verified prior full pack before retrying the same exact page
                Err(NativeMaintenanceOpenPackError::Full) => {
                    self.seal(true)?;
                    self.pack.append(page, Some(start)).map_err(|_| invalid())?
                }
                Err(_) => return Err(invalid()),
            };
            let last = retained.pages.last().ok_or_else(invalid)?;
            Ok(NativeRevisionColdArchivePublication {
                custody_checkpoint_sha256: retained.checkpoint_sha256.ok_or_else(invalid)?,
                member_root_sha256: last.record.member_root_sha256.clone(),
                start_member_sha256: last
                    .record
                    .start_member_sha256
                    .clone()
                    .ok_or_else(invalid)?,
            })
        })
    }

    /// Apply only the source open-pack threshold policy.
    fn seal_revision_custody_pack(
        &mut self,
        force: bool,
    ) -> Pin<Box<dyn Future<Output = Result<(), Self::Error>> + '_>> {
        Box::pin(async move { self.seal(force) })
    }
}

impl NativeRevisionColdClock for NativeMaintenanceArchiveWallClock {
    type Error = io::Error;

    /// Sample an actual UTC millisecond clock for cold journal effects.
    fn canonical_now(&mut self) -> io::Result<String> {
        self.now().ok_or_else(invalid)
    }
}

/// Actual cycle port with separate verified journal handles for its two owners.
pub struct NativeMaintenanceEvidenceCycleHost<'a, Archive, Store> {
    pub archive: &'a mut Archive,
    pub pack: &'a mut NativeMaintenanceOpenPack<Store>,
    pub remote: &'a mut NativeMaintenanceDatabaseLedgerReader,
    pub archive_journal: &'a mut NativeMaintenanceJournalStore,
    pub cold_journal: &'a mut NativeMaintenanceJournalStore,
}

impl<Archive, Store> NativeMaintenanceEvidenceReconciliationPort
    for NativeMaintenanceEvidenceCycleHost<'_, Archive, Store>
where
    Archive: NativeMaintenanceArchiveSegmentArchive,
    Store: NativeMaintenanceOpenPackStore,
{
    type Error = io::Error;
    type ColdCycle = NativeRevisionColdDrainResult;

    /// Consume only one actual retained source metadata proof when one exists.
    fn reconcile_shadow_metadata(
        &mut self,
    ) -> Pin<Box<dyn Future<Output = Result<(), Self::Error>> + '_>> {
        Box::pin(async move { reconcile_metadata(self.remote) })
    }

    /// Drain the exact gap page using real archive publication and durable acknowledgement.
    fn drain_revision_gap(
        &mut self,
    ) -> Pin<Box<dyn Future<Output = Result<NativeMaintenanceRevisionGapDrain, Self::Error>> + '_>>
    {
        Box::pin(async move {
            let mut journal = NativeMaintenanceArchiveJournal::new(self.archive_journal);
            let mut clock = NativeMaintenanceArchiveWallClock;
            let mut publisher = NativeMaintenanceArchivePublisher {
                archive: self.archive,
                journal: &mut journal,
                clock: &mut clock,
            };
            let result = drain_native_maintenance_revision_gap(self.remote, &mut publisher)
                .map_err(|_| invalid())?;
            Ok(match result {
                NativeMaintenanceGapDrainResult::Idle { page_sha256 } => {
                    NativeMaintenanceRevisionGapDrain::Idle {
                        page_sha256: format_digest(*page_sha256.as_bytes()),
                    }
                }
                NativeMaintenanceGapDrainResult::Captured {
                    publication,
                    acknowledgement,
                } => NativeMaintenanceRevisionGapDrain::Captured {
                    graph_manifest_sha256: format_digest(publication.manifest_object_sha256),
                    page_sha256: format_digest(*acknowledgement.page_sha256.as_bytes()),
                },
            })
        })
    }

    /// Execute bounded cold custody through the actual open pack and both journal owners.
    fn drain_revision_cold_page<'a>(
        &'a mut self,
        cutoff_at: &'a str,
    ) -> crate::NativeMaintenanceColdCycleFuture<'a, Self::ColdCycle, Self::Error> {
        Box::pin(async move {
            let mut pack_clock = NativeMaintenanceArchiveWallClock;
            let mut clock = NativeMaintenanceArchiveWallClock;
            let mut archive = NativeMaintenanceColdArchiveHost {
                pack: self.pack,
                archive: self.archive,
                journal: self.archive_journal,
                clock: &mut pack_clock,
            };
            let mut journal = NativeMaintenanceColdJournal::new(self.cold_journal);
            drain_native_maintenance_revision_cold_page(
                cutoff_at,
                self.remote,
                &mut archive,
                &mut journal,
                &mut clock,
            )
            .await
            .map_err(|_| invalid())
        })
    }
}

// read the genuine fixed proof status, then finalize only its exact admitted identity
fn reconcile_metadata(remote: &mut NativeMaintenanceDatabaseLedgerReader) -> io::Result<()> {
    let status = remote.read_document("adjustment-shadow-metadata-custody-status-v1", &[])?;
    let proof = read_status(&status).ok_or_else(invalid)?;
    if let Some(proof) = proof {
        let consumption =
            remote.read_document("adjustment-shadow-metadata-custody-finalize-v1", &[&proof])?;
        let wire = NativeWirePrimitives;
        let value = wire.decode_json(&consumption).ok_or_else(invalid)?;
        if consumption.len() > 16 * 1024
            || wire.encode_ordered_json(&value).as_deref() != Some(&consumption)
            || crate::shadow_metadata_custody::validate_consumption(&value, &wire).is_none()
            || text(&value, "proofSha256").as_deref() != Some(&proof)
        {
            return Err(invalid());
        }
    }
    Ok(())
}

// validate all exact source proof/status fields before authorizing a reconciliation
fn read_status(bytes: &[u8]) -> Option<Option<String>> {
    let mut wire = NativeWirePrimitives;
    let value = wire.decode_json(bytes)?;
    if bytes.len() > 32 * 1024
        || wire.encode_ordered_json(&value)?.as_slice() != bytes
        || !exact_keys(&value, &["contractVersion", "preparation", "proof"])
        || text(&value, "contractVersion")? != "adjustment-shadow-metadata-custody-status/v1"
    {
        return None;
    }
    let proof = field(&value, "proof")?;
    let preparation = field(&value, "preparation")?;
    if *proof == JsonValue::Null {
        return (*preparation == JsonValue::Null).then_some(None);
    }
    crate::shadow_metadata_custody_proof::validate_shadow_custody_proof(
        proof,
        &mut wire,
        &NativeWirePrimitives,
    )?;
    let identity = text(proof, "proofSha256")?;
    if *preparation != JsonValue::Null {
        crate::shadow_metadata_custody_preparation::validate_shadow_custody_preparation(
            preparation,
            &mut wire,
            &NativeWirePrimitives,
        )?;
        if text(preparation, "proofSha256")? != identity {
            return None;
        }
    }
    Some(Some(identity))
}

// bounded source evidence cycle failure
fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "maintenance evidence cycle unavailable",
    )
}
