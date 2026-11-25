use std::fs::{self, Permissions};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use sha2::{Digest, Sha256};

use crate::native_maintenance_capture_installed_closure::FIT_SOURCE_FILES;
use crate::native_maintenance_fit_runtime_readiness::{
    NativeMaintenanceFitRuntimeCommand, NativeMaintenanceFitRuntimeCommandRunner,
    NativeMaintenanceFitRuntimeFileIdentity, NativeMaintenanceFitRuntimeFixturePaths,
    NativeMaintenanceFitRuntimePaths, NativeMaintenanceFitRuntimeReadiness,
    NativeMaintenanceFitRuntimeReadinessError, native_fit_runtime_manifest,
};

const SOURCE_COMMIT: &str = "1111111111111111111111111111111111111111";
static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    root: PathBuf,
    release_root: PathBuf,
    paths: NativeMaintenanceFitRuntimePaths,
    numerical_root: PathBuf,
    bwrap: PathBuf,
}

/// Native readiness observes only its fixed controls and the admitted native libraries.
#[test]
fn native_fit_runtime_manifest_uses_no_interpreter_probes() {
    let fixture = Fixture::create();
    let (mut runner, calls) = FixtureRunner::valid();
    let library = NativeMaintenanceFitRuntimeFileIdentity {
        gid: 1000,
        mode: 0o600,
        path: "/fixture/libxgboost.so".to_owned(),
        sha256: "a".repeat(64),
        size: 123,
        uid: 1000,
    };
    let manifest = native_fit_runtime_manifest(
        &fixture.paths,
        &mut runner,
        library.clone(),
        library.clone(),
        library.clone(),
        library.clone(),
        library,
    )
    .unwrap();
    assert_eq!(
        manifest.contract_version(),
        "weather-native-adjustment-fit-runtime-readiness/v1"
    );
    let json = manifest.canonical_json();
    assert!(json.contains("\"xgboost\":"));
    assert!(json.contains("\"openblas\":"));
    assert!(json.contains("\"gfortran\":"));
    assert!(json.contains("\"quadmath\":"));
    assert!(!json.contains("\"python\":"));
    assert!(!json.contains("\"node\":"));
    let calls = calls.lock().unwrap();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].executable.file_name().unwrap(), "bwrap");
    assert_eq!(calls[1].executable.file_name().unwrap(), "systemd-run");
}

impl Fixture {
    /// Materialize one complete installed closure and fixed runtime tree.
    fn create() -> Self {
        let sequence = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "weather-fit-runtime-readiness-{}-{sequence}",
            std::process::id(),
        ));
        fs::create_dir(&root).expect("create fixture root");
        let release_root = create_installed_closure(&root);
        let runtime = root.join("runtime");
        let bwrap = runtime.join("usr-bin/bwrap");
        let systemctl = runtime.join("usr-bin/systemctl");
        let systemd_run = runtime.join("usr-bin/systemd-run");
        let linuxbrew_prefix = runtime.join("linuxbrew");
        let linuxbrew_lib = linuxbrew_prefix.join("lib");
        let python_root = linuxbrew_prefix.join("Cellar/python@3.14/3.14.7");
        let python = python_root.join("bin/python3.14");
        let python_cellar_lib = python_root.join("lib");
        let python_stdlib = python_cellar_lib.join("python3.14");
        let node = runtime.join("node/bin/node");
        let numerical_root = runtime.join("numerical-site-packages");
        let system_directories = [
            runtime.join("system/usr"),
            runtime.join("system/usr-lib"),
            runtime.join("system/usr-lib64"),
        ];

        // create every fixed runtime executable as one immutable regular file
        for path in [&bwrap, &systemctl, &systemd_run, &python, &node] {
            write_mode(path, b"fixture executable\n", 0o500);
        }
        write_mode(&linuxbrew_lib.join("libfixture.so"), b"linuxbrew\n", 0o400);
        write_mode(&python_stdlib.join("os.py"), b"stdlib\n", 0o400);
        // create exact private numerical packages and markers
        for directory in [
            numerical_root.clone(),
            numerical_root.join("numpy"),
            numerical_root.join("xgboost"),
            numerical_root.join("numpy-2.5.3.dist-info"),
            numerical_root.join("xgboost_cpu-3.4.1.dist-info"),
        ] {
            fs::create_dir_all(&directory).expect("create numerical directory");
            fs::set_permissions(&directory, Permissions::from_mode(0o700))
                .expect("make numerical directory private");
        }
        write_mode(
            &numerical_root.join("numpy/__init__.py"),
            b"__version__='2.5.3'\n",
            0o600,
        );
        write_mode(
            &numerical_root.join("xgboost/__init__.py"),
            b"__version__='3.4.1'\n",
            0o600,
        );
        // retain literal nonwritable system directory identities
        for directory in &system_directories {
            fs::create_dir_all(directory).expect("create system directory fixture");
            fs::set_permissions(directory, Permissions::from_mode(0o755))
                .expect("set system directory mode");
        }
        let uid = fs::symlink_metadata(&root)
            .expect("fixture root metadata")
            .uid();
        let paths =
            NativeMaintenanceFitRuntimePaths::fixture(NativeMaintenanceFitRuntimeFixturePaths {
                bwrap: bwrap.clone(),
                node,
                python,
                python_cellar_lib,
                python_stdlib,
                linuxbrew_prefix,
                linuxbrew_lib,
                numerical_site_packages: numerical_root.clone(),
                system_directories: system_directories.into_iter().collect(),
                systemctl,
                systemd_run,
                uid,
            });
        Self {
            root,
            release_root,
            paths,
            numerical_root,
            bwrap,
        }
    }
}

impl Drop for Fixture {
    /// Thaw and remove only this process-local fixture.
    fn drop(&mut self) {
        thaw_tree(&self.root);
        fs::remove_dir_all(&self.root).expect("remove fit-runtime fixture");
    }
}

#[derive(Clone)]
struct FixtureRunner {
    calls: Arc<Mutex<Vec<NativeMaintenanceFitRuntimeCommand>>>,
    node_version: &'static str,
}

impl FixtureRunner {
    /// Create one successful nonprocess runner and its shared call log.
    fn valid() -> (Self, Arc<Mutex<Vec<NativeMaintenanceFitRuntimeCommand>>>) {
        let calls = Arc::new(Mutex::new(Vec::new()));
        (
            Self {
                calls: Arc::clone(&calls),
                node_version: "v24.16.0\n",
            },
            calls,
        )
    }
}

impl NativeMaintenanceFitRuntimeCommandRunner for FixtureRunner {
    /// Return source-exact output without starting a host process.
    fn run(
        &mut self,
        command: &NativeMaintenanceFitRuntimeCommand,
    ) -> Result<Vec<u8>, NativeMaintenanceFitRuntimeReadinessError> {
        self.calls
            .lock()
            .expect("lock fixture commands")
            .push(command.clone());
        let executable = command
            .executable
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or(NativeMaintenanceFitRuntimeReadinessError::Process)?;
        let first = command.arguments.first().and_then(|value| value.to_str());
        let output = match (executable, first) {
            ("bwrap", Some("--version")) => "bubblewrap 0.11.1\n",
            ("node", Some("--version")) => self.node_version,
            ("python3.14", Some("--version")) => "Python 3.14.7\n",
            ("systemd-run", Some("--version")) => "systemd 259 (259.1)\n",
            ("python3.14", Some("-I")) => "2.5.3\n3.4.1\n",
            _ => return Err(NativeMaintenanceFitRuntimeReadinessError::Process),
        };
        Ok(output.as_bytes().to_vec())
    }
}

/// Verify complete fixed identities and isolated import admission.
#[test]
fn fit_runtime_readiness_captures_source_manifest_without_host_processes() {
    let fixture = Fixture::create();
    let (runner, calls) = FixtureRunner::valid();
    let mut owner = NativeMaintenanceFitRuntimeReadiness::for_fixture(
        fixture.release_root.clone(),
        fixture.paths.clone(),
        runner,
    );
    let captured = owner.capture().expect("capture fixture runtime");
    assert_eq!(
        captured.manifest.contract_version(),
        "adjustment-fit-runtime-readiness/v1"
    );
    assert_eq!(
        captured.installed_closure.release.source_commit,
        SOURCE_COMMIT
    );
    assert_eq!(
        captured.installed_closure.fit_source_members.len(),
        FIT_SOURCE_FILES.len(),
    );
    let canonical = captured.manifest.canonical_json();
    assert!(canonical.starts_with("{\"bwrap\":"));
    assert!(canonical.contains("\"numericalSitePackages\":"));
    assert_eq!(captured.readiness_sha256, digest(canonical.as_bytes()));
    let calls = calls.lock().expect("read fixture commands");
    assert_eq!(calls.len(), 5);
    let import = calls.last().expect("isolated import command");
    assert!(import.clear_environment);
    assert_eq!(
        import
            .environment
            .iter()
            .map(|(name, _)| name.to_string_lossy().into_owned())
            .collect::<Vec<_>>(),
        ["HOME", "LC_ALL", "PATH", "PYTHONDONTWRITEBYTECODE", "TZ"],
    );
    assert_eq!(import.arguments[0], "-I");
    assert_eq!(import.arguments[1], "-S");
}

/// Refuse import hooks before the offline Python import probe.
#[test]
fn fit_runtime_readiness_refuses_numerical_path_injection_markers() {
    let fixture = Fixture::create();
    write_mode(
        &fixture.numerical_root.join("foreign.pth"),
        b"import foreign\n",
        0o600,
    );
    let (runner, calls) = FixtureRunner::valid();
    let mut owner = NativeMaintenanceFitRuntimeReadiness::for_fixture(
        fixture.release_root.clone(),
        fixture.paths.clone(),
        runner,
    );
    assert_eq!(
        owner.capture(),
        Err(NativeMaintenanceFitRuntimeReadinessError::Markers),
    );
    assert_eq!(calls.lock().expect("read fixture commands").len(), 4);
}

/// Refuse tool substitution before runtime tree traversal.
#[test]
fn fit_runtime_readiness_refuses_fixed_version_drift() {
    let fixture = Fixture::create();
    let (mut runner, calls) = FixtureRunner::valid();
    runner.node_version = "v24.17.0\n";
    let mut owner = NativeMaintenanceFitRuntimeReadiness::for_fixture(
        fixture.release_root.clone(),
        fixture.paths.clone(),
        runner,
    );
    assert_eq!(
        owner.capture(),
        Err(NativeMaintenanceFitRuntimeReadinessError::Version),
    );
    assert_eq!(calls.lock().expect("read fixture commands").len(), 4);
}

/// Refuse group-writable fixed runtime artifacts after version admission.
#[test]
fn fit_runtime_readiness_refuses_writable_runtime_file() {
    let fixture = Fixture::create();
    fs::set_permissions(&fixture.bwrap, Permissions::from_mode(0o520))
        .expect("make fixture runtime writable");
    let (runner, _) = FixtureRunner::valid();
    let mut owner = NativeMaintenanceFitRuntimeReadiness::for_fixture(
        fixture.release_root.clone(),
        fixture.paths.clone(),
        runner,
    );
    assert_eq!(
        owner.capture(),
        Err(NativeMaintenanceFitRuntimeReadinessError::File),
    );
}

/// Materialize the complete exact installed runner manifest.
fn create_installed_closure(root: &Path) -> PathBuf {
    let release_root = root.join("releases").join(SOURCE_COMMIT);
    fs::create_dir_all(&release_root).expect("create installed release root");
    let mut manifest = String::new();
    // bind every required fitter source to one immutable fixture member
    for path in FIT_SOURCE_FILES {
        let bytes = format!("fixture:{path}\n").into_bytes();
        write_mode(&release_root.join(path), &bytes, 0o400);
        manifest.push_str(&format!("{}  {path}\n", digest(&bytes)));
    }
    write_mode(
        &release_root.join("MANIFEST.sha256"),
        manifest.as_bytes(),
        0o400,
    );
    freeze_directories(&release_root);
    release_root
}

/// Write one regular fixture file with an exact mode.
fn write_mode(path: &Path, bytes: &[u8], mode: u32) {
    fs::create_dir_all(path.parent().expect("fixture file parent"))
        .expect("create fixture file parent");
    fs::write(path, bytes).expect("write fixture file");
    fs::set_permissions(path, Permissions::from_mode(mode)).expect("set fixture file mode");
}

/// Freeze every installed directory after all children exist.
fn freeze_directories(root: &Path) {
    let mut directories = Vec::new();
    collect_directories(root, &mut directories);
    directories.sort_by_key(|path| std::cmp::Reverse(path.components().count()));
    // remove owner-write permission from leaves upward
    for directory in directories {
        fs::set_permissions(directory, Permissions::from_mode(0o500))
            .expect("freeze installed directory");
    }
}

/// Collect literal fixture directories without following links.
fn collect_directories(path: &Path, directories: &mut Vec<PathBuf>) {
    directories.push(path.to_owned());
    // recurse only through owned fixture directories
    for child in fs::read_dir(path).expect("read fixture directory") {
        let child = child.expect("read fixture child");
        if child.file_type().expect("fixture child type").is_dir() {
            collect_directories(&child.path(), directories);
        }
    }
}

/// Restore fixture permissions recursively for bounded cleanup.
fn thaw_tree(path: &Path) {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return;
    };
    // restore directory traversal before visiting children
    if metadata.is_dir() {
        let _ = fs::set_permissions(path, Permissions::from_mode(0o700));
        if let Ok(children) = fs::read_dir(path) {
            // thaw every owned child before removing the root
            for child in children.flatten() {
                thaw_tree(&child.path());
            }
        }
    } else {
        let _ = fs::set_permissions(path, Permissions::from_mode(0o600));
    }
}

/// Hash one fixture byte sequence in lowercase source spelling.
fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
