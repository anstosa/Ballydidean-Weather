//! Compose the native provider, causal reads, and PostgreSQL canary repository.

use std::sync::Arc;

use weather_contracts::SiteConfiguration;

use weather_worker::{
    OPEN_METEO_ECMWF_SINGLE_RUN_ADAPTER_VERSION, OPEN_METEO_ECMWF_SINGLE_RUN_MODEL, TaskFailure,
    TemperatureCanaryFetchLimits, TemperatureCanarySource,
};

use crate::{
    CausalTemperatureCalendarPort, DatabaseTemperatureCanaryRepository,
    DatabaseTemperatureRecentErrorBuilder, NativeOutboundHttp, NativePostgresPool,
    NativePostgresTemperatureSession, NativePredecessorModelCalendar, NativeRuntimeMaterial,
    NativeTemperatureCanaryProvider, NativeWirePrimitives,
    TemperatureCanaryPersistenceDatabasePort, TemperatureCanaryRevisionArchiveMode,
    WorkerTemperatureCanaryBindings, WorkerTemperatureCanaryBound, WorkerTemperatureCanaryBoundFor,
    WorkerTemperatureCanaryTask, format_predecessor_source_utc, parse_predecessor_source_utc,
};

/// Create the concrete persistence database selected for one collection binding.
pub trait NativeTemperatureCanaryDatabaseFactory {
    type Database: TemperatureCanaryPersistenceDatabasePort;

    /// Preserve the requested archive mode while constructing its actual database adapter.
    fn bind_database(
        &mut self,
        pool: &NativePostgresPool,
        archive_mode: TemperatureCanaryRevisionArchiveMode,
    ) -> Result<Self::Database, TaskFailure>;
}

/// Ordinary serving database factory for the no-archive fast path.
#[derive(Clone, Copy, Debug, Default)]
pub struct NativeTemperatureCanaryServingDatabaseFactory;

impl NativeTemperatureCanaryDatabaseFactory for NativeTemperatureCanaryServingDatabaseFactory {
    type Database = NativePostgresTemperatureSession;

    /// Bind the real PostgreSQL session and refuse to downgrade enabled archive mode.
    fn bind_database(
        &mut self,
        pool: &NativePostgresPool,
        archive_mode: TemperatureCanaryRevisionArchiveMode,
    ) -> Result<Self::Database, TaskFailure> {
        // enabled archival requires the existing archive database composition
        if archive_mode == TemperatureCanaryRevisionArchiveMode::Enabled {
            return Err(binding_failure(
                "native_temperature_revision_archive_binding_required",
            ));
        }
        Ok(NativePostgresTemperatureSession::from_pool(pool.clone()))
    }
}

/// Pinned local-calendar adapter used by causal recent-error reduction.
pub struct NativeTemperatureCanaryCalendar {
    calendar: NativePredecessorModelCalendar,
}

impl NativeTemperatureCanaryCalendar {
    /// Load the same exact predecessor model calendar admitted by native API startup.
    pub fn load() -> Result<Self, TaskFailure> {
        NativePredecessorModelCalendar::new()
            .map(|calendar| Self { calendar })
            .ok_or_else(|| binding_failure("native_temperature_calendar_unavailable"))
    }
}

impl CausalTemperatureCalendarPort for NativeTemperatureCanaryCalendar {
    /// Format one admitted epoch with the exact millisecond UTC spelling.
    fn utc_string(&mut self, epoch_ms: i64) -> Result<String, TaskFailure> {
        format_predecessor_source_utc(epoch_ms)
            .ok_or_else(|| binding_failure("native_temperature_clock_invalid"))
    }

    /// Resolve one canonical UTC instant through the pinned Los Angeles calendar.
    fn local_date(&mut self, valid_at: &str, epoch_ms: i64) -> Result<String, TaskFailure> {
        // bind the local projection to the same canonical UTC instant
        if parse_predecessor_source_utc(valid_at)
            .is_none_or(|instant| instant.epoch_milliseconds != epoch_ms || instant.utc != valid_at)
        {
            return Err(binding_failure("native_temperature_clock_invalid"));
        }
        self.calendar
            .features(valid_at)
            .map(|features| features.local_date)
            .map_err(|_| binding_failure("native_temperature_clock_invalid"))
    }
}

/// Production binding retaining the shared PostgreSQL and outbound HTTP pools.
pub struct NativeWorkerTemperatureBindings<DatabaseFactory> {
    archive_mode: TemperatureCanaryRevisionArchiveMode,
    client: Arc<NativeOutboundHttp>,
    compatibility_origin: Option<String>,
    database_factory: DatabaseFactory,
    pool: NativePostgresPool,
}

impl<DatabaseFactory> NativeWorkerTemperatureBindings<DatabaseFactory> {
    /// Retain process-shared clients and the explicit archive database selection.
    pub const fn new(
        pool: NativePostgresPool,
        client: Arc<NativeOutboundHttp>,
        compatibility_origin: Option<String>,
        archive_mode: TemperatureCanaryRevisionArchiveMode,
        database_factory: DatabaseFactory,
    ) -> Self {
        Self {
            archive_mode,
            client,
            compatibility_origin,
            database_factory,
            pool,
        }
    }
}

impl<Database, Values, DatabaseFactory> WorkerTemperatureCanaryBindings<Database, Values>
    for NativeWorkerTemperatureBindings<DatabaseFactory>
where
    DatabaseFactory: NativeTemperatureCanaryDatabaseFactory,
{
    type Repository<'a>
        = DatabaseTemperatureCanaryRepository<DatabaseFactory::Database, NativeRuntimeMaterial>
    where
        Self: 'a,
        Database: 'a,
        Values: 'a;
    type Provider<'a>
        = NativeTemperatureCanaryProvider
    where
        Self: 'a,
        Database: 'a,
        Values: 'a;
    type RecentError<'a>
        = DatabaseTemperatureRecentErrorBuilder<NativePostgresPool, NativeTemperatureCanaryCalendar>
    where
        Self: 'a,
        Database: 'a,
        Values: 'a;
    type Time<'a>
        = NativeWirePrimitives
    where
        Self: 'a,
        Database: 'a,
        Values: 'a;

    /// Bind all four collector owners without creating another HTTP or PostgreSQL pool.
    fn bind<'a>(
        &'a mut self,
        _database: &'a mut Database,
        _values: &'a mut Values,
    ) -> Result<WorkerTemperatureCanaryBoundFor<'a, Self, Database, Values>, TaskFailure> {
        let persistence = self
            .database_factory
            .bind_database(&self.pool, self.archive_mode)?;
        let calendar = NativeTemperatureCanaryCalendar::load()?;
        Ok(WorkerTemperatureCanaryBound {
            provider: NativeTemperatureCanaryProvider::new(
                Arc::clone(&self.client),
                self.compatibility_origin.clone(),
            ),
            recent_error: DatabaseTemperatureRecentErrorBuilder::new(self.pool.clone(), calendar),
            repository: DatabaseTemperatureCanaryRepository::new(
                persistence,
                NativeRuntimeMaterial,
                self.archive_mode,
            ),
            time: NativeWirePrimitives,
        })
    }
}

/// Build the exact private single-run source identity from the admitted site catalog.
pub fn native_temperature_canary_source(site: &SiteConfiguration) -> TemperatureCanarySource {
    TemperatureCanarySource {
        active: true,
        adapter_version: OPEN_METEO_ECMWF_SINGLE_RUN_ADAPTER_VERSION.to_owned(),
        capability: "forecast".to_owned(),
        dataset: "single_run".to_owned(),
        latitude: site.site.latitude,
        longitude: site.site.longitude,
        provider_key: "open-meteo".to_owned(),
        site_slug: site.site.key.clone(),
        source_kind: "forecast".to_owned(),
        upstream_model: OPEN_METEO_ECMWF_SINGLE_RUN_MODEL.to_owned(),
    }
}

/// Build one actual worker canary task with an explicit archive database factory.
pub fn native_worker_temperature_task<DatabaseFactory, Clock>(
    site: &SiteConfiguration,
    bindings: NativeWorkerTemperatureBindings<DatabaseFactory>,
    clock: Clock,
    fetch_limits: TemperatureCanaryFetchLimits,
) -> WorkerTemperatureCanaryTask<NativeWorkerTemperatureBindings<DatabaseFactory>, Clock> {
    WorkerTemperatureCanaryTask::new(
        bindings,
        clock,
        native_temperature_canary_source(site),
        fetch_limits,
    )
}

/// Build one ordinary no-archive worker canary task over the shared process pools.
pub fn native_worker_temperature_task_without_archive<Clock>(
    site: &SiteConfiguration,
    pool: NativePostgresPool,
    client: Arc<NativeOutboundHttp>,
    compatibility_origin: Option<String>,
    clock: Clock,
    fetch_limits: TemperatureCanaryFetchLimits,
) -> WorkerTemperatureCanaryTask<
    NativeWorkerTemperatureBindings<NativeTemperatureCanaryServingDatabaseFactory>,
    Clock,
> {
    native_worker_temperature_task(
        site,
        NativeWorkerTemperatureBindings::new(
            pool,
            client,
            compatibility_origin,
            TemperatureCanaryRevisionArchiveMode::Disabled,
            NativeTemperatureCanaryServingDatabaseFactory,
        ),
        clock,
        fetch_limits,
    )
}

/// Preserve one stable binding failure without provider attempt inflation.
fn binding_failure(code: &str) -> TaskFailure {
    TaskFailure {
        attempts: 0,
        code: code.to_owned(),
    }
}
