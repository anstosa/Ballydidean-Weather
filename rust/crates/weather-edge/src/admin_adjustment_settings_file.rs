use std::error::Error;
use std::fmt::{Display, Formatter};
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use crate::private_file_state::{
    atomic_replace_private_file, create_parent_directory, read_optional_bytes, write_private_file,
};
use crate::{
    DISABLED_FORECAST_ADJUSTMENT_SETTINGS, ForecastAdjustmentSettings,
    ForecastAdjustmentSettingsDiagnostic, ForecastAdjustmentSettingsStatus,
    ForecastAdjustmentSettingsStore,
};

const ADJUSTMENT_SETTINGS_MARKER: &str = "forecast-adjustment-settings/v1";
const ADJUSTMENT_SETTINGS_MARKER_CONTENT: &[u8] = b"forecast-adjustment-settings/v1\n";

/// authorization evidence admitted by the existing configured-admin boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AdmittedForecastAdjustmentSettingsInitialization(());

impl AdmittedForecastAdjustmentSettingsInitialization {
    /// admit configured administrator state without handling credentials here.
    pub const fn admitted() -> Self {
        Self(())
    }
}

/// the live configured-admin evidence observed at first initialization.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForecastAdjustmentSettingsInitializationEvidence {
    Missing,
    Invalid,
    Admitted(AdmittedForecastAdjustmentSettingsInitialization),
}

/// the live non-credential evidence reader retained outside file mechanics.
pub trait ForecastAdjustmentSettingsInitializationEvidenceReader {
    /// read current initialization evidence on each first-init attempt.
    fn read_initialization_evidence(&mut self) -> ForecastAdjustmentSettingsInitializationEvidence;
}

/// one complete JSON file supplied by the existing serialization boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdmittedForecastAdjustmentSettingsJson(Vec<u8>);

impl AdmittedForecastAdjustmentSettingsJson {
    /// admit one complete settings file including its trailing newline.
    pub fn admitted(content: impl Into<Vec<u8>>) -> Self {
        Self(content.into())
    }

    /// borrow the admitted complete file content.
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

/// the existing JSON adapter retained outside filesystem mechanics.
pub trait ForecastAdjustmentSettingsJsonAdapter {
    type Error;

    /// decode one admitted settings file through the canonical JSON boundary.
    fn decode(&mut self, content: &[u8]) -> Result<ForecastAdjustmentSettings, Self::Error>;

    /// encode one validated snapshot as a complete canonical settings file.
    fn encode(
        &mut self,
        settings: ForecastAdjustmentSettings,
    ) -> Result<AdmittedForecastAdjustmentSettingsJson, Self::Error>;
}

/// the unique temporary path boundary used by atomic filesystem replacement.
pub trait ForecastAdjustmentSettingsTemporaryPathSource {
    /// return one admitted same-filesystem temporary path for this target.
    fn next_temporary_path(&mut self, target: &Path) -> PathBuf;
}

/// one observable settings file-state failure.
#[derive(Debug)]
pub enum FileForecastAdjustmentSettingsError<AdapterError> {
    Io(io::Error),
    Adapter(AdapterError),
    MissingSettingsFile,
    InvalidMarker,
    MissingMarker,
    InitializationNotAdmitted,
    InvalidInitializationEvidence,
}

impl<AdapterError: Display> Display for FileForecastAdjustmentSettingsError<AdapterError> {
    /// preserve source state errors while retaining ordinary adapter failures.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        // preserve the owning Node store's state failure categories
        match self {
            Self::Io(error) => Display::fmt(error, formatter),
            Self::Adapter(error) => Display::fmt(error, formatter),
            Self::MissingSettingsFile => {
                formatter.write_str("forecast adjustment settings file is missing")
            }
            Self::InvalidMarker => {
                formatter.write_str("forecast adjustment settings marker is invalid")
            }
            Self::MissingMarker => {
                formatter.write_str("forecast adjustment settings marker is missing")
            }
            Self::InitializationNotAdmitted => {
                formatter.write_str("admin authentication is not configured")
            }
            Self::InvalidInitializationEvidence => {
                formatter.write_str("admin auth state is invalid")
            }
        }
    }
}

impl<AdapterError> Error for FileForecastAdjustmentSettingsError<AdapterError> where
    AdapterError: Error + 'static
{
}

impl<AdapterError> From<io::Error> for FileForecastAdjustmentSettingsError<AdapterError> {
    /// retain one ordinary filesystem error without relabeling it.
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

/// the real local file implementation of the adjustment settings store.
#[derive(Debug)]
pub struct FileForecastAdjustmentSettingsStore<Adapter, TemporaryPaths, InitializationEvidence> {
    adapter: Adapter,
    default_json: AdmittedForecastAdjustmentSettingsJson,
    initialization_evidence: InitializationEvidence,
    initialized: bool,
    marker_path: PathBuf,
    settings_path: PathBuf,
    temporary_paths: TemporaryPaths,
}

impl<Adapter, TemporaryPaths, InitializationEvidence>
    FileForecastAdjustmentSettingsStore<Adapter, TemporaryPaths, InitializationEvidence>
{
    /// create one file store over caller-owned paths and admitted boundaries.
    pub fn new(
        settings_path: PathBuf,
        default_json: AdmittedForecastAdjustmentSettingsJson,
        initialization_evidence: InitializationEvidence,
        adapter: Adapter,
        temporary_paths: TemporaryPaths,
    ) -> Self {
        let marker_path = PathBuf::from(format!("{}.initialized", settings_path.display()));
        Self {
            adapter,
            default_json,
            initialization_evidence,
            initialized: false,
            marker_path,
            settings_path,
            temporary_paths,
        }
    }

    /// borrow the exact marker path derived by the Node store.
    pub fn marker_path(&self) -> &Path {
        &self.marker_path
    }

    /// report whether this process completed or observed initialization.
    pub const fn initialized(&self) -> bool {
        self.initialized
    }
}

impl<Adapter, TemporaryPaths, InitializationEvidence>
    FileForecastAdjustmentSettingsStore<Adapter, TemporaryPaths, InitializationEvidence>
where
    Adapter: ForecastAdjustmentSettingsJsonAdapter,
    TemporaryPaths: ForecastAdjustmentSettingsTemporaryPathSource,
    InitializationEvidence: ForecastAdjustmentSettingsInitializationEvidenceReader,
{
    /// read one trusted settings snapshot or return the exact state error.
    fn read_settings(
        &mut self,
    ) -> Result<ForecastAdjustmentSettings, FileForecastAdjustmentSettingsError<Adapter::Error>>
    {
        self.initialize()?;
        let content = read_optional_bytes(&self.settings_path)?
            .ok_or(FileForecastAdjustmentSettingsError::MissingSettingsFile)?;
        self.adapter
            .decode(&content)
            .map_err(FileForecastAdjustmentSettingsError::Adapter)
    }

    /// replace one validated snapshot with marker repair ordering.
    fn write_settings(
        &mut self,
        settings: ForecastAdjustmentSettings,
    ) -> Result<(), FileForecastAdjustmentSettingsError<Adapter::Error>> {
        let document = self
            .adapter
            .encode(settings)
            .map_err(FileForecastAdjustmentSettingsError::Adapter)?;
        let marker = read_optional_text(&self.marker_path)?;

        // repair a damaged marker after the settings file is durable
        if marker.as_deref() != Some(ADJUSTMENT_SETTINGS_MARKER)
            && (marker.is_some() || self.initialized)
        {
            self.atomic_write(&self.settings_path.clone(), document.as_bytes())?;
            self.atomic_write(
                &self.marker_path.clone(),
                ADJUSTMENT_SETTINGS_MARKER_CONTENT,
            )?;
            self.initialized = true;
            return Ok(());
        }

        self.initialize()?;
        self.atomic_write(&self.settings_path.clone(), document.as_bytes())
    }

    /// record first initialization before making default settings visible.
    fn initialize(&mut self) -> Result<(), FileForecastAdjustmentSettingsError<Adapter::Error>> {
        let marker = read_optional_text(&self.marker_path)?;

        // never recreate defaults after initialization or marker damage
        if let Some(marker) = marker {
            // reject an installed marker with unknown contents
            if marker != ADJUSTMENT_SETTINGS_MARKER {
                return Err(FileForecastAdjustmentSettingsError::InvalidMarker);
            }
            self.initialized = true;
            return Ok(());
        }

        // reject in-process loss of the persistent marker
        if self.initialized {
            return Err(FileForecastAdjustmentSettingsError::MissingMarker);
        }

        // read current configured-admin evidence on every first-init attempt
        match self.initialization_evidence.read_initialization_evidence() {
            ForecastAdjustmentSettingsInitializationEvidence::Missing => {
                return Err(FileForecastAdjustmentSettingsError::InitializationNotAdmitted);
            }
            ForecastAdjustmentSettingsInitializationEvidence::Invalid => {
                return Err(FileForecastAdjustmentSettingsError::InvalidInitializationEvidence);
            }
            ForecastAdjustmentSettingsInitializationEvidence::Admitted(_authorization) => {}
        }
        create_parent_directory(&self.marker_path)?;

        // let only one first initializer create the marker
        match create_new_private_file(&self.marker_path, ADJUSTMENT_SETTINGS_MARKER_CONTENT) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                let installed = read_optional_text(&self.marker_path)?;

                // reject a raced or invalid marker instead of assuming authority
                if installed.as_deref() != Some(ADJUSTMENT_SETTINGS_MARKER) {
                    return Err(FileForecastAdjustmentSettingsError::InvalidMarker);
                }
                self.initialized = true;
                return Ok(());
            }
            Err(error) => return Err(FileForecastAdjustmentSettingsError::Io(error)),
        }

        let default_json = self.default_json.clone();
        self.atomic_create_if_missing(&self.settings_path.clone(), default_json.as_bytes())?;
        self.initialized = true;
        Ok(())
    }

    /// replace one file by writing a private temporary and renaming it.
    fn atomic_write(
        &mut self,
        target: &Path,
        content: &[u8],
    ) -> Result<(), FileForecastAdjustmentSettingsError<Adapter::Error>> {
        let temporary = self.temporary_paths.next_temporary_path(target);
        atomic_replace_private_file(target, &temporary, content)?;
        Ok(())
    }

    /// publish a default only when no concurrent settings value exists.
    fn atomic_create_if_missing(
        &mut self,
        target: &Path,
        content: &[u8],
    ) -> Result<(), FileForecastAdjustmentSettingsError<Adapter::Error>> {
        create_parent_directory(target)?;
        let temporary = self.temporary_paths.next_temporary_path(target);
        write_private_file(&temporary, content)?;
        let link_result = fs::hard_link(&temporary, target);
        let cleanup_result = fs::remove_file(&temporary);

        // retain only an already-installed concurrent settings value
        if let Err(error) = link_result {
            // preserve the source's EEXIST-only race handling
            if error.kind() != io::ErrorKind::AlreadyExists {
                cleanup_result?;
                return Err(FileForecastAdjustmentSettingsError::Io(error));
            }
        }

        // require temporary cleanup unless the file already disappeared
        if let Err(error) = cleanup_result {
            // tolerate only the source's missing-temporary cleanup case
            if error.kind() != io::ErrorKind::NotFound {
                return Err(FileForecastAdjustmentSettingsError::Io(error));
            }
        }

        Ok(())
    }
}

impl<Adapter, TemporaryPaths, InitializationEvidence> ForecastAdjustmentSettingsStore
    for FileForecastAdjustmentSettingsStore<Adapter, TemporaryPaths, InitializationEvidence>
where
    Adapter: ForecastAdjustmentSettingsJsonAdapter,
    TemporaryPaths: ForecastAdjustmentSettingsTemporaryPathSource,
    InitializationEvidence: ForecastAdjustmentSettingsInitializationEvidenceReader,
{
    type Error = FileForecastAdjustmentSettingsError<Adapter::Error>;

    /// fail closed when initialization, reading, or decoding is untrusted.
    fn read_status(&mut self) -> ForecastAdjustmentSettingsStatus {
        // expose no storage diagnostic through the ordinary status value
        match self.read_settings() {
            Ok(settings) => ForecastAdjustmentSettingsStatus {
                settings,
                error: None,
            },
            Err(_) => ForecastAdjustmentSettingsStatus {
                settings: DISABLED_FORECAST_ADJUSTMENT_SETTINGS,
                error: Some(ForecastAdjustmentSettingsDiagnostic::Unavailable),
            },
        }
    }

    /// persist one complete settings document before returning its snapshot.
    fn replace_atomically(
        &mut self,
        settings: ForecastAdjustmentSettings,
    ) -> Result<ForecastAdjustmentSettings, Self::Error> {
        self.write_settings(settings)?;
        Ok(settings)
    }
}

/// read one optional trimmed UTF-8 marker.
fn read_optional_text(path: &Path) -> io::Result<Option<String>> {
    let content = read_optional_bytes(path)?;

    // decode only present marker content
    match content {
        None => Ok(None),
        Some(content) => String::from_utf8(content)
            .map(|text| Some(text.trim().to_owned()))
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error)),
    }
}

/// create one marker without replacing an existing initializer.
fn create_new_private_file(path: &Path, content: &[u8]) -> io::Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(content)
}
