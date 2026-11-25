//! Native fixed-input/output wind numerical fitter entrypoint.

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
fn main() -> std::process::ExitCode {
    use weather_runtime::{
        NativeModelEvidenceValues, NativeWindMaintenanceCliIo, NativeWindMaintenanceFitCalendar,
        run_wind_maintenance_cli, wind_maintenance_process_exit_code,
    };
    // bind the native fitter to the same independently verified local calendar authority
    let Some(calendar) = NativeWindMaintenanceFitCalendar::new() else {
        eprintln!("weather-maintenance-wind-fit: calendar unavailable");
        return std::process::ExitCode::FAILURE;
    };
    let arguments = std::env::args().skip(1).collect::<Vec<_>>();
    let result = run_wind_maintenance_cli(
        &arguments,
        &NativeModelEvidenceValues::new(),
        &calendar,
        &mut NativeWindMaintenanceCliIo,
    );
    // preserve the completed fit-only command's original exit mapping
    if let Err(error) = &result {
        eprintln!("weather-maintenance-wind-fit: {error:?}");
    }
    std::process::ExitCode::from(wind_maintenance_process_exit_code(&result))
}

/// Refuse unsupported native process architectures explicitly.
#[cfg(not(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
)))]
fn main() -> std::process::ExitCode {
    std::process::ExitCode::FAILURE
}
