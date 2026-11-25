use std::error::Error;
use std::fmt::{Display, Formatter};
use std::fs::{self, DirBuilder, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use weather_edge::{
    AdmittedAdminMutation, AdmittedAdminOperator, AdmittedPropertySensorLayoutJson,
    AdmittedPropertySensorUpdateInstant, DecodedPropertySensorLayoutUpdate,
    FilePropertySensorLayoutStore, PropertySensorIcon, PropertySensorLayoutEntry,
    PropertySensorLayoutJsonAdapter, PropertySensorLayoutOutcome, PropertySensorLayoutRequest,
    PropertySensorLayoutStore, PropertySensorLayoutTemporaryPathSource,
    PropertySensorLayoutUpdateInstantSource, PropertySensorMapCenter,
    handle_property_sensor_layout,
};

static TEMPORARY_ROOT_SEQUENCE: AtomicU64 = AtomicU64::new(0);
const CENTER: PropertySensorMapCenter = PropertySensorMapCenter {
    latitude: 47.95,
    longitude: -122.43,
};
const UPDATED_AT: &str = "2026-11-23T12:34:56.000Z";
const INITIAL_JSON: &[u8] = b"{\"sensors\":[{\"displayName\":\"Wind mast\",\"icon\":\"wind\",\"latitude\":47.951,\"longitude\":-122.431,\"sensorKey\":\"wind-1\",\"updatedAt\":\"2026-11-22T10:00:00.000Z\"},{\"displayName\":\"Old soil\",\"icon\":\"rain\",\"latitude\":47.9504,\"longitude\":-122.4282,\"sensorKey\":\"soil-1\",\"updatedAt\":\"2026-11-22T11:00:00.000Z\"},{\"displayName\":\"Legacy air\",\"latitude\":47.949,\"longitude\":-122.429,\"sensorKey\":\"air-quality-1\",\"updatedAt\":\"2026-11-22T12:00:00.000Z\"}],\"version\":1}\n";
const WRITTEN_JSON: &[u8] = b"{\"sensors\":[{\"displayName\":\"Legacy air\",\"icon\":null,\"latitude\":47.949,\"longitude\":-122.429,\"sensorKey\":\"air-quality-1\",\"updatedAt\":\"2026-11-22T12:00:00.000Z\"},{\"displayName\":\"Orchard soil\",\"icon\":\"temperature\",\"latitude\":47.9505,\"longitude\":-122.4281,\"sensorKey\":\"soil-1\",\"updatedAt\":\"2026-11-23T12:34:56.000Z\"},{\"displayName\":\"Wind mast\",\"icon\":\"wind\",\"latitude\":47.951,\"longitude\":-122.431,\"sensorKey\":\"wind-1\",\"updatedAt\":\"2026-11-22T10:00:00.000Z\"}],\"version\":1}\n";
const INVALID_ENVELOPE_JSON: &[u8] = b"{\"sensors\":[],\"version\":2}\n";
const INVALID_COORDINATE_JSON: &[u8] = b"{\"sensors\":[{\"displayName\":\"Bad\",\"icon\":\"wind\",\"latitude\":90,\"longitude\":0,\"sensorKey\":\"bad-1\",\"updatedAt\":\"2026-11-22T10:00:00.000Z\"}],\"version\":1}\n";

type FixtureFileStore = FilePropertySensorLayoutStore<FixtureJsonAdapter, FixtureTemporaryPaths>;

#[derive(Debug, Clone, PartialEq, Eq)]
struct FixtureJsonError(&'static str);

impl Display for FixtureJsonError {
    /// render the exact source-derived canonical adapter failure.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.0)
    }
}

impl Error for FixtureJsonError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FixtureJsonAdapter;

impl PropertySensorLayoutJsonAdapter for FixtureJsonAdapter {
    type Error = FixtureJsonError;

    /// decode only exact source-derived documents without implementing JSON.
    fn decode(&mut self, content: &[u8]) -> Result<Vec<PropertySensorLayoutEntry>, Self::Error> {
        // admit the canonical initial document
        if content == INITIAL_JSON {
            return Ok(initial_layout());
        }

        // admit the canonical replacement document
        if content == WRITTEN_JSON {
            return Ok(replaced_layout());
        }

        // retain the source parser's bounded public prefix
        if content == bounded_json().as_bytes() {
            return Ok((0..64).map(bounded_entry).collect());
        }

        // preserve the source envelope error category
        if content == INVALID_ENVELOPE_JSON {
            return Err(FixtureJsonError("property sensor layout is invalid"));
        }

        // preserve the source coordinate error category
        if content == INVALID_COORDINATE_JSON {
            return Err(FixtureJsonError("sensor position coordinates are invalid"));
        }

        Err(FixtureJsonError("property sensor layout is invalid"))
    }

    /// encode only the exact complete validated replacement fixture.
    fn encode(
        &mut self,
        layout: &[PropertySensorLayoutEntry],
    ) -> Result<AdmittedPropertySensorLayoutJson, Self::Error> {
        // reject any state not admitted by this focused canonical fixture
        if layout != replaced_layout() {
            return Err(FixtureJsonError("unexpected layout fixture"));
        }

        Ok(AdmittedPropertySensorLayoutJson::admitted(WRITTEN_JSON))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FixtureTemporaryPaths {
    next: u64,
}

impl PropertySensorLayoutTemporaryPathSource for FixtureTemporaryPaths {
    /// return one bounded unique same-directory fixture temporary path.
    fn next_temporary_path(&mut self, target: &Path) -> PathBuf {
        self.next += 1;
        PathBuf::from(format!("{}.fixture-{}.tmp", target.display(), self.next))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FixtureUpdateInstants;

impl PropertySensorLayoutUpdateInstantSource for FixtureUpdateInstants {
    type Error = FixtureJsonError;

    /// return one canonical source-derived update instant.
    fn admitted_update_instant(
        &mut self,
    ) -> Result<AdmittedPropertySensorUpdateInstant, Self::Error> {
        Ok(AdmittedPropertySensorUpdateInstant::admitted(UPDATED_AT))
    }
}

#[derive(Debug)]
struct TemporaryRoot(PathBuf);

impl TemporaryRoot {
    /// allocate one bounded disposable fixture root without a dependency.
    fn new() -> Self {
        let sequence = TEMPORARY_ROOT_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "weather-edge-layout-rust-{}-{sequence}",
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

/// build one admitted persisted placement.
fn entry(
    sensor_key: &str,
    display_name: &str,
    icon: Option<PropertySensorIcon>,
    latitude: f64,
    longitude: f64,
    updated_at: &str,
) -> PropertySensorLayoutEntry {
    PropertySensorLayoutEntry {
        display_name: display_name.to_owned(),
        icon,
        latitude,
        longitude,
        sensor_key: sensor_key.to_owned(),
        updated_at: AdmittedPropertySensorUpdateInstant::admitted(updated_at),
    }
}

/// build the exact source-derived initial layout order.
fn initial_layout() -> Vec<PropertySensorLayoutEntry> {
    vec![
        entry(
            "wind-1",
            "Wind mast",
            Some(PropertySensorIcon::Wind),
            47.951,
            -122.431,
            "2026-11-22T10:00:00.000Z",
        ),
        entry(
            "soil-1",
            "Old soil",
            Some(PropertySensorIcon::Rain),
            47.9504,
            -122.4282,
            "2026-11-22T11:00:00.000Z",
        ),
        entry(
            "air-quality-1",
            "Legacy air",
            None,
            47.949,
            -122.429,
            "2026-11-22T12:00:00.000Z",
        ),
    ]
}

/// build the complete deterministic replacement layout.
fn replaced_layout() -> Vec<PropertySensorLayoutEntry> {
    vec![
        entry(
            "air-quality-1",
            "Legacy air",
            None,
            47.949,
            -122.429,
            "2026-11-22T12:00:00.000Z",
        ),
        entry(
            "soil-1",
            "Orchard soil",
            Some(PropertySensorIcon::Temperature),
            47.9505,
            -122.4281,
            UPDATED_AT,
        ),
        entry(
            "wind-1",
            "Wind mast",
            Some(PropertySensorIcon::Wind),
            47.951,
            -122.431,
            "2026-11-22T10:00:00.000Z",
        ),
    ]
}

/// build one source-derived bounded fixture placement.
fn bounded_entry(index: usize) -> PropertySensorLayoutEntry {
    let icon = if index.is_multiple_of(2) {
        PropertySensorIcon::Temperature
    } else {
        PropertySensorIcon::Wind
    };
    entry(
        &format!("sensor-{index:02}"),
        &format!("Sensor {index}"),
        Some(icon),
        47.95,
        -122.43,
        "2026-11-22T10:00:00.000Z",
    )
}

/// build the exact oversized source fixture document.
fn bounded_json() -> String {
    let mut sensors = Vec::with_capacity(66);

    // retain the source fixture's exact property order and values
    for index in 0_usize..66 {
        let icon = if index.is_multiple_of(2) {
            "temperature"
        } else {
            "wind"
        };
        sensors.push(format!(
            "{{\"displayName\":\"Sensor {index}\",\"icon\":\"{icon}\",\"latitude\":47.95,\"longitude\":-122.43,\"sensorKey\":\"sensor-{index:02}\",\"updatedAt\":\"2026-11-22T10:00:00.000Z\"}}"
        ));
    }

    format!("{{\"sensors\":[{}],\"version\":1}}\n", sensors.join(","))
}

/// render one placement like the compiled Node oracle.
fn entry_signature(entry: &PropertySensorLayoutEntry) -> String {
    format!(
        "{}~{}~{}~{}~{}~{}",
        entry.sensor_key,
        entry.display_name,
        entry
            .icon
            .map(PropertySensorIcon::wire_value)
            .unwrap_or("null"),
        entry.latitude,
        entry.longitude,
        entry.updated_at.as_str()
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

/// count temporary files left beside one fixture target.
fn temporary_file_count(parent: &Path) -> usize {
    fs::read_dir(parent)
        .expect("read fixture directory")
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().ends_with(".tmp"))
        .count()
}

// verifies actual Node optional read, canonical bounds, and atomic file replacement parity
#[test]
fn matches_compiled_node_layout_file_lifecycle() {
    let oracle = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/property-sensor-layout-file-node-oracle.mjs");
    let output = Command::new("node")
        .arg(oracle)
        .output()
        .expect("run property sensor layout file Node oracle");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let node = String::from_utf8(output.stdout).expect("Node oracle UTF-8");
    let root = TemporaryRoot::new();
    let layout_path = root.0.join("state/property-sensor-layout.json");
    let mut store = FixtureFileStore::new(
        layout_path.clone(),
        FixtureJsonAdapter,
        FixtureTemporaryPaths { next: 0 },
    );
    assert_eq!(store.layout_path(), layout_path);
    let missing = store.read_layout().expect("read missing layout");
    let missing_exists = layout_path.exists();
    write_private(&layout_path, INITIAL_JSON);
    let before = store.read_layout().expect("read initial layout");
    let operator = AdmittedAdminOperator::admitted("verified-operator");
    let mutation = AdmittedAdminMutation::admitted(&operator);
    let mut update_instants = FixtureUpdateInstants;
    let outcome = handle_property_sensor_layout(
        CENTER,
        PropertySensorLayoutRequest::AdminReplace {
            mutation,
            sensor_key: "soil-1".to_owned(),
            input: DecodedPropertySensorLayoutUpdate {
                display_name: Some("  Orchard soil  ".to_owned()),
                icon: Some("temperature".to_owned()),
                latitude: Some(47.9505),
                longitude: Some(-122.4281),
            },
        },
        &mut store,
        &mut update_instants,
    );
    let saved = match outcome {
        PropertySensorLayoutOutcome::UpdateSuccess { data } => data,
        other => panic!("unexpected update outcome: {other:?}"),
    };
    let after = store.read_layout().expect("read replaced layout");
    let written = fs::read(&layout_path).expect("read raw replaced file");
    let fixture_modes = format!(
        "{}/{}",
        mode(layout_path.parent().expect("layout parent")),
        mode(&layout_path)
    );
    let leftovers = temporary_file_count(layout_path.parent().expect("layout parent"));
    write_private(&layout_path, bounded_json().as_bytes());
    let bounded = store.read_layout().expect("read bounded layout");
    write_private(&layout_path, INVALID_ENVELOPE_JSON);
    let invalid_envelope = store
        .read_layout()
        .expect_err("reject invalid layout envelope")
        .to_string();
    write_private(&layout_path, INVALID_COORDINATE_JSON);
    let invalid_coordinate = store
        .read_layout()
        .expect_err("reject invalid layout coordinate")
        .to_string();
    let rust = [
        format!("missing={}:{}", missing.len(), missing_exists),
        format!(
            "before={}",
            before
                .iter()
                .map(entry_signature)
                .collect::<Vec<_>>()
                .join(",")
        ),
        format!("saved={}", entry_signature(&saved)),
        format!(
            "after={}",
            after
                .iter()
                .map(entry_signature)
                .collect::<Vec<_>>()
                .join(",")
        ),
        format!(
            "written={}:newline={}",
            String::from_utf8_lossy(&written).trim_end(),
            written.ends_with(b"\n")
        ),
        format!("modes={fixture_modes}:temporary={leftovers}"),
        format!(
            "bounded={}:{}:{}",
            bounded.len(),
            entry_signature(bounded.first().expect("bounded first")),
            entry_signature(bounded.last().expect("bounded last"))
        ),
        format!("invalid-envelope={invalid_envelope}"),
        format!("invalid-coordinate={invalid_coordinate}"),
    ]
    .join("\n");
    assert_eq!(rust, node.trim_end());
}

// verifies ordinary filesystem errors remain distinguishable from canonical adapter errors
#[test]
fn preserves_layout_file_io_errors() {
    let root = TemporaryRoot::new();
    let directory_target = root.0.join("directory-target");
    fs::create_dir_all(&directory_target).expect("create directory target");
    let mut store = FixtureFileStore::new(
        directory_target,
        FixtureJsonAdapter,
        FixtureTemporaryPaths { next: 0 },
    );
    let error = store
        .read_layout()
        .expect_err("directory read must remain an io failure");
    assert!(error.source().is_some());
    assert_ne!(error.to_string(), "property sensor layout is invalid");
}
