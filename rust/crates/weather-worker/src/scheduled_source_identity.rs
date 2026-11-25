use weather_contracts::{
    DueSource, EcowittConfiguration, EcowittStationConfiguration, JsonValue, PublicStationAdapter,
    PublicStationConfiguration, PublicStationConfigurationStation,
    PublicStationSourceConfiguration, SiteConfiguration, SiteConfigurationSource, SourceCapability,
    SourceKind, TempestConfiguration, TempestStationConfiguration, TideConfiguration,
    TideSourceConfigurationSourceKindEnum, TideStationConfiguration,
};

/// Approved canonical JSON owner used only for equality at the configuration boundary.
pub trait ScheduledSourceMaterialComparison {
    type Error;

    /// Compare two generated JSON values after canonical serialization.
    fn values_equal(&mut self, left: &JsonValue, right: &JsonValue) -> Result<bool, Self::Error>;

    /// Compare a decoded database JSON value with one generated ordered object.
    fn value_and_object_equal(
        &mut self,
        value: &JsonValue,
        object: &[(Vec<u16>, JsonValue)],
    ) -> Result<bool, Self::Error>;
}

/// Borrowed loaded catalogs used by one scheduled-source admission.
#[derive(Debug, Clone, Copy)]
pub struct ScheduledSourceCatalogs<'a> {
    pub site: &'a SiteConfiguration,
    pub ecowitt: Option<&'a EcowittConfiguration>,
    pub public_stations: Option<&'a PublicStationConfiguration>,
    pub tempest: Option<&'a TempestConfiguration>,
    pub tides: Option<&'a TideConfiguration>,
}

/// Exact configured family selected before acquiring a source session.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ConfiguredScheduledSource<'a> {
    OpenMeteoCurrent(&'a SiteConfigurationSource),
    OpenMeteoForecast(&'a SiteConfigurationSource),
    Ecowitt {
        configuration: &'a EcowittConfiguration,
        station: &'a EcowittStationConfiguration,
    },
    Tempest {
        configuration: &'a TempestConfiguration,
        station: &'a TempestStationConfiguration,
    },
    AmbientWeather {
        configuration: &'a PublicStationConfiguration,
        station: &'a PublicStationConfigurationStation,
        source: &'a PublicStationSourceConfiguration,
    },
    WeatherUnderground {
        configuration: &'a PublicStationConfiguration,
        station: &'a PublicStationConfigurationStation,
        source: &'a PublicStationSourceConfiguration,
    },
    Netatmo {
        configuration: &'a PublicStationConfiguration,
        station: &'a PublicStationConfigurationStation,
        source: &'a PublicStationSourceConfiguration,
    },
    Purpleair {
        configuration: &'a PublicStationConfiguration,
        station: &'a PublicStationConfigurationStation,
        source: &'a PublicStationSourceConfiguration,
    },
    Tide {
        configuration: &'a TideConfiguration,
        station: &'a TideStationConfiguration,
    },
    Unconfigured,
}

/// Select the first loaded exact runtime contract using the Node branch precedence.
pub fn classify_scheduled_source<'a>(
    source: &DueSource,
    catalogs: ScheduledSourceCatalogs<'a>,
    comparison: &mut impl ScheduledSourceMaterialComparison,
) -> ConfiguredScheduledSource<'a> {
    let site_source = catalogs
        .site
        .sources
        .iter()
        .find(|candidate| candidate.key == source.sourceKey);
    let ecowitt_station = catalogs.ecowitt.and_then(|configuration| {
        configuration
            .stations
            .iter()
            .find(|candidate| candidate.sourceKey == source.sourceKey)
    });
    let tempest_station = catalogs.tempest.and_then(|configuration| {
        configuration
            .stations
            .iter()
            .find(|candidate| candidate.sourceKey == source.sourceKey)
    });
    let public_station = find_public_station_source(catalogs.public_stations, &source.sourceKey);
    let tide_station = catalogs.tides.and_then(|configuration| {
        configuration
            .stations
            .iter()
            .find(|candidate| candidate.source.key == source.sourceKey)
    });

    // retain Open-Meteo current before forecast and every later provider family
    if source.active
        && source.providerKey == "open-meteo"
        && source.sourceKind == SourceKind::ModelCurrent
        && source.siteSlug == catalogs.site.site.key
        && site_source.is_some_and(|configured| {
            configured.capabilities.contains(&SourceCapability::Current)
                && source_identity_matches_site(source, catalogs.site, configured, comparison)
        })
    {
        return ConfiguredScheduledSource::OpenMeteoCurrent(
            site_source.expect("matched current source"),
        );
    }
    if source.active
        && source.providerKey == "open-meteo"
        && source.sourceKind == SourceKind::Forecast
        && source.siteSlug == catalogs.site.site.key
        && site_source.is_some_and(|configured| {
            configured
                .capabilities
                .contains(&SourceCapability::Forecast)
                && source_identity_matches_site(source, catalogs.site, configured, comparison)
        })
    {
        return ConfiguredScheduledSource::OpenMeteoForecast(
            site_source.expect("matched forecast source"),
        );
    }
    if let (Some(configuration), Some(station)) = (catalogs.ecowitt, ecowitt_station)
        && source.active
        && source_identity_matches_ecowitt(source, configuration, station, comparison)
    {
        return ConfiguredScheduledSource::Ecowitt {
            configuration,
            station,
        };
    }
    if let (Some(configuration), Some(station)) = (catalogs.tempest, tempest_station)
        && source.active
        && source_identity_matches_tempest(source, configuration, station, comparison)
    {
        return ConfiguredScheduledSource::Tempest {
            configuration,
            station,
        };
    }
    if let (Some(configuration), Some((station, configured))) =
        (catalogs.public_stations, public_station)
        && source.active
        && configured.active
        && configured.capabilities.contains(&SourceCapability::Current)
        && source_identity_matches_public_station(
            source,
            configuration,
            station,
            configured,
            comparison,
        )
    {
        return match configured.adapter {
            PublicStationAdapter::AmbientWeather => ConfiguredScheduledSource::AmbientWeather {
                configuration,
                station,
                source: configured,
            },
            PublicStationAdapter::WeatherUnderground => {
                ConfiguredScheduledSource::WeatherUnderground {
                    configuration,
                    station,
                    source: configured,
                }
            }
            PublicStationAdapter::Netatmo => ConfiguredScheduledSource::Netatmo {
                configuration,
                station,
                source: configured,
            },
            PublicStationAdapter::Purpleair => ConfiguredScheduledSource::Purpleair {
                configuration,
                station,
                source: configured,
            },
        };
    }
    if let (Some(configuration), Some(station)) = (catalogs.tides, tide_station)
        && source.active
        && station.active
        && station.source.active
        && source_identity_matches_tide(source, configuration, station, comparison)
    {
        return ConfiguredScheduledSource::Tide {
            configuration,
            station,
        };
    }
    ConfiguredScheduledSource::Unconfigured
}

/// Match one loaded base-site source and fail closed when canonicalization fails.
pub fn source_identity_matches_site(
    source: &DueSource,
    site: &SiteConfiguration,
    configured: &SiteConfigurationSource,
    comparison: &mut impl ScheduledSourceMaterialComparison,
) -> bool {
    source.siteSlug == site.site.key
        && source.stationSlug == site.station.key
        && source.providerKey == site.provider.key
        && source.sourceKey == configured.key
        && source.sourceKind == configured.sourceKind
        && source.sourceConfigFingerprint == configured.fingerprint
        && source.timezone == site.site.timezone
        && comparison
            .values_equal(&source.materialProviderConfig, &configured.adapterConfig)
            .unwrap_or(false)
}

/// Match one loaded Ecowitt gateway and fail closed when canonicalization fails.
pub fn source_identity_matches_ecowitt(
    source: &DueSource,
    configuration: &EcowittConfiguration,
    station: &EcowittStationConfiguration,
    comparison: &mut impl ScheduledSourceMaterialComparison,
) -> bool {
    source.siteSlug == configuration.siteKey
        && source.stationSlug == station.key
        && source.providerKey == configuration.provider.key
        && source.sourceKey == station.sourceKey
        && source.sourceKind == SourceKind::PhysicalSensor
        && source.sourceConfigFingerprint == station.fingerprint
        && source.timezone == station.timezone
        && comparison
            .values_equal(&source.materialProviderConfig, &station.adapterConfig)
            .unwrap_or(false)
}

/// Match one loaded Tempest station and fail closed when canonicalization fails.
pub fn source_identity_matches_tempest(
    source: &DueSource,
    configuration: &TempestConfiguration,
    station: &TempestStationConfiguration,
    comparison: &mut impl ScheduledSourceMaterialComparison,
) -> bool {
    source.siteSlug == configuration.siteKey
        && source.stationSlug == station.key
        && source.providerKey == configuration.provider.key
        && source.sourceKey == station.sourceKey
        && source.sourceKind == SourceKind::PhysicalSensor
        && source.sourceConfigFingerprint == station.fingerprint
        && source.timezone == station.timezone
        && comparison
            .values_equal(&source.materialProviderConfig, &station.adapterConfig)
            .unwrap_or(false)
}

/// Match one loaded public-station source and fail closed on malformed JSON.
pub fn source_identity_matches_public_station(
    source: &DueSource,
    configuration: &PublicStationConfiguration,
    station: &PublicStationConfigurationStation,
    configured: &PublicStationSourceConfiguration,
    comparison: &mut impl ScheduledSourceMaterialComparison,
) -> bool {
    source.siteSlug == configuration.siteKey
        && source.stationSlug == station.key
        && source.providerKey == configured.providerKey
        && source.sourceKey == configured.key
        && source.sourceKind == SourceKind::PhysicalSensor
        && source.sourceConfigFingerprint == configured.fingerprint
        && source.timezone == station.timezone
        && comparison
            .value_and_object_equal(&source.materialProviderConfig, &configured.adapterConfig)
            .unwrap_or(false)
}

/// Match one loaded tide source and fail closed on malformed JSON.
pub fn source_identity_matches_tide(
    source: &DueSource,
    configuration: &TideConfiguration,
    station: &TideStationConfiguration,
    comparison: &mut impl ScheduledSourceMaterialComparison,
) -> bool {
    let configured_kind = match station.source.sourceKind {
        TideSourceConfigurationSourceKindEnum::TideObservation => SourceKind::TideObservation,
        TideSourceConfigurationSourceKindEnum::TidePrediction => SourceKind::TidePrediction,
    };
    source.siteSlug == configuration.siteKey
        && source.stationSlug == station.key
        && source.providerKey == configuration.provider.key
        && source.sourceKey == station.source.key
        && source.sourceKind == configured_kind
        && source.sourceConfigFingerprint == station.source.fingerprint
        && source.timezone == station.timezone
        && comparison
            .value_and_object_equal(
                &source.materialProviderConfig,
                &station.source.adapterConfig,
            )
            .unwrap_or(false)
}

/// Find the first configured public source in station then source order.
fn find_public_station_source<'a>(
    configuration: Option<&'a PublicStationConfiguration>,
    source_key: &str,
) -> Option<(
    &'a PublicStationConfigurationStation,
    &'a PublicStationSourceConfiguration,
)> {
    let configuration = configuration?;
    // scan the bounded parsed catalog in declaration order
    for station in &configuration.stations {
        if let Some(source) = station
            .sources
            .iter()
            .find(|candidate| candidate.key == source_key)
        {
            return Some((station, source));
        }
    }
    None
}
