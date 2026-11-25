//! Native value-only bridge for isolated numerical maintenance fitters.

use crate::{
    NativePredecessorModelCalendar, NativeWirePrimitives,
    maintenance_canonical_json::{exact_keys, field},
    native_maintenance_policy_rain::{
        create_native_rain_maintenance_policy_row, evaluate_native_rain_maintenance_development,
        evaluate_native_rain_maintenance_development_v3,
    },
};
use weather_contracts::JsonValue;
use weather_edge::ArchiveWirePrimitives;

pub const NATIVE_MAINTENANCE_FIT_BRIDGE_MAXIMUM_BYTES: usize = 64 * 1024 * 1024;

/// Closed value-only offline fit bridge command.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMaintenanceFitBridgeCommand {
    TemperatureInfer,
    RainDevelopment,
    RainDevelopmentV3,
}

/// Admit only the original pure historical numerical screening operations.
pub fn parse_native_maintenance_fit_bridge_command(
    args: &[String],
) -> Option<NativeMaintenanceFitBridgeCommand> {
    match args {
        [arg] if arg == "--temperature-infer" => {
            Some(NativeMaintenanceFitBridgeCommand::TemperatureInfer)
        }
        [arg] if arg == "--rain-development" => {
            Some(NativeMaintenanceFitBridgeCommand::RainDevelopment)
        }
        [arg] if arg == "--rain-development-v3" => {
            Some(NativeMaintenanceFitBridgeCommand::RainDevelopmentV3)
        }
        _ => None,
    }
}

/// Run the source historical policy evaluator with actual canonical evaluation-row identities.
pub fn run_native_maintenance_fit_bridge(
    command: NativeMaintenanceFitBridgeCommand,
    bytes: &[u8],
) -> Option<Vec<u8>> {
    if bytes.is_empty() || bytes.len() > NATIVE_MAINTENANCE_FIT_BRIDGE_MAXIMUM_BYTES {
        return None;
    }
    let wire = NativeWirePrimitives;
    let value = wire.decode_json(bytes)?;
    if command == NativeMaintenanceFitBridgeCommand::TemperatureInfer {
        return wire.encode_sorted_json(&crate::native_maintenance_temperature_fit_bridge::evaluate_native_temperature_fit_batch(&value)?);
    }
    let (rows, population) = match command {
        NativeMaintenanceFitBridgeCommand::TemperatureInfer => return None,
        NativeMaintenanceFitBridgeCommand::RainDevelopment => {
            let JsonValue::Array(rows) = &value else {
                return None;
            };
            (rows, None)
        }
        NativeMaintenanceFitBridgeCommand::RainDevelopmentV3 => {
            if !exact_keys(&value, &["population", "rows"]) {
                return None;
            }
            let JsonValue::Array(rows) = field(&value, "rows")? else {
                return None;
            };
            (rows, Some(field(&value, "population")?))
        }
    };
    let calendar = NativePredecessorModelCalendar::new()?;
    // preserve the source historical row constructor before applying its policy
    let rows = rows
        .iter()
        .map(|row| create_native_rain_maintenance_policy_row(row, &calendar).map(|row| row.value))
        .collect::<Option<Vec<_>>>()?;
    let report = match population {
        Some(population) => evaluate_native_rain_maintenance_development_v3(&rows, population)?,
        None => evaluate_native_rain_maintenance_development(&rows)?,
    };
    wire.encode_sorted_json(&report)
}
