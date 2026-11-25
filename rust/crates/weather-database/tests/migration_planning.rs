use weather_database::{
    AppliedMigration, MigrationDigest, MigrationPlanningError, MigrationScript, plan_migrations,
};

const INITIAL_SQL: &str =
    include_str!("../../../../packages/database/migrations/0001_initial_weather.sql");
const READINESS_SQL: &str =
    include_str!("../../../../packages/database/migrations/0002_worker_migration_readiness.sql");
const ECOWITT_SQL: &str =
    include_str!("../../../../packages/database/migrations/0003_ecowitt_measurements.sql");

/// build real ordered artifacts
fn catalog() -> Vec<MigrationScript> {
    vec![
        MigrationScript {
            name: "0001_initial_weather.sql".to_owned(),
            digest: MigrationDigest::admitted(
                "f4264482606a3476b4b54e3089e66c900f0ad5104ea678a9bf17e5e5ba385a62",
            ),
            sql: INITIAL_SQL.to_owned(),
        },
        MigrationScript {
            name: "0002_worker_migration_readiness.sql".to_owned(),
            digest: MigrationDigest::admitted(
                "954731c9ddb791c85defc3bffd887a3d30e9ff9fb5c9dac96e050ac52a796f03",
            ),
            sql: READINESS_SQL.to_owned(),
        },
        MigrationScript {
            name: "0003_ecowitt_measurements.sql".to_owned(),
            digest: MigrationDigest::admitted(
                "250e9b49176fda732b23f797a6dbce6878ca93818fa1bf6a408b5d6611ee5a48",
            ),
            sql: ECOWITT_SQL.to_owned(),
        },
    ]
}

/// convert an artifact to a ledger row
fn applied(script: &MigrationScript) -> AppliedMigration {
    AppliedMigration {
        name: script.name.clone(),
        digest: script.digest.clone(),
    }
}

// verifies exact current and pending order
#[test]
fn plans_the_unapplied_catalog_tail_in_source_order() {
    let mut catalog = catalog();
    catalog.swap(0, 2);
    let history = vec![applied(&catalog[2])];

    let plan = plan_migrations(&catalog, &history).expect("valid ordered prefix");

    assert_eq!(plan.current, vec![&catalog[2]]);
    assert_eq!(plan.pending, vec![&catalog[1], &catalog[0]]);
    assert_eq!(plan.pending[0].sql, READINESS_SQL);
    assert_eq!(plan.pending[1].sql, ECOWITT_SQL);
}

// verifies a complete catalog is already current
#[test]
fn returns_no_pending_scripts_for_a_complete_history() {
    let catalog = catalog();
    let history = catalog.iter().map(applied).collect::<Vec<_>>();

    let plan = plan_migrations(&catalog, &history).expect("complete ordered history");

    assert_eq!(plan.current, catalog.iter().collect::<Vec<_>>());
    assert!(plan.pending.is_empty());
}

// verifies the exact checksum error category
#[test]
fn rejects_checksum_drift_with_the_artifact_name() {
    let catalog = catalog();
    let history = vec![AppliedMigration {
        name: catalog[0].name.clone(),
        digest: MigrationDigest::admitted("0".repeat(64)),
    }];

    let error = plan_migrations(&catalog, &history).expect_err("checksum drift");

    assert_eq!(
        error,
        MigrationPlanningError::ChecksumMismatch {
            name: "0001_initial_weather.sql".to_owned(),
        }
    );
    assert_eq!(
        error.to_string(),
        "migration checksum mismatch: 0001_initial_weather.sql"
    );
}

// verifies applied rows remain an exact prefix
#[test]
fn rejects_a_reordered_or_trailing_history() {
    let catalog = catalog();
    let reordered = vec![applied(&catalog[1])];
    let mut trailing = catalog.iter().map(applied).collect::<Vec<_>>();
    trailing.push(AppliedMigration {
        name: "9999_candidate_only.sql".to_owned(),
        digest: MigrationDigest::admitted("9".repeat(64)),
    });

    assert_eq!(
        plan_migrations(&catalog, &reordered),
        Err(MigrationPlanningError::HistoryDiverges)
    );
    assert_eq!(
        plan_migrations(&catalog, &trailing),
        Err(MigrationPlanningError::HistoryDiverges)
    );
}

// verifies the source empty-catalog invariant
#[test]
fn rejects_an_empty_catalog() {
    let error = plan_migrations(&[], &[]).expect_err("empty catalog");

    assert_eq!(error, MigrationPlanningError::EmptyCatalog);
    assert_eq!(
        error.to_string(),
        "migration directory contains no ordered SQL files"
    );
}

// verifies both exact reviewed checksum substitutions
#[test]
fn accepts_only_the_reviewed_alternate_checksum_pairs() {
    let catalog = vec![
        MigrationScript {
            name: "0020_adjustment_revision_frontier.sql".to_owned(),
            digest: MigrationDigest::admitted(
                "56027b4f2cb2c3c83e746f14dac753b75d573753934898fa7a52f78faa56499c",
            ),
            sql: "canonical revision frontier".to_owned(),
        },
        MigrationScript {
            name: "0023_forecast_product_read_index.sql".to_owned(),
            digest: MigrationDigest::admitted(
                "e637542cb2e742fe8f22f60313a11f00adbc1d140a811ee718524849fcece12a",
            ),
            sql: "canonical forecast index".to_owned(),
        },
    ];
    let history = vec![
        AppliedMigration {
            name: catalog[0].name.clone(),
            digest: MigrationDigest::admitted(
                "58d50e88fa767212865e222ec02e55283ceac5b00538aa2898b5ebc2873b28f6",
            ),
        },
        AppliedMigration {
            name: catalog[1].name.clone(),
            digest: MigrationDigest::admitted(
                "7e48acb14710a625cdcbb796bf1bf49ab4e00bf1aeb9bb763ac86410e08663e0",
            ),
        },
    ];

    let plan = plan_migrations(&catalog, &history).expect("reviewed alternates");

    assert_eq!(plan.current, catalog.iter().collect::<Vec<_>>());
    assert!(plan.pending.is_empty());
}
