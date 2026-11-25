use std::error::Error;
use std::fmt::{Display, Formatter};

use weather_contracts::{TrendPointRow, TrendQuery};

/// the exact source-owned daily trend statement
pub const TREND_POINTS_SQL: &str = r#"
      WITH daily_sources AS (
        SELECT
          date_trunc('day', wr.valid_at AT TIME ZONE si.timezone) AT TIME ZONE si.timezone AS valid_at,
          (wr.valid_at AT TIME ZONE si.timezone)::date AS calendar_day,
          s.source_kind,
          AVG(wr.temperature_c) AS temperature_c,
          MAX(wr.temperature_c) AS temperature_maximum_c,
          MIN(wr.temperature_c) AS temperature_minimum_c,
          AVG(wr.apparent_temperature_c) AS apparent_temperature_c,
          SUM(wr.precipitation_mm) AS precipitation_mm,
          AVG(wr.wind_speed_mps) AS wind_speed_mps,
          MAX(wr.wind_gust_mps) AS wind_gust_mps,
          DEGREES(ATAN2(
            AVG(SIN(RADIANS(wr.wind_direction_degrees))),
            AVG(COS(RADIANS(wr.wind_direction_degrees)))
          )) AS wind_direction_degrees,
          AVG(wr.pressure_hpa) AS pressure_hpa,
          AVG(wr.relative_humidity_percent) AS relative_humidity_percent
        FROM weather_records wr
        JOIN sources s ON s.id = wr.source_id
        JOIN stations st ON st.id = s.station_id
        JOIN sites si ON si.id = st.site_id
        JOIN providers p ON p.id = s.provider_id
        WHERE si.slug = $1
          AND wr.valid_at >= $2
          AND wr.valid_at < $3
          AND si.active
          AND st.active
          AND s.active
          AND p.active
          AND s.source_kind IN ('model_current', 'reanalysis')
          AND weather_source_is_current(s.id)
        GROUP BY 1, 2, s.source_kind
      ),
      preferred_days AS (
        SELECT
          daily_sources.*,
          ROW_NUMBER() OVER (
            PARTITION BY valid_at
            ORDER BY CASE source_kind WHEN 'reanalysis' THEN 0 ELSE 1 END
          ) AS source_priority
        FROM daily_sources
      ),
      -- average equal hourly farm observations on fixed Pacific standard-time days
      physical_pm_hourly AS (
        SELECT
          (wr.valid_at AT TIME ZONE 'UTC' - interval '8 hours')::date AS calendar_day,
          s.id AS source_id,
          date_trunc('hour', wr.valid_at) AS hour_start,
          AVG(wr.pm25_micrograms_per_cubic_meter) AS hourly_concentration
        FROM weather_records wr
        JOIN sources s ON s.id = wr.source_id
        JOIN stations st ON st.id = s.station_id
        JOIN sites si ON si.id = st.site_id
        JOIN providers p ON p.id = s.provider_id
        WHERE si.slug = $1
          AND si.timezone = 'America/Los_Angeles'
          AND st.slug = 'ballydidean-ecowitt'
          AND p.provider_key = 'ecowitt-local'
          AND wr.valid_at >= $2
          AND wr.valid_at < $3
          AND wr.pm25_micrograms_per_cubic_meter >= 0
          AND wr.pm25_micrograms_per_cubic_meter < 'Infinity'::double precision
          AND si.active
          AND st.active
          AND s.active
          AND p.active
          AND s.source_kind = 'physical_sensor'
          AND wr.source_kind = 'physical_sensor'
          AND weather_source_is_current(s.id)
        GROUP BY 1, s.id, 3
      ),
      physical_pm_days AS (
        SELECT
          calendar_day,
          source_id,
          AVG(hourly_concentration) AS daily_concentration
        FROM physical_pm_hourly
        WHERE (calendar_day::timestamp AT TIME ZONE 'UTC') + interval '32 hours' <= $3
        GROUP BY calendar_day, source_id
        HAVING COUNT(*) >= 18
      ),
      preferred_pm_days AS (
        SELECT
          physical_pm_days.*,
          ROW_NUMBER() OVER (
            PARTITION BY calendar_day
            ORDER BY source_id
          ) AS source_priority
        FROM physical_pm_days
      )
      SELECT
        valid_at AS "validAt",
        temperature_c AS "temperatureC",
        temperature_maximum_c AS "temperatureMaximumC",
        temperature_minimum_c AS "temperatureMinimumC",
        apparent_temperature_c AS "apparentTemperatureC",
        precipitation_mm AS "precipitationMm",
        wind_speed_mps AS "windSpeedMps",
        wind_gust_mps AS "windGustMps",
        wind_direction_degrees AS "windDirectionDegrees",
        pressure_hpa AS "pressureHpa",
        relative_humidity_percent AS "relativeHumidityPercent",
        preferred_pm_days.daily_concentration AS "pm25MicrogramsPerCubicMeter"
      FROM preferred_days
      LEFT JOIN preferred_pm_days
        ON preferred_pm_days.calendar_day = preferred_days.calendar_day
        AND preferred_pm_days.source_priority = 1
      WHERE preferred_days.source_priority = 1
      ORDER BY preferred_days.valid_at ASC
    "#;

/// one trend query carrying admitted canonical epoch evidence
#[derive(Debug, Clone, PartialEq)]
pub struct AdmittedTrendQuery {
    query: TrendQuery,
    from_epoch_milliseconds: i64,
    to_epoch_milliseconds: i64,
}

impl AdmittedTrendQuery {
    /// retain a query already resolved by the shared calendar boundary
    pub fn admitted(
        query: TrendQuery,
        from_epoch_milliseconds: i64,
        to_epoch_milliseconds: i64,
    ) -> Self {
        Self {
            query,
            from_epoch_milliseconds,
            to_epoch_milliseconds,
        }
    }
}

/// one typed trend statement bind in placeholder order
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrendReadBind {
    Text(String),
    UtcInstant(String),
}

/// one exact trend read statement and typed binds
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrendReadPlan {
    pub sql: &'static str,
    pub binds: [TrendReadBind; 3],
}

/// one rejected trend repository query
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrendReadPlanError;

impl Display for TrendReadPlanError {
    /// render the source repository failure text
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("trend from must be earlier than to")
    }
}

impl Error for TrendReadPlanError {}

/// ordered typed trend rows after database decoding
#[derive(Debug, Clone, PartialEq)]
pub struct TrendPointResult {
    pub rows: Vec<TrendPointRow>,
}

/// build the exact ordered trend read plan
pub fn trend_read_plan(query: &AdmittedTrendQuery) -> Result<TrendReadPlan, TrendReadPlanError> {
    // preserve the source repository ordered-window guard
    if query.from_epoch_milliseconds >= query.to_epoch_milliseconds {
        return Err(TrendReadPlanError);
    }

    Ok(TrendReadPlan {
        sql: TREND_POINTS_SQL,
        binds: [
            TrendReadBind::Text(query.query.siteSlug.clone()),
            TrendReadBind::UtcInstant(query.query.from.clone()),
            TrendReadBind::UtcInstant(query.query.to.clone()),
        ],
    })
}

/// preserve database order after typed trend row decoding
pub fn project_trend_point_rows(rows: Vec<TrendPointRow>) -> TrendPointResult {
    TrendPointResult { rows }
}
