//! Native warm-restart storage for bounded forecast scheduler history.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::native_revision_spool_file::{
    create_synced_file, ensure_child_directory, read_optional_file, require_directory,
    sync_directory,
};

/// Exact scheduler restart-state wire contract.
pub const FORECAST_SCHEDULER_STATE_CONTRACT_VERSION: &str =
    "adjustment-evidence-scheduler-state/v1";
/// Maximum retained bytes for each current or previous state slot.
pub const FORECAST_SCHEDULER_STATE_MAXIMUM_BYTES: usize = 1_024 * 1_024;
/// Maximum finite attempt, error, or gap history entries.
pub const FORECAST_SCHEDULER_HISTORY_LIMIT: usize = 4 * 365;

const SCHEDULER_DIRECTORY: &str = "scheduler";
const CURRENT_STATE_NAME: &str = "current.json";
const PREVIOUS_STATE_NAME: &str = "previous.json";

/// One durable scheduler capture attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForecastSchedulerAttempt {
    pub committed_at: String,
    pub due_key: String,
    pub status: String,
}

/// One durable bounded scheduler failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForecastSchedulerError {
    pub at: String,
    pub code: String,
    pub due_key: String,
}

/// Closed durable gap reason retained by the source state schema.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForecastSchedulerGapReason {
    CaptureFailed,
    MissedCapture,
}

impl ForecastSchedulerGapReason {
    /// return the exact canonical wire spelling.
    pub const fn wire(self) -> &'static str {
        match self {
            Self::CaptureFailed => "capture_failed",
            Self::MissedCapture => "missed_capture",
        }
    }
}

/// One durable categorical scheduler gap.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForecastSchedulerGap {
    pub due_key: String,
    pub occurred_at: String,
    pub reason: ForecastSchedulerGapReason,
}

/// Complete bounded scheduler state retained across warm restarts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForecastSchedulerState {
    pub activation: String,
    pub attempts: Vec<ForecastSchedulerAttempt>,
    pub contract_version: &'static str,
    pub errors: Vec<ForecastSchedulerError>,
    pub gaps: Vec<ForecastSchedulerGap>,
    pub last_checked_at: Option<String>,
    pub updated_at: Option<String>,
}

impl ForecastSchedulerState {
    /// create the source's stable empty first-run state.
    pub fn empty() -> Self {
        Self {
            activation: "not_initialized".to_owned(),
            attempts: Vec::new(),
            contract_version: FORECAST_SCHEDULER_STATE_CONTRACT_VERSION,
            errors: Vec::new(),
            gaps: Vec::new(),
            last_checked_at: None,
            updated_at: None,
        }
    }
}

/// Existing exact JSON and instant owner for scheduler state documents.
pub trait ForecastSchedulerStateCodec {
    /// decode and validate one complete retained scheduler state.
    fn decode_state(&self, canonical_bytes: &[u8]) -> Option<ForecastSchedulerState>;

    /// validate and encode one complete state in canonical newline framing.
    fn encode_state(&self, state: &ForecastSchedulerState) -> Option<Vec<u8>>;
}

/// Ordinary scheduler state-store failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForecastSchedulerStateStoreError {
    InvalidState,
    StateTooLarge,
    Storage,
}

/// Concrete current/previous scheduler state store below an existing private root.
pub struct NativeForecastSchedulerStateStore {
    root: PathBuf,
}

impl NativeForecastSchedulerStateStore {
    /// admit one existing private root and create only its fixed scheduler child.
    pub fn initialize(root: PathBuf) -> io::Result<Self> {
        require_directory(&root)?;
        ensure_child_directory(&root.join(SCHEDULER_DIRECTORY))?;
        Ok(Self { root })
    }

    /// borrow the admitted adjustment-evidence root.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// recover current, previous, or one empty first-run state.
    pub fn read(
        &self,
        codec: &impl ForecastSchedulerStateCodec,
    ) -> Result<ForecastSchedulerState, ForecastSchedulerStateStoreError> {
        let mut invalid_observed = false;
        // prefer current while retaining one previous fallback after invalid state
        for name in [CURRENT_STATE_NAME, PREVIOUS_STATE_NAME] {
            match read_optional_file(
                &self.root.join(SCHEDULER_DIRECTORY).join(name),
                FORECAST_SCHEDULER_STATE_MAXIMUM_BYTES,
            ) {
                Ok(None) => continue,
                Ok(Some(bytes)) => {
                    if let Some(state) = codec.decode_state(&bytes) {
                        return Ok(state);
                    }
                    invalid_observed = true;
                }
                Err(_) => invalid_observed = true,
            }
        }
        // never reset known failed durable history to a fresh empty state
        if invalid_observed {
            return Err(ForecastSchedulerStateStoreError::InvalidState);
        }
        Ok(ForecastSchedulerState::empty())
    }

    /// rotate only validated current bytes before replacing current atomically.
    pub fn write(
        &mut self,
        state: &ForecastSchedulerState,
        codec: &impl ForecastSchedulerStateCodec,
    ) -> Result<(), ForecastSchedulerStateStoreError> {
        let encoded = codec
            .encode_state(state)
            .ok_or(ForecastSchedulerStateStoreError::InvalidState)?;
        // keep every new current slot inside the source read ceiling
        if encoded.len() > FORECAST_SCHEDULER_STATE_MAXIMUM_BYTES {
            return Err(ForecastSchedulerStateStoreError::StateTooLarge);
        }
        let directory = self.root.join(SCHEDULER_DIRECTORY);
        let current_path = directory.join(CURRENT_STATE_NAME);
        let previous_path = directory.join(PREVIOUS_STATE_NAME);
        let current = read_optional_file(&current_path, FORECAST_SCHEDULER_STATE_MAXIMUM_BYTES)
            .map_err(|_| ForecastSchedulerStateStoreError::Storage)?;
        // rotate only one complete current generation and preserve previous otherwise
        if let Some(current) = current {
            // preserve the previous generation when the current document is invalid
            if codec.decode_state(&current).is_some() {
                // preserve current write admission when previous rotation is unavailable
                let _ = self.atomic_replace(&previous_path, &current);
            }
        }
        self.atomic_replace(&current_path, &encoded)
    }

    /// publish one synced temporary through rename and directory synchronization.
    fn atomic_replace(
        &self,
        target: &Path,
        bytes: &[u8],
    ) -> Result<(), ForecastSchedulerStateStoreError> {
        let directory = target
            .parent()
            .ok_or(ForecastSchedulerStateStoreError::Storage)?;
        let unique = super::uuid_from_linux_entropy()
            .map_err(|_| ForecastSchedulerStateStoreError::Storage)?;
        let temporary = directory.join(format!(".state-{unique}.tmp"));
        let result = (|| {
            create_synced_file(&temporary, bytes)?;
            fs::rename(&temporary, target)?;
            sync_directory(directory)
        })();
        // remove only this invocation's temporary after a failed replacement
        if result.is_err() {
            match fs::remove_file(&temporary) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(_) => return Err(ForecastSchedulerStateStoreError::Storage),
            }
        }
        result.map_err(|_| ForecastSchedulerStateStoreError::Storage)
    }
}

#[cfg(test)]
mod tests {
    use std::fs::{self, DirBuilder, OpenOptions};
    use std::io::Write;
    use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    struct FixtureCodec;

    impl ForecastSchedulerStateCodec for FixtureCodec {
        /// decode one complete delimiter-framed scheduler fixture.
        fn decode_state(&self, canonical_bytes: &[u8]) -> Option<ForecastSchedulerState> {
            let source = std::str::from_utf8(canonical_bytes).ok()?;
            source.ends_with('\n').then_some(())?;
            let mut lines = source.lines();
            let header = lines.next()?.split('\t').collect::<Vec<_>>();
            let [
                "S",
                contract_version,
                activation,
                last_checked_at,
                updated_at,
            ] = header.as_slice()
            else {
                return None;
            };
            (*contract_version == FORECAST_SCHEDULER_STATE_CONTRACT_VERSION).then_some(())?;
            let mut state = ForecastSchedulerState {
                activation: (*activation).to_owned(),
                attempts: Vec::new(),
                contract_version: FORECAST_SCHEDULER_STATE_CONTRACT_VERSION,
                errors: Vec::new(),
                gaps: Vec::new(),
                last_checked_at: decode_optional(last_checked_at),
                updated_at: decode_optional(updated_at),
            };
            // restore each typed history entry in retained order
            for line in lines {
                let fields = line.split('\t').collect::<Vec<_>>();
                match fields.as_slice() {
                    ["A", committed_at, due_key, status] => {
                        state.attempts.push(ForecastSchedulerAttempt {
                            committed_at: (*committed_at).to_owned(),
                            due_key: (*due_key).to_owned(),
                            status: (*status).to_owned(),
                        });
                    }
                    ["E", at, code, due_key] => {
                        state.errors.push(ForecastSchedulerError {
                            at: (*at).to_owned(),
                            code: (*code).to_owned(),
                            due_key: (*due_key).to_owned(),
                        });
                    }
                    ["G", due_key, occurred_at, reason] => {
                        let reason = match *reason {
                            "capture_failed" => ForecastSchedulerGapReason::CaptureFailed,
                            "missed_capture" => ForecastSchedulerGapReason::MissedCapture,
                            _ => return None,
                        };
                        state.gaps.push(ForecastSchedulerGap {
                            due_key: (*due_key).to_owned(),
                            occurred_at: (*occurred_at).to_owned(),
                            reason,
                        });
                    }
                    _ => return None,
                }
            }
            // enforce the fixture's same bounded structural state contract
            if state.activation.len() > 64
                || state.attempts.len() > FORECAST_SCHEDULER_HISTORY_LIMIT
                || state.errors.len() > FORECAST_SCHEDULER_HISTORY_LIMIT
                || state.gaps.len() > FORECAST_SCHEDULER_HISTORY_LIMIT
                || state.last_checked_at.is_none() && *last_checked_at != "-"
                || state.updated_at.is_none() && *updated_at != "-"
            {
                return None;
            }
            Some(state)
        }

        /// encode one complete delimiter-framed scheduler fixture.
        fn encode_state(&self, state: &ForecastSchedulerState) -> Option<Vec<u8>> {
            // reject state outside the same closed fixture contract
            if state.contract_version != FORECAST_SCHEDULER_STATE_CONTRACT_VERSION
                || state.activation.len() > 64
                || state.attempts.len() > FORECAST_SCHEDULER_HISTORY_LIMIT
                || state.errors.len() > FORECAST_SCHEDULER_HISTORY_LIMIT
                || state.gaps.len() > FORECAST_SCHEDULER_HISTORY_LIMIT
                || !valid_field(&state.activation)
            {
                return None;
            }
            let mut lines = vec![format!(
                "S\t{}\t{}\t{}\t{}",
                state.contract_version,
                state.activation,
                encode_optional(state.last_checked_at.as_deref())?,
                encode_optional(state.updated_at.as_deref())?,
            )];
            // encode attempts in retained order
            for attempt in &state.attempts {
                valid_field(&attempt.committed_at).then_some(())?;
                valid_field(&attempt.due_key).then_some(())?;
                valid_field(&attempt.status).then_some(())?;
                lines.push(format!(
                    "A\t{}\t{}\t{}",
                    attempt.committed_at, attempt.due_key, attempt.status
                ));
            }
            // encode errors in retained order
            for error in &state.errors {
                valid_field(&error.at).then_some(())?;
                valid_field(&error.code).then_some(())?;
                valid_field(&error.due_key).then_some(())?;
                lines.push(format!(
                    "E\t{}\t{}\t{}",
                    error.at, error.code, error.due_key
                ));
            }
            // encode gaps in retained order
            for gap in &state.gaps {
                valid_field(&gap.due_key).then_some(())?;
                valid_field(&gap.occurred_at).then_some(())?;
                lines.push(format!(
                    "G\t{}\t{}\t{}",
                    gap.due_key,
                    gap.occurred_at,
                    gap.reason.wire()
                ));
            }
            Some(format!("{}\n", lines.join("\n")).into_bytes())
        }
    }

    struct OversizedCodec;

    impl ForecastSchedulerStateCodec for OversizedCodec {
        /// refuse decoding because the oversized fixture is write-only.
        fn decode_state(&self, _canonical_bytes: &[u8]) -> Option<ForecastSchedulerState> {
            None
        }

        /// return one body just beyond the source read ceiling.
        fn encode_state(&self, _state: &ForecastSchedulerState) -> Option<Vec<u8>> {
            Some(vec![b'x'; FORECAST_SCHEDULER_STATE_MAXIMUM_BYTES + 1])
        }
    }

    struct DisposableRoot(PathBuf);

    impl DisposableRoot {
        /// create one isolated owner-private scheduler root.
        fn new() -> Self {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("fixture clock")
                .as_nanos();
            let root = std::env::temp_dir().join(format!(
                "weather-forecast-scheduler-{}-{nonce}",
                std::process::id()
            ));
            let mut builder = DirBuilder::new();
            builder.mode(0o700).create(&root).expect("fixture root");
            Self(root)
        }
    }

    impl Drop for DisposableRoot {
        /// remove only this disposable scheduler tree.
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// encode one optional fixture field without ambiguity.
    fn encode_optional(value: Option<&str>) -> Option<&str> {
        match value {
            Some(value) if valid_field(value) && value != "-" => Some(value),
            Some(_) => None,
            None => Some("-"),
        }
    }

    /// decode one optional fixture field.
    fn decode_optional(value: &str) -> Option<String> {
        // reserve one fixture-only null spelling
        if value == "-" {
            return None;
        }
        Some(value.to_owned())
    }

    /// admit one delimiter-safe fixture field.
    fn valid_field(value: &str) -> bool {
        !value.contains(['\t', '\n', '\r'])
    }

    /// construct one state carrying all three durable history categories.
    fn state(activation: &str, hour: &str) -> ForecastSchedulerState {
        let due_key = format!("capture/2026-11-23T{hour}:35:00.000Z");
        let at = format!("2026-11-23T{hour}:36:00.000Z");
        ForecastSchedulerState {
            activation: activation.to_owned(),
            attempts: vec![ForecastSchedulerAttempt {
                committed_at: at.clone(),
                due_key: due_key.clone(),
                status: "committed".to_owned(),
            }],
            contract_version: FORECAST_SCHEDULER_STATE_CONTRACT_VERSION,
            errors: vec![ForecastSchedulerError {
                at: at.clone(),
                code: "capture_retry".to_owned(),
                due_key: due_key.clone(),
            }],
            gaps: vec![ForecastSchedulerGap {
                due_key,
                occurred_at: at.clone(),
                reason: ForecastSchedulerGapReason::CaptureFailed,
            }],
            last_checked_at: Some(at.clone()),
            updated_at: Some(at),
        }
    }

    /// replace one existing private fixture while retaining its mode.
    fn replace_private(path: &Path, bytes: &[u8]) {
        let mut file = OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(path)
            .expect("fixture replacement");
        file.write_all(bytes).expect("fixture bytes");
        file.sync_all().expect("fixture sync");
    }

    /// create one new private fixture file.
    fn create_private(path: &Path, bytes: &[u8]) {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)
            .expect("fixture file");
        file.write_all(bytes).expect("fixture bytes");
        file.sync_all().expect("fixture sync");
    }

    /// derive one fixed scheduler slot path.
    fn slot(root: &DisposableRoot, name: &str) -> PathBuf {
        root.0.join(SCHEDULER_DIRECTORY).join(name)
    }

    // verify first-run state and one warm-restart current generation
    #[test]
    fn reads_empty_first_run_and_written_current_after_restart() {
        let root = DisposableRoot::new();
        let mut store =
            NativeForecastSchedulerStateStore::initialize(root.0.clone()).expect("store");
        assert_eq!(
            store.read(&FixtureCodec),
            Ok(ForecastSchedulerState::empty())
        );
        let current = state("active", "00");
        store.write(&current, &FixtureCodec).expect("write");
        drop(store);

        let restarted =
            NativeForecastSchedulerStateStore::initialize(root.0.clone()).expect("restart");
        assert_eq!(restarted.read(&FixtureCodec), Ok(current.clone()));
        assert_eq!(
            fs::read(slot(&root, CURRENT_STATE_NAME)).expect("current bytes"),
            FixtureCodec
                .encode_state(&current)
                .expect("encoded current")
        );
        assert!(!slot(&root, PREVIOUS_STATE_NAME).exists());
    }

    // verify current rotation and exact previous generation retention
    #[test]
    fn rotates_only_one_valid_current_generation() {
        let root = DisposableRoot::new();
        let mut store =
            NativeForecastSchedulerStateStore::initialize(root.0.clone()).expect("store");
        let first = state("first", "00");
        let second = state("second", "06");
        store.write(&first, &FixtureCodec).expect("first write");
        store.write(&second, &FixtureCodec).expect("second write");

        assert_eq!(store.read(&FixtureCodec), Ok(second.clone()));
        assert_eq!(
            fs::read(slot(&root, CURRENT_STATE_NAME)).expect("current"),
            FixtureCodec.encode_state(&second).expect("second bytes")
        );
        assert_eq!(
            fs::read(slot(&root, PREVIOUS_STATE_NAME)).expect("previous"),
            FixtureCodec.encode_state(&first).expect("first bytes")
        );
    }

    // verify an unavailable previous generation does not stop a valid current write
    #[test]
    fn writes_current_when_previous_rotation_refuses() {
        let root = DisposableRoot::new();
        let mut store =
            NativeForecastSchedulerStateStore::initialize(root.0.clone()).expect("store");
        let first = state("first", "00");
        let second = state("second", "06");
        store.write(&first, &FixtureCodec).expect("first write");
        let mut builder = DirBuilder::new();
        builder
            .mode(0o700)
            .create(slot(&root, PREVIOUS_STATE_NAME))
            .expect("previous slot fixture");
        store.write(&second, &FixtureCodec).expect("second write");
        assert!(slot(&root, PREVIOUS_STATE_NAME).is_dir());
        assert_eq!(store.read(&FixtureCodec), Ok(second));
    }

    // verify invalid current falls back without resetting retained history
    #[test]
    fn falls_back_to_previous_and_never_resets_known_invalid_history() {
        let root = DisposableRoot::new();
        let mut store =
            NativeForecastSchedulerStateStore::initialize(root.0.clone()).expect("store");
        let first = state("first", "00");
        let second = state("second", "06");
        store.write(&first, &FixtureCodec).expect("first write");
        store.write(&second, &FixtureCodec).expect("second write");
        replace_private(&slot(&root, CURRENT_STATE_NAME), b"invalid current\n");

        assert_eq!(store.read(&FixtureCodec), Ok(first));
        replace_private(&slot(&root, PREVIOUS_STATE_NAME), b"invalid previous\n");
        assert_eq!(
            store.read(&FixtureCodec),
            Err(ForecastSchedulerStateStoreError::InvalidState)
        );
    }

    // verify invalid current is never rotated over a complete previous slot
    #[test]
    fn preserves_previous_when_replacing_invalid_current() {
        let root = DisposableRoot::new();
        let mut store =
            NativeForecastSchedulerStateStore::initialize(root.0.clone()).expect("store");
        let first = state("first", "00");
        let second = state("second", "06");
        let third = state("third", "12");
        store.write(&first, &FixtureCodec).expect("first write");
        store.write(&second, &FixtureCodec).expect("second write");
        let previous = fs::read(slot(&root, PREVIOUS_STATE_NAME)).expect("previous");
        replace_private(&slot(&root, CURRENT_STATE_NAME), b"invalid current\n");
        store.write(&third, &FixtureCodec).expect("third write");

        assert_eq!(store.read(&FixtureCodec), Ok(third));
        assert_eq!(
            fs::read(slot(&root, PREVIOUS_STATE_NAME)).expect("preserved previous"),
            previous
        );
    }

    // verify read and write both preserve the one-mebibyte slot ceiling
    #[test]
    fn preserves_maximum_bytes_without_mutating_existing_slots() {
        let root = DisposableRoot::new();
        let mut store =
            NativeForecastSchedulerStateStore::initialize(root.0.clone()).expect("store");
        assert_eq!(
            store.write(&ForecastSchedulerState::empty(), &OversizedCodec),
            Err(ForecastSchedulerStateStoreError::StateTooLarge)
        );
        assert!(!slot(&root, CURRENT_STATE_NAME).exists());

        let oversized = vec![b'x'; FORECAST_SCHEDULER_STATE_MAXIMUM_BYTES + 1];
        create_private(&slot(&root, CURRENT_STATE_NAME), &oversized);
        assert_eq!(
            store.read(&FixtureCodec),
            Err(ForecastSchedulerStateStoreError::InvalidState)
        );
        assert_eq!(
            store.write(&state("replacement", "18"), &FixtureCodec),
            Err(ForecastSchedulerStateStoreError::Storage)
        );
        assert_eq!(
            fs::metadata(slot(&root, CURRENT_STATE_NAME))
                .expect("oversized current")
                .len(),
            oversized.len() as u64
        );
    }
}
