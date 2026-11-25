use weather_contracts::{CurrentWeatherRecordRow, JsDateOrString, Nullable, SourceKind};
use weather_database::{
    CURRENT_WEATHER_SQL, CurrentWeatherBind, CurrentWeatherQuery, current_weather_query,
    project_current_weather_rows,
};

// build one generated row with representative nullable decimal and timing values
fn row() -> CurrentWeatherRecordRow {
    CurrentWeatherRecordRow {
        adapterVersion: Nullable::Null,
        apparentTemperatureC: Nullable::Value(12.5),
        blackGlobeTemperatureC: Nullable::Null,
        cloudCoverPercent: Nullable::Value(82.0),
        contentHash: "sha256-content".to_owned(),
        contractEpoch: Nullable::Null,
        deviceModel: Nullable::Null,
        deviceSerial: Nullable::Null,
        deviceVendor: Nullable::Null,
        firstReceivedAt: JsDateOrString::String(
            "2026-08-22T05:00:00.000Z".encode_utf16().collect(),
        ),
        id: "100".to_owned(),
        lastReceivedAt: JsDateOrString::String("2026-08-22T05:01:00.000Z".encode_utf16().collect()),
        pm25MicrogramsPerCubicMeter: Nullable::Null,
        precipitationMm: Nullable::Value(0.25),
        precipitationRateMmPerHour: Nullable::Null,
        pressureHpa: Nullable::Value(1018.25),
        productRunAt: Nullable::Null,
        providerKey: "open-meteo".to_owned(),
        providerMetadata: Nullable::Null,
        qualityMetadata: Nullable::Null,
        relativeHumidityPercent: Nullable::Value(78.0),
        revisionCount: 0,
        siteSlug: "ballydidean".to_owned(),
        sourceId: "10".to_owned(),
        sourceConfigFingerprint: Nullable::Null,
        sourceKey: "open-meteo-current-v1".to_owned(),
        sourceKind: SourceKind::ModelCurrent,
        stationSlug: "open-meteo-virtual".to_owned(),
        soilElectricalConductivityMicrosiemensPerCm: Nullable::Null,
        soilMoisturePercent: Nullable::Null,
        solarRadiationWm2: Nullable::Null,
        temperatureC: Nullable::Value(13.0),
        upstreamModel: Nullable::Value("best_match".to_owned()),
        upstreamTimezone: "GMT".to_owned(),
        uvIndex: Nullable::Null,
        validAt: JsDateOrString::String("2026-08-22T05:00:00.000Z".encode_utf16().collect()),
        waterLevelM: Nullable::Null,
        windDirectionDegrees: Nullable::Value(180.0),
        windGustMps: Nullable::Value(4.5),
        windSpeedMps: Nullable::Value(2.0),
        wetBulbGlobeTemperatureC: Nullable::Null,
        pm25HourlyBySensor: Nullable::Null,
        pm25HourlyConcentrations: Nullable::Value(vec![Nullable::Value(5.5), Nullable::Null]),
        pm25HourlyThrough: JsDateOrString::String(
            "2026-08-22T04:00:00.000Z".encode_utf16().collect(),
        ),
        pressureChange3hHpa: Nullable::Value(-1.25),
    }
}

#[test]
// bind required and optional current-weather values in placeholder order
fn builds_exact_current_weather_plan() {
    let plan = current_weather_query(CurrentWeatherQuery {
        site_slug: "ballydidean".to_owned(),
        station_slug: Some("open-meteo-virtual".to_owned()),
        source_id: Some("10".to_owned()),
        as_of: "2026-08-22T05:37:00.000Z".to_owned(),
    });

    assert_eq!(plan.sql, CURRENT_WEATHER_SQL);
    assert_eq!(
        plan.binds,
        [
            CurrentWeatherBind::Text("ballydidean".to_owned()),
            CurrentWeatherBind::NullableText(Some("open-meteo-virtual".to_owned())),
            CurrentWeatherBind::NullableText(Some("10".to_owned())),
            CurrentWeatherBind::UtcInstant("2026-08-22T05:37:00.000Z".to_owned()),
        ],
    );
}

#[test]
// preserve SQL null for absent station and source filters
fn binds_absent_filters_as_null() {
    let plan = current_weather_query(CurrentWeatherQuery {
        site_slug: "ballydidean".to_owned(),
        station_slug: None,
        source_id: None,
        as_of: "2026-08-22T05:37:00.000Z".to_owned(),
    });

    assert_eq!(plan.binds[1], CurrentWeatherBind::NullableText(None));
    assert_eq!(plan.binds[2], CurrentWeatherBind::NullableText(None));
}

#[test]
// retain the source-owned pressure particulate active and ordering semantics
fn retains_current_weather_sql_semantics() {
    assert!(CURRENT_WEATHER_SQL.contains("candidate.valid_at <= $4::timestamptz"));
    assert!(CURRENT_WEATHER_SQL.contains("JOIN LATERAL"));
    assert!(CURRENT_WEATHER_SQL.contains("WITH bounds AS NOT MATERIALIZED"));
    assert!(CURRENT_WEATHER_SQL.contains("raw_pm25 AS MATERIALIZED"));
    assert!(CURRENT_WEATHER_SQL.contains("generate_series(0, 11)"));
    assert!(CURRENT_WEATHER_SQL.contains("weather_source_is_current(s.id)"));
    assert!(CURRENT_WEATHER_SQL.contains("($2::text IS NULL OR st.slug = $2)"));
    assert!(CURRENT_WEATHER_SQL.contains("($3::bigint IS NULL OR s.id = $3)"));
    assert!(CURRENT_WEATHER_SQL.ends_with("      ORDER BY wr.source_id\n    "));
}

#[test]
// preserve every generated storage value without JSON or time coercion
fn preserves_typed_current_weather_projection() {
    let expected = row();
    let result = project_current_weather_rows(vec![expected.clone()]);
    assert_eq!(result.rows, vec![expected]);
    assert_eq!(result.rows[0].adapterVersion, Nullable::Null);
    assert_eq!(result.rows[0].pressureHpa, Nullable::Value(1018.25));
    assert_eq!(
        result.rows[0].pm25HourlyConcentrations,
        Nullable::Value(vec![Nullable::Value(5.5), Nullable::Null]),
    );
}
