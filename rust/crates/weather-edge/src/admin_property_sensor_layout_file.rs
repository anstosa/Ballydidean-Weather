use std::error::Error;
use std::fmt::{Display, Formatter};
use std::io;
use std::path::{Path, PathBuf};

use crate::private_file_state::{atomic_replace_private_file, read_optional_bytes};
use crate::{PropertySensorLayoutEntry, PropertySensorLayoutStore};

/// one complete layout JSON file supplied by the existing serialization boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdmittedPropertySensorLayoutJson(Vec<u8>);

impl AdmittedPropertySensorLayoutJson {
    /// admit one complete canonical layout file including its trailing newline.
    pub fn admitted(content: impl Into<Vec<u8>>) -> Self {
        Self(content.into())
    }

    /// borrow the admitted complete file content.
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

/// the existing canonical layout JSON adapter retained outside filesystem mechanics.
pub trait PropertySensorLayoutJsonAdapter {
    type Error;

    /// decode and validate one complete persisted layout envelope.
    fn decode(&mut self, content: &[u8]) -> Result<Vec<PropertySensorLayoutEntry>, Self::Error>;

    /// encode one complete validated layout envelope with its trailing newline.
    fn encode(
        &mut self,
        layout: &[PropertySensorLayoutEntry],
    ) -> Result<AdmittedPropertySensorLayoutJson, Self::Error>;
}

/// same-directory temporary path allocation retained outside filesystem mechanics.
pub trait PropertySensorLayoutTemporaryPathSource {
    /// return a fresh temporary path for one atomic replacement target.
    fn next_temporary_path(&mut self, target: &Path) -> PathBuf;
}

/// one ordinary file or canonical adapter failure.
#[derive(Debug)]
pub enum FilePropertySensorLayoutError<AdapterError> {
    Io(io::Error),
    Adapter(AdapterError),
}

impl<AdapterError: Display> Display for FilePropertySensorLayoutError<AdapterError> {
    /// render the underlying ordinary store failure.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        // preserve file and canonical adapter error text
        match self {
            Self::Io(error) => Display::fmt(error, formatter),
            Self::Adapter(error) => Display::fmt(error, formatter),
        }
    }
}

impl<AdapterError> Error for FilePropertySensorLayoutError<AdapterError>
where
    AdapterError: Error + 'static,
{
    /// retain the underlying file or adapter source.
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        // expose exactly the owning failure
        match self {
            Self::Io(error) => Some(error),
            Self::Adapter(error) => Some(error),
        }
    }
}

impl<AdapterError> From<io::Error> for FilePropertySensorLayoutError<AdapterError> {
    /// convert one filesystem failure without changing its kind.
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

/// real local-file layout storage over admitted canonical JSON and temporary paths.
#[derive(Debug)]
pub struct FilePropertySensorLayoutStore<Adapter, TemporaryPaths> {
    adapter: Adapter,
    layout_path: PathBuf,
    temporary_paths: TemporaryPaths,
}

impl<Adapter, TemporaryPaths> FilePropertySensorLayoutStore<Adapter, TemporaryPaths> {
    /// create one file store without reading or creating its target.
    pub fn new(layout_path: PathBuf, adapter: Adapter, temporary_paths: TemporaryPaths) -> Self {
        Self {
            adapter,
            layout_path,
            temporary_paths,
        }
    }

    /// borrow the configured non-credential layout path.
    pub fn layout_path(&self) -> &Path {
        &self.layout_path
    }
}

impl<Adapter, TemporaryPaths> PropertySensorLayoutStore
    for FilePropertySensorLayoutStore<Adapter, TemporaryPaths>
where
    Adapter: PropertySensorLayoutJsonAdapter,
    TemporaryPaths: PropertySensorLayoutTemporaryPathSource,
{
    type Error = FilePropertySensorLayoutError<Adapter::Error>;

    /// return the missing-file empty layout or decode the complete persisted state.
    fn read_layout(&mut self) -> Result<Vec<PropertySensorLayoutEntry>, Self::Error> {
        let content = read_optional_bytes(&self.layout_path)?;

        // preserve the source store's empty first-run layout
        match content {
            None => Ok(Vec::new()),
            Some(content) => self
                .adapter
                .decode(&content)
                .map_err(FilePropertySensorLayoutError::Adapter),
        }
    }

    /// encode and atomically replace the complete ordered layout.
    fn replace_layout_atomically(
        &mut self,
        layout: Vec<PropertySensorLayoutEntry>,
    ) -> Result<(), Self::Error> {
        let content = self
            .adapter
            .encode(&layout)
            .map_err(FilePropertySensorLayoutError::Adapter)?;
        let temporary = self.temporary_paths.next_temporary_path(&self.layout_path);
        atomic_replace_private_file(&self.layout_path, &temporary, content.as_bytes())?;
        Ok(())
    }
}
