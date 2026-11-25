//! Native PostgreSQL ports for fixed-gauge targets and retained rain inference.

use std::time::SystemTime;

use tokio_postgres::Row;
use weather_contracts::{
    AdjustmentRainGateRevisionPointer, AdjustmentRevisionCommitReceipt,
    AdjustmentRevisionRowReceipt, AdjustmentRevisionRowReceiptContractVersionLiteral,
    AdjustmentWeatherRevisionPointer, AdjustmentWeatherRevisionPointerSourceKindEnum, JsonValue,
    Nullable, PersistedRainAdjustmentRevision, RainAdjustmentCaptureKindEnum, RainAdjustmentHour,
};
use weather_database::{
    AdjustmentMaintenanceDatabasePort, AdjustmentRevisionGapMarker, ApiMaintenancePredictionPlan,
    ApiMaintenancePredictionWritePort, ApiMaintenanceRegistrationPlan, MaintenancePublicationPlan,
    PendingRainAdjustmentCaptureReadRow, PendingRainAdjustmentCapturesReadPlan,
    PendingRainAdjustmentRunReadPlan, PendingRainAdjustmentRunReadRow,
    PendingRainFixedGaugeHourReadPlan, PendingRainFixedGaugeHourReadRow,
    RAIN_FIXED_GAUGE_SOURCE_CATALOG_SQL, RainFixedGaugeAdapterConfigValue,
    RainFixedGaugeCaptureReadRow, RainFixedGaugeHourCapturesReadPlan,
    RainFixedGaugeHourEligibilityReadPlan, RainFixedGaugeSourceCatalogReadPlan,
    RainFixedGaugeSourceReadRow, RainMaintenancePredictionWritePort,
    WorkerRainAdjustmentAppendPool, WorkerRainAdjustmentAppendSession,
    WorkerRainAdjustmentInsertPlan, WorkerRainAdjustmentReadBind, WorkerRainAdjustmentReadPort,
    WorkerRainAdjustmentRevisionBind, WorkerRainAdjustmentRevisionGapPort,
    WorkerRainAdjustmentRevisionPlan, WorkerRainAdjustmentRevisionPort,
    WorkerRainAdjustmentRevisionSavepointCommand, WorkerRainFixedGaugeBindPlan,
    WorkerRainFixedGaugeBoundRow, WorkerRainFixedGaugeCatalogPlan,
    WorkerRainFixedGaugeExistingRowsPlan, WorkerRainFixedGaugeLockPlan,
    WorkerRainFixedGaugeReadBind, WorkerRainFixedGaugeReadError, WorkerRainFixedGaugeReadPort,
    WorkerRainFixedGaugeRunBind, WorkerRainFixedGaugeRunPlan, WorkerRainFixedGaugeSavepointCommand,
    WorkerRainFixedGaugeStoredRow, WorkerRainFixedGaugeWeatherBind,
    WorkerRainFixedGaugeWeatherPlan, WorkerRainFixedGaugeWritePool,
    WorkerRainFixedGaugeWriteSession, read_rain_fixed_gauge_target_source_catalog,
};
use weather_edge::ArchiveWireCodec;

use crate::{
    NativePostgresError, NativePostgresParameter as Parameter, NativePostgresPool,
    NativePostgresSession, NativePostgresType, NativeWirePrimitives, native_json::NativeJson,
    native_postgres_weather_rows as field,
};

const MAXIMUM_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;

/// One retained native session shared by serving inference and fixed-gauge writes.
pub struct NativePostgresWorkerRainSession {
    session: Option<NativePostgresSession>,
}

impl NativePostgresWorkerRainSession {
    /// Retain one checked-out connection until the owning transaction releases it.
    fn new(session: NativePostgresSession) -> Self {
        Self {
            session: Some(session),
        }
    }

    /// Borrow the still-retained PostgreSQL connection.
    fn session_mut(&mut self) -> Result<&mut NativePostgresSession, NativePostgresError> {
        self.session
            .as_mut()
            .ok_or(NativePostgresError::SessionReleased)
    }

    /// Return the connection at most once through the pool's cleanup path.
    fn release_inner(&mut self) {
        // consume ownership before returning the pool slot
        if let Some(session) = self.session.take() {
            let _ = session.release();
        }
    }
}

impl Drop for NativePostgresWorkerRainSession {
    /// Restore and release an abandoned transaction through native session cleanup.
    fn drop(&mut self) {
        self.release_inner();
    }
}

impl WorkerRainFixedGaugeReadPort for NativePostgresSession {
    type Error = NativePostgresError;

    /// Query and decode the immutable fixed-gauge source catalog.
    fn select_fixed_gauge_source_catalog(
        &mut self,
        plan: &RainFixedGaugeSourceCatalogReadPlan,
    ) -> Result<Vec<RainFixedGaugeSourceReadRow>, Self::Error> {
        let [WorkerRainFixedGaugeReadBind::SourceKeys(source_keys)] = &plan.binds else {
            return Err(NativePostgresError::InvalidParameter(
                "fixed gauge source keys",
            ));
        };
        self.query(plan.sql, &[Parameter::TextArray(source_keys.clone())])?
            .iter()
            .map(decode_fixed_gauge_source)
            .collect()
    }

    /// Query at most one latest pending fixed-gauge hour.
    fn select_pending_fixed_gauge_hour(
        &mut self,
        plan: &PendingRainFixedGaugeHourReadPlan,
    ) -> Result<Option<PendingRainFixedGaugeHourReadRow>, Self::Error> {
        let [station_ids, capture_epoch, source_keys, source_count] = &plan.binds;
        let parameters = vec![
            station_ids_parameter(station_ids)?,
            timestamp_parameter(capture_epoch)?,
            source_keys_parameter(source_keys)?,
            unsigned_i64_parameter(source_count, "fixed gauge source count")?,
        ];
        let rows = self.query(plan.sql, &parameters)?;
        optional_single_row(&rows, "pending fixed gauge hour")?
            .map(|row| {
                Ok(PendingRainFixedGaugeHourReadRow {
                    valid_at_ms: timestamp_milliseconds(row, "validAt")?,
                })
            })
            .transpose()
    }

    /// Query the single scalar fixed-gauge hour re-admission result.
    fn select_fixed_gauge_hour_eligibility(
        &mut self,
        plan: &RainFixedGaugeHourEligibilityReadPlan,
    ) -> Result<Option<bool>, Self::Error> {
        let [
            station_ids,
            valid_at,
            source_keys,
            source_count,
            capture_epoch,
        ] = &plan.binds;
        let parameters = vec![
            station_ids_parameter(station_ids)?,
            timestamp_parameter(valid_at)?,
            source_keys_parameter(source_keys)?,
            unsigned_i64_parameter(source_count, "fixed gauge source count")?,
            timestamp_parameter(capture_epoch)?,
        ];
        let rows = self.query(plan.sql, &parameters)?;
        optional_single_row(&rows, "fixed gauge hour eligibility")?
            .map(|row| field::column(row, "eligible"))
            .transpose()
    }

    /// Query and decode the ordered compressed fixed-gauge capture cohort.
    fn select_fixed_gauge_hour_captures(
        &mut self,
        plan: &RainFixedGaugeHourCapturesReadPlan,
    ) -> Result<Vec<RainFixedGaugeCaptureReadRow>, Self::Error> {
        let [station_ids, valid_at, capture_epoch] = &plan.binds;
        let parameters = vec![
            station_ids_parameter(station_ids)?,
            timestamp_parameter(valid_at)?,
            timestamp_parameter(capture_epoch)?,
        ];
        self.query(plan.sql, &parameters)?
            .iter()
            .map(decode_fixed_gauge_capture)
            .collect()
    }
}

impl WorkerRainFixedGaugeReadPort for NativePostgresPool {
    type Error = NativePostgresError;

    /// Execute one source-catalog read through an independently released pool lease.
    fn select_fixed_gauge_source_catalog(
        &mut self,
        plan: &RainFixedGaugeSourceCatalogReadPlan,
    ) -> Result<Vec<RainFixedGaugeSourceReadRow>, Self::Error> {
        with_session(self, |session| {
            WorkerRainFixedGaugeReadPort::select_fixed_gauge_source_catalog(session, plan)
        })
    }

    /// Execute one pending-hour read through an independently released pool lease.
    fn select_pending_fixed_gauge_hour(
        &mut self,
        plan: &PendingRainFixedGaugeHourReadPlan,
    ) -> Result<Option<PendingRainFixedGaugeHourReadRow>, Self::Error> {
        with_session(self, |session| {
            WorkerRainFixedGaugeReadPort::select_pending_fixed_gauge_hour(session, plan)
        })
    }

    /// Execute one eligibility read through an independently released pool lease.
    fn select_fixed_gauge_hour_eligibility(
        &mut self,
        plan: &RainFixedGaugeHourEligibilityReadPlan,
    ) -> Result<Option<bool>, Self::Error> {
        with_session(self, |session| {
            WorkerRainFixedGaugeReadPort::select_fixed_gauge_hour_eligibility(session, plan)
        })
    }

    /// Execute one capture-cohort read through an independently released pool lease.
    fn select_fixed_gauge_hour_captures(
        &mut self,
        plan: &RainFixedGaugeHourCapturesReadPlan,
    ) -> Result<Vec<RainFixedGaugeCaptureReadRow>, Self::Error> {
        with_session(self, |session| {
            WorkerRainFixedGaugeReadPort::select_fixed_gauge_hour_captures(session, plan)
        })
    }
}

impl WorkerRainFixedGaugeWritePool for NativePostgresPool {
    type Error = NativePostgresError;
    type Session = NativePostgresWorkerRainSession;

    /// Check out one connection retained across the complete fixed-gauge transaction.
    fn checkout(&mut self) -> Result<Self::Session, Self::Error> {
        NativePostgresPool::checkout(self).map(NativePostgresWorkerRainSession::new)
    }
}

impl WorkerRainFixedGaugeWriteSession for NativePostgresWorkerRainSession {
    type Error = NativePostgresError;

    /// Begin the all-row fixed-gauge transaction.
    fn begin(&mut self) -> Result<(), Self::Error> {
        self.session_mut()?.begin()
    }

    /// Read and validate the exact frozen source catalog on the retained transaction.
    fn read_source_catalog(
        &mut self,
        plan: WorkerRainFixedGaugeCatalogPlan,
    ) -> Result<weather_contracts::RainFixedGaugeTargetSourceCatalog, Self::Error> {
        // reject a substituted statement before querying catalog material
        if plan.sql != RAIN_FIXED_GAUGE_SOURCE_CATALOG_SQL {
            return Err(NativePostgresError::InvalidParameter(
                "fixed gauge source catalog SQL",
            ));
        }
        let catalog =
            read_rain_fixed_gauge_target_source_catalog(self.session_mut()?).map_err(|error| {
                match error {
                    WorkerRainFixedGaugeReadError::Port(error) => error,
                    WorkerRainFixedGaugeReadError::Admission(error) => NativePostgresError::decode(
                        "fixed gauge source catalog",
                        format!("{error:?}"),
                    ),
                    WorkerRainFixedGaugeReadError::Values(()) => NativePostgresError::decode(
                        "fixed gauge source catalog",
                        "unexpected value-owner failure",
                    ),
                }
            })?;
        // require the caller's plan to describe the same immutable catalog
        if catalog
            .sources
            .iter()
            .map(|source| source.sourceKey.as_str())
            .ne(plan.source_keys)
        {
            return Err(NativePostgresError::InvalidParameter(
                "fixed gauge source catalog",
            ));
        }
        Ok(catalog)
    }

    /// Acquire every transaction-scoped source lock in caller order.
    fn lock_sources(&mut self, plan: WorkerRainFixedGaugeLockPlan<'_>) -> Result<(), Self::Error> {
        let expected = plan.source_ids.len();
        let parameters = [Parameter::TextArray(
            plan.source_ids
                .iter()
                .map(|value| (*value).to_owned())
                .collect(),
        )];
        let rows = self.session_mut()?.query(plan.sql, &parameters)?;
        // require one lock result per requested source identity
        if rows.len() != expected {
            return Err(NativePostgresError::decode(
                "fixed gauge source locks",
                "unexpected row count",
            ));
        }
        Ok(())
    }

    /// Read and decode exact previously stored target rows in source order.
    fn read_existing_target_rows(
        &mut self,
        plan: WorkerRainFixedGaugeExistingRowsPlan<'_>,
    ) -> Result<Vec<WorkerRainFixedGaugeStoredRow>, Self::Error> {
        let parameters = [
            Parameter::I64Array(decimal_ids(&plan.source_ids, "fixed gauge source id")?),
            Parameter::Timestamp(plan.valid_at.to_owned()),
        ];
        self.session_mut()?
            .query(plan.sql, &parameters)?
            .iter()
            .map(|row| {
                Ok(WorkerRainFixedGaugeStoredRow {
                    source_id: field::id(row, "sourceId")?,
                    content_hash: field::column(row, "contentHash")?,
                    valid_at: field::utc(row, "validAt")?,
                })
            })
            .collect()
    }

    /// Insert one provenance run and decode every returned UUID identity.
    fn insert_target_run(
        &mut self,
        plan: WorkerRainFixedGaugeRunPlan<'_>,
    ) -> Result<Vec<String>, Self::Error> {
        let parameters = fixed_gauge_run_parameters(plan.binds)?;
        self.session_mut()?
            .query(plan.sql, &parameters)?
            .iter()
            .map(|row| field::id(row, "id"))
            .collect()
    }

    /// Insert one immutable target row using the exact typed 32-bind plan.
    fn insert_target_weather_row(
        &mut self,
        plan: WorkerRainFixedGaugeWeatherPlan<'_>,
    ) -> Result<u64, Self::Error> {
        let parameters = fixed_gauge_weather_parameters(&plan.binds)?;
        self.session_mut()?.execute(plan.sql, &parameters)
    }

    /// Execute one exact fixed-gauge binder savepoint transition.
    fn execute_savepoint(
        &mut self,
        command: WorkerRainFixedGaugeSavepointCommand,
    ) -> Result<(), Self::Error> {
        self.session_mut()?.batch_execute(command.sql())
    }

    /// Bind one fixed-gauge pointer array and flatten its exact batch receipt.
    fn bind_revision(
        &mut self,
        plan: WorkerRainFixedGaugeBindPlan<'_>,
    ) -> Result<Vec<WorkerRainFixedGaugeBoundRow>, Self::Error> {
        let parameter = json_parameter(weather_pointer_array_value(plan.pointers))?;
        let rows = self.session_mut()?.query(plan.sql, &[parameter])?;
        let mut bound = Vec::new();
        // flatten each server-owned batch wrapper without changing receipt order
        for row in &rows {
            let value = required_json(row, "value")?;
            bound.extend(decode_fixed_gauge_bound_rows(&value)?);
        }
        Ok(bound)
    }

    /// Execute the shared permanent revision-gap facade on this transaction.
    fn write_revision_gap(
        &mut self,
        plan: MaintenancePublicationPlan<'_, 3>,
    ) -> Result<Vec<AdjustmentRevisionGapMarker>, Self::Error> {
        AdjustmentMaintenanceDatabasePort::write_revision_gap(self.session_mut()?, plan)
    }

    /// Commit the complete fixed-gauge transaction.
    fn commit(&mut self) -> Result<(), Self::Error> {
        self.session_mut()?.commit()
    }

    /// Restore the complete fixed-gauge transaction.
    fn rollback(&mut self) -> Result<(), Self::Error> {
        self.session_mut()?.rollback()
    }

    /// Return the retained connection at most once.
    fn release(&mut self) {
        self.release_inner();
    }
}

impl WorkerRainAdjustmentReadPort for NativePostgresSession {
    type Error = NativePostgresError;

    /// Select at most one newest rain run awaiting this immutable model.
    fn select_pending_rain_run(
        &mut self,
        plan: &PendingRainAdjustmentRunReadPlan,
    ) -> Result<Option<PendingRainAdjustmentRunReadRow>, Self::Error> {
        let [WorkerRainAdjustmentReadBind::Sha256(model_sha256)] = &plan.binds else {
            return Err(NativePostgresError::InvalidParameter("rain model digest"));
        };
        let rows = self.query(plan.sql, &[Parameter::Text(model_sha256.clone())])?;
        optional_single_row(&rows, "pending rain adjustment run")?
            .map(|row| {
                Ok(PendingRainAdjustmentRunReadRow {
                    run_initialized_at_ms: timestamp_milliseconds(row, "run_initialized_at")?,
                })
            })
            .transpose()
    }

    /// Select and decode the exact ordered capture cohort for one decision clock.
    fn select_pending_rain_captures(
        &mut self,
        plan: &PendingRainAdjustmentCapturesReadPlan,
    ) -> Result<Vec<PendingRainAdjustmentCaptureReadRow>, Self::Error> {
        let [run_initialized_at, decision_at] = &plan.binds;
        let parameters = [
            rain_read_timestamp(run_initialized_at)?,
            rain_read_timestamp(decision_at)?,
        ];
        self.query(plan.sql, &parameters)?
            .iter()
            .map(decode_pending_rain_capture)
            .collect()
    }
}

impl WorkerRainAdjustmentReadPort for NativePostgresPool {
    type Error = NativePostgresError;

    /// Execute one pending-run read through an independently released pool lease.
    fn select_pending_rain_run(
        &mut self,
        plan: &PendingRainAdjustmentRunReadPlan,
    ) -> Result<Option<PendingRainAdjustmentRunReadRow>, Self::Error> {
        with_session(self, |session| {
            WorkerRainAdjustmentReadPort::select_pending_rain_run(session, plan)
        })
    }

    /// Execute one pending-capture read through an independently released pool lease.
    fn select_pending_rain_captures(
        &mut self,
        plan: &PendingRainAdjustmentCapturesReadPlan,
    ) -> Result<Vec<PendingRainAdjustmentCaptureReadRow>, Self::Error> {
        with_session(self, |session| {
            WorkerRainAdjustmentReadPort::select_pending_rain_captures(session, plan)
        })
    }
}

impl WorkerRainAdjustmentAppendPool for NativePostgresPool {
    type Error = NativePostgresError;
    type Session = NativePostgresWorkerRainSession;

    /// Check out one connection retained across serving append and revision custody.
    fn checkout(&mut self) -> Result<Self::Session, Self::Error> {
        NativePostgresPool::checkout(self).map(NativePostgresWorkerRainSession::new)
    }
}

impl WorkerRainAdjustmentAppendSession for NativePostgresWorkerRainSession {
    type Error = NativePostgresError;

    /// Begin the serving append transaction.
    fn begin(&mut self) -> Result<(), Self::Error> {
        self.session_mut()?.begin()
    }

    /// Insert one rain serving row and decode the exact database-authored revision.
    fn insert(
        &mut self,
        plan: WorkerRainAdjustmentInsertPlan<'_>,
    ) -> Result<Option<PersistedRainAdjustmentRevision>, Self::Error> {
        let parameters = rain_adjustment_insert_parameters(plan.binds);
        let rows = self.session_mut()?.query(plan.sql, &parameters)?;
        optional_single_row(&rows, "rain adjustment insert")?
            .map(decode_persisted_rain_adjustment)
            .transpose()
    }

    /// Commit the serving append transaction.
    fn commit(&mut self) -> Result<(), Self::Error> {
        self.session_mut()?.commit()
    }

    /// Restore the serving append transaction.
    fn rollback(&mut self) -> Result<(), Self::Error> {
        self.session_mut()?.rollback()
    }

    /// Return the retained connection at most once.
    fn release(&mut self) {
        self.release_inner();
    }
}

impl WorkerRainAdjustmentRevisionPort for NativePostgresWorkerRainSession {
    type Error = NativePostgresError;

    /// Execute one exact rain revision savepoint transition.
    fn execute_rain_revision_savepoint(
        &mut self,
        command: WorkerRainAdjustmentRevisionSavepointCommand,
    ) -> Result<(), Self::Error> {
        self.session_mut()?.batch_execute(command.sql())
    }

    /// Bind one rain-gate pointer and decode its exact row receipt wrapper.
    fn bind_rain_gate_revision(
        &mut self,
        plan: WorkerRainAdjustmentRevisionPlan<'_>,
    ) -> Result<Vec<AdjustmentRevisionRowReceipt>, Self::Error> {
        let [WorkerRainAdjustmentRevisionBind::PointerJson(pointer)] = plan.binds;
        let parameter = json_parameter(rain_pointer_value(pointer))?;
        self.session_mut()?
            .query(plan.sql, &[parameter])?
            .iter()
            .map(|row| decode_revision_row_receipt(&required_json(row, "value")?))
            .collect()
    }
}

impl WorkerRainAdjustmentRevisionGapPort for NativePostgresWorkerRainSession {
    type Error = NativePostgresError;

    /// Execute one shared permanent gap facade on the retained serving transaction.
    fn write_rain_revision_gap(
        &mut self,
        plan: MaintenancePublicationPlan<'_, 3>,
    ) -> Result<Vec<AdjustmentRevisionGapMarker>, Self::Error> {
        AdjustmentMaintenanceDatabasePort::write_revision_gap(self.session_mut()?, plan)
    }
}

impl WorkerRainAdjustmentRevisionGapPort for NativePostgresPool {
    type Error = NativePostgresError;

    /// Execute one post-commit gap facade through an independently released lease.
    fn write_rain_revision_gap(
        &mut self,
        plan: MaintenancePublicationPlan<'_, 3>,
    ) -> Result<Vec<AdjustmentRevisionGapMarker>, Self::Error> {
        with_session(self, |session| {
            AdjustmentMaintenanceDatabasePort::write_revision_gap(session, plan)
        })
    }
}

impl RainMaintenancePredictionWritePort for NativePostgresSession {
    type Error = NativePostgresError;

    /// Reuse the exact native API maintenance registration decoder for rain.
    fn register_rain_shadow(
        &mut self,
        plan: ApiMaintenanceRegistrationPlan<'_>,
    ) -> Result<Vec<weather_contracts::AdjustmentShadowRegistrationReceipt>, Self::Error> {
        ApiMaintenancePredictionWritePort::register_api_shadow(self, plan)
    }

    /// Reuse the exact native API maintenance prediction decoder for rain.
    fn append_rain_shadow(
        &mut self,
        plan: ApiMaintenancePredictionPlan<'_>,
    ) -> Result<Vec<weather_contracts::AdjustmentShadowPredictionReceipt>, Self::Error> {
        ApiMaintenancePredictionWritePort::append_api_shadow(self, plan)
    }
}

impl RainMaintenancePredictionWritePort for NativePostgresPool {
    type Error = NativePostgresError;

    /// Execute one rain registration through an independently released lease.
    fn register_rain_shadow(
        &mut self,
        plan: ApiMaintenanceRegistrationPlan<'_>,
    ) -> Result<Vec<weather_contracts::AdjustmentShadowRegistrationReceipt>, Self::Error> {
        with_session(self, |session| {
            RainMaintenancePredictionWritePort::register_rain_shadow(session, plan)
        })
    }

    /// Execute one rain prediction append through an independently released lease.
    fn append_rain_shadow(
        &mut self,
        plan: ApiMaintenancePredictionPlan<'_>,
    ) -> Result<Vec<weather_contracts::AdjustmentShadowPredictionReceipt>, Self::Error> {
        with_session(self, |session| {
            RainMaintenancePredictionWritePort::append_rain_shadow(session, plan)
        })
    }
}

/// Execute one pool-scoped operation and preserve its primary failure on cleanup.
fn with_session<T>(
    pool: &NativePostgresPool,
    operation: impl FnOnce(&mut NativePostgresSession) -> Result<T, NativePostgresError>,
) -> Result<T, NativePostgresError> {
    let mut session = pool.checkout()?;
    let result = operation(&mut session);
    let release = session.release();
    // report cleanup only after a successful operation
    match result {
        Ok(value) => {
            release?;
            Ok(value)
        }
        Err(error) => Err(error),
    }
}

/// Require at most one row for a scalar or optional exact query.
fn optional_single_row<'a>(
    rows: &'a [Row],
    label: &str,
) -> Result<Option<&'a Row>, NativePostgresError> {
    // reject ambiguous cardinality rather than selecting one row
    match rows {
        [] => Ok(None),
        [row] => Ok(Some(row)),
        _ => Err(NativePostgresError::decode(label, "multiple rows")),
    }
}

/// Decode one native timestamp as predecessor-compatible epoch milliseconds.
fn timestamp_milliseconds(row: &Row, name: &str) -> Result<i64, NativePostgresError> {
    field::epoch_milliseconds(field::column::<SystemTime>(row, name)?)
        .ok_or_else(|| NativePostgresError::decode(name, "timestamp is out of range"))
}

/// Decode one nullable native timestamp as epoch milliseconds.
fn optional_timestamp_milliseconds(
    row: &Row,
    name: &str,
) -> Result<Option<i64>, NativePostgresError> {
    field::column::<Option<SystemTime>>(row, name)?
        .map(|value| {
            field::epoch_milliseconds(value)
                .ok_or_else(|| NativePostgresError::decode(name, "timestamp is out of range"))
        })
        .transpose()
}

/// Convert one fixed-gauge station-id array to PostgreSQL's integer array domain.
fn station_ids_parameter(
    bind: &WorkerRainFixedGaugeReadBind,
) -> Result<Parameter, NativePostgresError> {
    let WorkerRainFixedGaugeReadBind::StationIds(values) = bind else {
        return Err(NativePostgresError::InvalidParameter(
            "fixed gauge station ids",
        ));
    };
    values
        .iter()
        .map(|value| i32::try_from(*value).map_err(|_| NativePostgresError::ParameterOutOfRange))
        .collect::<Result<Vec<_>, _>>()
        .map(Parameter::I32Array)
}

/// Convert one fixed-gauge UTC bind to PostgreSQL timestamptz.
fn timestamp_parameter(
    bind: &WorkerRainFixedGaugeReadBind,
) -> Result<Parameter, NativePostgresError> {
    let WorkerRainFixedGaugeReadBind::UtcInstant(value) = bind else {
        return Err(NativePostgresError::InvalidParameter(
            "fixed gauge UTC instant",
        ));
    };
    Ok(Parameter::Timestamp(value.clone()))
}

/// Convert one fixed-gauge source-key bind to PostgreSQL text array.
fn source_keys_parameter(
    bind: &WorkerRainFixedGaugeReadBind,
) -> Result<Parameter, NativePostgresError> {
    let WorkerRainFixedGaugeReadBind::SourceKeys(values) = bind else {
        return Err(NativePostgresError::InvalidParameter(
            "fixed gauge source keys",
        ));
    };
    Ok(Parameter::TextArray(values.clone()))
}

/// Convert one unsigned equality bind to PostgreSQL bigint without wrapping.
fn unsigned_i64_parameter(
    bind: &WorkerRainFixedGaugeReadBind,
    label: &'static str,
) -> Result<Parameter, NativePostgresError> {
    let WorkerRainFixedGaugeReadBind::Unsigned(value) = bind else {
        return Err(NativePostgresError::InvalidParameter(label));
    };
    i64::try_from(*value)
        .map(Parameter::I64)
        .map_err(|_| NativePostgresError::ParameterOutOfRange)
}

/// Decode one fixed-gauge catalog row without coercing its JSON scalars.
fn decode_fixed_gauge_source(
    row: &Row,
) -> Result<RainFixedGaugeSourceReadRow, NativePostgresError> {
    Ok(RainFixedGaugeSourceReadRow {
        source_id: field::id(row, "sourceId")?,
        source_key: field::column(row, "sourceKey")?,
        source_kind: field::column(row, "sourceKind")?,
        adapter_config: decode_adapter_config(required_json(row, "adapterConfig")?)?,
        source_config_fingerprint: field::column(row, "sourceConfigFingerprint")?,
        capabilities: field::column(row, "capabilities")?,
        cadence_seconds: optional_unsigned(row, "cadenceSeconds")?,
        active: field::column(row, "active")?,
        station_key: field::column(row, "stationKey")?,
        station_kind: field::column(row, "stationKind")?,
        latitude: field::column(row, "latitude")?,
        longitude: field::column(row, "longitude")?,
        serial: field::column(row, "serial")?,
        station_active: field::column(row, "stationActive")?,
        provider_key: field::column(row, "providerKey")?,
        provider_active: field::column(row, "providerActive")?,
        site_key: field::column(row, "siteKey")?,
        timezone: field::column(row, "timezone")?,
        site_active: field::column(row, "siteActive")?,
    })
}

/// Decode one nullable signed database count as an unsigned domain value.
fn optional_unsigned(row: &Row, name: &str) -> Result<Option<u64>, NativePostgresError> {
    field::column::<Option<i32>>(row, name)?
        .map(|value| u64::try_from(value).map_err(|error| NativePostgresError::decode(name, error)))
        .transpose()
}

/// Decode the closed fixed-gauge adapter object into its retained scalar domain.
fn decode_adapter_config(
    value: JsonValue,
) -> Result<Vec<(String, RainFixedGaugeAdapterConfigValue)>, NativePostgresError> {
    let JsonValue::Object(fields) = value else {
        return Err(NativePostgresError::decode(
            "adapterConfig",
            "expected JSON object",
        ));
    };
    fields
        .into_iter()
        .map(|(key, value)| {
            let key = String::from_utf16(&key)
                .map_err(|error| NativePostgresError::decode("adapterConfig", error))?;
            let value = match value {
                JsonValue::String(units) => RainFixedGaugeAdapterConfigValue::Text(
                    String::from_utf16(&units)
                        .map_err(|error| NativePostgresError::decode("adapterConfig", error))?,
                ),
                JsonValue::Number(number)
                    if number.is_finite()
                        && (0.0..=MAXIMUM_SAFE_INTEGER).contains(&number)
                        && number.fract() == 0.0 =>
                {
                    RainFixedGaugeAdapterConfigValue::Unsigned(number as u64)
                }
                _ => {
                    return Err(NativePostgresError::decode(
                        "adapterConfig",
                        "expected text or safe unsigned integer",
                    ));
                }
            };
            Ok((key, value))
        })
        .collect()
}

/// Decode one complete fixed-gauge raw capture row.
fn decode_fixed_gauge_capture(
    row: &Row,
) -> Result<RainFixedGaugeCaptureReadRow, NativePostgresError> {
    Ok(RainFixedGaugeCaptureReadRow {
        body_sha256: field::column(row, "bodySha256")?,
        claim_id: field::id(row, "claimId")?,
        completed_at_ms: timestamp_milliseconds(row, "completedAt")?,
        compressed_body: field::column(row, "compressedBody")?,
        station_id: field::unsigned(row, "stationId")?,
        valid_at_ms: timestamp_milliseconds(row, "validAt")?,
        window_end_exclusive_ms: timestamp_milliseconds(row, "windowEndExclusive")?,
        window_start_ms: timestamp_milliseconds(row, "windowStart")?,
    })
}

/// Parse positive decimal identities for PostgreSQL bigint-array binds.
fn decimal_ids(values: &[&str], label: &str) -> Result<Vec<i64>, NativePostgresError> {
    values
        .iter()
        .map(|value| {
            value
                .parse::<i64>()
                .ok()
                .filter(|value| *value > 0)
                .ok_or_else(|| NativePostgresError::decode(label, "invalid positive bigint"))
        })
        .collect()
}

/// Bind one fixed-gauge provenance run using the SQL plan's exact field domains.
fn fixed_gauge_run_parameters(
    binds: [WorkerRainFixedGaugeRunBind<'_>; 8],
) -> Result<[Parameter; 8], NativePostgresError> {
    let [
        WorkerRainFixedGaugeRunBind::Text(source_id),
        WorkerRainFixedGaugeRunBind::Text(requested_start),
        WorkerRainFixedGaugeRunBind::Text(requested_end),
        WorkerRainFixedGaugeRunBind::Text(source_fingerprint),
        WorkerRainFixedGaugeRunBind::Text(adapter_version),
        WorkerRainFixedGaugeRunBind::Json(request_metadata),
        WorkerRainFixedGaugeRunBind::Json(response_metadata),
        WorkerRainFixedGaugeRunBind::Text(response_checksum),
    ] = binds
    else {
        return Err(NativePostgresError::InvalidParameter(
            "fixed gauge provenance run",
        ));
    };
    Ok([
        Parameter::I64(positive_i64(source_id, "fixed gauge source id")?),
        Parameter::Timestamp(requested_start.to_owned()),
        Parameter::Timestamp(requested_end.to_owned()),
        Parameter::Text(source_fingerprint.to_owned()),
        Parameter::Text(adapter_version.to_owned()),
        Parameter::Json(request_metadata.to_owned()),
        Parameter::Json(response_metadata.to_owned()),
        Parameter::Text(response_checksum.to_owned()),
    ])
}

/// Bind one fixed-gauge target row using each SQL placeholder's actual domain.
fn fixed_gauge_weather_parameters(
    binds: &[WorkerRainFixedGaugeWeatherBind<'_>; 32],
) -> Result<Vec<Parameter>, NativePostgresError> {
    binds
        .iter()
        .enumerate()
        .map(|(index, bind)| match (index, bind) {
            (0, WorkerRainFixedGaugeWeatherBind::Text(value)) => {
                positive_i64(value, "fixed gauge source id").map(Parameter::I64)
            }
            (2 | 5, WorkerRainFixedGaugeWeatherBind::Text(value)) => {
                Ok(Parameter::Timestamp((*value).to_owned()))
            }
            (3, WorkerRainFixedGaugeWeatherBind::NullableText(value)) => Ok(value
                .map_or(Parameter::Null(NativePostgresType::Timestamp), |value| {
                    Parameter::Timestamp(value.to_owned())
                })),
            (4, WorkerRainFixedGaugeWeatherBind::Text(value)) => {
                Ok(Parameter::Uuid((*value).to_owned()))
            }
            (11 | 12, WorkerRainFixedGaugeWeatherBind::Json(value)) => {
                Ok(Parameter::Json((*value).to_owned()))
            }
            (13..=30, WorkerRainFixedGaugeWeatherBind::Number(value)) => {
                Ok(value.map_or(Parameter::Null(NativePostgresType::F64), Parameter::F64))
            }
            (_, WorkerRainFixedGaugeWeatherBind::Text(value)) => {
                Ok(Parameter::Text((*value).to_owned()))
            }
            (_, WorkerRainFixedGaugeWeatherBind::NullableText(value)) => Ok(value
                .map_or(Parameter::Null(NativePostgresType::Text), |value| {
                    Parameter::Text(value.to_owned())
                })),
            _ => Err(NativePostgresError::InvalidParameter(
                "fixed gauge weather row",
            )),
        })
        .collect()
}

/// Parse one positive decimal identity without accepting signed aliases.
fn positive_i64(value: &str, label: &str) -> Result<i64, NativePostgresError> {
    value
        .parse::<i64>()
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(|| NativePostgresError::decode(label, "invalid positive bigint"))
}

/// Decode one pending rain capture from native UUID, timestamp, and bytea columns.
fn decode_pending_rain_capture(
    row: &Row,
) -> Result<PendingRainAdjustmentCaptureReadRow, NativePostgresError> {
    let kind = match field::column::<String>(row, "kind")?.as_str() {
        "forecast" => RainAdjustmentCaptureKindEnum::Forecast,
        "station" => RainAdjustmentCaptureKindEnum::Station,
        _ => {
            return Err(NativePostgresError::decode(
                "kind",
                "unknown rain capture kind",
            ));
        }
    };
    Ok(PendingRainAdjustmentCaptureReadRow {
        claim_id: field::id(row, "id")?,
        kind,
        station_id: optional_unsigned(row, "station_id")?,
        run_initialized_at_ms: optional_timestamp_milliseconds(row, "run_initialized_at")?,
        window_start_ms: optional_timestamp_milliseconds(row, "window_start")?,
        window_end_exclusive_ms: optional_timestamp_milliseconds(row, "window_end_exclusive")?,
        completed_at_ms: timestamp_milliseconds(row, "completed_at")?,
        body_sha256: field::column(row, "body_sha256")?,
        compressed_body: field::column(row, "compressed_body")?,
    })
}

/// Convert one rain-read UTC bind to PostgreSQL timestamptz.
fn rain_read_timestamp(
    bind: &WorkerRainAdjustmentReadBind,
) -> Result<Parameter, NativePostgresError> {
    let WorkerRainAdjustmentReadBind::UtcInstant(value) = bind else {
        return Err(NativePostgresError::InvalidParameter("rain UTC instant"));
    };
    Ok(Parameter::Timestamp(value.clone()))
}

/// Bind the original seven-field rain serving insert to exact PostgreSQL types.
fn rain_adjustment_insert_parameters(binds: [&str; 7]) -> [Parameter; 7] {
    [
        Parameter::Timestamp(binds[0].to_owned()),
        Parameter::Text(binds[1].to_owned()),
        Parameter::Text(binds[2].to_owned()),
        Parameter::Uuid(binds[3].to_owned()),
        Parameter::Timestamp(binds[4].to_owned()),
        Parameter::Timestamp(binds[5].to_owned()),
        Parameter::Json(binds[6].to_owned()),
    ]
}

/// Decode one inserted rain serving row and its exact stored hours array.
fn decode_persisted_rain_adjustment(
    row: &Row,
) -> Result<PersistedRainAdjustmentRevision, NativePostgresError> {
    Ok(PersistedRainAdjustmentRevision {
        generatedAt: field::utc(row, "generatedAt")?,
        hours: decode_rain_hours(required_json(row, "hours")?)?,
        inputSha256: field::column(row, "inputSha256")?,
        modelSha256: field::column(row, "modelSha256")?,
        runInitializedAt: field::utc(row, "runInitializedAt")?,
        storedContentSha256: field::column(row, "storedContentSha256")?,
    })
}

/// Decode the exact persisted rain-hour array without scalar coercion.
fn decode_rain_hours(value: JsonValue) -> Result<Vec<RainAdjustmentHour>, NativePostgresError> {
    let JsonValue::Array(hours) = value else {
        return Err(NativePostgresError::decode("hours", "expected JSON array"));
    };
    hours
        .iter()
        .map(|hour| {
            exact_object(
                hour,
                &[
                    "validAt",
                    "modelLeadHours",
                    "rawPrecipitationMm",
                    "correctedPrecipitationMm",
                    "applied",
                    "reasonCode",
                ],
                "rain adjustment hour",
            )?;
            Ok(RainAdjustmentHour {
                validAt: json_text(hour, "validAt")?,
                modelLeadHours: json_unsigned(hour, "modelLeadHours")?,
                rawPrecipitationMm: json_number(hour, "rawPrecipitationMm")?,
                correctedPrecipitationMm: json_number(hour, "correctedPrecipitationMm")?,
                applied: json_boolean(hour, "applied")?,
                reasonCode: json_nullable_text(hour, "reasonCode")?,
            })
        })
        .collect()
}

/// Encode one fixed-gauge pointer array in its generated field order.
fn weather_pointer_array_value(pointers: &[AdjustmentWeatherRevisionPointer; 1]) -> JsonValue {
    JsonValue::Array(
        pointers
            .iter()
            .map(|pointer| {
                JsonValue::Object(vec![
                    member("productRunAt", nullable_string(&pointer.productRunAt)),
                    member(
                        "projectionIdentitySha256",
                        string(&pointer.projectionIdentitySha256),
                    ),
                    member("projectionSha256", string(&pointer.projectionSha256)),
                    member("sourceId", string(&pointer.sourceId)),
                    member(
                        "sourceKind",
                        string(pointer_source_kind(pointer.sourceKind)),
                    ),
                    member("stageReceiptSha256", string(&pointer.stageReceiptSha256)),
                    member("storedContentSha256", string(&pointer.storedContentSha256)),
                    member("validAt", string(&pointer.validAt)),
                ])
            })
            .collect(),
    )
}

/// Encode one rain-gate pointer in its generated field order.
fn rain_pointer_value(pointer: &AdjustmentRainGateRevisionPointer) -> JsonValue {
    JsonValue::Object(vec![
        member("inputSha256", string(&pointer.inputSha256)),
        member("modelSha256", string(&pointer.modelSha256)),
        member(
            "projectionIdentitySha256",
            string(&pointer.projectionIdentitySha256),
        ),
        member("projectionSha256", string(&pointer.projectionSha256)),
        member("runInitializedAt", string(&pointer.runInitializedAt)),
        member("stageReceiptSha256", string(&pointer.stageReceiptSha256)),
        member("storedContentSha256", string(&pointer.storedContentSha256)),
    ])
}

/// Return the exact generated wire value for one weather source kind.
const fn pointer_source_kind(kind: AdjustmentWeatherRevisionPointerSourceKindEnum) -> &'static str {
    // preserve the closed generated source-kind alternatives
    match kind {
        AdjustmentWeatherRevisionPointerSourceKindEnum::Forecast => "forecast",
        AdjustmentWeatherRevisionPointerSourceKindEnum::PhysicalSensor => "physical_sensor",
    }
}

/// Encode one ordered JSON value as a compact JSONB parameter.
fn json_parameter(value: JsonValue) -> Result<Parameter, NativePostgresError> {
    let mut bytes = NativeJson::encode_ordered(&value)
        .ok_or_else(|| NativePostgresError::decode("JSON parameter", "encoding failed"))?;
    // remove only the archive serializer's framing newline
    if bytes.pop() != Some(b'\n') {
        return Err(NativePostgresError::decode(
            "JSON parameter",
            "encoded JSON is not newline framed",
        ));
    }
    String::from_utf8(bytes)
        .map(Parameter::Json)
        .map_err(|error| NativePostgresError::decode("JSON parameter", error))
}

/// Require one non-null JSON/JSONB result column.
fn required_json(row: &Row, name: &str) -> Result<JsonValue, NativePostgresError> {
    field::json(row, name)?
        .ok_or_else(|| NativePostgresError::decode(name, "database JSON result is null"))
}

/// Decode and flatten the exact fixed-gauge revision batch wrapper.
fn decode_fixed_gauge_bound_rows(
    value: &JsonValue,
) -> Result<Vec<WorkerRainFixedGaugeBoundRow>, NativePostgresError> {
    exact_object(value, &["contractVersion", "receipts"], "revision batch")?;
    // require the fixed batch wrapper before decoding entries
    if json_text(value, "contractVersion")? != "adjustment-revision-batch-receipt/v1" {
        return Err(NativePostgresError::decode(
            "contractVersion",
            "unexpected revision batch version",
        ));
    }
    let JsonValue::Array(receipts) = json_member(value, "receipts")? else {
        return Err(NativePostgresError::decode(
            "receipts",
            "expected JSON array",
        ));
    };
    receipts
        .iter()
        .map(|receipt| {
            exact_object(
                receipt,
                &[
                    "projectionKind",
                    "revisionReceipt",
                    "sourceId",
                    "storedContentSha256",
                    "validAt",
                ],
                "weather revision batch entry",
            )?;
            let revision_receipt = decode_commit_receipt(json_member(receipt, "revisionReceipt")?)?;
            // bind the redundant wrapper discriminator to the authenticated receipt
            if json_text(receipt, "projectionKind")?
                != commit_projection_kind(revision_receipt.projectionKind)
            {
                return Err(NativePostgresError::decode(
                    "projectionKind",
                    "revision batch kind differs from receipt",
                ));
            }
            Ok(WorkerRainFixedGaugeBoundRow {
                source_id: json_text(receipt, "sourceId")?,
                valid_at: json_text(receipt, "validAt")?,
                stored_content_sha256: json_text(receipt, "storedContentSha256")?,
                revision_receipt,
            })
        })
        .collect()
}

/// Decode one exact rain revision row wrapper.
fn decode_revision_row_receipt(
    value: &JsonValue,
) -> Result<AdjustmentRevisionRowReceipt, NativePostgresError> {
    exact_object(
        value,
        &["contractVersion", "revisionReceipt"],
        "revision row receipt",
    )?;
    // require the fixed row wrapper before decoding its authenticated receipt
    if json_text(value, "contractVersion")? != "adjustment-revision-row-receipt/v1" {
        return Err(NativePostgresError::decode(
            "contractVersion",
            "unexpected revision row version",
        ));
    }
    Ok(AdjustmentRevisionRowReceipt {
        contractVersion: AdjustmentRevisionRowReceiptContractVersionLiteral,
        revisionReceipt: decode_commit_receipt(json_member(value, "revisionReceipt")?)?,
    })
}

/// Decode one authenticated server-owned revision commit receipt.
fn decode_commit_receipt(
    value: &JsonValue,
) -> Result<AdjustmentRevisionCommitReceipt, NativePostgresError> {
    ArchiveWireCodec::new(NativeWirePrimitives)
        .decode_revision_commit_value(value)
        .ok_or_else(|| NativePostgresError::decode("revision receipt", "contract mismatch"))
}

/// Return the exact generated wire value for one commit projection kind.
const fn commit_projection_kind(
    kind: weather_contracts::AdjustmentRevisionCommitReceiptProjectionKindEnum,
) -> &'static str {
    // preserve every generated commit projection alternative
    match kind {
        weather_contracts::AdjustmentRevisionCommitReceiptProjectionKindEnum::ActualBestMatch => {
            "actual_best_match"
        }
        weather_contracts::AdjustmentRevisionCommitReceiptProjectionKindEnum::NativeSource => {
            "native_source"
        }
        weather_contracts::AdjustmentRevisionCommitReceiptProjectionKindEnum::RainGateInput => {
            "rain_gate_input"
        }
        weather_contracts::AdjustmentRevisionCommitReceiptProjectionKindEnum::ShadowPrediction => {
            "shadow_prediction"
        }
        weather_contracts::AdjustmentRevisionCommitReceiptProjectionKindEnum::TargetRevision => {
            "target_revision"
        }
    }
}

/// Require one exact object key set without accepting duplicates or extensions.
fn exact_object<'a>(
    value: &'a JsonValue,
    keys: &[&str],
    label: &str,
) -> Result<&'a [(Vec<u16>, JsonValue)], NativePostgresError> {
    let JsonValue::Object(fields) = value else {
        return Err(NativePostgresError::decode(label, "expected JSON object"));
    };
    // require every expected key exactly once and reject all extensions
    if fields.len() != keys.len()
        || !keys.iter().all(|key| {
            fields
                .iter()
                .filter(|(name, _)| name.iter().copied().eq(key.encode_utf16()))
                .count()
                == 1
        })
    {
        return Err(NativePostgresError::decode(label, "unexpected JSON keys"));
    }
    Ok(fields)
}

/// Borrow one named JSON object member without key normalization.
fn json_member<'a>(value: &'a JsonValue, name: &str) -> Result<&'a JsonValue, NativePostgresError> {
    let JsonValue::Object(fields) = value else {
        return Err(NativePostgresError::decode(name, "expected JSON object"));
    };
    fields
        .iter()
        .find_map(|(key, value)| key.iter().copied().eq(name.encode_utf16()).then_some(value))
        .ok_or_else(|| NativePostgresError::decode(name, "missing JSON field"))
}

/// Decode one required JSON string without scalar coercion.
fn json_text(value: &JsonValue, name: &str) -> Result<String, NativePostgresError> {
    let JsonValue::String(units) = json_member(value, name)? else {
        return Err(NativePostgresError::decode(name, "expected JSON string"));
    };
    String::from_utf16(units).map_err(|error| NativePostgresError::decode(name, error))
}

/// Decode one required finite JSON number without scalar coercion.
fn json_number(value: &JsonValue, name: &str) -> Result<f64, NativePostgresError> {
    let JsonValue::Number(number) = json_member(value, name)? else {
        return Err(NativePostgresError::decode(name, "expected JSON number"));
    };
    // preserve the generated finite-number rule
    if !number.is_finite() {
        return Err(NativePostgresError::decode(name, "number is not finite"));
    }
    Ok(*number)
}

/// Decode one required safe unsigned JSON integer.
fn json_unsigned(value: &JsonValue, name: &str) -> Result<u64, NativePostgresError> {
    let number = json_number(value, name)?;
    // reject fractional, negative, or rounded JavaScript integer values
    if !(0.0..=MAXIMUM_SAFE_INTEGER).contains(&number) || number.fract() != 0.0 {
        return Err(NativePostgresError::decode(
            name,
            "expected safe unsigned integer",
        ));
    }
    Ok(number as u64)
}

/// Decode one required JSON boolean without scalar coercion.
fn json_boolean(value: &JsonValue, name: &str) -> Result<bool, NativePostgresError> {
    let JsonValue::Boolean(value) = json_member(value, name)? else {
        return Err(NativePostgresError::decode(name, "expected JSON boolean"));
    };
    Ok(*value)
}

/// Decode one explicit JSON null or string into the generated nullable domain.
fn json_nullable_text(
    value: &JsonValue,
    name: &str,
) -> Result<Nullable<String>, NativePostgresError> {
    // retain explicit null instead of accepting a missing member
    match json_member(value, name)? {
        JsonValue::Null => Ok(Nullable::Null),
        JsonValue::String(units) => String::from_utf16(units)
            .map(Nullable::Value)
            .map_err(|error| NativePostgresError::decode(name, error)),
        _ => Err(NativePostgresError::decode(
            name,
            "expected JSON string or null",
        )),
    }
}

/// Build one ordered UTF-16 JSON object member.
fn member(name: &str, value: JsonValue) -> (Vec<u16>, JsonValue) {
    (name.encode_utf16().collect(), value)
}

/// Build one JSON string without changing its UTF-16 spelling.
fn string(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}

/// Project one generated nullable string to JSON null or string.
fn nullable_string(value: &Nullable<String>) -> JsonValue {
    // preserve the generated nullable discriminator exactly
    match value {
        Nullable::Null => JsonValue::Null,
        Nullable::Value(value) => string(value),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Map the original rain serving insert to typed UUID, UTC and JSON fields.
    #[test]
    fn native_worker_rain_serving_insert_binds_exact_domains() {
        let run = "2026-11-24T12:00:00.000Z";
        let decision = "2026-11-24T20:00:00.000Z";
        let claim = "01234567-89ab-4cde-8123-0123456789ab";
        let parameters = rain_adjustment_insert_parameters([
            run,
            "model-sha",
            "source-sha",
            claim,
            run,
            decision,
            "[]",
        ]);
        assert_eq!(parameters[0], Parameter::Timestamp(run.to_owned()));
        assert_eq!(parameters[3], Parameter::Uuid(claim.to_owned()));
        assert_eq!(parameters[5], Parameter::Timestamp(decision.to_owned()));
        assert_eq!(parameters[6], Parameter::Json("[]".to_owned()));
    }

    /// Preserve the original fixed-gauge run identity beyond JavaScript safe integer precision.
    #[test]
    fn native_worker_fixed_gauge_run_binds_exact_bigint_and_json() {
        use WorkerRainFixedGaugeRunBind::*;
        let start = "2026-11-24T12:00:00.000Z";
        let end = "2026-11-24T13:00:00.000Z";
        let parameters = fixed_gauge_run_parameters([
            Text("9007199254740993"),
            Text(start),
            Text(end),
            Text("fingerprint"),
            Text("adapter/v1"),
            Json("{}"),
            Json("{}"),
            Text("checksum"),
        ])
        .unwrap();
        assert_eq!(parameters[0], Parameter::I64(9_007_199_254_740_993));
        assert_eq!(parameters[1], Parameter::Timestamp(start.to_owned()));
        assert_eq!(parameters[5], Parameter::Json("{}".to_owned()));
    }
}
