use std::path::PathBuf;
use std::process::Command;

use weather_adjustment::{
    AdmittedTemperatureRecentErrorForecast, AdmittedTemperatureRecentErrorHour,
    AdmittedTemperatureRecentErrorInput, AdmittedTemperatureRecentErrorStation,
    build_admitted_temperature_recent_error_state,
};
use weather_contracts::{
    EcmwfTemperatureCanaryRecentErrorState, EcmwfTemperatureCanaryRecentErrorStateCohortLiteral,
    ForecastObservationStationKey, Nullable,
};

struct Oracle {
    state: EcmwfTemperatureCanaryRecentErrorState,
    slots: Vec<(String, String, String)>,
    target_epoch_ms: i64,
    window_end_epoch_ms: i64,
}

struct FixtureHour {
    age: u8,
    forecast: Option<FixtureForecast>,
    local_date: String,
    stations: Vec<AdmittedTemperatureRecentErrorStation>,
    valid_at: String,
}

struct FixtureForecast {
    key: String,
    raw_temperature_c: f64,
    run_initialized_at: String,
}

/// Compare supported, insufficient, clipped, and empty states to the compiled worker.
#[test]
fn matches_compiled_causal_temperature_recent_error_state() {
    // cover every source support disposition without repository or clock I/O
    for name in ["supported", "insufficient", "clipped", "no-evidence"] {
        let oracle = node_oracle(name);
        let fixture = fixture(name, &oracle);
        let hours = fixture
            .iter()
            .map(|hour| AdmittedTemperatureRecentErrorHour {
                age_hours: hour.age,
                forecast: hour.forecast.as_ref().map(|forecast| {
                    AdmittedTemperatureRecentErrorForecast {
                        key: &forecast.key,
                        raw_temperature_c: forecast.raw_temperature_c,
                        run_initialized_at: &forecast.run_initialized_at,
                    }
                }),
                local_date: &hour.local_date,
                stations: &hour.stations,
                valid_at: &hour.valid_at,
                valid_at_epoch_ms: oracle.window_end_epoch_ms - i64::from(hour.age) * 3_600_000,
            })
            .collect::<Vec<_>>();
        let actual =
            build_admitted_temperature_recent_error_state(AdmittedTemperatureRecentErrorInput {
                hours: &hours,
                target_run_initialized_at: &oracle.state.targetRunInitializedAt,
                target_run_initialized_at_epoch_ms: oracle.target_epoch_ms,
                window_end_valid_at: &oracle.state.windowEndValidAt,
                window_end_valid_at_epoch_ms: oracle.window_end_epoch_ms,
            })
            .expect("admitted causal evidence must reduce");
        assert_eq!(actual, oracle.state, "{name}");
    }
}

/// Invoke the compiled worker reducer with a fake repository.
fn node_oracle(name: &str) -> Oracle {
    let script =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/temperature_recent_error_oracle.mjs");
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
    let mut lines = stdout.lines();
    let state = parse_state(lines.next().expect("state line"));
    let slots = lines
        .next()
        .expect("slot line")
        .split(',')
        .map(|slot| {
            let mut fields = slot.split('|');
            let valid_at = fields.next().expect("slot validAt");
            let local_date = fields.next().expect("slot local date");
            let run_initialized_at = fields.next().expect("slot source run");
            assert!(fields.next().is_none(), "slot field count");
            (
                valid_at.to_owned(),
                local_date.to_owned(),
                run_initialized_at.to_owned(),
            )
        })
        .collect();
    let (target, window) = lines
        .next()
        .expect("epoch line")
        .split_once('\t')
        .expect("epoch fields");
    assert!(lines.next().is_none(), "oracle line count");
    Oracle {
        state,
        slots,
        target_epoch_ms: target.parse().expect("target epoch"),
        window_end_epoch_ms: window.parse().expect("window epoch"),
    }
}

/// Decode one canonical state from the oracle's tab record.
fn parse_state(line: &str) -> EcmwfTemperatureCanaryRecentErrorState {
    let fields = line.split('\t').collect::<Vec<_>>();
    assert_eq!(fields.len(), 12, "state field count");
    EcmwfTemperatureCanaryRecentErrorState {
        b24C: parse_nullable_f64(fields[0]),
        b72C: parse_nullable_f64(fields[1]),
        cohort: EcmwfTemperatureCanaryRecentErrorStateCohortLiteral,
        localDates: fields[2].parse().expect("local date count"),
        mad72C: parse_nullable_f64(fields[3]),
        maximumSourceRunInitializedAt: parse_nullable_string(fields[4]),
        maximumSourceValidAt: parse_nullable_string(fields[5]),
        n24: fields[6].parse().expect("short count"),
        n72: fields[7].parse().expect("long count"),
        sourceKeys: if fields[8].is_empty() {
            Vec::new()
        } else {
            fields[8].split(',').map(str::to_owned).collect()
        },
        supported: fields[9] == "1",
        targetRunInitializedAt: fields[10].to_owned(),
        windowEndValidAt: fields[11].to_owned(),
    }
}

/// Decode one exact optional binary64.
fn parse_nullable_f64(value: &str) -> Nullable<f64> {
    if value == "null" {
        Nullable::Null
    } else {
        Nullable::Value(f64::from_bits(
            u64::from_str_radix(value, 16).expect("binary64 bits"),
        ))
    }
}

/// Decode one optional source timestamp.
fn parse_nullable_string(value: &str) -> Nullable<String> {
    if value == "null" {
        Nullable::Null
    } else {
        Nullable::Value(value.to_owned())
    }
}

/// Build one Rust-side admitted corpus from the oracle's calendar boundary.
fn fixture(name: &str, oracle: &Oracle) -> Vec<FixtureHour> {
    oracle
        .slots
        .iter()
        .enumerate()
        .map(
            |(age, (valid_at, local_date, run_initialized_at))| FixtureHour {
                age: age as u8,
                forecast: forecast(name, age as u8, valid_at, run_initialized_at),
                local_date: local_date.clone(),
                stations: station_rows(name, age as u8),
                valid_at: valid_at.clone(),
            },
        )
        .collect()
}

/// Reproduce the oracle's causal forecast selection and numeric values.
fn forecast(
    name: &str,
    age: u8,
    valid_at: &str,
    run_initialized_at: &str,
) -> Option<FixtureForecast> {
    let present = match name {
        "supported" => age < 40 && age != 2,
        "insufficient" => age < 8,
        "clipped" => age < 24,
        "no-evidence" => true,
        _ => panic!("unknown fixture {name}"),
    };
    // omit the same source hours before building their identities
    if !present {
        return None;
    }
    Some(FixtureForecast {
        key: format!("{run_initialized_at}/{valid_at}/k-{age:02}"),
        raw_temperature_c: if name == "clipped" {
            0.0
        } else {
            10.0 + f64::from(age % 5) * 0.25
        },
        run_initialized_at: run_initialized_at.to_owned(),
    })
}

/// Reproduce the oracle's station coverage and missing-value cases.
fn station_rows(name: &str, age: u8) -> Vec<AdmittedTemperatureRecentErrorStation> {
    if name == "no-evidence" {
        return vec![
            station(ForecastObservationStationKey::AmbientMaxweather, 9.0),
            station(ForecastObservationStationKey::Tempest126537, 10.0),
            station(ForecastObservationStationKey::Tempest168853, 11.0),
        ];
    }
    // preserve the source's coverage gap before weighted normalization
    if name == "supported" && age.is_multiple_of(11) {
        return vec![
            station(ForecastObservationStationKey::BallydideanEcowitt, 12.0),
            station(ForecastObservationStationKey::Tempest64255, 13.0),
        ];
    }
    let center = if name == "clipped" {
        20.0
    } else {
        12.0 + (f64::from(age % 7) - 3.0) * 0.4
    };
    vec![
        station(ForecastObservationStationKey::AmbientMerlin, center - 2.0),
        station(ForecastObservationStationKey::BallydideanEcowitt, center),
        AdmittedTemperatureRecentErrorStation {
            physical_station_key: ForecastObservationStationKey::Tempest38270,
            temperature_c: if age.is_multiple_of(7) {
                Nullable::Null
            } else {
                Nullable::Value(center + 1.0)
            },
        },
        station(ForecastObservationStationKey::Tempest64255, center + 2.0),
    ]
}

/// Build one present admitted station value.
fn station(
    physical_station_key: ForecastObservationStationKey,
    temperature_c: f64,
) -> AdmittedTemperatureRecentErrorStation {
    AdmittedTemperatureRecentErrorStation {
        physical_station_key,
        temperature_c: Nullable::Value(temperature_c),
    }
}
