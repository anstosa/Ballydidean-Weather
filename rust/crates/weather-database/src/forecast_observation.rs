use std::error::Error;
use std::fmt::{Display, Formatter};

use weather_contracts::{
    ForecastObservationInput, ForecastObservationInputRow, ForecastObservationInputSiteSlugLiteral,
    ForecastObservationInputTimezoneLiteral, JsDateOrString,
};

/// the exact source-owned fixed-site observation input statement
pub const FORECAST_OBSERVATION_INPUT_SQL: &str = r#"
      WITH target_site AS (
        SELECT
          si.id,
          si.slug,
          si.timezone,
          (($1::timestamptz AT TIME ZONE si.timezone)::date::timestamp
            AT TIME ZONE si.timezone) AS projection_from
        FROM sites si
        WHERE si.slug = 'ballydidean'
          AND si.timezone = 'America/Los_Angeles'
          AND si.active
      ), geometry AS (
        SELECT
          target_site.*,
          LEAST(
            target_site.projection_from - interval '3 hours',
            $1::timestamptz - interval '24 hours 20 minutes'
          ) AS query_from
        FROM target_site
      )
      SELECT
        geometry.slug AS "siteSlug",
        geometry.timezone,
        geometry.projection_from AS "projectionFrom",
        geometry.query_from AS "queryFrom",
        $1::timestamptz AS through,
        input.id,
        input.valid_at AS "validAt",
        input.last_received_at AS "lastReceivedAt",
        input.source_id AS "sourceId",
        input.source_key AS "sourceKey",
        input.source_kind AS "sourceKind",
        input.station_slug AS "stationSlug",
        input.provider_key AS "providerKey",
        input.temperature_c AS "temperatureC",
        input.apparent_temperature_c AS "apparentTemperatureC",
        input.wind_speed_mps AS "windSpeedMps",
        input.wind_gust_mps AS "windGustMps",
        input.wind_direction_degrees AS "windDirectionDegrees",
        input.precipitation_rate_mm_per_hour AS "precipitationRateMmPerHour",
        input.cloud_cover_percent AS "cloudCoverPercent",
        input.relative_humidity_percent AS "relativeHumidityPercent",
        input.pm25_micrograms_per_cubic_meter AS "pm25MicrogramsPerCubicMeter",
        input.uv_index AS "uvIndex",
        input.pressure_hpa AS "pressureHpa",
        input.solar_radiation_wm2 AS "solarRadiationWm2"
      FROM geometry
      LEFT JOIN LATERAL (
        SELECT
          wr.id, wr.valid_at, wr.last_received_at, wr.source_id,
          s.source_key, s.source_kind, st.slug AS station_slug,
          p.provider_key, wr.temperature_c, wr.apparent_temperature_c,
          wr.wind_speed_mps, wr.wind_gust_mps, wr.wind_direction_degrees,
          wr.precipitation_rate_mm_per_hour, wr.cloud_cover_percent,
          wr.relative_humidity_percent, wr.pm25_micrograms_per_cubic_meter,
          wr.uv_index, wr.pressure_hpa, wr.solar_radiation_wm2
        FROM stations st
        JOIN sources s ON s.station_id = st.id
        JOIN providers p ON p.id = s.provider_id
        JOIN weather_records wr ON wr.source_id = s.id
        WHERE st.site_id = geometry.id
          AND st.active
          AND s.active
          AND p.active
          AND s.source_kind IN ('physical_sensor', 'model_current')
          AND weather_source_is_current(s.id)
          AND wr.valid_at >= geometry.query_from
          AND wr.valid_at < $1::timestamptz
        ORDER BY
          CASE s.source_kind WHEN 'physical_sensor' THEN 0 ELSE 1 END,
          wr.valid_at DESC,
          wr.id DESC
        LIMIT 20001
      ) input ON true
      ORDER BY
        CASE input.source_kind WHEN 'physical_sensor' THEN 0 ELSE 1 END,
        input.valid_at DESC NULLS LAST,
        input.id DESC NULLS LAST
    "#;

/// one admitted nominal data instant for the fixed private read
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdmittedForecastObservationQuery {
    as_of: String,
}

impl AdmittedForecastObservationQuery {
    /// retain one canonical UTC instant admitted by the shared time boundary
    pub fn admitted(as_of: String) -> Self {
        Self { as_of }
    }
}

/// one typed private observation statement bind
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ForecastObservationInputBind {
    UtcInstant(String),
}

/// the exact private observation statement and bind
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForecastObservationInputReadPlan {
    pub sql: &'static str,
    pub binds: [ForecastObservationInputBind; 1],
}

/// one decoded database row with optional left-lateral observation data
#[derive(Debug, Clone, PartialEq)]
pub struct ForecastObservationInputDbRow {
    pub projection_from: JsDateOrString,
    pub query_from: JsDateOrString,
    pub record: Option<ForecastObservationInputRow>,
    pub site_slug: String,
    pub through: JsDateOrString,
    pub timezone: String,
}

/// one rejected fixed-site database projection
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForecastObservationInputProjectionError {
    IdentityMismatch,
    RowLimitExceeded,
}

impl Display for ForecastObservationInputProjectionError {
    /// render one bounded repository projection failure
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        // distinguish fixed identity and archive completeness failures
        match self {
            Self::IdentityMismatch => formatter.write_str("forecast observation identity mismatch"),
            Self::RowLimitExceeded => {
                formatter.write_str("forecast observation input row limit exceeded")
            }
        }
    }
}

impl Error for ForecastObservationInputProjectionError {}

/// build the exact one-bind fixed-site private read
pub fn forecast_observation_input_read_plan(
    query: &AdmittedForecastObservationQuery,
) -> ForecastObservationInputReadPlan {
    ForecastObservationInputReadPlan {
        sql: FORECAST_OBSERVATION_INPUT_SQL,
        binds: [ForecastObservationInputBind::UtcInstant(
            query.as_of.clone(),
        )],
    }
}

/// project decoded rows to the generated fixed-site input contract
pub fn project_forecast_observation_input_rows(
    rows: Vec<ForecastObservationInputDbRow>,
) -> Result<Option<ForecastObservationInput>, ForecastObservationInputProjectionError> {
    let Some(first) = rows.first() else {
        return Ok(None);
    };

    // require the SQL-owned fixed site and timezone before constructing literals
    if first.site_slug != ForecastObservationInputSiteSlugLiteral::WIRE_VALUE
        || first.timezone != ForecastObservationInputTimezoneLiteral::WIRE_VALUE
    {
        return Err(ForecastObservationInputProjectionError::IdentityMismatch);
    }
    let projection_from = first.projection_from.clone();
    let query_from = first.query_from.clone();
    let through = first.through.clone();
    let records = rows
        .into_iter()
        .filter_map(|row| row.record)
        .collect::<Vec<_>>();

    // refuse an incomplete archive instead of truncating projection input
    if records.len() > 20_000 {
        return Err(ForecastObservationInputProjectionError::RowLimitExceeded);
    }

    Ok(Some(ForecastObservationInput {
        projectionFrom: projection_from,
        queryFrom: query_from,
        records,
        siteSlug: ForecastObservationInputSiteSlugLiteral,
        timezone: ForecastObservationInputTimezoneLiteral,
        through,
    }))
}
