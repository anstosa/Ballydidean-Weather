//! Compose scheduled provider dispatchers over one retained native HTTP client.

use std::fmt::Display;
use std::sync::Arc;

use weather_contracts::JsonValue;
use weather_database::{
    ScheduledCheckpointReadPort, ScheduledIngestionCommandPort, ScheduledRunFailureCommandPort,
    ScheduledRunStartCommandPort, SerializedJson, Sha256Digest,
};
use weather_edge::{ArchiveWirePrimitives, RevisionArchiveCanonicalCodec};
use weather_providers::ProviderControls;
use weather_worker::{
    SourceRunResult, TaskFailure, WorkerDueSource, WorkerScheduledSourceDispatcher,
};

use crate::{
    AmbientWeatherRuntimeValues, EcowittRuntimeValues, NativeAmbientWeatherTransport,
    NativeEcowittTransport, NativeNetatmoTransport, NativeNoaaTideTransport,
    NativeOpenMeteoScheduledTransport, NativeOutboundHttp, NativeOutboundHttpError,
    NativePurpleAirTransport, NativeRuntimeMaterial, NativeTempestTideTransport,
    NativeWirePrimitives, NativeWundergroundTransport, NoaaTideRuntimeValues,
    OpenMeteoDatabaseBoundary, OpenMeteoForecastEvidenceValues, OpenMeteoRuntimeValues,
    TempestRuntimeValues, WorkerAmbientWeatherDispatcher, WorkerConfiguration,
    WorkerEcowittDispatcher, WorkerIterationDatabaseRepository, WorkerNoaaTideDispatcher,
    WorkerOpenMeteoDispatcher, WorkerOpenMeteoForecastDispatcher, WorkerPublicStationEvidence,
    WorkerSupplementalPublicStationDispatcher, WorkerSupplementalPublicStationOperations,
    WorkerTempestDispatcher,
};

/// Final fallback required by each existing family dispatcher.
#[derive(Clone, Copy, Debug, Default)]
pub struct NativeWorkerProviderTerminal;

impl<Repository> WorkerScheduledSourceDispatcher<Repository> for NativeWorkerProviderTerminal {
    /// Reject a typed source which escaped its selected family route.
    fn run_source<Clock: FnMut() -> i64>(
        &mut self,
        _repository: &mut Repository,
        _source: &WorkerDueSource,
        _clock: &mut Clock,
    ) -> Result<SourceRunResult, TaskFailure> {
        Err(provider_failure("native_provider_route_missing"))
    }
}

/// Native evidence owner shared by paired forecasts and supplemental public stations.
#[derive(Clone, Copy, Debug, Default)]
pub struct NativeWorkerProviderEvidence;

impl OpenMeteoForecastEvidenceValues for NativeWorkerProviderEvidence {
    /// Hash exact ordered response-checksum material with the native SHA-256 owner.
    fn forecast_sha256(&mut self, bytes: &[u8]) -> Result<Sha256Digest, TaskFailure> {
        Ok(Sha256Digest::admitted(sha256_hex(bytes)))
    }

    /// Serialize forecast response metadata in producer order without archive line framing.
    fn forecast_metadata_json(
        &mut self,
        entries: &[(Vec<u16>, JsonValue)],
    ) -> Result<SerializedJson, TaskFailure> {
        serialize_object(entries)
    }
}

impl WorkerPublicStationEvidence for NativeWorkerProviderEvidence {
    /// Hash the exact newline-framed upstream response checksums.
    fn sha256(&mut self, material: &[u8]) -> Result<Sha256Digest, TaskFailure> {
        Ok(Sha256Digest::admitted(sha256_hex(material)))
    }
}

type NativeOpenMeteoCurrentDispatcher =
    WorkerOpenMeteoDispatcher<NativeOpenMeteoScheduledTransport, NativeWorkerProviderTerminal>;

type NativeOpenMeteoDispatcher = WorkerOpenMeteoForecastDispatcher<
    NativeOpenMeteoScheduledTransport,
    NativeWorkerProviderEvidence,
    NativeOpenMeteoCurrentDispatcher,
>;

type NativeAmbientDispatcher =
    WorkerAmbientWeatherDispatcher<NativeAmbientWeatherTransport, NativeWorkerProviderTerminal>;

type NativeEcowittDispatcher =
    WorkerEcowittDispatcher<NativeEcowittTransport, NativeWorkerProviderTerminal>;

type NativeNoaaDispatcher =
    WorkerNoaaTideDispatcher<NativeNoaaTideTransport, NativeWorkerProviderTerminal>;

type NativeSupplementalDispatcher = WorkerSupplementalPublicStationDispatcher<
    NativeWundergroundTransport,
    NativeNetatmoTransport,
    NativePurpleAirTransport,
    NativeWorkerProviderEvidence,
    NativeWorkerProviderTerminal,
>;

type NativeTempestDispatcher =
    WorkerTempestDispatcher<NativeTempestTideTransport, NativeWorkerProviderTerminal>;

/// Concrete scheduled-provider runner retaining all configured family dispatchers.
pub struct NativeWorkerProviderRunner {
    ambient: Option<NativeAmbientDispatcher>,
    ecowitt: Option<NativeEcowittDispatcher>,
    noaa: Option<NativeNoaaDispatcher>,
    open_meteo: NativeOpenMeteoDispatcher,
    supplemental: Option<NativeSupplementalDispatcher>,
    tempest: Option<NativeTempestDispatcher>,
}

impl<'a, Database, Values>
    WorkerScheduledSourceDispatcher<WorkerIterationDatabaseRepository<'a, Database, Values>>
    for NativeWorkerProviderRunner
where
    Database: OpenMeteoDatabaseBoundary,
    Values: AmbientWeatherRuntimeValues
        + EcowittRuntimeValues
        + NoaaTideRuntimeValues
        + OpenMeteoRuntimeValues
        + TempestRuntimeValues,
    <Database::Session as ScheduledCheckpointReadPort>::Error: Display,
    <Database::Session as ScheduledRunStartCommandPort>::Error: Display,
    <Database::Session as ScheduledIngestionCommandPort>::Error: Display,
    <Database::Session as ScheduledRunFailureCommandPort>::Error: Display,
{
    /// Route each typed due source into its already-established scheduler owner.
    fn run_source<Clock: FnMut() -> i64>(
        &mut self,
        repository: &mut WorkerIterationDatabaseRepository<'a, Database, Values>,
        source: &WorkerDueSource,
        clock: &mut Clock,
    ) -> Result<SourceRunResult, TaskFailure> {
        // select exactly one family without consuming an additional clock sample
        match source {
            WorkerDueSource::Ambient(_) => self.ambient.as_mut().map_or_else(
                || Err(provider_failure("provider_unavailable")),
                |dispatcher| dispatcher.run_source(repository, source, clock),
            ),
            WorkerDueSource::Ecowitt(_) => self.ecowitt.as_mut().map_or_else(
                || Err(provider_failure("provider_unavailable")),
                |dispatcher| dispatcher.run_source(repository, source, clock),
            ),
            WorkerDueSource::NoaaTide(_) => self.noaa.as_mut().map_or_else(
                || Err(provider_failure("provider_unavailable")),
                |dispatcher| dispatcher.run_source(repository, source, clock),
            ),
            WorkerDueSource::OpenMeteo(_) => self.open_meteo.run_source(repository, source, clock),
            WorkerDueSource::PublicStation(_) => self.supplemental.as_mut().map_or_else(
                || Err(provider_failure("provider_unavailable")),
                |dispatcher| dispatcher.run_source(repository, source, clock),
            ),
            WorkerDueSource::Tempest(_) => self.tempest.as_mut().map_or_else(
                || Err(provider_failure("provider_unavailable")),
                |dispatcher| dispatcher.run_source(repository, source, clock),
            ),
            WorkerDueSource::Unconfigured(_) => {
                Err(provider_failure("native_provider_route_missing"))
            }
            // preserve the decoder's typed configuration failure if called outside the coordinator
            WorkerDueSource::InvalidConfiguration { failure, .. } => Err(failure.clone()),
        }
    }
}

/// Factory retaining the single process-wide outbound runtime and connection pool.
pub struct NativeWorkerProviderFactory {
    client: Arc<NativeOutboundHttp>,
}

impl NativeWorkerProviderFactory {
    /// Start the one outbound runtime used by every configured scheduled provider.
    pub fn new() -> Result<Self, NativeOutboundHttpError> {
        NativeOutboundHttp::new().map(|client| Self {
            client: Arc::new(client),
        })
    }

    /// Adopt an existing process-wide outbound runtime without creating provider-local pools.
    pub const fn with_client(client: Arc<NativeOutboundHttp>) -> Self {
        Self { client }
    }

    /// Load generated configuration identities and bind every actual native transport.
    pub fn build(
        &self,
        configuration: &WorkerConfiguration,
    ) -> Result<NativeWorkerProviderRunner, TaskFailure> {
        let mut material = NativeRuntimeMaterial;
        let compatibility_origin = configuration.open_meteo_compatibility_origin.clone();
        let open_meteo = WorkerOpenMeteoDispatcher::from_site(
            &configuration.site,
            &mut material,
            NativeOpenMeteoScheduledTransport::with_client(
                Arc::clone(&self.client),
                compatibility_origin.clone(),
            ),
            NativeWorkerProviderTerminal,
            ProviderControls::default(),
        )?
        .with_forecast(
            &configuration.site,
            &mut material,
            NativeOpenMeteoScheduledTransport::with_client(
                Arc::clone(&self.client),
                compatibility_origin,
            ),
            NativeWorkerProviderEvidence,
        )?;

        // construct both public-station schedulers only when the catalog was loaded
        let (ambient, supplemental) = match configuration.public_stations.as_ref() {
            Some(catalog) => {
                let ambient_transport = NativeAmbientWeatherTransport::with_client(Arc::clone(
                    &self.client,
                ))
                .map_err(|_| provider_failure("native_provider_transport_initialization_failed"))?;
                let ambient = WorkerAmbientWeatherDispatcher::from_configuration(
                    catalog,
                    &mut material,
                    Some(ambient_transport),
                    NativeWorkerProviderTerminal,
                )?;
                let operations = WorkerSupplementalPublicStationOperations::new(
                    NativeWundergroundTransport::with_shared_client(Arc::clone(&self.client))
                        .map_err(|_| {
                            provider_failure("native_provider_transport_initialization_failed")
                        })?,
                    NativeNetatmoTransport::with_shared_client(Arc::clone(&self.client)).map_err(
                        |_| provider_failure("native_provider_transport_initialization_failed"),
                    )?,
                    NativePurpleAirTransport::with_client(Arc::clone(&self.client)).map_err(
                        |_| provider_failure("native_provider_transport_initialization_failed"),
                    )?,
                    NativeWorkerProviderEvidence,
                );
                let supplemental = WorkerSupplementalPublicStationDispatcher::from_configuration(
                    catalog,
                    &mut material,
                    operations,
                    NativeWorkerProviderTerminal,
                )?;
                (Some(ambient), Some(supplemental))
            }
            None => (None, None),
        };

        // construct the local gateway dispatcher only when its catalog was loaded
        let ecowitt = configuration
            .ecowitt
            .as_ref()
            .map(|catalog| {
                WorkerEcowittDispatcher::from_configuration(
                    catalog,
                    &mut material,
                    NativeEcowittTransport::with_client(Arc::clone(&self.client)),
                    NativeWorkerProviderTerminal,
                )
            })
            .transpose()?;

        // construct NOAA independently because it owns no Tempest credential
        let noaa = configuration
            .tides
            .as_ref()
            .map(|catalog| {
                let transport = NativeNoaaTideTransport::with_client(Arc::clone(&self.client))
                    .map_err(|_| {
                        provider_failure("native_provider_transport_initialization_failed")
                    })?;
                WorkerNoaaTideDispatcher::from_configuration(
                    catalog,
                    &mut material,
                    transport,
                    NativeWorkerProviderTerminal,
                )
            })
            .transpose()?;

        // require the loader's credential/configuration pair before admitting Tempest traffic
        let tempest = match (
            configuration.tempest.as_ref(),
            configuration.tempest_api_key.as_ref(),
        ) {
            (Some(catalog), Some(api_key)) => {
                let transport = NativeTempestTideTransport::with_client(
                    Arc::clone(&self.client),
                    api_key.clone(),
                )
                .map_err(|_| provider_failure("native_provider_transport_initialization_failed"))?;
                WorkerTempestDispatcher::from_configuration(
                    catalog,
                    &mut material,
                    Some(transport),
                    NativeWorkerProviderTerminal,
                )
                .map(Some)?
            }
            (None, None) => None,
            _ => return Err(provider_failure("tempest_configuration_pair_required")),
        };

        Ok(NativeWorkerProviderRunner {
            ambient,
            ecowitt,
            noaa,
            open_meteo,
            supplemental,
            tempest,
        })
    }
}

/// Serialize one ordered provider metadata object without archive newline framing.
fn serialize_object(entries: &[(Vec<u16>, JsonValue)]) -> Result<SerializedJson, TaskFailure> {
    let mut bytes = ArchiveWirePrimitives::encode_ordered_json(
        &NativeWirePrimitives,
        &JsonValue::Object(entries.to_vec()),
    )
    .ok_or_else(|| provider_failure("native_provider_evidence_invalid"))?;
    // remove only the framing byte guaranteed by the native archive encoder
    if bytes.pop() != Some(b'\n') {
        return Err(provider_failure("native_provider_evidence_invalid"));
    }
    String::from_utf8(bytes)
        .map(SerializedJson::admitted)
        .map_err(|_| provider_failure("native_provider_evidence_invalid"))
}

/// Render the native SHA-256 output as the database contract's lowercase hexadecimal digest.
fn sha256_hex(bytes: &[u8]) -> String {
    let digest = RevisionArchiveCanonicalCodec::sha256(&NativeWirePrimitives, bytes);
    let mut text = String::with_capacity(64);
    // append exactly two lowercase hexadecimal digits for every digest byte
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(text, "{byte:02x}");
    }
    text
}

/// Construct one bounded factory or terminal routing failure.
fn provider_failure(code: &str) -> TaskFailure {
    TaskFailure {
        attempts: 0,
        code: code.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use weather_contracts::JsonValue;
    use weather_database::{DatabaseConfiguration, SerializedJson};

    use super::{
        NativeWorkerProviderEvidence, NativeWorkerProviderFactory, serialize_object, sha256_hex,
    };
    use crate::{
        OpenMeteoForecastEvidenceValues, WorkerConfiguration, WorkerPublicStationEvidence,
        native_worker_public_tide_catalogs::parse_tide_configuration,
        native_worker_site_catalogs::parse_site_configuration,
    };

    /// Native evidence uses exact bytes for both scheduled checksum owners.
    #[test]
    fn provider_evidence_uses_shared_native_hash_and_ordered_json() {
        let mut evidence = NativeWorkerProviderEvidence;
        let expected = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        assert_eq!(sha256_hex(b"abc"), expected);
        assert_eq!(evidence.forecast_sha256(b"abc").unwrap().as_str(), expected);
        assert_eq!(evidence.sha256(b"abc").unwrap().as_str(), expected);

        let fields = vec![
            ("z".encode_utf16().collect(), JsonValue::Number(1.0)),
            ("a".encode_utf16().collect(), JsonValue::Boolean(true)),
        ];
        assert_eq!(
            serialize_object(&fields).unwrap(),
            SerializedJson::admitted(r#"{"z":1,"a":true}"#)
        );
    }

    /// Actual site and NOAA source fixtures build without traffic or a Tempest credential.
    #[test]
    fn factory_builds_source_dispatchers_without_provider_traffic() {
        let site =
            parse_site_configuration(include_bytes!("../../../../config/sites/ballydidean.json"))
                .unwrap();
        let tides =
            parse_tide_configuration(include_bytes!("../../../../config/tides/noaa.json")).unwrap();
        let configuration = WorkerConfiguration {
            adjustment_maintenance_api_origin: None,
            database: DatabaseConfiguration {
                application_name: "provider-factory-test".to_owned(),
                database: "weather".to_owned(),
                host: "database.invalid".to_owned(),
                lock_timeout_ms: 10_000,
                password: "fixture".to_owned(),
                port: 5432,
                ssl: false,
                statement_timeout_ms: 30_000,
                user: "worker".to_owned(),
            },
            ecowitt: None,
            instance: "provider-factory-test".to_owned(),
            migration_authorization: None,
            migration_directory: "packages/database/migrations".to_owned(),
            open_meteo_compatibility_origin: None,
            public_stations: None,
            rain_collection_enabled: false,
            site,
            site_configuration_path: "config/sites/ballydidean.json".to_owned(),
            tempest: None,
            tempest_api_key: None,
            temperature_canary_kill_switch: None,
            tides: Some(tides),
            version: "2026.11.24-1".to_owned(),
        };

        let factory = NativeWorkerProviderFactory::new().unwrap();
        let _runner = factory.build(&configuration).unwrap();
    }
}
