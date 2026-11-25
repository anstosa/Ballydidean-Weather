use weather_worker::{
    NoWorkerSidecars, WorkerContinuation, WorkerProcessFailure, WorkerProcessIteration,
    WorkerProcessPhase, WorkerScheduledSourceDispatcher, WorkerSidecarPhase,
    run_worker_iteration_with_phase,
};

use crate::{
    OpenMeteoRuntimeValues, WorkerIterationDatabasePort, WorkerIterationDatabaseRepository,
};

/// Retained-work drain boundary used before the process-owned pool closes.
pub trait WorkerIterationDrainPort<Database> {
    /// Wait for or cancel one admitted in-flight iteration without closing its pool.
    fn drain_iteration(&mut self, database: &mut Database);
}

/// Runtime adapter joining process startup to the ordinary database-backed drain.
pub struct DatabaseWorkerProcessIteration<
    Values,
    Dispatcher,
    Clock,
    Drain,
    Sidecars = NoWorkerSidecars,
> {
    clock: Clock,
    dispatcher: Dispatcher,
    drain: Drain,
    sidecars: Sidecars,
    values: Values,
}

impl<Values, Dispatcher, Clock, Drain>
    DatabaseWorkerProcessIteration<Values, Dispatcher, Clock, Drain, NoWorkerSidecars>
{
    /// Bind the admitted value, provider, clock, and graceful-drain boundaries.
    pub const fn new(values: Values, dispatcher: Dispatcher, clock: Clock, drain: Drain) -> Self {
        Self {
            clock,
            dispatcher,
            drain,
            sidecars: NoWorkerSidecars,
            values,
        }
    }
}

impl<Values, Dispatcher, Clock, Drain, Sidecars>
    DatabaseWorkerProcessIteration<Values, Dispatcher, Clock, Drain, Sidecars>
{
    /// Replace the current sidecar phase while retaining the same process-owned boundaries.
    pub fn with_sidecar_phase<Phase>(
        self,
        phase: Phase,
    ) -> DatabaseWorkerProcessIteration<Values, Dispatcher, Clock, Drain, Phase> {
        DatabaseWorkerProcessIteration {
            clock: self.clock,
            dispatcher: self.dispatcher,
            drain: self.drain,
            sidecars: phase,
            values: self.values,
        }
    }
}

impl<Configuration, Database, Values, Dispatcher, Clock, Drain, Sidecars>
    WorkerProcessIteration<Configuration, Database>
    for DatabaseWorkerProcessIteration<Values, Dispatcher, Clock, Drain, Sidecars>
where
    Clock: FnMut() -> i64,
    Database: WorkerIterationDatabasePort,
    for<'a> Dispatcher:
        WorkerScheduledSourceDispatcher<WorkerIterationDatabaseRepository<'a, Database, Values>>,
    Drain: WorkerIterationDrainPort<Database>,
    for<'a> Sidecars: WorkerSidecarPhase<WorkerIterationDatabaseRepository<'a, Database, Values>>,
    Values: OpenMeteoRuntimeValues,
{
    /// Execute the existing ordered worker coordinator over the retained database pool.
    fn run_iteration(
        &mut self,
        _configuration: &Configuration,
        database: &mut Database,
        continuation: &mut WorkerContinuation,
    ) -> Result<(), WorkerProcessFailure> {
        let mut repository = WorkerIterationDatabaseRepository::new(database, &mut self.values);
        run_worker_iteration_with_phase(
            continuation,
            &mut repository,
            &mut self.dispatcher,
            &mut self.sidecars,
            &mut self.clock,
        )
        .map(|_| ())
        .map_err(|failure| WorkerProcessFailure {
            code: failure.code,
            phase: WorkerProcessPhase::Iteration,
        })
    }

    /// Drain the admitted retained operation before process-owned database close.
    fn drain_iteration(&mut self, database: &mut Database) {
        self.drain.drain_iteration(database);
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::fmt::{Display, Formatter};
    use std::rc::Rc;

    use weather_contracts::{JsonValue, NormalizedWeatherRecord, ScheduledCheckpointState};
    use weather_database::{AdmittedIngestionError, SerializedJson, Sha256Digest};
    use weather_worker::{
        OPEN_METEO_CURRENT_ADAPTER_VERSION, OpenMeteoCursor, OpenMeteoScheduledSource,
        OpenMeteoSourceKind, SourceRunResult, SourceRunStatus, TaskFailure, WorkerDueSource,
        WorkerHeartbeat, WorkerProcessIterationFactory,
    };

    use super::*;
    use crate::{RuntimePersistenceOperation, RuntimeTimeValue};

    type Trace = Rc<RefCell<Vec<String>>>;

    #[derive(Clone, Copy, Debug)]
    struct FakeDatabaseError;

    impl Display for FakeDatabaseError {
        // render one bounded injected database failure
        fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("database failure")
        }
    }

    struct FakeDatabase {
        due_sources: Vec<WorkerDueSource>,
        fail_discovery: bool,
        heartbeat: Option<WorkerHeartbeat>,
        trace: Trace,
    }

    impl WorkerIterationDatabasePort for FakeDatabase {
        type Error = FakeDatabaseError;

        // return sources in the exact injected durable order
        fn discover_due_sources(
            &mut self,
            as_of_ms: i64,
        ) -> Result<Vec<WorkerDueSource>, Self::Error> {
            self.trace.borrow_mut().push(format!("discover:{as_of_ms}"));

            // expose one typed persistence failure without provider dispatch
            if self.fail_discovery {
                return Err(FakeDatabaseError);
            }
            Ok(self.due_sources.clone())
        }

        // retain the actual heartbeat produced by the worker coordinator
        fn update_worker_heartbeat(
            &mut self,
            heartbeat: WorkerHeartbeat,
        ) -> Result<(), Self::Error> {
            self.trace.borrow_mut().push(format!(
                "heartbeat:{}:{:?}",
                heartbeat.last_loop_at_ms, heartbeat.last_success_at_ms
            ));
            self.heartbeat = Some(heartbeat);
            Ok(())
        }
    }

    #[derive(Default)]
    struct FixtureValues;

    impl OpenMeteoRuntimeValues for FixtureValues {
        // reject unused time serialization in the iteration-only fixture
        fn utc_instant(
            &mut self,
            _milliseconds: i64,
            _value: RuntimeTimeValue,
        ) -> Result<String, TaskFailure> {
            unreachable!("iteration fixture does not serialize instants")
        }

        // reject unused time parsing in the iteration-only fixture
        fn epoch_milliseconds(
            &mut self,
            _instant: &str,
            _value: RuntimeTimeValue,
        ) -> Result<i64, TaskFailure> {
            unreachable!("iteration fixture does not parse instants")
        }

        // reject unused JSON serialization in the iteration-only fixture
        fn serialize_json_object(
            &mut self,
            _value: &[(Vec<u16>, JsonValue)],
        ) -> Result<SerializedJson, TaskFailure> {
            unreachable!("iteration fixture does not serialize JSON")
        }

        // reject unused request evidence in the iteration-only fixture
        fn request_metadata_json(
            &mut self,
            _endpoint: &str,
        ) -> Result<SerializedJson, TaskFailure> {
            unreachable!("iteration fixture does not serialize requests")
        }

        // reject unused cursor serialization in the iteration-only fixture
        fn provider_cursor_json(
            &mut self,
            _cursor: Option<&OpenMeteoCursor>,
        ) -> Result<Option<SerializedJson>, TaskFailure> {
            unreachable!("iteration fixture does not serialize cursors")
        }

        // reject unused checkpoint parsing in the iteration-only fixture
        fn checkpoint_cursor(
            &mut self,
            _checkpoint: &ScheduledCheckpointState,
        ) -> Result<Option<OpenMeteoCursor>, TaskFailure> {
            unreachable!("iteration fixture does not parse checkpoints")
        }

        // reject unused record hashing in the iteration-only fixture
        fn weather_record_content_hash(
            &mut self,
            _record: &NormalizedWeatherRecord,
        ) -> Result<Sha256Digest, TaskFailure> {
            unreachable!("iteration fixture does not hash records")
        }

        // reject unused digest admission in the iteration-only fixture
        fn sha256_digest(&mut self, _value: &str) -> Result<Sha256Digest, TaskFailure> {
            unreachable!("iteration fixture does not admit digests")
        }

        // reject unused ingestion classification in the iteration-only fixture
        fn ingestion_error(
            &mut self,
            _failure: &TaskFailure,
        ) -> Result<AdmittedIngestionError, TaskFailure> {
            unreachable!("iteration fixture does not classify ingestion")
        }

        // classify the one persistence boundary used by the fixture
        fn persistence_failure(
            &mut self,
            _operation: RuntimePersistenceOperation,
            _message: &str,
        ) -> TaskFailure {
            TaskFailure {
                attempts: 1,
                code: "iteration_database_failed".to_owned(),
            }
        }
    }

    struct FakeDispatcher {
        trace: Trace,
    }

    impl<Repository> WorkerScheduledSourceDispatcher<Repository> for FakeDispatcher {
        // capture the actual successful provider completion clock
        fn run_source<C: FnMut() -> i64>(
            &mut self,
            _repository: &mut Repository,
            source: &WorkerDueSource,
            success_clock: &mut C,
        ) -> Result<SourceRunResult, TaskFailure> {
            self.trace
                .borrow_mut()
                .push(format!("dispatch:{}", source.source_id()));
            Ok(SourceRunResult {
                attempts: 1,
                reason: None,
                record_count: 3,
                run_id: Some("run-1".to_owned()),
                secondary_error_codes: Vec::new(),
                source_id: source.source_id().to_owned(),
                status: SourceRunStatus::Succeeded,
                successful_at_ms: Some(success_clock()),
            })
        }
    }

    struct FakeSidecarPhase {
        trace: Trace,
    }

    impl<'a> WorkerSidecarPhase<WorkerIterationDatabaseRepository<'a, FakeDatabase, FixtureValues>>
        for FakeSidecarPhase
    {
        /// Reuse the retained repository and the iteration's loop and sidecar clocks.
        fn run<C: FnMut() -> i64>(
            &mut self,
            repository: &mut WorkerIterationDatabaseRepository<'a, FakeDatabase, FixtureValues>,
            loop_at_ms: i64,
            clock: &mut C,
        ) {
            self.trace
                .borrow_mut()
                .push(format!("sidecar-loop:{loop_at_ms}"));
            // use the same scoped pool as a configured task would
            repository.with_retained_resources(|database, _values| {
                database.trace.borrow_mut().push(format!(
                    "sidecar-source:{}",
                    database.due_sources[0].source_id()
                ));
            });
            let completed_at_ms = clock();
            self.trace
                .borrow_mut()
                .push(format!("sidecar-clock:{completed_at_ms}"));
        }
    }

    struct FakeDrain {
        trace: Trace,
    }

    impl WorkerIterationDrainPort<FakeDatabase> for FakeDrain {
        // retain shutdown drain evidence before pool close
        fn drain_iteration(&mut self, _database: &mut FakeDatabase) {
            self.trace.borrow_mut().push("drain".to_owned());
        }
    }

    // construct one admitted current source for the ordinary dispatcher
    fn due_source() -> WorkerDueSource {
        WorkerDueSource::OpenMeteo(OpenMeteoScheduledSource {
            active: true,
            adapter_version: OPEN_METEO_CURRENT_ADAPTER_VERSION,
            cadence_seconds: 60,
            id: "source-current".to_owned(),
            material_config_identity: "identity-current".to_owned(),
            provider_key: "open-meteo".to_owned(),
            site_slug: "site-a".to_owned(),
            source_config_fingerprint: "fingerprint-current".to_owned(),
            source_key: "current".to_owned(),
            source_kind: OpenMeteoSourceKind::ModelCurrent,
            station_slug: "station-a".to_owned(),
            timezone: "America/Los_Angeles".to_owned(),
        })
    }

    // execute the real ordinary drain through the process adapter
    #[test]
    fn database_process_iteration_runs_dispatch_and_heartbeat_in_order() {
        let trace = Rc::new(RefCell::new(Vec::new()));
        let mut database = FakeDatabase {
            due_sources: vec![due_source()],
            fail_discovery: false,
            heartbeat: None,
            trace: Rc::clone(&trace),
        };
        let mut ticks = VecDeque::from([100, 110, 120, 130]);
        let clock = move || ticks.pop_front().expect("fixture clock should have a tick");
        let mut iteration = DatabaseWorkerProcessIteration::new(
            FixtureValues,
            FakeDispatcher {
                trace: Rc::clone(&trace),
            },
            clock,
            FakeDrain {
                trace: Rc::clone(&trace),
            },
        );
        let mut continuation = WorkerContinuation::new(Some(50));

        WorkerProcessIteration::run_iteration(
            &mut iteration,
            &(),
            &mut database,
            &mut continuation,
        )
        .expect("ordinary database iteration should complete");
        WorkerProcessIteration::<(), FakeDatabase>::drain_iteration(&mut iteration, &mut database);

        assert_eq!(continuation.last_success_at_ms(), Some(120));
        assert_eq!(
            database.heartbeat,
            Some(WorkerHeartbeat {
                activity: None,
                last_loop_at_ms: 130,
                last_success_at_ms: Some(120),
            })
        );
        assert_eq!(
            trace.borrow().as_slice(),
            [
                "discover:110",
                "dispatch:source-current",
                "heartbeat:130:Some(120)",
                "drain",
            ]
        );
    }

    // execute one sidecar phase on the retained repository before heartbeat persistence
    #[test]
    fn database_process_iteration_runs_sidecars_before_heartbeat() {
        let trace = Rc::new(RefCell::new(Vec::new()));
        let mut database = FakeDatabase {
            due_sources: vec![due_source()],
            fail_discovery: false,
            heartbeat: None,
            trace: Rc::clone(&trace),
        };
        let mut ticks = VecDeque::from([100, 110, 120, 130, 140]);
        let clock = move || ticks.pop_front().expect("fixture clock should have a tick");
        let mut iteration = crate::DatabaseWorkerProcessIterationFactory::new(
            FixtureValues,
            clock,
            FakeDrain {
                trace: Rc::clone(&trace),
            },
        )
        .create_iteration(
            &(),
            &mut database,
            crate::AdmittedDatabaseWorkerIteration {
                dispatcher: FakeDispatcher {
                    trace: Rc::clone(&trace),
                },
                sidecar_phase: FakeSidecarPhase {
                    trace: Rc::clone(&trace),
                },
            },
        )
        .expect("admitted sidecar iteration should construct");
        let mut continuation = WorkerContinuation::new(Some(50));

        WorkerProcessIteration::run_iteration(
            &mut iteration,
            &(),
            &mut database,
            &mut continuation,
        )
        .expect("sidecar-aware database iteration should complete");

        assert_eq!(continuation.last_success_at_ms(), Some(120));
        assert_eq!(
            database.heartbeat,
            Some(WorkerHeartbeat {
                activity: None,
                last_loop_at_ms: 140,
                last_success_at_ms: Some(120),
            })
        );
        assert_eq!(
            trace.borrow().as_slice(),
            [
                "discover:110",
                "dispatch:source-current",
                "sidecar-loop:110",
                "sidecar-source:source-current",
                "sidecar-clock:130",
                "heartbeat:140:Some(120)",
            ]
        );
    }

    // retain the typed process phase when database discovery fails
    #[test]
    fn database_process_iteration_maps_repository_failure() {
        let trace = Rc::new(RefCell::new(Vec::new()));
        let mut database = FakeDatabase {
            due_sources: Vec::new(),
            fail_discovery: true,
            heartbeat: None,
            trace: Rc::clone(&trace),
        };
        let mut ticks = VecDeque::from([100, 110]);
        let clock = move || ticks.pop_front().expect("fixture clock should have a tick");
        let mut iteration = DatabaseWorkerProcessIteration::new(
            FixtureValues,
            FakeDispatcher {
                trace: Rc::clone(&trace),
            },
            clock,
            FakeDrain {
                trace: Rc::clone(&trace),
            },
        );
        let mut continuation = WorkerContinuation::new(None);

        assert_eq!(
            WorkerProcessIteration::run_iteration(
                &mut iteration,
                &(),
                &mut database,
                &mut continuation,
            ),
            Err(WorkerProcessFailure {
                code: "iteration_database_failed".to_owned(),
                phase: WorkerProcessPhase::Iteration,
            })
        );
        assert_eq!(trace.borrow().as_slice(), ["discover:110"]);
        assert_eq!(database.heartbeat, None);
    }
}
