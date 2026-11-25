//! Compose native object, first-receipt, channel, and capacity ownership for edge capture.

use std::collections::BTreeSet;
use std::io;
use std::path::{Path, PathBuf};

use crate::native_revision_spool_file::ensure_child_directory;
use crate::revision_archive_canonical::instant;
use crate::{
    ForecastCaptureChannel, ForecastCaptureSource, ForecastCaptureVersion,
    ForecastEvidenceMaterial, ForecastEvidenceMaterialError, ForecastEvidenceOutcome,
    ForecastEvidenceReceiptCodec, ForecastEvidenceWindow, ForecastResponseEvidenceStore,
    PreparedForecastEvidence, RevisionArchiveCanonicalClock,
};

use super::forecast_allocation::{
    ForecastAllocationRefusal, ForecastAllocationStatfsPort, NativeForecastObjectAllocation,
};
use super::forecast_channels::{
    ForecastChannelCommitError, ForecastChannelCommitInput, ForecastChannelCommitOutcome,
    ForecastChannelCommitRefusal, ForecastIssuanceChannel, NativeForecastChannelStore,
};
use super::forecast_objects::{
    ForecastObjectCommitError, ForecastObjectCommitInput, ForecastObjectCommitOutcome,
    ForecastObjectCommitRefusal, ForecastObjectSha256, NativeForecastObjectStore,
};

const DIRECTORIES: [&str; 4] = ["objects", "receipts", "channels", "channel-bindings"];

/// the predecessor's snapshot of in-process capture counters.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NativeForecastEvidenceStatus {
    pub async_errors: u64,
    pub async_gaps: u64,
    pub bytes: u64,
    pub capacity_exhausted: u64,
    pub channel_assertions: usize,
    pub channel_collisions: u64,
    pub close_without_finish: u64,
    pub collisions: u64,
    pub finished_successes: u64,
    pub free_space_refusal: u64,
    pub inode_refusal: u64,
    pub object_too_large: u64,
    pub objects: usize,
    pub provenance_incomplete: u64,
}

/// a typed failure stays below the successful forecast response.
#[derive(Debug)]
pub enum NativeForecastCaptureStoreError {
    CommitClock,
    Accounting(io::Error),
    Object(ForecastObjectCommitError),
    Channel(ForecastChannelCommitError),
    ChannelRefused(ForecastChannelCommitRefusal),
}

struct Retained<Statfs> {
    objects: NativeForecastObjectStore,
    channels: NativeForecastChannelStore,
    allocation: NativeForecastObjectAllocation<Statfs>,
}

/// one writer using the same directory ownership as the retained Node runtime.
pub struct NativeForecastCaptureStore<Codec, Statfs> {
    material: ForecastEvidenceMaterial<Codec>,
    retained: Option<Retained<Statfs>>,
    initialized: bool,
    blocked: BTreeSet<ForecastObjectSha256>,
    status: NativeForecastEvidenceStatus,
}

impl<Codec: ForecastEvidenceReceiptCodec, Statfs: ForecastAllocationStatfsPort>
    NativeForecastCaptureStore<Codec, Statfs>
{
    /// initialize the exact configured root; preserve ordinary serving if evidence cannot start.
    pub fn initialize(
        root: PathBuf,
        codec: Codec,
        statfs: Statfs,
        clock: &mut impl RevisionArchiveCanonicalClock,
    ) -> Self {
        let mut store = Self {
            material: ForecastEvidenceMaterial::new(codec),
            retained: None,
            initialized: false,
            blocked: BTreeSet::new(),
            status: NativeForecastEvidenceStatus::default(),
        };
        match open_retained(root, &store.material, statfs, clock) {
            Some(retained) => {
                store.retained = Some(retained);
                store.initialized = true;
                store.refresh_status();
            }
            None => store.status.async_errors += 1,
        }
        store
    }

    /// expose only the current ordinary status counters to the existing health adapter.
    pub const fn status(&self) -> NativeForecastEvidenceStatus {
        self.status
    }

    /// expose readiness without making an evidence failure a public listener error.
    pub const fn initialized(&self) -> bool {
        self.initialized
    }

    /// commit one immutable object/receipt followed by optional first channel attribution.
    fn commit_retained(
        &mut self,
        prepared: &PreparedForecastEvidence,
        first_edge_committed_at: &str,
    ) -> Result<ForecastEvidenceOutcome, NativeForecastCaptureStoreError> {
        use ForecastEvidenceOutcome as Outcome;
        use NativeForecastCaptureStoreError as Error;
        // validate even duplicate attempts against the existing canonical wall clock boundary
        instant(self.material.codec(), first_edge_committed_at).ok_or(Error::CommitClock)?;
        if !self.initialized {
            self.status.async_gaps += 1;
            return Ok(Outcome::StoreUnavailable);
        }
        let identity = ForecastObjectSha256::admitted(prepared.edge_receipt_identity_sha256());
        // retain one process-local refusal for an already observed stable identity collision
        if self.blocked.contains(&identity) {
            self.status.async_gaps += 1;
            return Ok(Outcome::IdentityCollision);
        }
        let Some(retained) = self.retained.as_mut() else {
            self.status.async_gaps += 1;
            return Ok(Outcome::StoreUnavailable);
        };
        let object_result = retained.objects.commit(
            ForecastObjectCommitInput {
                compressed_object: prepared.compressed_object(),
                first_edge_committed_at,
                prepared,
            },
            &self.material,
            &mut retained.allocation,
        );
        self.refresh_status();
        // keep new receipts, duplicate receipts, ordinary pressure, and write faults distinct
        match object_result {
            Ok(ForecastObjectCommitOutcome::IdentityCollision) => {
                self.blocked.insert(identity);
                self.status.collisions += 1;
                self.status.async_gaps += 1;
                self.refresh_allocation()?;
                Ok(Outcome::IdentityCollision)
            }
            Ok(ForecastObjectCommitOutcome::Refused(refusal)) => {
                match refusal {
                    ForecastObjectCommitRefusal::Allocation => self.record_allocation_refusal(),
                    ForecastObjectCommitRefusal::ObjectTooLarge => {
                        self.status.object_too_large += 1
                    }
                }
                self.status.async_gaps += 1;
                Ok(match refusal {
                    ForecastObjectCommitRefusal::Allocation => Outcome::CapacityExhausted,
                    ForecastObjectCommitRefusal::ObjectTooLarge => Outcome::ObjectTooLarge,
                })
            }
            Ok(ForecastObjectCommitOutcome::Created { object_created }) => {
                // refresh an object possibly published by another process before using capacity
                if !object_created {
                    self.refresh_allocation()?;
                }
                self.status.finished_successes += 1;
                self.status.provenance_incomplete += prepared.row_count() as u64;
                self.commit_channel(prepared, identity, false)?;
                self.refresh_allocation()?;
                Ok(Outcome::Created)
            }
            Ok(ForecastObjectCommitOutcome::Duplicate) => {
                self.commit_channel(prepared, identity, true)?;
                self.refresh_allocation()?;
                Ok(Outcome::Duplicate)
            }
            Err(error) => {
                self.handle_object_write_failure(error)?;
                Err(Error::Object(error))
            }
        }
    }

    /// disable further capture only after an actual attempted object or receipt write fails.
    fn handle_object_write_failure(
        &mut self,
        error: ForecastObjectCommitError,
    ) -> Result<(), NativeForecastCaptureStoreError> {
        // earlier codec/read/admission faults do not revoke already validated storage
        match error {
            ForecastObjectCommitError::ObjectCollision
            | ForecastObjectCommitError::ObjectPublication => {}
            ForecastObjectCommitError::ReceiptPublication { orphan_object } => {
                // count a newly retained object without a first receipt
                if orphan_object {
                    self.status.async_gaps += 1;
                }
            }
            _ => return Ok(()),
        }
        let refresh = self.refresh_allocation();
        self.initialized = false;
        refresh
    }

    /// retain the first trusted channel, preserving failure categories and committed receipt bytes.
    fn commit_channel(
        &mut self,
        prepared: &PreparedForecastEvidence,
        identity: ForecastObjectSha256,
        receipt_was_existing: bool,
    ) -> Result<(), NativeForecastCaptureStoreError> {
        use NativeForecastCaptureStoreError as Error;
        let Some(retained) = self.retained.as_mut() else {
            return Ok(());
        };
        let result = retained.channels.commit(
            ForecastChannelCommitInput {
                edge_receipt_identity_sha256: identity,
                issuance_channel: prepared.issuance_channel().map(issuance_channel),
                receipt_was_existing,
            },
            &self.material,
            &mut retained.allocation,
        );
        match result {
            Ok(ForecastChannelCommitOutcome::Created) => {
                self.status.channel_assertions += 1;
                Ok(())
            }
            Ok(
                ForecastChannelCommitOutcome::Existing | ForecastChannelCommitOutcome::LegacyNoop,
            ) => Ok(()),
            Ok(ForecastChannelCommitOutcome::Refused(reason)) => {
                // record the source's exact bounded capacity reason below the response
                match reason {
                    ForecastChannelCommitRefusal::Allocation => self.record_allocation_refusal(),
                    ForecastChannelCommitRefusal::Capacity => self.status.capacity_exhausted += 1,
                    ForecastChannelCommitRefusal::ObjectTooLarge => {}
                }
                Err(Error::ChannelRefused(reason))
            }
            Err(error) => {
                // binding failures are tracked separately from forecast-content collisions
                if matches!(
                    error,
                    ForecastChannelCommitError::BindingCollision
                        | ForecastChannelCommitError::BindingPublication
                ) {
                    self.status.channel_collisions += 1;
                }
                Err(Error::Channel(error))
            }
        }
    }

    /// project bounded capacity refusals to the predecessor's disjoint counters.
    fn record_allocation_refusal(&mut self) {
        let reason = self.retained.as_ref().and_then(
            // use only the current decision immediately after admission
            |retained| retained.allocation.last_refusal(),
        );
        match reason {
            Some(ForecastAllocationRefusal::FreeSpace) => self.status.free_space_refusal += 1,
            Some(ForecastAllocationRefusal::Inodes) => self.status.inode_refusal += 1,
            Some(ForecastAllocationRefusal::Capacity) | None => self.status.capacity_exhausted += 1,
        }
    }

    /// refresh actual Linux allocation after cross-writer observations.
    fn refresh_allocation(&mut self) -> Result<(), NativeForecastCaptureStoreError> {
        if let Some(retained) = self.retained.as_mut() {
            retained
                .allocation
                .refresh_from_disk()
                .map_err(NativeForecastCaptureStoreError::Accounting)?;
        }
        self.refresh_status();
        Ok(())
    }

    /// update only counters derived from the accepted native directory census.
    fn refresh_status(&mut self) {
        if let Some(retained) = self.retained.as_ref() {
            let census = retained.allocation.census();
            self.status.bytes = census.allocated_bytes;
            self.status.objects = census.object_count;
            self.status.channel_assertions = census.channel_binding_count;
        }
    }
}

impl<Codec: ForecastEvidenceReceiptCodec, Statfs: ForecastAllocationStatfsPort>
    ForecastResponseEvidenceStore for NativeForecastCaptureStore<Codec, Statfs>
{
    type Prepared = PreparedForecastEvidence;
    type Error = NativeForecastCaptureStoreError;

    /// prepare only when the original native store startup has validated its retained data.
    fn prepare(
        &mut self,
        source: ForecastCaptureSource<'_>,
        window: Option<ForecastEvidenceWindow>,
        capture: ForecastCaptureVersion,
    ) -> Option<Self::Prepared> {
        // leave ordinary public serving usable even when evidence initialization was refused
        if !self.initialized {
            self.status.async_gaps += 1;
            return None;
        }
        match self.material.prepare(source, window, capture) {
            Ok(prepared) => Some(prepared),
            Err(reason) => {
                // distinguish a bounded oversized capture from other projection failures
                if reason == ForecastEvidenceMaterialError::ObjectTooLarge {
                    self.status.object_too_large += 1;
                } else {
                    self.status.async_errors += 1;
                }
                self.status.async_gaps += 1;
                None
            }
        }
    }

    /// commit exact finalized material on the already serialized writer lane.
    fn commit(
        &mut self,
        prepared: &Self::Prepared,
        first_edge_committed_at: &str,
    ) -> Result<ForecastEvidenceOutcome, Self::Error> {
        self.commit_retained(prepared, first_edge_committed_at)
    }

    /// retain one observed socket close that did not finish the response.
    fn note_close_without_finish(&mut self) {
        self.status.close_without_finish += 1;
    }

    /// account a writer rejection after the ordinary forecast response is already complete.
    fn note_write_failure(&mut self) {
        self.status.async_errors += 1;
        self.status.async_gaps += 1;
    }
}

/// bind only the fixed retained evidence directories before validating existing content.
fn ensure_evidence_directories(root: &Path) -> io::Result<()> {
    ensure_child_directory(root)?;
    // preserve the original fixed release-admitted root structure
    for child in DIRECTORIES {
        ensure_child_directory(&root.join(child))?;
    }
    Ok(())
}

/// retain the original trusted channel spelling for one first receipt.
const fn issuance_channel(channel: ForecastCaptureChannel) -> ForecastIssuanceChannel {
    match channel {
        ForecastCaptureChannel::PublicGet => ForecastIssuanceChannel::PublicGet,
        ForecastCaptureChannel::SchedulerRequest => ForecastIssuanceChannel::SchedulerRequest,
    }
}

use super::forecast_snapshot::{
    FrozenForecastEvidenceCatalog, NativeForecastSnapshotCatalog, NativeForecastSnapshotError,
};

impl<Codec: ForecastEvidenceReceiptCodec, Statfs: ForecastAllocationStatfsPort>
    NativeForecastCaptureStore<Codec, Statfs>
{
    /// freeze the exact current native forecast custody snapshot using the same material owner.
    pub fn freeze_snapshot(
        &self,
        clock: &mut impl RevisionArchiveCanonicalClock,
    ) -> Result<Option<FrozenForecastEvidenceCatalog>, NativeForecastSnapshotError> {
        let Some(retained) = self.retained.as_ref() else {
            return Ok(None);
        };
        let reader = NativeForecastSnapshotCatalog::new(retained.objects.root().to_path_buf())?;
        reader.freeze(&self.material, clock).map(Some)
    }
}

/// provision fixed children and prove retained state before admitting normal capture.
fn open_retained<Codec: ForecastEvidenceReceiptCodec, Statfs: ForecastAllocationStatfsPort>(
    root: PathBuf,
    material: &ForecastEvidenceMaterial<Codec>,
    statfs: Statfs,
    clock: &mut impl RevisionArchiveCanonicalClock,
) -> Option<Retained<Statfs>> {
    ensure_evidence_directories(&root).ok()?;
    let objects = NativeForecastObjectStore::new(root.clone()).ok()?;
    let channels = NativeForecastChannelStore::new(root.clone()).ok()?;
    let reader = NativeForecastSnapshotCatalog::new(root.clone()).ok()?;
    reader.freeze(material, clock).ok()?;
    let allocation = NativeForecastObjectAllocation::new(root, statfs).ok()?;
    Some(Retained {
        objects,
        channels,
        allocation,
    })
}
