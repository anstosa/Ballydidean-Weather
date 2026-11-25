//! Loopback lifecycle proof for the concrete lazy-PostgreSQL API server.

use std::{
    io::{ErrorKind, Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    num::NonZeroU16,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use tokio::{runtime::Builder, sync::oneshot, task::LocalSet};
use weather_api::{
    AdjustmentRevisionCaptureWitnessLoader, AdmittedAdjustmentRevisionCaptureWitness,
    CanonicalInstant, WeatherApiStartupClock, WeatherApiStartupListenerConfiguration,
    WeatherApiStartupLoadError, WeatherApiStartupPortError, WeatherApiStartupRuntimeLoader,
    start_weather_api,
};
use weather_contracts::{
    LoadedForecastAdjustmentRainRuntimeRegistryV1, LoadedForecastAdjustmentRuntimeV1,
    LoadedForecastAdjustmentTemperatureCanaryRuntime, LoadedForecastAdjustmentWindCanaryRuntime,
};
use weather_database::{
    DatabasePasswordFileRead, DatabaseProcessEnvironmentKey, DatabaseProcessEnvironmentRead,
    DatabaseProcessReadError,
};

use crate::{
    ApiMigrationDirectoryResolve, ApiProcessEnvironmentKey, ApiProcessEnvironmentRead,
    ApiProcessMigrationSource, ApiProcessReadError, NativeApiMaintenanceCandidateFiles,
    NativeApiModelAdmission, NativeApiPreparedListeners, NativeHttpListenerConfiguration,
    NativePostgresApiConnector, NativePostgresApiExternal, NativePostgresPublicHandler,
    NativeWirePrimitives, RetainedWeatherApiStartup, StartedNativePostgresApi,
    WeatherApiProcessApplications, load_api_process_configuration, native_postgres_api_external,
    serve_started_native_postgres_api,
};

const EXPECTED_NOT_FOUND: &[u8] =
    b"{\"error\":{\"code\":\"not_found\",\"message\":\"Endpoint not found\"}}";

/// Supply only explicit fixture configuration values without consulting process state.
struct ProcessEnvironment {
    database_port: u16,
}

impl DatabaseProcessEnvironmentRead for ProcessEnvironment {
    /// Return the complete inert database configuration selected by the test.
    fn read_environment(
        &self,
        key: DatabaseProcessEnvironmentKey,
    ) -> Result<Option<String>, DatabaseProcessReadError> {
        Ok(match key {
            DatabaseProcessEnvironmentKey::ApplicationName => {
                Some("weather-api-loopback-test".to_owned())
            }
            DatabaseProcessEnvironmentKey::DatabaseName => Some("weather_fixture".to_owned()),
            DatabaseProcessEnvironmentKey::Host => Some("127.0.0.1".to_owned()),
            DatabaseProcessEnvironmentKey::PasswordFile => Some("/fixture/not-a-secret".to_owned()),
            DatabaseProcessEnvironmentKey::Port => Some(self.database_port.to_string()),
            DatabaseProcessEnvironmentKey::Ssl => Some("false".to_owned()),
            DatabaseProcessEnvironmentKey::User => Some("weather_fixture".to_owned()),
            DatabaseProcessEnvironmentKey::LockTimeoutMs
            | DatabaseProcessEnvironmentKey::StatementTimeoutMs => None,
        })
    }
}

impl ApiProcessEnvironmentRead for ProcessEnvironment {
    /// Return one fixed source release and no optional listener or authorization settings.
    fn read_api_environment(
        &self,
        key: ApiProcessEnvironmentKey,
    ) -> Result<Option<String>, ApiProcessReadError> {
        Ok((key == ApiProcessEnvironmentKey::Release).then(|| "2026.11.24-1".to_owned()))
    }
}

/// Return one bounded dummy line only for the injected fixture reference.
struct PasswordFiles;

impl DatabasePasswordFileRead for PasswordFiles {
    /// Avoid every ambient filesystem or credential read during configuration loading.
    fn read_password_file(&self, path: &str) -> Result<Vec<u8>, DatabaseProcessReadError> {
        assert_eq!(path, "/fixture/not-a-secret");
        Ok(b"fixture-only-password\n".to_vec())
    }
}

/// Resolve migration input beneath the checked-out source tree without reading its contents.
struct MigrationDirectories {
    repository_root: PathBuf,
}

impl ApiMigrationDirectoryResolve for MigrationDirectories {
    /// Preserve the selected relative source migration directory under the fixture root.
    fn resolve_migration_directory(&self, path: &str) -> Result<PathBuf, ApiProcessReadError> {
        Ok(self.repository_root.join(path))
    }
}

/// Exercise startup's contained fail-raw model dispositions without fixture registry I/O.
struct FailRawModels;

impl WeatherApiStartupRuntimeLoader for FailRawModels {
    /// Keep wind disabled through the startup owner's ordinary contained failure path.
    fn load_wind_canary(
        &mut self,
    ) -> Result<LoadedForecastAdjustmentWindCanaryRuntime, WeatherApiStartupLoadError> {
        Err(WeatherApiStartupLoadError)
    }

    /// This branch cannot be reached after the contained wind loader failure.
    fn load_qualified_wind(
        &mut self,
    ) -> Result<LoadedForecastAdjustmentRuntimeV1, WeatherApiStartupLoadError> {
        Err(WeatherApiStartupLoadError)
    }

    /// Keep temperature disabled through its independent contained failure path.
    fn load_temperature(
        &mut self,
    ) -> Result<LoadedForecastAdjustmentTemperatureCanaryRuntime, WeatherApiStartupLoadError> {
        Err(WeatherApiStartupLoadError)
    }

    /// Keep rain disabled through its independent contained failure path.
    fn load_rain(
        &mut self,
    ) -> Result<LoadedForecastAdjustmentRainRuntimeRegistryV1, WeatherApiStartupLoadError> {
        Err(WeatherApiStartupLoadError)
    }
}

/// Return one source-valid startup snapshot clock without sampling the host clock.
struct StartupClock;

impl WeatherApiStartupClock for StartupClock {
    /// Preserve one exact millisecond instant in both startup representations.
    fn capture_loaded_at(&mut self) -> Result<CanonicalInstant, WeatherApiStartupPortError> {
        Ok(CanonicalInstant {
            epoch_milliseconds: 1_795_558_400_000,
            utc: "2026-11-24T22:13:20.000Z".to_owned(),
        })
    }
}

/// Refuse private witness loading because this fixture configures no maintenance listener.
struct NoWitness;

impl AdjustmentRevisionCaptureWitnessLoader for NoWitness {
    /// Prove public-only startup never needs private capture material.
    fn load_verified_capture_witness(
        &mut self,
    ) -> Result<AdmittedAdjustmentRevisionCaptureWitness, WeatherApiStartupLoadError> {
        Err(WeatherApiStartupLoadError)
    }
}

/// Build the production external owner over the already admitted fixture migration source.
fn build_external(
    source: ApiProcessMigrationSource,
) -> Result<NativePostgresApiExternal, WeatherApiStartupPortError> {
    Ok(native_postgres_api_external(source))
}

/// Return bounded listener limits for the single finite loopback exchange.
fn listener_configuration() -> NativeHttpListenerConfiguration {
    NativeHttpListenerConfiguration::new(
        2,
        Duration::from_secs(4),
        Duration::from_secs(12),
        Duration::from_secs(1),
        32 * 1024,
        true,
    )
    .expect("valid loopback listener limits")
}

/// Exchange one close-delimited request with the already-bound native listener.
fn exchange(address: SocketAddr, request: &[u8]) -> Vec<u8> {
    let mut socket = TcpStream::connect(address).expect("connect native API loopback");
    socket
        .set_read_timeout(Some(Duration::from_secs(4)))
        .expect("bound loopback response deadline");
    socket.write_all(request).expect("write loopback request");
    let mut response = Vec::new();
    socket
        .read_to_end(&mut response)
        .expect("read complete loopback response");
    response
}

/// Serve an unknown route, then prove connection, listener, request, and pool drain completion.
#[test]
fn native_postgres_api_serves_and_drains_without_database_connection() {
    let database_sentinel = TcpListener::bind(("127.0.0.1", 0)).expect("bind database sentinel");
    database_sentinel
        .set_nonblocking(true)
        .expect("make database sentinel observable");
    let database_port = database_sentinel.local_addr().unwrap().port();
    let api_reservation = TcpListener::bind(("127.0.0.1", 0)).expect("reserve API port");
    let api_port = NonZeroU16::new(api_reservation.local_addr().unwrap().port()).unwrap();
    drop(api_reservation);

    let repository_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .expect("canonical source fixture root");
    let configuration = load_api_process_configuration(
        &ProcessEnvironment { database_port },
        &PasswordFiles,
        &MigrationDirectories {
            repository_root: repository_root.clone(),
        },
    )
    .expect("load source fixture configuration");
    let migration = configuration.migration.clone();
    let listeners =
        NativeApiPreparedListeners::loopback(listener_configuration(), listener_configuration());
    let applications = WeatherApiProcessApplications::new(
        build_external
            as fn(
                ApiProcessMigrationSource,
            ) -> Result<NativePostgresApiExternal, WeatherApiStartupPortError>,
        listeners,
    );
    let mut startup = RetainedWeatherApiStartup::new(
        configuration,
        NativePostgresApiConnector,
        NativeApiMaintenanceCandidateFiles::new(repository_root)
            .expect("bind source fixture candidate root"),
        NativeWirePrimitives,
        applications,
    );
    let mut models = FailRawModels;
    let mut clock = StartupClock;
    let mut witness = NoWitness;
    let started = start_weather_api(
        &mut models,
        &mut clock,
        &mut witness,
        &mut startup,
        WeatherApiStartupListenerConfiguration {
            public_port: api_port,
            maintenance_port: None,
            maintenance_archive_origin: None,
        },
    )
    .expect("prepare public-only native API");
    assert!(!started.maintenance_listener_active);
    let listeners = startup.into_binding().into_listeners();
    let public_address = listeners.public_local_addr().expect("bound public address");
    let model_admission = Arc::new(NativeApiModelAdmission::production().unwrap());
    let application = StartedNativePostgresApi {
        started,
        listeners,
        public_handler: NativePostgresPublicHandler::with_time_values(
            migration,
            model_admission.service_time_values().clone(),
        ),
        models: model_admission,
    };

    let runtime = Builder::new_current_thread().enable_all().build().unwrap();
    let local = LocalSet::new();
    let (result, response) = local.block_on(&runtime, async move {
        let (stop, stopped) = oneshot::channel();
        // publish shutdown only after the finite request has received its complete wire response
        let client = std::thread::spawn(move || {
            let response = exchange(
                public_address,
                b"GET /api/v1/does-not-exist HTTP/1.1\r\nHost: api\r\nConnection: close\r\n\r\n",
            );
            let _ = stop.send(());
            response
        });
        let result = serve_started_native_postgres_api(
            application,
            Duration::from_secs(1),
            Duration::from_secs(1),
            async {
                let _ = stopped.await;
            },
        )
        .await;
        (result, client.join().expect("join loopback client"))
    });
    let report = result.expect("serve and drain native API");
    assert_eq!(report.public.accepted_connections, 1);
    assert_eq!(report.public.clean_connections, 1);
    assert_eq!(report.public.protocol_errors, 0);
    assert!(report.maintenance.is_none());

    let separator = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .expect("complete HTTP response head");
    let (head, body) = response.split_at(separator + 4);
    assert!(head.starts_with(b"HTTP/1.1 404 Not Found\r\n"));
    assert_eq!(body, EXPECTED_NOT_FOUND);
    assert!(TcpStream::connect_timeout(&public_address, Duration::from_millis(250)).is_err());
    // the unknown route completes before repository checkout, so the sentinel sees no PostgreSQL peer
    match database_sentinel.accept() {
        Err(error) if error.kind() == ErrorKind::WouldBlock => {}
        Ok(_) => panic!("lazy PostgreSQL pool unexpectedly opened a connection"),
        Err(error) => panic!("database sentinel failed: {error}"),
    }
}
