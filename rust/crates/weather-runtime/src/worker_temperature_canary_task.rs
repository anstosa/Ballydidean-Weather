//! Connect the retained worker sidecar phase to the typed temperature-canary collector.

use weather_worker::{
    TaskFailure, TemperatureCanaryCollectionResult, TemperatureCanaryExecution,
    TemperatureCanaryFetchLimits, TemperatureCanaryProvider, TemperatureCanaryRecentErrorBuilder,
    TemperatureCanaryRepository, TemperatureCanarySource, TemperatureCanaryTimeAdapter,
    collect_ecmwf_temperature_canary_runs,
};

use crate::RetainedWorkerSidecarTask;

/// Four admitted owners prepared from one retained worker database and canonical value scope.
pub struct WorkerTemperatureCanaryBound<Repository, Provider, RecentError, Time> {
    pub provider: Provider,
    pub recent_error: RecentError,
    pub repository: Repository,
    pub time: Time,
}

/// The concrete prepared binding returned for one scoped task execution.
pub type WorkerTemperatureCanaryBoundFor<'a, Bindings, Database, Values> =
    WorkerTemperatureCanaryBound<
        <Bindings as WorkerTemperatureCanaryBindings<Database, Values>>::Repository<'a>,
        <Bindings as WorkerTemperatureCanaryBindings<Database, Values>>::Provider<'a>,
        <Bindings as WorkerTemperatureCanaryBindings<Database, Values>>::RecentError<'a>,
        <Bindings as WorkerTemperatureCanaryBindings<Database, Values>>::Time<'a>,
    >;

/// Prepare the collector's admitted ports from exact reborrows of retained worker owners.
pub trait WorkerTemperatureCanaryBindings<Database, Values>: Sized {
    type Repository<'a>: TemperatureCanaryRepository
    where
        Self: 'a,
        Database: 'a,
        Values: 'a;
    type Provider<'a>: TemperatureCanaryProvider
    where
        Self: 'a,
        Database: 'a,
        Values: 'a;
    type RecentError<'a>: TemperatureCanaryRecentErrorBuilder
    where
        Self: 'a,
        Database: 'a,
        Values: 'a;
    type Time<'a>: TemperatureCanaryTimeAdapter
    where
        Self: 'a,
        Database: 'a,
        Values: 'a;

    /// Split or prepare the exact database and canonical-value owners required by one collection.
    fn bind<'a>(
        &'a mut self,
        database: &'a mut Database,
        values: &'a mut Values,
    ) -> Result<WorkerTemperatureCanaryBoundFor<'a, Self, Database, Values>, TaskFailure>;
}

/// Retained sidecar task owning the checked source, limits, bindings, and collection clock.
pub struct WorkerTemperatureCanaryTask<Bindings, Clock> {
    bindings: Bindings,
    clock: Clock,
    fetch_limits: TemperatureCanaryFetchLimits,
    source: TemperatureCanarySource,
}

impl<Bindings, Clock> WorkerTemperatureCanaryTask<Bindings, Clock> {
    /// Bind one already admitted source without opening database or provider resources.
    pub const fn new(
        bindings: Bindings,
        clock: Clock,
        source: TemperatureCanarySource,
        fetch_limits: TemperatureCanaryFetchLimits,
    ) -> Self {
        Self {
            bindings,
            clock,
            fetch_limits,
            source,
        }
    }
}

impl<Database, Values, Bindings, Clock> RetainedWorkerSidecarTask<Database, Values, ()>
    for WorkerTemperatureCanaryTask<Bindings, Clock>
where
    Bindings: WorkerTemperatureCanaryBindings<Database, Values>,
    Clock: FnMut() -> i64,
{
    type Output = TemperatureCanaryCollectionResult;

    /// Run the actual bounded collector through one retained resource binding and clock sample.
    fn run(
        &mut self,
        (): (),
        database: &mut Database,
        values: &mut Values,
    ) -> Result<Self::Output, TaskFailure> {
        let now_ms = (self.clock)();
        let mut bound = self.bindings.bind(database, values)?;
        let mut execution = TemperatureCanaryExecution {
            fetch_limits: self.fetch_limits,
            provider: &mut bound.provider,
            recent_error: &mut bound.recent_error,
            repository: &mut bound.repository,
            source: &self.source,
            time: &mut bound.time,
        };
        let collection = collect_ecmwf_temperature_canary_runs(now_ms, &mut execution)?;
        Ok(TemperatureCanaryCollectionResult::from(&collection))
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use weather_worker::{
        OPEN_METEO_ECMWF_SINGLE_RUN_ADAPTER_VERSION, OPEN_METEO_ECMWF_SINGLE_RUN_MODEL,
        TemperatureCanaryBatch, TemperatureCanaryFetchControls, TemperatureCanaryPersistedRun,
        TemperatureCanaryRecentErrorState, TemperatureCanaryRequest,
        latest_available_ecmwf_initialization,
    };

    use super::*;

    const NOW_MS: i64 = 1_800_000_000_000;
    const SIX_HOURS_MS: i64 = 6 * 3_600_000;

    #[derive(Default)]
    struct Events {
        binds: usize,
        inventory_reads: usize,
        provider_fetches: usize,
        recent_builds: usize,
        time_formats: usize,
    }

    struct Repository {
        events: Rc<RefCell<Events>>,
        existing: Vec<String>,
    }

    impl TemperatureCanaryRepository for Repository {
        /// Return the complete admitted warmup inventory.
        fn list_run_initializations(
            &mut self,
            _site_slug: &str,
            _from: &str,
            _to: &str,
        ) -> Result<Vec<String>, TaskFailure> {
            self.events.borrow_mut().inventory_reads += 1;
            Ok(self.existing.clone())
        }

        /// Reject unexpected persistence when the fixture has no missing runs.
        fn persist_run(&mut self, _run: &TemperatureCanaryPersistedRun) -> Result<(), TaskFailure> {
            panic!("complete inventory must not persist another run")
        }
    }

    struct Provider {
        events: Rc<RefCell<Events>>,
    }

    impl TemperatureCanaryProvider for Provider {
        /// Reject unexpected traffic when the fixture has no missing runs.
        fn fetch_single_run(
            &mut self,
            _request: &TemperatureCanaryRequest,
            _controls: TemperatureCanaryFetchControls,
        ) -> Result<TemperatureCanaryBatch, TaskFailure> {
            self.events.borrow_mut().provider_fetches += 1;
            panic!("complete inventory must not fetch a provider run")
        }
    }

    struct RecentError {
        events: Rc<RefCell<Events>>,
    }

    impl TemperatureCanaryRecentErrorBuilder for RecentError {
        /// Reject unexpected reduction when the fixture has no missing runs.
        fn build_recent_error_state(
            &mut self,
            _site_slug: &str,
            _target_run_initialized_at: &str,
            _target_run_initialized_at_ms: i64,
        ) -> Result<TemperatureCanaryRecentErrorState, TaskFailure> {
            self.events.borrow_mut().recent_builds += 1;
            panic!("complete inventory must not build recent state")
        }
    }

    struct Time {
        events: Rc<RefCell<Events>>,
    }

    impl TemperatureCanaryTimeAdapter for Time {
        /// Format fixture UTC instants through the admitted adapter boundary.
        fn utc_string(&mut self, epoch_ms: i64) -> Result<String, TaskFailure> {
            self.events.borrow_mut().time_formats += 1;
            Ok(epoch_ms.to_string())
        }
    }

    struct Bindings {
        events: Rc<RefCell<Events>>,
        existing: Vec<String>,
    }

    impl WorkerTemperatureCanaryBindings<(), ()> for Bindings {
        type Repository<'a> = Repository;
        type Provider<'a> = Provider;
        type RecentError<'a> = RecentError;
        type Time<'a> = Time;

        /// Prepare all admitted fixture owners from the exact retained scope.
        fn bind<'a>(
            &'a mut self,
            _database: &'a mut (),
            _values: &'a mut (),
        ) -> Result<WorkerTemperatureCanaryBoundFor<'a, Self, (), ()>, TaskFailure> {
            self.events.borrow_mut().binds += 1;
            Ok(WorkerTemperatureCanaryBound {
                provider: Provider {
                    events: self.events.clone(),
                },
                recent_error: RecentError {
                    events: self.events.clone(),
                },
                repository: Repository {
                    events: self.events.clone(),
                    existing: self.existing.clone(),
                },
                time: Time {
                    events: self.events.clone(),
                },
            })
        }
    }

    /// Run the real collector with live-shaped source, clock, and retained owner flow.
    #[test]
    fn runs_actual_collector_without_missing_runs() {
        let latest = latest_available_ecmwf_initialization(NOW_MS).unwrap();
        // retain the complete six-hour warmup inventory admitted by the collector
        let existing = (0..=12)
            .map(|offset| (latest - i64::from(offset) * SIX_HOURS_MS).to_string())
            .collect();
        let events = Rc::new(RefCell::new(Events::default()));
        let clock_calls = Rc::new(RefCell::new(0));
        let clock_events = clock_calls.clone();
        let mut task = WorkerTemperatureCanaryTask::new(
            Bindings {
                events: events.clone(),
                existing,
            },
            move || {
                *clock_events.borrow_mut() += 1;
                NOW_MS
            },
            source(),
            TemperatureCanaryFetchLimits {
                max_body_bytes: Some(400_000),
                timeout_ms: Some(4_000),
            },
        );

        let result = task.run((), &mut (), &mut ()).unwrap();

        assert_eq!(
            result,
            TemperatureCanaryCollectionResult {
                failed_runs: 0,
                persisted_runs: 0,
            }
        );
        assert_eq!(*clock_calls.borrow(), 1);
        let events = events.borrow();
        assert_eq!(events.binds, 1);
        assert_eq!(events.inventory_reads, 1);
        assert_eq!(events.provider_fetches, 0);
        assert_eq!(events.recent_builds, 0);
        assert_eq!(events.time_formats, 15);
    }

    /// Build the exact active private single-run source identity.
    fn source() -> TemperatureCanarySource {
        TemperatureCanarySource {
            active: true,
            adapter_version: OPEN_METEO_ECMWF_SINGLE_RUN_ADAPTER_VERSION.to_owned(),
            capability: "forecast".to_owned(),
            dataset: "single_run".to_owned(),
            latitude: 47.95,
            longitude: -122.43,
            provider_key: "open-meteo".to_owned(),
            site_slug: "home".to_owned(),
            source_kind: "forecast".to_owned(),
            upstream_model: OPEN_METEO_ECMWF_SINGLE_RUN_MODEL.to_owned(),
        }
    }
}
