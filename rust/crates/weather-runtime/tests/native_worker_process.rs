use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;
use std::time::Duration;

use tokio::sync::oneshot;
use weather_runtime::{
    AnchoredWorkerProcessScheduler, NativeBackendRuntime, NativeWorkerProcessOneShotTimer,
    run_native_worker_process,
};
use weather_worker::{
    AdmittedWorkerProcessConfiguration, AdmittedWorkerStartupTask, WorkerContinuation,
    WorkerHealth, WorkerProcessBackfillPort, WorkerProcessEnvironmentPort, WorkerProcessFailure,
    WorkerProcessIteration, WorkerProcessIterationFactory, WorkerProcessOptions, WorkerProcessPool,
    WorkerProcessPoolFactory, WorkerProcessPorts, WorkerProcessRuntimeLoader,
    WorkerProcessStartResult, WorkerProcessTrigger, start_worker_process,
};

type Trace = Rc<RefCell<Vec<String>>>;

#[derive(Clone)]
struct FixtureConfiguration {
    backfill: AdmittedWorkerStartupTask,
}

impl AdmittedWorkerProcessConfiguration for FixtureConfiguration {
    // retain one stable scheduler identity
    fn instance(&self) -> &str {
        "native-worker-fixture"
    }

    // keep optional maintenance outside this process-lifecycle fixture
    fn startup_maintenance(&self) -> AdmittedWorkerStartupTask {
        AdmittedWorkerStartupTask::Inactive
    }

    // expose the requested backfill lifecycle
    fn startup_backfill(&self) -> AdmittedWorkerStartupTask {
        self.backfill
    }
}

struct FixtureEnvironment {
    configuration: FixtureConfiguration,
    trace: Trace,
}

impl WorkerProcessEnvironmentPort for FixtureEnvironment {
    type Configuration = FixtureConfiguration;

    // admit the complete fixture configuration
    fn load_configuration(&mut self) -> Result<Self::Configuration, WorkerProcessFailure> {
        self.trace.borrow_mut().push("configuration".to_owned());
        Ok(self.configuration.clone())
    }
}

struct FixturePool {
    trace: Trace,
}

impl WorkerProcessPool<FixtureConfiguration> for FixturePool {
    // record readiness before durable health access
    fn assert_readiness(
        &mut self,
        _configuration: &FixtureConfiguration,
    ) -> Result<(), WorkerProcessFailure> {
        self.trace.borrow_mut().push("readiness".to_owned());
        Ok(())
    }

    // return one stable successful continuation point
    fn read_durable_health(
        &mut self,
        _configuration: &FixtureConfiguration,
    ) -> Result<WorkerHealth, WorkerProcessFailure> {
        self.trace.borrow_mut().push("durable-health".to_owned());
        Ok(WorkerHealth {
            last_loop_at_ms: Some(37),
            last_success_at_ms: Some(31),
            live: true,
            ready: true,
            stale: false,
        })
    }

    // expose every terminal close to the lifecycle assertion
    fn close(&mut self) -> Result<(), WorkerProcessFailure> {
        self.trace.borrow_mut().push("close".to_owned());
        Ok(())
    }
}

struct FixturePoolFactory {
    trace: Trace,
}

impl WorkerProcessPoolFactory<FixtureConfiguration> for FixturePoolFactory {
    type Pool = FixturePool;

    // construct one retained fixture pool
    fn create_pool(
        &mut self,
        _configuration: &FixtureConfiguration,
    ) -> Result<Self::Pool, WorkerProcessFailure> {
        self.trace.borrow_mut().push("pool".to_owned());
        Ok(FixturePool {
            trace: Rc::clone(&self.trace),
        })
    }
}

struct FixtureLoaders {
    trace: Trace,
}

impl WorkerProcessRuntimeLoader<FixtureConfiguration, FixturePool> for FixtureLoaders {
    type Runtime = ();

    // admit one dependency-free runtime marker
    fn load_required_runtime(
        &mut self,
        _configuration: &FixtureConfiguration,
        _pool: &mut FixturePool,
    ) -> Result<Self::Runtime, WorkerProcessFailure> {
        self.trace.borrow_mut().push("required-runtime".to_owned());
        Ok(())
    }

    // reject unexpected optional maintenance loading
    fn load_startup_maintenance(
        &mut self,
        _configuration: &FixtureConfiguration,
        _pool: &mut FixturePool,
        _runtime: &mut Self::Runtime,
    ) {
        panic!("fixture does not admit startup maintenance");
    }
}

struct FixtureBackfill {
    trace: Trace,
}

impl WorkerProcessBackfillPort<FixtureConfiguration, FixturePool> for FixtureBackfill {
    type Handle = ();

    // record one admitted startup workload
    fn start(
        &mut self,
        _configuration: &FixtureConfiguration,
        _pool: &mut FixturePool,
    ) -> Self::Handle {
        self.trace.borrow_mut().push("backfill-start".to_owned());
    }

    // record the graceful drain before pool close
    fn drain(&mut self, _handle: Self::Handle, _pool: &mut FixturePool) {
        self.trace.borrow_mut().push("backfill-drain".to_owned());
    }
}

struct FixtureIterationFactory {
    shutdown: Option<oneshot::Sender<()>>,
    trace: Trace,
}

struct FixtureIteration {
    runs: usize,
    shutdown: Option<oneshot::Sender<()>>,
    trace: Trace,
}

impl WorkerProcessIterationFactory<FixtureConfiguration, FixturePool, ()>
    for FixtureIterationFactory
{
    type Iteration = FixtureIteration;

    // retain the timer-driven shutdown sender in the iteration
    fn create_iteration(
        self,
        _configuration: &FixtureConfiguration,
        _pool: &mut FixturePool,
        _runtime: (),
    ) -> Result<Self::Iteration, WorkerProcessFailure> {
        self.trace.borrow_mut().push("iteration-runner".to_owned());
        Ok(FixtureIteration {
            runs: 0,
            shutdown: self.shutdown,
            trace: self.trace,
        })
    }
}

impl WorkerProcessIteration<FixtureConfiguration, FixturePool> for FixtureIteration {
    // count immediate and timer-delivered iterations
    fn run_iteration(
        &mut self,
        _configuration: &FixtureConfiguration,
        _pool: &mut FixturePool,
        continuation: &mut WorkerContinuation,
    ) -> Result<(), WorkerProcessFailure> {
        self.runs += 1;
        self.trace.borrow_mut().push(format!(
            "iteration:{}:{:?}",
            self.runs,
            continuation.last_success_at_ms()
        ));

        // request shutdown only after one real timer poll
        if self.runs == 2 {
            self.shutdown
                .take()
                .expect("normal fixture retains its shutdown sender")
                .send(())
                .expect("native process still owns the shutdown receiver");
        }
        Ok(())
    }

    // expose any unexpected in-flight shutdown drain
    fn drain_iteration(&mut self, _pool: &mut FixturePool) {
        self.trace.borrow_mut().push("iteration-drain".to_owned());
    }
}

// assemble the narrow typed process boundary around one scheduler
fn fixture_ports<Scheduler>(
    trace: &Trace,
    scheduler: Scheduler,
    backfill: AdmittedWorkerStartupTask,
    shutdown: Option<oneshot::Sender<()>>,
) -> WorkerProcessPorts<
    FixtureEnvironment,
    FixturePoolFactory,
    FixtureLoaders,
    Scheduler,
    FixtureBackfill,
    FixtureIterationFactory,
> {
    WorkerProcessPorts {
        backfill: FixtureBackfill {
            trace: Rc::clone(trace),
        },
        environment: FixtureEnvironment {
            configuration: FixtureConfiguration { backfill },
            trace: Rc::clone(trace),
        },
        iteration_factory: FixtureIterationFactory {
            shutdown,
            trace: Rc::clone(trace),
        },
        loaders: FixtureLoaders {
            trace: Rc::clone(trace),
        },
        pool_factory: FixturePoolFactory {
            trace: Rc::clone(trace),
        },
        scheduler,
    }
}

/// Drive one immediate iteration, one actual Tokio timer poll, and graceful shutdown.
#[test]
fn native_runtime_drives_one_retained_poll_then_drains_and_closes() {
    let runtime = NativeBackendRuntime::new().expect("native runtime should initialize");
    runtime.block_on(async {
        let trace = Rc::new(RefCell::new(Vec::new()));
        let scheduler_trace = Rc::clone(&trace);
        let diagnostic_trace = Rc::clone(&trace);
        let clock_trace = Rc::clone(&trace);
        let mut times = VecDeque::from([59_990, 59_995, 60_000, 60_000]);
        let (timer, events) = NativeWorkerProcessOneShotTimer::new();
        let scheduler = AnchoredWorkerProcessScheduler::new(
            move || {
                let now = times.pop_front().expect("fixture admits four clock reads");
                clock_trace.borrow_mut().push(format!("clock:{now}"));
                now
            },
            timer,
            move |key: &str| {
                scheduler_trace.borrow_mut().push(format!("digest:{key}"));
                Ok([0, 0, 0, 0])
            },
            move |failure: &WorkerProcessFailure| {
                diagnostic_trace
                    .borrow_mut()
                    .push(format!("diagnostic:{}", failure.code));
            },
        );
        let (shutdown_send, shutdown_receive) = oneshot::channel();
        let result = start_worker_process(
            WorkerProcessOptions { once: false },
            fixture_ports(
                &trace,
                scheduler,
                AdmittedWorkerStartupTask::Active,
                Some(shutdown_send),
            ),
        )
        .expect("normal native worker should start");
        let WorkerProcessStartResult::Running(process) = result else {
            panic!("normal native worker should remain running");
        };

        let report = tokio::time::timeout(
            Duration::from_secs(1),
            run_native_worker_process(process, events, async move {
                shutdown_receive
                    .await
                    .expect("timer iteration should request shutdown");
            }),
        )
        .await
        .expect("native worker should stop without waiting for the next cadence")
        .expect("native worker should shut down cleanly");

        assert_eq!(report.initial_trigger, WorkerProcessTrigger::Completed);
        assert_eq!(report.polls, 1);
        assert_eq!(report.busy_polls, 0);
        assert_eq!(report.completed_polls, 1);
        assert_eq!(report.failed_polls, 0);
        assert_eq!(
            trace.borrow().as_slice(),
            [
                "configuration",
                "pool",
                "required-runtime",
                "readiness",
                "durable-health",
                "iteration-runner",
                "digest:native-worker-fixture",
                "clock:59990",
                "clock:59995",
                "iteration:1:Some(31)",
                "backfill-start",
                "iteration:2:Some(31)",
                "clock:60000",
                "clock:60000",
                "backfill-drain",
                "close",
            ]
        );
    });
}

/// Keep one-shot startup closed without digesting a key or arming a native timer.
#[test]
fn once_mode_does_not_start_the_native_scheduler() {
    let runtime = NativeBackendRuntime::new().expect("native runtime should initialize");
    runtime.block_on(async {
        let trace = Rc::new(RefCell::new(Vec::new()));
        let scheduler_trace = Rc::clone(&trace);
        let diagnostic_trace = Rc::clone(&trace);
        let (timer, _events) = NativeWorkerProcessOneShotTimer::new();
        let scheduler = AnchoredWorkerProcessScheduler::new(
            || panic!("one-shot startup must not read the scheduler clock"),
            timer,
            move |key: &str| {
                scheduler_trace.borrow_mut().push(format!("digest:{key}"));
                Ok([0, 0, 0, 0])
            },
            move |failure: &WorkerProcessFailure| {
                diagnostic_trace
                    .borrow_mut()
                    .push(format!("diagnostic:{}", failure.code));
            },
        );
        let result = start_worker_process(
            WorkerProcessOptions { once: true },
            fixture_ports(&trace, scheduler, AdmittedWorkerStartupTask::Inactive, None),
        )
        .expect("one-shot native worker should complete");

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
                "readiness",
                "durable-health",
                "iteration-runner",
                "iteration:1:Some(31)",
                "close",
            ]
        );
    });
}
