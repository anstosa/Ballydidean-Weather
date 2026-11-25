use std::error::Error;
use std::fmt::{Display, Formatter};

use weather_contracts::{DailyPrecipitationQuery, DailyPrecipitationRow};

/// the exact source-owned nearest-gauge daily precipitation statement
pub const DAILY_PRECIPITATION_SQL: &str = r#"
      WITH nearest_source AS (
        SELECT
          s.id AS source_id,
          st.slug AS station_slug
        FROM sources s
        JOIN stations st ON st.id = s.station_id
        JOIN sites si ON si.id = st.site_id
        JOIN providers p ON p.id = s.provider_id
        WHERE si.slug = $1
          AND si.active
          AND st.active
          AND s.active
          AND p.active
          AND s.source_kind = 'physical_sensor'
          AND weather_source_is_current(s.id)
          AND EXISTS (
            SELECT 1
            FROM weather_records candidate
            WHERE candidate.source_id = s.id
              AND candidate.valid_at >= $2
              AND candidate.valid_at < $3
              AND candidate.precipitation_mm IS NOT NULL
          )
        ORDER BY
          POWER(st.latitude - si.latitude, 2) +
            POWER((st.longitude - si.longitude) * COS(RADIANS(si.latitude)), 2),
          st.slug ASC,
          s.id ASC
        LIMIT 1
      )
      SELECT
        SUM(wr.precipitation_mm)::double precision AS "accumulationMm",
        nearest_source.source_id AS "sourceId",
        nearest_source.station_slug AS "stationSlug",
        MAX(wr.valid_at) AS "validThrough"
      FROM nearest_source
      JOIN weather_records wr ON wr.source_id = nearest_source.source_id
      WHERE wr.valid_at >= $2
        AND wr.valid_at < $3
        AND wr.precipitation_mm IS NOT NULL
      GROUP BY nearest_source.source_id, nearest_source.station_slug
    "#;

/// one daily query carrying admitted canonical epoch evidence
#[derive(Debug, Clone, PartialEq)]
pub struct AdmittedDailyPrecipitationQuery {
    query: DailyPrecipitationQuery,
    from_epoch_milliseconds: i64,
    to_epoch_milliseconds: i64,
}

impl AdmittedDailyPrecipitationQuery {
    /// retain a query already resolved by the shared site-calendar boundary
    pub fn admitted(
        query: DailyPrecipitationQuery,
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

/// one typed daily precipitation bind in placeholder order
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DailyPrecipitationBind {
    Text(String),
    UtcInstant(String),
}

/// one exact daily precipitation read and typed binds
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DailyPrecipitationReadPlan {
    pub sql: &'static str,
    pub binds: [DailyPrecipitationBind; 3],
}

/// one rejected daily precipitation repository window
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DailyPrecipitationReadPlanError;

impl Display for DailyPrecipitationReadPlanError {
    /// render the source repository failure text
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("daily precipitation from must be earlier than to")
    }
}

impl Error for DailyPrecipitationReadPlanError {}

/// one nullable typed daily precipitation result
#[derive(Debug, Clone, PartialEq)]
pub struct DailyPrecipitationResult {
    pub row: Option<DailyPrecipitationRow>,
}

/// bind one admitted local-day window to the exact repository statement
pub fn daily_precipitation_read_plan(
    query: &AdmittedDailyPrecipitationQuery,
) -> Result<DailyPrecipitationReadPlan, DailyPrecipitationReadPlanError> {
    // preserve the source repository ordered-window guard
    if query.from_epoch_milliseconds >= query.to_epoch_milliseconds {
        return Err(DailyPrecipitationReadPlanError);
    }

    Ok(DailyPrecipitationReadPlan {
        sql: DAILY_PRECIPITATION_SQL,
        binds: [
            DailyPrecipitationBind::Text(query.query.siteSlug.clone()),
            DailyPrecipitationBind::UtcInstant(query.query.from.clone()),
            DailyPrecipitationBind::UtcInstant(query.query.to.clone()),
        ],
    })
}

/// preserve the source repository's first-row-or-null behavior
pub fn project_daily_precipitation_row(
    rows: Vec<DailyPrecipitationRow>,
) -> DailyPrecipitationResult {
    DailyPrecipitationResult {
        row: rows.into_iter().next(),
    }
}
