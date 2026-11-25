//! Exact PostgreSQL plans for Ambient Weather historical public-station backfill.
//!
//! The Node worker uses the shared normalized-weather backfill lifecycle for
//! Ambient, NOAA, and Tempest records. This module retains that one lifecycle
//! and adds Ambient's distinct startup source-association query.

use weather_contracts::{JsonValue, SourceKind};

pub use crate::tide_backfill::{
    ABANDON_EXPIRED_TIDE_BACKFILL_RUNS_SQL as ABANDON_EXPIRED_AMBIENT_BACKFILL_RUNS_SQL,
    ASSERT_TIDE_BACKFILL_RUN_SQL as ASSERT_AMBIENT_BACKFILL_RUN_SQL,
    AdmittedTideBackfillIdentity as AdmittedAmbientBackfillIdentity,
    CompleteTideBackfill as CompleteAmbientBackfill,
    FINALIZE_FAILED_TIDE_BACKFILL_RUN_SQL as FINALIZE_FAILED_AMBIENT_BACKFILL_RUN_SQL,
    FINALIZE_SUCCESSFUL_TIDE_BACKFILL_RUN_SQL as FINALIZE_SUCCESSFUL_AMBIENT_BACKFILL_RUN_SQL,
    FIND_RUNNING_TIDE_BACKFILL_RUN_SQL as FIND_RUNNING_AMBIENT_BACKFILL_RUN_SQL,
    FailTideBackfill as FailAmbientBackfill,
    HAS_SUCCESSFUL_TIDE_BACKFILL_CHUNK_SQL as HAS_SUCCESSFUL_AMBIENT_BACKFILL_CHUNK_SQL,
    INSERT_TIDE_BACKFILL_RUN_SQL as INSERT_AMBIENT_BACKFILL_RUN_SQL,
    StartTideBackfillRun as StartAmbientBackfillRun,
    StartedTideBackfillRun as StartedAmbientBackfillRun,
    TideBackfillAbandonExpiredQuery as AmbientBackfillAbandonExpiredQuery,
    TideBackfillCommandPort as AmbientBackfillCommandPort,
    TideBackfillExecutionError as AmbientBackfillExecutionError,
    TideBackfillFailureBind as AmbientBackfillFailureBind,
    TideBackfillFailureQuery as AmbientBackfillFailureQuery,
    TideBackfillFindRunningBind as AmbientBackfillFindRunningBind,
    TideBackfillFindRunningQuery as AmbientBackfillFindRunningQuery,
    TideBackfillGuard as AmbientBackfillGuard,
    TideBackfillIdentityBind as AmbientBackfillIdentityBind,
    TideBackfillInputError as AmbientBackfillInputError,
    TideBackfillOutcome as AmbientBackfillOutcome,
    TideBackfillOutcomeBind as AmbientBackfillOutcomeBind,
    TideBackfillOutcomeQuery as AmbientBackfillOutcomeQuery,
    TideBackfillPoolPort as AmbientBackfillPoolPort,
    TideBackfillRunIdentityQuery as AmbientBackfillRunIdentityQuery,
    TideBackfillRunStartBind as AmbientBackfillRunStartBind,
    TideBackfillRunStartQuery as AmbientBackfillRunStartQuery,
    TideBackfillRunningLookup as AmbientBackfillRunningLookup,
    TideBackfillSuccessBind as AmbientBackfillSuccessBind,
    TideBackfillSuccessQuery as AmbientBackfillSuccessQuery,
    TideBackfillSuccessfulChunkQuery as AmbientBackfillSuccessfulChunkQuery,
    UPSERT_TIDE_BACKFILL_OUTCOME_SQL as UPSERT_AMBIENT_BACKFILL_OUTCOME_SQL,
    complete_tide_backfill as complete_ambient_backfill,
    fail_tide_backfill as fail_ambient_backfill,
    start_tide_backfill_run as start_ambient_backfill_run,
    tide_backfill_abandon_expired_query as ambient_backfill_abandon_expired_query,
    tide_backfill_failure_query as ambient_backfill_failure_query,
    tide_backfill_outcome_query as ambient_backfill_outcome_query,
    tide_backfill_run_identity_query as ambient_backfill_run_identity_query,
    tide_backfill_run_start_query as ambient_backfill_run_start_query,
    tide_backfill_success_query as ambient_backfill_success_query,
    tide_backfill_successful_chunk_query as ambient_backfill_successful_chunk_query,
};

/// The source-owned active public-station association query.
pub const AMBIENT_BACKFILL_SOURCE_ASSOCIATION_SQL: &str = r#"
      SELECT
        s.material_provider_config AS "materialProviderConfig",
        p.provider_key AS "providerKey",
        si.slug AS "siteSlug",
        s.source_config_fingerprint AS "sourceConfigFingerprint",
        s.id AS "sourceId",
        s.source_key AS "sourceKey",
        s.source_kind AS "sourceKind",
        st.slug AS "stationSlug",
        si.timezone
      FROM sources s
      JOIN stations st ON st.id = s.station_id
      JOIN sites si ON si.id = st.site_id
      JOIN providers p ON p.id = s.provider_id
      WHERE si.slug = $1
        AND s.source_key = ANY($2::text[])
        AND s.active
        AND st.active
        AND si.active
        AND p.active
      ORDER BY s.source_key
    "#;

/// One typed source-association bind in PostgreSQL placeholder order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AmbientBackfillSourceAssociationBind {
    Text(String),
    TextArray(Vec<String>),
}

/// The exact two-bind source-association query.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AmbientBackfillSourceAssociationQuery {
    pub sql: &'static str,
    pub binds: [AmbientBackfillSourceAssociationBind; 2],
}

/// One decoded active source row from the association query.
#[derive(Clone, Debug, PartialEq)]
pub struct AmbientBackfillSourceAssociationRow {
    pub material_provider_config: JsonValue,
    pub provider_key: String,
    pub site_slug: String,
    pub source_config_fingerprint: String,
    pub source_id: String,
    pub source_key: String,
    pub source_kind: SourceKind,
    pub station_slug: String,
    pub timezone: String,
}

/// The injected read boundary for startup association.
pub trait AmbientBackfillSourceReadPort {
    type Error;

    /// Read active rows through the exact source-owned query.
    fn read_ambient_backfill_sources(
        &mut self,
        query: &AmbientBackfillSourceAssociationQuery,
    ) -> Result<Vec<AmbientBackfillSourceAssociationRow>, Self::Error>;
}

/// Build the exact active source-association query without executing PostgreSQL.
pub fn ambient_backfill_source_association_query(
    site_slug: &str,
    source_keys: &[String],
) -> AmbientBackfillSourceAssociationQuery {
    AmbientBackfillSourceAssociationQuery {
        sql: AMBIENT_BACKFILL_SOURCE_ASSOCIATION_SQL,
        binds: [
            AmbientBackfillSourceAssociationBind::Text(site_slug.to_owned()),
            AmbientBackfillSourceAssociationBind::TextArray(source_keys.to_vec()),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AMBIENT_BACKFILL_SOURCE_ASSOCIATION_SQL, AmbientBackfillSourceAssociationBind,
        ambient_backfill_source_association_query,
    };

    /// Pin the Node source query and its two bind positions.
    #[test]
    fn builds_exact_active_source_association_query() {
        let query = ambient_backfill_source_association_query(
            "ballydidean",
            &["ambient-east".to_owned(), "ambient-west".to_owned()],
        );

        assert_eq!(query.sql, AMBIENT_BACKFILL_SOURCE_ASSOCIATION_SQL);
        assert_eq!(
            query.binds,
            [
                AmbientBackfillSourceAssociationBind::Text("ballydidean".to_owned()),
                AmbientBackfillSourceAssociationBind::TextArray(vec![
                    "ambient-east".to_owned(),
                    "ambient-west".to_owned(),
                ]),
            ]
        );
        assert!(query.sql.contains("s.source_key = ANY($2::text[])"));
        assert!(query.sql.contains("ORDER BY s.source_key"));
    }
}
