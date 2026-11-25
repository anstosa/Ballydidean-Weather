//! Byte-exact service response encoding against the compiled predecessor handler.

use super::{FixtureRoot, NodePrimitives};
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use weather_api::{
    CurrentTimeResolver, ForecastObservationInputResponse,
    ForecastObservationInputResponseContractVersionLiteral,
    ForecastObservationInputResponseMetrics, ForecastObservationInputResponseProvenance,
    ForecastObservationInputResponseRecord, ForecastObservationInputResponseSite,
    InactiveForecastDecisionConfig, WeatherServiceBody, WeatherServiceWireEncoder,
    group_active_sites, project_forecast_weather_records,
};
use weather_contracts::{self as c, FieldPresence, JsonValue, Nullable};
use weather_database::ActiveSiteRow;
use weather_runtime::NativeWeatherServiceWireEncoder;

const DATA_AT: &str = "2026-08-22T05:00:00.000Z";
const VALID_AT: &str = "2026-08-22T06:00:00.000Z";
const RECEIVED_AT: &str = "2026-08-21T07:00:00.000Z";
const VERSION: &str = "wire-fixture";

/// Compare every service body variant to the original handler's exact JSON.stringify bytes.
#[test]
fn public_service_wire_matches_original_node() {
    let fixture = FixtureRoot::new("public-service-wire");
    let oracle =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/public-service-wire-node-oracle.mjs");
    let result = Command::new("node")
        .arg(oracle)
        .arg(&fixture.0)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let sites = group_active_sites(vec![source(false), source(true)]);
    let site = sites.data[0].clone();
    let clock = NodePrimitives.resolve(&instant(DATA_AT)).unwrap();
    let projected = project_forecast_weather_records(
        vec![forecast_row()],
        &site,
        &clock,
        &NodePrimitives,
        InactiveForecastDecisionConfig::default(),
    )
    .unwrap();
    let forecast = projected.first().unwrap();
    let record = c::ApiWeatherRecord {
        freshness: forecast.freshness.clone(),
        id: forecast.id.clone(),
        metadata: forecast.metadata.clone(),
        metrics: forecast.metrics.clone(),
        productRunAt: forecast.productRunAt.clone(),
        provenance: forecast.provenance.clone(),
        receivedAt: forecast.receivedAt.clone(),
        revisionCount: forecast.revisionCount,
        validAt: forecast.validAt.clone(),
    };
    let current = c::ApiCurrentWeatherRecord {
        freshness: record.freshness.clone(),
        id: record.id.clone(),
        metadata: record.metadata.clone(),
        metrics: record.metrics.clone(),
        productRunAt: record.productRunAt.clone(),
        provenance: record.provenance.clone(),
        receivedAt: record.receivedAt.clone(),
        revisionCount: record.revisionCount,
        validAt: record.validAt.clone(),
        airQuality: Nullable::Null,
        pressureChange3hHpa: n(1.5),
    };
    let adjustment = adjustment_runtime();
    let temperature = temperature_runtime();
    let bodies = [
        ("sites", WeatherServiceBody::Sites(sites)),
        (
            "current",
            WeatherServiceBody::Current(c::ApiCurrentWeatherResponse {
                data: vec![current],
                generatedAt: DATA_AT.to_owned(),
                site: site.clone(),
            }),
        ),
        (
            "history",
            WeatherServiceBody::History(c::ApiHistoryResponse {
                data: vec![record.clone()],
                generatedAt: DATA_AT.to_owned(),
                page: c::ApiHistoryResponsePage {
                    limit: 10,
                    nextCursor: Nullable::Null,
                },
                site: site.clone(),
            }),
        ),
        (
            "forecast",
            WeatherServiceBody::Forecast(Box::new(c::ApiForecastResponse {
                adjustmentRuntime: adjustment.clone(),
                airQualityContext: Vec::new(),
                rainAdjustmentRuntime: c::ApiRainAdjustmentRuntime {
                    state: c::ApiRainAdjustmentRuntimeStateEnum::Disabled,
                    activeBundle: Nullable::Null,
                    reasonCode: Nullable::Value(
                        c::ApiRainAdjustmentRuntimeReasonCodeEnum::ModelUnavailable,
                    ),
                    loadedAt: DATA_AT.to_owned(),
                    source: Nullable::Null,
                },
                data: projected,
                days: c::ForecastDays::Value1,
                generatedAt: DATA_AT.to_owned(),
                pressureContext: Vec::new(),
                site: site.clone(),
                temperatureAdjustmentRuntime: temperature.clone(),
            })),
        ),
        (
            "precipitation",
            WeatherServiceBody::DailyPrecipitation(c::ApiDailyPrecipitationResponse {
                data: Nullable::Value(c::ApiDailyPrecipitation {
                    accumulationMm: 2.54,
                    source: c::ApiDailyPrecipitationSource {
                        sourceId: "13".to_owned(),
                        stationSlug: "gauge-station".to_owned(),
                    },
                    validThrough: DATA_AT.to_owned(),
                }),
                generatedAt: DATA_AT.to_owned(),
                site: site.clone(),
            }),
        ),
        (
            "tides",
            WeatherServiceBody::Tides(c::ApiTidesResponse {
                data: vec![c::ApiTideRecord {
                    eventType: Nullable::Value(c::ApiTideRecordEventTypeEnum::High),
                    kind: c::ApiTideRecordKindEnum::Prediction,
                    source: c::ApiTideRecordSource {
                        attribution: c::ApiTideRecordSourceAttribution {
                            label: "NOAA".to_owned(),
                            url: "https://example.test/tides".to_owned(),
                        },
                        providerKey: "noaa".to_owned(),
                        stationName: "Tide station".to_owned(),
                        stationSlug: "tide-station".to_owned(),
                    },
                    validAt: VALID_AT.to_owned(),
                    waterLevelM: 3.271,
                }],
                generatedAt: DATA_AT.to_owned(),
                site: site.clone(),
            }),
        ),
        (
            "trends",
            WeatherServiceBody::Trends(c::ApiTrendsResponse {
                data: vec![c::ApiTrendPoint {
                    metrics: c::ApiTrendPointMetrics {
                        apparentTemperatureC: n(10.1),
                        pm25MicrogramsPerCubicMeter: n(12.3),
                        precipitationMm: n(1.2),
                        pressureHpa: n(1012.4),
                        relativeHumidityPercent: n(82.0),
                        temperatureC: n(11.5),
                        temperatureMaximumC: n(19.4),
                        temperatureMinimumC: n(8.2),
                        windDirectionDegrees: n(-135.0),
                        windGustMps: n(7.2),
                        windSpeedMps: n(4.1),
                    },
                    validAt: RECEIVED_AT.to_owned(),
                }],
                generatedAt: DATA_AT.to_owned(),
                site,
            }),
        ),
        (
            "rain",
            WeatherServiceBody::RainCollection(rain_collection()),
        ),
        (
            "health",
            WeatherServiceBody::Health(Box::new(c::ApiHealthResponse {
                data: c::ApiHealthResponseData {
                    adjustmentRuntime: adjustment,
                    temperatureAdjustmentRuntime: temperature,
                    database: c::ApiHealthResponseDataDatabaseEnum::Ready,
                    live: c::ApiHealthResponseDataLiveLiteral,
                    migration: c::ApiHealthResponseDataMigration {
                        status: c::ApiHealthResponseDataMigrationStatusEnum::Current,
                        version: s("fixture-migration"),
                    },
                    ready: true,
                    version: VERSION.to_owned(),
                    worker: c::ApiHealthResponseDataWorker {
                        freshness: c::ApiHealthResponseDataWorkerFreshnessEnum::Fresh,
                    },
                },
            })),
        ),
        (
            "observation",
            WeatherServiceBody::ForecastObservationInput(Box::new(observation())),
        ),
        (
            "error",
            WeatherServiceBody::Error(c::ApiErrorResponse {
                error: c::ApiErrorResponseError {
                    code: "not_found".to_owned(),
                    message: "Endpoint not found".to_owned(),
                },
            }),
        ),
    ];
    let mut encoder = NativeWeatherServiceWireEncoder::new(NodePrimitives);
    // reuse one actual encoder across every response and compare exact bytes including ordering
    for (name, body) in bodies {
        let expected = fs::read(fixture.0.join(format!("{name}.json"))).unwrap();
        let encoded = encoder.encode(&body).expect("concrete service encoder");
        assert_eq!(
            String::from_utf8_lossy(encoded),
            String::from_utf8_lossy(&expected),
            "{name}"
        );
    }
}

/// Build one original source catalog row shared by the Rust service projections.
fn source(gauge: bool) -> ActiveSiteRow {
    let mut row = ActiveSiteRow {
        attribution_label: "Weather data by Open-Meteo".to_owned(),
        attribution_url: "https://open-meteo.com/".to_owned(),
        latitude: 47.95,
        longitude: -122.42,
        provider_key: "open-meteo".to_owned(),
        provider_name: "Open-Meteo".to_owned(),
        site_name: "Ballydidean".to_owned(),
        site_slug: "ballydidean".to_owned(),
        source_id: "12".to_owned(),
        source_key: "open-meteo-forecast-v4".to_owned(),
        source_kind: c::SourceKind::Forecast,
        station_kind: c::StationKind::Virtual,
        station_latitude: 47.96,
        station_longitude: -122.43,
        station_name: "Open-Meteo Forecast".to_owned(),
        station_slug: "open-meteo-forecast".to_owned(),
        timezone: "America/Los_Angeles".to_owned(),
    };
    // retain both source variants in the same site catalog
    if gauge {
        row.attribution_label = "Gauge".to_owned();
        row.attribution_url = "https://example.test/gauge".to_owned();
        row.provider_key = "gauge".to_owned();
        row.provider_name = "Gauge".to_owned();
        row.source_id = "13".to_owned();
        row.source_key = "gauge-source".to_owned();
        row.source_kind = c::SourceKind::PhysicalSensor;
        row.station_kind = c::StationKind::Physical;
        row.station_name = "Gauge station".to_owned();
        row.station_slug = "gauge-station".to_owned();
    }
    row
}

/// Build the exact typed raw record projected by the original forecast implementation.
fn forecast_row() -> c::WeatherRecordRow {
    c::WeatherRecordRow {
        adapterVersion: s("open-meteo-forecast-daily/v4"),
        apparentTemperatureC: n(11.7),
        blackGlobeTemperatureC: Nullable::Null,
        cloudCoverPercent: n(72.0),
        contentHash: "sha256-fixture".to_owned(),
        contractEpoch: s("v4"),
        deviceModel: s("model-x"),
        deviceSerial: Nullable::Null,
        deviceVendor: s("vendor-x"),
        firstReceivedAt: instant(RECEIVED_AT),
        id: "forecast-fixture".to_owned(),
        lastReceivedAt: instant(RECEIVED_AT),
        pm25MicrogramsPerCubicMeter: n(5.0),
        precipitationMm: n(0.1),
        precipitationRateMmPerHour: n(0.1),
        pressureHpa: n(1018.1),
        productRunAt: Nullable::Value(instant(RECEIVED_AT)),
        providerKey: "open-meteo".to_owned(),
        providerMetadata: Nullable::Value(vec![
            field("dataset", json_text("forecast")),
            field("elevation_m", JsonValue::Number(15.5)),
            field("grid_cell", json_text("cell-1")),
        ]),
        qualityMetadata: Nullable::Value(vec![
            field("confidence_percent", JsonValue::Number(95.0)),
            field("flags", JsonValue::Array(vec![json_text("fixture")])),
            field("status", json_text("ready")),
        ]),
        relativeHumidityPercent: n(83.0),
        revisionCount: 2,
        siteSlug: "ballydidean".to_owned(),
        sourceId: "12".to_owned(),
        sourceConfigFingerprint: s("fingerprint"),
        sourceKey: "open-meteo-forecast-v4".to_owned(),
        sourceKind: c::SourceKind::Forecast,
        stationSlug: "open-meteo-forecast".to_owned(),
        soilElectricalConductivityMicrosiemensPerCm: Nullable::Null,
        soilMoisturePercent: Nullable::Null,
        solarRadiationWm2: n(120.0),
        temperatureC: n(12.1),
        upstreamModel: s("best_match"),
        upstreamTimezone: "GMT".to_owned(),
        uvIndex: n(0.2),
        validAt: instant(VALID_AT),
        waterLevelM: Nullable::Null,
        windDirectionDegrees: n(180.0),
        windGustMps: n(3.1),
        windSpeedMps: n(1.8),
        wetBulbGlobeTemperatureC: Nullable::Null,
    }
}

/// Construct the unconfigured wind runtime's exact nullable and missing identities.
fn adjustment_runtime() -> c::ApiForecastAdjustmentRuntime {
    c::ApiForecastAdjustmentRuntime {
        actionSha256: FieldPresence::Missing,
        activationMode: Nullable::Null,
        activeBundle: Nullable::Null,
        authorizationSha256: Nullable::Null,
        candidateArtifactSha256: Nullable::Null,
        enabledMetrics: Vec::new(),
        evaluationReportSha256: Nullable::Null,
        expiresAt: Nullable::Null,
        fullMemberRootSha256: FieldPresence::Missing,
        loadedAt: DATA_AT.to_owned(),
        qualificationReceiptSha256: Nullable::Null,
        reasonCode: Nullable::Value(
            c::ApiForecastAdjustmentRuntimeReasonCodeEnum::RegistryInactive,
        ),
        state: c::ApiForecastAdjustmentRuntimeStateEnum::Disabled,
        policyReportSha256: FieldPresence::Missing,
        transferReportSha256: Nullable::Null,
    }
}

/// Construct the independent disabled temperature runtime with all required explicit nulls.
fn temperature_runtime() -> c::ApiForecastTemperatureAdjustmentRuntime {
    c::ApiForecastTemperatureAdjustmentRuntime {
        actionSha256: Nullable::Null,
        activationMode: Nullable::Null,
        activeBundle: Nullable::Null,
        authorizationSha256: Nullable::Null,
        expiresAt: Nullable::Null,
        fullMemberRootSha256: Nullable::Null,
        loadedAt: DATA_AT.to_owned(),
        policyReportSha256: Nullable::Null,
        reasonCode: Nullable::Value(
            c::ApiForecastTemperatureAdjustmentRuntimeReasonCodeEnum::RegistryInactive,
        ),
        source: Nullable::Null,
        state: c::ApiForecastTemperatureAdjustmentRuntimeStateEnum::Disabled,
    }
}

/// Construct the original bounded public collection counters and explicit missing timestamps.
fn rain_collection() -> c::ApiRainCollectionResponse {
    c::ApiRainCollectionResponse {
        data: c::ApiRainCollectionResponseData {
            contractVersion: c::ApiRainCollectionResponseDataContractVersionLiteral,
            modelEnabled: false,
            qualificationEnabled: c::ApiRainCollectionResponseDataQualificationEnabledLiteral,
            stationAccessAuthorized: true,
            expectedStations: 12,
            startsAt: "2026-09-13T00:00:00.000Z".to_owned(),
            expiresAt: "2027-10-08T00:00:00.000Z".to_owned(),
            claims: 12,
            receipts: 11,
            validForecasts: 3,
            timelyForecasts: 2,
            validStationWindows: 7,
            stationsSeen: 4,
            failedRequests: 1,
            pendingRequests: 2,
            unknownRequests: 1,
            compressedBytes: 4096,
            lastClaimAt: s(DATA_AT),
            lastReceiptAt: s(DATA_AT),
            lastForecastReceiptAt: Nullable::Null,
            lastStationReceiptAt: Nullable::Null,
            pausedUntil: Nullable::Null,
        },
        generatedAt: DATA_AT.to_owned(),
        version: VERSION.to_owned(),
    }
}

/// Construct the original private observation projection with no extra public metrics.
fn observation() -> ForecastObservationInputResponse {
    ForecastObservationInputResponse {
        contractVersion: ForecastObservationInputResponseContractVersionLiteral,
        generatedAt: DATA_AT.to_owned(),
        projectionFrom: "2026-08-21T07:00:00.000Z".to_owned(),
        queryFrom: "2026-08-21T04:00:00.000Z".to_owned(),
        records: vec![ForecastObservationInputResponseRecord {
            id: "forecast-fixture".to_owned(),
            metrics: ForecastObservationInputResponseMetrics {
                apparentTemperatureC: n(11.7),
                cloudCoverPercent: n(72.0),
                pm25MicrogramsPerCubicMeter: n(5.0),
                precipitationRateMmPerHour: n(0.1),
                pressureHpa: n(1018.1),
                relativeHumidityPercent: n(83.0),
                solarRadiationWm2: n(120.0),
                temperatureC: n(12.1),
                uvIndex: n(0.2),
                windDirectionDegrees: n(180.0),
                windGustMps: n(3.1),
                windSpeedMps: n(1.8),
            },
            provenance: ForecastObservationInputResponseProvenance {
                providerKey: "gauge".to_owned(),
                sourceId: "13".to_owned(),
                sourceKey: "gauge-source".to_owned(),
                sourceKind: c::ForecastObservationInputRowSourceKindEnum::PhysicalSensor,
                stationSlug: "gauge-station".to_owned(),
            },
            receivedAt: RECEIVED_AT.to_owned(),
            validAt: DATA_AT.to_owned(),
        }],
        site: ForecastObservationInputResponseSite {
            slug: "ballydidean".to_owned(),
            timezone: "America/Los_Angeles".to_owned(),
        },
        through: DATA_AT.to_owned(),
    }
}

/// Render one fixture repository instant without normalizing its wire string.
fn instant(value: &str) -> c::JsDateOrString {
    c::JsDateOrString::String(value.encode_utf16().collect())
}
/// Wrap one present fixture metric.
fn n(value: f64) -> Nullable<f64> {
    Nullable::Value(value)
}
/// Wrap one present fixture string.
fn s(value: &str) -> Nullable<String> {
    Nullable::Value(value.to_owned())
}
/// Construct one typed JSON value used by the real metadata projector.
fn json_text(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}
/// Preserve one exact provider metadata member.
fn field(key: &str, value: JsonValue) -> (Vec<u16>, JsonValue) {
    (key.encode_utf16().collect(), value)
}
