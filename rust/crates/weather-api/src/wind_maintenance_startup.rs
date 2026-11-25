use weather_adjustment::{
    AdmittedWindRowFeatures, AdmittedWindV2MetricOrder, evaluate_admitted_wind_v2_row,
};
use weather_contracts::{
    ApiForecastAdjustmentDecision, ApiForecastAdjustmentRuntime,
    ApiForecastAdjustmentRuntimeActivationModeEnum, ApiForecastAdjustmentRuntimeStateEnum,
    FieldPresence, ForecastAdjustmentCandidateV2, ForecastAdjustmentFailRawDecisionStateEnum,
    ForecastAdjustmentMetric, ForecastAdjustmentStartupSnapshot,
    ForecastAdjustmentWindCanaryMetric, ForecastAdjustmentWindQualifiedMaintenanceDecision,
    ForecastAdjustmentWindQualifiedMaintenanceDecisionActivationKindLiteral,
    ForecastAdjustmentWindQualifiedMaintenanceDecisionAdjustedMetricsRecord,
    ForecastAdjustmentWindQualifiedMaintenanceDecisionAlgorithmContractVersionLiteral,
    ForecastAdjustmentWindQualifiedMaintenanceDecisionContractVersionLiteral,
    ForecastAdjustmentWindQualifiedMaintenanceDecisionReasonCodeNull,
    ForecastAdjustmentWindQualifiedMaintenanceDecisionStateLiteral, JsDateOrString,
    LoadedApiForecastAdjustmentRuntime, LoadedForecastAdjustmentWindCanaryRuntime,
    LoadedForecastAdjustmentWindCanaryRuntimeMaintenanceActive,
    MaintenanceShadowComparatorAuthorityKind, Nullable, WeatherRecordRow,
};

use crate::{
    CanonicalInstant, CurrentProjectionError, CurrentTimeResolver, WindForecastRowAdmission,
    forecast_projection::fail_raw_adjustment, wind_model::canonical_weather_metrics,
};

/// resolve model-specific row material for one maintenance-qualified V2 candidate
pub trait ForecastMaintenanceWindModelAdmissionBoundary {
    /// resolve calendar, source, and runtime-fingerprint facts outside the pure model kernel
    fn admit(
        &self,
        candidate: &ForecastAdjustmentCandidateV2,
        row: &WeatherRecordRow,
    ) -> Result<WindForecastRowAdmission, CurrentProjectionError>;
}

/// apply one qualified maintenance model and project its public runtime metadata
pub trait ForecastMaintenanceWindAdjustmentBoundary {
    /// project the admitted active runtime at its immutable startup load instant
    fn project_runtime(&self, loaded_at: &CanonicalInstant) -> ApiForecastAdjustmentRuntime;

    /// apply the admitted candidate to one original forecast row
    fn apply(
        &self,
        row: &WeatherRecordRow,
    ) -> Result<ApiForecastAdjustmentDecision, CurrentProjectionError>;
}

/// immutable identities retained after selecting one maintenance-qualified startup snapshot
#[derive(Debug, PartialEq)]
pub struct MaintenanceWindForecastStartup {
    loaded_at: CanonicalInstant,
    active_bundle_sha256: String,
    candidate_artifact_sha256: String,
    enabled_metrics: Vec<ForecastAdjustmentMetric>,
    action_sha256: String,
    full_member_root_sha256: String,
    policy_report_sha256: String,
}

impl MaintenanceWindForecastStartup {
    /// admit only the active maintenance-qualified grammar selected by the original loader
    pub fn from_snapshot(
        snapshot: &ForecastAdjustmentStartupSnapshot,
        time: &dyn CurrentTimeResolver,
    ) -> Result<Option<Self>, CurrentProjectionError> {
        let selection = match maintenance_selection(snapshot)? {
            Some(selection) => selection,
            None => return Ok(None),
        };
        let loaded_at = resolve_instant(time, &snapshot.loadedAt)?;
        let mut enabled_metrics = Vec::new();
        // preserve first occurrence order from the generated candidate pairs
        for pair in &selection.active.bundle.candidate.enabledMetricBands {
            // deduplicate only the public metric projection, as the original Set does
            if !enabled_metrics.contains(&pair.metric) {
                enabled_metrics.push(pair.metric);
            }
        }

        Ok(Some(Self {
            loaded_at,
            active_bundle_sha256: selection.active.bundle.bundleSha256.clone(),
            candidate_artifact_sha256: selection
                .active
                .bundle
                .candidate
                .candidateArtifactSha256
                .clone(),
            enabled_metrics,
            action_sha256: selection
                .active
                .bundle
                .maintenanceAuthority
                .actionSha256
                .clone(),
            full_member_root_sha256: selection
                .active
                .bundle
                .maintenanceAuthority
                .fullMemberRootSha256
                .clone(),
            policy_report_sha256: selection
                .active
                .bundle
                .maintenanceAuthority
                .policyReportSha256
                .clone(),
        }))
    }

    /// rebind the borrowed candidate only when every cached startup identity still matches
    pub fn model_owner<'a>(
        &'a self,
        snapshot: &'a ForecastAdjustmentStartupSnapshot,
        admission: &'a dyn ForecastMaintenanceWindModelAdmissionBoundary,
    ) -> Option<AdmittedMaintenanceWindModelOwner<'a>> {
        let selection = maintenance_selection(snapshot).ok().flatten()?;
        let bundle = &selection.active.bundle;
        // reject a caller-supplied snapshot with any different selected policy or artifact.
        if snapshot.loadedAt != self.loaded_at.utc
            || bundle.bundleSha256 != self.active_bundle_sha256
            || bundle.candidate.candidateArtifactSha256 != self.candidate_artifact_sha256
            || bundle.maintenanceAuthority.actionSha256 != self.action_sha256
            || bundle.maintenanceAuthority.fullMemberRootSha256 != self.full_member_root_sha256
            || bundle.maintenanceAuthority.policyReportSha256 != self.policy_report_sha256
        {
            return None;
        }

        Some(AdmittedMaintenanceWindModelOwner {
            candidate: &bundle.candidate,
            active_bundle_sha256: &bundle.bundleSha256,
            action_sha256: &bundle.maintenanceAuthority.actionSha256,
            full_member_root_sha256: &bundle.maintenanceAuthority.fullMemberRootSha256,
            policy_report_sha256: &bundle.maintenanceAuthority.policyReportSha256,
            enabled_metrics: &self.enabled_metrics,
            admission,
        })
    }

    /// return the exact load instant projected during startup admission
    pub fn loaded_at(&self) -> &CanonicalInstant {
        &self.loaded_at
    }
}

/// borrowed active candidate, qualified policy evidence, and row admission adapter
pub struct AdmittedMaintenanceWindModelOwner<'a> {
    candidate: &'a ForecastAdjustmentCandidateV2,
    active_bundle_sha256: &'a str,
    action_sha256: &'a str,
    full_member_root_sha256: &'a str,
    policy_report_sha256: &'a str,
    enabled_metrics: &'a [ForecastAdjustmentMetric],
    admission: &'a dyn ForecastMaintenanceWindModelAdmissionBoundary,
}

impl ForecastMaintenanceWindAdjustmentBoundary for AdmittedMaintenanceWindModelOwner<'_> {
    /// project only public serving identities and omit legacy authorization fields
    fn project_runtime(&self, loaded_at: &CanonicalInstant) -> ApiForecastAdjustmentRuntime {
        ApiForecastAdjustmentRuntime {
            actionSha256: FieldPresence::Value(self.action_sha256.to_owned()),
            activationMode: Nullable::Value(
                ApiForecastAdjustmentRuntimeActivationModeEnum::MaintenanceQualified,
            ),
            activeBundle: Nullable::Value(self.active_bundle_sha256.to_owned()),
            authorizationSha256: Nullable::Null,
            candidateArtifactSha256: Nullable::Value(
                self.candidate.candidateArtifactSha256.clone(),
            ),
            enabledMetrics: self.enabled_metrics.to_vec(),
            evaluationReportSha256: Nullable::Null,
            expiresAt: Nullable::Null,
            fullMemberRootSha256: FieldPresence::Value(self.full_member_root_sha256.to_owned()),
            loadedAt: loaded_at.utc.clone(),
            qualificationReceiptSha256: Nullable::Null,
            reasonCode: Nullable::Null,
            state: ApiForecastAdjustmentRuntimeStateEnum::Active,
            policyReportSha256: FieldPresence::Value(self.policy_report_sha256.to_owned()),
            transferReportSha256: Nullable::Null,
        }
    }

    /// apply the existing V2 numerical kernel and attach only admitted qualified authority
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
            AdmittedWindV2MetricOrder::Maintenance,
        );
        // return the exact whole-row refusal selected by the pure kernel
        if let Some(reason) = outcome.reason {
            return Ok(fail_raw_adjustment(
                reason,
                ForecastAdjustmentFailRawDecisionStateEnum::NotApplicable,
            ));
        }
        let Some(lead_band) = outcome.lead_band else {
            return Err(wind_startup_error(
                "successful maintenance wind evaluation omitted its admitted lead band",
            ));
        };

        let mut adjusted_metrics = Vec::with_capacity(3);
        let mut applied_metrics = Vec::with_capacity(3);
        // retain the original maintenance metric order in both generated collections
        if let Some(value) = outcome.direction_adjusted {
            adjusted_metrics.push((ForecastAdjustmentMetric::WindDirectionDegrees, value));
            applied_metrics.push(ForecastAdjustmentWindCanaryMetric::WindDirectionDegrees);
        }
        // append gust before speed to match the TypeScript allowlist
        if let Some(value) = outcome.gust_adjusted {
            adjusted_metrics.push((ForecastAdjustmentMetric::WindGustMps, value));
            applied_metrics.push(ForecastAdjustmentWindCanaryMetric::WindGustMps);
        }
        // complete the decision with the last source-ordered wind metric
        if let Some(value) = outcome.speed_adjusted {
            adjusted_metrics.push((ForecastAdjustmentMetric::WindSpeedMps, value));
            applied_metrics.push(ForecastAdjustmentWindCanaryMetric::WindSpeedMps);
        }

        Ok(ApiForecastAdjustmentDecision::MaintenanceQualified(
            ForecastAdjustmentWindQualifiedMaintenanceDecision {
                actionSha256: self.action_sha256.to_owned(),
                activationKind:
                    ForecastAdjustmentWindQualifiedMaintenanceDecisionActivationKindLiteral,
                adjustedMetrics:
                    ForecastAdjustmentWindQualifiedMaintenanceDecisionAdjustedMetricsRecord(
                        adjusted_metrics,
                    ),
                algorithmContractVersion:
                    ForecastAdjustmentWindQualifiedMaintenanceDecisionAlgorithmContractVersionLiteral,
                appliedMetrics: applied_metrics,
                candidateArtifactSha256: self.candidate.candidateArtifactSha256.clone(),
                contractVersion:
                    ForecastAdjustmentWindQualifiedMaintenanceDecisionContractVersionLiteral,
                fullMemberRootSha256: self.full_member_root_sha256.to_owned(),
                leadBand: lead_band,
                policyReportSha256: self.policy_report_sha256.to_owned(),
                rawForecastProvenance: admitted.raw_forecast_provenance,
                reasonCode: ForecastAdjustmentWindQualifiedMaintenanceDecisionReasonCodeNull,
                state: ForecastAdjustmentWindQualifiedMaintenanceDecisionStateLiteral,
            },
        ))
    }
}

/// borrowed maintenance active selection after exact comparator identity admission
struct MaintenanceWindSelection<'a> {
    active: &'a LoadedForecastAdjustmentWindCanaryRuntimeMaintenanceActive,
}

/// select only a qualified maintenance bundle whose three identities agree
fn maintenance_selection(
    snapshot: &ForecastAdjustmentStartupSnapshot,
) -> Result<Option<MaintenanceWindSelection<'_>>, CurrentProjectionError> {
    let active = match &snapshot.runtime {
        LoadedApiForecastAdjustmentRuntime::Wind(
            LoadedForecastAdjustmentWindCanaryRuntime::MaintenanceActive(active),
        ) => active,
        _ => return Ok(None),
    };
    let artifact_identity = match &active.comparatorAuthority.artifactIdentitySha256 {
        Nullable::Value(value) => value,
        Nullable::Null => {
            return Err(wind_startup_error(
                "maintenance-qualified wind authority requires an artifact identity",
            ));
        }
    };
    // require the loader-selected artifact and both generated bundle identities to agree
    if active.comparatorAuthority.authorityKind
        != MaintenanceShadowComparatorAuthorityKind::MaintenanceQualified
        || artifact_identity != &active.bundle.bundleSha256
        || artifact_identity != &active.bundle.maintenanceBundleSha256
    {
        return Err(wind_startup_error(
            "maintenance-qualified wind authority must select both active bundle identities",
        ));
    }
    Ok(Some(MaintenanceWindSelection { active }))
}

/// resolve one immutable trusted startup timestamp without local date parsing
fn resolve_instant(
    time: &dyn CurrentTimeResolver,
    value: &str,
) -> Result<CanonicalInstant, CurrentProjectionError> {
    let instant = time.resolve(&JsDateOrString::String(value.encode_utf16().collect()))?;
    // reject adapters that rewrite the loader's original timestamp bytes
    if instant.utc != value {
        return Err(wind_startup_error(
            "time adapter changed a trusted wind startup timestamp",
        ));
    }
    Ok(instant)
}

/// construct one bounded maintenance startup admission error
fn wind_startup_error(detail: &str) -> CurrentProjectionError {
    CurrentProjectionError {
        code: "forecast_projection.wind_maintenance_startup",
        detail: detail.to_owned(),
    }
}
