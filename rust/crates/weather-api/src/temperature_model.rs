use weather_adjustment::{
    AdmittedTemperatureMosApplicationInput, AdmittedTemperatureMosCalendar,
    BorrowedTemperatureMosServingModel, apply_admitted_temperature_mos_forecast,
};
use weather_contracts::{
    ApplyTemperatureCanaryInputV1, ForecastTemperatureCanaryDecisionV1,
    ForecastTemperatureCanaryDecisionV1BranchEnum,
    ForecastTemperatureCanaryDecisionV1ContractVersionLiteral,
    ForecastTemperatureCanaryDecisionV1SourceForecast,
    ForecastTemperatureCanaryDecisionV1StateEnum, JsDateOrString, Nullable,
    TemperatureCanaryDecisionReasonValue, TemperatureCanarySourceForecast,
    TemperatureCanarySourceForecastModelCycleEnum, TemperatureMosRuntimeAppliedResultBranchEnum,
    TemperatureMosRuntimeForecastModelCycleEnum, TemperatureMosRuntimeModelMaterialScopeEnum,
    TemperatureMosRuntimeRawReason, TemperatureMosRuntimeRawResult,
    TemperatureMosRuntimeRawResultAppliedLiteral, TemperatureMosRuntimeResult,
    TemperatureMosServingModel,
};

use crate::maintenance_temperature_model_causality::validate_mos_model_causality;
use crate::maintenance_temperature_recent_error_admission::validate_mos_recent_error_state;
use crate::{
    AdmittedTemperatureCanaryRuntime, CanonicalInstant, CurrentProjectionError,
    CurrentTimeResolver, ForecastTemperatureAdjustmentBoundary,
};

const HOUR_MILLISECONDS: i64 = 3_600_000;
const MODEL_50R1_CUTOVER_MILLISECONDS: i64 = 1_778_565_600_000;

/// admitted source calendar and model-era identity owner
pub trait ForecastTemperatureMosAdmissionBoundary {
    /// return already resolved harmonics after validating source cycle identity
    fn admit_calendar(
        &self,
        source: &TemperatureCanarySourceForecast,
        run_initialized_at: &CanonicalInstant,
        valid_at: &CanonicalInstant,
    ) -> Result<AdmittedTemperatureMosCalendar, CurrentProjectionError>;

    /// Resolve the original site-local date and hour for monthly model scope and embargo.
    fn local_date_hour(
        &self,
        utc: &CanonicalInstant,
    ) -> Result<(String, u8), CurrentProjectionError>;
}

/// Admitted startup identity of the bundle that supplied one numerical model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AdmittedTemperatureMosModelOrigin<'a> {
    active_bundle_sha256: &'a str,
}

impl<'a> AdmittedTemperatureMosModelOrigin<'a> {
    /// Bind one already verified active bundle identity to its selected model.
    pub const fn admitted(active_bundle_sha256: &'a str) -> Self {
        Self {
            active_bundle_sha256,
        }
    }
}

/// one prevalidated selected model and its non-codec admission owners
#[derive(Clone, Copy)]
pub struct AdmittedTemperatureMosModelOwner<'a, Model = &'a TemperatureMosServingModel> {
    pub model: Model,
    pub origin: AdmittedTemperatureMosModelOrigin<'a>,
    pub served_adapter_version: &'a str,
    pub time_resolver: &'a dyn CurrentTimeResolver,
    pub admission: &'a dyn ForecastTemperatureMosAdmissionBoundary,
}

impl<'a, Model> ForecastTemperatureAdjustmentBoundary
    for AdmittedTemperatureMosModelOwner<'a, Model>
where
    Model: Copy + Into<BorrowedTemperatureMosServingModel<'a>>,
{
    /// apply the admitted source, receipt, calendar, and model material
    fn apply(
        &self,
        runtime: &AdmittedTemperatureCanaryRuntime,
        input: &ApplyTemperatureCanaryInputV1,
        recent_error_state_sha256: Option<&str>,
    ) -> Result<ForecastTemperatureCanaryDecisionV1, CurrentProjectionError> {
        Ok(self.apply_inner(runtime, input, recent_error_state_sha256))
    }
}

impl<'a, Model> AdmittedTemperatureMosModelOwner<'a, Model>
where
    Model: Copy + Into<BorrowedTemperatureMosServingModel<'a>>,
{
    /// project one exact Node-compatible canary decision without serialization or I/O
    fn apply_inner(
        &self,
        runtime: &AdmittedTemperatureCanaryRuntime,
        input: &ApplyTemperatureCanaryInputV1,
        recent_error_state_sha256: Option<&str>,
    ) -> ForecastTemperatureCanaryDecisionV1 {
        // reject startup composition drift before inspecting source or running inference
        if runtime.evidence.active_bundle_sha256 != self.origin.active_bundle_sha256 {
            return fallback_decision(
                input.rawBestMatchTemperatureC.clone(),
                ForecastTemperatureCanaryDecisionV1StateEnum::Disabled,
                TemperatureCanaryDecisionReasonValue::BundleInvalid,
                Nullable::Null,
            );
        }

        // require the coupled sidecar source and causal material before authorization checks
        let (Nullable::Value(source), Nullable::Value(recent_error_state)) =
            (&input.sourceForecast, &input.recentErrorState)
        else {
            return fallback_decision(
                input.rawBestMatchTemperatureC.clone(),
                ForecastTemperatureCanaryDecisionV1StateEnum::RawFallback,
                TemperatureCanaryDecisionReasonValue::MissingSourceForecast,
                Nullable::Value(runtime.evidence.active_bundle_sha256.clone()),
            );
        };

        let evaluated_at = match resolve_instant(self.time_resolver, &input.evaluatedAt) {
            Ok(value) => value,
            Err(reason) => {
                return fallback_decision(
                    input.rawBestMatchTemperatureC.clone(),
                    ForecastTemperatureCanaryDecisionV1StateEnum::RawFallback,
                    reason,
                    Nullable::Value(runtime.evidence.active_bundle_sha256.clone()),
                );
            }
        };

        // recheck request-time authorization after proving a source is present
        if !runtime_is_authorized(runtime, &evaluated_at) {
            return fallback_decision(
                input.rawBestMatchTemperatureC.clone(),
                ForecastTemperatureCanaryDecisionV1StateEnum::Disabled,
                TemperatureCanaryDecisionReasonValue::CanaryExpired,
                Nullable::Null,
            );
        }

        AuthorityFreeTemperatureMosOwner::new(
            &runtime.evidence.active_bundle_sha256,
            self.model.into(),
            self.served_adapter_version,
            runtime.maximum_receipt_age_hours,
            runtime.source_delay_hours,
            self.time_resolver,
            self.admission,
        )
        .apply_admitted(
            input,
            source,
            recent_error_state,
            &evaluated_at,
            recent_error_state_sha256,
        )
    }
}

/// Authority-free source and numerical owner used only after package admission.
pub(crate) struct AuthorityFreeTemperatureMosOwner<'a> {
    bundle_sha256: &'a str,
    model: BorrowedTemperatureMosServingModel<'a>,
    served_adapter_version: &'a str,
    maximum_receipt_age_hours: u64,
    source_delay_hours: u64,
    time_resolver: &'a dyn CurrentTimeResolver,
    admission: &'a dyn ForecastTemperatureMosAdmissionBoundary,
}

impl<'a> AuthorityFreeTemperatureMosOwner<'a> {
    /// Bind a verified candidate package without constructing activation authority.
    pub(crate) const fn new(
        bundle_sha256: &'a str,
        model: BorrowedTemperatureMosServingModel<'a>,
        served_adapter_version: &'a str,
        maximum_receipt_age_hours: u64,
        source_delay_hours: u64,
        time_resolver: &'a dyn CurrentTimeResolver,
        admission: &'a dyn ForecastTemperatureMosAdmissionBoundary,
    ) -> Self {
        Self {
            bundle_sha256,
            model,
            served_adapter_version,
            maximum_receipt_age_hours,
            source_delay_hours,
            time_resolver,
            admission,
        }
    }

    /// Apply an admitted candidate source without checking any activation window.
    pub(crate) fn apply(
        &self,
        input: &ApplyTemperatureCanaryInputV1,
        recent_error_state_sha256: Option<&str>,
    ) -> ForecastTemperatureCanaryDecisionV1 {
        // require the same coupled source and causal state as the served path
        let (Nullable::Value(source), Nullable::Value(recent_error_state)) =
            (&input.sourceForecast, &input.recentErrorState)
        else {
            return fallback_decision(
                input.rawBestMatchTemperatureC.clone(),
                ForecastTemperatureCanaryDecisionV1StateEnum::RawFallback,
                TemperatureCanaryDecisionReasonValue::MissingSourceForecast,
                Nullable::Value(self.bundle_sha256.to_owned()),
            );
        };
        let evaluated_at = match resolve_instant(self.time_resolver, &input.evaluatedAt) {
            Ok(value) => value,
            Err(reason) => {
                return fallback_decision(
                    input.rawBestMatchTemperatureC.clone(),
                    ForecastTemperatureCanaryDecisionV1StateEnum::RawFallback,
                    reason,
                    Nullable::Value(self.bundle_sha256.to_owned()),
                );
            }
        };
        self.apply_admitted(
            input,
            source,
            recent_error_state,
            &evaluated_at,
            recent_error_state_sha256,
        )
    }

    /// Apply one present source after the caller's authorization boundary is complete.
    fn apply_admitted(
        &self,
        input: &ApplyTemperatureCanaryInputV1,
        source: &TemperatureCanarySourceForecast,
        recent_error_state: &weather_contracts::TemperatureMosRuntimeRecentErrorState,
        evaluated_at: &CanonicalInstant,
        recent_error_state_sha256: Option<&str>,
    ) -> ForecastTemperatureCanaryDecisionV1 {
        let admitted = match self.admit_source(input, source, evaluated_at) {
            Ok(admitted) => admitted,
            Err(reason) => {
                return fallback_decision(
                    input.rawBestMatchTemperatureC.clone(),
                    ForecastTemperatureCanaryDecisionV1StateEnum::RawFallback,
                    reason,
                    Nullable::Value(self.bundle_sha256.to_owned()),
                );
            }
        };
        let result = self
            .evaluate_admitted_mos(source, &admitted, recent_error_state)
            .unwrap_or_else(|reason| {
                TemperatureMosRuntimeResult::Raw(TemperatureMosRuntimeRawResult {
                    applied: TemperatureMosRuntimeRawResultAppliedLiteral,
                    predictionTemperatureC: source.rawTemperatureC,
                    rawTemperatureC: source.rawTemperatureC,
                    reason,
                })
            });
        let source_forecast = decision_source(source, self.source_delay_hours);
        let state_sha256 = recent_error_state_sha256
            .map(|value| Nullable::Value(value.to_owned()))
            .unwrap_or(Nullable::Null);

        // preserve the numerical core's active versus raw branch without changing raw metrics
        match result {
            TemperatureMosRuntimeResult::Applied(applied) => ForecastTemperatureCanaryDecisionV1 {
                branch: Nullable::Value(map_branch(applied.branch)),
                bundleSha256: Nullable::Value(self.bundle_sha256.to_owned()),
                contractVersion: ForecastTemperatureCanaryDecisionV1ContractVersionLiteral,
                correctedTemperatureC: Nullable::Value(applied.predictionTemperatureC),
                rawBestMatchTemperatureC: input.rawBestMatchTemperatureC.clone(),
                reasonCode: Nullable::Null,
                recentErrorStateSha256: state_sha256,
                sourceForecast: Nullable::Value(source_forecast),
                state: ForecastTemperatureCanaryDecisionV1StateEnum::Active,
            },
            TemperatureMosRuntimeResult::Raw(raw) => ForecastTemperatureCanaryDecisionV1 {
                branch: Nullable::Null,
                bundleSha256: Nullable::Value(self.bundle_sha256.to_owned()),
                contractVersion: ForecastTemperatureCanaryDecisionV1ContractVersionLiteral,
                correctedTemperatureC: Nullable::Null,
                rawBestMatchTemperatureC: input.rawBestMatchTemperatureC.clone(),
                reasonCode: Nullable::Value(map_raw_reason(raw.reason)),
                recentErrorStateSha256: state_sha256,
                sourceForecast: Nullable::Value(source_forecast),
                state: ForecastTemperatureCanaryDecisionV1StateEnum::RawFallback,
            },
        }
    }

    /// Check truthful model and per-run evidence before evaluating any numerical feature.
    fn evaluate_admitted_mos(
        &self,
        source: &TemperatureCanarySourceForecast,
        admitted: &AdmittedSource,
        recent_error_state: &weather_contracts::TemperatureMosRuntimeRecentErrorState,
    ) -> Result<TemperatureMosRuntimeResult, TemperatureMosRuntimeRawReason> {
        let run = &admitted.run_initialized_at;
        let valid = &admitted.valid_at;
        validate_mos_forecast(self.model, source, run)?;
        validate_mos_model_causality(self.model, run, valid, self.time_resolver, self.admission)?;
        validate_mos_recent_error_state(recent_error_state, run, self.time_resolver)?;
        let calendar = self
            .admission
            .admit_calendar(source, run, valid)
            .map_err(|_| TemperatureMosRuntimeRawReason::InvalidForecast)?;
        Ok(apply_admitted_temperature_mos_forecast(
            AdmittedTemperatureMosApplicationInput {
                calendar,
                model: self.model,
                model_cycle: map_model_cycle(source.modelCycle),
                model_lead_hours: source.modelLeadHours,
                raw_relative_humidity_percent: &source.rawRelativeHumidityPercent,
                raw_temperature_c: source.rawTemperatureC,
                raw_wind_speed_mps: &source.rawWindSpeedMps,
                recent_error_state,
            },
        ))
    }

    /// admit source identity, geometry, availability, and bounded receipt age
    fn admit_source(
        &self,
        input: &ApplyTemperatureCanaryInputV1,
        source: &TemperatureCanarySourceForecast,
        evaluated_at: &CanonicalInstant,
    ) -> Result<AdmittedSource, TemperatureCanaryDecisionReasonValue> {
        // bind the one variable source identity not already closed by generated literals
        if source.adapterVersion != self.served_adapter_version {
            return Err(TemperatureCanaryDecisionReasonValue::SourceIdentityMismatch);
        }
        let run_initialized_at = resolve_instant(self.time_resolver, &source.runInitializedAt)?;
        let first_received_at = resolve_instant(self.time_resolver, &source.firstReceivedAt)?;
        let valid_at = resolve_instant(self.time_resolver, &source.validAt)?;

        let lead_milliseconds = i64::try_from(source.modelLeadHours)
            .ok()
            .and_then(|lead| lead.checked_mul(HOUR_MILLISECONDS))
            .ok_or(TemperatureCanaryDecisionReasonValue::SourceTimeMismatch)?;
        let actual_lead_milliseconds = valid_at
            .epoch_milliseconds
            .checked_sub(run_initialized_at.epoch_milliseconds)
            .ok_or(TemperatureCanaryDecisionReasonValue::SourceTimeMismatch)?;
        // bind the sidecar hour to the requested row and its initialization lead
        if source.validAt != input.validAt || actual_lead_milliseconds != lead_milliseconds {
            return Err(TemperatureCanaryDecisionReasonValue::SourceTimeMismatch);
        }

        let assumed_available_at = i64::try_from(self.source_delay_hours)
            .ok()
            .and_then(|delay| delay.checked_mul(HOUR_MILLISECONDS))
            .and_then(|delay| run_initialized_at.epoch_milliseconds.checked_add(delay))
            .ok_or(TemperatureCanaryDecisionReasonValue::SourceIdentityMismatch)?;
        let available_at = first_received_at
            .epoch_milliseconds
            .max(assumed_available_at);
        // reject backdated or not-yet-available acquisition receipts
        if first_received_at.epoch_milliseconds < run_initialized_at.epoch_milliseconds
            || evaluated_at.epoch_milliseconds < available_at
        {
            return Err(TemperatureCanaryDecisionReasonValue::SourceNotAvailable);
        }

        let maximum_age = i64::try_from(self.maximum_receipt_age_hours)
            .ok()
            .and_then(|hours| hours.checked_mul(HOUR_MILLISECONDS))
            .ok_or(TemperatureCanaryDecisionReasonValue::SourceIdentityMismatch)?;
        let receipt_age = evaluated_at
            .epoch_milliseconds
            .checked_sub(available_at)
            .ok_or(TemperatureCanaryDecisionReasonValue::SourceNotAvailable)?;
        // retain Node's strict-greater-than receipt expiry boundary
        if receipt_age > maximum_age {
            return Err(TemperatureCanaryDecisionReasonValue::SourceStale);
        }
        // constrain serving to the fitted delayed operational window
        if valid_at.epoch_milliseconds <= evaluated_at.epoch_milliseconds
            || !(7..=18).contains(&source.modelLeadHours)
            || source.modelLeadHours < self.source_delay_hours
        {
            return Err(TemperatureCanaryDecisionReasonValue::OutsideOperationalWindow);
        }

        Ok(AdmittedSource {
            run_initialized_at,
            valid_at,
        })
    }
}

struct AdmittedSource {
    run_initialized_at: CanonicalInstant,
    valid_at: CanonicalInstant,
}

/// Retain the pure MOS provider cycle, finite features and selected scope guards.
fn validate_mos_forecast(
    model: BorrowedTemperatureMosServingModel<'_>,
    source: &TemperatureCanarySourceForecast,
    run: &CanonicalInstant,
) -> Result<(), TemperatureMosRuntimeRawReason> {
    let cycle = if run.epoch_milliseconds >= MODEL_50R1_CUTOVER_MILLISECONDS {
        TemperatureCanarySourceForecastModelCycleEnum::Value50r1
    } else {
        TemperatureCanarySourceForecastModelCycleEnum::Value49r1
    };
    // require an actual six-hour initialization and the matching numerical predictor era
    if !source.rawTemperatureC.is_finite()
        || run.epoch_milliseconds.rem_euclid(6 * HOUR_MILLISECONDS) != 0
        || source.modelCycle != cycle
        || [&source.rawRelativeHumidityPercent, &source.rawWindSpeedMps]
            .into_iter()
            .any(|value| matches!(value, Nullable::Value(number) if !number.is_finite()))
    {
        return Err(TemperatureMosRuntimeRawReason::InvalidForecast);
    }
    let scope = match model {
        BorrowedTemperatureMosServingModel::Monthly(model) => model.scope,
        BorrowedTemperatureMosServingModel::Permanent(model) => model.scope,
    };
    // preserve Node scope rejection ahead of model and state validation
    match scope {
        TemperatureMosRuntimeModelMaterialScopeEnum::InitializationFirst12
            if !(1..=12).contains(&source.modelLeadHours) =>
        {
            Err(TemperatureMosRuntimeRawReason::OutsideInitializationFirst12)
        }
        TemperatureMosRuntimeModelMaterialScopeEnum::AssumedDelay6Next12
            if !(7..=18).contains(&source.modelLeadHours) =>
        {
            Err(TemperatureMosRuntimeRawReason::OutsideAssumedDelay6Next12)
        }
        _ => Ok(()),
    }
}

/// resolve one canonical string through the admitted time owner
fn resolve_instant(
    resolver: &dyn CurrentTimeResolver,
    value: &str,
) -> Result<CanonicalInstant, TemperatureCanaryDecisionReasonValue> {
    resolver
        .resolve(&JsDateOrString::String(value.encode_utf16().collect()))
        .map_err(|_| TemperatureCanaryDecisionReasonValue::SourceIdentityMismatch)
}

/// check one cached authorization against the request safety instant
fn runtime_is_authorized(
    runtime: &AdmittedTemperatureCanaryRuntime,
    evaluated_at: &CanonicalInstant,
) -> bool {
    runtime
        .authorization
        .is_active_at(evaluated_at.epoch_milliseconds)
}

/// build one raw-preserving decision without model or source evidence
fn fallback_decision(
    raw_temperature: Nullable<f64>,
    state: ForecastTemperatureCanaryDecisionV1StateEnum,
    reason: TemperatureCanaryDecisionReasonValue,
    bundle_sha256: Nullable<String>,
) -> ForecastTemperatureCanaryDecisionV1 {
    ForecastTemperatureCanaryDecisionV1 {
        branch: Nullable::Null,
        bundleSha256: bundle_sha256,
        contractVersion: ForecastTemperatureCanaryDecisionV1ContractVersionLiteral,
        correctedTemperatureC: Nullable::Null,
        rawBestMatchTemperatureC: raw_temperature,
        reasonCode: Nullable::Value(reason),
        recentErrorStateSha256: Nullable::Null,
        sourceForecast: Nullable::Null,
        state,
    }
}

/// append the source delay-derived operational horizon to admitted provenance
fn decision_source(
    source: &TemperatureCanarySourceForecast,
    source_delay_hours: u64,
) -> ForecastTemperatureCanaryDecisionV1SourceForecast {
    ForecastTemperatureCanaryDecisionV1SourceForecast {
        adapterVersion: source.adapterVersion.clone(),
        dataset: source.dataset,
        firstReceivedAt: source.firstReceivedAt.clone(),
        modelCycle: source.modelCycle,
        modelLeadHours: source.modelLeadHours,
        providerKey: source.providerKey,
        providerResponseSha256: source.providerResponseSha256.clone(),
        rawRelativeHumidityPercent: source.rawRelativeHumidityPercent.clone(),
        rawTemperatureC: source.rawTemperatureC,
        rawWindSpeedMps: source.rawWindSpeedMps.clone(),
        runInitializedAt: source.runInitializedAt.clone(),
        upstreamModel: source.upstreamModel,
        validAt: source.validAt.clone(),
        operationalHorizonHours: source.modelLeadHours - source_delay_hours,
    }
}

/// map the source canary cycle into the numerical model cycle
fn map_model_cycle(
    cycle: TemperatureCanarySourceForecastModelCycleEnum,
) -> TemperatureMosRuntimeForecastModelCycleEnum {
    // preserve the generated two-era cycle set
    match cycle {
        TemperatureCanarySourceForecastModelCycleEnum::Value49r1 => {
            TemperatureMosRuntimeForecastModelCycleEnum::Value49r1
        }
        TemperatureCanarySourceForecastModelCycleEnum::Value50r1 => {
            TemperatureMosRuntimeForecastModelCycleEnum::Value50r1
        }
    }
}

/// map the numerical branch into the public decision branch
fn map_branch(
    branch: TemperatureMosRuntimeAppliedResultBranchEnum,
) -> ForecastTemperatureCanaryDecisionV1BranchEnum {
    // preserve the generated closed branch set
    match branch {
        TemperatureMosRuntimeAppliedResultBranchEnum::Adaptive => {
            ForecastTemperatureCanaryDecisionV1BranchEnum::Adaptive
        }
        TemperatureMosRuntimeAppliedResultBranchEnum::Direct => {
            ForecastTemperatureCanaryDecisionV1BranchEnum::Direct
        }
    }
}

/// map every numerical raw reason into the public canary vocabulary
fn map_raw_reason(reason: TemperatureMosRuntimeRawReason) -> TemperatureCanaryDecisionReasonValue {
    // retain the complete generated source reason set
    match reason {
        TemperatureMosRuntimeRawReason::InvalidForecast => {
            TemperatureCanaryDecisionReasonValue::InvalidForecast
        }
        TemperatureMosRuntimeRawReason::InferenceError => {
            TemperatureCanaryDecisionReasonValue::InferenceError
        }
        TemperatureMosRuntimeRawReason::InvalidModel => {
            TemperatureCanaryDecisionReasonValue::InvalidModel
        }
        TemperatureMosRuntimeRawReason::InvalidRecentErrorState => {
            TemperatureCanaryDecisionReasonValue::InvalidRecentErrorState
        }
        TemperatureMosRuntimeRawReason::ModelIdentityMismatch => {
            TemperatureCanaryDecisionReasonValue::ModelIdentityMismatch
        }
        TemperatureMosRuntimeRawReason::ModelNotSupported => {
            TemperatureCanaryDecisionReasonValue::ModelNotSupported
        }
        TemperatureMosRuntimeRawReason::ModelNotYetAvailable => {
            TemperatureCanaryDecisionReasonValue::ModelNotYetAvailable
        }
        TemperatureMosRuntimeRawReason::OutsideAssumedDelay6Next12 => {
            TemperatureCanaryDecisionReasonValue::OutsideAssumedDelay6Next12
        }
        TemperatureMosRuntimeRawReason::OutsideInitializationFirst12 => {
            TemperatureCanaryDecisionReasonValue::OutsideInitializationFirst12
        }
        TemperatureMosRuntimeRawReason::RecentErrorStateAsOfMismatch => {
            TemperatureCanaryDecisionReasonValue::RecentErrorStateAsOfMismatch
        }
        TemperatureMosRuntimeRawReason::RecentErrorStateContainsFutureData => {
            TemperatureCanaryDecisionReasonValue::RecentErrorStateContainsFutureData
        }
        TemperatureMosRuntimeRawReason::RecentErrorStateOutsideWindow => {
            TemperatureCanaryDecisionReasonValue::RecentErrorStateOutsideWindow
        }
        TemperatureMosRuntimeRawReason::RecentErrorStateRunMismatch => {
            TemperatureCanaryDecisionReasonValue::RecentErrorStateRunMismatch
        }
        TemperatureMosRuntimeRawReason::RecentErrorStateSourceRunMismatch => {
            TemperatureCanaryDecisionReasonValue::RecentErrorStateSourceRunMismatch
        }
        TemperatureMosRuntimeRawReason::StrengthBandNotSupported => {
            TemperatureCanaryDecisionReasonValue::StrengthBandNotSupported
        }
        TemperatureMosRuntimeRawReason::UnsupportedCohort => {
            TemperatureCanaryDecisionReasonValue::UnsupportedCohort
        }
    }
}
