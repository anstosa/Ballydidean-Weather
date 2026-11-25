use std::error::Error;
use std::fmt::{Display, Formatter};

use weather_contracts::{
    EcmwfTemperatureCanaryRecentErrorState, EcmwfTemperatureCanaryRecentErrorStateCohortLiteral,
    EcmwfTemperatureCanaryRunAdapterVersionLiteral, EcmwfTemperatureCanaryRunModelCycleEnum,
    EcmwfTemperatureCanaryRunStateStatusEnum, EcmwfTemperatureCanaryRunUpstreamModelLiteral,
    JsDateOrString, Nullable, PersistedEcmwfTemperatureRevision,
};
use weather_database::{
    AdmittedEcmwfCanaryBestMatchRow, AdmittedRevisionUtcInstant, ECMWF_CANARY_BEST_MATCH_SQL,
    ECMWF_CANARY_EXISTING_HOURS_SQL, ECMWF_CANARY_EXISTING_RUN_SQL, ECMWF_CANARY_INSERT_HOUR_SQL,
    ECMWF_CANARY_INSERT_RUN_SQL, ECMWF_CANARY_LOGICAL_RECEIPT_SQL, ECMWF_CANARY_SITE_SQL,
    ECMWF_CANARY_UPDATE_RECEIPT_SQL, ECMWF_TEMPERATURE_CANARY_RUN_INITIALIZATIONS_SQL,
    EcmwfCanaryBestMatchQuery, EcmwfCanaryExistingHoursQuery, EcmwfCanaryExistingRunQuery,
    EcmwfCanaryHourInsertQuery, EcmwfCanaryLogicalReceiptQuery, EcmwfCanaryPersistenceBind,
    EcmwfCanaryPersistencePort, EcmwfCanaryReceiptUpdateQuery, EcmwfCanaryRevisionArchivePort,
    EcmwfCanaryRevisionSavepointCommand, EcmwfCanaryRunInsertQuery, EcmwfCanarySiteQuery,
    EcmwfCanarySiteRow, EcmwfTemperatureCanaryReadBind,
    EcmwfTemperatureCanaryRunInitializationsReadPlan, SerializedJson, Sha256Digest,
    StoredEcmwfCanaryHourIdentity, StoredEcmwfCanaryRun,
};
use weather_runtime::{
    DatabaseTemperatureCanaryRepository, TemperatureCanaryPersistenceDatabasePort,
    TemperatureCanaryPersistencePreparationPort, TemperatureCanaryRecentErrorDigestPort,
    TemperatureCanaryRevisionArchiveMode, TemperatureCanaryRunDigestMaterial,
};
use weather_worker::{
    TaskFailure, TemperatureCanaryBatch, TemperatureCanaryExecution,
    TemperatureCanaryFetchControls, TemperatureCanaryFetchLimits, TemperatureCanaryHour,
    TemperatureCanaryProvider, TemperatureCanaryRecentErrorBuilder,
    TemperatureCanaryRecentErrorState, TemperatureCanaryRepository, TemperatureCanaryRequest,
    TemperatureCanarySource, TemperatureCanaryTimeAdapter, collect_ecmwf_temperature_canary_runs,
};

const RUN_AT: &str = "2026-09-07T18:00:00.000Z";
const RUN_EPOCH: i64 = 1_788_804_000_000;
const RECEIVED_AT: &str = "2026-09-08T00:35:00.000Z";
const RECEIVED_EPOCH: i64 = 1_788_827_700_000;
const LOGICAL_RECEIPT: &str = "2026-09-08T00:36:00.000Z";
const WARMUP_START: &str = "2026-09-04T18:00:00.000Z";
const WARMUP_START_EPOCH: i64 = 1_788_544_800_000;
const WARMUP_START_OFFSET: &str = "2026-09-04T11:00:00-07:00";
const OLDER_CYCLE_OFFSET: &str = "2026-09-06T23:00:00-07:00";

const UTC_FIXTURES: &[(i64, &str)] = &[
    (1_788_544_800_000, "2026-09-04T18:00:00.000Z"),
    (1_788_566_400_000, "2026-09-05T00:00:00.000Z"),
    (1_788_588_000_000, "2026-09-05T06:00:00.000Z"),
    (1_788_609_600_000, "2026-09-05T12:00:00.000Z"),
    (1_788_631_200_000, "2026-09-05T18:00:00.000Z"),
    (1_788_652_800_000, "2026-09-06T00:00:00.000Z"),
    (1_788_674_400_000, "2026-09-06T06:00:00.000Z"),
    (1_788_696_000_000, "2026-09-06T12:00:00.000Z"),
    (1_788_717_600_000, "2026-09-06T18:00:00.000Z"),
    (1_788_739_200_000, "2026-09-07T00:00:00.000Z"),
    (1_788_760_800_000, "2026-09-07T06:00:00.000Z"),
    (1_788_778_800_000, "2026-09-07T11:00:00.000Z"),
    (1_788_782_400_000, "2026-09-07T12:00:00.000Z"),
    (RUN_EPOCH, RUN_AT),
    (1_788_807_600_000, "2026-09-07T19:00:00.000Z"),
    (1_788_811_200_000, "2026-09-07T20:00:00.000Z"),
    (1_788_814_800_000, "2026-09-07T21:00:00.000Z"),
    (1_788_818_400_000, "2026-09-07T22:00:00.000Z"),
    (1_788_822_000_000, "2026-09-07T23:00:00.000Z"),
    (1_788_825_600_000, "2026-09-08T00:00:00.000Z"),
    (1_788_829_200_000, "2026-09-08T01:00:00.000Z"),
    (1_788_832_800_000, "2026-09-08T02:00:00.000Z"),
    (1_788_836_400_000, "2026-09-08T03:00:00.000Z"),
    (1_788_840_000_000, "2026-09-08T04:00:00.000Z"),
    (1_788_843_600_000, "2026-09-08T05:00:00.000Z"),
    (1_788_847_200_000, "2026-09-08T06:00:00.000Z"),
    (1_788_850_800_000, "2026-09-08T07:00:00.000Z"),
    (1_788_854_400_000, "2026-09-08T08:00:00.000Z"),
    (1_788_858_000_000, "2026-09-08T09:00:00.000Z"),
    (1_788_861_600_000, "2026-09-08T10:00:00.000Z"),
    (1_788_865_200_000, "2026-09-08T11:00:00.000Z"),
    (1_788_868_800_000, "2026-09-08T12:00:00.000Z"),
    (1_788_872_400_000, "2026-09-08T13:00:00.000Z"),
    (1_788_876_000_000, "2026-09-08T14:00:00.000Z"),
    (1_788_879_600_000, "2026-09-08T15:00:00.000Z"),
    (1_788_883_200_000, "2026-09-08T16:00:00.000Z"),
    (1_788_886_800_000, "2026-09-08T17:00:00.000Z"),
    (1_788_890_400_000, "2026-09-08T18:00:00.000Z"),
    (1_788_894_000_000, "2026-09-08T19:00:00.000Z"),
    (RECEIVED_EPOCH, RECEIVED_AT),
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct FixtureError(&'static str);

impl Display for FixtureError {
    /// Render one injected database error.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.0)
    }
}

impl Error for FixtureError {}

#[derive(Clone, Debug, PartialEq)]
struct FixtureDatabase {
    archive_allowed: bool,
    best_match_query: Option<EcmwfCanaryBestMatchQuery>,
    commit_fails: bool,
    hour_queries: Vec<EcmwfCanaryHourInsertQuery>,
    inventory: Vec<JsDateOrString>,
    publication_count: usize,
    run_query: Option<EcmwfCanaryRunInsertQuery>,
    staged_revision: Option<PersistedEcmwfTemperatureRevision>,
    stored: StoredEcmwfCanaryRun,
    trace: Vec<String>,
}

impl EcmwfCanaryPersistencePort for FixtureDatabase {
    type Error = FixtureError;

    /// Record one transaction start.
    fn begin_transaction(&mut self) -> Result<(), Self::Error> {
        self.trace.push("begin".to_owned());
        Ok(())
    }

    /// Return the admitted active site identity.
    fn select_site(
        &mut self,
        query: &EcmwfCanarySiteQuery,
    ) -> Result<Option<EcmwfCanarySiteRow>, Self::Error> {
        assert_eq!(query.sql, ECMWF_CANARY_SITE_SQL);
        self.trace.push("site".to_owned());
        Ok(Some(EcmwfCanarySiteRow {
            id: "site-1".to_owned(),
            latitude: 56.0,
            longitude: -3.0,
        }))
    }

    /// Capture and return one newly inserted run.
    fn insert_run(
        &mut self,
        query: &EcmwfCanaryRunInsertQuery,
    ) -> Result<Option<StoredEcmwfCanaryRun>, Self::Error> {
        assert_eq!(query.sql, ECMWF_CANARY_INSERT_RUN_SQL);
        self.trace.push("insert-run".to_owned());
        self.run_query = Some(query.clone());
        Ok(Some(self.stored.clone()))
    }

    /// Capture each exact hour insert.
    fn insert_hour(&mut self, query: &EcmwfCanaryHourInsertQuery) -> Result<(), Self::Error> {
        assert_eq!(query.sql, ECMWF_CANARY_INSERT_HOUR_SQL);
        let EcmwfCanaryPersistenceBind::Count(lead) = query.binds[2] else {
            panic!("hour lead bind")
        };
        self.trace.push(format!("hour-{lead}"));
        self.hour_queries.push(query.clone());
        Ok(())
    }

    /// Keep retry-only lookup outside this new-run fixture.
    fn select_existing_run(
        &mut self,
        query: &EcmwfCanaryExistingRunQuery,
    ) -> Result<Option<StoredEcmwfCanaryRun>, Self::Error> {
        assert_eq!(query.sql, ECMWF_CANARY_EXISTING_RUN_SQL);
        Err(FixtureError("unexpected existing run"))
    }

    /// Keep retry-only hours outside this new-run fixture.
    fn select_existing_hours(
        &mut self,
        query: &EcmwfCanaryExistingHoursQuery,
    ) -> Result<Vec<StoredEcmwfCanaryHourIdentity>, Self::Error> {
        assert_eq!(query.sql, ECMWF_CANARY_EXISTING_HOURS_SQL);
        Err(FixtureError("unexpected existing hours"))
    }

    /// Keep retry-only receipt update outside this new-run fixture.
    fn update_last_received(
        &mut self,
        query: &EcmwfCanaryReceiptUpdateQuery,
    ) -> Result<(), Self::Error> {
        assert_eq!(query.sql, ECMWF_CANARY_UPDATE_RECEIPT_SQL);
        Err(FixtureError("unexpected receipt update"))
    }

    /// Record commit before optional publication.
    fn commit_transaction(&mut self) -> Result<(), Self::Error> {
        self.trace.push("commit".to_owned());
        (!self.commit_fails)
            .then_some(())
            .ok_or(FixtureError("commit failed"))
    }

    /// Record an outer rollback if the adapter rejects persistence.
    fn rollback_transaction(&mut self) -> Result<(), Self::Error> {
        self.trace.push("rollback".to_owned());
        Ok(())
    }

    /// Record retained client release after the transaction result.
    fn release_client(&mut self) {
        self.trace.push("release".to_owned());
    }
}

impl EcmwfCanaryRevisionArchivePort for FixtureDatabase {
    type Publication = &'static str;

    /// Return one database-owned logical receipt.
    fn select_logical_receipt(
        &mut self,
        query: &EcmwfCanaryLogicalReceiptQuery,
    ) -> Result<Option<AdmittedRevisionUtcInstant>, Self::Error> {
        assert!(self.archive_allowed);
        assert_eq!(query.sql, ECMWF_CANARY_LOGICAL_RECEIPT_SQL);
        self.trace.push("clock".to_owned());
        Ok(Some(AdmittedRevisionUtcInstant::admitted(LOGICAL_RECEIPT)))
    }

    /// Record exact archive savepoint commands.
    fn execute_revision_savepoint(
        &mut self,
        command: EcmwfCanaryRevisionSavepointCommand,
    ) -> Result<(), Self::Error> {
        assert!(self.archive_allowed);
        let label = match command {
            EcmwfCanaryRevisionSavepointCommand::Savepoint => "savepoint",
            EcmwfCanaryRevisionSavepointCommand::RollbackTo => "rollback-savepoint",
            EcmwfCanaryRevisionSavepointCommand::Release => "release-savepoint",
        };
        self.trace.push(label.to_owned());
        Ok(())
    }

    /// Capture the exact Best Match window and return an admitted source gap.
    fn select_best_match_rows(
        &mut self,
        query: &EcmwfCanaryBestMatchQuery,
    ) -> Result<Vec<AdmittedEcmwfCanaryBestMatchRow>, Self::Error> {
        assert!(self.archive_allowed);
        assert_eq!(query.sql, ECMWF_CANARY_BEST_MATCH_SQL);
        self.trace.push("best-match".to_owned());
        self.best_match_query = Some(query.clone());
        Ok(Vec::new())
    }

    /// Capture the generated canonical archive projection.
    fn stage_ecmwf_temperature_revision(
        &mut self,
        revision: &PersistedEcmwfTemperatureRevision,
    ) -> Result<Option<Self::Publication>, Self::Error> {
        assert!(self.archive_allowed);
        self.trace.push("stage".to_owned());
        self.staged_revision = Some(revision.clone());
        Ok(Some("publication"))
    }

    /// Record postcommit publication.
    fn publish_ecmwf_temperature_revision(
        &mut self,
        _publication: Self::Publication,
    ) -> Result<(), Self::Error> {
        assert!(self.archive_allowed);
        self.trace.push("publish".to_owned());
        self.publication_count += 1;
        Ok(())
    }
}

impl TemperatureCanaryPersistenceDatabasePort for FixtureDatabase {
    /// Return every older warmup cycle so the collector requests only the latest.
    fn read_temperature_canary_run_initializations(
        &mut self,
        plan: &EcmwfTemperatureCanaryRunInitializationsReadPlan,
    ) -> Result<Vec<JsDateOrString>, Self::Error> {
        assert_eq!(plan.sql, ECMWF_TEMPERATURE_CANARY_RUN_INITIALIZATIONS_SQL);
        assert_eq!(
            plan.binds,
            [
                EcmwfTemperatureCanaryReadBind::Text("ballydidean".to_owned()),
                EcmwfTemperatureCanaryReadBind::UtcInstant(WARMUP_START.to_owned()),
                EcmwfTemperatureCanaryReadBind::UtcInstant(RUN_AT.to_owned()),
            ],
        );
        self.trace.push("inventory".to_owned());
        Ok(self.inventory.clone())
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
struct FixturePreparation {
    digest_material: Option<TemperatureCanaryRunDigestMaterial>,
    formatted_epochs: Vec<i64>,
    hour_leads: Vec<u8>,
}

impl TemperatureCanaryPersistencePreparationPort for FixturePreparation {
    /// Resolve only fixed already normalized fixture instants.
    fn utc_epoch_milliseconds(&mut self, value: &str) -> Result<i64, TaskFailure> {
        // normalize the original offset representations before binding or comparing
        match value {
            OLDER_CYCLE_OFFSET => return Ok(1_788_760_800_000),
            WARMUP_START_OFFSET => return Ok(WARMUP_START_EPOCH),
            _ => {}
        }
        UTC_FIXTURES
            .iter()
            .find_map(|(epoch, instant)| (*instant == value).then_some(*epoch))
            .ok_or_else(fixture_failure)
    }

    /// Format only fixed already normalized fixture epochs.
    fn utc_string(&mut self, epoch_milliseconds: i64) -> Result<String, TaskFailure> {
        self.formatted_epochs.push(epoch_milliseconds);
        fixture_utc(epoch_milliseconds)
    }

    /// Return admitted JSON produced by the fixture's canonical owner.
    fn serialize_recent_error_state(
        &mut self,
        _state: &EcmwfTemperatureCanaryRecentErrorState,
    ) -> Result<SerializedJson, TaskFailure> {
        Ok(SerializedJson::admitted("{}"))
    }

    /// Return one deterministic admitted hour digest and retain lead order.
    fn digest_hour(&mut self, hour: &TemperatureCanaryHour) -> Result<Sha256Digest, TaskFailure> {
        self.hour_leads.push(hour.model_lead_hours);
        Ok(Sha256Digest::admitted(format!(
            "{:064x}",
            hour.model_lead_hours
        )))
    }

    /// Capture exact run digest material before returning its admitted hash.
    fn digest_run(
        &mut self,
        material: &TemperatureCanaryRunDigestMaterial,
    ) -> Result<Sha256Digest, TaskFailure> {
        self.digest_material = Some(material.clone());
        Ok(Sha256Digest::admitted("e".repeat(64)))
    }
}

impl TemperatureCanaryRecentErrorDigestPort for FixturePreparation {
    /// Return one admitted canonical recent-state digest.
    fn digest_recent_error_state(
        &mut self,
        _state: &EcmwfTemperatureCanaryRecentErrorState,
    ) -> Result<Sha256Digest, TaskFailure> {
        Ok(Sha256Digest::admitted("d".repeat(64)))
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct FixtureTime;

impl TemperatureCanaryTimeAdapter for FixtureTime {
    /// Format only fixed collector fixture epochs.
    fn utc_string(&mut self, epoch_ms: i64) -> Result<String, TaskFailure> {
        fixture_utc(epoch_ms)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct FixtureProvider;

impl TemperatureCanaryProvider for FixtureProvider {
    /// Return one complete normalized latest-run batch.
    fn fetch_single_run(
        &mut self,
        request: &TemperatureCanaryRequest,
        controls: TemperatureCanaryFetchControls,
    ) -> Result<TemperatureCanaryBatch, TaskFailure> {
        assert_eq!(request.run_initialized_at, RUN_AT);
        assert_eq!(controls.max_attempts, 1);
        let hours = (1..=18)
            .map(|lead| TemperatureCanaryHour {
                model_lead_hours: lead,
                raw_relative_humidity_percent: Some(75.0),
                raw_temperature_c: 10.0,
                raw_wind_speed_mps: Some(3.0),
                valid_at: fixture_utc(RUN_EPOCH + i64::from(lead) * 3_600_000)
                    .expect("fixture hour"),
                valid_at_ms: RUN_EPOCH + i64::from(lead) * 3_600_000,
            })
            .collect();
        Ok(TemperatureCanaryBatch {
            adapter_version: "open-meteo-ecmwf-single-run/v1".to_owned(),
            attempts: 1,
            hours,
            model_cycle: "50r1".to_owned(),
            provider_response_sha256: "a".repeat(64),
            received_at: RECEIVED_AT.to_owned(),
            run_initialized_at: RUN_AT.to_owned(),
            upstream_model: "ecmwf_ifs".to_owned(),
        })
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct FixtureRecentError;

impl TemperatureCanaryRecentErrorBuilder for FixtureRecentError {
    /// Return the exact cold causal state for the latest run.
    fn build_recent_error_state(
        &mut self,
        site_slug: &str,
        target_run_initialized_at: &str,
        target_run_initialized_at_ms: i64,
    ) -> Result<TemperatureCanaryRecentErrorState, TaskFailure> {
        assert_eq!(site_slug, "ballydidean");
        assert_eq!(target_run_initialized_at, RUN_AT);
        assert_eq!(target_run_initialized_at_ms, RUN_EPOCH);
        Ok(TemperatureCanaryRecentErrorState {
            b24_c: None,
            b72_c: None,
            local_dates: 0,
            mad72_c: None,
            maximum_source_run_initialized_at: None,
            maximum_source_valid_at: None,
            n24: 0,
            n72: 0,
            source_keys: Vec::new(),
            supported: false,
            target_run_initialized_at: RUN_AT.to_owned(),
            window_end_valid_at: "2026-09-07T11:00:00.000Z".to_owned(),
        })
    }
}

/// Exercise collector persistence through archive-enabled and disabled runtime paths.
#[test]
fn collector_executes_typed_database_persistence_and_optional_archive() {
    let (archived, archived_repository) =
        collect_fixture(TemperatureCanaryRevisionArchiveMode::Enabled);
    assert_eq!(archived.persisted_runs, 1);
    assert_eq!(archived.failed_runs, 0);
    assert_eq!(archived.requested_runs, [RUN_AT]);
    let archived_database = archived_repository.database();
    assert_eq!(trace(&archived_database.trace), expected_trace(true));
    assert_eq!(archived_database.publication_count, 1);
    assert_eq!(archived_database.hour_queries.len(), 18);
    let run_query = archived_database.run_query.as_ref().expect("run query");
    assert_eq!(run_query.sql, ECMWF_CANARY_INSERT_RUN_SQL);
    assert_eq!(digest_bind(&run_query.binds[11]), "e".repeat(64));
    assert_eq!(digest_bind(&run_query.binds[8]), "d".repeat(64));
    let best_match = archived_database
        .best_match_query
        .as_ref()
        .expect("best match query");
    assert_eq!(
        render_best_match_binds(&best_match.binds),
        "ballydidean|2026-09-08T01:00:00.000Z|2026-09-08T13:00:00.000Z|2026-09-08T00:36:00.000Z"
    );
    let revision = archived_database
        .staged_revision
        .as_ref()
        .expect("staged revision");
    assert_eq!(revision.hours.len(), 18);
    assert_eq!(revision.contentHash, "e".repeat(64));
    assert_eq!(revision.logicalReceivedAt, LOGICAL_RECEIPT);
    let material = archived_repository
        .preparation()
        .digest_material
        .as_ref()
        .expect("digest material");
    assert_eq!(material.hour_content_hashes.len(), 18);
    assert_eq!(
        material.state_status,
        EcmwfTemperatureCanaryRunStateStatusEnum::Cold
    );
    let inventory_epochs = inventory_clock_formats();
    assert_eq!(
        archived_repository.preparation().formatted_epochs,
        [inventory_epochs.as_slice(), &[1_788_872_400_000]].concat()
    );

    let (plain, plain_repository) = collect_fixture(TemperatureCanaryRevisionArchiveMode::Disabled);
    assert_eq!(plain.persisted_runs, 1);
    assert_eq!(plain.failed_runs, 0);
    assert_eq!(
        trace(&plain_repository.database().trace),
        expected_trace(false)
    );
    assert_eq!(plain_repository.database().publication_count, 0);
    assert!(plain_repository.database().best_match_query.is_none());
    assert!(plain_repository.database().staged_revision.is_none());
    assert_eq!(
        plain_repository.preparation().formatted_epochs,
        inventory_epochs
    );
}

/// Keep the inclusive binds and canonical returned run order without changing warmup selection.
#[test]
fn normalizes_inventory_clocks_and_rejects_invalid_bounds_before_database_read() {
    let mut repository = DatabaseTemperatureCanaryRepository::new(
        fixture_database(false),
        FixturePreparation::default(),
        TemperatureCanaryRevisionArchiveMode::Disabled,
    );
    for (from, to) in [(RUN_AT, WARMUP_START), ("2026-09-04T18:00:00.000", RUN_AT)] {
        assert_eq!(
            repository
                .list_run_initializations("ballydidean", from, to)
                .expect_err("invalid inventory interval")
                .code,
            "temperature_canary_persistence_failed"
        );
    }
    assert!(repository.database().trace.is_empty());
    let runs = repository
        .list_run_initializations("ballydidean", WARMUP_START_OFFSET, RUN_AT)
        .expect("ordered warmup inventory");
    assert_eq!(runs.len(), 12);
    assert_eq!(runs[0], "2026-09-07T12:00:00.000Z");
    assert_eq!(runs[1], "2026-09-07T06:00:00.000Z");
    assert_eq!(runs[11], WARMUP_START);
    assert_eq!(repository.database().trace, ["inventory"]);
}

/// Fail the whole inventory when one invalid storage Date cannot become UTC.
#[test]
fn rejects_an_invalid_storage_date() {
    let mut database = fixture_database(false);
    database.inventory.push(JsDateOrString::InvalidDate);
    let mut repository = DatabaseTemperatureCanaryRepository::new(
        database,
        FixturePreparation::default(),
        TemperatureCanaryRevisionArchiveMode::Disabled,
    );
    assert_eq!(
        repository
            .list_run_initializations("ballydidean", WARMUP_START, RUN_AT)
            .expect_err("invalid storage date")
            .code,
        "temperature_canary_persistence_failed"
    );
    assert_eq!(repository.database().trace, ["inventory"]);
}

/// Preserve the original normalization order before the archive-only lookup clock.
fn inventory_clock_formats() -> Vec<i64> {
    let mut epochs = vec![WARMUP_START_EPOCH, RUN_EPOCH];
    // append the 12 existing rows from newest to oldest
    epochs.extend(
        (0..12)
            .rev()
            .map(|cycle| WARMUP_START_EPOCH + cycle * 21_600_000),
    );
    epochs
}

/// A serving commit failure rolls back and never executes staged publication work.
#[test]
fn failed_serving_commit_does_not_publish_the_staged_revision() {
    let mut database = fixture_database(true);
    database.commit_fails = true;
    let (result, repository) =
        collect_fixture_with_database(TemperatureCanaryRevisionArchiveMode::Enabled, database);

    assert_eq!(result.persisted_runs, 0);
    assert_eq!(result.failed_runs, 1);
    assert_eq!(repository.database().publication_count, 0);
    let trace = &repository.database().trace;
    assert_eq!(trace.iter().filter(|event| *event == "commit").count(), 1);
    assert_eq!(trace.iter().filter(|event| *event == "rollback").count(), 1);
    assert!(!trace.iter().any(|event| event == "publish"));
}

/// Run the real collector with one runtime persistence adapter.
fn collect_fixture(
    archive_mode: TemperatureCanaryRevisionArchiveMode,
) -> (
    weather_worker::TemperatureCanaryCollection,
    DatabaseTemperatureCanaryRepository<FixtureDatabase, FixturePreparation>,
) {
    collect_fixture_with_database(
        archive_mode,
        fixture_database(archive_mode == TemperatureCanaryRevisionArchiveMode::Enabled),
    )
}

/// Run the real collector with one explicitly configured database fixture.
fn collect_fixture_with_database(
    archive_mode: TemperatureCanaryRevisionArchiveMode,
    database: FixtureDatabase,
) -> (
    weather_worker::TemperatureCanaryCollection,
    DatabaseTemperatureCanaryRepository<FixtureDatabase, FixturePreparation>,
) {
    let mut repository = DatabaseTemperatureCanaryRepository::new(
        database,
        FixturePreparation::default(),
        archive_mode,
    );
    let mut provider = FixtureProvider;
    let mut recent_error = FixtureRecentError;
    let mut time = FixtureTime;
    let source = TemperatureCanarySource {
        active: true,
        adapter_version: "open-meteo-ecmwf-single-run/v1".to_owned(),
        capability: "forecast".to_owned(),
        dataset: "single_run".to_owned(),
        latitude: 56.0,
        longitude: -3.0,
        provider_key: "open-meteo".to_owned(),
        site_slug: "ballydidean".to_owned(),
        source_kind: "forecast".to_owned(),
        upstream_model: "ecmwf_ifs".to_owned(),
    };
    let result = collect_ecmwf_temperature_canary_runs(
        RECEIVED_EPOCH,
        &mut TemperatureCanaryExecution {
            fetch_limits: TemperatureCanaryFetchLimits::default(),
            provider: &mut provider,
            recent_error: &mut recent_error,
            repository: &mut repository,
            source: &source,
            time: &mut time,
        },
    )
    .expect("collector execution");
    (result, repository)
}

/// Build one database fixture with all older warmup cycles present.
fn fixture_database(archive_allowed: bool) -> FixtureDatabase {
    let inventory = [
        "2026-09-04T18:00:00.000Z",
        "2026-09-05T00:00:00.000Z",
        "2026-09-05T06:00:00.000Z",
        "2026-09-05T12:00:00.000Z",
        "2026-09-05T18:00:00.000Z",
        "2026-09-06T00:00:00.000Z",
        "2026-09-06T06:00:00.000Z",
        "2026-09-06T12:00:00.000Z",
        "2026-09-06T18:00:00.000Z",
        "2026-09-07T00:00:00.000Z",
        "2026-09-07T06:00:00.000Z",
        "2026-09-07T12:00:00.000Z",
    ]
    .into_iter()
    .rev()
    .enumerate()
    // mix the original driver's Date and explicit-zone string representations
    .map(|(index, value)| match index {
        0 => JsDateOrString::DateEpochMilliseconds(1_788_782_400_000),
        1 => JsDateOrString::String(OLDER_CYCLE_OFFSET.encode_utf16().collect()),
        _ => JsDateOrString::String(value.encode_utf16().collect()),
    })
    .collect();
    FixtureDatabase {
        archive_allowed,
        best_match_query: None,
        commit_fails: false,
        hour_queries: Vec::new(),
        inventory,
        publication_count: 0,
        run_query: None,
        staged_revision: None,
        stored: StoredEcmwfCanaryRun {
            adapter_version: EcmwfTemperatureCanaryRunAdapterVersionLiteral,
            content_hash: Sha256Digest::admitted("e".repeat(64)),
            first_received_at: RECEIVED_AT.to_owned(),
            id: "run-1".to_owned(),
            model_cycle: EcmwfTemperatureCanaryRunModelCycleEnum::Value50r1,
            provider_response_sha256: Sha256Digest::admitted("a".repeat(64)),
            recent_error_state: generated_cold_state(),
            recent_error_state_sha256: Sha256Digest::admitted("d".repeat(64)),
            run_initialized_at: RUN_AT.to_owned(),
            state_reason: "no_causal_forecast_observation_pairs".to_owned(),
            state_status: EcmwfTemperatureCanaryRunStateStatusEnum::Cold,
            upstream_model: EcmwfTemperatureCanaryRunUpstreamModelLiteral,
        },
        trace: Vec::new(),
    }
}

/// Build the generated cold state returned by the database fixture.
fn generated_cold_state() -> EcmwfTemperatureCanaryRecentErrorState {
    EcmwfTemperatureCanaryRecentErrorState {
        b24C: Nullable::Null,
        b72C: Nullable::Null,
        cohort: EcmwfTemperatureCanaryRecentErrorStateCohortLiteral,
        localDates: 0,
        mad72C: Nullable::Null,
        maximumSourceRunInitializedAt: Nullable::Null,
        maximumSourceValidAt: Nullable::Null,
        n24: 0,
        n72: 0,
        sourceKeys: Vec::new(),
        supported: false,
        targetRunInitializedAt: RUN_AT.to_owned(),
        windowEndValidAt: "2026-09-07T11:00:00.000Z".to_owned(),
    }
}

/// Resolve one static canonical UTC fixture without implementing a codec.
fn fixture_utc(epoch_milliseconds: i64) -> Result<String, TaskFailure> {
    UTC_FIXTURES
        .iter()
        .find_map(|(epoch, instant)| {
            (*epoch == epoch_milliseconds).then_some((*instant).to_owned())
        })
        .ok_or_else(fixture_failure)
}

/// Build one stable fake-boundary failure.
fn fixture_failure() -> TaskFailure {
    TaskFailure {
        attempts: 0,
        code: "fixture_time_missing".to_owned(),
    }
}

/// Return a digest bind or fail the fixture immediately.
fn digest_bind(bind: &EcmwfCanaryPersistenceBind) -> String {
    let EcmwfCanaryPersistenceBind::Digest(value) = bind else {
        panic!("digest bind")
    };
    value.as_str().to_owned()
}

/// Render Best Match placeholder order for exact window evidence.
fn render_best_match_binds(binds: &[EcmwfCanaryPersistenceBind; 4]) -> String {
    binds
        .iter()
        .map(|bind| match bind {
            EcmwfCanaryPersistenceBind::Text(value)
            | EcmwfCanaryPersistenceBind::UtcInstant(value) => value.clone(),
            _ => panic!("Best Match text bind"),
        })
        .collect::<Vec<_>>()
        .join("|")
}

/// Build the expected shared transaction trace for one archive mode.
fn expected_trace(archive_enabled: bool) -> String {
    let mut trace = vec![
        "inventory".to_owned(),
        "begin".to_owned(),
        "site".to_owned(),
        "insert-run".to_owned(),
    ];

    // retain all eighteen hour writes in exact lead order
    for lead in 1..=18 {
        trace.push(format!("hour-{lead}"));
    }
    // include optional archive stage before commit and publication after commit
    if archive_enabled {
        trace.extend(
            [
                "clock",
                "savepoint",
                "best-match",
                "stage",
                "release-savepoint",
            ]
            .into_iter()
            .map(str::to_owned),
        );
    }
    trace.push("commit".to_owned());
    // publish only the archive-enabled committed path
    if archive_enabled {
        trace.push("publish".to_owned());
    }
    trace.push("release".to_owned());
    trace.join(",")
}

/// Join one observed command trace.
fn trace(values: &[String]) -> String {
    values.join(",")
}
