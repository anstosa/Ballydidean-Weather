//! Unix filesystem custody for installed API and worker maintenance candidate material.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Component, Path, PathBuf};

use weather_contracts::AdjustmentShadowFamily;

use crate::api_maintenance_candidate::{ApiMaintenanceCandidateError, valid_hash};
use crate::installed_artifact_files::{
    require_root_owned_regular_file, require_root_read_only_file,
};

const CATALOG_PATH: &str = "/run/weather/adjustment-candidate-catalog.json";
const RAIN_CONTROL_SELECTOR: &str =
    "config/forecast-adjustments/ballydidean-rain-control-reference.json";
const RAIN_RUNTIME_ROOT: &str = "config/forecast-adjustments";
const RAIN_RUNTIME_REGISTRY: &str = "config/forecast-adjustments/ballydidean-rain-runtime.json";

// linux uapi asm-generic/fcntl.h
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
const O_NOFOLLOW: i32 = 1 << 17;
// linux arm64 asm/fcntl.h
#[cfg(all(target_os = "linux", target_arch = "aarch64"))]
const O_NOFOLLOW: i32 = 1 << 15;

type CandidateError = ApiMaintenanceCandidateError;

/// Exact installed member bytes retained for the approved candidate loader.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiMaintenanceCandidateMembers {
    pub action: Vec<u8>,
    pub projection: Vec<u8>,
    pub parity: Vec<u8>,
    pub bundle: Vec<u8>,
}

/// exact installed qualified canary runtime bytes
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiQualifiedCanaryMembers {
    pub registry: Vec<u8>,
    pub action: Vec<u8>,
    pub bundle: Vec<u8>,
}

/// Exact installed rain candidate bytes including its technical package receipt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerRainCandidateMembers {
    pub members: ApiMaintenanceCandidateMembers,
    pub package: Vec<u8>,
}

/// Exact installed fixed-control state and ordinal artifact bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerRainControlMembers {
    pub state: Vec<u8>,
    pub ordinal: Vec<u8>,
}

/// Exact independently installed rain control selector and its action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerRainControlSelectorMembers {
    pub selector: Vec<u8>,
    pub action: Vec<u8>,
}

/// Exact incumbent registry and installed qualified rain serving members.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerRainServingMembers {
    pub registry: Vec<u8>,
    pub action: Vec<u8>,
    pub bundle: Vec<u8>,
    pub package: Vec<u8>,
}

/// Read root-installed catalog and candidate bytes without parsing their contents.
pub trait ApiMaintenanceCandidateFiles {
    /// Read the fixed root-owned read-only bind-mounted candidate catalog.
    fn read_catalog(&mut self) -> Result<Vec<u8>, ApiMaintenanceCandidateError>;

    /// Admit all four root-owned members before reading any candidate bytes.
    fn read_members(
        &mut self,
        family: AdjustmentShadowFamily,
        action_sha256: &str,
        candidate_sha256: &str,
        bundle_sha256: &str,
    ) -> Result<ApiMaintenanceCandidateMembers, ApiMaintenanceCandidateError>;
}

/// API custody extension restricted to qualified wind and temperature canary material.
pub trait ApiQualifiedCanaryFiles: ApiMaintenanceCandidateFiles {
    /// admit the qualified registry, action, and bundle before reading any bytes
    fn read_qualified_canary_members(
        &mut self,
        family: AdjustmentShadowFamily,
        action_sha256: &str,
        bundle_sha256: &str,
    ) -> Result<ApiQualifiedCanaryMembers, ApiMaintenanceCandidateError>;
}

/// Worker custody extension restricted to installed rain candidate material.
pub trait RainMaintenanceCandidateFiles: ApiMaintenanceCandidateFiles {
    /// Admit all five rain candidate members before reading any candidate bytes.
    fn read_rain_members(
        &mut self,
        action_sha256: &str,
        candidate_sha256: &str,
        bundle_sha256: &str,
    ) -> Result<WorkerRainCandidateMembers, ApiMaintenanceCandidateError>;

    /// Admit both fixed-control members before reading either file.
    fn read_rain_control_members(
        &mut self,
        state_sha256: &str,
        ordinal_sha256: &str,
    ) -> Result<WorkerRainControlMembers, ApiMaintenanceCandidateError>;
}

/// Worker custody extension for the independently installed control reference selector.
pub trait WorkerRainControlReferenceFiles: RainMaintenanceCandidateFiles {
    /// Admit the fixed selector and its content-addressed action before reading either file.
    fn read_worker_rain_control_selector(
        &mut self,
        action_sha256: &str,
    ) -> Result<WorkerRainControlSelectorMembers, ApiMaintenanceCandidateError>;
}

/// Worker custody extension for incumbent and qualified serving rain material.
pub trait WorkerRainServingFiles: ApiMaintenanceCandidateFiles {
    /// Read the fixed incumbent registry through its inspected no-follow descriptor.
    fn read_worker_rain_registry(&mut self) -> Result<Vec<u8>, ApiMaintenanceCandidateError>;

    /// Admit the qualified registry and all three addressed members before reading any bytes.
    fn read_worker_rain_serving_members(
        &mut self,
        action_sha256: &str,
        candidate_sha256: &str,
        bundle_sha256: &str,
    ) -> Result<WorkerRainServingMembers, ApiMaintenanceCandidateError>;
}

/// Concrete Unix reader rooted at one absolute immutable application directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeApiMaintenanceCandidateFiles {
    app_root: PathBuf,
}

impl NativeApiMaintenanceCandidateFiles {
    /// Bind candidate member lookup to one lexically resolved absolute app root.
    pub fn new(app_root: impl Into<PathBuf>) -> Result<Self, ApiMaintenanceCandidateError> {
        let app_root = normalize_absolute(&app_root.into()).ok_or(CandidateError::File)?;
        Ok(Self { app_root })
    }

    /// Borrow the admitted application root for composition diagnostics.
    pub fn app_root(&self) -> &Path {
        &self.app_root
    }
}

impl ApiMaintenanceCandidateFiles for NativeApiMaintenanceCandidateFiles {
    /// Require the fixed catalog file and its literal read-only file bind mount before reading.
    fn read_catalog(&mut self) -> Result<Vec<u8>, ApiMaintenanceCandidateError> {
        let catalog = Path::new(CATALOG_PATH);
        require_root_read_only_file(CATALOG_PATH).map_err(|_| CandidateError::File)?;
        fs::read(catalog).map_err(|_| CandidateError::File)
    }

    /// Resolve, admit, then read the exact action, projection, parity, and bundle members.
    fn read_members(
        &mut self,
        family: AdjustmentShadowFamily,
        action_sha256: &str,
        candidate_sha256: &str,
        bundle_sha256: &str,
    ) -> Result<ApiMaintenanceCandidateMembers, ApiMaintenanceCandidateError> {
        let (family_name, bundle_directory) = match family {
            AdjustmentShadowFamily::Temperature => ("temperature", "temperature-canary-bundles"),
            AdjustmentShadowFamily::Wind => ("wind", "wind-canary-bundles"),
            AdjustmentShadowFamily::Rain => return Err(CandidateError::File),
        };
        let paths = self.shadow_paths(
            family_name,
            bundle_directory,
            action_sha256,
            candidate_sha256,
            bundle_sha256,
        )?;
        admit_member_paths(&paths)?;
        read_shadow_members(paths)
    }
}

impl ApiQualifiedCanaryFiles for NativeApiMaintenanceCandidateFiles {
    /// resolve only the serving registry, action, and runtime bundle
    fn read_qualified_canary_members(
        &mut self,
        family: AdjustmentShadowFamily,
        action_sha256: &str,
        bundle_sha256: &str,
    ) -> Result<ApiQualifiedCanaryMembers, ApiMaintenanceCandidateError> {
        let (registry, bundle_directory) = match family {
            AdjustmentShadowFamily::Temperature => (
                "config/forecast-adjustments/ballydidean-temperature-canary.json",
                "temperature-canary-bundles",
            ),
            AdjustmentShadowFamily::Wind => (
                "config/forecast-adjustments/ballydidean-wind-canary.json",
                "wind-canary-bundles",
            ),
            AdjustmentShadowFamily::Rain => return Err(CandidateError::File),
        };
        let paths = [
            self.member_path(Path::new(registry))?,
            self.addressed_path("actions", action_sha256)?,
            self.addressed_path(bundle_directory, bundle_sha256)?,
        ];
        admit_member_paths(&paths)?;
        let [registry, action, bundle] = paths;
        Ok(ApiQualifiedCanaryMembers {
            registry: read_member(registry)?,
            action: read_member(action)?,
            bundle: read_member(bundle)?,
        })
    }
}

impl RainMaintenanceCandidateFiles for NativeApiMaintenanceCandidateFiles {
    /// Resolve, admit, then read the exact rain action, projection, parity, artifact, and package.
    fn read_rain_members(
        &mut self,
        action_sha256: &str,
        candidate_sha256: &str,
        bundle_sha256: &str,
    ) -> Result<WorkerRainCandidateMembers, ApiMaintenanceCandidateError> {
        let paths = self.shadow_paths(
            "rain",
            "rain-runtime-artifacts",
            action_sha256,
            candidate_sha256,
            bundle_sha256,
        )?;
        let package = self.addressed_path("rain-model-packages", candidate_sha256)?;

        // admit the common members and the technical package before any read
        admit_member_paths(&paths)?;
        admit_member_paths(std::slice::from_ref(&package))?;
        Ok(WorkerRainCandidateMembers {
            members: read_shadow_members(paths)?,
            package: read_member(package)?,
        })
    }

    /// Resolve, admit, then read the fixed rain control state and ordinal artifact.
    fn read_rain_control_members(
        &mut self,
        state_sha256: &str,
        ordinal_sha256: &str,
    ) -> Result<WorkerRainControlMembers, ApiMaintenanceCandidateError> {
        let paths = [
            self.addressed_path("rain-maintenance-control-states", state_sha256)?,
            self.addressed_path("rain-runtime-artifacts", ordinal_sha256)?,
        ];
        admit_member_paths(&paths)?;
        let [state, ordinal] = paths;
        Ok(WorkerRainControlMembers {
            state: read_member(state)?,
            ordinal: read_member(ordinal)?,
        })
    }
}

impl WorkerRainControlReferenceFiles for NativeApiMaintenanceCandidateFiles {
    /// Resolve and admit the independent selector pair before reading its first member.
    fn read_worker_rain_control_selector(
        &mut self,
        action_sha256: &str,
    ) -> Result<WorkerRainControlSelectorMembers, ApiMaintenanceCandidateError> {
        let paths = [
            self.member_path(Path::new(RAIN_CONTROL_SELECTOR))?,
            self.addressed_path("actions", action_sha256)?,
        ];
        admit_member_paths(&paths)?;
        let [selector, action] = paths;
        Ok(WorkerRainControlSelectorMembers {
            selector: read_member(selector)?,
            action: read_member(action)?,
        })
    }
}

impl WorkerRainServingFiles for NativeApiMaintenanceCandidateFiles {
    /// Preserve the incumbent registry's real-root, no-link, and stable-descriptor read.
    fn read_worker_rain_registry(&mut self) -> Result<Vec<u8>, ApiMaintenanceCandidateError> {
        self.read_incumbent_rain_registry()
    }

    /// Resolve and admit the installed serving set before reading its first member.
    fn read_worker_rain_serving_members(
        &mut self,
        action_sha256: &str,
        candidate_sha256: &str,
        bundle_sha256: &str,
    ) -> Result<WorkerRainServingMembers, ApiMaintenanceCandidateError> {
        let paths = [
            self.member_path(Path::new(RAIN_RUNTIME_REGISTRY))?,
            self.addressed_path("actions", action_sha256)?,
            self.addressed_path("rain-runtime-artifacts", bundle_sha256)?,
            self.addressed_path("rain-model-packages", candidate_sha256)?,
        ];
        admit_member_paths(&paths)?;
        let [registry, action, bundle, package] = paths;
        Ok(WorkerRainServingMembers {
            registry: read_member(registry)?,
            action: read_member(action)?,
            bundle: read_member(bundle)?,
            package: read_member(package)?,
        })
    }
}

impl NativeApiMaintenanceCandidateFiles {
    /// Read the fixed runtime registry while binding every byte to the inspected descriptor.
    fn read_incumbent_rain_registry(&self) -> Result<Vec<u8>, CandidateError> {
        let runtime_root = self.member_path(Path::new(RAIN_RUNTIME_ROOT))?;
        let root_metadata =
            fs::symlink_metadata(&runtime_root).map_err(|_| CandidateError::File)?;
        let root_real = fs::canonicalize(&runtime_root).map_err(|_| CandidateError::File)?;
        // reject linked, aliased, and non-directory runtime roots
        if root_metadata.file_type().is_symlink()
            || !root_metadata.is_dir()
            || root_real != runtime_root
        {
            return Err(CandidateError::File);
        }

        let registry = self.member_path(Path::new(RAIN_RUNTIME_REGISTRY))?;
        let inspected = fs::symlink_metadata(&registry).map_err(|_| CandidateError::File)?;
        let registry_real = fs::canonicalize(&registry).map_err(|_| CandidateError::File)?;
        // reject aliases and nonregular registry nodes before opening
        if inspected.file_type().is_symlink() || !inspected.is_file() || registry_real != registry {
            return Err(CandidateError::File);
        }

        let mut file = open_read_no_follow(&registry).map_err(|_| CandidateError::File)?;
        let opened = file.metadata().map_err(|_| CandidateError::File)?;
        // bind the opened descriptor to the exact node inspected above
        if !opened.is_file()
            || opened.dev() != inspected.dev()
            || opened.ino() != inspected.ino()
            || opened.len() != inspected.len()
        {
            return Err(CandidateError::File);
        }

        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)
            .map_err(|_| CandidateError::File)?;
        let after = file.metadata().map_err(|_| CandidateError::File)?;
        // reject descriptor replacement or size mutation during the read
        if after.dev() != opened.dev() || after.ino() != opened.ino() || after.len() != opened.len()
        {
            return Err(CandidateError::File);
        }
        Ok(bytes)
    }

    /// Resolve the members common to all three shadow families in the original layout.
    fn shadow_paths(
        &self,
        family: &str,
        bundle_directory: &str,
        action_sha256: &str,
        candidate_sha256: &str,
        bundle_sha256: &str,
    ) -> Result<[PathBuf; 4], CandidateError> {
        Ok([
            self.addressed_path("actions", action_sha256)?,
            self.addressed_path(&format!("shadow-catalog/{family}"), candidate_sha256)?,
            self.addressed_path(&format!("model-parity/{family}"), candidate_sha256)?,
            self.addressed_path(bundle_directory, bundle_sha256)?,
        ])
    }

    /// Resolve one fixed member directory and an admitted content-addressed filename.
    fn addressed_path(&self, directory: &str, digest: &str) -> Result<PathBuf, CandidateError> {
        // reject caller-provided names that are not content addresses
        if !valid_hash(digest) {
            return Err(CandidateError::File);
        }
        let relative = Path::new("config/forecast-adjustments/ballydidean")
            .join(directory)
            .join(format!("sha256-{digest}.json"));
        self.member_path(&relative)
    }

    /// Resolve one fixed relative member while proving it remains below the app root.
    fn member_path(&self, relative: &Path) -> Result<PathBuf, CandidateError> {
        let candidate =
            normalize_absolute(&self.app_root.join(relative)).ok_or(CandidateError::File)?;
        let child = candidate
            .strip_prefix(&self.app_root)
            .map_err(|_| CandidateError::File)?;
        // reject the application root itself instead of treating it as a member
        if child.as_os_str().is_empty() {
            return Err(CandidateError::File);
        }
        Ok(candidate)
    }
}

/// Open one final registry component without following a replacement link.
pub(crate) fn open_read_no_follow(path: &Path) -> io::Result<File> {
    #[cfg(all(
        target_os = "linux",
        any(target_arch = "x86_64", target_arch = "aarch64")
    ))]
    {
        OpenOptions::new()
            .read(true)
            .custom_flags(O_NOFOLLOW)
            .open(path)
    }
    #[cfg(not(all(
        target_os = "linux",
        any(target_arch = "x86_64", target_arch = "aarch64")
    )))]
    {
        let _ = path;
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "no verified no-follow flag for this unix target",
        ))
    }
}

/// Admit an entire batch before allowing the caller to read its first member.
fn admit_member_paths(paths: &[PathBuf]) -> Result<(), CandidateError> {
    // retain original root-owned, regular and immutable member custody
    for path in paths {
        require_root_owned_regular_file(path).map_err(|_| CandidateError::File)?;
    }
    Ok(())
}

/// Read the common member set after completing custody admission.
fn read_shadow_members(
    [action, projection, parity, bundle]: [PathBuf; 4],
) -> Result<ApiMaintenanceCandidateMembers, CandidateError> {
    Ok(ApiMaintenanceCandidateMembers {
        action: read_member(action)?,
        projection: read_member(projection)?,
        parity: read_member(parity)?,
        bundle: read_member(bundle)?,
    })
}

/// Read one admitted member without exporting the path on error.
fn read_member(path: PathBuf) -> Result<Vec<u8>, CandidateError> {
    fs::read(path).map_err(|_| CandidateError::File)
}

/// Resolve dot and parent components without consulting filesystem links.
fn normalize_absolute(path: &Path) -> Option<PathBuf> {
    // mirror Node resolve's absolute requirement at the concrete owner boundary
    if !path.is_absolute() {
        return None;
    }
    let mut normalized = PathBuf::from("/");
    // fold only platform path components while preserving ordinary byte names
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
