use weather_contracts::{
    Nullable, PublicStationConfiguration, StationKind, TideConfiguration,
    TideSourceConfigurationSourceKindEnum,
};

use crate::bootstrap::{
    ACTIVE_SITE_SQL, BootstrapBind, BootstrapDriverError, BootstrapError, BootstrapJsonEncoder,
    BootstrapPool, PHYSICAL_SOURCE_SQL, PROVIDER_SQL, PUBLIC_STATION_SQL, SOURCE_SQL, STATION_SQL,
    nullable_text, put_record, query_id, query_source_id, wire_enum, with_bootstrap,
};

/// Stable identifiers produced by one idempotent public-station bootstrap.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootstrapPublicStationReceipt {
    pub provider_ids: Vec<(String, String)>,
    pub source_ids: Vec<(String, String)>,
    pub station_ids: Vec<(String, String)>,
}

/// Stable identifiers produced by one idempotent NOAA tide bootstrap.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootstrapTideReceipt {
    pub provider_id: String,
    pub source_ids: Vec<(String, String)>,
    pub station_ids: Vec<(String, String)>,
}

/// Bootstrap every generated public provider, station and immutable source in order.
pub fn bootstrap_public_station_configuration<Pool, Encoder>(
    pool: &mut Pool,
    configuration: &PublicStationConfiguration,
    encoder: &mut Encoder,
) -> Result<BootstrapPublicStationReceipt, BootstrapError<BootstrapDriverError<Pool>, Encoder::Error>>
where
    Pool: BootstrapPool,
    Encoder: BootstrapJsonEncoder,
{
    with_bootstrap(pool, |session| {
        let site_id = query_id(
            session,
            ACTIVE_SITE_SQL,
            &[BootstrapBind::Text(&configuration.siteKey)],
            || "public-station site bootstrap".to_owned(),
        )?;
        let mut provider_ids = Vec::with_capacity(configuration.providers.len());

        // retain declaration order while matching JavaScript record replacement semantics
        for provider in &configuration.providers {
            let provider_id = query_id(
                session,
                PROVIDER_SQL,
                &[
                    BootstrapBind::Text(&provider.key),
                    BootstrapBind::Text(&provider.displayName),
                    BootstrapBind::Text(&provider.attributionLabel),
                    BootstrapBind::Text(&provider.attributionUrl),
                    BootstrapBind::Bool(provider.active),
                ],
                || format!("public-station provider {} bootstrap", provider.key),
            )?;
            put_record(&mut provider_ids, &provider.key, provider_id);
        }

        let mut station_ids = Vec::with_capacity(configuration.stations.len());
        let source_capacity = configuration
            .stations
            .iter()
            .map(|station| station.sources.len())
            .sum();
        let mut source_ids = Vec::with_capacity(source_capacity);

        // retain station and nested source execution order from the generated catalog
        for station in &configuration.stations {
            let station_id = query_id(
                session,
                PUBLIC_STATION_SQL,
                &[
                    BootstrapBind::BigintText(&site_id),
                    BootstrapBind::Text(&station.key),
                    BootstrapBind::Text(&station.displayName),
                    BootstrapBind::Float(station.latitude),
                    BootstrapBind::Float(station.longitude),
                    nullable_text(&station.vendor),
                    nullable_text(&station.model),
                    nullable_text(&station.serial),
                    BootstrapBind::Bool(station.active),
                ],
                || format!("public station {} bootstrap", station.key),
            )?;
            put_record(&mut station_ids, &station.key, station_id.clone());

            // preserve each station's declared immutable-source order
            for source in &station.sources {
                let provider_id = provider_ids
                    .iter()
                    .find_map(|(key, id)| (key == &source.providerKey).then_some(id.as_str()))
                    .ok_or_else(|| BootstrapError::MissingProvider(source.providerKey.clone()))?;
                let adapter_config = encoder
                    .object(&source.adapterConfig)
                    .map_err(BootstrapError::Encode)?;
                let capabilities = encoder
                    .capabilities(&source.capabilities)
                    .map_err(BootstrapError::Encode)?;
                // map the generated nullable cadence without numeric coercion
                let cadence_seconds = match &source.cadenceSeconds {
                    Nullable::Null => BootstrapBind::NullInteger,
                    Nullable::Value(value) => BootstrapBind::Unsigned(*value),
                };
                let source_id = query_source_id(
                    session,
                    PHYSICAL_SOURCE_SQL,
                    &[
                        BootstrapBind::BigintText(&station_id),
                        BootstrapBind::BigintText(provider_id),
                        BootstrapBind::Text(&source.key),
                        BootstrapBind::Json(adapter_config.as_str()),
                        BootstrapBind::Text(&source.fingerprint),
                        BootstrapBind::Json(capabilities.as_str()),
                        cadence_seconds,
                        BootstrapBind::Bool(source.active),
                    ],
                    "public-station ",
                    &source.key,
                )?;
                put_record(&mut source_ids, &source.key, source_id);
            }
        }

        Ok(BootstrapPublicStationReceipt {
            provider_ids,
            source_ids,
            station_ids,
        })
    })
}

/// Bootstrap the generated NOAA provider, stations and immutable tide sources in order.
pub fn bootstrap_tide_configuration<Pool, Encoder>(
    pool: &mut Pool,
    configuration: &TideConfiguration,
    encoder: &mut Encoder,
) -> Result<BootstrapTideReceipt, BootstrapError<BootstrapDriverError<Pool>, Encoder::Error>>
where
    Pool: BootstrapPool,
    Encoder: BootstrapJsonEncoder,
{
    with_bootstrap(pool, |session| {
        let site_id = query_id(
            session,
            ACTIVE_SITE_SQL,
            &[BootstrapBind::Text(&configuration.siteKey)],
            || "tide site bootstrap".to_owned(),
        )?;
        let provider_id = query_id(
            session,
            PROVIDER_SQL,
            &[
                BootstrapBind::Text(&configuration.provider.key),
                BootstrapBind::Text(&configuration.provider.displayName),
                BootstrapBind::Text(&configuration.provider.attributionLabel),
                BootstrapBind::Text(&configuration.provider.attributionUrl),
                BootstrapBind::Bool(configuration.provider.active),
            ],
            || "tide provider bootstrap".to_owned(),
        )?;
        let mut station_ids = Vec::with_capacity(configuration.stations.len());
        let mut source_ids = Vec::with_capacity(configuration.stations.len());

        // retain ordinary station and source execution order from the generated catalog
        for station in &configuration.stations {
            let station_kind = wire_enum(station.kind, StationKind::WIRE_VALUES);
            let station_id = query_id(
                session,
                STATION_SQL,
                &[
                    BootstrapBind::BigintText(&site_id),
                    BootstrapBind::Text(&station.key),
                    BootstrapBind::Text(&station.displayName),
                    BootstrapBind::Text(station_kind),
                    BootstrapBind::Float(station.latitude),
                    BootstrapBind::Float(station.longitude),
                    BootstrapBind::Text(&station.vendor),
                    BootstrapBind::Text(&station.model),
                    BootstrapBind::Text(&station.serial),
                    BootstrapBind::Bool(station.active),
                ],
                || format!("tide station {} bootstrap", station.key),
            )?;
            let source = &station.source;
            let adapter_config = encoder
                .object(&source.adapterConfig)
                .map_err(BootstrapError::Encode)?;
            let capabilities = encoder
                .capabilities(&source.capabilities)
                .map_err(BootstrapError::Encode)?;
            let source_kind = wire_enum(
                source.sourceKind,
                TideSourceConfigurationSourceKindEnum::WIRE_VALUES,
            );
            let source_id = query_source_id(
                session,
                SOURCE_SQL,
                &[
                    BootstrapBind::BigintText(&station_id),
                    BootstrapBind::BigintText(&provider_id),
                    BootstrapBind::Text(&source.key),
                    BootstrapBind::Text(source_kind),
                    BootstrapBind::Json(adapter_config.as_str()),
                    BootstrapBind::Text(&source.fingerprint),
                    BootstrapBind::Json(capabilities.as_str()),
                    BootstrapBind::Unsigned(source.cadenceSeconds),
                    BootstrapBind::Bool(station.active && source.active),
                ],
                "tide ",
                &source.key,
            )?;
            put_record(&mut station_ids, &station.key, station_id);
            put_record(&mut source_ids, &source.key, source_id);
        }

        Ok(BootstrapTideReceipt {
            provider_id,
            source_ids,
            station_ids,
        })
    })
}
