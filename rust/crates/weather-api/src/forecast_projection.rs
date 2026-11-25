use std::borrow::Borrow;

use weather_adjustment::{
    ValidatedWindCanaryRuntime, WindCanaryActivationDecision, WindCanaryAuthorizationWindow,
    WindCanaryMetricScope, decide_wind_canary_activation,
};
use weather_contracts::{
    ApiForecastAdjustmentDecision, ApiForecastAdjustmentRuntime,
    ApiForecastAdjustmentRuntimeActivationModeEnum, ApiForecastAdjustmentRuntimeReasonCodeEnum,
    ApiForecastAdjustmentRuntimeStateEnum, ApiForecastWeatherRecord, ApiRainAdjustmentDecision,
    ApiRainAdjustmentDecisionContractVersionLiteral, ApiRainAdjustmentDecisionStateEnum,
    ApiRainAdjustmentRuntimeReasonCodeEnum, ApiSite, ApiWeatherRecord, ApiWeatherRecordMetadata,
    ApiWeatherRecordMetadataDevice, ApiWeatherRecordMetadataUpstream, ApiWeatherRecordMetrics,
    ApiWeatherRecordProvenance, ApiWeatherRecordProvenanceAttribution,
    CoreAdjustmentApplicationResult, CoreAdjustmentApplicationResultReasonEnum, FieldPresence,
    ForecastAdjustmentDecision, ForecastAdjustmentFailRawDecision,
    ForecastAdjustmentFailRawDecisionAdjustedMetricsRecord,
    ForecastAdjustmentFailRawDecisionAppliedMetricsEmptyArray,
    ForecastAdjustmentFailRawDecisionContractVersionLiteral,
    ForecastAdjustmentFailRawDecisionStateEnum, ForecastAdjustmentMetric,
    ForecastAdjustmentMetricBand, ForecastAdjustmentRawForecastProvenance,
    ForecastAdjustmentReasonCode, ForecastAdjustmentWindCanaryActiveDecision,
    ForecastAdjustmentWindCanaryActiveDecisionActivationKindLiteral,
    ForecastAdjustmentWindCanaryActiveDecisionAdjustedMetricsRecord,
    ForecastAdjustmentWindCanaryActiveDecisionAlgorithmContractVersionLiteral,
    ForecastAdjustmentWindCanaryActiveDecisionContractVersionLiteral,
    ForecastAdjustmentWindCanaryActiveDecisionReasonCodeNull,
    ForecastAdjustmentWindCanaryActiveDecisionStateLiteral, ForecastAdjustmentWindCanaryMetric,
    ForecastLeadBandKey, ForecastTemperatureCanaryDecisionV1,
    ForecastTemperatureCanaryDecisionV1ContractVersionLiteral,
    ForecastTemperatureCanaryDecisionV1StateEnum, Nullable, TemperatureCanaryDecisionReasonValue,
    WeatherRecordRow,
};

use crate::current_projection::{
    CanonicalInstant, CurrentProjectionError, CurrentTimeResolver, describe_freshness,
    index_sources, map_nullable_instant, project_provider, project_quality, source_kind_label,
};

/// inactive adjustment reasons from startup
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InactiveForecastDecisionConfig {
    pub rain_reason_code: ApiRainAdjustmentRuntimeReasonCodeEnum,
    pub adjustment_reason_code: ForecastAdjustmentReasonCode,
    pub temperature_reason_code: TemperatureCanaryDecisionReasonValue,
}

/// admitted active wind runtime identities
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdmittedWindCanaryEvidence {
    pub active_bundle_sha256: String,
    pub authorization_sha256: String,
    pub candidate_artifact_sha256: String,
    pub transfer_report_sha256: String,
}

/// prevalidated active wind runtime selection
#[derive(Debug, Clone, PartialEq)]
pub struct AdmittedWindCanaryRuntime {
    pub environment_kill_switch: Option<String>,
    pub authorization: WindCanaryAuthorizationWindow,
    pub expires_at: Option<CanonicalInstant>,
    pub enabled_metric_bands: Vec<ForecastAdjustmentMetricBand>,
    pub evaluated_at_epoch_ms: i64,
    pub evidence: AdmittedWindCanaryEvidence,
}

/// preverified corrections for one forecast row
#[derive(Debug, Clone, PartialEq)]
pub struct AdmittedWindCorrections {
    pub lead_band: ForecastLeadBandKey,
    pub raw_forecast_provenance: ForecastAdjustmentRawForecastProvenance,
    pub wind_direction_degrees: Option<CoreAdjustmentApplicationResult>,
    pub wind_gust_mps: Option<CoreAdjustmentApplicationResult>,
    pub wind_speed_mps: Option<CoreAdjustmentApplicationResult>,
}

/// admitted row result after source and lead checks
#[derive(Debug, Clone, PartialEq)]
pub enum AdmittedWindRowOutput {
    FailRaw(ForecastAdjustmentReasonCode),
    Corrections(Box<AdmittedWindCorrections>),
    Decision(Box<ForecastAdjustmentDecision>),
}

impl Default for InactiveForecastDecisionConfig {
    /// use the node defaults without startup registries
    fn default() -> Self {
        Self {
            rain_reason_code: ApiRainAdjustmentRuntimeReasonCodeEnum::ModelUnavailable,
            adjustment_reason_code: ForecastAdjustmentReasonCode::RegistryInactive,
            temperature_reason_code: TemperatureCanaryDecisionReasonValue::RegistryInactive,
        }
    }
}

/// project ordered inactive forecast rows
pub fn project_forecast_weather_records(
    rows: Vec<WeatherRecordRow>,
    site: &ApiSite,
    data_at: &CanonicalInstant,
    time_resolver: &dyn CurrentTimeResolver,
    decision_config: InactiveForecastDecisionConfig,
) -> Result<Vec<ApiForecastWeatherRecord>, CurrentProjectionError> {
    validate_inactive_config(decision_config)?;
    let raw_records = project_raw_forecast_records(&rows, site, data_at, time_resolver)?;
    let adjustments = rows
        .iter()
        .map(|_| inactive_adjustment_decision(decision_config.adjustment_reason_code))
        .collect();

    Ok(append_forecast_decisions(
        &rows,
        raw_records,
        adjustments,
        decision_config,
    ))
}

/// project forecast rows with admitted active wind outputs
pub fn project_forecast_weather_records_with_wind_adjustments(
    rows: Vec<WeatherRecordRow>,
    site: &ApiSite,
    data_at: &CanonicalInstant,
    time_resolver: &dyn CurrentTimeResolver,
    decision_config: InactiveForecastDecisionConfig,
    runtime: &AdmittedWindCanaryRuntime,
    row_outputs: &[AdmittedWindRowOutput],
) -> Result<Vec<ApiForecastWeatherRecord>, CurrentProjectionError> {
    validate_inactive_config(decision_config)?;

    // require one admitted runtime result per row
    if row_outputs.len() != rows.len() {
        return Err(CurrentProjectionError {
            code: "forecast_projection.adjustments",
            detail: "wind adjustment outputs must align with forecast rows".to_owned(),
        });
    }

    let raw_records = project_raw_forecast_records(&rows, site, data_at, time_resolver)?;
    let adjustments = rows
        .iter()
        .zip(row_outputs)
        .map(|(row, output)| project_wind_adjustment(row, runtime, || output))
        .collect();

    Ok(append_forecast_decisions(
        &rows,
        raw_records,
        adjustments,
        decision_config,
    ))
}

/// project forecast rows with already admitted generic adjustment decisions
pub(crate) fn project_forecast_weather_records_with_adjustment_decisions<Apply>(
    rows: Vec<WeatherRecordRow>,
    site: &ApiSite,
    data_at: &CanonicalInstant,
    time_resolver: &dyn CurrentTimeResolver,
    decision_config: InactiveForecastDecisionConfig,
    mut apply: Apply,
) -> Result<Vec<ApiForecastWeatherRecord>, CurrentProjectionError>
where
    Apply: FnMut(&WeatherRecordRow) -> ApiForecastAdjustmentDecision,
{
    validate_inactive_config(decision_config)?;
    let raw_records = project_raw_forecast_records(&rows, site, data_at, time_resolver)?;
    // apply only after every raw row has projected successfully
    let adjustments = rows.iter().map(&mut apply).collect();
    Ok(append_forecast_decisions(
        &rows,
        raw_records,
        adjustments,
        decision_config,
    ))
}

/// project one row-local adjustment failure without hiding sibling decisions
pub(crate) fn forecast_adjustment_application_failure() -> ApiForecastAdjustmentDecision {
    fail_raw_adjustment(
        ForecastAdjustmentReasonCode::AdjustmentError,
        ForecastAdjustmentFailRawDecisionStateEnum::NotApplicable,
    )
}

/// project one admitted wind canary lifecycle at its request safety clock
pub fn project_wind_canary_runtime(
    loaded_at: &CanonicalInstant,
    runtime: &AdmittedWindCanaryRuntime,
) -> Result<ApiForecastAdjustmentRuntime, CurrentProjectionError> {
    project_wind_canary_runtime_at(loaded_at, runtime, runtime.evaluated_at_epoch_ms)
}

/// Project lifecycle at an explicit request clock without cloning startup material.
pub(crate) fn project_wind_canary_runtime_at(
    loaded_at: &CanonicalInstant,
    runtime: &AdmittedWindCanaryRuntime,
    evaluated_at_epoch_ms: i64,
) -> Result<ApiForecastAdjustmentRuntime, CurrentProjectionError> {
    // require the admitted display instant to match the authorization geometry
    match (runtime.authorization, runtime.expires_at.as_ref()) {
        (
            WindCanaryAuthorizationWindow::TimeLimited {
                expires_at_epoch_ms,
                ..
            },
            Some(expires_at),
        ) if expires_at.epoch_milliseconds == expires_at_epoch_ms => {}
        (WindCanaryAuthorizationWindow::Permanent { .. }, None) => {}
        _ => {
            return Err(CurrentProjectionError {
                code: "forecast_projection.runtime",
                detail: "wind canary authorization geometry is invalid".to_owned(),
            });
        }
    }

    let activation = decide_wind_canary_activation(
        runtime.environment_kill_switch.as_deref(),
        ValidatedWindCanaryRuntime::Candidate {
            authorization: runtime.authorization,
            enabled_metric_bands: &runtime.enabled_metric_bands,
        },
        evaluated_at_epoch_ms,
        ForecastAdjustmentMetric::WindSpeedMps,
        ForecastLeadBandKey::Value001024,
    );

    // redact all candidate evidence when the request clock disables the canary
    if let WindCanaryActivationDecision::Disabled(reason) = activation {
        return Ok(disabled_wind_runtime(
            loaded_at,
            wind_runtime_disabled_reason(reason),
        ));
    }

    let mut enabled_metrics = Vec::new();

    // preserve source order while collapsing repeated lead-band metrics
    for metric_band in &runtime.enabled_metric_bands {
        // retain only the first occurrence of each public metric identity
        if !enabled_metrics.contains(&metric_band.metric) {
            enabled_metrics.push(metric_band.metric);
        }
    }

    Ok(ApiForecastAdjustmentRuntime {
        actionSha256: FieldPresence::Missing,
        activationMode: Nullable::Value(ApiForecastAdjustmentRuntimeActivationModeEnum::WindCanary),
        activeBundle: Nullable::Value(runtime.evidence.active_bundle_sha256.clone()),
        authorizationSha256: Nullable::Value(runtime.evidence.authorization_sha256.clone()),
        candidateArtifactSha256: Nullable::Value(
            runtime.evidence.candidate_artifact_sha256.clone(),
        ),
        enabledMetrics: enabled_metrics,
        evaluationReportSha256: Nullable::Null,
        // preserve finite and permanent authorization response shapes
        expiresAt: match &runtime.expires_at {
            Some(expires_at) => Nullable::Value(expires_at.utc.clone()),
            None => Nullable::Null,
        },
        fullMemberRootSha256: FieldPresence::Missing,
        loadedAt: loaded_at.utc.clone(),
        qualificationReceiptSha256: Nullable::Null,
        reasonCode: Nullable::Null,
        state: ApiForecastAdjustmentRuntimeStateEnum::Active,
        policyReportSha256: FieldPresence::Missing,
        transferReportSha256: Nullable::Value(runtime.evidence.transfer_report_sha256.clone()),
    })
}

/// project one safety-disabled wind lifecycle without candidate evidence
fn disabled_wind_runtime(
    loaded_at: &CanonicalInstant,
    reason_code: ApiForecastAdjustmentRuntimeReasonCodeEnum,
) -> ApiForecastAdjustmentRuntime {
    ApiForecastAdjustmentRuntime {
        actionSha256: FieldPresence::Missing,
        activationMode: Nullable::Null,
        activeBundle: Nullable::Null,
        authorizationSha256: Nullable::Null,
        candidateArtifactSha256: Nullable::Null,
        enabledMetrics: Vec::new(),
        evaluationReportSha256: Nullable::Null,
        expiresAt: Nullable::Null,
        fullMemberRootSha256: FieldPresence::Missing,
        loadedAt: loaded_at.utc.clone(),
        qualificationReceiptSha256: Nullable::Null,
        reasonCode: Nullable::Value(reason_code),
        state: ApiForecastAdjustmentRuntimeStateEnum::Disabled,
        policyReportSha256: FieldPresence::Missing,
        transferReportSha256: Nullable::Null,
    }
}

/// map one disabled canary decision to its public lifecycle reason
fn wind_runtime_disabled_reason(
    reason: weather_contracts::LoadedForecastAdjustmentWindCanaryRuntimeV1DisabledReasonCodeEnum,
) -> ApiForecastAdjustmentRuntimeReasonCodeEnum {
    // preserve the shared generated disabled-reason vocabulary
    match reason {
        weather_contracts::LoadedForecastAdjustmentWindCanaryRuntimeV1DisabledReasonCodeEnum::BundleInvalid => {
            ApiForecastAdjustmentRuntimeReasonCodeEnum::BundleInvalid
        }
        weather_contracts::LoadedForecastAdjustmentWindCanaryRuntimeV1DisabledReasonCodeEnum::BundleMissing => {
            ApiForecastAdjustmentRuntimeReasonCodeEnum::BundleMissing
        }
        weather_contracts::LoadedForecastAdjustmentWindCanaryRuntimeV1DisabledReasonCodeEnum::CanaryExpired => {
            ApiForecastAdjustmentRuntimeReasonCodeEnum::CanaryExpired
        }
        weather_contracts::LoadedForecastAdjustmentWindCanaryRuntimeV1DisabledReasonCodeEnum::CanaryKilled => {
            ApiForecastAdjustmentRuntimeReasonCodeEnum::CanaryKilled
        }
        weather_contracts::LoadedForecastAdjustmentWindCanaryRuntimeV1DisabledReasonCodeEnum::RegistryInactive => {
            ApiForecastAdjustmentRuntimeReasonCodeEnum::RegistryInactive
        }
        weather_contracts::LoadedForecastAdjustmentWindCanaryRuntimeV1DisabledReasonCodeEnum::RegistryInvalid => {
            ApiForecastAdjustmentRuntimeReasonCodeEnum::RegistryInvalid
        }
    }
}

/// project optional raw pressure context without forecast adjustment decisions
pub fn project_forecast_pressure_context(
    rows: Vec<WeatherRecordRow>,
    site: &ApiSite,
    data_at: &CanonicalInstant,
    time_resolver: &dyn CurrentTimeResolver,
) -> Vec<ApiWeatherRecord> {
    // isolate malformed optional context from the primary forecast response
    project_raw_forecast_records(&rows, site, data_at, time_resolver).unwrap_or_default()
}

/// project ordered raw forecast records
fn project_raw_forecast_records(
    rows: &[WeatherRecordRow],
    site: &ApiSite,
    data_at: &CanonicalInstant,
    time_resolver: &dyn CurrentTimeResolver,
) -> Result<Vec<ApiWeatherRecord>, CurrentProjectionError> {
    let sources = index_sources(site);
    let mut raw_records = Vec::with_capacity(rows.len());

    // validate and project every raw row first
    for row in rows {
        let source = sources
            .get(row.sourceId.as_str())
            .ok_or_else(|| CurrentProjectionError {
                code: "forecast_projection.source",
                detail: format!("Missing active metadata for source {}", row.sourceId),
            })?;
        let valid_at = time_resolver.resolve(&row.validAt)?;
        let record = ApiWeatherRecord {
            freshness: describe_freshness(valid_at.epoch_milliseconds, data_at.epoch_milliseconds),
            id: row.id.clone(),
            metadata: project_metadata(row, time_resolver)?,
            metrics: project_metrics(row),
            productRunAt: map_nullable_instant(&row.productRunAt, time_resolver)?,
            provenance: ApiWeatherRecordProvenance {
                attribution: ApiWeatherRecordProvenanceAttribution {
                    label: source.attribution.label.clone(),
                    url: source.attribution.url.clone(),
                },
                label: source_kind_label(row.sourceKind).to_owned(),
                providerKey: source.providerKey.clone(),
                sourceId: row.sourceId.clone(),
                sourceKey: source.key.clone(),
                sourceKind: row.sourceKind,
                stationSlug: row.stationSlug.clone(),
            },
            receivedAt: time_resolver.resolve(&row.lastReceivedAt)?.utc,
            revisionCount: row.revisionCount,
            validAt: valid_at.utc,
        };
        raw_records.push(record);
    }

    Ok(raw_records)
}

/// append aligned adjustment decisions
fn append_forecast_decisions(
    rows: &[WeatherRecordRow],
    raw_records: Vec<ApiWeatherRecord>,
    adjustments: Vec<ApiForecastAdjustmentDecision>,
    decision_config: InactiveForecastDecisionConfig,
) -> Vec<ApiForecastWeatherRecord> {
    let mut records = Vec::with_capacity(raw_records.len());

    // append decisions only after raw projection succeeds
    for ((row, record), adjustment) in rows.iter().zip(raw_records).zip(adjustments) {
        let rain_adjustment = inactive_rain_decision(row, decision_config.rain_reason_code);
        let temperature_adjustment =
            inactive_temperature_decision(row, decision_config.temperature_reason_code);
        records.push(ApiForecastWeatherRecord {
            freshness: record.freshness,
            id: record.id,
            metadata: record.metadata,
            metrics: record.metrics,
            productRunAt: record.productRunAt,
            provenance: record.provenance,
            receivedAt: record.receivedAt,
            revisionCount: record.revisionCount,
            validAt: record.validAt,
            rainAdjustment: rain_adjustment,
            adjustment,
            temperatureAdjustment: temperature_adjustment,
        });
    }

    records
}

/// validate admitted inactive runtime reasons
fn validate_inactive_config(
    config: InactiveForecastDecisionConfig,
) -> Result<(), CurrentProjectionError> {
    // require one startup-disabled generic reason
    if !matches!(
        config.adjustment_reason_code,
        ForecastAdjustmentReasonCode::BundleInvalid
            | ForecastAdjustmentReasonCode::BundleMissing
            | ForecastAdjustmentReasonCode::CanaryExpired
            | ForecastAdjustmentReasonCode::CanaryKilled
            | ForecastAdjustmentReasonCode::PolicyRaw
            | ForecastAdjustmentReasonCode::RegistryInactive
            | ForecastAdjustmentReasonCode::RegistryInvalid
    ) {
        return Err(CurrentProjectionError {
            code: "forecast_projection.decision_config",
            detail: "generic adjustment reason is not an inactive runtime reason".to_owned(),
        });
    }

    // require one startup-disabled temperature reason
    if !matches!(
        config.temperature_reason_code,
        TemperatureCanaryDecisionReasonValue::BundleInvalid
            | TemperatureCanaryDecisionReasonValue::BundleMissing
            | TemperatureCanaryDecisionReasonValue::CanaryExpired
            | TemperatureCanaryDecisionReasonValue::CanaryKilled
            | TemperatureCanaryDecisionReasonValue::PolicyRaw
            | TemperatureCanaryDecisionReasonValue::RegistryInactive
            | TemperatureCanaryDecisionReasonValue::RegistryInvalid
    ) {
        return Err(CurrentProjectionError {
            code: "forecast_projection.decision_config",
            detail: "temperature adjustment reason is not an inactive runtime reason".to_owned(),
        });
    }

    Ok(())
}

/// project the inactive rain fallback
fn inactive_rain_decision(
    row: &WeatherRecordRow,
    reason_code: ApiRainAdjustmentRuntimeReasonCodeEnum,
) -> ApiRainAdjustmentDecision {
    ApiRainAdjustmentDecision {
        contractVersion: ApiRainAdjustmentDecisionContractVersionLiteral,
        state: ApiRainAdjustmentDecisionStateEnum::Disabled,
        reasonCode: Nullable::Value(rain_reason_code(reason_code).to_owned()),
        bundleSha256: Nullable::Null,
        correctedPrecipitationMm: Nullable::Null,
        rawBestMatchPrecipitationMm: row.precipitationMm.clone(),
        sourceForecast: Nullable::Null,
    }
}

/// map one inactive rain reason
fn rain_reason_code(reason_code: ApiRainAdjustmentRuntimeReasonCodeEnum) -> &'static str {
    // preserve the startup rain reason literals
    match reason_code {
        ApiRainAdjustmentRuntimeReasonCodeEnum::PolicyRaw => "policy_raw",
        ApiRainAdjustmentRuntimeReasonCodeEnum::RegistryInvalid => "registry_invalid",
        ApiRainAdjustmentRuntimeReasonCodeEnum::ModelUnavailable => "model_unavailable",
    }
}

/// project one admitted wind runtime result
pub(crate) fn project_wind_adjustment<Output>(
    row: &WeatherRecordRow,
    runtime: &AdmittedWindCanaryRuntime,
    output: impl FnOnce() -> Output,
) -> ApiForecastAdjustmentDecision
where
    Output: Borrow<AdmittedWindRowOutput>,
{
    let validated_runtime = ValidatedWindCanaryRuntime::Candidate {
        authorization: runtime.authorization,
        enabled_metric_bands: &runtime.enabled_metric_bands,
    };
    let global_activation = decide_wind_canary_activation(
        runtime.environment_kill_switch.as_deref(),
        validated_runtime,
        runtime.evaluated_at_epoch_ms,
        ForecastAdjustmentMetric::WindSpeedMps,
        ForecastLeadBandKey::Value001024,
    );

    // let global kill and authorization state override row failures
    if let WindCanaryActivationDecision::Disabled(reason) = global_activation {
        return fail_raw_adjustment(
            wind_disabled_reason(reason),
            ForecastAdjustmentFailRawDecisionStateEnum::Disabled,
        );
    }

    let output = output();
    let output = output.borrow();

    // reuse the exact generated model decision without applying corrections twice
    if let AdmittedWindRowOutput::Decision(decision) = output {
        return ApiForecastAdjustmentDecision::Domain(decision.as_ref().clone());
    }

    // retain explicit upstream source or lead failures
    let AdmittedWindRowOutput::Corrections(corrections) = output else {
        let AdmittedWindRowOutput::FailRaw(reason) = output else {
            unreachable!("closed admitted wind row output")
        };
        return fail_raw_adjustment(
            *reason,
            ForecastAdjustmentFailRawDecisionStateEnum::NotApplicable,
        );
    };

    let metrics = [
        (
            ForecastAdjustmentWindCanaryMetric::WindDirectionDegrees,
            ForecastAdjustmentMetric::WindDirectionDegrees,
            &row.windDirectionDegrees,
            corrections.wind_direction_degrees.as_ref(),
        ),
        (
            ForecastAdjustmentWindCanaryMetric::WindGustMps,
            ForecastAdjustmentMetric::WindGustMps,
            &row.windGustMps,
            corrections.wind_gust_mps.as_ref(),
        ),
        (
            ForecastAdjustmentWindCanaryMetric::WindSpeedMps,
            ForecastAdjustmentMetric::WindSpeedMps,
            &row.windSpeedMps,
            corrections.wind_speed_mps.as_ref(),
        ),
    ];
    let mut adjusted_metrics = Vec::new();
    let mut applied_metrics = Vec::new();
    let mut failures = Vec::new();

    // retain the source wind allowlist order
    for (wind_metric, metric, raw_value, correction) in metrics {
        // skip absent provider metrics without inventing values
        if matches!(raw_value, Nullable::Null) {
            continue;
        }

        let activation = decide_wind_canary_activation(
            runtime.environment_kill_switch.as_deref(),
            validated_runtime,
            runtime.evaluated_at_epoch_ms,
            metric,
            corrections.lead_band,
        );

        // reject a late global activation inconsistency
        if let WindCanaryActivationDecision::Disabled(reason) = activation {
            return fail_raw_adjustment(
                wind_disabled_reason(reason),
                ForecastAdjustmentFailRawDecisionStateEnum::Disabled,
            );
        }

        // skip runtime-disabled metric-band pairs
        if activation == WindCanaryActivationDecision::Active(WindCanaryMetricScope::NotEnabled) {
            failures.push(CoreAdjustmentApplicationResultReasonEnum::DisabledMetricBand);
            continue;
        }

        let Some(correction) = correction else {
            return fail_raw_adjustment(
                ForecastAdjustmentReasonCode::AdjustmentError,
                ForecastAdjustmentFailRawDecisionStateEnum::NotApplicable,
            );
        };

        // record only valid successful derived metrics
        if correction.applied {
            if correction.reason != CoreAdjustmentApplicationResultReasonEnum::Adjusted
                || !correction.adjustedValue.is_finite()
            {
                return fail_raw_adjustment(
                    ForecastAdjustmentReasonCode::AdjustmentError,
                    ForecastAdjustmentFailRawDecisionStateEnum::NotApplicable,
                );
            }
            adjusted_metrics.push((wind_metric, correction.adjustedValue));
            applied_metrics.push(wind_metric);
        } else {
            failures.push(correction.reason);
        }
    }

    // remain raw when no metric can be safely adjusted
    if applied_metrics.is_empty() {
        let reason = failures
            .first()
            .copied()
            .map(core_failure_reason)
            .unwrap_or(ForecastAdjustmentReasonCode::MetricNotEnabled);
        return fail_raw_adjustment(
            reason,
            ForecastAdjustmentFailRawDecisionStateEnum::NotApplicable,
        );
    }

    ApiForecastAdjustmentDecision::Domain(ForecastAdjustmentDecision::WindCanaryActive(
        ForecastAdjustmentWindCanaryActiveDecision {
            activationKind: ForecastAdjustmentWindCanaryActiveDecisionActivationKindLiteral,
            adjustedMetrics: ForecastAdjustmentWindCanaryActiveDecisionAdjustedMetricsRecord(
                adjusted_metrics,
            ),
            algorithmContractVersion:
                ForecastAdjustmentWindCanaryActiveDecisionAlgorithmContractVersionLiteral,
            appliedMetrics: applied_metrics,
            authorizationSha256: runtime.evidence.authorization_sha256.clone(),
            candidateArtifactSha256: runtime.evidence.candidate_artifact_sha256.clone(),
            contractVersion: ForecastAdjustmentWindCanaryActiveDecisionContractVersionLiteral,
            leadBand: corrections.lead_band,
            rawForecastProvenance: corrections.raw_forecast_provenance.clone(),
            reasonCode: ForecastAdjustmentWindCanaryActiveDecisionReasonCodeNull,
            state: ForecastAdjustmentWindCanaryActiveDecisionStateLiteral,
            transferReportSha256: runtime.evidence.transfer_report_sha256.clone(),
        },
    ))
}

/// map one disabled wind runtime reason
fn wind_disabled_reason(
    reason: weather_contracts::LoadedForecastAdjustmentWindCanaryRuntimeV1DisabledReasonCodeEnum,
) -> ForecastAdjustmentReasonCode {
    // preserve the generated runtime reason vocabulary
    match reason {
        weather_contracts::LoadedForecastAdjustmentWindCanaryRuntimeV1DisabledReasonCodeEnum::BundleInvalid => {
            ForecastAdjustmentReasonCode::BundleInvalid
        }
        weather_contracts::LoadedForecastAdjustmentWindCanaryRuntimeV1DisabledReasonCodeEnum::BundleMissing => {
            ForecastAdjustmentReasonCode::BundleMissing
        }
        weather_contracts::LoadedForecastAdjustmentWindCanaryRuntimeV1DisabledReasonCodeEnum::CanaryExpired => {
            ForecastAdjustmentReasonCode::CanaryExpired
        }
        weather_contracts::LoadedForecastAdjustmentWindCanaryRuntimeV1DisabledReasonCodeEnum::CanaryKilled => {
            ForecastAdjustmentReasonCode::CanaryKilled
        }
        weather_contracts::LoadedForecastAdjustmentWindCanaryRuntimeV1DisabledReasonCodeEnum::RegistryInactive => {
            ForecastAdjustmentReasonCode::RegistryInactive
        }
        weather_contracts::LoadedForecastAdjustmentWindCanaryRuntimeV1DisabledReasonCodeEnum::RegistryInvalid => {
            ForecastAdjustmentReasonCode::RegistryInvalid
        }
    }
}

/// map one core fail-raw reason
fn core_failure_reason(
    reason: CoreAdjustmentApplicationResultReasonEnum,
) -> ForecastAdjustmentReasonCode {
    // retain the node public reason mapping
    match reason {
        CoreAdjustmentApplicationResultReasonEnum::Adjusted => {
            ForecastAdjustmentReasonCode::AdjustmentError
        }
        CoreAdjustmentApplicationResultReasonEnum::CalendarFingerprintMismatch => {
            ForecastAdjustmentReasonCode::RuntimeFingerprintMismatch
        }
        CoreAdjustmentApplicationResultReasonEnum::CoefficientMissing
        | CoreAdjustmentApplicationResultReasonEnum::RootMissing => {
            ForecastAdjustmentReasonCode::CoefficientMissing
        }
        CoreAdjustmentApplicationResultReasonEnum::DisabledMetricBand => {
            ForecastAdjustmentReasonCode::MetricNotEnabled
        }
        CoreAdjustmentApplicationResultReasonEnum::ForecastIdentityMismatch => {
            ForecastAdjustmentReasonCode::IdentityMismatch
        }
        CoreAdjustmentApplicationResultReasonEnum::RawDirectionCalm => {
            ForecastAdjustmentReasonCode::DirectionCalm
        }
        CoreAdjustmentApplicationResultReasonEnum::RawValueInvalid => {
            ForecastAdjustmentReasonCode::MetricOutOfBounds
        }
        CoreAdjustmentApplicationResultReasonEnum::RawValueOod => {
            ForecastAdjustmentReasonCode::TrainingEnvelopeMismatch
        }
    }
}

/// project one fail-raw generic decision
pub(crate) fn fail_raw_adjustment(
    reason_code: ForecastAdjustmentReasonCode,
    state: ForecastAdjustmentFailRawDecisionStateEnum,
) -> ApiForecastAdjustmentDecision {
    ApiForecastAdjustmentDecision::Domain(ForecastAdjustmentDecision::FailRaw(
        ForecastAdjustmentFailRawDecision {
            adjustedMetrics: ForecastAdjustmentFailRawDecisionAdjustedMetricsRecord,
            appliedMetrics: ForecastAdjustmentFailRawDecisionAppliedMetricsEmptyArray,
            contractVersion: ForecastAdjustmentFailRawDecisionContractVersionLiteral,
            reasonCode: reason_code,
            state,
        },
    ))
}

/// project the inactive generic fallback
fn inactive_adjustment_decision(
    reason_code: ForecastAdjustmentReasonCode,
) -> ApiForecastAdjustmentDecision {
    fail_raw_adjustment(
        reason_code,
        ForecastAdjustmentFailRawDecisionStateEnum::Disabled,
    )
}

/// project the inactive temperature fallback
fn inactive_temperature_decision(
    row: &WeatherRecordRow,
    reason_code: TemperatureCanaryDecisionReasonValue,
) -> ForecastTemperatureCanaryDecisionV1 {
    ForecastTemperatureCanaryDecisionV1 {
        branch: Nullable::Null,
        bundleSha256: Nullable::Null,
        contractVersion: ForecastTemperatureCanaryDecisionV1ContractVersionLiteral,
        correctedTemperatureC: Nullable::Null,
        rawBestMatchTemperatureC: row.temperatureC.clone(),
        reasonCode: Nullable::Value(reason_code),
        recentErrorStateSha256: Nullable::Null,
        sourceForecast: Nullable::Null,
        state: ForecastTemperatureCanaryDecisionV1StateEnum::Disabled,
    }
}

/// project forecast-safe public metadata
fn project_metadata(
    row: &WeatherRecordRow,
    time_resolver: &dyn CurrentTimeResolver,
) -> Result<ApiWeatherRecordMetadata, CurrentProjectionError> {
    // omit fully absent device metadata
    let device = match (&row.deviceVendor, &row.deviceModel, &row.deviceSerial) {
        (Nullable::Null, Nullable::Null, Nullable::Null) => Nullable::Null,
        _ => Nullable::Value(ApiWeatherRecordMetadataDevice {
            model: row.deviceModel.clone(),
            serial: row.deviceSerial.clone(),
            vendor: row.deviceVendor.clone(),
        }),
    };
    let quality = project_quality(&row.qualityMetadata)?;
    let provider = project_provider(&row.providerMetadata, None, time_resolver)?;

    Ok(ApiWeatherRecordMetadata {
        device,
        provider,
        quality,
        upstream: ApiWeatherRecordMetadataUpstream {
            model: row.upstreamModel.clone(),
            timezone: row.upstreamTimezone.clone(),
        },
    })
}

/// copy canonical forecast metrics
fn project_metrics(row: &WeatherRecordRow) -> ApiWeatherRecordMetrics {
    ApiWeatherRecordMetrics {
        apparentTemperatureC: row.apparentTemperatureC.clone(),
        blackGlobeTemperatureC: row.blackGlobeTemperatureC.clone(),
        cloudCoverPercent: row.cloudCoverPercent.clone(),
        pm25MicrogramsPerCubicMeter: row.pm25MicrogramsPerCubicMeter.clone(),
        precipitationMm: row.precipitationMm.clone(),
        precipitationRateMmPerHour: row.precipitationRateMmPerHour.clone(),
        pressureHpa: row.pressureHpa.clone(),
        relativeHumidityPercent: row.relativeHumidityPercent.clone(),
        soilElectricalConductivityMicrosiemensPerCm: row
            .soilElectricalConductivityMicrosiemensPerCm
            .clone(),
        soilMoisturePercent: row.soilMoisturePercent.clone(),
        solarRadiationWm2: row.solarRadiationWm2.clone(),
        temperatureC: row.temperatureC.clone(),
        uvIndex: row.uvIndex.clone(),
        windDirectionDegrees: row.windDirectionDegrees.clone(),
        windGustMps: row.windGustMps.clone(),
        windSpeedMps: row.windSpeedMps.clone(),
        wetBulbGlobeTemperatureC: row.wetBulbGlobeTemperatureC.clone(),
    }
}
