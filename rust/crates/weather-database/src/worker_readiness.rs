use std::error::Error;
use std::fmt::{Display, Formatter};

use weather_contracts::JsDateOrString;

/// PostgreSQL feature floor required by the runtime schema.
pub const MINIMUM_SUPPORTED_POSTGRES_VERSION_NUM: u64 = 150_000;
/// Exact PostgreSQL server-version query issued before migration readiness.
pub const POSTGRES_SERVER_VERSION_NUM_SQL: &str =
    "SELECT current_setting('server_version_num') AS server_version_num";
/// Exact durable heartbeat query issued by the worker health command.
pub const WORKER_HEARTBEAT_READ_SQL: &str = "
      SELECT last_loop_at, last_success_at
      FROM worker_heartbeats
      WHERE worker_instance = $1
    ";

const MAXIMUM_JAVASCRIPT_SAFE_INTEGER: i64 = 9_007_199_254_740_991;

/// Closed scalar bind domain for ordinary worker readiness statements.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkerReadinessBind {
    Text(String),
}

/// Exact zero-bind PostgreSQL server-version statement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PostgresServerVersionReadPlan {
    pub binds: [WorkerReadinessBind; 0],
    pub sql: &'static str,
}

/// Exact one-bind durable worker-heartbeat statement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerHeartbeatReadPlan {
    pub binds: [WorkerReadinessBind; 1],
    pub sql: &'static str,
}

/// One server-version row before the source's numeric admission.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PostgresServerVersionReadRow {
    pub server_version_num: String,
}

/// One durable worker-heartbeat row before canonical time admission.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerHeartbeatReadRow {
    pub last_loop_at: JsDateOrString,
    pub last_success_at: Option<JsDateOrString>,
}

/// Stable ordinary rejection for a missing, malformed, or old PostgreSQL server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnsupportedPostgresVersionError {
    displayed_version: String,
}

impl UnsupportedPostgresVersionError {
    /// Return the source-compatible version rendering used by the failure message.
    pub fn displayed_version(&self) -> &str {
        &self.displayed_version
    }
}

impl Display for UnsupportedPostgresVersionError {
    /// Render the existing PostgreSQL feature-floor failure.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "PostgreSQL 15 or newer is required; server_version_num={}",
            self.displayed_version
        )
    }
}

impl Error for UnsupportedPostgresVersionError {}

/// Build the exact zero-bind server-version read.
pub const fn postgres_server_version_read_plan() -> PostgresServerVersionReadPlan {
    PostgresServerVersionReadPlan {
        binds: [],
        sql: POSTGRES_SERVER_VERSION_NUM_SQL,
    }
}

/// Build the exact one-bind worker-heartbeat read.
pub fn worker_heartbeat_read_plan(worker_instance: &str) -> WorkerHeartbeatReadPlan {
    WorkerHeartbeatReadPlan {
        binds: [WorkerReadinessBind::Text(worker_instance.to_owned())],
        sql: WORKER_HEARTBEAT_READ_SQL,
    }
}

/// Admit one optional remote scalar with the source's safe-integer and version floor.
pub fn admit_supported_postgres_version_num(
    server_version_num: Option<&str>,
) -> Result<u64, UnsupportedPostgresVersionError> {
    let version = server_version_num
        .and_then(|value| value.trim().parse::<i64>().ok())
        .filter(|value| value.unsigned_abs() <= MAXIMUM_JAVASCRIPT_SAFE_INTEGER as u64);

    // enforce the feature floor before admitting the positive version
    if let Some(value) =
        version.filter(|value| *value >= MINIMUM_SUPPORTED_POSTGRES_VERSION_NUM as i64)
    {
        return Ok(value as u64);
    }
    // allocate only for the source's bounded failure diagnostic
    let displayed_version = version.map_or_else(|| "NaN".to_owned(), |value| value.to_string());
    Err(UnsupportedPostgresVersionError { displayed_version })
}

/// Admit one typed server-version row returned by the exact read plan.
pub fn admit_supported_postgres_version(
    row: Option<&PostgresServerVersionReadRow>,
) -> Result<u64, UnsupportedPostgresVersionError> {
    admit_supported_postgres_version_num(row.map(|row| row.server_version_num.as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Lock the exact Node query text and typed bind arity.
    #[test]
    fn builds_exact_worker_boot_queries() {
        assert_eq!(
            postgres_server_version_read_plan(),
            PostgresServerVersionReadPlan {
                binds: [],
                sql: "SELECT current_setting('server_version_num') AS server_version_num",
            }
        );
        assert_eq!(
            worker_heartbeat_read_plan("worker-main"),
            WorkerHeartbeatReadPlan {
                binds: [WorkerReadinessBind::Text("worker-main".to_owned())],
                sql: "\n      SELECT last_loop_at, last_success_at\n      FROM worker_heartbeats\n      WHERE worker_instance = $1\n    ",
            }
        );
    }

    /// Preserve both the non-null loop time and nullable success time.
    #[test]
    fn owns_nullable_worker_heartbeat_rows() {
        let row = WorkerHeartbeatReadRow {
            last_loop_at: JsDateOrString::DateEpochMilliseconds(1_700_000_000_000),
            last_success_at: None,
        };

        assert_eq!(row.last_success_at, None);
        assert_eq!(
            row.last_loop_at,
            JsDateOrString::DateEpochMilliseconds(1_700_000_000_000)
        );
    }

    /// Accept the exact feature floor and preserve the source's ordinary old-version error.
    #[test]
    fn admits_postgres_15_and_rejects_postgres_14() {
        let supported = PostgresServerVersionReadRow {
            server_version_num: "150000".to_owned(),
        };
        let unsupported = PostgresServerVersionReadRow {
            server_version_num: "140012".to_owned(),
        };

        assert_eq!(
            admit_supported_postgres_version(Some(&supported)),
            Ok(150_000)
        );
        let error = admit_supported_postgres_version(Some(&unsupported))
            .expect_err("PostgreSQL 14 must remain below the feature floor");
        assert_eq!(error.displayed_version(), "140012");
        assert_eq!(
            error.to_string(),
            "PostgreSQL 15 or newer is required; server_version_num=140012"
        );
    }

    /// Stabilize the source's NaN failure for an absent or malformed version row.
    #[test]
    fn rejects_missing_or_malformed_server_versions() {
        // exercise both source paths that JavaScript Number renders as NaN
        for value in [None, Some("not-a-version")] {
            let error = admit_supported_postgres_version_num(value)
                .expect_err("missing and malformed versions must be rejected");
            assert_eq!(error.displayed_version(), "NaN");
            assert_eq!(
                error.to_string(),
                "PostgreSQL 15 or newer is required; server_version_num=NaN"
            );
        }
    }
}
