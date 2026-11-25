use std::error::Error;
use std::fmt::{Display, Formatter};
use std::path::PathBuf;
use std::process::Command;

use weather_contracts::{
    CanonicalWeatherMetrics, DeviceMetadata, FieldPresence, JsonValue, NormalizedWeatherRecord,
    Nullable, SourceKind, WeatherRecordMetadata,
};
use weather_database::{
    ASSERT_SCHEDULED_RUN_SQL, AdmittedRevisionUtcInstant, CheckpointUpdateBind,
    CompleteScheduledIngestion, ExistingWeatherRevisionIdentity, FINALIZE_SCHEDULED_RUN_SQL,
    FinalizeScheduledRunBind, FinalizeScheduledRunQuery, INSERT_SCHEDULED_CHECKPOINT_SQL,
    PersistedWeatherAdjustmentRevision, PreparedWeatherRecord,
    RELEASE_WEATHER_REVISION_ARCHIVE_SQL, ROLLBACK_WEATHER_REVISION_ARCHIVE_SQL,
    RetainedSourceSession, SAVEPOINT_WEATHER_REVISION_ARCHIVE_SQL, ScheduledCheckpointQuery,
    ScheduledIngestionCommandPort, ScheduledIngestionExecutionError, ScheduledIngestionGuard,
    ScheduledRunIdentityBind, ScheduledRunIdentityQuery, SerializedJson, Sha256Digest,
    UPDATE_SCHEDULED_CHECKPOINT_SQL, WeatherRecordBind, WeatherRecordStorageEvidence,
    WeatherRecordUpsertQuery, WeatherRevisionArchivePort, WeatherRevisionContextBind,
    WeatherRevisionContextQuery, WeatherRevisionLookupRequest, WeatherRevisionSavepointCommand,
    WeatherRevisionSelectionBind, WeatherRevisionSelectionQuery, WeatherRevisionSourceContext,
    complete_scheduled_ingestion, complete_scheduled_ingestion_with_revision_archive,
    weather_record_upsert_query,
};

const WINDOW_START: &str = "2026-11-23T10:00:00.000Z";
const WINDOW_END_EXCLUSIVE: &str = "2026-11-23T11:00:00.000Z";
const LAST_VALID_AT: &str = "2026-11-23T10:55:00.000Z";
const RECEIVED_AT: &str = "2026-11-23T10:56:00.000Z";
const CHECKSUM: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const CONTENT_HASH: &str = "afab5522454cb11e98ba63f6a333c4aade9abc705e0834d703ac5b0252e81f91";

#[derive(Debug, Clone, PartialEq, Eq)]
struct FixturePort {
    checkpoint_row_count: usize,
    fail_commit: bool,
    source_id: String,
    trace: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FixtureError(&'static str);

impl Display for FixtureError {
    /// render one injected fixture failure.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.0)
    }
}

impl Error for FixtureError {}

impl RetainedSourceSession for FixturePort {
    /// return the immutable fixture source identity
    fn source_id(&self) -> &str {
        &self.source_id
    }
}

impl ScheduledIngestionCommandPort for FixturePort {
    type Error = FixtureError;

    /// record the transaction start.
    fn begin_transaction(&mut self) -> Result<(), Self::Error> {
        self.trace.push("begin".to_owned());
        Ok(())
    }

    /// verify and record the exact running-row identity query.
    fn assert_scheduled_run(
        &mut self,
        query: &ScheduledRunIdentityQuery,
    ) -> Result<usize, Self::Error> {
        assert_eq!(query.sql, ASSERT_SCHEDULED_RUN_SQL);
        assert_eq!(
            query.binds,
            [
                ScheduledRunIdentityBind::BigintText("run-1".to_owned()),
                ScheduledRunIdentityBind::BigintText("source-1".to_owned()),
                ScheduledRunIdentityBind::UtcInstant(WINDOW_START.to_owned()),
                ScheduledRunIdentityBind::UtcInstant(WINDOW_END_EXCLUSIVE.to_owned()),
            ]
        );
        self.trace.push(format!(
            "assert:run-1:source-1:{WINDOW_START}:{WINDOW_END_EXCLUSIVE}"
        ));
        Ok(1)
    }

    /// verify and record the full normalized record write.
    fn upsert_weather_records(
        &mut self,
        query: &WeatherRecordUpsertQuery,
    ) -> Result<(), Self::Error> {
        assert_eq!(query.record_count, 1);
        assert_eq!(query.binds.len(), 32);
        assert!(query.sql.contains("INSERT INTO weather_records"));
        assert!(query.sql.contains("($1, $2, $3, $4, $5, $5, $6, $6"));
        assert_eq!(
            query.binds[11],
            WeatherRecordBind::NullableJson(Some(
                SerializedJson::admitted(r#"{"accepted":true}"#,)
            ))
        );
        assert_eq!(
            query.binds[12],
            WeatherRecordBind::NullableJson(Some(SerializedJson::admitted(
                r#"{"requestId":"response-7"}"#,
            )))
        );
        assert_eq!(
            query.binds[31],
            WeatherRecordBind::ContentHash(Sha256Digest::admitted(CONTENT_HASH))
        );
        self.trace.push(format!(
            "records:{}:{}",
            query.record_count,
            query.binds.len()
        ));
        Ok(())
    }

    /// verify and record the checkpoint compare-and-set.
    fn advance_scheduled_checkpoint(
        &mut self,
        query: &ScheduledCheckpointQuery,
    ) -> Result<usize, Self::Error> {
        // the fixture exercises the optimistic update branch
        match query {
            ScheduledCheckpointQuery::Update { sql, binds } => {
                assert_eq!(*sql, UPDATE_SCHEDULED_CHECKPOINT_SQL);
                assert_eq!(
                    binds,
                    &[
                        CheckpointUpdateBind::BigintText("source-1".to_owned()),
                        CheckpointUpdateBind::Version(7),
                        CheckpointUpdateBind::UtcInstant(LAST_VALID_AT.to_owned()),
                        CheckpointUpdateBind::UtcInstant(WINDOW_START.to_owned()),
                        CheckpointUpdateBind::UtcInstant(WINDOW_END_EXCLUSIVE.to_owned()),
                        CheckpointUpdateBind::NullableJson(Some(SerializedJson::admitted(
                            r#"{"cursor":"cursor-8"}"#,
                        ))),
                    ]
                );
                self.trace
                    .push(r#"checkpoint:update:7:{"cursor":"cursor-8"}"#.to_owned());
            }
            ScheduledCheckpointQuery::Insert { .. } => panic!("expected update checkpoint"),
        }

        Ok(self.checkpoint_row_count)
    }

    /// verify and record the durable success evidence.
    fn finalize_scheduled_run(
        &mut self,
        query: &FinalizeScheduledRunQuery,
    ) -> Result<usize, Self::Error> {
        assert_eq!(query.sql, FINALIZE_SCHEDULED_RUN_SQL);
        assert_eq!(
            query.binds,
            [
                FinalizeScheduledRunBind::BigintText("run-1".to_owned()),
                FinalizeScheduledRunBind::BigintText("source-1".to_owned()),
                FinalizeScheduledRunBind::Count(2),
                FinalizeScheduledRunBind::Count(1),
                FinalizeScheduledRunBind::NullableJson(Some(SerializedJson::admitted(
                    r#"{"etag":"response-9"}"#,
                ))),
                FinalizeScheduledRunBind::NullableChecksum(Some(Sha256Digest::admitted(CHECKSUM))),
                FinalizeScheduledRunBind::ScheduledMode,
            ]
        );
        self.trace.push(format!(
            r#"finalize:2:1:{{"etag":"response-9"}}:{CHECKSUM}:scheduled"#
        ));
        Ok(1)
    }

    /// record transaction commit.
    fn commit_transaction(&mut self) -> Result<(), Self::Error> {
        self.trace.push("commit".to_owned());

        // inject a commit failure at the same point as the Node fixture
        if self.fail_commit {
            return Err(FixtureError("commit failed"));
        }

        Ok(())
    }

    /// record transaction rollback.
    fn rollback_transaction(&mut self) -> Result<(), Self::Error> {
        self.trace.push("rollback".to_owned());
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ArchivePublication;

#[derive(Debug, Clone, PartialEq, Eq)]
struct ArchiveFixturePort {
    archive_mode: &'static str,
    requests: Vec<WeatherRevisionLookupRequest>,
    scheduled: FixturePort,
}

impl ArchiveFixturePort {
    /// create one archive-capable scheduled fixture on a single port.
    fn new(checkpoint_row_count: usize, fail_commit: bool, archive_mode: &'static str) -> Self {
        Self {
            archive_mode,
            requests: Vec::new(),
            scheduled: FixturePort {
                checkpoint_row_count,
                fail_commit,
                source_id: "source-1".to_owned(),
                trace: Vec::new(),
            },
        }
    }

    /// borrow the shared scheduled and archive trace.
    fn trace(&self) -> &[String] {
        &self.scheduled.trace
    }
}

impl RetainedSourceSession for ArchiveFixturePort {
    /// return the one source retained by both command surfaces.
    fn source_id(&self) -> &str {
        self.scheduled.source_id()
    }
}

impl ScheduledIngestionCommandPort for ArchiveFixturePort {
    type Error = FixtureError;

    /// delegate transaction start to the shared scheduled port.
    fn begin_transaction(&mut self) -> Result<(), Self::Error> {
        self.scheduled.begin_transaction()
    }

    /// delegate the running-row assertion to the shared scheduled port.
    fn assert_scheduled_run(
        &mut self,
        query: &ScheduledRunIdentityQuery,
    ) -> Result<usize, Self::Error> {
        self.scheduled.assert_scheduled_run(query)
    }

    /// delegate serving writes to the shared scheduled port.
    fn upsert_weather_records(
        &mut self,
        query: &WeatherRecordUpsertQuery,
    ) -> Result<(), Self::Error> {
        self.scheduled.upsert_weather_records(query)
    }

    /// delegate checkpoint advancement to the shared scheduled port.
    fn advance_scheduled_checkpoint(
        &mut self,
        query: &ScheduledCheckpointQuery,
    ) -> Result<usize, Self::Error> {
        self.scheduled.advance_scheduled_checkpoint(query)
    }

    /// delegate successful finalization to the shared scheduled port.
    fn finalize_scheduled_run(
        &mut self,
        query: &FinalizeScheduledRunQuery,
    ) -> Result<usize, Self::Error> {
        self.scheduled.finalize_scheduled_run(query)
    }

    /// delegate the one transaction commit to the shared scheduled port.
    fn commit_transaction(&mut self) -> Result<(), Self::Error> {
        self.scheduled.commit_transaction()
    }

    /// delegate the one transaction rollback to the shared scheduled port.
    fn rollback_transaction(&mut self) -> Result<(), Self::Error> {
        self.scheduled.rollback_transaction()
    }
}

impl WeatherRevisionArchivePort for ArchiveFixturePort {
    type Error = FixtureError;
    type Publication = ArchivePublication;

    /// validate typed hash, time, and identity requests before JSON encoding.
    fn serialize_revision_requests(
        &mut self,
        requests: &[WeatherRevisionLookupRequest],
    ) -> Result<SerializedJson, Self::Error> {
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].content_hash.as_str(), CONTENT_HASH);
        assert_eq!(requests[0].product_run_at, None);
        assert_eq!(requests[0].source_kind, SourceKind::PhysicalSensor);
        assert_eq!(requests[0].valid_at.as_str(), LAST_VALID_AT);
        self.requests = requests.to_vec();
        Ok(SerializedJson::admitted("fixture-revision-requests"))
    }

    /// select no prior row so the prepared value is archive eligible.
    fn select_existing_weather_revisions(
        &mut self,
        query: &WeatherRevisionSelectionQuery,
    ) -> Result<Vec<ExistingWeatherRevisionIdentity>, Self::Error> {
        assert_eq!(
            query.binds,
            [
                WeatherRevisionSelectionBind::Text("source-1".to_owned()),
                WeatherRevisionSelectionBind::Json(SerializedJson::admitted(
                    "fixture-revision-requests",
                )),
            ]
        );
        self.scheduled.trace.push("revision-select:1".to_owned());
        Ok(Vec::new())
    }

    /// return one locked source lineage after the serving write.
    fn load_weather_revision_source_context(
        &mut self,
        query: &WeatherRevisionContextQuery,
    ) -> Result<Option<WeatherRevisionSourceContext>, Self::Error> {
        assert_eq!(
            query.binds,
            [
                WeatherRevisionContextBind::Text("run-1".to_owned()),
                WeatherRevisionContextBind::Text("source-1".to_owned()),
            ]
        );
        self.scheduled.trace.push("revision-context".to_owned());
        Ok(Some(WeatherRevisionSourceContext {
            adapter_version: "adapter/v1".to_owned(),
            logical_received_at: AdmittedRevisionUtcInstant::admitted("2026-11-23T10:54:00.000Z"),
            provider_key: "provider-1".to_owned(),
            source_config_fingerprint: "fingerprint-1".to_owned(),
            source_key: "source-key-1".to_owned(),
        }))
    }

    /// execute exact savepoint operations on the shared transaction trace.
    fn execute_weather_revision_savepoint(
        &mut self,
        command: WeatherRevisionSavepointCommand,
    ) -> Result<(), Self::Error> {
        // retain the source command spelling and transaction order
        match command {
            WeatherRevisionSavepointCommand::Savepoint => {
                assert_eq!(command.sql(), SAVEPOINT_WEATHER_REVISION_ARCHIVE_SQL);
                self.scheduled.trace.push("revision-savepoint".to_owned());
            }
            WeatherRevisionSavepointCommand::RollbackTo => {
                assert_eq!(command.sql(), ROLLBACK_WEATHER_REVISION_ARCHIVE_SQL);
                self.scheduled.trace.push("revision-rollback-to".to_owned());
            }
            WeatherRevisionSavepointCommand::Release => {
                assert_eq!(command.sql(), RELEASE_WEATHER_REVISION_ARCHIVE_SQL);
                self.scheduled.trace.push("revision-release".to_owned());
            }
        }

        Ok(())
    }

    /// stage the exact changed record after its serving upsert.
    fn stage_weather_revisions(
        &mut self,
        revisions: &[PersistedWeatherAdjustmentRevision],
    ) -> Result<Option<Self::Publication>, Self::Error> {
        assert_eq!(revisions.len(), 1);
        assert_eq!(revisions[0].content_hash.as_str(), CONTENT_HASH);
        assert_eq!(revisions[0].record.validAt, LAST_VALID_AT);
        assert_eq!(
            revisions[0].logical_received_at.as_str(),
            "2026-11-23T10:54:00.000Z"
        );

        // preserve the source's swallowed stage failure path
        if self.archive_mode == "stage-failure" {
            self.scheduled.trace.push("revision-stage!".to_owned());
            return Err(FixtureError("stage failed"));
        }

        self.scheduled.trace.push("revision-stage:1".to_owned());
        Ok(Some(ArchivePublication))
    }

    /// record the post-commit publication attempt.
    fn publish_weather_revisions(
        &mut self,
        _publication: Self::Publication,
    ) -> Result<(), Self::Error> {
        self.scheduled.trace.push("revision-publish".to_owned());
        Ok(())
    }
}

/// encode one fixture key through the generated UTF-16 JSON storage.
fn utf16(value: &str) -> Vec<u16> {
    value.encode_utf16().collect()
}

/// build the canonical normalized row shared with the Node oracle.
fn prepared_record() -> PreparedWeatherRecord {
    PreparedWeatherRecord {
        record: NormalizedWeatherRecord {
            metadata: WeatherRecordMetadata {
                device: Nullable::Value(DeviceMetadata {
                    model: FieldPresence::Value("GW2000".to_owned()),
                    serial: FieldPresence::Value("station-7".to_owned()),
                    vendor: FieldPresence::Value("Ecowitt".to_owned()),
                }),
                model: Nullable::Null,
                provider: Nullable::Value(vec![(
                    utf16("requestId"),
                    JsonValue::String(utf16("response-7")),
                )]),
                quality: Nullable::Value(vec![(utf16("accepted"), JsonValue::Boolean(true))]),
                upstreamTimezone: "Europe/London".to_owned(),
            },
            metrics: CanonicalWeatherMetrics {
                apparentTemperatureC: Nullable::Value(8.25),
                blackGlobeTemperatureC: Nullable::Null,
                cloudCoverPercent: Nullable::Value(75.0),
                pm25MicrogramsPerCubicMeter: Nullable::Value(4.5),
                precipitationMm: Nullable::Value(0.2),
                precipitationRateMmPerHour: Nullable::Value(1.25),
                pressureHpa: Nullable::Value(1008.4),
                relativeHumidityPercent: Nullable::Value(86.0),
                soilElectricalConductivityMicrosiemensPerCm: Nullable::Null,
                soilMoisturePercent: Nullable::Null,
                solarRadiationWm2: Nullable::Value(32.0),
                temperatureC: Nullable::Value(9.5),
                uvIndex: Nullable::Value(0.0),
                waterLevelM: Nullable::Null,
                windDirectionDegrees: Nullable::Value(245.0),
                windGustMps: Nullable::Value(8.75),
                windSpeedMps: Nullable::Value(4.25),
                wetBulbGlobeTemperatureC: Nullable::Null,
            },
            productRunAt: Nullable::Null,
            receivedAt: RECEIVED_AT.to_owned(),
            sourceId: "source-1".to_owned(),
            sourceKind: SourceKind::PhysicalSensor,
            validAt: LAST_VALID_AT.to_owned(),
        },
        storage: WeatherRecordStorageEvidence {
            content_hash: Sha256Digest::admitted(CONTENT_HASH),
            provider_metadata_json: Some(SerializedJson::admitted(r#"{"requestId":"response-7"}"#)),
            quality_metadata_json: Some(SerializedJson::admitted(r#"{"accepted":true}"#)),
        },
    }
}

/// build the complete shared scheduled-ingestion fixture.
fn input() -> CompleteScheduledIngestion {
    CompleteScheduledIngestion {
        attempts: 2,
        expected_checkpoint_version: Some(7),
        last_valid_at: LAST_VALID_AT.to_owned(),
        provider_cursor_json: Some(SerializedJson::admitted(r#"{"cursor":"cursor-8"}"#)),
        records: vec![prepared_record()],
        response_metadata_json: Some(SerializedJson::admitted(r#"{"etag":"response-9"}"#)),
        run_id: "run-1".to_owned(),
        upstream_response_checksum: Some(Sha256Digest::admitted(CHECKSUM)),
        window_end_exclusive: WINDOW_END_EXCLUSIVE.to_owned(),
        window_start: WINDOW_START.to_owned(),
    }
}

/// render the exact trace line consumed from the Node oracle.
fn rendered_trace(label: &str, trace: &[String]) -> String {
    format!("{label}={}", trace.join("|"))
}

// verifies compiled TypeScript success, mismatch, and commit ordering parity
#[test]
fn matches_compiled_node_transaction_ordering() {
    let oracle =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/scheduled-ingestion-node-oracle.mjs");
    let output = Command::new("node")
        .arg(oracle)
        .output()
        .expect("run scheduled ingestion Node oracle");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let node_lines = String::from_utf8(output.stdout).expect("Node oracle UTF-8");
    let node_lines = node_lines.lines().collect::<Vec<_>>();

    let mut success_port = FixturePort {
        checkpoint_row_count: 1,
        fail_commit: false,
        source_id: "source-1".to_owned(),
        trace: Vec::new(),
    };
    complete_scheduled_ingestion(&input(), &mut success_port).expect("scheduled success");
    assert_eq!(
        node_lines[0],
        rendered_trace("success", &success_port.trace)
    );

    let mut mismatch_port = FixturePort {
        checkpoint_row_count: 0,
        fail_commit: false,
        source_id: "source-1".to_owned(),
        trace: Vec::new(),
    };
    let error = complete_scheduled_ingestion(&input(), &mut mismatch_port)
        .expect_err("checkpoint mismatch");
    assert_eq!(node_lines[2], format!("checkpoint-error={error}"));
    assert_eq!(
        error,
        ScheduledIngestionExecutionError::GuardedWrite {
            guard: ScheduledIngestionGuard::CheckpointCompareAndSet,
            row_count: 0,
        }
    );
    assert_eq!(
        node_lines[1],
        rendered_trace("checkpoint-mismatch", &mismatch_port.trace)
    );
    let mut commit_failure_port = FixturePort {
        checkpoint_row_count: 1,
        fail_commit: true,
        source_id: "source-1".to_owned(),
        trace: Vec::new(),
    };
    let error = complete_scheduled_ingestion(&input(), &mut commit_failure_port)
        .expect_err("commit failure");
    assert_eq!(
        error,
        ScheduledIngestionExecutionError::Command(FixtureError("commit failed"))
    );
    assert_eq!(
        node_lines[3],
        rendered_trace("commit-failure", &commit_failure_port.trace)
    );
    assert_eq!(node_lines[4], "commit-error=commit failed");

    // find one appended archive signature without coupling legacy line indices
    let archive_line = |label: &str| {
        node_lines
            .iter()
            .copied()
            .find(|line| line.starts_with(label))
            .expect("archive oracle line")
    };
    let mut archive_success = ArchiveFixturePort::new(1, false, "armed");
    complete_scheduled_ingestion_with_revision_archive(&input(), &mut archive_success)
        .expect("archive scheduled success");
    assert_eq!(
        archive_line("archive-success="),
        rendered_trace("archive-success", archive_success.trace())
    );

    let mut archive_stage_failure = ArchiveFixturePort::new(1, false, "stage-failure");
    complete_scheduled_ingestion_with_revision_archive(&input(), &mut archive_stage_failure)
        .expect("swallowed archive stage failure");
    assert_eq!(
        archive_line("archive-stage-failure="),
        rendered_trace("archive-stage-failure", archive_stage_failure.trace())
    );

    let mut archive_checkpoint_failure = ArchiveFixturePort::new(0, false, "armed");
    let error = complete_scheduled_ingestion_with_revision_archive(
        &input(),
        &mut archive_checkpoint_failure,
    )
    .expect_err("archive checkpoint failure");
    assert_eq!(
        archive_line("archive-checkpoint-failure="),
        rendered_trace(
            "archive-checkpoint-failure",
            archive_checkpoint_failure.trace()
        )
    );
    assert_eq!(
        archive_line("archive-checkpoint-error="),
        format!("archive-checkpoint-error={error}")
    );

    let mut archive_commit_failure = ArchiveFixturePort::new(1, true, "armed");
    let error =
        complete_scheduled_ingestion_with_revision_archive(&input(), &mut archive_commit_failure)
            .expect_err("archive commit failure");
    assert_eq!(
        archive_line("archive-commit-failure="),
        rendered_trace("archive-commit-failure", archive_commit_failure.trace())
    );
    assert_eq!(
        archive_line("archive-commit-error="),
        format!("archive-commit-error={error}")
    );
}

// verifies exact row placeholder and cross-source rejection
#[test]
fn builds_exact_record_binds_and_rejects_cross_source_rows() {
    let record = prepared_record();
    let query = weather_record_upsert_query("source-1", "run-1", std::slice::from_ref(&record))
        .expect("record plan");
    assert_eq!(query.binds.len(), 32);
    assert!(query.sql.contains("$12::jsonb, $13::jsonb"));

    let error = weather_record_upsert_query("source-2", "run-1", std::slice::from_ref(&record))
        .expect_err("cross-source record");
    assert_eq!(
        error,
        weather_database::ScheduledIngestionInputError::RecordSourceMismatch {
            expected: "source-2".to_owned(),
            actual: "source-1".to_owned(),
        }
    );

    let mut mismatched_input = input();
    mismatched_input.records[0].record.sourceId = "source-2".to_owned();
    let mut port = FixturePort {
        checkpoint_row_count: 1,
        fail_commit: false,
        source_id: "source-1".to_owned(),
        trace: Vec::new(),
    };
    let error = complete_scheduled_ingestion(&mismatched_input, &mut port)
        .expect_err("retained source mismatch");
    assert_eq!(
        error,
        ScheduledIngestionExecutionError::Input(
            weather_database::ScheduledIngestionInputError::RecordSourceMismatch {
                expected: "source-1".to_owned(),
                actual: "source-2".to_owned(),
            }
        )
    );
    assert!(port.trace.is_empty());
}

// verifies the guarded SQL remains byte-owned by the TypeScript source
#[test]
fn copies_exact_source_guard_checkpoint_and_finalization_sql() {
    const SOURCE: &str = include_str!("../../../../packages/database/src/repositories.ts");

    assert!(SOURCE.contains(ASSERT_SCHEDULED_RUN_SQL));
    assert!(SOURCE.contains(INSERT_SCHEDULED_CHECKPOINT_SQL));
    assert!(SOURCE.contains(UPDATE_SCHEDULED_CHECKPOINT_SQL));
    assert!(SOURCE.contains(FINALIZE_SCHEDULED_RUN_SQL));
}
