//! Bind verified source epoch documents to the actual local journal and genesis archive.

use std::{cell::RefCell, future::Future, io, pin::Pin};

use crate::{
    NativeMaintenanceArchiveJournal, NativeMaintenanceArchiveSegmentArchive,
    NativeMaintenanceArchiveSegmentClock, NativeMaintenanceCaptureEpochLedgerAuthority,
    NativeMaintenanceCaptureGenesisPort, NativeMaintenanceCaptureRemoteAuthority,
    NativeMaintenanceDatabaseLedgerReader, NativeMaintenanceJournalStore, NativeWirePrimitives,
    VerifiedNativeMaintenanceCaptureEpochSnapshot, VerifiedNativeMaintenanceCaptureEpochWitness,
    VerifiedNativeMaintenanceDatabaseLedger, build_native_maintenance_archive_future_only_genesis,
    initialize_native_maintenance_future_only_journal, publish_native_maintenance_archive_segment,
};

/// Carry the complete verified documents across the two independently owned effects.
#[derive(Default)]
pub struct NativeMaintenanceCaptureGenesisDocuments {
    witness: RefCell<Option<(VerifiedNativeMaintenanceCaptureEpochWitness, Vec<u8>)>>,
    snapshot: RefCell<Option<(VerifiedNativeMaintenanceCaptureEpochSnapshot, Vec<u8>)>>,
}

impl NativeMaintenanceCaptureGenesisDocuments {
    /// Retain the exact authenticated epoch witness read during lifecycle initialization.
    pub(crate) fn retained_witness(
        &self,
    ) -> Option<(VerifiedNativeMaintenanceCaptureEpochWitness, Vec<u8>)> {
        self.witness.borrow().clone()
    }
}

/// Real epoch and ledger reads retaining their full canonical source bytes.
pub struct NativeMaintenanceCaptureEpochLedgerHost<'a> {
    pub remote: &'a mut NativeMaintenanceDatabaseLedgerReader,
    pub documents: &'a NativeMaintenanceCaptureGenesisDocuments,
}

impl NativeMaintenanceCaptureEpochLedgerAuthority for NativeMaintenanceCaptureEpochLedgerHost<'_> {
    type Error = io::Error;

    /// Verify and retain the full root-authenticated witness before narrowing it.
    fn read_verified_epoch_witness(
        &mut self,
    ) -> Pin<
        Box<
            dyn Future<Output = Result<VerifiedNativeMaintenanceCaptureEpochWitness, Self::Error>>
                + '_,
        >,
    > {
        Box::pin(async move {
            let mut authority = NativeMaintenanceCaptureRemoteAuthority::new(self.remote);
            let (witness, bytes) = authority
                .read_verified_capture_epoch_witness_with_bytes()
                .map_err(|_| invalid())?;
            self.documents
                .witness
                .replace(Some((witness.clone(), bytes)));
            Ok(witness)
        })
    }

    /// Verify and retain the actual zero-frontier snapshot from its source.
    fn read_verified_epoch_snapshot(
        &mut self,
    ) -> Pin<
        Box<
            dyn Future<Output = Result<VerifiedNativeMaintenanceCaptureEpochSnapshot, Self::Error>>
                + '_,
        >,
    > {
        Box::pin(async move {
            let mut authority = NativeMaintenanceCaptureRemoteAuthority::new(self.remote);
            let (snapshot, bytes) = authority
                .read_verified_capture_epoch_snapshot_with_bytes()
                .map_err(|_| invalid())?;
            self.documents
                .snapshot
                .replace(Some((snapshot.clone(), bytes)));
            Ok(snapshot)
        })
    }

    /// Read the actual rolling source ledger without a reconstructed receipt.
    fn read_verified_database_ledger(
        &mut self,
    ) -> Pin<
        Box<dyn Future<Output = Result<VerifiedNativeMaintenanceDatabaseLedger, Self::Error>> + '_>,
    > {
        Box::pin(async move { self.remote.fetch() })
    }
}

/// Real journal and archive genesis publication owners sharing one retained source evidence set.
pub struct NativeMaintenanceCaptureGenesisHost<'a, Archive, Clock> {
    pub archive: &'a mut Archive,
    pub clock: &'a mut Clock,
    pub documents: &'a NativeMaintenanceCaptureGenesisDocuments,
    pub journal: &'a mut NativeMaintenanceJournalStore,
}

impl<Archive, Clock> NativeMaintenanceCaptureGenesisPort
    for NativeMaintenanceCaptureGenesisHost<'_, Archive, Clock>
where
    Archive: NativeMaintenanceArchiveSegmentArchive,
    Clock: NativeMaintenanceArchiveSegmentClock,
{
    type Error = io::Error;

    /// Sample the controller's real commit clock.
    fn canonical_now(&mut self) -> Result<String, Self::Error> {
        self.clock.now().ok_or_else(invalid)
    }

    /// Append exactly the complete witness that was verified in this lifecycle invocation.
    fn initialize_future_only_lifecycle<'a>(
        &'a mut self,
        now: &'a str,
        witness: &'a VerifiedNativeMaintenanceCaptureEpochWitness,
    ) -> Pin<Box<dyn Future<Output = Result<(), Self::Error>> + 'a>> {
        Box::pin(async move {
            let retained = self.documents.witness.borrow();
            let (verified, bytes) = retained.as_ref().ok_or_else(invalid)?;
            // bind the narrow scheduler projection to the actual retained source bytes
            if verified != witness {
                return Err(invalid());
            }
            initialize_native_maintenance_future_only_journal(
                self.journal,
                now,
                bytes,
                &mut NativeWirePrimitives,
                &NativeWirePrimitives,
            )
            .map_err(|_| invalid())
        })
    }

    /// Publish the exact previously verified witness and snapshot into the durable graph.
    fn archive_future_only_genesis<'a>(
        &'a mut self,
        snapshot: &'a VerifiedNativeMaintenanceCaptureEpochSnapshot,
        witness: &'a VerifiedNativeMaintenanceCaptureEpochWitness,
    ) -> Pin<Box<dyn Future<Output = Result<(), Self::Error>> + 'a>> {
        Box::pin(async move {
            let retained_witness = self.documents.witness.borrow();
            let retained_snapshot = self.documents.snapshot.borrow();
            let (witness_projection, witness_bytes) =
                retained_witness.as_ref().ok_or_else(invalid)?;
            let (snapshot_projection, snapshot_bytes) =
                retained_snapshot.as_ref().ok_or_else(invalid)?;
            // prevent a projection from authorizing unrelated source bytes
            if witness_projection != witness || snapshot_projection != snapshot {
                return Err(invalid());
            }
            let genesis = build_native_maintenance_archive_future_only_genesis(
                witness_bytes,
                snapshot_bytes,
                &mut NativeWirePrimitives,
                &NativeWirePrimitives,
            )
            .map_err(|_| invalid())?;
            let mut journal = NativeMaintenanceArchiveJournal::new(self.journal);
            publish_native_maintenance_archive_segment(
                self.archive,
                &mut journal,
                self.clock,
                &genesis.due_key,
                &genesis.segment,
            )
            .map_err(|_| invalid())?;
            Ok(())
        })
    }
}

// return one bounded local genesis refusal
fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "maintenance genesis unavailable",
    )
}
