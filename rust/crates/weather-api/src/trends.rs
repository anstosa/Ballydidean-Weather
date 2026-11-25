use weather_contracts::{
    ApiSite, ApiTrendPoint, ApiTrendPointMetrics, ApiTrendsResponse, JsDate, Nullable,
    TrendPointRow, TrendQuery,
};
use weather_database::{AdmittedTrendQuery, TrendReadPlan, TrendReadPlanError, trend_read_plan};

use crate::current_projection::{CanonicalInstant, CurrentProjectionError};
use crate::{ErrorDispatch, QueryParameter};

/// one accepted compatibility-only trend range
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LegacyTrendRange {
    TwentyFourHours,
    SevenDays,
    ThirtyDays,
}

/// admitted calendar and repository-date boundary for trends
pub trait TrendTimeBoundary {
    /// resolve the fixed 2019 local-calendar start in one site timezone
    fn calendar_start(
        &self,
        generated_at: &CanonicalInstant,
        timezone: &str,
    ) -> Result<CanonicalInstant, CurrentProjectionError>;

    /// resolve one decoded database date to canonical UTC
    fn resolve_row_time(&self, value: &JsDate) -> Result<CanonicalInstant, CurrentProjectionError>;
}

/// one calendar trend query with admitted epoch evidence
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedTrendQuery {
    pub query: TrendQuery,
    from_epoch_milliseconds: i64,
    to_epoch_milliseconds: i64,
}

/// parse the compatibility-only trend range selector
pub fn parse_legacy_trend_range(
    parameters: &[QueryParameter<'_>],
) -> Result<Option<LegacyTrendRange>, ErrorDispatch> {
    // reject the first unsupported trend filter
    for parameter in parameters {
        // preserve the route's range-only query surface
        if parameter.name != "range" {
            return Err(ErrorDispatch {
                status: 400,
                code: "invalid_query",
                message: format!("Unsupported query parameter: {}", parameter.name),
                allow_get_head: false,
            });
        }
    }

    let value = optional_range_value(parameters)?;

    // preserve the three accepted cached-client values without changing the window
    match value {
        None => Ok(None),
        Some("24h") => Ok(Some(LegacyTrendRange::TwentyFourHours)),
        Some("7d") => Ok(Some(LegacyTrendRange::SevenDays)),
        Some("30d") => Ok(Some(LegacyTrendRange::ThirtyDays)),
        Some(_) => Err(invalid_query()),
    }
}

/// build the fixed local-calendar trend query through the admitted time boundary
pub fn calendar_trend_query(
    site: &ApiSite,
    generated_at: &CanonicalInstant,
    boundary: &dyn TrendTimeBoundary,
) -> Result<ParsedTrendQuery, CurrentProjectionError> {
    let from = boundary.calendar_start(generated_at, &site.timezone)?;

    // require the injected calendar boundary to precede the response instant
    if from.epoch_milliseconds >= generated_at.epoch_milliseconds {
        return Err(CurrentProjectionError {
            code: "trends.window",
            detail: "trend from must be earlier than to".to_owned(),
        });
    }

    Ok(ParsedTrendQuery {
        query: TrendQuery {
            from: from.utc,
            siteSlug: site.slug.clone(),
            to: generated_at.utc.clone(),
        },
        from_epoch_milliseconds: from.epoch_milliseconds,
        to_epoch_milliseconds: generated_at.epoch_milliseconds,
    })
}

/// bind one parsed trend query to the exact database read plan
pub fn trends_read_plan(parsed: &ParsedTrendQuery) -> Result<TrendReadPlan, TrendReadPlanError> {
    trend_read_plan(&AdmittedTrendQuery::admitted(
        parsed.query.clone(),
        parsed.from_epoch_milliseconds,
        parsed.to_epoch_milliseconds,
    ))
}

/// project ordered daily trend rows to the canonical API response
pub fn project_trends_response(
    rows: Vec<TrendPointRow>,
    site: ApiSite,
    generated_at: &CanonicalInstant,
    boundary: &dyn TrendTimeBoundary,
) -> Result<ApiTrendsResponse, CurrentProjectionError> {
    let mut data = Vec::with_capacity(rows.len());

    // preserve ascending repository bucket order
    for row in rows {
        data.push(ApiTrendPoint {
            metrics: ApiTrendPointMetrics {
                apparentTemperatureC: canonical_metric(row.apparentTemperatureC),
                pm25MicrogramsPerCubicMeter: canonical_metric(row.pm25MicrogramsPerCubicMeter),
                precipitationMm: canonical_metric(row.precipitationMm),
                pressureHpa: canonical_metric(row.pressureHpa),
                relativeHumidityPercent: canonical_metric(row.relativeHumidityPercent),
                temperatureC: canonical_metric(row.temperatureC),
                temperatureMaximumC: canonical_metric(row.temperatureMaximumC),
                temperatureMinimumC: canonical_metric(row.temperatureMinimumC),
                windDirectionDegrees: canonical_metric(row.windDirectionDegrees),
                windGustMps: canonical_metric(row.windGustMps),
                windSpeedMps: canonical_metric(row.windSpeedMps),
            },
            validAt: boundary.resolve_row_time(&row.validAt)?.utc,
        });
    }

    Ok(ApiTrendsResponse {
        data,
        generatedAt: generated_at.utc.clone(),
        site,
    })
}

/// read one optional range value with duplicate and empty semantics
fn optional_range_value<'a>(
    parameters: &'a [QueryParameter<'a>],
) -> Result<Option<&'a str>, ErrorDispatch> {
    let mut matches = parameters
        .iter()
        .filter(|parameter| parameter.name == "range");
    let first = matches.next();

    // reject the second submitted range
    if matches.next().is_some() {
        return Err(ErrorDispatch {
            status: 400,
            code: "invalid_query",
            message: "range may be provided once".to_owned(),
            allow_get_head: false,
        });
    }

    Ok(first.and_then(|parameter| (!parameter.value.is_empty()).then_some(parameter.value)))
}

/// normalize non-finite storage numbers to the public JSON null value
fn canonical_metric(value: Nullable<f64>) -> Nullable<f64> {
    // retain only finite public metric values
    match value {
        Nullable::Value(value) if value.is_finite() => Nullable::Value(value),
        _ => Nullable::Null,
    }
}

/// build the shared public query validation response
fn invalid_query() -> ErrorDispatch {
    ErrorDispatch {
        status: 400,
        code: "invalid_query",
        message: "Request query is invalid".to_owned(),
        allow_get_head: false,
    }
}
