use std::error::Error;
use std::fmt::{Display, Formatter};
use std::path::PathBuf;
use std::process::Command;
use std::{cell::RefCell, rc::Rc};

use weather_edge::{
    AdmittedAdminMutation, AdmittedAdminOperator, AdmittedPropertySensorUpdateInstant,
    DecodedPropertySensorLayoutUpdate, InMemoryPropertySensorLayoutStore, PropertySensorIcon,
    PropertySensorLayoutEntry, PropertySensorLayoutOutcome, PropertySensorLayoutRequest,
    PropertySensorLayoutStore, PropertySensorLayoutUpdateInstantSource,
    PropertySensorLayoutValidationError, PropertySensorMapCenter, handle_property_sensor_layout,
    validate_property_sensor_layout_update,
};

const CENTER: PropertySensorMapCenter = PropertySensorMapCenter {
    latitude: 47.95,
    longitude: -122.43,
};
const UPDATED_AT: &str = "2026-11-23T12:34:56.000Z";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FixtureStoreError;

impl Display for FixtureStoreError {
    /// render one injected atomic write failure.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("write failed")
    }
}

impl Error for FixtureStoreError {}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FixtureUpdateInstants {
    calls: usize,
}

impl FixtureUpdateInstants {
    /// create one fixed admitted update-time boundary.
    const fn fixed() -> Self {
        Self { calls: 0 }
    }
}

impl PropertySensorLayoutUpdateInstantSource for FixtureUpdateInstants {
    type Error = FixtureStoreError;

    /// return one fixed canonical instant and retain call count.
    fn admitted_update_instant(
        &mut self,
    ) -> Result<AdmittedPropertySensorUpdateInstant, Self::Error> {
        self.calls += 1;
        Ok(AdmittedPropertySensorUpdateInstant::admitted(UPDATED_AT))
    }
}

#[derive(Debug, Clone, PartialEq)]
struct FailingStore {
    layout: Vec<PropertySensorLayoutEntry>,
}

impl PropertySensorLayoutStore for FailingStore {
    type Error = FixtureStoreError;

    /// return the unchanged admitted fixture layout.
    fn read_layout(&mut self) -> Result<Vec<PropertySensorLayoutEntry>, Self::Error> {
        Ok(self.layout.clone())
    }

    /// fail before publishing the replacement layout.
    fn replace_layout_atomically(
        &mut self,
        _layout: Vec<PropertySensorLayoutEntry>,
    ) -> Result<(), Self::Error> {
        Err(FixtureStoreError)
    }
}

#[derive(Debug, Clone)]
struct SequenceStore {
    fail_read: bool,
    trace: Rc<RefCell<Vec<&'static str>>>,
}

impl PropertySensorLayoutStore for SequenceStore {
    type Error = FixtureStoreError;

    /// record the current-layout read before returning its fixture result.
    fn read_layout(&mut self) -> Result<Vec<PropertySensorLayoutEntry>, Self::Error> {
        self.trace.borrow_mut().push("read");

        // inject one ordinary current-layout read failure
        if self.fail_read {
            return Err(FixtureStoreError);
        }

        Ok(Vec::new())
    }

    /// record atomic replacement after the admitted update instant.
    fn replace_layout_atomically(
        &mut self,
        _layout: Vec<PropertySensorLayoutEntry>,
    ) -> Result<(), Self::Error> {
        self.trace.borrow_mut().push("replace");
        Ok(())
    }
}

#[derive(Debug, Clone)]
struct SequenceUpdateInstants {
    calls: usize,
    trace: Rc<RefCell<Vec<&'static str>>>,
}

impl PropertySensorLayoutUpdateInstantSource for SequenceUpdateInstants {
    type Error = FixtureStoreError;

    /// record update-time admission between read and replacement.
    fn admitted_update_instant(
        &mut self,
    ) -> Result<AdmittedPropertySensorUpdateInstant, Self::Error> {
        self.calls += 1;
        self.trace.borrow_mut().push("clock");
        Ok(AdmittedPropertySensorUpdateInstant::admitted(UPDATED_AT))
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

/// build one decoded eligible update field set.
fn update_input(display_name: &str) -> DecodedPropertySensorLayoutUpdate {
    DecodedPropertySensorLayoutUpdate {
        display_name: Some(display_name.to_owned()),
        icon: Some("temperature".to_owned()),
        latitude: Some(47.9505),
        longitude: Some(-122.4281),
    }
}

/// render one placement like the compiled Node semantic oracle.
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

/// render one typed response like the compiled Node semantic oracle.
fn outcome_signature(outcome: &PropertySensorLayoutOutcome) -> String {
    // retain reads, saved entries, failures, and method allow-lists
    match outcome {
        PropertySensorLayoutOutcome::ReadSuccess { data, omit_body } => {
            // omit only the serialized response body for HEAD
            let body = if *omit_body {
                "head".to_owned()
            } else if data.is_empty() {
                "empty".to_owned()
            } else {
                data.iter()
                    .map(entry_signature)
                    .collect::<Vec<_>>()
                    .join(",")
            };
            format!(
                "{}:{body}:{}:-",
                outcome.status_code(),
                outcome.cache_control()
            )
        }
        PropertySensorLayoutOutcome::UpdateSuccess { data } => format!(
            "{}:{}:{}:-",
            outcome.status_code(),
            entry_signature(data),
            outcome.cache_control()
        ),
        PropertySensorLayoutOutcome::InvalidRequest => "400:invalid_request:no-store:-".to_owned(),
        PropertySensorLayoutOutcome::StoreFailure => {
            "500:internal_server_error:no-store:-".to_owned()
        }
        PropertySensorLayoutOutcome::MethodNotAllowed { allow } => format!(
            "405:method_not_allowed:no-store:{}",
            allow.header_value().replace(' ', "_")
        ),
    }
}

/// execute one admitted administrator sensor replacement.
fn replace<Store: PropertySensorLayoutStore>(
    mutation: AdmittedAdminMutation<'_>,
    store: &mut Store,
    update_instants: &mut FixtureUpdateInstants,
    sensor_key: &str,
    input: DecodedPropertySensorLayoutUpdate,
) -> PropertySensorLayoutOutcome {
    handle_property_sensor_layout(
        CENTER,
        PropertySensorLayoutRequest::AdminReplace {
            mutation,
            sensor_key: sensor_key.to_owned(),
            input,
        },
        store,
        update_instants,
    )
}

/// execute one public layout read.
fn read<Store: PropertySensorLayoutStore>(
    store: &mut Store,
    update_instants: &mut FixtureUpdateInstants,
    omit_body: bool,
) -> PropertySensorLayoutOutcome {
    handle_property_sensor_layout(
        CENTER,
        PropertySensorLayoutRequest::PublicRead { omit_body },
        store,
        update_instants,
    )
}

// verifies actual source handler and store policy across the user workflow
#[test]
fn matches_compiled_node_property_sensor_layout_paths() {
    let oracle = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/property-sensor-layout-node-oracle.mjs");
    let output = Command::new("node")
        .arg(oracle)
        .output()
        .expect("run property sensor layout Node oracle");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let node = String::from_utf8(output.stdout).expect("Node oracle UTF-8");
    let operator = AdmittedAdminOperator::admitted("verified-operator");
    let mutation = AdmittedAdminMutation::admitted(&operator);
    let mut update_instants = FixtureUpdateInstants::fixed();
    let mut missing_store = InMemoryPropertySensorLayoutStore::missing();
    let missing = read(&mut missing_store, &mut update_instants, false);
    let missing_head = read(&mut missing_store, &mut update_instants, true);
    let public_unsupported = handle_property_sensor_layout(
        CENTER,
        PropertySensorLayoutRequest::PublicUnsupported,
        &mut missing_store,
        &mut update_instants,
    );
    let mut store = InMemoryPropertySensorLayoutStore::persisted(initial_layout());
    let before = read(&mut store, &mut update_instants, false);
    let update = replace(
        mutation,
        &mut store,
        &mut update_instants,
        "soil-1",
        update_input("  Orchard soil  "),
    );
    let after = read(&mut store, &mut update_instants, false);
    let repeated = replace(
        mutation,
        &mut store,
        &mut update_instants,
        "soil-1",
        update_input("  Orchard soil  "),
    );
    let after_repeated = read(&mut store, &mut update_instants, false);
    let invalid_key = replace(
        mutation,
        &mut store,
        &mut update_instants,
        "../soil",
        update_input("Orchard soil"),
    );
    let invalid_name = replace(
        mutation,
        &mut store,
        &mut update_instants,
        "soil-2",
        update_input(" "),
    );
    let outside = replace(
        mutation,
        &mut store,
        &mut update_instants,
        "soil-2",
        DecodedPropertySensorLayoutUpdate {
            latitude: Some(CENTER.latitude + 0.026),
            ..update_input("Orchard soil")
        },
    );
    let after_invalid = read(&mut store, &mut update_instants, false);
    let admin_unsupported = handle_property_sensor_layout(
        CENTER,
        PropertySensorLayoutRequest::AdminUnsupported {
            operator: &operator,
        },
        &mut store,
        &mut update_instants,
    );
    let persisted = store.read_layout().expect("infallible layout read");
    let mut failing = FailingStore { layout: persisted };
    let failed_write = replace(
        mutation,
        &mut failing,
        &mut update_instants,
        "soil-2",
        update_input("North soil"),
    );
    let after_failed_write = read(&mut failing, &mut update_instants, false);
    let rust = [
        ("missing", missing),
        ("missing-head", missing_head),
        ("public-unsupported", public_unsupported),
        ("before", before),
        ("update", update),
        ("after", after),
        ("repeated", repeated),
        ("after-repeated", after_repeated),
        ("invalid-key", invalid_key),
        ("invalid-name", invalid_name),
        ("outside", outside),
        ("after-invalid", after_invalid),
        ("admin-unsupported", admin_unsupported),
        ("failed-write", failed_write),
        ("after-failed-write", after_failed_write),
    ]
    .map(|(label, outcome)| format!("{label}={}", outcome_signature(&outcome)))
    .join("\n");
    assert_eq!(rust, node.trim_end());
}

// verifies exact rejected field families before reading or writing the store
#[test]
fn rejects_invalid_property_sensor_fields() {
    let cases = [
        (
            "Bad_Key",
            update_input("Soil"),
            PropertySensorLayoutValidationError::InvalidSensorKey,
        ),
        (
            "soil-1",
            DecodedPropertySensorLayoutUpdate {
                display_name: None,
                ..update_input("Soil")
            },
            PropertySensorLayoutValidationError::InvalidDisplayName,
        ),
        (
            "soil-1",
            DecodedPropertySensorLayoutUpdate {
                icon: Some("tree".to_owned()),
                ..update_input("Soil")
            },
            PropertySensorLayoutValidationError::InvalidIcon,
        ),
        (
            "soil-1",
            DecodedPropertySensorLayoutUpdate {
                latitude: None,
                ..update_input("Soil")
            },
            PropertySensorLayoutValidationError::InvalidCoordinates,
        ),
        (
            "soil-1",
            DecodedPropertySensorLayoutUpdate {
                longitude: Some(f64::INFINITY),
                ..update_input("Soil")
            },
            PropertySensorLayoutValidationError::InvalidCoordinates,
        ),
    ];

    // preserve the owning store's exact validation category
    for (sensor_key, input, expected) in cases {
        let error = validate_property_sensor_layout_update(
            CENTER,
            sensor_key,
            input,
            AdmittedPropertySensorUpdateInstant::admitted(UPDATED_AT),
        )
        .expect_err("invalid property sensor update");
        assert_eq!(error, expected);
    }
}

// verifies missing layouts default empty and persisted reads remain bounded
#[test]
fn defaults_missing_layout_and_bounds_persisted_reads() {
    let mut missing = InMemoryPropertySensorLayoutStore::missing();
    assert!(missing.read_layout().expect("missing layout").is_empty());
    let entries = (0..65)
        .map(|index| {
            entry(
                &format!("sensor-{index}"),
                "Sensor",
                Some(PropertySensorIcon::Temperature),
                CENTER.latitude,
                CENTER.longitude,
                UPDATED_AT,
            )
        })
        .collect();
    let mut persisted = InMemoryPropertySensorLayoutStore::persisted(entries);
    assert_eq!(persisted.read_layout().expect("bounded layout").len(), 64);
}

// verifies update time is admitted only after a successful current-layout read
#[test]
fn admits_update_time_between_layout_read_and_atomic_replacement() {
    let operator = AdmittedAdminOperator::admitted("verified-operator");
    let mutation = AdmittedAdminMutation::admitted(&operator);
    let trace = Rc::new(RefCell::new(Vec::new()));
    let mut store = SequenceStore {
        fail_read: false,
        trace: Rc::clone(&trace),
    };
    let mut update_instants = SequenceUpdateInstants {
        calls: 0,
        trace: Rc::clone(&trace),
    };
    let outcome = handle_property_sensor_layout(
        CENTER,
        PropertySensorLayoutRequest::AdminReplace {
            mutation,
            sensor_key: "soil-1".to_owned(),
            input: update_input("Orchard soil"),
        },
        &mut store,
        &mut update_instants,
    );
    assert!(matches!(
        outcome,
        PropertySensorLayoutOutcome::UpdateSuccess { .. }
    ));
    assert_eq!(&*trace.borrow(), &["read", "clock", "replace"]);
    assert_eq!(update_instants.calls, 1);

    trace.borrow_mut().clear();
    let mut failing_store = SequenceStore {
        fail_read: true,
        trace: Rc::clone(&trace),
    };
    let mut failing_instants = SequenceUpdateInstants {
        calls: 0,
        trace: Rc::clone(&trace),
    };
    let outcome = handle_property_sensor_layout(
        CENTER,
        PropertySensorLayoutRequest::AdminReplace {
            mutation,
            sensor_key: "soil-1".to_owned(),
            input: update_input("Orchard soil"),
        },
        &mut failing_store,
        &mut failing_instants,
    );
    assert_eq!(outcome, PropertySensorLayoutOutcome::StoreFailure);
    assert_eq!(&*trace.borrow(), &["read"]);
    assert_eq!(failing_instants.calls, 0);
}
