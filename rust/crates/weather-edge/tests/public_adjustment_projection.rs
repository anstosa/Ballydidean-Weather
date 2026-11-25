use std::fmt::Write as _;
use std::io::Write as _;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Mutex;

use weather_contracts::JsonValue;
use weather_edge::{
    DEFAULT_FORECAST_ADJUSTMENT_SETTINGS, ForecastAdjustmentSettings,
    ForecastAdjustmentSettingsStore, InMemoryForecastAdjustmentSettingsStore,
    PublicAdjustmentJsonCodec, PublicAdjustmentProjector, PublicApiProjection,
    PublicApiResponseProjector,
};

/// Run only the disposable Node codec or the extracted real Node projection.
fn node(mode: &[&str], body: &[u8]) -> Vec<u8> {
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/public-adjustment-projection-node-oracle.mjs");
    let mut process = Command::new("node")
        .arg(script)
        .args(mode)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Node fixture");
    process.stdin.take().unwrap().write_all(body).unwrap();
    let output = process.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}

/// Encode one string for the test transport without JSON serialization in Rust.
fn witness_string(units: &[u16], output: &mut String) {
    output.push('s');
    // preserve exact code units for this test-only transport
    for (index, unit) in units.iter().enumerate() {
        if index > 0 {
            output.push(',');
        }
        write!(output, "{unit}").unwrap();
    }
    output.push('\n');
}

/// Send the actual generated typed value to Node's real JSON serializer.
fn witness(value: &JsonValue, output: &mut String) {
    match value {
        JsonValue::Null => output.push_str("n\n"),
        JsonValue::Boolean(true) => output.push_str("t\n"),
        JsonValue::Boolean(false) => output.push_str("f\n"),
        JsonValue::Number(number) => writeln!(output, "d{number}").unwrap(),
        JsonValue::String(units) => witness_string(units, output),
        JsonValue::Array(values) => {
            writeln!(output, "a{}", values.len()).unwrap();
            // retain original array order
            for value in values {
                witness(value, output);
            }
        }
        JsonValue::Object(fields) => {
            writeln!(output, "o{}", fields.len()).unwrap();
            // retain original property order
            for (key, value) in fields {
                witness_string(key, output);
                witness(value, output);
            }
        }
    }
}

/// Reuse Node's native JSON codec on actual typed fixture outputs.
fn encode(value: &JsonValue) -> Vec<u8> {
    let mut input = String::new();
    witness(value, &mut input);
    node(&["encode"], input.as_bytes())
}

struct Codec {
    input: JsonValue,
    approved_source: Vec<u8>,
}

impl PublicAdjustmentJsonCodec for Codec {
    /// Only the exact producer fixture is admitted by this test transport.
    fn decode(&self, bytes: &[u8]) -> Option<JsonValue> {
        (bytes == self.approved_source).then(|| self.input.clone())
    }

    /// Use Node to independently encode the typed Rust output.
    fn encode(&self, value: &JsonValue) -> Option<Vec<u8>> {
        Some(encode(value))
    }
}

/// Build one ordinary declared string field.
fn text(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}

/// Build one compact ordered fixture from source shape aliases.
fn object<const N: usize>(fields: [(&str, JsonValue); N]) -> JsonValue {
    JsonValue::Object(
        fields
            .into_iter()
            .map(
                // preserve the producer's original spelling and insertion order
                |(key, value)| (key.encode_utf16().collect(), value),
            )
            .collect(),
    )
}

/// Exercise wind/temperature/rain independence with independent metadata and raw metrics.
fn forecast_input() -> JsonValue {
    let metrics = || {
        JsonValue::Array(
            [
                "windSpeedMps",
                "temperatureC",
                "relativeHumidityPercent",
                "windGustMps",
            ]
            .into_iter()
            .map(text)
            .collect(),
        )
    };
    object([
        (
            "adjustmentRuntime",
            object([
                ("state", text("active")),
                ("authorizationSha256", text("retained-generic-identity")),
                ("enabledMetrics", metrics()),
                ("loadedAt", text("2026-11-23T16:00:00.000Z")),
            ]),
        ),
        (
            "temperatureAdjustmentRuntime",
            object([
                ("state", text("active")),
                ("activeBundle", text("temperature-identity")),
                ("loadedAt", text("2026-11-23T16:10:00.000Z")),
            ]),
        ),
        (
            "rainAdjustmentRuntime",
            object([
                ("state", text("active")),
                ("activeBundle", text("rain-identity")),
                ("loadedAt", text("2026-11-23T16:20:00.000Z")),
            ]),
        ),
        (
            "data",
            JsonValue::Array(vec![object([
                ("id", text("901")),
                (
                    "adjustment",
                    object([
                        ("state", text("active")),
                        ("appliedMetrics", metrics()),
                        (
                            "adjustedMetrics",
                            object([
                                ("temperatureC", JsonValue::Number(12.0)),
                                ("relativeHumidityPercent", JsonValue::Number(80.0)),
                                ("windSpeedMps", JsonValue::Number(3.25)),
                            ]),
                        ),
                    ]),
                ),
                (
                    "temperatureAdjustment",
                    object([
                        ("state", text("active")),
                        ("rawBestMatchTemperatureC", JsonValue::Number(11.5)),
                        ("bundleSha256", text("temperature-identity")),
                        ("correctedTemperatureC", JsonValue::Number(12.75)),
                    ]),
                ),
                (
                    "rainAdjustment",
                    object([
                        ("state", text("active")),
                        ("bundleSha256", text("rain-identity")),
                        ("correctedPrecipitationMm", JsonValue::Number(3.0)),
                        ("rawBestMatchPrecipitationMm", JsonValue::Number(2.0)),
                    ]),
                ),
            ])]),
        ),
        ("otherSourceField", text("retained")),
    ])
}

/// Compare exact JSON bytes for each policy using the same source before and after a settings write.
#[test]
fn concrete_projector_matches_node_forecast_switches_and_one_request_snapshot() {
    let input = forecast_input();
    let source = encode(&input);
    let settings = Mutex::new(InMemoryForecastAdjustmentSettingsStore::configured_first_read());
    let projector = PublicAdjustmentProjector::new(
        &settings,
        Codec {
            input,
            approved_source: source.clone(),
        },
    );
    // take each supported snapshot before an independent admin configuration update
    for (switches, snapshot) in [
        (
            "011",
            ForecastAdjustmentSettings {
                version: 1,
                temperature: false,
                wind: true,
                rain: true,
            },
        ),
        (
            "001",
            ForecastAdjustmentSettings {
                version: 1,
                temperature: false,
                wind: false,
                rain: true,
            },
        ),
        (
            "000",
            ForecastAdjustmentSettings {
                version: 1,
                temperature: false,
                wind: false,
                rain: false,
            },
        ),
    ] {
        settings
            .lock()
            .unwrap()
            .replace_atomically(snapshot)
            .unwrap();
        let captured = projector.prepare(PublicApiProjection::Forecast).unwrap();
        settings
            .lock()
            .unwrap()
            .replace_atomically(DEFAULT_FORECAST_ADJUSTMENT_SETTINGS)
            .unwrap();
        let rust = projector
            .project(PublicApiProjection::Forecast, &captured, &source)
            .unwrap();
        let node = node(&["project", "forecast", switches], &source);
        assert_eq!(rust, node, "{switches}");
    }
}

/// Keep the independent rain collection aggregate and its original context.
#[test]
fn concrete_projector_matches_node_rain_status_with_model_switch_off() {
    let input = object([
        ("generatedAt", text("2026-11-23T16:00:00.000Z")),
        (
            "data",
            object([
                ("modelEnabled", JsonValue::Boolean(true)),
                ("claims", JsonValue::Number(33.0)),
            ]),
        ),
    ]);
    let source = encode(&input);
    let settings = Mutex::new(InMemoryForecastAdjustmentSettingsStore::configured_first_read());
    settings
        .lock()
        .unwrap()
        .replace_atomically(ForecastAdjustmentSettings {
            version: 1,
            temperature: true,
            wind: true,
            rain: false,
        })
        .unwrap();
    let projector = PublicAdjustmentProjector::new(
        &settings,
        Codec {
            input,
            approved_source: source.clone(),
        },
    );
    let snapshot = projector
        .prepare(PublicApiProjection::RainCollection)
        .unwrap();
    assert_eq!(
        projector
            .project(PublicApiProjection::RainCollection, &snapshot, &source)
            .unwrap(),
        node(&["project", "rain", "110"], &source),
    );
}
