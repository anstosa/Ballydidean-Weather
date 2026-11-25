use std::fs::{self, Permissions};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use sha2::{Digest, Sha256};

use crate::native_maintenance_capture_installed_closure::{
    FIT_SOURCE_FILES, NativeMaintenanceCaptureInstalledClosure,
    NativeMaintenanceCaptureInstalledClosureError,
};

const SOURCE_COMMIT: &str = "1111111111111111111111111111111111111111";
static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    root: PathBuf,
    release_root: PathBuf,
}

impl Fixture {
    /// Materialize one complete frozen source-valid installed release.
    fn create(omit: Option<&str>) -> Self {
        let sequence = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "weather-capture-installed-{}-{sequence}",
            std::process::id(),
        ));
        let release_root = root.join("releases").join(SOURCE_COMMIT);
        fs::create_dir_all(&release_root).expect("create release root");
        let mut entries = Vec::new();
        // materialize every source-required fitter member at its exact relative path
        for path in FIT_SOURCE_FILES {
            if omit == Some(*path) {
                continue;
            }
            let bytes = format!("fixture:{path}\n").into_bytes();
            write_member(&release_root, path, &bytes);
            entries.push(((*path).to_owned(), digest(&bytes)));
        }
        let manifest = entries
            .iter()
            .map(|(path, digest)| format!("{digest}  {path}\n"))
            .collect::<String>();
        let manifest_path = release_root.join("MANIFEST.sha256");
        fs::write(&manifest_path, manifest).expect("write manifest");
        fs::set_permissions(&manifest_path, Permissions::from_mode(0o400))
            .expect("freeze manifest");
        freeze_directories(&release_root);
        Self { root, release_root }
    }
}

impl Drop for Fixture {
    /// Thaw and remove only this disposable installed-release fixture.
    fn drop(&mut self) {
        thaw_tree(&self.root);
        fs::remove_dir_all(&self.root).expect("remove installed release fixture");
    }
}

/// Accept a complete frozen tree and expose every exact fitter source identity.
#[test]
fn installed_closure_verifies_complete_manifest_and_fit_sources() {
    let fixture = Fixture::create(None);
    let proof = NativeMaintenanceCaptureInstalledClosure::for_test_release_root(
        fixture.release_root.clone(),
    )
    .verify()
    .expect("verify installed closure");
    assert_eq!(proof.release.source_commit, SOURCE_COMMIT);
    assert_eq!(proof.release.member_count, FIT_SOURCE_FILES.len());
    assert_eq!(proof.fit_source_members.len(), FIT_SOURCE_FILES.len());
    assert_eq!(proof.fit_source_members[0].path, FIT_SOURCE_FILES[0]);
}

/// Refuse a manifest that omits any source-required fitter artifact.
#[test]
fn installed_closure_refuses_incomplete_fit_source_manifest() {
    let fixture = Fixture::create(Some(FIT_SOURCE_FILES[0]));
    let result = NativeMaintenanceCaptureInstalledClosure::for_test_release_root(
        fixture.release_root.clone(),
    )
    .verify();
    assert_eq!(
        result,
        Err(NativeMaintenanceCaptureInstalledClosureError::Manifest),
    );
}

/// Refuse unlisted files and bytes that drift from their immutable digest.
#[test]
fn installed_closure_refuses_tree_or_member_drift() {
    let unlisted = Fixture::create(None);
    thaw_directory(&unlisted.release_root);
    let foreign = unlisted.release_root.join("foreign.mjs");
    fs::write(&foreign, b"foreign\n").expect("write foreign member");
    fs::set_permissions(&foreign, Permissions::from_mode(0o400)).expect("freeze foreign member");
    freeze_directories(&unlisted.release_root);
    let unlisted_result = NativeMaintenanceCaptureInstalledClosure::for_test_release_root(
        unlisted.release_root.clone(),
    )
    .verify();
    assert_eq!(
        unlisted_result,
        Err(NativeMaintenanceCaptureInstalledClosureError::Closure),
    );

    let changed = Fixture::create(None);
    let member = changed.release_root.join(FIT_SOURCE_FILES[0]);
    fs::set_permissions(&member, Permissions::from_mode(0o600)).expect("thaw member");
    fs::write(&member, b"changed\n").expect("change member");
    fs::set_permissions(&member, Permissions::from_mode(0o400)).expect("refreeze member");
    let changed_result = NativeMaintenanceCaptureInstalledClosure::for_test_release_root(
        changed.release_root.clone(),
    )
    .verify();
    assert_eq!(
        changed_result,
        Err(NativeMaintenanceCaptureInstalledClosureError::Member),
    );
}

/// Write and freeze one manifest-bound regular member.
fn write_member(root: &Path, relative: &str, bytes: &[u8]) {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().expect("member parent")).expect("create member parent");
    fs::write(&path, bytes).expect("write member");
    fs::set_permissions(&path, Permissions::from_mode(0o400)).expect("freeze member");
}

/// Freeze every existing fixture directory from leaves to root.
fn freeze_directories(root: &Path) {
    let mut directories = Vec::new();
    collect_directories(root, &mut directories);
    directories.sort_by_key(|path| std::cmp::Reverse(path.components().count()));
    // remove owner-write permission only after all children are complete
    for directory in directories {
        fs::set_permissions(directory, Permissions::from_mode(0o500))
            .expect("freeze fixture directory");
    }
}

/// Collect every literal directory in one disposable fixture.
fn collect_directories(path: &Path, directories: &mut Vec<PathBuf>) {
    directories.push(path.to_owned());
    let children = fs::read_dir(path).expect("read fixture directory");
    // recurse only through directories created by this fixture
    for child in children {
        let child = child.expect("read fixture child");
        if child.file_type().expect("fixture child type").is_dir() {
            collect_directories(&child.path(), directories);
        }
    }
}

/// Restore owner access recursively so disposable cleanup can remove frozen children.
fn thaw_tree(path: &Path) {
    let Ok(details) = fs::symlink_metadata(path) else {
        return;
    };
    if details.is_dir() {
        thaw_directory(path);
        if let Ok(children) = fs::read_dir(path) {
            // thaw every owned fixture child before removing the tree
            for child in children.flatten() {
                thaw_tree(&child.path());
            }
        }
    } else {
        let _ = fs::set_permissions(path, Permissions::from_mode(0o600));
    }
}

/// Restore owner traversal and write access to one fixture directory.
fn thaw_directory(path: &Path) {
    fs::set_permissions(path, Permissions::from_mode(0o700)).expect("thaw fixture directory");
}

/// Hash one fixture member with the approved SHA-256 implementation.
fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
