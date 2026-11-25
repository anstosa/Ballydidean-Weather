use std::error::Error;
use std::fmt::{Display, Formatter};

use weather_contracts::{FieldPresence, TideQuery, TideRecordRow};

const MAXIMUM_TIDE_RANGE_MILLISECONDS: i64 = 31 * 86_400_000;

/// the exact source-owned tide record statement
pub const TIDE_RECORDS_SQL: &str = r#"
      SELECT
        wr.source_id AS "sourceId",
        wr.source_kind AS "sourceKind",
        wr.valid_at AS "validAt",
        wr.water_level_m AS "waterLevelM",
        wr.provider_metadata ->> 'prediction_type' AS "predictionType",
        st.slug AS "stationSlug",
        st.display_name AS "stationName",
        p.provider_key AS "providerKey",
        p.attribution_label AS "attributionLabel",
        p.attribution_url AS "attributionUrl"
      FROM weather_records wr
      JOIN sources s ON s.id = wr.source_id
      JOIN stations st ON st.id = s.station_id
      JOIN sites si ON si.id = st.site_id
      JOIN providers p ON p.id = s.provider_id
      WHERE si.slug = $1
        AND wr.valid_at >= $2
        AND wr.valid_at < $3
        AND wr.water_level_m IS NOT NULL
        AND s.source_kind IN ('tide_observation', 'tide_prediction')
        AND si.active
        AND st.active
        AND s.active
        AND p.active
        AND weather_source_is_current(s.id)
      ORDER BY wr.valid_at ASC, wr.id ASC
      LIMIT $4
    "#;

/// one tide query carrying admitted canonical epoch evidence
#[derive(Debug, Clone, PartialEq)]
pub struct AdmittedTideQuery {
    query: TideQuery,
    from_epoch_milliseconds: i64,
    to_epoch_milliseconds: i64,
}

impl AdmittedTideQuery {
    /// retain a query already resolved by the shared time boundary
    pub fn admitted(
        query: TideQuery,
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

/// one typed tide statement bind in placeholder order
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TideReadBind {
    Text(String),
    UtcInstant(String),
    Limit(u64),
}

/// one exact tide read statement and typed binds
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TideReadPlan {
    pub sql: &'static str,
    pub binds: [TideReadBind; 4],
}

/// one rejected tide repository query
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TideReadPlanError {
    InvalidRangeOrLimit,
}

impl Display for TideReadPlanError {
    /// render the source repository failure text
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("tide query range or limit is invalid")
    }
}

impl Error for TideReadPlanError {}

/// ordered typed tide rows after database scalar and enum decoding
#[derive(Debug, Clone, PartialEq)]
pub struct TideRecordResult {
    pub rows: Vec<TideRecordRow>,
}

/// build the exact bounded tide read plan
pub fn tide_read_plan(query: &AdmittedTideQuery) -> Result<TideReadPlan, TideReadPlanError> {
    // apply the repository default only when the API omits its limit
    let limit = match query.query.limit {
        FieldPresence::Missing => 10_000,
        FieldPresence::Value(limit) => limit,
    };
    let duration = query
        .to_epoch_milliseconds
        .checked_sub(query.from_epoch_milliseconds);

    // preserve the combined source range and limit guard
    if !matches!(duration, Some(duration) if duration > 0 && duration <= MAXIMUM_TIDE_RANGE_MILLISECONDS)
        || !(1..=10_000).contains(&limit)
    {
        return Err(TideReadPlanError::InvalidRangeOrLimit);
    }

    Ok(TideReadPlan {
        sql: TIDE_RECORDS_SQL,
        binds: [
            TideReadBind::Text(query.query.siteSlug.clone()),
            TideReadBind::UtcInstant(query.query.from.clone()),
            TideReadBind::UtcInstant(query.query.to.clone()),
            TideReadBind::Limit(limit),
        ],
    })
}

/// preserve database order after typed row decoding
pub fn project_tide_record_rows(rows: Vec<TideRecordRow>) -> TideRecordResult {
    TideRecordResult { rows }
}
