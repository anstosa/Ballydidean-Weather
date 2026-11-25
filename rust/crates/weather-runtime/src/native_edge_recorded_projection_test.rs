//! Functional differential fixtures for native recorded-product projections.

use std::io::Write as _;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use weather_contracts::JsonValue;
use weather_edge::{
    AdmittedForecastRecordedInstant, ArchiveWirePrimitives, ForecastRecordedInputMetrics,
    ForecastRecordedInputProvenance, ForecastRecordedInputRecord, ForecastRecordedInputSourceKind,
    ForecastRecordedSolarSite, RevisionArchiveCanonicalCodec,
};

use crate::NativeWirePrimitives;
use crate::native_edge_recorded_owner::NativeForecastRecordedProjection;
use crate::native_edge_recorded_projection::NativeForecastRecordedProjectionOwner;

const NODE_PROJECTION: &str = r#"
import { readFileSync } from "node:fs";
import {
  buildForecastObservationPoints,
  buildSolarCloudEvidence,
} from "./apps/web/dist/index.js";

const fixture = JSON.parse(readFileSync(0, "utf8"));
process.stdout.write(JSON.stringify({
  observations: buildForecastObservationPoints(
    fixture.records,
    fixture.from,
    fixture.through,
  ),
  solar: buildSolarCloudEvidence(
    fixture.records,
    fixture.site,
    fixture.solarThrough,
  ),
}));
"#;

/// Compare nonempty native projections with the compiled original TypeScript functions.
#[test]
fn recorded_projections_match_compiled_typescript() {
    let projection_from = instant("2026-06-21T17:00:00.000Z");
    let through = instant("2026-06-21T21:30:00.000Z");
    let solar_through = instant("2026-06-22T05:00:00.000Z");
    let site = ForecastRecordedSolarSite {
        latitude: 47.950_429_954_185_445,
        longitude: -122.427_970_126_081_93,
    };
    let records = typed_records();
    let projection = NativeForecastRecordedProjectionOwner;
    let observations = projection
        .forecast_observation_points(&records, &projection_from, &through)
        .expect("native observation projection");
    let solar = projection
        .solar_cloud_evidence(&records, site, &solar_through)
        .expect("native solar projection")
        .unwrap_or(JsonValue::Null);
    let expected = object(vec![("observations", observations), ("solar", solar)]);
    let input = object(vec![
        ("from", text(&projection_from.value)),
        ("through", text(&through.value)),
        ("solarThrough", text(&solar_through.value)),
        (
            "site",
            object(vec![
                ("latitude", JsonValue::Number(site.latitude)),
                ("longitude", JsonValue::Number(site.longitude)),
            ]),
        ),
        (
            "records",
            JsonValue::Array(records.iter().map(record_value).collect()),
        ),
    ]);
    let actual = run_node_projection(&input);
    assert_eq!(actual, expected);

    let observations = field(&actual, "observations");
    // require the complete original hourly projection before inspecting witnesses
    let JsonValue::Array(points) = observations else {
        panic!("observation array");
    };
    assert_eq!(points.len(), 5);
    // locate the populated hour independently of array position
    let point = points
        .iter()
        .find(|point| string(field(point, "validAt")) == "2026-06-21T20:00:00.000Z")
        .expect("populated hour");
    let projected = field(point, "metrics");
    assert_eq!(number(field(projected, "temperatureC")), 12.0);
    assert_eq!(number(field(projected, "pm25MicrogramsPerCubicMeter")), 8.0);
    assert_eq!(number(field(projected, "cloudCoverPercent")), 25.0);
    assert_eq!(number(field(point, "pressureChange3hHpa")), 3.0);
    assert_eq!(number(field(projected, "windGustMps")), 10.0);
    let solar = field(&actual, "solar");
    assert_eq!(
        string(field(solar, "evaluatedAt")),
        "2026-06-21T20:11:00.000Z"
    );
    assert_eq!(string(field(field(solar, "ws90"), "recordId")), "farm-20");
}

/// Build genuine typed rows spanning source precedence, missing data, history, and solar evidence.
fn typed_records() -> Vec<ForecastRecordedInputRecord> {
    let mut records = vec![
        physical(
            "farm-17",
            "farm",
            "2026-06-21T17:10:00.000Z",
            metrics_with_pressure(1017.0),
        ),
        physical(
            "farm-18",
            "farm",
            "2026-06-21T18:10:00.000Z",
            metrics_with_pressure(1018.0),
        ),
        physical(
            "farm-19",
            "farm",
            "2026-06-21T19:10:00.000Z",
            ForecastRecordedInputMetrics {
                pressure_hpa: Some(1019.0),
                wind_gust_mps: Some(10.0),
                ..missing_metrics()
            },
        ),
        physical(
            "farm-19-later",
            "farm",
            "2026-06-21T19:50:00.000Z",
            ForecastRecordedInputMetrics {
                wind_gust_mps: Some(2.0),
                ..missing_metrics()
            },
        ),
        record(
            "farm-20",
            "ecowitt-local",
            "farm",
            "ecowitt-88f15505d89f-local-live-v1",
            ForecastRecordedInputSourceKind::PhysicalSensor,
            "ballydidean-ecowitt",
            "2026-06-21T20:10:00.000Z",
            "2026-06-21T20:11:00.000Z",
            ForecastRecordedInputMetrics {
                cloud_cover_percent: None,
                precipitation_rate_mm_per_hour: Some(0.0),
                pressure_hpa: Some(1020.0),
                relative_humidity_percent: Some(0.0),
                solar_radiation_wm2: Some(800.0),
                temperature_c: Some(10.0),
                ..missing_metrics()
            },
        ),
        physical(
            "farm-20-later",
            "farm",
            "2026-06-21T20:20:00.000Z",
            ForecastRecordedInputMetrics {
                pm25_micrograms_per_cubic_meter: Some(-1.0),
                temperature_c: Some(14.0),
                ..missing_metrics()
            },
        ),
        local(
            "a-1",
            "station-a",
            "station-a-source",
            "2026-06-21T20:05:00.000Z",
            ForecastRecordedInputMetrics {
                pm25_micrograms_per_cubic_meter: Some(4.0),
                temperature_c: Some(40.0),
                ..missing_metrics()
            },
        ),
        local(
            "a-2",
            "station-a",
            "station-a-source",
            "2026-06-21T20:15:00.000Z",
            ForecastRecordedInputMetrics {
                pm25_micrograms_per_cubic_meter: Some(8.0),
                ..missing_metrics()
            },
        ),
        local(
            "a-duplicate-feed",
            "station-a",
            "station-a-other",
            "2026-06-21T20:05:00.000Z",
            ForecastRecordedInputMetrics {
                pm25_micrograms_per_cubic_meter: Some(100.0),
                ..missing_metrics()
            },
        ),
        local(
            "b-1",
            "station-b",
            "station-b-source",
            "2026-06-21T20:10:00.000Z",
            ForecastRecordedInputMetrics {
                pm25_micrograms_per_cubic_meter: Some(10.0),
                ..missing_metrics()
            },
        ),
        regional(
            "regional-20",
            "2026-06-21T20:00:00.000Z",
            "2026-06-21T20:01:00.000Z",
            ForecastRecordedInputMetrics {
                cloud_cover_percent: Some(25.0),
                temperature_c: Some(60.0),
                ..missing_metrics()
            },
        ),
        physical(
            "missing-21",
            "farm",
            "2026-06-21T21:00:00.000Z",
            missing_metrics(),
        ),
        physical(
            "old-excluded",
            "farm",
            "2026-06-21T13:00:00.000Z",
            ForecastRecordedInputMetrics {
                temperature_c: Some(-50.0),
                ..missing_metrics()
            },
        ),
    ];
    let mut late = physical(
        "late-excluded",
        "farm",
        "2026-06-21T20:00:00.000Z",
        ForecastRecordedInputMetrics {
            temperature_c: Some(50.0),
            ..missing_metrics()
        },
    );
    late.received_at = instant("2026-06-21T21:31:00.000Z");
    records.push(late);
    records
}

/// Construct one farm physical record with original reception equal to validity.
fn physical(
    id: &str,
    source_id: &str,
    valid_at: &str,
    metrics: ForecastRecordedInputMetrics,
) -> ForecastRecordedInputRecord {
    record(
        id,
        "ecowitt-local",
        source_id,
        "ecowitt-88f15505d89f-local-live-v1",
        ForecastRecordedInputSourceKind::PhysicalSensor,
        "ballydidean-ecowitt",
        valid_at,
        valid_at,
        metrics,
    )
}

/// Construct one admitted local physical record.
fn local(
    id: &str,
    station: &str,
    source_id: &str,
    valid_at: &str,
    metrics: ForecastRecordedInputMetrics,
) -> ForecastRecordedInputRecord {
    record(
        id,
        "weatherflow-tempest",
        source_id,
        source_id,
        ForecastRecordedInputSourceKind::PhysicalSensor,
        station,
        valid_at,
        valid_at,
        metrics,
    )
}

/// Construct one admitted regional model-current record.
fn regional(
    id: &str,
    valid_at: &str,
    received_at: &str,
    metrics: ForecastRecordedInputMetrics,
) -> ForecastRecordedInputRecord {
    record(
        id,
        "open-meteo",
        "regional",
        "open-meteo-current-v1",
        ForecastRecordedInputSourceKind::ModelCurrent,
        "open-meteo-virtual",
        valid_at,
        received_at,
        metrics,
    )
}

/// Construct one fully typed projection record.
#[allow(clippy::too_many_arguments)]
fn record(
    id: &str,
    provider_key: &str,
    source_id: &str,
    source_key: &str,
    source_kind: ForecastRecordedInputSourceKind,
    station_slug: &str,
    valid_at: &str,
    received_at: &str,
    metrics: ForecastRecordedInputMetrics,
) -> ForecastRecordedInputRecord {
    ForecastRecordedInputRecord {
        id: id.to_owned(),
        metrics,
        provenance: ForecastRecordedInputProvenance {
            provider_key: provider_key.to_owned(),
            source_id: source_id.to_owned(),
            source_key: source_key.to_owned(),
            source_kind,
            station_slug: station_slug.to_owned(),
        },
        received_at: instant(received_at),
        valid_at: instant(valid_at),
    }
}

/// Construct the closed all-missing metric set.
fn missing_metrics() -> ForecastRecordedInputMetrics {
    ForecastRecordedInputMetrics {
        apparent_temperature_c: None,
        cloud_cover_percent: None,
        pm25_micrograms_per_cubic_meter: None,
        precipitation_rate_mm_per_hour: None,
        pressure_hpa: None,
        relative_humidity_percent: None,
        solar_radiation_wm2: None,
        temperature_c: None,
        uv_index: None,
        wind_direction_degrees: None,
        wind_gust_mps: None,
        wind_speed_mps: None,
    }
}

/// Construct one pressure-only metric set.
fn metrics_with_pressure(value: f64) -> ForecastRecordedInputMetrics {
    ForecastRecordedInputMetrics {
        pressure_hpa: Some(value),
        ..missing_metrics()
    }
}

/// Construct one canonical typed test instant.
fn instant(value: &str) -> AdmittedForecastRecordedInstant {
    let epoch_ms = NativeWirePrimitives
        .canonical_utc_milliseconds(value)
        .expect("canonical fixture instant");
    AdmittedForecastRecordedInstant::admitted(value, epoch_ms)
}

/// Run the compiled original functions against JSON derived from the typed Rust rows.
fn run_node_projection(input: &JsonValue) -> JsonValue {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let bytes = NativeWirePrimitives
        .encode_ordered_json(input)
        .expect("fixture JSON");
    let mut child = Command::new("node")
        .args(["--input-type=module", "--eval", NODE_PROJECTION])
        .current_dir(root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("compiled TypeScript projection oracle");
    child
        .stdin
        .take()
        .expect("Node stdin")
        .write_all(&bytes)
        .expect("write typed fixture");
    let output = child.wait_with_output().expect("Node projection output");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    NativeWirePrimitives
        .decode_json(&output.stdout)
        .expect("Node JSON output")
}

/// Encode one typed record into the exact original input field names.
fn record_value(record: &ForecastRecordedInputRecord) -> JsonValue {
    object(vec![
        ("id", text(&record.id)),
        ("validAt", text(&record.valid_at.value)),
        ("receivedAt", text(&record.received_at.value)),
        ("metrics", metrics_value(&record.metrics)),
        (
            "provenance",
            object(vec![
                ("providerKey", text(&record.provenance.provider_key)),
                ("sourceId", text(&record.provenance.source_id)),
                ("sourceKey", text(&record.provenance.source_key)),
                (
                    "sourceKind",
                    text(source_kind_value(record.provenance.source_kind)),
                ),
                ("stationSlug", text(&record.provenance.station_slug)),
            ]),
        ),
    ])
}

/// Encode the twelve exact nullable metric fields consumed by both projections.
fn metrics_value(metrics: &ForecastRecordedInputMetrics) -> JsonValue {
    object(vec![
        (
            "apparentTemperatureC",
            nullable(metrics.apparent_temperature_c),
        ),
        ("cloudCoverPercent", nullable(metrics.cloud_cover_percent)),
        (
            "pm25MicrogramsPerCubicMeter",
            nullable(metrics.pm25_micrograms_per_cubic_meter),
        ),
        (
            "precipitationRateMmPerHour",
            nullable(metrics.precipitation_rate_mm_per_hour),
        ),
        ("pressureHpa", nullable(metrics.pressure_hpa)),
        (
            "relativeHumidityPercent",
            nullable(metrics.relative_humidity_percent),
        ),
        ("solarRadiationWm2", nullable(metrics.solar_radiation_wm2)),
        ("temperatureC", nullable(metrics.temperature_c)),
        ("uvIndex", nullable(metrics.uv_index)),
        (
            "windDirectionDegrees",
            nullable(metrics.wind_direction_degrees),
        ),
        ("windGustMps", nullable(metrics.wind_gust_mps)),
        ("windSpeedMps", nullable(metrics.wind_speed_mps)),
    ])
}

/// Return one original source-kind wire spelling.
fn source_kind_value(source_kind: ForecastRecordedInputSourceKind) -> &'static str {
    // preserve the closed typed alternatives
    match source_kind {
        ForecastRecordedInputSourceKind::PhysicalSensor => "physical_sensor",
        ForecastRecordedInputSourceKind::ModelCurrent => "model_current",
    }
}

/// Encode one nullable finite fixture number.
fn nullable(value: Option<f64>) -> JsonValue {
    value.map_or(JsonValue::Null, JsonValue::Number)
}

/// Build one producer-ordered generated JSON object.
fn object(fields: Vec<(&str, JsonValue)>) -> JsonValue {
    JsonValue::Object(
        fields
            .into_iter()
            .map(|(name, value)| (name.encode_utf16().collect(), value))
            .collect(),
    )
}

/// Build one generated JSON string.
fn text(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}

/// Borrow one expected object member.
fn field<'a>(value: &'a JsonValue, name: &str) -> &'a JsonValue {
    // require object shape before looking up one closed fixture field
    let JsonValue::Object(fields) = value else {
        panic!("expected object for {name}");
    };
    // locate the closed fixture field by its UTF-16 key
    fields
        .iter()
        .find_map(|(key, value)| key.iter().copied().eq(name.encode_utf16()).then_some(value))
        .unwrap_or_else(|| panic!("missing {name}"))
}

/// Decode one expected generated JSON string.
fn string(value: &JsonValue) -> String {
    // require the original function's string output
    let JsonValue::String(value) = value else {
        panic!("expected string");
    };
    String::from_utf16(value).expect("fixture string")
}

/// Decode one expected generated JSON number.
fn number(value: &JsonValue) -> f64 {
    // require the original function's numeric output
    let JsonValue::Number(value) = value else {
        panic!("expected number");
    };
    *value
}
