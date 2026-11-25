use std::path::PathBuf;
use std::process::Command;

use weather_contracts::*;
use weather_edge::{
    CanonicalTimeAdapter, ForecastMapSite, WidgetForecastAdjustmentDeadline,
    WidgetForecastCanonicalInstant, WidgetForecastIndependentSourceEpochs,
    WidgetForecastProjectionError, WidgetForecastSourceEpochs, WidgetForecastV1Epochs,
    WidgetForecastV3Boundary, WidgetForecastV3CalendarBoundary, WidgetForecastV3Record,
    api_cache_control, api_request_timeout_ms, build_widget_forecast_v3, forecast_map_preload_link,
    project_widget_forecast_v1_hour, project_widget_forecast_v2_hour,
    summarize_widget_forecast_v2_status,
};

const GENERATED_AT_EPOCH_MS: i64 = 1_789_226_100_000;
const RECEIVED_AT_EPOCH_MS: i64 = 1_789_225_200_000;
const RUN_AT_EPOCH_MS: i64 = 1_789_214_400_000;
const SELECTED_RECEIVED_AT_EPOCH_MS: i64 = 1_789_224_600_000;
const V3_DAY_START_EPOCH_MS: i64 = 1_789_196_400_000;
const V3_RECEIVED_AT_EPOCH_MS: i64 = 1_789_226_101_000;
const V3_PRODUCT_RUN_AT_EPOCH_MS: i64 = 1_789_204_500_000;
const V3_SOURCE_RECEIVED_AT_EPOCH_MS: i64 = 1_789_225_800_000;
const V3_SUNSET_EPOCH_MS: i64 = 1_789_266_469_262;
const V3_HOURLY_INSTANTS: [&str; 32] = [
    "2026-09-12T07:00:00.000Z",
    "2026-09-12T08:00:00.000Z",
    "2026-09-12T09:00:00.000Z",
    "2026-09-12T10:00:00.000Z",
    "2026-09-12T11:00:00.000Z",
    "2026-09-12T12:00:00.000Z",
    "2026-09-12T13:00:00.000Z",
    "2026-09-12T14:00:00.000Z",
    "2026-09-12T15:00:00.000Z",
    "2026-09-12T16:00:00.000Z",
    "2026-09-12T17:00:00.000Z",
    "2026-09-12T18:00:00.000Z",
    "2026-09-12T19:00:00.000Z",
    "2026-09-12T20:00:00.000Z",
    "2026-09-12T21:00:00.000Z",
    "2026-09-12T22:00:00.000Z",
    "2026-09-12T23:00:00.000Z",
    "2026-09-13T00:00:00.000Z",
    "2026-09-13T01:00:00.000Z",
    "2026-09-13T02:00:00.000Z",
    "2026-09-13T03:00:00.000Z",
    "2026-09-13T04:00:00.000Z",
    "2026-09-13T05:00:00.000Z",
    "2026-09-13T06:00:00.000Z",
    "2026-09-13T07:00:00.000Z",
    "2026-09-13T08:00:00.000Z",
    "2026-09-13T09:00:00.000Z",
    "2026-09-13T10:00:00.000Z",
    "2026-09-13T11:00:00.000Z",
    "2026-09-13T12:00:00.000Z",
    "2026-09-13T13:00:00.000Z",
    "2026-09-13T14:00:00.000Z",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WidgetV1FixtureBranch {
    Raw,
    GenericTemperature,
    IndependentTemperature,
    Rain,
    Missing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WidgetV1FixtureOrdering {
    Valid,
    RawFuture,
    SelectedFuture,
    SelectedIncomplete,
}

#[derive(Debug, Clone, Copy)]
struct WidgetV1Fixture {
    rain_rate: Option<f64>,
    rain_amount: Option<f64>,
    temperature: Option<f64>,
    branch: WidgetV1FixtureBranch,
    selected: Option<f64>,
    deadline: &'static str,
    ordering: WidgetV1FixtureOrdering,
}

struct FixedCanonicalTimes;

impl CanonicalTimeAdapter for FixedCanonicalTimes {
    /// Return the fixed normalized provider timestamp paired with each frame.
    fn xweather_valid_time(&self, epoch_ms: i64) -> String {
        // expose only the two frame pairs exercised by this parity fixture
        match epoch_ms {
            1_798_803_600_000 => "20270101114000".into(),
            1_798_804_200_000 => "20270101115000".into(),
            _ => panic!("unexpected Xweather frame {epoch_ms}"),
        }
    }

    /// Return the fixed normalized UTC timestamp paired with each deadline.
    fn utc_iso(&self, epoch_ms: i64) -> String {
        // expose the fixed non-hour pairs exercised by parity fixtures
        match epoch_ms {
            GENERATED_AT_EPOCH_MS => "2026-09-12T15:15:00.000Z".into(),
            V3_RECEIVED_AT_EPOCH_MS => "2026-09-12T15:15:01.000Z".into(),
            1_789_226_099_999 => "2026-09-12T15:14:59.999Z".into(),
            V3_PRODUCT_RUN_AT_EPOCH_MS => "2026-09-12T09:15:00.000Z".into(),
            V3_SOURCE_RECEIVED_AT_EPOCH_MS => "2026-09-12T15:10:00.000Z".into(),
            1_789_231_500_000 => "2026-09-12T16:45:00.000Z".into(),
            V3_SUNSET_EPOCH_MS => "2026-09-13T02:27:49.262Z".into(),
            _ => {
                let offset = epoch_ms - V3_DAY_START_EPOCH_MS;
                // expose only the fixed contiguous v3 hourly pair table
                if offset >= 0 && offset % 3_600_000 == 0 {
                    let index = usize::try_from(offset / 3_600_000)
                        .expect("fixed v3 hour index must fit usize");
                    if let Some(value) = V3_HOURLY_INSTANTS.get(index) {
                        return (*value).into();
                    }
                }
                panic!("unexpected widget deadline {epoch_ms}")
            }
        }
    }
}

/// Invoke the extracted live Node function for one parity fixture.
fn node_oracle(arguments: &[&str]) -> String {
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/web_server_oracle.mjs");
    let output = Command::new("node")
        .arg(script)
        .args(arguments)
        .output()
        .expect("Node oracle must start");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("Node oracle must emit UTF-8")
}

/// Prove every retained public API cache and timeout class against Node.
#[test]
fn matches_api_cache_and_timeout_projection() {
    let paths = [
        "/api/v1/sites",
        "/api/v1/sites/ballydidean/current",
        "/api/v1/sites/ballydidean/forecast",
        "/api/v1/sites/ballydidean/tides",
        "/api/v1/sites/ballydidean/trends",
        "/api/v1/sites/ballydidean/history",
        "/api/v1/sites//current",
        "/api/v1/sites/ballydidean/current/extra",
        "/unknown",
    ];
    // compare each route to the exact functions extracted from web-server.mjs
    for pathname in paths {
        let actual = format!(
            "{}\t{}",
            api_cache_control(pathname),
            api_request_timeout_ms(pathname),
        );
        assert_eq!(actual, node_oracle(&["api", pathname]), "{pathname}");
    }
}

/// Prove forecast-only preload projection and ten-minute clock flooring against Node.
#[test]
fn matches_forecast_map_preload_projection() {
    let site = ForecastMapSite {
        latitude: 47.950_429_954_185_445,
        longitude: -122.427_970_126_081_93,
    };
    let cases = [
        (
            "/forecast",
            1_798_803_719_999_i64,
            "1798803600000",
            "20270101114000",
        ),
        (
            "/forecast/",
            1_798_804_200_001_i64,
            "1798804200000",
            "20270101115000",
        ),
        (
            "/",
            1_798_803_719_999_i64,
            "1798803600000",
            "20270101114000",
        ),
    ];
    // compare deterministic path and time cases to the actual Node projection
    for (pathname, epoch_ms, frame_epoch, valid_time) in cases {
        let actual =
            forecast_map_preload_link(pathname, Some(site), epoch_ms, &FixedCanonicalTimes);
        let epoch = epoch_ms.to_string();
        assert_eq!(
            actual,
            node_oracle(&[
                "preload",
                pathname,
                &epoch,
                frame_epoch,
                valid_time,
                "47.950429954185445",
                "-122.42797012608193",
            ]),
        );
    }
    assert_eq!(
        forecast_map_preload_link("/forecast", None, 0, &FixedCanonicalTimes),
        ""
    );
}

/// Prove complete widget v1 rain and temperature projection against TypeScript.
#[test]
fn matches_widget_v1_hour_projection() {
    let cases = [
        WidgetV1Fixture {
            rain_rate: Some(1.0),
            rain_amount: Some(2.0),
            temperature: Some(10.0),
            branch: WidgetV1FixtureBranch::Raw,
            selected: None,
            deadline: "permanent",
            ordering: WidgetV1FixtureOrdering::Valid,
        },
        WidgetV1Fixture {
            rain_rate: None,
            rain_amount: Some(2.0),
            temperature: None,
            branch: WidgetV1FixtureBranch::Raw,
            selected: None,
            deadline: "permanent",
            ordering: WidgetV1FixtureOrdering::Valid,
        },
        WidgetV1Fixture {
            rain_rate: None,
            rain_amount: None,
            temperature: None,
            branch: WidgetV1FixtureBranch::Missing,
            selected: None,
            deadline: "permanent",
            ordering: WidgetV1FixtureOrdering::Valid,
        },
        WidgetV1Fixture {
            rain_rate: Some(1.0),
            rain_amount: Some(2.0),
            temperature: Some(10.0),
            branch: WidgetV1FixtureBranch::GenericTemperature,
            selected: Some(12.0),
            deadline: "permanent",
            ordering: WidgetV1FixtureOrdering::Valid,
        },
        WidgetV1Fixture {
            rain_rate: Some(1.0),
            rain_amount: Some(2.0),
            temperature: Some(10.0),
            branch: WidgetV1FixtureBranch::GenericTemperature,
            selected: Some(12.0),
            deadline: "unavailable",
            ordering: WidgetV1FixtureOrdering::Valid,
        },
        WidgetV1Fixture {
            rain_rate: Some(1.0),
            rain_amount: Some(2.0),
            temperature: Some(10.0),
            branch: WidgetV1FixtureBranch::GenericTemperature,
            selected: Some(12.0),
            deadline: "2026-09-12T15:15:00.000Z",
            ordering: WidgetV1FixtureOrdering::Valid,
        },
        WidgetV1Fixture {
            rain_rate: Some(1.0),
            rain_amount: Some(2.0),
            temperature: None,
            branch: WidgetV1FixtureBranch::GenericTemperature,
            selected: Some(12.0),
            deadline: "permanent",
            ordering: WidgetV1FixtureOrdering::Valid,
        },
        WidgetV1Fixture {
            rain_rate: Some(1.0),
            rain_amount: Some(2.0),
            temperature: Some(10.0),
            branch: WidgetV1FixtureBranch::IndependentTemperature,
            selected: Some(12.0),
            deadline: "permanent",
            ordering: WidgetV1FixtureOrdering::Valid,
        },
        WidgetV1Fixture {
            rain_rate: Some(1.0),
            rain_amount: Some(2.0),
            temperature: Some(10.0),
            branch: WidgetV1FixtureBranch::Rain,
            selected: Some(0.5),
            deadline: "2026-09-12T15:00:00.000Z",
            ordering: WidgetV1FixtureOrdering::Valid,
        },
        WidgetV1Fixture {
            rain_rate: Some(1.0),
            rain_amount: Some(2.0),
            temperature: Some(10.0),
            branch: WidgetV1FixtureBranch::Rain,
            selected: Some(0.5),
            deadline: "unavailable",
            ordering: WidgetV1FixtureOrdering::Valid,
        },
        WidgetV1Fixture {
            rain_rate: Some(1.0),
            rain_amount: Some(2.0),
            temperature: Some(10.0),
            branch: WidgetV1FixtureBranch::Rain,
            selected: Some(0.5),
            deadline: "2026-09-12T02:00:00.000Z",
            ordering: WidgetV1FixtureOrdering::Valid,
        },
    ];

    // compare raw fallback absent adjusted and independent branches
    for fixture in cases {
        assert_widget_v1_parity(fixture);
    }
}

/// Prove v1 physical and time-order failures against the TypeScript diagnostics.
#[test]
fn matches_widget_v1_projection_failures() {
    let cases = [
        WidgetV1Fixture {
            rain_rate: Some(1.0),
            rain_amount: Some(2_001.0),
            temperature: Some(10.0),
            branch: WidgetV1FixtureBranch::Raw,
            selected: None,
            deadline: "permanent",
            ordering: WidgetV1FixtureOrdering::Valid,
        },
        WidgetV1Fixture {
            rain_rate: Some(1.0),
            rain_amount: Some(2.0),
            temperature: Some(71.0),
            branch: WidgetV1FixtureBranch::Raw,
            selected: None,
            deadline: "permanent",
            ordering: WidgetV1FixtureOrdering::Valid,
        },
        WidgetV1Fixture {
            rain_rate: Some(1.0),
            rain_amount: Some(2.0),
            temperature: Some(10.0),
            branch: WidgetV1FixtureBranch::IndependentTemperature,
            selected: Some(71.0),
            deadline: "permanent",
            ordering: WidgetV1FixtureOrdering::Valid,
        },
        WidgetV1Fixture {
            rain_rate: Some(1.0),
            rain_amount: Some(2.0),
            temperature: Some(10.0),
            branch: WidgetV1FixtureBranch::Raw,
            selected: None,
            deadline: "permanent",
            ordering: WidgetV1FixtureOrdering::RawFuture,
        },
        WidgetV1Fixture {
            rain_rate: Some(1.0),
            rain_amount: Some(2.0),
            temperature: Some(10.0),
            branch: WidgetV1FixtureBranch::IndependentTemperature,
            selected: Some(12.0),
            deadline: "permanent",
            ordering: WidgetV1FixtureOrdering::SelectedFuture,
        },
        WidgetV1Fixture {
            rain_rate: Some(1.0),
            rain_amount: Some(2.0),
            temperature: Some(10.0),
            branch: WidgetV1FixtureBranch::IndependentTemperature,
            selected: Some(12.0),
            deadline: "permanent",
            ordering: WidgetV1FixtureOrdering::SelectedIncomplete,
        },
        WidgetV1Fixture {
            rain_rate: Some(1.0),
            rain_amount: Some(2.0),
            temperature: Some(10.0),
            branch: WidgetV1FixtureBranch::Rain,
            selected: Some(0.5),
            deadline: "2026-09-12T15:16:00.000Z",
            ordering: WidgetV1FixtureOrdering::Valid,
        },
    ];

    // compare each closed failure before returning a public hour
    for fixture in cases {
        assert_widget_v1_parity(fixture);
    }
}

/// Prove the complete ordered widget v3 response against the compiled projector.
#[test]
fn matches_widget_v3_snapshot_projection() {
    let records = widget_v3_fixture_records("gap");
    let projected_records = widget_v3_projection_records(&records);
    let projected = build_widget_forecast_v3(
        widget_v3_boundary("gap"),
        &projected_records,
        &FixedCanonicalTimes,
    );
    assert_eq!(
        widget_v3_result_signature(projected),
        node_oracle(&["widget-v3-snapshot", "gap"]),
    );
}

/// Prove v3 grid and response-clock rejection parity against the compiled projector.
#[test]
fn matches_widget_v3_snapshot_failures() {
    // compare each complete-response rejection independently
    for mode in ["duplicate", "foreign", "future-generation"] {
        let records = widget_v3_fixture_records(mode);
        let projected_records = widget_v3_projection_records(&records);
        let projected = build_widget_forecast_v3(
            widget_v3_boundary(mode),
            &projected_records,
            &FixedCanonicalTimes,
        );
        assert_eq!(
            widget_v3_result_signature(projected),
            node_oracle(&["widget-v3-snapshot", mode]),
            "{mode}",
        );
    }
}

/// Prove typed cloud and wind reduction against the actual TypeScript projector.
#[test]
fn matches_widget_v2_hour_projection() {
    let cases = [
        (
            Some(20.0),
            Some(4.0),
            None,
            WidgetForecastAdjustmentDeadline::Permanent,
            ["20", "4", "none", "permanent"],
        ),
        (
            Some(20.0),
            Some(4.0),
            Some(7.0),
            WidgetForecastAdjustmentDeadline::Permanent,
            ["20", "4", "7", "permanent"],
        ),
        (
            Some(20.0),
            Some(4.0),
            Some(7.0),
            WidgetForecastAdjustmentDeadline::AtEpochMilliseconds(GENERATED_AT_EPOCH_MS),
            ["20", "4", "7", "2026-09-12T15:15:00.000Z"],
        ),
        (
            Some(20.0),
            Some(4.0),
            Some(7.0),
            WidgetForecastAdjustmentDeadline::Unavailable,
            ["20", "4", "7", "unavailable"],
        ),
        (
            None,
            None,
            None,
            WidgetForecastAdjustmentDeadline::Permanent,
            ["null", "null", "none", "permanent"],
        ),
        (
            Some(20.0),
            Some(4.0),
            Some(7.0),
            WidgetForecastAdjustmentDeadline::AtEpochMilliseconds(1_789_228_800_000),
            ["20", "4", "7", "2026-09-12T16:00:00.000Z"],
        ),
    ];
    // compare raw adjusted expired unavailable and bounded deadline cases
    for (cloud, wind, adjusted_wind, deadline, oracle_arguments) in cases {
        let record = forecast_record(cloud, wind, adjusted_wind);
        let projected = project_widget_forecast_v2_hour(
            base_hour(),
            Some(&record),
            source_epochs(),
            deadline,
            &FixedCanonicalTimes,
        )
        .expect("valid fixture must project");
        let actual = format!(
            "{}\n{}\n{}",
            field_signature(&projected.cloudCoverPercent),
            field_signature(&projected.windSpeedMps),
            status_wire_value(summarize_widget_forecast_v2_status(&[projected])),
        );
        assert_eq!(
            actual,
            node_oracle(&[
                "widget-v2-hour",
                oracle_arguments[0],
                oracle_arguments[1],
                oracle_arguments[2],
                oracle_arguments[3],
            ]),
        );
    }
}

/// Prove every homogeneous and empty v2 status result against TypeScript.
#[test]
fn matches_widget_v2_status_projection() {
    let cases = [
        (WidgetForecastMode::Adjusted, "adjusted"),
        (WidgetForecastMode::Raw, "raw"),
        (WidgetForecastMode::Unavailable, "unavailable"),
    ];
    // compare each homogeneous mode admitted by the generated contract
    for (mode, wire_value) in cases {
        let hour = status_hour(mode);
        assert_eq!(
            status_wire_value(summarize_widget_forecast_v2_status(&[hour])),
            node_oracle(&["widget-v2-status", wire_value]),
        );
    }
    assert_eq!(
        status_wire_value(summarize_widget_forecast_v2_status(&[])),
        node_oracle(&["widget-v2-status", "empty"]),
    );
}

/// Retain closed failures and missing-row behavior outside the happy path.
#[test]
fn rejects_invalid_widget_v2_inputs() {
    let invalid_cloud = forecast_record(Some(101.0), Some(4.0), None);
    assert_eq!(
        project_widget_forecast_v2_hour(
            base_hour(),
            Some(&invalid_cloud),
            source_epochs(),
            WidgetForecastAdjustmentDeadline::Permanent,
            &FixedCanonicalTimes,
        ),
        Err(WidgetForecastProjectionError::InvalidCloudCoverPercent),
    );

    let invalid_wind = forecast_record(Some(20.0), Some(151.0), None);
    assert_eq!(
        project_widget_forecast_v2_hour(
            base_hour(),
            Some(&invalid_wind),
            source_epochs(),
            WidgetForecastAdjustmentDeadline::Permanent,
            &FixedCanonicalTimes,
        ),
        Err(WidgetForecastProjectionError::InvalidWindSpeedMps),
    );

    let invalid_selected_wind = forecast_record(Some(20.0), Some(4.0), Some(151.0));
    assert_eq!(
        project_widget_forecast_v2_hour(
            base_hour(),
            Some(&invalid_selected_wind),
            source_epochs(),
            WidgetForecastAdjustmentDeadline::Permanent,
            &FixedCanonicalTimes,
        ),
        Err(WidgetForecastProjectionError::InvalidSelectedWindSpeedMps),
    );

    let record = forecast_record(Some(20.0), Some(4.0), None);
    let out_of_order = WidgetForecastSourceEpochs {
        generated_at_epoch_ms: GENERATED_AT_EPOCH_MS,
        received_at_epoch_ms: GENERATED_AT_EPOCH_MS + 1,
        product_run_at_epoch_ms: Some(RUN_AT_EPOCH_MS),
    };
    assert_eq!(
        project_widget_forecast_v2_hour(
            base_hour(),
            Some(&record),
            out_of_order,
            WidgetForecastAdjustmentDeadline::Permanent,
            &FixedCanonicalTimes,
        ),
        Err(WidgetForecastProjectionError::SourceTimestampsOutOfOrder),
    );

    let missing = project_widget_forecast_v2_hour(
        base_hour(),
        None,
        source_epochs(),
        WidgetForecastAdjustmentDeadline::Permanent,
        &FixedCanonicalTimes,
    )
    .expect("missing record must remain representable");
    assert_eq!(
        missing.cloudCoverPercent.mode,
        WidgetForecastMode::Unavailable
    );
    assert_eq!(missing.windSpeedMps.mode, WidgetForecastMode::Unavailable);
}

/// Build the complete canonical v3 clock and calendar boundary.
fn widget_v3_boundary(mode: &str) -> WidgetForecastV3Boundary {
    let received_at = if mode == "future-generation" {
        WidgetForecastCanonicalInstant {
            value: "2026-09-12T15:14:59.999Z".into(),
            epoch_ms: GENERATED_AT_EPOCH_MS - 1,
        }
    } else {
        WidgetForecastCanonicalInstant {
            value: "2026-09-12T15:15:01.000Z".into(),
            epoch_ms: V3_RECEIVED_AT_EPOCH_MS,
        }
    };
    WidgetForecastV3Boundary {
        generated_at: WidgetForecastCanonicalInstant {
            value: "2026-09-12T15:15:00.000Z".into(),
            epoch_ms: GENERATED_AT_EPOCH_MS,
        },
        received_at,
        calendar: WidgetForecastV3CalendarBoundary {
            date: "2026-09-12".into(),
            day_start: widget_v3_hour_instant(0),
            day_end: widget_v3_hour_instant(24),
            cutoff: widget_v3_hour_instant(20),
            anchor_start_epoch_ms: V3_DAY_START_EPOCH_MS + 7 * 3_600_000,
            overnight_end: widget_v3_hour_instant(31),
            sunset: Some(WidgetForecastCanonicalInstant {
                value: "2026-09-13T02:27:49.262Z".into(),
                epoch_ms: V3_SUNSET_EPOCH_MS,
            }),
        },
    }
}

/// Build source-derived canonical forecast rows for one v3 fixture mode.
fn widget_v3_fixture_records(mode: &str) -> Vec<(usize, ApiForecastWeatherRecord)> {
    let mut records = Vec::new();
    // retain every real hour except the one explicit gap fixture
    for (hour_index, valid_at) in V3_HOURLY_INSTANTS.iter().take(31).enumerate() {
        if mode == "gap" && hour_index == 27 {
            continue;
        }
        // repeat the next-date fixture's local hour values
        let local_index = if hour_index < 24 {
            hour_index
        } else {
            hour_index - 24
        };
        let cloud = 20.0 + local_index as f64;
        let wind = 4.0 + local_index as f64 / 10.0;
        let mut record = forecast_record(Some(cloud), Some(wind), Some(wind + 3.0));
        record.productRunAt = Nullable::Value("2026-09-12T09:15:00.000Z".into());
        record.receivedAt = "2026-09-12T15:10:00.000Z".into();
        record.validAt = (*valid_at).into();
        records.push((hour_index, record));
    }

    // add one exact duplicate row after the complete source grid
    if mode == "duplicate" {
        records.push(records[0].clone());
    }
    // move the final source row beyond the admitted overnight grid
    if mode == "foreign" {
        let final_record = records
            .last_mut()
            .expect("complete v3 fixture must contain a final row");
        final_record.0 = 37;
        final_record.1.validAt = "2026-09-13T20:00:00.000Z".into();
    }
    records
}

/// Bind fixture records to the typed v3 projection boundary.
fn widget_v3_projection_records<'a>(
    records: &'a [(usize, ApiForecastWeatherRecord)],
) -> Vec<WidgetForecastV3Record<'a>> {
    records
        .iter()
        .map(|(hour_index, record)| WidgetForecastV3Record {
            record,
            epochs: WidgetForecastV1Epochs {
                start_epoch_ms: V3_DAY_START_EPOCH_MS + *hour_index as i64 * 3_600_000,
                raw_source: WidgetForecastSourceEpochs {
                    generated_at_epoch_ms: GENERATED_AT_EPOCH_MS,
                    received_at_epoch_ms: V3_SOURCE_RECEIVED_AT_EPOCH_MS,
                    product_run_at_epoch_ms: Some(V3_PRODUCT_RUN_AT_EPOCH_MS),
                },
                rain_source: None,
                temperature_source: None,
                rain_decision_at_epoch_ms: None,
            },
            generic_deadline: WidgetForecastAdjustmentDeadline::Permanent,
            temperature_deadline: WidgetForecastAdjustmentDeadline::Unavailable,
        })
        .collect()
}

/// Build one fixed hourly canonical instant from the v3 pair table.
fn widget_v3_hour_instant(index: usize) -> WidgetForecastCanonicalInstant {
    WidgetForecastCanonicalInstant {
        value: V3_HOURLY_INSTANTS[index].into(),
        epoch_ms: V3_DAY_START_EPOCH_MS + index as i64 * 3_600_000,
    }
}

/// Flatten one complete v3 result into the Node oracle signature.
fn widget_v3_result_signature(
    result: Result<WidgetForecastV3Snapshot, WidgetForecastProjectionError>,
) -> String {
    let snapshot = match result {
        Ok(snapshot) => snapshot,
        Err(error) => return format!("error\t{}", widget_v3_error_message(error)),
    };
    let sunset = match &snapshot.calendar.sunset {
        Nullable::Value(value) => value.as_str(),
        Nullable::Null => "null",
    };
    let mut lines = vec![
        "ok".into(),
        WidgetForecastV3SnapshotSchemaVersionLiteral::WIRE_VALUE.into(),
        WidgetForecastSnapshotAttributionLabelLiteral::WIRE_VALUE.into(),
        WidgetForecastSnapshotAttributionLicenseUrlLiteral::WIRE_VALUE.into(),
        WidgetForecastSnapshotAttributionProviderUrlLiteral::WIRE_VALUE.into(),
        snapshot.calendar.date.clone(),
        snapshot.calendar.dayStart.clone(),
        snapshot.calendar.cutoff.clone(),
        snapshot.calendar.dayEnd.clone(),
        snapshot.calendar.overnightEnd.clone(),
        sunset.into(),
        snapshot.generatedAt.clone(),
        snapshot.receivedAt.clone(),
        WidgetForecastSnapshotSiteNameLiteral::WIRE_VALUE.into(),
        WidgetForecastSnapshotSiteSlugLiteral::WIRE_VALUE.into(),
        WidgetForecastSnapshotSiteTimezoneLiteral::WIRE_VALUE.into(),
        WidgetForecastSnapshotSiteLatitudeLiteral::WIRE_VALUE.to_string(),
        WidgetForecastSnapshotSiteLongitudeLiteral::WIRE_VALUE.to_string(),
        status_wire_value(snapshot.status).into(),
        snapshot.hours.len().to_string(),
    ];
    // retain every ordered hour and its four public fields
    for hour in &snapshot.hours {
        lines.push(hour.start.clone());
        lines.push(hour.end.clone());
        lines.push(field_signature(&hour.rainMmPerHour));
        lines.push(field_signature(&hour.temperatureC));
        lines.push(field_signature(&hour.cloudCoverPercent));
        lines.push(field_signature(&hour.windSpeedMps));
    }
    lines.join("\n")
}

/// Map one closed v3 build failure to the source projector diagnostic.
fn widget_v3_error_message(error: WidgetForecastProjectionError) -> &'static str {
    // expose only complete-response failures exercised by parity fixtures
    match error {
        WidgetForecastProjectionError::DuplicateOrForeignHour
        | WidgetForecastProjectionError::NonForecastHour => {
            "Widget forecast contains a duplicate or foreign hour"
        }
        WidgetForecastProjectionError::GeneratedAfterReceived => {
            "Widget forecast generatedAt is after receivedAt"
        }
        WidgetForecastProjectionError::GeneratedOutsideV3Window => {
            "Widget forecast v3 generatedAt is outside its calendar window"
        }
        _ => "unexpected v3 fixture error",
    }
}

/// Compare one Rust v1 fixture to the compiled TypeScript projector.
fn assert_widget_v1_parity(fixture: WidgetV1Fixture) {
    let record = (fixture.branch != WidgetV1FixtureBranch::Missing)
        .then(|| widget_v1_forecast_record(fixture));
    let projected = project_widget_forecast_v1_hour(
        record.as_ref(),
        "2026-09-12T15:00:00.000Z".into(),
        widget_v1_epochs(fixture),
        widget_v1_deadline(fixture.deadline),
        widget_v1_deadline(fixture.deadline),
        &FixedCanonicalTimes,
    );
    let actual = widget_v1_result_signature(projected);
    let arguments = [
        "widget-v1-hour".to_owned(),
        option_number_argument(fixture.rain_rate),
        option_number_argument(fixture.rain_amount),
        option_number_argument(fixture.temperature),
        widget_v1_branch_argument(fixture.branch).to_owned(),
        option_number_argument(fixture.selected),
        fixture.deadline.to_owned(),
        widget_v1_ordering_argument(fixture.ordering).to_owned(),
    ];
    let argument_refs = arguments.iter().map(String::as_str).collect::<Vec<_>>();
    assert_eq!(actual, node_oracle(&argument_refs), "{fixture:?}");
}

/// Build one canonical API row for a v1 parity fixture.
fn widget_v1_forecast_record(fixture: WidgetV1Fixture) -> ApiForecastWeatherRecord {
    let mut record = forecast_record(None, None, None);
    record.metrics.precipitationRateMmPerHour = nullable_number(fixture.rain_rate);
    record.metrics.precipitationMm = nullable_number(fixture.rain_amount);
    record.metrics.temperatureC = nullable_number(fixture.temperature);

    // mirror the raw source clock used by the Node fixture
    if fixture.ordering == WidgetV1FixtureOrdering::RawFuture {
        record.receivedAt = "2026-09-12T15:16:00.000Z".into();
    }

    // activate only the requested independent or generic branch
    match fixture.branch {
        WidgetV1FixtureBranch::GenericTemperature => {
            record.adjustment = generic_temperature_adjustment(fixture.selected);
        }
        WidgetV1FixtureBranch::IndependentTemperature => {
            record.temperatureAdjustment.state =
                ForecastTemperatureCanaryDecisionV1StateEnum::Active;
            record.temperatureAdjustment.correctedTemperatureC = nullable_number(fixture.selected);
            record.temperatureAdjustment.sourceForecast =
                widget_v1_temperature_source(fixture.ordering);
        }
        WidgetV1FixtureBranch::Rain => {
            record.rainAdjustment.state = ApiRainAdjustmentDecisionStateEnum::Active;
            record.rainAdjustment.correctedPrecipitationMm = nullable_number(fixture.selected);
            record.rainAdjustment.sourceForecast = widget_v1_rain_source(fixture);
        }
        WidgetV1FixtureBranch::Raw | WidgetV1FixtureBranch::Missing => {}
    }
    record
}

/// Build the complete prevalidated clock boundary for one v1 fixture.
fn widget_v1_epochs(fixture: WidgetV1Fixture) -> WidgetForecastV1Epochs {
    let selected_received_at_epoch_ms =
        if fixture.ordering == WidgetV1FixtureOrdering::SelectedFuture {
            GENERATED_AT_EPOCH_MS + 60_000
        } else {
            SELECTED_RECEIVED_AT_EPOCH_MS
        };
    let selected_source = (fixture.ordering != WidgetV1FixtureOrdering::SelectedIncomplete)
        .then_some(WidgetForecastIndependentSourceEpochs {
            received_at_epoch_ms: selected_received_at_epoch_ms,
            product_run_at_epoch_ms: Some(RUN_AT_EPOCH_MS),
        });
    let raw_received_at_epoch_ms = if fixture.ordering == WidgetV1FixtureOrdering::RawFuture {
        GENERATED_AT_EPOCH_MS + 60_000
    } else {
        RECEIVED_AT_EPOCH_MS
    };
    WidgetForecastV1Epochs {
        start_epoch_ms: RECEIVED_AT_EPOCH_MS,
        raw_source: WidgetForecastSourceEpochs {
            generated_at_epoch_ms: GENERATED_AT_EPOCH_MS,
            received_at_epoch_ms: raw_received_at_epoch_ms,
            product_run_at_epoch_ms: Some(RUN_AT_EPOCH_MS),
        },
        rain_source: selected_source,
        temperature_source: selected_source,
        rain_decision_at_epoch_ms: match fixture.deadline {
            "2026-09-12T15:00:00.000Z" => Some(RECEIVED_AT_EPOCH_MS),
            "2026-09-12T02:00:00.000Z" => Some(1_789_178_400_000),
            "2026-09-12T15:16:00.000Z" => Some(GENERATED_AT_EPOCH_MS + 60_000),
            _ => None,
        },
    }
}

/// Build one independent temperature source with fixed normalized clocks.
fn widget_v1_temperature_source(
    ordering: WidgetV1FixtureOrdering,
) -> Nullable<ForecastTemperatureCanaryDecisionV1SourceForecast> {
    // retain the selected-source null case independently from its clock pair
    if ordering == WidgetV1FixtureOrdering::SelectedIncomplete {
        return Nullable::Null;
    }
    Nullable::Value(ForecastTemperatureCanaryDecisionV1SourceForecast {
        adapterVersion: "fixture".into(),
        dataset: TemperatureCanarySourceForecastDatasetLiteral,
        firstReceivedAt: widget_v1_selected_received_at(ordering).into(),
        modelCycle: TemperatureCanarySourceForecastModelCycleEnum::Value50r1,
        modelLeadHours: 1,
        providerKey: TemperatureCanarySourceForecastProviderKeyLiteral,
        providerResponseSha256: "provider".into(),
        rawRelativeHumidityPercent: Nullable::Null,
        rawTemperatureC: 10.0,
        rawWindSpeedMps: Nullable::Null,
        runInitializedAt: "2026-09-12T12:00:00.000Z".into(),
        upstreamModel: TemperatureCanarySourceForecastUpstreamModelLiteral,
        validAt: "2026-09-12T15:00:00.000Z".into(),
        operationalHorizonHours: 1,
    })
}

/// Build one independent rain source with fixed normalized clocks.
fn widget_v1_rain_source(
    fixture: WidgetV1Fixture,
) -> Nullable<ApiRainAdjustmentDecisionSourceForecast> {
    // retain the selected-source null case independently from its decision clock
    if fixture.ordering == WidgetV1FixtureOrdering::SelectedIncomplete {
        return Nullable::Null;
    }
    Nullable::Value(ApiRainAdjustmentDecisionSourceForecast {
        runInitializedAt: "2026-09-12T12:00:00.000Z".into(),
        firstReceivedAt: widget_v1_selected_received_at(fixture.ordering).into(),
        validAt: "2026-09-12T15:00:00.000Z".into(),
        rawPrecipitationMm: 1.0,
        modelLeadHours: 1,
        decisionAt: fixture.deadline.into(),
        upstreamModel: ApiRainAdjustmentDecisionSourceForecastUpstreamModelLiteral,
        providerKey: ApiRainAdjustmentDecisionSourceForecastProviderKeyLiteral,
    })
}

/// Select the fixed normalized independent receipt string.
fn widget_v1_selected_received_at(ordering: WidgetV1FixtureOrdering) -> &'static str {
    // expose only the valid and future fixture pairs
    if ordering == WidgetV1FixtureOrdering::SelectedFuture {
        "2026-09-12T15:16:00.000Z"
    } else {
        "2026-09-12T14:50:00.000Z"
    }
}

/// Build one active generic temperature decision.
fn generic_temperature_adjustment(selected: Option<f64>) -> ApiForecastAdjustmentDecision {
    ApiForecastAdjustmentDecision::Domain(ForecastAdjustmentDecision::Active(
        ForecastAdjustmentActiveDecision {
            adjustedMetrics: ForecastAdjustmentActiveDecisionAdjustedMetricsRecord(
                selected
                    .map(|value| vec![(ForecastAdjustmentMetric::TemperatureC, value)])
                    .unwrap_or_default(),
            ),
            algorithmContractVersion:
                ForecastAdjustmentActiveDecisionAlgorithmContractVersionLiteral,
            appliedMetrics: vec![ForecastAdjustmentMetric::TemperatureC],
            candidateArtifactSha256: "candidate".into(),
            contractVersion: ForecastAdjustmentActiveDecisionContractVersionLiteral,
            evaluationReportSha256: "evaluation".into(),
            leadBand: ForecastLeadBandKey::Value001024,
            qualificationReceiptSha256: "qualification".into(),
            rawForecastProvenance: ForecastAdjustmentRawForecastProvenance {
                adapterVersion: "fixture".into(),
                cohort: ForecastAdjustmentRawForecastProvenanceCohortLiteral,
                contractEpoch: "fixture".into(),
                dataset: "fixture".into(),
                referenceAt: "2026-09-12T15:00:00.000Z".into(),
                referenceKind: ForecastAdjustmentRawForecastProvenanceReferenceKindLiteral,
                sourceConfigFingerprint: "fixture".into(),
                sourceKey: "fixture".into(),
                targetLeadHours: 1,
                upstreamModel: "ecmwf_ifs".into(),
                validAt: "2026-09-12T15:00:00.000Z".into(),
            },
            reasonCode: ForecastAdjustmentActiveDecisionReasonCodeNull,
            state: ForecastAdjustmentActiveDecisionStateLiteral,
        },
    ))
}

/// Convert one fixed oracle deadline to its typed Rust boundary.
fn widget_v1_deadline(value: &str) -> WidgetForecastAdjustmentDeadline {
    // preserve exact absent, permanent, and fixed timestamp states
    match value {
        "unavailable" => WidgetForecastAdjustmentDeadline::Unavailable,
        "permanent" => WidgetForecastAdjustmentDeadline::Permanent,
        "2026-09-12T15:15:00.000Z" => {
            WidgetForecastAdjustmentDeadline::AtEpochMilliseconds(GENERATED_AT_EPOCH_MS)
        }
        "2026-09-12T16:00:00.000Z" => {
            WidgetForecastAdjustmentDeadline::AtEpochMilliseconds(1_789_228_800_000)
        }
        _ => WidgetForecastAdjustmentDeadline::Unavailable,
    }
}

/// Flatten one v1 result into the Node oracle's signature.
fn widget_v1_result_signature(
    result: Result<WidgetForecastHour, WidgetForecastProjectionError>,
) -> String {
    // retain the projector's stable error class as the Node diagnostic
    match result {
        Ok(hour) => format!(
            "ok\n{}\n{}\n{}\n{}",
            hour.start,
            hour.end,
            field_signature(&hour.rainMmPerHour),
            field_signature(&hour.temperatureC),
        ),
        Err(error) => format!("error\t{}", widget_v1_error_message(error)),
    }
}

/// Map one closed Rust projection failure to the source projector diagnostic.
fn widget_v1_error_message(error: WidgetForecastProjectionError) -> &'static str {
    // expose only failures exercised by the v1 parity boundary
    match error {
        WidgetForecastProjectionError::InvalidRain => "Widget forecast contains invalid rain",
        WidgetForecastProjectionError::InvalidTemperature => {
            "Widget forecast contains invalid temperature"
        }
        WidgetForecastProjectionError::InvalidSelectedRain => {
            "Widget forecast contains invalid selected rain"
        }
        WidgetForecastProjectionError::InvalidSelectedTemperature => {
            "Widget forecast contains invalid selected temperature"
        }
        WidgetForecastProjectionError::RainDecisionFromFuture => {
            "Widget forecast rain decision is from the future"
        }
        WidgetForecastProjectionError::SelectedSourceIncomplete => {
            "Widget forecast selected source is incomplete"
        }
        WidgetForecastProjectionError::SourceTimestampsOutOfOrder => {
            "Widget forecast source timestamps are out of order"
        }
        WidgetForecastProjectionError::CanonicalTimestampMismatch
        | WidgetForecastProjectionError::DuplicateOrForeignHour
        | WidgetForecastProjectionError::GeneratedAfterReceived
        | WidgetForecastProjectionError::GeneratedOutsideV3Window
        | WidgetForecastProjectionError::HourTimestampMismatch
        | WidgetForecastProjectionError::InvalidV3Calendar
        | WidgetForecastProjectionError::InvalidV3HourCount
        | WidgetForecastProjectionError::InvalidCloudCoverPercent
        | WidgetForecastProjectionError::InvalidWindSpeedMps
        | WidgetForecastProjectionError::InvalidSelectedWindSpeedMps
        | WidgetForecastProjectionError::NonForecastHour
        | WidgetForecastProjectionError::SourceTimestampMismatch
        | WidgetForecastProjectionError::TimestampOutOfRange => "unexpected v1 fixture error",
    }
}

/// Render one optional number for the Node command line.
fn option_number_argument(value: Option<f64>) -> String {
    // retain explicit null rather than a numeric sentinel
    value.map_or_else(|| "null".into(), |value| value.to_string())
}

/// Render one fixture branch for the Node oracle.
fn widget_v1_branch_argument(branch: WidgetV1FixtureBranch) -> &'static str {
    // keep branch spellings aligned with the oracle fixture
    match branch {
        WidgetV1FixtureBranch::Raw => "raw",
        WidgetV1FixtureBranch::GenericTemperature => "generic-temperature",
        WidgetV1FixtureBranch::IndependentTemperature => "independent-temperature",
        WidgetV1FixtureBranch::Rain => "rain",
        WidgetV1FixtureBranch::Missing => "missing",
    }
}

/// Render one fixture ordering for the Node oracle.
fn widget_v1_ordering_argument(ordering: WidgetV1FixtureOrdering) -> &'static str {
    // keep ordering spellings aligned with the oracle fixture
    match ordering {
        WidgetV1FixtureOrdering::Valid => "valid",
        WidgetV1FixtureOrdering::RawFuture => "raw-future",
        WidgetV1FixtureOrdering::SelectedFuture => "selected-future",
        WidgetV1FixtureOrdering::SelectedIncomplete => "selected-incomplete",
    }
}

/// Build the fixed source epoch tuple paired with the canonical fixture strings.
fn source_epochs() -> WidgetForecastSourceEpochs {
    WidgetForecastSourceEpochs {
        generated_at_epoch_ms: GENERATED_AT_EPOCH_MS,
        received_at_epoch_ms: RECEIVED_AT_EPOCH_MS,
        product_run_at_epoch_ms: Some(RUN_AT_EPOCH_MS),
    }
}

/// Build one stable v1 hour whose modes make the v2 status observable.
fn base_hour() -> WidgetForecastHour {
    WidgetForecastHour {
        end: "2026-09-12T16:00:00.000Z".into(),
        rainMmPerHour: raw_base_field(),
        start: "2026-09-12T15:00:00.000Z".into(),
        temperatureC: raw_base_field(),
    }
}

/// Build one raw inherited field used only by the status projection.
fn raw_base_field() -> WidgetForecastField {
    WidgetForecastField {
        mode: WidgetForecastMode::Raw,
        raw: Nullable::Value(1.0),
        rawSource: Nullable::Null,
        reason: WidgetForecastReason::RawForecast,
        selected: Nullable::Value(1.0),
        selectedSource: Nullable::Null,
        selectedUntil: Nullable::Null,
    }
}

/// Build one synthetic hour whose four fields share a mode.
fn status_hour(mode: WidgetForecastMode) -> WidgetForecastV2Hour {
    let field = WidgetForecastField {
        mode,
        raw: Nullable::Null,
        rawSource: Nullable::Null,
        reason: WidgetForecastReason::Missing,
        selected: Nullable::Null,
        selectedSource: Nullable::Null,
        selectedUntil: Nullable::Null,
    };
    WidgetForecastV2Hour {
        end: "2026-09-12T16:00:00.000Z".into(),
        rainMmPerHour: field.clone(),
        start: "2026-09-12T15:00:00.000Z".into(),
        temperatureC: field.clone(),
        cloudCoverPercent: field.clone(),
        windSpeedMps: field,
    }
}

/// Build one canonical API forecast record with optional active wind output.
fn forecast_record(
    cloud_cover_percent: Option<f64>,
    wind_speed_mps: Option<f64>,
    adjusted_wind_speed_mps: Option<f64>,
) -> ApiForecastWeatherRecord {
    ApiForecastWeatherRecord {
        freshness: ApiWeatherRecordFreshness {
            ageSeconds: 0,
            label: "fresh".into(),
            status: ApiWeatherRecordFreshnessStatusEnum::Fresh,
        },
        id: "forecast-hour".into(),
        metadata: ApiWeatherRecordMetadata {
            device: Nullable::Null,
            provider: Nullable::Null,
            quality: Nullable::Null,
            upstream: ApiWeatherRecordMetadataUpstream {
                model: Nullable::Value("ecmwf_ifs".into()),
                timezone: "UTC".into(),
            },
        },
        metrics: forecast_metrics(cloud_cover_percent, wind_speed_mps),
        productRunAt: Nullable::Value("2026-09-12T12:00:00.000Z".into()),
        provenance: ApiWeatherRecordProvenance {
            attribution: ApiWeatherRecordProvenanceAttribution {
                label: "fixture".into(),
                url: "https://example.invalid".into(),
            },
            label: "fixture".into(),
            providerKey: "fixture".into(),
            sourceId: "fixture".into(),
            sourceKey: "fixture".into(),
            sourceKind: SourceKind::Forecast,
            stationSlug: "fixture".into(),
        },
        receivedAt: "2026-09-12T15:00:00.000Z".into(),
        revisionCount: 0,
        validAt: "2026-09-12T15:00:00.000Z".into(),
        rainAdjustment: raw_rain_adjustment(),
        adjustment: forecast_adjustment(adjusted_wind_speed_mps),
        temperatureAdjustment: raw_temperature_adjustment(),
    }
}

/// Build the canonical metric record with only the two v2 values populated.
fn forecast_metrics(
    cloud_cover_percent: Option<f64>,
    wind_speed_mps: Option<f64>,
) -> ApiWeatherRecordMetrics {
    ApiWeatherRecordMetrics {
        apparentTemperatureC: Nullable::Null,
        blackGlobeTemperatureC: Nullable::Null,
        cloudCoverPercent: nullable_number(cloud_cover_percent),
        pm25MicrogramsPerCubicMeter: Nullable::Null,
        precipitationMm: Nullable::Null,
        precipitationRateMmPerHour: Nullable::Null,
        pressureHpa: Nullable::Null,
        relativeHumidityPercent: Nullable::Null,
        soilElectricalConductivityMicrosiemensPerCm: Nullable::Null,
        soilMoisturePercent: Nullable::Null,
        solarRadiationWm2: Nullable::Null,
        temperatureC: Nullable::Null,
        uvIndex: Nullable::Null,
        windDirectionDegrees: Nullable::Null,
        windGustMps: Nullable::Null,
        windSpeedMps: nullable_number(wind_speed_mps),
        wetBulbGlobeTemperatureC: Nullable::Null,
    }
}

/// Build either an active wind decision or a fail-raw canonical decision.
fn forecast_adjustment(adjusted_wind_speed_mps: Option<f64>) -> ApiForecastAdjustmentDecision {
    let Some(adjusted_wind_speed_mps) = adjusted_wind_speed_mps else {
        return ApiForecastAdjustmentDecision::Domain(ForecastAdjustmentDecision::FailRaw(
            ForecastAdjustmentFailRawDecision {
                adjustedMetrics: ForecastAdjustmentFailRawDecisionAdjustedMetricsRecord,
                appliedMetrics: ForecastAdjustmentFailRawDecisionAppliedMetricsEmptyArray,
                contractVersion: ForecastAdjustmentFailRawDecisionContractVersionLiteral,
                reasonCode: ForecastAdjustmentReasonCode::MetricNotEnabled,
                state: ForecastAdjustmentFailRawDecisionStateEnum::Disabled,
            },
        ));
    };

    ApiForecastAdjustmentDecision::Domain(ForecastAdjustmentDecision::Active(
        ForecastAdjustmentActiveDecision {
            adjustedMetrics: ForecastAdjustmentActiveDecisionAdjustedMetricsRecord(vec![(
                ForecastAdjustmentMetric::WindSpeedMps,
                adjusted_wind_speed_mps,
            )]),
            algorithmContractVersion:
                ForecastAdjustmentActiveDecisionAlgorithmContractVersionLiteral,
            appliedMetrics: vec![ForecastAdjustmentMetric::WindSpeedMps],
            candidateArtifactSha256: "candidate".into(),
            contractVersion: ForecastAdjustmentActiveDecisionContractVersionLiteral,
            evaluationReportSha256: "evaluation".into(),
            leadBand: ForecastLeadBandKey::Value001024,
            qualificationReceiptSha256: "qualification".into(),
            rawForecastProvenance: ForecastAdjustmentRawForecastProvenance {
                adapterVersion: "fixture".into(),
                cohort: ForecastAdjustmentRawForecastProvenanceCohortLiteral,
                contractEpoch: "fixture".into(),
                dataset: "fixture".into(),
                referenceAt: "2026-09-12T15:00:00.000Z".into(),
                referenceKind: ForecastAdjustmentRawForecastProvenanceReferenceKindLiteral,
                sourceConfigFingerprint: "fixture".into(),
                sourceKey: "fixture".into(),
                targetLeadHours: 1,
                upstreamModel: "ecmwf_ifs".into(),
                validAt: "2026-09-12T15:00:00.000Z".into(),
            },
            reasonCode: ForecastAdjustmentActiveDecisionReasonCodeNull,
            state: ForecastAdjustmentActiveDecisionStateLiteral,
        },
    ))
}

/// Build one inactive rain decision irrelevant to this projection.
fn raw_rain_adjustment() -> ApiRainAdjustmentDecision {
    ApiRainAdjustmentDecision {
        contractVersion: ApiRainAdjustmentDecisionContractVersionLiteral,
        state: ApiRainAdjustmentDecisionStateEnum::Disabled,
        reasonCode: Nullable::Value("disabled".into()),
        bundleSha256: Nullable::Null,
        correctedPrecipitationMm: Nullable::Null,
        rawBestMatchPrecipitationMm: Nullable::Null,
        sourceForecast: Nullable::Null,
    }
}

/// Build one inactive temperature decision irrelevant to this projection.
fn raw_temperature_adjustment() -> ForecastTemperatureCanaryDecisionV1 {
    ForecastTemperatureCanaryDecisionV1 {
        branch: Nullable::Null,
        bundleSha256: Nullable::Null,
        contractVersion: ForecastTemperatureCanaryDecisionV1ContractVersionLiteral,
        correctedTemperatureC: Nullable::Null,
        rawBestMatchTemperatureC: Nullable::Null,
        reasonCode: Nullable::Null,
        recentErrorStateSha256: Nullable::Null,
        sourceForecast: Nullable::Null,
        state: ForecastTemperatureCanaryDecisionV1StateEnum::Disabled,
    }
}

/// Convert a test option to the generated nullable representation.
fn nullable_number(value: Option<f64>) -> Nullable<f64> {
    // retain exact absence instead of a numeric sentinel
    match value {
        Some(value) => Nullable::Value(value),
        None => Nullable::Null,
    }
}

/// Flatten one generated field to the Node oracle's wire signature.
fn field_signature(field: &WidgetForecastField) -> String {
    let (raw_run_at, raw_received_at) = source_signature(&field.rawSource);
    let (selected_run_at, selected_received_at) = source_signature(&field.selectedSource);
    format!(
        "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
        mode_wire_value(field.mode),
        nullable_number_signature(&field.raw),
        raw_run_at,
        raw_received_at,
        reason_wire_value(field.reason),
        nullable_number_signature(&field.selected),
        selected_run_at,
        selected_received_at,
        nullable_string_signature(&field.selectedUntil),
    )
}

/// Flatten one generated nullable source pair.
fn source_signature(source: &Nullable<WidgetForecastSource>) -> (&str, &str) {
    // preserve source nullability in both flattened columns
    match source {
        Nullable::Null => ("null", "null"),
        Nullable::Value(source) => (
            nullable_string_signature(&source.runAt),
            source.receivedAt.as_str(),
        ),
    }
}

/// Flatten one generated nullable number without allocating for null.
fn nullable_number_signature(value: &Nullable<f64>) -> String {
    // mirror JavaScript's primitive string conversion for fixture values
    match value {
        Nullable::Null => "null".into(),
        Nullable::Value(value) => value.to_string(),
    }
}

/// Flatten one generated nullable string.
fn nullable_string_signature(value: &Nullable<String>) -> &str {
    // return the canonical string or the oracle's null token
    match value {
        Nullable::Null => "null",
        Nullable::Value(value) => value,
    }
}

/// Return the canonical widget mode wire value.
fn mode_wire_value(mode: WidgetForecastMode) -> &'static str {
    // map only the generated closed enum variants
    match mode {
        WidgetForecastMode::Adjusted => "adjusted",
        WidgetForecastMode::Raw => "raw",
        WidgetForecastMode::Unavailable => "unavailable",
    }
}

/// Return the canonical widget reason wire value.
fn reason_wire_value(reason: WidgetForecastReason) -> &'static str {
    // cover every generated reason even when this slice emits a subset
    match reason {
        WidgetForecastReason::DeadlineExpired => "deadline_expired",
        WidgetForecastReason::DeadlineUnavailable => "deadline_unavailable",
        WidgetForecastReason::GenericAdjustment => "generic_adjustment",
        WidgetForecastReason::IndependentAdjustment => "independent_adjustment",
        WidgetForecastReason::Missing => "missing",
        WidgetForecastReason::RainAdjustment => "rain_adjustment",
        WidgetForecastReason::RawForecast => "raw_forecast",
    }
}

/// Return the canonical widget status wire value.
fn status_wire_value(status: WidgetForecastStatus) -> &'static str {
    // map only the generated closed enum variants
    match status {
        WidgetForecastStatus::Adjusted => "adjusted",
        WidgetForecastStatus::Raw => "raw",
        WidgetForecastStatus::Unavailable => "unavailable",
        WidgetForecastStatus::Mixed => "mixed",
    }
}
