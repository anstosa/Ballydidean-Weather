//! exact site and weather response projections

use weather_contracts::{
    ApiAirQualityHourlyContext, ApiCurrentWeatherRecord, ApiCurrentWeatherResponse,
    ApiHistoryResponse, ApiPropertySensorSnapshot, ApiSite, ApiSitesResponse, ApiSource,
    ApiSourceAttribution, ApiStation, ApiWeatherRecord, ApiWeatherRecordFreshness,
    ApiWeatherRecordFreshnessStatusEnum, ApiWeatherRecordMetadata, ApiWeatherRecordMetadataDevice,
    ApiWeatherRecordMetadataProvider, ApiWeatherRecordMetadataQuality,
    ApiWeatherRecordMetadataUpstream, ApiWeatherRecordMetrics, ApiWeatherRecordProvenance,
    ApiWeatherRecordProvenanceAttribution, FieldPresence, JsonValue, Nullable,
    OrderedFiniteNumberRecord, SourceKind, StationKind,
};

use crate::api_service_json_values::{
    array, enum_text, member, nullable, number, object, text, unsigned,
};

/// preserves active site attribution order
fn source_attribution_value(value: &ApiSourceAttribution) -> JsonValue {
    object([("label", text(&value.label)), ("url", text(&value.url))])
}

/// preserves grouped source member order
fn source_value(value: &ApiSource) -> JsonValue {
    object([
        ("attribution", source_attribution_value(&value.attribution)),
        ("id", text(&value.id)),
        ("key", text(&value.key)),
        ("kind", enum_text(value.kind, SourceKind::WIRE_VALUES)),
        ("providerKey", text(&value.providerKey)),
        ("providerName", text(&value.providerName)),
        ("provenanceLabel", text(&value.provenanceLabel)),
    ])
}

/// preserves grouped station member order
fn station_value(value: &ApiStation) -> JsonValue {
    object([
        ("kind", enum_text(value.kind, StationKind::WIRE_VALUES)),
        ("latitude", number(value.latitude)),
        ("longitude", number(value.longitude)),
        ("name", text(&value.name)),
        ("slug", text(&value.slug)),
        ("sources", array(&value.sources, source_value)),
    ])
}

/// preserves site order and shape
pub(crate) fn site_value(value: &ApiSite) -> JsonValue {
    object([
        ("latitude", number(value.latitude)),
        ("longitude", number(value.longitude)),
        ("name", text(&value.name)),
        ("slug", text(&value.slug)),
        ("stations", array(&value.stations, station_value)),
        ("timezone", text(&value.timezone)),
    ])
}

/// preserves shared freshness shape
pub(crate) fn freshness_value(value: &ApiWeatherRecordFreshness) -> JsonValue {
    object([
        ("ageSeconds", unsigned(value.ageSeconds)),
        ("label", text(&value.label)),
        (
            "status",
            enum_text(
                value.status,
                ApiWeatherRecordFreshnessStatusEnum::WIRE_VALUES,
            ),
        ),
    ])
}

/// preserves hourly particulate null slots
fn air_quality_value(value: &ApiAirQualityHourlyContext) -> JsonValue {
    object([
        (
            "hourlyPm25",
            array(&value.hourlyPm25, |reading| {
                nullable(reading, |reading| number(*reading))
            }),
        ),
        ("through", text(&value.through)),
    ])
}

/// preserves numeric record order
fn ordered_number_record_value(value: &OrderedFiniteNumberRecord) -> JsonValue {
    JsonValue::Object(
        value
            .0
            .iter()
            .map(|(key, value)| member(key, number(*value)))
            .collect(),
    )
}

/// distinguishes absent and null air quality
fn property_sensor_value(value: &ApiPropertySensorSnapshot) -> JsonValue {
    let mut fields = Vec::with_capacity(5);
    // preserve conditional spread order
    if let FieldPresence::Value(air_quality) = &value.airQuality {
        fields.push(member(
            "airQuality",
            nullable(air_quality, air_quality_value),
        ));
    }
    fields.extend([
        member(
            "channel",
            nullable(&value.channel, |channel| unsigned(*channel)),
        ),
        member("key", text(&value.key)),
        member("model", text(&value.model)),
        member("readings", ordered_number_record_value(&value.readings)),
    ]);
    JsonValue::Object(fields)
}

/// preserves device metadata order
fn metadata_device_value(value: &ApiWeatherRecordMetadataDevice) -> JsonValue {
    object([
        ("model", nullable(&value.model, |model| text(model))),
        ("serial", nullable(&value.serial, |serial| text(serial))),
        ("vendor", nullable(&value.vendor, |vendor| text(vendor))),
    ])
}

/// preserves provider metadata order
fn metadata_provider_value(value: &ApiWeatherRecordMetadataProvider) -> JsonValue {
    object([
        ("dataset", nullable(&value.dataset, |dataset| text(dataset))),
        (
            "elevationM",
            nullable(&value.elevationM, |elevation| number(*elevation)),
        ),
        ("gridCell", nullable(&value.gridCell, |cell| text(cell))),
        (
            "propertySensors",
            nullable(&value.propertySensors, |sensors| {
                array(sensors, property_sensor_value)
            }),
        ),
    ])
}

/// preserves quality metadata order
fn metadata_quality_value(value: &ApiWeatherRecordMetadataQuality) -> JsonValue {
    object([
        (
            "confidencePercent",
            nullable(&value.confidencePercent, |confidence| number(*confidence)),
        ),
        (
            "flags",
            nullable(&value.flags, |flags| array(flags, |flag| text(flag))),
        ),
        (
            "interpolation",
            nullable(&value.interpolation, |interpolation| text(interpolation)),
        ),
        ("status", nullable(&value.status, |status| text(status))),
    ])
}

/// preserves upstream metadata order
fn metadata_upstream_value(value: &ApiWeatherRecordMetadataUpstream) -> JsonValue {
    object([
        ("model", nullable(&value.model, |model| text(model))),
        ("timezone", text(&value.timezone)),
    ])
}

/// preserves metadata nullability and order
pub(crate) fn metadata_value(value: &ApiWeatherRecordMetadata) -> JsonValue {
    object([
        ("device", nullable(&value.device, metadata_device_value)),
        (
            "provider",
            nullable(&value.provider, metadata_provider_value),
        ),
        ("quality", nullable(&value.quality, metadata_quality_value)),
        ("upstream", metadata_upstream_value(&value.upstream)),
    ])
}

/// preserves original weather metric order
pub(crate) fn metrics_value(value: &ApiWeatherRecordMetrics) -> JsonValue {
    object([
        (
            "apparentTemperatureC",
            nullable(&value.apparentTemperatureC, |metric| number(*metric)),
        ),
        (
            "blackGlobeTemperatureC",
            nullable(&value.blackGlobeTemperatureC, |metric| number(*metric)),
        ),
        (
            "cloudCoverPercent",
            nullable(&value.cloudCoverPercent, |metric| number(*metric)),
        ),
        (
            "pm25MicrogramsPerCubicMeter",
            nullable(&value.pm25MicrogramsPerCubicMeter, |metric| number(*metric)),
        ),
        (
            "precipitationMm",
            nullable(&value.precipitationMm, |metric| number(*metric)),
        ),
        (
            "precipitationRateMmPerHour",
            nullable(&value.precipitationRateMmPerHour, |metric| number(*metric)),
        ),
        (
            "pressureHpa",
            nullable(&value.pressureHpa, |metric| number(*metric)),
        ),
        (
            "relativeHumidityPercent",
            nullable(&value.relativeHumidityPercent, |metric| number(*metric)),
        ),
        (
            "soilElectricalConductivityMicrosiemensPerCm",
            nullable(
                &value.soilElectricalConductivityMicrosiemensPerCm,
                |metric| number(*metric),
            ),
        ),
        (
            "soilMoisturePercent",
            nullable(&value.soilMoisturePercent, |metric| number(*metric)),
        ),
        (
            "solarRadiationWm2",
            nullable(&value.solarRadiationWm2, |metric| number(*metric)),
        ),
        (
            "temperatureC",
            nullable(&value.temperatureC, |metric| number(*metric)),
        ),
        (
            "uvIndex",
            nullable(&value.uvIndex, |metric| number(*metric)),
        ),
        (
            "windDirectionDegrees",
            nullable(&value.windDirectionDegrees, |metric| number(*metric)),
        ),
        (
            "windGustMps",
            nullable(&value.windGustMps, |metric| number(*metric)),
        ),
        (
            "windSpeedMps",
            nullable(&value.windSpeedMps, |metric| number(*metric)),
        ),
        (
            "wetBulbGlobeTemperatureC",
            nullable(&value.wetBulbGlobeTemperatureC, |metric| number(*metric)),
        ),
    ])
}

/// preserves provenance attribution order
fn provenance_attribution_value(value: &ApiWeatherRecordProvenanceAttribution) -> JsonValue {
    object([("label", text(&value.label)), ("url", text(&value.url))])
}

/// preserves original provenance order
pub(crate) fn provenance_value(value: &ApiWeatherRecordProvenance) -> JsonValue {
    object([
        (
            "attribution",
            provenance_attribution_value(&value.attribution),
        ),
        ("label", text(&value.label)),
        ("providerKey", text(&value.providerKey)),
        ("sourceId", text(&value.sourceId)),
        ("sourceKey", text(&value.sourceKey)),
        (
            "sourceKind",
            enum_text(value.sourceKind, SourceKind::WIRE_VALUES),
        ),
        ("stationSlug", text(&value.stationSlug)),
    ])
}

/// builds shared base record members
#[allow(clippy::too_many_arguments)]
pub(crate) fn weather_record_members(
    freshness: &ApiWeatherRecordFreshness,
    id: &str,
    metadata: &ApiWeatherRecordMetadata,
    metrics: &ApiWeatherRecordMetrics,
    product_run_at: &Nullable<String>,
    provenance: &ApiWeatherRecordProvenance,
    received_at: &str,
    revision_count: u64,
    valid_at: &str,
) -> Vec<(Vec<u16>, JsonValue)> {
    vec![
        member("freshness", freshness_value(freshness)),
        member("id", text(id)),
        member("metadata", metadata_value(metadata)),
        member("metrics", metrics_value(metrics)),
        member(
            "productRunAt",
            nullable(product_run_at, |instant| text(instant)),
        ),
        member("provenance", provenance_value(provenance)),
        member("receivedAt", text(received_at)),
        member("revisionCount", unsigned(revision_count)),
        member("validAt", text(valid_at)),
    ]
}

/// preserves base weather record order
pub(crate) fn weather_record_value(value: &ApiWeatherRecord) -> JsonValue {
    JsonValue::Object(weather_record_members(
        &value.freshness,
        &value.id,
        &value.metadata,
        &value.metrics,
        &value.productRunAt,
        &value.provenance,
        &value.receivedAt,
        value.revisionCount,
        &value.validAt,
    ))
}

/// preserves current weather record order
fn current_weather_record_value(value: &ApiCurrentWeatherRecord) -> JsonValue {
    let mut fields = weather_record_members(
        &value.freshness,
        &value.id,
        &value.metadata,
        &value.metrics,
        &value.productRunAt,
        &value.provenance,
        &value.receivedAt,
        value.revisionCount,
        &value.validAt,
    );
    fields.extend([
        member("airQuality", nullable(&value.airQuality, air_quality_value)),
        member(
            "pressureChange3hHpa",
            nullable(&value.pressureChange3hHpa, |pressure| number(*pressure)),
        ),
    ]);
    JsonValue::Object(fields)
}

/// preserves sites response order
pub(crate) fn sites_value(value: &ApiSitesResponse) -> JsonValue {
    object([("data", array(&value.data, site_value))])
}

/// preserves current response order
pub(crate) fn current_value(value: &ApiCurrentWeatherResponse) -> JsonValue {
    object([
        ("data", array(&value.data, current_weather_record_value)),
        ("generatedAt", text(&value.generatedAt)),
        ("site", site_value(&value.site)),
    ])
}

/// preserves history response order
pub(crate) fn history_value(value: &ApiHistoryResponse) -> JsonValue {
    object([
        ("data", array(&value.data, weather_record_value)),
        ("generatedAt", text(&value.generatedAt)),
        (
            "page",
            object([
                ("limit", unsigned(value.page.limit)),
                (
                    "nextCursor",
                    nullable(&value.page.nextCursor, |cursor| text(cursor)),
                ),
            ]),
        ),
        ("site", site_value(&value.site)),
    ])
}
