use weather_adjustment::{apply_capped_correction, apply_core_adjustment};
use weather_contracts::{
    CoreAdjustmentApplicationInput, CoreAdjustmentApplicationInputEnvelope,
    CoreAdjustmentApplicationResultReasonEnum as AdjustmentReason, ForecastAdjustmentMetric,
    Nullable,
};

/// Create the TypeScript core-adjustment fixture in generated Rust storage.
fn base_input() -> CoreAdjustmentApplicationInput {
    CoreAdjustmentApplicationInput {
        calendarFingerprintMatches: true,
        coefficient: Nullable::Value(1.0),
        enabled: true,
        envelope: Nullable::Value(CoreAdjustmentApplicationInputEnvelope {
            maximum: 20.0,
            minimum: -2.0,
        }),
        identityMatches: true,
        metric: ForecastAdjustmentMetric::TemperatureC,
        rawForecastValue: 20.0,
        rawWindSpeedMps: Nullable::Value(3.0),
        rootAvailable: true,
    }
}

/// Mirror the correction-cap and final-range TypeScript oracle.
#[test]
fn applies_frozen_correction_caps_before_final_ranges() {
    assert_eq!(
        apply_capped_correction(ForecastAdjustmentMetric::TemperatureC, 68.0, 9.0),
        Ok(70.0),
    );
    assert_eq!(
        apply_capped_correction(
            ForecastAdjustmentMetric::RelativeHumidityPercent,
            95.0,
            25.0,
        ),
        Ok(100.0),
    );
    assert_eq!(
        apply_capped_correction(ForecastAdjustmentMetric::WindSpeedMps, 2.0, -20.0),
        Ok(0.0),
    );
    assert_eq!(
        apply_capped_correction(ForecastAdjustmentMetric::WindGustMps, 145.0, 20.0),
        Ok(150.0),
    );
    assert_eq!(
        apply_capped_correction(ForecastAdjustmentMetric::WindDirectionDegrees, 350.0, 90.0),
        Ok(35.0),
    );
    assert_eq!(
        apply_capped_correction(ForecastAdjustmentMetric::WindDirectionDegrees, 2.0, -5.0),
        Ok(357.0),
    );
}

/// Mirror the inclusive envelope and every core fail-raw guard.
#[test]
fn fails_raw_on_frozen_guards() {
    let base = base_input();
    let adjusted = apply_core_adjustment(&base);
    assert_eq!(adjusted.adjustedValue, 21.0);
    assert!(adjusted.applied);
    assert_eq!(adjusted.reason, AdjustmentReason::Adjusted);

    let mut out_of_envelope = base_input();
    out_of_envelope.rawForecastValue = 20.01;
    let result = apply_core_adjustment(&out_of_envelope);
    assert_eq!(result.adjustedValue, 20.01);
    assert!(!result.applied);
    assert_eq!(result.reason, AdjustmentReason::RawValueOod);

    let mut wrong_identity = base_input();
    wrong_identity.identityMatches = false;
    assert_eq!(apply_core_adjustment(&wrong_identity).adjustedValue, 20.0);

    let mut wrong_calendar = base_input();
    wrong_calendar.calendarFingerprintMatches = false;
    assert!(!apply_core_adjustment(&wrong_calendar).applied);

    let mut missing_root = base_input();
    missing_root.rootAvailable = false;
    assert_eq!(
        apply_core_adjustment(&missing_root).reason,
        AdjustmentReason::RootMissing,
    );

    let mut calm_direction = base_input();
    calm_direction.metric = ForecastAdjustmentMetric::WindDirectionDegrees;
    calm_direction.envelope = Nullable::Null;
    calm_direction.rawForecastValue = 180.0;
    calm_direction.rawWindSpeedMps = Nullable::Value(0.99);
    assert_eq!(
        apply_core_adjustment(&calm_direction).reason,
        AdjustmentReason::RawDirectionCalm,
    );

    calm_direction.rawWindSpeedMps = Nullable::Value(1.0);
    assert!(apply_core_adjustment(&calm_direction).applied);
}

/// Prove guard precedence and non-finite handling explicitly.
#[test]
fn preserves_fail_raw_guard_precedence() {
    let mut input = base_input();
    input.identityMatches = false;
    input.calendarFingerprintMatches = false;
    input.enabled = false;
    input.rootAvailable = false;
    input.coefficient = Nullable::Null;
    assert_eq!(
        apply_core_adjustment(&input).reason,
        AdjustmentReason::ForecastIdentityMismatch,
    );

    let mut missing_coefficient = base_input();
    missing_coefficient.coefficient = Nullable::Value(f64::NAN);
    assert_eq!(
        apply_core_adjustment(&missing_coefficient).reason,
        AdjustmentReason::CoefficientMissing,
    );

    let mut invalid_raw = base_input();
    invalid_raw.rawForecastValue = f64::INFINITY;
    let result = apply_core_adjustment(&invalid_raw);
    assert!(!result.applied);
    assert_eq!(result.reason, AdjustmentReason::RawValueInvalid);
    assert!(result.adjustedValue.is_infinite());
}
