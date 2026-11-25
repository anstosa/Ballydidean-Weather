use std::collections::BTreeMap;
use std::fs::{self, DirBuilder};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use weather_contracts::JsonValue;
use weather_edge::ArchiveWirePrimitives;

use super::{
    COLD_ARCHIVE_DIRECTORIES, CaptureEpochPublicGitJson, SOURCE_COMMIT, SOURCE_RELEASE,
    TARGET_COMMIT, initialize_native_capture_epoch_cold_archive,
    verify_capture_epoch_inert_v14_git_release,
};
use crate::{CaptureEpochInitError, NativeWirePrimitives};

static FIXTURE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

struct FixtureRoot(PathBuf);

impl FixtureRoot {
    /// Create one process-unique canonical private cold archive root.
    fn create() -> Self {
        let sequence = FIXTURE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "weather-capture-epoch-authority-{}-{sequence}",
            std::process::id()
        ));
        DirBuilder::new().mode(0o700).create(&path).unwrap();
        Self(fs::canonicalize(path).unwrap())
    }
}

impl Drop for FixtureRoot {
    /// Remove only this disposable fixture tree.
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct FixtureGit {
    responses: BTreeMap<String, JsonValue>,
    requests: Vec<String>,
}

impl CaptureEpochPublicGitJson for FixtureGit {
    /// Return only explicitly captured public metadata fixtures.
    fn read_json(&mut self, suffix: &str) -> Result<JsonValue, CaptureEpochInitError> {
        self.requests.push(suffix.to_owned());
        self.responses
            .get(suffix)
            .cloned()
            .ok_or(CaptureEpochInitError::Deployment)
    }
}

/// Verify the fixed tag, direct-child and Check-before-Publish authority chain.
#[test]
fn proves_exact_inert_v14_public_release_chain() {
    let target_release = "2026.10.09-2";
    let mut git = fixture_git(target_release, false);

    let proof = verify_capture_epoch_inert_v14_git_release(target_release, &mut git).unwrap();

    assert_eq!(proof.target_release, target_release);
    assert_eq!(proof.target_commit, TARGET_COMMIT);
    assert_eq!(git.requests.len(), 5);
    assert_eq!(git.requests[0], format!("git/ref/tags/{SOURCE_RELEASE}"));
}

/// Reject a newer failed workflow rather than accepting an older successful run.
#[test]
fn rejects_stale_success_when_latest_check_failed() {
    let target_release = "2026.10.09-2";
    let mut git = fixture_git(target_release, true);

    assert_eq!(
        verify_capture_epoch_inert_v14_git_release(target_release, &mut git),
        Err(CaptureEpochInitError::Deployment)
    );
}

/// Create and revalidate only the six exact service-owned cold archive children.
#[test]
fn initializes_exact_cold_archive_children_idempotently() {
    let root = FixtureRoot::create();
    let owner = fs::metadata(&root.0).unwrap();

    initialize_native_capture_epoch_cold_archive(&root.0, owner.uid(), owner.gid()).unwrap();
    initialize_native_capture_epoch_cold_archive(&root.0, owner.uid(), owner.gid()).unwrap();

    let mut names = fs::read_dir(&root.0)
        .unwrap()
        // retain only the fixed fixture entry names
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect::<Vec<_>>();
    names.sort();
    let mut expected = COLD_ARCHIVE_DIRECTORIES
        .iter()
        .map(|value| (*value).to_owned())
        .collect::<Vec<_>>();
    expected.sort();
    assert_eq!(names, expected);
    // require exact private ownership for every created child
    for directory in COLD_ARCHIVE_DIRECTORIES {
        let metadata = fs::symlink_metadata(root.0.join(directory)).unwrap();
        assert!(metadata.is_dir());
        assert_eq!(metadata.permissions().mode() & 0o777, 0o700);
        assert_eq!((metadata.uid(), metadata.gid()), (owner.uid(), owner.gid()));
    }
}

/// Build exact captured GitHub documents for the retained five-read proof.
fn fixture_git(target_release: &str, include_newer_failed_check: bool) -> FixtureGit {
    let mut responses = BTreeMap::new();
    responses.insert(
        format!("git/ref/tags/{SOURCE_RELEASE}"),
        json(&format!(
            "{{\"object\":{{\"sha\":\"{SOURCE_COMMIT}\",\"type\":\"commit\"}}}}"
        )),
    );
    responses.insert(
        format!("git/ref/tags/{target_release}"),
        json(&format!(
            "{{\"object\":{{\"sha\":\"{TARGET_COMMIT}\",\"type\":\"commit\"}}}}"
        )),
    );
    responses.insert(
        format!("compare/{SOURCE_COMMIT}...{TARGET_COMMIT}?per_page=100"),
        json(&format!(
            "{{\"ahead_by\":1,\"base_commit\":{{\"sha\":\"{SOURCE_COMMIT}\"}},\"behind_by\":0,\"commits\":[{{\"parents\":[{{\"sha\":\"{SOURCE_COMMIT}\"}}],\"sha\":\"{TARGET_COMMIT}\"}}],\"status\":\"ahead\",\"total_commits\":1}}"
        )),
    );
    responses.insert(
        format!(
            "actions/workflows/check.yml/runs?head_sha={TARGET_COMMIT}&event=push&per_page=100"
        ),
        workflow_document("check.yml", include_newer_failed_check),
    );
    responses.insert(
        format!(
            "actions/workflows/publish-images.yml/runs?head_sha={TARGET_COMMIT}&event=push&per_page=100"
        ),
        workflow_document("publish-images.yml", false),
    );
    FixtureGit {
        responses,
        requests: Vec::new(),
    }
}

/// Build a complete workflow page with an optional newer failed exact run.
fn workflow_document(filename: &str, include_newer_failed: bool) -> JsonValue {
    // keep publication creation after the successful Check update
    let successful_created_at = if filename == "publish-images.yml" {
        "2026-10-09T00:02:00.000Z"
    } else {
        "2026-10-09T00:00:00.000Z"
    };
    let success = workflow_run(filename, 1, "success", successful_created_at);
    let runs = if include_newer_failed {
        format!(
            "{success},{}",
            workflow_run(filename, 2, "failure", "2026-10-09T00:01:00.000Z")
        )
    } else {
        success
    };
    let total = if include_newer_failed { 2 } else { 1 };
    json(&format!(
        "{{\"total_count\":{total},\"workflow_runs\":[{runs}]}}"
    ))
}

/// Build one exact same-repository completed push run.
fn workflow_run(filename: &str, id: u64, conclusion: &str, created_at: &str) -> String {
    let updated_at = if filename == "check.yml" {
        "2026-10-09T00:01:00.000Z"
    } else {
        "2026-10-09T00:03:00.000Z"
    };
    format!(
        "{{\"conclusion\":\"{conclusion}\",\"created_at\":\"{created_at}\",\"event\":\"push\",\"head_repository\":{{\"fork\":false,\"full_name\":\"anstosa/Ballydidean-Weather\",\"id\":1342404160}},\"head_sha\":\"{TARGET_COMMIT}\",\"id\":{id},\"path\":\".github/workflows/{filename}\",\"repository\":{{\"fork\":false,\"full_name\":\"anstosa/Ballydidean-Weather\",\"id\":1342404160}},\"run_attempt\":1,\"status\":\"completed\",\"updated_at\":\"{updated_at}\"}}"
    )
}

/// Decode one complete ordinary JSON fixture through the production primitive.
fn json(value: &str) -> JsonValue {
    NativeWirePrimitives.decode_json(value.as_bytes()).unwrap()
}
