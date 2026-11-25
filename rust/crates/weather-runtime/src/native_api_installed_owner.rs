//! Concrete installed forecast runtime owner for the native API startup.

use base64::{Engine as _, engine::general_purpose::STANDARD};

use weather_contracts::{
    ForecastAdjustmentTemperatureCanaryRuntimeBundle as TemperatureBundle,
    LoadedForecastAdjustmentTemperatureCanaryRuntime as LoadedTemperature,
    LoadedForecastAdjustmentTemperatureCanaryRuntimeV1 as LoadedLegacyTemperature,
};
use weather_edge::{ArchiveWirePrimitives, RevisionArchiveCanonicalCodec};
use weather_worker::TemperatureCanaryAuthorization;

use crate::{
    ApiMaintenanceCandidateError, ApiProcessReadError, ApiStandardRuntimeValues,
    CurrentApiProcessEnvironment, InstalledApiCanarySwitches, InstalledQualifiedCanaryLoaders,
    NativeApiMaintenanceCandidateFiles, NativeForecastRuntimeFiles,
    NativeInstalledApiRuntimeLoader, NativeInstalledQualifiedCanaryLoaders,
    NativeModelEvidenceValues, NativePredecessorModelCalendar, NativeWirePrimitives,
    WorkerConfiguration, load_installed_temperature_runtime, system_worker_health_milliseconds,
};

const INSTALLED_APPLICATION_ROOT: &str = "/opt/weather";

impl ApiStandardRuntimeValues for NativeModelEvidenceValues {
    /// Require the exact proven predecessor model-calendar fingerprint retained in the native binary.
    fn standard_calendar_matches(&self, icu_version: &str, tzdata_version: &str) -> bool {
        NativePredecessorModelCalendar::new()
            .is_some_and(|calendar| calendar.fingerprint_matches(icu_version, tzdata_version))
    }

    /// Encode the original immutable artifact bytes using canonical padded standard base64.
    fn encode_standard_base64(&self, bytes: &[u8]) -> Option<String> {
        Some(STANDARD.encode(bytes))
    }
}

/// Four concrete installed runtime selections with their original fixed filesystem and clock owners.
pub type NativeApiInstalledOwner = NativeInstalledApiRuntimeLoader<
    NativeModelEvidenceValues,
    NativeWirePrimitives,
    NativeApiMaintenanceCandidateFiles,
    NativeInstalledQualifiedCanaryLoaders<
        NativeApiMaintenanceCandidateFiles,
        NativeWirePrimitives,
        NativeModelEvidenceValues,
    >,
    fn() -> Option<String>,
>;

/// Categorize failure before any installed model registry is read.
#[derive(Debug)]
pub enum NativeApiInstalledOwnerError {
    CandidateFiles(ApiMaintenanceCandidateError),
    CanaryEnvironment(ApiProcessReadError),
    ModelCalendar,
}

/// Sample one real startup canary decision clock in the original canonical UTC spelling.
fn canonical_now() -> Option<String> {
    NativeWirePrimitives.format_utc_milliseconds(system_worker_health_milliseconds()?)
}

/// Bind the native API to the actual predecessor registries, model catalog, and kill switches.
pub fn native_installed_api_runtime_loader()
-> Result<NativeApiInstalledOwner, NativeApiInstalledOwnerError> {
    // refuse candidate runtime selection if the embedded calendar could not be proved
    NativePredecessorModelCalendar::new().ok_or(NativeApiInstalledOwnerError::ModelCalendar)?;
    let files = NativeApiMaintenanceCandidateFiles::new(INSTALLED_APPLICATION_ROOT)
        .map_err(NativeApiInstalledOwnerError::CandidateFiles)?;
    let switches = InstalledApiCanarySwitches::read(&CurrentApiProcessEnvironment)
        .map_err(NativeApiInstalledOwnerError::CanaryEnvironment)?;
    let values = NativeModelEvidenceValues::new();
    let qualified =
        NativeInstalledQualifiedCanaryLoaders::new(files.clone(), NativeWirePrimitives, values);
    Ok(NativeInstalledApiRuntimeLoader::new(
        NativeForecastRuntimeFiles::installed(),
        values,
        NativeWirePrimitives,
        files,
        qualified,
        canonical_now as fn() -> Option<String>,
        switches,
    ))
}

/// Load the worker's genuine temperature runtime using its already admitted configuration.
pub fn load_native_worker_temperature_authorization(
    configuration: &WorkerConfiguration,
    files: &NativeApiMaintenanceCandidateFiles,
    values: NativeModelEvidenceValues,
) -> TemperatureCanaryAuthorization {
    let mut qualified =
        NativeInstalledQualifiedCanaryLoaders::new(files.clone(), NativeWirePrimitives, values);
    let runtime = load_installed_temperature_runtime(
        &NativeForecastRuntimeFiles::installed(),
        &values,
        configuration
            .temperature_canary_kill_switch
            .map(|switch| switch.as_str()),
        &mut canonical_now,
        &mut || qualified.load_qualified_temperature_canary(),
    );
    match runtime {
        LoadedTemperature::MaintenanceActive(_) => {
            TemperatureCanaryAuthorization::MaintenanceQualified
        }
        LoadedTemperature::Legacy {
            value: LoadedLegacyTemperature::Active(active),
            ..
        } => legacy_worker_authorization(&active.bundle)
            .unwrap_or(TemperatureCanaryAuthorization::Inactive),
        _ => TemperatureCanaryAuthorization::Inactive,
    }
}

/// Preserve the exact loaded permanent or time-limited legacy authorization geometry.
fn legacy_worker_authorization(
    bundle: &TemperatureBundle,
) -> Option<TemperatureCanaryAuthorization> {
    match bundle {
        TemperatureBundle::TimeLimited(bundle) => {
            Some(TemperatureCanaryAuthorization::TimeLimited {
                activated_at_ms: NativeWirePrimitives
                    .canonical_utc_milliseconds(&bundle.authorization.activatedAt)?,
                expires_at_ms: NativeWirePrimitives
                    .canonical_utc_milliseconds(&bundle.authorization.expiresAt)?,
            })
        }
        TemperatureBundle::Permanent(bundle) => Some(TemperatureCanaryAuthorization::Permanent {
            activated_at_ms: NativeWirePrimitives
                .canonical_utc_milliseconds(&bundle.authorization.activatedAt)?,
        }),
    }
}

#[cfg(test)]
#[path = "native_api_installed_owner_test.rs"]
mod tests;
