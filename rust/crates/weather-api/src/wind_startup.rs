use weather_adjustment::WindCanaryAuthorizationWindow;
use weather_contracts::{
    ForecastAdjustmentStartupSnapshot, ForecastAdjustmentWindCanaryCandidateV1,
    ForecastAdjustmentWindCanaryRuntimeBundle, JsDateOrString, LoadedApiForecastAdjustmentRuntime,
    LoadedForecastAdjustmentWindCanaryRuntime, LoadedForecastAdjustmentWindCanaryRuntimeV1,
};

use crate::{
    AdmittedWindCanaryEvidence, AdmittedWindCanaryRuntime, AdmittedWindModelOwner,
    CanonicalInstant, CurrentProjectionError, CurrentTimeResolver,
    ForecastWindModelAdmissionBoundary,
};

/// active legacy wind material projected once from an admitted startup snapshot
#[derive(Debug, PartialEq)]
pub struct LegacyWindForecastStartup {
    loaded_at: CanonicalInstant,
    runtime: AdmittedWindCanaryRuntime,
}

impl LegacyWindForecastStartup {
    /// project only an active legacy wind runtime through the canonical time boundary
    pub fn from_snapshot(
        snapshot: &ForecastAdjustmentStartupSnapshot,
        time: &dyn CurrentTimeResolver,
    ) -> Result<Option<Self>, CurrentProjectionError> {
        // exclude disabled, standard, and maintenance selections
        let selection = match legacy_selection(snapshot) {
            Some(selection) => selection,
            None => return Ok(None),
        };
        let loaded_at = resolve_instant(time, &snapshot.loadedAt)?;
        let activated_at = resolve_instant(time, selection.activated_at)?;
        // preserve the bundle's finite or permanent authorization shape
        let (authorization, expires_at) = match selection.expires_at {
            Some(expires_at) => {
                let expires_at = resolve_instant(time, expires_at)?;
                // reject finite authority with empty or reversed geometry
                if expires_at.epoch_milliseconds <= activated_at.epoch_milliseconds {
                    return Err(wind_startup_error(
                        "finite wind authorization must expire after activation",
                    ));
                }
                (
                    WindCanaryAuthorizationWindow::TimeLimited {
                        activated_at_epoch_ms: activated_at.epoch_milliseconds,
                        expires_at_epoch_ms: expires_at.epoch_milliseconds,
                    },
                    Some(expires_at),
                )
            }
            None => (
                WindCanaryAuthorizationWindow::Permanent {
                    activated_at_epoch_ms: activated_at.epoch_milliseconds,
                },
                None,
            ),
        };
        let evaluated_at_epoch_ms = loaded_at.epoch_milliseconds;

        Ok(Some(Self {
            loaded_at,
            runtime: AdmittedWindCanaryRuntime {
                environment_kill_switch: None,
                authorization,
                expires_at,
                enabled_metric_bands: selection.candidate.enabledMetricBands.clone(),
                evaluated_at_epoch_ms,
                evidence: AdmittedWindCanaryEvidence {
                    active_bundle_sha256: selection.active_bundle_sha256.to_owned(),
                    authorization_sha256: selection.authorization_sha256.to_owned(),
                    candidate_artifact_sha256: selection.candidate.candidateArtifactSha256.clone(),
                    transfer_report_sha256: selection.transfer_report_sha256.to_owned(),
                },
            },
        }))
    }

    /// borrow the exact candidate from a snapshot matching this startup identity
    pub fn model_owner<'a>(
        &'a self,
        snapshot: &'a ForecastAdjustmentStartupSnapshot,
        admission: &'a dyn ForecastWindModelAdmissionBoundary,
    ) -> Option<AdmittedWindModelOwner<'a>> {
        let selection = legacy_selection(snapshot)?;
        // reject a caller-supplied snapshot with a different admitted identity
        if snapshot.loadedAt != self.loaded_at.utc
            || selection.active_bundle_sha256 != self.runtime.evidence.active_bundle_sha256
            || selection.authorization_sha256 != self.runtime.evidence.authorization_sha256
            || selection.candidate.candidateArtifactSha256
                != self.runtime.evidence.candidate_artifact_sha256
            || selection.transfer_report_sha256 != self.runtime.evidence.transfer_report_sha256
            || selection.candidate.enabledMetricBands != self.runtime.enabled_metric_bands
        {
            return None;
        }

        Some(AdmittedWindModelOwner {
            candidate: selection.candidate,
            evidence: &self.runtime.evidence,
            admission,
        })
    }

    /// return the canonical startup load instant
    pub fn loaded_at(&self) -> &CanonicalInstant {
        &self.loaded_at
    }

    /// return the admitted request-lifecycle runtime
    pub fn runtime(&self) -> &AdmittedWindCanaryRuntime {
        &self.runtime
    }
}

/// borrowed identities and clocks from one active legacy wind bundle
struct LegacyWindSelection<'a> {
    active_bundle_sha256: &'a str,
    activated_at: &'a str,
    authorization_sha256: &'a str,
    candidate: &'a ForecastAdjustmentWindCanaryCandidateV1,
    expires_at: Option<&'a str>,
    transfer_report_sha256: &'a str,
}

/// select only the legacy active branch without copying candidate package vectors
fn legacy_selection(
    snapshot: &ForecastAdjustmentStartupSnapshot,
) -> Option<LegacyWindSelection<'_>> {
    // keep all non-legacy-active startup variants outside this adapter
    let active = match &snapshot.runtime {
        LoadedApiForecastAdjustmentRuntime::Wind(
            LoadedForecastAdjustmentWindCanaryRuntime::Legacy {
                value: LoadedForecastAdjustmentWindCanaryRuntimeV1::Active(active),
                ..
            },
        ) => active,
        _ => return None,
    };
    // retain the authorization kind selected by the loader
    Some(match &active.bundle {
        ForecastAdjustmentWindCanaryRuntimeBundle::TimeLimited(bundle) => LegacyWindSelection {
            active_bundle_sha256: &bundle.bundleSha256,
            activated_at: &bundle.authorization.activatedAt,
            authorization_sha256: &bundle.authorization.authorizationSha256,
            candidate: &bundle.candidate,
            expires_at: Some(&bundle.authorization.expiresAt),
            transfer_report_sha256: &bundle.transferReport.transferReportSha256,
        },
        ForecastAdjustmentWindCanaryRuntimeBundle::Permanent(bundle) => LegacyWindSelection {
            active_bundle_sha256: &bundle.bundleSha256,
            activated_at: &bundle.authorization.activatedAt,
            authorization_sha256: &bundle.authorization.authorizationSha256,
            candidate: &bundle.candidate,
            expires_at: None,
            transfer_report_sha256: &bundle.transferReport.transferReportSha256,
        },
    })
}

/// resolve one trusted loaded timestamp without local parsing
fn resolve_instant(
    time: &dyn CurrentTimeResolver,
    value: &str,
) -> Result<CanonicalInstant, CurrentProjectionError> {
    let instant = time.resolve(&JsDateOrString::String(value.encode_utf16().collect()))?;
    // reject adapters that transform trusted immutable timestamp text
    if instant.utc != value {
        return Err(wind_startup_error(
            "time adapter changed a trusted wind startup timestamp",
        ));
    }
    Ok(instant)
}

/// construct one bounded startup projection error
fn wind_startup_error(detail: &str) -> CurrentProjectionError {
    CurrentProjectionError {
        code: "forecast_projection.wind_startup",
        detail: detail.to_owned(),
    }
}
