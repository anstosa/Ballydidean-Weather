use std::error::Error;
use std::fmt::{Display, Formatter};

use weather_contracts::{
    ApiSite, FieldPresence, ForecastPressureContextQuery, ForecastProductSelection, ForecastQuery,
};
use weather_database::{
    AdmittedForecastPressureContextQuery, AdmittedForecastQuery, ForecastPressureContextPlan,
    ForecastPressureContextPlanError, ForecastReadPlan, ForecastReadPlanError,
    forecast_pressure_context_plan, forecast_read_plan,
};

use crate::{CanonicalInstant, ForecastSelection};

/// one repository read window resolved by the admitted site-calendar boundary
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedForecastReadWindow {
    pub as_of: CanonicalInstant,
    pub end_exclusive: CanonicalInstant,
    pub hours: u64,
}

/// one pressure-context window resolved by the admitted UTC offset boundary
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedForecastPressureContextWindow {
    pub as_of: CanonicalInstant,
    pub context_start: CanonicalInstant,
    pub context_end: CanonicalInstant,
    pub oldest_run: CanonicalInstant,
}

/// one rejected link between the endpoint selector and resolved read window
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForecastReadError {
    IncompatibleResolvedWindow,
    Repository(ForecastReadPlanError),
}

impl Display for ForecastReadError {
    /// render the bounded forecast read failure
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        // distinguish endpoint geometry from repository validation
        match self {
            Self::IncompatibleResolvedWindow => {
                formatter.write_str("resolved forecast window does not match the endpoint selector")
            }
            Self::Repository(error) => Display::fmt(error, formatter),
        }
    }
}

impl Error for ForecastReadError {}

/// bind one typed forecast endpoint selection to the exact database read plan
pub fn forecast_endpoint_read_plan(
    site: &ApiSite,
    selection: ForecastSelection,
    window: &ResolvedForecastReadWindow,
) -> Result<ForecastReadPlan, ForecastReadError> {
    let product_selection = match selection {
        ForecastSelection::Daily { days } => {
            let expected_hours = u64::from(days) * 24 + 13;

            // admit the civil-day duration plus twelve prior and one trailing AQ hour
            if window.hours.abs_diff(expected_hours) > 1 {
                return Err(ForecastReadError::IncompatibleResolvedWindow);
            }
            FieldPresence::Missing
        }
        ForecastSelection::Overnight => {
            // admit only the reviewed DST-aware midnight-through-07:00 durations
            if !(30..=32).contains(&window.hours) {
                return Err(ForecastReadError::IncompatibleResolvedWindow);
            }
            FieldPresence::Value(ForecastProductSelection::AnchorContaining)
        }
    };
    let query = ForecastQuery {
        asOf: window.as_of.utc.clone(),
        hours: window.hours,
        productSelection: product_selection,
        siteSlug: site.slug.clone(),
    };

    forecast_read_plan(&AdmittedForecastQuery::admitted(
        query,
        window.as_of.epoch_milliseconds,
        window.end_exclusive.utc.clone(),
        window.end_exclusive.epoch_milliseconds,
    ))
    .map_err(ForecastReadError::Repository)
}

/// bind one resolved pressure-context window to the exact database plan
pub fn forecast_pressure_context_read_plan(
    site: &ApiSite,
    window: &ResolvedForecastPressureContextWindow,
) -> Result<ForecastPressureContextPlan, ForecastPressureContextPlanError> {
    forecast_pressure_context_plan(&AdmittedForecastPressureContextQuery::admitted(
        ForecastPressureContextQuery {
            asOf: window.as_of.utc.clone(),
            siteSlug: site.slug.clone(),
        },
        window.as_of.epoch_milliseconds,
        window.context_start.utc.clone(),
        window.context_start.epoch_milliseconds,
        window.context_end.utc.clone(),
        window.context_end.epoch_milliseconds,
        window.oldest_run.utc.clone(),
        window.oldest_run.epoch_milliseconds,
    ))
}
