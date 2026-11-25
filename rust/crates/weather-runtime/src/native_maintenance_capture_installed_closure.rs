//! Immutable installed predecessor runner closure for capture readiness.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Read;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::api_maintenance_candidate_files::open_read_no_follow;
use crate::native_maintenance_capture_readiness::{
    NativeMaintenanceCaptureInstalledReleaseAuthority, NativeMaintenanceCaptureReleaseGeneration,
    VerifiedNativeMaintenanceInstalledRelease,
};

const NATIVE_RUNNER_RELEASES_ROOT: &str =
    "/home/ubuntu/.weather/adjustment-maintenance/runner/releases";
const MANIFEST_NAME: &str = "MANIFEST.sha256";
const MANIFEST_MAXIMUM_BYTES: usize = 64 * 1_024;

/// Exact distinct source members in the predecessor's public fitter allowlist.
pub(crate) const FIT_SOURCE_FILES: &[&str] = &[
    "scripts/research/build_moisture_targets.py",
    "scripts/research/build_rain_sub24.py",
    "scripts/research/export_moisture_history.py",
    "scripts/research/export_rain_wind_runtime.py",
    "scripts/research/rain_context.py",
    "scripts/research/rain_context_features.py",
    "scripts/research/rain_event_guard.py",
    "scripts/research/rain_hurdle.py",
    "scripts/research/rain_hurdle_calibration.py",
    "scripts/research/rain_ordinal.py",
    "scripts/research/rain_recency.py",
    "scripts/research/rain_recency_calibration.py",
    "scripts/research/rain_refresh.py",
    "scripts/research/rain_residual.py",
    "scripts/research/rain_search.py",
    "scripts/research/rain_sub24.py",
    "scripts/research/rain_trajectory.py",
    "scripts/research/rain_trajectory_features.py",
    "scripts/research/rain_wind.py",
    "scripts/research/rain_wind_features.py",
    "scripts/research/rain_wind_source.py",
    "scripts/research/retain_moisture_research.py",
    "scripts/research/run_rain_sub24.py",
    "packages/forecast-adjustment/package.json",
    "scripts/research/adjustment-maintenance-runtime/forecast/algorithm-v1.js",
    "scripts/research/adjustment-maintenance-runtime/forecast/apply.js",
    "scripts/research/adjustment-maintenance-runtime/forecast/bootstrap-v1.js",
    "scripts/research/adjustment-maintenance-runtime/forecast/calendar.js",
    "scripts/research/adjustment-maintenance-runtime/forecast/candidate.js",
    "scripts/research/adjustment-maintenance-runtime/forecast/evaluate.js",
    "scripts/research/adjustment-maintenance-runtime/forecast/evidence.js",
    "scripts/research/adjustment-maintenance-runtime/forecast/holdout-ledger.js",
    "scripts/research/adjustment-maintenance-runtime/forecast/index.js",
    "scripts/research/adjustment-maintenance-runtime/forecast/maintenance-capture-epoch.js",
    "scripts/research/adjustment-maintenance-runtime/forecast/maintenance-policy.js",
    "scripts/research/adjustment-maintenance-runtime/forecast/maintenance-revision-projection.js",
    "scripts/research/adjustment-maintenance-runtime/forecast/maintenance-runtime-package.js",
    "scripts/research/adjustment-maintenance-runtime/forecast/maintenance-shadow-catalog.js",
    "scripts/research/adjustment-maintenance-runtime/forecast/maintenance-shadow-comparator.js",
    "scripts/research/adjustment-maintenance-runtime/forecast/maintenance-shadow-values.js",
    "scripts/research/adjustment-maintenance-runtime/forecast/performance-scorecard.js",
    "scripts/research/adjustment-maintenance-runtime/forecast/rain-hurdle-wind-artifact.js",
    "scripts/research/adjustment-maintenance-runtime/forecast/rain-hurdle-wind.js",
    "scripts/research/adjustment-maintenance-runtime/forecast/rain-maintenance-controls.js",
    "scripts/research/adjustment-maintenance-runtime/forecast/rain-fixed-gauge-target.js",
    "scripts/research/adjustment-maintenance-runtime/forecast/rain-runtime-registry.js",
    "scripts/research/adjustment-maintenance-runtime/forecast/runtime-bundle.js",
    "scripts/research/adjustment-maintenance-runtime/forecast/runtime-loader.js",
    "scripts/research/adjustment-maintenance-runtime/forecast/temperature-analog-research.js",
    "scripts/research/adjustment-maintenance-runtime/forecast/temperature-analog-shrinkage.js",
    "scripts/research/adjustment-maintenance-runtime/forecast/temperature-analog-stress-validation.js",
    "scripts/research/adjustment-maintenance-runtime/forecast/temperature-analog-validation.js",
    "scripts/research/adjustment-maintenance-runtime/forecast/temperature-canary.js",
    "scripts/research/adjustment-maintenance-runtime/forecast/temperature-causal-guard.js",
    "scripts/research/adjustment-maintenance-runtime/forecast/temperature-frozen-replay.js",
    "scripts/research/adjustment-maintenance-runtime/forecast/temperature-lead-research.js",
    "scripts/research/adjustment-maintenance-runtime/forecast/temperature-live-replay-events.js",
    "scripts/research/adjustment-maintenance-runtime/forecast/temperature-mos-runtime.js",
    "scripts/research/adjustment-maintenance-runtime/forecast/temperature-nowcast-research.js",
    "scripts/research/adjustment-maintenance-runtime/forecast/temperature-weather-research.js",
    "scripts/research/adjustment-maintenance-runtime/forecast/wind-canary.js",
    "packages/domain/package.json",
    "scripts/research/adjustment-maintenance-runtime/node_modules/@weather/domain/dist/forecast-adjustment.js",
    "scripts/research/adjustment-maintenance-runtime/node_modules/@weather/domain/dist/forecast-anchor-record.js",
    "scripts/research/adjustment-maintenance-runtime/node_modules/@weather/domain/dist/index.js",
    "scripts/research/adjustment-maintenance-runtime/node_modules/@weather/domain/dist/ingestion.js",
    "scripts/research/adjustment-maintenance-runtime/node_modules/@weather/domain/dist/provenance.js",
    "scripts/research/adjustment-maintenance-runtime/node_modules/@weather/domain/dist/rain-collection.js",
    "scripts/research/adjustment-maintenance-runtime/node_modules/@weather/domain/dist/weather-record.js",
    "scripts/research/temperature_refresh.py",
    "scripts/research/temperature_shortlead_models.py",
    "scripts/research/temperature_seasonal_ridge.py",
    "apps/worker/package.json",
    "scripts/research/adjustment-maintenance-runtime/worker/forecast-adjustment-wind-refresh-cli.js",
];

/// Contained immutable-closure failure without exposing installed host paths.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMaintenanceCaptureInstalledClosureError {
    Root,
    Manifest,
    Closure,
    Member,
}

/// One installed fit source identity inherited from the complete runner manifest.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeMaintenanceCaptureFitSourceIdentity {
    pub path: String,
    pub sha256: String,
}

/// Complete installed closure proof plus every exact fitted-source member identity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedNativeMaintenanceInstalledClosure {
    pub release: VerifiedNativeMaintenanceInstalledRelease,
    pub fit_source_members: Vec<NativeMaintenanceCaptureFitSourceIdentity>,
}

/// Fixed-generation reader for one literal frozen predecessor release directory.
pub struct NativeMaintenanceCaptureInstalledClosure {
    release_root: PathBuf,
}

impl NativeMaintenanceCaptureInstalledClosure {
    /// Select one source-commit child below the fixed production runner release root.
    pub fn native_node_predecessor(
        source_commit: &str,
    ) -> Result<Self, NativeMaintenanceCaptureInstalledClosureError> {
        // close the only caller-selected path component before constructing the fixed root
        if !valid_commit(source_commit) {
            return Err(NativeMaintenanceCaptureInstalledClosureError::Root);
        }
        Ok(Self {
            release_root: Path::new(NATIVE_RUNNER_RELEASES_ROOT).join(source_commit),
        })
    }

    /// Bind one isolated literal fixture release without weakening production construction.
    #[cfg(test)]
    pub(crate) fn for_test_release_root(release_root: PathBuf) -> Self {
        Self { release_root }
    }

    /// Verify the complete immutable manifest and return its fitted-source identities.
    pub fn verify(
        &self,
    ) -> Result<
        VerifiedNativeMaintenanceInstalledClosure,
        NativeMaintenanceCaptureInstalledClosureError,
    > {
        use NativeMaintenanceCaptureInstalledClosureError as Error;

        let authority = verify_release_root(&self.release_root)?;
        let manifest_path = authority.release_root.join(MANIFEST_NAME);
        let manifest_bytes = read_manifest(&manifest_path, &authority)?;
        let entries = parse_manifest(&manifest_bytes)?;
        let discovered = inspect_tree(&authority)?;
        // require the manifest to enumerate every installed runtime byte except itself
        if discovered != entries.keys().cloned().collect() {
            return Err(Error::Closure);
        }
        // bind every installed member to its immutable manifest digest
        for (path, expected_sha256) in &entries {
            let digest = hash_member(&authority.release_root.join(path), &authority)?;
            if &digest != expected_sha256 {
                return Err(Error::Member);
            }
        }
        let fit_source_members = FIT_SOURCE_FILES
            .iter()
            .map(|path| {
                entries
                    .get(*path)
                    .map(|sha256| NativeMaintenanceCaptureFitSourceIdentity {
                        path: (*path).to_owned(),
                        sha256: sha256.clone(),
                    })
                    .ok_or(Error::Manifest)
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(VerifiedNativeMaintenanceInstalledClosure {
            release: VerifiedNativeMaintenanceInstalledRelease {
                generation: NativeMaintenanceCaptureReleaseGeneration::NodePredecessor,
                manifest_sha256: sha256(&manifest_bytes),
                member_count: entries.len(),
                source_commit: authority.source_commit,
            },
            fit_source_members,
        })
    }
}

impl NativeMaintenanceCaptureInstalledReleaseAuthority
    for NativeMaintenanceCaptureInstalledClosure
{
    type Error = NativeMaintenanceCaptureInstalledClosureError;

    /// Re-run complete installed closure verification for every readiness observation.
    fn read_verified_installed_release(
        &mut self,
    ) -> Result<VerifiedNativeMaintenanceInstalledRelease, Self::Error> {
        Ok(self.verify()?.release)
    }
}

struct ReleaseAuthority {
    release_root: PathBuf,
    source_commit: String,
    uid: u32,
    gid: u32,
    dev: u64,
}

/// Admit only a literal immutable source-commit directory below `releases`.
fn verify_release_root(
    release_root: &Path,
) -> Result<ReleaseAuthority, NativeMaintenanceCaptureInstalledClosureError> {
    use NativeMaintenanceCaptureInstalledClosureError::Root;

    // prohibit relative or normalized aliases before any member is opened
    if !release_root.is_absolute() {
        return Err(Root);
    }
    let details = fs::symlink_metadata(release_root).map_err(|_| Root)?;
    let canonical = fs::canonicalize(release_root).map_err(|_| Root)?;
    let source_commit = release_root
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| valid_commit(name))
        .ok_or(Root)?;
    // bind the root name, parent, mode and literal canonical identity
    if !details.is_dir()
        || details.file_type().is_symlink()
        || release_root.parent().and_then(Path::file_name) != Some(std::ffi::OsStr::new("releases"))
        || canonical != release_root
        || details.mode() & 0o777 != 0o500
    {
        return Err(Root);
    }
    Ok(ReleaseAuthority {
        release_root: release_root.to_owned(),
        source_commit: source_commit.to_owned(),
        uid: details.uid(),
        gid: details.gid(),
        dev: details.dev(),
    })
}

/// Read the single-link immutable manifest through a bounded no-follow descriptor.
fn read_manifest(
    path: &Path,
    authority: &ReleaseAuthority,
) -> Result<Vec<u8>, NativeMaintenanceCaptureInstalledClosureError> {
    use NativeMaintenanceCaptureInstalledClosureError::Manifest;

    let before = fs::symlink_metadata(path).map_err(|_| Manifest)?;
    let mut file = open_read_no_follow(path).map_err(|_| Manifest)?;
    let opened = file.metadata().map_err(|_| Manifest)?;
    // preserve exact installed owner, filesystem, link count, mode and path identity
    if !valid_file(&before, &opened, authority, 0o400)
        || fs::canonicalize(path).ok().as_deref() != Some(path)
    {
        return Err(Manifest);
    }
    let opened_size = usize::try_from(opened.size()).map_err(|_| Manifest)?;
    let mut bytes = Vec::with_capacity(MANIFEST_MAXIMUM_BYTES.min(opened_size));
    file.by_ref()
        .take((MANIFEST_MAXIMUM_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| Manifest)?;
    // require one bounded nonempty newline-terminated manifest
    if bytes.is_empty() || bytes.len() > MANIFEST_MAXIMUM_BYTES || bytes.last() != Some(&b'\n') {
        return Err(Manifest);
    }
    Ok(bytes)
}

/// Parse the exact installed manifest grammar and mandatory fitter closure.
fn parse_manifest(
    bytes: &[u8],
) -> Result<BTreeMap<String, String>, NativeMaintenanceCaptureInstalledClosureError> {
    use NativeMaintenanceCaptureInstalledClosureError::Manifest;

    let source = std::str::from_utf8(bytes).map_err(|_| Manifest)?;
    let body = source.strip_suffix('\n').ok_or(Manifest)?;
    let mut entries = BTreeMap::new();
    // validate every exact relative file binding without path normalization
    for line in body.split('\n') {
        let (digest, path) = line.split_once("  ").ok_or(Manifest)?;
        // reject malformed digests, duplicate paths, absolutes and traversal segments
        if !valid_hash(digest)
            || !valid_manifest_path(path)
            || entries.insert(path.to_owned(), digest.to_owned()).is_some()
        {
            return Err(Manifest);
        }
    }
    // prohibit an empty closure and require every reviewed fitter source
    if entries.is_empty()
        || FIT_SOURCE_FILES
            .iter()
            .any(|path| !entries.contains_key(*path))
    {
        return Err(Manifest);
    }
    Ok(entries)
}

/// Enumerate the complete literal regular-file tree without following links.
fn inspect_tree(
    authority: &ReleaseAuthority,
) -> Result<BTreeSet<String>, NativeMaintenanceCaptureInstalledClosureError> {
    use NativeMaintenanceCaptureInstalledClosureError::Closure;

    let mut discovered = BTreeSet::new();
    let mut pending = vec![authority.release_root.clone()];
    // inspect each literal directory exactly once
    while let Some(directory) = pending.pop() {
        let details = fs::symlink_metadata(&directory).map_err(|_| Closure)?;
        // keep every directory on the admitted owner filesystem with immutable mode
        if !details.is_dir()
            || details.file_type().is_symlink()
            || details.uid() != authority.uid
            || details.gid() != authority.gid
            || details.dev() != authority.dev
            || details.mode() & 0o777 != 0o500
            || fs::canonicalize(&directory).ok().as_deref() != Some(directory.as_path())
        {
            return Err(Closure);
        }
        let children = fs::read_dir(&directory).map_err(|_| Closure)?;
        // reject links and special files while retaining exact relative names
        for child in children {
            let child = child.map_err(|_| Closure)?;
            let file_type = child.file_type().map_err(|_| Closure)?;
            let path = child.path();
            let relative = path
                .strip_prefix(&authority.release_root)
                .ok()
                .and_then(Path::to_str)
                .ok_or(Closure)?
                .to_owned();
            if file_type.is_dir() {
                pending.push(path);
            } else if file_type.is_file() {
                discovered.insert(relative);
            } else {
                return Err(Closure);
            }
        }
    }
    discovered.remove(MANIFEST_NAME);
    Ok(discovered)
}

/// Hash one exact single-link immutable member through a no-follow descriptor.
fn hash_member(
    path: &Path,
    authority: &ReleaseAuthority,
) -> Result<String, NativeMaintenanceCaptureInstalledClosureError> {
    use NativeMaintenanceCaptureInstalledClosureError::Member;

    let before = fs::symlink_metadata(path).map_err(|_| Member)?;
    let mut file = open_read_no_follow(path).map_err(|_| Member)?;
    let opened = file.metadata().map_err(|_| Member)?;
    // prohibit links, aliases, foreign files, and writable installed bytes
    if !valid_file(&before, &opened, authority, 0o400)
        || fs::canonicalize(path).ok().as_deref() != Some(path)
    {
        return Err(Member);
    }
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 16 * 1_024];
    // stream each exact member without allocating according to its file size
    loop {
        let count = file.read(&mut buffer).map_err(|_| Member)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(lower_hex(hasher.finalize().into()))
}

/// Compare path and descriptor metadata against one installed file authority.
fn valid_file(
    before: &fs::Metadata,
    opened: &fs::Metadata,
    authority: &ReleaseAuthority,
    mode: u32,
) -> bool {
    before.is_file()
        && opened.is_file()
        && !before.file_type().is_symlink()
        && before.dev() == opened.dev()
        && before.ino() == opened.ino()
        && opened.uid() == authority.uid
        && opened.gid() == authority.gid
        && opened.dev() == authority.dev
        && opened.nlink() == 1
        && opened.mode() & 0o777 == mode
}

/// Admit the source's exact relative manifest member alphabet.
fn valid_manifest_path(path: &str) -> bool {
    let mut bytes = path.bytes();
    let Some(first) = bytes.next() else {
        return false;
    };
    // retain the source regex alphabet and prohibit absolute or traversal paths
    first.is_ascii_alphanumeric()
        && bytes.all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'@' | b'/' | b'+' | b'-')
        })
        && !path.starts_with('/')
        && !path.split('/').any(|segment| matches!(segment, "." | ".."))
}

/// Admit one immutable forty-character lowercase source commit.
fn valid_commit(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// Admit one lowercase SHA-256 manifest identity.
fn valid_hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// Hash one exact manifest byte sequence.
fn sha256(bytes: &[u8]) -> String {
    lower_hex(Sha256::digest(bytes).into())
}

/// Render a SHA-256 value in the source's lowercase hexadecimal alphabet.
fn lower_hex(digest: [u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut value = String::with_capacity(64);
    // emit every byte without an intermediate formatter allocation
    for byte in digest {
        value.push(char::from(HEX[usize::from(byte >> 4)]));
        value.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    value
}
