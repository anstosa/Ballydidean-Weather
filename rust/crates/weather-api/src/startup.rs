use std::num::NonZeroU16;
use std::sync::Arc;

use weather_contracts::{
    AdjustmentRevisionCaptureEpochWitness, FieldPresence, ForecastAdjustmentStartupSnapshot,
    ForecastRainAdjustmentStartupSnapshot, ForecastTemperatureAdjustmentStartupSnapshot,
    LoadedApiForecastAdjustmentRuntime, LoadedForecastAdjustmentRainRuntimeRegistryV1,
    LoadedForecastAdjustmentRainRuntimeRegistryV1Disabled,
    LoadedForecastAdjustmentRainRuntimeRegistryV1DisabledArtifactSha256Null,
    LoadedForecastAdjustmentRainRuntimeRegistryV1DisabledReasonCodeEnum,
    LoadedForecastAdjustmentRainRuntimeRegistryV1DisabledStateLiteral,
    LoadedForecastAdjustmentRuntimeV1, LoadedForecastAdjustmentRuntimeV1Disabled,
    LoadedForecastAdjustmentRuntimeV1DisabledBundleNull,
    LoadedForecastAdjustmentRuntimeV1DisabledReasonCodeEnum,
    LoadedForecastAdjustmentRuntimeV1DisabledStateLiteral,
    LoadedForecastAdjustmentTemperatureCanaryRuntime,
    LoadedForecastAdjustmentTemperatureCanaryRuntimeV1,
    LoadedForecastAdjustmentTemperatureCanaryRuntimeV1Disabled,
    LoadedForecastAdjustmentTemperatureCanaryRuntimeV1DisabledBundleNull,
    LoadedForecastAdjustmentTemperatureCanaryRuntimeV1DisabledReasonCodeEnum,
    LoadedForecastAdjustmentTemperatureCanaryRuntimeV1DisabledStateLiteral,
    LoadedForecastAdjustmentWindCanaryRuntime, LoadedForecastAdjustmentWindCanaryRuntimeV1,
    LoadedForecastAdjustmentWindCanaryRuntimeV1Disabled,
    LoadedForecastAdjustmentWindCanaryRuntimeV1DisabledBundleNull,
    LoadedForecastAdjustmentWindCanaryRuntimeV1DisabledReasonCodeEnum,
    LoadedForecastAdjustmentWindCanaryRuntimeV1DisabledStateLiteral,
};

use crate::{
    AdjustmentMaintenanceListenerOwner, AdmittedAdjustmentMaintenanceAuthority, CanonicalInstant,
};

/// One contained startup loader failure without path or secret detail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WeatherApiStartupLoadError;

/// One bounded clock, preparation, or listener failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WeatherApiStartupPortError;

/// The exact startup phase that failed outside contained loaders.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WeatherApiStartupError {
    Clock,
    PreparePublic,
    PublicListenerSettings,
    MaintenanceListenerSettings,
    PrepareMaintenance,
    ListenPublic,
    ListenMaintenance,
}

/// Opaque evidence that an archive-origin setting was parsed by its admitted adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdmittedMaintenanceArchiveOrigin {
    canonical: String,
}

impl AdmittedMaintenanceArchiveOrigin {
    /// Retain one canonical origin already parsed by the external URL adapter.
    pub fn admitted(canonical: String) -> Self {
        Self { canonical }
    }

    /// Expose the admitted canonical origin only to the local preparation adapter.
    pub fn canonical(&self) -> &str {
        &self.canonical
    }
}

/// Typed listener settings after port and origin parsing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WeatherApiStartupListenerConfiguration {
    pub public_port: NonZeroU16,
    pub maintenance_port: Option<NonZeroU16>,
    pub maintenance_archive_origin: Option<AdmittedMaintenanceArchiveOrigin>,
}

/// One fully present optional maintenance-listener configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdmittedMaintenanceListenerConfiguration {
    pub port: NonZeroU16,
    pub archive_origin: AdmittedMaintenanceArchiveOrigin,
}

/// One cryptographically verified capture witness paired with its admitted instant.
#[derive(Debug, Clone, PartialEq)]
pub struct AdmittedAdjustmentRevisionCaptureWitness {
    pub witness: AdjustmentRevisionCaptureEpochWitness,
    pub capture_epoch: CanonicalInstant,
}

impl AdmittedAdjustmentRevisionCaptureWitness {
    /// Pair a verified witness with its already parsed canonical instant.
    pub fn from_verified(
        witness: AdjustmentRevisionCaptureEpochWitness,
        capture_epoch: CanonicalInstant,
    ) -> Result<Self, WeatherApiStartupLoadError> {
        // reject a typed instant belonging to another verified witness
        if witness.epochAt != capture_epoch.utc {
            return Err(WeatherApiStartupLoadError);
        }
        Ok(Self {
            witness,
            capture_epoch,
        })
    }
}

/// Immutable adjustment snapshots shared by public and maintenance preparation.
#[derive(Debug, Clone, PartialEq)]
pub struct WeatherApiStartupSnapshots {
    pub adjustment: ForecastAdjustmentStartupSnapshot,
    pub temperature_adjustment: ForecastTemperatureAdjustmentStartupSnapshot,
    pub rain_adjustment: ForecastRainAdjustmentStartupSnapshot,
}

/// Admitted optional maintenance application construction input.
#[derive(Debug, Clone, PartialEq)]
pub struct WeatherApiMaintenanceStartupPreparation {
    pub authority: AdmittedAdjustmentMaintenanceAuthority,
    pub configuration: AdmittedMaintenanceListenerConfiguration,
    pub capture_witness: AdmittedAdjustmentRevisionCaptureWitness,
}

/// Successful startup result without exposing local listener handles.
#[derive(Debug, Clone, PartialEq)]
pub struct StartedWeatherApi {
    pub snapshots: Arc<WeatherApiStartupSnapshots>,
    pub maintenance_listener_active: bool,
}

/// Startup-only adjustment loaders implemented by admitted local adapters.
pub trait WeatherApiStartupRuntimeLoader {
    /// Load the isolated wind canary selection once.
    fn load_wind_canary(
        &mut self,
    ) -> Result<LoadedForecastAdjustmentWindCanaryRuntime, WeatherApiStartupLoadError>;

    /// Load the qualified wind fallback only for an inactive canary registry.
    fn load_qualified_wind(
        &mut self,
    ) -> Result<LoadedForecastAdjustmentRuntimeV1, WeatherApiStartupLoadError>;

    /// Load the independent temperature selection once.
    fn load_temperature(
        &mut self,
    ) -> Result<LoadedForecastAdjustmentTemperatureCanaryRuntime, WeatherApiStartupLoadError>;

    /// Load the independent rain selection once.
    fn load_rain(
        &mut self,
    ) -> Result<LoadedForecastAdjustmentRainRuntimeRegistryV1, WeatherApiStartupLoadError>;
}

/// Shared admitted startup wall-clock boundary.
pub trait WeatherApiStartupClock {
    /// Capture one canonical instant after every runtime selection.
    fn capture_loaded_at(&mut self) -> Result<CanonicalInstant, WeatherApiStartupPortError>;
}

/// Verified capture-witness loader used only for fully configured maintenance.
pub trait AdjustmentRevisionCaptureWitnessLoader {
    /// Load one already verified capture witness from its external adapter.
    fn load_verified_capture_witness(
        &mut self,
    ) -> Result<AdmittedAdjustmentRevisionCaptureWitness, WeatherApiStartupLoadError>;
}

/// Raw listener settings whose private values remain unparsed until witness admission.
pub trait WeatherApiStartupRawListenerSettings {
    /// Parse and admit the mandatory public listener port.
    fn admit_public_port(&mut self) -> Result<NonZeroU16, WeatherApiStartupPortError>;

    /// Report whether the raw optional maintenance port setting exists without parsing it.
    fn maintenance_port_present(&self) -> bool;

    /// Report whether the raw optional archive-origin setting exists without parsing it.
    fn maintenance_archive_origin_present(&self) -> bool;

    /// Parse and admit the maintenance port after the capture witness is available.
    fn admit_maintenance_port(&mut self) -> Result<NonZeroU16, WeatherApiStartupPortError>;

    /// Parse and admit the archive origin after the capture witness is available.
    fn admit_maintenance_archive_origin(
        &mut self,
    ) -> Result<AdmittedMaintenanceArchiveOrigin, WeatherApiStartupPortError>;
}

impl WeatherApiStartupRawListenerSettings for WeatherApiStartupListenerConfiguration {
    /// Return the public port admitted before this typed configuration was built.
    fn admit_public_port(&mut self) -> Result<NonZeroU16, WeatherApiStartupPortError> {
        Ok(self.public_port)
    }

    /// Report whether the typed maintenance port is present.
    fn maintenance_port_present(&self) -> bool {
        self.maintenance_port.is_some()
    }

    /// Report whether the typed archive origin is present.
    fn maintenance_archive_origin_present(&self) -> bool {
        self.maintenance_archive_origin.is_some()
    }

    /// Return the already admitted maintenance port when present.
    fn admit_maintenance_port(&mut self) -> Result<NonZeroU16, WeatherApiStartupPortError> {
        self.maintenance_port.ok_or(WeatherApiStartupPortError)
    }

    /// Return the already admitted archive origin when present.
    fn admit_maintenance_archive_origin(
        &mut self,
    ) -> Result<AdmittedMaintenanceArchiveOrigin, WeatherApiStartupPortError> {
        self.maintenance_archive_origin
            .clone()
            .ok_or(WeatherApiStartupPortError)
    }
}

/// Local preparation and listener lifecycle without socket construction details.
pub trait WeatherApiStartupApplicationPort {
    /// Prepare the public application before optional maintenance resources.
    fn prepare_public(
        &mut self,
        snapshots: &Arc<WeatherApiStartupSnapshots>,
    ) -> Result<(), WeatherApiStartupPortError>;

    /// Prepare the separate maintenance application after witness admission.
    fn prepare_maintenance(
        &mut self,
        snapshots: &Arc<WeatherApiStartupSnapshots>,
        preparation: &WeatherApiMaintenanceStartupPreparation,
    ) -> Result<(), WeatherApiStartupPortError>;

    /// Start the already prepared public listener.
    fn listen_public(&mut self, port: NonZeroU16) -> Result<(), WeatherApiStartupPortError>;

    /// Start the already prepared separate maintenance listener.
    fn listen_maintenance(&mut self, port: NonZeroU16) -> Result<(), WeatherApiStartupPortError>;
}

/// Select immutable runtimes, prepare applications, then start listeners in Node order.
pub fn start_weather_api(
    loader: &mut dyn WeatherApiStartupRuntimeLoader,
    clock: &mut dyn WeatherApiStartupClock,
    witness_loader: &mut dyn AdjustmentRevisionCaptureWitnessLoader,
    application: &mut dyn WeatherApiStartupApplicationPort,
    mut configuration: WeatherApiStartupListenerConfiguration,
) -> Result<StartedWeatherApi, WeatherApiStartupError> {
    start_weather_api_with_raw_listener_settings(
        loader,
        clock,
        witness_loader,
        application,
        &mut configuration,
    )
}

/// Select immutable runtimes and defer private listener parsing until witness admission.
pub fn start_weather_api_with_raw_listener_settings(
    loader: &mut dyn WeatherApiStartupRuntimeLoader,
    clock: &mut dyn WeatherApiStartupClock,
    witness_loader: &mut dyn AdjustmentRevisionCaptureWitnessLoader,
    application: &mut dyn WeatherApiStartupApplicationPort,
    settings: &mut dyn WeatherApiStartupRawListenerSettings,
) -> Result<StartedWeatherApi, WeatherApiStartupError> {
    let wind_canary = loader
        .load_wind_canary()
        .unwrap_or_else(|_| disabled_wind_canary());
    let adjustment_runtime = select_wind_runtime(loader, wind_canary);
    let temperature_runtime = loader
        .load_temperature()
        .unwrap_or_else(|_| disabled_temperature());
    let rain_runtime = loader.load_rain().unwrap_or_else(|_| disabled_rain());
    let loaded_at = clock
        .capture_loaded_at()
        .map_err(|_| WeatherApiStartupError::Clock)?;
    // share one immutable model allocation across startup and both request applications
    let snapshots = Arc::new(WeatherApiStartupSnapshots {
        adjustment: ForecastAdjustmentStartupSnapshot {
            loadedAt: loaded_at.utc.clone(),
            runtime: adjustment_runtime,
        },
        temperature_adjustment: ForecastTemperatureAdjustmentStartupSnapshot {
            loadedAt: loaded_at.utc.clone(),
            runtime: temperature_runtime,
        },
        rain_adjustment: ForecastRainAdjustmentStartupSnapshot {
            loadedAt: loaded_at.utc,
            runtime: rain_runtime,
        },
    });

    application
        .prepare_public(&snapshots)
        .map_err(|_| WeatherApiStartupError::PreparePublic)?;

    let public_port = settings
        .admit_public_port()
        .map_err(|_| WeatherApiStartupError::PublicListenerSettings)?;

    // inspect only setting presence before deciding whether a witness is needed
    let maintenance =
        if settings.maintenance_port_present() && settings.maintenance_archive_origin_present() {
            // contain a missing or invalid root witness without parsing private settings
            match witness_loader.load_verified_capture_witness() {
                Ok(capture_witness) => {
                    let port = settings
                        .admit_maintenance_port()
                        .map_err(|_| WeatherApiStartupError::MaintenanceListenerSettings)?;
                    let archive_origin = settings
                        .admit_maintenance_archive_origin()
                        .map_err(|_| WeatherApiStartupError::MaintenanceListenerSettings)?;
                    Some(WeatherApiMaintenanceStartupPreparation {
                        authority: AdmittedAdjustmentMaintenanceAuthority::admitted(
                            AdjustmentMaintenanceListenerOwner::AdjustmentMaintenanceApplication,
                        ),
                        configuration: AdmittedMaintenanceListenerConfiguration {
                            port,
                            archive_origin,
                        },
                        capture_witness,
                    })
                }
                Err(_) => None,
            }
        } else {
            None
        };

    // construct maintenance resources only after both settings and witness exist
    if let Some(preparation) = maintenance.as_ref() {
        application
            .prepare_maintenance(&snapshots, preparation)
            .map_err(|_| WeatherApiStartupError::PrepareMaintenance)?;
    }

    application
        .listen_public(public_port)
        .map_err(|_| WeatherApiStartupError::ListenPublic)?;

    // retain Node's public-before-maintenance listener order
    if let Some(preparation) = maintenance.as_ref() {
        application
            .listen_maintenance(preparation.configuration.port)
            .map_err(|_| WeatherApiStartupError::ListenMaintenance)?;
    }

    Ok(StartedWeatherApi {
        snapshots,
        maintenance_listener_active: maintenance.is_some(),
    })
}

/// Apply active-canary, registry-inactive fallback, and terminal-canary priority.
fn select_wind_runtime(
    loader: &mut dyn WeatherApiStartupRuntimeLoader,
    wind_canary: LoadedForecastAdjustmentWindCanaryRuntime,
) -> LoadedApiForecastAdjustmentRuntime {
    // preserve an explicitly active canary without touching the qualified registry
    if wind_canary_is_active(&wind_canary) {
        return LoadedApiForecastAdjustmentRuntime::Wind(wind_canary);
    }

    // fall back only for the exact inactive-registry reason
    if wind_canary_is_registry_inactive(&wind_canary) {
        return LoadedApiForecastAdjustmentRuntime::Standard(
            loader
                .load_qualified_wind()
                .unwrap_or_else(|_| disabled_qualified_wind()),
        );
    }

    LoadedApiForecastAdjustmentRuntime::Wind(wind_canary)
}

/// Identify either legacy or maintenance active wind runtimes.
fn wind_canary_is_active(runtime: &LoadedForecastAdjustmentWindCanaryRuntime) -> bool {
    matches!(
        runtime,
        LoadedForecastAdjustmentWindCanaryRuntime::Legacy {
            value: LoadedForecastAdjustmentWindCanaryRuntimeV1::Active(_),
            ..
        } | LoadedForecastAdjustmentWindCanaryRuntime::MaintenanceActive(_)
    )
}

/// Identify the sole canary state allowed to load the qualified fallback.
fn wind_canary_is_registry_inactive(runtime: &LoadedForecastAdjustmentWindCanaryRuntime) -> bool {
    matches!(
        runtime,
        LoadedForecastAdjustmentWindCanaryRuntime::Legacy {
            value: LoadedForecastAdjustmentWindCanaryRuntimeV1::Disabled(
                LoadedForecastAdjustmentWindCanaryRuntimeV1Disabled {
                    reasonCode:
                        LoadedForecastAdjustmentWindCanaryRuntimeV1DisabledReasonCodeEnum::RegistryInactive,
                    ..
                }
            ),
            ..
        }
    )
}

/// Build the source fail-raw result for a wind-canary loader exception.
fn disabled_wind_canary() -> LoadedForecastAdjustmentWindCanaryRuntime {
    LoadedForecastAdjustmentWindCanaryRuntime::Legacy {
        value: LoadedForecastAdjustmentWindCanaryRuntimeV1::Disabled(
            LoadedForecastAdjustmentWindCanaryRuntimeV1Disabled {
                bundle: LoadedForecastAdjustmentWindCanaryRuntimeV1DisabledBundleNull,
                reasonCode:
                    LoadedForecastAdjustmentWindCanaryRuntimeV1DisabledReasonCodeEnum::BundleInvalid,
                state: LoadedForecastAdjustmentWindCanaryRuntimeV1DisabledStateLiteral,
            },
        ),
        comparatorAuthority: FieldPresence::Missing,
    }
}

/// Build the source fail-raw result for a qualified loader exception.
fn disabled_qualified_wind() -> LoadedForecastAdjustmentRuntimeV1 {
    LoadedForecastAdjustmentRuntimeV1::Disabled(LoadedForecastAdjustmentRuntimeV1Disabled {
        bundle: LoadedForecastAdjustmentRuntimeV1DisabledBundleNull,
        reasonCode: LoadedForecastAdjustmentRuntimeV1DisabledReasonCodeEnum::BundleInvalid,
        state: LoadedForecastAdjustmentRuntimeV1DisabledStateLiteral,
    })
}

/// Build the source fail-raw result for a temperature loader exception.
fn disabled_temperature() -> LoadedForecastAdjustmentTemperatureCanaryRuntime {
    LoadedForecastAdjustmentTemperatureCanaryRuntime::Legacy {
        value: LoadedForecastAdjustmentTemperatureCanaryRuntimeV1::Disabled(
            LoadedForecastAdjustmentTemperatureCanaryRuntimeV1Disabled {
                bundle: LoadedForecastAdjustmentTemperatureCanaryRuntimeV1DisabledBundleNull,
                reasonCode:
                    LoadedForecastAdjustmentTemperatureCanaryRuntimeV1DisabledReasonCodeEnum::BundleInvalid,
                state: LoadedForecastAdjustmentTemperatureCanaryRuntimeV1DisabledStateLiteral,
            },
        ),
        comparatorAuthority: FieldPresence::Missing,
    }
}

/// Build the source fail-raw result for a rain loader exception.
fn disabled_rain() -> LoadedForecastAdjustmentRainRuntimeRegistryV1 {
    LoadedForecastAdjustmentRainRuntimeRegistryV1::Disabled(
        LoadedForecastAdjustmentRainRuntimeRegistryV1Disabled {
            artifactSha256: LoadedForecastAdjustmentRainRuntimeRegistryV1DisabledArtifactSha256Null,
            comparatorAuthority: FieldPresence::Missing,
            reasonCode:
                LoadedForecastAdjustmentRainRuntimeRegistryV1DisabledReasonCodeEnum::RegistryInvalid,
            state: LoadedForecastAdjustmentRainRuntimeRegistryV1DisabledStateLiteral,
        },
    )
}
