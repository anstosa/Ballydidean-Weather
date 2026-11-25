use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::Command;

use weather_database::{
    AdmittedHealthMigrationAuthorization, AdmittedHealthMigrationIdentity, AppliedMigration,
    HEALTH_LATEST_WORKER_HEARTBEAT_SQL, HEALTH_MIGRATION_LEDGER_SQL, HEALTH_PING_SQL,
    HealthMigrationReadinessError, MigrationDigest, MigrationPlanningError, MigrationScript,
    health_latest_worker_heartbeat_read_plan, health_migration_ledger_read_plan,
    health_ping_read_plan, verify_health_migration_readiness,
};

/// Match production Node health SQL and the exact local artifact-ledger version.
#[test]
fn matches_compiled_node_health_readiness() {
    let oracle = node_oracle();
    let ping = health_ping_read_plan();
    let migration = health_migration_ledger_read_plan();
    let heartbeat = health_latest_worker_heartbeat_read_plan();

    assert_eq!(normalize_sql(ping.sql), oracle["ping-sql"]);
    assert_eq!(normalize_sql(migration.sql), oracle["migration-sql"]);
    assert_eq!(normalize_sql(heartbeat.sql), oracle["heartbeat-sql"]);
    assert_eq!(ping.sql, HEALTH_PING_SQL);
    assert_eq!(migration.sql, HEALTH_MIGRATION_LEDGER_SQL);
    assert_eq!(heartbeat.sql, HEALTH_LATEST_WORKER_HEARTBEAT_SQL);
    assert!(ping.binds.is_empty());
    assert!(migration.binds.is_empty());
    assert!(heartbeat.binds.is_empty());

    let (identity, applied) = exact_identity(&oracle);
    let readiness = verify_health_migration_readiness(&identity, &applied)
        .expect("exact local ledger must be current");
    assert_eq!(readiness.version, "0023_forecast_product_read_index.sql");
    assert_eq!(
        oracle["current"],
        "ready|current|0023_forecast_product_read_index.sql|2026-09-07T18:02:00.000Z"
    );
}

/// Preserve pending, trailing, authorization release, and authorization digest categories.
#[test]
fn enforces_migration_readiness_authorization_boundaries() {
    let oracle = node_oracle();
    let (mut identity, mut applied) = exact_identity(&oracle);
    applied.pop();
    assert_eq!(
        verify_health_migration_readiness(&identity, &applied),
        Err(HealthMigrationReadinessError::PendingArtifacts)
    );

    let (_, mut trailing) = exact_identity(&oracle);
    trailing.push(AppliedMigration {
        name: "9999_future.sql".to_owned(),
        digest: MigrationDigest::admitted("f".repeat(64)),
    });
    assert_eq!(
        verify_health_migration_readiness(&identity, &trailing),
        Err(HealthMigrationReadinessError::Planning(
            MigrationPlanningError::HistoryDiverges
        ))
    );

    identity.applied_history_sha256 = MigrationDigest::admitted("authorized-ledger");
    identity.authorization = Some(AdmittedHealthMigrationAuthorization {
        history_sha256: MigrationDigest::admitted("authorized-ledger"),
        release: "2026.11.23-1".to_owned(),
    });
    identity.release = "2026.11.23-1".to_owned();
    assert!(verify_health_migration_readiness(&identity, &trailing).is_ok());

    identity.release = "2026.11.23-2".to_owned();
    assert_eq!(
        verify_health_migration_readiness(&identity, &trailing),
        Err(HealthMigrationReadinessError::AuthorizationReleaseMismatch)
    );
    identity.release = "2026.11.23-1".to_owned();
    identity.applied_history_sha256 = MigrationDigest::admitted("different-ledger");
    assert_eq!(
        verify_health_migration_readiness(&identity, &trailing),
        Err(HealthMigrationReadinessError::AuthorizationHistoryMismatch)
    );
}

/// require exact alternate ledgers while admitting authorized future tails.
#[test]
fn binds_alternate_migrations_to_reviewed_ledgers() {
    let oracle = node_oracle();
    let (mut identity, mut applied) = exact_identity(&oracle);
    let frontier = applied
        .iter_mut()
        .find(|migration| migration.name == "0020_adjustment_revision_frontier.sql")
        .expect("frontier migration");
    frontier.digest = MigrationDigest::admitted(
        "58d50e88fa767212865e222ec02e55283ceac5b00538aa2898b5ebc2873b28f6",
    );

    identity.applied_history_sha256 = MigrationDigest::admitted("wrong-ledger");
    assert_eq!(
        verify_health_migration_readiness(&identity, &applied),
        Err(HealthMigrationReadinessError::AlternateHistoryMismatch)
    );
    identity.applied_history_sha256 = MigrationDigest::admitted(
        "3fa9ed85218278aa3b20537d9fbe9b88ef1adee7d7b2f80fde832c45d9cb4554",
    );
    assert!(verify_health_migration_readiness(&identity, &applied).is_ok());

    // model rollback readiness against a newer ledger
    applied.push(AppliedMigration {
        name: "9999_future.sql".to_owned(),
        digest: MigrationDigest::admitted("f".repeat(64)),
    });
    identity.applied_history_sha256 = MigrationDigest::admitted("mixed-generation-ledger");
    identity.authorization = Some(AdmittedHealthMigrationAuthorization {
        history_sha256: MigrationDigest::admitted("mixed-generation-ledger"),
        release: "2026.11.23-1".to_owned(),
    });
    identity.release = "2026.11.23-1".to_owned();
    assert_eq!(
        verify_health_migration_readiness(&identity, &applied)
            .expect("authorized newer ledger remains ready for this artifact generation")
            .version,
        "0023_forecast_product_read_index.sql"
    );
}

/// Invoke the compiled API store over fake no-socket query results.
fn node_oracle() -> BTreeMap<String, String> {
    let script =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/health-reads-node-oracle.mjs");
    let output = Command::new("node")
        .arg(script)
        .output()
        .expect("Node health oracle must start");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("Node health oracle must emit UTF-8")
        .lines()
        .map(|line| {
            let (key, value) = line.split_once('=').expect("oracle field");
            (key.to_owned(), value.to_owned())
        })
        .collect()
}

/// Build admitted artifacts and applied rows from production local file hashes.
fn exact_identity(
    oracle: &BTreeMap<String, String>,
) -> (AdmittedHealthMigrationIdentity, Vec<AppliedMigration>) {
    let rows = oracle["ledger"]
        .split(',')
        .map(|entry| {
            let (name, checksum) = entry.split_once(':').expect("ledger row");
            (name.to_owned(), MigrationDigest::admitted(checksum))
        })
        .collect::<Vec<_>>();
    let artifacts = rows
        .iter()
        .map(|(name, digest)| MigrationScript {
            name: name.clone(),
            digest: digest.clone(),
            sql: String::new(),
        })
        .collect();
    let applied = rows
        .into_iter()
        .map(|(name, digest)| AppliedMigration { name, digest })
        .collect();
    (
        AdmittedHealthMigrationIdentity {
            applied_history_sha256: MigrationDigest::admitted(&oracle["history-sha256"]),
            artifacts,
            authorization: None,
            release: "development".to_owned(),
        },
        applied,
    )
}

/// Collapse irrelevant whitespace while retaining every SQL token.
fn normalize_sql(sql: &str) -> String {
    sql.split_whitespace().collect::<Vec<_>>().join(" ")
}
