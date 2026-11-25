//! Bind all installed API model selections to one startup loader.

use weather_api::{WeatherApiStartupLoadError, WeatherApiStartupRuntimeLoader};
use weather_contracts::{
    LoadedForecastAdjustmentRainRuntimeRegistryV1, LoadedForecastAdjustmentRuntimeV1,
    LoadedForecastAdjustmentTemperatureCanaryRuntime, LoadedForecastAdjustmentWindCanaryRuntime,
};

use crate::{
    ApiMaintenanceCandidateCodec, ApiProcessEnvironmentKey, ApiProcessEnvironmentRead,
    ApiProcessReadError, ApiStandardRuntimeValues, NativeForecastRuntimeFiles,
    WorkerRainServingFiles, WorkerRainServingValues, load_installed_api_rain_runtime,
    load_installed_standard_runtime, load_installed_temperature_runtime,
    load_installed_wind_runtime,
};

/// Root-installed qualified canary owners retained independently of legacy selection.
pub trait InstalledQualifiedCanaryLoaders {
    /// load the selected wind package from the installed qualification catalog
    fn load_qualified_wind_canary(
        &mut self,
    ) -> Result<LoadedForecastAdjustmentWindCanaryRuntime, WeatherApiStartupLoadError>;

    /// load the selected temperature package from the installed qualification catalog
    fn load_qualified_temperature_canary(
        &mut self,
    ) -> Result<LoadedForecastAdjustmentTemperatureCanaryRuntime, WeatherApiStartupLoadError>;
}

/// Exact canary environment values captured before model selection.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct InstalledApiCanarySwitches {
    pub wind: Option<String>,
    pub temperature: Option<String>,
}

impl InstalledApiCanarySwitches {
    /// read the original keys without collapsing unset or empty values
    pub fn read(environment: &impl ApiProcessEnvironmentRead) -> Result<Self, ApiProcessReadError> {
        Ok(Self {
            wind: environment
                .read_api_environment(ApiProcessEnvironmentKey::WindCanaryKillSwitch)?,
            temperature: environment
                .read_api_environment(ApiProcessEnvironmentKey::TemperatureCanaryKillSwitch)?,
        })
    }
}

/// Native startup owner with one filesystem root and shared value effects for all four selections.
pub struct NativeInstalledApiRuntimeLoader<Values, Codec, RainFiles, Qualified, Clock> {
    files: NativeForecastRuntimeFiles,
    values: Values,
    codec: Codec,
    qualified_rain_files: RainFiles,
    qualified_canaries: Qualified,
    now: Clock,
    switches: InstalledApiCanarySwitches,
}

impl<Values, Codec, RainFiles, Qualified, Clock>
    NativeInstalledApiRuntimeLoader<Values, Codec, RainFiles, Qualified, Clock>
{
    /// retain startup owners without reading files or advancing either canary clock
    pub const fn new(
        files: NativeForecastRuntimeFiles,
        values: Values,
        codec: Codec,
        qualified_rain_files: RainFiles,
        qualified_canaries: Qualified,
        now: Clock,
        switches: InstalledApiCanarySwitches,
    ) -> Self {
        Self {
            files,
            values,
            codec,
            qualified_rain_files,
            qualified_canaries,
            now,
            switches,
        }
    }
}

impl<Values, Codec, RainFiles, Qualified, Clock> WeatherApiStartupRuntimeLoader
    for NativeInstalledApiRuntimeLoader<Values, Codec, RainFiles, Qualified, Clock>
where
    Values: ApiStandardRuntimeValues + WorkerRainServingValues,
    Codec: ApiMaintenanceCandidateCodec,
    RainFiles: WorkerRainServingFiles,
    Qualified: InstalledQualifiedCanaryLoaders,
    Clock: FnMut() -> Option<String>,
{
    /// select the installed wind canary with its independent qualification effect
    fn load_wind_canary(
        &mut self,
    ) -> Result<LoadedForecastAdjustmentWindCanaryRuntime, WeatherApiStartupLoadError> {
        Ok(load_installed_wind_runtime(
            &self.files,
            &self.values,
            self.switches.wind.as_deref(),
            &mut self.now,
            &mut || self.qualified_canaries.load_qualified_wind_canary(),
        ))
    }

    /// select the standard registry only when the startup owner requests its fallback
    fn load_qualified_wind(
        &mut self,
    ) -> Result<LoadedForecastAdjustmentRuntimeV1, WeatherApiStartupLoadError> {
        Ok(load_installed_standard_runtime(&self.files, &self.values))
    }

    /// select temperature independently of either wind disposition
    fn load_temperature(
        &mut self,
    ) -> Result<LoadedForecastAdjustmentTemperatureCanaryRuntime, WeatherApiStartupLoadError> {
        Ok(load_installed_temperature_runtime(
            &self.files,
            &self.values,
            self.switches.temperature.as_deref(),
            &mut self.now,
            &mut || self.qualified_canaries.load_qualified_temperature_canary(),
        ))
    }

    /// select the exact retained rain registry independently of canary clocks
    fn load_rain(
        &mut self,
    ) -> Result<LoadedForecastAdjustmentRainRuntimeRegistryV1, WeatherApiStartupLoadError> {
        Ok(load_installed_api_rain_runtime(
            &self.files,
            &mut self.qualified_rain_files,
            &mut self.codec,
            &self.values,
        ))
    }
}
