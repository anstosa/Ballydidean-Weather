use std::error::Error;
use std::fmt::{Display, Formatter};
use std::path::PathBuf;
use std::process::Command;

use weather_contracts::{
    IngestionError, IngestionErrorClassification, IngestionState, JsonValue, Nullable,
};
use weather_database::{
    ASSERT_RUNNING_SCHEDULED_MODE_SQL, AdmittedAdapterVersion, AdmittedIngestionError,
    AdmittedScheduledRunTimes, AssertRunningScheduledBind, AssertRunningScheduledQuery,
    FINALIZE_FAILED_INGESTION_SQL, FIND_RUNNING_INGESTION_SQL, FailScheduledIngestion,
    FinalizeFailedIngestionBind, FinalizeFailedIngestionQuery, FindRunningIngestionBind,
    FindRunningIngestionQuery, GET_SCHEDULED_CHECKPOINT_SQL, GetScheduledCheckpointBind,
    GetScheduledCheckpointQuery, INSERT_SCHEDULED_INGESTION_RUN_SQL, InsertScheduledIngestionBind,
    InsertScheduledIngestionQuery, RetainedSourceSession, RunningIngestionLookup,
    ScheduledCheckpointDbRow, ScheduledCheckpointReadError, ScheduledCheckpointReadPort,
    ScheduledRunFailureCommandPort, ScheduledRunFailureError, ScheduledRunFailureGuard,
    ScheduledRunStartCommandPort, ScheduledRunStartError, SerializedJson, Sha256Digest,
    StartScheduledIngestion, StartedScheduledIngestion, fail_scheduled_ingestion,
    get_scheduled_checkpoint, start_scheduled_ingestion,
};

const REQUESTED_START: &str = "2027-01-10T10:00:00.000Z";
const REQUESTED_END_EXCLUSIVE: &str = "2027-01-10T11:00:00.000Z";
const DEADLINE_AT: &str = "2027-01-10T11:15:00.000Z";
const LAST_VALID_AT: &str = "2027-01-10T10:55:00.000Z";
const FINGERPRINT: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

#[derive(Debug, Clone, PartialEq, Eq)]
struct FixtureError(&'static str);

impl Display for FixtureError {
    /// render one fixture failure
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.0)
    }
}

impl Error for FixtureError {}

#[derive(Debug, Clone, PartialEq, Eq)]
struct StartPort {
    existing_run_id: Option<String>,
    trace: Vec<String>,
}

impl RetainedSourceSession for StartPort {
    /// return the immutable fixture source identity
    fn source_id(&self) -> &str {
        "source-1"
    }
}

impl ScheduledRunStartCommandPort for StartPort {
    type Error = FixtureError;

    /// verify and record the running-row lookup
    fn find_running_ingestion(
        &mut self,
        query: &FindRunningIngestionQuery,
    ) -> Result<RunningIngestionLookup, Self::Error> {
        assert_eq!(query.sql, FIND_RUNNING_INGESTION_SQL);
        assert_eq!(
            query.binds,
            [FindRunningIngestionBind::BigintText("source-1".to_owned())]
        );
        self.trace.push("lookup:source-1".to_owned());
        Ok(RunningIngestionLookup {
            first_run_id: self.existing_run_id.clone(),
            row_count: usize::from(self.existing_run_id.is_some()),
        })
    }

    /// verify and record the scheduled running-row insert
    fn insert_scheduled_ingestion(
        &mut self,
        query: &InsertScheduledIngestionQuery,
    ) -> Result<Option<StartedScheduledIngestion>, Self::Error> {
        assert_eq!(query.sql, INSERT_SCHEDULED_INGESTION_RUN_SQL);
        assert_eq!(
            query.binds,
            [
                InsertScheduledIngestionBind::BigintText("source-1".to_owned()),
                InsertScheduledIngestionBind::ScheduledMode,
                InsertScheduledIngestionBind::UtcInstant(REQUESTED_START.to_owned()),
                InsertScheduledIngestionBind::UtcInstant(REQUESTED_END_EXCLUSIVE.to_owned()),
                InsertScheduledIngestionBind::Fingerprint(Sha256Digest::admitted(FINGERPRINT)),
                InsertScheduledIngestionBind::Text("open-meteo/v1".to_owned()),
                InsertScheduledIngestionBind::NullChunkPlanVersion,
                InsertScheduledIngestionBind::UtcInstant(DEADLINE_AT.to_owned()),
                InsertScheduledIngestionBind::Count(2),
                InsertScheduledIngestionBind::NullableJson(Some(SerializedJson::admitted(
                    r#"{"requestId":"request-7"}"#,
                ))),
            ]
        );
        self.trace.push("insert:scheduled:2".to_owned());
        Ok(Some(StartedScheduledIngestion {
            id: "run-1".to_owned(),
            started_at: REQUESTED_START.to_owned(),
            state: IngestionState::Running,
        }))
    }
}

#[derive(Debug, Clone, PartialEq)]
struct CheckpointPort {
    include_row: bool,
    row_source_id: String,
    trace: Vec<String>,
}

impl RetainedSourceSession for CheckpointPort {
    /// return the immutable fixture source identity
    fn source_id(&self) -> &str {
        "source-1"
    }
}

impl ScheduledCheckpointReadPort for CheckpointPort {
    type Error = FixtureError;

    /// verify and return one optional decoded checkpoint row
    fn get_scheduled_checkpoint(
        &mut self,
        query: &GetScheduledCheckpointQuery,
    ) -> Result<Option<ScheduledCheckpointDbRow>, Self::Error> {
        assert_eq!(query.sql, GET_SCHEDULED_CHECKPOINT_SQL);
        assert_eq!(
            query.binds,
            [GetScheduledCheckpointBind::BigintText(
                "source-1".to_owned()
            )]
        );
        self.trace.push("read:source-1".to_owned());

        // mirror the optional first-row projection
        if !self.include_row {
            return Ok(None);
        }

        Ok(Some(ScheduledCheckpointDbRow {
            last_committed_at: "2027-01-10T10:56:00.000Z".to_owned(),
            last_valid_at: LAST_VALID_AT.to_owned(),
            provider_cursor: Nullable::Value(vec![(
                utf16("cursor"),
                JsonValue::String(utf16("cursor-8")),
            )]),
            source_id: self.row_source_id.clone(),
            version: 7.0,
            window_end_exclusive: REQUESTED_END_EXCLUSIVE.to_owned(),
            window_start: REQUESTED_START.to_owned(),
        }))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FailurePort {
    assertion_row_count: usize,
    trace: Vec<String>,
}

impl RetainedSourceSession for FailurePort {
    /// return the immutable fixture source identity
    fn source_id(&self) -> &str {
        "source-1"
    }
}

impl ScheduledRunFailureCommandPort for FailurePort {
    type Error = FixtureError;

    /// record transaction start
    fn begin_transaction(&mut self) -> Result<(), Self::Error> {
        self.trace.push("begin".to_owned());
        Ok(())
    }

    /// verify and record the scheduled running-mode guard
    fn assert_running_scheduled(
        &mut self,
        query: &AssertRunningScheduledQuery,
    ) -> Result<usize, Self::Error> {
        assert_eq!(query.sql, ASSERT_RUNNING_SCHEDULED_MODE_SQL);
        assert_eq!(
            query.binds,
            [
                AssertRunningScheduledBind::BigintText("run-1".to_owned()),
                AssertRunningScheduledBind::BigintText("source-1".to_owned()),
                AssertRunningScheduledBind::ScheduledMode,
            ]
        );
        self.trace
            .push("assert:run-1:source-1:scheduled".to_owned());
        Ok(self.assertion_row_count)
    }

    /// verify and record failed-run durable evidence
    fn finalize_failed_ingestion(
        &mut self,
        query: &FinalizeFailedIngestionQuery,
    ) -> Result<usize, Self::Error> {
        assert_eq!(query.sql, FINALIZE_FAILED_INGESTION_SQL);
        assert_eq!(
            query.binds,
            [
                FinalizeFailedIngestionBind::BigintText("run-1".to_owned()),
                FinalizeFailedIngestionBind::BigintText("source-1".to_owned()),
                FinalizeFailedIngestionBind::Count(3),
                FinalizeFailedIngestionBind::NullableJson(Some(SerializedJson::admitted(
                    r#"{"requestId":"response-8"}"#,
                ))),
                FinalizeFailedIngestionBind::Classification(
                    IngestionErrorClassification::Retryable,
                ),
                FinalizeFailedIngestionBind::Text("network_timeout".to_owned()),
                FinalizeFailedIngestionBind::Text("provider timed out".to_owned()),
            ]
        );
        self.trace.push(
            r#"finalize:3:{"requestId":"response-8"}:retryable:network_timeout:provider timed out"#
                .to_owned(),
        );
        Ok(1)
    }

    /// record transaction commit
    fn commit_transaction(&mut self) -> Result<(), Self::Error> {
        self.trace.push("commit".to_owned());
        Ok(())
    }

    /// record transaction rollback
    fn rollback_transaction(&mut self) -> Result<(), Self::Error> {
        self.trace.push("rollback".to_owned());
        Ok(())
    }
}

/// encode one fixture value through generated UTF-16 JSON storage
fn utf16(value: &str) -> Vec<u16> {
    value.encode_utf16().collect()
}

/// build one shared validated scheduled-start fixture
fn start_input() -> StartScheduledIngestion {
    StartScheduledIngestion {
        adapter_version: AdmittedAdapterVersion::admitted("open-meteo/v1"),
        attempts: 2,
        request_metadata_json: Some(SerializedJson::admitted(r#"{"requestId":"request-7"}"#)),
        source_config_fingerprint: Sha256Digest::admitted(FINGERPRINT),
        times: AdmittedScheduledRunTimes::admitted(
            REQUESTED_START,
            REQUESTED_END_EXCLUSIVE,
            DEADLINE_AT,
        ),
    }
}

/// build one shared validated scheduled-failure fixture
fn failure_input() -> FailScheduledIngestion {
    FailScheduledIngestion {
        attempts: 3,
        error: AdmittedIngestionError::admitted(IngestionError {
            classification: IngestionErrorClassification::Retryable,
            code: "network_timeout".to_owned(),
            message: "provider timed out".to_owned(),
            metadata: weather_contracts::FieldPresence::Missing,
        }),
        response_metadata_json: Some(SerializedJson::admitted(r#"{"requestId":"response-8"}"#)),
        run_id: "run-1".to_owned(),
    }
}

/// render one Node-compatible trace line
fn rendered_trace(label: &str, trace: &[String]) -> String {
    format!("{label}={}", trace.join("|"))
}

// verifies compiled Node lifecycle and transaction ordering parity
#[test]
fn matches_compiled_node_scheduled_lifecycle() {
    let oracle = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/scheduled-run-lifecycle-node-oracle.mjs");
    let output = Command::new("node")
        .arg(oracle)
        .output()
        .expect("run scheduled lifecycle Node oracle");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("Node oracle UTF-8");
    let lines = stdout.lines().collect::<Vec<_>>();

    let mut start_port = StartPort {
        existing_run_id: None,
        trace: Vec::new(),
    };
    let started =
        start_scheduled_ingestion(&start_input(), &mut start_port).expect("scheduled start");
    assert_eq!(started.id, "run-1");
    assert_eq!(started.started_at, REQUESTED_START);
    assert_eq!(started.state, IngestionState::Running);
    assert_eq!(lines[0], rendered_trace("start", &start_port.trace));

    let mut existing_port = StartPort {
        existing_run_id: Some("run-existing".to_owned()),
        trace: Vec::new(),
    };
    let error = start_scheduled_ingestion(&start_input(), &mut existing_port)
        .expect_err("running conflict");
    assert_eq!(
        error,
        ScheduledRunStartError::ExistingRunningIngestion {
            run_id: "run-existing".to_owned(),
        }
    );
    assert_eq!(
        lines[1],
        rendered_trace("start-existing", &existing_port.trace)
    );
    assert_eq!(lines[2], format!("start-error={error}"));

    let mut checkpoint_port = CheckpointPort {
        include_row: true,
        row_source_id: "source-1".to_owned(),
        trace: Vec::new(),
    };
    let checkpoint = get_scheduled_checkpoint(&mut checkpoint_port)
        .expect("checkpoint read")
        .expect("checkpoint row");
    assert_eq!(checkpoint.sourceId, "source-1");
    assert_eq!(checkpoint.version, 7.0);
    assert_eq!(checkpoint.lastValidAt, LAST_VALID_AT);
    assert_eq!(
        checkpoint.providerCursor,
        Nullable::Value(vec![(
            utf16("cursor"),
            JsonValue::String(utf16("cursor-8")),
        )])
    );
    assert_eq!(
        lines[3],
        rendered_trace("checkpoint", &checkpoint_port.trace)
    );

    let mut missing_port = CheckpointPort {
        include_row: false,
        row_source_id: "source-1".to_owned(),
        trace: Vec::new(),
    };
    assert_eq!(
        get_scheduled_checkpoint(&mut missing_port).expect("missing checkpoint"),
        None
    );
    assert_eq!(
        lines[4],
        rendered_trace("checkpoint-missing", &missing_port.trace)
    );

    let mut cross_source_port = CheckpointPort {
        include_row: true,
        row_source_id: "source-2".to_owned(),
        trace: Vec::new(),
    };
    let error =
        get_scheduled_checkpoint(&mut cross_source_port).expect_err("cross-source checkpoint row");
    assert_eq!(
        error,
        ScheduledCheckpointReadError::SourceMismatch {
            expected: "source-1".to_owned(),
            actual: "source-2".to_owned(),
        }
    );
    assert_eq!(cross_source_port.trace, ["read:source-1"]);

    let mut failure_port = FailurePort {
        assertion_row_count: 1,
        trace: Vec::new(),
    };
    fail_scheduled_ingestion(&failure_input(), &mut failure_port).expect("scheduled failure");
    assert_eq!(lines[5], rendered_trace("failure", &failure_port.trace));

    let mut mismatch_port = FailurePort {
        assertion_row_count: 0,
        trace: Vec::new(),
    };
    let error = fail_scheduled_ingestion(&failure_input(), &mut mismatch_port)
        .expect_err("running mode mismatch");
    assert_eq!(
        error,
        ScheduledRunFailureError::GuardedWrite {
            guard: ScheduledRunFailureGuard::RunningMode,
            row_count: 0,
        }
    );
    assert_eq!(
        lines[6],
        rendered_trace("failure-mismatch", &mismatch_port.trace)
    );
    assert_eq!(lines[7], format!("failure-error={error}"));
}

// verifies every copied lifecycle SQL string remains source-owned
#[test]
fn copies_exact_source_scheduled_lifecycle_sql() {
    const SOURCE: &str = include_str!("../../../../packages/database/src/repositories.ts");

    assert!(SOURCE.contains(FIND_RUNNING_INGESTION_SQL));
    assert!(SOURCE.contains(INSERT_SCHEDULED_INGESTION_RUN_SQL));
    assert!(SOURCE.contains(GET_SCHEDULED_CHECKPOINT_SQL));
    assert!(SOURCE.contains(ASSERT_RUNNING_SCHEDULED_MODE_SQL));
    assert!(SOURCE.contains(FINALIZE_FAILED_INGESTION_SQL));
}
