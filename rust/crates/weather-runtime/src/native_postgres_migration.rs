//! Native PostgreSQL binding for checked migrations and generated catalog bootstrap.

use weather_database::{
    AppliedMigration, BootstrapBind, BootstrapPool, BootstrapRows, BootstrapSession,
    CheckedMigrationSession, MigrationCommandPort, MigrationDigest, MigrationScript,
    PostgresServerVersionReadPlan, PostgresServerVersionReadRow,
};

use crate::{
    MigrationBootstrapDriverPool, NativePostgresError, NativePostgresParameter, NativePostgresPool,
    NativePostgresSession, NativePostgresType,
};

const ACQUIRE_ADVISORY_LOCK_SQL: &str = "SELECT pg_advisory_lock($1::bigint)";
const RELEASE_ADVISORY_LOCK_SQL: &str = "SELECT pg_advisory_unlock($1::bigint)";
const READ_APPLIED_MIGRATIONS_SQL: &str =
    "SELECT name, checksum FROM schema_migrations ORDER BY name";
const RECORD_MIGRATION_SQL: &str = "INSERT INTO schema_migrations (name, checksum) VALUES ($1, $2)";

/// one owned native session shared by migration and bootstrap adapters
pub struct NativePostgresMigrationSession {
    session: Option<NativePostgresSession>,
}

impl NativePostgresMigrationSession {
    /// retain one checked-out native connection
    fn new(session: NativePostgresSession) -> Self {
        Self {
            session: Some(session),
        }
    }

    /// borrow the retained native connection
    fn session(&mut self) -> Result<&mut NativePostgresSession, NativePostgresError> {
        self.session
            .as_mut()
            .ok_or(NativePostgresError::SessionReleased)
    }

    /// release the retained connection at most once
    fn release_session(&mut self) {
        // consume the owned session so its pool slot is returned exactly once
        if let Some(session) = self.session.take() {
            let _ = session.release();
        }
    }
}

impl Drop for NativePostgresMigrationSession {
    /// release one forgotten migration or bootstrap session
    fn drop(&mut self) {
        self.release_session();
    }
}

impl BootstrapPool for NativePostgresPool {
    type Session = NativePostgresMigrationSession;

    /// acquire one session from the retained native process pool
    fn checkout(&mut self) -> Result<Self::Session, NativePostgresError> {
        NativePostgresPool::checkout(self).map(NativePostgresMigrationSession::new)
    }
}

impl BootstrapSession for NativePostgresMigrationSession {
    type Error = NativePostgresError;

    /// begin the generated catalog transaction
    fn begin(&mut self) -> Result<(), Self::Error> {
        self.session()?.begin()
    }

    /// execute one generated bootstrap statement with exact typed binds
    fn query(
        &mut self,
        sql: &'static str,
        binds: &[BootstrapBind<'_>],
    ) -> Result<BootstrapRows, Self::Error> {
        let parameters = bootstrap_parameters(binds)?;
        let rows = self.session()?.query(sql, &parameters)?;
        let row_count = u64::try_from(rows.len())
            .map_err(|error| NativePostgresError::decode("row_count", error))?;
        let first_id = rows
            .first()
            .map(
                // decode generated identifiers from their actual bigint domain
                |row| {
                    row.try_get::<_, i64>(0)
                        .map(|id| id.to_string())
                        .map_err(|error| NativePostgresError::decode("id", error))
                },
            )
            .transpose()?;
        Ok(BootstrapRows {
            row_count: Some(row_count),
            first_id,
        })
    }

    /// commit the complete generated catalog
    fn commit(&mut self) -> Result<(), Self::Error> {
        self.session()?.commit()
    }

    /// roll back the complete generated catalog
    fn rollback(&mut self) -> Result<(), Self::Error> {
        self.session()?.rollback()
    }

    /// return this session to the native process pool
    fn release(&mut self) {
        self.release_session();
    }
}

impl MigrationCommandPort for NativePostgresMigrationSession {
    type Error = NativePostgresError;

    /// acquire the exact retained advisory lock
    fn acquire_advisory_lock(&mut self, key: i64) -> Result<(), Self::Error> {
        self.session()?
            .query(
                ACQUIRE_ADVISORY_LOCK_SQL,
                &[NativePostgresParameter::I64(key)],
            )
            .map(|_| ())
    }

    /// read the canonical migration ledger in filename order
    fn read_applied_migrations(&mut self) -> Result<Vec<AppliedMigration>, Self::Error> {
        self.session()?
            .query(READ_APPLIED_MIGRATIONS_SQL, &[])?
            .into_iter()
            .map(|row| {
                let name = row
                    .try_get::<_, String>("name")
                    .map_err(|error| NativePostgresError::decode("name", error))?;
                let checksum = row
                    .try_get::<_, String>("checksum")
                    .map_err(|error| NativePostgresError::decode("checksum", error))?;
                Ok(AppliedMigration {
                    name,
                    digest: MigrationDigest::admitted(checksum),
                })
            })
            .collect()
    }

    /// begin one migration-file transaction
    fn begin_transaction(&mut self) -> Result<(), Self::Error> {
        self.session()?.begin()
    }

    /// submit one already-reviewed migration script unchanged
    fn submit_script(&mut self, script: &MigrationScript) -> Result<(), Self::Error> {
        self.session()?.batch_execute(&script.sql)
    }

    /// record one immutable migration filename and digest
    fn record_migration(
        &mut self,
        name: &str,
        digest: &MigrationDigest,
    ) -> Result<(), Self::Error> {
        self.session()?
            .execute(
                RECORD_MIGRATION_SQL,
                &[
                    NativePostgresParameter::Text(name.to_owned()),
                    NativePostgresParameter::Text(digest.as_str().to_owned()),
                ],
            )
            .map(|_| ())
    }

    /// commit one migration-file transaction
    fn commit_transaction(&mut self) -> Result<(), Self::Error> {
        self.session()?.commit()
    }

    /// roll back one migration-file transaction
    fn rollback_transaction(&mut self) -> Result<(), Self::Error> {
        self.session()?.rollback()
    }

    /// release the exact retained advisory lock
    fn release_advisory_lock(&mut self, key: i64) -> Result<(), Self::Error> {
        self.session()?
            .query(
                RELEASE_ADVISORY_LOCK_SQL,
                &[NativePostgresParameter::I64(key)],
            )
            .map(|_| ())
    }

    /// return the acquired connection to its process pool
    fn release_client(&mut self) {
        self.release_session();
    }
}

impl CheckedMigrationSession for NativePostgresMigrationSession {
    /// read the exact PostgreSQL server version setting
    fn query_server_version(
        &mut self,
        plan: PostgresServerVersionReadPlan,
    ) -> Result<Option<PostgresServerVersionReadRow>, Self::Error> {
        let rows = self.session()?.query(plan.sql, &[])?;
        let Some(row) = rows.into_iter().next() else {
            return Ok(None);
        };
        let server_version_num = row
            .try_get::<_, String>("server_version_num")
            .map_err(|error| NativePostgresError::decode("server_version_num", error))?;
        Ok(Some(PostgresServerVersionReadRow { server_version_num }))
    }

    /// configure one bounded timeout through the retained set_config statement
    fn configure_timeout(&mut self, sql: &'static str, value: &str) -> Result<(), Self::Error> {
        self.session()?
            .query(sql, &[NativePostgresParameter::Text(value.to_owned())])
            .map(|_| ())
    }

    /// execute one fixed migration control statement
    fn execute_control_statement(&mut self, sql: &'static str) -> Result<(), Self::Error> {
        self.session()?.batch_execute(sql)
    }
}

impl MigrationBootstrapDriverPool for NativePostgresPool {
    type MigrationSession = NativePostgresMigrationSession;

    /// acquire migration work from the same retained native pool
    fn checkout_migration(&mut self) -> Result<Self::MigrationSession, NativePostgresError> {
        NativePostgresPool::checkout(self).map(NativePostgresMigrationSession::new)
    }

    /// close the retained native process pool
    fn close(&mut self) -> Result<(), NativePostgresError> {
        NativePostgresPool::close(self)
    }
}

/// project generated bootstrap binds into exact PostgreSQL domains
fn bootstrap_parameters(
    binds: &[BootstrapBind<'_>],
) -> Result<Vec<NativePostgresParameter>, NativePostgresError> {
    binds.iter().map(bootstrap_parameter).collect()
}

/// project one generated bootstrap bind without statement-position guessing
fn bootstrap_parameter(
    bind: &BootstrapBind<'_>,
) -> Result<NativePostgresParameter, NativePostgresError> {
    let parameter = match bind {
        BootstrapBind::Text(value) => NativePostgresParameter::Text((*value).to_owned()),
        BootstrapBind::BigintText(value) => NativePostgresParameter::I64(
            value
                .parse::<i64>()
                .map_err(|_| NativePostgresError::InvalidParameter("bootstrap_bigint"))?,
        ),
        BootstrapBind::Json(value) => NativePostgresParameter::Json((*value).to_owned()),
        BootstrapBind::Unsigned(value) => NativePostgresParameter::I32(
            i32::try_from(*value)
                .map_err(|_| NativePostgresError::InvalidParameter("bootstrap_integer"))?,
        ),
        BootstrapBind::Float(value) => NativePostgresParameter::F64(*value),
        BootstrapBind::Bool(value) => NativePostgresParameter::Bool(*value),
        BootstrapBind::NullText => NativePostgresParameter::Null(NativePostgresType::Text),
        BootstrapBind::NullInteger => NativePostgresParameter::Null(NativePostgresType::I32),
    };
    Ok(parameter)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// preserve exact bootstrap domains without SQL-position inference
    #[test]
    fn projects_typed_bootstrap_parameters() {
        assert_eq!(
            bootstrap_parameters(&[
                BootstrapBind::BigintText("42"),
                BootstrapBind::Text("station"),
                BootstrapBind::Json("{\"active\":true}"),
                BootstrapBind::Unsigned(60),
                BootstrapBind::NullText,
                BootstrapBind::NullInteger,
            ])
            .unwrap(),
            [
                NativePostgresParameter::I64(42),
                NativePostgresParameter::Text("station".to_owned()),
                NativePostgresParameter::Json("{\"active\":true}".to_owned()),
                NativePostgresParameter::I32(60),
                NativePostgresParameter::Null(NativePostgresType::Text),
                NativePostgresParameter::Null(NativePostgresType::I32),
            ]
        );
    }

    /// retain the reviewed migration statement identities
    #[test]
    fn retains_exact_migration_statements() {
        assert_eq!(
            ACQUIRE_ADVISORY_LOCK_SQL,
            "SELECT pg_advisory_lock($1::bigint)"
        );
        assert_eq!(
            RELEASE_ADVISORY_LOCK_SQL,
            "SELECT pg_advisory_unlock($1::bigint)"
        );
        assert_eq!(
            READ_APPLIED_MIGRATIONS_SQL,
            "SELECT name, checksum FROM schema_migrations ORDER BY name"
        );
        assert_eq!(
            RECORD_MIGRATION_SQL,
            "INSERT INTO schema_migrations (name, checksum) VALUES ($1, $2)"
        );
    }
}
