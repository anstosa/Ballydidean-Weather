use std::cell::RefCell;
use std::cmp::Ordering;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use weather_api::{CanonicalInstant, CurrentProjectionError, CurrentTimeResolver};
use weather_contracts::JsDateOrString;
use weather_database::{
    AdmittedHealthMigrationAuthorization, AppliedMigration, MigrationArtifactNameOrder,
    MigrationArtifactSha256, MigrationDigest, ProcessMigrationArtifactFilesystem,
    verify_health_migration_readiness,
};
use weather_runtime::{WorkerBootValues, WorkerFilesystemBootValues, WorkerMigrationSource};

struct Source(PathBuf);

impl WorkerMigrationSource for Source {
    /// Use the repository's retained catalog, not any installed database root.
    fn migration_directory(&self) -> &Path {
        &self.0
    }

    /// The fixture uses a complete equal-generation ledger.
    fn migration_authorization(&self) -> Option<&AdmittedHealthMigrationAuthorization> {
        None
    }

    /// Load exactly one unchanged source release identity.
    fn release(&self) -> &str {
        "development"
    }
}

struct ShippedOrder;

impl MigrationArtifactNameOrder for ShippedOrder {
    /// The shipped catalog has unique numeric prefixes and no locale-dependent ties.
    fn compare_names(&self, left: &str, right: &str) -> Ordering {
        left[..4].cmp(&right[..4])
    }
}

struct CapturedDigestInputs(Rc<RefCell<Vec<Vec<u8>>>>);

impl MigrationArtifactSha256 for CapturedDigestInputs {
    /// Record exactly what the approved hash owner would receive.
    fn sha256(&mut self, bytes: &[u8]) -> MigrationDigest {
        self.0.borrow_mut().push(bytes.to_vec());
        MigrationDigest::admitted("a".repeat(64))
    }
}

struct FixtureTime;

impl CurrentTimeResolver for FixtureTime {
    /// Return the one already decoded fixture timestamp.
    fn resolve(&self, value: &JsDateOrString) -> Result<CanonicalInstant, CurrentProjectionError> {
        assert_eq!(
            value,
            &JsDateOrString::DateEpochMilliseconds(1_787_374_800_000)
        );
        Ok(CanonicalInstant {
            epoch_milliseconds: 1_787_374_800_000,
            utc: "2026-08-22T05:00:00.000Z".to_owned(),
        })
    }
}

/// Load the shipped bytes with real filesystem custody and feed the exact applied ledger preimage.
#[test]
fn runtime_loads_the_shipped_migration_material_without_database_io() {
    let source = Source(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../packages/database/migrations"),
    );
    let witnessed = Rc::new(RefCell::new(Vec::new()));
    let mut values = WorkerFilesystemBootValues::new(
        ProcessMigrationArtifactFilesystem,
        ShippedOrder,
        CapturedDigestInputs(Rc::clone(&witnessed)),
        || Some(1_787_374_800_000),
        FixtureTime,
    );
    let catalog = values.load_migration_artifacts(&source).unwrap();
    assert_eq!(
        catalog.artifacts.first().unwrap().name,
        "0001_initial_weather.sql"
    );
    assert_eq!(
        catalog.artifacts.last().unwrap().name,
        "0023_forecast_product_read_index.sql"
    );
    assert_eq!(catalog.release, "development");
    assert_eq!(witnessed.borrow().len(), catalog.artifacts.len());
    // verify every ordered hash input against the original deployed SQL material
    for (artifact, hashed) in catalog.artifacts.iter().zip(witnessed.borrow().iter()) {
        assert_eq!(artifact.sql.as_bytes(), hashed);
        assert_eq!(
            std::fs::read(source.0.join(&artifact.name)).unwrap(),
            *hashed
        );
    }
    let applied: Vec<_> = catalog
        .artifacts
        .iter()
        .map(
            // construct the complete admitted typed ledger without querying a database
            |script| AppliedMigration {
                name: script.name.clone(),
                digest: script.digest.clone(),
            },
        )
        .collect();
    let expected_history = applied
        .iter()
        .map(
            // capture the known source's exact delimiter and final line break
            |row| format!("{}:{}\n", row.name, row.digest.as_str()),
        )
        .collect::<String>();
    let identity =
        WorkerBootValues::<Source>::admit_migration_identity(&mut values, catalog, &applied)
            .unwrap();
    assert_eq!(
        witnessed.borrow().last().unwrap(),
        expected_history.as_bytes()
    );
    assert_eq!(
        verify_health_migration_readiness(&identity, &applied)
            .unwrap()
            .version,
        "0023_forecast_product_read_index.sql"
    );
    assert_eq!(
        WorkerBootValues::<Source>::health_now_ms(&mut values).unwrap(),
        1_787_374_800_000
    );
    assert_eq!(
        WorkerBootValues::<Source>::heartbeat_epoch_ms(
            &mut values,
            &JsDateOrString::DateEpochMilliseconds(1_787_374_800_000),
        )
        .unwrap(),
        1_787_374_800_000
    );
}
