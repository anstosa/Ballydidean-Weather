use std::error::Error;
use std::fmt::{Display, Formatter};

use weather_contracts::{FieldPresence, HistoryQuery, SourceKind, WeatherRecordRow};

const CURRENT_SOURCE_PREDICATE: &str = "weather_source_is_current(s.id)";

const WEATHER_RECORD_SELECTION: &str = r#"
    wr.id,
    wr.content_hash AS "contentHash",
    wr.source_id AS "sourceId",
    NULL::text AS "sourceConfigFingerprint",
    NULL::text AS "adapterVersion",
    NULL::text AS "contractEpoch",
    s.source_key AS "sourceKey",
    wr.source_kind AS "sourceKind",
    wr.valid_at AS "validAt",
    wr.product_run_at AS "productRunAt",
    wr.first_received_at AS "firstReceivedAt",
    wr.last_received_at AS "lastReceivedAt",
    wr.upstream_timezone AS "upstreamTimezone",
    wr.upstream_model AS "upstreamModel",
    wr.device_vendor AS "deviceVendor",
    wr.device_model AS "deviceModel",
    wr.device_serial AS "deviceSerial",
    wr.quality_metadata AS "qualityMetadata",
    wr.provider_metadata AS "providerMetadata",
    wr.revision_count AS "revisionCount",
    wr.temperature_c AS "temperatureC",
    wr.apparent_temperature_c AS "apparentTemperatureC",
    wr.precipitation_mm AS "precipitationMm",
    wr.wind_speed_mps AS "windSpeedMps",
    wr.wind_gust_mps AS "windGustMps",
    wr.pressure_hpa AS "pressureHpa",
    wr.relative_humidity_percent AS "relativeHumidityPercent",
    wr.cloud_cover_percent AS "cloudCoverPercent",
    wr.wind_direction_degrees AS "windDirectionDegrees",
    wr.black_globe_temperature_c AS "blackGlobeTemperatureC",
    wr.pm25_micrograms_per_cubic_meter AS "pm25MicrogramsPerCubicMeter",
    wr.precipitation_rate_mm_per_hour AS "precipitationRateMmPerHour",
    wr.soil_electrical_conductivity_us_cm AS "soilElectricalConductivityMicrosiemensPerCm",
    wr.soil_moisture_percent AS "soilMoisturePercent",
    wr.solar_radiation_wm2 AS "solarRadiationWm2",
    wr.uv_index AS "uvIndex",
    wr.wet_bulb_globe_temperature_c AS "wetBulbGlobeTemperatureC",
    wr.water_level_m AS "waterLevelM",
    st.slug AS "stationSlug",
    si.slug AS "siteSlug",
    p.provider_key AS "providerKey"
  "#;

const WEATHER_RECORD_LATERAL_SELECTION: &str = r#"
    candidate.id,
    candidate.source_id,
    candidate.source_kind,
    candidate.valid_at,
    candidate.product_run_at,
    candidate.first_received_at,
    candidate.last_received_at,
    candidate.upstream_timezone,
    candidate.upstream_model,
    candidate.device_vendor,
    candidate.device_model,
    candidate.device_serial,
    candidate.quality_metadata,
    candidate.provider_metadata,
    candidate.temperature_c,
    candidate.apparent_temperature_c,
    candidate.precipitation_mm,
    candidate.wind_speed_mps,
    candidate.wind_gust_mps,
    candidate.pressure_hpa,
    candidate.relative_humidity_percent,
    candidate.cloud_cover_percent,
    candidate.wind_direction_degrees,
    candidate.content_hash,
    candidate.revision_count,
    candidate.black_globe_temperature_c,
    candidate.pm25_micrograms_per_cubic_meter,
    candidate.precipitation_rate_mm_per_hour,
    candidate.soil_electrical_conductivity_us_cm,
    candidate.soil_moisture_percent,
    candidate.solar_radiation_wm2,
    candidate.uv_index,
    candidate.wet_bulb_globe_temperature_c,
    candidate.water_level_m
  "#;

/// one typed PostgreSQL history bind in placeholder order
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HistoryReadBind {
    Text(String),
    SourceKind(SourceKind),
    UtcInstant(String),
    BigintText(String),
    Limit(u64),
}

/// one exact dynamic history statement and its typed binds
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryReadPlan {
    pub sql: String,
    pub binds: Vec<HistoryReadBind>,
}

/// one rejected history repository input
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoryReadPlanError {
    LimitOutsideRepositoryRange,
    ReversedRange,
}

impl Display for HistoryReadPlanError {
    /// render the owning repository failure text
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        // preserve the source repository error categories
        match self {
            Self::LimitOutsideRepositoryRange => {
                formatter.write_str("history limit must be between 1 and 251")
            }
            Self::ReversedRange => formatter.write_str("history from must be earlier than to"),
        }
    }
}

impl Error for HistoryReadPlanError {}

/// one ordered typed history result after database scalar decoding
#[derive(Debug, Clone, PartialEq)]
pub struct WeatherHistoryResult {
    pub rows: Vec<WeatherRecordRow>,
}

/// build the exact source-owned dynamic history read
pub fn weather_history_read_plan(
    query: &HistoryQuery,
) -> Result<HistoryReadPlan, HistoryReadPlanError> {
    // apply the repository default only when the caller omits its limit
    let limit = match query.limit {
        FieldPresence::Missing => 100,
        FieldPresence::Value(limit) => limit,
    };

    // enforce the repository edge bound independently from the public API
    if !(1..=251).contains(&limit) {
        return Err(HistoryReadPlanError::LimitOutsideRepositoryRange);
    }

    // canonical UTC strings preserve chronological ordering
    if let (FieldPresence::Value(from), FieldPresence::Value(to)) = (&query.from, &query.to)
        && from >= to
    {
        return Err(HistoryReadPlanError::ReversedRange);
    }

    let mut source_conditions = vec![
        "si.slug = $1".to_owned(),
        "si.active".to_owned(),
        "st.active".to_owned(),
        "s.active".to_owned(),
        "p.active".to_owned(),
        "s.source_kind IN ('physical_sensor', 'model_current', 'reanalysis')".to_owned(),
        CURRENT_SOURCE_PREDICATE.to_owned(),
    ];
    let mut record_conditions = vec!["candidate.source_id = s.id".to_owned()];
    let mut binds = vec![HistoryReadBind::Text(query.siteSlug.clone())];

    // append the optional station bind and predicate
    if let FieldPresence::Value(station_slug) = &query.stationSlug {
        binds.push(HistoryReadBind::Text(station_slug.clone()));
        source_conditions.push(format!("st.slug = ${}", binds.len()));
    }

    // append the optional source bind and predicate
    if let FieldPresence::Value(source_id) = &query.sourceId {
        binds.push(HistoryReadBind::BigintText(source_id.clone()));
        source_conditions.push(format!("s.id = ${}", binds.len()));
    }

    // append the optional provenance bind and predicate
    if let FieldPresence::Value(source_kind) = query.sourceKind {
        binds.push(HistoryReadBind::SourceKind(source_kind));
        source_conditions.push(format!("s.source_kind = ${}", binds.len()));
    }

    // append the inclusive lower time bound
    if let FieldPresence::Value(from) = &query.from {
        binds.push(HistoryReadBind::UtcInstant(from.clone()));
        record_conditions.push(format!("candidate.valid_at >= ${}", binds.len()));
    }

    // append the exclusive upper time bound
    if let FieldPresence::Value(to) = &query.to {
        binds.push(HistoryReadBind::UtcInstant(to.clone()));
        record_conditions.push(format!("candidate.valid_at < ${}", binds.len()));
    }

    // append the stable descending tuple continuation
    if let FieldPresence::Value(cursor) = &query.cursor {
        binds.push(HistoryReadBind::UtcInstant(cursor.validAt.clone()));
        let valid_at_parameter = binds.len();
        binds.push(HistoryReadBind::BigintText(cursor.id.clone()));
        let id_parameter = binds.len();
        record_conditions.push(format!(
            "(candidate.valid_at, candidate.id) < (${valid_at_parameter}::timestamptz, ${id_parameter}::bigint)",
        ));
    }

    binds.push(HistoryReadBind::Limit(limit));
    let limit_parameter = binds.len();
    let sql = format!(
        r#"
      SELECT
        {WEATHER_RECORD_SELECTION}
      FROM sources s
      JOIN stations st ON st.id = s.station_id
      JOIN sites si ON si.id = st.site_id
      JOIN providers p ON p.id = s.provider_id
      JOIN LATERAL (
        SELECT {WEATHER_RECORD_LATERAL_SELECTION}
        FROM weather_records candidate
        WHERE {record_conditions}
        ORDER BY candidate.valid_at DESC, candidate.id DESC
        LIMIT ${limit_parameter}
      ) wr ON true
      WHERE {source_conditions}
      ORDER BY wr.valid_at DESC, wr.id DESC
      LIMIT ${limit_parameter}
    "#,
        record_conditions = record_conditions.join("\n          AND "),
        source_conditions = source_conditions.join("\n        AND "),
    );

    Ok(HistoryReadPlan { sql, binds })
}

/// preserve database order after typed scalar and enum decoding
pub fn project_weather_history_rows(rows: Vec<WeatherRecordRow>) -> WeatherHistoryResult {
    WeatherHistoryResult { rows }
}
