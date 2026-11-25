use std::cell::RefCell;
use std::num::NonZeroU16;
use std::rc::Rc;
use std::sync::Arc;

use weather_api::{
    AdjustmentRevisionCaptureWitnessLoader, AdmittedAdjustmentRevisionCaptureWitness,
    AdmittedMaintenanceArchiveOrigin, CanonicalInstant, WeatherApiMaintenanceStartupPreparation,
    WeatherApiStartupApplicationPort, WeatherApiStartupClock, WeatherApiStartupError,
    WeatherApiStartupListenerConfiguration, WeatherApiStartupLoadError, WeatherApiStartupPortError,
    WeatherApiStartupRawListenerSettings, WeatherApiStartupRuntimeLoader,
    WeatherApiStartupSnapshots, start_weather_api, start_weather_api_with_raw_listener_settings,
};
use weather_contracts::{
    AdjustmentRevisionCaptureEpochWitness,
    AdjustmentRevisionCaptureEpochWitnessActivationKindLiteral,
    AdjustmentRevisionCaptureEpochWitnessArchiveCommitOrdinalLiteral,
    AdjustmentRevisionCaptureEpochWitnessContractVersionLiteral,
    AdjustmentRevisionCaptureEpochWitnessControlPlaneVersionEnum, FieldPresence,
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

type Trace = Rc<RefCell<Vec<String>>>;

struct FixtureLoaders {
    events: Trace,
    wind: Result<LoadedForecastAdjustmentWindCanaryRuntime, WeatherApiStartupLoadError>,
    qualified: Result<LoadedForecastAdjustmentRuntimeV1, WeatherApiStartupLoadError>,
    temperature:
        Result<LoadedForecastAdjustmentTemperatureCanaryRuntime, WeatherApiStartupLoadError>,
    rain: Result<LoadedForecastAdjustmentRainRuntimeRegistryV1, WeatherApiStartupLoadError>,
}

impl WeatherApiStartupRuntimeLoader for FixtureLoaders {
    /// Load the fixture wind selection and record source order.
    fn load_wind_canary(
        &mut self,
    ) -> Result<LoadedForecastAdjustmentWindCanaryRuntime, WeatherApiStartupLoadError> {
        self.events.borrow_mut().push("wind".to_owned());
        self.wind.clone()
    }

    /// Load the fixture qualified fallback and record source order.
    fn load_qualified_wind(
        &mut self,
    ) -> Result<LoadedForecastAdjustmentRuntimeV1, WeatherApiStartupLoadError> {
        self.events.borrow_mut().push("qualified".to_owned());
        self.qualified.clone()
    }

    /// Load the fixture temperature selection and record source order.
    fn load_temperature(
        &mut self,
    ) -> Result<LoadedForecastAdjustmentTemperatureCanaryRuntime, WeatherApiStartupLoadError> {
        self.events.borrow_mut().push("temperature".to_owned());
        self.temperature.clone()
    }

    /// Load the fixture rain selection and record source order.
    fn load_rain(
        &mut self,
    ) -> Result<LoadedForecastAdjustmentRainRuntimeRegistryV1, WeatherApiStartupLoadError> {
        self.events.borrow_mut().push("rain".to_owned());
        self.rain.clone()
    }
}

struct FixtureClock {
    events: Trace,
}

impl WeatherApiStartupClock for FixtureClock {
    /// Return one fixed admitted startup instant.
    fn capture_loaded_at(&mut self) -> Result<CanonicalInstant, WeatherApiStartupPortError> {
        self.events.borrow_mut().push("clock".to_owned());
        Ok(CanonicalInstant {
            epoch_milliseconds: 1_778_032_923_000,
            utc: "2026-05-05T01:02:03.000Z".to_owned(),
        })
    }
}

struct FixtureWitnessLoader {
    events: Trace,
    result: Result<AdmittedAdjustmentRevisionCaptureWitness, WeatherApiStartupLoadError>,
}

impl AdjustmentRevisionCaptureWitnessLoader for FixtureWitnessLoader {
    /// Return one fixture verified witness and record lazy loading.
    fn load_verified_capture_witness(
        &mut self,
    ) -> Result<AdmittedAdjustmentRevisionCaptureWitness, WeatherApiStartupLoadError> {
        self.events.borrow_mut().push("witness".to_owned());
        self.result.clone()
    }
}

struct FixtureRawListenerSettings {
    events: Trace,
    public_port: Result<NonZeroU16, WeatherApiStartupPortError>,
    maintenance_port: Option<Result<NonZeroU16, WeatherApiStartupPortError>>,
    archive_origin: Option<Result<AdmittedMaintenanceArchiveOrigin, WeatherApiStartupPortError>>,
}

impl WeatherApiStartupRawListenerSettings for FixtureRawListenerSettings {
    /// Admit the fixture public setting and record its deferred read.
    fn admit_public_port(&mut self) -> Result<NonZeroU16, WeatherApiStartupPortError> {
        self.events.borrow_mut().push("public-settings".to_owned());
        self.public_port
    }

    /// Report maintenance-port source presence without parsing its value.
    fn maintenance_port_present(&self) -> bool {
        self.maintenance_port.is_some()
    }

    /// Report archive-origin source presence without parsing its value.
    fn maintenance_archive_origin_present(&self) -> bool {
        self.archive_origin.is_some()
    }

    /// Admit the fixture maintenance port and record the actual parse point.
    fn admit_maintenance_port(&mut self) -> Result<NonZeroU16, WeatherApiStartupPortError> {
        self.events
            .borrow_mut()
            .push("maintenance-port-settings".to_owned());
        self.maintenance_port
            .expect("fixture maintenance port must be present")
    }

    /// Admit the fixture origin and record the actual parse point.
    fn admit_maintenance_archive_origin(
        &mut self,
    ) -> Result<AdmittedMaintenanceArchiveOrigin, WeatherApiStartupPortError> {
        self.events
            .borrow_mut()
            .push("maintenance-origin-settings".to_owned());
        self.archive_origin
            .clone()
            .expect("fixture archive origin must be present")
    }
}

struct FixtureApplication {
    events: Trace,
}

impl WeatherApiStartupApplicationPort for FixtureApplication {
    /// Record public preparation after immutable snapshot construction.
    fn prepare_public(
        &mut self,
        snapshots: &Arc<WeatherApiStartupSnapshots>,
    ) -> Result<(), WeatherApiStartupPortError> {
        assert_eq!(snapshots.adjustment.loadedAt, "2026-05-05T01:02:03.000Z");
        self.events.borrow_mut().push("prepare-public".to_owned());
        Ok(())
    }

    /// Record maintenance preparation after capture-witness admission.
    fn prepare_maintenance(
        &mut self,
        snapshots: &Arc<WeatherApiStartupSnapshots>,
        preparation: &WeatherApiMaintenanceStartupPreparation,
    ) -> Result<(), WeatherApiStartupPortError> {
        assert_eq!(
            snapshots.temperature_adjustment.loadedAt,
            snapshots.rain_adjustment.loadedAt
        );
        assert_eq!(
            preparation.capture_witness.capture_epoch.utc,
            preparation.capture_witness.witness.epochAt
        );
        assert_eq!(
            preparation.configuration.archive_origin.canonical(),
            "https://archive.invalid"
        );
        self.events
            .borrow_mut()
            .push("prepare-maintenance".to_owned());
        Ok(())
    }

    /// Record the public listener before optional maintenance.
    fn listen_public(&mut self, port: NonZeroU16) -> Result<(), WeatherApiStartupPortError> {
        self.events
            .borrow_mut()
            .push(format!("listen-public:{}", port.get()));
        Ok(())
    }

    /// Record the separate maintenance listener after the public listener.
    fn listen_maintenance(&mut self, port: NonZeroU16) -> Result<(), WeatherApiStartupPortError> {
        self.events
            .borrow_mut()
            .push(format!("listen-maintenance:{}", port.get()));
        Ok(())
    }
}

#[test]
// preserve qualified fallback, shared clock, verified maintenance gating, and listener order
fn starts_public_and_maintenance_applications_in_node_order() {
    let events = Rc::new(RefCell::new(Vec::new()));
    let qualified =
        qualified_disabled(LoadedForecastAdjustmentRuntimeV1DisabledReasonCodeEnum::BundleMissing);
    let mut loaders = FixtureLoaders {
        events: Rc::clone(&events),
        wind: Ok(wind_disabled(
            LoadedForecastAdjustmentWindCanaryRuntimeV1DisabledReasonCodeEnum::RegistryInactive,
        )),
        qualified: Ok(qualified.clone()),
        temperature: Ok(temperature_disabled(
            LoadedForecastAdjustmentTemperatureCanaryRuntimeV1DisabledReasonCodeEnum::RegistryInactive,
        )),
        rain: Ok(rain_disabled(
            LoadedForecastAdjustmentRainRuntimeRegistryV1DisabledReasonCodeEnum::PolicyRaw,
        )),
    };
    let mut clock = FixtureClock {
        events: Rc::clone(&events),
    };
    let mut witness_loader = FixtureWitnessLoader {
        events: Rc::clone(&events),
        result: Ok(capture_witness()),
    };
    let mut application = FixtureApplication {
        events: Rc::clone(&events),
    };
    let started = start_weather_api(
        &mut loaders,
        &mut clock,
        &mut witness_loader,
        &mut application,
        configured_listeners(),
    )
    .expect("startup should succeed");

    assert_eq!(
        started.snapshots.adjustment.runtime,
        LoadedApiForecastAdjustmentRuntime::Standard(qualified)
    );
    assert_eq!(
        started.snapshots.adjustment.loadedAt,
        started.snapshots.temperature_adjustment.loadedAt
    );
    assert_eq!(
        started.snapshots.adjustment.loadedAt,
        started.snapshots.rain_adjustment.loadedAt
    );
    assert!(started.maintenance_listener_active);
    assert_eq!(
        *events.borrow(),
        [
            "wind",
            "qualified",
            "temperature",
            "rain",
            "clock",
            "prepare-public",
            "witness",
            "prepare-maintenance",
            "listen-public:8080",
            "listen-maintenance:8081",
        ]
    );
}

#[test]
// keep terminal wind policy and independent loader failures fail-raw
fn contains_family_loader_failures_and_skips_incomplete_maintenance_settings() {
    let events = Rc::new(RefCell::new(Vec::new()));
    let terminal_wind = wind_disabled(
        LoadedForecastAdjustmentWindCanaryRuntimeV1DisabledReasonCodeEnum::CanaryKilled,
    );
    let mut loaders = FixtureLoaders {
        events: Rc::clone(&events),
        wind: Ok(terminal_wind.clone()),
        qualified: Err(WeatherApiStartupLoadError),
        temperature: Err(WeatherApiStartupLoadError),
        rain: Err(WeatherApiStartupLoadError),
    };
    let mut clock = FixtureClock {
        events: Rc::clone(&events),
    };
    let mut witness_loader = FixtureWitnessLoader {
        events: Rc::clone(&events),
        result: Ok(capture_witness()),
    };
    let mut application = FixtureApplication {
        events: Rc::clone(&events),
    };
    let started = start_weather_api(
        &mut loaders,
        &mut clock,
        &mut witness_loader,
        &mut application,
        WeatherApiStartupListenerConfiguration {
            public_port: port(8080),
            maintenance_port: Some(port(8081)),
            maintenance_archive_origin: None,
        },
    )
    .expect("public startup should remain available");

    assert_eq!(
        started.snapshots.adjustment.runtime,
        LoadedApiForecastAdjustmentRuntime::Wind(terminal_wind)
    );
    let LoadedForecastAdjustmentTemperatureCanaryRuntime::Legacy { value, .. } =
        &started.snapshots.temperature_adjustment.runtime
    else {
        panic!("temperature loader failure must use the legacy fail-raw result")
    };
    assert!(matches!(
        value,
        LoadedForecastAdjustmentTemperatureCanaryRuntimeV1::Disabled(
            LoadedForecastAdjustmentTemperatureCanaryRuntimeV1Disabled {
                reasonCode:
                    LoadedForecastAdjustmentTemperatureCanaryRuntimeV1DisabledReasonCodeEnum::BundleInvalid,
                ..
            }
        )
    ));
    assert!(matches!(
        started.snapshots.rain_adjustment.runtime,
        LoadedForecastAdjustmentRainRuntimeRegistryV1::Disabled(
            LoadedForecastAdjustmentRainRuntimeRegistryV1Disabled {
                reasonCode:
                    LoadedForecastAdjustmentRainRuntimeRegistryV1DisabledReasonCodeEnum::RegistryInvalid,
                ..
            }
        )
    ));
    assert!(!started.maintenance_listener_active);
    assert_eq!(
        *events.borrow(),
        [
            "wind",
            "temperature",
            "rain",
            "clock",
            "prepare-public",
            "listen-public:8080",
        ]
    );
}

#[test]
// contain capture-witness loading failure after public preparation
fn keeps_public_listener_when_configured_maintenance_witness_is_unavailable() {
    let events = Rc::new(RefCell::new(Vec::new()));
    let mut loaders = FixtureLoaders {
        events: Rc::clone(&events),
        wind: Ok(wind_disabled(
            LoadedForecastAdjustmentWindCanaryRuntimeV1DisabledReasonCodeEnum::CanaryKilled,
        )),
        qualified: Err(WeatherApiStartupLoadError),
        temperature: Ok(temperature_disabled(
            LoadedForecastAdjustmentTemperatureCanaryRuntimeV1DisabledReasonCodeEnum::RegistryInactive,
        )),
        rain: Ok(rain_disabled(
            LoadedForecastAdjustmentRainRuntimeRegistryV1DisabledReasonCodeEnum::PolicyRaw,
        )),
    };
    let mut clock = FixtureClock {
        events: Rc::clone(&events),
    };
    let mut witness_loader = FixtureWitnessLoader {
        events: Rc::clone(&events),
        result: Err(WeatherApiStartupLoadError),
    };
    let mut application = FixtureApplication {
        events: Rc::clone(&events),
    };
    let started = start_weather_api(
        &mut loaders,
        &mut clock,
        &mut witness_loader,
        &mut application,
        configured_listeners(),
    )
    .expect("witness failure should not block public startup");

    assert!(!started.maintenance_listener_active);
    assert_eq!(
        *events.borrow(),
        [
            "wind",
            "temperature",
            "rain",
            "clock",
            "prepare-public",
            "witness",
            "listen-public:8080",
        ]
    );
}

#[test]
// leave malformed private settings unread when the optional witness is unavailable
fn unavailable_witness_keeps_public_listener_without_parsing_private_settings() {
    let events = Rc::new(RefCell::new(Vec::new()));
    let mut loaders = raw_settings_loaders(&events);
    let mut clock = FixtureClock {
        events: Rc::clone(&events),
    };
    let mut witness_loader = FixtureWitnessLoader {
        events: Rc::clone(&events),
        result: Err(WeatherApiStartupLoadError),
    };
    let mut application = FixtureApplication {
        events: Rc::clone(&events),
    };
    let mut settings = malformed_private_settings(&events);

    let started = start_weather_api_with_raw_listener_settings(
        &mut loaders,
        &mut clock,
        &mut witness_loader,
        &mut application,
        &mut settings,
    )
    .expect("unavailable witness must leave the public API running");

    assert!(!started.maintenance_listener_active);
    assert_eq!(
        *events.borrow(),
        [
            "wind",
            "temperature",
            "rain",
            "clock",
            "prepare-public",
            "public-settings",
            "witness",
            "listen-public:8080",
        ]
    );
}

#[test]
// parse the same malformed private setting only after witness admission
fn admitted_witness_exposes_malformed_private_setting() {
    let events = Rc::new(RefCell::new(Vec::new()));
    let mut loaders = raw_settings_loaders(&events);
    let mut clock = FixtureClock {
        events: Rc::clone(&events),
    };
    let mut witness_loader = FixtureWitnessLoader {
        events: Rc::clone(&events),
        result: Ok(capture_witness()),
    };
    let mut application = FixtureApplication {
        events: Rc::clone(&events),
    };
    let mut settings = malformed_private_settings(&events);

    let result = start_weather_api_with_raw_listener_settings(
        &mut loaders,
        &mut clock,
        &mut witness_loader,
        &mut application,
        &mut settings,
    );

    assert_eq!(
        result,
        Err(WeatherApiStartupError::MaintenanceListenerSettings)
    );
    assert_eq!(
        *events.borrow(),
        [
            "wind",
            "temperature",
            "rain",
            "clock",
            "prepare-public",
            "public-settings",
            "witness",
            "maintenance-port-settings",
        ]
    );
}

#[test]
// reject a verified witness paired with another admitted epoch instant
fn rejects_mismatched_capture_witness_time_pair() {
    let witness = capture_witness_value();
    let mismatch = AdmittedAdjustmentRevisionCaptureWitness::from_verified(
        witness,
        CanonicalInstant {
            epoch_milliseconds: 1,
            utc: "2000-01-01T00:00:00.001Z".to_owned(),
        },
    );
    assert_eq!(mismatch, Err(WeatherApiStartupLoadError));
}

/// Build one admitted nonzero listener port.
fn port(value: u16) -> NonZeroU16 {
    NonZeroU16::new(value).expect("fixture port must be nonzero")
}

/// Build public plus fully present optional maintenance settings.
fn configured_listeners() -> WeatherApiStartupListenerConfiguration {
    WeatherApiStartupListenerConfiguration {
        public_port: port(8080),
        maintenance_port: Some(port(8081)),
        maintenance_archive_origin: Some(AdmittedMaintenanceArchiveOrigin::admitted(
            "https://archive.invalid".to_owned(),
        )),
    }
}

/// Build ordinary disabled runtimes for raw-settings ordering tests.
fn raw_settings_loaders(events: &Trace) -> FixtureLoaders {
    FixtureLoaders {
        events: Rc::clone(events),
        wind: Ok(wind_disabled(
            LoadedForecastAdjustmentWindCanaryRuntimeV1DisabledReasonCodeEnum::CanaryKilled,
        )),
        qualified: Err(WeatherApiStartupLoadError),
        temperature: Ok(temperature_disabled(
            LoadedForecastAdjustmentTemperatureCanaryRuntimeV1DisabledReasonCodeEnum::RegistryInactive,
        )),
        rain: Ok(rain_disabled(
            LoadedForecastAdjustmentRainRuntimeRegistryV1DisabledReasonCodeEnum::PolicyRaw,
        )),
    }
}

/// Build present private raw settings whose port parser refuses the value.
fn malformed_private_settings(events: &Trace) -> FixtureRawListenerSettings {
    FixtureRawListenerSettings {
        events: Rc::clone(events),
        public_port: Ok(port(8080)),
        maintenance_port: Some(Err(WeatherApiStartupPortError)),
        archive_origin: Some(Ok(AdmittedMaintenanceArchiveOrigin::admitted(
            "https://archive.invalid".to_owned(),
        ))),
    }
}

/// Build one typed disabled wind-canary runtime.
fn wind_disabled(
    reason: LoadedForecastAdjustmentWindCanaryRuntimeV1DisabledReasonCodeEnum,
) -> LoadedForecastAdjustmentWindCanaryRuntime {
    LoadedForecastAdjustmentWindCanaryRuntime::Legacy {
        value: LoadedForecastAdjustmentWindCanaryRuntimeV1::Disabled(
            LoadedForecastAdjustmentWindCanaryRuntimeV1Disabled {
                bundle: LoadedForecastAdjustmentWindCanaryRuntimeV1DisabledBundleNull,
                reasonCode: reason,
                state: LoadedForecastAdjustmentWindCanaryRuntimeV1DisabledStateLiteral,
            },
        ),
        comparatorAuthority: FieldPresence::Missing,
    }
}

/// Build one typed disabled qualified runtime.
fn qualified_disabled(
    reason: LoadedForecastAdjustmentRuntimeV1DisabledReasonCodeEnum,
) -> LoadedForecastAdjustmentRuntimeV1 {
    LoadedForecastAdjustmentRuntimeV1::Disabled(LoadedForecastAdjustmentRuntimeV1Disabled {
        bundle: LoadedForecastAdjustmentRuntimeV1DisabledBundleNull,
        reasonCode: reason,
        state: LoadedForecastAdjustmentRuntimeV1DisabledStateLiteral,
    })
}

/// Build one typed disabled temperature runtime.
fn temperature_disabled(
    reason: LoadedForecastAdjustmentTemperatureCanaryRuntimeV1DisabledReasonCodeEnum,
) -> LoadedForecastAdjustmentTemperatureCanaryRuntime {
    LoadedForecastAdjustmentTemperatureCanaryRuntime::Legacy {
        value: LoadedForecastAdjustmentTemperatureCanaryRuntimeV1::Disabled(
            LoadedForecastAdjustmentTemperatureCanaryRuntimeV1Disabled {
                bundle: LoadedForecastAdjustmentTemperatureCanaryRuntimeV1DisabledBundleNull,
                reasonCode: reason,
                state: LoadedForecastAdjustmentTemperatureCanaryRuntimeV1DisabledStateLiteral,
            },
        ),
        comparatorAuthority: FieldPresence::Missing,
    }
}

/// Build one typed disabled rain runtime.
fn rain_disabled(
    reason: LoadedForecastAdjustmentRainRuntimeRegistryV1DisabledReasonCodeEnum,
) -> LoadedForecastAdjustmentRainRuntimeRegistryV1 {
    LoadedForecastAdjustmentRainRuntimeRegistryV1::Disabled(
        LoadedForecastAdjustmentRainRuntimeRegistryV1Disabled {
            artifactSha256: LoadedForecastAdjustmentRainRuntimeRegistryV1DisabledArtifactSha256Null,
            comparatorAuthority: FieldPresence::Missing,
            reasonCode: reason,
            state: LoadedForecastAdjustmentRainRuntimeRegistryV1DisabledStateLiteral,
        },
    )
}

/// Build one admitted verified capture witness and canonical instant pair.
fn capture_witness() -> AdmittedAdjustmentRevisionCaptureWitness {
    AdmittedAdjustmentRevisionCaptureWitness::from_verified(
        capture_witness_value(),
        CanonicalInstant {
            epoch_milliseconds: 946_684_800_000,
            utc: "2000-01-01T00:00:00.000Z".to_owned(),
        },
    )
    .expect("fixture witness and instant must agree")
}

/// Build one source-shaped verified capture witness value.
fn capture_witness_value() -> AdjustmentRevisionCaptureEpochWitness {
    AdjustmentRevisionCaptureEpochWitness {
        activationKind: AdjustmentRevisionCaptureEpochWitnessActivationKindLiteral,
        archiveCommitOrdinal: AdjustmentRevisionCaptureEpochWitnessArchiveCommitOrdinalLiteral,
        catalogFrontierSha256: "1".repeat(64),
        contractVersion: AdjustmentRevisionCaptureEpochWitnessContractVersionLiteral,
        controlPlaneSha256: "2".repeat(64),
        controlPlaneVersion: AdjustmentRevisionCaptureEpochWitnessControlPlaneVersionEnum::Value17,
        databaseMigrationHistorySha256: "3".repeat(64),
        epochAt: "2000-01-01T00:00:00.000Z".to_owned(),
        servingSnapshotSha256: "4".repeat(64),
        sourceCommit: "abc123".to_owned(),
        sourceRelease: "v1".to_owned(),
        sourceServerImageDigest: "sha256:server".to_owned(),
        sourceWebImageDigest: "sha256:web".to_owned(),
        witnessSha256: "5".repeat(64),
    }
}
