use weather_contracts::{
    AdjustmentRevisionCommitReceiptContractVersionLiteral, CanonicalWeatherMetrics,
    RainFixedGaugeTargetRevisionRow, WeatherRecordMetadata,
};

use super::*;
use crate::MaintenancePublicationBind;

#[derive(Debug, PartialEq)]
enum Event {
    Savepoint(WorkerRainFixedGaugeSavepointCommand),
    Bind(String),
    Gap(String),
}

#[derive(Default)]
struct Session {
    bind_calls: usize,
    events: Vec<Event>,
}

impl WorkerRainFixedGaugeWriteSession for Session {
    type Error = &'static str;

    /// keep outer transaction setup outside this focused helper fixture
    fn begin(&mut self) -> Result<(), Self::Error> {
        unreachable!("the focused binder helper does not begin transactions")
    }

    /// keep catalog reads outside this focused helper fixture
    fn read_source_catalog(
        &mut self,
        _: WorkerRainFixedGaugeCatalogPlan,
    ) -> Result<RainFixedGaugeTargetSourceCatalog, Self::Error> {
        unreachable!("the focused binder helper does not read the catalog")
    }

    /// keep source locking outside this focused helper fixture
    fn lock_sources(&mut self, _: WorkerRainFixedGaugeLockPlan<'_>) -> Result<(), Self::Error> {
        unreachable!("the focused binder helper does not lock sources")
    }

    /// keep existing-row reads outside this focused helper fixture
    fn read_existing_target_rows(
        &mut self,
        _: WorkerRainFixedGaugeExistingRowsPlan<'_>,
    ) -> Result<Vec<WorkerRainFixedGaugeStoredRow>, Self::Error> {
        unreachable!("the focused binder helper does not read rows")
    }

    /// keep provenance writes outside this focused helper fixture
    fn insert_target_run(
        &mut self,
        _: WorkerRainFixedGaugeRunPlan<'_>,
    ) -> Result<Vec<String>, Self::Error> {
        unreachable!("the focused binder helper does not insert runs")
    }

    /// keep weather writes outside this focused helper fixture
    fn insert_target_weather_row(
        &mut self,
        _: WorkerRainFixedGaugeWeatherPlan<'_>,
    ) -> Result<u64, Self::Error> {
        unreachable!("the focused binder helper does not insert rows")
    }

    /// retain every named savepoint transition in execution order
    fn execute_savepoint(
        &mut self,
        command: WorkerRainFixedGaugeSavepointCommand,
    ) -> Result<(), Self::Error> {
        self.events.push(Event::Savepoint(command));
        Ok(())
    }

    /// fail the sixth ordered row after five otherwise valid receipts
    fn bind_revision(
        &mut self,
        plan: WorkerRainFixedGaugeBindPlan<'_>,
    ) -> Result<Vec<WorkerRainFixedGaugeBoundRow>, Self::Error> {
        let pointer = &plan.pointers[0];
        self.events.push(Event::Bind(pointer.sourceId.clone()));
        self.bind_calls += 1;
        // fail only after a partial receipt vector has accumulated internally
        if self.bind_calls == 6 {
            return Err("sixth bind failed");
        }
        Ok(vec![WorkerRainFixedGaugeBoundRow {
            source_id: pointer.sourceId.clone(),
            valid_at: pointer.validAt.clone(),
            stored_content_sha256: pointer.storedContentSha256.clone(),
            revision_receipt: receipt(pointer, self.bind_calls),
        }])
    }

    /// project the shared gap facade back to its exact row identity
    fn write_revision_gap(
        &mut self,
        plan: MaintenancePublicationPlan<'_, 3>,
    ) -> Result<Vec<AdjustmentRevisionGapMarker>, Self::Error> {
        let [
            MaintenancePublicationBind::Text(_),
            MaintenancePublicationBind::RevisionKeyJson(
                AdjustmentRevisionAdmissionKey::WeatherRecord(key),
            ),
            MaintenancePublicationBind::GapJson(gap),
        ] = plan.binds
        else {
            panic!("fixed-gauge gaps must retain their weather-record key")
        };
        self.events.push(Event::Gap(key.sourceId.clone()));
        Ok(vec![AdjustmentRevisionGapMarker {
            logical_key_sha256: gap.logicalKeySha256.clone(),
            projection_identity_sha256: gap.projectionIdentitySha256.clone(),
            projection_kind: gap.projectionKind,
            projection_sha256: gap.projectionSha256.clone(),
            reason: gap.reason,
            stored_content_sha256: key.storedContentSha256.clone(),
        }])
    }

    /// keep outer commits outside this focused helper fixture
    fn commit(&mut self) -> Result<(), Self::Error> {
        unreachable!("the focused binder helper does not commit")
    }

    /// keep outer rollback outside this focused helper fixture
    fn rollback(&mut self) -> Result<(), Self::Error> {
        unreachable!("the focused binder helper does not roll back")
    }

    /// keep pool release outside this focused helper fixture
    fn release(&mut self) {
        unreachable!("the focused binder helper does not release sessions")
    }
}

/// prove one partial bind becomes only an ordered all-row gap disposition
#[test]
fn failed_mid_group_bind_rolls_back_before_marking_all_twelve_gaps() {
    let group = group();
    let gap = revision_gap(&group, AdjustmentRevisionGapReason::DatabaseBindFailed);
    let mut session = Session::default();

    let outcome: Result<
        RainFixedGaugeTargetRevisionResult,
        WorkerRainFixedGaugeWriteError<&'static str, ()>,
    > = bind_group_or_mark_gap(&mut session, &group, &gap);

    let RainFixedGaugeTargetRevisionResult::Gap(result) = outcome.expect("gap recovery should win")
    else {
        panic!("partial bind receipts must not escape as a committed group")
    };
    assert_eq!(
        result.gap.reason,
        AdjustmentRevisionGapReasonEnum::DatabaseBindFailed
    );
    let mut expected = vec![Event::Savepoint(
        WorkerRainFixedGaugeSavepointCommand::Create,
    )];
    expected.extend((101..=106).map(|source_id| Event::Bind(source_id.to_string())));
    expected.extend([
        Event::Savepoint(WorkerRainFixedGaugeSavepointCommand::RollbackTo),
        Event::Savepoint(WorkerRainFixedGaugeSavepointCommand::Release),
    ]);
    expected.extend((101..=112).map(|source_id| Event::Gap(source_id.to_string())));
    assert_eq!(session.events, expected);
}

/// build one valid target receipt for each pre-failure bind
fn receipt(
    pointer: &AdjustmentWeatherRevisionPointer,
    ordinal: usize,
) -> AdjustmentRevisionCommitReceipt {
    AdjustmentRevisionCommitReceipt {
        archiveCommitOrdinal: ordinal.to_string(),
        archiveCommittedAt: "2026-10-08T18:00:00.000Z".to_owned(),
        contractVersion: AdjustmentRevisionCommitReceiptContractVersionLiteral,
        frontierSha256: "e".repeat(64),
        predecessorFrontierSha256: "f".repeat(64),
        projectionIdentitySha256: pointer.projectionIdentitySha256.clone(),
        projectionKind: AdjustmentRevisionCommitReceiptProjectionKindEnum::TargetRevision,
        projectionSha256: pointer.projectionSha256.clone(),
        receiptSha256: format!("{ordinal:064x}"),
        stageReceiptSha256: pointer.stageReceiptSha256.clone(),
    }
}

/// build the twelve ordered target rows used by the binder loop
fn group() -> RainFixedGaugeTargetRevisionGroup {
    let rows = (101..=112)
        .enumerate()
        .map(|(index, source_id)| RainFixedGaugeTargetRevisionRow {
            normalizedRecord: NormalizedWeatherRecord {
                metadata: WeatherRecordMetadata {
                    device: Nullable::Null,
                    model: Nullable::Null,
                    provider: Nullable::Null,
                    quality: Nullable::Null,
                    upstreamTimezone: "America/Los_Angeles".to_owned(),
                },
                metrics: empty_metrics(),
                productRunAt: Nullable::Null,
                receivedAt: "2026-10-08T18:00:00.000Z".to_owned(),
                sourceId: source_id.to_string(),
                sourceKind: SourceKind::PhysicalSensor,
                validAt: "2026-10-08T16:00:00.000Z".to_owned(),
            },
            stationId: source_id,
            storedContentSha256: format!("{:064x}", index + 1),
        })
        .collect();
    RainFixedGaugeTargetRevisionGroup {
        logicalKeySha256: "a".repeat(64),
        projectionIdentitySha256: "b".repeat(64),
        projectionSha256: "b".repeat(64),
        rows,
        stageReceiptSha256: Nullable::Value("c".repeat(64)),
        validAt: "2026-10-08T16:00:00.000Z".to_owned(),
    }
}

/// build nullable metrics sufficient for a typed binder fixture
fn empty_metrics() -> CanonicalWeatherMetrics {
    CanonicalWeatherMetrics {
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
        temperatureC: Nullable::Null,
        uvIndex: Nullable::Null,
        waterLevelM: Nullable::Null,
        wetBulbGlobeTemperatureC: Nullable::Null,
        windDirectionDegrees: Nullable::Null,
        windGustMps: Nullable::Null,
        windSpeedMps: Nullable::Null,
    }
}
