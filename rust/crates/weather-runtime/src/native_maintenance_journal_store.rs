//! Durable native initialization and append primitive for the maintenance journal.

use std::fmt::{self, Display, Formatter};
use std::fs::{self, File, OpenOptions};
use std::io::{ErrorKind, Write};
use std::os::fd::AsRawFd;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use weather_api::CurrentTimeResolver;
use weather_contracts::JsonValue;

use crate::MaintenanceCanonicalJson;
use crate::native_maintenance_controller_lock::{
    NativeMaintenancePrivateDirectory, ensure_native_maintenance_private_directory,
};
use crate::native_maintenance_journal::{
    NATIVE_MAINTENANCE_JOURNAL_CONTRACT_VERSION, NATIVE_MAINTENANCE_JOURNAL_MAXIMUM_BYTES,
    NativeMaintenanceJournalError, NativeMaintenanceJournalFiles,
    NativeMaintenanceJournalHostFiles, NativeMaintenanceJournalRecord,
    verify_native_maintenance_journal_bytes,
};

const EXT4_MAGIC: libc::c_long = 0xef53;
static TEMPORARY_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Value-free fixed-root, lock, append, head, and journal admission failures.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMaintenanceJournalStoreError {
    Directory,
    Filesystem,
    Lock,
    LockOccupied,
    Input,
    Journal(NativeMaintenanceJournalError),
    File,
    Codec,
    Exhausted,
}

impl Display for NativeMaintenanceJournalStoreError {
    /// Render one bounded durable store operation category.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Directory => "maintenance journal state directory is invalid",
            Self::Filesystem => "maintenance journal filesystem is invalid",
            Self::Lock => "maintenance journal lock is invalid",
            Self::LockOccupied => "maintenance journal lock is occupied",
            Self::Input => "maintenance journal addition is invalid",
            Self::Journal(_) => "maintenance journal chain is invalid",
            Self::File => "maintenance journal durable file operation failed",
            Self::Codec => "maintenance journal serialization failed",
            Self::Exhausted => "maintenance journal generation is exhausted",
        })
    }
}

impl std::error::Error for NativeMaintenanceJournalStoreError {
    /// Preserve only the source journal admission failure when available.
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Journal(error) => Some(error),
            _ => None,
        }
    }
}

impl From<NativeMaintenanceJournalError> for NativeMaintenanceJournalStoreError {
    /// Retain verified-chain failure ownership at the journal boundary.
    fn from(error: NativeMaintenanceJournalError) -> Self {
        Self::Journal(error)
    }
}

/// One exact pending append event before generation and content identities exist.
#[derive(Clone, Copy, Debug)]
pub struct NativeMaintenanceJournalAddition<'a> {
    pub due_key: Option<&'a str>,
    pub kind: &'a str,
    pub payload: &'a JsonValue,
    pub record_key: &'a str,
    pub recorded_at: &'a str,
}

/// Owned lifecycle mutation derived while the durable verified journal lock is held.
pub struct NativeMaintenanceOwnedJournalAddition {
    pub due_key: Option<String>,
    pub kind: String,
    pub payload: JsonValue,
    pub record_key: String,
    pub recorded_at: String,
}

/// Path-free verified initialization or append projection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeMaintenanceJournalStoreStatus {
    pub head_sha256: Option<String>,
    pub records: u64,
}

/// Fixed-root owner for exclusive verified append and bounded head reconciliation.
#[derive(Debug)]
pub struct NativeMaintenanceJournalStore {
    root: PathBuf,
    uid: u32,
}

impl NativeMaintenanceJournalStore {
    /// Initialize the fixed production state root and reconcile its durable heads.
    pub fn initialize(
        codec: &mut impl MaintenanceCanonicalJson,
        time: &dyn CurrentTimeResolver,
    ) -> Result<(Self, NativeMaintenanceJournalStoreStatus), NativeMaintenanceJournalStoreError>
    {
        let root =
            ensure_native_maintenance_private_directory(NativeMaintenancePrivateDirectory::State)
                .map_err(|_| NativeMaintenanceJournalStoreError::Directory)?;
        validate_root(&root, true)?;
        let mut store = Self {
            root,
            uid: current_uid(),
        };
        let status = store.reconcile_heads(codec, time)?;
        Ok((store, status))
    }

    /// Read, verify, and return the complete chain under the same exclusive lock.
    pub fn read_verified(
        &mut self,
        codec: &mut impl MaintenanceCanonicalJson,
        time: &dyn CurrentTimeResolver,
    ) -> Result<Vec<NativeMaintenanceJournalRecord>, NativeMaintenanceJournalStoreError> {
        let _lock = self.acquire_lock()?;
        self.read_verified_locked(codec, time)
    }

    /// Reconcile current and previous projections from the verified append-only chain.
    pub fn reconcile_heads(
        &mut self,
        codec: &mut impl MaintenanceCanonicalJson,
        time: &dyn CurrentTimeResolver,
    ) -> Result<NativeMaintenanceJournalStoreStatus, NativeMaintenanceJournalStoreError> {
        let _lock = self.acquire_lock()?;
        let records = self.read_verified_locked(codec, time)?;
        self.write_heads(&records, codec)?;
        status(&records)
    }

    /// Verify, append, fsync, and rotate one nonempty ordered event batch.
    pub fn append(
        &mut self,
        additions: &[NativeMaintenanceJournalAddition<'_>],
        codec: &mut impl MaintenanceCanonicalJson,
        time: &dyn CurrentTimeResolver,
    ) -> Result<NativeMaintenanceJournalStoreStatus, NativeMaintenanceJournalStoreError> {
        use NativeMaintenanceJournalStoreError as Error;

        // match the lifecycle owner by refusing an empty mutation primitive
        if additions.is_empty() {
            return Err(Error::Input);
        }
        let _lock = self.acquire_lock()?;
        let existing_bytes = self.read_journal_bytes()?;
        let existing = verify_native_maintenance_journal_bytes(&existing_bytes, codec, time)?;
        let (append_bytes, complete_bytes) =
            prepare_append(&existing_bytes, &existing, additions, codec)?;
        let appended = verify_native_maintenance_journal_bytes(&complete_bytes, codec, time)?;
        // require every requested generation before mutating the durable journal
        if appended.len() != existing.len() + additions.len() {
            return Err(Error::Journal(NativeMaintenanceJournalError::Chain));
        }
        self.append_bytes(&append_bytes)?;
        self.write_heads(&appended, codec)?;
        status(&appended)
    }

    /// Verify, decide and append one lifecycle mutation under a single original journal lock.
    pub fn mutate_checked<E>(
        &mut self,
        codec: &mut impl MaintenanceCanonicalJson,
        time: &dyn CurrentTimeResolver,
        decide: impl FnOnce(
            &[NativeMaintenanceJournalRecord],
        ) -> Result<Vec<NativeMaintenanceOwnedJournalAddition>, E>,
    ) -> Result<(), E>
    where
        E: From<NativeMaintenanceJournalStoreError>,
    {
        let _lock = self.acquire_lock()?;
        let existing_bytes = self.read_journal_bytes()?;
        let existing = verify_native_maintenance_journal_bytes(&existing_bytes, codec, time)
            .map_err(NativeMaintenanceJournalStoreError::from)?;
        let owned = decide(&existing)?;
        // retain exact source idempotence without rotating or rewriting the heads
        if owned.is_empty() {
            return Ok(());
        }
        let additions: Vec<_> = owned
            .iter()
            .map(|addition| NativeMaintenanceJournalAddition {
                due_key: addition.due_key.as_deref(),
                kind: &addition.kind,
                payload: &addition.payload,
                record_key: &addition.record_key,
                recorded_at: &addition.recorded_at,
            })
            .collect();
        let (append_bytes, complete_bytes) =
            prepare_append(&existing_bytes, &existing, &additions, codec)?;
        let appended = verify_native_maintenance_journal_bytes(&complete_bytes, codec, time)
            .map_err(NativeMaintenanceJournalStoreError::from)?;
        // require each exact requested new generation before releasing the verified append bytes
        if appended.len() != existing.len() + additions.len() {
            return Err(NativeMaintenanceJournalStoreError::Journal(
                NativeMaintenanceJournalError::Chain,
            )
            .into());
        }
        self.append_bytes(&append_bytes)?;
        self.write_heads(&appended, codec)?;
        Ok(())
    }

    /// Construct a disposable-root owner while retaining the same file semantics.
    #[cfg(test)]
    pub(crate) fn initialize_fixture(
        root: PathBuf,
        codec: &mut impl MaintenanceCanonicalJson,
        time: &dyn CurrentTimeResolver,
    ) -> Result<(Self, NativeMaintenanceJournalStoreStatus), NativeMaintenanceJournalStoreError>
    {
        validate_root(&root, false)?;
        let mut store = Self {
            root,
            uid: current_uid(),
        };
        let status = store.reconcile_heads(codec, time)?;
        Ok((store, status))
    }

    /// Read one coherent bounded journal snapshot while the caller holds the lock.
    fn read_journal_bytes(&self) -> Result<Vec<u8>, NativeMaintenanceJournalStoreError> {
        let mut files = NativeMaintenanceJournalHostFiles::for_root(self.root.clone(), self.uid);
        files.read_journal().map_err(Into::into)
    }

    /// Verify one coherent journal snapshot while the caller holds the lock.
    fn read_verified_locked(
        &self,
        codec: &mut impl MaintenanceCanonicalJson,
        time: &dyn CurrentTimeResolver,
    ) -> Result<Vec<NativeMaintenanceJournalRecord>, NativeMaintenanceJournalStoreError> {
        let bytes = self.read_journal_bytes()?;
        verify_native_maintenance_journal_bytes(&bytes, codec, time).map_err(Into::into)
    }

    /// Acquire one nonblocking kernel lock on the validated literal journal lock inode.
    fn acquire_lock(&self) -> Result<File, NativeMaintenanceJournalStoreError> {
        use NativeMaintenanceJournalStoreError as Error;

        let path = self.root.join("journal.lock");
        let held = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&path)
            .map_err(|_| Error::Lock)?;
        let metadata = held.metadata().map_err(|_| Error::Lock)?;
        let current = fs::symlink_metadata(&path).map_err(|_| Error::Lock)?;
        // bind a private single-link lock path to the descriptor before flock
        if !private_file(&metadata, self.uid) || !same_inode(&metadata, &current) {
            return Err(Error::Lock);
        }
        // safety: the owned descriptor remains live for the complete transaction
        let result = unsafe { libc::flock(held.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
        // preserve nonblocking serialization instead of queueing another writer
        if result != 0 {
            let error = std::io::Error::last_os_error();
            // distinguish ordinary lock occupancy from an invalid lock operation
            if error.kind() == ErrorKind::WouldBlock {
                return Err(Error::LockOccupied);
            }
            return Err(Error::Lock);
        }
        Ok(held)
    }

    /// Append and fsync exact record lines before rotating any head projection.
    fn append_bytes(&self, bytes: &[u8]) -> Result<(), NativeMaintenanceJournalStoreError> {
        use NativeMaintenanceJournalStoreError as Error;

        let path = self.root.join("journal.jsonl");
        let mut file = OpenOptions::new()
            .append(true)
            .create(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(path)
            .map_err(|_| Error::File)?;
        let details = file.metadata().map_err(|_| Error::File)?;
        let appended_size = details
            .size()
            .checked_add(u64::try_from(bytes.len()).map_err(|_| Error::File)?)
            .ok_or(Error::File)?;
        // preserve the source append target and complete-journal byte ceiling
        if !private_file(&details, self.uid)
            || appended_size > NATIVE_MAINTENANCE_JOURNAL_MAXIMUM_BYTES as u64
        {
            return Err(Error::File);
        }
        file.write_all(bytes).map_err(|_| Error::File)?;
        file.sync_all().map_err(|_| Error::File)
    }

    /// Atomically rotate current and previous projections, then fsync their directory.
    fn write_heads(
        &self,
        records: &[NativeMaintenanceJournalRecord],
        codec: &mut impl MaintenanceCanonicalJson,
    ) -> Result<(), NativeMaintenanceJournalStoreError> {
        use NativeMaintenanceJournalStoreError as Error;

        let current = records.last();
        let current_value = head_value(current);
        let current_bytes = codec
            .encode_canonical_json(&current_value)
            .ok_or(Error::Codec)?;
        write_atomic_private_file(&self.root.join("head.current"), &current_bytes)?;
        let previous = records
            .len()
            .checked_sub(2)
            .and_then(|index| records.get(index));
        let previous_path = self.root.join("head.previous");
        // remove only the absent previous projection at genesis or one-record history
        if let Some(previous) = previous {
            let previous_bytes = codec
                .encode_canonical_json(&head_value(Some(previous)))
                .ok_or(Error::Codec)?;
            write_atomic_private_file(&previous_path, &previous_bytes)?;
        } else {
            match fs::remove_file(previous_path) {
                Ok(()) => {}
                Err(error) if error.kind() == ErrorKind::NotFound => {}
                Err(_) => return Err(Error::File),
            }
        }
        sync_private_directory(&self.root)
    }
}

/// Build and preverify every canonical record line before durable mutation.
fn prepare_append(
    existing_bytes: &[u8],
    existing: &[NativeMaintenanceJournalRecord],
    additions: &[NativeMaintenanceJournalAddition<'_>],
    codec: &mut impl MaintenanceCanonicalJson,
) -> Result<(Vec<u8>, Vec<u8>), NativeMaintenanceJournalStoreError> {
    use NativeMaintenanceJournalStoreError as Error;

    let mut generation = existing.last().map_or(0, |record| record.generation);
    let mut previous_sha256 = existing.last().map(|record| record.record_sha256.clone());
    let mut append_bytes = Vec::new();
    // append every requested record in one deterministic source order
    for addition in additions {
        // close the pending event kind and ordinary-object payload before hashing
        if !valid_kind(addition.kind) || !matches!(addition.payload, JsonValue::Object(_)) {
            return Err(Error::Input);
        }
        generation = generation.checked_add(1).ok_or(Error::Exhausted)?;
        let payload_bytes = codec
            .encode_canonical_json(addition.payload)
            .ok_or(Error::Codec)?;
        let payload_sha256 = codec
            .sha256(&payload_bytes)
            .ok_or(Error::Codec)?
            .as_str()
            .to_owned();
        let without_hash = object(vec![
            string_field(
                "contractVersion",
                NATIVE_MAINTENANCE_JOURNAL_CONTRACT_VERSION,
            ),
            string_field("generation", &generation.to_string()),
            string_field("kind", addition.kind),
            string_field("recordKey", addition.record_key),
            nullable_string_field("dueKey", addition.due_key),
            string_field("recordedAt", addition.recorded_at),
            nullable_owned_string_field("previousSha256", previous_sha256.as_deref()),
            string_field("payloadSha256", &payload_sha256),
            ("payload".encode_utf16().collect(), addition.payload.clone()),
        ]);
        let unsigned_bytes = codec
            .encode_canonical_json(&without_hash)
            .ok_or(Error::Codec)?;
        let record_sha256 = codec
            .sha256(&unsigned_bytes)
            .ok_or(Error::Codec)?
            .as_str()
            .to_owned();
        let JsonValue::Object(mut fields) = without_hash else {
            return Err(Error::Codec);
        };
        fields.push(string_field("recordSha256", &record_sha256));
        let record_bytes = codec
            .encode_canonical_json(&JsonValue::Object(fields))
            .ok_or(Error::Codec)?;
        append_bytes
            .len()
            .checked_add(record_bytes.len())
            .ok_or(Error::File)?;
        append_bytes.extend_from_slice(&record_bytes);
        previous_sha256 = Some(record_sha256);
    }
    let complete_length = existing_bytes
        .len()
        .checked_add(append_bytes.len())
        .ok_or(Error::File)?;
    // refuse an oversized completed journal before allocating or writing it
    if complete_length > NATIVE_MAINTENANCE_JOURNAL_MAXIMUM_BYTES {
        return Err(Error::File);
    }
    let mut complete_bytes = Vec::with_capacity(complete_length);
    complete_bytes.extend_from_slice(existing_bytes);
    complete_bytes.extend_from_slice(&append_bytes);
    Ok((append_bytes, complete_bytes))
}

/// Render one bounded current or previous journal-head projection.
fn head_value(record: Option<&NativeMaintenanceJournalRecord>) -> JsonValue {
    let generation = record.map_or_else(|| "0".to_owned(), |record| record.generation.to_string());
    object(vec![
        string_field("generation", &generation),
        nullable_owned_string_field(
            "recordSha256",
            record.map(|record| record.record_sha256.as_str()),
        ),
    ])
}

/// Project one path-free verified chain status.
fn status(
    records: &[NativeMaintenanceJournalRecord],
) -> Result<NativeMaintenanceJournalStoreStatus, NativeMaintenanceJournalStoreError> {
    Ok(NativeMaintenanceJournalStoreStatus {
        head_sha256: records.last().map(|record| record.record_sha256.clone()),
        records: u64::try_from(records.len())
            .map_err(|_| NativeMaintenanceJournalStoreError::Exhausted)?,
    })
}

/// Atomically replace one owner-private state projection through a unique temporary file.
fn write_atomic_private_file(
    path: &Path,
    bytes: &[u8],
) -> Result<(), NativeMaintenanceJournalStoreError> {
    use NativeMaintenanceJournalStoreError as Error;

    let parent = path.parent().ok_or(Error::File)?;
    let mut temporary = None;
    // resolve bounded stale-name collisions without replacing an existing temporary inode
    for _ in 0..16 {
        let sequence = TEMPORARY_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let candidate = parent.join(format!(".state-{}-{sequence}.tmp", std::process::id()));
        let opened = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&candidate);
        // retain the first genuinely unique private temporary inode
        match opened {
            Ok(file) => {
                temporary = Some((candidate, file));
                break;
            }
            Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
            Err(_) => return Err(Error::File),
        }
    }
    let (temporary_path, mut file) = temporary.ok_or(Error::File)?;
    file.write_all(bytes).map_err(|_| Error::File)?;
    file.sync_all().map_err(|_| Error::File)?;
    drop(file);
    fs::rename(temporary_path, path).map_err(|_| Error::File)
}

/// Fsync one held private directory after the complete head rotation.
fn sync_private_directory(path: &Path) -> Result<(), NativeMaintenanceJournalStoreError> {
    let directory = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_DIRECTORY)
        .open(path)
        .map_err(|_| NativeMaintenanceJournalStoreError::File)?;
    directory
        .sync_all()
        .map_err(|_| NativeMaintenanceJournalStoreError::File)
}

/// Validate the fixed production root or one explicitly disposable test root.
fn validate_root(
    root: &Path,
    require_ext4: bool,
) -> Result<(), NativeMaintenanceJournalStoreError> {
    use NativeMaintenanceJournalStoreError as Error;

    let details = fs::symlink_metadata(root).map_err(|_| Error::Directory)?;
    // require one literal owner-private state directory
    if !details.is_dir()
        || details.file_type().is_symlink()
        || details.uid() != current_uid()
        || details.permissions().mode() & 0o777 != 0o700
        || fs::canonicalize(root).ok().as_deref() != Some(root)
    {
        return Err(Error::Directory);
    }
    // preserve the production native-ext4 boundary without burdening disposable fixtures
    if require_ext4 && filesystem_type(root)? != EXT4_MAGIC {
        return Err(Error::Filesystem);
    }
    Ok(())
}

/// Read one filesystem type through the held literal path without mutation.
fn filesystem_type(path: &Path) -> Result<libc::c_long, NativeMaintenanceJournalStoreError> {
    let path = std::ffi::CString::new(path.as_os_str().as_bytes())
        .map_err(|_| NativeMaintenanceJournalStoreError::Filesystem)?;
    let mut details = std::mem::MaybeUninit::<libc::statfs>::uninit();
    // safety: statfs initializes the output only after a successful return
    let result = unsafe { libc::statfs(path.as_ptr(), details.as_mut_ptr()) };
    // reject an unavailable or substituted filesystem observation
    if result != 0 {
        return Err(NativeMaintenanceJournalStoreError::Filesystem);
    }
    // safety: the successful syscall initialized the complete statfs value
    Ok(unsafe { details.assume_init() }.f_type)
}

/// Recognize the source's closed pending event-kind grammar.
fn valid_kind(value: &str) -> bool {
    (1..=64).contains(&value.len())
        && value.bytes().enumerate().all(|(index, byte)| {
            // require the leading lowercase letter before the remaining closed alphabet
            if index == 0 {
                byte.is_ascii_lowercase()
            } else {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_'
            }
        })
}

/// Classify one exact owner-private single-link file.
fn private_file(details: &fs::Metadata, uid: u32) -> bool {
    details.is_file()
        && !details.file_type().is_symlink()
        && details.uid() == uid
        && details.permissions().mode() & 0o777 == 0o600
        && details.nlink() == 1
}

/// Bind two metadata observations to the same inode.
fn same_inode(left: &fs::Metadata, right: &fs::Metadata) -> bool {
    left.dev() == right.dev() && left.ino() == right.ino()
}

/// Construct one recursive generated JSON object.
fn object(fields: Vec<(Vec<u16>, JsonValue)>) -> JsonValue {
    JsonValue::Object(fields)
}

/// Construct one generated JSON string member.
fn string_field(key: &str, value: &str) -> (Vec<u16>, JsonValue) {
    (
        key.encode_utf16().collect(),
        JsonValue::String(value.encode_utf16().collect()),
    )
}

/// Construct one borrowed nullable generated JSON string member.
fn nullable_string_field(key: &str, value: Option<&str>) -> (Vec<u16>, JsonValue) {
    nullable_owned_string_field(key, value)
}

/// Construct one nullable generated JSON string member.
fn nullable_owned_string_field(key: &str, value: Option<&str>) -> (Vec<u16>, JsonValue) {
    (
        key.encode_utf16().collect(),
        value.map_or(JsonValue::Null, |value| {
            JsonValue::String(value.encode_utf16().collect())
        }),
    )
}

/// Read the process real uid used by the predecessor owner checks.
fn current_uid() -> u32 {
    // libc exposes the immutable process credential without host mutation
    unsafe { libc::getuid() }
}
