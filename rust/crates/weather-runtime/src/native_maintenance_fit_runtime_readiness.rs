//! Fixed host fit-runtime verification for maintenance capture readiness.

use std::ffi::OsString;
use std::fs::{self, File};
use std::io::{self, Read};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};

use crate::api_maintenance_candidate_files::open_read_no_follow;
use crate::native_maintenance_capture_installed_closure::{
    NativeMaintenanceCaptureInstalledClosure, NativeMaintenanceCaptureInstalledClosureError,
    VerifiedNativeMaintenanceInstalledClosure,
};

const CONTRACT_VERSION: &str = "adjustment-fit-runtime-readiness/v1";
const NATIVE_CONTRACT_VERSION: &str = "weather-native-adjustment-fit-runtime-readiness/v1";
const BWRAP_PATH: &str = "/usr/bin/bwrap";
const SYSTEMD_RUN_PATH: &str = "/usr/bin/systemd-run";
const SYSTEMCTL_PATH: &str = "/usr/bin/systemctl";
const NODE_PATH: &str = "/home/ubuntu/n/bin/node";
const PYTHON_PATH: &str = "/home/linuxbrew/.linuxbrew/Cellar/python@3.14/3.14.7/bin/python3.14";
const PYTHON_CELLAR_LIB: &str = "/home/linuxbrew/.linuxbrew/Cellar/python@3.14/3.14.7/lib";
const PYTHON_STDLIB: &str = "/home/linuxbrew/.linuxbrew/Cellar/python@3.14/3.14.7/lib/python3.14";
const LINUXBREW_PREFIX: &str = "/home/linuxbrew/.linuxbrew";
const LINUXBREW_LIB: &str = "/home/linuxbrew/.linuxbrew/lib";
const NUMERICAL_RUNTIME_ROOT: &str =
    "/home/ubuntu/.weather/research-runtimes/xgboost-cpu-3.4.1/lib/python3.14/site-packages";
const XGBOOST_NATIVE_LIBRARY: &str = "/home/ubuntu/.weather/research-runtimes/xgboost-cpu-3.4.1/lib/python3.14/site-packages/xgboost/lib/libxgboost.so";
const XGBOOST_NATIVE_SHA256: &str =
    "2850cc04daff59421d9cb2caefbff5ab064c78795802b403a5b7ee3f7e6b8eb4";
const OPENMP_NATIVE_LIBRARY: &str = "/home/ubuntu/.weather/research-runtimes/xgboost-cpu-3.4.1/lib/python3.14/site-packages/xgboost_cpu.libs/libgomp-e985bcbb.so.1.0.0";
const OPENMP_NATIVE_SHA256: &str =
    "a43904e4fa297301d4640dc1bb3c8a3480b406f99e498eba9b1914b68aab604a";
// exact predecessor numerical-library closure used by the native temperature fitter
const OPENBLAS_NATIVE_LIBRARY: &str = "/home/ubuntu/.weather/research-runtimes/xgboost-cpu-3.4.1/lib/python3.14/site-packages/numpy.libs/libscipy_openblas64_-f48b354e.so";
const OPENBLAS_NATIVE_SHA256: &str =
    "432e1c1739731468ba305771af96773939e43016643e2d630f58cb45b6670e96";
const GFORTRAN_NATIVE_LIBRARY: &str = "/home/ubuntu/.weather/research-runtimes/xgboost-cpu-3.4.1/lib/python3.14/site-packages/numpy.libs/libgfortran-83c28eba-468e71e5.so.5.0.0";
const GFORTRAN_NATIVE_SHA256: &str =
    "a6a368328d02a5d8a149746f13f6268f8cc9192df5c18777b25c3c9b5ef52160";
const QUADMATH_NATIVE_LIBRARY: &str = "/home/ubuntu/.weather/research-runtimes/xgboost-cpu-3.4.1/lib/python3.14/site-packages/numpy.libs/libquadmath-2284e583-a9307bba.so.0.0.0";
const QUADMATH_NATIVE_SHA256: &str =
    "6bc3069003caec1f075be3c51ae8355332f081fa9465910c4b5724e7fe7d8ad4";
const COMMAND_OUTPUT_MAXIMUM_BYTES: usize = 16 * 1_024;
const COMMAND_DEADLINE: Duration = Duration::from_secs(30);
const COMMAND_POLL_INTERVAL: Duration = Duration::from_millis(10);

/// One bounded fit-runtime refusal without host paths or process output.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMaintenanceFitRuntimeReadinessError {
    InstalledClosure(NativeMaintenanceCaptureInstalledClosureError),
    NativeInstallation(crate::NativeMaintenanceCaptureNativeInstallationError),
    NumericalRoot,
    Version,
    Directory,
    File,
    Tree,
    Markers,
    Import,
    Process,
}

/// Exact immutable regular-file identity retained by the source receipt.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeMaintenanceFitRuntimeFileIdentity {
    pub gid: u32,
    pub mode: u32,
    pub path: String,
    pub sha256: String,
    pub size: u64,
    pub uid: u32,
}

/// Exact trusted system-directory identity retained by the source receipt.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeMaintenanceFitRuntimeDirectoryIdentity {
    pub gid: u32,
    pub mode: u32,
    pub path: String,
    pub uid: u32,
}

/// Exact complete runtime-tree identity retained by the source receipt.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeMaintenanceFitRuntimeTreeIdentity {
    pub file_count: u64,
    pub gid: u32,
    pub mode: u32,
    pub path: String,
    pub total_bytes: u64,
    pub tree_sha256: String,
    pub uid: u32,
}

/// Fixed tool and numerical-package versions admitted by the predecessor.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeMaintenanceFitRuntimeVersions {
    pub bwrap: String,
    pub node: String,
    pub numpy: String,
    pub python: String,
    pub systemd: String,
    pub xgboost: String,
}

/// Source-compatible fit-runtime readiness manifest before canonical hashing.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeMaintenancePredecessorFitRuntimeManifest {
    pub bwrap: NativeMaintenanceFitRuntimeFileIdentity,
    pub contract_version: String,
    pub linuxbrew_lib: NativeMaintenanceFitRuntimeTreeIdentity,
    pub node: NativeMaintenanceFitRuntimeFileIdentity,
    pub numerical_site_packages: NativeMaintenanceFitRuntimeTreeIdentity,
    pub python: NativeMaintenanceFitRuntimeFileIdentity,
    pub python_cellar_lib: NativeMaintenanceFitRuntimeTreeIdentity,
    pub python_stdlib: NativeMaintenanceFitRuntimeTreeIdentity,
    pub system_directories: Vec<NativeMaintenanceFitRuntimeDirectoryIdentity>,
    pub systemctl: NativeMaintenanceFitRuntimeFileIdentity,
    pub systemd_run: NativeMaintenanceFitRuntimeFileIdentity,
    pub versions: NativeMaintenanceFitRuntimeVersions,
}

impl NativeMaintenancePredecessorFitRuntimeManifest {
    /// Encode the exact recursively key-sorted source manifest.
    pub fn canonical_json(&self) -> String {
        format!(
            concat!(
                "{{\"bwrap\":{},\"contractVersion\":{},\"linuxbrewLib\":{},",
                "\"node\":{},\"numericalSitePackages\":{},\"python\":{},",
                "\"pythonCellarLib\":{},\"pythonStdlib\":{},\"systemDirectories\":[{}],",
                "\"systemctl\":{},\"systemdRun\":{},\"versions\":{}}}"
            ),
            file_identity_json(&self.bwrap),
            json_string(&self.contract_version),
            tree_identity_json(&self.linuxbrew_lib),
            file_identity_json(&self.node),
            tree_identity_json(&self.numerical_site_packages),
            file_identity_json(&self.python),
            tree_identity_json(&self.python_cellar_lib),
            tree_identity_json(&self.python_stdlib),
            self.system_directories
                .iter()
                .map(directory_identity_json)
                .collect::<Vec<_>>()
                .join(","),
            file_identity_json(&self.systemctl),
            file_identity_json(&self.systemd_run),
            versions_json(&self.versions),
        )
    }
}

/// Native fit readiness binds only the fixed host controls and existing numerical library bytes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeMaintenanceNativeFitRuntimeManifest {
    pub bwrap: NativeMaintenanceFitRuntimeFileIdentity,
    pub bwrap_version: String,
    pub system_directories: Vec<NativeMaintenanceFitRuntimeDirectoryIdentity>,
    pub systemctl: NativeMaintenanceFitRuntimeFileIdentity,
    pub systemd_run: NativeMaintenanceFitRuntimeFileIdentity,
    pub systemd_version: String,
    pub xgboost: NativeMaintenanceFitRuntimeFileIdentity,
    pub openmp: NativeMaintenanceFitRuntimeFileIdentity,
    pub openblas: NativeMaintenanceFitRuntimeFileIdentity,
    pub gfortran: NativeMaintenanceFitRuntimeFileIdentity,
    pub quadmath: NativeMaintenanceFitRuntimeFileIdentity,
}

/// Disjoint readiness contracts retain the predecessor bytes without invoking them for native fits.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NativeMaintenanceFitRuntimeManifest {
    Predecessor(Box<NativeMaintenancePredecessorFitRuntimeManifest>),
    Native(Box<NativeMaintenanceNativeFitRuntimeManifest>),
}

impl NativeMaintenanceFitRuntimeManifest {
    /// Return the exact generation-specific receipt version.
    pub fn contract_version(&self) -> &str {
        match self {
            Self::Predecessor(manifest) => &manifest.contract_version,
            Self::Native(_) => NATIVE_CONTRACT_VERSION,
        }
    }

    /// Encode the selected manifest without introducing a synthetic interpreter receipt.
    pub fn canonical_json(&self) -> String {
        match self {
            Self::Predecessor(manifest) => manifest.canonical_json(),
            Self::Native(manifest) => format!(
                "{{\"bwrap\":{},\"bwrapVersion\":{},\"contractVersion\":{},\"gfortran\":{},\"openblas\":{},\"openmp\":{},\"quadmath\":{},\"systemDirectories\":[{}],\"systemctl\":{},\"systemdRun\":{},\"systemdVersion\":{},\"xgboost\":{}}}",
                file_identity_json(&manifest.bwrap),
                json_string(&manifest.bwrap_version),
                json_string(NATIVE_CONTRACT_VERSION),
                file_identity_json(&manifest.gfortran),
                file_identity_json(&manifest.openblas),
                file_identity_json(&manifest.openmp),
                file_identity_json(&manifest.quadmath),
                manifest
                    .system_directories
                    .iter()
                    .map(directory_identity_json)
                    .collect::<Vec<_>>()
                    .join(","),
                file_identity_json(&manifest.systemctl),
                file_identity_json(&manifest.systemd_run),
                json_string(&manifest.systemd_version),
                file_identity_json(&manifest.xgboost),
            ),
        }
    }
}

/// Complete source closure and fixed host readiness observation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedNativeMaintenanceFitRuntimeReadiness {
    pub installed_closure: VerifiedNativeMaintenanceInstalledClosure,
    pub manifest: NativeMaintenanceFitRuntimeManifest,
    pub readiness_sha256: String,
}

/// One shell-free process request with an optional exact environment.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeMaintenanceFitRuntimeCommand {
    pub executable: PathBuf,
    pub arguments: Vec<OsString>,
    pub clear_environment: bool,
    pub environment: Vec<(OsString, OsString)>,
}

/// Execute only fixed readiness commands through a bounded process owner.
pub trait NativeMaintenanceFitRuntimeCommandRunner {
    /// Return successful stdout only after enforcing the request bounds.
    fn run(
        &mut self,
        command: &NativeMaintenanceFitRuntimeCommand,
    ) -> Result<Vec<u8>, NativeMaintenanceFitRuntimeReadinessError>;
}

/// Production shell-free fixed-command runner.
#[derive(Clone, Copy, Debug, Default)]
pub struct NativeMaintenanceFitRuntimeProcess;

impl NativeMaintenanceFitRuntimeCommandRunner for NativeMaintenanceFitRuntimeProcess {
    /// Run one fixed executable with bounded stdout, stderr, and wall time.
    fn run(
        &mut self,
        request: &NativeMaintenanceFitRuntimeCommand,
    ) -> Result<Vec<u8>, NativeMaintenanceFitRuntimeReadinessError> {
        let mut command = Command::new(&request.executable);
        command
            .args(&request.arguments)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        // retain the source's isolated Python import environment only for that request
        if request.clear_environment {
            command.env_clear();
        }
        command.envs(request.environment.iter().cloned());
        let mut child = command
            .spawn()
            .map_err(|_| NativeMaintenanceFitRuntimeReadinessError::Process)?;
        let stdout = child
            .stdout
            .take()
            .ok_or(NativeMaintenanceFitRuntimeReadinessError::Process)?;
        let stderr = child
            .stderr
            .take()
            .ok_or(NativeMaintenanceFitRuntimeReadinessError::Process)?;
        let stdout_reader = thread::spawn(move || read_bounded(stdout));
        let stderr_reader = thread::spawn(move || read_bounded(stderr));
        let deadline = Instant::now() + COMMAND_DEADLINE;
        let status = loop {
            // observe completion without allowing one fixed probe to hang readiness
            if let Some(status) = child
                .try_wait()
                .map_err(|_| NativeMaintenanceFitRuntimeReadinessError::Process)?
            {
                break status;
            }
            // terminate only this fixed child after the predecessor's deadline
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                let _ = stdout_reader.join();
                let _ = stderr_reader.join();
                return Err(NativeMaintenanceFitRuntimeReadinessError::Process);
            }
            thread::sleep(COMMAND_POLL_INTERVAL);
        };
        let stdout = join_bounded(stdout_reader)?;
        let _stderr = join_bounded(stderr_reader)?;
        // refuse nonzero fixed probes without retaining their output
        if !status.success() {
            return Err(NativeMaintenanceFitRuntimeReadinessError::Process);
        }
        Ok(stdout)
    }
}

#[derive(Clone, Copy)]
enum NumericalRootAuthority {
    Production,
    #[cfg(test)]
    Fixture,
}

/// Explicit isolated runtime paths used only by readiness fixtures.
#[cfg(test)]
pub(crate) struct NativeMaintenanceFitRuntimeFixturePaths {
    pub(crate) bwrap: PathBuf,
    pub(crate) node: PathBuf,
    pub(crate) python: PathBuf,
    pub(crate) python_cellar_lib: PathBuf,
    pub(crate) python_stdlib: PathBuf,
    pub(crate) linuxbrew_prefix: PathBuf,
    pub(crate) linuxbrew_lib: PathBuf,
    pub(crate) numerical_site_packages: PathBuf,
    pub(crate) system_directories: Vec<PathBuf>,
    pub(crate) systemctl: PathBuf,
    pub(crate) systemd_run: PathBuf,
    pub(crate) uid: u32,
}

#[derive(Clone)]
pub(crate) struct NativeMaintenanceFitRuntimePaths {
    bwrap: PathBuf,
    node: PathBuf,
    python: PathBuf,
    python_cellar_lib: PathBuf,
    python_stdlib: PathBuf,
    linuxbrew_prefix: PathBuf,
    linuxbrew_lib: PathBuf,
    numerical_site_packages: PathBuf,
    system_directories: Vec<PathBuf>,
    systemctl: PathBuf,
    systemd_run: PathBuf,
    system_uid: u32,
    user_uid: u32,
    numerical_authority: NumericalRootAuthority,
}

impl NativeMaintenanceFitRuntimePaths {
    /// Select the source's literal reviewed production runtime.
    fn production() -> Self {
        Self {
            bwrap: PathBuf::from(BWRAP_PATH),
            node: PathBuf::from(NODE_PATH),
            python: PathBuf::from(PYTHON_PATH),
            python_cellar_lib: PathBuf::from(PYTHON_CELLAR_LIB),
            python_stdlib: PathBuf::from(PYTHON_STDLIB),
            linuxbrew_prefix: PathBuf::from(LINUXBREW_PREFIX),
            linuxbrew_lib: PathBuf::from(LINUXBREW_LIB),
            numerical_site_packages: PathBuf::from(NUMERICAL_RUNTIME_ROOT),
            system_directories: vec![
                PathBuf::from("/usr"),
                PathBuf::from("/usr/lib"),
                PathBuf::from("/usr/lib64"),
            ],
            systemctl: PathBuf::from(SYSTEMCTL_PATH),
            systemd_run: PathBuf::from(SYSTEMD_RUN_PATH),
            system_uid: 0,
            user_uid: unsafe { libc::getuid() },
            numerical_authority: NumericalRootAuthority::Production,
        }
    }

    /// Bind isolated paths without weakening the production constructor.
    #[cfg(test)]
    pub(crate) fn fixture(fixture: NativeMaintenanceFitRuntimeFixturePaths) -> Self {
        let NativeMaintenanceFitRuntimeFixturePaths {
            bwrap,
            node,
            python,
            python_cellar_lib,
            python_stdlib,
            linuxbrew_prefix,
            linuxbrew_lib,
            numerical_site_packages,
            system_directories,
            systemctl,
            systemd_run,
            uid,
        } = fixture;
        Self {
            bwrap,
            node,
            python,
            python_cellar_lib,
            python_stdlib,
            linuxbrew_prefix,
            linuxbrew_lib,
            numerical_site_packages,
            system_directories,
            systemctl,
            systemd_run,
            system_uid: uid,
            user_uid: uid,
            numerical_authority: NumericalRootAuthority::Fixture,
        }
    }
}

/// Actual distinct source installed-generation closures required before numerical runtime proof.
enum InstalledFitClosure {
    NodePredecessor(NativeMaintenanceCaptureInstalledClosure),
    Native(crate::NativeMaintenanceCaptureNativeInstallation),
}

impl InstalledFitClosure {
    /// Verify the exact source closure or actual running native executable closure.
    fn verify(
        &self,
    ) -> Result<VerifiedNativeMaintenanceInstalledClosure, NativeMaintenanceFitRuntimeReadinessError>
    {
        match self {
            Self::NodePredecessor(owner) => owner
                .verify()
                .map_err(NativeMaintenanceFitRuntimeReadinessError::InstalledClosure),
            Self::Native(owner) => {
                let verified = owner
                    .verify()
                    .map_err(NativeMaintenanceFitRuntimeReadinessError::NativeInstallation)?;
                Ok(VerifiedNativeMaintenanceInstalledClosure {
                    release: verified.release,
                    fit_source_members: verified.fit_source_members,
                })
            }
        }
    }
}

/// Fixed installed closure plus host fit-runtime verifier.
pub struct NativeMaintenanceFitRuntimeReadiness<Runner> {
    installed: InstalledFitClosure,
    paths: NativeMaintenanceFitRuntimePaths,
    runner: Runner,
}

impl NativeMaintenanceFitRuntimeReadiness<NativeMaintenanceFitRuntimeProcess> {
    /// Bind one immutable predecessor release to the literal production runtime.
    pub fn native_node_predecessor(
        source_commit: &str,
    ) -> Result<Self, NativeMaintenanceFitRuntimeReadinessError> {
        let installed =
            NativeMaintenanceCaptureInstalledClosure::native_node_predecessor(source_commit)
                .map_err(NativeMaintenanceFitRuntimeReadinessError::InstalledClosure)?;
        Ok(Self {
            installed: InstalledFitClosure::NodePredecessor(installed),
            paths: NativeMaintenanceFitRuntimePaths::production(),
            runner: NativeMaintenanceFitRuntimeProcess,
        })
    }

    /// Bind the actual installed native executable generation to the same fitted-source contract.
    pub fn native_installed(
        source_commit: &str,
    ) -> Result<Self, NativeMaintenanceFitRuntimeReadinessError> {
        let installed =
            crate::NativeMaintenanceCaptureNativeInstallation::native_release(source_commit)
                .map_err(NativeMaintenanceFitRuntimeReadinessError::NativeInstallation)?;
        Ok(Self {
            installed: InstalledFitClosure::Native(installed),
            paths: NativeMaintenanceFitRuntimePaths::production(),
            runner: NativeMaintenanceFitRuntimeProcess,
        })
    }
}

impl<Runner: NativeMaintenanceFitRuntimeCommandRunner>
    NativeMaintenanceFitRuntimeReadiness<Runner>
{
    /// Re-run the complete fixed readiness observation and discard its receipt.
    pub fn verify(&mut self) -> Result<(), NativeMaintenanceFitRuntimeReadinessError> {
        self.capture().map(drop)
    }

    /// Verify installed source bytes, host identities, versions, markers, and imports.
    pub fn capture(
        &mut self,
    ) -> Result<
        VerifiedNativeMaintenanceFitRuntimeReadiness,
        NativeMaintenanceFitRuntimeReadinessError,
    > {
        let installed_closure = self.installed.verify()?;
        // the native workstation uses the existing x86-64 numerical library directly
        if installed_closure.release.generation
            == crate::NativeMaintenanceCaptureReleaseGeneration::Native
        {
            let manifest = self.capture_native_manifest()?;
            let readiness_sha256 = sha256(manifest.canonical_json().as_bytes());
            return Ok(VerifiedNativeMaintenanceFitRuntimeReadiness {
                installed_closure,
                manifest,
                readiness_sha256,
            });
        }
        verify_numerical_root(&self.paths)?;
        let versions = fixed_runtime_versions(&self.paths, &mut self.runner)?;
        let manifest = NativeMaintenanceFitRuntimeManifest::Predecessor(Box::new(
            NativeMaintenancePredecessorFitRuntimeManifest {
                bwrap: file_identity(&self.paths.bwrap, self.paths.system_uid)?,
                contract_version: CONTRACT_VERSION.to_owned(),
                linuxbrew_lib: tree_identity(
                    &self.paths.linuxbrew_lib,
                    TreePolicy::links(&self.paths.linuxbrew_prefix),
                    self.paths.user_uid,
                )?,
                node: file_identity(&self.paths.node, self.paths.user_uid)?,
                numerical_site_packages: tree_identity(
                    &self.paths.numerical_site_packages,
                    TreePolicy::private_hard_links(),
                    self.paths.user_uid,
                )?,
                python: file_identity(&self.paths.python, self.paths.user_uid)?,
                python_cellar_lib: tree_identity(
                    &self.paths.python_cellar_lib,
                    TreePolicy::links(&self.paths.linuxbrew_prefix),
                    self.paths.user_uid,
                )?,
                python_stdlib: tree_identity(
                    &self.paths.python_stdlib,
                    TreePolicy::plain(),
                    self.paths.user_uid,
                )?,
                system_directories: self
                    .paths
                    .system_directories
                    .iter()
                    .map(|path| directory_identity(path, self.paths.system_uid))
                    .collect::<Result<Vec<_>, _>>()?,
                systemctl: file_identity(&self.paths.systemctl, self.paths.system_uid)?,
                systemd_run: file_identity(&self.paths.systemd_run, self.paths.system_uid)?,
                versions,
            },
        ));
        validate_numerical_package_markers(&self.paths.numerical_site_packages)?;
        validate_numerical_runtime_imports(&self.paths, &mut self.runner)?;
        let readiness_sha256 = sha256(manifest.canonical_json().as_bytes());
        Ok(VerifiedNativeMaintenanceFitRuntimeReadiness {
            installed_closure,
            manifest,
            readiness_sha256,
        })
    }

    /// Observe only the native sandbox controls and pinned preexisting numerical libraries.
    fn capture_native_manifest(
        &mut self,
    ) -> Result<NativeMaintenanceFitRuntimeManifest, NativeMaintenanceFitRuntimeReadinessError>
    {
        use NativeMaintenanceFitRuntimeReadinessError as Error;
        // the reviewed workstation library is x86-64 and never a Blueberry ARM64 process dependency
        if !cfg!(target_arch = "x86_64") {
            return Err(Error::Version);
        }
        let xgboost = file_identity(Path::new(XGBOOST_NATIVE_LIBRARY), self.paths.user_uid)?;
        let openmp = file_identity(Path::new(OPENMP_NATIVE_LIBRARY), self.paths.user_uid)?;
        let openblas = file_identity(Path::new(OPENBLAS_NATIVE_LIBRARY), self.paths.user_uid)?;
        let gfortran = file_identity(Path::new(GFORTRAN_NATIVE_LIBRARY), self.paths.user_uid)?;
        let quadmath = file_identity(Path::new(QUADMATH_NATIVE_LIBRARY), self.paths.user_uid)?;
        // require the actual preexisting wheel library identities without importing Python or NumPy
        if xgboost.sha256 != XGBOOST_NATIVE_SHA256
            || xgboost.size != 22_296_529
            || openmp.sha256 != OPENMP_NATIVE_SHA256
            || openmp.size != 253_289
            || openblas.sha256 != OPENBLAS_NATIVE_SHA256
            || openblas.size != 24_325_657
            || gfortran.sha256 != GFORTRAN_NATIVE_SHA256
            || gfortran.size != 2_862_249
            || quadmath.sha256 != QUADMATH_NATIVE_SHA256
            || quadmath.size != 275_553
        {
            return Err(Error::Version);
        }
        native_fit_runtime_manifest(
            &self.paths,
            &mut self.runner,
            xgboost,
            openmp,
            openblas,
            gfortran,
            quadmath,
        )
    }

    /// Bind disposable paths and a nonprocess runner for ordinary unit tests.
    #[cfg(test)]
    pub(crate) fn for_fixture(
        release_root: PathBuf,
        paths: NativeMaintenanceFitRuntimePaths,
        runner: Runner,
    ) -> Self {
        Self {
            installed: InstalledFitClosure::NodePredecessor(
                NativeMaintenanceCaptureInstalledClosure::for_test_release_root(release_root),
            ),
            paths,
            runner,
        }
    }
}

#[derive(Clone, Copy)]
struct TreePolicy<'a> {
    allow_hard_links: bool,
    allowed_link_prefix: Option<&'a Path>,
    owner_private: bool,
}

impl<'a> TreePolicy<'a> {
    /// Select a tree without links or private-mode requirements.
    const fn plain() -> Self {
        Self {
            allow_hard_links: false,
            allowed_link_prefix: None,
            owner_private: false,
        }
    }

    /// Select one Linuxbrew tree with reviewed in-prefix links.
    const fn links(prefix: &'a Path) -> Self {
        Self {
            allow_hard_links: false,
            allowed_link_prefix: Some(prefix),
            owner_private: false,
        }
    }

    /// Select the private numerical tree while retaining package hard links.
    const fn private_hard_links() -> Self {
        Self {
            allow_hard_links: true,
            allowed_link_prefix: None,
            owner_private: true,
        }
    }
}

enum TreeRecord {
    Directory {
        mode: u32,
        path: String,
    },
    File {
        mode: u32,
        nlink: u64,
        path: String,
        sha256: String,
        size: u64,
    },
    Link {
        mode: u32,
        path: String,
        target: String,
    },
}

impl TreeRecord {
    /// Borrow the relative path used for source lexical ordering.
    fn path(&self) -> &str {
        match self {
            Self::Directory { path, .. } | Self::File { path, .. } | Self::Link { path, .. } => {
                path
            }
        }
    }

    /// Encode one recursively sorted canonical record.
    fn canonical_json(&self) -> String {
        match self {
            Self::Directory { mode, path } => format!(
                "{{\"kind\":\"directory\",\"mode\":{mode},\"path\":{}}}",
                json_string(path),
            ),
            Self::File {
                mode,
                nlink,
                path,
                sha256,
                size,
            } => format!(
                concat!(
                    "{{\"kind\":\"file\",\"mode\":{mode},\"nlink\":{nlink},",
                    "\"path\":{},\"sha256\":{},\"size\":{size}}}"
                ),
                json_string(path),
                json_string(sha256),
                mode = mode,
                nlink = nlink,
                size = size,
            ),
            Self::Link { mode, path, target } => format!(
                concat!(
                    "{{\"kind\":\"link\",\"mode\":{mode},\"path\":{},",
                    "\"target\":{}}}"
                ),
                json_string(path),
                json_string(target),
                mode = mode,
            ),
        }
    }
}

/// Require the production numerical path chain or an explicitly injected test fixture.
fn verify_numerical_root(
    paths: &NativeMaintenanceFitRuntimePaths,
) -> Result<(), NativeMaintenanceFitRuntimeReadinessError> {
    // production accepts only the literal reviewed private ancestor chain
    if matches!(
        paths.numerical_authority,
        NumericalRootAuthority::Production
    ) {
        let expected = Path::new(NUMERICAL_RUNTIME_ROOT);
        if paths.numerical_site_packages != expected {
            return Err(NativeMaintenanceFitRuntimeReadinessError::NumericalRoot);
        }
        let ancestors = [
            Path::new("/home/ubuntu/.weather"),
            Path::new("/home/ubuntu/.weather/research-runtimes"),
            Path::new("/home/ubuntu/.weather/research-runtimes/xgboost-cpu-3.4.1"),
            Path::new("/home/ubuntu/.weather/research-runtimes/xgboost-cpu-3.4.1/lib"),
            Path::new("/home/ubuntu/.weather/research-runtimes/xgboost-cpu-3.4.1/lib/python3.14"),
            expected,
        ];
        // retain every exact owner-private production ancestor
        for ancestor in ancestors {
            let metadata = fs::symlink_metadata(ancestor)
                .map_err(|_| NativeMaintenanceFitRuntimeReadinessError::NumericalRoot)?;
            if !metadata.is_dir()
                || metadata.file_type().is_symlink()
                || metadata.uid() != paths.user_uid
                || metadata.mode() & 0o777 != 0o700
                || fs::canonicalize(ancestor).ok().as_deref() != Some(ancestor)
            {
                return Err(NativeMaintenanceFitRuntimeReadinessError::NumericalRoot);
            }
        }
    }
    Ok(())
}

/// Read and admit the exact fixed tool versions.
fn fixed_runtime_versions<Runner: NativeMaintenanceFitRuntimeCommandRunner>(
    paths: &NativeMaintenanceFitRuntimePaths,
    runner: &mut Runner,
) -> Result<NativeMaintenanceFitRuntimeVersions, NativeMaintenanceFitRuntimeReadinessError> {
    let bwrap = version_text(runner, &paths.bwrap)?;
    let node = version_text(runner, &paths.node)?;
    let python = version_text(runner, &paths.python)?;
    let systemd = version_text(runner, &paths.systemd_run)?;
    // reject substitutions before hashing the user-owned numerical tree
    if bwrap != "bubblewrap 0.11.1"
        || node != "v24.16.0"
        || python != "Python 3.14.7"
        || !systemd.starts_with("systemd 259 ")
    {
        return Err(NativeMaintenanceFitRuntimeReadinessError::Version);
    }
    Ok(NativeMaintenanceFitRuntimeVersions {
        bwrap,
        node,
        numpy: "2.5.3".to_owned(),
        python,
        systemd,
        xgboost: "3.4.1".to_owned(),
    })
}

/// Build the native control manifest after the caller has admitted exact numerical libraries.
pub(crate) fn native_fit_runtime_manifest<Runner: NativeMaintenanceFitRuntimeCommandRunner>(
    paths: &NativeMaintenanceFitRuntimePaths,
    runner: &mut Runner,
    xgboost: NativeMaintenanceFitRuntimeFileIdentity,
    openmp: NativeMaintenanceFitRuntimeFileIdentity,
    openblas: NativeMaintenanceFitRuntimeFileIdentity,
    gfortran: NativeMaintenanceFitRuntimeFileIdentity,
    quadmath: NativeMaintenanceFitRuntimeFileIdentity,
) -> Result<NativeMaintenanceFitRuntimeManifest, NativeMaintenanceFitRuntimeReadinessError> {
    let bwrap_version = version_text(runner, &paths.bwrap)?;
    let systemd_version = version_text(runner, &paths.systemd_run)?;
    // retain only the original fixed service-manager and sandbox versions
    if bwrap_version != "bubblewrap 0.11.1" || !systemd_version.starts_with("systemd 259 ") {
        return Err(NativeMaintenanceFitRuntimeReadinessError::Version);
    }
    Ok(NativeMaintenanceFitRuntimeManifest::Native(Box::new(
        NativeMaintenanceNativeFitRuntimeManifest {
            bwrap: file_identity(&paths.bwrap, paths.system_uid)?,
            bwrap_version,
            system_directories: paths
                .system_directories
                .iter()
                .map(|path| directory_identity(path, paths.system_uid))
                .collect::<Result<Vec<_>, _>>()?,
            systemctl: file_identity(&paths.systemctl, paths.system_uid)?,
            systemd_run: file_identity(&paths.systemd_run, paths.system_uid)?,
            systemd_version,
            xgboost,
            openmp,
            openblas,
            gfortran,
            quadmath,
        },
    )))
}

/// Run one no-shell version command and trim its bounded line.
fn version_text<Runner: NativeMaintenanceFitRuntimeCommandRunner>(
    runner: &mut Runner,
    executable: &Path,
) -> Result<String, NativeMaintenanceFitRuntimeReadinessError> {
    let bytes = runner.run(&NativeMaintenanceFitRuntimeCommand {
        executable: executable.to_owned(),
        arguments: vec![OsString::from("--version")],
        clear_environment: false,
        environment: Vec::new(),
    })?;
    String::from_utf8(bytes)
        .map(|value| value.trim().to_owned())
        .map_err(|_| NativeMaintenanceFitRuntimeReadinessError::Version)
}

/// Inspect one exact trusted directory without following aliases.
fn directory_identity(
    path: &Path,
    expected_uid: u32,
) -> Result<NativeMaintenanceFitRuntimeDirectoryIdentity, NativeMaintenanceFitRuntimeReadinessError>
{
    let metadata = fs::symlink_metadata(path)
        .map_err(|_| NativeMaintenanceFitRuntimeReadinessError::Directory)?;
    // reject aliases, writable roots, and foreign owners
    if !metadata.is_dir()
        || metadata.file_type().is_symlink()
        || metadata.uid() != expected_uid
        || metadata.mode() & 0o022 != 0
        || fs::canonicalize(path).ok().as_deref() != Some(path)
    {
        return Err(NativeMaintenanceFitRuntimeReadinessError::Directory);
    }
    Ok(NativeMaintenanceFitRuntimeDirectoryIdentity {
        gid: metadata.gid(),
        mode: metadata.mode() & 0o777,
        path: path_text(path)?,
        uid: metadata.uid(),
    })
}

/// Inspect and hash one exact single-link trusted regular file.
fn file_identity(
    path: &Path,
    expected_uid: u32,
) -> Result<NativeMaintenanceFitRuntimeFileIdentity, NativeMaintenanceFitRuntimeReadinessError> {
    let before =
        fs::symlink_metadata(path).map_err(|_| NativeMaintenanceFitRuntimeReadinessError::File)?;
    let mut file =
        open_read_no_follow(path).map_err(|_| NativeMaintenanceFitRuntimeReadinessError::File)?;
    let opened = file
        .metadata()
        .map_err(|_| NativeMaintenanceFitRuntimeReadinessError::File)?;
    // bind owner, mode, link count, inode, and literal canonical path
    if !stable_file(&before, &opened)
        || before.uid() != expected_uid
        || before.nlink() != 1
        || before.mode() & 0o022 != 0
        || fs::canonicalize(path).ok().as_deref() != Some(path)
    {
        return Err(NativeMaintenanceFitRuntimeReadinessError::File);
    }
    let digest = hash_opened_file(&mut file, &opened)
        .map_err(|_| NativeMaintenanceFitRuntimeReadinessError::File)?;
    Ok(NativeMaintenanceFitRuntimeFileIdentity {
        gid: before.gid(),
        mode: before.mode() & 0o777,
        path: path_text(path)?,
        sha256: digest,
        size: before.size(),
        uid: before.uid(),
    })
}

/// Hash one complete tree with source-compatible canonical records.
fn tree_identity(
    root: &Path,
    policy: TreePolicy<'_>,
    expected_uid: u32,
) -> Result<NativeMaintenanceFitRuntimeTreeIdentity, NativeMaintenanceFitRuntimeReadinessError> {
    let root_metadata =
        fs::symlink_metadata(root).map_err(|_| NativeMaintenanceFitRuntimeReadinessError::Tree)?;
    // reject root aliases and foreign or broad private roots
    if !root_metadata.is_dir()
        || root_metadata.file_type().is_symlink()
        || root_metadata.uid() != expected_uid
        || (policy.owner_private && root_metadata.mode() & 0o077 != 0)
        || fs::canonicalize(root).ok().as_deref() != Some(root)
    {
        return Err(NativeMaintenanceFitRuntimeReadinessError::Tree);
    }
    let mut records = Vec::new();
    let mut pending = vec![root.to_owned()];
    let mut file_count = 0_u64;
    let mut total_bytes = 0_u64;
    // walk literal entries without following directory links
    while let Some(directory) = pending.pop() {
        let mut entries = fs::read_dir(&directory)
            .map_err(|_| NativeMaintenanceFitRuntimeReadinessError::Tree)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| NativeMaintenanceFitRuntimeReadinessError::Tree)?;
        entries.sort_by_key(fs::DirEntry::file_name);
        // retain every child through lstat rather than dirent hints
        for entry in entries {
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path)
                .map_err(|_| NativeMaintenanceFitRuntimeReadinessError::Tree)?;
            let relative = relative_text(root, &path)?;
            // every runtime node stays under the same service owner
            if metadata.uid() != expected_uid {
                return Err(NativeMaintenanceFitRuntimeReadinessError::Tree);
            }
            // retain directories for full mode hashing
            if metadata.is_dir() && !metadata.file_type().is_symlink() {
                if policy.owner_private && metadata.mode() & 0o077 != 0 {
                    return Err(NativeMaintenanceFitRuntimeReadinessError::Tree);
                }
                records.push(TreeRecord::Directory {
                    mode: metadata.mode() & 0o777,
                    path: relative,
                });
                pending.push(path);
                continue;
            }
            // hash regular files through a no-follow descriptor
            if metadata.is_file() && !metadata.file_type().is_symlink() {
                if (metadata.nlink() != 1 && !policy.allow_hard_links)
                    || (policy.owner_private && metadata.mode() & 0o066 != 0)
                {
                    return Err(NativeMaintenanceFitRuntimeReadinessError::Tree);
                }
                let digest = hash_regular_path(&path, &metadata)?;
                records.push(TreeRecord::File {
                    mode: metadata.mode() & 0o777,
                    nlink: metadata.nlink(),
                    path: relative,
                    sha256: digest,
                    size: metadata.size(),
                });
                file_count = file_count
                    .checked_add(1)
                    .ok_or(NativeMaintenanceFitRuntimeReadinessError::Tree)?;
                total_bytes = total_bytes
                    .checked_add(metadata.size())
                    .ok_or(NativeMaintenanceFitRuntimeReadinessError::Tree)?;
                continue;
            }
            // retain only reviewed Linuxbrew-prefix links
            if metadata.file_type().is_symlink() {
                let Some(prefix) = policy.allowed_link_prefix else {
                    return Err(NativeMaintenanceFitRuntimeReadinessError::Tree);
                };
                let target = fs::read_link(&path)
                    .map_err(|_| NativeMaintenanceFitRuntimeReadinessError::Tree)?;
                let resolved = fs::canonicalize(&path)
                    .map_err(|_| NativeMaintenanceFitRuntimeReadinessError::Tree)?;
                let system_loader = path == prefix.join("lib/ld.so")
                    && target == Path::new("/lib64/ld-linux-x86-64.so.2")
                    && resolved == Path::new("/usr/lib/x86_64-linux-gnu/ld-linux-x86-64.so.2");
                if !system_loader && !resolved.starts_with(prefix) {
                    return Err(NativeMaintenanceFitRuntimeReadinessError::Tree);
                }
                records.push(TreeRecord::Link {
                    mode: metadata.mode() & 0o777,
                    path: relative,
                    target: path_text(&target)?,
                });
                continue;
            }
            return Err(NativeMaintenanceFitRuntimeReadinessError::Tree);
        }
    }
    // fail closed for a path outside the independently proved Node ASCII collation profile
    if records.iter().any(|record| {
        !crate::native_fit_ascii_collation::native_fit_collation_admits(record.path())
    }) {
        return Err(NativeMaintenanceFitRuntimeReadinessError::Tree);
    }
    records.sort_by(|left, right| {
        crate::native_fit_ascii_collation::native_fit_collation_cmp(left.path(), right.path())
    });
    let records_json = format!(
        "[{}]",
        records
            .iter()
            .map(TreeRecord::canonical_json)
            .collect::<Vec<_>>()
            .join(",")
    );
    Ok(NativeMaintenanceFitRuntimeTreeIdentity {
        file_count,
        gid: root_metadata.gid(),
        mode: root_metadata.mode() & 0o777,
        path: path_text(root)?,
        total_bytes,
        tree_sha256: sha256(records_json.as_bytes()),
        uid: root_metadata.uid(),
    })
}

/// Require the exact pinned numerical marker set and prohibit import hooks.
fn validate_numerical_package_markers(
    root: &Path,
) -> Result<(), NativeMaintenanceFitRuntimeReadinessError> {
    let entries = fs::read_dir(root)
        .map_err(|_| NativeMaintenanceFitRuntimeReadinessError::Markers)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| NativeMaintenanceFitRuntimeReadinessError::Markers)?;
    let names = entries
        .iter()
        .map(|entry| entry.file_name().into_string())
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| NativeMaintenanceFitRuntimeReadinessError::Markers)?;
    let numpy = names
        .iter()
        .filter(|name| name.starts_with("numpy-") && name.ends_with(".dist-info"))
        .collect::<Vec<_>>();
    let xgboost = names
        .iter()
        .filter(|name| {
            (name.starts_with("xgboost-") || name.starts_with("xgboost_cpu-"))
                && name.ends_with(".dist-info")
        })
        .collect::<Vec<_>>();
    let injected = names.iter().any(|name| {
        (name.ends_with(".pth") && name != "_virtualenv.pth")
            || matches!(name.as_str(), "sitecustomize.py" | "usercustomize.py")
    });
    // retain the fixed single NumPy and XGBoost marker identities
    if numpy.len() != 1
        || numpy[0].as_str() != "numpy-2.5.3.dist-info"
        || xgboost.len() != 1
        || xgboost[0].as_str() != "xgboost_cpu-3.4.1.dist-info"
        || injected
    {
        return Err(NativeMaintenanceFitRuntimeReadinessError::Markers);
    }
    // reject marker substitution by files or links
    for marker in [numpy[0], xgboost[0]] {
        let metadata = fs::symlink_metadata(root.join(marker))
            .map_err(|_| NativeMaintenanceFitRuntimeReadinessError::Markers)?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(NativeMaintenanceFitRuntimeReadinessError::Markers);
        }
    }
    Ok(())
}

/// Import the fixed numerical packages through the pinned isolated Python.
fn validate_numerical_runtime_imports<Runner: NativeMaintenanceFitRuntimeCommandRunner>(
    paths: &NativeMaintenanceFitRuntimePaths,
    runner: &mut Runner,
) -> Result<(), NativeMaintenanceFitRuntimeReadinessError> {
    let root = path_text(&paths.numerical_site_packages)?;
    let root_json = json_string(&root);
    let source = format!(
        "import sys;sys.path.insert(0, {root_json});import numpy,xgboost;print(numpy.__version__ + '\\n' + xgboost.__version__)"
    );
    let output = runner.run(&NativeMaintenanceFitRuntimeCommand {
        executable: paths.python.clone(),
        arguments: vec![
            OsString::from("-I"),
            OsString::from("-S"),
            OsString::from("-c"),
            OsString::from(source),
        ],
        clear_environment: true,
        environment: vec![
            (OsString::from("HOME"), OsString::from("/home/sandbox")),
            (OsString::from("LC_ALL"), OsString::from("C.UTF-8")),
            (OsString::from("PATH"), OsString::from("/usr/bin:/bin")),
            (
                OsString::from("PYTHONDONTWRITEBYTECODE"),
                OsString::from("1"),
            ),
            (OsString::from("TZ"), OsString::from("UTC")),
        ],
    })?;
    // reject marker-only or import-shadowed packages
    if output != b"2.5.3\n3.4.1\n" {
        return Err(NativeMaintenanceFitRuntimeReadinessError::Import);
    }
    Ok(())
}

/// Hash one literal regular path while binding pre-open and post-read metadata.
fn hash_regular_path(
    path: &Path,
    before: &fs::Metadata,
) -> Result<String, NativeMaintenanceFitRuntimeReadinessError> {
    let mut file =
        open_read_no_follow(path).map_err(|_| NativeMaintenanceFitRuntimeReadinessError::Tree)?;
    let opened = file
        .metadata()
        .map_err(|_| NativeMaintenanceFitRuntimeReadinessError::Tree)?;
    if !stable_file(before, &opened) {
        return Err(NativeMaintenanceFitRuntimeReadinessError::Tree);
    }
    hash_opened_file(&mut file, &opened)
        .map_err(|_| NativeMaintenanceFitRuntimeReadinessError::Tree)
}

/// Stream one opened file and reject mutation during hashing.
fn hash_opened_file(file: &mut File, opened: &fs::Metadata) -> io::Result<String> {
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 256 * 1_024];
    // retain bounded chunks rather than allocating from file size
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    let after = file.metadata()?;
    // reject mutation while the descriptor was read
    if !stable_file(opened, &after) {
        return Err(io::Error::other("runtime file changed"));
    }
    Ok(lower_hex(hasher.finalize().into()))
}

/// Compare stable regular-file identity and mutation fields.
fn stable_file(left: &fs::Metadata, right: &fs::Metadata) -> bool {
    left.is_file()
        && right.is_file()
        && !left.file_type().is_symlink()
        && left.dev() == right.dev()
        && left.ino() == right.ino()
        && left.size() == right.size()
        && left.mode() == right.mode()
        && left.mtime() == right.mtime()
        && left.mtime_nsec() == right.mtime_nsec()
}

/// Read at most the source command-output ceiling plus one overflow byte.
fn read_bounded(mut reader: impl Read) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::with_capacity(COMMAND_OUTPUT_MAXIMUM_BYTES);
    reader
        .by_ref()
        .take((COMMAND_OUTPUT_MAXIMUM_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    // refuse output that crossed the fixed transport ceiling
    if bytes.len() > COMMAND_OUTPUT_MAXIMUM_BYTES {
        return Err(io::Error::other("runtime command output exceeded bound"));
    }
    Ok(bytes)
}

/// Join one bounded reader without exposing panic or output details.
fn join_bounded(
    reader: thread::JoinHandle<io::Result<Vec<u8>>>,
) -> Result<Vec<u8>, NativeMaintenanceFitRuntimeReadinessError> {
    reader
        .join()
        .map_err(|_| NativeMaintenanceFitRuntimeReadinessError::Process)?
        .map_err(|_| NativeMaintenanceFitRuntimeReadinessError::Process)
}

/// Convert one admitted absolute or relative path to its exact UTF-8 spelling.
fn path_text(path: &Path) -> Result<String, NativeMaintenanceFitRuntimeReadinessError> {
    path.to_str()
        .map(str::to_owned)
        .ok_or(NativeMaintenanceFitRuntimeReadinessError::Tree)
}

/// Convert one child path to source-style slash-separated relative text.
fn relative_text(
    root: &Path,
    path: &Path,
) -> Result<String, NativeMaintenanceFitRuntimeReadinessError> {
    path.strip_prefix(root)
        .ok()
        .and_then(Path::to_str)
        .map(str::to_owned)
        .ok_or(NativeMaintenanceFitRuntimeReadinessError::Tree)
}

/// Encode one string with the JSON implementation already used by the runtime.
fn json_string(value: &str) -> String {
    serde_json::to_string(value).expect("string JSON serialization is infallible")
}

/// Encode one source-sorted file identity object.
fn file_identity_json(value: &NativeMaintenanceFitRuntimeFileIdentity) -> String {
    format!(
        concat!(
            "{{\"gid\":{},\"mode\":{},\"path\":{},\"sha256\":{},",
            "\"size\":{},\"uid\":{}}}"
        ),
        value.gid,
        value.mode,
        json_string(&value.path),
        json_string(&value.sha256),
        value.size,
        value.uid,
    )
}

/// Encode one source-sorted directory identity object.
fn directory_identity_json(value: &NativeMaintenanceFitRuntimeDirectoryIdentity) -> String {
    format!(
        "{{\"gid\":{},\"mode\":{},\"path\":{},\"uid\":{}}}",
        value.gid,
        value.mode,
        json_string(&value.path),
        value.uid,
    )
}

/// Encode one source-sorted tree identity object.
fn tree_identity_json(value: &NativeMaintenanceFitRuntimeTreeIdentity) -> String {
    format!(
        concat!(
            "{{\"fileCount\":{},\"gid\":{},\"mode\":{},\"path\":{},",
            "\"totalBytes\":{},\"treeSha256\":{},\"uid\":{}}}"
        ),
        value.file_count,
        value.gid,
        value.mode,
        json_string(&value.path),
        value.total_bytes,
        json_string(&value.tree_sha256),
        value.uid,
    )
}

/// Encode one source-sorted fixed-version object.
fn versions_json(value: &NativeMaintenanceFitRuntimeVersions) -> String {
    format!(
        concat!(
            "{{\"bwrap\":{},\"node\":{},\"numpy\":{},\"python\":{},",
            "\"systemd\":{},\"xgboost\":{}}}"
        ),
        json_string(&value.bwrap),
        json_string(&value.node),
        json_string(&value.numpy),
        json_string(&value.python),
        json_string(&value.systemd),
        json_string(&value.xgboost),
    )
}

/// Hash one exact canonical byte sequence as lowercase SHA-256.
fn sha256(bytes: &[u8]) -> String {
    lower_hex(Sha256::digest(bytes).into())
}

/// Render a SHA-256 digest without formatter allocation.
fn lower_hex(digest: [u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut value = String::with_capacity(64);
    // emit every digest byte in lowercase source spelling
    for byte in digest {
        value.push(char::from(HEX[usize::from(byte >> 4)]));
        value.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    value
}
