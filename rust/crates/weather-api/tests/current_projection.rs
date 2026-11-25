use weather_api::{
    CanonicalInstant, CurrentProjectionError, CurrentTimeResolver, group_active_sites,
    project_current_weather_response,
};
use weather_contracts::{
    ApiWeatherRecordFreshnessStatusEnum, CurrentWeatherRecordRow, FieldPresence, JsDateOrString,
    JsonValue, Nullable, SourceKind, StationKind,
};
use weather_database::ActiveSiteRow;

/// explicit source-to-canonical time fixture
struct FixtureTimeResolver {
    values: Vec<(JsDateOrString, CanonicalInstant)>,
}

impl CurrentTimeResolver for FixtureTimeResolver {
    /// resolve one admitted fixture timestamp
    fn resolve(&self, value: &JsDateOrString) -> Result<CanonicalInstant, CurrentProjectionError> {
        self.values
            .iter()
            .find_map(|(source, canonical)| (source == value).then(|| canonical.clone()))
            .ok_or_else(|| CurrentProjectionError {
                code: "current_projection.time",
                detail: "fixture time adapter rejected the source timestamp".to_owned(),
            })
    }
}

/// encode one generated string
fn text(value: &str) -> Vec<u16> {
    value.encode_utf16().collect()
}

/// build one ordered object entry
fn entry(key: &str, value: JsonValue) -> (Vec<u16>, JsonValue) {
    (text(key), value)
}

/// build one admitted canonical instant
fn instant(epoch_milliseconds: i64, utc: &str) -> CanonicalInstant {
    CanonicalInstant {
        epoch_milliseconds,
        utc: utc.to_owned(),
    }
}

/// build node-derived source timestamp pairs
fn time_resolver() -> FixtureTimeResolver {
    FixtureTimeResolver {
        values: vec![
            (
                JsDateOrString::String(text("2026-08-21T22:00:00-07:00")),
                instant(1_787_374_800_000, "2026-08-22T05:00:00.000Z"),
            ),
            (
                JsDateOrString::String(text("2026-08-21T22:01:00-07:00")),
                instant(1_787_374_860_000, "2026-08-22T05:01:00.000Z"),
            ),
            (
                JsDateOrString::DateEpochMilliseconds(1_787_371_200_000),
                instant(1_787_371_200_000, "2026-08-22T04:00:00.000Z"),
            ),
            (
                JsDateOrString::String(text("2026-08-21T21:00:00-07:00")),
                instant(1_787_371_200_000, "2026-08-22T04:00:00.000Z"),
            ),
        ],
    }
}

/// build the admitted node request clock
fn request_time() -> CanonicalInstant {
    instant(1_787_377_020_000, "2026-08-22T05:37:00.000Z")
}

/// build one active source row
fn active_source(station_slug: &str, source_id: &str, source_kind: SourceKind) -> ActiveSiteRow {
    ActiveSiteRow {
        attribution_label: format!("Attribution {source_id}"),
        attribution_url: format!("https://provider.test/{source_id}"),
        latitude: 47.95,
        longitude: -122.42,
        provider_key: format!("provider-{source_id}"),
        provider_name: format!("Provider {source_id}"),
        site_name: "Ballydidean".to_owned(),
        site_slug: "ballydidean".to_owned(),
        source_id: source_id.to_owned(),
        source_key: format!("source-{source_id}"),
        source_kind,
        station_kind: StationKind::Virtual,
        station_latitude: 47.96,
        station_longitude: -122.43,
        station_name: station_slug.to_owned(),
        station_slug: station_slug.to_owned(),
        timezone: "America/Los_Angeles".to_owned(),
    }
}

/// build one representative current row
fn current_row(source_id: &str, station_slug: &str) -> CurrentWeatherRecordRow {
    let hourly = (0..12)
        .map(|index| {
            // preserve one explicit missing particulate hour
            if index == 4 {
                Nullable::Null
            } else {
                Nullable::Value(index as f64 + 0.5)
            }
        })
        .collect::<Vec<_>>();
    let sensor = JsonValue::Object(vec![
        entry("key", JsonValue::String(text("pm25-yard"))),
        entry("model", JsonValue::String(text("WH45"))),
        entry(
            "readings",
            JsonValue::Object(vec![
                entry("pm25MicrogramsPerCubicMeter", JsonValue::Number(8.25)),
                entry("bad-key", JsonValue::Number(99.0)),
                entry("temperatureC", JsonValue::Number(f64::NAN)),
            ]),
        ),
    ]);
    let malformed_sensor = JsonValue::Object(vec![
        entry("key", JsonValue::String(text("Invalid Key"))),
        entry("model", JsonValue::String(text("ignored"))),
    ]);

    CurrentWeatherRecordRow {
        adapterVersion: Nullable::Value("adapter-v1".to_owned()),
        apparentTemperatureC: Nullable::Value(12.5),
        blackGlobeTemperatureC: Nullable::Null,
        cloudCoverPercent: Nullable::Value(82.0),
        contentHash: "sha256-content".to_owned(),
        contractEpoch: Nullable::Value("2026-08".to_owned()),
        deviceModel: Nullable::Value("WS90".to_owned()),
        deviceSerial: Nullable::Null,
        deviceVendor: Nullable::Value("EcoWitt".to_owned()),
        firstReceivedAt: JsDateOrString::DateEpochMilliseconds(1_787_374_800_000),
        id: format!("record-{source_id}"),
        lastReceivedAt: JsDateOrString::String(text("2026-08-21T22:01:00-07:00")),
        pm25MicrogramsPerCubicMeter: Nullable::Value(8.25),
        precipitationMm: Nullable::Value(0.25),
        precipitationRateMmPerHour: Nullable::Null,
        pressureHpa: Nullable::Value(1018.25),
        productRunAt: Nullable::Value(JsDateOrString::DateEpochMilliseconds(1_787_371_200_000)),
        providerKey: format!("provider-{source_id}"),
        providerMetadata: Nullable::Value(vec![
            entry("dataset", JsonValue::String(text("observations"))),
            entry("elevation_m", JsonValue::Number(42.5)),
            entry("grid_cell", JsonValue::String(text("grid-7"))),
            entry(
                "property_sensors",
                JsonValue::Array(vec![sensor, malformed_sensor]),
            ),
            entry("private_token", JsonValue::String(text("not public"))),
        ]),
        qualityMetadata: Nullable::Value(vec![
            entry("confidence_percent", JsonValue::Number(98.5)),
            entry(
                "flags",
                JsonValue::Array(vec![
                    JsonValue::String(text("checked")),
                    JsonValue::Number(7.0),
                ]),
            ),
            entry("interpolation", JsonValue::String(text("none"))),
            entry("status", JsonValue::String(text("ok"))),
        ]),
        relativeHumidityPercent: Nullable::Value(78.0),
        revisionCount: 2,
        siteSlug: "ballydidean".to_owned(),
        sourceId: source_id.to_owned(),
        sourceConfigFingerprint: Nullable::Value("fingerprint".to_owned()),
        sourceKey: format!("ignored-row-key-{source_id}"),
        sourceKind: SourceKind::PhysicalSensor,
        stationSlug: station_slug.to_owned(),
        soilElectricalConductivityMicrosiemensPerCm: Nullable::Null,
        soilMoisturePercent: Nullable::Value(31.0),
        solarRadiationWm2: Nullable::Value(150.0),
        temperatureC: Nullable::Value(13.0),
        upstreamModel: Nullable::Null,
        upstreamTimezone: "America/Los_Angeles".to_owned(),
        uvIndex: Nullable::Value(2.0),
        validAt: JsDateOrString::String(text("2026-08-21T22:00:00-07:00")),
        waterLevelM: Nullable::Null,
        windDirectionDegrees: Nullable::Value(180.0),
        windGustMps: Nullable::Value(4.5),
        windSpeedMps: Nullable::Value(2.0),
        wetBulbGlobeTemperatureC: Nullable::Null,
        pm25HourlyBySensor: Nullable::Value(vec![(text("pm25-yard"), hourly.clone())]),
        pm25HourlyConcentrations: Nullable::Value(hourly),
        pm25HourlyThrough: JsDateOrString::String(text("2026-08-21T21:00:00-07:00")),
        pressureChange3hHpa: Nullable::Value(-1.25),
    }
}

#[test]
// project ordered current rows into the canonical response without coercing nulls
fn projects_canonical_current_response() {
    let time_resolver = time_resolver();
    let site = group_active_sites(vec![
        active_source("yard", "10", SourceKind::PhysicalSensor),
        active_source("virtual", "12", SourceKind::ModelCurrent),
    ])
    .data
    .remove(0);
    let mut second = current_row("12", "virtual");
    second.id = "record-second".to_owned();
    second.sourceKind = SourceKind::ModelCurrent;
    second.deviceModel = Nullable::Null;
    second.deviceVendor = Nullable::Null;
    second.providerMetadata = Nullable::Null;
    second.qualityMetadata = Nullable::Null;
    second.pm25HourlyConcentrations = Nullable::Null;
    let response = project_current_weather_response(
        vec![current_row("10", "yard"), second],
        site,
        request_time(),
        &time_resolver,
    )
    .expect("representative rows must project");

    assert_eq!(response.generatedAt, "2026-08-22T05:37:00.000Z");
    assert_eq!(response.data[0].id, "record-10");
    assert_eq!(response.data[1].id, "record-second");
    let record = &response.data[0];
    assert_eq!(record.validAt, "2026-08-22T05:00:00.000Z");
    assert_eq!(record.receivedAt, "2026-08-22T05:01:00.000Z");
    assert_eq!(
        record.productRunAt,
        Nullable::Value("2026-08-22T04:00:00.000Z".to_owned()),
    );
    assert_eq!(record.freshness.ageSeconds, 2_220);
    assert_eq!(
        record.freshness.status,
        ApiWeatherRecordFreshnessStatusEnum::Delayed,
    );
    assert_eq!(record.metrics.temperatureC, Nullable::Value(13.0));
    assert_eq!(record.metrics.blackGlobeTemperatureC, Nullable::Null);
    assert_eq!(record.pressureChange3hHpa, Nullable::Value(-1.25));
    assert_eq!(record.provenance.sourceKey, "source-10");
    assert_eq!(record.provenance.providerKey, "provider-10");
    assert_eq!(record.provenance.attribution.label, "Attribution 10");
    assert_eq!(record.provenance.label, "physical sensor measurement");
    let Nullable::Value(air_quality) = &record.airQuality else {
        panic!("complete particulate context must be public");
    };
    assert_eq!(air_quality.hourlyPm25.len(), 12);
    assert_eq!(air_quality.hourlyPm25[4], Nullable::Null);
    assert_eq!(air_quality.through, "2026-08-22T04:00:00.000Z");
    let Nullable::Value(provider) = &record.metadata.provider else {
        panic!("provider metadata must be public");
    };
    assert_eq!(provider.dataset, Nullable::Value("observations".to_owned()));
    assert_eq!(provider.elevationM, Nullable::Value(42.5));
    let Nullable::Value(sensors) = &provider.propertySensors else {
        panic!("sensor metadata must be public");
    };
    assert_eq!(sensors.len(), 1);
    assert_eq!(sensors[0].key, "pm25-yard");
    assert_eq!(sensors[0].channel, Nullable::Null);
    assert_eq!(
        sensors[0].readings.0,
        vec![("pm25MicrogramsPerCubicMeter".to_owned(), 8.25)],
    );
    assert!(matches!(
        sensors[0].airQuality,
        FieldPresence::Value(Nullable::Value(_)),
    ));
    assert_eq!(response.data[1].metadata.device, Nullable::Null);
    assert_eq!(response.data[1].metadata.provider, Nullable::Null);
    assert_eq!(response.data[1].metadata.quality, Nullable::Null);
    assert_eq!(response.data[1].airQuality, Nullable::Null);
}

#[test]
// fail closed when a repository row lacks matching active source metadata
fn rejects_missing_source_metadata() {
    let time_resolver = time_resolver();
    let site = group_active_sites(vec![active_source(
        "yard",
        "10",
        SourceKind::PhysicalSensor,
    )])
    .data
    .remove(0);
    let error = project_current_weather_response(
        vec![current_row("99", "missing")],
        site,
        request_time(),
        &time_resolver,
    )
    .expect_err("unknown source must fail");

    assert_eq!(error.code, "current_projection.source");
    assert_eq!(error.detail, "Missing active metadata for source 99");
}

#[test]
// reject corrupt timestamps rather than emitting noncanonical public values
fn rejects_invalid_repository_timestamp() {
    let time_resolver = time_resolver();
    let site = group_active_sites(vec![active_source(
        "yard",
        "10",
        SourceKind::PhysicalSensor,
    )])
    .data
    .remove(0);
    let mut row = current_row("10", "yard");
    row.validAt = JsDateOrString::InvalidDate;
    let error = project_current_weather_response(vec![row], site, request_time(), &time_resolver)
        .expect_err("invalid time must fail");

    assert_eq!(error.code, "current_projection.time");
}
