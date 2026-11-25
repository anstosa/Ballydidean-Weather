//! Bind the family transaction core to durable private files and explicit production host effects.

use std::fs::{self, File, OpenOptions};
use std::io::{ErrorKind, Read, Write};
use std::os::fd::RawFd;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use weather_contracts::JsonValue;
use weather_edge::{ArchiveWirePrimitives, RevisionArchiveCanonicalClock};

use crate::maintenance_canonical_json::{exact_keys, field, text, valid_hash};
use crate::native_family_release_target_authority::verify_native_family_release_transaction_proof;
use crate::{
    CaptureEpochPublicGitJson, FamilyActionAuthorityFiles, FamilyCatalogImageVerification,
    FamilyReleaseBoundImageRead, FamilyTargetWitnessRead, MaintenanceAnchorStatusJson,
    MaintenanceCanonicalJson, NativeFamilyReleaseCurrent, NativeFamilyReleaseCurrentObservation,
    NativeFamilyReleaseDurableProof, NativeFamilyReleaseInput, NativeFamilyReleaseLiveObservation,
    NativeFamilyReleaseMutationRecord, NativeFamilyReleaseMutationState,
    NativeFamilyReleaseOperatorOffRecord, NativeFamilyReleaseOutcome,
    NativeFamilyReleasePriorRecord, NativeFamilyReleaseTargetAuthorityPorts,
    NativeFamilyReleaseTargetAuthorityRead, NativeFamilyReleaseTransactionError,
    NativeFamilyReleaseTransactionPort, NativeFamilyReleaseTransactionResult,
    NativeFamilyReleaseVerifiedProof, NativeWirePrimitives,
    execute_native_family_release_transaction,
};

const TRANSACTION_VERSION: &str = "adjustment-family-release-transaction/v1";
const OPERATOR_OFF_VERSION: &str = "adjustment-family-operator-off-transaction/v1";
const CURRENT_VERSION: &str = "adjustment-family-release-current/v1";
const MAXIMUM_STATE_BYTES: usize = 16 * 1024 * 1024;
const MAXIMUM_TRANSACTIONS: usize = 4_096;
static TEMPORARY_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Lock-held current state exposing both transaction bytes and the independently verified authority.
#[derive(Clone, Debug, PartialEq)]
pub struct NativeFamilyReleaseInspectedCurrent {
    pub observation: NativeFamilyReleaseCurrentObservation,
    pub authority: NativeFamilyReleaseCurrent,
}

/// Verified compensation state returned only after restoring and observing the prebuilt release.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeFamilyReleaseCompensationObservation {
    pub action_sha256: String,
    pub live: NativeFamilyReleaseLiveObservation,
}

/// Existing exact host controller boundary for source inspection and release mutations.
pub trait NativeFamilyReleaseHostController {
    /// Inspect current files and runtime under the inherited release lock.
    fn inspect_current(
        &mut self,
        family: &str,
    ) -> Result<NativeFamilyReleaseInspectedCurrent, NativeFamilyReleaseTransactionError>;

    /// Rebind and apply the already persisted target catalog and release.
    fn apply_target(
        &mut self,
        record: &NativeFamilyReleaseMutationRecord,
        proof: &NativeFamilyReleaseDurableProof,
    ) -> Result<(), NativeFamilyReleaseTransactionError>;

    /// Independently observe the applied target's health and physical identities.
    fn verify_live(
        &mut self,
        record: &NativeFamilyReleaseMutationRecord,
        proof: &NativeFamilyReleaseDurableProof,
    ) -> Result<NativeFamilyReleaseLiveObservation, NativeFamilyReleaseTransactionError>;

    /// Restore only the prebuilt compensation and return its independently verified live state.
    fn compensate(
        &mut self,
        record: &NativeFamilyReleaseMutationRecord,
        proof: &NativeFamilyReleaseDurableProof,
    ) -> Result<NativeFamilyReleaseCompensationObservation, NativeFamilyReleaseTransactionError>;
}

/// Read-only target authority operation consumed directly by the transaction adapter.
pub trait NativeFamilyReleaseProofOwner {
    /// Retain the admitted capacity sample or refresh it before the proof clock.
    fn prepare(
        &mut self,
        _input: &NativeFamilyReleaseInput,
        _current: &NativeFamilyReleaseCurrent,
    ) -> Result<(), NativeFamilyReleaseTransactionError> {
        Ok(())
    }

    /// Verify all public, image, capacity, custody, and catalog material without mutation.
    fn verify(
        &mut self,
        input: &NativeFamilyReleaseInput,
        current: &NativeFamilyReleaseCurrentObservation,
        current_authority: &NativeFamilyReleaseCurrent,
        prior: Option<&NativeFamilyReleaseMutationRecord>,
        now: &str,
    ) -> Result<NativeFamilyReleaseVerifiedProof, NativeFamilyReleaseTransactionError>;
}

/// Direct adapter over the completed native target-authority API and its exact owners.
pub struct PreparedNativeFamilyReleaseProofOwner<
    'a,
    Host,
    Git,
    Authority,
    Source,
    Target,
    Compensation,
    Witness,
    Verifier,
    Codec,
    Time,
> {
    pub ports: NativeFamilyReleaseTargetAuthorityPorts<
        'a,
        Host,
        Git,
        Authority,
        Source,
        Target,
        Compensation,
        Witness,
        Verifier,
    >,
    pub codec: &'a mut Codec,
    pub time: &'a Time,
}

impl<Host, Git, Authority, Source, Target, Compensation, Witness, Verifier, Codec, Time>
    NativeFamilyReleaseProofOwner
    for PreparedNativeFamilyReleaseProofOwner<
        '_,
        Host,
        Git,
        Authority,
        Source,
        Target,
        Compensation,
        Witness,
        Verifier,
        Codec,
        Time,
    >
where
    Host: NativeFamilyReleaseTargetAuthorityRead,
    Git: CaptureEpochPublicGitJson,
    Authority: FamilyActionAuthorityFiles,
    Source: FamilyReleaseBoundImageRead,
    Target: FamilyReleaseBoundImageRead,
    Compensation: FamilyReleaseBoundImageRead,
    Witness: FamilyTargetWitnessRead,
    Verifier: FamilyCatalogImageVerification,
    Codec: MaintenanceAnchorStatusJson,
    Time: weather_api::CurrentTimeResolver,
{
    /// Give the installed target owner its exact post-CAS capacity preparation boundary.
    fn prepare(
        &mut self,
        input: &NativeFamilyReleaseInput,
        current: &NativeFamilyReleaseCurrent,
    ) -> Result<(), NativeFamilyReleaseTransactionError> {
        self.ports
            .host
            .prepare_capacity(input, current)
            .map_err(|_| NativeFamilyReleaseTransactionError::Proof)
    }

    /// Call the source-exact target-authority adapter without weakening any owner boundary.
    fn verify(
        &mut self,
        input: &NativeFamilyReleaseInput,
        current: &NativeFamilyReleaseCurrentObservation,
        current_authority: &NativeFamilyReleaseCurrent,
        prior: Option<&NativeFamilyReleaseMutationRecord>,
        now: &str,
    ) -> Result<NativeFamilyReleaseVerifiedProof, NativeFamilyReleaseTransactionError> {
        verify_native_family_release_transaction_proof(
            input,
            current,
            current_authority,
            prior,
            now,
            &mut self.ports,
            self.codec,
            self.time,
        )
    }
}

/// Fixed durable paths and inherited lock identity for the production transaction store.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeFamilyReleaseTransactionStorePaths {
    pub state_root: PathBuf,
    pub release_lock_root: PathBuf,
}

impl NativeFamilyReleaseTransactionStorePaths {
    /// Select the predecessor's fixed production state and release-lock roots.
    pub fn native() -> Self {
        Self {
            state_root: PathBuf::from("/opt/weather/current/deploy/state/adjustment-release-state"),
            release_lock_root: PathBuf::from("/opt/weather/current/deploy/state"),
        }
    }
}

/// Concrete owner-private durable transaction and current-authority store.
pub struct NativeFamilyReleaseTransactionStore {
    paths: NativeFamilyReleaseTransactionStorePaths,
    lock_fd: RawFd,
    uid: u32,
    gid: u32,
}

/// Role selected from one exact durable applying transaction before a native switch.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeFamilyReleaseSwitchRole {
    Target,
    Compensation,
}

impl NativeFamilyReleaseSwitchRole {
    /// Return the exact stage-proof role spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Target => "target",
            Self::Compensation => "compensation",
        }
    }
}

/// Closed native-switch arguments admitted against the root-owned transaction proof.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NativeFamilyReleaseSwitchRecordInput<'a> {
    pub release: &'a str,
    pub expected_current: &'a str,
    pub alternate_current: &'a str,
    pub commit: &'a str,
    pub catalog_sha256: &'a str,
    pub family_identity_sha256: &'a str,
    pub image_digests_sha256: &'a str,
    pub action_sha256: &'a str,
}

/// Re-read the locked durable transaction and authorize one exact native target or compensation switch.
pub fn verify_native_family_switch_record(
    input: &NativeFamilyReleaseSwitchRecordInput<'_>,
) -> Result<(String, NativeFamilyReleaseSwitchRole), NativeFamilyReleaseTransactionError> {
    let store = NativeFamilyReleaseTransactionStore::native()?;
    store.verify_lock()?;
    let document = match store.read_document(input.action_sha256)? {
        Some(StoredDocument::Mutation(document)) => document,
        _ => return Err(NativeFamilyReleaseTransactionError::Proof),
    };
    let record = &document.record;
    let proof = &document.proof;
    let action_sha256 = digest_json_value(&record.action)?;
    // bind common source, action, and immutable identity operands before role selection
    if record.input.action_sha256 != input.action_sha256
        || action_sha256 != input.action_sha256
        || record.input.expected_current_release != input.expected_current
        || record.input.expected_current_release != record.input.expected_source_release
        || record.source_settings_sha256 != record.input.expected_settings_sha256
        || record.target_commit != proof.target_commit
        || text(&record.action, "family").as_deref() != Some(record.input.family.as_str())
        || text(&record.action, "fencingToken").as_deref()
            != Some(record.input.fencing_token.as_str())
        || text(&record.action, "expectedSettingsSha256").as_deref()
            != Some(record.input.expected_settings_sha256.as_str())
        || text(&record.action, "expectedSourceCommit").as_deref()
            != Some(record.source_commit.as_str())
        || text(&record.action, "expectedSourceRelease").as_deref()
            != Some(record.input.expected_source_release.as_str())
        || !valid_hash(input.catalog_sha256)
        || !valid_hash(input.family_identity_sha256)
        || !valid_hash(input.image_digests_sha256)
    {
        return Err(NativeFamilyReleaseTransactionError::Proof);
    }
    let target_catalog_sha256 = digest_bytes(&proof.target_catalog_bytes)?;
    let compensation_catalog_sha256 = digest_bytes(&proof.baseline_catalog_bytes)?;
    let target = input.release == record.input.target_release
        && input.alternate_current == record.input.expected_current_release
        && input.commit == proof.target_commit
        && input.catalog_sha256 == target_catalog_sha256
        && input.family_identity_sha256 == proof.target_family_identity_sha256
        && input.image_digests_sha256 == proof.target_image_digests_sha256;
    let compensation = input.release == record.input.compensating_release
        && input.alternate_current == record.input.target_release
        && input.commit == proof.compensation_commit
        && input.catalog_sha256 == compensation_catalog_sha256
        && input.family_identity_sha256 == proof.compensation_family_identity_sha256
        && input.image_digests_sha256 == proof.compensation_image_digests_sha256;
    // target requires applying; compensation requires its independently durable recovery state
    let role = if target && record.state == NativeFamilyReleaseMutationState::Applying {
        NativeFamilyReleaseSwitchRole::Target
    } else if compensation && record.state == NativeFamilyReleaseMutationState::CompensationRequired
    {
        NativeFamilyReleaseSwitchRole::Compensation
    } else {
        return Err(NativeFamilyReleaseTransactionError::Proof);
    };
    Ok((record.input.family.clone(), role))
}

impl NativeFamilyReleaseTransactionStore {
    /// Bind production roots to the inherited update-script lock descriptor.
    pub fn native() -> Result<Self, NativeFamilyReleaseTransactionError> {
        let descriptor = std::env::var("WEATHER_RELEASE_LOCK_FD")
            .ok()
            .filter(|value| {
                !value.is_empty()
                    && value.len() <= 3
                    && value.as_bytes()[0] != b'0'
                    && value.bytes().all(|byte| byte.is_ascii_digit())
            })
            .and_then(|value| value.parse::<RawFd>().ok())
            .ok_or(NativeFamilyReleaseTransactionError::Lock)?;
        Ok(Self::for_paths(
            NativeFamilyReleaseTransactionStorePaths::native(),
            descriptor,
            0,
            0,
        ))
    }

    /// Bind disposable roots and an already held fixture lock without changing store semantics.
    pub fn for_paths(
        paths: NativeFamilyReleaseTransactionStorePaths,
        lock_fd: RawFd,
        uid: u32,
        gid: u32,
    ) -> Self {
        Self {
            paths,
            lock_fd,
            uid,
            gid,
        }
    }

    /// Select the sole crash-left mutation eligible for installed compensation recovery.
    pub fn read_unfinished_recovery_action(
        &self,
    ) -> Result<Option<String>, NativeFamilyReleaseTransactionError> {
        self.verify_lock()?;
        self.initialize()?;
        let mut names = fs::read_dir(self.transactions_root())
            .map_err(|_| NativeFamilyReleaseTransactionError::FailurePersistence)?
            .map(|entry| {
                entry
                    .map_err(|_| NativeFamilyReleaseTransactionError::FailurePersistence)?
                    .file_name()
                    .into_string()
                    .map_err(|_| NativeFamilyReleaseTransactionError::FailurePersistence)
            })
            .collect::<Result<Vec<_>, _>>()?;
        names.sort_unstable();
        // refuse an unbounded or foreign inventory before selecting recovery state
        if names.len() > MAXIMUM_TRANSACTIONS
            || names
                .iter()
                .any(|name| transaction_identity(name).is_none())
        {
            return Err(NativeFamilyReleaseTransactionError::FailurePersistence);
        }
        let mut selected = None;
        // inspect every retained transaction so a second recoverable mutation fails closed
        for name in names {
            let identity = transaction_identity(&name)
                .ok_or(NativeFamilyReleaseTransactionError::FailurePersistence)?;
            let document = self
                .read_document(identity)?
                .ok_or(NativeFamilyReleaseTransactionError::FailurePersistence)?;
            let recoverable = matches!(
                &document,
                StoredDocument::Mutation(document)
                    if matches!(
                        document.record.state,
                        NativeFamilyReleaseMutationState::Applying
                            | NativeFamilyReleaseMutationState::CompensationRequired
                    ) && document.compensation.state != CompensationState::Verified
            );
            // retain exactly one crash-left mutation and ignore non-applied prepared state
            if recoverable && selected.replace(identity.to_owned()).is_some() {
                return Err(NativeFamilyReleaseTransactionError::FailurePersistence);
            }
        }
        Ok(selected)
    }

    /// Append the exact prior capacity sample and atomically select only a ready refresh.
    pub(crate) fn refresh_capacity_receipt(
        &self,
        action_sha256: &str,
        previous: &[u8],
        refreshed: &[u8],
        ready: bool,
    ) -> Result<(), NativeFamilyReleaseTransactionError> {
        self.verify_lock()?;
        self.initialize()?;
        const MAXIMUM: usize = 64 * 1024;
        // admit only the fixed action address and bounded canonical receipt bytes
        if !valid_hash(action_sha256)
            || previous.is_empty()
            || previous.len() > MAXIMUM
            || refreshed.is_empty()
            || refreshed.len() > MAXIMUM
        {
            return Err(NativeFamilyReleaseTransactionError::Proof);
        }
        let path = self
            .paths
            .state_root
            .join(format!("capacity-sha256-{action_sha256}.json"));
        let existing = read_private_file(&path, MAXIMUM, self.uid, self.gid, false)?
            .ok_or(NativeFamilyReleaseTransactionError::Proof)?;
        // preserve the already admitted pre-pull receipt under the same release lock
        if existing != previous {
            return Err(NativeFamilyReleaseTransactionError::Proof);
        }
        let retained = if ready { previous } else { refreshed };
        let history = self.paths.state_root.join(format!(
            "capacity-history-sha256-{action_sha256}-{}.json",
            digest_bytes(retained)?
        ));
        match read_private_file(&history, MAXIMUM, self.uid, self.gid, true)? {
            None => write_exclusive(&history, retained, self.uid, self.gid)?,
            Some(bytes) if bytes == retained => (),
            Some(_) => return Err(NativeFamilyReleaseTransactionError::Proof),
        }
        // a measured refusal is retained without replacing the accepted pre-pull sample
        if ready && previous != refreshed {
            write_atomic(&path, refreshed, 0o600, self.uid, self.gid)?;
        }
        Ok(())
    }

    /// Verify that the inherited descriptor still names the exact fixed release-state inode.
    fn verify_lock(&self) -> Result<(), NativeFamilyReleaseTransactionError> {
        let held = fs::metadata(format!("/proc/self/fd/{}", self.lock_fd))
            .map_err(|_| NativeFamilyReleaseTransactionError::Lock)?;
        let root = fs::symlink_metadata(&self.paths.release_lock_root)
            .map_err(|_| NativeFamilyReleaseTransactionError::Lock)?;
        // reject a link, non-directory, or changed inherited lock identity
        if !root.is_dir()
            || root.file_type().is_symlink()
            || held.dev() != root.dev()
            || held.ino() != root.ino()
        {
            return Err(NativeFamilyReleaseTransactionError::Lock);
        }
        Ok(())
    }

    /// Initialize or verify only the fixed owner-private state directories.
    fn initialize(&self) -> Result<(), NativeFamilyReleaseTransactionError> {
        ensure_private_directory(&self.paths.state_root, self.uid, self.gid)?;
        ensure_private_directory(&self.transactions_root(), self.uid, self.gid)
    }

    /// Return the fixed content-addressed transaction directory.
    fn transactions_root(&self) -> PathBuf {
        self.paths.state_root.join("transactions")
    }

    /// Return one content-addressed transaction path after closing its operand.
    fn transaction_path(
        &self,
        action_sha256: &str,
    ) -> Result<PathBuf, NativeFamilyReleaseTransactionError> {
        // prohibit path selection outside one lowercase SHA-256 address
        if !valid_hash(action_sha256) {
            return Err(NativeFamilyReleaseTransactionError::Input);
        }
        Ok(self
            .transactions_root()
            .join(format!("sha256-{action_sha256}.json")))
    }

    /// Read and decode one exact durable document while preserving genuine absence.
    fn read_document(
        &self,
        action_sha256: &str,
    ) -> Result<Option<StoredDocument>, NativeFamilyReleaseTransactionError> {
        self.initialize()?;
        let Some(bytes) = read_private_file(
            &self.transaction_path(action_sha256)?,
            MAXIMUM_STATE_BYTES,
            self.uid,
            self.gid,
            true,
        )?
        else {
            return Ok(None);
        };
        decode_document(action_sha256, &bytes).map(Some)
    }

    /// Read the monotonic uint64 fence or its absent genesis.
    fn read_maximum_fence(&self) -> Result<String, NativeFamilyReleaseTransactionError> {
        self.initialize()?;
        let Some(bytes) = read_private_file(
            &self.paths.state_root.join("maximum-fence"),
            32,
            self.uid,
            self.gid,
            true,
        )?
        else {
            return Ok("0".to_owned());
        };
        let value = std::str::from_utf8(&bytes)
            .ok()
            .and_then(|value| value.strip_suffix('\n'))
            .filter(|value| canonical_u64(value, true))
            .ok_or(NativeFamilyReleaseTransactionError::SourceCompareAndSwap)?;
        Ok(value.to_owned())
    }

    /// Advance the durable fence after a recoverable record exists, permitting exact retry.
    fn write_maximum_fence(&self, value: &str) -> Result<(), NativeFamilyReleaseTransactionError> {
        let current = self.read_maximum_fence()?;
        let next = value
            .parse::<u64>()
            .map_err(|_| NativeFamilyReleaseTransactionError::PreparePersistence)?;
        let prior = current
            .parse::<u64>()
            .map_err(|_| NativeFamilyReleaseTransactionError::PreparePersistence)?;
        // never permit a fence regression after its transaction is recoverable
        if next < prior {
            return Err(NativeFamilyReleaseTransactionError::PreparePersistence);
        }
        // avoid replacing the durable inode for an exact response-lost retry
        if next != prior {
            write_atomic(
                &self.paths.state_root.join("maximum-fence"),
                format!("{value}\n").as_bytes(),
                0o600,
                self.uid,
                self.gid,
            )?;
        }
        Ok(())
    }

    /// Refuse a second action while another retained mutation remains unfinished.
    fn require_available_slot(
        &self,
        action_sha256: &str,
    ) -> Result<(), NativeFamilyReleaseTransactionError> {
        let mut names = fs::read_dir(self.transactions_root())
            .map_err(|_| NativeFamilyReleaseTransactionError::PreparePersistence)?
            .map(|entry| {
                entry
                    .map_err(|_| NativeFamilyReleaseTransactionError::PreparePersistence)?
                    .file_name()
                    .into_string()
                    .map_err(|_| NativeFamilyReleaseTransactionError::PreparePersistence)
            })
            .collect::<Result<Vec<_>, _>>()?;
        names.sort_unstable();
        // bound inventory traversal and reject foreign transaction names
        if names.len() > MAXIMUM_TRANSACTIONS
            || names
                .iter()
                .any(|name| transaction_identity(name).is_none())
        {
            return Err(NativeFamilyReleaseTransactionError::PreparePersistence);
        }
        // require every other retained transaction to release the single mutation slot
        for name in names {
            let identity = transaction_identity(&name)
                .ok_or(NativeFamilyReleaseTransactionError::PreparePersistence)?;
            if identity == action_sha256 {
                continue;
            }
            let document = self
                .read_document(identity)?
                .ok_or(NativeFamilyReleaseTransactionError::PreparePersistence)?;
            let terminal = match document {
                StoredDocument::OperatorOff(_) => true,
                StoredDocument::Mutation(document) => {
                    document.record.state == NativeFamilyReleaseMutationState::Acknowledged
                        || document.compensation.state == CompensationState::Verified
                }
            };
            // retain the predecessor's single unfinished action invariant
            if !terminal {
                return Err(NativeFamilyReleaseTransactionError::PreparePersistence);
            }
        }
        Ok(())
    }

    /// Create or replay one exact operator-off document before advancing its fence.
    fn acknowledge_operator_off(
        &self,
        record: &NativeFamilyReleaseOperatorOffRecord,
    ) -> Result<NativeFamilyReleaseOperatorOffRecord, NativeFamilyReleaseTransactionError> {
        self.initialize()?;
        let document = StoredDocument::OperatorOff(Box::new(record.clone()));
        let bytes = encode_document(&document)?;
        let path = self.transaction_path(&record.input.action_sha256)?;
        match self.read_document(&record.input.action_sha256)? {
            None => {
                self.require_available_slot(&record.input.action_sha256)?;
                write_exclusive(&path, &bytes, self.uid, self.gid)?;
            }
            Some(existing) if encode_document(&existing)? == bytes => {}
            Some(_) => return Err(NativeFamilyReleaseTransactionError::IdentityCollision),
        }
        self.write_maximum_fence(&record.input.fencing_token)?;
        Ok(record.clone())
    }

    /// Create or replay one byte-identical prepared document before advancing its fence.
    fn prepare(
        &self,
        record: &NativeFamilyReleaseMutationRecord,
        proof: &NativeFamilyReleaseDurableProof,
    ) -> Result<(), NativeFamilyReleaseTransactionError> {
        self.initialize()?;
        let document = StoredDocument::Mutation(Box::new(StoredMutationDocument {
            compensation: StoredCompensation {
                action_sha256: proof.compensation_action_sha256.clone(),
                release: record.input.compensating_release.clone(),
                state: CompensationState::Absent,
                verified_at: None,
            },
            proof: proof.clone(),
            record: record.clone(),
        }));
        let bytes = encode_document(&document)?;
        let path = self.transaction_path(&record.input.action_sha256)?;
        match self.read_document(&record.input.action_sha256)? {
            None => {
                self.require_available_slot(&record.input.action_sha256)?;
                write_exclusive(&path, &bytes, self.uid, self.gid)?;
            }
            Some(existing) if encode_document(&existing)? == bytes => {}
            Some(_) => return Err(NativeFamilyReleaseTransactionError::IdentityCollision),
        }
        self.write_maximum_fence(&record.input.fencing_token)
    }

    /// Replace only the mutable record inside an existing mutation document.
    fn update_record(
        &self,
        record: &NativeFamilyReleaseMutationRecord,
    ) -> Result<(), NativeFamilyReleaseTransactionError> {
        let mut document = self
            .read_document(&record.input.action_sha256)?
            .ok_or(NativeFamilyReleaseTransactionError::FailurePersistence)?;
        let StoredDocument::Mutation(document) = &mut document else {
            return Err(NativeFamilyReleaseTransactionError::FailurePersistence);
        };
        document.record = record.clone();
        self.replace_document(
            &record.input.action_sha256,
            &StoredDocument::Mutation(document.clone()),
        )
    }

    /// Replace the compensation projection without altering its immutable proof or record.
    fn update_compensation(
        &self,
        action_sha256: &str,
        compensation: StoredCompensation,
    ) -> Result<(), NativeFamilyReleaseTransactionError> {
        let mut document = self
            .read_document(action_sha256)?
            .ok_or(NativeFamilyReleaseTransactionError::FailurePersistence)?;
        let StoredDocument::Mutation(document) = &mut document else {
            return Err(NativeFamilyReleaseTransactionError::FailurePersistence);
        };
        document.compensation = compensation;
        self.replace_document(action_sha256, &StoredDocument::Mutation(document.clone()))
    }

    /// Atomically replace one existing durable document and sync its directory.
    fn replace_document(
        &self,
        action_sha256: &str,
        document: &StoredDocument,
    ) -> Result<(), NativeFamilyReleaseTransactionError> {
        let path = self.transaction_path(action_sha256)?;
        // prohibit converting absence into an update-only transition
        if fs::symlink_metadata(&path).is_err() {
            return Err(NativeFamilyReleaseTransactionError::FailurePersistence);
        }
        write_atomic(
            &path,
            &encode_document(document)?,
            0o600,
            self.uid,
            self.gid,
        )
    }

    /// Publish exact future current authority after target or compensation live verification.
    fn write_current(
        &self,
        record: &NativeFamilyReleaseMutationRecord,
        action_sha256: &str,
        live: &NativeFamilyReleaseLiveObservation,
        state: &str,
    ) -> Result<(), NativeFamilyReleaseTransactionError> {
        let acknowledged_at = record
            .verified_at
            .as_deref()
            .or(record.failed_at.as_deref())
            .ok_or(NativeFamilyReleaseTransactionError::AcknowledgementPersistence)?;
        let value = object(vec![
            ("acknowledgedAt", string(acknowledged_at)),
            ("actionSha256", string(action_sha256)),
            ("commit", string(&live.commit)),
            ("contractVersion", string(CURRENT_VERSION)),
            ("family", string(&record.input.family)),
            ("imageDigestsSha256", string(&live.image_digests_sha256)),
            ("release", string(&live.release)),
            ("state", string(state)),
        ]);
        let bytes = NativeWirePrimitives
            .encode_sorted_json(&value)
            .ok_or(NativeFamilyReleaseTransactionError::AcknowledgementPersistence)?;
        write_atomic(
            &self.paths.state_root.join("current.json"),
            &bytes,
            0o600,
            self.uid,
            self.gid,
        )
    }
}

/// Concrete transaction-port composition over retained files, host controller, and proof owner.
pub struct NativeFamilyReleaseProductionPort<Host, Proof, Clock> {
    pub store: NativeFamilyReleaseTransactionStore,
    pub host: Host,
    pub proof: Proof,
    pub clock: Clock,
    family: String,
    current_authority: Option<NativeFamilyReleaseCurrent>,
    loaded: Option<StoredMutationDocument>,
}

impl<Host, Proof, Clock> NativeFamilyReleaseProductionPort<Host, Proof, Clock> {
    /// Compose production owners without observing or mutating any state.
    pub fn new(
        store: NativeFamilyReleaseTransactionStore,
        host: Host,
        proof: Proof,
        clock: Clock,
        family: impl Into<String>,
    ) -> Self {
        Self {
            store,
            host,
            proof,
            clock,
            family: family.into(),
            current_authority: None,
            loaded: None,
        }
    }

    /// Attempt the prebuilt compensation and retain its categorical durable result.
    fn compensate_prepared(
        &mut self,
        record: &NativeFamilyReleaseMutationRecord,
        proof: &NativeFamilyReleaseDurableProof,
    ) -> Result<(), NativeFamilyReleaseTransactionError>
    where
        Host: NativeFamilyReleaseHostController,
        Clock: RevisionArchiveCanonicalClock,
    {
        let failed_at = self
            .clock
            .canonical_now()
            .ok_or(NativeFamilyReleaseTransactionError::FailurePersistence)?;
        let mut recovering = record.clone();
        recovering.failed_at.get_or_insert(failed_at.clone());
        recovering.state = NativeFamilyReleaseMutationState::CompensationRequired;
        // publish the recovery state before the privileged compensation switch is admitted
        self.store.update_record(&recovering)?;
        // publish verified compensation authority only after the host returns exact live state
        match self.host.compensate(&recovering, proof) {
            Ok(compensation) => {
                let verified_at = self
                    .clock
                    .canonical_now()
                    .ok_or(NativeFamilyReleaseTransactionError::FailurePersistence)?;
                self.store.write_current(
                    &recovering,
                    &compensation.action_sha256,
                    &compensation.live,
                    "compensated",
                )?;
                self.store.update_compensation(
                    &record.input.action_sha256,
                    StoredCompensation {
                        action_sha256: compensation.action_sha256,
                        release: compensation.live.release,
                        state: CompensationState::Verified,
                        verified_at: Some(verified_at),
                    },
                )
            }
            Err(error) => {
                let _ = self.store.update_compensation(
                    &record.input.action_sha256,
                    StoredCompensation {
                        action_sha256: proof.compensation_action_sha256.clone(),
                        release: record.input.compensating_release.clone(),
                        state: CompensationState::Failed,
                        verified_at: None,
                    },
                );
                Err(error)
            }
        }
    }

    /// Finish one crash-left applying or compensation-required transaction from durable proof.
    pub fn reconcile_compensation(
        &mut self,
        action_sha256: &str,
    ) -> Result<(), NativeFamilyReleaseTransactionError>
    where
        Host: NativeFamilyReleaseHostController,
        Clock: RevisionArchiveCanonicalClock,
    {
        let document = match self.store.read_document(action_sha256)? {
            Some(StoredDocument::Mutation(document)) => document,
            _ => return Err(NativeFamilyReleaseTransactionError::FailurePersistence),
        };
        // exact verified compensation is already terminal and must not mutate again
        if document.compensation.state == CompensationState::Verified {
            return Ok(());
        }
        // admit reconciliation only for the two crash/failure states selected by the source
        if !matches!(
            document.record.state,
            NativeFamilyReleaseMutationState::Applying
                | NativeFamilyReleaseMutationState::CompensationRequired
        ) {
            return Err(NativeFamilyReleaseTransactionError::FailurePersistence);
        }
        self.compensate_prepared(&document.record, &document.proof)
    }
}

// hash one exact retained byte member for native switch admission
fn digest_bytes(bytes: &[u8]) -> Result<String, NativeFamilyReleaseTransactionError> {
    MaintenanceCanonicalJson::sha256(&mut NativeWirePrimitives, bytes)
        .map(|digest| digest.as_str().to_owned())
        .filter(|digest| valid_hash(digest))
        .ok_or(NativeFamilyReleaseTransactionError::Proof)
}

// hash one canonical retained JSON value for content-address admission
fn digest_json_value(value: &JsonValue) -> Result<String, NativeFamilyReleaseTransactionError> {
    let bytes = MaintenanceCanonicalJson::encode_canonical_json(&mut NativeWirePrimitives, value)
        .ok_or(NativeFamilyReleaseTransactionError::Proof)?;
    digest_bytes(&bytes)
}

/// Execute the fixed command and finish only crash-left compensation from durable evidence.
pub fn run_native_family_release_production_command<Host, Proof, Clock>(
    input: NativeFamilyReleaseInput,
    port: &mut NativeFamilyReleaseProductionPort<Host, Proof, Clock>,
) -> Result<NativeFamilyReleaseTransactionResult, NativeFamilyReleaseTransactionError>
where
    Host: NativeFamilyReleaseHostController,
    Proof: NativeFamilyReleaseProofOwner,
    Clock: RevisionArchiveCanonicalClock,
{
    let action_sha256 = input.action_sha256.clone();
    let result = execute_native_family_release_transaction(input, port)?;
    // reconcile only the source's explicit crash-left recovery result
    if matches!(
        &result,
        NativeFamilyReleaseTransactionResult::Mutation(record)
            if record.state == NativeFamilyReleaseMutationState::CompensationRequired
    ) {
        port.reconcile_compensation(&action_sha256)?;
    }
    Ok(result)
}

impl<Host, Proof, Clock> NativeFamilyReleaseTransactionPort
    for NativeFamilyReleaseProductionPort<Host, Proof, Clock>
where
    Host: NativeFamilyReleaseHostController,
    Proof: NativeFamilyReleaseProofOwner,
    Clock: RevisionArchiveCanonicalClock,
{
    /// Verify the inherited lock identity once, then retain it across the complete callback.
    fn with_release_lock<T>(
        &mut self,
        operation: impl FnOnce(&mut Self) -> Result<T, NativeFamilyReleaseTransactionError>,
    ) -> Result<T, NativeFamilyReleaseTransactionError> {
        self.store.verify_lock()?;
        operation(self)
    }

    /// Return the actual native process clock in canonical predecessor spelling.
    fn clock(&mut self) -> Result<String, NativeFamilyReleaseTransactionError> {
        self.clock
            .canonical_now()
            .ok_or(NativeFamilyReleaseTransactionError::Proof)
    }

    /// Load and retain the durable proof needed by later compensation paths.
    fn read_prepared(
        &mut self,
        action_sha256: &str,
    ) -> Result<Option<NativeFamilyReleasePriorRecord>, NativeFamilyReleaseTransactionError> {
        self.loaded = None;
        // retain the mutation proof separately from the record returned to the core
        Ok(match self.store.read_document(action_sha256)? {
            None => None,
            Some(StoredDocument::OperatorOff(record)) => {
                Some(NativeFamilyReleasePriorRecord::OperatorOff(*record))
            }
            Some(StoredDocument::Mutation(document)) => {
                let record = document.record.clone();
                self.loaded = Some(*document);
                Some(NativeFamilyReleasePriorRecord::Mutation(record))
            }
        })
    }

    /// Combine the exact host observation with the store's durable monotonic fence.
    fn inspect_current(
        &mut self,
    ) -> Result<NativeFamilyReleaseCurrentObservation, NativeFamilyReleaseTransactionError> {
        let mut current = self.host.inspect_current(&self.family)?;
        current.observation.maximum_fence = self.store.read_maximum_fence()?;
        self.current_authority = Some(current.authority);
        Ok(current.observation)
    }

    /// Refresh only the already admitted source's proof context before capturing the proof time.
    fn prepare_proof(
        &mut self,
        input: &NativeFamilyReleaseInput,
        _current: &NativeFamilyReleaseCurrentObservation,
    ) -> Result<(), NativeFamilyReleaseTransactionError> {
        let authority = self
            .current_authority
            .as_ref()
            .ok_or(NativeFamilyReleaseTransactionError::Proof)?;
        self.proof.prepare(input, authority)
    }

    /// Run the completed native target-authority API against the lock-held current observation.
    fn verify_proof(
        &mut self,
        input: &NativeFamilyReleaseInput,
        current: &NativeFamilyReleaseCurrentObservation,
        prior: Option<&NativeFamilyReleaseMutationRecord>,
        now: &str,
    ) -> Result<NativeFamilyReleaseVerifiedProof, NativeFamilyReleaseTransactionError> {
        let authority = self
            .current_authority
            .as_ref()
            .ok_or(NativeFamilyReleaseTransactionError::Proof)?;
        self.proof.verify(input, current, authority, prior, now)
    }

    /// Persist the disjoint operator-off document and its fence.
    fn acknowledge_operator_off(
        &mut self,
        record: &NativeFamilyReleaseOperatorOffRecord,
    ) -> Result<NativeFamilyReleaseOperatorOffRecord, NativeFamilyReleaseTransactionError> {
        self.store.acknowledge_operator_off(record)
    }

    /// Persist the immutable proof and prepared record before the first mutation.
    fn prepare(
        &mut self,
        record: &NativeFamilyReleaseMutationRecord,
        proof: &NativeFamilyReleaseVerifiedProof,
    ) -> Result<(), NativeFamilyReleaseTransactionError> {
        self.store.prepare(record, &proof.durable)?;
        self.loaded = Some(
            match self.store.read_document(&record.input.action_sha256)? {
                Some(StoredDocument::Mutation(document)) => *document,
                _ => return Err(NativeFamilyReleaseTransactionError::PreparePersistence),
            },
        );
        Ok(())
    }

    /// Publish applying before invoking the exact host controller.
    fn mark_applying(
        &mut self,
        record: &NativeFamilyReleaseMutationRecord,
    ) -> Result<(), NativeFamilyReleaseTransactionError> {
        let mut applying = record.clone();
        applying.state = NativeFamilyReleaseMutationState::Applying;
        self.store.update_record(&applying)
    }

    /// Apply target and attempt only its prebuilt compensation on failure.
    fn apply_images(
        &mut self,
        record: &NativeFamilyReleaseMutationRecord,
        proof: &NativeFamilyReleaseVerifiedProof,
    ) -> Result<(), NativeFamilyReleaseTransactionError> {
        // compensate an application failure without hiding the target error
        if let Err(error) = self.host.apply_target(record, &proof.durable) {
            let _ = self.compensate_prepared(record, &proof.durable);
            return Err(error);
        }
        Ok(())
    }

    /// Verify target live state and compensate a separately failed verification.
    fn verify_live(
        &mut self,
        record: &NativeFamilyReleaseMutationRecord,
        proof: &NativeFamilyReleaseVerifiedProof,
    ) -> Result<NativeFamilyReleaseLiveObservation, NativeFamilyReleaseTransactionError> {
        match self.host.verify_live(record, &proof.durable) {
            Ok(live) => Ok(live),
            Err(error) => {
                let _ = self.compensate_prepared(record, &proof.durable);
                Err(error)
            }
        }
    }

    /// Ensure acknowledgement failures also obtain compensation before retaining failure state.
    fn record_failure(
        &mut self,
        record: &NativeFamilyReleaseMutationRecord,
    ) -> Result<(), NativeFamilyReleaseTransactionError> {
        let document = match self.store.read_document(&record.input.action_sha256)? {
            Some(StoredDocument::Mutation(document)) => document,
            _ => return Err(NativeFamilyReleaseTransactionError::FailurePersistence),
        };
        // avoid repeating a compensation already verified by an earlier failure boundary
        if document.compensation.state != CompensationState::Verified {
            let _ = self.compensate_prepared(record, &document.proof);
        }
        self.store.update_record(record)
    }

    /// Publish future current authority before marking the transaction acknowledged.
    fn acknowledge(
        &mut self,
        record: &NativeFamilyReleaseMutationRecord,
        live: &NativeFamilyReleaseLiveObservation,
    ) -> Result<NativeFamilyReleaseMutationRecord, NativeFamilyReleaseTransactionError> {
        self.store
            .write_current(record, &record.input.action_sha256, live, "acknowledged")?;
        self.store.update_record(record)?;
        Ok(record.clone())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CompensationState {
    Absent,
    Failed,
    Verified,
}

impl CompensationState {
    /// Return the exact durable compensation state spelling.
    const fn as_str(self) -> &'static str {
        match self {
            Self::Absent => "absent",
            Self::Failed => "failed",
            Self::Verified => "verified",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct StoredCompensation {
    action_sha256: String,
    release: String,
    state: CompensationState,
    verified_at: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
struct StoredMutationDocument {
    compensation: StoredCompensation,
    proof: NativeFamilyReleaseDurableProof,
    record: NativeFamilyReleaseMutationRecord,
}

#[derive(Clone, Debug, PartialEq)]
enum StoredDocument {
    Mutation(Box<StoredMutationDocument>),
    OperatorOff(Box<NativeFamilyReleaseOperatorOffRecord>),
}

impl From<StoredMutationDocument> for StoredDocument {
    /// Wrap one mutation document without changing its retained members.
    fn from(value: StoredMutationDocument) -> Self {
        Self::Mutation(Box::new(value))
    }
}

// encode one source-exact durable transaction document
fn encode_document(
    document: &StoredDocument,
) -> Result<Vec<u8>, NativeFamilyReleaseTransactionError> {
    let value = match document {
        StoredDocument::OperatorOff(record) => object(vec![
            ("contractVersion", string(OPERATOR_OFF_VERSION)),
            ("record", encode_operator_off(record)),
        ]),
        StoredDocument::Mutation(document) => object(vec![
            ("compensation", encode_compensation(&document.compensation)),
            ("contractVersion", string(TRANSACTION_VERSION)),
            ("proof", encode_proof(&document.proof)?),
            ("record", encode_record(&document.record)),
        ]),
    };
    NativeWirePrimitives
        .encode_sorted_json(&value)
        .ok_or(NativeFamilyReleaseTransactionError::PreparePersistence)
}

// decode and canonically rebind one durable transaction document
fn decode_document(
    action_sha256: &str,
    bytes: &[u8],
) -> Result<StoredDocument, NativeFamilyReleaseTransactionError> {
    let mut codec = NativeWirePrimitives;
    let value = codec
        .parse_canonical_object(bytes)
        .ok_or(NativeFamilyReleaseTransactionError::PreparePersistence)?;
    let version = text(&value, "contractVersion")
        .ok_or(NativeFamilyReleaseTransactionError::PreparePersistence)?;
    let document = if version == OPERATOR_OFF_VERSION {
        // preserve the intentionally disjoint no-proof operator-off document
        if !exact_keys(&value, &["contractVersion", "record"]) {
            return Err(NativeFamilyReleaseTransactionError::PreparePersistence);
        }
        StoredDocument::OperatorOff(Box::new(decode_operator_off(
            field(&value, "record")
                .ok_or(NativeFamilyReleaseTransactionError::PreparePersistence)?,
        )?))
    } else {
        // reject unsupported writers before decoding their retained proof bytes
        if version != TRANSACTION_VERSION
            || !exact_keys(
                &value,
                &["compensation", "contractVersion", "proof", "record"],
            )
        {
            return Err(NativeFamilyReleaseTransactionError::PreparePersistence);
        }
        StoredDocument::Mutation(Box::new(StoredMutationDocument {
            compensation: decode_compensation(
                field(&value, "compensation")
                    .ok_or(NativeFamilyReleaseTransactionError::PreparePersistence)?,
            )?,
            proof: decode_proof(
                field(&value, "proof")
                    .ok_or(NativeFamilyReleaseTransactionError::PreparePersistence)?,
            )?,
            record: decode_record(
                field(&value, "record")
                    .ok_or(NativeFamilyReleaseTransactionError::PreparePersistence)?,
            )?,
        }))
    };
    let retained_action = match &document {
        StoredDocument::OperatorOff(record) => &record.input.action_sha256,
        StoredDocument::Mutation(document) => &document.record.input.action_sha256,
    };
    // bind the path-derived identity and canonical bytes before returning state
    if retained_action != action_sha256 || encode_document(&document)?.as_slice() != bytes {
        return Err(NativeFamilyReleaseTransactionError::PreparePersistence);
    }
    Ok(document)
}

// encode the predecessor's fixed nine input operands
fn encode_input(input: &NativeFamilyReleaseInput) -> JsonValue {
    object(vec![
        ("actionSha256", string(&input.action_sha256)),
        ("compensatingRelease", string(&input.compensating_release)),
        (
            "expectedCurrentRelease",
            string(&input.expected_current_release),
        ),
        (
            "expectedSettingsSha256",
            string(&input.expected_settings_sha256),
        ),
        (
            "expectedSourceRelease",
            string(&input.expected_source_release),
        ),
        ("family", string(&input.family)),
        ("fencingToken", string(&input.fencing_token)),
        ("reportSha256", string(&input.report_sha256)),
        ("targetRelease", string(&input.target_release)),
    ])
}

// decode exactly the predecessor's fixed nine input operands
fn decode_input(
    value: &JsonValue,
) -> Result<NativeFamilyReleaseInput, NativeFamilyReleaseTransactionError> {
    let keys = [
        "actionSha256",
        "compensatingRelease",
        "expectedCurrentRelease",
        "expectedSettingsSha256",
        "expectedSourceRelease",
        "family",
        "fencingToken",
        "reportSha256",
        "targetRelease",
    ];
    // prohibit extensions or omissions in replay identity
    if !exact_keys(value, &keys) {
        return Err(NativeFamilyReleaseTransactionError::PreparePersistence);
    }
    Ok(NativeFamilyReleaseInput {
        action_sha256: required_text(value, "actionSha256")?,
        compensating_release: required_text(value, "compensatingRelease")?,
        expected_current_release: required_text(value, "expectedCurrentRelease")?,
        expected_settings_sha256: required_text(value, "expectedSettingsSha256")?,
        expected_source_release: required_text(value, "expectedSourceRelease")?,
        family: required_text(value, "family")?,
        fencing_token: required_text(value, "fencingToken")?,
        report_sha256: required_text(value, "reportSha256")?,
        target_release: required_text(value, "targetRelease")?,
    })
}

// encode one mutation record while preserving omission of unfinished optional fields
fn encode_record(record: &NativeFamilyReleaseMutationRecord) -> JsonValue {
    let mut fields = vec![
        ("action", record.action.clone()),
        ("createdAt", string(&record.created_at)),
        ("input", encode_input(&record.input)),
        ("sourceCommit", string(&record.source_commit)),
        (
            "sourceSettingsSha256",
            string(&record.source_settings_sha256),
        ),
        ("state", string(record.state.as_str())),
        ("targetCommit", string(&record.target_commit)),
    ];
    // append only fields present in the source object spread at this transition
    if let Some(value) = &record.failed_at {
        fields.push(("failedAt", string(value)));
    }
    if let Some(value) = record.outcome {
        fields.push(("outcome", string(value.as_str())));
    }
    if let Some(value) = &record.verified_at {
        fields.push(("verifiedAt", string(value)));
    }
    object(fields)
}

// decode one mutation record and its exact transition-dependent optional members
fn decode_record(
    value: &JsonValue,
) -> Result<NativeFamilyReleaseMutationRecord, NativeFamilyReleaseTransactionError> {
    let state = match required_text(value, "state")?.as_str() {
        "prepared" => NativeFamilyReleaseMutationState::Prepared,
        "applying" => NativeFamilyReleaseMutationState::Applying,
        "compensation_required" => NativeFamilyReleaseMutationState::CompensationRequired,
        "acknowledged" => NativeFamilyReleaseMutationState::Acknowledged,
        _ => return Err(NativeFamilyReleaseTransactionError::PreparePersistence),
    };
    let outcome = match optional_string(value, "outcome")? {
        None => None,
        Some(value) if value == "active" => Some(NativeFamilyReleaseOutcome::Active),
        Some(value) if value == "deployed_operator_off" => {
            Some(NativeFamilyReleaseOutcome::DeployedOperatorOff)
        }
        Some(_) => return Err(NativeFamilyReleaseTransactionError::PreparePersistence),
    };
    let record = NativeFamilyReleaseMutationRecord {
        action: field(value, "action")
            .cloned()
            .ok_or(NativeFamilyReleaseTransactionError::PreparePersistence)?,
        created_at: required_text(value, "createdAt")?,
        failed_at: optional_string(value, "failedAt")?,
        input: decode_input(
            field(value, "input").ok_or(NativeFamilyReleaseTransactionError::PreparePersistence)?,
        )?,
        outcome,
        source_commit: required_text(value, "sourceCommit")?,
        source_settings_sha256: required_text(value, "sourceSettingsSha256")?,
        state,
        target_commit: required_text(value, "targetCommit")?,
        verified_at: optional_string(value, "verifiedAt")?,
    };
    // require precisely the base fields plus each represented optional transition field
    let expected = 7
        + usize::from(record.failed_at.is_some())
        + usize::from(record.outcome.is_some())
        + usize::from(record.verified_at.is_some());
    if !matches!(value, JsonValue::Object(fields) if fields.len() == expected) {
        return Err(NativeFamilyReleaseTransactionError::PreparePersistence);
    }
    Ok(record)
}

// encode the disjoint no-mutation record
fn encode_operator_off(record: &NativeFamilyReleaseOperatorOffRecord) -> JsonValue {
    object(vec![
        ("completedAt", string(&record.completed_at)),
        ("input", encode_input(&record.input)),
        ("sourceCommit", string(&record.source_commit)),
        (
            "sourceSettingsSha256",
            string(&record.source_settings_sha256),
        ),
        ("state", string("operator_off_unapplied")),
    ])
}

// decode the exact disjoint no-mutation record
fn decode_operator_off(
    value: &JsonValue,
) -> Result<NativeFamilyReleaseOperatorOffRecord, NativeFamilyReleaseTransactionError> {
    // retain no proof or mutation-only fields in the operator-off document
    if !exact_keys(
        value,
        &[
            "completedAt",
            "input",
            "sourceCommit",
            "sourceSettingsSha256",
            "state",
        ],
    ) || text(value, "state").as_deref() != Some("operator_off_unapplied")
    {
        return Err(NativeFamilyReleaseTransactionError::PreparePersistence);
    }
    Ok(NativeFamilyReleaseOperatorOffRecord {
        completed_at: required_text(value, "completedAt")?,
        input: decode_input(
            field(value, "input").ok_or(NativeFamilyReleaseTransactionError::PreparePersistence)?,
        )?,
        source_commit: required_text(value, "sourceCommit")?,
        source_settings_sha256: required_text(value, "sourceSettingsSha256")?,
    })
}

// encode the source durable proof with exact base64 byte preservation
fn encode_proof(
    proof: &NativeFamilyReleaseDurableProof,
) -> Result<JsonValue, NativeFamilyReleaseTransactionError> {
    let wire = NativeWirePrimitives;
    Ok(object(vec![
        (
            "baselineCatalogBase64",
            string(
                &wire
                    .encode_standard_base64(&proof.baseline_catalog_bytes)
                    .ok_or(NativeFamilyReleaseTransactionError::PreparePersistence)?,
            ),
        ),
        (
            "compensationActionSha256",
            string(&proof.compensation_action_sha256),
        ),
        ("compensationCommit", string(&proof.compensation_commit)),
        (
            "compensationFamilyIdentitySha256",
            string(&proof.compensation_family_identity_sha256),
        ),
        (
            "compensationImageDigestsSha256",
            string(&proof.compensation_image_digests_sha256),
        ),
        (
            "sourceSettingsBase64",
            string(
                &wire
                    .encode_standard_base64(&proof.source_settings_bytes)
                    .ok_or(NativeFamilyReleaseTransactionError::PreparePersistence)?,
            ),
        ),
        (
            "targetCatalogBase64",
            string(
                &wire
                    .encode_standard_base64(&proof.target_catalog_bytes)
                    .ok_or(NativeFamilyReleaseTransactionError::PreparePersistence)?,
            ),
        ),
        ("targetCommit", string(&proof.target_commit)),
        (
            "targetFamilyIdentitySha256",
            string(&proof.target_family_identity_sha256),
        ),
        (
            "targetImageDigestsSha256",
            string(&proof.target_image_digests_sha256),
        ),
    ]))
}

// decode the exact durable proof and reject alternate base64 spellings
fn decode_proof(
    value: &JsonValue,
) -> Result<NativeFamilyReleaseDurableProof, NativeFamilyReleaseTransactionError> {
    let keys = [
        "baselineCatalogBase64",
        "compensationActionSha256",
        "compensationCommit",
        "compensationFamilyIdentitySha256",
        "compensationImageDigestsSha256",
        "sourceSettingsBase64",
        "targetCatalogBase64",
        "targetCommit",
        "targetFamilyIdentitySha256",
        "targetImageDigestsSha256",
    ];
    // prohibit any durable proof extension or missing recovery operand
    if !exact_keys(value, &keys) {
        return Err(NativeFamilyReleaseTransactionError::PreparePersistence);
    }
    Ok(NativeFamilyReleaseDurableProof {
        baseline_catalog_bytes: required_base64(value, "baselineCatalogBase64")?,
        compensation_action_sha256: required_text(value, "compensationActionSha256")?,
        compensation_commit: required_text(value, "compensationCommit")?,
        compensation_family_identity_sha256: required_text(
            value,
            "compensationFamilyIdentitySha256",
        )?,
        compensation_image_digests_sha256: required_text(value, "compensationImageDigestsSha256")?,
        source_settings_bytes: required_base64(value, "sourceSettingsBase64")?,
        target_catalog_bytes: required_base64(value, "targetCatalogBase64")?,
        target_commit: required_text(value, "targetCommit")?,
        target_family_identity_sha256: required_text(value, "targetFamilyIdentitySha256")?,
        target_image_digests_sha256: required_text(value, "targetImageDigestsSha256")?,
    })
}

// encode one categorical compensation projection
fn encode_compensation(compensation: &StoredCompensation) -> JsonValue {
    object(vec![
        ("actionSha256", string(&compensation.action_sha256)),
        ("release", string(&compensation.release)),
        ("state", string(compensation.state.as_str())),
        (
            "verifiedAt",
            compensation
                .verified_at
                .as_deref()
                .map_or(JsonValue::Null, string),
        ),
    ])
}

// decode one categorical compensation projection
fn decode_compensation(
    value: &JsonValue,
) -> Result<StoredCompensation, NativeFamilyReleaseTransactionError> {
    // preserve the exact fixed categorical compensation record
    if !exact_keys(value, &["actionSha256", "release", "state", "verifiedAt"]) {
        return Err(NativeFamilyReleaseTransactionError::PreparePersistence);
    }
    let state = match required_text(value, "state")?.as_str() {
        "absent" => CompensationState::Absent,
        "failed" => CompensationState::Failed,
        "verified" => CompensationState::Verified,
        _ => return Err(NativeFamilyReleaseTransactionError::PreparePersistence),
    };
    let verified_at = nullable_string(value, "verifiedAt")?;
    // only verified compensation may expose a completion clock
    if (state == CompensationState::Verified) != verified_at.is_some() {
        return Err(NativeFamilyReleaseTransactionError::PreparePersistence);
    }
    Ok(StoredCompensation {
        action_sha256: required_text(value, "actionSha256")?,
        release: required_text(value, "release")?,
        state,
        verified_at,
    })
}

// decode one required standard padded base64 field and its canonical spelling
fn required_base64(
    value: &JsonValue,
    key: &str,
) -> Result<Vec<u8>, NativeFamilyReleaseTransactionError> {
    let encoded = required_text(value, key)?;
    let wire = NativeWirePrimitives;
    let bytes = wire
        .decode_standard_base64(&encoded)
        .ok_or(NativeFamilyReleaseTransactionError::PreparePersistence)?;
    // reject alternate alphabets, padding, or ignored characters
    if wire.encode_standard_base64(&bytes).as_deref() != Some(encoded.as_str()) {
        return Err(NativeFamilyReleaseTransactionError::PreparePersistence);
    }
    Ok(bytes)
}

// decode one required UTF-16 string member
fn required_text(
    value: &JsonValue,
    key: &str,
) -> Result<String, NativeFamilyReleaseTransactionError> {
    text(value, key).ok_or(NativeFamilyReleaseTransactionError::PreparePersistence)
}

// decode one optional omitted UTF-16 string member
fn optional_string(
    value: &JsonValue,
    key: &str,
) -> Result<Option<String>, NativeFamilyReleaseTransactionError> {
    match field(value, key) {
        None => Ok(None),
        Some(JsonValue::String(units)) => String::from_utf16(units)
            .map(Some)
            .map_err(|_| NativeFamilyReleaseTransactionError::PreparePersistence),
        Some(_) => Err(NativeFamilyReleaseTransactionError::PreparePersistence),
    }
}

// decode one required nullable UTF-16 string member
fn nullable_string(
    value: &JsonValue,
    key: &str,
) -> Result<Option<String>, NativeFamilyReleaseTransactionError> {
    match field(value, key) {
        Some(JsonValue::Null) => Ok(None),
        Some(JsonValue::String(units)) => String::from_utf16(units)
            .map(Some)
            .map_err(|_| NativeFamilyReleaseTransactionError::PreparePersistence),
        _ => Err(NativeFamilyReleaseTransactionError::PreparePersistence),
    }
}

// recognize one content-addressed transaction filename
fn transaction_identity(name: &str) -> Option<&str> {
    name.strip_prefix("sha256-")
        .and_then(|value| value.strip_suffix(".json"))
        .filter(|value| valid_hash(value))
}

// recognize one canonical decimal uint64 string
fn canonical_u64(value: &str, allow_zero: bool) -> bool {
    !value.is_empty()
        && value.bytes().all(|byte| byte.is_ascii_digit())
        && (value.len() == 1 || !value.starts_with('0'))
        && (allow_zero || value != "0")
        && value.parse::<u64>().is_ok()
}

// create or verify one literal owner-private directory
fn ensure_private_directory(
    path: &Path,
    uid: u32,
    gid: u32,
) -> Result<(), NativeFamilyReleaseTransactionError> {
    // create only a genuinely absent leaf without adopting an existing foreign path
    match fs::symlink_metadata(path) {
        Ok(_) => {}
        Err(error) if error.kind() == ErrorKind::NotFound => {
            let mut builder = fs::DirBuilder::new();
            builder.mode(0o700);
            builder
                .create(path)
                .map_err(|_| NativeFamilyReleaseTransactionError::PreparePersistence)?;
        }
        Err(_) => return Err(NativeFamilyReleaseTransactionError::PreparePersistence),
    }
    let metadata = fs::symlink_metadata(path)
        .map_err(|_| NativeFamilyReleaseTransactionError::PreparePersistence)?;
    let canonical = fs::canonicalize(path)
        .map_err(|_| NativeFamilyReleaseTransactionError::PreparePersistence)?;
    // reject aliases, links, owner drift, and mode drift rather than repairing them
    if !metadata.is_dir()
        || metadata.file_type().is_symlink()
        || metadata.uid() != uid
        || metadata.gid() != gid
        || metadata.mode() & 0o777 != 0o700
        || canonical != path
    {
        return Err(NativeFamilyReleaseTransactionError::PreparePersistence);
    }
    Ok(())
}

// read one bounded owner-private regular file without following a final link
fn read_private_file(
    path: &Path,
    maximum: usize,
    uid: u32,
    gid: u32,
    absent: bool,
) -> Result<Option<Vec<u8>>, NativeFamilyReleaseTransactionError> {
    let mut file = match OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
    {
        Ok(file) => file,
        Err(error) if absent && error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(NativeFamilyReleaseTransactionError::PreparePersistence),
    };
    let before = file
        .metadata()
        .map_err(|_| NativeFamilyReleaseTransactionError::PreparePersistence)?;
    // require the source's owner-private single-link regular file boundary
    if !before.is_file()
        || before.uid() != uid
        || before.gid() != gid
        || before.mode() & 0o777 != 0o600
        || before.nlink() != 1
        || before.len() > maximum as u64
    {
        return Err(NativeFamilyReleaseTransactionError::PreparePersistence);
    }
    let mut bytes = Vec::with_capacity(usize::try_from(before.len()).unwrap_or(maximum));
    Read::by_ref(&mut file)
        .take((maximum + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| NativeFamilyReleaseTransactionError::PreparePersistence)?;
    let after = file
        .metadata()
        .map_err(|_| NativeFamilyReleaseTransactionError::PreparePersistence)?;
    // reject growth or inode replacement across the bounded descriptor read
    if bytes.len() > maximum
        || before.dev() != after.dev()
        || before.ino() != after.ino()
        || before.len() != after.len()
        || before.mtime() != after.mtime()
        || before.mtime_nsec() != after.mtime_nsec()
    {
        return Err(NativeFamilyReleaseTransactionError::PreparePersistence);
    }
    Ok(Some(bytes))
}

// create one exclusive durable file through a synced temporary hard link
fn write_exclusive(
    path: &Path,
    bytes: &[u8],
    uid: u32,
    gid: u32,
) -> Result<(), NativeFamilyReleaseTransactionError> {
    let (temporary, mut file) = create_temporary(path, 0o600, uid, gid)?;
    // remove the private temporary inode after an incomplete write
    if file
        .write_all(bytes)
        .and_then(|()| file.sync_all())
        .is_err()
    {
        drop(file);
        let _ = fs::remove_file(&temporary);
        return Err(NativeFamilyReleaseTransactionError::PreparePersistence);
    }
    drop(file);
    // leave no stale temporary inode after an exclusive publication collision
    if fs::hard_link(&temporary, path).is_err() {
        let _ = fs::remove_file(&temporary);
        return Err(NativeFamilyReleaseTransactionError::PreparePersistence);
    }
    fs::remove_file(&temporary)
        .map_err(|_| NativeFamilyReleaseTransactionError::PreparePersistence)?;
    sync_parent(path)
}

// atomically replace one durable file after syncing its complete temporary inode
fn write_atomic(
    path: &Path,
    bytes: &[u8],
    mode: u32,
    uid: u32,
    gid: u32,
) -> Result<(), NativeFamilyReleaseTransactionError> {
    let (temporary, mut file) = create_temporary(path, mode, uid, gid)?;
    // remove the private temporary inode after an incomplete write
    if file
        .write_all(bytes)
        .and_then(|()| file.sync_all())
        .is_err()
    {
        drop(file);
        let _ = fs::remove_file(&temporary);
        return Err(NativeFamilyReleaseTransactionError::PreparePersistence);
    }
    drop(file);
    // leave no stale temporary inode after a failed atomic replacement
    if fs::rename(&temporary, path).is_err() {
        let _ = fs::remove_file(&temporary);
        return Err(NativeFamilyReleaseTransactionError::PreparePersistence);
    }
    sync_parent(path)
}

/// Install one exact root-owned catalog through the same synced atomic file boundary.
pub(crate) fn install_native_family_release_catalog(
    path: &Path,
    bytes: &[u8],
    uid: u32,
    gid: u32,
) -> Result<(), NativeFamilyReleaseTransactionError> {
    write_atomic(path, bytes, 0o644, uid, gid)
}

// allocate one unique owner-bound temporary inode beside its final path
fn create_temporary(
    path: &Path,
    mode: u32,
    uid: u32,
    gid: u32,
) -> Result<(PathBuf, File), NativeFamilyReleaseTransactionError> {
    let parent = path
        .parent()
        .ok_or(NativeFamilyReleaseTransactionError::PreparePersistence)?;
    // resolve only bounded stale-name collisions without replacing any temporary inode
    for _ in 0..16 {
        let sequence = TEMPORARY_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let temporary = parent.join(format!(".family-{}-{sequence}.tmp", std::process::id()));
        let opened = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(mode)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&temporary);
        match opened {
            Ok(file) => {
                let metadata = file
                    .metadata()
                    .map_err(|_| NativeFamilyReleaseTransactionError::PreparePersistence)?;
                // ensure the process actually created the expected owner-bound inode
                if metadata.uid() != uid
                    || metadata.gid() != gid
                    || metadata.mode() & 0o777 != mode
                    || metadata.nlink() != 1
                {
                    return Err(NativeFamilyReleaseTransactionError::PreparePersistence);
                }
                return Ok((temporary, file));
            }
            Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
            Err(_) => return Err(NativeFamilyReleaseTransactionError::PreparePersistence),
        }
    }
    Err(NativeFamilyReleaseTransactionError::PreparePersistence)
}

// sync the containing private directory after a complete link or rename transition
fn sync_parent(path: &Path) -> Result<(), NativeFamilyReleaseTransactionError> {
    let parent = path
        .parent()
        .ok_or(NativeFamilyReleaseTransactionError::PreparePersistence)?;
    OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_DIRECTORY)
        .open(parent)
        .and_then(|file| file.sync_all())
        .map_err(|_| NativeFamilyReleaseTransactionError::PreparePersistence)
}

// construct one canonical JSON object from retained ordered members
fn object(fields: Vec<(&str, JsonValue)>) -> JsonValue {
    JsonValue::Object(
        fields
            .into_iter()
            .map(|(key, value)| (key.encode_utf16().collect(), value))
            .collect(),
    )
}

// construct one exact UTF-16 JSON string
fn string(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}
