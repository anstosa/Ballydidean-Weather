//! Nonblocking startup resumption of admitted public station archives.

use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use weather_contracts::PublicStationConfiguration;
use weather_worker::{
    AmbientWeatherBackfillProvider, PublicStationBackfillArguments, PublicStationBackfillDate,
    PublicStationBackfillDeadlines, PublicStationBackfillOperationError,
    PublicStationBackfillReport, PublicStationBackfillTimeAdapter,
    SupplementalPublicStationBackfillRepository, TaskFailure, WorkerProcessBackfillPort,
    execute_public_station_archive_backfill,
};

use crate::public_station_archive_binding::bind_public_station_archive;
use crate::{PublicStationBackfillPlanningRepository, SupplementalPublicStationBackfillProvider};

const STARTUP_FAILURE: &str = "public_station_backfill_failed";

/// The loaded public-station configuration and release for one startup job.
#[derive(Clone, Debug, PartialEq)]
pub struct PublicStationStartupInput {
    pub configuration: PublicStationConfiguration,
    pub release: String,
}

/// Obtain the optional archive configuration from the retained worker process.
pub trait PublicStationStartupConfiguration {
    /// Return the configured startup archive, or none when explicitly disabled.
    fn public_station_startup_input(&self) -> Option<PublicStationStartupInput>;
}

/// Share the process pool with one bounded background archive job.
pub trait PublicStationStartupPool {
    type Repository: PublicStationBackfillPlanningRepository
        + SupplementalPublicStationBackfillRepository
        + Send
        + 'static;

    /// Acquire an owned handle to the same bounded connection pool.
    fn share_startup_repository(&mut self) -> Result<Self::Repository, TaskFailure>;
}

/// Date and diagnostic clocks supplied by the retained runtime.
pub trait PublicStationStartupTime: PublicStationBackfillTimeAdapter {
    /// Read the same wall-clock milliseconds used for startup diagnostic duration.
    fn diagnostic_now(&mut self) -> i64;
}

/// Source-level outcome or a contained error before a source report was produced.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PublicStationStartupDiagnosticEvent {
    SourceRun,
    WorkerIteration,
}

/// One redacted startup result, matching the retained worker diagnostic fields.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PublicStationStartupDiagnostic<'a> {
    pub count: usize,
    pub duration_ms: i64,
    pub error_code: Option<&'static str>,
    pub event: PublicStationStartupDiagnosticEvent,
    pub release: &'a str,
    pub source_id: Option<&'a str>,
}

/// Worker log sink shared by the retained runner and startup thread.
pub trait PublicStationStartupDiagnostics {
    /// Record one bounded diagnostic without raw provider or database errors.
    fn record(&self, diagnostic: PublicStationStartupDiagnostic<'_>);
}

/// The result preserved across graceful shutdown.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PublicStationStartupOutcome {
    Completed(PublicStationBackfillReport),
    Failed,
}

/// One optional startup job joined when the owning worker shuts down.
pub enum PublicStationStartupHandle {
    Inactive,
    Running(JoinHandle<PublicStationStartupOutcome>),
    Failed,
}

/// Implements the normal worker startup path without occupying the scheduler.
pub struct PublicStationStartupBackfill<Ambient, Supplemental, Time, Diagnostics> {
    inputs: Option<(Ambient, Supplemental, Time)>,
    diagnostics: Arc<Diagnostics>,
    completed: Option<PublicStationStartupOutcome>,
}

impl<Ambient, Supplemental, Time, Diagnostics>
    PublicStationStartupBackfill<Ambient, Supplemental, Time, Diagnostics>
{
    /// Retain both provider owners and the time adapter for exactly one startup task.
    pub const fn new(
        ambient: Ambient,
        supplemental: Supplemental,
        time: Time,
        diagnostics: Arc<Diagnostics>,
    ) -> Self {
        Self {
            inputs: Some((ambient, supplemental, time)),
            diagnostics,
            completed: None,
        }
    }

    /// Return the optional final result after a graceful drain.
    pub const fn completed(&self) -> Option<&PublicStationStartupOutcome> {
        self.completed.as_ref()
    }
}

impl<Configuration, Pool, Ambient, Supplemental, Time, Diagnostics>
    WorkerProcessBackfillPort<Configuration, Pool>
    for PublicStationStartupBackfill<Ambient, Supplemental, Time, Diagnostics>
where
    Configuration: PublicStationStartupConfiguration,
    Pool: PublicStationStartupPool,
    Ambient: AmbientWeatherBackfillProvider + Send + 'static,
    Supplemental: SupplementalPublicStationBackfillProvider + Send + 'static,
    Time: PublicStationStartupTime + Send + 'static,
    Diagnostics: PublicStationStartupDiagnostics + Send + Sync + 'static,
{
    type Handle = PublicStationStartupHandle;

    /// Launch exactly one job after the process's ordinary initial iteration.
    fn start(&mut self, configuration: &Configuration, pool: &mut Pool) -> Self::Handle {
        // skip the entire archive workflow when the integration is disabled
        let Some(input) = configuration.public_station_startup_input() else {
            return PublicStationStartupHandle::Inactive;
        };
        // take the sole provider and clock owners without retaining a second live job
        let Some((ambient, supplemental, time)) = self.inputs.take() else {
            record_error(self.diagnostics.as_ref(), &input.release, 0);
            return PublicStationStartupHandle::Failed;
        };
        // share the existing pool without creating a second set of connections
        let Ok(repository) = pool.share_startup_repository() else {
            record_error(self.diagnostics.as_ref(), &input.release, 0);
            return PublicStationStartupHandle::Failed;
        };
        let release = input.release.clone();
        let diagnostics = Arc::clone(&self.diagnostics);
        let task = thread::Builder::new()
            .name("weather-public-station-backfill".to_owned())
            .spawn(
                // run the admitted archive workflow off the normal scheduler
                move || {
                    resume_public_station_archives(
                        input,
                        repository,
                        ambient,
                        supplemental,
                        time,
                        diagnostics.as_ref(),
                    )
                },
            );
        match task {
            Ok(task) => PublicStationStartupHandle::Running(task),
            Err(_) => {
                record_error(self.diagnostics.as_ref(), &release, 0);
                PublicStationStartupHandle::Failed
            }
        }
    }

    /// Join the startup archive before allowing the worker to close the shared pool.
    fn drain(&mut self, handle: Self::Handle, _pool: &mut Pool) {
        self.completed = match handle {
            PublicStationStartupHandle::Inactive => None,
            PublicStationStartupHandle::Running(task) => {
                Some(task.join().unwrap_or(PublicStationStartupOutcome::Failed))
            }
            PublicStationStartupHandle::Failed => Some(PublicStationStartupOutcome::Failed),
        };
    }
}

/// Resolve sources, exclude the current UTC day, then execute exact resume semantics.
fn resume_public_station_archives<R, A, S, T, D>(
    input: PublicStationStartupInput,
    mut repository: R,
    ambient: A,
    supplemental: S,
    mut time: T,
    diagnostics: &D,
) -> PublicStationStartupOutcome
where
    R: PublicStationBackfillPlanningRepository + SupplementalPublicStationBackfillRepository,
    A: AmbientWeatherBackfillProvider,
    S: SupplementalPublicStationBackfillProvider,
    T: PublicStationStartupTime,
    D: PublicStationStartupDiagnostics,
{
    let started_at = time.diagnostic_now();
    // preserve source resolution before the current UTC calendar read
    let report = repository
        .resolve_public_station_planning_sources(&input.configuration, &[])
        .and_then(|planning_sources| {
            let today = time.today_utc()?;
            let through = time.add_utc_days(&today, -1)?;
            let (sources, mut provider) = bind_public_station_archive(
                &input.configuration,
                planning_sources,
                ambient,
                supplemental,
                // retain PurpleAir pacing without coupling it to transport ownership
                |delay_ms| thread::sleep(Duration::from_millis(delay_ms)),
            )?;
            let arguments = PublicStationBackfillArguments {
                dry_run: false,
                from: None,
                resume: true,
                site: input.configuration.siteKey.clone(),
                source_keys: Vec::new(),
                to: through,
            };
            execute_public_station_archive_backfill(
                &arguments,
                &input.configuration.siteKey,
                &sources,
                &mut repository,
                &mut provider,
                &mut StartupCalendar {
                    today,
                    time: &mut time,
                },
            )
        });
    match report {
        Ok(report) => {
            // emit one redacted result per configured archive source
            for source in &report.sources {
                diagnostics.record(PublicStationStartupDiagnostic {
                    count: source.records,
                    duration_ms: time.diagnostic_now().saturating_sub(started_at),
                    error_code: (source.exit_code != 0).then_some(STARTUP_FAILURE),
                    event: PublicStationStartupDiagnosticEvent::SourceRun,
                    release: &input.release,
                    source_id: Some(&source.source),
                });
            }
            PublicStationStartupOutcome::Completed(report)
        }
        Err(_) => {
            record_error(
                diagnostics,
                &input.release,
                time.diagnostic_now().saturating_sub(started_at),
            );
            PublicStationStartupOutcome::Failed
        }
    }
}

/// Reuse the one captured UTC day for startup argument validation.
struct StartupCalendar<'a, Time> {
    today: PublicStationBackfillDate,
    time: &'a mut Time,
}

impl<Time: PublicStationBackfillTimeAdapter> PublicStationBackfillTimeAdapter
    for StartupCalendar<'_, Time>
{
    /// Return the admitted startup day without rereading the live clock.
    fn today_utc(&mut self) -> Result<PublicStationBackfillDate, TaskFailure> {
        Ok(self.today.clone())
    }

    /// Use the live owner for calendar arithmetic outside the captured date.
    fn add_utc_days(
        &mut self,
        date: &PublicStationBackfillDate,
        days: i64,
    ) -> Result<PublicStationBackfillDate, TaskFailure> {
        self.time.add_utc_days(date, days)
    }

    /// Keep each durable chunk's deadlines on the real execution clock.
    fn deadlines(
        &mut self,
    ) -> Result<PublicStationBackfillDeadlines, PublicStationBackfillOperationError> {
        self.time.deadlines()
    }
}

/// Contain configuration, selection, and execution failures at the startup boundary.
fn record_error(
    diagnostics: &impl PublicStationStartupDiagnostics,
    release: &str,
    duration_ms: i64,
) {
    diagnostics.record(PublicStationStartupDiagnostic {
        count: 0,
        duration_ms,
        error_code: Some(STARTUP_FAILURE),
        event: PublicStationStartupDiagnosticEvent::WorkerIteration,
        release,
        source_id: None,
    });
}
