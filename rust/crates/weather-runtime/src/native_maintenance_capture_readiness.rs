//! Read-only native projection of adjustment maintenance capture readiness.

use weather_api::CurrentTimeResolver;
use weather_contracts::JsDateOrString;

const CONTRACT_VERSION: &str = "adjustment-maintenance-capture-readiness/v1";
const LOCAL_READINESS_VERSION: &str = "adjustment-archive-local-readiness/v1";
const DATABASE_LEDGER_VERSION: &str = "adjustment-database-ledger/v3";
const FAMILY_LINEAGE_VERSION: &str = "adjustment-family-release-lineage/v2";
const CAPTURE_EPOCH_MIGRATION_HISTORY_SHA256: &str =
    "6de5c8c7efaa448aeb12bf1a9debe6fe7d4d4d1003ee0e21ab619ffa624c3424";

/// Distinguish the installed predecessor runtime from a native successor closure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMaintenanceCaptureReleaseGeneration {
    NodePredecessor,
    Native,
}

/// Verified immutable installed runner closure supplied by its independent authority.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedNativeMaintenanceInstalledRelease {
    pub generation: NativeMaintenanceCaptureReleaseGeneration,
    pub manifest_sha256: String,
    pub member_count: usize,
    pub source_commit: String,
}

/// Independently verified installed-release reader; it never selects a caller path.
pub trait NativeMaintenanceCaptureInstalledReleaseAuthority {
    type Error;

    /// Verify and return the complete installed release closure without mutating it.
    fn read_verified_installed_release(
        &mut self,
    ) -> Result<VerifiedNativeMaintenanceInstalledRelease, Self::Error>;
}

/// Existing local archive census projected unchanged into the readiness response.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeMaintenanceCaptureArchiveStatus {
    pub allocated_bytes: String,
    pub file_count: u64,
    pub incoming_count: u64,
    pub object_count: u64,
    pub task_inodes: String,
}

/// Exact result of the independent non-mutating local archive inspection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedNativeMaintenanceLocalArchiveReadiness {
    pub contract_version: String,
    pub ready: bool,
    pub reason: String,
    pub status: Option<NativeMaintenanceCaptureArchiveStatus>,
}

/// Exact supported rolling database ledger after its manifest has been verified.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedNativeMaintenanceDatabaseLedger {
    pub contract_version: String,
    pub migration_history_sha256: String,
    pub snapshot_at: String,
}

/// Verified retained zero-state capture epoch witness fields used by readiness.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedNativeMaintenanceCaptureEpochWitness {
    pub catalog_frontier_sha256: String,
    pub control_plane_sha256: String,
    pub control_plane_version: String,
    pub database_migration_history_sha256: String,
    pub epoch_at: String,
    pub serving_snapshot_sha256: String,
    pub source_commit: String,
    pub source_release: String,
    pub source_server_image_digest: String,
    pub witness_sha256: String,
}

/// Verified retained zero-state serving snapshot fields used by readiness.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedNativeMaintenanceCaptureEpochSnapshot {
    pub archive_commit_ordinal: String,
    pub cutoff_at: String,
    pub entry_count: u64,
    pub entries_empty: bool,
    pub frontier_sha256: String,
    pub snapshot_sha256: String,
}

/// Read-only prerequisite authorities excluding installed and live release generations.
pub trait NativeMaintenanceCaptureReadinessPorts {
    type Error;

    /// Verify the pinned predecessor fitter runtime and its numerical imports.
    fn verify_fit_runtime(&mut self) -> Result<(), Self::Error>;

    /// Inspect the existing local archive without initializing or repairing it.
    fn read_local_archive_readiness(
        &mut self,
    ) -> Result<VerifiedNativeMaintenanceLocalArchiveReadiness, Self::Error>;

    /// Read an exactly validated rolling database manifest and its snapshot clock.
    fn read_verified_database_ledger(
        &mut self,
    ) -> Result<VerifiedNativeMaintenanceDatabaseLedger, Self::Error>;

    /// Read and verify the retained capture epoch witness.
    fn read_verified_capture_epoch_witness(
        &mut self,
    ) -> Result<VerifiedNativeMaintenanceCaptureEpochWitness, Self::Error>;

    /// Read and verify the retained empty capture epoch snapshot.
    fn read_verified_capture_epoch_snapshot(
        &mut self,
    ) -> Result<VerifiedNativeMaintenanceCaptureEpochSnapshot, Self::Error>;
}

/// Closed adjustment family set authenticated by the predecessor readiness check.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMaintenanceCaptureFamily {
    Temperature,
    Wind,
    Rain,
}

impl NativeMaintenanceCaptureFamily {
    /// Return the exact source family spelling.
    pub const fn wire(self) -> &'static str {
        match self {
            Self::Temperature => "temperature",
            Self::Wind => "wind",
            Self::Rain => "rain",
        }
    }
}

/// Verified current family release observation from the live source authority.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedNativeMaintenanceFamilyCurrent {
    pub commit: String,
    pub family: String,
    pub release: String,
    pub source_server_image_digest: String,
}

/// Verified transitive current-to-epoch family lineage projection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedNativeMaintenanceFamilyLineage {
    pub contract_version: String,
    pub control_sha256: String,
    pub control_version: String,
    pub current_commit: String,
    pub current_release: String,
    pub epoch_ancestor_commit: String,
    pub epoch_witness_sha256: String,
    pub state: String,
    pub verified_at: String,
}

/// Live current-release and lineage authority kept separate from installed runner custody.
pub trait NativeMaintenanceCaptureSourceAuthority {
    type Error;

    /// Verify one fixed family through its live image and installed source material.
    fn read_verified_family_current(
        &mut self,
        family: NativeMaintenanceCaptureFamily,
    ) -> Result<VerifiedNativeMaintenanceFamilyCurrent, Self::Error>;

    /// Verify the common current release's transitive ancestry to the capture epoch.
    fn read_verified_family_lineage(
        &mut self,
    ) -> Result<VerifiedNativeMaintenanceFamilyLineage, Self::Error>;
}

/// Path-free capture-readiness response with the predecessor's exact refusal categories.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeMaintenanceCaptureReadiness {
    pub archive_status: Option<NativeMaintenanceCaptureArchiveStatus>,
    pub contract_version: &'static str,
    pub database_migration_history_sha256: Option<String>,
    pub ready: bool,
    pub reason: &'static str,
    pub release_manifest_sha256: Option<String>,
    pub source_commit: Option<String>,
    pub source_release: Option<String>,
}

impl NativeMaintenanceCaptureReadiness {
    /// Construct the source's initial installed-release refusal document.
    pub(crate) fn unavailable() -> Self {
        Self {
            archive_status: None,
            contract_version: CONTRACT_VERSION,
            database_migration_history_sha256: None,
            ready: false,
            reason: "installed_release_unavailable",
            release_manifest_sha256: None,
            source_commit: None,
            source_release: None,
        }
    }

    /// Select one exact refusal while preserving all earlier admitted evidence.
    fn refuse(mut self, reason: &'static str) -> Self {
        self.ready = false;
        self.reason = reason;
        self
    }
}

/// Inspect every source prerequisite in order without initializing capture state.
pub fn inspect_native_maintenance_capture_readiness<Installed, Ports, Source>(
    installed: &mut Installed,
    ports: &mut Ports,
    source: &mut Source,
    time: &dyn CurrentTimeResolver,
) -> NativeMaintenanceCaptureReadiness
where
    Installed: NativeMaintenanceCaptureInstalledReleaseAuthority,
    Ports: NativeMaintenanceCaptureReadinessPorts,
    Source: NativeMaintenanceCaptureSourceAuthority,
{
    inspect_native_maintenance_capture_readiness_for_generation(
        installed,
        ports,
        source,
        time,
        NativeMaintenanceCaptureReleaseGeneration::NodePredecessor,
    )
}

/// Verify only the explicitly expected independently authenticated installation generation.
pub(crate) fn inspect_native_maintenance_capture_readiness_for_generation<
    Installed,
    Ports,
    Source,
>(
    installed: &mut Installed,
    ports: &mut Ports,
    source: &mut Source,
    time: &dyn CurrentTimeResolver,
    generation: NativeMaintenanceCaptureReleaseGeneration,
) -> NativeMaintenanceCaptureReadiness
where
    Installed: NativeMaintenanceCaptureInstalledReleaseAuthority,
    Ports: NativeMaintenanceCaptureReadinessPorts,
    Source: NativeMaintenanceCaptureSourceAuthority,
{
    let mut result = NativeMaintenanceCaptureReadiness::unavailable();
    let Ok(closure) = installed.read_verified_installed_release() else {
        return result;
    };
    // admit only the matching separately verified installed generation
    if closure.generation != generation
        || closure.member_count == 0
        || !valid_hash(&closure.manifest_sha256)
        || !valid_commit(&closure.source_commit)
    {
        return result;
    }
    result.release_manifest_sha256 = Some(closure.manifest_sha256);
    result.source_commit = Some(closure.source_commit.clone());

    // preserve the source check order and first refusal reason
    if ports.verify_fit_runtime().is_err() {
        return result.refuse("fit_runtime_unavailable");
    }
    let Ok(local) = ports.read_local_archive_readiness() else {
        return result.refuse("local_archive_unready");
    };
    result.archive_status = local.status.clone();
    let status_is_valid = local.status.as_ref().is_some_and(valid_archive_status);
    // require the exact independently measured local readiness envelope
    if local.contract_version != LOCAL_READINESS_VERSION
        || !local.ready
        || local.reason != "ready"
        || !status_is_valid
    {
        return result.refuse("local_archive_unready");
    }
    let Ok(database) = ports.read_verified_database_ledger() else {
        return result.refuse("database_ledger_unavailable");
    };
    // accept only a port-verified live ledger with exact identity and canonical snapshot clock
    let Some(database_snapshot_at) = canonical_instant(time, &database.snapshot_at) else {
        return result.refuse("database_ledger_unavailable");
    };
    if database.contract_version != DATABASE_LEDGER_VERSION
        || !valid_hash(&database.migration_history_sha256)
    {
        return result.refuse("database_ledger_unavailable");
    }
    result.database_migration_history_sha256 = Some(database.migration_history_sha256);

    let Ok(witness) = ports.read_verified_capture_epoch_witness() else {
        return result.refuse("capture_epoch_unavailable");
    };
    let Ok(snapshot) = ports.read_verified_capture_epoch_snapshot() else {
        return result.refuse("capture_epoch_unavailable");
    };
    let Some(witness_epoch) = canonical_instant(time, &witness.epoch_at) else {
        return result.refuse("capture_epoch_mismatch");
    };
    // bind the retained 0021 zero proof to the current verified live ledger clock
    if witness.database_migration_history_sha256 != CAPTURE_EPOCH_MIGRATION_HISTORY_SHA256
        || snapshot.archive_commit_ordinal != "0"
        || snapshot.entry_count != 0
        || !snapshot.entries_empty
        || snapshot.frontier_sha256 != witness.catalog_frontier_sha256
        || snapshot.snapshot_sha256 != witness.serving_snapshot_sha256
        || snapshot.cutoff_at != witness.epoch_at
        || database_snapshot_at < witness_epoch
    {
        return result.refuse("capture_epoch_mismatch");
    }

    let mut currents = Vec::with_capacity(3);
    // authenticate every actual family source through its fixed family selector
    for family in [
        NativeMaintenanceCaptureFamily::Temperature,
        NativeMaintenanceCaptureFamily::Wind,
        NativeMaintenanceCaptureFamily::Rain,
    ] {
        let Ok(current) = source.read_verified_family_current(family) else {
            return result.refuse("current_release_unavailable");
        };
        currents.push((family, current));
    }
    let Ok(lineage) = source.read_verified_family_lineage() else {
        return result.refuse("current_lineage_unavailable");
    };
    let Some((_, first)) = currents.first() else {
        return result.refuse("current_release_mismatch");
    };
    let Some(lineage_verified_at) = canonical_instant(time, &lineage.verified_at) else {
        return result.refuse("current_release_mismatch");
    };
    // bind one common live release to its transitive root-owned epoch ancestry
    if !valid_commit(&first.commit)
        || !valid_release(&first.release)
        || currents.iter().any(|(family, current)| {
            current.family != family.wire()
                || current.commit != first.commit
                || current.release != first.release
                || current.source_server_image_digest != first.source_server_image_digest
        })
        || lineage.contract_version != FAMILY_LINEAGE_VERSION
        || lineage.state != "verified_epoch_descendant"
        || lineage.current_commit != first.commit
        || lineage.current_release != first.release
        || lineage.epoch_witness_sha256 != witness.witness_sha256
        || lineage.epoch_ancestor_commit != witness.source_commit
        || lineage.control_version != witness.control_plane_version
        || lineage.control_sha256 != witness.control_plane_sha256
        || lineage_verified_at < witness_epoch
        || closure.source_commit != witness.source_commit
    {
        return result.refuse("current_release_mismatch");
    }
    result.ready = true;
    result.reason = "ready";
    result.source_release = Some(first.release.clone());
    result
}

/// Resolve only an exact canonical UTC string through the selected production time owner.
fn canonical_instant(time: &dyn CurrentTimeResolver, value: &str) -> Option<i64> {
    let resolved = time
        .resolve(&JsDateOrString::String(value.encode_utf16().collect()))
        .ok()?;
    (resolved.utc == value).then_some(resolved.epoch_milliseconds)
}

/// Admit the exact lowercase SHA-256 alphabet used by every readiness authority.
fn valid_hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// Admit one immutable forty-character source commit.
fn valid_commit(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// Admit the predecessor's exact release-name grammar without normalizing it.
fn valid_release(value: &str) -> bool {
    let bytes = value.as_bytes();
    // preserve fixed date punctuation and the one-or-two digit nonzero release ordinal
    if !matches!(bytes.len(), 12 | 13)
        || bytes.get(4) != Some(&b'.')
        || bytes.get(7) != Some(&b'.')
        || bytes.get(10) != Some(&b'-')
        || !bytes[..4].iter().all(u8::is_ascii_digit)
        || !bytes[5..7].iter().all(u8::is_ascii_digit)
        || !bytes[8..10].iter().all(u8::is_ascii_digit)
    {
        return false;
    }
    let ordinal = &bytes[11..];
    ordinal[0].is_ascii_digit() && ordinal[0] != b'0' && ordinal.iter().all(u8::is_ascii_digit)
}

/// Keep the path-free archive status in the same canonical decimal representation.
fn valid_archive_status(status: &NativeMaintenanceCaptureArchiveStatus) -> bool {
    canonical_decimal(&status.allocated_bytes) && canonical_decimal(&status.task_inodes)
}

/// Admit one unsigned decimal string without alternate leading-zero spellings.
fn canonical_decimal(value: &str) -> bool {
    value
        .parse::<u64>()
        .ok()
        .is_some_and(|number| number.to_string() == value)
}
