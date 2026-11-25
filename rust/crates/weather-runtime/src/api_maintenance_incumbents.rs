//! select exact startup comparator authority and temperature incumbent owners

use weather_api::{
    AdmittedTemperatureCanaryAuthorization, AdmittedTemperatureCanaryRuntime,
    AdmittedTemperatureMosModelOrigin, AdmittedTemperatureMosModelOwner,
    BorrowedTemperatureMosServingModel, TemperatureMaintenanceIncumbent,
};
use weather_contracts::{
    FieldPresence, ForecastAdjustmentTemperatureCanaryRuntimeBundle,
    ForecastAdjustmentTemperatureCanaryRuntimeBundleV1ServedForecastIdentityAdapterVersionLiteral,
    ForecastAdjustmentTemperatureCanaryRuntimeBundleV1ServedForecastIdentitySourceDelayHoursLiteral,
    LoadedApiForecastAdjustmentRuntime, LoadedForecastAdjustmentRuntimeV1,
    LoadedForecastAdjustmentTemperatureCanaryRuntime,
    LoadedForecastAdjustmentTemperatureCanaryRuntimeMaintenanceActive,
    LoadedForecastAdjustmentTemperatureCanaryRuntimeMaintenanceActiveBundleServedForecastIdentityAdapterVersionLiteral,
    LoadedForecastAdjustmentTemperatureCanaryRuntimeMaintenanceActiveBundleServedForecastIdentityMaximumReceiptAgeHoursLiteral,
    LoadedForecastAdjustmentTemperatureCanaryRuntimeMaintenanceActiveBundleServedForecastIdentitySourceDelayHoursLiteral,
    LoadedForecastAdjustmentTemperatureCanaryRuntimeV1,
    LoadedForecastAdjustmentTemperatureCanaryRuntimeV1Active,
    LoadedForecastAdjustmentTemperatureCanaryRuntimeV1DisabledReasonCodeEnum,
    LoadedForecastAdjustmentWindCanaryRuntime, MaintenanceShadowServingAuthority,
    TemperatureCanaryDecisionReasonValue,
};

/// borrow the comparator authority actually retained by the selected wind runtime
pub fn wind_comparator_authority(
    runtime: &LoadedApiForecastAdjustmentRuntime,
) -> Option<&MaintenanceShadowServingAuthority> {
    // preserve optional legacy authority and required maintenance authority exactly
    match runtime {
        LoadedApiForecastAdjustmentRuntime::Standard(
            LoadedForecastAdjustmentRuntimeV1::Active(active),
        ) => present_authority(&active.comparatorAuthority),
        LoadedApiForecastAdjustmentRuntime::Standard(
            LoadedForecastAdjustmentRuntimeV1::Disabled(_),
        ) => None,
        LoadedApiForecastAdjustmentRuntime::Wind(
            LoadedForecastAdjustmentWindCanaryRuntime::Legacy {
                comparatorAuthority,
                ..
            },
        ) => present_authority(comparatorAuthority),
        LoadedApiForecastAdjustmentRuntime::Wind(
            LoadedForecastAdjustmentWindCanaryRuntime::MaintenanceActive(active),
        ) => Some(&active.comparatorAuthority),
        LoadedApiForecastAdjustmentRuntime::Wind(
            LoadedForecastAdjustmentWindCanaryRuntime::MaintenanceDisabled(disabled),
        ) => Some(&disabled.comparatorAuthority),
    }
}

/// select an exact temperature incumbent only when its startup authority is retained
pub fn temperature_comparator_incumbent<'a>(
    runtime: &'a LoadedForecastAdjustmentTemperatureCanaryRuntime,
    active: Option<(
        &'a AdmittedTemperatureCanaryRuntime,
        &'a AdmittedTemperatureMosModelOwner<'a, BorrowedTemperatureMosServingModel<'a>>,
    )>,
) -> Option<(
    TemperatureMaintenanceIncumbent<'a>,
    &'a MaintenanceShadowServingAuthority,
)> {
    // preserve optional legacy authority and required maintenance authority exactly
    match runtime {
        LoadedForecastAdjustmentTemperatureCanaryRuntime::Legacy {
            value,
            comparatorAuthority,
        } => {
            let authority = present_authority(comparatorAuthority)?;
            let incumbent = match value {
                LoadedForecastAdjustmentTemperatureCanaryRuntimeV1::Active(selected) => {
                    legacy_active_incumbent(selected, active)?
                }
                LoadedForecastAdjustmentTemperatureCanaryRuntimeV1::Disabled(disabled) => {
                    TemperatureMaintenanceIncumbent::Disabled {
                        reason_code: legacy_temperature_reason(disabled.reasonCode),
                    }
                }
            };
            Some((incumbent, authority))
        }
        LoadedForecastAdjustmentTemperatureCanaryRuntime::MaintenanceActive(selected) => {
            let incumbent = maintenance_active_incumbent(selected, active)?;
            Some((incumbent, &selected.comparatorAuthority))
        }
        LoadedForecastAdjustmentTemperatureCanaryRuntime::MaintenanceDisabled(disabled) => Some((
            TemperatureMaintenanceIncumbent::Disabled {
                reason_code: TemperatureCanaryDecisionReasonValue::PolicyRaw,
            },
            &disabled.comparatorAuthority,
        )),
    }
}

/// borrow one present optional comparator authority
fn present_authority(
    authority: &FieldPresence<MaintenanceShadowServingAuthority>,
) -> Option<&MaintenanceShadowServingAuthority> {
    // absence remains a hard comparator failure instead of a fabricated authority
    match authority {
        FieldPresence::Missing => None,
        FieldPresence::Value(authority) => Some(authority),
    }
}

/// bind a legacy active selection to its separately retained admitted owners
fn legacy_active_incumbent<'a>(
    selected: &LoadedForecastAdjustmentTemperatureCanaryRuntimeV1Active,
    active: Option<(
        &'a AdmittedTemperatureCanaryRuntime,
        &'a AdmittedTemperatureMosModelOwner<'a, BorrowedTemperatureMosServingModel<'a>>,
    )>,
) -> Option<TemperatureMaintenanceIncumbent<'a>> {
    let (runtime, owner) = active?;
    let (bundle_sha256, authorization_sha256, maximum_receipt_age_hours, expected_model, permanent) =
        match &selected.bundle {
            ForecastAdjustmentTemperatureCanaryRuntimeBundle::TimeLimited(bundle) => (
                bundle.bundleSha256.as_str(),
                bundle.authorization.authorizationSha256.as_str(),
                bundle.servedForecastIdentity.maximumReceiptAgeHours,
                LegacySelectedModel::Monthly(&bundle.model),
                false,
            ),
            ForecastAdjustmentTemperatureCanaryRuntimeBundle::Permanent(bundle) => (
                bundle.bundleSha256.as_str(),
                bundle.authorization.authorizationSha256.as_str(),
                bundle.servedForecastIdentity.maximumReceiptAgeHours,
                LegacySelectedModel::Permanent(&bundle.model),
                true,
            ),
        };
    // every separately retained owner must describe this exact loaded selection
    if runtime.evidence.active_bundle_sha256 != bundle_sha256
        || runtime.authorization.authorization_sha256() != Some(authorization_sha256)
        || runtime.maximum_receipt_age_hours != maximum_receipt_age_hours
        || runtime.source_delay_hours
            != ForecastAdjustmentTemperatureCanaryRuntimeBundleV1ServedForecastIdentitySourceDelayHoursLiteral::WIRE_VALUE
        || owner.origin != AdmittedTemperatureMosModelOrigin::admitted(bundle_sha256)
        || owner.served_adapter_version
            != ForecastAdjustmentTemperatureCanaryRuntimeBundleV1ServedForecastIdentityAdapterVersionLiteral::WIRE_VALUE
        || !expected_model.matches(owner.model)
        || !matches!(
            (&runtime.authorization, permanent),
            (
                AdmittedTemperatureCanaryAuthorization::TimeLimited { .. },
                false
            ) | (
                AdmittedTemperatureCanaryAuthorization::Permanent { .. },
                true
            )
        )
    {
        return None;
    }
    Some(TemperatureMaintenanceIncumbent::Active { runtime, owner })
}

enum LegacySelectedModel<'a> {
    Monthly(&'a weather_contracts::TemperatureMosRuntimeModel),
    Permanent(&'a weather_contracts::TemperatureMosPermanentRuntimeModel),
}

impl LegacySelectedModel<'_> {
    /// compare the admitted owner model without cloning selected package material
    fn matches(&self, model: BorrowedTemperatureMosServingModel<'_>) -> bool {
        // require the same model generation and complete generated value
        match (self, model) {
            (Self::Monthly(selected), BorrowedTemperatureMosServingModel::Monthly(model)) => {
                *selected == model
            }
            (Self::Permanent(selected), BorrowedTemperatureMosServingModel::Permanent(model)) => {
                *selected == model
            }
            _ => false,
        }
    }
}

/// bind a maintenance-qualified active selection to its retained admitted owners
fn maintenance_active_incumbent<'a>(
    selected: &LoadedForecastAdjustmentTemperatureCanaryRuntimeMaintenanceActive,
    active: Option<(
        &'a AdmittedTemperatureCanaryRuntime,
        &'a AdmittedTemperatureMosModelOwner<'a, BorrowedTemperatureMosServingModel<'a>>,
    )>,
) -> Option<TemperatureMaintenanceIncumbent<'a>> {
    let (runtime, owner) = active?;
    let bundle_sha256 = selected.bundle.bundleSha256.as_str();
    // maintenance packages use the fixed permanent source policy and exact selected model
    if runtime.evidence.active_bundle_sha256 != bundle_sha256
        || runtime.maximum_receipt_age_hours
            != LoadedForecastAdjustmentTemperatureCanaryRuntimeMaintenanceActiveBundleServedForecastIdentityMaximumReceiptAgeHoursLiteral::WIRE_VALUE
        || runtime.source_delay_hours
            != LoadedForecastAdjustmentTemperatureCanaryRuntimeMaintenanceActiveBundleServedForecastIdentitySourceDelayHoursLiteral::WIRE_VALUE
        || owner.origin != AdmittedTemperatureMosModelOrigin::admitted(bundle_sha256)
        || owner.served_adapter_version
            != LoadedForecastAdjustmentTemperatureCanaryRuntimeMaintenanceActiveBundleServedForecastIdentityAdapterVersionLiteral::WIRE_VALUE
        || !matches!(
            owner.model,
            BorrowedTemperatureMosServingModel::Permanent(model) if model == &selected.bundle.model
        )
        || !matches!(
            &runtime.authorization,
            AdmittedTemperatureCanaryAuthorization::MaintenanceQualified {
                action_sha256,
                full_member_root_sha256,
                policy_report_sha256,
            } if action_sha256 == &selected.bundle.maintenanceAuthority.actionSha256
                && full_member_root_sha256
                    == &selected.bundle.maintenanceAuthority.fullMemberRootSha256
                && policy_report_sha256
                    == &selected.bundle.maintenanceAuthority.policyReportSha256
        )
    {
        return None;
    }
    Some(TemperatureMaintenanceIncumbent::Active { runtime, owner })
}

/// map the closed legacy loader reason into the generated serving vocabulary
const fn legacy_temperature_reason(
    reason: LoadedForecastAdjustmentTemperatureCanaryRuntimeV1DisabledReasonCodeEnum,
) -> TemperatureCanaryDecisionReasonValue {
    // retain every startup-disabled reason without string conversion
    match reason {
        LoadedForecastAdjustmentTemperatureCanaryRuntimeV1DisabledReasonCodeEnum::BundleInvalid => {
            TemperatureCanaryDecisionReasonValue::BundleInvalid
        }
        LoadedForecastAdjustmentTemperatureCanaryRuntimeV1DisabledReasonCodeEnum::BundleMissing => {
            TemperatureCanaryDecisionReasonValue::BundleMissing
        }
        LoadedForecastAdjustmentTemperatureCanaryRuntimeV1DisabledReasonCodeEnum::CanaryExpired => {
            TemperatureCanaryDecisionReasonValue::CanaryExpired
        }
        LoadedForecastAdjustmentTemperatureCanaryRuntimeV1DisabledReasonCodeEnum::CanaryKilled => {
            TemperatureCanaryDecisionReasonValue::CanaryKilled
        }
        LoadedForecastAdjustmentTemperatureCanaryRuntimeV1DisabledReasonCodeEnum::RegistryInactive => {
            TemperatureCanaryDecisionReasonValue::RegistryInactive
        }
        LoadedForecastAdjustmentTemperatureCanaryRuntimeV1DisabledReasonCodeEnum::RegistryInvalid => {
            TemperatureCanaryDecisionReasonValue::RegistryInvalid
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// keep absent legacy authority unavailable to comparator replay
    #[test]
    fn rejects_missing_legacy_authority() {
        assert_eq!(present_authority(&FieldPresence::Missing), None);
    }

    /// preserve a representative legacy disabled reason without text mapping
    #[test]
    fn maps_legacy_temperature_disabled_reason() {
        assert_eq!(
            legacy_temperature_reason(
                LoadedForecastAdjustmentTemperatureCanaryRuntimeV1DisabledReasonCodeEnum::CanaryKilled,
            ),
            TemperatureCanaryDecisionReasonValue::CanaryKilled,
        );
    }
}
