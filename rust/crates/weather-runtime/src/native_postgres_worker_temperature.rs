//! Native PostgreSQL execution for the retained worker temperature canary path.

use tokio_postgres::Row;
use weather_contracts::{
    EcmwfTemperatureCanaryRecentErrorState, EcmwfTemperatureCanaryRecentErrorStateCohortLiteral,
    EcmwfTemperatureCanaryRunAdapterVersionLiteral, EcmwfTemperatureCanaryRunModelCycleEnum,
    EcmwfTemperatureCanaryRunStateStatusEnum, EcmwfTemperatureCanaryRunUpstreamModelLiteral,
    ForecastObservationHourlyMetrics, ForecastObservationHourlyStationRow,
    ForecastObservationProviderFamily, ForecastObservationStationKey, JsDateOrString, JsonValue,
    Nullable, PersistedEcmwfTemperatureRevision,
};
use weather_database::{
    AdmittedEcmwfCanaryBestMatchRow, AdmittedRevisionUtcInstant, CausalEcmwfPriorHourDbRow,
    CausalEcmwfPriorHoursReadPlan, CausalForecastObservationReadPlan, CausalTemperatureReadBind,
    EcmwfCanaryBestMatchQuery, EcmwfCanaryExistingHoursQuery, EcmwfCanaryExistingRunQuery,
    EcmwfCanaryHourInsertQuery, EcmwfCanaryLogicalReceiptQuery, EcmwfCanaryPersistenceBind,
    EcmwfCanaryPersistencePort, EcmwfCanaryReceiptUpdateQuery, EcmwfCanaryRevisionArchivePort,
    EcmwfCanaryRevisionSavepointCommand, EcmwfCanaryRunInsertQuery, EcmwfCanarySiteQuery,
    EcmwfCanarySiteRow, EcmwfTemperatureCanaryHourReadRow, EcmwfTemperatureCanaryReadBind,
    EcmwfTemperatureCanaryReadPort, EcmwfTemperatureCanaryRunInitializationsReadPlan,
    EcmwfTemperatureCanarySidecarHoursReadPlan, EcmwfTemperatureCanarySidecarRunReadPlan,
    EcmwfTemperatureCanaryStatusReadPlan, EcmwfTemperatureCanaryStatusReadRow,
    StoredEcmwfCanaryHourIdentity, StoredEcmwfCanaryRun,
};
use weather_edge::{ArchiveWirePrimitives, RevisionArchiveCanonicalCodec};
use weather_worker::TaskFailure;

use crate::{
    AdmittedCausalEcmwfPriorHourReadRow, AdmittedCausalTemperatureStationReadRow,
    CausalTemperatureDatabasePort, NativePostgresError, NativePostgresParameter as Parameter,
    NativePostgresPool, NativePostgresSession, NativePostgresType, NativeWirePrimitives,
    TemperatureCanaryPersistenceDatabasePort, native_postgres_weather_rows as field,
};

const HOUR_MILLISECONDS: i64 = 3_600_000;
const FIVE_MINUTES_MILLISECONDS: i64 = 300_000;

/// One retained worker session shared by canary reads and persistence transactions.
pub struct NativePostgresTemperatureSession {
    pool: NativePostgresPool,
    session: Option<NativePostgresSession>,
}

impl NativePostgresTemperatureSession {
    /// Checkout one retained session from the process pool.
    pub fn checkout(pool: &NativePostgresPool) -> Result<Self, NativePostgresError> {
        pool.checkout().map(|session| Self {
            pool: pool.clone(),
            session: Some(session),
        })
    }

    /// Bind a reusable lazy adapter to the process pool without opening a connection.
    pub fn from_pool(pool: NativePostgresPool) -> Self {
        Self {
            pool,
            session: None,
        }
    }

    /// Reborrow the same retained session for revision archive binding and gap writes.
    pub fn session_mut(&mut self) -> Result<&mut NativePostgresSession, NativePostgresError> {
        self.session()
    }

    /// Borrow the retained session until the persistence engine releases it.
    fn session(&mut self) -> Result<&mut NativePostgresSession, NativePostgresError> {
        // reacquire after the preceding persistence transaction released its client
        if self.session.is_none() {
            self.session = Some(self.pool.checkout()?);
        }
        self.session
            .as_mut()
            .ok_or(NativePostgresError::SessionReleased)
    }

    /// Return the retained session after the transaction engine completes.
    pub fn release(&mut self) {
        // release at most once because the trait lifecycle can call this after any outcome
        if let Some(session) = self.session.take() {
            let _ = session.release();
        }
    }
}

impl EcmwfCanaryPersistencePort for NativePostgresTemperatureSession {
    type Error = NativePostgresError;

    /// Begin the exact canary transaction.
    fn begin_transaction(&mut self) -> Result<(), Self::Error> {
        self.session()?.begin()
    }

    /// Read the active site identity.
    fn select_site(
        &mut self,
        query: &EcmwfCanarySiteQuery,
    ) -> Result<Option<EcmwfCanarySiteRow>, Self::Error> {
        let parameters = persistence_parameters(&query.binds)?;
        first(self.session()?, query.sql, &parameters, |row| {
            Ok(EcmwfCanarySiteRow {
                id: field::id(row, "id")?,
                latitude: field::column(row, "latitude")?,
                longitude: field::column(row, "longitude")?,
            })
        })
    }

    /// Insert one immutable canary run.
    fn insert_run(
        &mut self,
        query: &EcmwfCanaryRunInsertQuery,
    ) -> Result<Option<StoredEcmwfCanaryRun>, Self::Error> {
        let parameters = persistence_parameters_with_bigints(&query.binds, &[0])?;
        first(self.session()?, query.sql, &parameters, canary_run)
    }

    /// Insert one immutable canary hour.
    fn insert_hour(&mut self, query: &EcmwfCanaryHourInsertQuery) -> Result<(), Self::Error> {
        let parameters = persistence_parameters_with_bigints(&query.binds, &[0])?;
        self.session()?.execute(query.sql, &parameters).map(|_| ())
    }

    /// Lock and decode one existing canary run.
    fn select_existing_run(
        &mut self,
        query: &EcmwfCanaryExistingRunQuery,
    ) -> Result<Option<StoredEcmwfCanaryRun>, Self::Error> {
        let parameters = persistence_parameters_with_bigints(&query.binds, &[0])?;
        first(self.session()?, query.sql, &parameters, canary_run)
    }

    /// Read immutable hour hashes in lead order.
    fn select_existing_hours(
        &mut self,
        query: &EcmwfCanaryExistingHoursQuery,
    ) -> Result<Vec<StoredEcmwfCanaryHourIdentity>, Self::Error> {
        let parameters = persistence_parameters_with_bigints(&query.binds, &[0])?;
        rows(self.session()?, query.sql, &parameters, |row| {
            Ok(StoredEcmwfCanaryHourIdentity {
                content_hash: field::digest(row, "contentHash")?,
            })
        })
    }

    /// Advance the last receipt monotonically.
    fn update_last_received(
        &mut self,
        query: &EcmwfCanaryReceiptUpdateQuery,
    ) -> Result<(), Self::Error> {
        let parameters = persistence_parameters_with_bigints(&query.binds, &[0])?;
        self.session()?.execute(query.sql, &parameters).map(|_| ())
    }

    /// Commit the serving transaction.
    fn commit_transaction(&mut self) -> Result<(), Self::Error> {
        self.session()?.commit()
    }

    /// Roll back a failed serving transaction.
    fn rollback_transaction(&mut self) -> Result<(), Self::Error> {
        self.session()?.rollback()
    }

    /// Return the retained client after every persistence attempt.
    fn release_client(&mut self) {
        self.release();
    }
}

impl EcmwfCanaryRevisionArchivePort for NativePostgresTemperatureSession {
    type Publication = ();

    /// Read the database-owned causal receipt.
    fn select_logical_receipt(
        &mut self,
        query: &EcmwfCanaryLogicalReceiptQuery,
    ) -> Result<Option<AdmittedRevisionUtcInstant>, Self::Error> {
        first(self.session()?, query.sql, &[], |row| {
            field::optional_utc(row, "logicalReceivedAt")?.ok_or_else(|| {
                NativePostgresError::decode("logicalReceivedAt", "missing timestamp")
            })
        })
    }

    /// Execute the source-owned outer archive savepoint command.
    fn execute_revision_savepoint(
        &mut self,
        command: EcmwfCanaryRevisionSavepointCommand,
    ) -> Result<(), Self::Error> {
        self.session()?.batch_execute(command.sql())
    }

    /// Decode the exact Best Match rows visible at the causal receipt.
    fn select_best_match_rows(
        &mut self,
        query: &EcmwfCanaryBestMatchQuery,
    ) -> Result<Vec<AdmittedEcmwfCanaryBestMatchRow>, Self::Error> {
        let parameters = persistence_parameters(&query.binds)?;
        rows(self.session()?, query.sql, &parameters, |row| {
            let weather = field::weather(row)?;
            let valid_at_epoch_milliseconds = date_epoch(&weather.validAt, "validAt")?;
            let product_run_at_epoch_milliseconds = match &weather.productRunAt {
                Nullable::Null => None,
                Nullable::Value(value) => Some(date_epoch(value, "productRunAt")?),
            };
            Ok(AdmittedEcmwfCanaryBestMatchRow {
                content_hash: field::digest(row, "contentHash")?,
                product_run_at_epoch_milliseconds,
                row: weather,
                valid_at_epoch_milliseconds,
            })
        })
    }

    /// Refuse archive staging until the owning cold-store adapter is composed.
    fn stage_ecmwf_temperature_revision(
        &mut self,
        _revision: &PersistedEcmwfTemperatureRevision,
    ) -> Result<Option<Self::Publication>, Self::Error> {
        Err(NativePostgresError::InvalidParameter(
            "ECMWF revision archive adapter",
        ))
    }

    /// Reject an impossible publication from the non-archiving worker session.
    fn publish_ecmwf_temperature_revision(
        &mut self,
        _publication: Self::Publication,
    ) -> Result<(), Self::Error> {
        Err(NativePostgresError::InvalidParameter(
            "ECMWF revision archive publication",
        ))
    }
}

impl TemperatureCanaryPersistenceDatabasePort for NativePostgresTemperatureSession {
    /// Read the bounded warmup inventory in initialization order.
    fn read_temperature_canary_run_initializations(
        &mut self,
        plan: &EcmwfTemperatureCanaryRunInitializationsReadPlan,
    ) -> Result<Vec<JsDateOrString>, Self::Error> {
        let parameters = canary_read_parameters(&plan.binds)?;
        rows(self.session()?, plan.sql, &parameters, |row| {
            field::date(row, "runInitializedAt")
        })
    }
}

impl EcmwfTemperatureCanaryReadPort for NativePostgresTemperatureSession {
    type Error = NativePostgresError;

    /// Delegate the selected sidecar run read to the shared native row adapter.
    fn select_sidecar_run(
        &mut self,
        plan: &EcmwfTemperatureCanarySidecarRunReadPlan,
    ) -> Result<Option<StoredEcmwfCanaryRun>, Self::Error> {
        EcmwfTemperatureCanaryReadPort::select_sidecar_run(self.session()?, plan)
    }

    /// Delegate the ordered sidecar hour read to the shared native row adapter.
    fn select_sidecar_hours(
        &mut self,
        plan: &EcmwfTemperatureCanarySidecarHoursReadPlan,
    ) -> Result<Vec<EcmwfTemperatureCanaryHourReadRow>, Self::Error> {
        EcmwfTemperatureCanaryReadPort::select_sidecar_hours(self.session()?, plan)
    }

    /// Delegate the independent collector-status read to the shared native row adapter.
    fn select_status(
        &mut self,
        plan: &EcmwfTemperatureCanaryStatusReadPlan,
    ) -> Result<Option<EcmwfTemperatureCanaryStatusReadRow>, Self::Error> {
        EcmwfTemperatureCanaryReadPort::select_status(self.session()?, plan)
    }
}

impl CausalTemperatureDatabasePort for NativePostgresPool {
    /// Execute and decode the exact causal prior-run rows.
    fn read_causal_ecmwf_prior_hours(
        &mut self,
        plan: &CausalEcmwfPriorHoursReadPlan,
    ) -> Result<Vec<AdmittedCausalEcmwfPriorHourReadRow>, TaskFailure> {
        let mut session = self.checkout().map_err(|_| causal_failure())?;
        let parameters = causal_parameters(&plan.binds).map_err(|_| causal_failure())?;
        let result = rows(&mut session, plan.sql, &parameters, causal_prior_row)
            .map_err(|_| causal_failure());
        let _ = session.release();
        result
    }

    /// Execute the accepted raw-source query and reproduce hourly station projection.
    fn read_causal_temperature_station_hours(
        &mut self,
        plan: &CausalForecastObservationReadPlan,
    ) -> Result<Vec<AdmittedCausalTemperatureStationReadRow>, TaskFailure> {
        let [
            CausalTemperatureReadBind::Text(_),
            CausalTemperatureReadBind::UtcInstant(from),
            CausalTemperatureReadBind::UtcInstant(to),
            CausalTemperatureReadBind::UtcInstant(_),
            CausalTemperatureReadBind::TextArray(_),
        ] = &plan.binds
        else {
            return Err(causal_failure());
        };
        let mut session = self.checkout().map_err(|_| causal_failure())?;
        let parameters = causal_parameters(&plan.binds).map_err(|_| causal_failure())?;
        let result = rows(&mut session, plan.sql, &parameters, raw_station_row)
            .and_then(|raw| project_temperature_stations(raw, from, to))
            .map_err(|_| causal_failure());
        let _ = session.release();
        result
    }
}

/// Convert one persistence bind into its exact PostgreSQL binary type.
fn persistence_parameter(
    bind: &EcmwfCanaryPersistenceBind,
) -> Result<Parameter, NativePostgresError> {
    match bind {
        EcmwfCanaryPersistenceBind::Count(value) => i16::try_from(*value)
            .map(Parameter::I16)
            .map_err(|_| NativePostgresError::ParameterOutOfRange),
        EcmwfCanaryPersistenceBind::Digest(value) => Ok(Parameter::Text(value.as_str().to_owned())),
        EcmwfCanaryPersistenceBind::Json(value) => Ok(Parameter::Json(value.as_str().to_owned())),
        EcmwfCanaryPersistenceBind::NullableNumber(Some(value)) => Ok(Parameter::F64(*value)),
        EcmwfCanaryPersistenceBind::NullableNumber(None) => {
            Ok(Parameter::Null(NativePostgresType::F64))
        }
        EcmwfCanaryPersistenceBind::Number(value) => Ok(Parameter::F64(*value)),
        EcmwfCanaryPersistenceBind::Text(value) => Ok(Parameter::Text(value.clone())),
        EcmwfCanaryPersistenceBind::UtcInstant(value) => timestamp(value),
    }
}

/// Convert one fixed persistence bind array without changing order.
fn persistence_parameters<const N: usize>(
    binds: &[EcmwfCanaryPersistenceBind; N],
) -> Result<Vec<Parameter>, NativePostgresError> {
    binds.iter().map(persistence_parameter).collect()
}

/// Convert identifier text binds to bigint only at source-owned placeholder positions.
fn persistence_parameters_with_bigints<const N: usize>(
    binds: &[EcmwfCanaryPersistenceBind; N],
    bigint_indices: &[usize],
) -> Result<Vec<Parameter>, NativePostgresError> {
    binds
        .iter()
        .enumerate()
        .map(|(index, bind)| {
            // preserve ordinary text outside the explicitly bigint placeholder domain
            if bigint_indices.contains(&index) {
                let EcmwfCanaryPersistenceBind::Text(value) = bind else {
                    return Err(NativePostgresError::InvalidParameter("bigint bind"));
                };
                return value
                    .parse::<i64>()
                    .map(Parameter::I64)
                    .map_err(|error| NativePostgresError::decode("bigint bind", error));
            }
            persistence_parameter(bind)
        })
        .collect()
}

/// Convert one warmup read bind array without changing order.
fn canary_read_parameters<const N: usize>(
    binds: &[EcmwfTemperatureCanaryReadBind; N],
) -> Result<Vec<Parameter>, NativePostgresError> {
    binds
        .iter()
        .map(|bind| match bind {
            EcmwfTemperatureCanaryReadBind::Text(value) => Ok(Parameter::Text(value.clone())),
            EcmwfTemperatureCanaryReadBind::UtcInstant(value) => timestamp(value),
        })
        .collect()
}

/// Convert one causal read bind array without changing order.
fn causal_parameters<const N: usize>(
    binds: &[CausalTemperatureReadBind; N],
) -> Result<Vec<Parameter>, NativePostgresError> {
    binds
        .iter()
        .map(|bind| match bind {
            CausalTemperatureReadBind::Text(value) => Ok(Parameter::Text(value.clone())),
            CausalTemperatureReadBind::TextArray(values) => {
                Ok(Parameter::TextArray(values.clone()))
            }
            CausalTemperatureReadBind::UtcInstant(value) => timestamp(value),
        })
        .collect()
}

/// Bind one already admitted canonical UTC timestamp.
fn timestamp(value: &str) -> Result<Parameter, NativePostgresError> {
    NativeWirePrimitives
        .canonical_utc_milliseconds(value)
        .map(|_| Parameter::Timestamp(value.to_owned()))
        .ok_or_else(|| NativePostgresError::decode("timestamp parameter", "invalid UTC instant"))
}

/// Decode exactly zero or one row.
fn first<T>(
    session: &mut NativePostgresSession,
    sql: &str,
    parameters: &[Parameter],
    decode: impl FnOnce(&Row) -> Result<T, NativePostgresError>,
) -> Result<Option<T>, NativePostgresError> {
    let rows = session.query(sql, parameters)?;
    // reject non-unique singleton statements
    if rows.len() > 1 {
        return Err(NativePostgresError::decode(
            "query",
            "expected at most one row",
        ));
    }
    rows.first().map(decode).transpose()
}

/// Decode every returned row without hiding a post-query failure.
fn rows<T>(
    session: &mut NativePostgresSession,
    sql: &str,
    parameters: &[Parameter],
    mut decode: impl FnMut(&Row) -> Result<T, NativePostgresError>,
) -> Result<Vec<T>, NativePostgresError> {
    session
        .query(sql, parameters)?
        .iter()
        .map(&mut decode)
        .collect()
}

/// Decode one trusted stored canary run.
fn canary_run(row: &Row) -> Result<StoredEcmwfCanaryRun, NativePostgresError> {
    let adapter = field::column::<String>(row, "adapterVersion")?;
    let upstream = field::column::<String>(row, "upstreamModel")?;
    // reject drift from the generated literal identities
    if adapter != EcmwfTemperatureCanaryRunAdapterVersionLiteral::WIRE_VALUE
        || upstream != EcmwfTemperatureCanaryRunUpstreamModelLiteral::WIRE_VALUE
    {
        return Err(NativePostgresError::decode(
            "canary identity",
            "unsupported adapter or upstream model",
        ));
    }
    let recent_error = field::json(row, "recentErrorState")?
        .ok_or_else(|| NativePostgresError::decode("recentErrorState", "missing state"))?;
    Ok(StoredEcmwfCanaryRun {
        adapter_version: EcmwfTemperatureCanaryRunAdapterVersionLiteral,
        content_hash: field::digest(row, "contentHash")?,
        first_received_at: field::utc(row, "firstReceivedAt")?,
        id: field::id(row, "id")?,
        model_cycle: field::wire_enum(
            row,
            "modelCycle",
            EcmwfTemperatureCanaryRunModelCycleEnum::WIRE_VALUES,
        )?,
        provider_response_sha256: field::digest(row, "providerResponseSha256")?,
        recent_error_state: recent_error_state(&recent_error).ok_or_else(|| {
            NativePostgresError::decode("recentErrorState", "invalid canary state")
        })?,
        recent_error_state_sha256: field::digest(row, "recentErrorStateSha256")?,
        run_initialized_at: field::utc(row, "runInitializedAt")?,
        state_reason: field::column(row, "stateReason")?,
        state_status: field::wire_enum(
            row,
            "stateStatus",
            EcmwfTemperatureCanaryRunStateStatusEnum::WIRE_VALUES,
        )?,
        upstream_model: EcmwfTemperatureCanaryRunUpstreamModelLiteral,
    })
}

/// Decode the closed recent-error JSON contract.
fn recent_error_state(value: &JsonValue) -> Option<EcmwfTemperatureCanaryRecentErrorState> {
    use crate::maintenance_canonical_json::{field as get, text};
    let number = |key| match get(value, key)? {
        JsonValue::Null => Some(Nullable::Null),
        JsonValue::Number(number) if number.is_finite() => Some(Nullable::Value(*number)),
        _ => None,
    };
    let nullable_text = |key| match get(value, key)? {
        JsonValue::Null => Some(Nullable::Null),
        JsonValue::String(units) => String::from_utf16(units).ok().map(Nullable::Value),
        _ => None,
    };
    let unsigned = |key| match get(value, key)? {
        JsonValue::Number(number)
            if (0.0..=9_007_199_254_740_991.0).contains(number) && number.fract() == 0.0 =>
        {
            Some(*number as u64)
        }
        _ => None,
    };
    // require the exact generated cohort literal
    if text(value, "cohort")? != EcmwfTemperatureCanaryRecentErrorStateCohortLiteral::WIRE_VALUE {
        return None;
    }
    let JsonValue::Array(keys) = get(value, "sourceKeys")? else {
        return None;
    };
    let source_keys = keys
        .iter()
        .map(|key| match key {
            JsonValue::String(units) => String::from_utf16(units).ok(),
            _ => None,
        })
        .collect::<Option<Vec<_>>>()?;
    let JsonValue::Boolean(supported) = get(value, "supported")? else {
        return None;
    };
    Some(EcmwfTemperatureCanaryRecentErrorState {
        b24C: number("b24C")?,
        b72C: number("b72C")?,
        cohort: EcmwfTemperatureCanaryRecentErrorStateCohortLiteral,
        localDates: unsigned("localDates")?,
        mad72C: number("mad72C")?,
        maximumSourceRunInitializedAt: nullable_text("maximumSourceRunInitializedAt")?,
        maximumSourceValidAt: nullable_text("maximumSourceValidAt")?,
        n24: unsigned("n24")?,
        n72: unsigned("n72")?,
        sourceKeys: source_keys,
        supported: *supported,
        targetRunInitializedAt: text(value, "targetRunInitializedAt")?,
        windowEndValidAt: text(value, "windowEndValidAt")?,
    })
}

/// Convert one generated database date into its admitted epoch.
fn date_epoch(value: &JsDateOrString, name: &str) -> Result<i64, NativePostgresError> {
    match value {
        JsDateOrString::DateEpochMilliseconds(epoch) => Ok(*epoch),
        JsDateOrString::String(units) => String::from_utf16(units)
            .ok()
            .and_then(|value| NativeWirePrimitives.parse_explicit_utc_milliseconds(&value))
            .ok_or_else(|| NativePostgresError::decode(name, "invalid timestamp")),
        JsDateOrString::InvalidDate => Err(NativePostgresError::decode(name, "invalid timestamp")),
    }
}

/// Decode one causal prior-run database row and its clock evidence.
fn causal_prior_row(row: &Row) -> Result<AdmittedCausalEcmwfPriorHourReadRow, NativePostgresError> {
    let run_initialized_at = field::utc(row, "runInitializedAt")?;
    let valid_at = field::utc(row, "validAt")?;
    Ok(AdmittedCausalEcmwfPriorHourReadRow {
        run_initialized_at_epoch_ms: NativeWirePrimitives
            .canonical_utc_milliseconds(&run_initialized_at)
            .ok_or_else(|| NativePostgresError::decode("runInitializedAt", "invalid timestamp"))?,
        valid_at_epoch_ms: NativeWirePrimitives
            .canonical_utc_milliseconds(&valid_at)
            .ok_or_else(|| NativePostgresError::decode("validAt", "invalid timestamp"))?,
        row: CausalEcmwfPriorHourDbRow {
            content_hash: field::column(row, "contentHash")?,
            model_lead_hours: field::unsigned(row, "modelLeadHours")?,
            raw_relative_humidity_percent: field::nullable(row, "rawRelativeHumidityPercent")?,
            raw_temperature_c: field::column(row, "rawTemperatureC")?,
            raw_wind_speed_mps: field::nullable(row, "rawWindSpeedMps")?,
            run_initialized_at,
            valid_at,
        },
    })
}

#[derive(Clone)]
struct RawStationRow {
    accepted: bool,
    id: String,
    physical_station_key: ForecastObservationStationKey,
    source_key: String,
    temperature_c: Nullable<f64>,
    valid_at_epoch_ms: i64,
}

/// Decode and validate one accepted source-lineage row needed by the temperature reducer.
fn raw_station_row(row: &Row) -> Result<RawStationRow, NativePostgresError> {
    let source_key = field::column::<String>(row, "sourceKey")?;
    let lineage = lineage(&source_key)
        .ok_or_else(|| NativePostgresError::decode("sourceKey", "unexpected source lineage"))?;
    let station_slug = field::column::<String>(row, "stationSlug")?;
    let fingerprint = field::column::<String>(row, "sourceConfigFingerprint")?;
    let adapter = field::column::<String>(row, "adapterContract")?;
    // fail closed on immutable source drift
    if station_slug != lineage.station_slug
        || fingerprint != lineage.fingerprint
        || adapter != lineage.adapter
    {
        return Err(NativePostgresError::decode(
            "source lineage",
            "forecast observation lineage mismatch",
        ));
    }
    let valid_at = field::utc(row, "validAt")?;
    let valid_at_epoch_ms = NativeWirePrimitives
        .canonical_utc_milliseconds(&valid_at)
        .ok_or_else(|| NativePostgresError::decode("validAt", "invalid timestamp"))?;
    let start = lineage
        .start
        .map(|start| {
            NativeWirePrimitives
                .parse_explicit_utc_milliseconds(start)
                .ok_or_else(|| NativePostgresError::decode("source lineage", "invalid start"))
        })
        .transpose()?;
    let end = lineage
        .end
        .map(|end| {
            NativeWirePrimitives
                .parse_explicit_utc_milliseconds(end)
                .ok_or_else(|| NativePostgresError::decode("source lineage", "invalid end"))
        })
        .transpose()?;
    let in_interval = !start.is_some_and(|start| valid_at_epoch_ms < start)
        && !end.is_some_and(|end| valid_at_epoch_ms >= end);
    let quality_accepted = quality_accepted(
        field::json(row, "qualityMetadata")?.as_ref(),
        lineage.tempest,
        lineage.wu,
    );
    Ok(RawStationRow {
        accepted: in_interval && quality_accepted,
        id: field::id(row, "id")?,
        physical_station_key: lineage.station,
        source_key,
        temperature_c: field::nullable(row, "temperatureC")?,
        valid_at_epoch_ms,
    })
}

/// Project exact hourly temperature observations for every frozen physical station.
fn project_temperature_stations(
    mut rows: Vec<RawStationRow>,
    from: &str,
    to: &str,
) -> Result<Vec<AdmittedCausalTemperatureStationReadRow>, NativePostgresError> {
    let from_ms = NativeWirePrimitives
        .canonical_utc_milliseconds(from)
        .ok_or_else(|| NativePostgresError::decode("from", "invalid timestamp"))?;
    let to_ms = NativeWirePrimitives
        .canonical_utc_milliseconds(to)
        .ok_or_else(|| NativePostgresError::decode("to", "invalid timestamp"))?;
    // exclude literal interval and quality rejections before collision ordering
    rows.retain(|row| row.accepted);
    rows.sort_by(|left, right| {
        left.valid_at_epoch_ms
            .cmp(&right.valid_at_epoch_ms)
            .then_with(|| {
                station_rank(left.physical_station_key)
                    .cmp(&station_rank(right.physical_station_key))
            })
            .then_with(|| left.id.cmp(&right.id))
    });
    // reject every accepted duplicate station/timestamp temperature before projection
    if rows.windows(2).any(|pair| {
        pair[0].physical_station_key == pair[1].physical_station_key
            && pair[0].valid_at_epoch_ms == pair[1].valid_at_epoch_ms
            && matches!(&pair[0].temperature_c, Nullable::Value(_))
            && matches!(&pair[1].temperature_c, Nullable::Value(_))
    }) {
        return Err(NativePostgresError::decode(
            "temperatureC",
            "forecast observation collision",
        ));
    }
    let mut projected = Vec::new();
    let mut hour = from_ms;
    // emit the source's half-open sequence of UTC hours
    while hour < to_ms {
        let valid_at = NativeWirePrimitives
            .format_utc_milliseconds(hour)
            .ok_or_else(|| NativePostgresError::decode("validAt", "invalid timestamp"))?;
        // emit every frozen physical station in manifest order
        for (station, provider) in stations() {
            let selected = closest_temperature(&rows, station, hour)?;
            let (temperature, source_keys) = selected.map_or((Nullable::Null, Vec::new()), |row| {
                (row.temperature_c.clone(), vec![row.source_key.clone()])
            });
            projected.push(AdmittedCausalTemperatureStationReadRow {
                row: ForecastObservationHourlyStationRow {
                    metrics: ForecastObservationHourlyMetrics {
                        relativeHumidityPercent: Nullable::Null,
                        temperatureC: temperature,
                        windDirectionDegrees: Nullable::Null,
                        windGustMps: Nullable::Null,
                        windSpeedMps: Nullable::Null,
                    },
                    physicalStationKey: station,
                    providerFamily: provider,
                    sourceKeys: source_keys,
                    validAt: valid_at.clone(),
                },
                valid_at_epoch_ms: hour,
            });
        }
        hour = hour
            .checked_add(HOUR_MILLISECONDS)
            .ok_or_else(|| NativePostgresError::decode("validAt", "hour overflow"))?;
    }
    Ok(projected)
}

/// Select the closest non-null temperature within the exact five-minute half-open window.
fn closest_temperature(
    rows: &[RawStationRow],
    station: ForecastObservationStationKey,
    hour: i64,
) -> Result<Option<&RawStationRow>, NativePostgresError> {
    let mut candidates = rows
        .iter()
        .filter(|row| {
            row.physical_station_key == station
                && row.valid_at_epoch_ms >= hour - FIVE_MINUTES_MILLISECONDS
                && row.valid_at_epoch_ms < hour + FIVE_MINUTES_MILLISECONDS
                && matches!(&row.temperature_c, Nullable::Value(value) if value.is_finite())
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| {
        left.valid_at_epoch_ms
            .abs_diff(hour)
            .cmp(&right.valid_at_epoch_ms.abs_diff(hour))
            .then_with(|| left.valid_at_epoch_ms.cmp(&right.valid_at_epoch_ms))
            .then_with(|| left.id.cmp(&right.id))
    });
    // reject post-precedence duplicate temperature observations
    if candidates
        .windows(2)
        .any(|pair| pair[0].valid_at_epoch_ms == pair[1].valid_at_epoch_ms)
    {
        return Err(NativePostgresError::decode(
            "temperatureC",
            "forecast observation collision",
        ));
    }
    Ok(candidates.first().copied())
}

/// Preserve the worker's stable failure classification at the database boundary.
fn causal_failure() -> TaskFailure {
    TaskFailure {
        attempts: 0,
        code: "temperature recent-error database read failed".to_owned(),
    }
}

#[derive(Clone, Copy)]
struct Lineage {
    adapter: &'static str,
    end: Option<&'static str>,
    fingerprint: &'static str,
    start: Option<&'static str>,
    station: ForecastObservationStationKey,
    station_slug: &'static str,
    tempest: bool,
    wu: bool,
}

/// Resolve one exact accepted source lineage without accepting aliases.
fn lineage(source: &str) -> Option<Lineage> {
    let ordinary = |adapter, fingerprint, start, station, station_slug| Lineage {
        adapter,
        end: None,
        fingerprint,
        start,
        station,
        station_slug,
        tempest: false,
        wu: false,
    };
    Some(match source {
        "ambient-maxweather-observations-v1" => ordinary(
            "ambient-device-data/v1",
            "7a7528a6278924ca5280a1a6045b6647b7e660b112d7fa3008c542a17ff99df4",
            Some("2026-08-24T00:00:00Z"),
            ForecastObservationStationKey::AmbientMaxweather,
            "ambient-maxweather",
        ),
        "ambient-merlin-observations-v1" => ordinary(
            "ambient-device-data/v1",
            "c3829701bfc25a050022dc3965569d3a87376e8a43b5fdcb7621533f1ae3c65d",
            Some("2021-01-01T00:00:00Z"),
            ForecastObservationStationKey::AmbientMerlin,
            "ambient-merlin",
        ),
        "ecowitt-88f15505d89f-local-live-v1" => ordinary(
            "ecowitt-local-live/v1",
            "0a44488714d0fa807b924f8aea14965b437722e8cf9f8eae4bc8c81da8a0149d",
            None,
            ForecastObservationStationKey::BallydideanEcowitt,
            "ballydidean-ecowitt",
        ),
        "netatmo-nearby-observations-v1" => ordinary(
            "netatmo-public-measures/v1",
            "5495917dd2465a32d9878e73c68781a229b432901cdc4867875726351efbdbbc",
            Some("2022-06-21T00:00:00Z"),
            ForecastObservationStationKey::NetatmoNearby,
            "netatmo-nearby",
        ),
        "tempest-126537-observations-v2" => tempest_lineage(
            "34dafbd6584c93d55ed4d3d43dc7e74a0876165d4ddfc921413f7b826dff7ab7",
            "2023-12-17T00:00:00Z",
            ForecastObservationStationKey::Tempest126537,
            "tempest-126537",
        ),
        "tempest-168853-observations-v2" => tempest_lineage(
            "1c7a402337a44a5441775246cbc02da7994599dad6ab83dc04b248303facfea5",
            "2025-01-22T00:00:00Z",
            ForecastObservationStationKey::Tempest168853,
            "tempest-168853",
        ),
        "tempest-201058-observations-v2" => tempest_lineage(
            "a61cce798cddf682da9608dc245659fc7734a6d5304068939d3115ae7d81a50e",
            "2025-12-22T00:00:00Z",
            ForecastObservationStationKey::Tempest201058,
            "tempest-201058",
        ),
        "tempest-203055-observations-v2" => tempest_lineage(
            "9ead4c5359a6a9640f334be91397180aa62b90b0f0ce813b9ff26fe84537acc4",
            "2025-12-25T00:00:00Z",
            ForecastObservationStationKey::Tempest203055,
            "tempest-203055",
        ),
        "tempest-225947-observations-v2" => tempest_lineage(
            "b4dd6105d9a56a7c5d0dc4063f830e1cf28d693222a8de15536dd83d3a6178c4",
            "2026-07-14T00:00:00Z",
            ForecastObservationStationKey::Tempest225947,
            "tempest-225947",
        ),
        "tempest-38270-observations-v2" => tempest_lineage(
            "ce162067aced4ab3522fb83145a21e608ff24dec189097726188e96fd6cca52f",
            "2021-01-04T00:00:00Z",
            ForecastObservationStationKey::Tempest38270,
            "tempest-38270",
        ),
        "tempest-64255-observations-v2" => tempest_lineage(
            "8eb488a358375fc3526347d9ef6c9f23080095a22ea874a42ec400b0317d868a",
            "2021-12-10T00:00:00Z",
            ForecastObservationStationKey::Tempest64255,
            "tempest-64255",
        ),
        "wunderground-maxweather-history-v1" => Lineage {
            adapter: "wunderground-pws-history/v1",
            end: Some("2026-08-24T00:00:00Z"),
            fingerprint: "52dda6c5444d0a234fbe23d6218027d417ac966ecf291a7d5dfff42fd0dc207c",
            start: Some("2024-11-29T00:00:00Z"),
            station: ForecastObservationStationKey::AmbientMaxweather,
            station_slug: "ambient-maxweather",
            tempest: false,
            wu: true,
        },
        _ => return None,
    })
}

/// Build one frozen Tempest lineage entry.
fn tempest_lineage(
    fingerprint: &'static str,
    start: &'static str,
    station: ForecastObservationStationKey,
    station_slug: &'static str,
) -> Lineage {
    Lineage {
        adapter: "tempest-observations/v2",
        end: None,
        fingerprint,
        start: Some(start),
        station,
        station_slug,
        tempest: true,
        wu: false,
    }
}

/// Apply the source's literal quality allowlists.
fn quality_accepted(value: Option<&JsonValue>, tempest: bool, wu: bool) -> bool {
    let Some(JsonValue::Object(fields)) = value else {
        return value.is_none() || matches!(value, Some(JsonValue::Null));
    };
    let field = |name: &str| {
        fields.iter().find_map(|(key, value)| {
            String::from_utf16(key)
                .ok()
                .filter(|key| key == name)
                .map(|_| value)
        })
    };
    let flags_ok = match field("flags") {
        None => true,
        Some(JsonValue::Array(flags)) if tempest => flags.iter().all(|flag| {
            matches!(flag, JsonValue::String(units) if String::from_utf16(units).ok().as_deref() == Some("uv_index_out_of_range"))
        }),
        Some(JsonValue::Array(flags)) => flags.is_empty(),
        _ => false,
    };
    let status_ok = match field("status") {
        None => true,
        Some(JsonValue::String(units)) if wu => {
            String::from_utf16(units).ok().as_deref() == Some("provider_qc_1")
        }
        _ => false,
    };
    flags_ok && status_ok
}

/// Return the frozen station roster in predecessor order.
fn stations() -> [(
    ForecastObservationStationKey,
    ForecastObservationProviderFamily,
); 11] {
    [
        (
            ForecastObservationStationKey::AmbientMaxweather,
            ForecastObservationProviderFamily::Ambient,
        ),
        (
            ForecastObservationStationKey::AmbientMerlin,
            ForecastObservationProviderFamily::Ambient,
        ),
        (
            ForecastObservationStationKey::BallydideanEcowitt,
            ForecastObservationProviderFamily::Ecowitt,
        ),
        (
            ForecastObservationStationKey::NetatmoNearby,
            ForecastObservationProviderFamily::Netatmo,
        ),
        (
            ForecastObservationStationKey::Tempest126537,
            ForecastObservationProviderFamily::Tempest,
        ),
        (
            ForecastObservationStationKey::Tempest168853,
            ForecastObservationProviderFamily::Tempest,
        ),
        (
            ForecastObservationStationKey::Tempest201058,
            ForecastObservationProviderFamily::Tempest,
        ),
        (
            ForecastObservationStationKey::Tempest203055,
            ForecastObservationProviderFamily::Tempest,
        ),
        (
            ForecastObservationStationKey::Tempest225947,
            ForecastObservationProviderFamily::Tempest,
        ),
        (
            ForecastObservationStationKey::Tempest38270,
            ForecastObservationProviderFamily::Tempest,
        ),
        (
            ForecastObservationStationKey::Tempest64255,
            ForecastObservationProviderFamily::Tempest,
        ),
    ]
}

/// Map station enum values to their frozen lexical order.
fn station_rank(station: ForecastObservationStationKey) -> u8 {
    stations()
        .iter()
        .position(|(candidate, _)| *candidate == station)
        .map_or(u8::MAX, |index| index as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Preserve exact run identity, smallint lead, nullable metrics, and canonical UTC.
    #[test]
    fn native_postgres_worker_temperature_binds_exact_hour_types() {
        use EcmwfCanaryPersistenceBind::*;
        let binds = [
            Text("9007199254740993".to_owned()),
            Count(12),
            UtcInstant("2026-11-24T12:00:00.000Z".to_owned()),
            Number(7.5),
            NullableNumber(None),
        ];
        let parameters = persistence_parameters_with_bigints(&binds, &[0]).unwrap();
        assert_eq!(
            parameters,
            [
                Parameter::I64(9_007_199_254_740_993),
                Parameter::I16(12),
                Parameter::Timestamp("2026-11-24T12:00:00.000Z".to_owned()),
                Parameter::F64(7.5),
                Parameter::Null(NativePostgresType::F64),
            ]
        );
    }
}
