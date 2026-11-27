use weather_contracts::{
    ApiHistoryResponse, ApiHistoryResponsePage, ApiSite, ApiWeatherRecord,
    ApiWeatherRecordMetadata, ApiWeatherRecordMetadataDevice, ApiWeatherRecordMetadataUpstream,
    ApiWeatherRecordMetrics, ApiWeatherRecordProvenance, ApiWeatherRecordProvenanceAttribution,
    FieldPresence, HistoryQuery, HistoryQueryCursor, Nullable, SourceKind, WeatherRecordRow,
};
use weather_database::{
    AdmittedHistoryQuery, HistoryReadPlan, HistoryReadPlanError, weather_history_read_plan,
};

use crate::current_projection::{
    CanonicalInstant, CurrentProjectionError, CurrentTimeResolver, describe_freshness,
    index_sources, map_nullable_instant, project_provider, project_quality, source_kind_label,
};
use crate::{ErrorDispatch, QueryParameter, parse_current_selection};

const HISTORY_DEFAULT_LIMIT: u64 = 100;
const HISTORY_MAX_LIMIT: u64 = 250;

/// one query timestamp resolved by the admitted time boundary
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoryQueryTime {
    From,
    To,
}

/// one decoded opaque cursor after exact-field and time validation
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdmittedHistoryCursor {
    pub id: String,
    pub valid_at: CanonicalInstant,
}

/// one rejected injected history boundary value
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HistoryBoundaryError;

/// admitted UTC and opaque cursor boundary without local codecs
pub trait HistoryBoundaryAdapter {
    /// resolve one submitted query instant to canonical UTC
    fn resolve_query_time(
        &self,
        value: &str,
        field: HistoryQueryTime,
    ) -> Result<CanonicalInstant, HistoryBoundaryError>;

    /// decode and validate one opaque history cursor
    fn decode_cursor(&self, value: &str) -> Result<AdmittedHistoryCursor, HistoryBoundaryError>;

    /// encode one canonical continuation cursor
    fn encode_cursor(&self, cursor: &HistoryQueryCursor) -> Result<String, CurrentProjectionError>;
}

/// one validated public history query and its visible page size
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedHistoryQuery {
    pub query: HistoryQuery,
    pub requested_limit: u64,
    from_epoch_milliseconds: Option<i64>,
    to_epoch_milliseconds: Option<i64>,
}

/// bind one parsed public query to the exact database read plan
pub fn history_read_plan(
    parsed: &ParsedHistoryQuery,
) -> Result<HistoryReadPlan, HistoryReadPlanError> {
    weather_history_read_plan(&AdmittedHistoryQuery::admitted(
        parsed.query.clone(),
        parsed.from_epoch_milliseconds,
        parsed.to_epoch_milliseconds,
    ))
}

/// parse the frozen history filters against active site discovery
pub fn parse_history_query(
    parameters: &[QueryParameter<'_>],
    site: &ApiSite,
    boundary: &dyn HistoryBoundaryAdapter,
) -> Result<ParsedHistoryQuery, ErrorDispatch> {
    // reject the first unsupported history filter
    for parameter in parameters {
        // preserve the source query-name failure
        if !matches!(
            parameter.name,
            "cursor" | "from" | "limit" | "source" | "sourceKind" | "station" | "to"
        ) {
            return Err(ErrorDispatch {
                status: 400,
                code: "invalid_query",
                message: format!("Unsupported query parameter: {}", parameter.name),
                allow_get_head: false,
            });
        }
    }

    let requested_limit = parse_history_limit(parameters)?;
    let cursor_value = optional_history_value(parameters, "cursor")?;
    let from_value = optional_history_value(parameters, "from")?;
    let source_kind_value = optional_history_value(parameters, "sourceKind")?;
    let source_value = optional_history_value(parameters, "source")?;
    let station_value = optional_history_value(parameters, "station")?;
    let to_value = optional_history_value(parameters, "to")?;
    let mut selection_parameters = Vec::with_capacity(2);

    // preserve Node's source-syntax check before station-syntax validation
    if source_value.is_some_and(|value| !crate::is_positive_decimal(value)) {
        return Err(ErrorDispatch {
            status: 400,
            code: "invalid_query",
            message: "source must be a positive integer".to_owned(),
            allow_get_head: false,
        });
    }

    // validate station syntax only after a submitted source is syntactically valid
    if station_value.is_some_and(|value| !crate::is_stable_key(value)) {
        return Err(invalid_query());
    }

    // validate the station through the shared active-selection policy
    if let Some(station) = station_value {
        selection_parameters.push(QueryParameter {
            name: "station",
            value: station,
        });
    }

    // validate the source through the shared active-selection policy
    if let Some(source) = source_value {
        selection_parameters.push(QueryParameter {
            name: "source",
            value: source,
        });
    }

    let selection = parse_current_selection(&selection_parameters, site)?;

    // preserve absent cursors and admit submitted opaque cursors
    let cursor = match cursor_value {
        None => FieldPresence::Missing,
        Some(cursor) => {
            let cursor = boundary.decode_cursor(cursor).map_err(|_| ErrorDispatch {
                status: 400,
                code: "invalid_query",
                message: "cursor is invalid".to_owned(),
                allow_get_head: false,
            })?;
            FieldPresence::Value(HistoryQueryCursor {
                id: cursor.id,
                validAt: cursor.valid_at.utc,
            })
        }
    };
    let from = resolve_optional_query_time(from_value, HistoryQueryTime::From, boundary)?;

    // retain the lower bound's admitted instant for repository validation
    let from_epoch_milliseconds = match &from {
        FieldPresence::Missing => None,
        FieldPresence::Value(from) => Some(from.epoch_milliseconds),
    };

    // preserve absent provenance filters and generated wire values
    let source_kind = match source_kind_value {
        None => FieldPresence::Missing,
        Some(value) => FieldPresence::Value(parse_source_kind(value)?),
    };
    let to = resolve_optional_query_time(to_value, HistoryQueryTime::To, boundary)?;

    // retain the upper bound's admitted instant for repository validation
    let to_epoch_milliseconds = match &to {
        FieldPresence::Missing => None,
        FieldPresence::Value(to) => Some(to.epoch_milliseconds),
    };

    // reject reversed ranges using admitted epoch values
    if let (FieldPresence::Value(from), FieldPresence::Value(to)) = (&from, &to)
        && from.epoch_milliseconds >= to.epoch_milliseconds
    {
        return Err(ErrorDispatch {
            status: 400,
            code: "invalid_query",
            message: "from must be earlier than to".to_owned(),
            allow_get_head: false,
        });
    }

    Ok(ParsedHistoryQuery {
        query: HistoryQuery {
            cursor,
            from: map_query_time(from),
            limit: FieldPresence::Value(requested_limit + 1),
            siteSlug: site.slug.clone(),
            sourceId: field_presence(selection.source_id),
            sourceKind: source_kind,
            stationSlug: field_presence(selection.station_slug),
            to: map_query_time(to),
        },
        requested_limit,
        from_epoch_milliseconds,
        to_epoch_milliseconds,
    })
}

/// assemble one canonical ordered history page
pub fn project_history_response(
    mut rows: Vec<WeatherRecordRow>,
    site: ApiSite,
    data_at: CanonicalInstant,
    requested_limit: u64,
    time_resolver: &dyn CurrentTimeResolver,
    boundary: &dyn HistoryBoundaryAdapter,
) -> Result<ApiHistoryResponse, CurrentProjectionError> {
    // reject page sizes outside the public parser contract
    if !(1..=HISTORY_MAX_LIMIT).contains(&requested_limit) {
        return Err(CurrentProjectionError {
            code: "history_projection.limit",
            detail: "history page limit is outside the public range".to_owned(),
        });
    }

    let requested_limit = requested_limit as usize;
    let has_more = rows.len() > requested_limit;

    // remove the one lookahead row without changing repository order
    if has_more {
        rows.truncate(requested_limit);
    }

    let sources = index_sources(&site);
    let mut data = Vec::with_capacity(rows.len());

    // project every visible history row in repository order
    for row in rows {
        let source = sources
            .get(row.sourceId.as_str())
            .ok_or_else(|| CurrentProjectionError {
                code: "history_projection.source",
                detail: format!("Missing active metadata for source {}", row.sourceId),
            })?;
        let valid_at = time_resolver.resolve(&row.validAt)?;
        let received_at = time_resolver.resolve(&row.lastReceivedAt)?.utc;
        let product_run_at = map_nullable_instant(&row.productRunAt, time_resolver)?;
        let metadata = project_history_metadata(&row, time_resolver)?;
        let metrics = project_history_metrics(&row);
        let provenance = ApiWeatherRecordProvenance {
            attribution: ApiWeatherRecordProvenanceAttribution {
                label: source.attribution.label.clone(),
                url: source.attribution.url.clone(),
            },
            label: source_kind_label(row.sourceKind).to_owned(),
            providerKey: source.providerKey.clone(),
            sourceId: row.sourceId,
            sourceKey: source.key.clone(),
            sourceKind: row.sourceKind,
            stationSlug: row.stationSlug,
        };

        data.push(ApiWeatherRecord {
            freshness: describe_freshness(valid_at.epoch_milliseconds, data_at.epoch_milliseconds),
            id: row.id,
            metadata,
            metrics,
            productRunAt: product_run_at,
            provenance,
            receivedAt: received_at,
            revisionCount: row.revisionCount,
            validAt: valid_at.utc,
        });
    }

    // emit a continuation only for a page with a hidden lookahead row
    let next_cursor = match (has_more, data.last()) {
        (true, Some(last)) => Nullable::Value(boundary.encode_cursor(&HistoryQueryCursor {
            id: last.id.clone(),
            validAt: last.validAt.clone(),
        })?),
        _ => Nullable::Null,
    };

    Ok(ApiHistoryResponse {
        data,
        generatedAt: data_at.utc,
        page: ApiHistoryResponsePage {
            limit: requested_limit as u64,
            nextCursor: next_cursor,
        },
        site,
    })
}

/// parse the public page-size contract
fn parse_history_limit(parameters: &[QueryParameter<'_>]) -> Result<u64, ErrorDispatch> {
    // use the product default for absent and empty limits
    let Some(value) = optional_history_value(parameters, "limit")? else {
        return Ok(HISTORY_DEFAULT_LIMIT);
    };

    // require only decimal digits like the source parser
    if !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(ErrorDispatch {
            status: 400,
            code: "invalid_query",
            message: "limit must be an integer".to_owned(),
            allow_get_head: false,
        });
    }

    let significant = value.trim_start_matches('0');

    // match JavaScript numeric conversion without overflowing Rust integers
    let limit = match significant {
        "" => 0,
        value => value.parse::<u64>().unwrap_or(u64::MAX),
    };

    // preserve the frozen public limit range
    if !(1..=HISTORY_MAX_LIMIT).contains(&limit) {
        return Err(ErrorDispatch {
            status: 400,
            code: "invalid_query",
            message: format!("limit must be between 1 and {HISTORY_MAX_LIMIT}"),
            allow_get_head: false,
        });
    }

    Ok(limit)
}

/// read one optional history field with duplicate and empty semantics
fn optional_history_value<'a>(
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

/// resolve one optional submitted time through the admitted boundary
fn resolve_optional_query_time(
    value: Option<&str>,
    field: HistoryQueryTime,
    boundary: &dyn HistoryBoundaryAdapter,
) -> Result<FieldPresence<CanonicalInstant>, ErrorDispatch> {
    // preserve an absent query bound
    match value {
        None => Ok(FieldPresence::Missing),
        Some(value) => boundary
            .resolve_query_time(value, field)
            .map(FieldPresence::Value)
            .map_err(|_| invalid_query()),
    }
}

/// discard admitted epoch evidence after range validation
fn map_query_time(value: FieldPresence<CanonicalInstant>) -> FieldPresence<String> {
    // preserve absent versus submitted query bounds
    match value {
        FieldPresence::Missing => FieldPresence::Missing,
        FieldPresence::Value(value) => FieldPresence::Value(value.utc),
    }
}

/// map one optional owned field to generated presence storage
fn field_presence<Value>(value: Option<Value>) -> FieldPresence<Value> {
    // preserve absent versus selected fields
    match value {
        None => FieldPresence::Missing,
        Some(value) => FieldPresence::Value(value),
    }
}

/// parse one exact generated source kind
fn parse_source_kind(value: &str) -> Result<SourceKind, ErrorDispatch> {
    // select only generated wire values
    SourceKind::WIRE_VALUES
        .iter()
        .find_map(|(wire, source_kind)| (*wire == value).then_some(*source_kind))
        .ok_or_else(invalid_query)
}

/// project public metadata shared by current and history records
fn project_history_metadata(
    row: &WeatherRecordRow,
    time_resolver: &dyn CurrentTimeResolver,
) -> Result<ApiWeatherRecordMetadata, CurrentProjectionError> {
    // omit device metadata only when every component is null
    let device = match (&row.deviceVendor, &row.deviceModel, &row.deviceSerial) {
        (Nullable::Null, Nullable::Null, Nullable::Null) => Nullable::Null,
        _ => Nullable::Value(ApiWeatherRecordMetadataDevice {
            model: row.deviceModel.clone(),
            serial: row.deviceSerial.clone(),
            vendor: row.deviceVendor.clone(),
        }),
    };

    Ok(ApiWeatherRecordMetadata {
        device,
        provider: project_provider(&row.providerMetadata, None, time_resolver)?,
        quality: project_quality(&row.qualityMetadata)?,
        upstream: ApiWeatherRecordMetadataUpstream {
            model: row.upstreamModel.clone(),
            timezone: row.upstreamTimezone.clone(),
        },
    })
}

/// copy the canonical public weather metrics
fn project_history_metrics(row: &WeatherRecordRow) -> ApiWeatherRecordMetrics {
    ApiWeatherRecordMetrics {
        apparentTemperatureC: row.apparentTemperatureC.clone(),
        blackGlobeTemperatureC: row.blackGlobeTemperatureC.clone(),
        cloudCoverPercent: row.cloudCoverPercent.clone(),
        pm25MicrogramsPerCubicMeter: row.pm25MicrogramsPerCubicMeter.clone(),
        precipitationMm: row.precipitationMm.clone(),
        precipitationRateMmPerHour: row.precipitationRateMmPerHour.clone(),
        pressureHpa: row.pressureHpa.clone(),
        relativeHumidityPercent: row.relativeHumidityPercent.clone(),
        soilElectricalConductivityMicrosiemensPerCm: row
            .soilElectricalConductivityMicrosiemensPerCm
            .clone(),
        soilMoisturePercent: row.soilMoisturePercent.clone(),
        solarRadiationWm2: row.solarRadiationWm2.clone(),
        temperatureC: row.temperatureC.clone(),
        uvIndex: row.uvIndex.clone(),
        windDirectionDegrees: row.windDirectionDegrees.clone(),
        windGustMps: row.windGustMps.clone(),
        windSpeedMps: row.windSpeedMps.clone(),
        wetBulbGlobeTemperatureC: row.wetBulbGlobeTemperatureC.clone(),
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
