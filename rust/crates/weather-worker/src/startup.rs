use std::cell::RefCell;
use std::error::Error;
use std::fmt::{Display, Formatter};

use crate::{NonOverlappingGate, WorkerContinuation, WorkerHealth};

/// Production worker scheduler cadence copied from the Node entrypoint.
pub const WORKER_PROCESS_CADENCE_MS: u64 = 60_000;

/// One parsed process invocation.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct WorkerProcessOptions {
    pub once: bool,
}

/// One optional startup workload admitted by the configuration boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AdmittedWorkerStartupTask {
    Inactive,
    Active,
}

/// One rejected command-line invocation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WorkerProcessArgumentsError;

impl Display for WorkerProcessArgumentsError {
    /// Render the exact source entrypoint diagnostic.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("worker supports only the optional --once flag")
    }
}

impl Error for WorkerProcessArgumentsError {}

/// Startup or retained-process phase owning one bounded failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkerProcessPhase {
    Configuration,
    Pool,
    Runtime,
    Readiness,
    DurableHealth,
    Scheduler,
    Iteration,
    Close,
}

/// One typed process failure without transport-specific error material.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkerProcessFailure {
    pub code: String,
    pub phase: WorkerProcessPhase,
}

impl Display for WorkerProcessFailure {
    /// Render the bounded owning phase and code.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{:?}: {}", self.phase, self.code)
    }
}

impl Error for WorkerProcessFailure {}

/// Configuration fields consumed directly by process orchestration.
pub trait AdmittedWorkerProcessConfiguration {
    /// Return the durable scheduler key.
    fn instance(&self) -> &str;

    /// Report whether maintenance loading was explicitly admitted.
    fn startup_maintenance(&self) -> AdmittedWorkerStartupTask;

    /// Report whether startup backfill was explicitly admitted.
    fn startup_backfill(&self) -> AdmittedWorkerStartupTask;
}

/// Typed environment/configuration boundary.
pub trait WorkerProcessEnvironmentPort {
    type Configuration: AdmittedWorkerProcessConfiguration;

    /// Load and validate the complete worker configuration.
    fn load_configuration(&mut self) -> Result<Self::Configuration, WorkerProcessFailure>;
}

/// Typed database-pool construction boundary.
pub trait WorkerProcessPoolFactory<Configuration> {
    type Pool: WorkerProcessPool<Configuration>;

    /// Create the retained pool after configuration admission.
    fn create_pool(
        &mut self,
        configuration: &Configuration,
    ) -> Result<Self::Pool, WorkerProcessFailure>;
}

/// Database lifecycle and readiness operations owned by the process pool.
pub trait WorkerProcessPool<Configuration> {
    /// Verify the database contract before reading durable heartbeat state.
    fn assert_readiness(
        &mut self,
        configuration: &Configuration,
    ) -> Result<(), WorkerProcessFailure>;

    /// Read the last durable worker heartbeat after readiness succeeds.
    fn read_durable_health(
        &mut self,
        configuration: &Configuration,
    ) -> Result<WorkerHealth, WorkerProcessFailure>;

    /// Close the retained pool exactly once at process termination.
    fn close(&mut self) -> Result<(), WorkerProcessFailure>;
}

/// Required and admitted optional runtime loader boundary.
pub trait WorkerProcessRuntimeLoader<Configuration, Pool> {
    type Runtime;

    /// Load required provider and adjustment runtimes before readiness.
    fn load_required_runtime(
        &mut self,
        configuration: &Configuration,
        pool: &mut Pool,
    ) -> Result<Self::Runtime, WorkerProcessFailure>;

    /// Load optional maintenance material already admitted by configuration.
    fn load_startup_maintenance(
        &mut self,
        configuration: &Configuration,
        pool: &mut Pool,
        runtime: &mut Self::Runtime,
    );
}

/// Exact normal-mode scheduler registration.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkerProcessSchedule {
    pub cadence_ms: u64,
    pub key: String,
}

/// Timer owner used by the retained normal process.
pub trait WorkerProcessSchedulerPort {
    /// Arm normal polling before the immediate initial trigger.
    fn start(&mut self, schedule: WorkerProcessSchedule) -> Result<(), WorkerProcessFailure>;

    /// Stop future polls during graceful shutdown.
    fn stop(&mut self);

    /// Arm the next anchored slot after an elapsed timer is delivered.
    fn rearm_after_poll(&mut self) -> Result<(), WorkerProcessFailure>;

    /// Record one contained timer-trigger iteration failure.
    fn record_iteration_error(&mut self, failure: &WorkerProcessFailure);
}

/// Optional startup backfill owner.
pub trait WorkerProcessBackfillPort<Configuration, Pool> {
    type Handle;

    /// Start one admitted backfill after the immediate normal iteration.
    fn start(&mut self, configuration: &Configuration, pool: &mut Pool) -> Self::Handle;

    /// Drain one active backfill before the pool closes.
    fn drain(&mut self, handle: Self::Handle, pool: &mut Pool);
}

/// Actual ordinary iteration execution over retained process state.
pub trait WorkerProcessIteration<Configuration, Pool> {
    /// Run one current worker iteration and durable heartbeat update.
    fn run_iteration(
        &mut self,
        configuration: &Configuration,
        pool: &mut Pool,
        continuation: &mut WorkerContinuation,
    ) -> Result<(), WorkerProcessFailure>;

    /// Drain an active iteration before closing the pool.
    fn drain_iteration(&mut self, pool: &mut Pool);
}

/// Factory joining loaded runtimes to the retained iteration after durable health admission.
pub trait WorkerProcessIterationFactory<Configuration, Pool, Runtime> {
    type Iteration: WorkerProcessIteration<Configuration, Pool>;

    /// Create the retained iteration runner from the exact loaded runtime.
    fn create_iteration(
        self,
        configuration: &Configuration,
        pool: &mut Pool,
        runtime: Runtime,
    ) -> Result<Self::Iteration, WorkerProcessFailure>;
}

/// Owned process ports consumed by startup.
pub struct WorkerProcessPorts<Environment, Factory, Loaders, Scheduler, Backfill, IterationFactory>
{
    pub backfill: Backfill,
    pub environment: Environment,
    pub iteration_factory: IterationFactory,
    pub loaders: Loaders,
    pub pool_factory: Factory,
    pub scheduler: Scheduler,
}

/// One non-overlapping trigger disposition.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkerProcessTrigger {
    Busy,
    Completed,
}

/// One timer poll disposition after failure containment.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkerProcessPoll {
    Busy,
    Completed,
    Failed,
}

/// Startup returns either a closed compatibility run or a retained normal process.
pub enum WorkerProcessStartResult<Running> {
    Once(WorkerProcessTrigger),
    Running(Running),
}

/// Retained process type after its runtime has produced an iteration runner.
pub type RetainedWorkerProcess<
    Configuration,
    Pool,
    Scheduler,
    Backfill,
    IterationFactory,
    Runtime,
> = RunningWorkerProcess<
    Configuration,
    Pool,
    Scheduler,
    Backfill,
    <IterationFactory as WorkerProcessIterationFactory<Configuration, Pool, Runtime>>::Iteration,
    <Backfill as WorkerProcessBackfillPort<Configuration, Pool>>::Handle,
>;

/// Bounded startup result shared by one-shot and retained worker modes.
pub type WorkerProcessStartupResult<
    Configuration,
    Pool,
    Scheduler,
    Backfill,
    IterationFactory,
    Runtime,
> = Result<
    WorkerProcessStartResult<
        RetainedWorkerProcess<Configuration, Pool, Scheduler, Backfill, IterationFactory, Runtime>,
    >,
    WorkerProcessFailure,
>;

/// Retained normal process state guarded independently from the overlap gate.
struct RunningWorkerProcessState<Configuration, Pool, Scheduler, Backfill, Iteration, Handle> {
    backfill: Backfill,
    backfill_handle: Option<Handle>,
    closed: bool,
    configuration: Configuration,
    continuation: WorkerContinuation,
    initial_trigger: WorkerProcessTrigger,
    iteration: Iteration,
    pool: Pool,
    scheduler: Scheduler,
}

/// One retained normal-mode worker process.
pub struct RunningWorkerProcess<Configuration, Pool, Scheduler, Backfill, Iteration, Handle> {
    gate: NonOverlappingGate,
    state: RefCell<
        RunningWorkerProcessState<Configuration, Pool, Scheduler, Backfill, Iteration, Handle>,
    >,
}

impl<Configuration, Pool, Scheduler, Backfill, Iteration, Handle>
    RunningWorkerProcess<Configuration, Pool, Scheduler, Backfill, Iteration, Handle>
where
    Backfill: WorkerProcessBackfillPort<Configuration, Pool, Handle = Handle>,
    Iteration: WorkerProcessIteration<Configuration, Pool>,
    Pool: WorkerProcessPool<Configuration>,
    Scheduler: WorkerProcessSchedulerPort,
{
    /// Report the immediate trigger completed during startup.
    pub fn initial_trigger(&self) -> WorkerProcessTrigger {
        self.state.borrow().initial_trigger
    }

    /// Expose the process-owned overlap state for scheduler integration.
    pub const fn non_overlapping_gate(&self) -> &NonOverlappingGate {
        &self.gate
    }

    /// Execute one manual trigger only while the process is idle.
    pub fn trigger(&self) -> Result<WorkerProcessTrigger, WorkerProcessFailure> {
        // skip overlapping execution without touching retained process state
        let Some(_permit) = self.gate.try_enter() else {
            return Ok(WorkerProcessTrigger::Busy);
        };
        let mut state = self.state.borrow_mut();

        // reject triggers after the pool was closed
        if state.closed {
            return Err(WorkerProcessFailure {
                code: "worker_process_closed".to_owned(),
                phase: WorkerProcessPhase::Iteration,
            });
        }
        let RunningWorkerProcessState {
            configuration,
            continuation,
            iteration,
            pool,
            ..
        } = &mut *state;
        iteration.run_iteration(configuration, pool, continuation)?;
        Ok(WorkerProcessTrigger::Completed)
    }

    /// Execute one timer poll, contain iteration failure, and rearm polling.
    pub fn poll(&self) -> Result<WorkerProcessPoll, WorkerProcessFailure> {
        let trigger = self.trigger();
        let mut state = self.state.borrow_mut();
        let outcome = match trigger {
            Ok(WorkerProcessTrigger::Busy) => WorkerProcessPoll::Busy,
            Ok(WorkerProcessTrigger::Completed) => WorkerProcessPoll::Completed,
            Err(error) => {
                state.scheduler.record_iteration_error(&error);
                WorkerProcessPoll::Failed
            }
        };
        state.scheduler.rearm_after_poll()?;
        Ok(outcome)
    }

    /// Stop polling, drain admitted work, and close the pool in order.
    pub fn shutdown(&self) -> Result<(), WorkerProcessFailure> {
        let mut state = self.state.borrow_mut();

        // keep repeated signal handling idempotent after the first close
        if state.closed {
            return Ok(());
        }
        state.scheduler.stop();
        let RunningWorkerProcessState {
            backfill,
            backfill_handle,
            iteration,
            pool,
            ..
        } = &mut *state;

        // drain the admitted startup backfill before database close
        if let Some(handle) = backfill_handle.take() {
            backfill.drain(handle, pool);
        }
        // drain an active iteration before database close
        if self.gate.is_running() {
            iteration.drain_iteration(pool);
        }
        pool.close()?;
        state.closed = true;
        Ok(())
    }
}

/// Parse the source entrypoint's optional `--once` flag.
pub fn parse_worker_process_arguments(
    arguments: &[String],
) -> Result<WorkerProcessOptions, WorkerProcessArgumentsError> {
    // reject the first undocumented process control
    if arguments.iter().any(|argument| argument != "--once") {
        return Err(WorkerProcessArgumentsError);
    }
    Ok(WorkerProcessOptions {
        once: arguments.iter().any(|argument| argument == "--once"),
    })
}

/// Load, verify, and start one worker process using only typed injected ports.
pub fn start_worker_process<Environment, Factory, Loaders, Scheduler, Backfill, IterationFactory>(
    options: WorkerProcessOptions,
    ports: WorkerProcessPorts<Environment, Factory, Loaders, Scheduler, Backfill, IterationFactory>,
) -> WorkerProcessStartupResult<
    Environment::Configuration,
    Factory::Pool,
    Scheduler,
    Backfill,
    IterationFactory,
    Loaders::Runtime,
>
where
    Backfill: WorkerProcessBackfillPort<Environment::Configuration, Factory::Pool>,
    Environment: WorkerProcessEnvironmentPort,
    Factory: WorkerProcessPoolFactory<Environment::Configuration>,
    IterationFactory:
        WorkerProcessIterationFactory<Environment::Configuration, Factory::Pool, Loaders::Runtime>,
    Loaders: WorkerProcessRuntimeLoader<Environment::Configuration, Factory::Pool>,
    Scheduler: WorkerProcessSchedulerPort,
{
    let WorkerProcessPorts {
        backfill,
        mut environment,
        iteration_factory,
        mut loaders,
        mut pool_factory,
        scheduler,
    } = ports;
    let configuration = environment.load_configuration()?;
    let mut pool = pool_factory.create_pool(&configuration)?;
    let mut runtime = loaders.load_required_runtime(&configuration, &mut pool)?;

    // load optional maintenance only after explicit configuration admission
    if configuration.startup_maintenance() == AdmittedWorkerStartupTask::Active {
        loaders.load_startup_maintenance(&configuration, &mut pool, &mut runtime);
    }
    pool.assert_readiness(&configuration)?;
    let durable_health = pool.read_durable_health(&configuration)?;
    let continuation = WorkerContinuation::new(durable_health.last_success_at_ms);
    let iteration = iteration_factory.create_iteration(&configuration, &mut pool, runtime)?;
    let process = RunningWorkerProcess {
        gate: NonOverlappingGate::new(),
        state: RefCell::new(RunningWorkerProcessState {
            backfill,
            backfill_handle: None,
            closed: false,
            configuration,
            continuation,
            initial_trigger: WorkerProcessTrigger::Busy,
            iteration,
            pool,
            scheduler,
        }),
    };

    // execute one compatibility iteration without arming timers or backfill
    if options.once {
        let trigger = process.trigger()?;
        process.state.borrow_mut().pool.close()?;
        process.state.borrow_mut().closed = true;
        return Ok(WorkerProcessStartResult::Once(trigger));
    }

    {
        let mut state = process.state.borrow_mut();
        let schedule = WorkerProcessSchedule {
            cadence_ms: WORKER_PROCESS_CADENCE_MS,
            key: state.configuration.instance().to_owned(),
        };
        state.scheduler.start(schedule)?;
    }
    let initial_trigger = process.trigger()?;
    {
        let mut state = process.state.borrow_mut();
        state.initial_trigger = initial_trigger;

        // start backfill only after the normal initial iteration
        if state.configuration.startup_backfill() == AdmittedWorkerStartupTask::Active {
            let RunningWorkerProcessState {
                backfill,
                backfill_handle,
                configuration,
                pool,
                ..
            } = &mut *state;
            *backfill_handle = Some(backfill.start(configuration, pool));
        }
    }
    Ok(WorkerProcessStartResult::Running(process))
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::path::PathBuf;
    use std::process::Command;
    use std::rc::Rc;

    use super::*;

    type Trace = Rc<RefCell<Vec<String>>>;

    #[derive(Clone)]
    struct FakeConfiguration {
        backfill: AdmittedWorkerStartupTask,
        instance: String,
        maintenance: AdmittedWorkerStartupTask,
    }

    impl AdmittedWorkerProcessConfiguration for FakeConfiguration {
        // retain the scheduler identity admitted by configuration
        fn instance(&self) -> &str {
            &self.instance
        }

        // expose the admitted maintenance state
        fn startup_maintenance(&self) -> AdmittedWorkerStartupTask {
            self.maintenance
        }

        // expose the admitted backfill state
        fn startup_backfill(&self) -> AdmittedWorkerStartupTask {
            self.backfill
        }
    }

    struct FakeEnvironment {
        configuration: FakeConfiguration,
        trace: Trace,
    }

    impl WorkerProcessEnvironmentPort for FakeEnvironment {
        type Configuration = FakeConfiguration;

        // record configuration admission before every other startup phase
        fn load_configuration(&mut self) -> Result<Self::Configuration, WorkerProcessFailure> {
            self.trace.borrow_mut().push("configuration".to_owned());
            Ok(self.configuration.clone())
        }
    }

    struct FakePool {
        durable_success_at_ms: Option<i64>,
        trace: Trace,
    }

    impl WorkerProcessPool<FakeConfiguration> for FakePool {
        // record readiness before durable heartbeat access
        fn assert_readiness(
            &mut self,
            _configuration: &FakeConfiguration,
        ) -> Result<(), WorkerProcessFailure> {
            self.trace.borrow_mut().push("readiness".to_owned());
            Ok(())
        }

        // return the admitted durable continuation state
        fn read_durable_health(
            &mut self,
            _configuration: &FakeConfiguration,
        ) -> Result<WorkerHealth, WorkerProcessFailure> {
            self.trace.borrow_mut().push("durable-health".to_owned());
            Ok(WorkerHealth {
                last_loop_at_ms: Some(41),
                last_success_at_ms: self.durable_success_at_ms,
                live: true,
                ready: true,
                stale: false,
            })
        }

        // record the terminal pool close
        fn close(&mut self) -> Result<(), WorkerProcessFailure> {
            self.trace.borrow_mut().push("close".to_owned());
            Ok(())
        }
    }

    struct FakePoolFactory {
        durable_success_at_ms: Option<i64>,
        trace: Trace,
    }

    impl WorkerProcessPoolFactory<FakeConfiguration> for FakePoolFactory {
        type Pool = FakePool;

        // construct one retained fake pool
        fn create_pool(
            &mut self,
            _configuration: &FakeConfiguration,
        ) -> Result<Self::Pool, WorkerProcessFailure> {
            self.trace.borrow_mut().push("pool".to_owned());
            Ok(FakePool {
                durable_success_at_ms: self.durable_success_at_ms,
                trace: Rc::clone(&self.trace),
            })
        }
    }

    struct FakeLoaders {
        trace: Trace,
    }

    struct FakeRuntime;

    impl WorkerProcessRuntimeLoader<FakeConfiguration, FakePool> for FakeLoaders {
        type Runtime = FakeRuntime;

        // record required runtime loading before readiness
        fn load_required_runtime(
            &mut self,
            _configuration: &FakeConfiguration,
            _pool: &mut FakePool,
        ) -> Result<Self::Runtime, WorkerProcessFailure> {
            self.trace.borrow_mut().push("required-runtime".to_owned());
            Ok(FakeRuntime)
        }

        // record admitted optional maintenance loading
        fn load_startup_maintenance(
            &mut self,
            _configuration: &FakeConfiguration,
            _pool: &mut FakePool,
            _runtime: &mut Self::Runtime,
        ) {
            self.trace.borrow_mut().push("maintenance".to_owned());
        }
    }

    struct FakeScheduler {
        trace: Trace,
    }

    impl WorkerProcessSchedulerPort for FakeScheduler {
        // record the production cadence and durable key
        fn start(&mut self, schedule: WorkerProcessSchedule) -> Result<(), WorkerProcessFailure> {
            self.trace.borrow_mut().push(format!(
                "scheduler-start:{}:{}",
                schedule.key, schedule.cadence_ms
            ));
            Ok(())
        }

        // record scheduler shutdown before draining work
        fn stop(&mut self) {
            self.trace.borrow_mut().push("scheduler-stop".to_owned());
        }

        // retain the next timer after delivery regardless of iteration outcome
        fn rearm_after_poll(&mut self) -> Result<(), WorkerProcessFailure> {
            self.trace.borrow_mut().push("scheduler-rearm".to_owned());
            Ok(())
        }

        // retain contained timer failures without stopping later polls
        fn record_iteration_error(&mut self, failure: &WorkerProcessFailure) {
            self.trace
                .borrow_mut()
                .push(format!("scheduler-error:{}", failure.code));
        }
    }

    struct FakeBackfill {
        trace: Trace,
    }

    impl WorkerProcessBackfillPort<FakeConfiguration, FakePool> for FakeBackfill {
        type Handle = &'static str;

        // start one admitted post-trigger backfill
        fn start(
            &mut self,
            _configuration: &FakeConfiguration,
            _pool: &mut FakePool,
        ) -> Self::Handle {
            self.trace.borrow_mut().push("backfill-start".to_owned());
            "active-backfill"
        }

        // drain the retained backfill handle before close
        fn drain(&mut self, handle: Self::Handle, _pool: &mut FakePool) {
            self.trace
                .borrow_mut()
                .push(format!("backfill-drain:{handle}"));
        }
    }

    struct FakeIteration {
        fail_after: usize,
        runs: usize,
        trace: Trace,
    }

    struct FakeIterationFactory {
        fail_after: usize,
        trace: Trace,
    }

    impl WorkerProcessIterationFactory<FakeConfiguration, FakePool, FakeRuntime>
        for FakeIterationFactory
    {
        type Iteration = FakeIteration;

        // create the retained runner only after durable health is available
        fn create_iteration(
            self,
            _configuration: &FakeConfiguration,
            _pool: &mut FakePool,
            _runtime: FakeRuntime,
        ) -> Result<Self::Iteration, WorkerProcessFailure> {
            self.trace.borrow_mut().push("iteration-runner".to_owned());
            Ok(FakeIteration {
                fail_after: self.fail_after,
                runs: 0,
                trace: self.trace,
            })
        }
    }

    impl WorkerProcessIteration<FakeConfiguration, FakePool> for FakeIteration {
        // retain the durable success timestamp and support one contained failure
        fn run_iteration(
            &mut self,
            _configuration: &FakeConfiguration,
            _pool: &mut FakePool,
            continuation: &mut WorkerContinuation,
        ) -> Result<(), WorkerProcessFailure> {
            self.runs += 1;
            self.trace.borrow_mut().push(format!(
                "iteration:{}:{:?}",
                self.runs,
                continuation.last_success_at_ms()
            ));

            // fail only after the configured number of successful calls
            if self.runs > self.fail_after {
                return Err(WorkerProcessFailure {
                    code: "iteration_failed".to_owned(),
                    phase: WorkerProcessPhase::Iteration,
                });
            }
            Ok(())
        }

        // record an active iteration drain during shutdown
        fn drain_iteration(&mut self, _pool: &mut FakePool) {
            self.trace.borrow_mut().push("iteration-drain".to_owned());
        }
    }

    type FakeRunning = RunningWorkerProcess<
        FakeConfiguration,
        FakePool,
        FakeScheduler,
        FakeBackfill,
        FakeIteration,
        &'static str,
    >;

    // build one complete injected process port set
    fn fake_ports(
        trace: &Trace,
        maintenance: AdmittedWorkerStartupTask,
        backfill: AdmittedWorkerStartupTask,
        fail_after: usize,
    ) -> WorkerProcessPorts<
        FakeEnvironment,
        FakePoolFactory,
        FakeLoaders,
        FakeScheduler,
        FakeBackfill,
        FakeIterationFactory,
    > {
        WorkerProcessPorts {
            backfill: FakeBackfill {
                trace: Rc::clone(trace),
            },
            environment: FakeEnvironment {
                configuration: FakeConfiguration {
                    backfill,
                    instance: "worker-a".to_owned(),
                    maintenance,
                },
                trace: Rc::clone(trace),
            },
            iteration_factory: FakeIterationFactory {
                fail_after,
                trace: Rc::clone(trace),
            },
            loaders: FakeLoaders {
                trace: Rc::clone(trace),
            },
            pool_factory: FakePoolFactory {
                durable_success_at_ms: Some(37),
                trace: Rc::clone(trace),
            },
            scheduler: FakeScheduler {
                trace: Rc::clone(trace),
            },
        }
    }

    // match the source command-line admission
    #[test]
    fn arguments_accept_only_the_optional_once_flag() {
        assert_eq!(
            parse_worker_process_arguments(&[]),
            Ok(WorkerProcessOptions { once: false })
        );
        assert_eq!(
            parse_worker_process_arguments(&["--once".to_owned(), "--once".to_owned()]),
            Ok(WorkerProcessOptions { once: true })
        );
        assert_eq!(
            parse_worker_process_arguments(&["--watch".to_owned()]),
            Err(WorkerProcessArgumentsError)
        );
    }

    // preserve one-shot startup ordering without retained workloads
    #[test]
    fn once_loads_readiness_and_health_then_runs_and_closes() {
        let trace = Rc::new(RefCell::new(Vec::new()));
        let result = start_worker_process(
            WorkerProcessOptions { once: true },
            fake_ports(
                &trace,
                AdmittedWorkerStartupTask::Active,
                AdmittedWorkerStartupTask::Active,
                1,
            ),
        )
        .expect("one-shot worker should complete");

        assert!(matches!(
            result,
            WorkerProcessStartResult::Once(WorkerProcessTrigger::Completed)
        ));
        assert_eq!(
            trace.borrow().as_slice(),
            [
                "configuration",
                "pool",
                "required-runtime",
                "maintenance",
                "readiness",
                "durable-health",
                "iteration-runner",
                "iteration:1:Some(37)",
                "close",
            ]
        );
    }

    // preserve normal timer, immediate trigger, and admitted backfill order
    #[test]
    fn normal_start_arms_scheduler_then_triggers_before_backfill() {
        let trace = Rc::new(RefCell::new(Vec::new()));
        let result = start_worker_process(
            WorkerProcessOptions { once: false },
            fake_ports(
                &trace,
                AdmittedWorkerStartupTask::Inactive,
                AdmittedWorkerStartupTask::Active,
                2,
            ),
        )
        .expect("normal worker should start");
        let WorkerProcessStartResult::Running(process): WorkerProcessStartResult<FakeRunning> =
            result
        else {
            panic!("normal worker should remain running");
        };

        assert_eq!(process.initial_trigger(), WorkerProcessTrigger::Completed);
        assert_eq!(process.poll(), Ok(WorkerProcessPoll::Completed));
        assert_eq!(process.poll(), Ok(WorkerProcessPoll::Failed));
        assert_eq!(
            trace.borrow().as_slice(),
            [
                "configuration",
                "pool",
                "required-runtime",
                "readiness",
                "durable-health",
                "iteration-runner",
                "scheduler-start:worker-a:60000",
                "iteration:1:Some(37)",
                "backfill-start",
                "iteration:2:Some(37)",
                "scheduler-rearm",
                "iteration:3:Some(37)",
                "scheduler-error:iteration_failed",
                "scheduler-rearm",
            ]
        );
    }

    /// Keep explicit trigger errors out of background diagnostics and timer rearming.
    #[test]
    fn immediate_trigger_failure_propagates_without_scheduling_another_slot() {
        let trace = Rc::new(RefCell::new(Vec::new()));
        let result = start_worker_process(
            WorkerProcessOptions { once: false },
            fake_ports(
                &trace,
                AdmittedWorkerStartupTask::Inactive,
                AdmittedWorkerStartupTask::Active,
                0,
            ),
        );
        assert_eq!(
            result.err(),
            Some(WorkerProcessFailure {
                code: "iteration_failed".to_owned(),
                phase: WorkerProcessPhase::Iteration,
            })
        );
        assert_eq!(
            trace.borrow().as_slice(),
            [
                "configuration",
                "pool",
                "required-runtime",
                "readiness",
                "durable-health",
                "iteration-runner",
                "scheduler-start:worker-a:60000",
                "iteration:1:Some(37)",
            ]
        );
    }

    // preserve non-overlap and graceful drain ordering
    #[test]
    fn shutdown_drains_admitted_work_before_close() {
        let trace = Rc::new(RefCell::new(Vec::new()));
        let result = start_worker_process(
            WorkerProcessOptions { once: false },
            fake_ports(
                &trace,
                AdmittedWorkerStartupTask::Inactive,
                AdmittedWorkerStartupTask::Active,
                usize::MAX,
            ),
        )
        .expect("normal worker should start");
        let WorkerProcessStartResult::Running(process): WorkerProcessStartResult<FakeRunning> =
            result
        else {
            panic!("normal worker should remain running");
        };
        let permit = process
            .non_overlapping_gate()
            .try_enter()
            .expect("test should retain one active iteration");

        assert_eq!(process.trigger(), Ok(WorkerProcessTrigger::Busy));
        assert_eq!(process.poll(), Ok(WorkerProcessPoll::Busy));
        process.shutdown().expect("shutdown should close the pool");
        process
            .shutdown()
            .expect("repeated shutdown should be inert");
        drop(permit);
        assert_eq!(
            process.trigger(),
            Err(WorkerProcessFailure {
                code: "worker_process_closed".to_owned(),
                phase: WorkerProcessPhase::Iteration,
            })
        );
        assert_eq!(
            trace.borrow().as_slice(),
            [
                "configuration",
                "pool",
                "required-runtime",
                "readiness",
                "durable-health",
                "iteration-runner",
                "scheduler-start:worker-a:60000",
                "iteration:1:Some(37)",
                "backfill-start",
                "scheduler-rearm",
                "scheduler-stop",
                "backfill-drain:active-backfill",
                "iteration-drain",
                "close",
            ]
        );
    }

    // compare startup ordering and poll behavior with the compiled Node worker
    #[test]
    fn compiled_node_startup_and_scheduler_match_the_port_contract() {
        let script =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/worker-startup-node-oracle.mjs");
        let output = Command::new("node")
            .arg(script)
            .output()
            .expect("compiled Node worker startup oracle should start");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let stdout = String::from_utf8(output.stdout)
            .expect("compiled Node worker startup oracle should emit UTF-8");

        assert_eq!(
            stdout.lines().collect::<Vec<_>>(),
            [
                "startup-order=config,pool,runtime,readiness,health,runner,scheduler",
                "once-order=trigger,close",
                "shutdown-order=stop,backfill,close",
                "normal-order=start,trigger,backfill",
                "nonoverlap=false,true,1",
                "manual-failure=true,0,false",
                "poll=1,1,2,true,1",
                "anchored-slots=122443,302443,302443,2443",
            ]
        );
    }
}
