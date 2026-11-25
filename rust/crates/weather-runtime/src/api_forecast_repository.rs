//! Preserve authoritative forecast rows while admitting optional runtime provenance.

use weather_contracts::WeatherRecordRow;
use weather_database::{
    EcmwfTemperatureCanaryReadPort, ForecastReadPlan, decorate_forecast_provenance,
    forecast_provenance_plan,
};

use crate::api_repository::WeatherServicePostgresReadPort;

/// Execute the raw forecast read and decorate only valid optional provenance.
pub fn read_api_forecast<Database>(
    database: &mut Database,
    plan: &ForecastReadPlan,
) -> Result<Vec<WeatherRecordRow>, <Database as EcmwfTemperatureCanaryReadPort>::Error>
where
    Database: WeatherServicePostgresReadPort,
{
    let raw_rows = database.query_forecast(plan)?;
    let Some(provenance_plan) = forecast_provenance_plan(&raw_rows) else {
        // skip the optional statement when the authoritative query returned no rows
        return Ok(raw_rows);
    };
    let provenance_rows = match database.query_forecast_provenance(&provenance_plan) {
        Ok(rows) => rows,
        // preserve authoritative raw rows when optional provenance is unavailable
        Err(_) => return Ok(raw_rows),
    };

    Ok(decorate_forecast_provenance(raw_rows, provenance_rows))
}
