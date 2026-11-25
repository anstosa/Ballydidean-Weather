use weather_contracts::{
    ApiForecastTemperatureAdjustmentRuntime,
    ApiForecastTemperatureAdjustmentRuntimeActivationModeEnum,
    ApiForecastTemperatureAdjustmentRuntimeReasonCodeEnum,
    ApiForecastTemperatureAdjustmentRuntimeSource,
    ApiForecastTemperatureAdjustmentRuntimeSourceStateStatusEnum,
    ApiForecastTemperatureAdjustmentRuntimeStateEnum, ApiForecastWeatherRecord,
    ApplyTemperatureCanaryInputV1, EcmwfTemperatureCanaryRecentErrorState,
    EcmwfTemperatureCanaryRunModelCycleEnum, EcmwfTemperatureCanarySidecar,
    EcmwfTemperatureCanaryStatus, EcmwfTemperatureCanaryStatusStateStatusEnum,
    ForecastTemperatureCanaryDecisionV1, ForecastTemperatureCanaryDecisionV1ContractVersionLiteral,
    ForecastTemperatureCanaryDecisionV1StateEnum, Nullable, TemperatureCanaryDecisionReasonValue,
    TemperatureCanarySourceForecast, TemperatureCanarySourceForecastDatasetLiteral,
    TemperatureCanarySourceForecastModelCycleEnum,
    TemperatureCanarySourceForecastProviderKeyLiteral,
    TemperatureCanarySourceForecastUpstreamModelLiteral, TemperatureMosRuntimeRecentErrorState,
    TemperatureMosRuntimeRecentErrorStateCohortLiteral, WeatherRecordRow,
};

use crate::{CanonicalInstant, CurrentProjectionError, CurrentTimeResolver};

/// one admitted active temperature authority
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdmittedTemperatureCanaryAuthorization {
    TimeLimited {
        activated_at_epoch_ms: i64,
        authorization_sha256: String,
        expires_at: CanonicalInstant,
    },
    Permanent {
        activated_at_epoch_ms: i64,
        authorization_sha256: String,
    },
    MaintenanceQualified {
        action_sha256: String,
        full_member_root_sha256: String,
        policy_report_sha256: String,
    },
}

impl AdmittedTemperatureCanaryAuthorization {
    /// check whether this startup authority permits one request safety instant
    pub(crate) fn is_active_at(&self, epoch_milliseconds: i64) -> bool {
        // retain legacy clock geometry while qualified maintenance remains startup-authorized
        match self {
            Self::TimeLimited {
                activated_at_epoch_ms,
                expires_at,
                ..
            } => {
                epoch_milliseconds >= *activated_at_epoch_ms
                    && epoch_milliseconds < expires_at.epoch_milliseconds
            }
            Self::Permanent {
                activated_at_epoch_ms,
                ..
            } => epoch_milliseconds >= *activated_at_epoch_ms,
            Self::MaintenanceQualified { .. } => true,
        }
    }

    /// return the legacy authorization identity when this selection has one
    pub fn authorization_sha256(&self) -> Option<&str> {
        // maintenance qualification carries policy evidence instead of a canary authorization
        match self {
            Self::TimeLimited {
                authorization_sha256,
                ..
            }
            | Self::Permanent {
                authorization_sha256,
                ..
            } => Some(authorization_sha256),
            Self::MaintenanceQualified { .. } => None,
        }
    }
}

/// public evidence retained from one validated temperature bundle
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdmittedTemperatureCanaryEvidence {
    pub active_bundle_sha256: String,
}

/// prevalidated temperature runtime selection captured at construction
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdmittedTemperatureCanaryRuntime {
    pub authorization: AdmittedTemperatureCanaryAuthorization,
    pub evidence: AdmittedTemperatureCanaryEvidence,
    pub maximum_receipt_age_hours: u64,
    pub source_delay_hours: u64,
}

/// project one admitted temperature lifecycle at the request safety clock
pub fn project_temperature_canary_runtime(
    loaded_at: &CanonicalInstant,
    runtime: &AdmittedTemperatureCanaryRuntime,
    safety_at: &CanonicalInstant,
    source: Option<EcmwfTemperatureCanaryStatus>,
) -> ApiForecastTemperatureAdjustmentRuntime {
    let authorized = runtime
        .authorization
        .is_active_at(safety_at.epoch_milliseconds);
    let source = source.map(project_source_status);

    // redact all bundle identities outside the admitted authorization window
    if !authorized {
        return disabled_temperature_runtime(loaded_at, source);
    }

    // project the authority-specific public evidence without fabricating canary fields
    let (
        action_sha256,
        activation_mode,
        authorization_sha256,
        expires_at,
        full_member_root_sha256,
        policy_report_sha256,
    ) = match &runtime.authorization {
        AdmittedTemperatureCanaryAuthorization::TimeLimited {
            authorization_sha256,
            expires_at,
            ..
        } => (
            Nullable::Null,
            ApiForecastTemperatureAdjustmentRuntimeActivationModeEnum::TemperatureCanary,
            Nullable::Value(authorization_sha256.clone()),
            Nullable::Value(expires_at.utc.clone()),
            Nullable::Null,
            Nullable::Null,
        ),
        AdmittedTemperatureCanaryAuthorization::Permanent {
            authorization_sha256,
            ..
        } => (
            Nullable::Null,
            ApiForecastTemperatureAdjustmentRuntimeActivationModeEnum::TemperatureCanary,
            Nullable::Value(authorization_sha256.clone()),
            Nullable::Null,
            Nullable::Null,
            Nullable::Null,
        ),
        AdmittedTemperatureCanaryAuthorization::MaintenanceQualified {
            action_sha256,
            full_member_root_sha256,
            policy_report_sha256,
        } => (
            Nullable::Value(action_sha256.clone()),
            ApiForecastTemperatureAdjustmentRuntimeActivationModeEnum::MaintenanceQualified,
            Nullable::Null,
            Nullable::Null,
            Nullable::Value(full_member_root_sha256.clone()),
            Nullable::Value(policy_report_sha256.clone()),
        ),
    };

    ApiForecastTemperatureAdjustmentRuntime {
        actionSha256: action_sha256,
        activationMode: Nullable::Value(activation_mode),
        activeBundle: Nullable::Value(runtime.evidence.active_bundle_sha256.clone()),
        authorizationSha256: authorization_sha256,
        expiresAt: expires_at,
        fullMemberRootSha256: full_member_root_sha256,
        loadedAt: loaded_at.utc.clone(),
        policyReportSha256: policy_report_sha256,
        reasonCode: Nullable::Null,
        source: nullable(source),
        state: ApiForecastTemperatureAdjustmentRuntimeStateEnum::Active,
    }
}

/// build exact row inputs from one optional sidecar in repository order
pub fn temperature_canary_inputs(
    rows: &[WeatherRecordRow],
    sidecar: Option<&EcmwfTemperatureCanarySidecar>,
    safety_at: &CanonicalInstant,
    time_resolver: &dyn CurrentTimeResolver,
) -> Result<Vec<ApplyTemperatureCanaryInputV1>, CurrentProjectionError> {
    let recent_error_state = sidecar
        .map(|sidecar| Nullable::Value(project_recent_error_state(&sidecar.run.recentErrorState)));
    let mut inputs = Vec::with_capacity(rows.len());

    // preserve forecast row order while matching the last retained sidecar hour
    for row in rows {
        let valid_at = time_resolver.resolve(&row.validAt)?.utc;
        let hour = sidecar.and_then(|sidecar| {
            sidecar
                .hours
                .iter()
                .rev()
                .find(|hour| hour.validAt == valid_at)
        });
        let source_forecast = match (sidecar, hour, &row.upstreamModel) {
            (Some(sidecar), Some(hour), Nullable::Value(model)) if model == "best_match" => {
                Nullable::Value(TemperatureCanarySourceForecast {
                    adapterVersion:
                        weather_contracts::EcmwfTemperatureCanaryRunAdapterVersionLiteral::WIRE_VALUE
                            .to_owned(),
                    dataset: TemperatureCanarySourceForecastDatasetLiteral,
                    firstReceivedAt: sidecar.run.firstReceivedAt.clone(),
                    modelCycle: map_model_cycle(sidecar.run.modelCycle),
                    modelLeadHours: hour.modelLeadHours,
                    providerKey: TemperatureCanarySourceForecastProviderKeyLiteral,
                    providerResponseSha256: sidecar.run.providerResponseSha256.clone(),
                    rawRelativeHumidityPercent: hour.rawRelativeHumidityPercent.clone(),
                    rawTemperatureC: hour.rawTemperatureC,
                    rawWindSpeedMps: hour.rawWindSpeedMps.clone(),
                    runInitializedAt: sidecar.run.runInitializedAt.clone(),
                    upstreamModel: TemperatureCanarySourceForecastUpstreamModelLiteral,
                    validAt: hour.validAt.clone(),
                })
            }
            _ => Nullable::Null,
        };
        inputs.push(ApplyTemperatureCanaryInputV1 {
            evaluatedAt: safety_at.utc.clone(),
            rawBestMatchTemperatureC: row.temperatureC.clone(),
            recentErrorState: recent_error_state.clone().unwrap_or(Nullable::Null),
            sourceForecast: source_forecast,
            validAt: valid_at,
        });
    }

    Ok(inputs)
}

/// replace inactive defaults with aligned admitted temperature decisions
pub fn replace_temperature_adjustments(
    mut records: Vec<ApiForecastWeatherRecord>,
    decisions: Vec<ForecastTemperatureCanaryDecisionV1>,
) -> Result<Vec<ApiForecastWeatherRecord>, CurrentProjectionError> {
    // refuse partial decisions instead of shifting them onto different rows
    if records.len() != decisions.len() {
        return Err(CurrentProjectionError {
            code: "forecast_projection.temperature_adjustments",
            detail: "temperature adjustment outputs must align with forecast rows".to_owned(),
        });
    }

    // retain row order while changing only the temperature decision field
    for (record, decision) in records.iter_mut().zip(decisions) {
        record.temperatureAdjustment = decision;
    }
    Ok(records)
}

/// reproduce the Node containment fallback for one application exception
pub fn temperature_application_failure(
    raw_temperature: Nullable<f64>,
) -> ForecastTemperatureCanaryDecisionV1 {
    ForecastTemperatureCanaryDecisionV1 {
        branch: Nullable::Null,
        bundleSha256: Nullable::Null,
        contractVersion: ForecastTemperatureCanaryDecisionV1ContractVersionLiteral,
        correctedTemperatureC: Nullable::Null,
        rawBestMatchTemperatureC: raw_temperature,
        reasonCode: Nullable::Value(TemperatureCanaryDecisionReasonValue::BundleInvalid),
        recentErrorStateSha256: Nullable::Null,
        sourceForecast: Nullable::Null,
        state: ForecastTemperatureCanaryDecisionV1StateEnum::Disabled,
    }
}

/// project the bounded collector monitoring allowlist
fn project_source_status(
    source: EcmwfTemperatureCanaryStatus,
) -> ApiForecastTemperatureAdjustmentRuntimeSource {
    ApiForecastTemperatureAdjustmentRuntimeSource {
        adaptiveReady: source.stateStatus == EcmwfTemperatureCanaryStatusStateStatusEnum::Supported,
        firstReceivedAt: source.firstReceivedAt,
        hourCount: source.hourCount,
        latestRunInitializedAt: source.latestRunInitializedAt,
        stateReason: Nullable::Value(source.stateReason),
        stateStatus: map_source_status(source.stateStatus),
    }
}

/// map one generated collector status to the response vocabulary
fn map_source_status(
    status: EcmwfTemperatureCanaryStatusStateStatusEnum,
) -> ApiForecastTemperatureAdjustmentRuntimeSourceStateStatusEnum {
    // retain the closed collector status set
    match status {
        EcmwfTemperatureCanaryStatusStateStatusEnum::Supported => {
            ApiForecastTemperatureAdjustmentRuntimeSourceStateStatusEnum::Supported
        }
        EcmwfTemperatureCanaryStatusStateStatusEnum::Cold => {
            ApiForecastTemperatureAdjustmentRuntimeSourceStateStatusEnum::Cold
        }
        EcmwfTemperatureCanaryStatusStateStatusEnum::Insufficient => {
            ApiForecastTemperatureAdjustmentRuntimeSourceStateStatusEnum::Insufficient
        }
        EcmwfTemperatureCanaryStatusStateStatusEnum::Invalid => {
            ApiForecastTemperatureAdjustmentRuntimeSourceStateStatusEnum::Invalid
        }
    }
}

/// map one generated sidecar model-cycle identity to the application input
fn map_model_cycle(
    model_cycle: EcmwfTemperatureCanaryRunModelCycleEnum,
) -> TemperatureCanarySourceForecastModelCycleEnum {
    // retain the closed ECMWF model-cycle set
    match model_cycle {
        EcmwfTemperatureCanaryRunModelCycleEnum::Value49r1 => {
            TemperatureCanarySourceForecastModelCycleEnum::Value49r1
        }
        EcmwfTemperatureCanaryRunModelCycleEnum::Value50r1 => {
            TemperatureCanarySourceForecastModelCycleEnum::Value50r1
        }
    }
}

/// copy one admitted recent-error state to the model input type
fn project_recent_error_state(
    state: &EcmwfTemperatureCanaryRecentErrorState,
) -> TemperatureMosRuntimeRecentErrorState {
    TemperatureMosRuntimeRecentErrorState {
        b24C: state.b24C.clone(),
        b72C: state.b72C.clone(),
        cohort: TemperatureMosRuntimeRecentErrorStateCohortLiteral,
        localDates: state.localDates,
        mad72C: state.mad72C.clone(),
        maximumSourceRunInitializedAt: state.maximumSourceRunInitializedAt.clone(),
        maximumSourceValidAt: state.maximumSourceValidAt.clone(),
        n24: state.n24,
        n72: state.n72,
        sourceKeys: state.sourceKeys.clone(),
        supported: state.supported,
        targetRunInitializedAt: state.targetRunInitializedAt.clone(),
        windowEndValidAt: state.windowEndValidAt.clone(),
    }
}

/// project one expired canary lifecycle without model identities
fn disabled_temperature_runtime(
    loaded_at: &CanonicalInstant,
    source: Option<ApiForecastTemperatureAdjustmentRuntimeSource>,
) -> ApiForecastTemperatureAdjustmentRuntime {
    ApiForecastTemperatureAdjustmentRuntime {
        actionSha256: Nullable::Null,
        activationMode: Nullable::Null,
        activeBundle: Nullable::Null,
        authorizationSha256: Nullable::Null,
        expiresAt: Nullable::Null,
        fullMemberRootSha256: Nullable::Null,
        loadedAt: loaded_at.utc.clone(),
        policyReportSha256: Nullable::Null,
        reasonCode: Nullable::Value(
            ApiForecastTemperatureAdjustmentRuntimeReasonCodeEnum::CanaryExpired,
        ),
        source: nullable(source),
        state: ApiForecastTemperatureAdjustmentRuntimeStateEnum::Disabled,
    }
}

/// convert an optional value to the generated explicit-null shape
fn nullable<T>(value: Option<T>) -> Nullable<T> {
    // preserve absence as an explicit JSON null
    match value {
        Some(value) => Nullable::Value(value),
        None => Nullable::Null,
    }
}
