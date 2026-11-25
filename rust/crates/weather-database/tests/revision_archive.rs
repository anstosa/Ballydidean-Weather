use std::error::Error;
use std::fmt::{Display, Formatter};
use std::path::PathBuf;
use std::process::Command;

use weather_contracts::{
    CanonicalWeatherMetrics, NormalizedWeatherRecord, Nullable, SourceKind, WeatherRecordMetadata,
};
use weather_database::{
    AdmittedRevisionUtcInstant, ExistingWeatherRevisionIdentity,
    PersistedWeatherAdjustmentRevision, PreparedWeatherRecord,
    RELEASE_WEATHER_REVISION_ARCHIVE_SQL, ROLLBACK_WEATHER_REVISION_ARCHIVE_SQL,
    RetainedSourceSession, SAVEPOINT_WEATHER_REVISION_ARCHIVE_SQL,
    SELECT_EXISTING_WEATHER_REVISIONS_SQL, SerializedJson, Sha256Digest,
    WEATHER_REVISION_SOURCE_CONTEXT_SQL, WeatherRecordStorageEvidence, WeatherRevisionArchiveError,
    WeatherRevisionArchivePort, WeatherRevisionArchiveSelection, WeatherRevisionContextBind,
    WeatherRevisionContextQuery, WeatherRevisionLookupRequest, WeatherRevisionSavepointCommand,
    WeatherRevisionSelectionBind, WeatherRevisionSelectionQuery, WeatherRevisionSourceContext,
    publish_committed_weather_revisions, select_changed_weather_revision_inputs,
    stage_changed_weather_revisions,
};

const SOURCE_ID: &str = "source-1";
const RUN_ID: &str = "run-1";
const PRODUCT_RUN_AT: &str = "2026-11-23T06:00:00.000Z";
const LOGICAL_RECEIVED_AT: &str = "2026-11-23T10:14:00.000Z";
const VALID_AT: [&str; 3] = [
    "2026-11-23T10:00:00.000Z",
    "2026-11-23T10:05:00.000Z",
    "2026-11-23T10:10:00.000Z",
];
const CONTENT_HASHES: [&str; 3] = [
    "1111111111111111111111111111111111111111111111111111111111111111",
    "2222222222222222222222222222222222222222222222222222222222222222",
    "3333333333333333333333333333333333333333333333333333333333333333",
];

#[derive(Debug, Clone, PartialEq, Eq)]
struct FixtureError(&'static str);

impl Display for FixtureError {
    /// render one injected adapter failure.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.0)
    }
}

impl Error for FixtureError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FixturePublication;

#[derive(Debug, Clone, PartialEq, Eq)]
struct FixturePort {
    mode: &'static str,
    release_count: usize,
    requests: Vec<WeatherRevisionLookupRequest>,
    source_id: String,
    trace: Vec<String>,
}

impl FixturePort {
    /// create one empty retained archive port.
    fn new(mode: &'static str) -> Self {
        Self {
            mode,
            release_count: 0,
            requests: Vec::new(),
            source_id: SOURCE_ID.to_owned(),
            trace: Vec::new(),
        }
    }
}

impl RetainedSourceSession for FixturePort {
    /// return the immutable source identity.
    fn source_id(&self) -> &str {
        &self.source_id
    }
}

impl WeatherRevisionArchivePort for FixturePort {
    type Error = FixtureError;
    type Publication = FixturePublication;

    /// retain typed requests and return adapter-produced JSON evidence.
    fn serialize_revision_requests(
        &mut self,
        requests: &[WeatherRevisionLookupRequest],
    ) -> Result<SerializedJson, Self::Error> {
        self.requests = requests.to_vec();
        Ok(SerializedJson::admitted("fixture-request-json"))
    }

    /// verify exact SQL and expose the configured prior identities.
    fn select_existing_weather_revisions(
        &mut self,
        query: &WeatherRevisionSelectionQuery,
    ) -> Result<Vec<ExistingWeatherRevisionIdentity>, Self::Error> {
        assert_eq!(query.sql, SELECT_EXISTING_WEATHER_REVISIONS_SQL);
        assert_eq!(
            query.binds,
            [
                WeatherRevisionSelectionBind::Text(SOURCE_ID.to_owned()),
                WeatherRevisionSelectionBind::Json(SerializedJson::admitted(
                    "fixture-request-json"
                )),
            ]
        );
        self.trace.push(format!("select:{}", self.requests.len()));

        // make every candidate unchanged on the retry path
        if self.mode == "unchanged" {
            return Ok(self.requests.iter().map(existing_from_request).collect());
        }

        let first = existing_from_request(&self.requests[0]);
        let mut second = existing_from_request(&self.requests[1]);
        second.content_hash = Sha256Digest::admitted("0".repeat(64));
        Ok(vec![first, second])
    }

    /// verify exact context SQL and expose one locked lineage row.
    fn load_weather_revision_source_context(
        &mut self,
        query: &WeatherRevisionContextQuery,
    ) -> Result<Option<WeatherRevisionSourceContext>, Self::Error> {
        assert_eq!(query.sql, WEATHER_REVISION_SOURCE_CONTEXT_SQL);
        assert_eq!(
            query.binds,
            [
                WeatherRevisionContextBind::Text(RUN_ID.to_owned()),
                WeatherRevisionContextBind::Text(SOURCE_ID.to_owned()),
            ]
        );

        // preserve the source missing-row failure
        if self.mode == "missing-context" {
            self.trace.push("context:none".to_owned());
            return Ok(None);
        }

        self.trace.push("context".to_owned());
        Ok(Some(WeatherRevisionSourceContext {
            adapter_version: "adapter/v1".to_owned(),
            logical_received_at: AdmittedRevisionUtcInstant::admitted(LOGICAL_RECEIVED_AT),
            provider_key: "provider-1".to_owned(),
            source_config_fingerprint: "fingerprint-1".to_owned(),
            source_key: "source-key-1".to_owned(),
        }))
    }

    /// preserve exact savepoint commands and injected cleanup failures.
    fn execute_weather_revision_savepoint(
        &mut self,
        command: WeatherRevisionSavepointCommand,
    ) -> Result<(), Self::Error> {
        // route each exact command through the retained port
        match command {
            WeatherRevisionSavepointCommand::Savepoint => {
                assert_eq!(command.sql(), SAVEPOINT_WEATHER_REVISION_ARCHIVE_SQL);
                self.trace.push("savepoint".to_owned());
            }
            WeatherRevisionSavepointCommand::RollbackTo => {
                assert_eq!(command.sql(), ROLLBACK_WEATHER_REVISION_ARCHIVE_SQL);

                // preserve ordinary rollback failure propagation
                if self.mode == "rollback-failure" {
                    self.trace.push("rollback-to!".to_owned());
                    return Err(FixtureError("savepoint rollback failed"));
                }

                self.trace.push("rollback-to".to_owned());
            }
            WeatherRevisionSavepointCommand::Release => {
                assert_eq!(command.sql(), RELEASE_WEATHER_REVISION_ARCHIVE_SQL);
                self.release_count += 1;

                // fail only the first release so cleanup can finish
                if self.mode == "release-failure" && self.release_count == 1 {
                    self.trace.push("release!".to_owned());
                    return Err(FixtureError("savepoint release failed"));
                }

                self.trace.push("release".to_owned());
            }
        }

        Ok(())
    }

    /// verify changed eligibility and stage one publication.
    fn stage_weather_revisions(
        &mut self,
        revisions: &[PersistedWeatherAdjustmentRevision],
    ) -> Result<Option<Self::Publication>, Self::Error> {
        let valid_at = revisions
            .iter()
            .map(|revision| revision.record.validAt.as_str())
            .collect::<Vec<_>>();
        assert_eq!(valid_at, [VALID_AT[1], VALID_AT[2]]);
        assert_eq!(revisions[0].adapter_version, "adapter/v1");
        assert_eq!(
            revisions[0].logical_received_at.as_str(),
            LOGICAL_RECEIVED_AT
        );

        // inject the same stage failure as the compiled Node fixture
        if self.mode == "stage-failure" || self.mode == "rollback-failure" {
            self.trace.push("stage!".to_owned());
            return Err(FixtureError("stage failed"));
        }

        self.trace.push(format!("stage:{}", valid_at.join(",")));
        Ok(Some(FixturePublication))
    }

    /// expose one post-commit publication attempt.
    fn publish_weather_revisions(
        &mut self,
        _publication: Self::Publication,
    ) -> Result<(), Self::Error> {
        self.trace.push("publish".to_owned());

        // preserve successful ingestion after publication failure
        if self.mode == "publish-failure" {
            return Err(FixtureError("publish failed"));
        }

        Ok(())
    }
}

/// build one canonical generated record with a distinct serving identity.
fn record(index: usize) -> NormalizedWeatherRecord {
    NormalizedWeatherRecord {
        metadata: WeatherRecordMetadata {
            device: Nullable::Null,
            model: Nullable::Null,
            provider: Nullable::Null,
            quality: Nullable::Null,
            upstreamTimezone: "UTC".to_owned(),
        },
        metrics: CanonicalWeatherMetrics {
            apparentTemperatureC: Nullable::Null,
            blackGlobeTemperatureC: Nullable::Null,
            cloudCoverPercent: Nullable::Null,
            pm25MicrogramsPerCubicMeter: Nullable::Null,
            precipitationMm: Nullable::Null,
            precipitationRateMmPerHour: Nullable::Null,
            pressureHpa: Nullable::Null,
            relativeHumidityPercent: Nullable::Null,
            soilElectricalConductivityMicrosiemensPerCm: Nullable::Null,
            soilMoisturePercent: Nullable::Null,
            solarRadiationWm2: Nullable::Null,
            temperatureC: Nullable::Value(index as f64),
            uvIndex: Nullable::Null,
            waterLevelM: Nullable::Null,
            windDirectionDegrees: Nullable::Null,
            windGustMps: Nullable::Null,
            windSpeedMps: Nullable::Null,
            wetBulbGlobeTemperatureC: Nullable::Null,
        },
        // retain one nullable forecast product clock in the identity set
        productRunAt: if index == 1 {
            Nullable::Value(PRODUCT_RUN_AT.to_owned())
        } else {
            Nullable::Null
        },
        receivedAt: "2026-11-23T10:15:00.000Z".to_owned(),
        sourceId: SOURCE_ID.to_owned(),
        sourceKind: SourceKind::Forecast,
        validAt: VALID_AT[index].to_owned(),
    }
}

/// build prepared rows with shared-adapter hash evidence.
fn prepared_records() -> Vec<PreparedWeatherRecord> {
    CONTENT_HASHES
        .iter()
        .enumerate()
        .map(|(index, content_hash)| PreparedWeatherRecord {
            record: record(index),
            storage: WeatherRecordStorageEvidence {
                content_hash: Sha256Digest::admitted(*content_hash),
                provider_metadata_json: None,
                quality_metadata_json: None,
            },
        })
        .collect()
}

/// reproduce one decoded database row from a lookup request.
fn existing_from_request(
    request: &WeatherRevisionLookupRequest,
) -> ExistingWeatherRevisionIdentity {
    ExistingWeatherRevisionIdentity {
        content_hash: request.content_hash.clone(),
        product_run_at: request.product_run_at.clone(),
        source_kind: request.source_kind,
        valid_at: request.valid_at.clone(),
    }
}

/// execute one Rust helper mode and render the Node semantic signature.
fn execute_mode(mode: &'static str) -> String {
    let mut port = FixturePort::new(mode);
    // omit lookup work only for the absent-archiver fixture
    let selection = if mode == "disabled" {
        WeatherRevisionArchiveSelection::Disabled
    } else {
        WeatherRevisionArchiveSelection::Enabled
    };
    let changed =
        match select_changed_weather_revision_inputs(selection, &prepared_records(), &mut port) {
            Ok(changed) => changed,
            Err(error) => return rendered_result(Some(error.to_string()), &port.trace),
        };
    let staged = match stage_changed_weather_revisions(RUN_ID, &changed, &mut port) {
        Ok(staged) => staged,
        Err(error) => return rendered_result(Some(error.to_string()), &port.trace),
    };
    port.trace.push("commit".to_owned());
    // gate publication on the owning caller's commit result
    let commit_result = if mode == "commit-failure" {
        Err(FixtureError("commit failed"))
    } else {
        Ok(())
    };
    let committed = match staged.after_caller_commit(commit_result) {
        Ok(committed) => committed,
        Err(error) => return rendered_result(Some(error.to_string()), &port.trace),
    };
    publish_committed_weather_revisions(committed, &mut port);
    rendered_result(None, &port.trace)
}

/// render one archive-only result signature.
fn rendered_result(error: Option<String>, trace: &[String]) -> String {
    format!(
        "error={};trace={}",
        error.unwrap_or_else(|| "null".to_owned()),
        trace.join("|")
    )
}

// verifies compiled Node normal, recovery, commit, and publication parity
#[test]
fn matches_compiled_node_revision_archive_paths() {
    let oracle =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/revision-archive-node-oracle.mjs");
    let output = Command::new("node")
        .arg(oracle)
        .output()
        .expect("run revision archive Node oracle");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let node = String::from_utf8(output.stdout).expect("Node oracle UTF-8");
    let modes = [
        "disabled",
        "unchanged",
        "success",
        "stage-failure",
        "release-failure",
        "publish-failure",
        "commit-failure",
        "missing-context",
        "rollback-failure",
    ];
    let rust = modes
        .iter()
        .map(|mode| format!("{mode}={}", execute_mode(mode)))
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(rust, node.trim_end());
}

// verifies optional and empty selection avoid every adapter operation
#[test]
fn skips_disabled_and_empty_revision_selection() {
    let mut disabled = FixturePort::new("disabled");
    let changed = select_changed_weather_revision_inputs(
        WeatherRevisionArchiveSelection::Disabled,
        &prepared_records(),
        &mut disabled,
    )
    .expect("disabled selection");
    assert!(changed.is_empty());
    assert!(disabled.trace.is_empty());
    assert!(disabled.requests.is_empty());

    let mut empty = FixturePort::new("success");
    let changed = select_changed_weather_revision_inputs(
        WeatherRevisionArchiveSelection::Enabled,
        &[],
        &mut empty,
    )
    .expect("empty selection");
    assert!(changed.is_empty());
    assert!(empty.trace.is_empty());
    assert!(empty.requests.is_empty());
}

// verifies final duplicate prior rows replace earlier Map content like Node
#[test]
fn retains_final_existing_hash_for_duplicate_identity() {
    #[derive(Debug)]
    struct DuplicatePort(FixturePort);

    impl RetainedSourceSession for DuplicatePort {
        /// return the fixture source identity.
        fn source_id(&self) -> &str {
            self.0.source_id()
        }
    }

    impl WeatherRevisionArchivePort for DuplicatePort {
        type Error = FixtureError;
        type Publication = FixturePublication;

        /// delegate request serialization.
        fn serialize_revision_requests(
            &mut self,
            requests: &[WeatherRevisionLookupRequest],
        ) -> Result<SerializedJson, Self::Error> {
            self.0.serialize_revision_requests(requests)
        }

        /// return an old duplicate followed by the candidate's current hash.
        fn select_existing_weather_revisions(
            &mut self,
            _query: &WeatherRevisionSelectionQuery,
        ) -> Result<Vec<ExistingWeatherRevisionIdentity>, Self::Error> {
            let mut old = existing_from_request(&self.0.requests[0]);
            old.content_hash = Sha256Digest::admitted("0".repeat(64));
            let current = existing_from_request(&self.0.requests[0]);
            Ok(vec![old, current])
        }

        /// reject unused context lookup.
        fn load_weather_revision_source_context(
            &mut self,
            _query: &WeatherRevisionContextQuery,
        ) -> Result<Option<WeatherRevisionSourceContext>, Self::Error> {
            panic!("context must not load")
        }

        /// reject unused savepoint execution.
        fn execute_weather_revision_savepoint(
            &mut self,
            _command: WeatherRevisionSavepointCommand,
        ) -> Result<(), Self::Error> {
            panic!("savepoint must not execute")
        }

        /// reject unused staging.
        fn stage_weather_revisions(
            &mut self,
            _revisions: &[PersistedWeatherAdjustmentRevision],
        ) -> Result<Option<Self::Publication>, Self::Error> {
            panic!("stage must not execute")
        }

        /// reject unused publication.
        fn publish_weather_revisions(
            &mut self,
            _publication: Self::Publication,
        ) -> Result<(), Self::Error> {
            panic!("publish must not execute")
        }
    }

    let mut port = DuplicatePort(FixturePort::new("success"));
    let records = vec![prepared_records().remove(0)];
    let changed = select_changed_weather_revision_inputs(
        WeatherRevisionArchiveSelection::Enabled,
        &records,
        &mut port,
    )
    .expect("duplicate identity selection");
    assert!(changed.is_empty());
}

// verifies the public error retains exact source missing-row wording
#[test]
fn preserves_missing_context_error_text() {
    let error: WeatherRevisionArchiveError<FixtureError> =
        WeatherRevisionArchiveError::MissingSourceContext;
    assert_eq!(
        error.to_string(),
        "weather revision source context returned no row"
    );
}
