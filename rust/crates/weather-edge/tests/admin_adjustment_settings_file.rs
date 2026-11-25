use std::cell::RefCell;
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::fs::{self, DirBuilder, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};

use weather_edge::{
    AdmittedForecastAdjustmentSettingsInitialization, AdmittedForecastAdjustmentSettingsJson,
    FileForecastAdjustmentSettingsStore, ForecastAdjustmentSettings,
    ForecastAdjustmentSettingsInitializationEvidence,
    ForecastAdjustmentSettingsInitializationEvidenceReader, ForecastAdjustmentSettingsJsonAdapter,
    ForecastAdjustmentSettingsStatus, ForecastAdjustmentSettingsStore,
    ForecastAdjustmentSettingsTemporaryPathSource,
};

static TEMPORARY_ROOT_SEQUENCE: AtomicU64 = AtomicU64::new(0);
const DEFAULT_JSON: &[u8] = b"{\"version\":1,\"temperature\":true,\"wind\":true,\"rain\":true}\n";
type FixtureFileStore = FileForecastAdjustmentSettingsStore<
    FixtureJsonAdapter,
    FixtureTemporaryPaths,
    FixtureInitializationEvidenceReader,
>;
type TemporaryTargetTrace = Rc<RefCell<Vec<String>>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FixtureJsonError;

impl Display for FixtureJsonError {
    /// render one admitted fixture decode failure.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("invalid settings json")
    }
}

impl Error for FixtureJsonError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FixtureJsonAdapter;

impl ForecastAdjustmentSettingsJsonAdapter for FixtureJsonAdapter {
    type Error = FixtureJsonError;

    /// decode only canonical fixture documents without implementing JSON.
    fn decode(&mut self, content: &[u8]) -> Result<ForecastAdjustmentSettings, Self::Error> {
        let settings = [
            ForecastAdjustmentSettings {
                version: 1,
                temperature: false,
                wind: false,
                rain: false,
            },
            ForecastAdjustmentSettings {
                version: 1,
                temperature: false,
                wind: false,
                rain: true,
            },
            ForecastAdjustmentSettings {
                version: 1,
                temperature: false,
                wind: true,
                rain: false,
            },
            ForecastAdjustmentSettings {
                version: 1,
                temperature: false,
                wind: true,
                rain: true,
            },
            ForecastAdjustmentSettings {
                version: 1,
                temperature: true,
                wind: false,
                rain: false,
            },
            ForecastAdjustmentSettings {
                version: 1,
                temperature: true,
                wind: false,
                rain: true,
            },
            ForecastAdjustmentSettings {
                version: 1,
                temperature: true,
                wind: true,
                rain: false,
            },
            ForecastAdjustmentSettings {
                version: 1,
                temperature: true,
                wind: true,
                rain: true,
            },
        ];

        // retain only the eight admitted independent switch documents
        for settings in settings {
            // match the canonical adapter output byte-for-byte
            if content == fixture_json(settings).as_bytes() {
                return Ok(settings);
            }
        }

        Err(FixtureJsonError)
    }

    /// encode through the admitted fixed fixture boundary.
    fn encode(
        &mut self,
        settings: ForecastAdjustmentSettings,
    ) -> Result<AdmittedForecastAdjustmentSettingsJson, Self::Error> {
        Ok(AdmittedForecastAdjustmentSettingsJson::admitted(
            fixture_json(settings),
        ))
    }
}

#[derive(Debug, Clone)]
struct FixtureTemporaryPaths {
    next: u64,
    root: PathBuf,
    targets: Rc<RefCell<Vec<String>>>,
}

impl ForecastAdjustmentSettingsTemporaryPathSource for FixtureTemporaryPaths {
    /// return one bounded same-directory fixture temporary path.
    fn next_temporary_path(&mut self, target: &Path) -> PathBuf {
        self.next += 1;
        self.targets.borrow_mut().push(
            target
                .strip_prefix(&self.root)
                .expect("target below fixture root")
                .display()
                .to_string(),
        );
        PathBuf::from(format!("{}.fixture-{}.tmp", target.display(), self.next))
    }
}

#[derive(Debug, Clone)]
struct FixtureInitializationEvidenceReader {
    evidence: Rc<RefCell<ForecastAdjustmentSettingsInitializationEvidence>>,
}

impl ForecastAdjustmentSettingsInitializationEvidenceReader
    for FixtureInitializationEvidenceReader
{
    /// read current admitted configuration evidence from the fixture boundary.
    fn read_initialization_evidence(&mut self) -> ForecastAdjustmentSettingsInitializationEvidence {
        *self.evidence.borrow()
    }
}

#[derive(Debug)]
struct TemporaryRoot(PathBuf);

impl TemporaryRoot {
    /// allocate one bounded disposable fixture root without a dependency.
    fn new() -> Self {
        let sequence = TEMPORARY_ROOT_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "weather-edge-settings-rust-{}-{sequence}",
            std::process::id()
        ));
        let _cleanup = fs::remove_dir_all(&path);
        Self(path)
    }
}

impl Drop for TemporaryRoot {
    /// remove every bounded fixture file after the test.
    fn drop(&mut self) {
        let _cleanup = fs::remove_dir_all(&self.0);
    }
}

/// encode one fixture through its admitted adapter output.
fn fixture_json(settings: ForecastAdjustmentSettings) -> String {
    format!(
        "{{\"version\":1,\"temperature\":{},\"wind\":{},\"rain\":{}}}\n",
        settings.temperature, settings.wind, settings.rain
    )
}

/// render one typed status like the compiled Node oracle.
fn status_signature(status: ForecastAdjustmentSettingsStatus) -> String {
    format!(
        "{}:{}{}{}:{}",
        status.settings.version,
        u8::from(status.settings.temperature),
        u8::from(status.settings.wind),
        u8::from(status.settings.rain),
        status.error.map(|error| error.wire_value()).unwrap_or("-")
    )
}

/// read one ordinary permission mode.
fn mode(path: &Path) -> String {
    format!(
        "{:o}",
        fs::metadata(path)
            .expect("fixture metadata")
            .permissions()
            .mode()
            & 0o777
    )
}

/// write one private fixture file outside the store under test.
fn write_private(path: &Path, content: &[u8]) {
    let parent = path.parent().expect("fixture parent");
    let mut builder = DirBuilder::new();
    builder.recursive(true).mode(0o700);
    builder.create(parent).expect("create fixture parent");
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
        .expect("open fixture file");
    file.write_all(content).expect("write fixture file");
}

/// create one real file store and its observable temporary-target trace.
fn file_store(
    root: &TemporaryRoot,
    settings_path: PathBuf,
) -> (FixtureFileStore, TemporaryTargetTrace) {
    let targets = Rc::new(RefCell::new(Vec::new()));
    let temporary_paths = FixtureTemporaryPaths {
        next: 0,
        root: root.0.clone(),
        targets: Rc::clone(&targets),
    };
    let initialization_evidence = FixtureInitializationEvidenceReader {
        evidence: Rc::new(RefCell::new(
            ForecastAdjustmentSettingsInitializationEvidence::Admitted(
                AdmittedForecastAdjustmentSettingsInitialization::admitted(),
            ),
        )),
    };
    (
        FileForecastAdjustmentSettingsStore::new(
            settings_path,
            AdmittedForecastAdjustmentSettingsJson::admitted(DEFAULT_JSON.to_vec()),
            initialization_evidence,
            FixtureJsonAdapter,
            temporary_paths,
        ),
        targets,
    )
}

// verifies actual Node first-init, replacement, and marker repair file parity
#[test]
fn matches_compiled_node_settings_file_lifecycle() {
    let oracle =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/admin-settings-file-node-oracle.mjs");
    let output = Command::new("node")
        .arg(oracle)
        .output()
        .expect("run settings file Node oracle");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let node = String::from_utf8(output.stdout).expect("Node oracle UTF-8");
    let root = TemporaryRoot::new();
    let live_path = root
        .0
        .join("live")
        .join("forecast-adjustment-settings.json");
    let live_targets = Rc::new(RefCell::new(Vec::new()));
    let live_evidence = Rc::new(RefCell::new(
        ForecastAdjustmentSettingsInitializationEvidence::Missing,
    ));
    let mut live_store = FileForecastAdjustmentSettingsStore::new(
        live_path.clone(),
        AdmittedForecastAdjustmentSettingsJson::admitted(DEFAULT_JSON.to_vec()),
        FixtureInitializationEvidenceReader {
            evidence: Rc::clone(&live_evidence),
        },
        FixtureJsonAdapter,
        FixtureTemporaryPaths {
            next: 0,
            root: root.0.clone(),
            targets: Rc::clone(&live_targets),
        },
    );
    let live_missing = live_store.read_status();
    *live_evidence.borrow_mut() = ForecastAdjustmentSettingsInitializationEvidence::Invalid;
    let live_invalid = live_store.read_status();
    let live_settings_before = live_path.exists();
    let live_marker_before = live_store.marker_path().exists();
    let live_targets_before = live_targets.borrow().len();
    *live_evidence.borrow_mut() = ForecastAdjustmentSettingsInitializationEvidence::Admitted(
        AdmittedForecastAdjustmentSettingsInitialization::admitted(),
    );
    let live_admitted = live_store.read_status();
    let settings_path = root
        .0
        .join("state")
        .join("forecast-adjustment-settings.json");
    let (mut store, targets) = file_store(&root, settings_path.clone());
    let marker_path = store.marker_path().to_path_buf();
    let fresh = store.read_status();
    let fresh_modes = format!(
        "{}/{}/{}",
        mode(settings_path.parent().expect("settings parent")),
        mode(&settings_path),
        mode(&marker_path)
    );
    let settings_100 = ForecastAdjustmentSettings {
        version: 1,
        temperature: true,
        wind: false,
        rain: false,
    };
    let written = store
        .replace_atomically(settings_100)
        .expect("write settings");
    let after_write = store.read_status();
    write_private(&settings_path, b"{broken json\n");
    let corrupt = store.read_status();
    targets.borrow_mut().clear();
    let settings_001 = ForecastAdjustmentSettings {
        version: 1,
        temperature: false,
        wind: false,
        rain: true,
    };
    store
        .replace_atomically(settings_001)
        .expect("repair settings");
    let repaired = store.read_status();
    let repair_order = targets.borrow().join(",");
    write_private(&marker_path, b"damaged marker\n");
    let damaged_marker = store.read_status();
    targets.borrow_mut().clear();
    let settings_110 = ForecastAdjustmentSettings {
        version: 1,
        temperature: true,
        wind: true,
        rain: false,
    };
    store
        .replace_atomically(settings_110)
        .expect("repair marker");
    let marker_repaired = store.read_status();
    let marker_repair_order = targets.borrow().join(",");
    fs::remove_file(&marker_path).expect("remove marker");
    targets.borrow_mut().clear();
    let settings_010 = ForecastAdjustmentSettings {
        version: 1,
        temperature: false,
        wind: true,
        rain: false,
    };
    store
        .replace_atomically(settings_010)
        .expect("repair missing marker");
    let missing_marker_repaired = store.read_status();
    let missing_marker_repair_order = targets.borrow().join(",");

    let existing_path = root
        .0
        .join("existing")
        .join("forecast-adjustment-settings.json");
    let settings_101 = ForecastAdjustmentSettings {
        version: 1,
        temperature: true,
        wind: false,
        rain: true,
    };
    write_private(&existing_path, fixture_json(settings_101).as_bytes());
    let (mut existing_store, _existing_targets) = file_store(&root, existing_path.clone());
    let preserved_existing = existing_store.read_status();
    let existing_modes = format!(
        "{}/{}/{}",
        mode(existing_path.parent().expect("existing parent")),
        mode(&existing_path),
        mode(existing_store.marker_path())
    );
    let rust = [
        format!(
            "live-init={}:{}:{}:{live_settings_before}/{live_marker_before}/{live_targets_before}",
            status_signature(live_missing),
            status_signature(live_invalid),
            status_signature(live_admitted)
        ),
        format!("fresh={}:{}", status_signature(fresh), fresh_modes),
        format!(
            "write={}:{}",
            status_signature(ForecastAdjustmentSettingsStatus {
                settings: written,
                error: None,
            }),
            status_signature(after_write)
        ),
        format!("corrupt={}", status_signature(corrupt)),
        format!("repair={}:{}", status_signature(repaired), repair_order),
        format!("damaged-marker={}", status_signature(damaged_marker)),
        format!(
            "marker-repair={}:{}",
            status_signature(marker_repaired),
            marker_repair_order
        ),
        format!(
            "missing-marker-repair={}:{}",
            status_signature(missing_marker_repaired),
            missing_marker_repair_order
        ),
        format!(
            "preserved-existing={}:{}",
            status_signature(preserved_existing),
            existing_modes
        ),
    ]
    .join("\n");
    assert_eq!(rust, node.trim_end());
}
