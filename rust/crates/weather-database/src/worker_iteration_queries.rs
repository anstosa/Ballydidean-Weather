use std::error::Error;
use std::fmt::{Display, Formatter};

use weather_contracts::DueSource;

use crate::AdmittedRevisionUtcInstant;

/// Exact current-source discovery query used by ordinary worker iterations.
pub const DISCOVER_DUE_SOURCES_SQL: &str = r#"
      SELECT
        s.id,
        s.source_key AS "sourceKey",
        s.source_kind AS "sourceKind",
        s.source_config_fingerprint AS "sourceConfigFingerprint",
        s.material_provider_config AS "materialProviderConfig",
        s.cadence_seconds AS "cadenceSeconds",
        s.active,
        st.slug AS "stationSlug",
        si.slug AS "siteSlug",
        si.timezone,
        p.provider_key AS "providerKey"
      FROM sources s
      JOIN stations st ON st.id = s.station_id
      JOIN sites si ON si.id = st.site_id
      JOIN providers p ON p.id = s.provider_id
      LEFT JOIN ingestion_checkpoints checkpoint ON checkpoint.source_id = s.id
      WHERE s.active
        AND st.active
        AND si.active
        AND p.active
        AND weather_source_is_current(s.id)
        AND s.cadence_seconds IS NOT NULL
        AND (
          checkpoint.last_committed_at IS NULL
          OR checkpoint.last_committed_at + make_interval(secs => s.cadence_seconds) <= $1
        )
      ORDER BY checkpoint.last_committed_at ASC NULLS FIRST, s.id ASC
      LIMIT $2
    "#;

/// Exact independent worker-liveness upsert.
pub const UPDATE_WORKER_HEARTBEAT_SQL: &str = r#"
      INSERT INTO worker_heartbeats (
        worker_instance,
        last_loop_at,
        last_success_at,
        current_activity,
        worker_version
      )
      VALUES ($1, $2, $3, $4, $5)
      ON CONFLICT (worker_instance) DO UPDATE SET
        last_loop_at = EXCLUDED.last_loop_at,
        last_success_at = EXCLUDED.last_success_at,
        current_activity = EXCLUDED.current_activity,
        worker_version = EXCLUDED.worker_version,
        updated_at = clock_timestamp()
    "#;

pub const DEFAULT_DUE_SOURCE_LIMIT: u64 = 100;
pub const MAXIMUM_DUE_SOURCE_LIMIT: u64 = 500;

/// Closed owned bind domain for ordinary worker iteration queries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkerIterationQueryBind {
    Text(String),
    UtcInstant(String),
    NullableText(Option<String>),
    NullableUtcInstant(Option<String>),
    Unsigned(u64),
}

/// Exact two-bind due-source discovery plan decoded as generated rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DueSourceDiscoveryPlan {
    pub sql: &'static str,
    pub binds: [WorkerIterationQueryBind; 2],
}

/// One admitted heartbeat update before constructing its five binds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerHeartbeatUpdate {
    pub activity: Option<String>,
    pub instance: String,
    pub last_loop_at: AdmittedRevisionUtcInstant,
    pub last_success_at: Option<AdmittedRevisionUtcInstant>,
    pub version: String,
}

/// Exact five-bind independent heartbeat upsert plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerHeartbeatUpdatePlan {
    pub sql: &'static str,
    pub binds: [WorkerIterationQueryBind; 5],
}

/// Ordinary validation failures before driver execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkerIterationQueryError {
    InvalidDiscoveryLimit,
    InvalidHeartbeatFields,
}

impl Display for WorkerIterationQueryError {
    /// Preserve the exact Node validation diagnostics.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidDiscoveryLimit => {
                formatter.write_str("source discovery limit must be between 1 and 500")
            }
            Self::InvalidHeartbeatFields => {
                formatter.write_str("heartbeat fields must be non-empty and bounded")
            }
        }
    }
}

impl Error for WorkerIterationQueryError {}

/// Build bounded source discovery at one already admitted canonical instant.
pub fn discover_due_sources_plan(
    as_of: &AdmittedRevisionUtcInstant,
    limit: Option<u64>,
) -> Result<DueSourceDiscoveryPlan, WorkerIterationQueryError> {
    let limit = limit.unwrap_or(DEFAULT_DUE_SOURCE_LIMIT);
    // enforce the exact source range before constructing database binds
    if !(1..=MAXIMUM_DUE_SOURCE_LIMIT).contains(&limit) {
        return Err(WorkerIterationQueryError::InvalidDiscoveryLimit);
    }
    Ok(DueSourceDiscoveryPlan {
        sql: DISCOVER_DUE_SOURCES_SQL,
        binds: [
            WorkerIterationQueryBind::UtcInstant(as_of.as_str().to_owned()),
            WorkerIterationQueryBind::Unsigned(limit),
        ],
    })
}

/// Build the independent heartbeat upsert after exact UTF-16 field admission.
pub fn update_worker_heartbeat_plan(
    input: &WorkerHeartbeatUpdate,
) -> Result<WorkerHeartbeatUpdatePlan, WorkerIterationQueryError> {
    let instance_length = input.instance.encode_utf16().count();
    let version_length = input.version.encode_utf16().count();
    let activity_length = input
        .activity
        .as_ref()
        .map_or(0, |activity| activity.encode_utf16().count());
    // match JavaScript string lengths without trimming public heartbeat values
    if instance_length == 0
        || instance_length > 128
        || version_length == 0
        || version_length > 128
        || activity_length > 256
    {
        return Err(WorkerIterationQueryError::InvalidHeartbeatFields);
    }
    Ok(WorkerHeartbeatUpdatePlan {
        sql: UPDATE_WORKER_HEARTBEAT_SQL,
        binds: [
            WorkerIterationQueryBind::Text(input.instance.clone()),
            WorkerIterationQueryBind::UtcInstant(input.last_loop_at.as_str().to_owned()),
            WorkerIterationQueryBind::NullableUtcInstant(
                input
                    .last_success_at
                    .as_ref()
                    .map(|instant| instant.as_str().to_owned()),
            ),
            WorkerIterationQueryBind::NullableText(input.activity.clone()),
            WorkerIterationQueryBind::Text(input.version.clone()),
        ],
    })
}

/// Assert that the generated DTO remains the decoded row boundary for discovery.
pub type DueSourceDiscoveryRow = DueSource;
