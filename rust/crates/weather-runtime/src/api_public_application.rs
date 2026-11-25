//! own the prepared public weather application without transport behavior

use std::sync::Arc;

use weather_api::{
    AdmittedMaintenanceWindModelOwner, AdmittedPrivateRouteAuthority, AdmittedRainForecastRuntime,
    AdmittedStandardForecastAdjustmentOwner, AdmittedTemperatureCanaryRuntime,
    AdmittedTemperatureForecastServiceRuntime, AdmittedTemperatureMosModelOwner,
    AdmittedWindForecastServiceRuntime, AdmittedWindModelOwner, BorrowedTemperatureMosServingModel,
    CanonicalInstant, CurrentProjectionError, CurrentTimeResolver, DecodedServiceRequest,
    ForecastMaintenanceWindModelAdmissionBoundary, ForecastTemperatureMosAdmissionBoundary,
    ForecastWindModelAdmissionBoundary, InactiveForecastServiceRuntime,
    LegacyTemperatureForecastStartup, LegacyWindForecastStartup,
    MaintenanceTemperatureForecastStartup, MaintenanceWindForecastStartup,
    StandardForecastAdjustmentStartup, WeatherApiStartupSnapshots, WeatherServiceBoundaries,
    WeatherServiceClock, WeatherServiceRainFallbackDiagnostic, WeatherServiceRepository,
    WeatherServiceWireEncoder, WeatherServiceWireResponse, admit_rain_forecast_runtime,
    serve_private_wire_decoded_request, serve_wire_decoded_request,
    serve_wire_decoded_request_with_rain_diagnostics,
};
use weather_contracts::ForecastTemperatureAdjustmentStartupSnapshot;

use crate::{
    PooledWeatherApiReads, PooledWeatherServiceRepository, PostgresWeatherServiceRepository,
    WeatherApiSharedPool,
};

/// one installed active wind startup grammar and its matching admission boundary
enum BoundWindStartup {
    Legacy {
        startup: LegacyWindForecastStartup,
        admission: Arc<dyn ForecastWindModelAdmissionBoundary + Send + Sync>,
    },
    Maintenance {
        startup: MaintenanceWindForecastStartup,
        admission: Arc<dyn ForecastMaintenanceWindModelAdmissionBoundary + Send + Sync>,
    },
    Standard {
        startup: StandardForecastAdjustmentStartup,
        admission: Arc<dyn ForecastMaintenanceWindModelAdmissionBoundary + Send + Sync>,
    },
}

/// one request-scoped owner borrowed from the installed wind startup
enum WindForecastOwner<'a> {
    Legacy(AdmittedWindModelOwner<'a>),
    Maintenance(AdmittedMaintenanceWindModelOwner<'a>),
    Standard(AdmittedStandardForecastAdjustmentOwner<'a>),
}

/// one installed active temperature startup grammar
enum TemperatureForecastStartup {
    Legacy(LegacyTemperatureForecastStartup),
    Maintenance(MaintenanceTemperatureForecastStartup),
}

impl TemperatureForecastStartup {
    /// borrow the installed startup load instant
    fn loaded_at(&self) -> &CanonicalInstant {
        // preserve the exact selected grammar without a second projection
        match self {
            Self::Legacy(startup) => startup.loaded_at(),
            Self::Maintenance(startup) => startup.loaded_at(),
        }
    }

    /// borrow the installed request lifecycle runtime
    fn runtime(&self) -> &AdmittedTemperatureCanaryRuntime {
        // preserve the exact selected grammar without a second projection
        match self {
            Self::Legacy(startup) => startup.runtime(),
            Self::Maintenance(startup) => startup.runtime(),
        }
    }

    /// borrow the installed model through the shared admission boundary
    fn model_owner<'a>(
        &'a self,
        snapshot: &'a ForecastTemperatureAdjustmentStartupSnapshot,
        time: &'a dyn CurrentTimeResolver,
        admission: &'a dyn ForecastTemperatureMosAdmissionBoundary,
    ) -> Option<AdmittedTemperatureMosModelOwner<'a, BorrowedTemperatureMosServingModel<'a>>> {
        // bind only the startup grammar selected during process preparation
        match self {
            Self::Legacy(startup) => startup.model_owner(snapshot, time, admission),
            Self::Maintenance(startup) => startup.model_owner(snapshot, time, admission),
        }
    }
}

/// one indivisible active temperature startup and row admission binding
struct BoundTemperatureStartup {
    startup: TemperatureForecastStartup,
    admission: Arc<dyn ForecastTemperatureMosAdmissionBoundary + Send + Sync>,
}

/// prepared public application sharing one retained repository and startup snapshot
pub struct PreparedPublicWeatherApi<Pool, External> {
    repository: PooledWeatherServiceRepository<Pool, External>,
    snapshots: Arc<WeatherApiStartupSnapshots>,
    forecast_runtime: InactiveForecastServiceRuntime,
    rain_runtime: AdmittedRainForecastRuntime,
    wind: Option<Option<Arc<BoundWindStartup>>>,
    temperature: Option<Option<Arc<BoundTemperatureStartup>>>,
    release: Arc<str>,
}

impl<Pool, External> PreparedPublicWeatherApi<Pool, External> {
    /// retain the already prepared pool, original startup snapshots, and release identity
    pub fn new(
        repository: PooledWeatherServiceRepository<Pool, External>,
        snapshots: impl Into<Arc<WeatherApiStartupSnapshots>>,
        release: impl Into<Arc<str>>,
    ) -> Self {
        let snapshots: Arc<WeatherApiStartupSnapshots> = snapshots.into();
        let forecast_runtime = InactiveForecastServiceRuntime::from_startup(&snapshots);
        let rain_runtime = admit_rain_forecast_runtime(&snapshots.rain_adjustment);
        Self {
            repository,
            snapshots,
            forecast_runtime,
            rain_runtime,
            wind: None,
            temperature: None,
            release: release.into(),
        }
    }

    /// cache one parsed active legacy-wind startup and its shared row admission boundary
    pub fn bind_legacy_wind_startup(
        &mut self,
        time: &dyn CurrentTimeResolver,
        admission: Arc<dyn ForecastWindModelAdmissionBoundary + Send + Sync>,
    ) -> Result<(), CurrentProjectionError> {
        let startup = LegacyWindForecastStartup::from_snapshot(&self.snapshots.adjustment, time)?;
        // preserve old fallback behavior when the installed snapshot is not active legacy wind
        self.wind =
            startup.map(|startup| Some(Arc::new(BoundWindStartup::Legacy { startup, admission })));
        Ok(())
    }

    /// cache the one installed active wind grammar with only its matching row boundary
    pub fn bind_wind_startup(
        &mut self,
        time: &dyn CurrentTimeResolver,
        legacy_admission: Arc<dyn ForecastWindModelAdmissionBoundary + Send + Sync>,
        v2_admission: Arc<dyn ForecastMaintenanceWindModelAdmissionBoundary + Send + Sync>,
    ) -> Result<(), CurrentProjectionError> {
        let legacy = LegacyWindForecastStartup::from_snapshot(&self.snapshots.adjustment, time)?;
        // select the installed grammar once and never fall through at request time
        let startup = match legacy {
            Some(startup) => Some(BoundWindStartup::Legacy {
                startup,
                admission: legacy_admission,
            }),
            None => match MaintenanceWindForecastStartup::from_snapshot(
                &self.snapshots.adjustment,
                time,
            )? {
                Some(startup) => Some(BoundWindStartup::Maintenance {
                    startup,
                    admission: v2_admission,
                }),
                None => StandardForecastAdjustmentStartup::from_snapshot(
                    &self.snapshots.adjustment,
                    time,
                )?
                .map(|startup| BoundWindStartup::Standard {
                    startup,
                    admission: v2_admission,
                }),
            },
        };
        self.wind = Some(startup.map(Arc::new));
        Ok(())
    }

    /// cache one parsed active legacy-temperature startup and its shared row admission boundary
    pub fn bind_legacy_temperature_startup(
        &mut self,
        time: &dyn CurrentTimeResolver,
        admission: Arc<dyn ForecastTemperatureMosAdmissionBoundary + Send + Sync>,
    ) -> Result<(), CurrentProjectionError> {
        let startup = LegacyTemperatureForecastStartup::from_snapshot(
            &self.snapshots.temperature_adjustment,
            time,
        )?;
        self.temperature = Some(startup.map(|startup| {
            Arc::new(BoundTemperatureStartup {
                startup: TemperatureForecastStartup::Legacy(startup),
                admission,
            })
        }));
        Ok(())
    }

    /// cache the one installed active temperature startup and shared row admission boundary
    pub fn bind_temperature_startup(
        &mut self,
        time: &dyn CurrentTimeResolver,
        admission: Arc<dyn ForecastTemperatureMosAdmissionBoundary + Send + Sync>,
    ) -> Result<(), CurrentProjectionError> {
        let legacy = LegacyTemperatureForecastStartup::from_snapshot(
            &self.snapshots.temperature_adjustment,
            time,
        )?;
        // parse only the grammar installed by the retained startup snapshot
        let startup = match legacy {
            Some(startup) => Some(TemperatureForecastStartup::Legacy(startup)),
            None => MaintenanceTemperatureForecastStartup::from_snapshot(
                &self.snapshots.temperature_adjustment,
                time,
            )?
            .map(TemperatureForecastStartup::Maintenance),
        };
        self.temperature =
            Some(startup.map(|startup| Arc::new(BoundTemperatureStartup { startup, admission })));
        Ok(())
    }

    /// borrow the original startup snapshots for separate maintenance composition
    pub fn snapshots(&self) -> &WeatherApiStartupSnapshots {
        &self.snapshots
    }

    /// Share the immutable model material without copying bundle vectors.
    pub(crate) fn share_snapshots(&self) -> Arc<WeatherApiStartupSnapshots> {
        Arc::clone(&self.snapshots)
    }

    /// borrow the immutable release exposed by public health responses
    pub fn release(&self) -> &str {
        &self.release
    }
}

impl<Pool: WeatherApiSharedPool, External> PreparedPublicWeatherApi<Pool, External> {
    /// Share the already admitted API process pool for ordered shutdown.
    pub fn shared_pool(&self) -> Pool {
        self.repository.database().pool().share()
    }

    /// Prepare an independent worker with its own request values over the same pool and models.
    pub fn worker_with_values<Values>(
        &self,
        values: Values,
    ) -> PreparedPublicWeatherApi<Pool, Values> {
        let pool = self.repository.database().pool().share();
        let mut worker = PreparedPublicWeatherApi::new(
            PostgresWeatherServiceRepository::new(PooledWeatherApiReads::new(pool), values),
            self.share_snapshots(),
            Arc::clone(&self.release),
        );
        worker.wind = self
            .wind
            .as_ref()
            .map(|startup| startup.as_ref().map(Arc::clone));
        worker.temperature = self
            .temperature
            .as_ref()
            .map(|startup| startup.as_ref().map(Arc::clone));
        worker
    }
}

impl<Pool, External> PreparedPublicWeatherApi<Pool, External>
where
    PooledWeatherServiceRepository<Pool, External>: WeatherServiceRepository,
{
    /// execute one decoded public request through the retained application owners
    pub fn serve<'wire, Encoder: WeatherServiceWireEncoder>(
        &mut self,
        clock: &mut dyn WeatherServiceClock,
        approved: &WeatherServiceBoundaries<'_>,
        request: DecodedServiceRequest<'_>,
        encoder: &'wire mut Encoder,
    ) -> WeatherServiceWireResponse<'wire> {
        let wind_binding = wind_binding(&self.wind);
        let wind_owner = wind_owner(wind_binding.flatten(), &self.snapshots.adjustment);
        let wind_runtime = wind_runtime(wind_binding, wind_owner.as_ref());
        let temperature_binding = temperature_binding(&self.temperature);
        let temperature_owner = temperature_owner(
            temperature_binding.flatten(),
            &self.snapshots.temperature_adjustment,
            approved.time_resolver,
        );
        let temperature_runtime =
            temperature_runtime(temperature_binding, temperature_owner.as_ref());
        let boundaries = with_release(
            &self.release,
            &self.forecast_runtime,
            &self.rain_runtime,
            wind_runtime,
            temperature_runtime,
            approved,
        );
        // retain public dispatch so the private observation route remains inaccessible
        serve_wire_decoded_request(&mut self.repository, clock, &boundaries, request, encoder)
    }

    /// execute the same public path and report bounded rain fallback detail after encoding
    pub fn serve_with_rain_diagnostics<'wire, Encoder: WeatherServiceWireEncoder>(
        &mut self,
        clock: &mut dyn WeatherServiceClock,
        approved: &WeatherServiceBoundaries<'_>,
        request: DecodedServiceRequest<'_>,
        encoder: &'wire mut Encoder,
        reporter: &mut dyn FnMut(WeatherServiceRainFallbackDiagnostic) -> Result<(), ()>,
    ) -> WeatherServiceWireResponse<'wire> {
        let wind_binding = wind_binding(&self.wind);
        let wind_owner = wind_owner(wind_binding.flatten(), &self.snapshots.adjustment);
        let wind_runtime = wind_runtime(wind_binding, wind_owner.as_ref());
        let temperature_binding = temperature_binding(&self.temperature);
        let temperature_owner = temperature_owner(
            temperature_binding.flatten(),
            &self.snapshots.temperature_adjustment,
            approved.time_resolver,
        );
        let temperature_runtime =
            temperature_runtime(temperature_binding, temperature_owner.as_ref());
        let boundaries = with_release(
            &self.release,
            &self.forecast_runtime,
            &self.rain_runtime,
            wind_runtime,
            temperature_runtime,
            approved,
        );
        serve_wire_decoded_request_with_rain_diagnostics(
            &mut self.repository,
            clock,
            &boundaries,
            request,
            encoder,
            reporter,
        )
    }

    /// Use the same retained read store only when the observation transport has its own authority.
    pub fn serve_private_observation<'wire, Encoder: WeatherServiceWireEncoder>(
        &mut self,
        authority: AdmittedPrivateRouteAuthority,
        clock: &mut dyn WeatherServiceClock,
        approved: &WeatherServiceBoundaries<'_>,
        request: DecodedServiceRequest<'_>,
        encoder: &'wire mut Encoder,
    ) -> WeatherServiceWireResponse<'wire> {
        let boundaries = with_release(
            &self.release,
            &self.forecast_runtime,
            &self.rain_runtime,
            None,
            None,
            approved,
        );
        serve_private_wire_decoded_request(
            authority,
            &mut self.repository,
            clock,
            &boundaries,
            request,
            encoder,
        )
    }
}

/// expose whether installed wind was unbound, bound without startup, or bound active
fn wind_binding(
    binding: &Option<Option<Arc<BoundWindStartup>>>,
) -> Option<Option<&BoundWindStartup>> {
    binding
        .as_ref()
        .map(|startup| startup.as_ref().map(Arc::as_ref))
}

/// borrow the owner matching the one installed wind startup grammar
fn wind_owner<'a>(
    binding: Option<&'a BoundWindStartup>,
    snapshot: &'a weather_contracts::ForecastAdjustmentStartupSnapshot,
) -> Option<WindForecastOwner<'a>> {
    binding.and_then(|binding| {
        // keep each grammar paired only with its own typed admission boundary
        match binding {
            BoundWindStartup::Legacy { startup, admission } => startup
                .model_owner(snapshot, admission.as_ref())
                .map(WindForecastOwner::Legacy),
            BoundWindStartup::Maintenance { startup, admission } => startup
                .model_owner(snapshot, admission.as_ref())
                .map(WindForecastOwner::Maintenance),
            BoundWindStartup::Standard { startup, admission } => startup
                .model_owner(snapshot, admission.as_ref())
                .map(WindForecastOwner::Standard),
        }
    })
}

/// borrow the installed wind request runtime without copying candidate vectors
fn wind_runtime<'a>(
    binding: Option<Option<&'a BoundWindStartup>>,
    owner: Option<&'a WindForecastOwner<'a>>,
) -> Option<Option<AdmittedWindForecastServiceRuntime<'a>>> {
    binding.map(|binding| {
        binding.and_then(|binding| {
            // require the request owner to match the startup variant retained at preparation
            match (binding, owner?) {
                (BoundWindStartup::Legacy { startup, .. }, WindForecastOwner::Legacy(owner)) => {
                    Some(AdmittedWindForecastServiceRuntime::Legacy {
                        loaded_at: startup.loaded_at(),
                        runtime: startup.runtime(),
                        adjustment: owner,
                    })
                }
                (
                    BoundWindStartup::Maintenance { startup, .. },
                    WindForecastOwner::Maintenance(owner),
                ) => Some(AdmittedWindForecastServiceRuntime::Maintenance {
                    loaded_at: startup.loaded_at(),
                    adjustment: owner,
                }),
                (
                    BoundWindStartup::Standard { startup, .. },
                    WindForecastOwner::Standard(owner),
                ) => Some(AdmittedWindForecastServiceRuntime::Standard {
                    loaded_at: startup.loaded_at(),
                    adjustment: owner,
                }),
                _ => None,
            }
        })
    })
}

/// expose whether temperature was unbound, bound without startup, or bound active
fn temperature_binding(
    binding: &Option<Option<Arc<BoundTemperatureStartup>>>,
) -> Option<Option<&BoundTemperatureStartup>> {
    binding
        .as_ref()
        .map(|startup| startup.as_ref().map(Arc::as_ref))
}

/// borrow one active temperature owner with the request's admitted time boundary
fn temperature_owner<'a>(
    binding: Option<&'a BoundTemperatureStartup>,
    snapshot: &'a ForecastTemperatureAdjustmentStartupSnapshot,
    time: &'a dyn CurrentTimeResolver,
) -> Option<AdmittedTemperatureMosModelOwner<'a, BorrowedTemperatureMosServingModel<'a>>> {
    binding.and_then(|binding| {
        binding
            .startup
            .model_owner(snapshot, time, binding.admission.as_ref())
    })
}

/// borrow the temperature request runtime without reparsing time or copying model vectors
fn temperature_runtime<'a>(
    binding: Option<Option<&'a BoundTemperatureStartup>>,
    owner: Option<&'a AdmittedTemperatureMosModelOwner<'a, BorrowedTemperatureMosServingModel<'a>>>,
) -> Option<Option<AdmittedTemperatureForecastServiceRuntime<'a>>> {
    binding.map(|binding| {
        binding.and_then(|binding| {
            owner.map(|owner| AdmittedTemperatureForecastServiceRuntime {
                loaded_at: binding.startup.loaded_at(),
                runtime: binding.startup.runtime(),
                adjustment: owner,
            })
        })
    })
}

/// prefer an explicit prepared-temperature decision over any caller runtime
fn select_temperature_runtime<T>(prepared: Option<Option<T>>, caller: Option<T>) -> Option<T> {
    prepared.unwrap_or(caller)
}

/// prefer an explicit prepared-wind decision over any caller runtime
fn select_wind_runtime<T>(prepared: Option<Option<T>>, caller: Option<T>) -> Option<T> {
    prepared.unwrap_or(caller)
}

/// preserve request owners while sourcing the visible release only from process startup
fn with_release<'a>(
    release: &'a str,
    forecast_runtime: &'a InactiveForecastServiceRuntime,
    rain_runtime: &'a AdmittedRainForecastRuntime,
    prepared_wind_runtime: Option<Option<AdmittedWindForecastServiceRuntime<'a>>>,
    prepared_temperature_runtime: Option<Option<AdmittedTemperatureForecastServiceRuntime<'a>>>,
    approved: &WeatherServiceBoundaries<'a>,
) -> WeatherServiceBoundaries<'a> {
    WeatherServiceBoundaries {
        nominal_data_at: approved.nominal_data_at,
        time_resolver: approved.time_resolver,
        history: approved.history,
        tides: approved.tides,
        forecast: approved.forecast,
        forecast_runtime,
        // replace any caller selection whenever the process bound installed wind startup
        forecast_wind_runtime: select_wind_runtime(
            prepared_wind_runtime,
            approved.forecast_wind_runtime,
        ),
        // replace any caller selection whenever the process bound temperature startup
        forecast_temperature_runtime: select_temperature_runtime(
            prepared_temperature_runtime,
            approved.forecast_temperature_runtime,
        ),
        forecast_rain_runtime: Some(rain_runtime),
        trends: approved.trends,
        daily_precipitation: approved.daily_precipitation,
        rain_collection: approved.rain_collection,
        forecast_observation: approved.forecast_observation,
        version: Some(release),
    }
}

#[cfg(test)]
mod tests {
    use super::{select_temperature_runtime, select_wind_runtime};

    /// distinguish caller preservation from an explicit unavailable wind binding
    #[test]
    fn wind_runtime_selection_preserves_three_states() {
        assert_eq!(select_wind_runtime(None, Some(7)), Some(7));
        assert_eq!(select_wind_runtime(Some(None), Some(7)), None);
        assert_eq!(select_wind_runtime(Some(Some(11)), Some(7)), Some(11));
    }

    /// distinguish caller preservation from an explicit unavailable prepared binding
    #[test]
    fn temperature_runtime_selection_preserves_three_states() {
        assert_eq!(select_temperature_runtime(None, Some(7)), Some(7));
        assert_eq!(select_temperature_runtime(Some(None), Some(7)), None);
        assert_eq!(
            select_temperature_runtime(Some(Some(11)), Some(7)),
            Some(11),
        );
    }
}
