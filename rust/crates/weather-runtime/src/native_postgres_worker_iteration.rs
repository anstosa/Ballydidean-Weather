//! Native PostgreSQL execution for worker readiness, discovery, and heartbeats.

use std::error::Error;
use std::io::{Error as IoError, ErrorKind};
use std::time::{SystemTime, UNIX_EPOCH};

use tokio_postgres::Row;
use tokio_postgres::types::{FromSql, Type};
use weather_contracts::{DueSource, JsDateOrString, SourceKind};
use weather_database::{
    AppliedMigration, DueSourceDiscoveryPlan, DueSourceDiscoveryRow, HealthMigrationLedgerReadPlan,
    MigrationDigest, PostgresServerVersionReadPlan, PostgresServerVersionReadRow,
    WorkerHeartbeatReadPlan, WorkerHeartbeatReadRow, WorkerHeartbeatUpdatePlan,
    WorkerIterationQueryBind, WorkerReadinessBind,
};

use crate::{
    NativePostgresError, NativePostgresParameter, NativePostgresPool, NativePostgresType,
    WorkerBootDatabase, WorkerIterationPostgresPort, native_json::decode_json,
};

const NODE_MINIMUM_MILLISECONDS: i128 = -8_640_000_000_000_000;
const NODE_MAXIMUM_MILLISECONDS: i128 = 8_640_000_000_000_000;

impl WorkerIterationPostgresPort for NativePostgresPool {
    type Error = NativePostgresError;

    /// Execute the retained ordered discovery plan and decode every generated row.
    fn query_due_sources(
        &mut self,
        plan: &DueSourceDiscoveryPlan,
    ) -> Result<Vec<DueSourceDiscoveryRow>, Self::Error> {
        let parameters = iteration_parameters(&plan.binds);
        let rows = query_pool(self, plan.sql, &parameters)?;
        rows.iter().map(decode_due_source).collect()
    }

    /// Execute the independent heartbeat upsert with its exact five typed binds.
    fn execute_worker_heartbeat(
        &mut self,
        plan: &WorkerHeartbeatUpdatePlan,
    ) -> Result<(), Self::Error> {
        let parameters = iteration_parameters(&plan.binds);
        execute_pool(self, plan.sql, &parameters)?;
        Ok(())
    }
}

impl WorkerBootDatabase for NativePostgresPool {
    type Error = NativePostgresError;

    /// Read the one optional server-version row without coercing its text value.
    fn query_server_version(
        &mut self,
        plan: &PostgresServerVersionReadPlan,
    ) -> Result<Option<PostgresServerVersionReadRow>, Self::Error> {
        let mut rows = query_pool(self, plan.sql, &[])?;
        let row = optional_single_row(&mut rows, "server_version_num")?;
        row.map(|row| -> Result<_, NativePostgresError> {
            Ok(PostgresServerVersionReadRow {
                server_version_num: row.try_get("server_version_num")?,
            })
        })
        .transpose()
    }

    /// Decode the complete ordered migration ledger from the exact zero-bind plan.
    fn query_migration_ledger(
        &mut self,
        plan: &HealthMigrationLedgerReadPlan,
    ) -> Result<Vec<AppliedMigration>, Self::Error> {
        let rows = query_pool(self, plan.sql, &[])?;
        rows.iter().map(decode_applied_migration).collect()
    }

    /// Read the configured worker heartbeat and preserve nullable Date values.
    fn query_worker_heartbeat(
        &mut self,
        plan: &WorkerHeartbeatReadPlan,
    ) -> Result<Option<WorkerHeartbeatReadRow>, Self::Error> {
        let parameters = readiness_parameters(&plan.binds);
        let mut rows = query_pool(self, plan.sql, &parameters)?;
        let row = optional_single_row(&mut rows, "worker_heartbeat")?;
        row.map(decode_worker_heartbeat).transpose()
    }

    /// Close the one retained native pool and join its dedicated runtime thread.
    fn close_worker_pool(&mut self) -> Result<(), Self::Error> {
        self.close()
    }
}

/// Execute one query and return its session even when the statement fails.
fn query_pool(
    pool: &NativePostgresPool,
    sql: &str,
    parameters: &[NativePostgresParameter],
) -> Result<Vec<Row>, NativePostgresError> {
    let mut session = pool.checkout()?;
    let result = session.query(sql, parameters);
    let release = session.release();
    // preserve the statement failure while letting the session classify connection closure
    match result {
        Ok(rows) => {
            release?;
            Ok(rows)
        }
        Err(error) => {
            let _ = release;
            Err(error)
        }
    }
}

/// Execute one command and return its session even when the statement fails.
fn execute_pool(
    pool: &NativePostgresPool,
    sql: &str,
    parameters: &[NativePostgresParameter],
) -> Result<u64, NativePostgresError> {
    let mut session = pool.checkout()?;
    let result = session.execute(sql, parameters);
    let release = session.release();
    // preserve the command failure while letting the session classify connection closure
    match result {
        Ok(affected) => {
            release?;
            Ok(affected)
        }
        Err(error) => {
            let _ = release;
            Err(error)
        }
    }
}

/// Convert the iteration plan's closed bind domain into owned thread-safe parameters.
fn iteration_parameters(binds: &[WorkerIterationQueryBind]) -> Vec<NativePostgresParameter> {
    binds
        .iter()
        .map(|bind| match bind {
            WorkerIterationQueryBind::Text(value) => NativePostgresParameter::Text(value.clone()),
            WorkerIterationQueryBind::UtcInstant(value) => {
                NativePostgresParameter::Timestamp(value.clone())
            }
            WorkerIterationQueryBind::NullableText(Some(value)) => {
                NativePostgresParameter::Text(value.clone())
            }
            WorkerIterationQueryBind::NullableText(None) => {
                NativePostgresParameter::Null(NativePostgresType::Text)
            }
            WorkerIterationQueryBind::NullableUtcInstant(Some(value)) => {
                NativePostgresParameter::Timestamp(value.clone())
            }
            WorkerIterationQueryBind::NullableUtcInstant(None) => {
                NativePostgresParameter::Null(NativePostgresType::Timestamp)
            }
            WorkerIterationQueryBind::Unsigned(value) => NativePostgresParameter::U64(*value),
        })
        .collect()
}

/// Convert the readiness plan's closed bind domain into owned parameters.
fn readiness_parameters(binds: &[WorkerReadinessBind]) -> Vec<NativePostgresParameter> {
    binds
        .iter()
        .map(|bind| match bind {
            WorkerReadinessBind::Text(value) => NativePostgresParameter::Text(value.clone()),
        })
        .collect()
}

/// Require an optional query contract to return no more than one row.
fn optional_single_row(
    rows: &mut Vec<Row>,
    label: &'static str,
) -> Result<Option<Row>, NativePostgresError> {
    // reject impossible duplicate rows instead of silently choosing one
    if rows.len() > 1 {
        return Err(NativePostgresError::decode(
            label,
            "query returned more than one row",
        ));
    }
    Ok(rows.pop())
}

/// Decode one source row while preserving the database's result order.
fn decode_due_source(row: &Row) -> Result<DueSource, NativePostgresError> {
    let id: i64 = row.try_get("id")?;
    let cadence_seconds: i32 = row.try_get("cadenceSeconds")?;
    let source_kind: String = row.try_get("sourceKind")?;
    let source_kind = decode_source_kind(&source_kind)?;
    let material: PostgresJson = row.try_get("materialProviderConfig")?;
    let material_provider_config = decode_json(&material.0).ok_or_else(|| {
        NativePostgresError::decode(
            "materialProviderConfig",
            "database JSON is not a supported generated value",
        )
    })?;
    let cadence_seconds = u64::try_from(cadence_seconds).map_err(|_| {
        NativePostgresError::decode("cadenceSeconds", "database cadence is negative")
    })?;
    Ok(DueSource {
        active: row.try_get("active")?,
        cadenceSeconds: cadence_seconds,
        id: id.to_string(),
        materialProviderConfig: material_provider_config,
        providerKey: row.try_get("providerKey")?,
        siteSlug: row.try_get("siteSlug")?,
        sourceConfigFingerprint: row.try_get("sourceConfigFingerprint")?,
        sourceKey: row.try_get("sourceKey")?,
        sourceKind: source_kind,
        stationSlug: row.try_get("stationSlug")?,
        timezone: row.try_get("timezone")?,
    })
}

/// Decode one generated source-kind wire value.
fn decode_source_kind(value: &str) -> Result<SourceKind, NativePostgresError> {
    SourceKind::WIRE_VALUES
        .iter()
        .find_map(|(wire, source_kind)| (*wire == value).then_some(*source_kind))
        .ok_or_else(|| NativePostgresError::decode("sourceKind", "database enum is invalid"))
}

/// Decode one immutable migration ledger row.
fn decode_applied_migration(row: &Row) -> Result<AppliedMigration, NativePostgresError> {
    Ok(AppliedMigration {
        name: row.try_get("name")?,
        digest: MigrationDigest::admitted(row.try_get::<_, String>("checksum")?),
    })
}

/// Decode one durable worker heartbeat through PostgreSQL's timestamp owner.
fn decode_worker_heartbeat(row: Row) -> Result<WorkerHeartbeatReadRow, NativePostgresError> {
    let last_loop_at: SystemTime = row.try_get("last_loop_at")?;
    let last_success_at: Option<SystemTime> = row.try_get("last_success_at")?;
    Ok(WorkerHeartbeatReadRow {
        last_loop_at: js_date(last_loop_at),
        last_success_at: last_success_at.map(js_date),
    })
}

/// Convert PostgreSQL's timestamp into the predecessor's millisecond Date domain.
fn js_date(value: SystemTime) -> JsDateOrString {
    let milliseconds = match value.duration_since(UNIX_EPOCH) {
        Ok(duration) => {
            i128::from(duration.as_secs()) * 1_000 + i128::from(duration.subsec_millis())
        }
        Err(error) => {
            let duration = error.duration();
            let whole = i128::from(duration.as_secs()) * 1_000;
            let nanos = duration.subsec_nanos();
            let partial = i128::from(nanos / 1_000_000 + u32::from(nanos % 1_000_000 != 0));
            -(whole + partial)
        }
    };
    // represent PostgreSQL instants outside the ECMAScript Date range as Invalid Date
    if !(NODE_MINIMUM_MILLISECONDS..=NODE_MAXIMUM_MILLISECONDS).contains(&milliseconds) {
        return JsDateOrString::InvalidDate;
    }
    JsDateOrString::DateEpochMilliseconds(milliseconds as i64)
}

/// Raw PostgreSQL JSON/JSONB payload retained for the existing generated decoder.
struct PostgresJson(Vec<u8>);

impl<'a> FromSql<'a> for PostgresJson {
    /// Strip only PostgreSQL's JSONB protocol version and retain the exact JSON bytes.
    fn from_sql(ty: &Type, raw: &'a [u8]) -> Result<Self, Box<dyn Error + Sync + Send>> {
        // JSONB binary values begin with their one-byte protocol version
        let bytes = if *ty == Type::JSONB {
            let Some((&version, bytes)) = raw.split_first() else {
                return Err(Box::new(IoError::new(
                    ErrorKind::InvalidData,
                    "empty JSONB payload",
                )));
            };
            // version one is the only PostgreSQL JSONB wire format currently defined
            if version != 1 {
                return Err(Box::new(IoError::new(
                    ErrorKind::InvalidData,
                    "unsupported JSONB protocol version",
                )));
            }
            bytes
        } else {
            raw
        };
        Ok(Self(bytes.to_vec()))
    }

    /// Accept only PostgreSQL's native JSON scalar types.
    fn accepts(ty: &Type) -> bool {
        matches!(*ty, Type::JSON | Type::JSONB)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use weather_contracts::JsonValue;
    use weather_database::{
        AdmittedRevisionUtcInstant, WorkerHeartbeatUpdate, discover_due_sources_plan,
        update_worker_heartbeat_plan,
    };

    /// Preserve every typed discovery and heartbeat bind without stringly driver coercion.
    #[test]
    fn iteration_plans_map_to_owned_postgres_parameters() {
        let instant = AdmittedRevisionUtcInstant::admitted("2026-11-24T20:00:00.000Z");
        let discovery = discover_due_sources_plan(&instant, Some(7)).unwrap();
        assert_eq!(
            iteration_parameters(&discovery.binds),
            vec![
                NativePostgresParameter::Timestamp("2026-11-24T20:00:00.000Z".to_owned()),
                NativePostgresParameter::U64(7),
            ]
        );

        let heartbeat = update_worker_heartbeat_plan(&WorkerHeartbeatUpdate {
            activity: None,
            instance: "worker-main".to_owned(),
            last_loop_at: instant,
            last_success_at: None,
            version: "release".to_owned(),
        })
        .unwrap();
        assert_eq!(
            iteration_parameters(&heartbeat.binds),
            vec![
                NativePostgresParameter::Text("worker-main".to_owned()),
                NativePostgresParameter::Timestamp("2026-11-24T20:00:00.000Z".to_owned()),
                NativePostgresParameter::Null(NativePostgresType::Timestamp),
                NativePostgresParameter::Null(NativePostgresType::Text),
                NativePostgresParameter::Text("release".to_owned()),
            ]
        );
    }

    /// Match ECMAScript millisecond truncation on both sides of the Unix epoch.
    #[test]
    fn postgres_timestamps_map_to_javascript_dates() {
        assert_eq!(
            js_date(UNIX_EPOCH + std::time::Duration::from_nanos(1_999_999)),
            JsDateOrString::DateEpochMilliseconds(1)
        );
        assert_eq!(
            js_date(UNIX_EPOCH - std::time::Duration::from_nanos(1_000_001)),
            JsDateOrString::DateEpochMilliseconds(-2)
        );
    }

    /// Retain producer JSON member order after stripping the JSONB version byte.
    #[test]
    fn postgres_jsonb_uses_the_generated_json_decoder() {
        let decoded = PostgresJson::from_sql(&Type::JSONB, b"\x01{\"b\":1,\"a\":2}").unwrap();
        let JsonValue::Object(fields) = decode_json(&decoded.0).unwrap() else {
            panic!("fixture must decode as an object");
        };
        assert!(fields[0].0.iter().copied().eq("b".encode_utf16()));
        assert!(fields[1].0.iter().copied().eq("a".encode_utf16()));
    }
}
