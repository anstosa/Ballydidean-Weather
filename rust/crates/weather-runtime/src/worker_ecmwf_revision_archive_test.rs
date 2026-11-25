use std::cell::RefCell;
use std::rc::Rc;

use weather_api::{CanonicalInstant, CurrentProjectionError};
use weather_contracts::{
    AdjustmentRevisionCaptureEpochWitness,
    AdjustmentRevisionCaptureEpochWitnessActivationKindLiteral,
    AdjustmentRevisionCaptureEpochWitnessArchiveCommitOrdinalLiteral,
    AdjustmentRevisionCaptureEpochWitnessContractVersionLiteral,
    AdjustmentRevisionCaptureEpochWitnessControlPlaneVersionEnum,
    AdjustmentRevisionCommitReceiptContractVersionLiteral,
    AdjustmentRevisionCommitReceiptProjectionKindEnum, AdjustmentRevisionRowReceipt,
    AdjustmentRevisionRowReceiptContractVersionLiteral,
    AdjustmentRevisionStageReceiptContractVersionLiteral,
    AdjustmentRevisionStageReceiptDurableLiteral, EcmwfTemperatureCanaryHour,
    EcmwfTemperatureCanaryRecentErrorState, EcmwfTemperatureCanaryRecentErrorStateCohortLiteral,
    EcmwfTemperatureCanaryRunAdapterVersionLiteral, EcmwfTemperatureCanaryRunModelCycleEnum,
    EcmwfTemperatureCanaryRunUpstreamModelLiteral, JsDateOrString, JsonValue, Nullable, SourceKind,
    WeatherRecordRow,
};
use weather_database::{
    MaintenancePublicationBind, SerializedJson, WorkerRevisionArchiveWriteBind,
    WorkerRevisionArchiveWritePlan, WorkerWeatherRevisionBatchReceipt,
};

use super::*;

const BASE: i64 = 1_759_881_600_000;
const HASH_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const HASH_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const HASH_C: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const HASH_S: &str = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";

#[derive(Default)]
struct Values;

impl ApiMaintenanceReceiptCanonicalValues for Values {
    /// Parse the canonical fixture day without implementing production time behavior.
    fn canonical_utc_epoch_milliseconds(&self, value: &str) -> Option<i64> {
        FixtureTime
            .resolve(&JsDateOrString::String(text(value)))
            .ok()
            .map(
                // retain the approved resolver's already canonical epoch
                |instant| instant.epoch_milliseconds,
            )
    }

    /// Return one fixed projection identity for the fixture bytes.
    fn sha256(&self, _: &[u8]) -> Option<weather_database::Sha256Digest> {
        Some(weather_database::Sha256Digest::admitted(HASH_A))
    }

    /// Return one fixed ordered-field identity used by keys and receipts.
    fn unsigned_receipt_sha256(
        &self,
        _: &[(&str, MaintenanceReceiptScalar<'_>)],
    ) -> Option<weather_database::Sha256Digest> {
        Some(weather_database::Sha256Digest::admitted(HASH_S))
    }
}

impl WorkerRevisionArchiveWriteValues for Values {
    type Error = &'static str;

    /// This fixture never binds a grouped weather revision.
    fn serialize_weather_revision_pointers(
        &mut self,
        _: &[weather_contracts::AdjustmentWeatherRevisionPointer],
    ) -> Result<SerializedJson, Self::Error> {
        unreachable!("temperature fixture must not serialize weather pointers")
    }

    /// Serialize one already validated pointer through the injected owner.
    fn serialize_ecmwf_temperature_revision_pointer(
        &mut self,
        _: &AdjustmentEcmwfTemperatureRevisionPointer,
    ) -> Result<SerializedJson, Self::Error> {
        Ok(SerializedJson::admitted("ecmwf-pointer"))
    }

    /// Delegate database receipt clocks to the canonical fixture resolver.
    fn utc_epoch_milliseconds(&mut self, value: &str) -> Result<i64, Self::Error> {
        ApiMaintenanceReceiptCanonicalValues::canonical_utc_epoch_milliseconds(self, value)
            .ok_or("clock")
    }
}

impl EcmwfRevisionMaterialDigest for Values {
    /// Match JavaScript's six-decimal source coordinate spelling for fixture values.
    fn fixed_six(&mut self, value: f64) -> Option<String> {
        value.is_finite().then(|| format!("{value:.6}"))
    }

    /// Reuse the fixture's admitted digest owner.
    fn sha256(&mut self, bytes: &[u8]) -> Option<weather_database::Sha256Digest> {
        ApiMaintenanceReceiptCanonicalValues::sha256(self, bytes)
    }
}

impl EcmwfRevisionArchiveValues for Values {
    /// Assert the complete generated document reached the named-order owner.
    fn encode_ecmwf_revision_projection(
        &self,
        projection: &AdjustmentTemperatureNativeSourceProjection,
    ) -> Option<Vec<u8>> {
        (projection.rows.len() == 18).then(|| b"native-temperature-projection\n".to_vec())
    }
}

struct FixtureTime;

impl CurrentTimeResolver for FixtureTime {
    /// Resolve the one fixture day and its preceding capture-boundary hour.
    fn resolve(&self, value: &JsDateOrString) -> Result<CanonicalInstant, CurrentProjectionError> {
        let JsDateOrString::String(value) = value else {
            return Err(clock_error());
        };
        let value = String::from_utf16(value).map_err(|_| clock_error())?;
        let day_offset = if value.starts_with("2026-10-08") {
            0
        } else if value.starts_with("2026-10-07") {
            -86_400_000
        } else {
            return Err(clock_error());
        };
        let hour = value
            .get(11..13)
            .and_then(|part| part.parse::<i64>().ok())
            .ok_or_else(clock_error)?;
        let minute = value
            .get(14..16)
            .and_then(|part| part.parse::<i64>().ok())
            .ok_or_else(clock_error)?;
        Ok(CanonicalInstant {
            epoch_milliseconds: BASE + day_offset + hour * 3_600_000 + minute * 60_000,
            utc: value,
        })
    }
}

struct Database {
    bind_fails: bool,
    events: Rc<RefCell<Vec<&'static str>>>,
}

impl WorkerRevisionArchiveWritePort for Database {
    type Error = &'static str;

    /// Reject the unrelated grouped weather binder.
    fn bind_weather_adjustment_revisions(
        &mut self,
        _: WorkerRevisionArchiveWritePlan,
    ) -> Result<Vec<WorkerWeatherRevisionBatchReceipt>, Self::Error> {
        unreachable!("temperature fixture must not bind weather revisions")
    }

    /// Return one receipt bound to the fixture projection and stage identities.
    fn bind_ecmwf_temperature_revision(
        &mut self,
        plan: WorkerRevisionArchiveWritePlan,
    ) -> Result<Vec<AdjustmentRevisionRowReceipt>, Self::Error> {
        self.events.borrow_mut().push("bind");
        let [WorkerRevisionArchiveWriteBind::Json(json)] = plan.binds;
        assert_eq!(json.as_str(), "ecmwf-pointer");
        if self.bind_fails {
            return Err("bind failed");
        }
        Ok(vec![AdjustmentRevisionRowReceipt {
            contractVersion: AdjustmentRevisionRowReceiptContractVersionLiteral,
            revisionReceipt: commit_receipt(),
        }])
    }
}

impl EcmwfRevisionGapPort for Database {
    type Error = &'static str;

    /// Persist one exact gap and return the database-owned marker.
    fn write_ecmwf_revision_gap(
        &mut self,
        plan: MaintenancePublicationPlan<'_, 3>,
    ) -> Result<Vec<AdjustmentRevisionGapMarker>, Self::Error> {
        self.events.borrow_mut().push("mark-gap");
        let [
            MaintenancePublicationBind::Text("ecmwf_temperature"),
            MaintenancePublicationBind::RevisionKeyJson(
                AdjustmentRevisionAdmissionKey::EcmwfTemperature(key),
            ),
            MaintenancePublicationBind::GapJson(gap),
        ] = plan.binds
        else {
            return Err("gap plan differs");
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

impl EcmwfRevisionArchiveTransactionPort for Database {
    /// Record every nested bind transition in source order.
    fn execute_ecmwf_revision_bind_savepoint(
        &mut self,
        command: EcmwfRevisionBindSavepointCommand,
    ) -> Result<(), <Self as WorkerRevisionArchiveWritePort>::Error> {
        self.events.borrow_mut().push(match command {
            EcmwfRevisionBindSavepointCommand::Savepoint => "bind-savepoint",
            EcmwfRevisionBindSavepointCommand::RollbackTo => "bind-rollback",
            EcmwfRevisionBindSavepointCommand::Release => "bind-release",
        });
        Ok(())
    }
}

struct Client {
    events: Rc<RefCell<Vec<&'static str>>>,
    publish_fails: bool,
}

impl WorkerRevisionArchiveClient for Client {
    /// Return the authentic durable receipt for the fixture body.
    fn stage_revision(&mut self, _: &[u8]) -> Result<AdjustmentRevisionStageReceipt, TaskFailure> {
        self.events.borrow_mut().push("stage");
        Ok(stage_receipt())
    }

    /// Record post-commit publication and optionally simulate a refusal.
    fn publish_revision(
        &mut self,
        _: &[u8],
        _: &AdjustmentRevisionStageReceipt,
        _: &AdjustmentRevisionCommitReceipt,
    ) -> Result<(), TaskFailure> {
        self.events.borrow_mut().push("publish");
        (!self.publish_fails)
            .then_some(())
            .ok_or_else(ecmwf_revision_archive_failure)
    }

    /// Reject the grouped publication path in a single-run fixture.
    fn publish_revision_batch(
        &mut self,
        _: &[u8],
        _: &AdjustmentRevisionStageReceipt,
        _: &[AdjustmentRevisionCommitReceipt],
    ) -> Result<(), TaskFailure> {
        unreachable!("temperature fixture must not batch publish")
    }

    /// Record one value-free permanent disposition.
    fn record_revision_gap(&mut self, _: &AdjustmentRevisionGapInput) -> Result<(), TaskFailure> {
        self.events.borrow_mut().push("record-gap");
        Ok(())
    }
}

#[test]
fn stages_binds_and_publishes_only_after_caller_commit() {
    let events = Rc::new(RefCell::new(Vec::new()));
    let mut database = Database {
        bind_fails: false,
        events: Rc::clone(&events),
    };
    let mut client = Client {
        events: Rc::clone(&events),
        publish_fails: false,
    };
    let mut values = Values;
    let publication = stage_ecmwf_revision_archive(
        &revision(),
        &capture_witness(),
        &FixtureTime,
        &mut database,
        &mut values,
        &mut client,
    )
    .expect("stage");
    events.borrow_mut().push("commit");
    publish_ecmwf_revision_archive(publication, &mut database, &mut client).expect("publish");

    assert_eq!(
        events.borrow().as_slice(),
        [
            "stage",
            "bind-savepoint",
            "bind",
            "bind-release",
            "commit",
            "publish"
        ]
    );
}

#[test]
fn recovers_bind_then_relays_the_persisted_gap_after_commit() {
    let events = Rc::new(RefCell::new(Vec::new()));
    let mut database = Database {
        bind_fails: true,
        events: Rc::clone(&events),
    };
    let mut client = Client {
        events: Rc::clone(&events),
        publish_fails: false,
    };
    let mut values = Values;
    let publication = stage_ecmwf_revision_archive(
        &revision(),
        &capture_witness(),
        &FixtureTime,
        &mut database,
        &mut values,
        &mut client,
    )
    .expect("gap");
    events.borrow_mut().push("commit");
    publish_ecmwf_revision_archive(publication, &mut database, &mut client).expect("relay");

    assert_eq!(
        events.borrow().as_slice(),
        [
            "stage",
            "bind-savepoint",
            "bind",
            "bind-rollback",
            "bind-release",
            "mark-gap",
            "commit",
            "record-gap",
        ]
    );
}

#[test]
fn publication_refusal_marks_and_relays_the_post_commit_gap() {
    let events = Rc::new(RefCell::new(Vec::new()));
    let mut transaction = Database {
        bind_fails: false,
        events: Rc::clone(&events),
    };
    let mut pool = Database {
        bind_fails: false,
        events: Rc::clone(&events),
    };
    let mut client = Client {
        events: Rc::clone(&events),
        publish_fails: true,
    };
    let mut values = Values;
    let publication = stage_ecmwf_revision_archive(
        &revision(),
        &capture_witness(),
        &FixtureTime,
        &mut transaction,
        &mut values,
        &mut client,
    )
    .expect("stage");
    events.borrow_mut().push("commit");
    assert_eq!(
        publish_ecmwf_revision_archive(publication, &mut pool, &mut client),
        Err(EcmwfRevisionArchiveError::Publish)
    );
    assert_eq!(
        events.borrow().as_slice(),
        [
            "stage",
            "bind-savepoint",
            "bind",
            "bind-release",
            "commit",
            "publish",
            "mark-gap",
            "record-gap",
        ]
    );
}

/// Build the complete original worker fixture shape.
fn revision() -> PersistedEcmwfTemperatureRevision {
    PersistedEcmwfTemperatureRevision {
        adapterVersion: EcmwfTemperatureCanaryRunAdapterVersionLiteral,
        bestMatchRows: (0..12).map(best_match_row).collect(),
        contentHash: HASH_C.to_owned(),
        hours: (0..18).map(native_hour).collect(),
        logicalReceivedAt: "2026-10-08T00:05:00.000Z".to_owned(),
        modelCycle: EcmwfTemperatureCanaryRunModelCycleEnum::Value50r1,
        providerResponseSha256: "d".repeat(64),
        recentErrorState: EcmwfTemperatureCanaryRecentErrorState {
            b24C: Nullable::Null,
            b72C: Nullable::Null,
            cohort: EcmwfTemperatureCanaryRecentErrorStateCohortLiteral,
            localDates: 0,
            mad72C: Nullable::Null,
            maximumSourceRunInitializedAt: Nullable::Null,
            maximumSourceValidAt: Nullable::Null,
            n24: 0,
            n72: 0,
            sourceKeys: vec![],
            supported: false,
            targetRunInitializedAt: "2026-10-08T00:00:00.000Z".to_owned(),
            windowEndValidAt: "2026-10-07T17:00:00.000Z".to_owned(),
        },
        recentErrorStateSha256: "e".repeat(64),
        runInitializedAt: "2026-10-08T00:00:00.000Z".to_owned(),
        siteId: "3".to_owned(),
        siteLatitude: 47.95043,
        siteLongitude: -122.42797,
        siteSlug: "ballydidean".to_owned(),
        upstreamModel: EcmwfTemperatureCanaryRunUpstreamModelLiteral,
    }
}

/// Build one native source hour in one-based lead order.
fn native_hour(index: usize) -> EcmwfTemperatureCanaryHour {
    EcmwfTemperatureCanaryHour {
        contentHash: format!("{index:064x}"),
        modelLeadHours: (index + 1) as u64,
        rawRelativeHumidityPercent: Nullable::Value(80.0),
        rawTemperatureC: 10.0 + index as f64,
        rawWindSpeedMps: Nullable::Value(5.0),
        validAt: format!("2026-10-08T{:02}:00:00.000Z", index + 1),
    }
}

/// Build one best-match comparator aligned to native leads seven through eighteen.
fn best_match_row(index: usize) -> WeatherRecordRow {
    WeatherRecordRow {
        adapterVersion: Nullable::Null,
        apparentTemperatureC: Nullable::Null,
        blackGlobeTemperatureC: Nullable::Null,
        cloudCoverPercent: Nullable::Null,
        contentHash: format!("{:064x}", index + 100),
        contractEpoch: Nullable::Null,
        deviceModel: Nullable::Null,
        deviceSerial: Nullable::Null,
        deviceVendor: Nullable::Null,
        firstReceivedAt: JsDateOrString::String(text("2026-10-08T00:05:00.000Z")),
        id: format!("row-{index}"),
        lastReceivedAt: JsDateOrString::String(text("2026-10-08T00:05:00.000Z")),
        pm25MicrogramsPerCubicMeter: Nullable::Null,
        precipitationMm: Nullable::Null,
        precipitationRateMmPerHour: Nullable::Null,
        pressureHpa: Nullable::Null,
        productRunAt: Nullable::Value(JsDateOrString::String(text("2026-10-08T00:00:00.000Z"))),
        providerKey: "open-meteo".to_owned(),
        providerMetadata: Nullable::Value(vec![(
            text("dataset"),
            JsonValue::String(text("best_match")),
        )]),
        qualityMetadata: Nullable::Null,
        relativeHumidityPercent: Nullable::Null,
        revisionCount: 1,
        siteSlug: "ballydidean".to_owned(),
        sourceId: "7".to_owned(),
        sourceConfigFingerprint: Nullable::Null,
        sourceKey: "open-meteo-forecast-v4".to_owned(),
        sourceKind: SourceKind::Forecast,
        stationSlug: "property".to_owned(),
        soilElectricalConductivityMicrosiemensPerCm: Nullable::Null,
        soilMoisturePercent: Nullable::Null,
        solarRadiationWm2: Nullable::Null,
        temperatureC: Nullable::Value(9.0 + index as f64),
        upstreamModel: Nullable::Value("best_match".to_owned()),
        upstreamTimezone: "UTC".to_owned(),
        uvIndex: Nullable::Null,
        validAt: JsDateOrString::String(text(format!("2026-10-08T{:02}:00:00.000Z", index + 7))),
        waterLevelM: Nullable::Null,
        windDirectionDegrees: Nullable::Null,
        windGustMps: Nullable::Null,
        windSpeedMps: Nullable::Null,
        wetBulbGlobeTemperatureC: Nullable::Null,
    }
}

/// Return one valid stage receipt bound to the fixture body identity.
fn stage_receipt() -> AdjustmentRevisionStageReceipt {
    AdjustmentRevisionStageReceipt {
        contractVersion: AdjustmentRevisionStageReceiptContractVersionLiteral,
        durable: AdjustmentRevisionStageReceiptDurableLiteral,
        durableAt: "2026-10-08T00:06:00.000Z".to_owned(),
        projectionIdentitySha256: HASH_A.to_owned(),
        projectionKind: AdjustmentRevisionProjectionKind::NativeSource,
        projectionSha256: HASH_A.to_owned(),
        stageReceiptSha256: HASH_S.to_owned(),
    }
}

/// Return one valid server-owned commit receipt.
fn commit_receipt() -> AdjustmentRevisionCommitReceipt {
    AdjustmentRevisionCommitReceipt {
        archiveCommitOrdinal: "1".to_owned(),
        archiveCommittedAt: "2026-10-08T00:07:00.000Z".to_owned(),
        contractVersion: AdjustmentRevisionCommitReceiptContractVersionLiteral,
        frontierSha256: HASH_A.to_owned(),
        predecessorFrontierSha256: HASH_B.to_owned(),
        projectionIdentitySha256: HASH_A.to_owned(),
        projectionKind: AdjustmentRevisionCommitReceiptProjectionKindEnum::NativeSource,
        projectionSha256: HASH_A.to_owned(),
        receiptSha256: HASH_C.to_owned(),
        stageReceiptSha256: HASH_S.to_owned(),
    }
}

/// Return the fixed verified capture boundary.
fn capture_witness() -> AdmittedAdjustmentRevisionCaptureWitness {
    let instant = CanonicalInstant {
        epoch_milliseconds: BASE,
        utc: "2026-10-08T00:00:00.000Z".to_owned(),
    };
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
            epochAt: instant.utc.clone(),
            servingSnapshotSha256: HASH_A.to_owned(),
            sourceCommit: String::new(),
            sourceRelease: String::new(),
            sourceServerImageDigest: String::new(),
            sourceWebImageDigest: String::new(),
            witnessSha256: HASH_A.to_owned(),
        },
        instant,
    )
    .expect("matching fixture capture witness")
}

/// Encode one fixture string as UTF-16.
fn text(value: impl AsRef<str>) -> Vec<u16> {
    value.as_ref().encode_utf16().collect()
}

/// Build one deterministic fixture clock error.
fn clock_error() -> CurrentProjectionError {
    CurrentProjectionError {
        code: "fixture_clock",
        detail: "fixture clock differs".to_owned(),
    }
}
