use weather_adjustment::BorrowedTemperatureMosServingModel;
use weather_contracts::{
    ForecastAdjustmentTemperatureCanaryRuntimeBundle,
    ForecastAdjustmentTemperatureCanaryRuntimeBundleV1ServedForecastIdentityAdapterVersionLiteral,
    ForecastAdjustmentTemperatureCanaryRuntimeBundleV1ServedForecastIdentitySourceDelayHoursLiteral,
    ForecastTemperatureAdjustmentStartupSnapshot, JsDateOrString,
    LoadedForecastAdjustmentTemperatureCanaryRuntime,
    LoadedForecastAdjustmentTemperatureCanaryRuntimeMaintenanceActive,
    LoadedForecastAdjustmentTemperatureCanaryRuntimeMaintenanceActiveBundleServedForecastIdentityAdapterVersionLiteral,
    LoadedForecastAdjustmentTemperatureCanaryRuntimeMaintenanceActiveBundleServedForecastIdentityMaximumReceiptAgeHoursLiteral,
    LoadedForecastAdjustmentTemperatureCanaryRuntimeMaintenanceActiveBundleServedForecastIdentitySourceDelayHoursLiteral,
    LoadedForecastAdjustmentTemperatureCanaryRuntimeV1, MaintenanceShadowComparatorAuthorityKind,
    Nullable,
};

use crate::{
    AdmittedTemperatureCanaryAuthorization, AdmittedTemperatureCanaryEvidence,
    AdmittedTemperatureCanaryRuntime, AdmittedTemperatureMosModelOrigin,
    AdmittedTemperatureMosModelOwner, CanonicalInstant, CurrentProjectionError,
    CurrentTimeResolver, ForecastTemperatureMosAdmissionBoundary,
};

/// active legacy temperature material projected once from an admitted startup snapshot
#[derive(Debug, PartialEq)]
pub struct LegacyTemperatureForecastStartup {
    activated_at: CanonicalInstant,
    loaded_at: CanonicalInstant,
    runtime: AdmittedTemperatureCanaryRuntime,
}

impl LegacyTemperatureForecastStartup {
    /// project only an active legacy temperature runtime through the canonical time boundary
    pub fn from_snapshot(
        snapshot: &ForecastTemperatureAdjustmentStartupSnapshot,
        time: &dyn CurrentTimeResolver,
    ) -> Result<Option<Self>, CurrentProjectionError> {
        // exclude disabled and maintenance selections
        let selection = match legacy_selection(snapshot) {
            Some(selection) => selection,
            None => return Ok(None),
        };
        let loaded_at = resolve_instant(time, &snapshot.loadedAt)?;
        let activated_at = resolve_instant(time, selection.activated_at)?;
        // preserve the bundle's finite or permanent authorization shape
        let authorization = match selection.expires_at {
            Some(expires_at) => {
                let expires_at = resolve_instant(time, expires_at)?;
                // reject finite authority with empty or reversed geometry
                if expires_at.epoch_milliseconds <= activated_at.epoch_milliseconds {
                    return Err(temperature_startup_error(
                        "finite temperature authorization must expire after activation",
                    ));
                }
                AdmittedTemperatureCanaryAuthorization::TimeLimited {
                    activated_at_epoch_ms: activated_at.epoch_milliseconds,
                    authorization_sha256: selection.authorization_sha256.to_owned(),
                    expires_at,
                }
            }
            None => AdmittedTemperatureCanaryAuthorization::Permanent {
                activated_at_epoch_ms: activated_at.epoch_milliseconds,
                authorization_sha256: selection.authorization_sha256.to_owned(),
            },
        };

        Ok(Some(Self {
            activated_at,
            loaded_at,
            runtime: AdmittedTemperatureCanaryRuntime {
                authorization,
                evidence: AdmittedTemperatureCanaryEvidence {
                    active_bundle_sha256: selection.active_bundle_sha256.to_owned(),
                },
                maximum_receipt_age_hours: selection.maximum_receipt_age_hours,
                source_delay_hours:
                    ForecastAdjustmentTemperatureCanaryRuntimeBundleV1ServedForecastIdentitySourceDelayHoursLiteral::WIRE_VALUE,
            },
        }))
    }

    /// borrow the exact model from a snapshot matching this startup identity
    pub fn model_owner<'a>(
        &'a self,
        snapshot: &'a ForecastTemperatureAdjustmentStartupSnapshot,
        time: &'a dyn CurrentTimeResolver,
        admission: &'a dyn ForecastTemperatureMosAdmissionBoundary,
    ) -> Option<AdmittedTemperatureMosModelOwner<'a, BorrowedTemperatureMosServingModel<'a>>> {
        let selection = legacy_selection(snapshot)?;
        // reject a caller-supplied snapshot with a different admitted identity
        if snapshot.loadedAt != self.loaded_at.utc
            || selection.activated_at != self.activated_at.utc
            || selection.active_bundle_sha256 != self.runtime.evidence.active_bundle_sha256
            || Some(selection.authorization_sha256)
                != self.runtime.authorization.authorization_sha256()
            || selection.maximum_receipt_age_hours != self.runtime.maximum_receipt_age_hours
            || !authorization_shape_matches(&self.runtime.authorization, &selection)
        {
            return None;
        }

        Some(AdmittedTemperatureMosModelOwner {
            model: selection.model,
            origin: AdmittedTemperatureMosModelOrigin::admitted(
                &self.runtime.evidence.active_bundle_sha256,
            ),
            served_adapter_version:
                ForecastAdjustmentTemperatureCanaryRuntimeBundleV1ServedForecastIdentityAdapterVersionLiteral::WIRE_VALUE,
            time_resolver: time,
            admission,
        })
    }

    /// return the canonical startup load instant
    pub fn loaded_at(&self) -> &CanonicalInstant {
        &self.loaded_at
    }

    /// return the admitted request-lifecycle runtime
    pub fn runtime(&self) -> &AdmittedTemperatureCanaryRuntime {
        &self.runtime
    }
}

/// active maintenance-qualified temperature material projected from one validated snapshot
#[derive(Debug, PartialEq)]
pub struct MaintenanceTemperatureForecastStartup {
    loaded_at: CanonicalInstant,
    runtime: AdmittedTemperatureCanaryRuntime,
}

impl MaintenanceTemperatureForecastStartup {
    /// project only a maintenance-qualified active selection through the canonical time boundary
    pub fn from_snapshot(
        snapshot: &ForecastTemperatureAdjustmentStartupSnapshot,
        time: &dyn CurrentTimeResolver,
    ) -> Result<Option<Self>, CurrentProjectionError> {
        // exclude legacy and maintenance-disabled selections
        let selection = match maintenance_selection(snapshot)? {
            Some(selection) => selection,
            None => return Ok(None),
        };
        let loaded_at = resolve_instant(time, &snapshot.loadedAt)?;

        Ok(Some(Self {
            loaded_at,
            runtime: AdmittedTemperatureCanaryRuntime {
                authorization: AdmittedTemperatureCanaryAuthorization::MaintenanceQualified {
                    action_sha256: selection.action_sha256.to_owned(),
                    full_member_root_sha256: selection.full_member_root_sha256.to_owned(),
                    policy_report_sha256: selection.policy_report_sha256.to_owned(),
                },
                evidence: AdmittedTemperatureCanaryEvidence {
                    active_bundle_sha256: selection.active_bundle_sha256.to_owned(),
                },
                maximum_receipt_age_hours:
                    LoadedForecastAdjustmentTemperatureCanaryRuntimeMaintenanceActiveBundleServedForecastIdentityMaximumReceiptAgeHoursLiteral::WIRE_VALUE,
                source_delay_hours:
                    LoadedForecastAdjustmentTemperatureCanaryRuntimeMaintenanceActiveBundleServedForecastIdentitySourceDelayHoursLiteral::WIRE_VALUE,
            },
        }))
    }

    /// borrow the exact permanent model from a snapshot matching this startup identity
    pub fn model_owner<'a>(
        &'a self,
        snapshot: &'a ForecastTemperatureAdjustmentStartupSnapshot,
        time: &'a dyn CurrentTimeResolver,
        admission: &'a dyn ForecastTemperatureMosAdmissionBoundary,
    ) -> Option<AdmittedTemperatureMosModelOwner<'a, BorrowedTemperatureMosServingModel<'a>>> {
        let selection = maintenance_selection(snapshot).ok().flatten()?;
        // reject rebinding to a different load, bundle, or maintenance policy selection
        if snapshot.loadedAt != self.loaded_at.utc
            || selection.active_bundle_sha256 != self.runtime.evidence.active_bundle_sha256
            || !maintenance_authority_matches(&self.runtime.authorization, &selection)
            || self.runtime.maximum_receipt_age_hours
                != LoadedForecastAdjustmentTemperatureCanaryRuntimeMaintenanceActiveBundleServedForecastIdentityMaximumReceiptAgeHoursLiteral::WIRE_VALUE
            || self.runtime.source_delay_hours
                != LoadedForecastAdjustmentTemperatureCanaryRuntimeMaintenanceActiveBundleServedForecastIdentitySourceDelayHoursLiteral::WIRE_VALUE
        {
            return None;
        }

        Some(AdmittedTemperatureMosModelOwner {
            model: BorrowedTemperatureMosServingModel::Permanent(&selection.active.bundle.model),
            origin: AdmittedTemperatureMosModelOrigin::admitted(
                &self.runtime.evidence.active_bundle_sha256,
            ),
            served_adapter_version:
                LoadedForecastAdjustmentTemperatureCanaryRuntimeMaintenanceActiveBundleServedForecastIdentityAdapterVersionLiteral::WIRE_VALUE,
            time_resolver: time,
            admission,
        })
    }

    /// return the canonical startup load instant
    pub fn loaded_at(&self) -> &CanonicalInstant {
        &self.loaded_at
    }

    /// return the admitted maintenance request lifecycle
    pub fn runtime(&self) -> &AdmittedTemperatureCanaryRuntime {
        &self.runtime
    }
}

/// borrowed identities, clocks, and model from one active legacy temperature bundle
struct LegacyTemperatureSelection<'a> {
    active_bundle_sha256: &'a str,
    activated_at: &'a str,
    authorization_sha256: &'a str,
    expires_at: Option<&'a str>,
    maximum_receipt_age_hours: u64,
    model: BorrowedTemperatureMosServingModel<'a>,
}

/// borrowed identity and policy evidence from one active maintenance selection
struct MaintenanceTemperatureSelection<'a> {
    active: &'a LoadedForecastAdjustmentTemperatureCanaryRuntimeMaintenanceActive,
    active_bundle_sha256: &'a str,
    action_sha256: &'a str,
    full_member_root_sha256: &'a str,
    policy_report_sha256: &'a str,
}

/// select only the legacy active branch without copying numerical model vectors
fn legacy_selection(
    snapshot: &ForecastTemperatureAdjustmentStartupSnapshot,
) -> Option<LegacyTemperatureSelection<'_>> {
    // keep all non-legacy-active startup variants outside this adapter
    let active = match &snapshot.runtime {
        LoadedForecastAdjustmentTemperatureCanaryRuntime::Legacy {
            value: LoadedForecastAdjustmentTemperatureCanaryRuntimeV1::Active(active),
            ..
        } => active,
        _ => return None,
    };
    // retain the authorization kind selected by the loader
    Some(match &active.bundle {
        ForecastAdjustmentTemperatureCanaryRuntimeBundle::TimeLimited(bundle) => {
            LegacyTemperatureSelection {
                active_bundle_sha256: &bundle.bundleSha256,
                activated_at: &bundle.authorization.activatedAt,
                authorization_sha256: &bundle.authorization.authorizationSha256,
                expires_at: Some(&bundle.authorization.expiresAt),
                maximum_receipt_age_hours: bundle.servedForecastIdentity.maximumReceiptAgeHours,
                model: BorrowedTemperatureMosServingModel::Monthly(&bundle.model),
            }
        }
        ForecastAdjustmentTemperatureCanaryRuntimeBundle::Permanent(bundle) => {
            LegacyTemperatureSelection {
                active_bundle_sha256: &bundle.bundleSha256,
                activated_at: &bundle.authorization.activatedAt,
                authorization_sha256: &bundle.authorization.authorizationSha256,
                expires_at: None,
                maximum_receipt_age_hours: bundle.servedForecastIdentity.maximumReceiptAgeHours,
                model: BorrowedTemperatureMosServingModel::Permanent(&bundle.model),
            }
        }
    })
}

/// select only a maintenance active branch with its validated selected artifact identity
fn maintenance_selection(
    snapshot: &ForecastTemperatureAdjustmentStartupSnapshot,
) -> Result<Option<MaintenanceTemperatureSelection<'_>>, CurrentProjectionError> {
    // keep all non-maintenance-active variants outside this adapter
    let active = match &snapshot.runtime {
        LoadedForecastAdjustmentTemperatureCanaryRuntime::MaintenanceActive(active) => active,
        _ => return Ok(None),
    };
    // require the comparator's explicit selected artifact identity
    let artifact_identity = match &active.comparatorAuthority.artifactIdentitySha256 {
        Nullable::Value(value) => value,
        Nullable::Null => {
            return Err(temperature_startup_error(
                "maintenance-qualified temperature authority requires an artifact identity",
            ));
        }
    };
    // bind only the validated maintenance authority for the selected bundle
    if active.comparatorAuthority.authorityKind
        != MaintenanceShadowComparatorAuthorityKind::MaintenanceQualified
        || artifact_identity != &active.bundle.bundleSha256
    {
        return Err(temperature_startup_error(
            "maintenance-qualified temperature authority must select the active bundle",
        ));
    }

    Ok(Some(MaintenanceTemperatureSelection {
        active,
        active_bundle_sha256: &active.bundle.bundleSha256,
        action_sha256: &active.bundle.maintenanceAuthority.actionSha256,
        full_member_root_sha256: &active.bundle.maintenanceAuthority.fullMemberRootSha256,
        policy_report_sha256: &active.bundle.maintenanceAuthority.policyReportSha256,
    }))
}

/// confirm the snapshot still carries the projected authorization geometry
fn authorization_shape_matches(
    authorization: &AdmittedTemperatureCanaryAuthorization,
    selection: &LegacyTemperatureSelection<'_>,
) -> bool {
    // compare the authorization kind and exact finite expiry clock
    match (authorization, selection.expires_at) {
        (
            AdmittedTemperatureCanaryAuthorization::TimeLimited { expires_at, .. },
            Some(selection_expires_at),
        ) => selection_expires_at == expires_at.utc,
        (AdmittedTemperatureCanaryAuthorization::Permanent { .. }, None) => true,
        _ => false,
    }
}

/// confirm the snapshot still carries the projected qualified policy evidence
fn maintenance_authority_matches(
    authorization: &AdmittedTemperatureCanaryAuthorization,
    selection: &MaintenanceTemperatureSelection<'_>,
) -> bool {
    // require every maintenance digest captured during startup admission
    match authorization {
        AdmittedTemperatureCanaryAuthorization::MaintenanceQualified {
            action_sha256,
            full_member_root_sha256,
            policy_report_sha256,
        } => {
            action_sha256 == selection.action_sha256
                && full_member_root_sha256 == selection.full_member_root_sha256
                && policy_report_sha256 == selection.policy_report_sha256
        }
        _ => false,
    }
}

/// resolve one trusted loaded timestamp without local parsing
fn resolve_instant(
    time: &dyn CurrentTimeResolver,
    value: &str,
) -> Result<CanonicalInstant, CurrentProjectionError> {
    let instant = time.resolve(&JsDateOrString::String(value.encode_utf16().collect()))?;
    // reject adapters that transform trusted immutable timestamp text
    if instant.utc != value {
        return Err(temperature_startup_error(
            "time adapter changed a trusted temperature startup timestamp",
        ));
    }
    Ok(instant)
}

/// construct one bounded startup projection error
fn temperature_startup_error(detail: &str) -> CurrentProjectionError {
    CurrentProjectionError {
        code: "forecast_projection.temperature_startup",
        detail: detail.to_owned(),
    }
}
