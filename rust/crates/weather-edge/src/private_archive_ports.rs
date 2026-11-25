//! Bind the actual archive owners to the listener without reinterpreting their wire contracts.

use weather_contracts::{
    AdjustmentMaintenancePublishReceipt, AdjustmentMaintenanceStageInput,
    AdjustmentMaintenanceStageReceipt, AdjustmentRevisionStageReceipt,
};

use crate::private_archive_service::{
    PrivateArchiveFixedGaugeStore, PrivateArchivePermanentGapStore, PrivateArchiveRainControlStore,
    PrivateArchiveRevisionStore, PrivateArchiveShadowStore, PrivateArchiveStorage,
    PrivateArchiveStorageError, PrivateRevisionPublishInput, PrivateRevisionPublishReceipt,
};
use crate::private_revision_archive::{
    PrivateRevisionArchive, PrivateRevisionArchiveCodec, PrivateRevisionArchiveError,
    PrivateRevisionArchiveStorage,
};
use crate::private_shadow_archive::{
    PrivateShadowArchive, PrivateShadowArchiveCodec, PrivateShadowArchiveStorage,
    PrivateShadowPublishInput,
};
use crate::revision_archive_canonical::RevisionArchiveCanonicalClock;
use crate::revision_online_spool::RevisionOnlineSpoolError;

/// One listener composed from the five actual archive owners.
pub struct PrivateArchiveStores<Rain, FixedGauge, Shadow, Revision, Gap> {
    pub rain_control: Rain,
    pub fixed_gauge_gap: FixedGauge,
    pub shadow: Shadow,
    pub revision: Revision,
    pub permanent_gap: Gap,
}

impl<Rain, FixedGauge, Shadow, Revision, Gap> PrivateArchiveStorage
    for PrivateArchiveStores<Rain, FixedGauge, Shadow, Revision, Gap>
where
    Rain: PrivateArchiveRainControlStore,
    FixedGauge: PrivateArchiveFixedGaugeStore,
    Shadow: PrivateArchiveShadowStore,
    Revision: PrivateArchiveRevisionStore,
    Gap: PrivateArchivePermanentGapStore,
{
    /// Return the admitted rain control owner.
    fn rain_control_store(&mut self) -> &mut dyn PrivateArchiveRainControlStore {
        &mut self.rain_control
    }

    /// Return the admitted fixed-gauge disposition owner.
    fn fixed_gauge_gap_store(&mut self) -> &mut dyn PrivateArchiveFixedGaugeStore {
        &mut self.fixed_gauge_gap
    }

    /// Return the admitted compact/comparator shadow owner.
    fn shadow_store(&mut self) -> &mut dyn PrivateArchiveShadowStore {
        &mut self.shadow
    }

    /// Return the admitted single/grouped revision owner.
    fn revision_store(&mut self) -> &mut dyn PrivateArchiveRevisionStore {
        &mut self.revision
    }

    /// Return the admitted permanent metadata owner.
    fn permanent_gap_store(&mut self) -> &mut dyn PrivateArchivePermanentGapStore {
        &mut self.permanent_gap
    }
}

impl<Storage, Codec, Clock> PrivateArchiveRevisionStore
    for PrivateRevisionArchive<Storage, Codec, Clock>
where
    Storage: PrivateRevisionArchiveStorage,
    Codec: PrivateRevisionArchiveCodec,
    Clock: RevisionArchiveCanonicalClock,
{
    /// Stage the actual typed canonical revision before a database commit receipt can exist.
    fn stage_revision(
        &mut self,
        bytes: Vec<u8>,
    ) -> Result<AdjustmentRevisionStageReceipt, PrivateArchiveStorageError> {
        self.stage(&bytes).map_err(revision_error)
    }

    /// Publish the exact already-admitted database receipt or bounded group.
    fn publish_revision(
        &mut self,
        input: PrivateRevisionPublishInput,
    ) -> Result<PrivateRevisionPublishReceipt, PrivateArchiveStorageError> {
        self.publish(input).map_err(revision_error)
    }
}

impl<Storage, Codec, Clock> PrivateArchiveShadowStore
    for PrivateShadowArchive<Storage, Codec, Clock>
where
    Storage: PrivateShadowArchiveStorage,
    Codec: PrivateShadowArchiveCodec,
    Clock: RevisionArchiveCanonicalClock,
{
    /// Retain the actual parser-bound shadow stage.
    fn stage_shadow_revision(
        &mut self,
        input: AdjustmentMaintenanceStageInput,
    ) -> Result<AdjustmentMaintenanceStageReceipt, PrivateArchiveStorageError> {
        PrivateShadowArchive::stage_shadow_revision(self, input).map_err(online_error)
    }

    /// Keep compact v1 and value-bound v2 grammar on the actual shadow owner.
    fn publish_shadow_revision(
        &mut self,
        input: PrivateShadowPublishInput,
    ) -> Result<AdjustmentMaintenancePublishReceipt, PrivateArchiveStorageError> {
        PrivateShadowArchive::publish_shadow_revision(self, input).map_err(online_error)
    }
}

/// Keep only the recognized value-bearing capacity state retryable.
fn online_error(error: RevisionOnlineSpoolError) -> PrivateArchiveStorageError {
    match error {
        RevisionOnlineSpoolError::Capacity => PrivateArchiveStorageError::RevisionSpoolRefused,
        _ => PrivateArchiveStorageError::Rejected,
    }
}

/// Keep only the actual revision spool refusal distinct from general rejection.
fn revision_error(error: PrivateRevisionArchiveError) -> PrivateArchiveStorageError {
    match error {
        PrivateRevisionArchiveError::SpoolFull => PrivateArchiveStorageError::RevisionSpoolRefused,
        _ => PrivateArchiveStorageError::Rejected,
    }
}
