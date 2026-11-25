//! Explicit process environment bridge for the retained edge startup projection.

use std::env::VarError;
use std::fmt::{Display, Formatter};
use std::path::PathBuf;

use crate::EdgeStartupEnvironment;

/// One whitelisted, non-secret startup setting read from the process environment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EdgeProcessEnvironmentSetting {
    Port,
    AdjustmentMaintenanceInternalPort,
    Release,
    WebRevision,
    NodeEnvironment,
    AdjustmentScheduledCaptureEnabled,
    AdjustmentMaintenanceMigrationReady,
    AdjustmentArchiveTransferEnabled,
    AdjustmentEvidenceRoot,
    ApiOrigin,
    AdjustmentMaintenanceApiOrigin,
    XweatherMapOrigin,
    XweatherUsagePath,
    SiteConfigurationPath,
    AdminAuthPath,
    AdminBootstrapTokenPath,
    XweatherClientIdFile,
    XweatherClientSecretFile,
    PropertySensorLayoutPath,
}

impl EdgeProcessEnvironmentSetting {
    /// Return the exact deployed environment key without accepting caller-selected names.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Port => "PORT",
            Self::AdjustmentMaintenanceInternalPort => {
                "WEATHER_ADJUSTMENT_MAINTENANCE_INTERNAL_PORT"
            }
            Self::Release => "WEATHER_RELEASE",
            Self::WebRevision => "WEATHER_WEB_REVISION",
            Self::NodeEnvironment => "NODE_ENV",
            Self::AdjustmentScheduledCaptureEnabled => {
                "WEATHER_ADJUSTMENT_SCHEDULED_CAPTURE_ENABLED"
            }
            Self::AdjustmentMaintenanceMigrationReady => {
                "WEATHER_ADJUSTMENT_MAINTENANCE_MIGRATION_READY"
            }
            Self::AdjustmentArchiveTransferEnabled => "WEATHER_ADJUSTMENT_ARCHIVE_TRANSFER_ENABLED",
            Self::AdjustmentEvidenceRoot => "WEATHER_ADJUSTMENT_EVIDENCE_ROOT",
            Self::ApiOrigin => "WEATHER_API_ORIGIN",
            Self::AdjustmentMaintenanceApiOrigin => "WEATHER_ADJUSTMENT_MAINTENANCE_API_ORIGIN",
            Self::XweatherMapOrigin => "WEATHER_XWEATHER_MAP_ORIGIN",
            Self::XweatherUsagePath => "WEATHER_XWEATHER_USAGE_PATH",
            Self::SiteConfigurationPath => "WEATHER_SITE_CONFIG_PATH",
            Self::AdminAuthPath => "WEATHER_ADMIN_AUTH_PATH",
            Self::AdminBootstrapTokenPath => "WEATHER_ADMIN_BOOTSTRAP_TOKEN_PATH",
            Self::XweatherClientIdFile => "WEATHER_XWEATHER_CLIENT_ID_FILE",
            Self::XweatherClientSecretFile => "WEATHER_XWEATHER_CLIENT_SECRET_FILE",
            Self::PropertySensorLayoutPath => "WEATHER_PROPERTY_SENSOR_LAYOUT_PATH",
        }
    }
}

/// Fixed process inputs used by the edge startup bridge.
pub trait EdgeProcessSettingsReader {
    type Error;

    /// Read the process working directory without resolving any environment-selected path.
    fn current_working_directory(&mut self) -> Result<PathBuf, Self::Error>;

    /// Read only one enumerated setting while preserving unset separately from an empty string.
    fn read_optional(
        &mut self,
        setting: EdgeProcessEnvironmentSetting,
    ) -> Result<Option<String>, Self::Error>;
}

/// Owned values retained after the short-lived process environment read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnedEdgeStartupEnvironment {
    pub current_working_directory: PathBuf,
    pub port: Option<String>,
    pub adjustment_maintenance_internal_port: Option<String>,
    pub release: Option<String>,
    pub web_revision: Option<String>,
    pub node_environment: Option<String>,
    pub adjustment_scheduled_capture_enabled: Option<String>,
    pub adjustment_maintenance_migration_ready: Option<String>,
    pub adjustment_archive_transfer_enabled: Option<String>,
    pub adjustment_evidence_root: Option<String>,
    pub api_origin: Option<String>,
    pub adjustment_maintenance_api_origin: Option<String>,
    pub xweather_map_origin: Option<String>,
    pub xweather_usage_path: Option<String>,
    pub site_configuration_path: Option<String>,
    pub admin_auth_path: Option<String>,
    pub admin_bootstrap_token_path: Option<String>,
    pub xweather_client_id_file: Option<String>,
    pub xweather_client_secret_file: Option<String>,
    pub property_sensor_layout_path: Option<String>,
}

impl OwnedEdgeStartupEnvironment {
    /// Read the complete fixed startup whitelist exactly once through an injected owner.
    pub fn read<Reader: EdgeProcessSettingsReader>(
        reader: &mut Reader,
    ) -> Result<Self, Reader::Error> {
        Ok(Self {
            current_working_directory: reader.current_working_directory()?,
            port: reader.read_optional(EdgeProcessEnvironmentSetting::Port)?,
            adjustment_maintenance_internal_port: reader
                .read_optional(EdgeProcessEnvironmentSetting::AdjustmentMaintenanceInternalPort)?,
            release: reader.read_optional(EdgeProcessEnvironmentSetting::Release)?,
            web_revision: reader.read_optional(EdgeProcessEnvironmentSetting::WebRevision)?,
            node_environment: reader
                .read_optional(EdgeProcessEnvironmentSetting::NodeEnvironment)?,
            adjustment_scheduled_capture_enabled: reader
                .read_optional(EdgeProcessEnvironmentSetting::AdjustmentScheduledCaptureEnabled)?,
            adjustment_maintenance_migration_ready: reader.read_optional(
                EdgeProcessEnvironmentSetting::AdjustmentMaintenanceMigrationReady,
            )?,
            adjustment_archive_transfer_enabled: reader
                .read_optional(EdgeProcessEnvironmentSetting::AdjustmentArchiveTransferEnabled)?,
            adjustment_evidence_root: reader
                .read_optional(EdgeProcessEnvironmentSetting::AdjustmentEvidenceRoot)?,
            api_origin: reader.read_optional(EdgeProcessEnvironmentSetting::ApiOrigin)?,
            adjustment_maintenance_api_origin: reader
                .read_optional(EdgeProcessEnvironmentSetting::AdjustmentMaintenanceApiOrigin)?,
            xweather_map_origin: reader
                .read_optional(EdgeProcessEnvironmentSetting::XweatherMapOrigin)?,
            xweather_usage_path: reader
                .read_optional(EdgeProcessEnvironmentSetting::XweatherUsagePath)?,
            site_configuration_path: reader
                .read_optional(EdgeProcessEnvironmentSetting::SiteConfigurationPath)?,
            admin_auth_path: reader.read_optional(EdgeProcessEnvironmentSetting::AdminAuthPath)?,
            admin_bootstrap_token_path: reader
                .read_optional(EdgeProcessEnvironmentSetting::AdminBootstrapTokenPath)?,
            xweather_client_id_file: reader
                .read_optional(EdgeProcessEnvironmentSetting::XweatherClientIdFile)?,
            xweather_client_secret_file: reader
                .read_optional(EdgeProcessEnvironmentSetting::XweatherClientSecretFile)?,
            property_sensor_layout_path: reader
                .read_optional(EdgeProcessEnvironmentSetting::PropertySensorLayoutPath)?,
        })
    }

    /// Borrow every owned value as the existing startup projection input.
    pub fn as_startup_environment(&self) -> EdgeStartupEnvironment<'_> {
        EdgeStartupEnvironment {
            current_working_directory: &self.current_working_directory,
            port: self.port.as_deref(),
            adjustment_maintenance_internal_port: self
                .adjustment_maintenance_internal_port
                .as_deref(),
            release: self.release.as_deref(),
            web_revision: self.web_revision.as_deref(),
            node_environment: self.node_environment.as_deref(),
            adjustment_scheduled_capture_enabled: self
                .adjustment_scheduled_capture_enabled
                .as_deref(),
            adjustment_maintenance_migration_ready: self
                .adjustment_maintenance_migration_ready
                .as_deref(),
            adjustment_archive_transfer_enabled: self
                .adjustment_archive_transfer_enabled
                .as_deref(),
            adjustment_evidence_root: self.adjustment_evidence_root.as_deref(),
            api_origin: self.api_origin.as_deref(),
            adjustment_maintenance_api_origin: self.adjustment_maintenance_api_origin.as_deref(),
            xweather_map_origin: self.xweather_map_origin.as_deref(),
            xweather_usage_path: self.xweather_usage_path.as_deref(),
            site_configuration_path: self.site_configuration_path.as_deref(),
            admin_auth_path: self.admin_auth_path.as_deref(),
            admin_bootstrap_token_path: self.admin_bootstrap_token_path.as_deref(),
            xweather_client_id_file: self.xweather_client_id_file.as_deref(),
            xweather_client_secret_file: self.xweather_client_secret_file.as_deref(),
            property_sensor_layout_path: self.property_sensor_layout_path.as_deref(),
        }
    }
}

/// Standard runtime owner for the fixed process working directory and environment whitelist.
#[derive(Debug, Default, Clone, Copy)]
pub struct StdEdgeProcessSettingsReader;

/// Standard process setting failures without leaking environment values.
#[derive(Debug)]
pub enum StdEdgeProcessSettingsError {
    CurrentWorkingDirectory(std::io::Error),
    NonUnicode(EdgeProcessEnvironmentSetting),
}

impl Display for StdEdgeProcessSettingsError {
    /// Render only the setting identity and never its environment value.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CurrentWorkingDirectory(_) => {
                formatter.write_str("edge process working directory is unavailable")
            }
            Self::NonUnicode(setting) => write!(
                formatter,
                "edge process setting {} is not Unicode",
                setting.name()
            ),
        }
    }
}

impl std::error::Error for StdEdgeProcessSettingsError {
    /// Retain only the ordinary working-directory operating-system error.
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::CurrentWorkingDirectory(error) => Some(error),
            Self::NonUnicode(_) => None,
        }
    }
}

impl EdgeProcessSettingsReader for StdEdgeProcessSettingsReader {
    type Error = StdEdgeProcessSettingsError;

    /// Read the process working directory through the standard library owner.
    fn current_working_directory(&mut self) -> Result<PathBuf, Self::Error> {
        std::env::current_dir().map_err(StdEdgeProcessSettingsError::CurrentWorkingDirectory)
    }

    /// Read only the selected whitelisted value and preserve a present empty string.
    fn read_optional(
        &mut self,
        setting: EdgeProcessEnvironmentSetting,
    ) -> Result<Option<String>, Self::Error> {
        match std::env::var(setting.name()) {
            Ok(value) => Ok(Some(value)),
            Err(VarError::NotPresent) => Ok(None),
            Err(VarError::NotUnicode(_)) => Err(StdEdgeProcessSettingsError::NonUnicode(setting)),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::convert::Infallible;
    use std::path::Path;

    use super::*;

    const SETTINGS: [EdgeProcessEnvironmentSetting; 19] = [
        EdgeProcessEnvironmentSetting::Port,
        EdgeProcessEnvironmentSetting::AdjustmentMaintenanceInternalPort,
        EdgeProcessEnvironmentSetting::Release,
        EdgeProcessEnvironmentSetting::WebRevision,
        EdgeProcessEnvironmentSetting::NodeEnvironment,
        EdgeProcessEnvironmentSetting::AdjustmentScheduledCaptureEnabled,
        EdgeProcessEnvironmentSetting::AdjustmentMaintenanceMigrationReady,
        EdgeProcessEnvironmentSetting::AdjustmentArchiveTransferEnabled,
        EdgeProcessEnvironmentSetting::AdjustmentEvidenceRoot,
        EdgeProcessEnvironmentSetting::ApiOrigin,
        EdgeProcessEnvironmentSetting::AdjustmentMaintenanceApiOrigin,
        EdgeProcessEnvironmentSetting::XweatherMapOrigin,
        EdgeProcessEnvironmentSetting::XweatherUsagePath,
        EdgeProcessEnvironmentSetting::SiteConfigurationPath,
        EdgeProcessEnvironmentSetting::AdminAuthPath,
        EdgeProcessEnvironmentSetting::AdminBootstrapTokenPath,
        EdgeProcessEnvironmentSetting::XweatherClientIdFile,
        EdgeProcessEnvironmentSetting::XweatherClientSecretFile,
        EdgeProcessEnvironmentSetting::PropertySensorLayoutPath,
    ];

    #[derive(Debug)]
    struct FakeReader {
        working_directory: PathBuf,
        values: HashMap<EdgeProcessEnvironmentSetting, Option<String>>,
        reads: Vec<EdgeProcessEnvironmentSetting>,
    }

    impl FakeReader {
        /// Construct one isolated settings owner without consulting the actual process.
        fn new() -> Self {
            Self {
                working_directory: PathBuf::from("/srv/weather"),
                values: HashMap::new(),
                reads: Vec::new(),
            }
        }
    }

    impl EdgeProcessSettingsReader for FakeReader {
        type Error = Infallible;

        /// Return the fixture-owned working directory.
        fn current_working_directory(&mut self) -> Result<PathBuf, Self::Error> {
            Ok(self.working_directory.clone())
        }

        /// Record each typed whitelist read and retain unset independently.
        fn read_optional(
            &mut self,
            setting: EdgeProcessEnvironmentSetting,
        ) -> Result<Option<String>, Self::Error> {
            self.reads.push(setting);
            Ok(self.values.get(&setting).cloned().flatten())
        }
    }

    /// Prove every owned field maps from exactly one fixed whitelisted setting.
    #[test]
    fn reads_and_borrows_the_complete_startup_whitelist() {
        let mut reader = FakeReader::new();
        // Give each setting its own exact key name so swapped mappings are visible.
        for setting in SETTINGS {
            reader
                .values
                .insert(setting, Some(setting.name().to_owned()));
        }

        let owned = OwnedEdgeStartupEnvironment::read(&mut reader).expect("owned environment");
        let borrowed = owned.as_startup_environment();

        assert_eq!(
            borrowed.current_working_directory,
            Path::new("/srv/weather")
        );
        assert_eq!(borrowed.port, Some("PORT"));
        assert_eq!(
            borrowed.adjustment_maintenance_internal_port,
            Some("WEATHER_ADJUSTMENT_MAINTENANCE_INTERNAL_PORT")
        );
        assert_eq!(borrowed.release, Some("WEATHER_RELEASE"));
        assert_eq!(borrowed.web_revision, Some("WEATHER_WEB_REVISION"));
        assert_eq!(borrowed.node_environment, Some("NODE_ENV"));
        assert_eq!(
            borrowed.adjustment_scheduled_capture_enabled,
            Some("WEATHER_ADJUSTMENT_SCHEDULED_CAPTURE_ENABLED")
        );
        assert_eq!(
            borrowed.adjustment_maintenance_migration_ready,
            Some("WEATHER_ADJUSTMENT_MAINTENANCE_MIGRATION_READY")
        );
        assert_eq!(
            borrowed.adjustment_archive_transfer_enabled,
            Some("WEATHER_ADJUSTMENT_ARCHIVE_TRANSFER_ENABLED")
        );
        assert_eq!(
            borrowed.adjustment_evidence_root,
            Some("WEATHER_ADJUSTMENT_EVIDENCE_ROOT")
        );
        assert_eq!(borrowed.api_origin, Some("WEATHER_API_ORIGIN"));
        assert_eq!(
            borrowed.adjustment_maintenance_api_origin,
            Some("WEATHER_ADJUSTMENT_MAINTENANCE_API_ORIGIN")
        );
        assert_eq!(
            borrowed.xweather_map_origin,
            Some("WEATHER_XWEATHER_MAP_ORIGIN")
        );
        assert_eq!(
            borrowed.xweather_usage_path,
            Some("WEATHER_XWEATHER_USAGE_PATH")
        );
        assert_eq!(
            borrowed.site_configuration_path,
            Some("WEATHER_SITE_CONFIG_PATH")
        );
        assert_eq!(borrowed.admin_auth_path, Some("WEATHER_ADMIN_AUTH_PATH"));
        assert_eq!(
            borrowed.admin_bootstrap_token_path,
            Some("WEATHER_ADMIN_BOOTSTRAP_TOKEN_PATH")
        );
        assert_eq!(
            borrowed.xweather_client_id_file,
            Some("WEATHER_XWEATHER_CLIENT_ID_FILE")
        );
        assert_eq!(
            borrowed.xweather_client_secret_file,
            Some("WEATHER_XWEATHER_CLIENT_SECRET_FILE")
        );
        assert_eq!(
            borrowed.property_sensor_layout_path,
            Some("WEATHER_PROPERTY_SENSOR_LAYOUT_PATH")
        );
        assert_eq!(reader.reads, SETTINGS);
    }

    /// Prove Node nullish semantics keep a configured empty string distinct from absence.
    #[test]
    fn preserves_present_empty_strings_and_unset_values() {
        let mut reader = FakeReader::new();
        reader
            .values
            .insert(EdgeProcessEnvironmentSetting::Port, Some(String::new()));
        reader.values.insert(
            EdgeProcessEnvironmentSetting::ApiOrigin,
            Some(String::new()),
        );

        let owned = OwnedEdgeStartupEnvironment::read(&mut reader).expect("owned environment");
        let borrowed = owned.as_startup_environment();

        assert_eq!(borrowed.port, Some(""));
        assert_eq!(borrowed.api_origin, Some(""));
        assert_eq!(borrowed.release, None);
        assert_eq!(borrowed.adjustment_maintenance_api_origin, None);
        assert_eq!(borrowed.xweather_client_secret_file, None);
    }
}
