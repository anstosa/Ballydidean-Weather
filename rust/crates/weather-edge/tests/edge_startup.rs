#![cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::convert::Infallible;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::rc::Rc;

use weather_edge::{
    AdmittedPublicApiOrigin, EdgeStartupArchive, EdgeStartupConfiguration,
    EdgeStartupConfigurationError, EdgeStartupEnvironment, EdgeStartupLifecycle,
    EdgeStartupLifecycleConfiguration, EdgeStartupLifecyclePorts, EdgeStartupOrigins,
    EdgeStartupPublicOwners, EdgeStartupSchedulerReadiness, EdgeXweatherCredentialByteReader,
    EdgeXweatherCredentialsError, ForecastSchedulerConfiguration, NativeEdgeStartupAdmission,
    parse_edge_listener_port, project_edge_startup_configuration_with_credentials,
};

/// Read the original startup statements' output, not a handwritten expected boot sequence.
fn source_startup_witness() -> BTreeMap<String, String> {
    let command =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/edge-startup-node-oracle.mjs");
    let output = Command::new("node")
        .arg(command)
        .arg("--tsv")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| {
            let (key, value) = line.split_once('\t').unwrap();
            (key.to_owned(), value.to_owned())
        })
        .collect()
}

struct FixtureOrigins;

/// Retain the configured paths passed to the Xweather file owner in boot order.
struct FixtureMapCredentials {
    reads: Vec<PathBuf>,
    has_secret: bool,
}

impl EdgeXweatherCredentialByteReader for FixtureMapCredentials {
    type Error = Infallible;

    /// Supply a disposable credential pair without accessing the process filesystem.
    fn read_optional(&mut self, path: &Path) -> Result<Option<Vec<u8>>, Self::Error> {
        self.reads.push(path.to_owned());
        Ok(match path.to_str() {
            Some("/fixture/map-id") => Some(b" map-id\n".to_vec()),
            Some("/fixture/map-secret") if self.has_secret => Some(b"map-secret".to_vec()),
            _ => None,
        })
    }
}

/// Keep map credentials on the same startup path, before any listener-port refusal.
#[test]
fn admits_the_configured_map_pair_before_listener_configuration() {
    let mut admission = NativeEdgeStartupAdmission::new(FixtureOrigins);
    let mut reader = FixtureMapCredentials {
        reads: Vec::new(),
        has_secret: true,
    };
    let mut environment = EdgeStartupEnvironment::new(Path::new("/fixture/weather"));
    environment.xweather_client_id_file = Some("/fixture/map-id");
    environment.xweather_client_secret_file = Some("/fixture/map-secret");
    let (configuration, credentials) = project_edge_startup_configuration_with_credentials(
        environment,
        &mut admission,
        &mut reader,
    )
    .unwrap();
    let credentials = credentials.unwrap();
    assert_eq!(credentials.client_id(), "map-id");
    assert_eq!(credentials.client_secret(), "map-secret");
    assert_eq!(configuration.port, 3000);
    assert_eq!(
        configuration.admin_bootstrap_token_path,
        Path::new("/fixture/map-secret")
    );
    assert_eq!(
        reader.reads,
        [
            Path::new("/fixture/map-id"),
            Path::new("/fixture/map-secret")
        ]
    );

    environment.port = Some("not-a-port");
    reader.has_secret = false;
    assert!(matches!(
        project_edge_startup_configuration_with_credentials(
            environment,
            &mut admission,
            &mut reader
        ),
        Err(EdgeStartupConfigurationError::XweatherCredentials(
            EdgeXweatherCredentialsError::PartialProvisioning
        ))
    ));
}

impl EdgeStartupOrigins for FixtureOrigins {
    type Error = Infallible;
    type XweatherMapOrigin = String;

    /// Bind this fixture's pre-admitted public origin without opening a socket.
    fn api_origin(&mut self, value: &str) -> Result<AdmittedPublicApiOrigin, Self::Error> {
        Ok(AdmittedPublicApiOrigin::admitted(value))
    }

    /// Bind this fixture's compose-private origin, not a real deployment address.
    fn maintenance_api_origin(
        &mut self,
        value: &str,
    ) -> Result<AdmittedPublicApiOrigin, Self::Error> {
        Ok(AdmittedPublicApiOrigin::admitted(value))
    }

    /// Return only the supplied normalized provider URL string.
    fn xweather_map_origin(&mut self, value: &str) -> Result<String, Self::Error> {
        Ok(value.to_owned())
    }
}

struct FixtureLifecycle<'a> {
    config: &'a EdgeStartupConfiguration<String>,
    report: Rc<RefCell<Vec<String>>>,
    archive_refused: bool,
    site_ready: bool,
}

impl FixtureLifecycle<'_> {
    /// Record one owned lifecycle event without inspecting unrelated host state.
    fn note(&self, event: impl Into<String>) {
        self.report.borrow_mut().push(event.into());
    }
}

impl EdgeStartupLifecyclePorts for FixtureLifecycle<'_> {
    type Site = ();
    type Admin = ();
    type Evidence = ();
    type RevisionArchive = ();
    type Archive = ();
    type Scheduler = ForecastSchedulerConfiguration;
    type Assets = ();
    type PublicListener = ();
    type PrivateListener = ();
    type Error = &'static str;

    /// Return only the disposable site metadata disposition.
    fn load_site(&mut self) -> Option<Self::Site> {
        self.note("site");
        self.site_ready.then_some(())
    }

    /// Observe admin initialization after loading the optional site.
    fn initialize_admin(&mut self, site: Option<&()>) -> Result<(), Self::Error> {
        self.note("admin");
        assert_eq!(site.is_some(), self.site_ready);
        Ok(())
    }

    /// Observe the evidence owner without reading a real spool.
    fn initialize_evidence(&mut self) {
        self.note("evidence");
    }

    /// Observe the private spool owner before optional archive initialization.
    fn initialize_revision_archive(&mut self) -> Result<(), Self::Error> {
        self.note("revision");
        Ok(())
    }

    /// Preserve serving when this optional archive store refuses initialization.
    fn initialize_archive(&mut self) -> Result<(), Self::Error> {
        self.note("archive");
        // return only the disposable requested ordinary availability result
        if self.archive_refused {
            return Err("archive unavailable");
        }
        Ok(())
    }

    /// Preserve the two separate controller activation flags.
    fn initialize_scheduler(
        &mut self,
        config: ForecastSchedulerConfiguration,
    ) -> ForecastSchedulerConfiguration {
        self.note("scheduler");
        config
    }

    /// Install the fixed asset inventory without actual serving in this test.
    fn load_assets(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }

    /// Record the public listen request without triggering the async ready callback.
    fn request_public_listener(
        &mut self,
        _: EdgeStartupPublicOwners<'_, (), (), (), ForecastSchedulerConfiguration, ()>,
    ) -> Result<(), Self::Error> {
        self.note(format!("public:listen:{}:0.0.0.0", self.config.port));
        Ok(())
    }

    /// Record only the admitted optional private listen request.
    fn request_private_listener(&mut self, _: &()) -> Result<(), Self::Error> {
        self.note(format!(
            "private:listen:{}:0.0.0.0",
            self.config.adjustment_maintenance_internal_port.unwrap()
        ));
        Ok(())
    }

    /// Start only on the actual public-listener readiness event.
    fn start_scheduler(
        &mut self,
        scheduler: &mut ForecastSchedulerConfiguration,
    ) -> Result<bool, Self::Error> {
        self.note("scheduler:start");
        Ok(scheduler.enabled && scheduler.migration_ready)
    }

    /// Close only the fixture's modeled public listener.
    fn close_public_listener(&mut self, _: &mut ()) -> Result<(), Self::Error> {
        Ok(())
    }

    /// Close only the fixture's modeled private listener.
    fn close_private_listener(&mut self, _: &mut ()) -> Result<(), Self::Error> {
        Ok(())
    }
}

/// Provide only the exact disposable inputs retained in the Node startup oracle.
fn fixture_environment(active: bool, revision: &str) -> EdgeStartupEnvironment<'_> {
    let mut env = EdgeStartupEnvironment::new(Path::new("/fixture/weather"));
    // omit every optional input in the stable development case
    if active {
        env.node_environment = Some("production");
        env.port = Some("4100");
        env.release = Some("2026.10.08-3");
        env.web_revision = Some(revision);
        env.api_origin = Some("http://api:4101");
        env.adjustment_maintenance_api_origin = Some("http://api:9800");
        env.adjustment_maintenance_internal_port = Some("9801");
        env.adjustment_evidence_root = Some("evidence");
        env.adjustment_scheduled_capture_enabled = Some("true");
        env.adjustment_maintenance_migration_ready = Some("true");
        env.adjustment_archive_transfer_enabled = Some("true");
        env.site_configuration_path = Some("/fixture/site.json");
        env.admin_auth_path = Some("/fixture/admin.json");
        env.admin_bootstrap_token_path = Some("/fixture/bootstrap-file");
        env.property_sensor_layout_path = Some("/fixture/layout.json");
        env.xweather_map_origin = Some("https://maps.api.xweather.com");
    }
    env
}

/// Match normal boot/optional archive availability and exact supported config against source.
#[test]
fn edge_startup_configuration_and_readiness_match_node() {
    let reference = source_startup_witness();
    let revision = "a".repeat(40);
    // compare startup for the default, active and contained optional-archive failure cases
    for (name, active, archive_refused) in [
        ("development", false, false),
        ("active", true, false),
        ("archiveRefused", true, true),
    ] {
        let mut credential_files = FixtureMapCredentials {
            reads: Vec::new(),
            has_secret: false,
        };
        let (config, credentials) = project_edge_startup_configuration_with_credentials(
            fixture_environment(active, &revision),
            &mut NativeEdgeStartupAdmission::new(FixtureOrigins),
            &mut credential_files,
        )
        .unwrap();
        assert!(credentials.is_none());
        let value = |key| reference[&format!("{name}.{key}")].as_str();
        assert_eq!(
            config.adjustment_evidence_root.display().to_string(),
            value("adjustmentEvidenceRoot")
        );
        assert_eq!(config.api_origin.as_str(), value("apiOrigin"));
        assert_eq!(
            config
                .adjustment_maintenance_api_origin
                .as_ref()
                .map_or("null", AdmittedPublicApiOrigin::as_str),
            value("maintenanceApiOrigin")
        );
        assert_eq!(config.xweather_map_origin, value("xweatherOrigin"));
        assert_eq!(config.release, value("release"));
        assert_eq!(config.web_revision, value("webRevision"));
        assert_eq!(config.port.to_string(), value("port"));
        assert_eq!(
            config
                .adjustment_maintenance_internal_port
                .map_or("null".to_owned(), |port| port.to_string()),
            value("adjustmentMaintenanceInternalPort")
        );
        assert_eq!(
            config.site_configuration_path.display().to_string(),
            value("sitePath")
        );
        assert_eq!(
            config.admin_auth_path.display().to_string(),
            value("adminAuthPath")
        );
        assert_eq!(
            config.admin_bootstrap_token_path.display().to_string(),
            value("adminBootstrapPath")
        );
        assert_eq!(
            config.property_sensor_layout_path.display().to_string(),
            value("layoutPath")
        );
        assert_eq!(
            config.production_analytics.to_string(),
            value("productionAnalytics")
        );
        assert_eq!(
            config.scheduled_capture_enabled.to_string(),
            value("scheduledCaptureEnabled")
        );
        assert_eq!(
            config.scheduled_capture_migration_ready.to_string(),
            value("scheduledCaptureMigrationReady")
        );
        assert_eq!(
            config.scheduled_archive_transfer_enabled.to_string(),
            value("scheduledArchiveTransferEnabled")
        );

        let trace = Rc::new(RefCell::new(Vec::new()));
        let ports = FixtureLifecycle {
            config: &config,
            report: Rc::clone(&trace),
            archive_refused,
            site_ready: name == "active",
        };
        let mut startup = EdgeStartupLifecycle::initialize(
            ports,
            EdgeStartupLifecycleConfiguration::from(&config),
        )
        .unwrap();
        assert_eq!(
            startup.scheduler_readiness(),
            EdgeStartupSchedulerReadiness::AwaitingPublicListener
        );
        assert_eq!(
            matches!(startup.archive(), EdgeStartupArchive::Ready(_)).to_string(),
            value("archiveReady")
        );
        trace.borrow_mut().push("public:ready".to_owned());
        assert_eq!(startup.public_listener_ready().unwrap(), active);
        assert_eq!(trace.borrow().join(","), value("startupOrder"), "{name}");
    }

    // retain the source Number spelling while using a native bounded TCP port
    for (name, number) in [
        ("decimal", "3001"),
        ("hex", "0x0bb9"),
        ("octal", "0o5671"),
        ("binary", "0b101110111001"),
        ("exponent", "3.001e3"),
        ("whitespace", "\u{feff}3001\u{a0}"),
    ] {
        assert_eq!(
            parse_edge_listener_port(number).unwrap().to_string(),
            reference[&format!("port.{name}")]
        );
    }
}
