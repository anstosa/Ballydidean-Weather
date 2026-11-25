use weather_contracts::{
    ApiForecastAdjustmentRuntimeReasonCodeEnum,
    ApiForecastTemperatureAdjustmentRuntimeReasonCodeEnum, ForecastAdjustmentReasonCode,
    LoadedApiForecastAdjustmentRuntime, LoadedForecastAdjustmentRuntimeV1,
    LoadedForecastAdjustmentRuntimeV1DisabledReasonCodeEnum,
    LoadedForecastAdjustmentTemperatureCanaryRuntime,
    LoadedForecastAdjustmentTemperatureCanaryRuntimeV1,
    LoadedForecastAdjustmentTemperatureCanaryRuntimeV1DisabledReasonCodeEnum,
    LoadedForecastAdjustmentWindCanaryRuntime, LoadedForecastAdjustmentWindCanaryRuntimeV1,
    LoadedForecastAdjustmentWindCanaryRuntimeV1DisabledReasonCodeEnum,
    TemperatureCanaryDecisionReasonValue,
};

use crate::startup::WeatherApiStartupSnapshots;

/// closed startup refusal shared by immutable forecast runtime projections
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForecastStartupRefusal {
    BundleInvalid,
    BundleMissing,
    CanaryExpired,
    CanaryKilled,
    RegistryInactive,
    RegistryInvalid,
    PolicyRaw,
}

impl ForecastStartupRefusal {
    /// project the refusal into the public wind runtime contract
    pub const fn adjustment_runtime_reason(self) -> ApiForecastAdjustmentRuntimeReasonCodeEnum {
        // preserve the closed startup refusal identity
        match self {
            Self::BundleInvalid => ApiForecastAdjustmentRuntimeReasonCodeEnum::BundleInvalid,
            Self::BundleMissing => ApiForecastAdjustmentRuntimeReasonCodeEnum::BundleMissing,
            Self::CanaryExpired => ApiForecastAdjustmentRuntimeReasonCodeEnum::CanaryExpired,
            Self::CanaryKilled => ApiForecastAdjustmentRuntimeReasonCodeEnum::CanaryKilled,
            Self::RegistryInactive => ApiForecastAdjustmentRuntimeReasonCodeEnum::RegistryInactive,
            Self::RegistryInvalid => ApiForecastAdjustmentRuntimeReasonCodeEnum::RegistryInvalid,
            Self::PolicyRaw => ApiForecastAdjustmentRuntimeReasonCodeEnum::PolicyRaw,
        }
    }

    /// project the refusal into the public temperature runtime contract
    pub const fn temperature_runtime_reason(
        self,
    ) -> ApiForecastTemperatureAdjustmentRuntimeReasonCodeEnum {
        // preserve the closed startup refusal identity
        match self {
            Self::BundleInvalid => {
                ApiForecastTemperatureAdjustmentRuntimeReasonCodeEnum::BundleInvalid
            }
            Self::BundleMissing => {
                ApiForecastTemperatureAdjustmentRuntimeReasonCodeEnum::BundleMissing
            }
            Self::CanaryExpired => {
                ApiForecastTemperatureAdjustmentRuntimeReasonCodeEnum::CanaryExpired
            }
            Self::CanaryKilled => {
                ApiForecastTemperatureAdjustmentRuntimeReasonCodeEnum::CanaryKilled
            }
            Self::RegistryInactive => {
                ApiForecastTemperatureAdjustmentRuntimeReasonCodeEnum::RegistryInactive
            }
            Self::RegistryInvalid => {
                ApiForecastTemperatureAdjustmentRuntimeReasonCodeEnum::RegistryInvalid
            }
            Self::PolicyRaw => ApiForecastTemperatureAdjustmentRuntimeReasonCodeEnum::PolicyRaw,
        }
    }

    /// project the refusal into a wind adjustment decision
    pub const fn adjustment_decision_reason(self) -> ForecastAdjustmentReasonCode {
        // preserve the closed startup refusal identity
        match self {
            Self::BundleInvalid => ForecastAdjustmentReasonCode::BundleInvalid,
            Self::BundleMissing => ForecastAdjustmentReasonCode::BundleMissing,
            Self::CanaryExpired => ForecastAdjustmentReasonCode::CanaryExpired,
            Self::CanaryKilled => ForecastAdjustmentReasonCode::CanaryKilled,
            Self::RegistryInactive => ForecastAdjustmentReasonCode::RegistryInactive,
            Self::RegistryInvalid => ForecastAdjustmentReasonCode::RegistryInvalid,
            Self::PolicyRaw => ForecastAdjustmentReasonCode::PolicyRaw,
        }
    }

    /// project the refusal into a temperature adjustment decision
    pub const fn temperature_decision_reason(self) -> TemperatureCanaryDecisionReasonValue {
        // preserve the closed startup refusal identity
        match self {
            Self::BundleInvalid => TemperatureCanaryDecisionReasonValue::BundleInvalid,
            Self::BundleMissing => TemperatureCanaryDecisionReasonValue::BundleMissing,
            Self::CanaryExpired => TemperatureCanaryDecisionReasonValue::CanaryExpired,
            Self::CanaryKilled => TemperatureCanaryDecisionReasonValue::CanaryKilled,
            Self::RegistryInactive => TemperatureCanaryDecisionReasonValue::RegistryInactive,
            Self::RegistryInvalid => TemperatureCanaryDecisionReasonValue::RegistryInvalid,
            Self::PolicyRaw => TemperatureCanaryDecisionReasonValue::PolicyRaw,
        }
    }
}

/// construction-snapshotted inactive forecast lifecycle and refusal state
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InactiveForecastServiceRuntime {
    adjustment_loaded_at: String,
    temperature_loaded_at: String,
    adjustment_refusal: Option<ForecastStartupRefusal>,
    temperature_refusal: Option<ForecastStartupRefusal>,
}

impl InactiveForecastServiceRuntime {
    /// cache trusted startup snapshots without request-time decoding
    pub fn from_startup(startup: &WeatherApiStartupSnapshots) -> Self {
        Self {
            adjustment_loaded_at: startup.adjustment.loadedAt.clone(),
            temperature_loaded_at: startup.temperature_adjustment.loadedAt.clone(),
            adjustment_refusal: adjustment_refusal(&startup.adjustment.runtime),
            temperature_refusal: temperature_refusal(&startup.temperature_adjustment.runtime),
        }
    }

    /// construct the neutral runtime used before startup snapshots exist
    pub fn unconfigured(
        adjustment_loaded_at: impl Into<String>,
        temperature_loaded_at: impl Into<String>,
    ) -> Self {
        Self {
            adjustment_loaded_at: adjustment_loaded_at.into(),
            temperature_loaded_at: temperature_loaded_at.into(),
            adjustment_refusal: None,
            temperature_refusal: None,
        }
    }

    /// return the exact trusted wind startup instant
    pub fn adjustment_loaded_at(&self) -> &str {
        &self.adjustment_loaded_at
    }

    /// return the exact trusted temperature startup instant
    pub fn temperature_loaded_at(&self) -> &str {
        &self.temperature_loaded_at
    }

    /// return the cached wind refusal when startup disabled adjustment
    pub fn adjustment_refusal(&self) -> Option<ForecastStartupRefusal> {
        self.adjustment_refusal
    }

    /// return the cached temperature refusal when startup disabled adjustment
    pub fn temperature_refusal(&self) -> Option<ForecastStartupRefusal> {
        self.temperature_refusal
    }
}

/// classify every generated wind startup state once
fn adjustment_refusal(
    runtime: &LoadedApiForecastAdjustmentRuntime,
) -> Option<ForecastStartupRefusal> {
    // keep active and disabled startup states exhaustive
    match runtime {
        LoadedApiForecastAdjustmentRuntime::Standard(
            LoadedForecastAdjustmentRuntimeV1::Active(_),
        ) => None,
        LoadedApiForecastAdjustmentRuntime::Standard(
            LoadedForecastAdjustmentRuntimeV1::Disabled(disabled),
        ) => Some(standard_refusal(disabled.reasonCode)),
        LoadedApiForecastAdjustmentRuntime::Wind(
            LoadedForecastAdjustmentWindCanaryRuntime::Legacy {
                value: LoadedForecastAdjustmentWindCanaryRuntimeV1::Active(_),
                ..
            },
        ) => None,
        LoadedApiForecastAdjustmentRuntime::Wind(
            LoadedForecastAdjustmentWindCanaryRuntime::Legacy {
                value: LoadedForecastAdjustmentWindCanaryRuntimeV1::Disabled(disabled),
                ..
            },
        ) => Some(canary_refusal(disabled.reasonCode)),
        LoadedApiForecastAdjustmentRuntime::Wind(
            LoadedForecastAdjustmentWindCanaryRuntime::MaintenanceActive(_),
        ) => None,
        LoadedApiForecastAdjustmentRuntime::Wind(
            LoadedForecastAdjustmentWindCanaryRuntime::MaintenanceDisabled(_),
        ) => Some(ForecastStartupRefusal::PolicyRaw),
    }
}

/// classify every generated temperature startup state once
fn temperature_refusal(
    runtime: &LoadedForecastAdjustmentTemperatureCanaryRuntime,
) -> Option<ForecastStartupRefusal> {
    // keep active and disabled startup states exhaustive
    match runtime {
        LoadedForecastAdjustmentTemperatureCanaryRuntime::Legacy {
            value: LoadedForecastAdjustmentTemperatureCanaryRuntimeV1::Active(_),
            ..
        } => None,
        LoadedForecastAdjustmentTemperatureCanaryRuntime::Legacy {
            value: LoadedForecastAdjustmentTemperatureCanaryRuntimeV1::Disabled(disabled),
            ..
        } => Some(temperature_canary_refusal(disabled.reasonCode)),
        LoadedForecastAdjustmentTemperatureCanaryRuntime::MaintenanceActive(_) => None,
        LoadedForecastAdjustmentTemperatureCanaryRuntime::MaintenanceDisabled(_) => {
            Some(ForecastStartupRefusal::PolicyRaw)
        }
    }
}

/// map each standard loader refusal without string conversion
const fn standard_refusal(
    reason: LoadedForecastAdjustmentRuntimeV1DisabledReasonCodeEnum,
) -> ForecastStartupRefusal {
    // keep generated standard reasons exhaustive
    match reason {
        LoadedForecastAdjustmentRuntimeV1DisabledReasonCodeEnum::BundleInvalid => {
            ForecastStartupRefusal::BundleInvalid
        }
        LoadedForecastAdjustmentRuntimeV1DisabledReasonCodeEnum::BundleMissing => {
            ForecastStartupRefusal::BundleMissing
        }
        LoadedForecastAdjustmentRuntimeV1DisabledReasonCodeEnum::RegistryInactive => {
            ForecastStartupRefusal::RegistryInactive
        }
        LoadedForecastAdjustmentRuntimeV1DisabledReasonCodeEnum::RegistryInvalid => {
            ForecastStartupRefusal::RegistryInvalid
        }
    }
}

/// map each wind canary loader refusal without string conversion
const fn canary_refusal(
    reason: LoadedForecastAdjustmentWindCanaryRuntimeV1DisabledReasonCodeEnum,
) -> ForecastStartupRefusal {
    // keep generated wind reasons exhaustive
    match reason {
        LoadedForecastAdjustmentWindCanaryRuntimeV1DisabledReasonCodeEnum::BundleInvalid => {
            ForecastStartupRefusal::BundleInvalid
        }
        LoadedForecastAdjustmentWindCanaryRuntimeV1DisabledReasonCodeEnum::BundleMissing => {
            ForecastStartupRefusal::BundleMissing
        }
        LoadedForecastAdjustmentWindCanaryRuntimeV1DisabledReasonCodeEnum::CanaryExpired => {
            ForecastStartupRefusal::CanaryExpired
        }
        LoadedForecastAdjustmentWindCanaryRuntimeV1DisabledReasonCodeEnum::CanaryKilled => {
            ForecastStartupRefusal::CanaryKilled
        }
        LoadedForecastAdjustmentWindCanaryRuntimeV1DisabledReasonCodeEnum::RegistryInactive => {
            ForecastStartupRefusal::RegistryInactive
        }
        LoadedForecastAdjustmentWindCanaryRuntimeV1DisabledReasonCodeEnum::RegistryInvalid => {
            ForecastStartupRefusal::RegistryInvalid
        }
    }
}

/// map each temperature canary loader refusal without string conversion
const fn temperature_canary_refusal(
    reason: LoadedForecastAdjustmentTemperatureCanaryRuntimeV1DisabledReasonCodeEnum,
) -> ForecastStartupRefusal {
    // keep generated temperature reasons exhaustive
    match reason {
        LoadedForecastAdjustmentTemperatureCanaryRuntimeV1DisabledReasonCodeEnum::BundleInvalid => {
            ForecastStartupRefusal::BundleInvalid
        }
        LoadedForecastAdjustmentTemperatureCanaryRuntimeV1DisabledReasonCodeEnum::BundleMissing => {
            ForecastStartupRefusal::BundleMissing
        }
        LoadedForecastAdjustmentTemperatureCanaryRuntimeV1DisabledReasonCodeEnum::CanaryExpired => {
            ForecastStartupRefusal::CanaryExpired
        }
        LoadedForecastAdjustmentTemperatureCanaryRuntimeV1DisabledReasonCodeEnum::CanaryKilled => {
            ForecastStartupRefusal::CanaryKilled
        }
        LoadedForecastAdjustmentTemperatureCanaryRuntimeV1DisabledReasonCodeEnum::RegistryInactive => {
            ForecastStartupRefusal::RegistryInactive
        }
        LoadedForecastAdjustmentTemperatureCanaryRuntimeV1DisabledReasonCodeEnum::RegistryInvalid => {
            ForecastStartupRefusal::RegistryInvalid
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// verify every refusal projects to all four generated contracts
    #[test]
    fn projects_closed_refusals_to_generated_reasons() {
        let cases = [
            (
                ForecastStartupRefusal::BundleInvalid,
                ApiForecastAdjustmentRuntimeReasonCodeEnum::BundleInvalid,
                ApiForecastTemperatureAdjustmentRuntimeReasonCodeEnum::BundleInvalid,
                ForecastAdjustmentReasonCode::BundleInvalid,
                TemperatureCanaryDecisionReasonValue::BundleInvalid,
            ),
            (
                ForecastStartupRefusal::BundleMissing,
                ApiForecastAdjustmentRuntimeReasonCodeEnum::BundleMissing,
                ApiForecastTemperatureAdjustmentRuntimeReasonCodeEnum::BundleMissing,
                ForecastAdjustmentReasonCode::BundleMissing,
                TemperatureCanaryDecisionReasonValue::BundleMissing,
            ),
            (
                ForecastStartupRefusal::CanaryExpired,
                ApiForecastAdjustmentRuntimeReasonCodeEnum::CanaryExpired,
                ApiForecastTemperatureAdjustmentRuntimeReasonCodeEnum::CanaryExpired,
                ForecastAdjustmentReasonCode::CanaryExpired,
                TemperatureCanaryDecisionReasonValue::CanaryExpired,
            ),
            (
                ForecastStartupRefusal::CanaryKilled,
                ApiForecastAdjustmentRuntimeReasonCodeEnum::CanaryKilled,
                ApiForecastTemperatureAdjustmentRuntimeReasonCodeEnum::CanaryKilled,
                ForecastAdjustmentReasonCode::CanaryKilled,
                TemperatureCanaryDecisionReasonValue::CanaryKilled,
            ),
            (
                ForecastStartupRefusal::RegistryInactive,
                ApiForecastAdjustmentRuntimeReasonCodeEnum::RegistryInactive,
                ApiForecastTemperatureAdjustmentRuntimeReasonCodeEnum::RegistryInactive,
                ForecastAdjustmentReasonCode::RegistryInactive,
                TemperatureCanaryDecisionReasonValue::RegistryInactive,
            ),
            (
                ForecastStartupRefusal::RegistryInvalid,
                ApiForecastAdjustmentRuntimeReasonCodeEnum::RegistryInvalid,
                ApiForecastTemperatureAdjustmentRuntimeReasonCodeEnum::RegistryInvalid,
                ForecastAdjustmentReasonCode::RegistryInvalid,
                TemperatureCanaryDecisionReasonValue::RegistryInvalid,
            ),
            (
                ForecastStartupRefusal::PolicyRaw,
                ApiForecastAdjustmentRuntimeReasonCodeEnum::PolicyRaw,
                ApiForecastTemperatureAdjustmentRuntimeReasonCodeEnum::PolicyRaw,
                ForecastAdjustmentReasonCode::PolicyRaw,
                TemperatureCanaryDecisionReasonValue::PolicyRaw,
            ),
        ];

        // verify every closed refusal projection
        for (refusal, adjustment_runtime, temperature_runtime, adjustment, temperature) in cases {
            assert_eq!(refusal.adjustment_runtime_reason(), adjustment_runtime);
            assert_eq!(refusal.temperature_runtime_reason(), temperature_runtime);
            assert_eq!(refusal.adjustment_decision_reason(), adjustment);
            assert_eq!(refusal.temperature_decision_reason(), temperature);
        }
    }

    /// verify generated startup reasons retain their closed identity
    #[test]
    fn maps_generated_disabled_reasons_without_strings() {
        assert_eq!(
            standard_refusal(
                LoadedForecastAdjustmentRuntimeV1DisabledReasonCodeEnum::RegistryInvalid,
            ),
            ForecastStartupRefusal::RegistryInvalid,
        );
        assert_eq!(
            canary_refusal(
                LoadedForecastAdjustmentWindCanaryRuntimeV1DisabledReasonCodeEnum::CanaryKilled,
            ),
            ForecastStartupRefusal::CanaryKilled,
        );
        assert_eq!(
            temperature_canary_refusal(
                LoadedForecastAdjustmentTemperatureCanaryRuntimeV1DisabledReasonCodeEnum::CanaryExpired,
            ),
            ForecastStartupRefusal::CanaryExpired,
        );
    }

    /// verify unconfigured state preserves supplied instants without an explicit veto
    #[test]
    fn unconfigured_runtime_preserves_loaded_at_strings() {
        let runtime = InactiveForecastServiceRuntime::unconfigured("adjustment", "temperature");

        assert_eq!(runtime.adjustment_loaded_at(), "adjustment");
        assert_eq!(runtime.temperature_loaded_at(), "temperature");
        assert_eq!(runtime.adjustment_refusal(), None,);
        assert_eq!(runtime.temperature_refusal(), None,);
    }
}
