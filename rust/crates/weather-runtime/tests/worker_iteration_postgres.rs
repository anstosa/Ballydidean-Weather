use std::cell::RefCell;
use std::collections::VecDeque;
use std::fmt::{Display, Formatter};
use std::rc::Rc;

use weather_contracts::{DueSource, JsonValue, SourceKind};
use weather_database::{
    AdmittedRevisionUtcInstant, DueSourceDiscoveryPlan, DueSourceDiscoveryRow,
    WorkerHeartbeatUpdatePlan, WorkerIterationQueryBind,
};
use weather_runtime::{
    PostgresWorkerIterationDatabase, WorkerDueSourceProjection, WorkerIterationDatabasePort,
    WorkerIterationPostgresPort, WorkerIterationTime,
};
use weather_worker::{
    HeartbeatRepository, SourceRunResult, SourceRunStatus, TaskFailure, WorkerContinuation,
    WorkerDueSource, WorkerHeartbeat, WorkerIterationRepository, WorkerScheduledSourceDispatcher,
    run_worker_iteration,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FixtureError;

impl Display for FixtureError {
    /// render one stable fixture diagnostic.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("fixture failure")
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum DatabaseEvent {
    Discovery(DueSourceDiscoveryPlan),
    Heartbeat(WorkerHeartbeatUpdatePlan),
}

struct Driver {
    events: Rc<RefCell<Vec<DatabaseEvent>>>,
    rows: Vec<DueSourceDiscoveryRow>,
}

impl WorkerIterationPostgresPort for Driver {
    type Error = FixtureError;

    /// retain the discovery plan and return rows in durable query order.
    fn query_due_sources(
        &mut self,
        plan: &DueSourceDiscoveryPlan,
    ) -> Result<Vec<DueSourceDiscoveryRow>, Self::Error> {
        self.events
            .borrow_mut()
            .push(DatabaseEvent::Discovery(plan.clone()));
        Ok(std::mem::take(&mut self.rows))
    }

    /// retain the exact heartbeat statement and admitted binds.
    fn execute_worker_heartbeat(
        &mut self,
        plan: &WorkerHeartbeatUpdatePlan,
    ) -> Result<(), Self::Error> {
        self.events
            .borrow_mut()
            .push(DatabaseEvent::Heartbeat(plan.clone()));
        Ok(())
    }
}

struct Time {
    epochs: Rc<RefCell<Vec<i64>>>,
}

impl WorkerIterationTime for Time {
    type Error = FixtureError;

    /// retain each captured epoch and provide its fixture UTC representation.
    fn utc_milliseconds(
        &mut self,
        epoch_milliseconds: i64,
    ) -> Result<AdmittedRevisionUtcInstant, Self::Error> {
        self.epochs.borrow_mut().push(epoch_milliseconds);
        Ok(AdmittedRevisionUtcInstant::admitted(format!(
            "{epoch_milliseconds}-utc"
        )))
    }
}

struct Projection;

impl WorkerDueSourceProjection for Projection {
    /// project the two ordinary non-dispatchable fixture outcomes.
    fn project_due_source(
        &mut self,
        row: &DueSourceDiscoveryRow,
    ) -> Result<Option<WorkerDueSource>, TaskFailure> {
        match row.id.as_str() {
            "unconfigured" => Ok(None),
            "invalid" => Err(TaskFailure {
                attempts: 2,
                code: "invalid_configuration".to_owned(),
            }),
            _ => unreachable!("fixture contains only non-dispatchable rows"),
        }
    }
}

struct Repository<'a, Database>(&'a mut Database);

impl<Database> HeartbeatRepository for Repository<'_, Database>
where
    Database: WorkerIterationDatabasePort,
{
    /// route one heartbeat through the concrete database adapter.
    fn update_worker_heartbeat(&mut self, heartbeat: WorkerHeartbeat) -> Result<(), TaskFailure> {
        self.0
            .update_worker_heartbeat(heartbeat)
            .map_err(|error| TaskFailure {
                attempts: 1,
                code: error.to_string(),
            })
    }
}

impl<Database> WorkerIterationRepository for Repository<'_, Database>
where
    Database: WorkerIterationDatabasePort,
{
    /// route ordered discovery through the concrete database adapter.
    fn discover_due_sources(&mut self, as_of_ms: i64) -> Result<Vec<WorkerDueSource>, TaskFailure> {
        self.0
            .discover_due_sources(as_of_ms)
            .map_err(|error| TaskFailure {
                attempts: 1,
                code: error.to_string(),
            })
    }
}

#[derive(Default)]
struct Dispatcher {
    calls: usize,
}

impl<Repository> WorkerScheduledSourceDispatcher<Repository> for Dispatcher {
    /// retain any accidental provider dispatch for a non-dispatchable row.
    fn run_source<C: FnMut() -> i64>(
        &mut self,
        _repository: &mut Repository,
        source: &WorkerDueSource,
        _success_clock: &mut C,
    ) -> Result<SourceRunResult, TaskFailure> {
        self.calls += 1;
        Err(TaskFailure {
            attempts: 1,
            code: format!("unexpected_dispatch:{}", source.source_id()),
        })
    }
}

/// build one generated due-source row with the selected durable identity.
fn row(id: &str) -> DueSource {
    DueSource {
        active: true,
        cadenceSeconds: 60,
        id: id.to_owned(),
        materialProviderConfig: JsonValue::Object(Vec::new()),
        providerKey: "provider".to_owned(),
        siteSlug: "site".to_owned(),
        sourceConfigFingerprint: format!("{id}-fingerprint"),
        sourceKey: format!("{id}-source"),
        sourceKind: SourceKind::ModelCurrent,
        stationSlug: "station".to_owned(),
        timezone: "America/Los_Angeles".to_owned(),
    }
}

/// retain both non-dispatchable rows and persist one degraded heartbeat with exact binds.
#[test]
fn postgres_iteration_preserves_row_slots_without_dispatch_and_updates_heartbeat() {
    let events = Rc::new(RefCell::new(Vec::new()));
    let epochs = Rc::new(RefCell::new(Vec::new()));
    let driver = Driver {
        events: Rc::clone(&events),
        rows: vec![row("unconfigured"), row("invalid")],
    };
    let time = Time {
        epochs: Rc::clone(&epochs),
    };
    let mut database = PostgresWorkerIterationDatabase::new(
        driver,
        time,
        Projection,
        "worker-main".to_owned(),
        "worker-v1".to_owned(),
    );
    let mut repository = Repository(&mut database);
    let mut dispatcher = Dispatcher::default();
    let mut continuation = WorkerContinuation::new(Some(500));
    let mut ticks = VecDeque::from([1_000, 2_000, 3_000, 4_000, 5_000, 6_000, 7_000]);
    let result = run_worker_iteration(
        &mut continuation,
        &mut repository,
        &mut dispatcher,
        &mut || ticks.pop_front().expect("fixture clock exhausted"),
    )
    .unwrap();

    assert_eq!(dispatcher.calls, 0);
    assert_eq!(result.completed_at_ms, 7_000);
    assert_eq!(result.last_success_at_ms, Some(500));
    assert_eq!(result.sources.len(), 2);
    assert_eq!(result.sources[0].source_id, "unconfigured");
    assert_eq!(result.sources[0].status, SourceRunStatus::Skipped);
    assert_eq!(result.sources[1].source_id, "invalid");
    assert_eq!(result.sources[1].status, SourceRunStatus::Failed);
    assert_eq!(
        result.sources[1].secondary_error_codes,
        ["invalid_configuration"]
    );
    assert_eq!(*epochs.borrow(), [2_000, 7_000, 500]);

    let observed = events.borrow();
    let DatabaseEvent::Discovery(discovery) = &observed[0] else {
        panic!("missing discovery plan")
    };
    assert_eq!(
        discovery.binds,
        [
            WorkerIterationQueryBind::UtcInstant("2000-utc".to_owned()),
            WorkerIterationQueryBind::Unsigned(100),
        ]
    );
    let DatabaseEvent::Heartbeat(heartbeat) = &observed[1] else {
        panic!("missing heartbeat plan")
    };
    assert_eq!(
        heartbeat.binds,
        [
            WorkerIterationQueryBind::Text("worker-main".to_owned()),
            WorkerIterationQueryBind::UtcInstant("7000-utc".to_owned()),
            WorkerIterationQueryBind::NullableUtcInstant(Some("500-utc".to_owned())),
            WorkerIterationQueryBind::NullableText(Some("degraded".to_owned())),
            WorkerIterationQueryBind::Text("worker-v1".to_owned()),
        ]
    );
}
