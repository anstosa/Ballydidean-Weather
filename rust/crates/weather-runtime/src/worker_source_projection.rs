//! Project generated due-source rows into owned worker tasks after matching loaded catalogs.

use weather_contracts::{
    DueSource, JsonValue, PublicStationAdapter, TideSourceConfigurationSourceKindEnum,
};
use weather_worker::{
    AMBIENT_WEATHER_ADAPTER_VERSION, AmbientWeatherScheduledSource, AmbientWeatherSourceKind,
    ConfiguredScheduledSource, ECOWITT_LOCAL_LIVE_ADAPTER_VERSION, EcowittScheduledSource,
    EcowittSourceKind, NOAA_TIDE_OBSERVATION_ADAPTER_VERSION, NOAA_TIDE_PREDICTION_ADAPTER_VERSION,
    NoaaTideScheduledSource, NoaaTideSourceKind, OPEN_METEO_CURRENT_ADAPTER_VERSION,
    OPEN_METEO_FORECAST_ADAPTER_VERSION, OpenMeteoScheduledSource, OpenMeteoSourceKind,
    PublicStationScheduledSource, ScheduledSourceCatalogs, ScheduledSourceMaterialComparison,
    TaskFailure, TempestScheduledSource, TempestSourceKind, WorkerDueSource,
    classify_scheduled_source, supplemental_public_station_contract,
};

use crate::{WorkerConfiguration, WorkerDueSourceProjection};

const OPEN_METEO_CURRENT_CATALOG_CONTRACT_VERSION: &str = "forecast-current/v1";
const OPEN_METEO_FORECAST_CATALOG_CONTRACT_VERSION: &str = "forecast-daily/v4";
const TEMPEST_CATALOG_CONTRACT_VERSION: &str = "tempest-observations/v2";

/// Approved canonical material owner shared with provider runtime configuration projections.
pub trait WorkerSourceMaterialIdentity: ScheduledSourceMaterialComparison {
    /// Return the exact admitted material identity consumed by the loaded provider configuration.
    fn material_identity(&mut self, value: &JsonValue) -> Result<String, TaskFailure>;
}

/// Bind selected providers to the original loaded configuration, not to bare database keys.
pub struct ConfiguredWorkerDueSourceProjection<'a, Material> {
    catalogs: ScheduledSourceCatalogs<'a>,
    material: Material,
}

impl<'a, Material> ConfiguredWorkerDueSourceProjection<'a, Material> {
    /// Reuse the actual process configuration that selected provider runtimes.
    pub const fn new(configuration: &'a WorkerConfiguration, material: Material) -> Self {
        Self::from_catalogs(
            ScheduledSourceCatalogs {
                site: &configuration.site,
                ecowitt: configuration.ecowitt.as_ref(),
                public_stations: configuration.public_stations.as_ref(),
                tempest: configuration.tempest.as_ref(),
                tides: configuration.tides.as_ref(),
            },
            material,
        )
    }

    /// Keep an already admitted view without cloning any catalog.
    pub const fn from_catalogs(catalogs: ScheduledSourceCatalogs<'a>, material: Material) -> Self {
        Self { catalogs, material }
    }
}

macro_rules! own_source {
    ($source:ident, $identity:ident, $type:ident, $($key:ident: $value:expr),* $(,)?) => {
        $type {
            active: $source.active,
            cadence_seconds: $source.cadenceSeconds,
            id: $source.id.clone(),
            material_config_identity: $identity,
            provider_key: $source.providerKey.clone(),
            site_slug: $source.siteSlug.clone(),
            source_config_fingerprint: $source.sourceConfigFingerprint.clone(),
            source_key: $source.sourceKey.clone(),
            station_slug: $source.stationSlug.clone(),
            timezone: $source.timezone.clone(),
            $($key: $value),*
        }
    };
}

impl<Material: WorkerSourceMaterialIdentity> WorkerDueSourceProjection
    for ConfiguredWorkerDueSourceProjection<'_, Material>
{
    /// Admit only the loaded source family, with its frozen contract, before opening any session.
    fn project_due_source(
        &mut self,
        source: &DueSource,
    ) -> Result<Option<WorkerDueSource>, TaskFailure> {
        let selection = classify_scheduled_source(source, self.catalogs, &mut self.material);
        // return the worker's ordinary skipped result without another canonicalization
        if selection == ConfiguredScheduledSource::Unconfigured {
            return Ok(None);
        }

        let (values, version) = required_contract(selection, source, self.catalogs);
        // reject invalid configured material before a provider or database session runs
        if !values.iter().any(
            // inspect the already decoded object, never parse or serialize JSON here
            |(key, value)| key.iter().copied().eq("contractVersion".encode_utf16())
                && matches!(value, JsonValue::String(text) if text.iter().copied().eq(version.encode_utf16())),
        ) {
            return Err(TaskFailure { attempts: 0, code: "source_adapter_contract_mismatch".to_owned() });
        }
        let identity = self
            .material
            .material_identity(&source.materialProviderConfig)?;
        Ok(Some(match selection {
            ConfiguredScheduledSource::OpenMeteoCurrent(_) => {
                WorkerDueSource::OpenMeteo(own_source!(source, identity, OpenMeteoScheduledSource,
                    source_kind: OpenMeteoSourceKind::ModelCurrent,
                    adapter_version: OPEN_METEO_CURRENT_ADAPTER_VERSION))
            }
            ConfiguredScheduledSource::OpenMeteoForecast(_) => {
                WorkerDueSource::OpenMeteo(own_source!(source, identity, OpenMeteoScheduledSource,
                    source_kind: OpenMeteoSourceKind::Forecast,
                    adapter_version: OPEN_METEO_FORECAST_ADAPTER_VERSION))
            }
            ConfiguredScheduledSource::Ecowitt { .. } => WorkerDueSource::Ecowitt(
                own_source!(source, identity, EcowittScheduledSource, source_kind: EcowittSourceKind::PhysicalSensor),
            ),
            ConfiguredScheduledSource::Tempest { .. } => WorkerDueSource::Tempest(
                own_source!(source, identity, TempestScheduledSource, source_kind: TempestSourceKind::PhysicalSensor),
            ),
            ConfiguredScheduledSource::AmbientWeather { .. } => WorkerDueSource::Ambient(
                own_source!(source, identity, AmbientWeatherScheduledSource, source_kind: AmbientWeatherSourceKind::PhysicalSensor),
            ),
            ConfiguredScheduledSource::WeatherUnderground {
                source: configured, ..
            }
            | ConfiguredScheduledSource::Netatmo {
                source: configured, ..
            }
            | ConfiguredScheduledSource::Purpleair {
                source: configured, ..
            } => WorkerDueSource::PublicStation(
                own_source!(source, identity, PublicStationScheduledSource, adapter: configured.adapter),
            ),
            ConfiguredScheduledSource::Tide { station, .. } => WorkerDueSource::NoaaTide(
                own_source!(source, identity, NoaaTideScheduledSource, source_kind:
                match station.source.sourceKind {
                    TideSourceConfigurationSourceKindEnum::TideObservation => NoaaTideSourceKind::Observation,
                    TideSourceConfigurationSourceKindEnum::TidePrediction => NoaaTideSourceKind::Prediction,
                }),
            ),
            ConfiguredScheduledSource::Unconfigured => {
                unreachable!("unconfigured source returned before material projection")
            }
        }))
    }
}

type ObjectEntries = [(Vec<u16>, JsonValue)];

/// Match Node's early contract check, including precedence when catalogs share a source key.
fn required_contract<'a>(
    selected: ConfiguredScheduledSource<'a>,
    source: &DueSource,
    catalogs: ScheduledSourceCatalogs<'a>,
) -> (&'a ObjectEntries, &'static str) {
    match selected {
        ConfiguredScheduledSource::OpenMeteoCurrent(configured) => {
            return (
                object_entries(&configured.adapterConfig),
                OPEN_METEO_CURRENT_CATALOG_CONTRACT_VERSION,
            );
        }
        ConfiguredScheduledSource::OpenMeteoForecast(configured) => {
            return (
                object_entries(&configured.adapterConfig),
                OPEN_METEO_FORECAST_CATALOG_CONTRACT_VERSION,
            );
        }
        _ => {}
    }
    // the source checks the first catalog candidate for the key before any I/O
    if let Some(station) = catalogs.ecowitt.and_then(|catalog| {
        catalog
            .stations
            .iter()
            .find(|candidate| candidate.sourceKey == source.sourceKey)
    }) {
        return (
            object_entries(&station.adapterConfig),
            ECOWITT_LOCAL_LIVE_ADAPTER_VERSION,
        );
    }
    if let Some(station) = catalogs.tempest.and_then(|catalog| {
        catalog
            .stations
            .iter()
            .find(|candidate| candidate.sourceKey == source.sourceKey)
    }) {
        return (
            object_entries(&station.adapterConfig),
            TEMPEST_CATALOG_CONTRACT_VERSION,
        );
    }
    if let Some(configured) = catalogs.public_stations.and_then(|catalog| {
        catalog
            .stations
            .iter()
            .flat_map(|station| station.sources.iter())
            .find(|candidate| candidate.key == source.sourceKey)
    }) {
        return (
            &configured.adapterConfig,
            public_station_contract(configured.adapter),
        );
    }
    let ConfiguredScheduledSource::Tide { station, .. } = selected else {
        unreachable!("only tide remains after the retained catalog checks");
    };
    (
        &station.source.adapterConfig,
        match station.source.sourceKind {
            TideSourceConfigurationSourceKindEnum::TideObservation => {
                NOAA_TIDE_OBSERVATION_ADAPTER_VERSION
            }
            TideSourceConfigurationSourceKindEnum::TidePrediction => {
                NOAA_TIDE_PREDICTION_ADAPTER_VERSION
            }
        },
    )
}

/// Obtain an existing generated object view; non-object input has no contract field.
fn object_entries(value: &JsonValue) -> &ObjectEntries {
    match value {
        JsonValue::Object(values) => values,
        _ => &[],
    }
}

/// Select the exact checked adapter contract from the source provider package.
const fn public_station_contract(adapter: PublicStationAdapter) -> &'static str {
    match supplemental_public_station_contract(adapter) {
        Some((version, _)) => version,
        None => AMBIENT_WEATHER_ADAPTER_VERSION,
    }
}
