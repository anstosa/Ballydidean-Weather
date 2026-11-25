//! Bootstrap the generated first-party Ecowitt and Tempest sensor catalogs.

use weather_contracts::{EcowittConfiguration, SourceCapability, TempestConfiguration};

use crate::bootstrap::{
    ACTIVE_SITE_SQL, BootstrapBind, BootstrapDriverError, BootstrapError, BootstrapJsonEncoder,
    BootstrapPool, ECOWITT_STATION_SQL, PHYSICAL_SOURCE_SQL, PROVIDER_SQL, TEMPEST_STATION_SQL,
    put_record, query_id, query_source_id, with_bootstrap,
};

/// ordered durable identifiers produced by either physical-sensor catalog bootstrap.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SensorBootstrapReceipt {
    pub provider_id: String,
    pub source_ids: Vec<(String, String)>,
    pub station_ids: Vec<(String, String)>,
}

/// bootstrap every generated Tempest station and immutable physical source.
pub fn bootstrap_tempest_configuration<Pool, Encoder>(
    pool: &mut Pool,
    configuration: &TempestConfiguration,
    encoder: &mut Encoder,
) -> Result<SensorBootstrapReceipt, BootstrapError<BootstrapDriverError<Pool>, Encoder::Error>>
where
    Pool: BootstrapPool,
    Encoder: BootstrapJsonEncoder,
{
    with_bootstrap(pool, |session| {
        let site_id = query_id(
            session,
            ACTIVE_SITE_SQL,
            &[BootstrapBind::Text(&configuration.siteKey)],
            || "Tempest site bootstrap".to_owned(),
        )?;
        let provider_id = bootstrap_provider(
            session,
            &configuration.provider.key,
            &configuration.provider.displayName,
            &configuration.provider.attributionLabel,
            &configuration.provider.attributionUrl,
            configuration.provider.active,
            "Tempest provider bootstrap",
        )?;
        let mut station_ids = Vec::with_capacity(configuration.stations.len());
        let mut source_ids = Vec::with_capacity(configuration.stations.len());

        // preserve configured station order in both returned identifier records
        for station in &configuration.stations {
            let station_id = query_id(
                session,
                TEMPEST_STATION_SQL,
                &[
                    BootstrapBind::BigintText(&site_id),
                    BootstrapBind::Text(&station.key),
                    BootstrapBind::Text(&station.displayName),
                    BootstrapBind::Float(station.latitude),
                    BootstrapBind::Float(station.longitude),
                    BootstrapBind::Text(&station.serial),
                    BootstrapBind::Bool(station.active),
                ],
                || format!("Tempest station {} bootstrap", station.key),
            )?;
            let material = encoder
                .value(&station.adapterConfig)
                .map_err(BootstrapError::Encode)?;
            let capabilities = encoder
                .capabilities(&[SourceCapability::Current, SourceCapability::Historical])
                .map_err(BootstrapError::Encode)?;
            let source_id = query_source_id(
                session,
                PHYSICAL_SOURCE_SQL,
                &[
                    BootstrapBind::BigintText(&station_id),
                    BootstrapBind::BigintText(&provider_id),
                    BootstrapBind::Text(&station.sourceKey),
                    BootstrapBind::Json(material.as_str()),
                    BootstrapBind::Text(&station.fingerprint),
                    BootstrapBind::Json(capabilities.as_str()),
                    BootstrapBind::Unsigned(station.cadenceSeconds),
                    BootstrapBind::Bool(station.active),
                ],
                "Tempest ",
                &station.sourceKey,
            )?;
            put_record(&mut station_ids, &station.key, station_id);
            put_record(&mut source_ids, &station.sourceKey, source_id);
        }

        Ok(SensorBootstrapReceipt {
            provider_id,
            source_ids,
            station_ids,
        })
    })
}

/// bootstrap every generated Ecowitt gateway and immutable physical source.
pub fn bootstrap_ecowitt_configuration<Pool, Encoder>(
    pool: &mut Pool,
    configuration: &EcowittConfiguration,
    encoder: &mut Encoder,
) -> Result<SensorBootstrapReceipt, BootstrapError<BootstrapDriverError<Pool>, Encoder::Error>>
where
    Pool: BootstrapPool,
    Encoder: BootstrapJsonEncoder,
{
    with_bootstrap(pool, |session| {
        let site_id = query_id(
            session,
            ACTIVE_SITE_SQL,
            &[BootstrapBind::Text(&configuration.siteKey)],
            || "Ecowitt site bootstrap".to_owned(),
        )?;
        let provider_id = bootstrap_provider(
            session,
            &configuration.provider.key,
            &configuration.provider.displayName,
            &configuration.provider.attributionLabel,
            &configuration.provider.attributionUrl,
            configuration.provider.active,
            "Ecowitt provider bootstrap",
        )?;
        let mut station_ids = Vec::with_capacity(configuration.stations.len());
        let mut source_ids = Vec::with_capacity(configuration.stations.len());

        // preserve configured gateway order in both returned identifier records
        for station in &configuration.stations {
            let station_id = query_id(
                session,
                ECOWITT_STATION_SQL,
                &[
                    BootstrapBind::BigintText(&site_id),
                    BootstrapBind::Text(&station.key),
                    BootstrapBind::Text(&station.displayName),
                    BootstrapBind::Float(station.latitude),
                    BootstrapBind::Float(station.longitude),
                    BootstrapBind::Text(&station.model),
                    BootstrapBind::Text(&station.expectedMac),
                    BootstrapBind::Bool(station.active),
                ],
                || format!("Ecowitt station {} bootstrap", station.key),
            )?;
            let material = encoder
                .value(&station.adapterConfig)
                .map_err(BootstrapError::Encode)?;
            let capabilities = encoder
                .capabilities(&[SourceCapability::Current])
                .map_err(BootstrapError::Encode)?;
            let source_id = query_source_id(
                session,
                PHYSICAL_SOURCE_SQL,
                &[
                    BootstrapBind::BigintText(&station_id),
                    BootstrapBind::BigintText(&provider_id),
                    BootstrapBind::Text(&station.sourceKey),
                    BootstrapBind::Json(material.as_str()),
                    BootstrapBind::Text(&station.fingerprint),
                    BootstrapBind::Json(capabilities.as_str()),
                    BootstrapBind::Unsigned(station.cadenceSeconds),
                    BootstrapBind::Bool(station.active),
                ],
                "Ecowitt ",
                &station.sourceKey,
            )?;
            put_record(&mut station_ids, &station.key, station_id);
            put_record(&mut source_ids, &station.sourceKey, source_id);
        }

        Ok(SensorBootstrapReceipt {
            provider_id,
            source_ids,
            station_ids,
        })
    })
}

/// upsert one shared provider row and return its durable identifier.
fn bootstrap_provider<Session, EncodeError>(
    session: &mut Session,
    key: &str,
    display_name: &str,
    attribution_label: &str,
    attribution_url: &str,
    active: bool,
    label: &'static str,
) -> Result<String, BootstrapError<Session::Error, EncodeError>>
where
    Session: crate::bootstrap::BootstrapSession,
{
    query_id(
        session,
        PROVIDER_SQL,
        &[
            BootstrapBind::Text(key),
            BootstrapBind::Text(display_name),
            BootstrapBind::Text(attribution_label),
            BootstrapBind::Text(attribution_url),
            BootstrapBind::Bool(active),
        ],
        || label.to_owned(),
    )
}
