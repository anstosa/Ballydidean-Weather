use super::{
    ADJUSTMENT_ROLLING_SCHEDULE_CONTRACT, ADJUSTMENT_ROLLING_SCHEDULE_SHA256,
    NativeAdjustmentRollingCaptureHorizonInput, NativeAdjustmentRollingScheduleBootstrapInput,
    NativeAdjustmentRollingWindowInput, NativeMaintenanceRollingScheduleError,
    adjustment_rolling_local_date_at, build_adjustment_rolling_schedule_bootstrap,
    build_adjustment_rolling_window, extend_adjustment_rolling_capture_horizon,
};

const EPOCH: &str = "2026-10-09T04:10:45.000Z";
const WITNESS: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

/// Bind the canonical LF-framed policy and finite bootstrap to the captured Node fixture.
#[test]
fn schedule_contract_and_bootstrap_match_node_fixture() {
    assert_eq!(
        ADJUSTMENT_ROLLING_SCHEDULE_CONTRACT.rain_minimum_training_dates,
        180
    );
    assert_eq!(
        ADJUSTMENT_ROLLING_SCHEDULE_CONTRACT.rain_calibration_days,
        90
    );
    assert_eq!(
        ADJUSTMENT_ROLLING_SCHEDULE_CONTRACT.rain_confirmation_days,
        334
    );
    assert_eq!(
        ADJUSTMENT_ROLLING_SCHEDULE_SHA256,
        "7c17f5d1a8e8249cd0aa4820638169e51f6edb3433017f50ab4c959e44c62f1f"
    );
    let bootstrap = build_adjustment_rolling_schedule_bootstrap(
        NativeAdjustmentRollingScheduleBootstrapInput {
            epoch_at: EPOCH,
            epoch_witness_sha256: WITNESS,
        },
    )
    .expect("bootstrap");
    assert_eq!(bootstrap.first_complete_local_date, "2026-10-09");
    assert_eq!(bootstrap.horizon_end_at, "2029-08-27T07:00:00.000Z");
    assert_eq!(
        bootstrap.bootstrap_sha256,
        "83f1eabf954aa278012d6006a68266a8865f27ce95dfaba24aa8c9d8119351f4"
    );
}

/// Match rain, temperature, and wind Node projections across leap and DST boundaries.
#[test]
fn all_family_windows_match_node_boundary_fixtures() {
    let rain = window("rain", "2027-08", "2027-08-02T12:00:00.000Z");
    assert_eq!(rain.source_floor_at, "2026-10-09T07:00:00.000Z");
    assert_eq!(rain.training_end_at, "2027-04-19T00:00:00.000Z");
    assert_eq!(
        rain.calibration_start_at.as_deref(),
        Some("2027-04-26T00:00:00.000Z")
    );
    assert_eq!(
        rain.calibration_end_at.as_deref(),
        Some("2027-07-25T00:00:00.000Z")
    );
    assert_eq!(rain.interval_start_at, "2027-09-01T07:00:00.000Z");
    assert_eq!(rain.interval_end_at, "2028-07-31T07:00:00.000Z");
    assert_eq!(rain.terminal_at, "2028-08-07T07:00:00.000Z");
    assert_eq!(
        rain.plan_sha256,
        "b0b20db1fa25de993d4e38a9fd51fa76980d791a92f0157d4e52668ffb31c8a9"
    );

    let temperature = window("temperature", "2027-10", "2027-10-02T12:00:00.000Z");
    assert_eq!(temperature.interval_start_at, "2027-11-01T07:00:00.000Z");
    assert_eq!(temperature.interval_end_at, "2028-11-01T07:00:00.000Z");
    assert_eq!(temperature.terminal_at, "2028-11-08T07:00:00.000Z");
    assert_eq!(
        temperature.plan_sha256,
        "dd364cd3e60861a47b21f4d1b6df557d130588359555610bff9793c8df99cdfb"
    );

    let wind = window("wind", "2036-10", "2036-10-02T12:00:00.000Z");
    assert_eq!(wind.interval_start_at, "2036-11-01T07:00:00.000Z");
    assert_eq!(wind.interval_end_at, "2037-11-02T08:00:00.000Z");
    assert_eq!(wind.terminal_at, "2037-11-09T08:00:00.000Z");
    assert_eq!(
        wind.plan_sha256,
        "d0e234fab712a75493dbdb8b4b02e577227a43cd6b837ebad916533270752a8d"
    );
}

/// Preserve local dates on both sides of the predecessor's spring and autumn transitions.
#[test]
fn local_dates_and_horizon_extension_match_node_fixture() {
    let values = [
        "2027-03-14T09:59:59.999Z",
        "2027-03-14T10:00:00.000Z",
        "2027-11-07T08:59:59.999Z",
        "2027-11-07T09:00:00.000Z",
    ];
    // retain the captured Node Intl date on every transition boundary
    for (value, expected) in
        values
            .into_iter()
            .zip(["2027-03-14", "2027-03-14", "2027-11-07", "2027-11-07"])
    {
        assert_eq!(
            adjustment_rolling_local_date_at(value).expect("local date"),
            expected
        );
    }
    assert_eq!(
        extend_adjustment_rolling_capture_horizon(NativeAdjustmentRollingCaptureHorizonInput {
            current_end_at: "2028-08-26T07:00:00.000Z",
            member_terminal_at: "2029-08-07T07:00:00.000Z",
        })
        .expect("extended horizon"),
        "2029-08-07T07:00:00.000Z"
    );
}

/// Reject future fit months and windows without the source's post-epoch support.
#[test]
fn invalid_future_and_support_windows_remain_rejected() {
    let future = build_adjustment_rolling_window(NativeAdjustmentRollingWindowInput {
        epoch_at: EPOCH,
        epoch_witness_sha256: WITNESS,
        family: "rain",
        fit_month: "2027-09",
        predecessor_terminal_at: None,
        requested_at: "2027-08-02T12:00:00.000Z",
    });
    assert_eq!(
        future,
        Err(NativeMaintenanceRollingScheduleError::FutureMonth)
    );
    let unsupported = build_adjustment_rolling_window(NativeAdjustmentRollingWindowInput {
        epoch_at: EPOCH,
        epoch_witness_sha256: WITNESS,
        family: "wind",
        fit_month: "2026-11",
        predecessor_terminal_at: None,
        requested_at: "2026-11-02T12:00:00.000Z",
    });
    assert_eq!(
        unsupported,
        Err(NativeMaintenanceRollingScheduleError::CalendarSupport)
    );
}

/// Build one admitted family fixture through the exact public input boundary.
fn window(
    family: &str,
    fit_month: &str,
    requested_at: &str,
) -> super::NativeAdjustmentRollingWindow {
    build_adjustment_rolling_window(NativeAdjustmentRollingWindowInput {
        epoch_at: EPOCH,
        epoch_witness_sha256: WITNESS,
        family,
        fit_month,
        predecessor_terminal_at: None,
        requested_at,
    })
    .expect("rolling window")
}
