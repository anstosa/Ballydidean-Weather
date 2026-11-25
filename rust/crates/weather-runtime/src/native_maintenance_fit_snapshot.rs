//! Immutable native fitter code and canonical input snapshots on the reviewed tmpfs boundary.

use std::collections::BTreeMap;
use std::fmt::{self, Display, Formatter};
use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Read, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;
use sha2::{Digest, Sha256};
use weather_contracts::JsonValue;

use crate::api_maintenance_candidate_files::open_read_no_follow;
use crate::{
    MaintenanceCanonicalJson, NativeMaintenanceCaptureFitSourceIdentity,
    NativeMaintenanceCaptureReleaseGeneration, VerifiedNativeMaintenanceCaptureNativeInstallation,
    VerifiedNativeMaintenanceInstalledClosure, VerifiedNativeMaintenanceInstalledRelease,
};

const INPUTS_CONTRACT_VERSION: &str = "adjustment-fit-input-snapshots/v1";
const SNAPSHOT_CONTRACT_VERSION: &str = "adjustment-fit-snapshot/v1";
const RELEASES_ROOT: &str = "/home/ubuntu/.weather/adjustment-maintenance/runner/releases";
const MANIFEST_NAME: &str = "MANIFEST.sha256";
const NATIVE_WIND_EXECUTABLE: &str = "bin/weather-maintenance-wind-fit";
const NATIVE_FIT_EXECUTABLE: &str = "bin/weather-maintenance-fit";
const TMPFS_ROOT: &str = "/dev/shm";
const CODE_PREFIX: &str = "weather-adjustment-fit-code-";
const INPUT_PREFIX: &str = "weather-adjustment-fit-input-";
const TMPFS_MAGIC: libc::c_long = 0x0102_1994;
const CODE_MAXIMUM_BYTES: u64 = 64 * 1_024 * 1_024;
const INPUT_MAXIMUM_BYTES: u64 = 512 * 1_024 * 1_024;
const MANIFEST_MAXIMUM_BYTES: u64 = 64 * 1_024;
const CREATE_ATTEMPTS: u64 = 128;

static PRIVATE_KEY_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)(?:password|secret|credential|private.?key|access.?token|refresh.?token|ssh.?(?:auth|agent|key)|dbus|archive.?root|repository.?root)",
    )
    .expect("fixed private-key regex")
});
static PRIVATE_VALUE_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?:-----BEGIN [^-]*PRIVATE KEY-----|/(?:home|root|mnt)(?:/|$)|\.ssh(?:/|$)|SSH_AUTH_SOCK|GITHUB_TOKEN|GH_TOKEN|AWS_SECRET_ACCESS_KEY|DBUS_SESSION_BUS_ADDRESS)",
    )
    .expect("fixed private-value regex")
});

const RAIN_PYTHON: &[&str] = &[
    "build_moisture_targets.py",
    "build_rain_sub24.py",
    "export_moisture_history.py",
    "export_rain_wind_runtime.py",
    "rain_context.py",
    "rain_context_features.py",
    "rain_event_guard.py",
    "rain_hurdle.py",
    "rain_hurdle_calibration.py",
    "rain_ordinal.py",
    "rain_recency.py",
    "rain_recency_calibration.py",
    "rain_refresh.py",
    "rain_residual.py",
    "rain_search.py",
    "rain_sub24.py",
    "rain_trajectory.py",
    "rain_trajectory_features.py",
    "rain_wind.py",
    "rain_wind_features.py",
    "rain_wind_source.py",
    "retain_moisture_research.py",
    "run_rain_sub24.py",
];
const FORECAST_JAVASCRIPT: &[&str] = &[
    "algorithm-v1.js",
    "apply.js",
    "bootstrap-v1.js",
    "calendar.js",
    "candidate.js",
    "evaluate.js",
    "evidence.js",
    "holdout-ledger.js",
    "index.js",
    "maintenance-capture-epoch.js",
    "maintenance-policy.js",
    "maintenance-revision-projection.js",
    "maintenance-runtime-package.js",
    "maintenance-shadow-catalog.js",
    "maintenance-shadow-comparator.js",
    "maintenance-shadow-values.js",
    "performance-scorecard.js",
    "rain-hurdle-wind-artifact.js",
    "rain-hurdle-wind.js",
    "rain-maintenance-controls.js",
    "rain-fixed-gauge-target.js",
    "rain-runtime-registry.js",
    "runtime-bundle.js",
    "runtime-loader.js",
    "temperature-analog-research.js",
    "temperature-analog-shrinkage.js",
    "temperature-analog-stress-validation.js",
    "temperature-analog-validation.js",
    "temperature-canary.js",
    "temperature-causal-guard.js",
    "temperature-frozen-replay.js",
    "temperature-lead-research.js",
    "temperature-live-replay-events.js",
    "temperature-mos-runtime.js",
    "temperature-nowcast-research.js",
    "temperature-weather-research.js",
    "wind-canary.js",
];
const DOMAIN_JAVASCRIPT: &[&str] = &[
    "forecast-adjustment.js",
    "forecast-anchor-record.js",
    "index.js",
    "ingestion.js",
    "provenance.js",
    "rain-collection.js",
    "weather-record.js",
];

/// One closed fitter family accepted by the isolated launcher.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMaintenanceFitFamily {
    Rain,
    Temperature,
    Wind,
}

impl NativeMaintenanceFitFamily {
    /// Return the exact source family spelling.
    pub const fn as_str(self) -> &'static str {
        // retain the closed source family vocabulary
        match self {
            Self::Rain => "rain",
            Self::Temperature => "temperature",
            Self::Wind => "wind",
        }
    }

    /// Return the exact canonical input filename mounted for the fitter.
    pub const fn input_filename(self) -> &'static str {
        // bind each family to its sole reviewed input member
        match self {
            Self::Rain => "rain.json",
            Self::Temperature => "temperature.json",
            Self::Wind => "wind.json",
        }
    }

    /// Return the exact native relative code entry used by the launcher.
    pub const fn entry_relative_path(self) -> &'static str {
        // select the actual native fitter instead of an interpreter entry
        match self {
            Self::Rain | Self::Temperature => NATIVE_FIT_EXECUTABLE,
            Self::Wind => NATIVE_WIND_EXECUTABLE,
        }
    }
}

/// The two source snapshot roles admitted by exact root-prefix inspection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMaintenanceFitSnapshotKind {
    Code,
    Input,
}

impl NativeMaintenanceFitSnapshotKind {
    /// Return the exact source role spelling.
    pub const fn as_str(self) -> &'static str {
        // retain the two closed snapshot kinds
        match self {
            Self::Code => "code",
            Self::Input => "input",
        }
    }

    /// Return the required direct tmpfs child prefix.
    const fn prefix(self) -> &'static str {
        // bind role identity to the original direct-child namespace
        match self {
            Self::Code => CODE_PREFIX,
            Self::Input => INPUT_PREFIX,
        }
    }
}

/// Source-exact immutable snapshot census and tree identity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeMaintenanceFitSnapshotIdentity {
    pub contract_version: &'static str,
    pub file_count: u64,
    pub kind: NativeMaintenanceFitSnapshotKind,
    pub root_sha256: String,
    pub total_bytes: u64,
}

/// Installed generation and actual code identity bound to one code snapshot.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeMaintenanceFitCodeProof {
    pub fit_executable_sha256: Option<String>,
    pub generation: NativeMaintenanceCaptureReleaseGeneration,
    pub manifest_sha256: String,
    pub members: Vec<NativeMaintenanceFitCodeMemberProof>,
    pub source_commit: String,
    pub wind_executable_sha256: Option<String>,
}

/// One actual installed source and immutable snapshot destination binding.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeMaintenanceFitCodeMemberProof {
    pub destination_sha256: String,
    pub destination_mode: u32,
    pub destination_path: String,
    pub source_path: String,
    pub source_sha256: String,
}

/// Path-free closed snapshot construction or inspection failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMaintenanceFitSnapshotError {
    Release,
    Manifest,
    Source,
    Input,
    PrivateMaterial,
    Capacity,
    Directory,
    File,
    Snapshot,
    Filesystem,
    Cleanup,
}

impl Display for NativeMaintenanceFitSnapshotError {
    /// Render one stable operation category without host paths or input material.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Release => "fit source release is invalid",
            Self::Manifest => "fit source manifest is invalid",
            Self::Source => "fit source member is invalid",
            Self::Input => "fit input is invalid",
            Self::PrivateMaterial => "fit input contains private material",
            Self::Capacity => "fit snapshot byte ceiling exceeded",
            Self::Directory => "fit snapshot directory is invalid",
            Self::File => "fit snapshot file operation failed",
            Self::Snapshot => "fit snapshot identity is invalid",
            Self::Filesystem => "fit snapshot filesystem is invalid",
            Self::Cleanup => "fit snapshot cleanup failed",
        })
    }
}

impl std::error::Error for NativeMaintenanceFitSnapshotError {}

/// Installed release authority retained for repeated bounded snapshot operations.
pub struct NativeMaintenanceFitSnapshotOwner {
    release_root: PathBuf,
    release: VerifiedNativeMaintenanceInstalledRelease,
    source_members: BTreeMap<String, String>,
}

impl NativeMaintenanceFitSnapshotOwner {
    /// Bind an independently verified predecessor release for rain and temperature fits.
    pub fn node_predecessor(
        installed: &VerifiedNativeMaintenanceInstalledClosure,
    ) -> Result<Self, NativeMaintenanceFitSnapshotError> {
        Self::from_verified(
            Path::new(RELEASES_ROOT).join(&installed.release.source_commit),
            &installed.release,
            &installed.fit_source_members,
            NativeMaintenanceCaptureReleaseGeneration::NodePredecessor,
        )
    }

    /// Bind an independently verified native release including the native wind generation.
    pub fn native(
        installed: &VerifiedNativeMaintenanceCaptureNativeInstallation,
    ) -> Result<Self, NativeMaintenanceFitSnapshotError> {
        Self::from_verified(
            Path::new(RELEASES_ROOT).join(&installed.release.source_commit),
            &installed.release,
            &installed.fit_source_members,
            NativeMaintenanceCaptureReleaseGeneration::Native,
        )
    }

    /// Build one exclusive code/input pair and inspect both immutable trees.
    pub fn create_snapshots(
        &self,
        family: NativeMaintenanceFitFamily,
        input: &JsonValue,
        codec: &mut impl MaintenanceCanonicalJson,
    ) -> Result<NativeMaintenanceFitSnapshots, NativeMaintenanceFitSnapshotError> {
        reject_private_material(input)?;
        let input_bytes = codec
            .encode_canonical_json(input)
            .ok_or(NativeMaintenanceFitSnapshotError::Input)?;
        // require the exact canonical line framing before any filesystem allocation
        if !input_bytes.ends_with(b"\n") {
            return Err(NativeMaintenanceFitSnapshotError::Input);
        }
        // refuse oversized decoded input before allocating either private root
        if input_bytes.len() as u64 > INPUT_MAXIMUM_BYTES {
            return Err(NativeMaintenanceFitSnapshotError::Capacity);
        }
        let manifest = self.verify_release_manifest()?;
        // prohibit the predecessor generation from reaching the removed Node wind entry
        if family == NativeMaintenanceFitFamily::Wind
            && self.release.generation != NativeMaintenanceCaptureReleaseGeneration::Native
        {
            return Err(NativeMaintenanceFitSnapshotError::Release);
        }
        let code_root = IssuedSnapshotRoot::create(NativeMaintenanceFitSnapshotKind::Code)?;
        let input_root = IssuedSnapshotRoot::create(NativeMaintenanceFitSnapshotKind::Input)?;
        let copied = self.copy_code(family, &manifest, code_root.path())?;
        write_immutable_file(
            input_root.path(),
            family.input_filename(),
            &input_bytes,
            0o400,
        )?;
        let code_snapshot = inspect_native_maintenance_fit_snapshot(
            code_root.path(),
            NativeMaintenanceFitSnapshotKind::Code,
        )?;
        let input_snapshot = inspect_native_maintenance_fit_snapshot(
            input_root.path(),
            NativeMaintenanceFitSnapshotKind::Input,
        )?;
        Ok(NativeMaintenanceFitSnapshots {
            code_proof: NativeMaintenanceFitCodeProof {
                fit_executable_sha256: copied.fit_executable_sha256,
                generation: self.release.generation,
                manifest_sha256: self.release.manifest_sha256.clone(),
                members: copied.members,
                source_commit: self.release.source_commit.clone(),
                wind_executable_sha256: copied.wind_executable_sha256,
            },
            code_root,
            code_snapshot,
            family,
            input_root,
            input_snapshot,
        })
    }

    /// Construct an isolated test owner without weakening either production constructor.
    #[cfg(test)]
    pub(crate) fn for_fixture(
        release_root: PathBuf,
        release: VerifiedNativeMaintenanceInstalledRelease,
        source_members: Vec<NativeMaintenanceCaptureFitSourceIdentity>,
    ) -> Result<Self, NativeMaintenanceFitSnapshotError> {
        let generation = release.generation;
        Self::from_verified(release_root, &release, &source_members, generation)
    }

    /// Validate the independently proved generation and retain unique source bindings.
    fn from_verified(
        release_root: PathBuf,
        release: &VerifiedNativeMaintenanceInstalledRelease,
        source_members: &[NativeMaintenanceCaptureFitSourceIdentity],
        expected_generation: NativeMaintenanceCaptureReleaseGeneration,
    ) -> Result<Self, NativeMaintenanceFitSnapshotError> {
        // reject cross-generation receipts and malformed fixed identities
        if release.generation != expected_generation
            || !valid_commit(&release.source_commit)
            || !valid_hash(&release.manifest_sha256)
            || release.member_count == 0
        {
            return Err(NativeMaintenanceFitSnapshotError::Release);
        }
        let mut retained = BTreeMap::new();
        // preserve every independently verified source identity exactly once
        for member in source_members {
            // reject malformed or duplicate fitted-source bindings
            if !valid_snapshot_relative_path(&member.path)
                || !valid_hash(&member.sha256)
                || retained
                    .insert(member.path.clone(), member.sha256.clone())
                    .is_some()
            {
                return Err(NativeMaintenanceFitSnapshotError::Source);
            }
        }
        Ok(Self {
            release_root,
            release: release.clone(),
            source_members: retained,
        })
    }

    /// Re-read the immutable installed manifest and bind it to the supplied closure proof.
    fn verify_release_manifest(
        &self,
    ) -> Result<BTreeMap<String, String>, NativeMaintenanceFitSnapshotError> {
        let authority = admit_release_root(&self.release_root, &self.release.source_commit)?;
        let manifest_path = self.release_root.join(MANIFEST_NAME);
        let bytes = read_release_member(&manifest_path, &authority, 0o400, MANIFEST_MAXIMUM_BYTES)?;
        // reject a changed manifest before trusting any member path from it
        if sha256(&bytes) != self.release.manifest_sha256 {
            return Err(NativeMaintenanceFitSnapshotError::Manifest);
        }
        let manifest = parse_manifest(&bytes)?;
        // retain the independently verified full member census
        if manifest.len() != self.release.member_count {
            return Err(NativeMaintenanceFitSnapshotError::Manifest);
        }
        Ok(manifest)
    }

    /// Copy one closed family population and retain every actual installed binding.
    fn copy_code(
        &self,
        family: NativeMaintenanceFitFamily,
        manifest: &BTreeMap<String, String>,
        code_root: &Path,
    ) -> Result<CopiedCodeProof, NativeMaintenanceFitSnapshotError> {
        let copies = fit_code_files(family, self.release.generation);
        let mut copied_bytes = 0_u64;
        let mut proof = CopiedCodeProof::default();
        // copy only the closed source population through stable no-follow descriptors
        for copy in copies {
            let expected = manifest
                .get(&copy.source)
                .ok_or(NativeMaintenanceFitSnapshotError::Manifest)?;
            // inherited numerical sources must match the independent fitted-source proof
            if copy.source_mode == 0o400 && self.source_members.get(&copy.source) != Some(expected)
            {
                return Err(NativeMaintenanceFitSnapshotError::Source);
            }
            let authority = admit_release_root(&self.release_root, &self.release.source_commit)?;
            let bytes = read_release_member(
                &self.release_root.join(&copy.source),
                &authority,
                copy.source_mode,
                CODE_MAXIMUM_BYTES,
            )?;
            // bind copied bytes to the manifest rather than a caller-supplied digest
            if sha256(&bytes) != *expected {
                return Err(NativeMaintenanceFitSnapshotError::Source);
            }
            // retain the independently verified binary or unchanged predecessor source bytes
            let destination_bytes = bytes;
            copied_bytes = copied_bytes
                .checked_add(destination_bytes.len() as u64)
                .ok_or(NativeMaintenanceFitSnapshotError::Capacity)?;
            // stop an expanded source population before it becomes usable
            if copied_bytes > CODE_MAXIMUM_BYTES {
                return Err(NativeMaintenanceFitSnapshotError::Capacity);
            }
            write_immutable_file(
                code_root,
                &copy.destination,
                &destination_bytes,
                copy.destination_mode,
            )?;
            proof.members.push(NativeMaintenanceFitCodeMemberProof {
                destination_sha256: sha256(&destination_bytes),
                destination_mode: copy.destination_mode,
                destination_path: copy.destination.clone(),
                source_path: copy.source.clone(),
                source_sha256: expected.clone(),
            });
            // expose each native executable under its distinct reviewed role
            if copy.source == NATIVE_WIND_EXECUTABLE {
                proof.wind_executable_sha256 = Some(expected.clone());
            } else if copy.source == NATIVE_FIT_EXECUTABLE {
                proof.fit_executable_sha256 = Some(expected.clone());
            }
        }
        Ok(proof)
    }
}

/// One issued pair whose private roots cannot be copied or caller-constructed.
pub struct NativeMaintenanceFitSnapshots {
    code_proof: NativeMaintenanceFitCodeProof,
    code_root: IssuedSnapshotRoot,
    code_snapshot: NativeMaintenanceFitSnapshotIdentity,
    family: NativeMaintenanceFitFamily,
    input_root: IssuedSnapshotRoot,
    input_snapshot: NativeMaintenanceFitSnapshotIdentity,
}

impl NativeMaintenanceFitSnapshots {
    /// Return the exact issued code root for read-only launcher binding.
    pub fn code_root(&self) -> &Path {
        self.code_root.path()
    }

    /// Return the inspected immutable code-tree identity.
    pub const fn code_snapshot(&self) -> &NativeMaintenanceFitSnapshotIdentity {
        &self.code_snapshot
    }

    /// Return the installed release and optional native executable proof.
    pub const fn code_proof(&self) -> &NativeMaintenanceFitCodeProof {
        &self.code_proof
    }

    /// Return the fixed family selected for this pair.
    pub const fn family(&self) -> NativeMaintenanceFitFamily {
        self.family
    }

    /// Return the exact source pair contract version.
    pub const fn contract_version(&self) -> &'static str {
        INPUTS_CONTRACT_VERSION
    }

    /// Return the exact issued input root for read-only launcher binding.
    pub fn input_root(&self) -> &Path {
        self.input_root.path()
    }

    /// Return the inspected immutable canonical-input tree identity.
    pub const fn input_snapshot(&self) -> &NativeMaintenanceFitSnapshotIdentity {
        &self.input_snapshot
    }

    /// Remove both exact issued roots and report any cleanup refusal.
    pub fn close(mut self) -> Result<(), NativeMaintenanceFitSnapshotError> {
        // close the untrusted input before the code snapshot in source order
        let input_failed = self.input_root.close().is_err();
        // close the code root even when input cleanup failed
        let code_failed = self.code_root.close().is_err();
        // surface cleanup refusal without retaining either issued authority
        if input_failed || code_failed {
            return Err(NativeMaintenanceFitSnapshotError::Cleanup);
        }
        Ok(())
    }
}

struct SourceCopy {
    destination: String,
    destination_mode: u32,
    source: String,
    source_mode: u32,
}

impl SourceCopy {
    /// Declare one inherited immutable source-to-snapshot copy.
    fn inherited(source: impl Into<String>, destination: impl Into<String>) -> Self {
        Self {
            destination: destination.into(),
            destination_mode: 0o400,
            source: source.into(),
            source_mode: 0o400,
        }
    }

    /// Declare one literal installed native executable copy.
    fn native_executable(path: &'static str) -> Self {
        Self {
            destination: path.to_owned(),
            destination_mode: 0o500,
            source: path.to_owned(),
            source_mode: 0o500,
        }
    }
}

#[derive(Default)]
struct CopiedCodeProof {
    fit_executable_sha256: Option<String>,
    members: Vec<NativeMaintenanceFitCodeMemberProof>,
    wind_executable_sha256: Option<String>,
}

/// Return one exact inherited allowlist or the selected native fitter binary.
fn fit_code_files(
    family: NativeMaintenanceFitFamily,
    generation: NativeMaintenanceCaptureReleaseGeneration,
) -> Vec<SourceCopy> {
    // the native generation never copies an interpreter application closure
    if generation == NativeMaintenanceCaptureReleaseGeneration::Native {
        return vec![SourceCopy::native_executable(family.entry_relative_path())];
    }
    let mut files = Vec::new();
    // retain every reviewed numerical Python source in its original package location
    if family == NativeMaintenanceFitFamily::Rain {
        files.extend(RAIN_PYTHON.iter().map(|name| {
            let path = format!("scripts/research/{name}");
            SourceCopy::inherited(path.clone(), path)
        }));
    } else {
        files.extend([
            SourceCopy::inherited(
                "scripts/research/temperature_refresh.py",
                "scripts/research/temperature_refresh.py",
            ),
            SourceCopy::inherited(
                "scripts/research/temperature_shortlead_models.py",
                "scripts/research/temperature_shortlead_models.py",
            ),
            SourceCopy::inherited(
                "scripts/research/temperature_seasonal_ridge.py",
                "scripts/research/temperature_seasonal_ridge.py",
            ),
        ]);
    }
    files.push(SourceCopy::inherited(
        "packages/forecast-adjustment/package.json",
        "packages/forecast-adjustment/package.json",
    ));
    // retain only the two temperature helpers or the complete rain package
    let forecast = if family == NativeMaintenanceFitFamily::Temperature {
        &["calendar.js", "temperature-mos-runtime.js"][..]
    } else {
        FORECAST_JAVASCRIPT
    };
    // reproduce the source package mapping into its ordinary dist tree
    for name in forecast {
        let source = format!("scripts/research/adjustment-maintenance-runtime/forecast/{name}");
        let destination = format!("packages/forecast-adjustment/dist/{name}");
        files.push(SourceCopy::inherited(source, destination));
    }
    // rain alone retains the domain package required by its JavaScript helpers
    if family == NativeMaintenanceFitFamily::Rain {
        files.push(SourceCopy::inherited(
            "packages/domain/package.json",
            "node_modules/@weather/domain/package.json",
        ));
        // reproduce every reviewed domain runtime member in source order
        for name in DOMAIN_JAVASCRIPT {
            let source = format!(
                "scripts/research/adjustment-maintenance-runtime/node_modules/@weather/domain/dist/{name}"
            );
            let destination = format!("node_modules/@weather/domain/dist/{name}");
            files.push(SourceCopy::inherited(source, destination));
        }
    }
    files
}

struct IssuedSnapshotRoot {
    kind: NativeMaintenanceFitSnapshotKind,
    path: PathBuf,
    retained: bool,
}

impl IssuedSnapshotRoot {
    /// Create one direct owner-private directory on native shared-memory tmpfs.
    fn create(
        kind: NativeMaintenanceFitSnapshotKind,
    ) -> Result<Self, NativeMaintenanceFitSnapshotError> {
        let root = Path::new(TMPFS_ROOT);
        // require the original native tmpfs before allocating any snapshot member
        if filesystem_type(root)? != TMPFS_MAGIC {
            return Err(NativeMaintenanceFitSnapshotError::Filesystem);
        }
        // retry only bounded random names on exclusive creation collisions
        for _ in 0..CREATE_ATTEMPTS {
            let mut entropy = [0_u8; 16];
            getrandom::fill(&mut entropy)
                .map_err(|_| NativeMaintenanceFitSnapshotError::Directory)?;
            let path = root.join(format!("{}{}", kind.prefix(), lower_hex_16(entropy)));
            let mut builder = fs::DirBuilder::new();
            builder.mode(0o700);
            match builder.create(&path) {
                Ok(()) => {
                    return Ok(Self {
                        kind,
                        path,
                        retained: true,
                    });
                }
                Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
                Err(_) => return Err(NativeMaintenanceFitSnapshotError::Directory),
            }
        }
        Err(NativeMaintenanceFitSnapshotError::Directory)
    }

    /// Borrow the exact uncopyable issued root path.
    fn path(&self) -> &Path {
        &self.path
    }

    /// Remove only this exact issued direct tmpfs child.
    fn close(&mut self) -> Result<(), NativeMaintenanceFitSnapshotError> {
        // make explicit close and drop idempotent for this sole owner
        if !self.retained {
            return Ok(());
        }
        // fail closed if internal authority no longer names its issued root namespace
        if self.path.parent() != Some(Path::new(TMPFS_ROOT))
            || !self
                .path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with(self.kind.prefix()))
        {
            return Err(NativeMaintenanceFitSnapshotError::Cleanup);
        }
        match fs::remove_dir_all(&self.path) {
            Ok(()) => {
                self.retained = false;
                Ok(())
            }
            Err(error) if error.kind() == ErrorKind::NotFound => {
                self.retained = false;
                Ok(())
            }
            Err(_) => Err(NativeMaintenanceFitSnapshotError::Cleanup),
        }
    }
}

impl Drop for IssuedSnapshotRoot {
    /// Best-effort close the exact issued root on partial construction or unwind.
    fn drop(&mut self) {
        let _ = self.close();
    }
}

struct ReleaseAuthority {
    dev: u64,
    gid: u32,
    uid: u32,
}

/// Admit one immutable release directory already named by a verified source commit.
fn admit_release_root(
    root: &Path,
    source_commit: &str,
) -> Result<ReleaseAuthority, NativeMaintenanceFitSnapshotError> {
    let details =
        fs::symlink_metadata(root).map_err(|_| NativeMaintenanceFitSnapshotError::Release)?;
    // bind the literal root name, ownership, immutable mode and canonical identity
    if !details.is_dir()
        || details.file_type().is_symlink()
        || root.file_name().and_then(|name| name.to_str()) != Some(source_commit)
        || details.mode() & 0o777 != 0o500
        || fs::canonicalize(root).ok().as_deref() != Some(root)
    {
        return Err(NativeMaintenanceFitSnapshotError::Release);
    }
    Ok(ReleaseAuthority {
        dev: details.dev(),
        gid: details.gid(),
        uid: details.uid(),
    })
}

/// Read one stable immutable installed member through a no-follow descriptor.
fn read_release_member(
    path: &Path,
    authority: &ReleaseAuthority,
    mode: u32,
    maximum_bytes: u64,
) -> Result<Vec<u8>, NativeMaintenanceFitSnapshotError> {
    let before =
        fs::symlink_metadata(path).map_err(|_| NativeMaintenanceFitSnapshotError::Source)?;
    let mut file =
        open_read_no_follow(path).map_err(|_| NativeMaintenanceFitSnapshotError::Source)?;
    let opened = file
        .metadata()
        .map_err(|_| NativeMaintenanceFitSnapshotError::Source)?;
    // prohibit aliases, links, foreign members, mutable modes and oversized reads
    if !same_release_file(&before, &opened, authority, mode)
        || opened.len() > maximum_bytes
        || fs::canonicalize(path).ok().as_deref() != Some(path)
    {
        return Err(NativeMaintenanceFitSnapshotError::Source);
    }
    let capacity =
        usize::try_from(opened.len()).map_err(|_| NativeMaintenanceFitSnapshotError::Capacity)?;
    let mut bytes = Vec::with_capacity(capacity);
    file.read_to_end(&mut bytes)
        .map_err(|_| NativeMaintenanceFitSnapshotError::Source)?;
    let after = file
        .metadata()
        .map_err(|_| NativeMaintenanceFitSnapshotError::Source)?;
    // reject replacement or mutation across the complete bounded read
    if !same_file(&opened, &after) || bytes.len() as u64 != opened.len() {
        return Err(NativeMaintenanceFitSnapshotError::Source);
    }
    Ok(bytes)
}

/// Compare one member against its release root and opened descriptor authority.
fn same_release_file(
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

/// Compare the mutation-sensitive metadata observed through one held descriptor.
fn same_file(left: &fs::Metadata, right: &fs::Metadata) -> bool {
    left.dev() == right.dev()
        && left.ino() == right.ino()
        && left.len() == right.len()
        && left.mode() == right.mode()
        && left.mtime() == right.mtime()
        && left.mtime_nsec() == right.mtime_nsec()
}

/// Parse one exact installed manifest without path normalization or duplicate overwrite.
fn parse_manifest(
    bytes: &[u8],
) -> Result<BTreeMap<String, String>, NativeMaintenanceFitSnapshotError> {
    let source =
        std::str::from_utf8(bytes).map_err(|_| NativeMaintenanceFitSnapshotError::Manifest)?;
    let body = source
        .strip_suffix('\n')
        .ok_or(NativeMaintenanceFitSnapshotError::Manifest)?;
    let mut entries = BTreeMap::new();
    // validate every exact relative binding before exposing any member path
    for line in body.split('\n') {
        let (digest, path) = line
            .split_once("  ")
            .ok_or(NativeMaintenanceFitSnapshotError::Manifest)?;
        // reject malformed digests, paths and duplicate member identities
        if !valid_hash(digest)
            || !valid_manifest_path(path)
            || entries.insert(path.to_owned(), digest.to_owned()).is_some()
        {
            return Err(NativeMaintenanceFitSnapshotError::Manifest);
        }
    }
    // prohibit an empty installed source closure
    if entries.is_empty() {
        return Err(NativeMaintenanceFitSnapshotError::Manifest);
    }
    Ok(entries)
}

/// Write one new immutable regular file beneath an already issued root.
fn write_immutable_file(
    root: &Path,
    relative: &str,
    bytes: &[u8],
    mode: u32,
) -> Result<(), NativeMaintenanceFitSnapshotError> {
    // refuse alternate relative spellings even for compile-time allowlist entries
    if !valid_snapshot_relative_path(relative) || !matches!(mode, 0o400 | 0o500) {
        return Err(NativeMaintenanceFitSnapshotError::File);
    }
    let destination = root.join(relative);
    let parent = Path::new(relative)
        .parent()
        .ok_or(NativeMaintenanceFitSnapshotError::Directory)?;
    create_private_parents(root, parent)?;
    let mut options = OpenOptions::new();
    options
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW);
    let mut file = options
        .open(&destination)
        .map_err(|_| NativeMaintenanceFitSnapshotError::File)?;
    file.write_all(bytes)
        .map_err(|_| NativeMaintenanceFitSnapshotError::File)?;
    file.sync_all()
        .map_err(|_| NativeMaintenanceFitSnapshotError::File)?;
    fs::set_permissions(&destination, fs::Permissions::from_mode(mode))
        .map_err(|_| NativeMaintenanceFitSnapshotError::File)
}

/// Create and verify every fixed private destination directory component.
fn create_private_parents(
    root: &Path,
    relative: &Path,
) -> Result<(), NativeMaintenanceFitSnapshotError> {
    // retain root-level files without constructing an alternate directory
    if relative.as_os_str().is_empty() {
        return Ok(());
    }
    let mut current = root.to_owned();
    // construct each static relative component beneath the issued root
    for component in relative.components() {
        let std::path::Component::Normal(component) = component else {
            return Err(NativeMaintenanceFitSnapshotError::Directory);
        };
        current.push(component);
        match fs::DirBuilder::new().mode(0o700).create(&current) {
            Ok(()) => {}
            Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
            Err(_) => return Err(NativeMaintenanceFitSnapshotError::Directory),
        }
        let details = fs::symlink_metadata(&current)
            .map_err(|_| NativeMaintenanceFitSnapshotError::Directory)?;
        // require ordinary owner-private directories instead of substituted links
        if !details.is_dir()
            || details.file_type().is_symlink()
            || details.uid() != current_uid()
            || details.mode() & 0o777 != 0o700
        {
            return Err(NativeMaintenanceFitSnapshotError::Directory);
        }
    }
    Ok(())
}

/// Inspect one source-compatible immutable direct tmpfs snapshot.
pub fn inspect_native_maintenance_fit_snapshot(
    root: &Path,
    kind: NativeMaintenanceFitSnapshotKind,
) -> Result<NativeMaintenanceFitSnapshotIdentity, NativeMaintenanceFitSnapshotError> {
    admit_snapshot_root(root, kind)?;
    let mut records = Vec::new();
    let mut pending = vec![root.to_owned()];
    let mut file_count = 0_u64;
    let mut total_bytes = 0_u64;
    // walk every literal directory without following entry links
    while let Some(directory) = pending.pop() {
        let entries = fs::read_dir(&directory)
            .map_err(|_| NativeMaintenanceFitSnapshotError::Snapshot)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| NativeMaintenanceFitSnapshotError::Snapshot)?;
        // inspect every child through lstat rather than dirent hints
        for entry in entries {
            let path = entry.path();
            let details = fs::symlink_metadata(&path)
                .map_err(|_| NativeMaintenanceFitSnapshotError::Snapshot)?;
            let relative = relative_path(root, &path)?;
            // retain only nodes owned by the current service identity
            if details.uid() != current_uid() {
                return Err(NativeMaintenanceFitSnapshotError::Snapshot);
            }
            // retain owner-private directories for complete mode hashing
            if details.is_dir() && !details.file_type().is_symlink() {
                // reject broad directory access before recording its mode
                if details.mode() & 0o077 != 0 {
                    return Err(NativeMaintenanceFitSnapshotError::Snapshot);
                }
                records.push(TreeRecord::Directory {
                    mode: details.mode() & 0o777,
                    path: relative,
                });
                pending.push(path);
                continue;
            }
            // hash only owner-private single-link regular files
            if details.is_file() && !details.file_type().is_symlink() {
                // reject hard links and nonprivate file modes
                if details.nlink() != 1 || details.mode() & 0o066 != 0 {
                    return Err(NativeMaintenanceFitSnapshotError::Snapshot);
                }
                let digest = hash_snapshot_file(&path, &details)?;
                records.push(TreeRecord::File {
                    mode: details.mode() & 0o777,
                    nlink: details.nlink(),
                    path: relative,
                    sha256: digest,
                    size: details.size(),
                });
                file_count = file_count
                    .checked_add(1)
                    .ok_or(NativeMaintenanceFitSnapshotError::Snapshot)?;
                total_bytes = total_bytes
                    .checked_add(details.size())
                    .ok_or(NativeMaintenanceFitSnapshotError::Snapshot)?;
                continue;
            }
            return Err(NativeMaintenanceFitSnapshotError::Snapshot);
        }
    }
    // fail closed outside the independently proved pinned Node path collation profile
    if records.iter().any(|record| {
        !crate::native_fit_ascii_collation::native_fit_collation_admits(record.path())
    }) {
        return Err(NativeMaintenanceFitSnapshotError::Snapshot);
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
    // retain the source half-gibibyte ceiling for raw input trees
    if kind == NativeMaintenanceFitSnapshotKind::Input && total_bytes > INPUT_MAXIMUM_BYTES {
        return Err(NativeMaintenanceFitSnapshotError::Capacity);
    }
    Ok(NativeMaintenanceFitSnapshotIdentity {
        contract_version: SNAPSHOT_CONTRACT_VERSION,
        file_count,
        kind,
        root_sha256: sha256(records_json.as_bytes()),
        total_bytes,
    })
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
}

impl TreeRecord {
    /// Borrow the admitted relative path used by source collation.
    fn path(&self) -> &str {
        // both record kinds retain one exact relative path
        match self {
            Self::Directory { path, .. } | Self::File { path, .. } => path,
        }
    }

    /// Encode the exact recursively sorted source JSON record.
    fn canonical_json(&self) -> String {
        // retain source key order and decimal metadata spelling for both record kinds
        match self {
            Self::Directory { mode, path } => {
                format!(r#"{{"kind":"directory","mode":{mode},"path":"{path}"}}"#)
            }
            Self::File {
                mode,
                nlink,
                path,
                sha256,
                size,
            } => format!(
                r#"{{"kind":"file","mode":{mode},"nlink":{nlink},"path":"{path}","sha256":"{sha256}","size":{size}}}"#
            ),
        }
    }
}

/// Admit one direct owner-only snapshot root on Linux tmpfs.
fn admit_snapshot_root(
    root: &Path,
    kind: NativeMaintenanceFitSnapshotKind,
) -> Result<(), NativeMaintenanceFitSnapshotError> {
    let absolute = root
        .canonicalize()
        .map_err(|_| NativeMaintenanceFitSnapshotError::Snapshot)?;
    let details =
        fs::symlink_metadata(root).map_err(|_| NativeMaintenanceFitSnapshotError::Snapshot)?;
    let valid_name = root
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with(kind.prefix()));
    // prohibit aliases, nesting, links, foreign roots and non-tmpfs storage
    if !root.is_absolute()
        || root.parent() != Some(Path::new(TMPFS_ROOT))
        || !valid_name
        || !details.is_dir()
        || details.file_type().is_symlink()
        || details.uid() != current_uid()
        || details.nlink() < 2
        || details.mode() & 0o777 != 0o700
        || absolute != root
        || filesystem_type(root)? != TMPFS_MAGIC
    {
        return Err(NativeMaintenanceFitSnapshotError::Snapshot);
    }
    Ok(())
}

/// Hash one stable snapshot regular file through a no-follow descriptor.
fn hash_snapshot_file(
    path: &Path,
    before: &fs::Metadata,
) -> Result<String, NativeMaintenanceFitSnapshotError> {
    let mut file =
        open_read_no_follow(path).map_err(|_| NativeMaintenanceFitSnapshotError::Snapshot)?;
    let opened = file
        .metadata()
        .map_err(|_| NativeMaintenanceFitSnapshotError::Snapshot)?;
    // bind the opened file to the lstat observation before reading any bytes
    if !same_file(before, &opened) {
        return Err(NativeMaintenanceFitSnapshotError::Snapshot);
    }
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 16 * 1_024];
    // stream the exact file without allocating according to untrusted metadata
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|_| NativeMaintenanceFitSnapshotError::Snapshot)?;
        // stop only after the held descriptor reaches eof
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    let after = file
        .metadata()
        .map_err(|_| NativeMaintenanceFitSnapshotError::Snapshot)?;
    // reject mutation across the complete digest operation
    if !same_file(&opened, &after) {
        return Err(NativeMaintenanceFitSnapshotError::Snapshot);
    }
    Ok(lower_hex(hasher.finalize().into()))
}

/// Reject private field identities and host-private string leaves recursively.
fn reject_private_material(value: &JsonValue) -> Result<(), NativeMaintenanceFitSnapshotError> {
    // inspect each retained array member without changing semantic order
    if let JsonValue::Array(values) = value {
        // recurse through every retained array value
        for value in values {
            reject_private_material(value)?;
        }
        return Ok(());
    }
    // inspect every object key and nested value before serialization
    if let JsonValue::Object(fields) = value {
        // validate every generated key before descending into its value
        for (key, value) in fields {
            let key = String::from_utf16_lossy(key);
            // reject private control material by field identity
            if PRIVATE_KEY_PATTERN.is_match(&key) {
                return Err(NativeMaintenanceFitSnapshotError::PrivateMaterial);
            }
            reject_private_material(value)?;
        }
        return Ok(());
    }
    // reject host paths and credential markers in string leaves
    if let JsonValue::String(units) = value {
        let value = String::from_utf16_lossy(units);
        // reject source-private host markers in any string leaf
        if PRIVATE_VALUE_PATTERN.is_match(&value) {
            return Err(NativeMaintenanceFitSnapshotError::PrivateMaterial);
        }
    }
    // reject nonfinite generated numbers before codec-specific serialization
    if matches!(value, JsonValue::Number(number) if !number.is_finite()) {
        return Err(NativeMaintenanceFitSnapshotError::Input);
    }
    Ok(())
}

/// Convert one literal descendant path to the source portable relative spelling.
fn relative_path(root: &Path, path: &Path) -> Result<String, NativeMaintenanceFitSnapshotError> {
    let relative = path
        .strip_prefix(root)
        .map_err(|_| NativeMaintenanceFitSnapshotError::Snapshot)?;
    let value = relative
        .to_str()
        .ok_or(NativeMaintenanceFitSnapshotError::Snapshot)?
        .to_owned();
    // prohibit platform separators and alternate relative spellings
    if !valid_snapshot_relative_path(&value) {
        return Err(NativeMaintenanceFitSnapshotError::Snapshot);
    }
    Ok(value)
}

/// Admit the source portable relative member alphabet and prohibit traversal.
fn valid_snapshot_relative_path(value: &str) -> bool {
    let mut components = value.split('/');
    // require every nonempty component to use the exact source alphabet
    components.all(|component| {
        !component.is_empty()
            && !matches!(component, "." | "..")
            && component.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'@' | b'-')
            })
    })
}

/// Admit the installed manifest alphabet including its reviewed plus character.
fn valid_manifest_path(value: &str) -> bool {
    let mut components = value.split('/');
    // prohibit empty and traversal components under the broader manifest alphabet
    components.all(|component| {
        !component.is_empty()
            && !matches!(component, "." | "..")
            && component.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'@' | b'+' | b'-')
            })
    })
}

/// Read one filesystem type without following a caller-controlled member.
fn filesystem_type(path: &Path) -> Result<libc::c_long, NativeMaintenanceFitSnapshotError> {
    use std::os::unix::ffi::OsStrExt;

    let path = std::ffi::CString::new(path.as_os_str().as_bytes())
        .map_err(|_| NativeMaintenanceFitSnapshotError::Filesystem)?;
    let mut details = std::mem::MaybeUninit::<libc::statfs>::uninit();
    // safety: statfs initializes the output only after a successful return
    let result = unsafe { libc::statfs(path.as_ptr(), details.as_mut_ptr()) };
    // reject an unavailable filesystem observation
    if result != 0 {
        return Err(NativeMaintenanceFitSnapshotError::Filesystem);
    }
    // safety: the successful syscall initialized the complete statfs value
    Ok(unsafe { details.assume_init() }.f_type)
}

/// Admit one immutable lowercase source commit.
fn valid_commit(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// Admit one lowercase SHA-256 identity.
fn valid_hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// Hash exact bytes with the approved SHA-256 implementation.
fn sha256(bytes: &[u8]) -> String {
    lower_hex(Sha256::digest(bytes).into())
}

/// Render one SHA-256 value in lowercase hexadecimal.
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

/// Render one random 128-bit root suffix in lowercase hexadecimal.
fn lower_hex_16(bytes: [u8; 16]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut value = String::with_capacity(32);
    // emit every random byte without formatting allocations
    for byte in bytes {
        value.push(char::from(HEX[usize::from(byte >> 4)]));
        value.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    value
}

/// Read the effective service owner used by source snapshot admission.
fn current_uid() -> u32 {
    // safety: geteuid has no preconditions and returns the current effective uid
    unsafe { libc::geteuid() }
}

#[cfg(test)]
#[path = "native_maintenance_fit_snapshot_test.rs"]
mod tests;
