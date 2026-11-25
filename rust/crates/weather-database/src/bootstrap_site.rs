use weather_contracts::{Nullable, SiteConfiguration, SourceKind, StationKind};

use crate::bootstrap::{
    BootstrapBind, BootstrapDriverError, BootstrapError, BootstrapJsonEncoder, BootstrapPool,
    BootstrapSession, PROVIDER_SQL, SITE_SQL, SOURCE_SQL, STATION_SQL, nullable_text, query_id,
    query_source_id, wire_enum, with_bootstrap,
};

/// Stable identifiers produced by one idempotent base-site bootstrap.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootstrapSiteReceipt {
    pub provider_id: String,
    pub site_id: String,
    pub source_ids: Vec<String>,
    pub station_id: String,
}

/// Bootstrap the generated base site, provider, station and immutable source catalog.
pub fn bootstrap_site_configuration<Pool, Encoder>(
    pool: &mut Pool,
    configuration: &SiteConfiguration,
    encoder: &mut Encoder,
) -> Result<BootstrapSiteReceipt, BootstrapError<BootstrapDriverError<Pool>, Encoder::Error>>
where
    Pool: BootstrapPool,
    Encoder: BootstrapJsonEncoder,
{
    with_bootstrap(pool, |session| {
        let site = session
            .query(
                SITE_SQL,
                &[
                    BootstrapBind::Text(&configuration.site.key),
                    BootstrapBind::Text(&configuration.site.displayName),
                    BootstrapBind::Float(configuration.site.latitude),
                    BootstrapBind::Float(configuration.site.longitude),
                    BootstrapBind::Text(&configuration.site.timezone),
                    BootstrapBind::Bool(configuration.site.active),
                ],
            )
            .map_err(BootstrapError::Driver)?;
        let provider = session
            .query(
                PROVIDER_SQL,
                &[
                    BootstrapBind::Text(&configuration.provider.key),
                    BootstrapBind::Text(&configuration.provider.displayName),
                    BootstrapBind::Text(&configuration.provider.attributionLabel),
                    BootstrapBind::Text(&configuration.provider.attributionUrl),
                    BootstrapBind::Bool(configuration.provider.active),
                ],
            )
            .map_err(BootstrapError::Driver)?;
        // node runs both initial statements before requiring either returned id
        let site_id = site
            .first_id
            .ok_or_else(|| BootstrapError::MissingRow("site bootstrap".to_owned()))?;
        let provider_id = provider
            .first_id
            .ok_or_else(|| BootstrapError::MissingRow("provider bootstrap".to_owned()))?;
        let station_kind = wire_enum(configuration.station.kind, StationKind::WIRE_VALUES);
        let station_id = query_id(
            session,
            STATION_SQL,
            &[
                BootstrapBind::BigintText(&site_id),
                BootstrapBind::Text(&configuration.station.key),
                BootstrapBind::Text(&configuration.station.displayName),
                BootstrapBind::Text(station_kind),
                BootstrapBind::Float(configuration.station.latitude),
                BootstrapBind::Float(configuration.station.longitude),
                nullable_text(&configuration.station.vendor),
                nullable_text(&configuration.station.model),
                nullable_text(&configuration.station.serial),
                BootstrapBind::Bool(configuration.station.active),
            ],
            || "station bootstrap".to_owned(),
        )?;
        let mut source_ids = Vec::with_capacity(configuration.sources.len());

        // retain configured source order and reject silent material drift
        for source in &configuration.sources {
            let adapter_config = encoder
                .value(&source.adapterConfig)
                .map_err(BootstrapError::Encode)?;
            let capabilities = encoder
                .capabilities(&source.capabilities)
                .map_err(BootstrapError::Encode)?;
            let source_kind = wire_enum(source.sourceKind, SourceKind::WIRE_VALUES);
            let cadence_seconds = match &source.cadenceSeconds {
                Nullable::Null => BootstrapBind::NullInteger,
                Nullable::Value(value) => BootstrapBind::Unsigned(*value),
            };
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
                    cadence_seconds,
                    BootstrapBind::Bool(source.active),
                ],
                "",
                &source.key,
            )?;
            source_ids.push(source_id);
        }

        Ok(BootstrapSiteReceipt {
            provider_id,
            site_id,
            source_ids,
            station_id,
        })
    })
}
