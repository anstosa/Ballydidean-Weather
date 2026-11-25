//! Production provider ownership for worker startup and historical backfills.

use std::sync::Arc;

use weather_providers::ProviderControls;

use crate::{
    NativeAmbientWeatherTransport, NativeNetatmoTransport, NativeNoaaTideTransport,
    NativeOpenMeteoTransport, NativeOpenMeteoValues, NativeOutboundHttp, NativeOutboundHttpError,
    NativePublicBackfillTime, NativePurpleAirTransport, NativeTempestTideTransport,
    NativeWorkerProviderEvidence, NativeWundergroundTransport, OpenMeteoArchiveOperation,
    PublicStationStartupDiagnostics, WorkerPublicStationArchiveOwners,
    WorkerSupplementalPublicStationOperations,
};

/// Actual supplemental public-station archive provider over the shared native HTTP client.
pub type NativeSupplementalBackfillProvider = WorkerSupplementalPublicStationOperations<
    NativeWundergroundTransport,
    NativeNetatmoTransport,
    NativePurpleAirTransport,
    NativeWorkerProviderEvidence,
>;

/// Actual Open-Meteo archive provider with native calendar, JSON, and receipt-clock values.
pub type NativeOpenMeteoArchiveBackfillProvider =
    OpenMeteoArchiveOperation<NativeOpenMeteoTransport, NativeOpenMeteoValues>;

/// Existing worker-startup owner specialized to actual native archive providers.
pub type NativePublicStationStartupOwners<Diagnostics> = WorkerPublicStationArchiveOwners<
    NativeAmbientWeatherTransport,
    NativeSupplementalBackfillProvider,
    NativePublicBackfillTime,
    Diagnostics,
>;

/// Factory retaining the shared process HTTP runtime for every backfill family.
pub struct NativeWorkerBackfillOwner {
    client: Arc<NativeOutboundHttp>,
}

impl NativeWorkerBackfillOwner {
    /// Start one outbound runtime for a standalone historical-backfill process.
    pub fn new() -> Result<Self, NativeOutboundHttpError> {
        NativeOutboundHttp::new().map(|client| Self {
            client: Arc::new(client),
        })
    }

    /// Adopt the worker's process-wide outbound runtime without creating another pool.
    pub const fn with_client(client: Arc<NativeOutboundHttp>) -> Self {
        Self { client }
    }

    /// Compose the actual public-station providers and clock used by startup coordination.
    pub fn public_station_startup<Diagnostics>(
        &self,
        diagnostics: Arc<Diagnostics>,
    ) -> Result<NativePublicStationStartupOwners<Diagnostics>, NativeOutboundHttpError>
    where
        Diagnostics: PublicStationStartupDiagnostics,
    {
        let ambient = NativeAmbientWeatherTransport::with_client(Arc::clone(&self.client))?;
        let supplemental = self.supplemental_public_station()?;
        Ok(WorkerPublicStationArchiveOwners {
            ambient,
            supplemental,
            time: NativePublicBackfillTime::default(),
            diagnostics,
        })
    }

    /// Compose the actual supplemental public-station operations with no preflight traffic.
    pub fn supplemental_public_station(
        &self,
    ) -> Result<NativeSupplementalBackfillProvider, NativeOutboundHttpError> {
        Ok(WorkerSupplementalPublicStationOperations::new(
            NativeWundergroundTransport::with_shared_client(Arc::clone(&self.client))?,
            NativeNetatmoTransport::with_shared_client(Arc::clone(&self.client))?,
            NativePurpleAirTransport::with_client(Arc::clone(&self.client))?,
            NativeWorkerProviderEvidence,
        ))
    }

    /// Compose the actual Tempest bulk provider from an already-admitted credential.
    pub fn tempest(
        &self,
        api_key: String,
    ) -> Result<NativeTempestTideTransport, NativeOutboundHttpError> {
        NativeTempestTideTransport::with_client(Arc::clone(&self.client), api_key)
    }

    /// Compose the credential-free actual NOAA observation and prediction provider.
    pub fn noaa(&self) -> Result<NativeNoaaTideTransport, NativeOutboundHttpError> {
        NativeNoaaTideTransport::with_client(Arc::clone(&self.client))
    }

    /// Compose the actual Open-Meteo Archive provider and native value owners.
    pub fn open_meteo_archive(
        &self,
        compatibility_origin: Option<String>,
    ) -> NativeOpenMeteoArchiveBackfillProvider {
        OpenMeteoArchiveOperation::new(
            NativeOpenMeteoTransport::with_shared_client(Arc::clone(&self.client)),
            NativeOpenMeteoValues::new(),
            compatibility_origin,
            ProviderControls::default(),
        )
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::NativeWorkerBackfillOwner;
    use crate::{PublicStationStartupDiagnostic, PublicStationStartupDiagnostics};

    /// Bounded diagnostic sink proving startup composition without provider traffic.
    #[derive(Default)]
    struct Diagnostics(Mutex<usize>);

    impl PublicStationStartupDiagnostics for Diagnostics {
        /// Count only an actually emitted startup diagnostic.
        fn record(&self, _diagnostic: PublicStationStartupDiagnostic<'_>) {
            // retain a finite count without storing source identities
            if let Ok(mut count) = self.0.lock() {
                *count += 1;
            }
        }
    }

    /// Every credential-free provider owner composes without traffic or a database pool.
    #[test]
    fn composes_actual_credential_free_backfill_owners() {
        let owner = NativeWorkerBackfillOwner::new().unwrap();
        let _startup = owner
            .public_station_startup(Arc::new(Diagnostics::default()))
            .unwrap();
        let _noaa = owner.noaa().unwrap();
        let _open_meteo = owner.open_meteo_archive(None);
    }
}
