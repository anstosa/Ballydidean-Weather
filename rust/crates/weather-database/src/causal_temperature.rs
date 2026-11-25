use std::error::Error;
use std::fmt::{Display, Formatter};

use weather_contracts::{
    EcmwfTemperatureCanaryPriorHour, ForecastObservationHourlyStationRow,
    ForecastObservationStationKey, Nullable,
};

const HOUR_MILLISECONDS: i64 = 3_600_000;
const MAX_FORECAST_TRAINING_MILLISECONDS: i64 = 450 * 86_400_000 + 2 * HOUR_MILLISECONDS;

/// The exact source-owned causal station observation statement.
pub const CAUSAL_FORECAST_OBSERVATION_HOURLY_STATIONS_SQL: &str =
    include_str!("../fixtures/causal-forecast-observation-hourly-stations.sql");
/// The exact source-owned causal prior ECMWF statement.
pub const CAUSAL_ECMWF_TEMPERATURE_PRIOR_HOURS_SQL: &str =
    include_str!("../fixtures/causal-ecmwf-temperature-prior-hours.sql");

/// Frozen accepted source keys in the Node domain-manifest order.
pub const CAUSAL_FORECAST_OBSERVATION_SOURCE_KEYS: [&str; 12] = [
    "ambient-maxweather-observations-v1",
    "ambient-merlin-observations-v1",
    "ecowitt-88f15505d89f-local-live-v1",
    "netatmo-nearby-observations-v1",
    "tempest-126537-observations-v2",
    "tempest-168853-observations-v2",
    "tempest-201058-observations-v2",
    "tempest-203055-observations-v2",
    "tempest-225947-observations-v2",
    "tempest-38270-observations-v2",
    "tempest-64255-observations-v2",
    "wunderground-maxweather-history-v1",
];

/// One admitted causal physical-station training window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdmittedCausalForecastObservationQuery {
    as_of: String,
    from: String,
    from_epoch_milliseconds: i64,
    site_slug: String,
    to: String,
    to_epoch_milliseconds: i64,
}

impl AdmittedCausalForecastObservationQuery {
    /// Retain canonical strings and exact epoch bounds resolved by the time owner.
    pub fn admitted(
        site_slug: String,
        from: String,
        from_epoch_milliseconds: i64,
        to: String,
        to_epoch_milliseconds: i64,
        as_of: String,
    ) -> Self {
        Self {
            as_of,
            from,
            from_epoch_milliseconds,
            site_slug,
            to,
            to_epoch_milliseconds,
        }
    }
}

/// One admitted inclusive ECMWF prior-hour interval.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdmittedCausalEcmwfPriorHoursQuery {
    from: String,
    from_epoch_milliseconds: i64,
    site_slug: String,
    target_run_initialized_at: String,
    to_inclusive: String,
    to_inclusive_epoch_milliseconds: i64,
}

impl AdmittedCausalEcmwfPriorHoursQuery {
    /// Retain canonical strings and exact epoch bounds resolved by the time owner.
    pub fn admitted(
        site_slug: String,
        from: String,
        from_epoch_milliseconds: i64,
        to_inclusive: String,
        to_inclusive_epoch_milliseconds: i64,
        target_run_initialized_at: String,
    ) -> Self {
        Self {
            from,
            from_epoch_milliseconds,
            site_slug,
            target_run_initialized_at,
            to_inclusive,
            to_inclusive_epoch_milliseconds,
        }
    }
}

/// PostgreSQL scalar or array values in exact placeholder order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CausalTemperatureReadBind {
    Text(String),
    TextArray(Vec<String>),
    UtcInstant(String),
}

/// One exact causal station observation statement and its five binds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CausalForecastObservationReadPlan {
    pub binds: [CausalTemperatureReadBind; 5],
    pub sql: &'static str,
}

/// One exact causal ECMWF prior-hour statement and its four binds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CausalEcmwfPriorHoursReadPlan {
    pub binds: [CausalTemperatureReadBind; 4],
    pub sql: &'static str,
}

/// One rejected causal input read window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CausalTemperatureReadPlanError {
    InvalidObservationWindow,
    InvalidPriorHourWindow,
    ObservationWindowNotHourly,
}

impl Display for CausalTemperatureReadPlanError {
    /// Render the matching source window failure category.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidObservationWindow => {
                "forecast training range must be increasing and at most 450 days"
            }
            Self::InvalidPriorHourWindow => "ECMWF causal prior-hour interval is invalid",
            Self::ObservationWindowNotHourly => {
                "causal observation projection bounds must align to UTC hours"
            }
        })
    }
}

impl Error for CausalTemperatureReadPlanError {}

/// One decoded prior-hour row after PostgreSQL alias and scalar conversion.
#[derive(Debug, Clone, PartialEq)]
pub struct CausalEcmwfPriorHourDbRow {
    pub content_hash: String,
    pub model_lead_hours: u64,
    pub raw_relative_humidity_percent: Nullable<f64>,
    pub raw_temperature_c: f64,
    pub raw_wind_speed_mps: Nullable<f64>,
    pub run_initialized_at: String,
    pub valid_at: String,
}

/// The temperature-only station payload consumed by the pure reducer adapter.
#[derive(Debug, Clone, PartialEq)]
pub struct CausalTemperatureStationReadRow {
    pub physical_station_key: ForecastObservationStationKey,
    pub temperature_c: Nullable<f64>,
    pub valid_at: String,
}

/// Build the complete causal physical-station read plan.
pub fn causal_forecast_observation_read_plan(
    query: &AdmittedCausalForecastObservationQuery,
) -> Result<CausalForecastObservationReadPlan, CausalTemperatureReadPlanError> {
    let Some(duration) = query
        .to_epoch_milliseconds
        .checked_sub(query.from_epoch_milliseconds)
    else {
        return Err(CausalTemperatureReadPlanError::InvalidObservationWindow);
    };
    // preserve the source's ordered bounded training interval
    if duration <= 0 || duration > MAX_FORECAST_TRAINING_MILLISECONDS {
        return Err(CausalTemperatureReadPlanError::InvalidObservationWindow);
    }
    // require the exact hourly projection boundary admitted by the time owner
    if query.from_epoch_milliseconds.rem_euclid(HOUR_MILLISECONDS) != 0
        || query.to_epoch_milliseconds.rem_euclid(HOUR_MILLISECONDS) != 0
    {
        return Err(CausalTemperatureReadPlanError::ObservationWindowNotHourly);
    }

    Ok(CausalForecastObservationReadPlan {
        binds: [
            CausalTemperatureReadBind::Text(query.site_slug.clone()),
            CausalTemperatureReadBind::UtcInstant(query.from.clone()),
            CausalTemperatureReadBind::UtcInstant(query.to.clone()),
            CausalTemperatureReadBind::UtcInstant(query.as_of.clone()),
            CausalTemperatureReadBind::TextArray(
                CAUSAL_FORECAST_OBSERVATION_SOURCE_KEYS
                    .iter()
                    .map(|key| (*key).to_owned())
                    .collect(),
            ),
        ],
        sql: CAUSAL_FORECAST_OBSERVATION_HOURLY_STATIONS_SQL,
    })
}

/// Build the complete causal prior ECMWF hour read plan.
pub fn causal_ecmwf_prior_hours_read_plan(
    query: &AdmittedCausalEcmwfPriorHoursQuery,
) -> Result<CausalEcmwfPriorHoursReadPlan, CausalTemperatureReadPlanError> {
    let Some(duration) = query
        .to_inclusive_epoch_milliseconds
        .checked_sub(query.from_epoch_milliseconds)
    else {
        return Err(CausalTemperatureReadPlanError::InvalidPriorHourWindow);
    };
    // preserve the source's inclusive ordered maximum 72-slot interval
    if !(0..=71 * HOUR_MILLISECONDS).contains(&duration) {
        return Err(CausalTemperatureReadPlanError::InvalidPriorHourWindow);
    }

    Ok(CausalEcmwfPriorHoursReadPlan {
        binds: [
            CausalTemperatureReadBind::Text(query.site_slug.clone()),
            CausalTemperatureReadBind::UtcInstant(query.from.clone()),
            CausalTemperatureReadBind::UtcInstant(query.to_inclusive.clone()),
            CausalTemperatureReadBind::UtcInstant(query.target_run_initialized_at.clone()),
        ],
        sql: CAUSAL_ECMWF_TEMPERATURE_PRIOR_HOURS_SQL,
    })
}

/// Project decoded ECMWF rows in database order and derive their source keys.
pub fn project_causal_ecmwf_prior_hours(
    rows: Vec<CausalEcmwfPriorHourDbRow>,
) -> Vec<EcmwfTemperatureCanaryPriorHour> {
    rows.into_iter()
        .map(|row| EcmwfTemperatureCanaryPriorHour {
            contentHash: row.content_hash,
            modelLeadHours: row.model_lead_hours,
            rawRelativeHumidityPercent: row.raw_relative_humidity_percent,
            rawTemperatureC: row.raw_temperature_c,
            rawWindSpeedMps: row.raw_wind_speed_mps,
            validAt: row.valid_at.clone(),
            key: format!("{}/{}", row.run_initialized_at, row.valid_at),
            runInitializedAt: row.run_initialized_at,
        })
        .collect()
}

/// Retain only the nullable station temperature fields required by the reducer.
pub fn project_causal_temperature_station_rows(
    rows: Vec<ForecastObservationHourlyStationRow>,
) -> Vec<CausalTemperatureStationReadRow> {
    rows.into_iter()
        .map(|row| CausalTemperatureStationReadRow {
            physical_station_key: row.physicalStationKey,
            temperature_c: row.metrics.temperatureC,
            valid_at: row.validAt,
        })
        .collect()
}
