//! Native host authority for the retained capture-epoch initialization command.

use std::ffi::CString;
use std::fs::{self, DirBuilder};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use hyper::Method;
use hyper::header::{ACCEPT, HeaderName, HeaderValue, USER_AGENT};
use url::Url;
use weather_contracts::JsonValue;
use weather_edge::ArchiveWirePrimitives;

use crate::maintenance_canonical_json::field;
use crate::{
    CaptureEpochDeploymentAuthority, CaptureEpochDeploymentProof, CaptureEpochInitError,
    CaptureEpochInitializationHostAuthority, NativeOutboundHttp, NativeOutboundHttpRequest,
    NativeWirePrimitives, read_capture_epoch_target_environment,
};

const DEFAULT_RELEASES_DIRECTORY: &str = "/opt/weather/current/deploy/releases";
const DEFAULT_COLD_ARCHIVE_ROOT: &str = "/var/lib/weather/xweather/adjustment-evidence";
const COLD_ARCHIVE_UID: u32 = 10_002;
const COLD_ARCHIVE_GID: u32 = 10_002;
const SOURCE_RELEASE: &str = "2026.10.09-1";
const SOURCE_COMMIT: &str = "56b327d9c750946f6f6963b6fe1fa5c9bba791ca";
const TARGET_COMMIT: &str = "f6daa89d135661eb60dd5479c5d0d6c23e7198a9";
const GITHUB_REPOSITORY: &str = "anstosa/Ballydidean-Weather";
const GITHUB_REPOSITORY_ID: u64 = 1_342_404_160;
const GITHUB_BODY_MAXIMUM: u64 = 4_194_304;
const GITHUB_TIMEOUT: Duration = Duration::from_secs(15);
const COLD_ARCHIVE_DIRECTORIES: &[&str] = &[
    "revision-commit-receipts",
    "revision-frontier-successors",
    "revision-projections",
    "shadow-revision-capsules",
    "rain-control-states",
    "rain-control-state-stage-receipts",
];

/// Read one bounded public GitHub response without credential or redirect authority.
pub trait CaptureEpochPublicGitJson {
    /// Return the decoded response for one repository-relative API suffix.
    fn read_json(&mut self, suffix: &str) -> Result<JsonValue, CaptureEpochInitError>;
}

/// Pooled native transport for the predecessor's public Weather Git proof.
pub struct NativeCaptureEpochPublicGitJson {
    client: Arc<NativeOutboundHttp>,
}

impl NativeCaptureEpochPublicGitJson {
    /// Bind public metadata reads to one retained pooled native HTTP client.
    pub const fn new(client: Arc<NativeOutboundHttp>) -> Self {
        Self { client }
    }
}

impl CaptureEpochPublicGitJson for NativeCaptureEpochPublicGitJson {
    /// Fetch one exact no-redirect GitHub document under the fixed public repository.
    fn read_json(&mut self, suffix: &str) -> Result<JsonValue, CaptureEpochInitError> {
        let url = Url::parse(&format!(
            "https://api.github.com/repos/{GITHUB_REPOSITORY}/{suffix}"
        ))
        .map_err(|_| CaptureEpochInitError::Deployment)?;
        let request = NativeOutboundHttpRequest::new(Method::GET, url)
            .map_err(|_| CaptureEpochInitError::Deployment)?
            .header(
                ACCEPT,
                HeaderValue::from_static("application/vnd.github+json"),
            )
            .header(
                HeaderName::from_static("x-github-api-version"),
                HeaderValue::from_static("2026-03-10"),
            )
            .header(USER_AGENT, HeaderValue::from_static("weather-native/1"))
            .manual_redirect();
        let response = self
            .client
            .execute(request, GITHUB_TIMEOUT, GITHUB_BODY_MAXIMUM)
            .map_err(|_| CaptureEpochInitError::Deployment)?;
        // treat redirects, rate limits and all non-success responses as authority failures
        if response.status != 200 {
            return Err(CaptureEpochInitError::Deployment);
        }
        NativeWirePrimitives
            .decode_json(&response.body)
            .ok_or(CaptureEpochInitError::Deployment)
    }
}

/// Concrete deployment and cold-archive authority retained by the init process.
pub struct NativeCaptureEpochInitializationAuthority<Git = NativeCaptureEpochPublicGitJson> {
    releases_directory: PathBuf,
    cold_archive_root: PathBuf,
    cold_archive_uid: u32,
    cold_archive_gid: u32,
    git: Git,
}

impl NativeCaptureEpochInitializationAuthority<NativeCaptureEpochPublicGitJson> {
    /// Bind the production release namespace, archive root and public proof transport.
    pub fn native(client: Arc<NativeOutboundHttp>) -> Self {
        Self::for_paths(
            NativeCaptureEpochPublicGitJson::new(client),
            PathBuf::from(DEFAULT_RELEASES_DIRECTORY),
            PathBuf::from(DEFAULT_COLD_ARCHIVE_ROOT),
            COLD_ARCHIVE_UID,
            COLD_ARCHIVE_GID,
        )
    }
}

impl<Git> NativeCaptureEpochInitializationAuthority<Git> {
    /// Bind explicit already selected paths and owner identities for composition or fixtures.
    pub fn for_paths(
        git: Git,
        releases_directory: PathBuf,
        cold_archive_root: PathBuf,
        cold_archive_uid: u32,
        cold_archive_gid: u32,
    ) -> Self {
        Self {
            releases_directory,
            cold_archive_root,
            cold_archive_uid,
            cold_archive_gid,
            git,
        }
    }
}

impl<Git> CaptureEpochDeploymentAuthority for NativeCaptureEpochInitializationAuthority<Git>
where
    Git: CaptureEpochPublicGitJson,
{
    /// Read the requested fixed root-owned release environment through the existing owner.
    fn read_target_environment(
        &mut self,
        target_release: &str,
    ) -> Result<Vec<u8>, CaptureEpochInitError> {
        read_capture_epoch_target_environment(&self.releases_directory, target_release)
    }

    /// Prove the exact public v13-to-v14 bridge and its successful release workflows.
    fn verify_inert_target_release(
        &mut self,
        target_release: &str,
    ) -> Result<CaptureEpochDeploymentProof, CaptureEpochInitError> {
        verify_capture_epoch_inert_v14_git_release(target_release, &mut self.git)
    }
}

impl<Git> CaptureEpochInitializationHostAuthority for NativeCaptureEpochInitializationAuthority<Git>
where
    Git: CaptureEpochPublicGitJson,
{
    /// Require the original effective root process identity without consulting environment text.
    fn require_root(&mut self) -> Result<(), CaptureEpochInitError> {
        // libc exposes the process credential used by filesystem authorization
        (unsafe { libc::geteuid() } == 0)
            .then_some(())
            .ok_or(CaptureEpochInitError::Root)
    }

    /// Create and validate only the six fixed service-owned cold archive directories.
    fn initialize_cold_archive(&mut self) -> Result<(), CaptureEpochInitError> {
        initialize_native_capture_epoch_cold_archive(
            &self.cold_archive_root,
            self.cold_archive_uid,
            self.cold_archive_gid,
        )
    }
}

/// Verify the exact direct-child target, tag identities and authoritative workflow ordering.
pub fn verify_capture_epoch_inert_v14_git_release(
    target_release: &str,
    git: &mut impl CaptureEpochPublicGitJson,
) -> Result<CaptureEpochDeploymentProof, CaptureEpochInitError> {
    use CaptureEpochInitError::Deployment;

    // retain the same immutable release grammar before any public request
    if !crate::capture_epoch_init_deployment::valid_capture_epoch_target_release(target_release)
        || target_release == SOURCE_RELEASE
    {
        return Err(Deployment);
    }
    let source_commit = resolve_public_weather_tag(SOURCE_RELEASE, git)?;
    let target_commit = resolve_public_weather_tag(target_release, git)?;
    // reject moved source or bridge tags before workflow evidence
    if source_commit != SOURCE_COMMIT || target_commit != TARGET_COMMIT {
        return Err(Deployment);
    }
    verify_direct_child(&source_commit, &target_commit, git)?;
    let check = latest_successful_workflow(&target_commit, "check.yml", git)?;
    let publish = latest_successful_workflow(&target_commit, "publish-images.yml", git)?;
    // preserve the exact successful Check-before-Publish causal order
    if check.updated_at.ok_or(Deployment)? > publish.created_at {
        return Err(Deployment);
    }
    Ok(CaptureEpochDeploymentProof {
        target_release: target_release.to_owned(),
        target_commit,
    })
}

/// Create the exact six cold-reader children without repairing an existing entry.
pub fn initialize_native_capture_epoch_cold_archive(
    root: &Path,
    expected_uid: u32,
    expected_gid: u32,
) -> Result<(), CaptureEpochInitError> {
    require_owned_private_directory(root, expected_uid, expected_gid, None)?;
    let canonical_root = fs::canonicalize(root).map_err(|_| CaptureEpochInitError::ColdArchive)?;
    // create only source-declared children beneath the already canonical root
    for directory in COLD_ARCHIVE_DIRECTORIES {
        let path = root.join(directory);
        let created = match DirBuilder::new().mode(0o700).create(&path) {
            Ok(()) => true,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => false,
            Err(_) => return Err(CaptureEpochInitError::ColdArchive),
        };
        // assign service ownership only to a new directory that does not already have it
        if created {
            let metadata =
                fs::symlink_metadata(&path).map_err(|_| CaptureEpochInitError::ColdArchive)?;
            if metadata.uid() != expected_uid || metadata.gid() != expected_gid {
                chown_directory(&path, expected_uid, expected_gid)?;
            }
        }
        require_owned_private_directory(&path, expected_uid, expected_gid, Some(&canonical_root))?;
    }
    Ok(())
}

/// Resolve a direct or annotated public tag through at most three tag objects.
fn resolve_public_weather_tag(
    release: &str,
    git: &mut impl CaptureEpochPublicGitJson,
) -> Result<String, CaptureEpochInitError> {
    let mut document = git.read_json(&format!("git/ref/tags/{release}"))?;
    // follow only the predecessor's fixed annotated-tag depth
    for _ in 0..3 {
        let object = field(&document, "object").ok_or(CaptureEpochInitError::Deployment)?;
        let sha = json_text(object, "sha").ok_or(CaptureEpochInitError::Deployment)?;
        if !valid_lower_hex(&sha, 40) {
            return Err(CaptureEpochInitError::Deployment);
        }
        match json_text(object, "type").as_deref() {
            Some("commit") => return Ok(sha),
            Some("tag") => document = git.read_json(&format!("git/tags/{sha}"))?,
            _ => return Err(CaptureEpochInitError::Deployment),
        }
    }
    Err(CaptureEpochInitError::Deployment)
}

/// Require the exact one-commit ancestry relation between the two fixed public tags.
fn verify_direct_child(
    source_commit: &str,
    target_commit: &str,
    git: &mut impl CaptureEpochPublicGitJson,
) -> Result<(), CaptureEpochInitError> {
    let document = git.read_json(&format!(
        "compare/{source_commit}...{target_commit}?per_page=100"
    ))?;
    let commits = json_array(&document, "commits").ok_or(CaptureEpochInitError::Deployment)?;
    let base = field(&document, "base_commit").ok_or(CaptureEpochInitError::Deployment)?;
    let commit = commits.first().ok_or(CaptureEpochInitError::Deployment)?;
    let parents = json_array(commit, "parents").ok_or(CaptureEpochInitError::Deployment)?;
    let parent = parents.first().ok_or(CaptureEpochInitError::Deployment)?;
    // require a complete exact direct-child comparison rather than a truncated page
    if json_text(&document, "status").as_deref() != Some("ahead")
        || json_integer(&document, "ahead_by") != Some(1)
        || json_integer(&document, "behind_by") != Some(0)
        || json_integer(&document, "total_commits") != Some(1)
        || json_text(base, "sha").as_deref() != Some(source_commit)
        || commits.len() != 1
        || json_text(commit, "sha").as_deref() != Some(target_commit)
        || parents.len() != 1
        || json_text(parent, "sha").as_deref() != Some(source_commit)
    {
        return Err(CaptureEpochInitError::Deployment);
    }
    Ok(())
}

struct WorkflowEvidence {
    created_at: i64,
    updated_at: Option<i64>,
}

/// Select only the newest same-repository successful push run for one workflow.
fn latest_successful_workflow(
    commit: &str,
    filename: &str,
    git: &mut impl CaptureEpochPublicGitJson,
) -> Result<WorkflowEvidence, CaptureEpochInitError> {
    let document = git.read_json(&format!(
        "actions/workflows/{filename}/runs?head_sha={commit}&event=push&per_page=100"
    ))?;
    let total = json_integer(&document, "total_count").ok_or(CaptureEpochInitError::Deployment)?;
    let runs = json_array(&document, "workflow_runs").ok_or(CaptureEpochInitError::Deployment)?;
    // require one complete bounded page before filtering the fixed repository identity
    if !(1..=100).contains(&total) || runs.len() != total as usize {
        return Err(CaptureEpochInitError::Deployment);
    }
    let mut scoped = Vec::new();
    // retain only exact-commit push runs from the non-fork source repository
    for run in runs {
        if json_text(run, "head_sha").as_deref() == Some(commit)
            && json_text(run, "event").as_deref() == Some("push")
            && repository_matches(field(run, "repository"))
            && repository_matches(field(run, "head_repository"))
        {
            let id = positive_integer(run, "id").ok_or(CaptureEpochInitError::Deployment)?;
            positive_integer(run, "run_attempt").ok_or(CaptureEpochInitError::Deployment)?;
            if json_text(run, "path").as_deref()
                != Some(format!(".github/workflows/{filename}").as_str())
            {
                return Err(CaptureEpochInitError::Deployment);
            }
            let created_at = json_instant(run, "created_at")?;
            scoped.push((created_at, id, run));
        }
    }
    let (_, _, selected) = scoped
        .into_iter()
        .max_by_key(|(created_at, id, _)| (*created_at, *id))
        .ok_or(CaptureEpochInitError::Deployment)?;
    // a newer queued, failed or cancelled run blocks stale success
    if json_text(selected, "status").as_deref() != Some("completed")
        || json_text(selected, "conclusion").as_deref() != Some("success")
    {
        return Err(CaptureEpochInitError::Deployment);
    }
    Ok(WorkflowEvidence {
        created_at: json_instant(selected, "created_at")?,
        updated_at: json_text(selected, "updated_at")
            .and_then(|value| NativeWirePrimitives.parse_explicit_utc_milliseconds(&value)),
    })
}

/// Match both nested repository identities used by the source workflow filter.
fn repository_matches(value: Option<&JsonValue>) -> bool {
    let Some(value) = value else {
        return false;
    };
    json_integer(value, "id") == Some(GITHUB_REPOSITORY_ID)
        && json_text(value, "full_name").as_deref() == Some(GITHUB_REPOSITORY)
        && matches!(field(value, "fork"), Some(JsonValue::Boolean(false)))
}

/// Parse one canonical workflow timestamp through the approved native date owner.
fn json_instant(value: &JsonValue, key: &str) -> Result<i64, CaptureEpochInitError> {
    let value = json_text(value, key).ok_or(CaptureEpochInitError::Deployment)?;
    NativeWirePrimitives
        .parse_explicit_utc_milliseconds(&value)
        .ok_or(CaptureEpochInitError::Deployment)
}

/// Borrow one array field without normalizing an absent or scalar value.
fn json_array<'a>(value: &'a JsonValue, key: &str) -> Option<&'a [JsonValue]> {
    let JsonValue::Array(values) = field(value, key)? else {
        return None;
    };
    Some(values)
}

/// Decode one exact UTF-16 string field.
fn json_text(value: &JsonValue, key: &str) -> Option<String> {
    let JsonValue::String(value) = field(value, key)? else {
        return None;
    };
    String::from_utf16(value).ok()
}

/// Decode one nonnegative JavaScript-safe integer field.
fn json_integer(value: &JsonValue, key: &str) -> Option<u64> {
    let JsonValue::Number(value) = field(value, key)? else {
        return None;
    };
    (value.is_finite() && value.fract() == 0.0 && (0.0..=9_007_199_254_740_991.0).contains(value))
        .then_some(*value as u64)
}

/// Decode one strictly positive JavaScript-safe integer field.
fn positive_integer(value: &JsonValue, key: &str) -> Option<u64> {
    json_integer(value, key).filter(|value| *value > 0)
}

/// Match one fixed-width lowercase hexadecimal identity.
fn valid_lower_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

/// Require one canonical private directory with exact ownership and optional parent binding.
fn require_owned_private_directory(
    path: &Path,
    expected_uid: u32,
    expected_gid: u32,
    expected_parent: Option<&Path>,
) -> Result<(), CaptureEpochInitError> {
    let metadata = fs::symlink_metadata(path).map_err(|_| CaptureEpochInitError::ColdArchive)?;
    let canonical = fs::canonicalize(path).map_err(|_| CaptureEpochInitError::ColdArchive)?;
    // reject links, aliases, broad modes, foreign ownership and parent substitutions
    if !metadata.is_dir()
        || metadata.file_type().is_symlink()
        || metadata.permissions().mode() & 0o777 != 0o700
        || metadata.uid() != expected_uid
        || metadata.gid() != expected_gid
        || canonical != path
        || expected_parent.is_some_and(|parent| canonical.parent() != Some(parent))
    {
        return Err(CaptureEpochInitError::ColdArchive);
    }
    Ok(())
}

/// Assign exact service ownership to one newly created admitted directory.
fn chown_directory(path: &Path, uid: u32, gid: u32) -> Result<(), CaptureEpochInitError> {
    let path = CString::new(path.as_os_str().as_bytes())
        .map_err(|_| CaptureEpochInitError::ColdArchive)?;
    // libc chown is required only for newly created fixed directories
    if unsafe { libc::chown(path.as_ptr(), uid, gid) } != 0 {
        return Err(CaptureEpochInitError::ColdArchive);
    }
    Ok(())
}

#[cfg(test)]
#[path = "native_capture_epoch_init_authority_test.rs"]
mod tests;
