use std::path::PathBuf;
use std::process::Command;

use weather_adjustment::{
    AdmittedTemperatureMosApplicationInput, AdmittedTemperatureMosCalendar,
    AdmittedTemperatureMosFeatureInput, AdmittedTemperatureMosFeatures,
    AdmittedTemperatureMosRuntimeInput, apply_admitted_temperature_mos_forecast,
    apply_admitted_temperature_mos_runtime, build_admitted_temperature_mos_features,
};
use weather_contracts::{
    Nullable, TemperatureMosPermanentRuntimeModel,
    TemperatureMosPermanentRuntimeModelContractVersionLiteral,
    TemperatureMosRuntimeAppliedResultBranchEnum, TemperatureMosRuntimeForecastModelCycleEnum,
    TemperatureMosRuntimeModel, TemperatureMosRuntimeModelContractVersionLiteral,
    TemperatureMosRuntimeModelMaterialCohortLiteral,
    TemperatureMosRuntimeModelMaterialLearnedStrengthContractVersionLiteral,
    TemperatureMosRuntimeModelMaterialScopeEnum as TemperatureMosScope,
    TemperatureMosRuntimeModelMaterialStrengthBands, TemperatureMosRuntimeRawReason,
    TemperatureMosRuntimeRecentErrorState, TemperatureMosRuntimeRecentErrorStateCohortLiteral,
    TemperatureMosRuntimeResult, TemperatureMosRuntimeStrengthBand, TemperatureMosServingModel,
};

const DIRECT_FEATURE_COUNT: usize = 35;
const ADAPTIVE_FEATURE_COUNT: usize = 49;
const TRAINING_CUTOFF: &str = "2026-08-25T07:00:00.000Z";

struct Fixture {
    lead: u64,
    model: TemperatureMosServingModel,
    model_cycle: TemperatureMosRuntimeForecastModelCycleEnum,
    raw: f64,
    raw_humidity: Nullable<f64>,
    raw_wind: Nullable<f64>,
    state: TemperatureMosRuntimeRecentErrorState,
}

#[derive(Debug, Clone, Copy)]
struct OracleCalendar {
    annual_basis: [f64; 4],
    cycle_basis: [f64; 2],
    daily_basis: [f64; 4],
}

struct Oracle {
    adaptive: Option<[f64; ADAPTIVE_FEATURE_COUNT]>,
    calendar: Option<OracleCalendar>,
    direct: Option<[f64; DIRECT_FEATURE_COUNT]>,
    result: String,
}

/// Compare exact inference values and numerical fallbacks with the compiled Node runtime.
#[test]
fn matches_compiled_temperature_mos_runtime() {
    let cases = [
        "adaptive-first",
        "adaptive-second",
        "direct",
        "delayed-first",
        "delayed-second",
        "permanent",
        "cap-positive",
        "cap-negative",
        "lead-initialization",
        "lead-delayed",
        "model-unsupported",
        "strength-unsupported",
        "missing-direct",
        "missing-adaptive",
        "invalid-alpha",
        "nonfinite-coefficient",
        "raw-nonfinite",
        "weather-missing",
    ];
    // exercise both branches, both scopes, both model variants, caps, and raw fallbacks
    for name in cases {
        let oracle = node_oracle(name);
        let direct = oracle.direct.unwrap_or([0.0; DIRECT_FEATURE_COUNT]);
        let fixture = fixture(name);
        let actual = apply_admitted_temperature_mos_runtime(AdmittedTemperatureMosRuntimeInput {
            features: AdmittedTemperatureMosFeatures {
                direct: &direct,
                adaptive: oracle.adaptive.as_ref(),
            },
            model: &fixture.model,
            model_lead_hours: fixture.lead,
            raw_temperature_c: fixture.raw,
            recent_error_state: &fixture.state,
        });
        assert_eq!(result_signature(actual), oracle.result, "{name}");
    }
}

/// Compare the Rust feature owners and their evaluator composition to compiled Node arrays.
#[test]
fn builds_compiled_temperature_mos_features() {
    let cases = [
        "adaptive-first",
        "adaptive-second",
        "direct",
        "delayed-first",
        "delayed-second",
        "permanent",
        "cap-positive",
        "cap-negative",
        "weather-missing",
        "model-49-feature",
        "cycle-zero-feature",
    ];
    // cover calendar, cycle, missing-weather, direct, adaptive, and delayed feature terms
    for name in cases {
        let oracle = node_oracle(name);
        let fixture = fixture(name);
        let calendar = oracle.calendar.expect("feature fixture calendar");
        let admitted_calendar = AdmittedTemperatureMosCalendar {
            annual_basis: calendar.annual_basis,
            cycle_basis: calendar.cycle_basis,
            daily_basis: calendar.daily_basis,
        };
        let built = build_admitted_temperature_mos_features(AdmittedTemperatureMosFeatureInput {
            calendar: admitted_calendar,
            model_cycle: fixture.model_cycle,
            model_lead_hours: fixture.lead,
            raw_relative_humidity_percent: &fixture.raw_humidity,
            raw_temperature_c: fixture.raw,
            raw_wind_speed_mps: &fixture.raw_wind,
            recent_error_state: &fixture.state,
            scope: model_scope(&fixture.model),
        })
        .expect("compiled feature fixture must build");
        assert_float_arrays_eq(
            &built.direct,
            oracle.direct.as_ref().expect("compiled direct features"),
            name,
        );
        // compare the adaptive vector only for supported causal state
        match (&built.adaptive, &oracle.adaptive) {
            (Some(actual), Some(expected)) => assert_float_arrays_eq(actual, expected, name),
            (None, None) => {}
            _ => panic!("adaptive feature presence differs for {name}"),
        }

        // feature-only era and cycle probes intentionally bypass the public source validator
        if !matches!(name, "model-49-feature" | "cycle-zero-feature") {
            let composed =
                apply_admitted_temperature_mos_forecast(AdmittedTemperatureMosApplicationInput {
                    calendar: admitted_calendar,
                    model: &fixture.model,
                    model_cycle: fixture.model_cycle,
                    model_lead_hours: fixture.lead,
                    raw_relative_humidity_percent: &fixture.raw_humidity,
                    raw_temperature_c: fixture.raw,
                    raw_wind_speed_mps: &fixture.raw_wind,
                    recent_error_state: &fixture.state,
                });
            assert_eq!(result_signature(composed), oracle.result, "compose {name}");
        }
    }
}

/// Prove admitted feature failures retain their owning source reasons.
#[test]
fn rejects_missing_or_nonfinite_normalized_features() {
    let model = monthly_model();
    let supported_state = recent_error_state(true);
    let unsupported_state = recent_error_state(false);
    let direct = [0.0; DIRECT_FEATURE_COUNT];
    let mut invalid_direct = direct;
    invalid_direct[3] = f64::NAN;
    let mut invalid_adaptive = [0.0; ADAPTIVE_FEATURE_COUNT];
    invalid_adaptive[3] = f64::INFINITY;

    let cases = [
        (
            &unsupported_state,
            &invalid_direct,
            None,
            TemperatureMosRuntimeRawReason::InvalidForecast,
        ),
        (
            &supported_state,
            &direct,
            None,
            TemperatureMosRuntimeRawReason::InvalidRecentErrorState,
        ),
        (
            &supported_state,
            &direct,
            Some(&invalid_adaptive),
            TemperatureMosRuntimeRawReason::InvalidRecentErrorState,
        ),
    ];
    // retain the feature builder's direct versus adaptive failure distinction
    for (state, direct, adaptive, expected_reason) in cases {
        let result = apply_admitted_temperature_mos_runtime(AdmittedTemperatureMosRuntimeInput {
            features: AdmittedTemperatureMosFeatures { direct, adaptive },
            model: &model,
            model_lead_hours: 5,
            raw_temperature_c: 16.4,
            recent_error_state: state,
        });
        let TemperatureMosRuntimeResult::Raw(raw) = result else {
            panic!("invalid admitted features must fail raw");
        };
        assert_eq!(raw.reason, expected_reason);
    }
}

/// Invoke the compiled source runtime and return its derived normalized features.
fn node_oracle(name: &str) -> Oracle {
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/temperature_mos_inference_oracle.mjs");
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
    let direct = parse_features(fields.next().expect("direct feature field"));
    let adaptive = parse_features(fields.next().expect("adaptive feature field"));
    let result = fields.next().expect("result field").to_owned();
    let calendar = parse_calendar(fields.next().expect("calendar field"));
    assert!(fields.next().is_none(), "oracle field count");
    Oracle {
        adaptive: adaptive.map(|values| values.try_into().expect("49 adaptive features")),
        calendar,
        direct: direct.map(|values| values.try_into().expect("35 direct features")),
        result,
    }
}

/// Decode the Node-resolved calendar boundary supplied to the Rust feature owner.
fn parse_calendar(field: &str) -> Option<OracleCalendar> {
    // preserve the absent marker for invalid-forecast fixtures
    if field == "-" {
        return None;
    }
    let values = parse_features(field).expect("calendar harmonic features");
    assert_eq!(values.len(), 10, "calendar field count");
    Some(OracleCalendar {
        annual_basis: values[0..4].try_into().expect("annual basis"),
        daily_basis: values[4..8].try_into().expect("daily basis"),
        cycle_basis: values[8..10].try_into().expect("cycle basis"),
    })
}

/// Decode one comma-separated exact binary64 feature vector.
fn parse_features(field: &str) -> Option<Vec<f64>> {
    // preserve the oracle's absent-vector marker
    if field == "-" {
        return None;
    }
    Some(
        field
            .split(',')
            .map(|bits| {
                f64::from_bits(u64::from_str_radix(bits, 16).expect("binary64 feature bits"))
            })
            .collect(),
    )
}

/// Build the generated model and recent-state fixture for one oracle case.
fn fixture(name: &str) -> Fixture {
    let mut lead = 5;
    let mut raw = 16.4;
    let mut raw_humidity = Nullable::Value(78.0);
    let mut raw_wind = Nullable::Value(3.2);
    let mut state = recent_error_state(true);
    let mut monthly = monthly_model_value();
    let mut model_cycle = TemperatureMosRuntimeForecastModelCycleEnum::Value50r1;
    let mut permanent = false;

    // change only the model dimension named by the source fixture
    match name {
        "adaptive-first" => {}
        "adaptive-second" => lead = 10,
        "direct" => state = recent_error_state(false),
        "delayed-first" => {
            lead = 7;
            monthly.scope = TemperatureMosScope::AssumedDelay6Next12;
        }
        "delayed-second" => {
            lead = 13;
            monthly.scope = TemperatureMosScope::AssumedDelay6Next12;
        }
        "permanent" => permanent = true,
        "cap-positive" => {
            raw = 69.0;
            state = recent_error_state(false);
            monthly.directCoefficients = Nullable::Value(cap_coefficients(100.0));
        }
        "cap-negative" => {
            raw = -99.0;
            state = recent_error_state(false);
            monthly.directCoefficients = Nullable::Value(cap_coefficients(-100.0));
        }
        "lead-initialization" => lead = 13,
        "lead-delayed" => {
            lead = 6;
            monthly.scope = TemperatureMosScope::AssumedDelay6Next12;
        }
        "model-unsupported" => monthly.supported = false,
        "strength-unsupported" => monthly.strengthBands.hours1To6.supported = false,
        "missing-direct" => {
            state = recent_error_state(false);
            monthly.directCoefficients = Nullable::Null;
        }
        "missing-adaptive" => monthly.adaptiveCoefficients = Nullable::Null,
        "invalid-alpha" => monthly.strengthBands.hours1To6.alpha = 0.75,
        "nonfinite-coefficient" => {
            let Nullable::Value(coefficients) = &mut monthly.adaptiveCoefficients else {
                unreachable!();
            };
            coefficients[4] = f64::NAN;
        }
        "raw-nonfinite" => raw = f64::NAN,
        "weather-missing" => {
            raw_humidity = Nullable::Null;
            raw_wind = Nullable::Null;
        }
        "model-49-feature" => {
            model_cycle = TemperatureMosRuntimeForecastModelCycleEnum::Value49r1;
        }
        "cycle-zero-feature" => {}
        _ => panic!("unknown fixture {name}"),
    }

    let model = if permanent {
        TemperatureMosServingModel::Permanent(permanent_model_from(monthly))
    } else {
        TemperatureMosServingModel::Monthly(monthly)
    };
    Fixture {
        lead,
        model,
        model_cycle,
        raw,
        raw_humidity,
        raw_wind,
        state,
    }
}

/// Borrow the fitted scope shared by both generated model variants.
fn model_scope(model: &TemperatureMosServingModel) -> TemperatureMosScope {
    // erase only the already admitted contract variant
    match model {
        TemperatureMosServingModel::Monthly(model) => model.scope,
        TemperatureMosServingModel::Permanent(model) => model.scope,
    }
}

/// Compare fixed arrays by exact binary64 value and report their owning fixture.
fn assert_float_arrays_eq<const N: usize>(actual: &[f64; N], expected: &[f64; N], name: &str) {
    // retain exact feature position evidence without decimal tolerance
    for index in 0..N {
        assert_eq!(
            actual[index].to_bits(),
            expected[index].to_bits(),
            "{name} feature {index}",
        );
    }
}

/// Build one generated monthly model with the source coefficient tables.
fn monthly_model() -> TemperatureMosServingModel {
    TemperatureMosServingModel::Monthly(monthly_model_value())
}

/// Build the owned monthly variant used by mutable fixture setup.
fn monthly_model_value() -> TemperatureMosRuntimeModel {
    TemperatureMosRuntimeModel {
        adaptiveCoefficients: Nullable::Value(adaptive_coefficients()),
        cohort: TemperatureMosRuntimeModelMaterialCohortLiteral,
        directCoefficients: Nullable::Value(direct_coefficients()),
        learnedStrengthContractVersion:
            TemperatureMosRuntimeModelMaterialLearnedStrengthContractVersionLiteral,
        scope: TemperatureMosScope::InitializationFirst12,
        strengthBands: strength_bands(),
        supported: true,
        trainingCutoffUtc: TRAINING_CUTOFF.to_owned(),
        contractVersion: TemperatureMosRuntimeModelContractVersionLiteral,
        month: "2026-09".to_owned(),
    }
}

/// Convert shared monthly material into the generated permanent contract.
fn permanent_model_from(model: TemperatureMosRuntimeModel) -> TemperatureMosPermanentRuntimeModel {
    TemperatureMosPermanentRuntimeModel {
        adaptiveCoefficients: model.adaptiveCoefficients,
        cohort: model.cohort,
        directCoefficients: model.directCoefficients,
        learnedStrengthContractVersion: model.learnedStrengthContractVersion,
        scope: model.scope,
        strengthBands: model.strengthBands,
        supported: model.supported,
        trainingCutoffUtc: model.trainingCutoffUtc,
        contractVersion: TemperatureMosPermanentRuntimeModelContractVersionLiteral,
        effectiveFrom: TRAINING_CUTOFF.to_owned(),
        latestTrainingValidAt: "2026-08-25T00:00:00.000Z".to_owned(),
    }
}

/// Build both learned-strength bands in generated storage.
fn strength_bands() -> TemperatureMosRuntimeModelMaterialStrengthBands {
    TemperatureMosRuntimeModelMaterialStrengthBands {
        hours1To6: TemperatureMosRuntimeStrengthBand {
            alpha: 1.0,
            supported: true,
            trainingCutoffUtc: TRAINING_CUTOFF.to_owned(),
        },
        hours7To12: TemperatureMosRuntimeStrengthBand {
            alpha: 0.8,
            supported: true,
            trainingCutoffUtc: TRAINING_CUTOFF.to_owned(),
        },
    }
}

/// Build the exact 35-value direct coefficient fixture.
fn direct_coefficients() -> Vec<f64> {
    (0..DIRECT_FEATURE_COUNT)
        .map(|index| ((index % 7) as i32 - 3) as f64 * 0.017)
        .collect()
}

/// Build the exact 49-value adaptive coefficient fixture.
fn adaptive_coefficients() -> Vec<f64> {
    (0..ADAPTIVE_FEATURE_COUNT)
        .map(|index| ((index % 9) as i32 - 4) as f64 * 0.011)
        .collect()
}

/// Build one intercept-only correction cap fixture.
fn cap_coefficients(intercept: f64) -> Vec<f64> {
    let mut coefficients = vec![0.0; DIRECT_FEATURE_COUNT];
    coefficients[0] = intercept;
    coefficients
}

/// Build one generated prevalidated causal recent-error receipt.
fn recent_error_state(supported: bool) -> TemperatureMosRuntimeRecentErrorState {
    TemperatureMosRuntimeRecentErrorState {
        b24C: if supported {
            Nullable::Value(0.8)
        } else {
            Nullable::Null
        },
        b72C: if supported {
            Nullable::Value(0.3)
        } else {
            Nullable::Null
        },
        cohort: TemperatureMosRuntimeRecentErrorStateCohortLiteral,
        localDates: if supported { 3 } else { 0 },
        mad72C: if supported {
            Nullable::Value(0.4)
        } else {
            Nullable::Null
        },
        maximumSourceRunInitializedAt: if supported {
            Nullable::Value("2026-09-06T12:00:00.000Z".to_owned())
        } else {
            Nullable::Null
        },
        maximumSourceValidAt: if supported {
            Nullable::Value("2026-09-07T05:00:00.000Z".to_owned())
        } else {
            Nullable::Null
        },
        n24: if supported { 12 } else { 0 },
        n72: if supported { 50 } else { 0 },
        sourceKeys: if supported {
            (0..50).map(|index| format!("source-{index}")).collect()
        } else {
            Vec::new()
        },
        supported,
        targetRunInitializedAt: "2026-09-07T12:00:00.000Z".to_owned(),
        windowEndValidAt: "2026-09-07T05:00:00.000Z".to_owned(),
    }
}

/// Render exact generated result values in the Node oracle's compact form.
fn result_signature(result: TemperatureMosRuntimeResult) -> String {
    // distinguish the active and raw generated variants
    match result {
        TemperatureMosRuntimeResult::Applied(applied) => format!(
            "applied:{}:{:016x}:{:016x}:{:016x}:{:016x}:{:016x}",
            branch_name(applied.branch),
            applied.alpha.to_bits(),
            applied.rawTemperatureC.to_bits(),
            applied.unscaledCorrectionC.to_bits(),
            applied.incumbentHalfStrengthTemperatureC.to_bits(),
            applied.predictionTemperatureC.to_bits(),
        ),
        TemperatureMosRuntimeResult::Raw(raw) => format!(
            "raw:{}:{:016x}",
            reason_name(raw.reason),
            raw.rawTemperatureC.to_bits(),
        ),
    }
}

/// Return one generated branch's canonical wire name.
fn branch_name(branch: TemperatureMosRuntimeAppliedResultBranchEnum) -> &'static str {
    // map the closed branch set
    match branch {
        TemperatureMosRuntimeAppliedResultBranchEnum::Adaptive => "adaptive",
        TemperatureMosRuntimeAppliedResultBranchEnum::Direct => "direct",
    }
}

/// Return one generated raw reason's canonical wire name.
fn reason_name(reason: TemperatureMosRuntimeRawReason) -> &'static str {
    // map the complete contract so unexpected fallbacks remain observable
    match reason {
        TemperatureMosRuntimeRawReason::InvalidForecast => "invalid_forecast",
        TemperatureMosRuntimeRawReason::InferenceError => "inference_error",
        TemperatureMosRuntimeRawReason::InvalidModel => "invalid_model",
        TemperatureMosRuntimeRawReason::InvalidRecentErrorState => "invalid_recent_error_state",
        TemperatureMosRuntimeRawReason::ModelIdentityMismatch => "model_identity_mismatch",
        TemperatureMosRuntimeRawReason::ModelNotSupported => "model_not_supported",
        TemperatureMosRuntimeRawReason::ModelNotYetAvailable => "model_not_yet_available",
        TemperatureMosRuntimeRawReason::OutsideAssumedDelay6Next12 => {
            "outside_assumed_delay6_next12"
        }
        TemperatureMosRuntimeRawReason::OutsideInitializationFirst12 => {
            "outside_initialization_first12"
        }
        TemperatureMosRuntimeRawReason::RecentErrorStateAsOfMismatch => {
            "recent_error_state_as_of_mismatch"
        }
        TemperatureMosRuntimeRawReason::RecentErrorStateContainsFutureData => {
            "recent_error_state_contains_future_data"
        }
        TemperatureMosRuntimeRawReason::RecentErrorStateOutsideWindow => {
            "recent_error_state_outside_window"
        }
        TemperatureMosRuntimeRawReason::RecentErrorStateRunMismatch => {
            "recent_error_state_run_mismatch"
        }
        TemperatureMosRuntimeRawReason::RecentErrorStateSourceRunMismatch => {
            "recent_error_state_source_run_mismatch"
        }
        TemperatureMosRuntimeRawReason::StrengthBandNotSupported => "strength_band_not_supported",
        TemperatureMosRuntimeRawReason::UnsupportedCohort => "unsupported_cohort",
    }
}
