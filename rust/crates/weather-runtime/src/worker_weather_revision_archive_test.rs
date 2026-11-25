use std::cell::RefCell;
use std::rc::Rc;

use weather_api::{AdmittedAdjustmentRevisionCaptureWitness, CanonicalInstant};
use weather_contracts::{
    AdjustmentRevisionCaptureEpochWitness,
    AdjustmentRevisionCaptureEpochWitnessActivationKindLiteral,
    AdjustmentRevisionCaptureEpochWitnessArchiveCommitOrdinalLiteral,
    AdjustmentRevisionCaptureEpochWitnessContractVersionLiteral,
    AdjustmentRevisionCaptureEpochWitnessControlPlaneVersionEnum,
    AdjustmentRevisionCommitReceiptContractVersionLiteral,
    AdjustmentRevisionCommitReceiptProjectionKindEnum,
    AdjustmentRevisionStageReceiptContractVersionLiteral,
    AdjustmentRevisionStageReceiptDurableLiteral, AdjustmentShadowRegistrationSlot,
    CanonicalWeatherMetrics, JsonValue, NormalizedWeatherRecord, WeatherRecordMetadata,
};
use weather_database::{
    AdjustmentRevisionGapMarker, AdjustmentShadowRegistrationSlotQueryPlan,
    FinalizeScheduledRunQuery, MaintenancePublicationBind, PendingPhysicalWeatherRevisionReadPlan,
    PreparedWeatherRecord, WeatherRecordStorageEvidence, WorkerRevisionArchiveWritePlan,
    WorkerWeatherRevisionBatchReceipt, WorkerWeatherRevisionBoundReceipt,
};

use super::*;

const HASH_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const HASH_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const HASH_C: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

#[derive(Clone)]
struct Database {
    fail_commit: bool,
    trace: Rc<RefCell<Vec<String>>>,
}

impl RetainedSourceSession for Database {
    /// Return the one fixture source identity.
    fn source_id(&self) -> &str {
        "1"
    }
}

impl ScheduledIngestionCommandPort for Database {
    type Error = &'static str;

    /// Record transaction start.
    fn begin_transaction(&mut self) -> Result<(), Self::Error> {
        self.trace.borrow_mut().push("begin".to_owned());
        Ok(())
    }

    /// Admit the fixture running row.
    fn assert_scheduled_run(
        &mut self,
        _: &ScheduledRunIdentityQuery,
    ) -> Result<usize, Self::Error> {
        Ok(1)
    }

    /// Record the serving write before archive staging.
    fn upsert_weather_records(&mut self, _: &WeatherRecordUpsertQuery) -> Result<(), Self::Error> {
        self.trace.borrow_mut().push("upsert".to_owned());
        Ok(())
    }

    /// Admit the checkpoint after archive staging.
    fn advance_scheduled_checkpoint(
        &mut self,
        _: &ScheduledCheckpointQuery,
    ) -> Result<usize, Self::Error> {
        Ok(1)
    }

    /// Admit the final scheduled row.
    fn finalize_scheduled_run(
        &mut self,
        _: &FinalizeScheduledRunQuery,
    ) -> Result<usize, Self::Error> {
        Ok(1)
    }

    /// Commit or inject the requested failure.
    fn commit_transaction(&mut self) -> Result<(), Self::Error> {
        self.trace.borrow_mut().push("commit".to_owned());
        if self.fail_commit {
            Err("commit")
        } else {
            Ok(())
        }
    }

    /// Record transaction rollback.
    fn rollback_transaction(&mut self) -> Result<(), Self::Error> {
        self.trace.borrow_mut().push("rollback".to_owned());
        Ok(())
    }
}

impl WorkerWeatherRevisionSessionPort for Database {
    /// Encode one opaque lookup body.
    fn serialize_weather_revision_requests(
        &mut self,
        _: &[WeatherRevisionLookupRequest],
    ) -> Result<SerializedJson, Self::Error> {
        Ok(SerializedJson::admitted("[]"))
    }

    /// Return no matching predecessor rows.
    fn select_existing_weather_revision_rows(
        &mut self,
        _: &WeatherRevisionSelectionQuery,
    ) -> Result<Vec<ExistingWeatherRevisionIdentity>, Self::Error> {
        Ok(Vec::new())
    }

    /// Return the exact forecast source lineage.
    fn load_weather_revision_context(
        &mut self,
        _: &WeatherRevisionContextQuery,
    ) -> Result<Option<WeatherRevisionSourceContext>, Self::Error> {
        Ok(Some(WeatherRevisionSourceContext {
            adapter_version: "adapter/v1".to_owned(),
            logical_received_at: weather_database::AdmittedRevisionUtcInstant::admitted("t200"),
            provider_key: "open-meteo".to_owned(),
            source_config_fingerprint: HASH_A.to_owned(),
            source_key: "open-meteo-forecast-v4".to_owned(),
        }))
    }

    /// Record the outer transaction savepoint lifecycle.
    fn execute_weather_revision_archive_savepoint(
        &mut self,
        command: WeatherRevisionSavepointCommand,
    ) -> Result<(), Self::Error> {
        self.trace.borrow_mut().push(format!("outer:{command:?}"));
        Ok(())
    }

    /// Record the nested bind savepoint lifecycle.
    fn execute_weather_revision_bind_savepoint(
        &mut self,
        command: WorkerWeatherRevisionBindSavepointCommand,
    ) -> Result<(), Self::Error> {
        self.trace.borrow_mut().push(format!("bind:{command:?}"));
        Ok(())
    }

    /// Record a post-commit marker through the distinct pool-owned method.
    fn write_post_commit_weather_revision_gap(
        &mut self,
        key: &AdjustmentRevisionAdmissionKey,
        gap: &AdjustmentRevisionGapInput,
    ) -> Result<Vec<AdjustmentRevisionGapMarker>, Self::Error> {
        self.trace.borrow_mut().push("marker".to_owned());
        let AdjustmentRevisionAdmissionKey::WeatherRecord(key) = key else {
            panic!("weather key")
        };
        Ok(vec![AdjustmentRevisionGapMarker {
            logical_key_sha256: gap.logicalKeySha256.clone(),
            projection_identity_sha256: gap.projectionIdentitySha256.clone(),
            projection_kind: gap.projectionKind,
            projection_sha256: gap.projectionSha256.clone(),
            reason: gap.reason,
            stored_content_sha256: key.storedContentSha256.clone(),
        }])
    }
}

impl PendingPhysicalWeatherRevisionReadPort for Database {
    type Error = &'static str;

    /// Prohibit the physical query for the forecast fixture.
    fn read_pending_physical_weather_revisions(
        &mut self,
        _: PendingPhysicalWeatherRevisionReadPlan,
    ) -> Result<Vec<weather_database::PendingPhysicalWeatherRevisionRow>, Self::Error> {
        panic!("forecast must not read physical backlog")
    }
}

impl WorkerRevisionArchiveWritePort for Database {
    type Error = &'static str;

    /// Return one exact ordered receipt per forecast pointer.
    fn bind_weather_adjustment_revisions(
        &mut self,
        plan: WorkerRevisionArchiveWritePlan,
    ) -> Result<Vec<WorkerWeatherRevisionBatchReceipt>, Self::Error> {
        assert_eq!(
            plan.sql,
            weather_database::BIND_WEATHER_ADJUSTMENT_REVISIONS_SQL
        );
        self.trace.borrow_mut().push("bind:write".to_owned());
        let receipts = (1..=168)
            .map(|hour| WorkerWeatherRevisionBoundReceipt {
                projection_kind: AdjustmentRevisionCommitReceiptProjectionKindEnum::ActualBestMatch,
                revision_receipt: commit_receipt(),
                source_id: "1".to_owned(),
                stored_content_sha256: content_hash(hour),
                valid_at: format!("t{hour}"),
            })
            .collect();
        Ok(vec![WorkerWeatherRevisionBatchReceipt {
            contract_version: "adjustment-revision-batch-receipt/v1".to_owned(),
            receipts,
        }])
    }

    /// Reject the unrelated ECMWF binder.
    fn bind_ecmwf_temperature_revision(
        &mut self,
        _: WorkerRevisionArchiveWritePlan,
    ) -> Result<Vec<weather_contracts::AdjustmentRevisionRowReceipt>, Self::Error> {
        panic!("weather fixture must not bind ECMWF")
    }
}

impl AdjustmentMaintenanceDatabasePort for Database {
    type Error = &'static str;

    /// Reject unrelated registration reads.
    fn read_schedule_slot(
        &mut self,
        _: AdjustmentShadowRegistrationSlotQueryPlan,
    ) -> Result<Vec<AdjustmentShadowRegistrationSlot>, Self::Error> {
        panic!("weather fixture must not read schedule")
    }

    /// Reject unrelated shadow admission.
    fn read_shadow_body_admission(
        &mut self,
        _: weather_database::MaintenancePublicationPlan<'_, 4>,
    ) -> Result<Vec<bool>, Self::Error> {
        panic!("weather fixture must not read shadow body")
    }

    /// Reject unrelated shadow receipt reads.
    fn read_shadow_revision_receipt(
        &mut self,
        _: weather_database::MaintenancePublicationPlan<'_, 4>,
    ) -> Result<Vec<AdjustmentRevisionCommitReceipt>, Self::Error> {
        panic!("weather fixture must not read shadow receipt")
    }

    /// Reject unrelated current-pointer reads.
    fn read_current_revision_receipt(
        &mut self,
        _: weather_database::MaintenancePublicationPlan<'_, 1>,
    ) -> Result<Vec<AdjustmentRevisionCommitReceipt>, Self::Error> {
        panic!("weather fixture must not read current pointer")
    }

    /// Record and echo one exact post-commit permanent gap.
    fn write_revision_gap(
        &mut self,
        plan: weather_database::MaintenancePublicationPlan<'_, 3>,
    ) -> Result<Vec<AdjustmentRevisionGapMarker>, Self::Error> {
        let [
            MaintenancePublicationBind::Text("weather_record"),
            MaintenancePublicationBind::RevisionKeyJson(
                AdjustmentRevisionAdmissionKey::WeatherRecord(key),
            ),
            MaintenancePublicationBind::GapJson(gap),
        ] = plan.binds
        else {
            panic!("weather gap binds")
        };
        self.trace.borrow_mut().push("marker".to_owned());
        Ok(vec![AdjustmentRevisionGapMarker {
            logical_key_sha256: gap.logicalKeySha256.clone(),
            projection_identity_sha256: gap.projectionIdentitySha256.clone(),
            projection_kind: gap.projectionKind,
            projection_sha256: gap.projectionSha256.clone(),
            reason: gap.reason,
            stored_content_sha256: key.storedContentSha256.clone(),
        }])
    }
}

#[derive(Default)]
struct Values;

impl WorkerWeatherRevisionMaterialValues for Values {
    /// Parse one compact fixture clock.
    fn utc_epoch_milliseconds(&mut self, value: &str) -> Option<i64> {
        value.strip_prefix('t')?.parse::<i64>().ok()
    }

    /// Return one deterministic named-order body.
    fn encode_revision_batch_projection(
        &mut self,
        _: &weather_contracts::AdjustmentRevisionBatchProjection,
    ) -> Option<Vec<u8>> {
        Some(b"weather-body".to_vec())
    }

    /// Hash body bytes distinctly from lineage material.
    fn sha256(&mut self, bytes: &[u8]) -> Option<weather_database::Sha256Digest> {
        Some(weather_database::Sha256Digest::admitted(
            if bytes == b"weather-body" {
                HASH_B
            } else {
                HASH_A
            },
        ))
    }
}

impl ApiMaintenanceReceiptCanonicalValues for Values {
    /// Parse one compact fixture clock.
    fn canonical_utc_epoch_milliseconds(&self, value: &str) -> Option<i64> {
        value.strip_prefix('t')?.parse::<i64>().ok()
    }

    /// Hash body bytes distinctly from lineage material.
    fn sha256(&self, bytes: &[u8]) -> Option<weather_database::Sha256Digest> {
        Some(weather_database::Sha256Digest::admitted(
            if bytes == b"weather-body" {
                HASH_B
            } else {
                HASH_A
            },
        ))
    }

    /// Return the fixture stage or logical-key digest.
    fn unsigned_receipt_sha256(
        &self,
        _: &[(&str, MaintenanceReceiptScalar<'_>)],
    ) -> Option<weather_database::Sha256Digest> {
        Some(weather_database::Sha256Digest::admitted(HASH_C))
    }
}

impl WorkerRevisionArchiveWriteValues for Values {
    type Error = &'static str;

    /// Encode the pointer batch as one bounded opaque document.
    fn serialize_weather_revision_pointers(
        &mut self,
        _: &[AdjustmentWeatherRevisionPointer],
    ) -> Result<SerializedJson, Self::Error> {
        Ok(SerializedJson::admitted("[]"))
    }

    /// Reject unrelated ECMWF pointer encoding.
    fn serialize_ecmwf_temperature_revision_pointer(
        &mut self,
        _: &weather_contracts::AdjustmentEcmwfTemperatureRevisionPointer,
    ) -> Result<SerializedJson, Self::Error> {
        panic!("weather fixture must not serialize ECMWF")
    }

    /// Parse one compact fixture clock.
    fn utc_epoch_milliseconds(&mut self, value: &str) -> Result<i64, Self::Error> {
        value
            .strip_prefix('t')
            .ok_or("clock")?
            .parse()
            .map_err(|_| "clock")
    }
}

struct Client {
    fail_publish: bool,
    trace: Rc<RefCell<Vec<String>>>,
}

impl WorkerRevisionArchiveClient for Client {
    /// Return one durable receipt for the exact body identity.
    fn stage_revision(&mut self, _: &[u8]) -> Result<AdjustmentRevisionStageReceipt, TaskFailure> {
        self.trace.borrow_mut().push("stage".to_owned());
        Ok(AdjustmentRevisionStageReceipt {
            contractVersion: AdjustmentRevisionStageReceiptContractVersionLiteral,
            durable: AdjustmentRevisionStageReceiptDurableLiteral,
            durableAt: "t201".to_owned(),
            projectionIdentitySha256: HASH_B.to_owned(),
            projectionKind: AdjustmentRevisionProjectionKind::ActualBestMatch,
            projectionSha256: HASH_B.to_owned(),
            stageReceiptSha256: HASH_C.to_owned(),
        })
    }

    /// Reject the unrelated single-row publication.
    fn publish_revision(
        &mut self,
        _: &[u8],
        _: &AdjustmentRevisionStageReceipt,
        _: &AdjustmentRevisionCommitReceipt,
    ) -> Result<(), TaskFailure> {
        panic!("weather group must publish as a batch")
    }

    /// Record publication after commit and optionally inject failure.
    fn publish_revision_batch(
        &mut self,
        _: &[u8],
        _: &AdjustmentRevisionStageReceipt,
        _: &[AdjustmentRevisionCommitReceipt],
    ) -> Result<(), TaskFailure> {
        self.trace.borrow_mut().push("publish".to_owned());
        if self.fail_publish {
            Err(TaskFailure {
                attempts: 1,
                code: "publish".to_owned(),
            })
        } else {
            Ok(())
        }
    }

    /// Record the value-free archive disposition.
    fn record_revision_gap(&mut self, _: &AdjustmentRevisionGapInput) -> Result<(), TaskFailure> {
        self.trace.borrow_mut().push("gap".to_owned());
        Ok(())
    }
}

/// Build one server receipt bound to the body and stage identities.
fn commit_receipt() -> AdjustmentRevisionCommitReceipt {
    AdjustmentRevisionCommitReceipt {
        archiveCommitOrdinal: "1".to_owned(),
        archiveCommittedAt: "t202".to_owned(),
        contractVersion: AdjustmentRevisionCommitReceiptContractVersionLiteral,
        frontierSha256: HASH_A.to_owned(),
        predecessorFrontierSha256: HASH_A.to_owned(),
        projectionIdentitySha256: HASH_B.to_owned(),
        projectionKind: AdjustmentRevisionCommitReceiptProjectionKindEnum::ActualBestMatch,
        projectionSha256: HASH_B.to_owned(),
        receiptSha256: HASH_A.to_owned(),
        stageReceiptSha256: HASH_C.to_owned(),
    }
}

/// Build one complete 168-hour Best Match scheduled input.
fn input() -> CompleteScheduledIngestion {
    CompleteScheduledIngestion {
        attempts: 1,
        expected_checkpoint_version: None,
        last_valid_at: "t168".to_owned(),
        provider_cursor_json: None,
        records: (1..=168).map(prepared_record).collect(),
        response_metadata_json: None,
        run_id: "run".to_owned(),
        upstream_response_checksum: None,
        window_end_exclusive: "t169".to_owned(),
        window_start: "t0".to_owned(),
    }
}

/// Build one complete Best Match row.
fn prepared_record(hour: usize) -> PreparedWeatherRecord {
    let content_hash = content_hash(hour);
    PreparedWeatherRecord {
        record: NormalizedWeatherRecord {
            metadata: WeatherRecordMetadata {
                device: Nullable::Null,
                model: Nullable::Value("best_match".to_owned()),
                provider: Nullable::Value(vec![(
                    "dataset".encode_utf16().collect(),
                    JsonValue::String("best_match".encode_utf16().collect()),
                )]),
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
                temperatureC: Nullable::Value(10.0),
                uvIndex: Nullable::Null,
                waterLevelM: Nullable::Null,
                windDirectionDegrees: Nullable::Value(90.0),
                windGustMps: Nullable::Null,
                windSpeedMps: Nullable::Value(2.0),
                wetBulbGlobeTemperatureC: Nullable::Null,
            },
            productRunAt: Nullable::Value("t0".to_owned()),
            receivedAt: "t200".to_owned(),
            sourceId: "1".to_owned(),
            sourceKind: SourceKind::Forecast,
            validAt: format!("t{hour}"),
        },
        storage: WeatherRecordStorageEvidence {
            content_hash: weather_database::Sha256Digest::admitted(content_hash),
            provider_metadata_json: None,
            quality_metadata_json: None,
        },
    }
}

/// Build a stable lowercase hash unique to one hour.
fn content_hash(hour: usize) -> String {
    format!("{hour:064x}")
}

/// Build one startup-verified capture witness for the fixture epoch.
fn capture_witness() -> AdmittedAdjustmentRevisionCaptureWitness {
    AdmittedAdjustmentRevisionCaptureWitness::from_verified(
        AdjustmentRevisionCaptureEpochWitness {
            activationKind: AdjustmentRevisionCaptureEpochWitnessActivationKindLiteral,
            archiveCommitOrdinal: AdjustmentRevisionCaptureEpochWitnessArchiveCommitOrdinalLiteral,
            catalogFrontierSha256: HASH_A.to_owned(),
            contractVersion: AdjustmentRevisionCaptureEpochWitnessContractVersionLiteral,
            controlPlaneSha256: HASH_A.to_owned(),
            controlPlaneVersion:
                AdjustmentRevisionCaptureEpochWitnessControlPlaneVersionEnum::Value14,
            databaseMigrationHistorySha256: HASH_A.to_owned(),
            epochAt: "t0".to_owned(),
            servingSnapshotSha256: HASH_A.to_owned(),
            sourceCommit: String::new(),
            sourceRelease: String::new(),
            sourceServerImageDigest: String::new(),
            sourceWebImageDigest: String::new(),
            witnessSha256: HASH_A.to_owned(),
        },
        CanonicalInstant {
            epoch_milliseconds: 0,
            utc: "t0".to_owned(),
        },
    )
    .expect("matching witness")
}

#[test]
fn publishes_only_after_the_serving_commit() {
    let trace = Rc::new(RefCell::new(Vec::new()));
    let mut database = Database {
        fail_commit: false,
        trace: trace.clone(),
    };
    let mut values = Values;
    let mut client = Client {
        fail_publish: false,
        trace: trace.clone(),
    };

    complete_scheduled_ingestion_with_weather_revision_archive(
        &input(),
        &mut database,
        &capture_witness(),
        &mut values,
        &mut client,
    )
    .expect("archive completion");

    let trace = trace.borrow();
    let commit = trace
        .iter()
        .position(|event| event == "commit")
        .expect("commit");
    let publish = trace
        .iter()
        .position(|event| event == "publish")
        .expect("publish");
    assert!(commit < publish);
    assert!(
        trace
            .iter()
            .position(|event| event == "stage")
            .expect("stage")
            < commit
    );
}

#[test]
fn commit_failure_rolls_back_without_publication() {
    let trace = Rc::new(RefCell::new(Vec::new()));
    let mut database = Database {
        fail_commit: true,
        trace: trace.clone(),
    };
    let mut values = Values;
    let mut client = Client {
        fail_publish: false,
        trace: trace.clone(),
    };

    complete_scheduled_ingestion_with_weather_revision_archive(
        &input(),
        &mut database,
        &capture_witness(),
        &mut values,
        &mut client,
    )
    .expect_err("commit failure");

    let trace = trace.borrow();
    assert!(trace.iter().any(|event| event == "rollback"));
    assert!(!trace.iter().any(|event| event == "publish"));
}

#[test]
fn publish_failure_marks_rows_after_commit_and_records_one_group_gap() {
    let trace = Rc::new(RefCell::new(Vec::new()));
    let mut database = Database {
        fail_commit: false,
        trace: trace.clone(),
    };
    let mut values = Values;
    let mut client = Client {
        fail_publish: true,
        trace: trace.clone(),
    };

    complete_scheduled_ingestion_with_weather_revision_archive(
        &input(),
        &mut database,
        &capture_witness(),
        &mut values,
        &mut client,
    )
    .expect("serving success survives archive publication failure");

    let trace = trace.borrow();
    let commit = trace
        .iter()
        .position(|event| event == "commit")
        .expect("commit");
    let publish = trace
        .iter()
        .position(|event| event == "publish")
        .expect("publish");
    let marker = trace
        .iter()
        .position(|event| event == "marker")
        .expect("marker");
    let gap = trace.iter().position(|event| event == "gap").expect("gap");
    assert!(commit < publish && publish < marker && marker < gap);
    assert_eq!(trace.iter().filter(|event| *event == "marker").count(), 168);
    assert_eq!(trace.iter().filter(|event| *event == "gap").count(), 1);
}
