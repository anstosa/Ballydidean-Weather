use std::path::PathBuf;
use std::process::Command;

use weather_adjustment::{
    ValidatedWindCanaryRuntime, WindCanaryActivationDecision, WindCanaryAuthorizationWindow,
    WindCanaryMetricScope, decide_wind_canary_activation,
};
use weather_contracts::{
    ForecastAdjustmentMetric, ForecastAdjustmentMetricBand, ForecastLeadBandKey,
    LoadedForecastAdjustmentWindCanaryRuntimeV1DisabledReasonCodeEnum as WindCanaryDisabledReason,
};

const FINITE_ACTIVATED_AT_MS: i64 = 1_788_418_800_000;
const FINITE_EXPIRES_AT_MS: i64 = 1_789_023_600_000;
const PERMANENT_ACTIVATED_AT_MS: i64 = 1_790_872_400_936;

/// Invoke the extracted compiled wind activation decision.
fn node_oracle(
    kill_switch: Option<&str>,
    runtime: &str,
    evaluated_at_epoch_ms: i64,
    metric: &str,
    lead_band: &str,
) -> String {
    let script =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/wind_canary_activation_oracle.mjs");
    let output = Command::new("node")
        .arg(script)
        .arg(kill_switch.unwrap_or("undefined"))
        .arg(runtime)
        .arg(evaluated_at_epoch_ms.to_string())
        .arg(metric)
        .arg(lead_band)
        .output()
        .expect("Node oracle must start");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("Node oracle must emit UTF-8")
}

/// Prove kill registry and authorization priority against compiled Node behavior.
#[test]
fn matches_wind_canary_activation_priority_and_boundaries() {
    let enabled = enabled_metric_bands();
    let cases = [
        (
            Some("1"),
            "inactive",
            FINITE_ACTIVATED_AT_MS,
            ForecastAdjustmentMetric::WindSpeedMps,
            ForecastLeadBandKey::Value001024,
        ),
        (
            Some("0"),
            "inactive",
            FINITE_ACTIVATED_AT_MS,
            ForecastAdjustmentMetric::WindSpeedMps,
            ForecastLeadBandKey::Value001024,
        ),
        (
            None,
            "finite",
            FINITE_ACTIVATED_AT_MS - 1,
            ForecastAdjustmentMetric::WindSpeedMps,
            ForecastLeadBandKey::Value001024,
        ),
        (
            None,
            "finite",
            FINITE_ACTIVATED_AT_MS,
            ForecastAdjustmentMetric::WindSpeedMps,
            ForecastLeadBandKey::Value001024,
        ),
        (
            None,
            "finite",
            FINITE_EXPIRES_AT_MS - 1,
            ForecastAdjustmentMetric::WindGustMps,
            ForecastLeadBandKey::Value001024,
        ),
        (
            None,
            "finite",
            FINITE_EXPIRES_AT_MS,
            ForecastAdjustmentMetric::WindSpeedMps,
            ForecastLeadBandKey::Value001024,
        ),
        (
            Some("01"),
            "permanent",
            PERMANENT_ACTIVATED_AT_MS,
            ForecastAdjustmentMetric::WindSpeedMps,
            ForecastLeadBandKey::Value001024,
        ),
        (
            None,
            "permanent",
            PERMANENT_ACTIVATED_AT_MS - 1,
            ForecastAdjustmentMetric::WindSpeedMps,
            ForecastLeadBandKey::Value001024,
        ),
        (
            None,
            "permanent",
            4_102_444_800_000,
            ForecastAdjustmentMetric::WindSpeedMps,
            ForecastLeadBandKey::Value001024,
        ),
    ];
    // compare literal switch and inclusive-exclusive time cases to compiled source
    for (kill_switch, runtime_name, evaluated_at, metric, lead_band) in cases {
        let runtime = runtime_for(runtime_name, &enabled);
        let actual = decision_name(decide_wind_canary_activation(
            kill_switch,
            runtime,
            evaluated_at,
            metric,
            lead_band,
        ));
        assert_eq!(
            actual,
            node_oracle(
                kill_switch,
                runtime_name,
                evaluated_at,
                metric_name(metric),
                lead_band_name(lead_band),
            ),
            "{runtime_name} at {evaluated_at}",
        );
    }
}

/// Prove active runtime scope stays limited to enabled wind metric-band pairs.
#[test]
fn matches_wind_canary_metric_scope() {
    let enabled = enabled_metric_bands();
    let cases = [
        (
            ForecastAdjustmentMetric::WindSpeedMps,
            ForecastLeadBandKey::Value001024,
        ),
        (
            ForecastAdjustmentMetric::WindDirectionDegrees,
            ForecastLeadBandKey::Value001024,
        ),
        (
            ForecastAdjustmentMetric::WindGustMps,
            ForecastLeadBandKey::Value025048,
        ),
        (
            ForecastAdjustmentMetric::TemperatureC,
            ForecastLeadBandKey::Value001024,
        ),
    ];
    // compare the source candidate mask and hard wind allowlist
    for (metric, lead_band) in cases {
        let actual = decision_name(decide_wind_canary_activation(
            None,
            runtime_for("finite", &enabled),
            FINITE_ACTIVATED_AT_MS,
            metric,
            lead_band,
        ));
        assert_eq!(
            actual,
            node_oracle(
                None,
                "finite",
                FINITE_ACTIVATED_AT_MS,
                metric_name(metric),
                lead_band_name(lead_band),
            ),
        );
    }
}

/// Build the exact two-pair source fixture in generated DTO storage.
fn enabled_metric_bands() -> Vec<ForecastAdjustmentMetricBand> {
    vec![
        ForecastAdjustmentMetricBand {
            leadBand: ForecastLeadBandKey::Value001024,
            metric: ForecastAdjustmentMetric::WindGustMps,
        },
        ForecastAdjustmentMetricBand {
            leadBand: ForecastLeadBandKey::Value001024,
            metric: ForecastAdjustmentMetric::WindSpeedMps,
        },
    ]
}

/// Select one prevalidated registry or authorization fixture.
fn runtime_for<'a>(
    name: &str,
    enabled_metric_bands: &'a [ForecastAdjustmentMetricBand],
) -> ValidatedWindCanaryRuntime<'a> {
    // pair each fixture name with its already parsed authorization geometry
    match name {
        "inactive" => ValidatedWindCanaryRuntime::RegistryInactive,
        "finite" => ValidatedWindCanaryRuntime::Candidate {
            authorization: WindCanaryAuthorizationWindow::TimeLimited {
                activated_at_epoch_ms: FINITE_ACTIVATED_AT_MS,
                expires_at_epoch_ms: FINITE_EXPIRES_AT_MS,
            },
            enabled_metric_bands,
        },
        "permanent" => ValidatedWindCanaryRuntime::Candidate {
            authorization: WindCanaryAuthorizationWindow::Permanent {
                activated_at_epoch_ms: PERMANENT_ACTIVATED_AT_MS,
            },
            enabled_metric_bands,
        },
        _ => panic!("unknown runtime fixture {name}"),
    }
}

/// Render one Rust decision in the oracle's compact form.
fn decision_name(decision: WindCanaryActivationDecision) -> &'static str {
    // retain the canonical runtime reason and scope names
    match decision {
        WindCanaryActivationDecision::Disabled(WindCanaryDisabledReason::CanaryKilled) => {
            "disabled:canary_killed"
        }
        WindCanaryActivationDecision::Disabled(WindCanaryDisabledReason::CanaryExpired) => {
            "disabled:canary_expired"
        }
        WindCanaryActivationDecision::Disabled(_) => "disabled:unexpected",
        WindCanaryActivationDecision::LegacyFallback => "legacy_fallback",
        WindCanaryActivationDecision::Active(WindCanaryMetricScope::Enabled) => "active:enabled",
        WindCanaryActivationDecision::Active(WindCanaryMetricScope::NotEnabled) => {
            "active:not_enabled"
        }
    }
}

/// Return one generated metric's canonical wire name.
fn metric_name(metric: ForecastAdjustmentMetric) -> &'static str {
    // map only generated closed variants used by the parity matrix
    match metric {
        ForecastAdjustmentMetric::RelativeHumidityPercent => "relativeHumidityPercent",
        ForecastAdjustmentMetric::TemperatureC => "temperatureC",
        ForecastAdjustmentMetric::WindDirectionDegrees => "windDirectionDegrees",
        ForecastAdjustmentMetric::WindGustMps => "windGustMps",
        ForecastAdjustmentMetric::WindSpeedMps => "windSpeedMps",
    }
}

/// Return one generated lead band's canonical wire name.
fn lead_band_name(lead_band: ForecastLeadBandKey) -> &'static str {
    // map only generated closed variants used by the parity matrix
    match lead_band {
        ForecastLeadBandKey::Value001024 => "001-024",
        ForecastLeadBandKey::Value025048 => "025-048",
        ForecastLeadBandKey::Value049072 => "049-072",
        ForecastLeadBandKey::Value073096 => "073-096",
        ForecastLeadBandKey::Value097120 => "097-120",
        ForecastLeadBandKey::Value121144 => "121-144",
        ForecastLeadBandKey::Value145168 => "145-168",
    }
}
