use std::cell::RefCell;
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::fs::{self, DirBuilder, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};

use weather_edge::{
    AdmittedForecastAdjustmentScorecardJson, AdmittedForecastAdjustmentScorecardNow,
    AdmittedForecastAdjustmentScorecardSnapshot, FileForecastAdjustmentScorecardStore,
    ForecastAdjustmentScorecardContractVersion, ForecastAdjustmentScorecardFileAdapter,
    ForecastAdjustmentScorecardStore,
};

const LEGACY_DIGEST: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const V2_DIGEST: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const PREVIOUS_DIGEST: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const CHANGED_DIGEST: &str = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
const LEGACY_BYTES: &[u8] =
    b"{\"contractVersion\":\"forecast-adjustment-scorecard/v1\",\"fixture\":\"legacy\"}\n";
const V2_BYTES: &[u8] =
    b"{\"contractVersion\":\"forecast-adjustment-scorecard/v2\",\"fixture\":\"v2\"}\n";
const CHANGED_BYTES: &[u8] =
    b"{\"contractVersion\":\"forecast-adjustment-scorecard/v1\",\"fixture\":\"changed\"}\n";
const NOW: &str = "2026-10-07T01:30:00.000Z";
const NOW_EPOCH_MS: i64 = 1_791_336_600_000;
const GENERATED_AT_EPOCH_MS: i64 = 1_791_334_800_000;
const VALID_THROUGH_EPOCH_MS: i64 = 4_095_104_400_000;

static ROOT_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FixtureAdapterError;

impl Display for FixtureAdapterError {
    /// render one fixture admission failure.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("fixture scorecard admission failed")
    }
}

impl Error for FixtureAdapterError {}

#[derive(Debug, Clone)]
struct FixtureAdapter {
    trace: Rc<RefCell<Vec<&'static str>>>,
}

impl FixtureAdapter {
    /// create one injected canonical boundary.
    fn new(trace: Rc<RefCell<Vec<&'static str>>>) -> Self {
        Self { trace }
    }
}

impl ForecastAdjustmentScorecardFileAdapter for FixtureAdapter {
    type Error = FixtureAdapterError;

    /// decode only the three admitted fixture pointer documents.
    fn decode_pointer(&self, content: &[u8]) -> Result<String, Self::Error> {
        self.trace.borrow_mut().push("pointer");
        // match exact injected documents
        if content == pointer_bytes(LEGACY_DIGEST).as_bytes() {
            return Ok(LEGACY_DIGEST.to_owned());
        }
        // match exact injected documents
        if content == pointer_bytes(V2_DIGEST).as_bytes() {
            return Ok(V2_DIGEST.to_owned());
        }
        // match exact injected documents
        if content == pointer_bytes(PREVIOUS_DIGEST).as_bytes() {
            return Ok(PREVIOUS_DIGEST.to_owned());
        }
        Err(FixtureAdapterError)
    }

    /// return the injected digest for exact fixture bytes.
    fn sha256_lower_hex(&self, content: &[u8]) -> Result<String, Self::Error> {
        self.trace.borrow_mut().push("hash");
        // bind exact legacy bytes
        if content == LEGACY_BYTES {
            return Ok(LEGACY_DIGEST.to_owned());
        }
        // bind exact v2 bytes
        if content == V2_BYTES {
            return Ok(V2_DIGEST.to_owned());
        }
        // expose changed bytes as another digest
        if content == CHANGED_BYTES {
            return Ok(CHANGED_DIGEST.to_owned());
        }
        Err(FixtureAdapterError)
    }

    /// admit one fixture scorecard after checking generation and clock.
    fn decode_scorecard(
        &self,
        expected_version: ForecastAdjustmentScorecardContractVersion,
        content: &[u8],
        now: &AdmittedForecastAdjustmentScorecardNow,
    ) -> Result<AdmittedForecastAdjustmentScorecardSnapshot, Self::Error> {
        self.trace.borrow_mut().push("scorecard");
        // bind the captured clock
        if now.canonical != NOW || now.epoch_ms != NOW_EPOCH_MS {
            return Err(FixtureAdapterError);
        }
        let expected: &[u8] = match expected_version {
            ForecastAdjustmentScorecardContractVersion::V1 => LEGACY_BYTES,
            ForecastAdjustmentScorecardContractVersion::V2 => V2_BYTES,
        };
        // bind content to its requested generation
        if content != expected {
            return Err(FixtureAdapterError);
        }
        Ok(AdmittedForecastAdjustmentScorecardSnapshot::admitted(
            expected_version,
            GENERATED_AT_EPOCH_MS,
            AdmittedForecastAdjustmentScorecardJson::admitted(content.to_vec()),
            VALID_THROUGH_EPOCH_MS,
        ))
    }
}

#[derive(Debug)]
struct TemporaryRoot(PathBuf);

impl TemporaryRoot {
    /// create one disposable private evidence root.
    fn new() -> Self {
        let sequence = ROOT_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "weather-edge-scorecard-file-{}-{sequence}",
            std::process::id()
        ));
        let _cleanup = fs::remove_dir_all(&path);
        create_private_directory(&path);
        Self(path)
    }

    /// create one direct private generation directory.
    fn generation(&self, name: &str) -> PathBuf {
        let path = self.0.join(name);
        create_private_directory(&path);
        path
    }
}

impl Drop for TemporaryRoot {
    /// remove every disposable fixture member.
    fn drop(&mut self) {
        let _cleanup = fs::remove_dir_all(&self.0);
    }
}

/// create one exact private directory.
fn create_private_directory(path: &Path) {
    let mut builder = DirBuilder::new();
    builder
        .mode(0o700)
        .create(path)
        .expect("create private root");
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
        .expect("set private directory mode");
}

/// write one exact private fixture file.
fn write_private(path: &Path, content: &[u8]) {
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
        .expect("open private fixture");
    file.write_all(content).expect("write private fixture");
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).expect("set private file mode");
}

/// encode one pointer through the injected fixture boundary.
fn pointer_bytes(digest: &str) -> String {
    format!("{{\"sha256\":\"{digest}\"}}\n")
}

/// return the one fixed admitted read clock.
fn now() -> AdmittedForecastAdjustmentScorecardNow {
    AdmittedForecastAdjustmentScorecardNow {
        canonical: NOW.to_owned(),
        epoch_ms: NOW_EPOCH_MS,
    }
}

/// create one store sharing an observable adapter trace.
fn store(
    root: &TemporaryRoot,
) -> (
    FileForecastAdjustmentScorecardStore<FixtureAdapter>,
    Rc<RefCell<Vec<&'static str>>>,
) {
    let trace = Rc::new(RefCell::new(Vec::new()));
    (
        FileForecastAdjustmentScorecardStore::new(
            root.0.clone(),
            FixtureAdapter::new(Rc::clone(&trace)),
        ),
        trace,
    )
}

/// read one legacy publication from actual private files.
#[test]
fn reads_legacy_pointer_and_exact_hash_addressed_bytes() {
    let root = TemporaryRoot::new();
    let legacy = root.generation("scorecards");
    write_private(
        &legacy.join(format!("sha256-{LEGACY_DIGEST}.json")),
        LEGACY_BYTES,
    );
    write_private(
        &root.0.join("current.json"),
        pointer_bytes(LEGACY_DIGEST).as_bytes(),
    );
    let (store, trace) = store(&root);

    let candidates = store
        .read_candidates(&now())
        .expect("read legacy scorecard from actual files");

    let selected = candidates.legacy.expect("legacy candidate");
    assert_eq!(
        selected.contract_version(),
        ForecastAdjustmentScorecardContractVersion::V1
    );
    assert_eq!(selected.scorecard().as_bytes(), LEGACY_BYTES);
    assert!(candidates.current_v2.is_none());
    assert!(candidates.pending_v2.is_none());
    assert!(!candidates.previous_v2_present);
    assert_eq!(&*trace.borrow(), &["pointer", "hash", "scorecard"]);
}

/// keep pending and current selection independent of unselected object files.
#[test]
fn pending_then_current_reads_only_the_selected_v2_object() {
    let root = TemporaryRoot::new();
    root.generation("scorecards");
    let v2 = root.generation("scorecards-v2");
    write_private(&v2.join(format!("sha256-{V2_DIGEST}.json")), V2_BYTES);
    write_private(
        &root.0.join("v2-current.json"),
        pointer_bytes(V2_DIGEST).as_bytes(),
    );
    write_private(
        &root.0.join("v2-previous.json"),
        pointer_bytes(PREVIOUS_DIGEST).as_bytes(),
    );
    write_private(
        &root.0.join("v2-pending.json"),
        pointer_bytes(V2_DIGEST).as_bytes(),
    );
    let (store, trace) = store(&root);

    let pending = store
        .read_candidates(&now())
        .expect("pending does not read missing previous object");
    let selected = pending.pending_v2.expect("pending candidate");
    assert_eq!(
        selected.contract_version(),
        ForecastAdjustmentScorecardContractVersion::V2
    );
    assert_eq!(selected.scorecard().as_bytes(), V2_BYTES);
    assert!(pending.current_v2.is_none());
    assert!(pending.legacy.is_none());
    assert!(pending.previous_v2_present);
    assert_eq!(
        &*trace.borrow(),
        &["pointer", "pointer", "pointer", "hash", "scorecard"]
    );

    fs::remove_file(root.0.join("v2-pending.json")).expect("remove pending pointer");
    trace.borrow_mut().clear();
    let current = store
        .read_candidates(&now())
        .expect("current does not read missing previous object");
    let selected = current.current_v2.expect("current candidate");
    assert_eq!(selected.scorecard().as_bytes(), V2_BYTES);
    assert!(current.pending_v2.is_none());
    assert!(current.previous_v2_present);
    assert_eq!(
        &*trace.borrow(),
        &["pointer", "pointer", "hash", "scorecard"]
    );
}

/// fail before decode when selected bytes are absent or changed.
#[test]
fn selected_object_absence_and_digest_drift_fail_closed() {
    let root = TemporaryRoot::new();
    let legacy = root.generation("scorecards");
    write_private(
        &root.0.join("current.json"),
        pointer_bytes(LEGACY_DIGEST).as_bytes(),
    );
    let (store, trace) = store(&root);

    assert!(store.read_candidates(&now()).is_err());
    assert_eq!(&*trace.borrow(), &["pointer"]);

    trace.borrow_mut().clear();
    write_private(
        &legacy.join(format!("sha256-{LEGACY_DIGEST}.json")),
        CHANGED_BYTES,
    );
    let error = store
        .read_candidates(&now())
        .expect_err("changed bytes must not satisfy the pointer");
    assert_eq!(error.to_string(), "scorecard content hash is invalid");
    assert_eq!(&*trace.borrow(), &["pointer", "hash"]);
}
