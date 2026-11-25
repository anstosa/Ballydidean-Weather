//! admit the loader-selected standard qualified forecast adjustment runtime

use weather_adjustment::{
    AdmittedWindRowFeatures, AdmittedWindV2MetricOrder, evaluate_admitted_wind_v2_row,
};
use weather_contracts::{
    ApiForecastAdjustmentDecision, ApiForecastAdjustmentRuntime,
    ApiForecastAdjustmentRuntimeActivationModeEnum, ApiForecastAdjustmentRuntimeStateEnum,
    FieldPresence, ForecastAdjustmentActiveDecision,
    ForecastAdjustmentActiveDecisionAdjustedMetricsRecord,
    ForecastAdjustmentActiveDecisionAlgorithmContractVersionLiteral,
    ForecastAdjustmentActiveDecisionContractVersionLiteral,
    ForecastAdjustmentActiveDecisionReasonCodeNull, ForecastAdjustmentActiveDecisionStateLiteral,
    ForecastAdjustmentCandidateV2, ForecastAdjustmentFailRawDecisionStateEnum,
    ForecastAdjustmentMetric, ForecastAdjustmentStartupSnapshot, JsDateOrString,
    LoadedApiForecastAdjustmentRuntime, LoadedForecastAdjustmentRuntimeV1,
    LoadedForecastAdjustmentRuntimeV1Active, Nullable, WeatherRecordRow,
};

use crate::{
    CanonicalInstant, CurrentProjectionError, CurrentTimeResolver,
    ForecastMaintenanceWindAdjustmentBoundary, ForecastMaintenanceWindModelAdmissionBoundary,
    WindForecastRowAdmission, forecast_projection::fail_raw_adjustment,
    wind_model::canonical_weather_metrics,
};

/// immutable identities retained after selecting one standard qualified startup snapshot
#[derive(Debug, PartialEq)]
pub struct StandardForecastAdjustmentStartup {
    loaded_at: CanonicalInstant,
    active_bundle_sha256: String,
    candidate_artifact_sha256: String,
    evaluation_report_sha256: String,
    qualification_receipt_sha256: String,
    enabled_metrics: Vec<ForecastAdjustmentMetric>,
}

impl StandardForecastAdjustmentStartup {
    /// admit only the active standard grammar selected by the original loader
    pub fn from_snapshot(
        snapshot: &ForecastAdjustmentStartupSnapshot,
        time: &dyn CurrentTimeResolver,
    ) -> Result<Option<Self>, CurrentProjectionError> {
        let Some(active) = standard_selection(snapshot) else {
            return Ok(None);
        };
        let loaded_at = resolve_instant(time, &snapshot.loadedAt)?;
        let mut enabled_metrics = Vec::new();
        // preserve first occurrence order from the generated candidate pairs
        for pair in &active.bundle.candidate.enabledMetricBands {
            // deduplicate only the public metric projection, as the original Set does
            if !enabled_metrics.contains(&pair.metric) {
                enabled_metrics.push(pair.metric);
            }
        }

        Ok(Some(Self {
            loaded_at,
            active_bundle_sha256: active.bundle.bundleSha256.clone(),
            candidate_artifact_sha256: active.bundle.candidate.candidateArtifactSha256.clone(),
            evaluation_report_sha256: active
                .bundle
                .evaluationReport
                .evaluationReportSha256
                .clone(),
            qualification_receipt_sha256: active
                .bundle
                .qualificationReceipt
                .qualificationReceiptSha256
                .clone(),
            enabled_metrics,
        }))
    }

    /// rebind the borrowed candidate only when every cached public identity still matches
    pub fn model_owner<'a>(
        &'a self,
        snapshot: &'a ForecastAdjustmentStartupSnapshot,
        admission: &'a dyn ForecastMaintenanceWindModelAdmissionBoundary,
    ) -> Option<AdmittedStandardForecastAdjustmentOwner<'a>> {
        let active = standard_selection(snapshot)?;
        let bundle = &active.bundle;
        // reject a caller-supplied snapshot with a different qualified selection
        if snapshot.loadedAt != self.loaded_at.utc
            || bundle.bundleSha256 != self.active_bundle_sha256
            || bundle.candidate.candidateArtifactSha256 != self.candidate_artifact_sha256
            || bundle.evaluationReport.evaluationReportSha256 != self.evaluation_report_sha256
            || bundle.qualificationReceipt.qualificationReceiptSha256
                != self.qualification_receipt_sha256
        {
            return None;
        }

        Some(AdmittedStandardForecastAdjustmentOwner {
            candidate: &bundle.candidate,
            active_bundle_sha256: &bundle.bundleSha256,
            evaluation_report_sha256: &bundle.evaluationReport.evaluationReportSha256,
            qualification_receipt_sha256: &bundle.qualificationReceipt.qualificationReceiptSha256,
            enabled_metrics: &self.enabled_metrics,
            admission,
        })
    }

    /// return the exact load instant projected during startup admission
    pub fn loaded_at(&self) -> &CanonicalInstant {
        &self.loaded_at
    }
}

/// borrowed standard candidate, evidence identities, and row admission adapter
pub struct AdmittedStandardForecastAdjustmentOwner<'a> {
    candidate: &'a ForecastAdjustmentCandidateV2,
    active_bundle_sha256: &'a str,
    evaluation_report_sha256: &'a str,
    qualification_receipt_sha256: &'a str,
    enabled_metrics: &'a [ForecastAdjustmentMetric],
    admission: &'a dyn ForecastMaintenanceWindModelAdmissionBoundary,
}

impl ForecastMaintenanceWindAdjustmentBoundary for AdmittedStandardForecastAdjustmentOwner<'_> {
    /// project only the public qualified runtime identities
    fn project_runtime(&self, loaded_at: &CanonicalInstant) -> ApiForecastAdjustmentRuntime {
        ApiForecastAdjustmentRuntime {
            actionSha256: FieldPresence::Missing,
            activationMode: Nullable::Value(
                ApiForecastAdjustmentRuntimeActivationModeEnum::Qualified,
            ),
            activeBundle: Nullable::Value(self.active_bundle_sha256.to_owned()),
            authorizationSha256: Nullable::Null,
            candidateArtifactSha256: Nullable::Value(
                self.candidate.candidateArtifactSha256.clone(),
            ),
            enabledMetrics: self.enabled_metrics.to_vec(),
            evaluationReportSha256: Nullable::Value(self.evaluation_report_sha256.to_owned()),
            expiresAt: Nullable::Null,
            fullMemberRootSha256: FieldPresence::Missing,
            loadedAt: loaded_at.utc.clone(),
            qualificationReceiptSha256: Nullable::Value(
                self.qualification_receipt_sha256.to_owned(),
            ),
            reasonCode: Nullable::Null,
            state: ApiForecastAdjustmentRuntimeStateEnum::Active,
            policyReportSha256: FieldPresence::Missing,
            transferReportSha256: Nullable::Null,
        }
    }

    /// apply the standard five-metric V2 kernel and attach qualified evidence identities
    fn apply(
        &self,
        row: &WeatherRecordRow,
    ) -> Result<ApiForecastAdjustmentDecision, CurrentProjectionError> {
        let admitted = match self.admission.admit(self.candidate, row)? {
            WindForecastRowAdmission::Admitted(admitted) => admitted,
            WindForecastRowAdmission::Rejected(reason) => {
                return Ok(fail_raw_adjustment(
                    reason,
                    ForecastAdjustmentFailRawDecisionStateEnum::NotApplicable,
                ));
            }
        };
        let metrics = canonical_weather_metrics(row);
        let outcome = evaluate_admitted_wind_v2_row(
            self.candidate,
            AdmittedWindRowFeatures {
                calendar: admitted.calendar,
                calendar_fingerprint_matches: admitted.calendar_fingerprint_matches,
                metrics: &metrics,
                raw_forecast_provenance: &admitted.raw_forecast_provenance,
                source_identity: admitted.source_identity,
            },
            AdmittedWindV2MetricOrder::Standard,
        );
        // return the exact whole-row refusal selected by the pure kernel
        if let Some(reason) = outcome.reason {
            return Ok(fail_raw_adjustment(
                reason,
                ForecastAdjustmentFailRawDecisionStateEnum::NotApplicable,
            ));
        }
        let Some(lead_band) = outcome.lead_band else {
            return Err(standard_startup_error(
                "successful standard evaluation omitted its admitted lead band",
            ));
        };

        let mut adjusted_metrics = Vec::with_capacity(5);
        let mut applied_metrics = Vec::with_capacity(5);
        // retain the original standard metric order in both generated collections
        append_metric(
            &mut adjusted_metrics,
            &mut applied_metrics,
            ForecastAdjustmentMetric::RelativeHumidityPercent,
            outcome.humidity_adjusted,
        );
        append_metric(
            &mut adjusted_metrics,
            &mut applied_metrics,
            ForecastAdjustmentMetric::TemperatureC,
            outcome.temperature_adjusted,
        );
        append_metric(
            &mut adjusted_metrics,
            &mut applied_metrics,
            ForecastAdjustmentMetric::WindDirectionDegrees,
            outcome.direction_adjusted,
        );
        append_metric(
            &mut adjusted_metrics,
            &mut applied_metrics,
            ForecastAdjustmentMetric::WindGustMps,
            outcome.gust_adjusted,
        );
        append_metric(
            &mut adjusted_metrics,
            &mut applied_metrics,
            ForecastAdjustmentMetric::WindSpeedMps,
            outcome.speed_adjusted,
        );

        Ok(ApiForecastAdjustmentDecision::Domain(
            weather_contracts::ForecastAdjustmentDecision::Active(
                ForecastAdjustmentActiveDecision {
                    adjustedMetrics: ForecastAdjustmentActiveDecisionAdjustedMetricsRecord(
                        adjusted_metrics,
                    ),
                    algorithmContractVersion:
                        ForecastAdjustmentActiveDecisionAlgorithmContractVersionLiteral,
                    appliedMetrics: applied_metrics,
                    candidateArtifactSha256: self.candidate.candidateArtifactSha256.clone(),
                    contractVersion: ForecastAdjustmentActiveDecisionContractVersionLiteral,
                    evaluationReportSha256: self.evaluation_report_sha256.to_owned(),
                    leadBand: lead_band,
                    qualificationReceiptSha256: self.qualification_receipt_sha256.to_owned(),
                    rawForecastProvenance: admitted.raw_forecast_provenance,
                    reasonCode: ForecastAdjustmentActiveDecisionReasonCodeNull,
                    state: ForecastAdjustmentActiveDecisionStateLiteral,
                },
            ),
        ))
    }
}

/// append one applied metric while preserving the source allowlist order
fn append_metric(
    adjusted: &mut Vec<(ForecastAdjustmentMetric, f64)>,
    applied: &mut Vec<ForecastAdjustmentMetric>,
    metric: ForecastAdjustmentMetric,
    value: Option<f64>,
) {
    // omit absent or guarded metrics exactly as the TypeScript partial record does
    if let Some(value) = value {
        adjusted.push((metric, value));
        applied.push(metric);
    }
}

/// select only the loader-admitted standard active grammar
fn standard_selection(
    snapshot: &ForecastAdjustmentStartupSnapshot,
) -> Option<&LoadedForecastAdjustmentRuntimeV1Active> {
    match &snapshot.runtime {
        LoadedApiForecastAdjustmentRuntime::Standard(
            LoadedForecastAdjustmentRuntimeV1::Active(active),
        ) => Some(active),
        _ => None,
    }
}

/// resolve one immutable trusted startup timestamp without local date parsing
fn resolve_instant(
    time: &dyn CurrentTimeResolver,
    value: &str,
) -> Result<CanonicalInstant, CurrentProjectionError> {
    let instant = time.resolve(&JsDateOrString::String(value.encode_utf16().collect()))?;
    // reject adapters that rewrite the loader's original timestamp bytes
    if instant.utc != value {
        return Err(standard_startup_error(
            "time adapter changed a trusted standard startup timestamp",
        ));
    }
    Ok(instant)
}

/// construct one bounded standard startup admission error
fn standard_startup_error(detail: &str) -> CurrentProjectionError {
    CurrentProjectionError {
        code: "forecast_projection.standard_adjustment_startup",
        detail: detail.to_owned(),
    }
}
