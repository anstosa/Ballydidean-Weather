use std::collections::HashSet;

use weather_contracts::{
    ApiForecastWeatherRecord, ApiRainAdjustmentDecision,
    ApiRainAdjustmentDecisionContractVersionLiteral, ApiRainAdjustmentDecisionSourceForecast,
    ApiRainAdjustmentDecisionSourceForecastProviderKeyLiteral,
    ApiRainAdjustmentDecisionSourceForecastUpstreamModelLiteral,
    ApiRainAdjustmentDecisionStateEnum, ApiRainAdjustmentRuntime,
    ApiRainAdjustmentRuntimeReasonCodeEnum, ApiRainAdjustmentRuntimeSource,
    ApiRainAdjustmentRuntimeStateEnum, ForecastRainAdjustmentStartupSnapshot,
    LoadedForecastAdjustmentRainRuntimeRegistryV1,
    LoadedForecastAdjustmentRainRuntimeRegistryV1ActiveArtifactSha256Literal,
    LoadedForecastAdjustmentRainRuntimeRegistryV1DisabledReasonCodeEnum, Nullable,
    WeatherRecordRow,
};

use crate::{CanonicalInstant, CurrentProjectionError, CurrentTimeResolver};

const HOUR_MILLISECONDS: i64 = 3_600_000;

/// one admitted startup rain policy frozen at service construction
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdmittedRainForecastRuntime {
    Active {
        loaded_at: String,
    },
    Disabled {
        loaded_at: String,
        reason_code: ApiRainAdjustmentRuntimeReasonCodeEnum,
    },
}

/// Admit the immutable generated startup selection without reparsing its captured clock.
pub fn admit_rain_forecast_runtime(
    snapshot: &ForecastRainAdjustmentStartupSnapshot,
) -> AdmittedRainForecastRuntime {
    // preserve the generated registry state and its exact startup refusal
    match &snapshot.runtime {
        LoadedForecastAdjustmentRainRuntimeRegistryV1::Active(_) => {
            AdmittedRainForecastRuntime::Active {
                loaded_at: snapshot.loadedAt.clone(),
            }
        }
        LoadedForecastAdjustmentRainRuntimeRegistryV1::Disabled(disabled) => {
            let reason_code = match disabled.reasonCode {
                LoadedForecastAdjustmentRainRuntimeRegistryV1DisabledReasonCodeEnum::PolicyRaw => {
                    ApiRainAdjustmentRuntimeReasonCodeEnum::PolicyRaw
                }
                LoadedForecastAdjustmentRainRuntimeRegistryV1DisabledReasonCodeEnum::RegistryInvalid => {
                    ApiRainAdjustmentRuntimeReasonCodeEnum::RegistryInvalid
                }
            };
            AdmittedRainForecastRuntime::Disabled {
                loaded_at: snapshot.loadedAt.clone(),
                reason_code,
            }
        }
    }
}

/// one canonical retained rain hour decoded by the repository adapter
#[derive(Debug, Clone, PartialEq)]
pub struct AdmittedRainAdjustmentHour {
    pub valid_at: CanonicalInstant,
    pub model_lead_hours: u64,
    pub raw_precipitation_mm: f64,
    pub corrected_precipitation_mm: f64,
    pub applied: bool,
    pub reason_code: Option<String>,
}

/// one canonical retained rain run decoded without exposing private inputs
#[derive(Debug, Clone, PartialEq)]
pub struct AdmittedRainAdjustmentRun {
    pub run_initialized_at: CanonicalInstant,
    pub model_sha256: String,
    pub first_received_at: CanonicalInstant,
    pub decision_at: CanonicalInstant,
    pub generated_at: CanonicalInstant,
    pub hours: Vec<AdmittedRainAdjustmentHour>,
}

/// validate one complete retained public-safe run at the request safety clock
pub fn valid_rain_adjustment_run(
    run: &AdmittedRainAdjustmentRun,
    safety_at: &CanonicalInstant,
) -> bool {
    // bind the retained output to the exact model shipped with this API build
    if run.model_sha256
        != LoadedForecastAdjustmentRainRuntimeRegistryV1ActiveArtifactSha256Literal::WIRE_VALUE
        || run.hours.is_empty()
        || run.hours.len() > 23
    {
        return false;
    }

    let initialized = run.run_initialized_at.epoch_milliseconds;
    let decision = run.decision_at.epoch_milliseconds;
    let received = run.first_received_at.epoch_milliseconds;
    let generated = run.generated_at.epoch_milliseconds;
    let safety = safety_at.epoch_milliseconds;

    // reject future, stale, late, and shifted retained evidence
    if initialized.rem_euclid(6 * HOUR_MILLISECONDS) != 0
        || decision != initialized + 8 * HOUR_MILLISECONDS
        || received < initialized
        || received > decision
        || decision > safety
        || generated < decision
        || generated > safety
        || safety - decision >= 12 * HOUR_MILLISECONDS
    {
        return false;
    }

    let mut leads = HashSet::with_capacity(run.hours.len());

    // reject a partially malformed or duplicate output projection
    for hour in &run.hours {
        let reason_valid = hour.reason_code.as_deref().is_some_and(valid_reason_code);

        // bind physical values and application state to the original model lead
        if !(9..=31).contains(&hour.model_lead_hours)
            || !leads.insert(hour.model_lead_hours)
            || hour.valid_at.epoch_milliseconds
                != initialized + hour.model_lead_hours as i64 * HOUR_MILLISECONDS
            || !hour.raw_precipitation_mm.is_finite()
            || !(0.0..=2000.0).contains(&hour.raw_precipitation_mm)
            || !hour.corrected_precipitation_mm.is_finite()
            || hour.corrected_precipitation_mm < 0.0
            || (hour.applied
                && (hour.corrected_precipitation_mm > 30.0 || hour.reason_code.is_some()))
            || (!hour.applied
                && (hour.corrected_precipitation_mm != hour.raw_precipitation_mm || !reason_valid))
        {
            return false;
        }
    }
    true
}

/// project exact per-row decisions from one optional retained run
pub fn project_rain_adjustments(
    rows: &[WeatherRecordRow],
    run: Option<&AdmittedRainAdjustmentRun>,
    safety_at: &CanonicalInstant,
    selection: Option<&AdmittedRainForecastRuntime>,
    time_resolver: &dyn CurrentTimeResolver,
) -> Result<Vec<ApiRainAdjustmentDecision>, CurrentProjectionError> {
    let run = run.filter(|run| valid_rain_adjustment_run(run, safety_at));
    project_admitted_rain_adjustments(rows, run, selection, time_resolver)
}

/// project decisions from one already validated optional retained run
pub(crate) fn project_admitted_rain_adjustments(
    rows: &[WeatherRecordRow],
    run: Option<&AdmittedRainAdjustmentRun>,
    selection: Option<&AdmittedRainForecastRuntime>,
    time_resolver: &dyn CurrentTimeResolver,
) -> Result<Vec<ApiRainAdjustmentDecision>, CurrentProjectionError> {
    let mut decisions = Vec::with_capacity(rows.len());

    // retain repository order while selecting only Best Match forecast identities
    for row in rows {
        let valid_at = time_resolver.resolve(&row.validAt)?;
        let selected_run = match &row.upstreamModel {
            Nullable::Value(model) if model == "best_match" => run,
            _ => None,
        };
        decisions.push(project_rain_adjustment(
            selected_run,
            selected_run.is_some(),
            &valid_at,
            row.precipitationMm.clone(),
            selection,
        ));
    }
    Ok(decisions)
}

/// project the response lifecycle from the same selected retained run
pub fn project_rain_adjustment_runtime(
    run: Option<&AdmittedRainAdjustmentRun>,
    safety_at: &CanonicalInstant,
    selection: Option<&AdmittedRainForecastRuntime>,
) -> ApiRainAdjustmentRuntime {
    let run = run.filter(|run| valid_rain_adjustment_run(run, safety_at));
    project_admitted_rain_adjustment_runtime(run, safety_at, selection)
}

/// project lifecycle state from one already validated optional retained run
pub(crate) fn project_admitted_rain_adjustment_runtime(
    run: Option<&AdmittedRainAdjustmentRun>,
    safety_at: &CanonicalInstant,
    selection: Option<&AdmittedRainForecastRuntime>,
) -> ApiRainAdjustmentRuntime {
    // let an explicit startup refusal override all retained output
    if let Some(AdmittedRainForecastRuntime::Disabled {
        loaded_at,
        reason_code,
    }) = selection
    {
        return ApiRainAdjustmentRuntime {
            state: ApiRainAdjustmentRuntimeStateEnum::Disabled,
            activeBundle: Nullable::Null,
            reasonCode: Nullable::Value(*reason_code),
            loadedAt: loaded_at.clone(),
            source: Nullable::Null,
        };
    }

    let loaded_at = match selection {
        Some(AdmittedRainForecastRuntime::Active { loaded_at }) => loaded_at.as_str(),
        Some(AdmittedRainForecastRuntime::Disabled { .. }) => {
            unreachable!("explicit disabled rain runtime returned above")
        }
        None => safety_at.utc.as_str(),
    };
    let active = admitted_rain_adjustment_model_enabled(run, selection);

    // expose only bounded retained-run provenance while an applied hour remains usable
    if let (true, Some(run)) = (active, run) {
        return ApiRainAdjustmentRuntime {
            state: ApiRainAdjustmentRuntimeStateEnum::Active,
            activeBundle: Nullable::Value(run.model_sha256.clone()),
            reasonCode: Nullable::Null,
            loadedAt: loaded_at.to_owned(),
            source: Nullable::Value(ApiRainAdjustmentRuntimeSource {
                runInitializedAt: run.run_initialized_at.utc.clone(),
                firstReceivedAt: run.first_received_at.utc.clone(),
                decisionAt: run.decision_at.utc.clone(),
                hourCount: run.hours.iter().filter(|hour| hour.applied).count() as u64,
            }),
        };
    }

    ApiRainAdjustmentRuntime {
        state: ApiRainAdjustmentRuntimeStateEnum::Disabled,
        activeBundle: Nullable::Null,
        reasonCode: Nullable::Value(ApiRainAdjustmentRuntimeReasonCodeEnum::ModelUnavailable),
        loadedAt: loaded_at.to_owned(),
        source: Nullable::Null,
    }
}

/// Report whether one retained run makes the selected startup model publicly available.
pub fn rain_adjustment_model_enabled(
    run: Option<&AdmittedRainAdjustmentRun>,
    safety_at: &CanonicalInstant,
    selection: Option<&AdmittedRainForecastRuntime>,
) -> bool {
    // an explicit startup refusal suppresses every retained model result
    if matches!(
        selection,
        Some(AdmittedRainForecastRuntime::Disabled { .. })
    ) {
        return false;
    }
    let run = run.filter(|run| valid_rain_adjustment_run(run, safety_at));
    admitted_rain_adjustment_model_enabled(run, selection)
}

/// report model availability from one already validated optional retained run
pub(crate) fn admitted_rain_adjustment_model_enabled(
    run: Option<&AdmittedRainAdjustmentRun>,
    selection: Option<&AdmittedRainForecastRuntime>,
) -> bool {
    // preserve an explicit startup refusal for internal admitted callers
    if matches!(
        selection,
        Some(AdmittedRainForecastRuntime::Disabled { .. })
    ) {
        return false;
    }
    run.is_some_and(|run| run.hours.iter().any(|hour| hour.applied))
}

/// replace inactive defaults with aligned admitted rain decisions
pub fn replace_rain_adjustments(
    mut records: Vec<ApiForecastWeatherRecord>,
    decisions: Vec<ApiRainAdjustmentDecision>,
) -> Result<Vec<ApiForecastWeatherRecord>, CurrentProjectionError> {
    // refuse partial decisions instead of shifting them onto different rows
    if records.len() != decisions.len() {
        return Err(CurrentProjectionError {
            code: "forecast_projection.rain_adjustments",
            detail: "rain adjustment outputs must align with forecast rows".to_owned(),
        });
    }

    // retain row order while changing only the rain decision field
    for (record, decision) in records.iter_mut().zip(decisions) {
        record.rainAdjustment = decision;
    }
    Ok(records)
}

/// project one exact per-row rain decision
fn project_rain_adjustment(
    run: Option<&AdmittedRainAdjustmentRun>,
    valid: bool,
    valid_at: &CanonicalInstant,
    raw_precipitation: Nullable<f64>,
    selection: Option<&AdmittedRainForecastRuntime>,
) -> ApiRainAdjustmentDecision {
    // let explicit startup policy override retained adjusted rows
    if let Some(AdmittedRainForecastRuntime::Disabled { reason_code, .. }) = selection {
        return disabled_decision(raw_precipitation, runtime_reason(*reason_code));
    }

    // select an hour only from a fully valid retained run
    let hour = if valid {
        run.and_then(|run| {
            run.hours
                .iter()
                .find(|hour| hour.valid_at.epoch_milliseconds == valid_at.epoch_milliseconds)
        })
    } else {
        None
    };
    // require one finite nonnegative public raw amount before exposing correction
    let raw_is_valid =
        matches!(raw_precipitation, Nullable::Value(value) if value.is_finite() && value >= 0.0);
    let active = hour.is_some_and(|hour| hour.applied) && raw_is_valid;

    // expose corrected output only when run, hour, and raw identity all remain admissible
    if let (true, Some(run), Some(hour)) = (active, run, hour) {
        return ApiRainAdjustmentDecision {
            contractVersion: ApiRainAdjustmentDecisionContractVersionLiteral,
            state: ApiRainAdjustmentDecisionStateEnum::Active,
            reasonCode: Nullable::Null,
            bundleSha256: Nullable::Value(run.model_sha256.clone()),
            correctedPrecipitationMm: Nullable::Value(hour.corrected_precipitation_mm),
            rawBestMatchPrecipitationMm: raw_precipitation,
            sourceForecast: Nullable::Value(ApiRainAdjustmentDecisionSourceForecast {
                runInitializedAt: run.run_initialized_at.utc.clone(),
                firstReceivedAt: run.first_received_at.utc.clone(),
                validAt: valid_at.utc.clone(),
                rawPrecipitationMm: hour.raw_precipitation_mm,
                modelLeadHours: hour.model_lead_hours,
                decisionAt: run.decision_at.utc.clone(),
                upstreamModel: ApiRainAdjustmentDecisionSourceForecastUpstreamModelLiteral,
                providerKey: ApiRainAdjustmentDecisionSourceForecastProviderKeyLiteral,
            }),
        };
    }

    // preserve invalid-run and valid-run row fallback reason precedence
    let reason = if !valid {
        "model_unavailable".to_owned()
    } else {
        hour.and_then(|hour| hour.reason_code.clone())
            .unwrap_or_else(|| "outside_operational_window".to_owned())
    };
    // distinguish valid-run row fallback from unavailable-run disablement
    let state = if valid {
        ApiRainAdjustmentDecisionStateEnum::RawFallback
    } else {
        ApiRainAdjustmentDecisionStateEnum::Disabled
    };
    // retain model provenance only for a completely valid run
    let bundle_sha256 = if valid {
        Nullable::Value(
            run.expect("valid retained run must be present")
                .model_sha256
                .clone(),
        )
    } else {
        Nullable::Null
    };
    ApiRainAdjustmentDecision {
        contractVersion: ApiRainAdjustmentDecisionContractVersionLiteral,
        state,
        reasonCode: Nullable::Value(reason),
        bundleSha256: bundle_sha256,
        correctedPrecipitationMm: Nullable::Null,
        rawBestMatchPrecipitationMm: raw_precipitation,
        sourceForecast: Nullable::Null,
    }
}

/// build one explicit startup-disabled row decision
fn disabled_decision(
    raw_precipitation: Nullable<f64>,
    reason_code: &'static str,
) -> ApiRainAdjustmentDecision {
    ApiRainAdjustmentDecision {
        contractVersion: ApiRainAdjustmentDecisionContractVersionLiteral,
        state: ApiRainAdjustmentDecisionStateEnum::Disabled,
        reasonCode: Nullable::Value(reason_code.to_owned()),
        bundleSha256: Nullable::Null,
        correctedPrecipitationMm: Nullable::Null,
        rawBestMatchPrecipitationMm: raw_precipitation,
        sourceForecast: Nullable::Null,
    }
}

/// map one startup runtime reason to the decision string vocabulary
fn runtime_reason(reason: ApiRainAdjustmentRuntimeReasonCodeEnum) -> &'static str {
    // preserve the generated startup reason set
    match reason {
        ApiRainAdjustmentRuntimeReasonCodeEnum::PolicyRaw => "policy_raw",
        ApiRainAdjustmentRuntimeReasonCodeEnum::RegistryInvalid => "registry_invalid",
        ApiRainAdjustmentRuntimeReasonCodeEnum::ModelUnavailable => "model_unavailable",
    }
}

/// validate the source-owned bounded snake-case reason format
fn valid_reason_code(reason: &str) -> bool {
    // preserve the exact 1..80 lowercase ASCII and underscore allowance
    !reason.is_empty()
        && reason.len() <= 80
        && reason
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte == b'_')
}
