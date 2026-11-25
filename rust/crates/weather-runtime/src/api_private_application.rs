//! Prepared private maintenance application over the already retained weather API pool.

use std::sync::Arc;

use weather_api::{
    AdjustmentMaintenanceAdmissionBoundary, AdjustmentMaintenanceBodyRead,
    AdjustmentMaintenanceCaptureCallback, AdjustmentMaintenanceRelayPorts,
    AdjustmentMaintenanceServiceRelay, AdjustmentMaintenanceWireEncoder,
    AdjustmentMaintenanceWireResponse, DatabaseAdjustmentMaintenanceRepository,
    DecodedAdjustmentMaintenanceRequest, UnifiedAdjustmentMaintenanceServicePorts,
    WeatherApiMaintenanceStartupPreparation, WeatherApiStartupSnapshots,
    serve_adjustment_maintenance_wire_request_with_repository,
};

use crate::{
    ApiMaintenanceRequestBodyCodec, ApiMaintenanceRevisionEvidenceValues,
    ApiMaintenanceShadowEvidenceValues, LoadedApiMaintenanceCandidates,
    PooledApiMaintenanceCaptureReads, PooledWeatherApiMaintenance, RetainedApiMaintenanceAdmission,
    RetainedApiMaintenanceRequestBody, WeatherApiMaintenanceCapturePool, WeatherApiMaintenancePool,
    WeatherApiSharedPool,
};

/// Request-local owners keep codecs and relay scratch independent of concurrent requests.
pub struct ApiMaintenanceRequestOwners<'admission, 'capture, 'relay> {
    pub admission: &'admission dyn AdjustmentMaintenanceAdmissionBoundary,
    pub capture: Option<&'capture mut dyn AdjustmentMaintenanceCaptureCallback>,
    pub relay: &'relay mut dyn AdjustmentMaintenanceServiceRelay,
}

/// Retain only request-local codec, callback and relay owners for the encoded entrypoint.
pub struct EncodedApiMaintenanceRequestOwners<'a, Codec, Values> {
    pub body_codec: &'a mut Codec,
    pub evidence_values: &'a Values,
    pub capture: Option<&'a mut dyn AdjustmentMaintenanceCaptureCallback>,
    pub relay: &'a mut dyn AdjustmentMaintenanceServiceRelay,
}

/// One private dispatcher with the startup-admitted epoch and retained pool handle.
pub struct PreparedPrivateMaintenanceApi<Pool> {
    pool: Pool,
    preparation: WeatherApiMaintenanceStartupPreparation,
    snapshots: Arc<WeatherApiStartupSnapshots>,
    candidates: Arc<LoadedApiMaintenanceCandidates>,
}

impl<Pool> PreparedPrivateMaintenanceApi<Pool> {
    /// Retain private dispatch authorization only after startup preparation.
    pub fn new(
        pool: Pool,
        preparation: WeatherApiMaintenanceStartupPreparation,
        snapshots: impl Into<Arc<WeatherApiStartupSnapshots>>,
        candidates: LoadedApiMaintenanceCandidates,
    ) -> Self {
        Self {
            pool,
            preparation,
            snapshots: snapshots.into(),
            candidates: Arc::new(candidates),
        }
    }

    /// Borrow the original immutable candidate set and the serving choices used by capture.
    pub fn capture_material(
        &self,
    ) -> (
        &WeatherApiMaintenanceStartupPreparation,
        &WeatherApiStartupSnapshots,
        &LoadedApiMaintenanceCandidates,
    ) {
        (&self.preparation, &self.snapshots, &self.candidates)
    }
}

impl<Pool: WeatherApiMaintenancePool + WeatherApiSharedPool> PreparedPrivateMaintenanceApi<Pool> {
    /// Share the exact immutable startup model and candidate allocations with a request worker.
    pub fn shared_worker(&self) -> Self {
        Self {
            pool: self.pool.share(),
            preparation: self.preparation.clone(),
            snapshots: Arc::clone(&self.snapshots),
            candidates: Arc::clone(&self.candidates),
        }
    }

    /// Clone only the existing authorized process pool handle for a separate capture lease.
    pub fn shared_pool(&self) -> Pool {
        self.pool.share()
    }

    /// Decode one bounded original HTTP body through the actual private application admission.
    pub fn serve_encoded<'encoded, Codec, Values, Encoder>(
        &self,
        request: DecodedAdjustmentMaintenanceRequest<'_>,
        encoded_body: &[u8],
        owners: EncodedApiMaintenanceRequestOwners<'_, Codec, Values>,
        encoder: &'encoded mut Encoder,
    ) -> AdjustmentMaintenanceWireResponse<'encoded>
    where
        Codec: ApiMaintenanceRequestBodyCodec,
        Values: ApiMaintenanceShadowEvidenceValues + ApiMaintenanceRevisionEvidenceValues,
        Encoder: AdjustmentMaintenanceWireEncoder,
    {
        let mut body = RetainedApiMaintenanceRequestBody::new(encoded_body, owners.body_codec);
        let admission = RetainedApiMaintenanceAdmission::new(
            &self.preparation.capture_witness,
            owners.evidence_values,
        );
        self.serve(
            request,
            &mut body,
            ApiMaintenanceRequestOwners {
                admission: &admission,
                capture: owners.capture,
                relay: owners.relay,
            },
            encoder,
        )
    }

    /// Use one request handle for private schedule and publication while capture retains its own.
    pub fn serve<'encoded, Encoder: AdjustmentMaintenanceWireEncoder>(
        &self,
        request: DecodedAdjustmentMaintenanceRequest<'_>,
        body: &mut dyn AdjustmentMaintenanceBodyRead,
        owners: ApiMaintenanceRequestOwners<'_, '_, '_>,
        encoder: &'encoded mut Encoder,
    ) -> AdjustmentMaintenanceWireResponse<'encoded> {
        let mut pool = self.pool.share();
        let mut repository = DatabaseAdjustmentMaintenanceRepository::new(
            PooledWeatherApiMaintenance::from_shared_handle(&mut pool),
        );
        serve_adjustment_maintenance_wire_request_with_repository(
            self.preparation.authority,
            &mut repository,
            body,
            UnifiedAdjustmentMaintenanceServicePorts {
                capture_epoch: self.preparation.capture_witness.capture_epoch.clone(),
                admission: owners.admission,
                capture: owners.capture,
                relay: AdjustmentMaintenanceRelayPorts::Unified(owners.relay),
            },
            request,
            encoder,
        )
    }
}

impl<Pool: WeatherApiMaintenancePool + WeatherApiMaintenanceCapturePool + WeatherApiSharedPool>
    PreparedPrivateMaintenanceApi<Pool>
{
    /// Lend the capture callback the same capacity without holding a dispatcher lease.
    pub fn capture_reads(&self) -> PooledApiMaintenanceCaptureReads<Pool> {
        PooledApiMaintenanceCaptureReads::new(self.pool.share())
    }
}
