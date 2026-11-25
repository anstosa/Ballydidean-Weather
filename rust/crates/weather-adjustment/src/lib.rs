#![allow(non_snake_case)]

mod performance_scorecard_amount;
mod performance_scorecard_evaluation;
mod performance_scorecard_rain;
mod rain_hurdle_wind_features;
mod rain_portable_model;
mod temperature_mos_inference;
mod temperature_recent_error;
mod wind_development;
mod wind_development_station;
mod wind_moving_block_bootstrap;
mod wind_robust_hierarchy;
mod wind_row_evaluation;
mod wind_training_events;
mod wind_training_support;

pub use performance_scorecard_amount::{
    BalancedForecastAdjustmentScore, CoalescedForecastAdjustmentPairs,
    ForecastAdjustmentScorecardCalendar, ForecastAdjustmentScorecardError,
    balance_forecast_adjustment_performance_pairs, coalesce_forecast_adjustment_performance_pairs,
    prepare_forecast_adjustment_performance_pairs, score_balanced_forecast_adjustment_pairs,
};
pub use performance_scorecard_evaluation::{
    ForecastAdjustmentPerformanceEvaluationError, evaluate_forecast_adjustment_performance,
};
pub use performance_scorecard_rain::{
    ForecastAdjustmentRainDiagnosticCalendar, ForecastAdjustmentRainDiagnosticsError,
    UnavailableForecastAdjustmentRainDiagnostics,
    UnavailableForecastAdjustmentRainThresholdSummary,
    evaluate_forecast_adjustment_rain_diagnostics,
    unavailable_forecast_adjustment_rain_diagnostics,
};
pub use rain_hurdle_wind_features::{
    AdmittedRainHurdleWindCalendar, AdmittedRainHurdleWindFeatureInput,
    AdmittedRainHurdleWindStations, build_admitted_rain_hurdle_wind_features,
};
pub use rain_portable_model::RainHurdleWindPortableModel;
pub use temperature_mos_inference::{
    AdmittedTemperatureMosApplicationInput, AdmittedTemperatureMosCalendar,
    AdmittedTemperatureMosFeatureInput, AdmittedTemperatureMosFeatures,
    AdmittedTemperatureMosRuntimeInput, BorrowedTemperatureMosServingModel,
    TemperatureMosFeatureAssembly, apply_admitted_temperature_mos_forecast,
    apply_admitted_temperature_mos_runtime, build_admitted_temperature_mos_features,
};
pub use temperature_recent_error::{
    AdmittedTemperatureRecentErrorForecast, AdmittedTemperatureRecentErrorHour,
    AdmittedTemperatureRecentErrorInput, AdmittedTemperatureRecentErrorStation,
    TemperatureRecentErrorReductionError, build_admitted_temperature_recent_error_state,
};
pub use wind_development::{
    AdmittedWindDevelopmentDate, WindAuxiliaryModel, WindDevelopmentError,
    WindDevelopmentQualification, WindDevelopmentQualifiedPair, WindDevelopmentRunError,
    evaluate_wind_development_loso_fold, qualify_wind_maintenance_development,
    validate_wind_development_fold, wind_development_pair_passes,
};
pub use wind_development_station::{
    AdmittedWindDevelopmentFold, RetainedWindDevelopmentPairedEvent,
    RetainedWindDevelopmentStationScore, WindDevelopmentStationError,
    score_wind_development_stations,
};
pub use wind_moving_block_bootstrap::{
    WindBootstrapDateSlot, WindBootstrapPairedEvent, WindBootstrapScoredWindow,
    WindMovingBlockBootstrapError, WindMovingBlockBootstrapResult, WindMovingBlockStartPlan,
    create_wind_moving_block_start_plan, score_wind_moving_block_bootstrap,
    score_wind_moving_block_bootstrap_plan,
};
pub use wind_robust_hierarchy::{
    AdmittedWindHierarchyObservation, WindRobustHierarchyError, fit_wind_robust_hierarchy,
};
pub use wind_row_evaluation::{
    AdmittedWindBundleRow, AdmittedWindRowCalendar, AdmittedWindRowFeatures,
    AdmittedWindRowSourceIdentity, AdmittedWindV2MetricOrder, WindV2RowOutcome,
    evaluate_admitted_wind_bundle_row, evaluate_admitted_wind_v2_row,
};
pub use wind_training_events::{
    AdmittedWindForecastTrainingRow, AdmittedWindStationTrainingRow, AdmittedWindTrainingRow,
    ReadyWindMaintenanceTraining, WindMaintenanceTrainingPreparation, WindTrainingEvent,
    WindTrainingEventError, WindTrainingSet, build_retained_wind_training_events,
    fit_wind_training_event_pair, prepare_wind_maintenance_training, scalar_wind_network_actual,
};
pub use wind_training_support::retained_wind_sufficiency_failures;

use std::error::Error;
use std::fmt::{Display, Formatter};

use weather_contracts::retained_rain_hurdle_wind::{FrozenRainHead, HEADS};
pub use weather_contracts::retained_rain_hurdle_wind::{
    RETAINED_RAIN_MODEL_MONTH, RETAINED_RAIN_MODEL_SHA256,
};
use weather_contracts::{
    CoreAdjustmentApplicationInput, CoreAdjustmentApplicationResult,
    CoreAdjustmentApplicationResultReasonEnum as AdjustmentReason, ForecastAdjustmentMetric,
    ForecastAdjustmentMetricBand, ForecastAdjustmentMetricPolicy, ForecastLeadBandKey,
    LoadedForecastAdjustmentWindCanaryRuntimeV1DisabledReasonCodeEnum as WindCanaryDisabledReason,
    Nullable, RainHurdleWindFeaturePerformance, RainHurdleWindPortableArtifactV1Heads,
    RainHurdleWindPortableArtifactV1HeadsNativeHead,
    RainHurdleWindPortableArtifactV1HeadsObjectiveEnum as RainHeadObjective,
    RainHurdleWindPortableArtifactV1Rules,
};

const MINIMUM_DIRECTION_WIND_SPEED_MPS: f64 = 1.0;
const RAIN_FEATURE_COUNT: usize = 107;
const RAIN_THRESHOLDS: [f64; 3] = [0.1, 1.0, 2.5];

/// Prevalidated authorization time geometry with parsed epoch boundaries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindCanaryAuthorizationWindow {
    TimeLimited {
        activated_at_epoch_ms: i64,
        expires_at_epoch_ms: i64,
    },
    Permanent {
        activated_at_epoch_ms: i64,
    },
}

/// Prevalidated runtime selection supplied after registry and bundle checks.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ValidatedWindCanaryRuntime<'a> {
    RegistryInactive,
    Candidate {
        authorization: WindCanaryAuthorizationWindow,
        enabled_metric_bands: &'a [ForecastAdjustmentMetricBand],
    },
}

/// Metric-band disposition for one active wind canary runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindCanaryMetricScope {
    Enabled,
    NotEnabled,
}

/// API-facing runtime selection after kill, registry, time, and scope precedence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindCanaryActivationDecision {
    Disabled(WindCanaryDisabledReason),
    LegacyFallback,
    Active(WindCanaryMetricScope),
}

/// Select the wind runtime without parsing artifacts, clocks, or environment state.
pub fn decide_wind_canary_activation(
    environment_kill_switch: Option<&str>,
    runtime: ValidatedWindCanaryRuntime<'_>,
    evaluated_at_epoch_ms: i64,
    metric: ForecastAdjustmentMetric,
    lead_band: ForecastLeadBandKey,
) -> WindCanaryActivationDecision {
    // let the literal one-way switch override registry fallback and candidate state
    if environment_kill_switch == Some("1") {
        return WindCanaryActivationDecision::Disabled(WindCanaryDisabledReason::CanaryKilled);
    }

    // preserve API fallback only for an explicitly inactive registry
    let ValidatedWindCanaryRuntime::Candidate {
        authorization,
        enabled_metric_bands,
    } = runtime
    else {
        return WindCanaryActivationDecision::LegacyFallback;
    };

    // enforce the inclusive activation and exclusive finite expiry boundaries
    if !wind_canary_authorized_at(authorization, evaluated_at_epoch_ms) {
        return WindCanaryActivationDecision::Disabled(WindCanaryDisabledReason::CanaryExpired);
    }

    // keep an active runtime distinct from one enabled metric-band pair
    let scope = if wind_canary_metric_in_scope(enabled_metric_bands, metric, lead_band) {
        WindCanaryMetricScope::Enabled
    } else {
        WindCanaryMetricScope::NotEnabled
    };
    WindCanaryActivationDecision::Active(scope)
}

/// Evaluate one already parsed wind authorization window.
fn wind_canary_authorized_at(
    authorization: WindCanaryAuthorizationWindow,
    evaluated_at_epoch_ms: i64,
) -> bool {
    // preserve permanent and finite authorization boundary differences
    match authorization {
        WindCanaryAuthorizationWindow::TimeLimited {
            activated_at_epoch_ms,
            expires_at_epoch_ms,
        } => {
            evaluated_at_epoch_ms >= activated_at_epoch_ms
                && evaluated_at_epoch_ms < expires_at_epoch_ms
        }
        WindCanaryAuthorizationWindow::Permanent {
            activated_at_epoch_ms,
        } => evaluated_at_epoch_ms >= activated_at_epoch_ms,
    }
}

/// Check the hard wind allowlist and candidate metric-band mask.
fn wind_canary_metric_in_scope(
    enabled_metric_bands: &[ForecastAdjustmentMetricBand],
    metric: ForecastAdjustmentMetric,
    lead_band: ForecastLeadBandKey,
) -> bool {
    // exclude non-wind metrics even if malformed prevalidated data names them
    if !matches!(
        metric,
        ForecastAdjustmentMetric::WindDirectionDegrees
            | ForecastAdjustmentMetric::WindGustMps
            | ForecastAdjustmentMetric::WindSpeedMps
    ) {
        return false;
    }
    enabled_metric_bands
        .iter()
        .any(|pair| pair.metric == metric && pair.leadBand == lead_band)
}

/// Invalid arithmetic supplied directly to correction application.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NonFiniteCorrectionInput;

impl Display for NonFiniteCorrectionInput {
    /// Render the existing stable TypeScript diagnostic.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("correction inputs must be finite")
    }
}

impl Error for NonFiniteCorrectionInput {}

#[derive(Debug, Clone, Copy)]
struct MetricPolicy {
    correction_minimum: f64,
    correction_maximum: f64,
    final_minimum: f64,
    final_maximum: f64,
    final_maximum_exclusive: bool,
}

/// Invalid normalized rain-hurdle model tables or inference inputs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RainHurdleWindInferenceError {
    InvalidFeatureVector,
    InvalidLocalMonth,
    InvalidRawAmount,
    InvalidRules,
    InvalidEventHead,
    InvalidAmountHead,
    InvalidTree,
    InvalidEventProbability,
    InvalidAmountProjection,
}

/// Borrowed validated v1 rain-hurdle tables for repeated allocation-free inference.
#[derive(Debug)]
pub struct RainHurdleWindV1Model<'a> {
    heads: RainHurdleWindHeads<'a>,
    rules: &'a [RainHurdleWindPortableArtifactV1Rules],
    category_scales: &'a [f64],
}

#[derive(Debug, Clone, Copy)]
enum RainHurdleWindHeads<'a> {
    Portable(&'a RainHurdleWindPortableArtifactV1Heads),
    Retained(&'static [FrozenRainHead; 4]),
}

impl<'a> RainHurdleWindV1Model<'a> {
    /// Validate compact typed tables once before repeated inference.
    pub fn new(
        heads: &'a RainHurdleWindPortableArtifactV1Heads,
        rules: &'a [RainHurdleWindPortableArtifactV1Rules],
        category_scales: &'a [f64],
    ) -> Result<Self, RainHurdleWindInferenceError> {
        validate_rain_rules(rules, category_scales)?;
        validate_rain_head(
            &heads.atLeast0_1,
            RainHeadObjective::BinaryLogistic,
            RainHurdleWindInferenceError::InvalidEventHead,
        )?;
        validate_rain_head(
            &heads.atLeast1_0,
            RainHeadObjective::BinaryLogistic,
            RainHurdleWindInferenceError::InvalidEventHead,
        )?;
        validate_rain_head(
            &heads.atLeast2_5,
            RainHeadObjective::BinaryLogistic,
            RainHurdleWindInferenceError::InvalidEventHead,
        )?;
        validate_rain_head(
            &heads.amount,
            RainHeadObjective::RegGamma,
            RainHurdleWindInferenceError::InvalidAmountHead,
        )?;
        Ok(Self {
            heads: RainHurdleWindHeads::Portable(heads),
            rules,
            category_scales,
        })
    }

    /// Evaluate the production v1 hurdle decision loop from one normalized feature vector.
    pub fn predict(
        &self,
        features: &[f32],
    ) -> Result<RainHurdleWindFeaturePerformance, RainHurdleWindInferenceError> {
        self.predict_projected(features, |_| Ok(()), |_, projected, ()| projected)
    }

    /// Evaluate shared native geometry around admitted occurrence and amount projections.
    pub(crate) fn predict_projected<Context, Occurrence, Selected>(
        &self,
        features: &[f32],
        project_occurrence: Occurrence,
        project_selected: Selected,
    ) -> Result<RainHurdleWindFeaturePerformance, RainHurdleWindInferenceError>
    where
        Occurrence: FnOnce(&mut [f64; 3]) -> Result<Context, RainHurdleWindInferenceError>,
        Selected: FnOnce(f64, f64, Context) -> f64,
    {
        // preserve missing NaNs while excluding infinities and wrong geometry
        if features.len() != RAIN_FEATURE_COUNT || features.iter().any(|value| value.is_infinite())
        {
            return Err(RainHurdleWindInferenceError::InvalidFeatureVector);
        }
        let raw = f64::from(features[5]);
        // require one usable nonnegative raw amount
        if !raw.is_finite() || raw < 0.0 {
            return Err(RainHurdleWindInferenceError::InvalidRawAmount);
        }

        let mut scores = [
            f64::from(score_rain_head(self.head(0), features)),
            f64::from(score_rain_head(self.head(1), features)),
            f64::from(score_rain_head(self.head(2), features)),
        ];
        let context = project_occurrence(&mut scores)?;
        let mut category = 0;
        // let the highest called event replace every lower category
        for (index, threshold) in RAIN_THRESHOLDS.iter().copied().enumerate() {
            let score = scores[index];
            // reject any invalid event head result before amount projection
            if !score.is_finite() || !(0.0..=1.0).contains(&score) {
                return Err(RainHurdleWindInferenceError::InvalidEventProbability);
            }
            let called = match self.rules[index].cutoff {
                Nullable::Null => raw >= threshold,
                Nullable::Value(cutoff) => score >= cutoff,
            };
            // retain the highest satisfied threshold
            if called {
                category = index + 1;
            }
        }

        let positive_amount_mm =
            f64::from(score_rain_head(self.head(3), features)).clamp(0.1, 30.0);
        // return dry when no event head or raw threshold calls wet
        if category == 0 {
            return Ok(RainHurdleWindFeaturePerformance {
                correctedPrecipitationMm: project_selected(raw, 0.0, context),
                occurrenceProbabilityAtLeast0_1: scores[0],
                occurrenceProbabilityAtLeast1_0: scores[1],
                occurrenceProbabilityAtLeast2_5: scores[2],
                positiveAmountMm: positive_amount_mm,
            });
        }

        let base = (0.25 * raw + 0.75 * positive_amount_mm).clamp(0.0, 30.0);
        let scaled = base * self.category_scales[category - 1];
        // preserve JavaScript Math.min/Math.max propagation of undefined amount arithmetic
        if scaled.is_nan() {
            return Err(RainHurdleWindInferenceError::InvalidAmountProjection);
        }
        let lower = RAIN_THRESHOLDS[category - 1];
        let upper = match category {
            1 => 1.0 - f64::EPSILON / 2.0,
            2 => 2.5 - f64::EPSILON,
            _ => 30.0,
        };
        let projected = scaled.max(lower).min(upper);
        // reject undefined arithmetic before returning the public result
        if !projected.is_finite() || !(0.0..=30.0).contains(&projected) {
            return Err(RainHurdleWindInferenceError::InvalidAmountProjection);
        }
        Ok(RainHurdleWindFeaturePerformance {
            correctedPrecipitationMm: project_selected(raw, projected, context),
            occurrenceProbabilityAtLeast0_1: scores[0],
            occurrenceProbabilityAtLeast1_0: scores[1],
            occurrenceProbabilityAtLeast2_5: scores[2],
            positiveAmountMm: positive_amount_mm,
        })
    }

    /// Build and evaluate one admitted forecast/station feature row.
    pub fn predict_admitted(
        &self,
        input: AdmittedRainHurdleWindFeatureInput<'_>,
    ) -> Result<RainHurdleWindFeaturePerformance, RainHurdleWindInferenceError> {
        self.build_and_predict_admitted(input)
            .map(|(_, performance)| performance)
    }

    /// Build once and return both the archival feature row and its prediction.
    pub fn build_and_predict_admitted(
        &self,
        input: AdmittedRainHurdleWindFeatureInput<'_>,
    ) -> Result<
        ([f32; RAIN_FEATURE_COUNT], RainHurdleWindFeaturePerformance),
        RainHurdleWindInferenceError,
    > {
        let features = build_admitted_rain_hurdle_wind_features(input)?;
        let performance = self.predict(&features)?;
        Ok((features, performance))
    }

    /// Borrow one head from either admitted portable tables or generated retained tables.
    fn head(&self, index: usize) -> RainHeadRef<'_> {
        match self.heads {
            RainHurdleWindHeads::Portable(heads) => RainHeadRef::Portable(match index {
                0 => &heads.atLeast0_1,
                1 => &heads.atLeast1_0,
                2 => &heads.atLeast2_5,
                _ => &heads.amount,
            }),
            RainHurdleWindHeads::Retained(heads) => RainHeadRef::Retained(&heads[index]),
        }
    }
}

impl RainHurdleWindV1Model<'static> {
    /// Borrow the build-time validated retained incumbent without allocation or revalidation.
    pub fn retained() -> Self {
        use weather_contracts::retained_rain_hurdle_wind::{CATEGORY_SCALES, RULES};

        Self {
            heads: RainHurdleWindHeads::Retained(&HEADS),
            rules: &RULES,
            category_scales: &CATEGORY_SCALES,
        }
    }
}

/// Apply the frozen metric correction cap and final-value range.
pub fn apply_capped_correction(
    metric: ForecastAdjustmentMetric,
    raw_forecast_value: f64,
    coefficient: f64,
) -> Result<f64, NonFiniteCorrectionInput> {
    // reject the same undefined arithmetic inputs as the TypeScript implementation
    if !raw_forecast_value.is_finite() || !coefficient.is_finite() {
        return Err(NonFiniteCorrectionInput);
    }

    Ok(apply_capped_correction_finite(
        metric,
        raw_forecast_value,
        coefficient,
    ))
}

/// Apply every frozen guard and retain the exact raw value on failure.
pub fn apply_core_adjustment(
    input: &CoreAdjustmentApplicationInput,
) -> CoreAdjustmentApplicationResult {
    // require exact production forecast identity
    if !input.identityMatches {
        return fail_raw(input, AdjustmentReason::ForecastIdentityMismatch);
    }

    // require the pinned runtime calendar
    if !input.calendarFingerprintMatches {
        return fail_raw(input, AdjustmentReason::CalendarFingerprintMismatch);
    }

    // reject disabled metric-band pairs
    if !input.enabled {
        return fail_raw(input, AdjustmentReason::DisabledMetricBand);
    }

    // require one fitted root
    if !input.rootAvailable {
        return fail_raw(input, AdjustmentReason::RootMissing);
    }

    // require a finite selected hierarchy coefficient
    let Nullable::Value(coefficient) = input.coefficient else {
        return fail_raw(input, AdjustmentReason::CoefficientMissing);
    };
    // reject a non-finite coefficient as missing
    if !coefficient.is_finite() {
        return fail_raw(input, AdjustmentReason::CoefficientMissing);
    }

    // reject invalid raw provider values
    if !raw_value_is_canonical(input.metric, input.rawForecastValue) {
        return fail_raw(input, AdjustmentReason::RawValueInvalid);
    }

    // require application-time wind for direction only
    if input.metric == ForecastAdjustmentMetric::WindDirectionDegrees {
        let Nullable::Value(raw_wind_speed_mps) = input.rawWindSpeedMps else {
            return fail_raw(input, AdjustmentReason::RawDirectionCalm);
        };
        // reject missing, invalid, or calm application-time wind
        if !raw_wind_speed_mps.is_finite() || raw_wind_speed_mps < MINIMUM_DIRECTION_WIND_SPEED_MPS
        {
            return fail_raw(input, AdjustmentReason::RawDirectionCalm);
        }
    } else {
        // enforce inclusive scalar training envelopes
        let Nullable::Value(envelope) = &input.envelope else {
            return fail_raw(input, AdjustmentReason::RawValueOod);
        };
        // reject malformed envelopes or values outside them
        if !envelope.minimum.is_finite()
            || !envelope.maximum.is_finite()
            || envelope.minimum > envelope.maximum
            || input.rawForecastValue < envelope.minimum
            || input.rawForecastValue > envelope.maximum
        {
            return fail_raw(input, AdjustmentReason::RawValueOod);
        }
    }

    CoreAdjustmentApplicationResult {
        adjustedValue: apply_capped_correction_finite(
            input.metric,
            input.rawForecastValue,
            coefficient,
        ),
        applied: true,
        reason: AdjustmentReason::Adjusted,
    }
}

/// Return the unchanged raw forecast with one exact guard reason.
fn fail_raw(
    input: &CoreAdjustmentApplicationInput,
    reason: AdjustmentReason,
) -> CoreAdjustmentApplicationResult {
    CoreAdjustmentApplicationResult {
        adjustedValue: input.rawForecastValue,
        applied: false,
        reason,
    }
}

/// Apply a correction after both operands have been proven finite.
fn apply_capped_correction_finite(
    metric: ForecastAdjustmentMetric,
    raw_forecast_value: f64,
    coefficient: f64,
) -> f64 {
    let policy = metric_policy(metric);
    let capped_coefficient =
        coefficient.clamp(policy.correction_minimum, policy.correction_maximum);

    // wrap direction into its final circle
    if metric == ForecastAdjustmentMetric::WindDirectionDegrees {
        return (raw_forecast_value + capped_coefficient).rem_euclid(360.0);
    }

    (raw_forecast_value + capped_coefficient).clamp(policy.final_minimum, policy.final_maximum)
}

/// Validate a raw value against its frozen policy.
fn raw_value_is_canonical(metric: ForecastAdjustmentMetric, raw_value: f64) -> bool {
    let policy = metric_policy(metric);
    raw_value.is_finite()
        && raw_value >= policy.final_minimum
        && if policy.final_maximum_exclusive {
            raw_value < policy.final_maximum
        } else {
            raw_value <= policy.final_maximum
        }
}

/// Return the exact policy constants from the TypeScript domain package.
const fn metric_policy(metric: ForecastAdjustmentMetric) -> MetricPolicy {
    match metric {
        ForecastAdjustmentMetric::RelativeHumidityPercent => MetricPolicy {
            correction_minimum: -20.0,
            correction_maximum: 20.0,
            final_minimum: 0.0,
            final_maximum: 100.0,
            final_maximum_exclusive: false,
        },
        ForecastAdjustmentMetric::TemperatureC => MetricPolicy {
            correction_minimum: -5.0,
            correction_maximum: 5.0,
            final_minimum: -100.0,
            final_maximum: 70.0,
            final_maximum_exclusive: false,
        },
        ForecastAdjustmentMetric::WindDirectionDegrees => MetricPolicy {
            correction_minimum: -45.0,
            correction_maximum: 45.0,
            final_minimum: 0.0,
            final_maximum: 360.0,
            final_maximum_exclusive: true,
        },
        ForecastAdjustmentMetric::WindGustMps => MetricPolicy {
            correction_minimum: -12.0,
            correction_maximum: 12.0,
            final_minimum: 0.0,
            final_maximum: 150.0,
            final_maximum_exclusive: false,
        },
        ForecastAdjustmentMetric::WindSpeedMps => MetricPolicy {
            correction_minimum: -8.0,
            correction_maximum: 8.0,
            final_minimum: 0.0,
            final_maximum: 150.0,
            final_maximum_exclusive: false,
        },
    }
}

/// Project the same serving limits into the original candidate policy table order.
pub fn forecast_adjustment_metric_policies() -> [ForecastAdjustmentMetricPolicy; 5] {
    [
        ForecastAdjustmentMetric::RelativeHumidityPercent,
        ForecastAdjustmentMetric::TemperatureC,
        ForecastAdjustmentMetric::WindDirectionDegrees,
        ForecastAdjustmentMetric::WindGustMps,
        ForecastAdjustmentMetric::WindSpeedMps,
    ]
    .map(|metric| {
        let policy = metric_policy(metric);
        ForecastAdjustmentMetricPolicy {
            correctionMaximum: policy.correction_maximum,
            correctionMinimum: policy.correction_minimum,
            finalMaximum: policy.final_maximum,
            finalMaximumExclusive: policy.final_maximum_exclusive,
            finalMinimum: policy.final_minimum,
            metric,
            wrapsFinalValue: metric == ForecastAdjustmentMetric::WindDirectionDegrees,
        }
    })
}

/// Validate the three frozen event rules and their positive amount scales.
fn validate_rain_rules(
    rules: &[RainHurdleWindPortableArtifactV1Rules],
    category_scales: &[f64],
) -> Result<(), RainHurdleWindInferenceError> {
    // require one rule and scale per frozen event threshold
    if rules.len() != RAIN_THRESHOLDS.len() || category_scales.len() != RAIN_THRESHOLDS.len() {
        return Err(RainHurdleWindInferenceError::InvalidRules);
    }
    // close thresholds cutoffs and scales before the hot inference path
    for (index, threshold) in RAIN_THRESHOLDS.iter().copied().enumerate() {
        let rule = &rules[index];
        let cutoff_is_valid = match rule.cutoff {
            Nullable::Null => true,
            Nullable::Value(cutoff) => cutoff.is_finite() && (0.0..=1.0).contains(&cutoff),
        };
        // reject any noncanonical event rule
        if rule.threshold != threshold
            || !cutoff_is_valid
            || !category_scales[index].is_finite()
            || category_scales[index] <= 0.0
        {
            return Err(RainHurdleWindInferenceError::InvalidRules);
        }
    }
    Ok(())
}

/// Validate one compact head with the same typed tree invariants as artifact loading.
fn validate_rain_head(
    head: &RainHurdleWindPortableArtifactV1HeadsNativeHead,
    expected_objective: RainHeadObjective,
    invalid_head: RainHurdleWindInferenceError,
) -> Result<(), RainHurdleWindInferenceError> {
    let base_score_is_valid = head.baseScore.is_finite()
        && head.baseScore > 0.0
        && (expected_objective == RainHeadObjective::RegGamma || head.baseScore < 1.0);
    // require the expected objective and at least one normalized tree
    if head.objective != expected_objective || !base_score_is_valid || head.trees.is_empty() {
        return Err(invalid_head);
    }
    // validate every tree once so traversal can stay bounds-check-only
    for tree in &head.trees {
        validate_rain_tree(tree)?;
    }
    Ok(())
}

/// Validate one five-array numerical tree without copying its tables.
fn validate_rain_tree(tree: &[Vec<f64>; 5]) -> Result<(), RainHurdleWindInferenceError> {
    let length = tree[0].len();
    // require aligned nonempty node arrays
    if length == 0 || tree.iter().any(|values| values.len() != length) {
        return Err(RainHurdleWindInferenceError::InvalidTree);
    }
    // reject malformed nodes and backward or dangling child links
    for (node, feature_value) in tree[0].iter().copied().enumerate() {
        let Some(feature) = exact_tree_integer(feature_value) else {
            return Err(RainHurdleWindInferenceError::InvalidTree);
        };
        let Some(left) = exact_tree_integer(tree[2][node]) else {
            return Err(RainHurdleWindInferenceError::InvalidTree);
        };
        let Some(right) = exact_tree_integer(tree[3][node]) else {
            return Err(RainHurdleWindInferenceError::InvalidTree);
        };
        let missing_left = tree[4][node];
        let terminal = left == -1 && right == -1;
        let branch = left > node as i64
            && right > node as i64
            && left < length as i64
            && right < length as i64;
        // retain only finite validated XGBoost node geometry
        if feature < 0
            || feature >= RAIN_FEATURE_COUNT as i64
            || !tree[1][node].is_finite()
            || (missing_left != 0.0 && missing_left != 1.0)
            || (!terminal && !branch)
        {
            return Err(RainHurdleWindInferenceError::InvalidTree);
        }
    }
    Ok(())
}

/// Convert one finite integral tree cell without accepting normalized fractions.
fn exact_tree_integer(value: f64) -> Option<i64> {
    // keep only exactly representable table integers
    if value.is_finite()
        && value.fract() == 0.0
        && value >= i64::MIN as f64
        && value <= i64::MAX as f64
    {
        Some(value as i64)
    } else {
        None
    }
}

#[derive(Clone, Copy)]
enum RainHeadRef<'a> {
    Portable(&'a RainHurdleWindPortableArtifactV1HeadsNativeHead),
    Retained(&'a FrozenRainHead),
}

impl RainHeadRef<'_> {
    /// Return the shared objective for either table owner.
    const fn objective(self) -> RainHeadObjective {
        match self {
            Self::Portable(head) => head.objective,
            Self::Retained(head) => head.objective,
        }
    }

    /// Return the shared base score for either table owner.
    const fn base_score(self) -> f64 {
        match self {
            Self::Portable(head) => head.baseScore,
            Self::Retained(head) => head.base_score,
        }
    }
}

#[derive(Clone, Copy)]
enum RainTreeRef<'a> {
    Portable(&'a [Vec<f64>; 5]),
    Retained(&'a [&'static [f64]; 5]),
}

impl<'a> RainTreeRef<'a> {
    /// Borrow one common tree column without copying either representation.
    fn column(self, index: usize) -> &'a [f64] {
        match self {
            Self::Portable(tree) => &tree[index],
            Self::Retained(tree) => tree[index],
        }
    }
}

/// Traverse one prevalidated portable or retained head with float32 split semantics.
fn score_rain_head(head: RainHeadRef<'_>, features: &[f32]) -> f32 {
    let base_score = head.base_score();
    let objective = head.objective();
    let mut margin = match objective {
        RainHeadObjective::BinaryLogistic => (base_score / (1.0 - base_score)).ln(),
        RainHeadObjective::RegGamma => base_score.ln(),
    };
    // accumulate one leaf margin from each validated tree
    match head {
        RainHeadRef::Portable(head) => {
            // retain source tree order from the uploaded artifact
            for tree in &head.trees {
                margin += score_rain_tree(RainTreeRef::Portable(tree), features);
            }
        }
        RainHeadRef::Retained(head) => {
            // retain source tree order from generated static tables
            for tree in head.trees {
                margin += score_rain_tree(RainTreeRef::Retained(tree), features);
            }
        }
    }
    let prediction = match objective {
        RainHeadObjective::BinaryLogistic => 1.0 / (1.0 + (-margin).exp()),
        RainHeadObjective::RegGamma => margin.exp(),
    };
    prediction as f32
}

/// Traverse one shared five-column tree and return its leaf margin.
fn score_rain_tree(tree: RainTreeRef<'_>, features: &[f32]) -> f64 {
    let mut node = 0_usize;
    // follow forward-only child links until the leaf marker
    while tree.column(2)[node] != -1.0 {
        let feature_index = tree.column(0)[node] as usize;
        let value = features.get(feature_index).copied().unwrap_or(f32::NAN);
        let go_left = if value.is_nan() {
            tree.column(4)[node] == 1.0
        } else {
            value < tree.column(1)[node] as f32
        };
        node = if go_left {
            tree.column(2)[node] as usize
        } else {
            tree.column(3)[node] as usize
        };
    }
    tree.column(1)[node]
}
