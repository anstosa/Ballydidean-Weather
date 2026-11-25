//! Concrete precipitation, trend, tide, collection, and observation response values.

use weather_api::{
    ForecastObservationInputResponse, ForecastObservationInputResponseContractVersionLiteral,
    ForecastObservationInputResponseMetrics, ForecastObservationInputResponseProvenance,
    ForecastObservationInputResponseRecord, ForecastObservationInputResponseSite,
};
use weather_contracts::{
    ApiDailyPrecipitation, ApiDailyPrecipitationResponse, ApiRainCollectionResponse,
    ApiRainCollectionResponseData, ApiRainCollectionResponseDataContractVersionLiteral,
    ApiRainCollectionResponseDataQualificationEnabledLiteral, ApiTideRecord,
    ApiTideRecordEventTypeEnum, ApiTideRecordKindEnum, ApiTideRecordSource,
    ApiTideRecordSourceAttribution, ApiTidesResponse, ApiTrendPoint, ApiTrendPointMetrics,
    ApiTrendsResponse, ForecastObservationInputRowSourceKindEnum, JsonValue,
};

use crate::api_service_json_values::{
    array, boolean, enum_text, nullable, number, object, text, unsigned,
};
use crate::api_service_weather_values::site_value;

/// Project one daily precipitation response in the original route member order.
pub(crate) fn precipitation_value(response: &ApiDailyPrecipitationResponse) -> JsonValue {
    object([
        ("data", nullable(&response.data, daily_precipitation_value)),
        ("generatedAt", text(&response.generatedAt)),
        ("site", site_value(&response.site)),
    ])
}

/// Project one trend response in the original route member order.
pub(crate) fn trends_value(response: &ApiTrendsResponse) -> JsonValue {
    object([
        ("data", array(&response.data, trend_point_value)),
        ("generatedAt", text(&response.generatedAt)),
        ("site", site_value(&response.site)),
    ])
}

/// Project one tide response in the original route member order.
pub(crate) fn tides_value(response: &ApiTidesResponse) -> JsonValue {
    object([
        ("data", array(&response.data, tide_record_value)),
        ("generatedAt", text(&response.generatedAt)),
        ("site", site_value(&response.site)),
    ])
}

/// Project one rain collection response in the original route member order.
pub(crate) fn rain_collection_value(response: &ApiRainCollectionResponse) -> JsonValue {
    object([
        ("data", rain_collection_data_value(&response.data)),
        ("generatedAt", text(&response.generatedAt)),
        ("version", text(&response.version)),
    ])
}

/// Project one private forecast observation response in the original route member order.
pub(crate) fn observation_value(response: &ForecastObservationInputResponse) -> JsonValue {
    object([
        (
            "contractVersion",
            text(ForecastObservationInputResponseContractVersionLiteral::WIRE_VALUE),
        ),
        ("generatedAt", text(&response.generatedAt)),
        ("projectionFrom", text(&response.projectionFrom)),
        ("queryFrom", text(&response.queryFrom)),
        (
            "records",
            array(&response.records, observation_record_value),
        ),
        ("site", observation_site_value(&response.site)),
        ("through", text(&response.through)),
    ])
}

/// Project one present daily precipitation value and its nested source.
fn daily_precipitation_value(value: &ApiDailyPrecipitation) -> JsonValue {
    object([
        ("accumulationMm", number(value.accumulationMm)),
        (
            "source",
            object([
                ("sourceId", text(&value.source.sourceId)),
                ("stationSlug", text(&value.source.stationSlug)),
            ]),
        ),
        ("validThrough", text(&value.validThrough)),
    ])
}

/// Project one public trend bucket in its source object order.
fn trend_point_value(value: &ApiTrendPoint) -> JsonValue {
    object([
        ("metrics", trend_metrics_value(&value.metrics)),
        ("validAt", text(&value.validAt)),
    ])
}

/// Project all nullable trend metrics without changing explicit nulls.
fn trend_metrics_value(metrics: &ApiTrendPointMetrics) -> JsonValue {
    object([
        (
            "apparentTemperatureC",
            nullable(&metrics.apparentTemperatureC, number_value),
        ),
        (
            "pm25MicrogramsPerCubicMeter",
            nullable(&metrics.pm25MicrogramsPerCubicMeter, number_value),
        ),
        (
            "precipitationMm",
            nullable(&metrics.precipitationMm, number_value),
        ),
        ("pressureHpa", nullable(&metrics.pressureHpa, number_value)),
        (
            "relativeHumidityPercent",
            nullable(&metrics.relativeHumidityPercent, number_value),
        ),
        (
            "temperatureC",
            nullable(&metrics.temperatureC, number_value),
        ),
        (
            "temperatureMaximumC",
            nullable(&metrics.temperatureMaximumC, number_value),
        ),
        (
            "temperatureMinimumC",
            nullable(&metrics.temperatureMinimumC, number_value),
        ),
        (
            "windDirectionDegrees",
            nullable(&metrics.windDirectionDegrees, number_value),
        ),
        ("windGustMps", nullable(&metrics.windGustMps, number_value)),
        (
            "windSpeedMps",
            nullable(&metrics.windSpeedMps, number_value),
        ),
    ])
}

/// Project one tide record with the original nullable event spelling.
fn tide_record_value(value: &ApiTideRecord) -> JsonValue {
    object([
        (
            "eventType",
            nullable(&value.eventType, tide_event_type_value),
        ),
        (
            "kind",
            enum_text(value.kind, ApiTideRecordKindEnum::WIRE_VALUES),
        ),
        ("source", tide_source_value(&value.source)),
        ("validAt", text(&value.validAt)),
        ("waterLevelM", number(value.waterLevelM)),
    ])
}

/// Select the generated event type spelling for one present tide event.
fn tide_event_type_value(value: &ApiTideRecordEventTypeEnum) -> JsonValue {
    enum_text(*value, ApiTideRecordEventTypeEnum::WIRE_VALUES)
}

/// Project one tide source in the original nested object order.
fn tide_source_value(source: &ApiTideRecordSource) -> JsonValue {
    object([
        ("attribution", tide_attribution_value(&source.attribution)),
        ("providerKey", text(&source.providerKey)),
        ("stationName", text(&source.stationName)),
        ("stationSlug", text(&source.stationSlug)),
    ])
}

/// Project one tide attribution without normalizing its source strings.
fn tide_attribution_value(attribution: &ApiTideRecordSourceAttribution) -> JsonValue {
    object([
        ("label", text(&attribution.label)),
        ("url", text(&attribution.url)),
    ])
}

/// Project bounded rain collection aggregates in the original spread order.
fn rain_collection_data_value(data: &ApiRainCollectionResponseData) -> JsonValue {
    object([
        (
            "contractVersion",
            text(ApiRainCollectionResponseDataContractVersionLiteral::WIRE_VALUE),
        ),
        ("modelEnabled", boolean(data.modelEnabled)),
        (
            "qualificationEnabled",
            boolean(ApiRainCollectionResponseDataQualificationEnabledLiteral::WIRE_VALUE),
        ),
        (
            "stationAccessAuthorized",
            boolean(data.stationAccessAuthorized),
        ),
        ("expectedStations", unsigned(data.expectedStations)),
        ("startsAt", text(&data.startsAt)),
        ("expiresAt", text(&data.expiresAt)),
        ("claims", unsigned(data.claims)),
        ("receipts", unsigned(data.receipts)),
        ("validForecasts", unsigned(data.validForecasts)),
        ("timelyForecasts", unsigned(data.timelyForecasts)),
        ("validStationWindows", unsigned(data.validStationWindows)),
        ("stationsSeen", unsigned(data.stationsSeen)),
        ("failedRequests", unsigned(data.failedRequests)),
        ("pendingRequests", unsigned(data.pendingRequests)),
        ("unknownRequests", unsigned(data.unknownRequests)),
        ("compressedBytes", unsigned(data.compressedBytes)),
        (
            "lastClaimAt",
            nullable(&data.lastClaimAt, |value| text(value)),
        ),
        (
            "lastReceiptAt",
            nullable(&data.lastReceiptAt, |value| text(value)),
        ),
        (
            "lastForecastReceiptAt",
            nullable(&data.lastForecastReceiptAt, |value| text(value)),
        ),
        (
            "lastStationReceiptAt",
            nullable(&data.lastStationReceiptAt, |value| text(value)),
        ),
        (
            "pausedUntil",
            nullable(&data.pausedUntil, |value| text(value)),
        ),
    ])
}

/// Project one compact private observation row in producer order.
fn observation_record_value(record: &ForecastObservationInputResponseRecord) -> JsonValue {
    object([
        ("id", text(&record.id)),
        ("metrics", observation_metrics_value(&record.metrics)),
        (
            "provenance",
            observation_provenance_value(&record.provenance),
        ),
        ("receivedAt", text(&record.receivedAt)),
        ("validAt", text(&record.validAt)),
    ])
}

/// Project all compact private observation metrics with explicit nulls.
fn observation_metrics_value(metrics: &ForecastObservationInputResponseMetrics) -> JsonValue {
    object([
        (
            "apparentTemperatureC",
            nullable(&metrics.apparentTemperatureC, number_value),
        ),
        (
            "cloudCoverPercent",
            nullable(&metrics.cloudCoverPercent, number_value),
        ),
        (
            "pm25MicrogramsPerCubicMeter",
            nullable(&metrics.pm25MicrogramsPerCubicMeter, number_value),
        ),
        (
            "precipitationRateMmPerHour",
            nullable(&metrics.precipitationRateMmPerHour, number_value),
        ),
        ("pressureHpa", nullable(&metrics.pressureHpa, number_value)),
        (
            "relativeHumidityPercent",
            nullable(&metrics.relativeHumidityPercent, number_value),
        ),
        (
            "solarRadiationWm2",
            nullable(&metrics.solarRadiationWm2, number_value),
        ),
        (
            "temperatureC",
            nullable(&metrics.temperatureC, number_value),
        ),
        ("uvIndex", nullable(&metrics.uvIndex, number_value)),
        (
            "windDirectionDegrees",
            nullable(&metrics.windDirectionDegrees, number_value),
        ),
        ("windGustMps", nullable(&metrics.windGustMps, number_value)),
        (
            "windSpeedMps",
            nullable(&metrics.windSpeedMps, number_value),
        ),
    ])
}

/// Project compact observation provenance using the generated source kind spelling.
fn observation_provenance_value(
    provenance: &ForecastObservationInputResponseProvenance,
) -> JsonValue {
    object([
        ("providerKey", text(&provenance.providerKey)),
        ("sourceId", text(&provenance.sourceId)),
        ("sourceKey", text(&provenance.sourceKey)),
        (
            "sourceKind",
            enum_text(
                provenance.sourceKind,
                ForecastObservationInputRowSourceKindEnum::WIRE_VALUES,
            ),
        ),
        ("stationSlug", text(&provenance.stationSlug)),
    ])
}

/// Project the fixed observation site object in its original member order.
fn observation_site_value(site: &ForecastObservationInputResponseSite) -> JsonValue {
    object([
        ("slug", text(&site.slug)),
        ("timezone", text(&site.timezone)),
    ])
}

/// Adapt one borrowed finite metric for nullable value projection.
fn number_value(value: &f64) -> JsonValue {
    number(*value)
}
