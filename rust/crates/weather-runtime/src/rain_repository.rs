use weather_api::{
    AdmittedRainAdjustmentHour, AdmittedRainAdjustmentRun, CanonicalInstant,
    ForecastRainAdjustmentRead,
};
use weather_contracts::{Nullable, RainAdjustmentRun, RainCollectionStatus};
use weather_database::{
    RainAdjustmentRunReadRequest, RainServingReadError, RainServingReadPort,
    read_rain_adjustment_run, read_rain_collection_status,
};

/// Existing canonical time owner for retained rain storage timestamps.
pub trait RainRepositoryTimeBoundary {
    type Error;

    /// Admit one canonical UTC string and its epoch without implementing a codec here.
    fn admit_rain_instant(&mut self, value: &str) -> Result<CanonicalInstant, Self::Error>;
}

/// Separate database and time-admission failures for the runtime repository seam.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RainRepositoryReadError<DatabaseError, TimeError> {
    Database(RainServingReadError<DatabaseError>),
    Time(TimeError),
}

/// Execute the exact database read and project one API-admitted retained run.
pub fn read_admitted_rain_adjustment_run<Database, Time>(
    database: &mut Database,
    time: &mut Time,
    read: ForecastRainAdjustmentRead,
) -> Result<Option<AdmittedRainAdjustmentRun>, RainRepositoryReadError<Database::Error, Time::Error>>
where
    Database: RainServingReadPort,
    Time: RainRepositoryTimeBoundary,
{
    let as_of = time
        .admit_rain_instant(&read.as_of)
        .map_err(RainRepositoryReadError::Time)?;
    let request = RainAdjustmentRunReadRequest {
        as_of: as_of.utc,
        as_of_epoch_milliseconds: Some(as_of.epoch_milliseconds),
        site_slug: read.site_slug,
    };
    let run =
        read_rain_adjustment_run(database, &request).map_err(RainRepositoryReadError::Database)?;
    run.map(|run| admit_rain_adjustment_run(run, time))
        .transpose()
        .map_err(RainRepositoryReadError::Time)
}

/// Execute the aggregate-only collection status read through its typed DB port.
pub fn read_admitted_rain_collection_status<Database: RainServingReadPort>(
    database: &mut Database,
) -> Result<RainCollectionStatus, RainRepositoryReadError<Database::Error, ()>> {
    read_rain_collection_status(database).map_err(RainRepositoryReadError::Database)
}

/// Convert one public-safe stored run into the API's canonical admitted geometry.
fn admit_rain_adjustment_run<Time: RainRepositoryTimeBoundary>(
    run: RainAdjustmentRun,
    time: &mut Time,
) -> Result<AdmittedRainAdjustmentRun, Time::Error> {
    let run_initialized_at = time.admit_rain_instant(&run.runInitializedAt)?;
    let first_received_at = time.admit_rain_instant(&run.firstReceivedAt)?;
    let decision_at = time.admit_rain_instant(&run.decisionAt)?;
    let generated_at = time.admit_rain_instant(&run.generatedAt)?;
    let mut hours = Vec::with_capacity(run.hours.len());

    // preserve database JSON array order while admitting every retained hour timestamp
    for hour in run.hours {
        hours.push(AdmittedRainAdjustmentHour {
            valid_at: time.admit_rain_instant(&hour.validAt)?,
            model_lead_hours: hour.modelLeadHours,
            raw_precipitation_mm: hour.rawPrecipitationMm,
            corrected_precipitation_mm: hour.correctedPrecipitationMm,
            applied: hour.applied,
            reason_code: nullable_string(hour.reasonCode),
        });
    }
    Ok(AdmittedRainAdjustmentRun {
        run_initialized_at,
        model_sha256: run.modelSha256,
        first_received_at,
        decision_at,
        generated_at,
        hours,
    })
}

/// Convert one generated nullable reason into the API's internal optional shape.
fn nullable_string(value: Nullable<String>) -> Option<String> {
    match value {
        Nullable::Null => None,
        Nullable::Value(value) => Some(value),
    }
}
