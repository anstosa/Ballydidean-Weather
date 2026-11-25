//! Typed projection of the retained Node edge process startup configuration.

#[cfg(test)]
use std::convert::Infallible;
use std::path::{Path, PathBuf};

use crate::{
    AdmittedPublicApiOrigin, EdgeXweatherCredentialByteReader, EdgeXweatherCredentialPaths,
    EdgeXweatherCredentials, EdgeXweatherCredentialsError, PublicTemplateConfigurationError,
    PublicTemplateSettings, load_edge_xweather_credentials,
};

const DEFAULT_ADMIN_AUTH_PATH: &str = "/var/lib/weather/xweather/admin-auth.json";
const DEFAULT_ADMIN_BOOTSTRAP_TOKEN_PATH: &str = "/run/secrets/weather_xweather_client_secret";
const DEFAULT_API_ORIGIN: &str = "http://api:3001";
const DEFAULT_EVIDENCE_ROOT: &str = "/var/lib/weather/xweather/adjustment-evidence";
const DEFAULT_LAYOUT_PATH: &str = "/var/lib/weather/xweather/property-sensor-layout.json";
const DEFAULT_PORT: &str = "3000";
const DEFAULT_XWEATHER_MAP_ORIGIN: &str = "https://maps.api.xweather.com";
const DEVELOPMENT: &str = "development";

/// Environment values admitted by the process owner without reading the host in this module.
#[derive(Debug, Clone, Copy)]
pub struct EdgeStartupEnvironment<'a> {
    pub current_working_directory: &'a Path,
    pub port: Option<&'a str>,
    pub adjustment_maintenance_internal_port: Option<&'a str>,
    pub release: Option<&'a str>,
    pub web_revision: Option<&'a str>,
    pub node_environment: Option<&'a str>,
    pub adjustment_scheduled_capture_enabled: Option<&'a str>,
    pub adjustment_maintenance_migration_ready: Option<&'a str>,
    pub adjustment_archive_transfer_enabled: Option<&'a str>,
    pub adjustment_evidence_root: Option<&'a str>,
    pub api_origin: Option<&'a str>,
    pub adjustment_maintenance_api_origin: Option<&'a str>,
    pub xweather_map_origin: Option<&'a str>,
    pub xweather_usage_path: Option<&'a str>,
    pub site_configuration_path: Option<&'a str>,
    pub admin_auth_path: Option<&'a str>,
    pub admin_bootstrap_token_path: Option<&'a str>,
    pub xweather_client_id_file: Option<&'a str>,
    pub xweather_client_secret_file: Option<&'a str>,
    pub property_sensor_layout_path: Option<&'a str>,
}

impl<'a> EdgeStartupEnvironment<'a> {
    /// Construct the absent-environment case around one process-owned working directory.
    pub const fn new(current_working_directory: &'a Path) -> Self {
        Self {
            current_working_directory,
            port: None,
            adjustment_maintenance_internal_port: None,
            release: None,
            web_revision: None,
            node_environment: None,
            adjustment_scheduled_capture_enabled: None,
            adjustment_maintenance_migration_ready: None,
            adjustment_archive_transfer_enabled: None,
            adjustment_evidence_root: None,
            api_origin: None,
            adjustment_maintenance_api_origin: None,
            xweather_map_origin: None,
            xweather_usage_path: None,
            site_configuration_path: None,
            admin_auth_path: None,
            admin_bootstrap_token_path: None,
            xweather_client_id_file: None,
            xweather_client_secret_file: None,
            property_sensor_layout_path: None,
        }
    }
}

/// URL, port, and lexical path operations retained by the process integration boundary.
pub trait EdgeStartupAdmission {
    type Error;
    type XweatherMapOrigin;

    /// Resolve the process working directory with the runtime's existing path owner.
    fn resolve_working_root(&mut self, value: &Path) -> Result<PathBuf, Self::Error>;

    /// Resolve one configured evidence path against the admitted process root.
    fn resolve_evidence_root(
        &mut self,
        working_root: &Path,
        value: &str,
    ) -> Result<PathBuf, Self::Error>;

    /// Parse one listener port with the predecessor runtime's numeric semantics.
    fn parse_port(&mut self, value: &str) -> Result<u16, Self::Error>;

    /// Admit the ordinary credential-free core API origin.
    fn admit_api_origin(&mut self, value: &str) -> Result<AdmittedPublicApiOrigin, Self::Error>;

    /// Admit the optional compose-private maintenance origin.
    fn admit_maintenance_api_origin(
        &mut self,
        value: &str,
    ) -> Result<AdmittedPublicApiOrigin, Self::Error>;

    /// Admit the HTTPS-or-loopback Xweather map origin.
    fn admit_xweather_map_origin(
        &mut self,
        value: &str,
    ) -> Result<Self::XweatherMapOrigin, Self::Error>;
}

/// Fully admitted values used to construct the public and optional private edge listeners.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EdgeStartupConfiguration<XweatherMapOrigin> {
    pub working_root: PathBuf,
    pub public_root: PathBuf,
    pub compiled_root: PathBuf,
    pub admin_login_path: PathBuf,
    pub port: u16,
    pub adjustment_maintenance_internal_port: Option<u16>,
    pub release: String,
    pub web_revision: String,
    pub production_analytics: bool,
    pub scheduled_capture_enabled: bool,
    pub scheduled_capture_migration_ready: bool,
    pub scheduled_archive_transfer_enabled: bool,
    pub adjustment_evidence_root: PathBuf,
    pub api_origin: AdmittedPublicApiOrigin,
    pub adjustment_maintenance_api_origin: Option<AdmittedPublicApiOrigin>,
    pub xweather_map_origin: XweatherMapOrigin,
    pub xweather_usage_path: Option<PathBuf>,
    pub site_configuration_path: PathBuf,
    pub admin_auth_path: PathBuf,
    pub admin_bootstrap_token_path: PathBuf,
    pub property_sensor_layout_path: PathBuf,
}

/// Startup failures retain the exact owning input boundary for diagnostics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EdgeStartupConfigurationError<Error, CredentialError = std::convert::Infallible> {
    WorkingRoot(Error),
    EvidenceRoot(Error),
    Port(Error),
    AdjustmentMaintenanceInternalPort(Error),
    ApiOrigin(Error),
    AdjustmentMaintenanceApiOrigin(Error),
    XweatherMapOrigin(Error),
    XweatherCredentials(CredentialError),
    PublicTemplate(PublicTemplateConfigurationError),
}

/// Isolate the pure projection for unit fixtures of environment precedence.
#[cfg(test)]
fn project_edge_startup_configuration<Admission: EdgeStartupAdmission>(
    environment: EdgeStartupEnvironment<'_>,
    admission: &mut Admission,
) -> Result<
    EdgeStartupConfiguration<Admission::XweatherMapOrigin>,
    EdgeStartupConfigurationError<Admission::Error, Infallible>,
> {
    project_edge_startup_with(environment, admission, |_| Ok(())).map(
        // discard the inert credential projection for no-host configuration consumers
        |(configuration, ())| configuration,
    )
}

/// One production startup projection with privately retained map credentials.
pub type EdgeStartupWithCredentials<Admission, Reader> = Result<
    (
        EdgeStartupConfiguration<<Admission as EdgeStartupAdmission>::XweatherMapOrigin>,
        Option<EdgeXweatherCredentials>,
    ),
    EdgeStartupConfigurationError<
        <Admission as EdgeStartupAdmission>::Error,
        EdgeXweatherCredentialsError<<Reader as EdgeXweatherCredentialByteReader>::Error>,
    >,
>;

/// Admit map origin, read its configured pair, and only then parse listener settings.
pub fn project_edge_startup_configuration_with_credentials<
    Admission: EdgeStartupAdmission,
    Reader: EdgeXweatherCredentialByteReader,
>(
    environment: EdgeStartupEnvironment<'_>,
    admission: &mut Admission,
    reader: &mut Reader,
) -> EdgeStartupWithCredentials<Admission, Reader> {
    project_edge_startup_with(environment, admission, |paths| {
        load_edge_xweather_credentials(paths, reader)
    })
}

type EdgeStartupProjection<Admission, Credentials, CredentialError> = Result<
    (
        EdgeStartupConfiguration<<Admission as EdgeStartupAdmission>::XweatherMapOrigin>,
        Credentials,
    ),
    EdgeStartupConfigurationError<<Admission as EdgeStartupAdmission>::Error, CredentialError>,
>;

/// Keep the predecessor's startup ordering with or without the file owner attached.
fn project_edge_startup_with<Admission: EdgeStartupAdmission, Credentials, CredentialError>(
    environment: EdgeStartupEnvironment<'_>,
    admission: &mut Admission,
    load_credentials: impl FnOnce(
        EdgeXweatherCredentialPaths<'_>,
    ) -> Result<Credentials, CredentialError>,
) -> EdgeStartupProjection<Admission, Credentials, CredentialError> {
    let working_root = admission
        .resolve_working_root(environment.current_working_directory)
        .map_err(EdgeStartupConfigurationError::WorkingRoot)?;
    let evidence_value = environment
        .adjustment_evidence_root
        .unwrap_or(DEFAULT_EVIDENCE_ROOT);
    let adjustment_evidence_root = admission
        .resolve_evidence_root(&working_root, evidence_value)
        .map_err(EdgeStartupConfigurationError::EvidenceRoot)?;
    let api_origin = admission
        .admit_api_origin(environment.api_origin.unwrap_or(DEFAULT_API_ORIGIN))
        .map_err(EdgeStartupConfigurationError::ApiOrigin)?;
    let adjustment_maintenance_api_origin = environment
        .adjustment_maintenance_api_origin
        .map(|value| {
            admission
                .admit_maintenance_api_origin(value)
                .map_err(EdgeStartupConfigurationError::AdjustmentMaintenanceApiOrigin)
        })
        .transpose()?;
    let xweather_map_origin = admission
        .admit_xweather_map_origin(
            environment
                .xweather_map_origin
                .unwrap_or(DEFAULT_XWEATHER_MAP_ORIGIN),
        )
        .map_err(EdgeStartupConfigurationError::XweatherMapOrigin)?;
    let credentials = load_credentials(EdgeXweatherCredentialPaths {
        client_id_file: environment.xweather_client_id_file,
        client_secret_file: environment.xweather_client_secret_file,
    })
    .map_err(EdgeStartupConfigurationError::XweatherCredentials)?;
    let port = admission
        .parse_port(environment.port.unwrap_or(DEFAULT_PORT))
        .map_err(EdgeStartupConfigurationError::Port)?;
    let adjustment_maintenance_internal_port = environment
        .adjustment_maintenance_internal_port
        .map(|value| {
            admission
                .parse_port(value)
                .map_err(EdgeStartupConfigurationError::AdjustmentMaintenanceInternalPort)
        })
        .transpose()?;
    let template_settings = PublicTemplateSettings::from_runtime_values(
        environment.release,
        environment.web_revision,
        environment.node_environment,
        None,
    )
    .map_err(EdgeStartupConfigurationError::PublicTemplate)?;
    let release = template_settings.release().to_owned();
    let web_revision = environment.web_revision.unwrap_or(DEVELOPMENT).to_owned();
    let production_analytics =
        environment.node_environment == Some("production") && release != DEVELOPMENT;
    let public_root = working_root.join("apps/web/public");
    let compiled_root = working_root.join("apps/web/dist");
    let admin_login_path = public_root.join("admin-login.html");
    let site_configuration_path = environment.site_configuration_path.map_or_else(
        || working_root.join("config/sites/ballydidean.json"),
        PathBuf::from,
    );
    let admin_auth_path = environment
        .admin_auth_path
        .map_or_else(|| PathBuf::from(DEFAULT_ADMIN_AUTH_PATH), PathBuf::from);
    let admin_bootstrap_token_path = PathBuf::from(
        environment
            .admin_bootstrap_token_path
            .or(environment.xweather_client_secret_file)
            .unwrap_or(DEFAULT_ADMIN_BOOTSTRAP_TOKEN_PATH),
    );
    let property_sensor_layout_path = environment
        .property_sensor_layout_path
        .map_or_else(|| PathBuf::from(DEFAULT_LAYOUT_PATH), PathBuf::from);

    Ok((
        EdgeStartupConfiguration {
            working_root,
            public_root,
            compiled_root,
            admin_login_path,
            port,
            adjustment_maintenance_internal_port,
            release,
            web_revision,
            production_analytics,
            scheduled_capture_enabled: environment.adjustment_scheduled_capture_enabled
                == Some("true"),
            scheduled_capture_migration_ready: environment.adjustment_maintenance_migration_ready
                == Some("true"),
            scheduled_archive_transfer_enabled: environment.adjustment_archive_transfer_enabled
                == Some("true"),
            adjustment_evidence_root,
            api_origin,
            adjustment_maintenance_api_origin,
            xweather_map_origin,
            xweather_usage_path: environment.xweather_usage_path.map(PathBuf::from),
            site_configuration_path,
            admin_auth_path,
            admin_bootstrap_token_path,
            property_sensor_layout_path,
        },
        credentials,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum FakeAdmissionError {
        InvalidPort,
    }

    #[derive(Debug, Default)]
    struct FakeAdmission {
        api_origins: Vec<String>,
        maintenance_origins: Vec<String>,
        map_origins: Vec<String>,
        ports: Vec<String>,
        evidence_values: Vec<String>,
    }

    impl EdgeStartupAdmission for FakeAdmission {
        type Error = FakeAdmissionError;
        type XweatherMapOrigin = String;

        /// Keep the fixture's already-absolute process root.
        fn resolve_working_root(&mut self, value: &Path) -> Result<PathBuf, Self::Error> {
            Ok(value.to_path_buf())
        }

        /// Resolve relative evidence roots exactly once beneath the fixture process root.
        fn resolve_evidence_root(
            &mut self,
            working_root: &Path,
            value: &str,
        ) -> Result<PathBuf, Self::Error> {
            self.evidence_values.push(value.to_owned());
            let value = Path::new(value);
            // preserve absolute defaults while resolving configured relative paths.
            Ok(if value.is_absolute() {
                value.to_path_buf()
            } else {
                working_root.join(value)
            })
        }

        /// Admit the fixture's decimal listener ports.
        fn parse_port(&mut self, value: &str) -> Result<u16, Self::Error> {
            self.ports.push(value.to_owned());
            value
                .parse::<u16>()
                .map_err(|_| FakeAdmissionError::InvalidPort)
                .and_then(|port| {
                    // retain the deployed nonzero listener range.
                    (port > 0)
                        .then_some(port)
                        .ok_or(FakeAdmissionError::InvalidPort)
                })
        }

        /// Record the exact core origin sent to the approved URL owner.
        fn admit_api_origin(
            &mut self,
            value: &str,
        ) -> Result<AdmittedPublicApiOrigin, Self::Error> {
            self.api_origins.push(value.to_owned());
            Ok(AdmittedPublicApiOrigin::admitted(value))
        }

        /// Record the exact optional maintenance origin sent to its stricter owner.
        fn admit_maintenance_api_origin(
            &mut self,
            value: &str,
        ) -> Result<AdmittedPublicApiOrigin, Self::Error> {
            self.maintenance_origins.push(value.to_owned());
            Ok(AdmittedPublicApiOrigin::admitted(value))
        }

        /// Record the map origin without implementing URL rules in the fixture.
        fn admit_xweather_map_origin(
            &mut self,
            value: &str,
        ) -> Result<Self::XweatherMapOrigin, Self::Error> {
            self.map_origins.push(value.to_owned());
            Ok(value.to_owned())
        }
    }

    /// Prove every absent environment value selects the retained Node default.
    #[test]
    fn projects_default_startup_configuration() {
        let mut admission = FakeAdmission::default();
        let configuration = project_edge_startup_configuration(
            EdgeStartupEnvironment::new(Path::new("/srv/weather")),
            &mut admission,
        )
        .expect("default configuration");

        assert_eq!(configuration.working_root, Path::new("/srv/weather"));
        assert_eq!(
            configuration.public_root,
            Path::new("/srv/weather/apps/web/public")
        );
        assert_eq!(
            configuration.compiled_root,
            Path::new("/srv/weather/apps/web/dist")
        );
        assert_eq!(
            configuration.admin_login_path,
            Path::new("/srv/weather/apps/web/public/admin-login.html")
        );
        assert_eq!(configuration.port, 3000);
        assert_eq!(configuration.adjustment_maintenance_internal_port, None);
        assert_eq!(configuration.release, DEVELOPMENT);
        assert_eq!(configuration.web_revision, DEVELOPMENT);
        assert!(!configuration.production_analytics);
        assert!(!configuration.scheduled_capture_enabled);
        assert!(!configuration.scheduled_capture_migration_ready);
        assert!(!configuration.scheduled_archive_transfer_enabled);
        assert_eq!(
            configuration.adjustment_evidence_root,
            Path::new(DEFAULT_EVIDENCE_ROOT)
        );
        assert_eq!(configuration.api_origin.as_str(), DEFAULT_API_ORIGIN);
        assert_eq!(configuration.adjustment_maintenance_api_origin, None);
        assert_eq!(
            configuration.xweather_map_origin,
            DEFAULT_XWEATHER_MAP_ORIGIN
        );
        assert_eq!(configuration.xweather_usage_path, None);
        assert_eq!(
            configuration.site_configuration_path,
            Path::new("/srv/weather/config/sites/ballydidean.json")
        );
        assert_eq!(
            configuration.admin_auth_path,
            Path::new(DEFAULT_ADMIN_AUTH_PATH)
        );
        assert_eq!(
            configuration.admin_bootstrap_token_path,
            Path::new(DEFAULT_ADMIN_BOOTSTRAP_TOKEN_PATH)
        );
        assert_eq!(
            configuration.property_sensor_layout_path,
            Path::new(DEFAULT_LAYOUT_PATH)
        );
        assert_eq!(admission.ports, [DEFAULT_PORT]);
        assert_eq!(admission.api_origins, [DEFAULT_API_ORIGIN]);
        assert_eq!(admission.map_origins, [DEFAULT_XWEATHER_MAP_ORIGIN]);
        assert_eq!(admission.evidence_values, [DEFAULT_EVIDENCE_ROOT]);
    }

    /// Prove configured values and exact true flags survive admission without fallback.
    #[test]
    fn projects_configured_startup_configuration() {
        let mut environment = EdgeStartupEnvironment::new(Path::new("/srv/weather"));
        environment.port = Some("4080");
        environment.adjustment_maintenance_internal_port = Some("4081");
        environment.release = Some("2026.11.23-2");
        environment.web_revision = Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
        environment.node_environment = Some("production");
        environment.adjustment_scheduled_capture_enabled = Some("true");
        environment.adjustment_maintenance_migration_ready = Some("true");
        environment.adjustment_archive_transfer_enabled = Some("true");
        environment.adjustment_evidence_root = Some("state/evidence");
        environment.api_origin = Some("https://api.example.test");
        environment.adjustment_maintenance_api_origin = Some("http://api:4081");
        environment.xweather_map_origin = Some("https://maps.example.test");
        environment.xweather_usage_path = Some("state/xweather-usage.json");
        environment.site_configuration_path = Some("config/custom-site.json");
        environment.admin_auth_path = Some("state/admin-auth.json");
        environment.admin_bootstrap_token_path = Some("state/bootstrap-token");
        environment.xweather_client_secret_file = Some("ignored-client-secret");
        environment.property_sensor_layout_path = Some("state/layout.json");
        let mut admission = FakeAdmission::default();

        let configuration = project_edge_startup_configuration(environment, &mut admission)
            .expect("configured startup");

        assert_eq!(configuration.port, 4080);
        assert_eq!(
            configuration.adjustment_maintenance_internal_port,
            Some(4081)
        );
        assert_eq!(configuration.release, "2026.11.23-2");
        assert_eq!(
            configuration.web_revision,
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        );
        assert!(configuration.production_analytics);
        assert!(configuration.scheduled_capture_enabled);
        assert!(configuration.scheduled_capture_migration_ready);
        assert!(configuration.scheduled_archive_transfer_enabled);
        assert_eq!(
            configuration.adjustment_evidence_root,
            Path::new("/srv/weather/state/evidence")
        );
        assert_eq!(
            configuration.api_origin.as_str(),
            "https://api.example.test"
        );
        assert_eq!(
            configuration
                .adjustment_maintenance_api_origin
                .as_ref()
                .map(AdmittedPublicApiOrigin::as_str),
            Some("http://api:4081")
        );
        assert_eq!(
            configuration.xweather_map_origin,
            "https://maps.example.test"
        );
        assert_eq!(
            configuration.xweather_usage_path.as_deref(),
            Some(Path::new("state/xweather-usage.json"))
        );
        assert_eq!(
            configuration.site_configuration_path,
            Path::new("config/custom-site.json")
        );
        assert_eq!(
            configuration.admin_auth_path,
            Path::new("state/admin-auth.json")
        );
        assert_eq!(
            configuration.admin_bootstrap_token_path,
            Path::new("state/bootstrap-token")
        );
        assert_eq!(
            configuration.property_sensor_layout_path,
            Path::new("state/layout.json")
        );
        assert_eq!(admission.ports, ["4080", "4081"]);
        assert_eq!(admission.maintenance_origins, ["http://api:4081"]);
    }

    /// Prove feature switches require lowercase true and bootstrap uses the secret fallback.
    #[test]
    fn preserves_exact_flags_and_bootstrap_fallback_precedence() {
        let mut environment = EdgeStartupEnvironment::new(Path::new("/srv/weather"));
        environment.release = Some(DEVELOPMENT);
        environment.node_environment = Some("production");
        environment.adjustment_scheduled_capture_enabled = Some("TRUE");
        environment.adjustment_maintenance_migration_ready = Some("1");
        environment.adjustment_archive_transfer_enabled = Some("false");
        environment.xweather_client_secret_file = Some("state/client-secret");
        let mut admission = FakeAdmission::default();

        let configuration = project_edge_startup_configuration(environment, &mut admission)
            .expect("exact flag configuration");

        assert!(!configuration.production_analytics);
        assert!(!configuration.scheduled_capture_enabled);
        assert!(!configuration.scheduled_capture_migration_ready);
        assert!(!configuration.scheduled_archive_transfer_enabled);
        assert_eq!(
            configuration.admin_bootstrap_token_path,
            Path::new("state/client-secret")
        );
    }
}
