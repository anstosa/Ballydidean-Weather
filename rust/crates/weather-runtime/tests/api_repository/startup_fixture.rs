//! Composed API startup and public repository fixture over one shared pool.

use std::cell::RefCell;
use std::num::NonZeroU16;
use std::path::PathBuf;
use std::rc::Rc;

#[path = "maintenance_capture_fixture.rs"]
pub(crate) mod maintenance_capture_fixture;

#[path = "../support/capture_epoch.rs"]
mod capture_epoch;

use capture_epoch::{FIXTURE_EPOCH, node_capture_epoch_cases};

use weather_api::{
    AdmittedMaintenanceArchiveOrigin, CanonicalInstant, WeatherApiStartupClock,
    WeatherApiStartupListenerConfiguration, WeatherApiStartupLoadError, WeatherApiStartupPortError,
    WeatherApiStartupRuntimeLoader, WeatherServiceWireEncoder,
    start_weather_api_with_raw_listener_settings,
};
use weather_contracts::{
    ForecastAdjustmentMaintenanceRuntimePackage, JsonValue,
    LoadedForecastAdjustmentRainRuntimeRegistryV1, LoadedForecastAdjustmentRuntimeV1,
    LoadedForecastAdjustmentTemperatureCanaryRuntime, LoadedForecastAdjustmentWindCanaryRuntime,
};
use weather_database::{
    DatabasePasswordFileRead, DatabaseProcessEnvironmentKey, DatabaseProcessEnvironmentRead,
    DatabaseProcessReadError, Sha256Digest,
};
use weather_runtime::{
    ApiMaintenanceCandidateCodec, ApiMaintenanceCandidateError, ApiMaintenanceCandidateFiles,
    ApiMaintenanceCandidateMembers, ApiMigrationDirectoryResolve, ApiProcessEnvironmentKey,
    ApiProcessEnvironmentRead, ApiProcessReadError, CaptureEpochWitnessError,
    CaptureEpochWitnessFile, PreparedPrivateMaintenanceApi, PreparedPublicWeatherApi,
    RetainedCaptureEpochWitnessLoader, RetainedWeatherApiStartup, RuntimeMigrationSource,
    WeatherApiPreparedListeners, WeatherApiProcessApplications, WeatherApiReadPoolConnector,
    load_api_process_configuration,
};

use super::{
    ApiPoolEvent, FixtureApiReadPool, FixtureCheckout, FixtureExternal, FixturePoolError,
    InactiveForecastServiceRuntime, QueryParameter, SIDECAR_AS_OF, UnusedBoundary,
    WeatherServiceBody, WeatherServiceBoundaries, instant,
};

type Trace = Rc<RefCell<Vec<String>>>;

/// Connector that exposes the identity of the one prepared shared fixture pool.
struct StartupConnector {
    checkout_count: usize,
    pool_events: Rc<RefCell<Vec<ApiPoolEvent>>>,
    trace: Trace,
}

impl WeatherApiReadPoolConnector for StartupConnector {
    type Error = FixturePoolError;
    type Pool = FixtureApiReadPool;

    /// Prepare one ready checkout and record the backing shared queue identity.
    fn prepare_pool(
        &mut self,
        _configuration: weather_database::DatabasePoolConfiguration,
    ) -> Result<Self::Pool, Self::Error> {
        let attempts = Rc::new(RefCell::new(std::collections::VecDeque::from_iter(
            std::iter::repeat_n(FixtureCheckout::Ready, self.checkout_count),
        )));
        self.trace
            .borrow_mut()
            .push(format!("prepare-pool:{}", Rc::as_ptr(&attempts) as usize));
        Ok(FixtureApiReadPool {
            events: Rc::clone(&self.pool_events),
            attempts,
        })
    }
}

/// Fail-raw loader fixture that still exercises the original family load order.
struct StartupLoaders {
    trace: Trace,
}

impl WeatherApiStartupRuntimeLoader for StartupLoaders {
    /// Record and fail the wind candidate load through the ordinary contained path.
    fn load_wind_canary(
        &mut self,
    ) -> Result<LoadedForecastAdjustmentWindCanaryRuntime, WeatherApiStartupLoadError> {
        self.trace.borrow_mut().push("wind".to_owned());
        Err(WeatherApiStartupLoadError)
    }

    /// Detect an unexpected qualified lookup for a failed wind candidate load.
    fn load_qualified_wind(
        &mut self,
    ) -> Result<LoadedForecastAdjustmentRuntimeV1, WeatherApiStartupLoadError> {
        panic!("failed wind candidate must remain terminal")
    }

    /// Record and fail the temperature candidate load through the contained path.
    fn load_temperature(
        &mut self,
    ) -> Result<LoadedForecastAdjustmentTemperatureCanaryRuntime, WeatherApiStartupLoadError> {
        self.trace.borrow_mut().push("temperature".to_owned());
        Err(WeatherApiStartupLoadError)
    }

    /// Record and fail the rain candidate load through the contained path.
    fn load_rain(
        &mut self,
    ) -> Result<LoadedForecastAdjustmentRainRuntimeRegistryV1, WeatherApiStartupLoadError> {
        self.trace.borrow_mut().push("rain".to_owned());
        Err(WeatherApiStartupLoadError)
    }
}

/// Fixed startup clock preserving one immutable snapshot instant.
struct StartupClock {
    trace: Trace,
}

impl WeatherApiStartupClock for StartupClock {
    /// Record the single snapshot clock capture.
    fn capture_loaded_at(&mut self) -> Result<CanonicalInstant, WeatherApiStartupPortError> {
        self.trace.borrow_mut().push("clock".to_owned());
        Ok(instant(1_778_032_923_000, "2026-05-05T01:02:03.000Z"))
    }
}

/// Verified witness loader recording the private admission point.
struct StartupWitnessFile {
    trace: Trace,
    bytes: Vec<u8>,
}

impl CaptureEpochWitnessFile for StartupWitnessFile {
    /// Supply original witness bytes only when the composed startup requests them.
    fn read_installed_witness(&mut self) -> Result<Vec<u8>, CaptureEpochWitnessError> {
        self.trace.borrow_mut().push("witness".to_owned());
        Ok(std::mem::take(&mut self.bytes))
    }
}

/// Candidate file owner proving maintenance preparation invokes the real loader once.
struct EmptyCandidateFiles {
    trace: Trace,
}

impl ApiMaintenanceCandidateFiles for EmptyCandidateFiles {
    /// Return one canonical empty v1 catalog after recording private preparation.
    fn read_catalog(&mut self) -> Result<Vec<u8>, ApiMaintenanceCandidateError> {
        self.trace.borrow_mut().push("load-candidates".to_owned());
        Ok(
            b"{\"contractVersion\":\"adjustment-installed-candidate-catalog/v1\",\"entries\":[]}\n"
                .to_vec(),
        )
    }

    /// Reject member loading because the empty catalog selects no candidate.
    fn read_members(
        &mut self,
        _family: weather_contracts::AdjustmentShadowFamily,
        _action_sha256: &str,
        _candidate_sha256: &str,
        _bundle_sha256: &str,
    ) -> Result<ApiMaintenanceCandidateMembers, ApiMaintenanceCandidateError> {
        panic!("empty candidate catalog has no members")
    }
}

/// Minimal approved codec for the exact empty canonical catalog fixture.
struct EmptyCandidateCodec;

impl weather_runtime::MaintenanceCanonicalJson for EmptyCandidateCodec {
    /// Decode only the exact retained empty catalog bytes.
    fn parse_canonical_object(&mut self, bytes: &[u8]) -> Option<JsonValue> {
        (bytes
            == b"{\"contractVersion\":\"adjustment-installed-candidate-catalog/v1\",\"entries\":[]}\n")
            .then(|| {
                JsonValue::Object(vec![
                    (
                        "contractVersion".encode_utf16().collect(),
                        JsonValue::String(
                            "adjustment-installed-candidate-catalog/v1"
                                .encode_utf16()
                                .collect(),
                        ),
                    ),
                    ("entries".encode_utf16().collect(), JsonValue::Array(Vec::new())),
                ])
            })
    }

    /// Reject unselected candidate JSON encoding in this empty-catalog fixture.
    fn encode_canonical_json(&mut self, _value: &JsonValue) -> Option<Vec<u8>> {
        None
    }

    /// Reject hashing because no catalog candidate reaches identity admission.
    fn sha256(&mut self, _bytes: &[u8]) -> Option<Sha256Digest> {
        None
    }
}

impl ApiMaintenanceCandidateCodec for EmptyCandidateCodec {
    /// Reject clocks because no selected candidate reaches registration admission.
    fn is_canonical_utc_instant(&mut self, _value: &str) -> bool {
        false
    }

    /// Reject package decoding because no selected candidate reaches package admission.
    fn decode_maintenance_package(
        &mut self,
        _value: &JsonValue,
    ) -> Option<ForecastAdjustmentMaintenanceRuntimePackage> {
        None
    }
}

/// Listener fixture retaining both real prepared applications for composed assertions.
pub(super) struct PreparedListeners {
    pub(super) trace: Trace,
    pub(super) public: Option<PreparedPublicWeatherApi<FixtureApiReadPool, FixtureExternal>>,
    pub(super) maintenance: Option<PreparedPrivateMaintenanceApi<FixtureApiReadPool>>,
}

impl WeatherApiPreparedListeners<FixtureApiReadPool, FixtureExternal> for PreparedListeners {
    /// Retain the real public handler at the first listener transition.
    fn listen_public(
        &mut self,
        port: NonZeroU16,
        application: PreparedPublicWeatherApi<FixtureApiReadPool, FixtureExternal>,
    ) -> Result<(), WeatherApiStartupPortError> {
        self.trace
            .borrow_mut()
            .push(format!("listen-public:{}", port.get()));
        self.public = Some(application);
        Ok(())
    }

    /// Retain the private handler and prove its admitted witness and candidate load.
    fn listen_maintenance(
        &mut self,
        port: NonZeroU16,
        application: PreparedPrivateMaintenanceApi<FixtureApiReadPool>,
    ) -> Result<(), WeatherApiStartupPortError> {
        let (preparation, snapshots, candidates) = application.capture_material();
        assert_eq!(preparation.capture_witness.capture_epoch.utc, FIXTURE_EPOCH);
        assert_eq!(snapshots.adjustment.loadedAt, "2026-05-05T01:02:03.000Z");
        assert_eq!(
            candidates.registration_sha256(weather_contracts::AdjustmentShadowFamily::Wind),
            None
        );
        self.trace
            .borrow_mut()
            .push(format!("listen-maintenance:{}", port.get()));
        self.maintenance = Some(application);
        Ok(())
    }
}

/// Typed wire encoder returning one exact public sites representation.
struct SitesEncoder {
    bytes: Vec<u8>,
    saw_sites: bool,
}

impl WeatherServiceWireEncoder for SitesEncoder {
    type Error = ();

    /// Validate the typed body before returning fixture-owned JSON bytes.
    fn encode<'a>(&'a mut self, body: &WeatherServiceBody) -> Result<&'a [u8], Self::Error> {
        let WeatherServiceBody::Sites(sites) = body else {
            return Err(());
        };
        self.saw_sites = sites
            .data
            .first()
            .is_some_and(|site| site.slug == "ballydidean");
        Ok(&self.bytes)
    }
}

/// Fixture environment supplying only the normal process configuration inputs.
struct ProcessEnvironment;

impl DatabaseProcessEnvironmentRead for ProcessEnvironment {
    /// Return the minimum complete database configuration without reading process state.
    fn read_environment(
        &self,
        key: DatabaseProcessEnvironmentKey,
    ) -> Result<Option<String>, DatabaseProcessReadError> {
        Ok(match key {
            DatabaseProcessEnvironmentKey::DatabaseName => Some("weather".to_owned()),
            DatabaseProcessEnvironmentKey::Host => Some("database".to_owned()),
            DatabaseProcessEnvironmentKey::PasswordFile => Some("/fixture/password".to_owned()),
            DatabaseProcessEnvironmentKey::User => Some("weather".to_owned()),
            _ => None,
        })
    }
}

impl ApiProcessEnvironmentRead for ProcessEnvironment {
    /// Return one explicit release and otherwise preserve normal defaults.
    fn read_api_environment(
        &self,
        key: ApiProcessEnvironmentKey,
    ) -> Result<Option<String>, ApiProcessReadError> {
        Ok((key == ApiProcessEnvironmentKey::Release).then(|| "2026.11.23-1".to_owned()))
    }
}

/// Fixture mounted-password owner returning one bounded nonempty line.
struct PasswordFiles;

impl DatabasePasswordFileRead for PasswordFiles {
    /// Return the selected fixture password bytes without touching the filesystem.
    fn read_password_file(&self, path: &str) -> Result<Vec<u8>, DatabaseProcessReadError> {
        assert_eq!(path, "/fixture/password");
        Ok(b"fixture-password\n".to_vec())
    }
}

/// Fixture migration resolver retaining the configured/default relative path.
struct MigrationDirectories;

impl ApiMigrationDirectoryResolve for MigrationDirectories {
    /// Resolve the default migration directory beneath one inert fixture root.
    fn resolve_migration_directory(&self, path: &str) -> Result<PathBuf, ApiProcessReadError> {
        Ok(PathBuf::from("/fixture").join(path))
    }
}

/// Reusable prepared handlers and their one shared fixture-pool trace.
pub(super) struct PreparedApiFixture {
    pub(super) listeners: PreparedListeners,
    pub(super) pool_events: Rc<RefCell<Vec<ApiPoolEvent>>>,
    pub(super) prepared_identity: usize,
}

/// Compose both actual API handlers once through the complete startup boundary.
pub(super) fn start_prepared_api_fixture() -> PreparedApiFixture {
    start_prepared_api_fixture_with_checkouts(1)
}

/// Compose both handlers with the exact number of ready database checkouts a test needs.
pub(super) fn start_prepared_api_fixture_with_checkouts(
    checkout_count: usize,
) -> PreparedApiFixture {
    assert!(checkout_count > 0);
    let trace = Rc::new(RefCell::new(Vec::new()));
    let pool_events = Rc::new(RefCell::new(Vec::new()));
    let configuration =
        load_api_process_configuration(&ProcessEnvironment, &PasswordFiles, &MigrationDirectories)
            .expect("fixture process configuration");
    let listeners = PreparedListeners {
        trace: Rc::clone(&trace),
        public: None,
        maintenance: None,
    };
    let applications = WeatherApiProcessApplications::new(
        {
            let trace = Rc::clone(&trace);
            move |migration| {
                assert_eq!(migration.release(), "2026.11.23-1");
                trace.borrow_mut().push("bind-public".to_owned());
                Ok(FixtureExternal::default())
            }
        },
        listeners,
    );
    let connector = StartupConnector {
        checkout_count,
        pool_events: Rc::clone(&pool_events),
        trace: Rc::clone(&trace),
    };
    let mut application = RetainedWeatherApiStartup::new(
        configuration,
        connector,
        EmptyCandidateFiles {
            trace: Rc::clone(&trace),
        },
        EmptyCandidateCodec,
        applications,
    );
    let mut loaders = StartupLoaders {
        trace: Rc::clone(&trace),
    };
    let mut clock = StartupClock {
        trace: Rc::clone(&trace),
    };
    let witness = node_capture_epoch_cases()
        .into_iter()
        .next()
        .expect("original Node witness");
    let mut witness = RetainedCaptureEpochWitnessLoader::new(
        StartupWitnessFile {
            trace: Rc::clone(&trace),
            bytes: witness.bytes.clone(),
        },
        witness.clone(),
        witness,
    );
    let mut settings = WeatherApiStartupListenerConfiguration {
        public_port: NonZeroU16::new(8080).expect("public port"),
        maintenance_port: Some(NonZeroU16::new(8081).expect("maintenance port")),
        maintenance_archive_origin: Some(AdmittedMaintenanceArchiveOrigin::admitted(
            "https://archive.invalid".to_owned(),
        )),
    };

    let started = start_weather_api_with_raw_listener_settings(
        &mut loaders,
        &mut clock,
        &mut witness,
        &mut application,
        &mut settings,
    )
    .expect("composed startup");
    assert!(started.maintenance_listener_active);
    let applications = application.into_binding();
    let listeners = applications.into_listeners();
    let maintenance = listeners
        .maintenance
        .as_ref()
        .expect("maintenance application");
    let public = listeners.public.as_ref().expect("public application");
    // the two retained handlers use the same immutable startup model allocation
    assert!(std::ptr::eq(
        public.snapshots(),
        maintenance.capture_material().1
    ));
    assert!(std::ptr::eq(public.snapshots(), started.snapshots.as_ref()));

    let prepared_identity = trace
        .borrow()
        .iter()
        .find_map(|event| event.strip_prefix("prepare-pool:"))
        .and_then(|value| value.parse::<usize>().ok())
        .expect("prepared pool identity");
    assert_eq!(
        pool_events.borrow().as_slice(),
        [
            ApiPoolEvent::Share(prepared_identity),
            ApiPoolEvent::Share(prepared_identity)
        ]
    );
    let expected_trace = vec![
        "wind".to_owned(),
        "temperature".to_owned(),
        "rain".to_owned(),
        "clock".to_owned(),
        format!("prepare-pool:{prepared_identity}"),
        "bind-public".to_owned(),
        "witness".to_owned(),
        "load-candidates".to_owned(),
        "load-candidates".to_owned(),
        "listen-public:8080".to_owned(),
        "listen-maintenance:8081".to_owned(),
    ];
    assert_eq!(*trace.borrow(), expected_trace);
    PreparedApiFixture {
        listeners,
        pool_events,
        prepared_identity,
    }
}

/// Compose real startup, application, wire and SQL owners over one shared pool.
#[test]
fn starts_and_serves_public_sites_over_one_retained_pool() {
    let PreparedApiFixture {
        mut listeners,
        pool_events,
        prepared_identity,
    } = start_prepared_api_fixture();

    let boundary = UnusedBoundary;
    let inactive = InactiveForecastServiceRuntime::unconfigured(SIDECAR_AS_OF, SIDECAR_AS_OF);
    let boundaries = WeatherServiceBoundaries {
        nominal_data_at: None,
        time_resolver: &boundary,
        history: &boundary,
        tides: &boundary,
        forecast: &boundary,
        forecast_runtime: &inactive,
        forecast_wind_runtime: None,
        forecast_temperature_runtime: None,
        forecast_rain_runtime: None,
        trends: &boundary,
        daily_precipitation: &boundary,
        rain_collection: &boundary,
        forecast_observation: &boundary,
        version: None,
    };
    let mut encoder = SitesEncoder {
        bytes: b"{\"data\":[{\"slug\":\"ballydidean\"}]}".to_vec(),
        saw_sites: false,
    };
    let response = listeners
        .public
        .as_mut()
        .expect("prepared public application")
        .serve(
            &mut super::FixtureClock,
            &boundaries,
            weather_api::DecodedServiceRequest {
                method: "GET",
                pathname: "/api/v1/sites",
                query: &[] as &[QueryParameter<'_>],
            },
            &mut encoder,
        );

    assert_eq!(response.status, 200);
    assert_eq!(response.headers.cache_control, Some("no-store"));
    assert_eq!(response.body, b"{\"data\":[{\"slug\":\"ballydidean\"}]}");
    assert!(encoder.saw_sites);
    assert_eq!(
        pool_events.borrow().as_slice(),
        [
            ApiPoolEvent::Share(prepared_identity),
            ApiPoolEvent::Share(prepared_identity),
            ApiPoolEvent::Checkout,
            ApiPoolEvent::Query(weather_database::LIST_ACTIVE_SITES_SQL, 0),
            ApiPoolEvent::Release(weather_runtime::ApiReadSessionDisposition::Recycle),
        ]
    );
    maintenance_capture_fixture::check_worker_capture(
        listeners
            .maintenance
            .as_ref()
            .expect("prepared private application"),
    );
}
