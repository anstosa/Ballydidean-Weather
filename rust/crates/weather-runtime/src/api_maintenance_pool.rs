//! Run private maintenance statements on the existing Weather API PostgreSQL pool.

use weather_api::DatabaseAdjustmentMaintenanceRepository;
use weather_contracts::{
    AdjustmentRevisionCommitReceipt, AdjustmentShadowPredictionReceipt,
    AdjustmentShadowRegistrationReceipt, AdjustmentShadowRegistrationSlot,
};
use weather_database::{
    AdjustmentMaintenanceDatabasePort, AdjustmentRevisionGapMarker,
    AdjustmentShadowRegistrationSlotQueryPlan, ApiMaintenancePredictionPlan,
    ApiMaintenancePredictionWritePort, ApiMaintenanceRegistrationPlan, MaintenancePublicationPlan,
};

use crate::api_read_pool::run_api_pool_query;
use crate::{
    ApiReadPoolLease, PooledWeatherApiReads, PostgresWeatherServiceRepository, WeatherApiReadPool,
};

/// Maintenance operations supported by the same retained pool as public API reads.
pub trait WeatherApiMaintenancePool: WeatherApiReadPool {
    type MaintenanceSession<'pool>: AdjustmentMaintenanceDatabasePort<Error = Self::Error>
        + ApiReadPoolLease
    where
        Self: 'pool;

    /// Acquire a single lease of the existing process pool for a maintenance statement.
    fn checkout_maintenance(&mut self) -> Result<Self::MaintenanceSession<'_>, Self::Error>;

    /// Start all three schedule queries through the existing pool with independent leases.
    ///
    /// Preserve input order on success; on failure, return the reported database error
    /// while the already-started queries keep their normal bounded release behavior.
    fn read_schedule_cohort(
        &mut self,
        plans: [AdjustmentShadowRegistrationSlotQueryPlan; 3],
    ) -> Result<[Vec<AdjustmentShadowRegistrationSlot>; 3], Self::Error>;
}

/// Compact writes leased from the same pool as public reads and private admission.
pub trait WeatherApiMaintenanceWritePool: WeatherApiMaintenancePool {
    type MaintenanceWriteSession<'pool>: ApiMaintenancePredictionWritePort<Error = Self::Error>
        + ApiReadPoolLease
    where
        Self: 'pool;

    /// Check out the retained pool lease for one compact write.
    fn checkout_maintenance_write(
        &mut self,
    ) -> Result<Self::MaintenanceWriteSession<'_>, Self::Error>;
}

/// A private, short-lived view of the retained pool; never constructs a second database pool.
pub struct PooledWeatherApiMaintenance<'pool, Pool> {
    pool: &'pool mut Pool,
}

impl<'pool, Pool> PooledWeatherApiMaintenance<'pool, Pool> {
    /// Borrow the private listener's existing pool handle for one repository request.
    pub const fn from_shared_handle(pool: &'pool mut Pool) -> Self {
        Self { pool }
    }
}

impl<Pool: WeatherApiMaintenancePool, External>
    PostgresWeatherServiceRepository<PooledWeatherApiReads<Pool>, External>
{
    /// Bind the existing private maintenance repository to the retained API pool.
    pub fn maintenance_repository(
        &mut self,
    ) -> DatabaseAdjustmentMaintenanceRepository<PooledWeatherApiMaintenance<'_, Pool>> {
        DatabaseAdjustmentMaintenanceRepository::new(
            PooledWeatherApiMaintenance::from_shared_handle(self.database_mut().pool_mut()),
        )
    }
}

impl<Pool: WeatherApiMaintenancePool> PooledWeatherApiMaintenance<'_, Pool> {
    /// Run a single database facade and recycle or discard its lease before returning.
    fn query<T>(
        &mut self,
        execute: impl FnOnce(&mut Pool::MaintenanceSession<'_>) -> Result<T, Pool::Error>,
    ) -> Result<T, Pool::Error> {
        let session = self.pool.checkout_maintenance()?;
        run_api_pool_query(session, execute, Pool::classify_session_failure)
    }
}

impl<Pool: WeatherApiMaintenancePool> AdjustmentMaintenanceDatabasePort
    for PooledWeatherApiMaintenance<'_, Pool>
{
    type Error = Pool::Error;

    /// Execute one exact family-bound plan through a normal pool lease.
    fn read_schedule_slot(
        &mut self,
        plan: AdjustmentShadowRegistrationSlotQueryPlan,
    ) -> Result<Vec<AdjustmentShadowRegistrationSlot>, Self::Error> {
        self.query(|session| session.read_schedule_slot(plan))
    }

    /// Delegate the production fanout to the shared pool without serializing its three queries.
    fn read_schedule_slots(
        &mut self,
        plans: [AdjustmentShadowRegistrationSlotQueryPlan; 3],
    ) -> Result<[Vec<AdjustmentShadowRegistrationSlot>; 3], Self::Error> {
        self.pool.read_schedule_cohort(plans)
    }

    /// Read the body admission facade before a receipt lookup.
    fn read_shadow_body_admission(
        &mut self,
        plan: MaintenancePublicationPlan<'_, 4>,
    ) -> Result<Vec<bool>, Self::Error> {
        self.query(|session| session.read_shadow_body_admission(plan))
    }

    /// Read the authoritative ordinary shadow receipt using an independent lease.
    fn read_shadow_revision_receipt(
        &mut self,
        plan: MaintenancePublicationPlan<'_, 4>,
    ) -> Result<Vec<AdjustmentRevisionCommitReceipt>, Self::Error> {
        self.query(|session| session.read_shadow_revision_receipt(plan))
    }

    /// Rebind one current revision pointer through its exact typed facade.
    fn read_current_revision_receipt(
        &mut self,
        plan: MaintenancePublicationPlan<'_, 1>,
    ) -> Result<Vec<AdjustmentRevisionCommitReceipt>, Self::Error> {
        self.query(|session| session.read_current_revision_receipt(plan))
    }

    /// Record a permanent disposition through the original single-statement database facade.
    fn write_revision_gap(
        &mut self,
        plan: MaintenancePublicationPlan<'_, 3>,
    ) -> Result<Vec<AdjustmentRevisionGapMarker>, Self::Error> {
        self.query(|session| session.write_revision_gap(plan))
    }
}

impl<Pool: WeatherApiMaintenanceWritePool> ApiMaintenancePredictionWritePort
    for PooledWeatherApiMaintenance<'_, Pool>
{
    type Error = Pool::Error;

    /// Register on one lease and apply the existing recycle/discard policy.
    fn register_api_shadow(
        &mut self,
        plan: ApiMaintenanceRegistrationPlan<'_>,
    ) -> Result<Vec<AdjustmentShadowRegistrationReceipt>, Self::Error> {
        let session = self.pool.checkout_maintenance_write()?;
        run_api_pool_query(
            session,
            |session| session.register_api_shadow(plan),
            Pool::classify_session_failure,
        )
    }

    /// Append on one lease and apply the same recycle/discard policy.
    fn append_api_shadow(
        &mut self,
        plan: ApiMaintenancePredictionPlan<'_>,
    ) -> Result<Vec<AdjustmentShadowPredictionReceipt>, Self::Error> {
        let session = self.pool.checkout_maintenance_write()?;
        run_api_pool_query(
            session,
            |session| session.append_api_shadow(plan),
            Pool::classify_session_failure,
        )
    }
}
