use weather_contracts::{
    ApiSite, ApiTideRecord, ApiTideRecordEventTypeEnum, ApiTideRecordKindEnum, ApiTideRecordSource,
    ApiTideRecordSourceAttribution, ApiTidesResponse, FieldPresence, Nullable, TideQuery,
    TideRecordRow, TideRecordRowSourceKindEnum,
};
use weather_database::{AdmittedTideQuery, TideReadPlan, TideReadPlanError, tide_read_plan};

use crate::current_projection::{CanonicalInstant, CurrentProjectionError, CurrentTimeResolver};
use crate::{ErrorDispatch, QueryParameter};

const DEFAULT_TIDE_FROM_OFFSET_MILLISECONDS: i64 = -24 * 3_600_000;
const DEFAULT_TIDE_TO_OFFSET_MILLISECONDS: i64 = 10 * 86_400_000;
const MAXIMUM_TIDE_RANGE_MILLISECONDS: i64 = 31 * 86_400_000;

/// one tide time resolved by the admitted time boundary
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TideQueryTime {
    From,
    To,
}

/// one rejected injected tide time value
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TideBoundaryError;

/// admitted UTC query and default-offset boundary
pub trait TideBoundaryAdapter {
    /// resolve one submitted tide bound to canonical UTC
    fn resolve_query_time(
        &self,
        value: &str,
        field: TideQueryTime,
    ) -> Result<CanonicalInstant, TideBoundaryError>;

    /// resolve one exact default offset without a local time codec
    fn offset_data_time(
        &self,
        data_at: &CanonicalInstant,
        offset_milliseconds: i64,
        field: TideQueryTime,
    ) -> Result<CanonicalInstant, TideBoundaryError>;
}

/// one parsed tide query with admitted epoch evidence
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedTideQuery {
    pub query: TideQuery,
    from_epoch_milliseconds: i64,
    to_epoch_milliseconds: i64,
}

/// parse one bounded tide range with source-owned defaults
pub fn parse_tide_query(
    parameters: &[QueryParameter<'_>],
    site: &ApiSite,
    data_at: &CanonicalInstant,
    boundary: &dyn TideBoundaryAdapter,
) -> Result<ParsedTideQuery, ErrorDispatch> {
    // reject the first unsupported tide filter
    for parameter in parameters {
        // preserve the source query-name failure
        if !matches!(parameter.name, "from" | "to") {
            return Err(ErrorDispatch {
                status: 400,
                code: "invalid_query",
                message: format!("Unsupported query parameter: {}", parameter.name),
                allow_get_head: false,
            });
        }
    }

    let from_value = optional_tide_value(parameters, "from")?;
    let to_value = optional_tide_value(parameters, "to")?;

    // resolve the submitted lower bound or exact one-day default
    let from = match from_value {
        Some(value) => boundary.resolve_query_time(value, TideQueryTime::From),
        None => boundary.offset_data_time(
            data_at,
            DEFAULT_TIDE_FROM_OFFSET_MILLISECONDS,
            TideQueryTime::From,
        ),
    }
    .map_err(|_| invalid_query())?;

    // resolve the submitted upper bound or exact ten-day default
    let to = match to_value {
        Some(value) => boundary.resolve_query_time(value, TideQueryTime::To),
        None => boundary.offset_data_time(
            data_at,
            DEFAULT_TIDE_TO_OFFSET_MILLISECONDS,
            TideQueryTime::To,
        ),
    }
    .map_err(|_| invalid_query())?;
    let duration = to.epoch_milliseconds.checked_sub(from.epoch_milliseconds);

    // require one ordered range no longer than thirty-one days
    if !matches!(duration, Some(duration) if duration > 0 && duration <= MAXIMUM_TIDE_RANGE_MILLISECONDS)
    {
        return Err(invalid_query());
    }

    Ok(ParsedTideQuery {
        query: TideQuery {
            from: from.utc,
            limit: FieldPresence::Missing,
            siteSlug: site.slug.clone(),
            to: to.utc,
        },
        from_epoch_milliseconds: from.epoch_milliseconds,
        to_epoch_milliseconds: to.epoch_milliseconds,
    })
}

/// bind one parsed tide query to the exact database read plan
pub fn tides_read_plan(parsed: &ParsedTideQuery) -> Result<TideReadPlan, TideReadPlanError> {
    tide_read_plan(&AdmittedTideQuery::admitted(
        parsed.query.clone(),
        parsed.from_epoch_milliseconds,
        parsed.to_epoch_milliseconds,
    ))
}

/// project ordered tide rows to the canonical API response
pub fn project_tides_response(
    rows: Vec<TideRecordRow>,
    site: ApiSite,
    data_at: &CanonicalInstant,
    time_resolver: &dyn CurrentTimeResolver,
) -> Result<ApiTidesResponse, CurrentProjectionError> {
    let mut data = Vec::with_capacity(rows.len());

    // preserve ascending repository order
    for row in rows {
        // map only the source-owned high and low event markers
        let event_type = match &row.predictionType {
            Nullable::Value(value) if value == "H" => {
                Nullable::Value(ApiTideRecordEventTypeEnum::High)
            }
            Nullable::Value(value) if value == "L" => {
                Nullable::Value(ApiTideRecordEventTypeEnum::Low)
            }
            _ => Nullable::Null,
        };

        // distinguish observed from predicted water levels
        let kind = match row.sourceKind {
            TideRecordRowSourceKindEnum::TideObservation => ApiTideRecordKindEnum::Observation,
            TideRecordRowSourceKindEnum::TidePrediction => ApiTideRecordKindEnum::Prediction,
        };

        data.push(ApiTideRecord {
            eventType: event_type,
            kind,
            source: ApiTideRecordSource {
                attribution: ApiTideRecordSourceAttribution {
                    label: row.attributionLabel,
                    url: row.attributionUrl,
                },
                providerKey: row.providerKey,
                stationName: row.stationName,
                stationSlug: row.stationSlug,
            },
            validAt: time_resolver.resolve(&row.validAt)?.utc,
            waterLevelM: row.waterLevelM,
        });
    }

    Ok(ApiTidesResponse {
        data,
        generatedAt: data_at.utc.clone(),
        site,
    })
}

/// read one optional tide field with duplicate and empty semantics
fn optional_tide_value<'a>(
    parameters: &'a [QueryParameter<'a>],
    name: &str,
) -> Result<Option<&'a str>, ErrorDispatch> {
    let mut matches = parameters.iter().filter(|parameter| parameter.name == name);
    let first = matches.next();

    // reject the second matching parameter
    if matches.next().is_some() {
        return Err(ErrorDispatch {
            status: 400,
            code: "invalid_query",
            message: format!("{name} may be provided once"),
            allow_get_head: false,
        });
    }

    Ok(first.and_then(|parameter| (!parameter.value.is_empty()).then_some(parameter.value)))
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
