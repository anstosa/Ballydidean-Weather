use std::collections::BTreeMap;

use weather_contracts::ForecastObservationHourlyStationRow;
use weather_database::{
    AdmittedCausalEcmwfPriorHoursQuery, AdmittedCausalForecastObservationQuery,
    CausalEcmwfPriorHourDbRow, CausalEcmwfPriorHoursReadPlan, CausalForecastObservationReadPlan,
    causal_ecmwf_prior_hours_read_plan, causal_forecast_observation_read_plan,
    project_causal_ecmwf_prior_hours, project_causal_temperature_station_rows,
};
use weather_worker::{
    AdmittedTemperatureRecentErrorForecast, AdmittedTemperatureRecentErrorHour,
    AdmittedTemperatureRecentErrorInput, AdmittedTemperatureRecentErrorStation, TaskFailure,
    TemperatureCanaryRecentErrorBuilder, TemperatureCanaryRecentErrorState,
    build_admitted_temperature_recent_error_state,
};

const HOUR_MILLISECONDS: i64 = 3_600_000;
const TARGET_DELAY_HOURS: i64 = 7;
const WINDOW_HOURS: u8 = 72;

/// One prior-hour row with clocks parsed by the PostgreSQL runtime owner.
#[derive(Clone, Debug, PartialEq)]
pub struct AdmittedCausalEcmwfPriorHourReadRow {
    pub row: CausalEcmwfPriorHourDbRow,
    pub run_initialized_at_epoch_ms: i64,
    pub valid_at_epoch_ms: i64,
}

/// One station row with its valid-at clock parsed by the PostgreSQL runtime owner.
#[derive(Clone, Debug, PartialEq)]
pub struct AdmittedCausalTemperatureStationReadRow {
    pub row: ForecastObservationHourlyStationRow,
    pub valid_at_epoch_ms: i64,
}

/// Typed PostgreSQL execution boundary for both source-owned causal read plans.
pub trait CausalTemperatureDatabasePort {
    /// Execute the exact prior-hour plan and decode its typed rows.
    fn read_causal_ecmwf_prior_hours(
        &mut self,
        plan: &CausalEcmwfPriorHoursReadPlan,
    ) -> Result<Vec<AdmittedCausalEcmwfPriorHourReadRow>, TaskFailure>;

    /// Execute the exact physical-station plan and decode its typed rows.
    fn read_causal_temperature_station_hours(
        &mut self,
        plan: &CausalForecastObservationReadPlan,
    ) -> Result<Vec<AdmittedCausalTemperatureStationReadRow>, TaskFailure>;
}

/// Admitted UTC and local-calendar projection without a runtime date codec.
pub trait CausalTemperatureCalendarPort {
    /// Bind one parsed epoch millisecond to its canonical UTC string.
    fn utc_string(&mut self, epoch_ms: i64) -> Result<String, TaskFailure>;

    /// Resolve the source local date for one already-bound UTC instant.
    fn local_date(&mut self, valid_at: &str, epoch_ms: i64) -> Result<String, TaskFailure>;
}

/// Causal recent-error builder backed by injected PostgreSQL and calendar ports.
pub struct DatabaseTemperatureRecentErrorBuilder<Database, Calendar> {
    calendar: Calendar,
    database: Database,
}

impl<Database, Calendar> DatabaseTemperatureRecentErrorBuilder<Database, Calendar> {
    /// Bind typed database and calendar owners without performing reads.
    pub const fn new(database: Database, calendar: Calendar) -> Self {
        Self { calendar, database }
    }

    /// Inspect the database port after one collector execution.
    pub const fn database(&self) -> &Database {
        &self.database
    }
}

impl<Database, Calendar> TemperatureCanaryRecentErrorBuilder
    for DatabaseTemperatureRecentErrorBuilder<Database, Calendar>
where
    Calendar: CausalTemperatureCalendarPort,
    Database: CausalTemperatureDatabasePort,
{
    /// Execute both causal plans and reduce their matched 72-hour evidence.
    fn build_recent_error_state(
        &mut self,
        site_slug: &str,
        target_run_initialized_at: &str,
        target_run_initialized_at_ms: i64,
    ) -> Result<TemperatureCanaryRecentErrorState, TaskFailure> {
        build_causal_temperature_recent_error_state(
            &mut self.database,
            &mut self.calendar,
            site_slug,
            target_run_initialized_at,
            target_run_initialized_at_ms,
        )
    }
}

struct OwnedTemperatureRecentErrorSlot {
    age_hours: u8,
    forecast: Option<weather_contracts::EcmwfTemperatureCanaryPriorHour>,
    local_date: String,
    stations: Vec<AdmittedTemperatureRecentErrorStation>,
    valid_at: String,
    valid_at_epoch_ms: i64,
}

/// Build one canonical causal recent-error state through the source-owned plans and reducer.
pub fn build_causal_temperature_recent_error_state<Database, Calendar>(
    database: &mut Database,
    calendar: &mut Calendar,
    site_slug: &str,
    target_run_initialized_at: &str,
    target_run_initialized_at_ms: i64,
) -> Result<TemperatureCanaryRecentErrorState, TaskFailure>
where
    Calendar: CausalTemperatureCalendarPort,
    Database: CausalTemperatureDatabasePort,
{
    // require the same exact hourly canonical target admitted by the collector
    if site_slug.is_empty()
        || target_run_initialized_at_ms.rem_euclid(HOUR_MILLISECONDS) != 0
        || calendar.utc_string(target_run_initialized_at_ms)? != target_run_initialized_at
    {
        return Err(recent_error_failure(
            "temperature recent-error target differs",
        ));
    }
    let window_end_epoch_ms = target_run_initialized_at_ms
        .checked_sub(TARGET_DELAY_HOURS * HOUR_MILLISECONDS)
        .ok_or_else(|| recent_error_failure("temperature recent-error window overflowed"))?;
    let window_start_epoch_ms = window_end_epoch_ms
        .checked_sub((i64::from(WINDOW_HOURS) - 1) * HOUR_MILLISECONDS)
        .ok_or_else(|| recent_error_failure("temperature recent-error window overflowed"))?;
    let observation_end_epoch_ms = window_end_epoch_ms
        .checked_add(HOUR_MILLISECONDS)
        .ok_or_else(|| recent_error_failure("temperature recent-error window overflowed"))?;
    let window_start = calendar.utc_string(window_start_epoch_ms)?;
    let window_end = calendar.utc_string(window_end_epoch_ms)?;
    let observation_end = calendar.utc_string(observation_end_epoch_ms)?;
    let prior_plan =
        causal_ecmwf_prior_hours_read_plan(&AdmittedCausalEcmwfPriorHoursQuery::admitted(
            site_slug.to_owned(),
            window_start.clone(),
            window_start_epoch_ms,
            window_end.clone(),
            window_end_epoch_ms,
            target_run_initialized_at.to_owned(),
        ))
        .map_err(|_| recent_error_failure("temperature recent-error prior plan differs"))?;
    let station_plan =
        causal_forecast_observation_read_plan(&AdmittedCausalForecastObservationQuery::admitted(
            site_slug.to_owned(),
            window_start,
            window_start_epoch_ms,
            observation_end,
            observation_end_epoch_ms,
            target_run_initialized_at.to_owned(),
        ))
        .map_err(|_| recent_error_failure("temperature recent-error station plan differs"))?;
    let prior_rows = database.read_causal_ecmwf_prior_hours(&prior_plan)?;
    let station_rows = database.read_causal_temperature_station_hours(&station_plan)?;
    let mut forecasts = admitted_forecasts(
        prior_rows,
        calendar,
        window_start_epoch_ms,
        window_end_epoch_ms,
        target_run_initialized_at_ms,
    )?;
    let mut stations = admitted_stations(
        station_rows,
        calendar,
        window_start_epoch_ms,
        window_end_epoch_ms,
    )?;
    let mut slots = Vec::with_capacity(usize::from(WINDOW_HOURS));

    // materialize every fixed age slot before borrowing it for the pure reducer
    for age_hours in 0..WINDOW_HOURS {
        let valid_at_epoch_ms = window_end_epoch_ms
            .checked_sub(i64::from(age_hours) * HOUR_MILLISECONDS)
            .ok_or_else(|| recent_error_failure("temperature recent-error slot overflowed"))?;
        let valid_at = calendar.utc_string(valid_at_epoch_ms)?;
        let local_date = calendar.local_date(&valid_at, valid_at_epoch_ms)?;
        slots.push(OwnedTemperatureRecentErrorSlot {
            age_hours,
            forecast: forecasts.remove(&valid_at_epoch_ms),
            local_date,
            stations: stations.remove(&valid_at_epoch_ms).unwrap_or_default(),
            valid_at,
            valid_at_epoch_ms,
        });
    }
    // reject any decoded evidence that did not bind to one exact hourly slot
    if !forecasts.is_empty() || !stations.is_empty() {
        return Err(recent_error_failure(
            "temperature recent-error evidence range differs",
        ));
    }
    let admitted_hours = slots
        .iter()
        .map(|slot| AdmittedTemperatureRecentErrorHour {
            age_hours: slot.age_hours,
            forecast: slot.forecast.as_ref().map(|forecast| {
                AdmittedTemperatureRecentErrorForecast {
                    key: &forecast.key,
                    raw_temperature_c: forecast.rawTemperatureC,
                    run_initialized_at: &forecast.runInitializedAt,
                }
            }),
            local_date: &slot.local_date,
            stations: &slot.stations,
            valid_at: &slot.valid_at,
            valid_at_epoch_ms: slot.valid_at_epoch_ms,
        })
        .collect::<Vec<_>>();
    let state =
        build_admitted_temperature_recent_error_state(AdmittedTemperatureRecentErrorInput {
            hours: &admitted_hours,
            target_run_initialized_at,
            target_run_initialized_at_epoch_ms: target_run_initialized_at_ms,
            window_end_valid_at: &window_end,
            window_end_valid_at_epoch_ms: window_end_epoch_ms,
        })
        .map_err(|_| recent_error_failure("temperature recent-error reduction differs"))?;
    Ok(state.into())
}

// validate causal forecast clocks and retain the database projector's source keys
fn admitted_forecasts<Calendar: CausalTemperatureCalendarPort>(
    rows: Vec<AdmittedCausalEcmwfPriorHourReadRow>,
    calendar: &mut Calendar,
    window_start_epoch_ms: i64,
    window_end_epoch_ms: i64,
    target_run_initialized_at_ms: i64,
) -> Result<BTreeMap<i64, weather_contracts::EcmwfTemperatureCanaryPriorHour>, TaskFailure> {
    let mut previous_valid_at_ms = None;
    let mut epochs = Vec::with_capacity(rows.len());
    let mut raw_rows = Vec::with_capacity(rows.len());

    // preserve the SQL statement's strict valid-at order and causal run selection
    for admitted in rows {
        let expected_lead_ms = i64::try_from(admitted.row.model_lead_hours)
            .ok()
            .and_then(|lead| lead.checked_mul(HOUR_MILLISECONDS));
        let actual_lead_ms = admitted
            .valid_at_epoch_ms
            .checked_sub(admitted.run_initialized_at_epoch_ms);
        if admitted.row.valid_at != calendar.utc_string(admitted.valid_at_epoch_ms)?
            || admitted.row.run_initialized_at
                != calendar.utc_string(admitted.run_initialized_at_epoch_ms)?
            || admitted.valid_at_epoch_ms < window_start_epoch_ms
            || admitted.valid_at_epoch_ms > window_end_epoch_ms
            || admitted.valid_at_epoch_ms.rem_euclid(HOUR_MILLISECONDS) != 0
            || admitted.run_initialized_at_epoch_ms >= target_run_initialized_at_ms
            || !(7..=18).contains(&admitted.row.model_lead_hours)
            || expected_lead_ms != actual_lead_ms
            || previous_valid_at_ms.is_some_and(|previous| admitted.valid_at_epoch_ms <= previous)
        {
            return Err(recent_error_failure(
                "temperature recent-error forecast evidence differs",
            ));
        }
        previous_valid_at_ms = Some(admitted.valid_at_epoch_ms);
        epochs.push(admitted.valid_at_epoch_ms);
        raw_rows.push(admitted.row);
    }
    let projected = project_causal_ecmwf_prior_hours(raw_rows);
    Ok(epochs.into_iter().zip(projected).collect())
}

// validate station clocks and group the database projection by exact hourly slot
fn admitted_stations<Calendar: CausalTemperatureCalendarPort>(
    rows: Vec<AdmittedCausalTemperatureStationReadRow>,
    calendar: &mut Calendar,
    window_start_epoch_ms: i64,
    window_end_epoch_ms: i64,
) -> Result<BTreeMap<i64, Vec<AdmittedTemperatureRecentErrorStation>>, TaskFailure> {
    let mut previous_valid_at_ms = None;
    let mut epochs = Vec::with_capacity(rows.len());
    let mut raw_rows = Vec::with_capacity(rows.len());

    // preserve ascending database hours while allowing multiple physical stations per hour
    for admitted in rows {
        if admitted.row.validAt != calendar.utc_string(admitted.valid_at_epoch_ms)?
            || admitted.valid_at_epoch_ms < window_start_epoch_ms
            || admitted.valid_at_epoch_ms > window_end_epoch_ms
            || admitted.valid_at_epoch_ms.rem_euclid(HOUR_MILLISECONDS) != 0
            || previous_valid_at_ms.is_some_and(|previous| admitted.valid_at_epoch_ms < previous)
        {
            return Err(recent_error_failure(
                "temperature recent-error station evidence differs",
            ));
        }
        previous_valid_at_ms = Some(admitted.valid_at_epoch_ms);
        epochs.push(admitted.valid_at_epoch_ms);
        raw_rows.push(admitted.row);
    }
    let projected = project_causal_temperature_station_rows(raw_rows);
    let mut grouped: BTreeMap<i64, Vec<AdmittedTemperatureRecentErrorStation>> = BTreeMap::new();

    // retain projector order and reject duplicate physical identities within one hour
    for (epoch_ms, row) in epochs.into_iter().zip(projected) {
        let hour = grouped.entry(epoch_ms).or_default();
        if hour
            .iter()
            .any(|station| station.physical_station_key == row.physical_station_key)
        {
            return Err(recent_error_failure(
                "temperature recent-error station evidence differs",
            ));
        }
        hour.push(AdmittedTemperatureRecentErrorStation {
            physical_station_key: row.physical_station_key,
            temperature_c: row.temperature_c,
        });
    }
    Ok(grouped)
}

// build one stable recent-error adapter failure
fn recent_error_failure(code: &str) -> TaskFailure {
    TaskFailure {
        attempts: 0,
        code: code.to_owned(),
    }
}
