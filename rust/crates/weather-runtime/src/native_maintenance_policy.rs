//! Native dispatch of the complete original family qualification policy.

use crate::NativeMaintenanceCaptureFamily;
use crate::native_maintenance_policy_rain::evaluate_native_rain_maintenance_policy;
use crate::native_maintenance_policy_temperature_wind::{
    NativeMaintenancePolicyKind, evaluate_native_temperature_wind_maintenance_policy,
};
use weather_contracts::JsonValue;

/// Evaluate one exact original promotion or regression population with the selected numerical policy.
pub fn eval_native_maintenance_policy(
    family: NativeMaintenanceCaptureFamily,
    kind: NativeMaintenancePolicyKind,
    rows: &[JsonValue],
    epoch: Option<&JsonValue>,
) -> Option<JsonValue> {
    // dispatch the complete original closed family set without fallback
    match family {
        NativeMaintenanceCaptureFamily::Temperature | NativeMaintenanceCaptureFamily::Wind => {
            evaluate_native_temperature_wind_maintenance_policy(family, kind, rows, epoch)
        }
        NativeMaintenanceCaptureFamily::Rain => {
            evaluate_native_rain_maintenance_policy(kind, rows, epoch)
        }
    }
}
