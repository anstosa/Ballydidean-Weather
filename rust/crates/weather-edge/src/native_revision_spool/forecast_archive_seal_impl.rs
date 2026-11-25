//! Attach the existing combined-input producer seal to native archive custody.

use crate::ForecastEvidenceReceiptCodec;
use crate::forecast_scheduler_due::ForecastSchedulerCalendarPort;
use crate::revision_archive_canonical::RevisionArchiveCanonicalClock;

use super::NativeForecastArchiveOperator;
use crate::native_revision_spool::forecast_allocation::ForecastAllocationStatfsPort;
use crate::native_revision_spool::forecast_archive_lock_binding::BoundForecastArchiveLockCodec;
use crate::native_revision_spool::forecast_cycle_input_seal::ForecastCycleInputSeal;
use crate::native_revision_spool::forecast_cycle_online_next::ForecastCyclePendingFinalProvider;
use crate::native_revision_spool::forecast_cycle_online_seal::{
    ForecastCycleOnlineSealError, ForecastCycleOnlineSealPorts, ForecastCycleOnlineSealRequest,
    seal_forecast_cycle_inputs,
};
use crate::native_revision_spool::forecast_cycle_online_transfer::{
    NativeForecastCycleOnlineTransfer, NativeForecastCycleOnlineTransferError,
};
use crate::native_revision_spool::forecast_cycle_page_binding::{
    BoundForecastCyclePageCodec, ForecastCycleHashPair,
};

impl<Codec, Calendar, Clock, Statfs>
    NativeForecastArchiveOperator<'_, Codec, Calendar, Clock, Statfs>
where
    Codec: ForecastEvidenceReceiptCodec + ForecastCycleHashPair,
    Calendar: ForecastSchedulerCalendarPort,
    Clock: RevisionArchiveCanonicalClock,
    Statfs: ForecastAllocationStatfsPort,
{
    /// Freeze one fully acknowledged producer population under the original archive custody lease.
    pub fn seal_inputs(
        &mut self,
        request: ForecastCycleOnlineSealRequest,
    ) -> Result<
        ForecastCycleInputSeal,
        ForecastCycleOnlineSealError<NativeForecastCycleOnlineTransferError>,
    > {
        let codec = BoundForecastCyclePageCodec::new(self.bytes);
        let lock = BoundForecastArchiveLockCodec::new(self.bytes, self.calendar);
        let manifests = &mut self.manifests;
        let final_acks = &mut self.final_acks;
        seal_forecast_cycle_inputs(
            request,
            ForecastCycleOnlineSealPorts::new(
                self.page_acks.slots(),
                &self.transfers,
                &mut self.seals,
                &codec,
                &mut self.clock,
                &lock,
            ),
            // recover closure directly under the already held seal lease
            |slots, transfers, seals| {
                let mut provider = NativeForecastCycleOnlineTransfer::new(
                    slots, transfers, seals, manifests, final_acks, &codec,
                )?;
                provider
                    .recover_acknowledged_finals()
                    .map_err(NativeForecastCycleOnlineTransferError::Storage)
            },
        )
    }
}
