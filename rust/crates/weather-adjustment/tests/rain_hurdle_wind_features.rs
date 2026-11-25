use std::path::PathBuf;
use std::process::Command;

use weather_adjustment::{
    AdmittedRainHurdleWindCalendar, AdmittedRainHurdleWindFeatureInput,
    AdmittedRainHurdleWindStations, RainHurdleWindV1Model,
    build_admitted_rain_hurdle_wind_features,
};
use weather_contracts::{
    Nullable, RainHurdleWindPortableArtifactV1Heads,
    RainHurdleWindPortableArtifactV1HeadsNativeHead,
    RainHurdleWindPortableArtifactV1HeadsObjectiveEnum as RainHeadObjective,
    RainHurdleWindPortableArtifactV1Rules, RainWindForecastHour,
};

const FORECAST_HOUR_COUNT: usize = 48;
const STATION_COUNT: usize = 12;
const DECISION_DELAY_HOURS: usize = 8;

struct Oracle {
    calendar: AdmittedRainHurdleWindCalendar,
    features: [f32; 107],
    weights: [f64; STATION_COUNT],
}

struct Fixture {
    current: [RainWindForecastHour; FORECAST_HOUR_COUNT],
    model_lead_hours: u8,
    precipitation_by_lag: [[Nullable<f64>; STATION_COUNT]; 6],
    prior_12h: Option<[RainWindForecastHour; FORECAST_HOUR_COUNT]>,
    prior_6h: Option<[RainWindForecastHour; FORECAST_HOUR_COUNT]>,
    temperature_lag1: [Nullable<f64>; STATION_COUNT],
}

/// Compare every binary32 feature to the compiled production builder.
#[test]
fn matches_compiled_rain_hurdle_wind_features() {
    let heads = compact_heads();
    let rules = compact_rules();
    let scales = [1.0, 1.0, 1.0];
    let model = RainHurdleWindV1Model::new(&heads, &rules, &scales)
        .expect("compact source-shaped model must validate");
    // cover complete, missing, station-gate, and maximum trained-lead rows
    for name in ["complete", "missing", "no-near", "late"] {
        let oracle = node_oracle(name);
        let fixture = fixture(name);
        let lag_refs = std::array::from_fn(|index| &fixture.precipitation_by_lag[index]);
        let input = AdmittedRainHurdleWindFeatureInput {
            calendar: oracle.calendar,
            current_hours: &fixture.current,
            model_lead_hours: fixture.model_lead_hours,
            prior_12h_hours: fixture.prior_12h.as_ref(),
            prior_6h_hours: fixture.prior_6h.as_ref(),
            stations: AdmittedRainHurdleWindStations {
                precipitation_by_lag: lag_refs,
                station_weights: &oracle.weights,
                temperature_lag1: &fixture.temperature_lag1,
            },
        };
        let actual = build_admitted_rain_hurdle_wind_features(input)
            .expect("admitted feature fixture must build");
        let (composed_features, composed_performance) = model
            .build_and_predict_admitted(input)
            .expect("admitted feature fixture must compose");
        // retain exact missing branches and Float32Array rounding at every index
        for (index, value) in actual.iter().enumerate() {
            assert_eq!(
                value.to_bits(),
                oracle.features[index].to_bits(),
                "{name} feature {index}",
            );
            assert_eq!(
                composed_features[index].to_bits(),
                value.to_bits(),
                "{name} retained feature {index}",
            );
        }
        assert_eq!(
            composed_performance,
            model
                .predict(&actual)
                .expect("built feature row must predict"),
            "{name} admitted composition",
        );
    }
}

/// Build compact leaf-only heads for the feature-to-model composition assertion.
fn compact_heads() -> RainHurdleWindPortableArtifactV1Heads {
    RainHurdleWindPortableArtifactV1Heads {
        atLeast0_1: leaf_head(RainHeadObjective::BinaryLogistic, 0.5, 0.2),
        atLeast1_0: leaf_head(RainHeadObjective::BinaryLogistic, 0.5, -0.2),
        atLeast2_5: leaf_head(RainHeadObjective::BinaryLogistic, 0.5, -0.4),
        amount: leaf_head(RainHeadObjective::RegGamma, 2.0, 0.1),
    }
}

/// Build exact source thresholds for the compact composition model.
fn compact_rules() -> [RainHurdleWindPortableArtifactV1Rules; 3] {
    [
        RainHurdleWindPortableArtifactV1Rules {
            threshold: 0.1,
            cutoff: Nullable::Value(0.5),
        },
        RainHurdleWindPortableArtifactV1Rules {
            threshold: 1.0,
            cutoff: Nullable::Value(0.5),
        },
        RainHurdleWindPortableArtifactV1Rules {
            threshold: 2.5,
            cutoff: Nullable::Null,
        },
    ]
}

/// Build one valid single-leaf portable tree head.
fn leaf_head(
    objective: RainHeadObjective,
    base_score: f64,
    leaf: f64,
) -> RainHurdleWindPortableArtifactV1HeadsNativeHead {
    RainHurdleWindPortableArtifactV1HeadsNativeHead {
        objective,
        baseScore: base_score,
        trees: vec![[vec![0.0], vec![leaf], vec![-1.0], vec![-1.0], vec![0.0]]],
    }
}

/// Invoke the extracted compiled feature owner and decode exact values.
fn node_oracle(name: &str) -> Oracle {
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/rain_hurdle_wind_features_oracle.mjs");
    let output = Command::new("node")
        .arg(script)
        .arg(name)
        .output()
        .expect("Node oracle must start");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("Node oracle must emit UTF-8");
    let mut fields = stdout.split('\t');
    let features = parse_f32(fields.next().expect("feature field"));
    let weights = parse_f64(fields.next().expect("weight field"));
    let calendar = parse_f32(fields.next().expect("calendar field"));
    assert!(fields.next().is_none(), "oracle field count");
    Oracle {
        calendar: AdmittedRainHurdleWindCalendar {
            annual_sin: f64::from(calendar[0]),
            annual_cos: f64::from(calendar[1]),
            daily_sin: f64::from(calendar[2]),
            daily_cos: f64::from(calendar[3]),
        },
        features: features.try_into().expect("107 features"),
        weights: weights.try_into().expect("12 station weights"),
    }
}

/// Decode exact binary32 values from the oracle.
fn parse_f32(field: &str) -> Vec<f32> {
    field
        .split(',')
        .map(|bits| f32::from_bits(u32::from_str_radix(bits, 16).expect("binary32 bits")))
        .collect()
}

/// Decode exact binary64 values from the oracle.
fn parse_f64(field: &str) -> Vec<f64> {
    field
        .split(',')
        .map(|bits| f64::from_bits(u64::from_str_radix(bits, 16).expect("binary64 bits")))
        .collect()
}

/// Build the Rust-side typed equivalent of one Node fixture.
fn fixture(name: &str) -> Fixture {
    let model_lead_hours = if name == "late" { 23 } else { 5 };
    let lead = DECISION_DELAY_HOURS + usize::from(model_lead_hours);
    let mut current = forecast_hours(0.0);
    let prior_6h = Some(forecast_hours(0.3));
    let mut prior_12h = Some(forecast_hours(0.6));
    let mut precipitation_by_lag = std::array::from_fn(|lag_index| {
        let lag = [1.0, 2.0, 3.0, 6.0, 12.0, 24.0][lag_index];
        std::array::from_fn(|index| Nullable::Value(lag * 0.03 + index as f64 * 0.02))
    });
    let mut temperature_lag1 =
        std::array::from_fn(|index| Nullable::Value(7.0 + (index % 4) as f64));

    // reproduce only the missing dimensions selected by each source fixture
    if name == "missing" {
        current[lead - 4].precipitationMm = Nullable::Null;
        current[lead - 7].pressureHpa = Nullable::Null;
        current[lead - 4].windDirectionDegrees = Nullable::Null;
        prior_12h = None;
        // remove alternating station members with the same per-lag phase
        for (lag_index, lag_values) in precipitation_by_lag.iter_mut().enumerate() {
            for index in (lag_index % 2..STATION_COUNT).step_by(2) {
                lag_values[index] = Nullable::Null;
                // lag one owns the temperature window too
                if lag_index == 0 {
                    temperature_lag1[index] = Nullable::Null;
                }
            }
        }
    } else if name == "no-near" {
        // the source map loses both rain and temperature for these gauges
        for index in 0..3 {
            precipitation_by_lag[0][index] = Nullable::Null;
            temperature_lag1[index] = Nullable::Null;
        }
    }
    Fixture {
        current,
        model_lead_hours,
        precipitation_by_lag,
        prior_12h,
        prior_6h,
        temperature_lag1,
    }
}

/// Build one exact 48-hour generated forecast profile.
fn forecast_hours(precipitation_offset: f64) -> [RainWindForecastHour; FORECAST_HOUR_COUNT] {
    std::array::from_fn(|index| {
        let lead = (index + 1) as f64;
        RainWindForecastHour {
            leadHours: lead,
            precipitationMm: Nullable::Value(lead * 0.1 + precipitation_offset),
            temperatureC: Nullable::Value(8.0 + lead * 0.2),
            relativeHumidityPercent: Nullable::Value(45.0 + lead * 0.5),
            cloudCoverPercent: Nullable::Value((lead * 7.0) % 100.0),
            pressureHpa: Nullable::Value(995.0 + lead * 0.6),
            windSpeedMps: Nullable::Value(1.5 + lead * 0.12),
            windDirectionDegrees: Nullable::Value((lead * 17.0) % 360.0),
        }
    })
}
