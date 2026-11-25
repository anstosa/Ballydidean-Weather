use std::fs::{self, Permissions};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use sha2::{Digest, Sha256};

use crate::native_maintenance_capture_native_installation::{
    NATIVE_SOURCE_FILES, NativeMaintenanceCaptureNativeInstallation,
    NativeMaintenanceCaptureNativeInstallationError,
};
use crate::native_maintenance_capture_readiness::NativeMaintenanceCaptureReleaseGeneration;

const SOURCE_COMMIT: &str = "1111111111111111111111111111111111111111";
const EXECUTABLE_RELATIVE: &str = "bin/weather-maintenance-capture";
static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    root: PathBuf,
    release_root: PathBuf,
    executable: PathBuf,
}

impl Fixture {
    /// Materialize one complete immutable native release closure.
    fn create(include_executable_manifest: bool) -> Self {
        let sequence = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "weather-native-capture-installation-{}-{sequence}",
            std::process::id(),
        ));
        let release_root = root.join("releases").join(SOURCE_COMMIT);
        fs::create_dir_all(&release_root).expect("create native release root");
        let mut manifest = String::new();
        // bind every inherited fitter source into the native release manifest
        for path in NATIVE_SOURCE_FILES {
            let bytes = format!("fixture:{path}\n").into_bytes();
            write_member(&release_root, path, &bytes, 0o400);
            manifest.push_str(&format!("{}  {path}\n", digest(&bytes)));
        }
        // bind the two native fitter executables into the same immutable release
        for path in [
            "bin/weather-maintenance-fit",
            "bin/weather-maintenance-wind-fit",
        ] {
            let bytes = format!("fixture:{path}\n");
            write_member(&release_root, path, bytes.as_bytes(), 0o500);
            manifest.push_str(&format!("{}  {path}\n", digest(bytes.as_bytes())));
        }
        let executable = release_root.join(EXECUTABLE_RELATIVE);
        let executable_bytes = b"native maintenance capture executable\n";
        write_member(&release_root, EXECUTABLE_RELATIVE, executable_bytes, 0o500);
        // allow a focused missing-binary manifest refusal fixture
        if include_executable_manifest {
            manifest.push_str(&format!(
                "{}  {EXECUTABLE_RELATIVE}\n",
                digest(executable_bytes),
            ));
        }
        write_member(&release_root, "MANIFEST.sha256", manifest.as_bytes(), 0o400);
        freeze_directories(&release_root);
        Self {
            root,
            release_root,
            executable,
        }
    }
}

impl Drop for Fixture {
    /// Thaw and remove only this disposable native release fixture.
    fn drop(&mut self) {
        thaw_tree(&self.root);
        fs::remove_dir_all(&self.root).expect("remove native release fixture");
    }
}

/// Prove the actual executable and complete manifest as a native generation.
#[test]
fn native_installation_verifies_running_binary_and_full_release() {
    let fixture = Fixture::create(true);
    let proof = NativeMaintenanceCaptureNativeInstallation::for_fixture(
        fixture.release_root.clone(),
        fixture.executable.clone(),
    )
    .verify()
    .expect("verify native release");
    assert_eq!(
        proof.release.generation,
        NativeMaintenanceCaptureReleaseGeneration::Native,
    );
    assert_eq!(proof.release.source_commit, SOURCE_COMMIT);
    assert_eq!(proof.release.member_count, NATIVE_SOURCE_FILES.len() + 3);
    assert_eq!(proof.fit_source_members.len(), 2);
    assert_eq!(proof.executable.path, fixture.executable);
    assert_eq!(proof.executable.mode, 0o500);
    assert_eq!(
        proof.executable.sha256,
        digest(b"native maintenance capture executable\n"),
    );
}

/// Refuse a native release when the running executable is not its literal member.
#[test]
fn native_installation_refuses_nonrelease_current_executable() {
    let fixture = Fixture::create(true);
    let result = NativeMaintenanceCaptureNativeInstallation::for_fixture(
        fixture.release_root.clone(),
        fixture.root.join("other/weather-maintenance-capture"),
    )
    .verify();
    assert_eq!(
        result,
        Err(NativeMaintenanceCaptureNativeInstallationError::ExecutablePath),
    );
}

/// Refuse a binary whose immutable executable mode drifted.
#[test]
fn native_installation_refuses_nonexecutable_binary_mode() {
    let fixture = Fixture::create(true);
    thaw_directory(fixture.executable.parent().expect("binary parent"));
    fs::set_permissions(&fixture.executable, Permissions::from_mode(0o400))
        .expect("change executable mode");
    freeze_directory(fixture.executable.parent().expect("binary parent"));
    let result = NativeMaintenanceCaptureNativeInstallation::for_fixture(
        fixture.release_root.clone(),
        fixture.executable.clone(),
    )
    .verify();
    assert_eq!(
        result,
        Err(NativeMaintenanceCaptureNativeInstallationError::Executable),
    );
}

/// Refuse a full tree whose manifest does not authorize the native binary.
#[test]
fn native_installation_requires_binary_manifest_binding() {
    let fixture = Fixture::create(false);
    let result = NativeMaintenanceCaptureNativeInstallation::for_fixture(
        fixture.release_root.clone(),
        fixture.executable.clone(),
    )
    .verify();
    assert_eq!(
        result,
        Err(NativeMaintenanceCaptureNativeInstallationError::Manifest),
    );
}

/// Write one exact manifest member and role-specific immutable mode.
fn write_member(root: &Path, relative: &str, bytes: &[u8], mode: u32) {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().expect("member parent")).expect("create member parent");
    fs::write(&path, bytes).expect("write member");
    fs::set_permissions(&path, Permissions::from_mode(mode)).expect("freeze member");
}

/// Freeze every release directory from leaves to root.
fn freeze_directories(root: &Path) {
    let mut directories = Vec::new();
    collect_directories(root, &mut directories);
    directories.sort_by_key(|path| std::cmp::Reverse(path.components().count()));
    // remove owner-write permission only after all children exist
    for directory in directories {
        freeze_directory(&directory);
    }
}

/// Collect every literal directory in one owned fixture.
fn collect_directories(path: &Path, directories: &mut Vec<PathBuf>) {
    directories.push(path.to_owned());
    // recurse only through fixture-created directories
    for child in fs::read_dir(path).expect("read fixture directory") {
        let child = child.expect("read fixture child");
        if child.file_type().expect("fixture child type").is_dir() {
            collect_directories(&child.path(), directories);
        }
    }
}

/// Restore owner traversal and write access to one fixture directory.
fn thaw_directory(path: &Path) {
    fs::set_permissions(path, Permissions::from_mode(0o700)).expect("thaw fixture directory");
}

/// Restore the exact immutable release-directory mode.
fn freeze_directory(path: &Path) {
    fs::set_permissions(path, Permissions::from_mode(0o500)).expect("freeze fixture directory");
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
            // thaw every owned fixture child before removing the root
            for child in children.flatten() {
                thaw_tree(&child.path());
            }
        }
    } else {
        let _ = fs::set_permissions(path, Permissions::from_mode(0o600));
    }
}

/// Hash one fixture member in lowercase source spelling.
fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
