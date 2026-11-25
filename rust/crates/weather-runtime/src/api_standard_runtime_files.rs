//! Native filesystem input for installed forecast adjustment runtimes.

use std::fs::{self, Metadata};
use std::io::{self, Read};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::{Component, Path, PathBuf};

use crate::api_maintenance_candidate_files::open_read_no_follow;
use crate::maintenance_canonical_json::valid_hash;

const STANDARD_REGISTRY: &str = "ballydidean.json";
const WIND_REGISTRY: &str = "ballydidean-wind-canary.json";
const TEMPERATURE_REGISTRY: &str = "ballydidean-temperature-canary.json";
const RAIN_REGISTRY: &str = "ballydidean-rain-runtime.json";
const INSTALLED_STANDARD_ROOT: &str = "/opt/weather/config/forecast-adjustments";

/// Read fixed forecast registries and their selected immutable bundles.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeForecastRuntimeFiles {
    root: PathBuf,
}

impl NativeForecastRuntimeFiles {
    /// Bind the fixed production root while deferring filesystem effects until read.
    pub fn installed() -> Self {
        Self {
            root: PathBuf::from(INSTALLED_STANDARD_ROOT),
        }
    }

    /// Bind lookup to one byte-exact canonical absolute runtime root.
    pub fn new(root: impl Into<PathBuf>) -> io::Result<Self> {
        let root = root.into();
        let normalized = normalize_absolute(&root).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "forecast runtime root must be absolute",
            )
        })?;
        // reject aliases that Node path resolution would rewrite
        if root.as_os_str().as_bytes() != normalized.as_os_str().as_bytes() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "forecast runtime root must be byte-exact canonical",
            ));
        }
        validate_root(&root)?;
        Ok(Self { root })
    }

    /// Read the one fixed standard forecast registry.
    pub fn read_standard_registry(&self) -> io::Result<Vec<u8>> {
        self.read_member(&self.root.join(STANDARD_REGISTRY))
    }

    /// Read one lowercase content-addressed standard runtime bundle.
    pub fn read_standard_bundle(&self, bundle_sha256: &str) -> io::Result<Vec<u8>> {
        // reject caller input before constructing any installed path
        if !valid_hash(bundle_sha256) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "forecast runtime bundle identity is invalid",
            ));
        }
        let target = self
            .root
            .join("ballydidean")
            .join("bundles")
            .join(format!("sha256-{bundle_sha256}.json"));
        self.read_member(&target)
    }

    /// read the one fixed wind canary registry
    pub fn read_wind_registry(&self) -> io::Result<Vec<u8>> {
        self.read_member(&self.root.join(WIND_REGISTRY))
    }

    /// read one content-addressed wind canary bundle
    pub fn read_wind_bundle(&self, bundle_sha256: &str) -> io::Result<Vec<u8>> {
        // reject invalid addresses before path construction
        if !valid_hash(bundle_sha256) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "wind canary runtime bundle identity is invalid",
            ));
        }
        let target = self
            .root
            .join("ballydidean")
            .join("wind-canary-bundles")
            .join(format!("sha256-{bundle_sha256}.json"));
        self.read_member(&target)
    }

    /// read the fixed temperature canary registry
    pub fn read_temperature_registry(&self) -> io::Result<Vec<u8>> {
        self.read_member(&self.root.join(TEMPERATURE_REGISTRY))
    }

    /// read one content-addressed temperature canary bundle
    pub fn read_temperature_bundle(&self, bundle_sha256: &str) -> io::Result<Vec<u8>> {
        // validate the address before constructing the path
        if !valid_hash(bundle_sha256) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "temperature canary runtime bundle identity is invalid",
            ));
        }
        self.read_member(
            &self
                .root
                .join("ballydidean")
                .join("temperature-canary-bundles")
                .join(format!("sha256-{bundle_sha256}.json")),
        )
    }

    /// read the fixed rain incumbent registry
    pub fn read_rain_registry(&self) -> io::Result<Vec<u8>> {
        self.read_member(&self.root.join(RAIN_REGISTRY))
    }

    /// Read one fixed descendant while binding bytes to its opened identity.
    fn read_member(&self, target: &Path) -> io::Result<Vec<u8>> {
        validate_member_path(&self.root, target)?;
        let before = fs::symlink_metadata(target)?;
        // reject a replacement link or special node before canonical resolution
        if before.file_type().is_symlink() || !before.is_file() {
            return Err(invalid_member(
                "forecast runtime member is not a regular file",
            ));
        }
        let before_real = fs::canonicalize(target)?;
        let mut file = open_read_no_follow(target)?;
        let opened = file.metadata()?;
        // bind the descriptor to the path identity observed before open
        if !opened.is_file() || !same_file_metadata(&before, &opened) || before_real != target {
            return Err(invalid_member(
                "forecast runtime member changed before open",
            ));
        }

        let expected_len = usize::try_from(before.size())
            .map_err(|_| invalid_member("forecast runtime member is too large for this process"))?;
        let guarded_len = expected_len
            .checked_add(1)
            .ok_or_else(|| invalid_member("forecast runtime member length overflowed"))?;
        let mut bytes = Vec::new();
        // reserve the observed size without an allocation panic
        bytes
            .try_reserve_exact(expected_len)
            .map_err(|_| invalid_member("forecast runtime member allocation is unavailable"))?;
        file.by_ref()
            .take(
                u64::try_from(guarded_len)
                    .map_err(|_| invalid_member("forecast runtime member length overflowed"))?,
            )
            .read_to_end(&mut bytes)?;

        let opened_after = file.metadata()?;
        let after = fs::symlink_metadata(target)?;
        // reject a replacement link or special node before final resolution
        if after.file_type().is_symlink() || !after.is_file() {
            return Err(invalid_member(
                "forecast runtime member changed during read",
            ));
        }
        let after_real = fs::canonicalize(target)?;
        // require stable descriptor and path identities through the complete read
        if !same_file_metadata(&opened, &opened_after)
            || !same_file_metadata(&opened, &after)
            || after_real != before_real
            || bytes.len() != expected_len
        {
            return Err(invalid_member(
                "forecast runtime member changed during read",
            ));
        }
        Ok(bytes)
    }
}

/// Require every component of the configured root to remain canonical.
fn validate_root(root: &Path) -> io::Result<()> {
    let mut cursor = PathBuf::from("/");
    validate_directory(&cursor)?;
    // inspect every root component without following aliases
    for component in root.components() {
        match component {
            Component::RootDir | Component::CurDir => {}
            Component::Normal(part) => {
                cursor.push(part);
                validate_directory(&cursor)?;
            }
            Component::ParentDir | Component::Prefix(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "forecast runtime root is not canonical",
                ));
            }
        }
    }
    Ok(())
}

/// Require one root component to be a real directory at its literal path.
fn validate_directory(path: &Path) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    // reject links and nondirectories before canonical resolution
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(invalid_member("forecast runtime root component is invalid"));
    }
    let canonical = fs::canonicalize(path)?;
    // reject any component whose real identity has another spelling
    if canonical != path {
        return Err(invalid_member("forecast runtime root component is aliased"));
    }
    Ok(())
}

/// Require every selected descendant component to remain inside the root.
fn validate_member_path(root: &Path, target: &Path) -> io::Result<()> {
    validate_root(root)?;
    let relative = target
        .strip_prefix(root)
        .map_err(|_| invalid_member("forecast runtime member escapes its root"))?;
    // reject the root itself instead of treating it as a readable member
    if relative.as_os_str().is_empty() {
        return Err(invalid_member("forecast runtime member is missing"));
    }

    let mut cursor = root.to_path_buf();
    let mut components = relative.components().peekable();
    // verify each selected component immediately before opening the final file
    while let Some(component) = components.next() {
        let Component::Normal(part) = component else {
            return Err(invalid_member("forecast runtime member path is invalid"));
        };
        cursor.push(part);
        let metadata = fs::symlink_metadata(&cursor)?;
        let final_component = components.peek().is_none();
        // reject links and the wrong node kind before canonical resolution
        if metadata.file_type().is_symlink()
            || (final_component && !metadata.is_file())
            || (!final_component && !metadata.is_dir())
        {
            return Err(invalid_member("forecast runtime member path is invalid"));
        }
        let canonical = fs::canonicalize(&cursor)?;
        // require literal in-root resolution for every descendant
        if canonical != cursor || canonical == root || !canonical.starts_with(root) {
            return Err(invalid_member("forecast runtime member path is aliased"));
        }
    }
    Ok(())
}

/// Compare the stable file identity fields used by the original loader.
fn same_file_metadata(left: &Metadata, right: &Metadata) -> bool {
    left.dev() == right.dev() && left.ino() == right.ino() && left.size() == right.size()
}

/// Normalize one absolute Unix path without consulting the filesystem.
fn normalize_absolute(path: &Path) -> Option<PathBuf> {
    // reject relative roots before any filesystem traversal
    if !path.is_absolute() {
        return None;
    }
    let mut normalized = PathBuf::from("/");
    // fold dot segments exactly once before byte-spelling comparison
    for component in path.components() {
        match component {
            Component::RootDir | Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::Normal(part) => normalized.push(part),
            Component::Prefix(_) => return None,
        }
    }
    Some(normalized)
}

/// Construct one value-free invalid installed-member error.
fn invalid_member(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
