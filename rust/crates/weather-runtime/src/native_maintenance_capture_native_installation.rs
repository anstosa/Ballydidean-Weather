//! Immutable native maintenance-capture installation generation proof.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Read;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::api_maintenance_candidate_files::open_read_no_follow;
use crate::native_maintenance_capture_installed_closure::NativeMaintenanceCaptureFitSourceIdentity;
use crate::native_maintenance_capture_readiness::{
    NativeMaintenanceCaptureInstalledReleaseAuthority, NativeMaintenanceCaptureReleaseGeneration,
    VerifiedNativeMaintenanceInstalledRelease,
};

const NATIVE_RUNNER_RELEASES_ROOT: &str =
    "/home/ubuntu/.weather/adjustment-maintenance/runner/releases";
const MANIFEST_NAME: &str = "MANIFEST.sha256";
const MANIFEST_MAXIMUM_BYTES: usize = 64 * 1_024;
const NATIVE_EXECUTABLE_RELATIVE_PATH: &str = "bin/weather-maintenance-capture";
const NATIVE_FIT_EXECUTABLES: [&str; 2] = [
    "bin/weather-maintenance-fit",
    "bin/weather-maintenance-wind-fit",
];
// bind the actual native package provenance without retaining interpreter application sources
pub(crate) const NATIVE_SOURCE_FILES: &[&str] = &[
    "deploy/config/ssh_config.example",
    "rust/Cargo.lock",
    "rust/Cargo.toml",
    "rust/crates/weather-runtime/Cargo.toml",
    "rust/crates/weather-runtime/src/bin/weather-maintenance-capture.rs",
    "rust/crates/weather-runtime/src/bin/weather-maintenance-fit.rs",
    "rust/crates/weather-runtime/src/bin/weather-maintenance-wind-fit.rs",
    "rust/rust-toolchain.toml",
    "rust/crates/weather-runtime/src/lib.rs",
    "rust/crates/weather-runtime/src/native_maintenance_candidate_owner.rs",
    "rust/crates/weather-runtime/src/native_maintenance_capture_host_readiness.rs",
    "rust/crates/weather-runtime/src/native_maintenance_capture_native_installation.rs",
    "rust/crates/weather-runtime/src/native_maintenance_fit_candidate.rs",
    "rust/crates/weather-runtime/src/native_maintenance_fit_runtime_readiness.rs",
    "rust/crates/weather-runtime/src/native_maintenance_fit_sandbox.rs",
    "rust/crates/weather-runtime/src/native_maintenance_fit_snapshot.rs",
    "rust/crates/weather-runtime/src/native_maintenance_rain_fit.rs",
    "rust/crates/weather-runtime/src/native_maintenance_temperature_fit.rs",
    "scripts/systemd/native/weather-adjustment-archive.service",
    "scripts/systemd/native/weather-adjustment-daily.service",
    "scripts/systemd/native/weather-adjustment-daily.timer",
    "scripts/systemd/native/weather-adjustment-monthly.service",
    "scripts/systemd/native/weather-adjustment-monthly.timer",
];

/// Path-free native installation refusal category.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMaintenanceCaptureNativeInstallationError {
    Root,
    Manifest,
    Closure,
    Member,
    ExecutablePath,
    Executable,
}

/// Exact immutable native executable identity bound to the running process.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeMaintenanceCaptureNativeExecutableIdentity {
    pub dev: u64,
    pub gid: u32,
    pub ino: u64,
    pub mode: u32,
    pub path: PathBuf,
    pub sha256: String,
    pub size: u64,
    pub uid: u32,
}

/// Complete native-generation release closure and actual executable proof.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedNativeMaintenanceCaptureNativeInstallation {
    pub executable: NativeMaintenanceCaptureNativeExecutableIdentity,
    pub fit_source_members: Vec<NativeMaintenanceCaptureFitSourceIdentity>,
    pub release: VerifiedNativeMaintenanceInstalledRelease,
}

#[derive(Clone)]
enum CurrentExecutableAuthority {
    Process,
    #[cfg(test)]
    Fixture(PathBuf),
}

impl CurrentExecutableAuthority {
    /// Read the literal running executable path or isolated fixture value.
    fn path(&self) -> Result<PathBuf, NativeMaintenanceCaptureNativeInstallationError> {
        match self {
            Self::Process => std::env::current_exe()
                .map_err(|_| NativeMaintenanceCaptureNativeInstallationError::ExecutablePath),
            #[cfg(test)]
            Self::Fixture(path) => Ok(path.clone()),
        }
    }

    /// Read the actual running inode or the fixture executable inode.
    fn metadata(&self) -> Result<fs::Metadata, NativeMaintenanceCaptureNativeInstallationError> {
        match self {
            Self::Process => fs::metadata("/proc/self/exe")
                .map_err(|_| NativeMaintenanceCaptureNativeInstallationError::Executable),
            #[cfg(test)]
            Self::Fixture(path) => fs::metadata(path)
                .map_err(|_| NativeMaintenanceCaptureNativeInstallationError::Executable),
        }
    }
}

/// Fixed-generation reader for one native maintenance-capture release.
pub struct NativeMaintenanceCaptureNativeInstallation {
    release_root: PathBuf,
    current_executable: CurrentExecutableAuthority,
}

impl NativeMaintenanceCaptureNativeInstallation {
    /// Select one source-commit release below the fixed production runner root.
    pub fn native_release(
        source_commit: &str,
    ) -> Result<Self, NativeMaintenanceCaptureNativeInstallationError> {
        // close the only caller-selected path component before constructing the fixed root
        if !valid_commit(source_commit) {
            return Err(NativeMaintenanceCaptureNativeInstallationError::Root);
        }
        Ok(Self {
            release_root: Path::new(NATIVE_RUNNER_RELEASES_ROOT).join(source_commit),
            current_executable: CurrentExecutableAuthority::Process,
        })
    }

    /// Bind one disposable release and observed executable for ordinary unit tests.
    #[cfg(test)]
    pub(crate) fn for_fixture(release_root: PathBuf, current_executable: PathBuf) -> Self {
        Self {
            release_root,
            current_executable: CurrentExecutableAuthority::Fixture(current_executable),
        }
    }

    /// Verify the full release, fitted sources, and literal running native binary.
    pub fn verify(
        &self,
    ) -> Result<
        VerifiedNativeMaintenanceCaptureNativeInstallation,
        NativeMaintenanceCaptureNativeInstallationError,
    > {
        use NativeMaintenanceCaptureNativeInstallationError as Error;

        let authority = verify_release_root(&self.release_root)?;
        let manifest_path = authority.release_root.join(MANIFEST_NAME);
        let manifest_bytes = read_manifest(&manifest_path, &authority)?;
        let entries = parse_manifest(&manifest_bytes)?;
        let discovered = inspect_tree(&authority)?;
        // require the manifest to bind every installed byte except itself
        if discovered != entries.keys().cloned().collect() {
            return Err(Error::Closure);
        }
        let executable_path = authority.release_root.join(NATIVE_EXECUTABLE_RELATIVE_PATH);
        let current_executable = self.current_executable.path()?;
        // prohibit a copied, linked, or caller-selected executable outside this release
        if current_executable != executable_path
            || fs::canonicalize(&executable_path).ok().as_deref() != Some(executable_path.as_path())
        {
            return Err(Error::ExecutablePath);
        }
        let running_before = self.current_executable.metadata()?;
        let mut executable = None;
        // bind every installed member to its manifest digest and exact role mode
        for (relative, expected_sha256) in &entries {
            let path = authority.release_root.join(relative);
            let mode = if relative == NATIVE_EXECUTABLE_RELATIVE_PATH
                || NATIVE_FIT_EXECUTABLES.contains(&relative.as_str())
            {
                0o500
            } else {
                0o400
            };
            let identity = hash_member(&path, &authority, mode).map_err(|error| {
                // retain a distinct binary refusal without exposing its fixed path
                if relative == NATIVE_EXECUTABLE_RELATIVE_PATH {
                    Error::Executable
                } else {
                    error
                }
            })?;
            if &identity.sha256 != expected_sha256 {
                return Err(if relative == NATIVE_EXECUTABLE_RELATIVE_PATH {
                    Error::Executable
                } else {
                    Error::Member
                });
            }
            // retain only the running binary's opened inode identity
            if relative == NATIVE_EXECUTABLE_RELATIVE_PATH {
                executable = Some(identity);
            }
        }
        let executable = executable.ok_or(Error::Executable)?;
        let running_after = self.current_executable.metadata()?;
        let installed_after =
            fs::symlink_metadata(&executable_path).map_err(|_| Error::Executable)?;
        // bind the opened manifest member to the process's stable executing inode
        if !same_executable(&executable, &running_before)
            || !same_file(&running_before, &running_after)
            || installed_after.file_type().is_symlink()
            || !same_executable(&executable, &installed_after)
        {
            return Err(Error::Executable);
        }
        let fit_source_members = NATIVE_FIT_EXECUTABLES
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
        Ok(VerifiedNativeMaintenanceCaptureNativeInstallation {
            executable,
            fit_source_members,
            release: VerifiedNativeMaintenanceInstalledRelease {
                generation: NativeMaintenanceCaptureReleaseGeneration::Native,
                manifest_sha256: sha256(&manifest_bytes),
                member_count: entries.len(),
                source_commit: authority.source_commit,
            },
        })
    }
}

impl NativeMaintenanceCaptureInstalledReleaseAuthority
    for NativeMaintenanceCaptureNativeInstallation
{
    type Error = NativeMaintenanceCaptureNativeInstallationError;

    /// Re-run complete native installation verification for every observation.
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

/// Admit only a literal immutable commit directory below `releases`.
fn verify_release_root(
    release_root: &Path,
) -> Result<ReleaseAuthority, NativeMaintenanceCaptureNativeInstallationError> {
    use NativeMaintenanceCaptureNativeInstallationError::Root;

    // prohibit relative or normalized aliases before opening members
    if !release_root.is_absolute() {
        return Err(Root);
    }
    let metadata = fs::symlink_metadata(release_root).map_err(|_| Root)?;
    let canonical = fs::canonicalize(release_root).map_err(|_| Root)?;
    let source_commit = release_root
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| valid_commit(name))
        .ok_or(Root)?;
    // bind name, parent, owner filesystem, mode, and literal canonical identity
    if !metadata.is_dir()
        || metadata.file_type().is_symlink()
        || release_root.parent().and_then(Path::file_name) != Some(std::ffi::OsStr::new("releases"))
        || canonical != release_root
        || metadata.mode() & 0o777 != 0o500
    {
        return Err(Root);
    }
    Ok(ReleaseAuthority {
        release_root: release_root.to_owned(),
        source_commit: source_commit.to_owned(),
        uid: metadata.uid(),
        gid: metadata.gid(),
        dev: metadata.dev(),
    })
}

/// Read the bounded immutable manifest through a no-follow descriptor.
fn read_manifest(
    path: &Path,
    authority: &ReleaseAuthority,
) -> Result<Vec<u8>, NativeMaintenanceCaptureNativeInstallationError> {
    use NativeMaintenanceCaptureNativeInstallationError::Manifest;

    let before = fs::symlink_metadata(path).map_err(|_| Manifest)?;
    let mut file = open_read_no_follow(path).map_err(|_| Manifest)?;
    let opened = file.metadata().map_err(|_| Manifest)?;
    // preserve exact owner, filesystem, link count, mode, and path identity
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

/// Parse the exact manifest grammar and mandatory native closure members.
fn parse_manifest(
    bytes: &[u8],
) -> Result<BTreeMap<String, String>, NativeMaintenanceCaptureNativeInstallationError> {
    use NativeMaintenanceCaptureNativeInstallationError::Manifest;

    let source = std::str::from_utf8(bytes).map_err(|_| Manifest)?;
    let body = source.strip_suffix('\n').ok_or(Manifest)?;
    let mut entries = BTreeMap::new();
    // validate every exact relative file binding without normalization
    for line in body.split('\n') {
        let (digest, path) = line.split_once("  ").ok_or(Manifest)?;
        if !valid_hash(digest)
            || !valid_manifest_path(path)
            || entries.insert(path.to_owned(), digest.to_owned()).is_some()
        {
            return Err(Manifest);
        }
    }
    // require the exact generation-specific native package and provenance closure
    let expected = NATIVE_SOURCE_FILES
        .iter()
        .chain(NATIVE_FIT_EXECUTABLES.iter())
        .copied()
        .chain(std::iter::once(NATIVE_EXECUTABLE_RELATIVE_PATH))
        .collect::<BTreeSet<_>>();
    if entries.keys().map(String::as_str).collect::<BTreeSet<_>>() != expected {
        return Err(Manifest);
    }
    Ok(entries)
}

/// Enumerate the complete literal regular-file tree without following links.
fn inspect_tree(
    authority: &ReleaseAuthority,
) -> Result<BTreeSet<String>, NativeMaintenanceCaptureNativeInstallationError> {
    use NativeMaintenanceCaptureNativeInstallationError::Closure;

    let mut discovered = BTreeSet::new();
    let mut pending = vec![authority.release_root.clone()];
    // inspect every literal directory exactly once
    while let Some(directory) = pending.pop() {
        let metadata = fs::symlink_metadata(&directory).map_err(|_| Closure)?;
        // retain immutable owner directories on the release filesystem
        if !metadata.is_dir()
            || metadata.file_type().is_symlink()
            || metadata.uid() != authority.uid
            || metadata.gid() != authority.gid
            || metadata.dev() != authority.dev
            || metadata.mode() & 0o777 != 0o500
            || fs::canonicalize(&directory).ok().as_deref() != Some(directory.as_path())
        {
            return Err(Closure);
        }
        let children = fs::read_dir(&directory).map_err(|_| Closure)?;
        // reject links and special nodes while retaining exact relative names
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

/// Hash one immutable member and retain its opened inode identity.
fn hash_member(
    path: &Path,
    authority: &ReleaseAuthority,
    mode: u32,
) -> Result<
    NativeMaintenanceCaptureNativeExecutableIdentity,
    NativeMaintenanceCaptureNativeInstallationError,
> {
    use NativeMaintenanceCaptureNativeInstallationError::Member;

    let before = fs::symlink_metadata(path).map_err(|_| Member)?;
    let mut file = open_read_no_follow(path).map_err(|_| Member)?;
    let opened = file.metadata().map_err(|_| Member)?;
    // prohibit links, aliases, foreign files, and role-mode drift
    if !valid_file(&before, &opened, authority, mode)
        || fs::canonicalize(path).ok().as_deref() != Some(path)
    {
        return Err(Member);
    }
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1_024];
    // stream each exact member without trusting its file size for allocation
    loop {
        let count = file.read(&mut buffer).map_err(|_| Member)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    let after = file.metadata().map_err(|_| Member)?;
    // reject mutation while the opened member was hashed
    if !same_file(&opened, &after) {
        return Err(Member);
    }
    Ok(NativeMaintenanceCaptureNativeExecutableIdentity {
        dev: opened.dev(),
        gid: opened.gid(),
        ino: opened.ino(),
        mode: opened.mode() & 0o777,
        path: path.to_owned(),
        sha256: lower_hex(hasher.finalize().into()),
        size: opened.size(),
        uid: opened.uid(),
    })
}

/// Compare path and descriptor metadata against the release authority.
fn valid_file(
    before: &fs::Metadata,
    opened: &fs::Metadata,
    authority: &ReleaseAuthority,
    mode: u32,
) -> bool {
    before.is_file()
        && opened.is_file()
        && !before.file_type().is_symlink()
        && same_file(before, opened)
        && opened.uid() == authority.uid
        && opened.gid() == authority.gid
        && opened.dev() == authority.dev
        && opened.nlink() == 1
        && opened.mode() & 0o777 == mode
}

/// Compare stable inode and mutation fields for one opened member.
fn same_file(left: &fs::Metadata, right: &fs::Metadata) -> bool {
    left.dev() == right.dev()
        && left.ino() == right.ino()
        && left.size() == right.size()
        && left.mode() == right.mode()
        && left.mtime() == right.mtime()
        && left.mtime_nsec() == right.mtime_nsec()
}

/// Bind one retained member identity to the actual executing inode metadata.
fn same_executable(
    identity: &NativeMaintenanceCaptureNativeExecutableIdentity,
    metadata: &fs::Metadata,
) -> bool {
    metadata.is_file()
        && identity.dev == metadata.dev()
        && identity.ino == metadata.ino()
        && identity.size == metadata.size()
        && identity.mode == metadata.mode() & 0o777
        && identity.uid == metadata.uid()
        && identity.gid == metadata.gid()
}

/// Admit the inherited exact relative manifest-member alphabet.
fn valid_manifest_path(path: &str) -> bool {
    let mut bytes = path.bytes();
    let Some(first) = bytes.next() else {
        return false;
    };
    first.is_ascii_alphanumeric()
        && bytes.all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'@' | b'/' | b'+' | b'-')
        })
        && !path.starts_with('/')
        && !path.split('/').any(|segment| matches!(segment, "." | ".."))
}

/// Admit one forty-character lowercase release source commit.
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

/// Render one SHA-256 digest in lowercase source spelling.
fn lower_hex(digest: [u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut value = String::with_capacity(64);
    // emit every digest byte without formatter allocation
    for byte in digest {
        value.push(char::from(HEX[usize::from(byte >> 4)]));
        value.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    value
}
