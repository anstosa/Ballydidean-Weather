use std::cell::RefCell;
use std::collections::BTreeSet;
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::rc::Rc;

use weather_contracts::{
    FieldPresence, IngestionError, IngestionErrorClassification, JsonValue,
    NormalizedWeatherRecord, Nullable, PublicStationAdapter, PublicStationConfiguration,
    PublicStationConfigurationStation, PublicStationSourceConfiguration, ScheduledCheckpointState,
    SourceCapability, SourceKind,
};
use weather_database::{
    AdmittedIngestionError, AmbientBackfillAbandonExpiredQuery, AmbientBackfillCommandPort,
    AmbientBackfillFailureQuery, AmbientBackfillFindRunningQuery, AmbientBackfillOutcome,
    AmbientBackfillOutcomeBind, AmbientBackfillOutcomeQuery, AmbientBackfillPoolPort,
    AmbientBackfillRunIdentityQuery, AmbientBackfillRunStartQuery, AmbientBackfillRunningLookup,
    AmbientBackfillSourceAssociationBind, AmbientBackfillSourceAssociationQuery,
    AmbientBackfillSourceAssociationRow, AmbientBackfillSourceReadPort,
    AmbientBackfillSuccessQuery, AmbientBackfillSuccessfulChunkQuery, RetainedSourceSession,
    SerializedJson, Sha256Digest, StartedAmbientBackfillRun, WeatherRecordUpsertQuery,
};
use weather_runtime::{
    AmbientBackfillPersistenceOperation, AmbientBackfillRuntimeValues, AmbientWeatherRuntimeValues,
    DatabaseAmbientBackfillRepository, OpenMeteoRuntimeValues,
    PublicStationBackfillPlanningRepository, RuntimePersistenceOperation, RuntimeTimeValue,
};
use weather_worker::{
    AMBIENT_WEATHER_ADAPTER_VERSION, AmbientWeatherBackfillProvider, AmbientWeatherBackfillSource,
    AmbientWeatherCapability, AmbientWeatherDeviceIdentity, AmbientWeatherObservation,
    AmbientWeatherPayload, AmbientWeatherProviderCursor, AmbientWeatherRangeRequest,
    AmbientWeatherResponseMetadata, AmbientWeatherRuntimeConfiguration,
    AmbientWeatherScheduledSource, AmbientWeatherSourceKind, CanonicalWeatherMetrics,
    NormalizedAmbientWeatherRecord, OpenMeteoCursor, ProviderBatch, PublicStationBackfillArguments,
    PublicStationBackfillChunkIdentity, PublicStationBackfillCompletion, PublicStationBackfillDate,
    PublicStationBackfillDeadlines, PublicStationBackfillErrorClassification,
    PublicStationBackfillOperationError, PublicStationBackfillPlanningSource,
    PublicStationBackfillProviderFailure, PublicStationBackfillTimeAdapter,
    PublicStationNormalizedSample, ScheduledSourceMaterialComparison,
    SupplementalPublicStationBackfillRepository, SupplementalPublicStationRecord,
    SupplementalPublicStationResponseMetadata, TaskFailure, execute_ambient_weather_backfill,
    normalize_ambient_weather_range,
};

const SOURCE_ID: &str = "source-ambient";
const SOURCE_KEY: &str = "ambient-merlin-observations-v1";
const DEVICE_ID: &str = "5fe7d16a25083b000171f8e5";
const FINGERPRINT: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const CHECKSUM: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const CONTENT_HASH: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const START_MS: i64 = 1_788_220_800_000;
const END_MS: i64 = 1_788_307_200_000;
const TODAY_MS: i64 = 1_788_393_600_000;
const VALID_AT_MS: i64 = START_MS + 360_000;
const RECEIVED_MS: i64 = 1_788_397_200_000;
const PROVIDER_DEADLINE_MS: i64 = 1_788_397_290_000;
const RUN_DEADLINE_MS: i64 = 1_788_397_320_000;
type Trace = Rc<RefCell<Vec<String>>>;

#[derive(Clone, Debug, Eq, PartialEq)]
struct FixtureError(&'static str);

impl Display for FixtureError {
    /// Render one injected database failure.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.0)
    }
}

impl Error for FixtureError {}

struct FixtureDatabase {
    trace: Trace,
}

struct PlanningFixtureDatabase {
    expected_source_keys: Vec<String>,
    rows: Vec<AmbientBackfillSourceAssociationRow>,
}

impl AmbientBackfillSourceReadPort for PlanningFixtureDatabase {
    type Error = FixtureError;

    /// Return every adapter row through the original exact association query.
    fn read_ambient_backfill_sources(
        &mut self,
        query: &AmbientBackfillSourceAssociationQuery,
    ) -> Result<Vec<AmbientBackfillSourceAssociationRow>, Self::Error> {
        assert_eq!(query.binds.len(), 2);
        assert_eq!(
            query.binds,
            [
                AmbientBackfillSourceAssociationBind::Text("ballydidean".to_owned()),
                AmbientBackfillSourceAssociationBind::TextArray(self.expected_source_keys.clone()),
            ]
        );
        assert!(query.sql.contains("s.active"));
        assert!(query.sql.contains("st.active"));
        assert!(query.sql.contains("si.active"));
        assert!(query.sql.contains("p.active"));
        Ok(self.rows.clone())
    }
}

impl AmbientBackfillPoolPort for PlanningFixtureDatabase {
    type Error = FixtureError;
    type Session = FixtureSession;

    /// Reject chunk receipt reads during source-only planning association.
    fn has_successful_chunk(
        &mut self,
        _query: &AmbientBackfillSuccessfulChunkQuery,
    ) -> Result<usize, Self::Error> {
        unreachable!("planning association does not read chunk receipts")
    }

    /// Reject source locking during source-only planning association.
    fn acquire_source_session(
        &mut self,
        _source_id: &str,
    ) -> Result<Option<Self::Session>, Self::Error> {
        unreachable!("planning association does not acquire source sessions")
    }
}

impl AmbientBackfillSourceReadPort for FixtureDatabase {
    type Error = FixtureError;

    /// Return the active row selected by the real two-bind startup query.
    fn read_ambient_backfill_sources(
        &mut self,
        query: &AmbientBackfillSourceAssociationQuery,
    ) -> Result<Vec<AmbientBackfillSourceAssociationRow>, Self::Error> {
        assert_eq!(query.binds.len(), 2);
        assert!(query.sql.contains("s.source_key = ANY($2::text[])"));
        self.trace.borrow_mut().push("associate:2".to_owned());
        Ok(vec![AmbientBackfillSourceAssociationRow {
            material_provider_config: material(),
            provider_key: "ambient-weather-network".to_owned(),
            site_slug: "ballydidean".to_owned(),
            source_config_fingerprint: FINGERPRINT.to_owned(),
            source_id: SOURCE_ID.to_owned(),
            source_key: SOURCE_KEY.to_owned(),
            source_kind: SourceKind::PhysicalSensor,
            station_slug: "ambient-merlin".to_owned(),
            timezone: "America/Los_Angeles".to_owned(),
        }])
    }
}

impl AmbientBackfillPoolPort for FixtureDatabase {
    type Error = FixtureError;
    type Session = FixtureSession;

    /// Execute the exact successful-chunk receipt lookup.
    fn has_successful_chunk(
        &mut self,
        query: &AmbientBackfillSuccessfulChunkQuery,
    ) -> Result<usize, Self::Error> {
        assert_eq!(query.binds.len(), 6);
        self.trace.borrow_mut().push("resume:6".to_owned());
        Ok(0)
    }

    /// Acquire the exact source-scoped retained session.
    fn acquire_source_session(
        &mut self,
        source_id: &str,
    ) -> Result<Option<Self::Session>, Self::Error> {
        assert_eq!(source_id, SOURCE_ID);
        self.trace.borrow_mut().push("acquire".to_owned());
        Ok(Some(FixtureSession {
            source_id: source_id.to_owned(),
            trace: Rc::clone(&self.trace),
        }))
    }
}

struct FixtureSession {
    source_id: String,
    trace: Trace,
}

impl RetainedSourceSession for FixtureSession {
    /// Return the immutable retained source identity.
    fn source_id(&self) -> &str {
        &self.source_id
    }
}

impl AmbientBackfillCommandPort for FixtureSession {
    type Error = FixtureError;

    /// Capture exact expired-run recovery.
    fn abandon_expired_runs(
        &mut self,
        query: &AmbientBackfillAbandonExpiredQuery,
    ) -> Result<Vec<String>, Self::Error> {
        assert_eq!(query.binds.len(), 2);
        self.trace.borrow_mut().push("abandon:2".to_owned());
        Ok(Vec::new())
    }

    /// Capture the running-row guard before insertion.
    fn find_running_run(
        &mut self,
        query: &AmbientBackfillFindRunningQuery,
    ) -> Result<AmbientBackfillRunningLookup, Self::Error> {
        assert_eq!(query.binds.len(), 1);
        self.trace.borrow_mut().push("find:1".to_owned());
        Ok(AmbientBackfillRunningLookup {
            first_run_id: None,
            row_count: 0,
        })
    }

    /// Capture the exact ten-bind running receipt.
    fn insert_running_run(
        &mut self,
        query: &AmbientBackfillRunStartQuery,
    ) -> Result<Option<StartedAmbientBackfillRun>, Self::Error> {
        assert_eq!(query.binds.len(), 10);
        self.trace.borrow_mut().push("start:10".to_owned());
        Ok(Some(StartedAmbientBackfillRun {
            id: "run-ambient".to_owned(),
            started_at: "2026-09-03T01:00:00.000Z".to_owned(),
        }))
    }

    /// Capture transaction start.
    fn begin_transaction(&mut self) -> Result<(), Self::Error> {
        self.trace.borrow_mut().push("begin".to_owned());
        Ok(())
    }

    /// Capture the committed run identity guard.
    fn assert_backfill_run(
        &mut self,
        query: &AmbientBackfillRunIdentityQuery,
    ) -> Result<usize, Self::Error> {
        assert_eq!(query.binds.len(), 7);
        self.trace.borrow_mut().push("assert:7".to_owned());
        Ok(1)
    }

    /// Capture one actual normalized Ambient storage row.
    fn upsert_weather_records(
        &mut self,
        query: &WeatherRecordUpsertQuery,
    ) -> Result<(), Self::Error> {
        assert_eq!(query.record_count, 1);
        self.trace.borrow_mut().push("records:1".to_owned());
        Ok(())
    }

    /// Capture the durable successful or failed chunk receipt.
    fn upsert_backfill_outcome(
        &mut self,
        query: &AmbientBackfillOutcomeQuery,
    ) -> Result<(), Self::Error> {
        // retain exact outcome evidence in the ordering trace
        let outcome = match query.binds[9] {
            AmbientBackfillOutcomeBind::Outcome(AmbientBackfillOutcome::Succeeded) => "succeeded",
            AmbientBackfillOutcomeBind::Outcome(AmbientBackfillOutcome::Failed) => "failed",
            _ => panic!("unexpected outcome bind"),
        };
        self.trace.borrow_mut().push(format!("outcome:{outcome}"));
        Ok(())
    }

    /// Capture the final successful receipt.
    fn finalize_successful_run(
        &mut self,
        query: &AmbientBackfillSuccessQuery,
    ) -> Result<usize, Self::Error> {
        assert_eq!(query.binds.len(), 7);
        self.trace.borrow_mut().push("finalize:success".to_owned());
        Ok(1)
    }

    /// Reject the unused failure path in this normal fixture.
    fn finalize_failed_run(
        &mut self,
        _query: &AmbientBackfillFailureQuery,
    ) -> Result<usize, Self::Error> {
        unreachable!("normal fixture succeeds")
    }

    /// Capture transaction commit.
    fn commit_transaction(&mut self) -> Result<(), Self::Error> {
        self.trace.borrow_mut().push("commit".to_owned());
        Ok(())
    }

    /// Reject rollback in the normal fixture.
    fn rollback_transaction(&mut self) -> Result<(), Self::Error> {
        self.trace.borrow_mut().push("rollback".to_owned());
        Ok(())
    }

    /// Release the retained source lock last.
    fn release(self) -> Result<(), Self::Error> {
        self.trace.borrow_mut().push("release".to_owned());
        Ok(())
    }
}

struct FixtureValues;

impl OpenMeteoRuntimeValues for FixtureValues {
    /// Resolve only the admitted fixture instants.
    fn utc_instant(
        &mut self,
        milliseconds: i64,
        _value: RuntimeTimeValue,
    ) -> Result<String, TaskFailure> {
        // map every fixture epoch through the admitted time owner
        let instant = match milliseconds {
            START_MS => "2026-09-01T00:00:00.000Z",
            END_MS => "2026-09-02T00:00:00.000Z",
            VALID_AT_MS => "2026-09-01T00:06:00.000Z",
            RECEIVED_MS => "2026-09-03T01:00:00.000Z",
            RUN_DEADLINE_MS => "2026-09-03T01:02:00.000Z",
            _ => panic!("unexpected Ambient time {milliseconds}"),
        };
        Ok(instant.to_owned())
    }

    /// Reject scheduled checkpoint decoding in chunk-identity backfill.
    fn epoch_milliseconds(
        &mut self,
        _instant: &str,
        _value: RuntimeTimeValue,
    ) -> Result<i64, TaskFailure> {
        unreachable!("Ambient backfill does not decode scheduled checkpoints")
    }

    /// Serialize admitted metadata without implementing JSON in Rust.
    fn serialize_json_object(
        &mut self,
        value: &[(Vec<u16>, JsonValue)],
    ) -> Result<SerializedJson, TaskFailure> {
        assert!(!value.is_empty());
        Ok(SerializedJson::admitted("admitted-ambient-json"))
    }

    /// Reject scheduled request metadata.
    fn request_metadata_json(&mut self, _endpoint: &str) -> Result<SerializedJson, TaskFailure> {
        unreachable!("Ambient backfill owns distinct request metadata")
    }

    /// Reject scheduled Open-Meteo cursor serialization.
    fn provider_cursor_json(
        &mut self,
        _cursor: Option<&OpenMeteoCursor>,
    ) -> Result<Option<SerializedJson>, TaskFailure> {
        unreachable!("Ambient backfill resume is receipt based")
    }

    /// Reject scheduled Open-Meteo cursor decoding.
    fn checkpoint_cursor(
        &mut self,
        _checkpoint: &ScheduledCheckpointState,
    ) -> Result<Option<OpenMeteoCursor>, TaskFailure> {
        unreachable!("Ambient backfill resume is receipt based")
    }

    /// Admit the projected physical-sensor row hash.
    fn weather_record_content_hash(
        &mut self,
        record: &NormalizedWeatherRecord,
    ) -> Result<Sha256Digest, TaskFailure> {
        assert_eq!(record.sourceId, SOURCE_ID);
        assert_eq!(record.validAt, "2026-09-01T00:06:00.000Z");
        Ok(Sha256Digest::admitted(CONTENT_HASH))
    }

    /// Admit fixture fingerprints and upstream checksums.
    fn sha256_digest(&mut self, value: &str) -> Result<Sha256Digest, TaskFailure> {
        assert!(value == FINGERPRINT || value == CHECKSUM);
        Ok(Sha256Digest::admitted(value))
    }

    /// Reject the unrelated scheduled failure path.
    fn ingestion_error(
        &mut self,
        _failure: &TaskFailure,
    ) -> Result<AdmittedIngestionError, TaskFailure> {
        unreachable!("Ambient backfill has a classified failure boundary")
    }

    /// Retain the existing shared persistence classifier contract.
    fn persistence_failure(
        &mut self,
        operation: RuntimePersistenceOperation,
        _message: &str,
    ) -> TaskFailure {
        TaskFailure {
            attempts: 0,
            code: format!("persistence_{operation:?}"),
        }
    }
}

impl AmbientWeatherRuntimeValues for FixtureValues {
    /// Reject scheduled Ambient cursor serialization.
    fn ambient_weather_provider_cursor_json(
        &mut self,
        _cursor: Option<&AmbientWeatherProviderCursor>,
    ) -> Result<Option<SerializedJson>, TaskFailure> {
        unreachable!("Ambient backfill resume is receipt based")
    }

    /// Reject scheduled Ambient cursor decoding.
    fn ambient_weather_checkpoint_cursor(
        &mut self,
        _checkpoint: &ScheduledCheckpointState,
    ) -> Result<Option<AmbientWeatherProviderCursor>, TaskFailure> {
        unreachable!("Ambient backfill resume is receipt based")
    }
}

impl AmbientBackfillRuntimeValues for FixtureValues {
    /// Admit only the fixture's configured archive dates.
    fn ambient_backfill_date(
        &mut self,
        value: &str,
    ) -> Result<PublicStationBackfillDate, TaskFailure> {
        // bind each label to an externally resolved UTC midnight
        match value {
            "2026-09-01" => Ok(PublicStationBackfillDate::admitted(value, START_MS)),
            _ => panic!("unexpected Ambient date {value}"),
        }
    }

    /// Serialize the exact Node endpoint and station evidence.
    fn ambient_backfill_request_metadata_json(
        &mut self,
        endpoint: &str,
        station_key: &str,
    ) -> Result<Option<SerializedJson>, TaskFailure> {
        assert_eq!(endpoint, "ambient-weather");
        assert_eq!(station_key, "ambient-merlin");
        Ok(Some(SerializedJson::admitted(
            r#"{"endpoint":"ambient-weather","station":"ambient-merlin"}"#,
        )))
    }

    /// Admit an exact classified provider failure.
    fn ambient_backfill_ingestion_error(
        &mut self,
        failure: &PublicStationBackfillProviderFailure,
    ) -> Result<AdmittedIngestionError, TaskFailure> {
        // project the typed worker classification without reinterpreting it
        let classification = match failure.error.classification {
            PublicStationBackfillErrorClassification::Retryable => {
                IngestionErrorClassification::Retryable
            }
            PublicStationBackfillErrorClassification::RateLimited => {
                IngestionErrorClassification::RateLimited
            }
            PublicStationBackfillErrorClassification::Permanent => {
                IngestionErrorClassification::Permanent
            }
            PublicStationBackfillErrorClassification::InvalidPayload => {
                IngestionErrorClassification::InvalidPayload
            }
        };
        Ok(AdmittedIngestionError::admitted(IngestionError {
            classification,
            code: failure.error.code.clone(),
            message: failure.error.message.clone(),
            metadata: FieldPresence::Missing,
        }))
    }

    /// Classify one task-level Ambient database failure.
    fn ambient_backfill_task_failure(
        &mut self,
        operation: AmbientBackfillPersistenceOperation,
        _message: &str,
    ) -> TaskFailure {
        TaskFailure {
            attempts: 0,
            code: format!("ambient_{operation:?}"),
        }
    }

    /// Classify one committed Ambient database failure.
    fn ambient_backfill_operation_failure(
        &mut self,
        operation: AmbientBackfillPersistenceOperation,
        message: &str,
    ) -> PublicStationBackfillOperationError {
        PublicStationBackfillOperationError::unexpected(format!("{operation:?}: {message}"))
    }
}

impl ScheduledSourceMaterialComparison for FixtureValues {
    type Error = FixtureError;

    /// Compare decoded fixture values with object-key order ignored.
    fn values_equal(&mut self, left: &JsonValue, right: &JsonValue) -> Result<bool, Self::Error> {
        Ok(canonical_fixture_json_equal(left, right))
    }

    /// Compare one stored value with generated object entries canonically.
    fn value_and_object_equal(
        &mut self,
        value: &JsonValue,
        object: &[(Vec<u16>, JsonValue)],
    ) -> Result<bool, Self::Error> {
        Ok(canonical_fixture_json_equal(
            value,
            &JsonValue::Object(object.to_vec()),
        ))
    }
}

struct FixtureProvider;

impl AmbientWeatherBackfillProvider for FixtureProvider {
    /// Return one actually normalized in-window Ambient record.
    fn fetch(
        &mut self,
        request: AmbientWeatherRangeRequest,
        deadline_at_ms: i64,
    ) -> Result<
        ProviderBatch<
            NormalizedAmbientWeatherRecord,
            AmbientWeatherResponseMetadata,
            Option<AmbientWeatherProviderCursor>,
        >,
        PublicStationBackfillOperationError,
    > {
        assert_eq!(deadline_at_ms, PROVIDER_DEADLINE_MS);
        let payload = AmbientWeatherPayload {
            observations: vec![AmbientWeatherObservation {
                apparent_temperature_f: Some(54.0),
                date_utc_ms: VALID_AT_MS,
                device_identity: AmbientWeatherDeviceIdentity::Value(DEVICE_ID.to_owned()),
                hourly_rain_in: Some(0.1),
                pressure_in_hg: Some(30.0),
                relative_humidity_percent: Some(90.0),
                solar_radiation_wm2: Some(100.0),
                temperature_f: Some(53.6),
                uv_index: Some(1.0),
                wind_direction_degrees: Some(360.0),
                wind_gust_mph: Some(10.0),
                wind_speed_mph: Some(5.0),
            }],
        };
        let normalized = normalize_ambient_weather_range(&payload, &request, RECEIVED_MS, 2)
            .map_err(|failure| PublicStationBackfillOperationError::unexpected(failure.code))?;
        Ok(ProviderBatch {
            attempts: normalized.attempts,
            provider_cursor: normalized.provider_cursor,
            records: normalized.records,
            response_metadata: normalized.response_metadata,
            upstream_response_checksum: CHECKSUM.to_owned(),
        })
    }
}

struct FixtureTime;

impl PublicStationBackfillTimeAdapter for FixtureTime {
    /// Return the first UTC day after the archive fixture.
    fn today_utc(&mut self) -> Result<PublicStationBackfillDate, TaskFailure> {
        Ok(PublicStationBackfillDate::admitted("2026-09-03", TODAY_MS))
    }

    /// Add exact fixture UTC days through the admitted calendar owner.
    fn add_utc_days(
        &mut self,
        date: &PublicStationBackfillDate,
        days: i64,
    ) -> Result<PublicStationBackfillDate, TaskFailure> {
        let milliseconds = date.start_ms() + days * 86_400_000;
        // map every reachable fixture calendar date explicitly
        let label = match milliseconds {
            START_MS => "2026-09-01",
            END_MS => "2026-09-02",
            TODAY_MS => "2026-09-03",
            1_788_739_200_000 => "2026-09-07",
            _ => panic!("unexpected Ambient calendar {milliseconds}"),
        };
        Ok(PublicStationBackfillDate::admitted(label, milliseconds))
    }

    /// Return the admitted provider and durable-run deadlines.
    fn deadlines(
        &mut self,
    ) -> Result<PublicStationBackfillDeadlines, PublicStationBackfillOperationError> {
        Ok(PublicStationBackfillDeadlines {
            provider_deadline_at_ms: PROVIDER_DEADLINE_MS,
            run_deadline_at_ms: RUN_DEADLINE_MS,
            started_at_ms: RECEIVED_MS,
        })
    }
}

/// Build the exact checked Ambient material object.
fn material() -> JsonValue {
    JsonValue::Object(vec![
        (
            utf16("contractVersion"),
            JsonValue::String(utf16("ambient-device-data/v1")),
        ),
        (utf16("deviceId"), JsonValue::String(utf16(DEVICE_ID))),
        (
            utf16("macAddress"),
            JsonValue::String(utf16("F0:08:D1:07:B3:9E")),
        ),
        (
            utf16("sample"),
            JsonValue::String(utf16("every-distinct-provider-observation")),
        ),
    ])
}

/// Compare fixture JSON recursively while treating object keys as unordered.
fn canonical_fixture_json_equal(left: &JsonValue, right: &JsonValue) -> bool {
    // preserve scalar and array semantics while canonicalizing object key order
    match (left, right) {
        (JsonValue::Boolean(left), JsonValue::Boolean(right)) => left == right,
        (JsonValue::Number(left), JsonValue::Number(right)) => left == right,
        (JsonValue::String(left), JsonValue::String(right)) => left == right,
        (JsonValue::Null, JsonValue::Null) => true,
        (JsonValue::Array(left), JsonValue::Array(right)) => {
            left.len() == right.len()
                && left
                    .iter()
                    .zip(right)
                    .all(|(left, right)| canonical_fixture_json_equal(left, right))
        }
        (JsonValue::Object(left), JsonValue::Object(right)) => {
            left.len() == right.len()
                && left.iter().all(|(left_key, left_value)| {
                    right.iter().any(|(right_key, right_value)| {
                        left_key == right_key
                            && canonical_fixture_json_equal(left_value, right_value)
                    })
                })
        }
        _ => false,
    }
}

/// Return the stable fixture key for one public-station adapter.
fn planning_source_key(adapter: PublicStationAdapter) -> &'static str {
    // keep keys distinct across every supported adapter
    match adapter {
        PublicStationAdapter::AmbientWeather => "ambient-archive",
        PublicStationAdapter::WeatherUnderground => "wunderground-archive",
        PublicStationAdapter::Netatmo => "netatmo-archive",
        PublicStationAdapter::Purpleair => "purpleair-archive",
    }
}

/// Return the stable fixture provider for one public-station adapter.
fn planning_provider_key(adapter: PublicStationAdapter) -> &'static str {
    // mirror the configured provider identity stored in the database row
    match adapter {
        PublicStationAdapter::AmbientWeather => "ambient-provider",
        PublicStationAdapter::WeatherUnderground => "wunderground-provider",
        PublicStationAdapter::Netatmo => "netatmo-provider",
        PublicStationAdapter::Purpleair => "purpleair-provider",
    }
}

/// Return one distinct valid source fingerprint per adapter.
fn planning_fingerprint(adapter: PublicStationAdapter) -> String {
    // expose identity drift between otherwise similar adapter fixtures
    let byte = match adapter {
        PublicStationAdapter::AmbientWeather => 'a',
        PublicStationAdapter::WeatherUnderground => 'b',
        PublicStationAdapter::Netatmo => 'c',
        PublicStationAdapter::Purpleair => 'd',
    };
    byte.to_string().repeat(64)
}

/// Build adapter material whose stored key order can differ canonically.
fn planning_material(adapter: PublicStationAdapter, stored: bool) -> JsonValue {
    let contract = (
        utf16("contractVersion"),
        JsonValue::String(utf16(planning_source_key(adapter))),
    );
    let identity = (
        utf16("identity"),
        JsonValue::String(utf16(planning_provider_key(adapter))),
    );

    // reverse stored object keys to require canonical rather than raw equality
    if stored {
        JsonValue::Object(vec![identity, contract])
    } else {
        JsonValue::Object(vec![contract, identity])
    }
}

/// Build one generated historical source for the provider-independent planner.
fn planning_source(adapter: PublicStationAdapter) -> PublicStationSourceConfiguration {
    PublicStationSourceConfiguration {
        active: true,
        adapter,
        adapterConfig: match planning_material(adapter, false) {
            JsonValue::Object(value) => value,
            _ => unreachable!("planning material is an object"),
        },
        cadenceSeconds: Nullable::Null,
        capabilities: vec![SourceCapability::Historical],
        fingerprint: planning_fingerprint(adapter),
        historyEndDate: Nullable::Null,
        historyStartDate: Nullable::Value("2026-09-01".to_owned()),
        key: planning_source_key(adapter).to_owned(),
        maximumChunkDays: match adapter {
            PublicStationAdapter::AmbientWeather => 1,
            PublicStationAdapter::WeatherUnderground => 2,
            PublicStationAdapter::Netatmo => 3,
            PublicStationAdapter::Purpleair => 4,
        },
        providerKey: planning_provider_key(adapter).to_owned(),
    }
}

/// Build one active database association row for a configured adapter.
fn planning_row(adapter: PublicStationAdapter) -> AmbientBackfillSourceAssociationRow {
    AmbientBackfillSourceAssociationRow {
        material_provider_config: planning_material(adapter, true),
        provider_key: planning_provider_key(adapter).to_owned(),
        site_slug: "ballydidean".to_owned(),
        source_config_fingerprint: planning_fingerprint(adapter),
        source_id: format!("source-{}", planning_source_key(adapter)),
        source_key: planning_source_key(adapter).to_owned(),
        source_kind: SourceKind::PhysicalSensor,
        station_slug: "all-public-stations".to_owned(),
        timezone: "America/Los_Angeles".to_owned(),
    }
}

/// Build a checked catalog containing every supported historical adapter.
fn planning_configuration() -> PublicStationConfiguration {
    let adapters = [
        PublicStationAdapter::AmbientWeather,
        PublicStationAdapter::WeatherUnderground,
        PublicStationAdapter::Netatmo,
        PublicStationAdapter::Purpleair,
    ];
    let mut sources = adapters
        .into_iter()
        .map(planning_source)
        .collect::<Vec<_>>();
    let mut inactive = planning_source(PublicStationAdapter::Purpleair);
    inactive.active = false;
    inactive.key = "inactive-purpleair".to_owned();
    let mut current_only = planning_source(PublicStationAdapter::Netatmo);
    current_only.capabilities = vec![SourceCapability::Current];
    current_only.key = "current-netatmo".to_owned();
    sources.push(inactive);
    sources.push(current_only);

    PublicStationConfiguration {
        siteKey: "ballydidean".to_owned(),
        providers: Vec::new(),
        stations: vec![PublicStationConfigurationStation {
            active: false,
            displayName: "All public stations".to_owned(),
            key: "all-public-stations".to_owned(),
            latitude: 47.955,
            longitude: -122.418,
            model: Nullable::Null,
            serial: Nullable::Null,
            sources,
            timezone: "America/Los_Angeles".to_owned(),
            vendor: Nullable::Null,
        }],
        version: 1,
    }
}

/// Build one generated checked public-station catalog entry.
fn configuration() -> PublicStationConfiguration {
    PublicStationConfiguration {
        siteKey: "ballydidean".to_owned(),
        providers: Vec::new(),
        stations: vec![PublicStationConfigurationStation {
            active: true,
            displayName: "French Rd & Lapis Ln".to_owned(),
            key: "ambient-merlin".to_owned(),
            latitude: 47.955,
            longitude: -122.418,
            model: Nullable::Value("AMBWeatherV4.3.6".to_owned()),
            serial: Nullable::Value(DEVICE_ID.to_owned()),
            sources: vec![PublicStationSourceConfiguration {
                active: true,
                adapter: PublicStationAdapter::AmbientWeather,
                adapterConfig: match material() {
                    JsonValue::Object(value) => value,
                    _ => unreachable!("fixture material is an object"),
                },
                cadenceSeconds: Nullable::Value(3_600),
                capabilities: vec![SourceCapability::Current, SourceCapability::Historical],
                fingerprint: FINGERPRINT.to_owned(),
                historyEndDate: Nullable::Null,
                historyStartDate: Nullable::Value("2026-09-01".to_owned()),
                key: SOURCE_KEY.to_owned(),
                maximumChunkDays: 7,
                providerKey: "ambient-weather-network".to_owned(),
            }],
            timezone: "America/Los_Angeles".to_owned(),
            vendor: Nullable::Value("Ambient Weather".to_owned()),
        }],
        version: 1,
    }
}

/// Bind one resolved Ambient identity to the checked execution configuration.
fn ambient_execution_source(
    planning: &PublicStationBackfillPlanningSource,
) -> AmbientWeatherBackfillSource {
    assert_eq!(planning.adapter, PublicStationAdapter::AmbientWeather);
    let capabilities = BTreeSet::from([
        AmbientWeatherCapability::Current,
        AmbientWeatherCapability::Historical,
    ]);
    AmbientWeatherBackfillSource {
        configuration: AmbientWeatherRuntimeConfiguration {
            adapter_contract_version: AMBIENT_WEATHER_ADAPTER_VERSION,
            capabilities,
            device_id: DEVICE_ID.to_owned(),
            mac_address: "F0:08:D1:07:B3:9E".to_owned(),
            material_config_identity: "ambient-material".to_owned(),
            maximum_chunk_days: 7,
            model: Some("AMBWeatherV4.3.6".to_owned()),
            provider_key: "ambient-weather-network".to_owned(),
            serial: Some(DEVICE_ID.to_owned()),
            site_slug: "ballydidean".to_owned(),
            source_active: true,
            source_config_fingerprint: planning.source_config_fingerprint.clone(),
            source_key: planning.source_key.clone(),
            source_kind: AmbientWeatherSourceKind::PhysicalSensor,
            station_slug: "ambient-merlin".to_owned(),
            timezone: "America/Los_Angeles".to_owned(),
        },
        history_end: planning.history_end.clone(),
        history_start: planning.history_start.clone(),
        provider_active: true,
        source: AmbientWeatherScheduledSource {
            active: true,
            cadence_seconds: 3_600,
            id: planning.source_id.clone(),
            material_config_identity: "ambient-material".to_owned(),
            provider_key: "ambient-weather-network".to_owned(),
            site_slug: "ballydidean".to_owned(),
            source_config_fingerprint: planning.source_config_fingerprint.clone(),
            source_key: planning.source_key.clone(),
            source_kind: AmbientWeatherSourceKind::PhysicalSensor,
            station_slug: "ambient-merlin".to_owned(),
            timezone: "America/Los_Angeles".to_owned(),
        },
        station_active: true,
        storage_active: true,
    }
}

/// Convert fixture strings to generated JavaScript UTF-16 storage.
fn utf16(value: &str) -> Vec<u16> {
    value.encode_utf16().collect()
}

/// Build one supplemental record admitted through the shared normalized storage projection.
fn supplemental_record() -> SupplementalPublicStationRecord {
    SupplementalPublicStationRecord {
        dataset: "wunderground-pws-history/v1",
        device_id: "KWASHING123".to_owned(),
        model: Some("PWS".to_owned()),
        report_interval_minutes: 5,
        sampling: "provider-observation",
        serial: None,
        source_id: SOURCE_ID.to_owned(),
        received_at_ms: RECEIVED_MS,
        sample: PublicStationNormalizedSample {
            valid_at_ms: VALID_AT_MS,
            metrics: CanonicalWeatherMetrics {
                apparent_temperature_c: None,
                black_globe_temperature_c: None,
                cloud_cover_percent: None,
                pm25_micrograms_per_cubic_meter: None,
                precipitation_mm: Some(0.2),
                precipitation_rate_mm_per_hour: None,
                pressure_hpa: Some(1_016.0),
                relative_humidity_percent: Some(82.0),
                soil_electrical_conductivity_microsiemens_per_cm: None,
                soil_moisture_percent: None,
                solar_radiation_wm2: None,
                temperature_c: Some(12.0),
                uv_index: None,
                water_level_m: None,
                wind_direction_degrees: Some(180.0),
                wind_gust_mps: None,
                wind_speed_mps: Some(1.5),
                wet_bulb_globe_temperature_c: None,
            },
            quality_status: Some("ok".to_owned()),
            quality_flags: Vec::new(),
        },
        timezone: "America/Los_Angeles".to_owned(),
        vendor: "Weather Underground",
    }
}

/// Build the exact durable identity retained by both public-station record families.
fn supplemental_identity() -> PublicStationBackfillChunkIdentity {
    PublicStationBackfillChunkIdentity {
        adapter_version: "wunderground-pws-history/v1",
        chunk_plan_version: "public-station-calendar-range/v1",
        interval_end_exclusive: PublicStationBackfillDate::admitted("2026-09-02", END_MS),
        interval_start: PublicStationBackfillDate::admitted("2026-09-01", START_MS),
        requested_from_date: "2026-09-01".to_owned(),
        requested_to_date: "2026-09-01".to_owned(),
        source_config_fingerprint: FINGERPRINT.to_owned(),
        source_id: SOURCE_ID.to_owned(),
    }
}

/// Prove provider-neutral association, Ambient persistence, and final receipt order.
#[test]
fn associates_and_persists_ambient_archive_chunk() {
    let trace = Rc::new(RefCell::new(Vec::new()));
    let database = FixtureDatabase {
        trace: Rc::clone(&trace),
    };
    let mut repository = DatabaseAmbientBackfillRepository::new(database, FixtureValues);
    let configuration = configuration();
    let planning = repository
        .resolve_public_station_planning_sources(&configuration, &[])
        .expect("associate active historical source");
    assert_eq!(planning.len(), 1);
    assert_eq!(planning[0].source_id, SOURCE_ID);
    assert_eq!(planning[0].history_start.label(), "2026-09-01");
    let sources = [ambient_execution_source(&planning[0])];

    let report = execute_ambient_weather_backfill(
        &PublicStationBackfillArguments {
            dry_run: false,
            from: None,
            resume: true,
            site: "ballydidean".to_owned(),
            source_keys: Vec::new(),
            to: PublicStationBackfillDate::admitted("2026-09-01", START_MS),
        },
        "ballydidean",
        &sources,
        &mut repository,
        &mut FixtureProvider,
        &mut FixtureTime,
    )
    .expect("execute Ambient archive");
    assert_eq!(report.exit_code, 0);
    assert_eq!(report.sources[0].records, 1);
    assert_eq!(
        trace.borrow().as_slice(),
        [
            "associate:2",
            "resume:6",
            "acquire",
            "abandon:2",
            "find:1",
            "start:10",
            "begin",
            "assert:7",
            "records:1",
            "outcome:succeeded",
            "finalize:success",
            "commit",
            "release",
        ]
    );
}

/// Prove supplemental records use the same exact atomic backfill transaction and empty batches.
#[test]
fn persists_supplemental_archive_records_and_empty_receipts_atomically() {
    // exercise both the actual-row path and successful zero-record receipt path
    for records in [vec![supplemental_record()], Vec::new()] {
        let record_count = records.len();
        let trace = Rc::new(RefCell::new(Vec::new()));
        let database = FixtureDatabase {
            trace: Rc::clone(&trace),
        };
        let mut repository = DatabaseAmbientBackfillRepository::new(database, FixtureValues);
        let mut session = FixtureSession {
            source_id: SOURCE_ID.to_owned(),
            trace: Rc::clone(&trace),
        };
        repository
            .complete_supplemental_backfill_ingestion(
                &mut session,
                PublicStationBackfillCompletion {
                    attempts: 2,
                    identity: supplemental_identity(),
                    records,
                    response_metadata: SupplementalPublicStationResponseMetadata {
                        measurement_request_count: None,
                        normalized_record_count: record_count,
                        provider: "weather-underground",
                        raw_observation_count: Some(record_count),
                        resolution_seconds: None,
                    },
                    run_id: "run-supplemental".to_owned(),
                    upstream_response_checksum: CHECKSUM.to_owned(),
                },
            )
            .expect("atomically persist supplemental archive completion");
        let trace = trace.borrow();
        assert_eq!(trace.first().map(String::as_str), Some("begin"));
        assert_eq!(trace.last().map(String::as_str), Some("commit"));
        assert!(trace.iter().any(|entry| entry == "outcome:succeeded"));
        assert!(trace.iter().any(|entry| entry == "finalize:success"));
        assert_eq!(
            trace.iter().any(|entry| entry == "records:1"),
            record_count == 1
        );
    }
}

/// Prove all adapters use the exact active join and retain checked catalog order.
#[test]
fn resolves_every_public_station_adapter_for_planning() {
    let adapters = [
        PublicStationAdapter::AmbientWeather,
        PublicStationAdapter::WeatherUnderground,
        PublicStationAdapter::Netatmo,
        PublicStationAdapter::Purpleair,
    ];
    let expected_source_keys = adapters
        .iter()
        .map(|adapter| planning_source_key(*adapter).to_owned())
        .collect::<Vec<_>>();
    let rows = adapters
        .iter()
        .rev()
        .map(|adapter| planning_row(*adapter))
        .collect::<Vec<_>>();
    let database = PlanningFixtureDatabase {
        expected_source_keys: expected_source_keys.clone(),
        rows,
    };
    let mut repository = DatabaseAmbientBackfillRepository::new(database, FixtureValues);

    let sources = repository
        .resolve_public_station_planning_sources(&planning_configuration(), &[])
        .expect("resolve every configured historical adapter");

    assert_eq!(sources.len(), adapters.len());
    assert_eq!(
        sources
            .iter()
            .map(|source| source.adapter)
            .collect::<Vec<_>>(),
        adapters
    );
    assert_eq!(
        sources
            .iter()
            .map(|source| source.source_key.clone())
            .collect::<Vec<_>>(),
        expected_source_keys
    );
    assert_eq!(
        sources
            .iter()
            .map(|source| source.maximum_chunk_days)
            .collect::<Vec<_>>(),
        [1, 2, 3, 4]
    );
    assert!(
        sources
            .iter()
            .all(|source| source.history_start.label() == "2026-09-01")
    );
}
