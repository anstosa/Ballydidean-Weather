//! Positive fixed-gauge projection coverage against the compiled Node logical-key golden.

use std::cell::Cell;

use weather_api::CanonicalInstant;
use weather_contracts::{
    AdjustmentRainFixedGaugeTargetCaptureBody, AdjustmentRainFixedGaugeTargetCaptureMember,
    AdjustmentRainFixedGaugeTargetClaim, AdjustmentRainFixedGaugeTargetProjection,
    AdjustmentRainFixedGaugeTargetProjectionContractVersionLiteral,
    AdjustmentRainFixedGaugeTargetProjectionFamilyLiteral,
    AdjustmentRainFixedGaugeTargetProjectionProjectionKindLiteral,
    AdjustmentRainFixedGaugeTargetRow, AdjustmentRevisionProjectionKind,
    AdjustmentWeatherRevisionPointerSourceKindEnum, CanonicalWeatherMetrics,
    NormalizedWeatherRecord, Nullable, SourceKind, WeatherRecordMetadata,
};
use weather_database::{AdjustmentRevisionAdmissionKey, Sha256Digest};

use super::{
    ApiMaintenanceRevisionTransportDocument, RevisionRequestShape, project_revision_document,
};
use crate::{ApiMaintenanceReceiptCanonicalValues, MaintenanceReceiptScalar};

const VALID_AT: &str = "2026-10-08T16:00:00.000Z";
const WINDOW_END: &str = "2026-10-08T17:00:00.000Z";
const RECEIVED_AT: &str = "2026-10-08T18:00:00.000Z";
const VALID_AT_EPOCH_MS: i64 = 1_791_475_200_000;
const LOGICAL_KEY_SHA256: &str = "ab41db809153902430da7f556283a4a8753fea1e216e6fe99a4a7e9df46cee00";

struct Values {
    logical_key_hashes: Cell<u32>,
}

impl ApiMaintenanceReceiptCanonicalValues for Values {
    /// Resolve only the three retained fixture clocks at or after the inclusive witness.
    fn canonical_utc_epoch_milliseconds(&self, value: &str) -> Option<i64> {
        match value {
            VALID_AT => Some(VALID_AT_EPOCH_MS),
            WINDOW_END => Some(VALID_AT_EPOCH_MS + 3_600_000),
            RECEIVED_AT => Some(VALID_AT_EPOCH_MS + 7_200_000),
            _ => None,
        }
    }

    /// No document bytes are hashed while projecting an already decoded typed fixture.
    fn sha256(&self, _: &[u8]) -> Option<Sha256Digest> {
        unreachable!("fixed-gauge projection must not rehash document bytes")
    }

    /// Assert the one shared value-free logical key before returning the Node golden.
    fn unsigned_receipt_sha256(
        &self,
        fields: &[(&str, MaintenanceReceiptScalar<'_>)],
    ) -> Option<Sha256Digest> {
        self.logical_key_hashes
            .set(self.logical_key_hashes.get() + 1);
        assert_eq!(fields.len(), 3);
        assert_eq!(
            fields[0],
            (
                "contractVersion",
                MaintenanceReceiptScalar::Text("adjustment-rain-fixed-gauge-target-logical-key/v1")
            )
        );
        assert_eq!(fields[1].0, "sourceIds");
        let MaintenanceReceiptScalar::TextArray(source_ids) = fields[1].1 else {
            panic!("sourceIds must retain the original ordered string array");
        };
        assert_eq!(source_ids, source_ids_fixture());
        assert_eq!(
            fields[2],
            ("validAt", MaintenanceReceiptScalar::Text(VALID_AT))
        );
        Some(Sha256Digest::admitted(LOGICAL_KEY_SHA256))
    }
}

/// Produce the original frozen twelve-source catalog order.
fn source_ids_fixture() -> Vec<String> {
    (101..=112).map(|source_id| source_id.to_string()).collect()
}

/// Produce nullable metrics sufficient for the already decoded typed projection boundary.
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

/// Build one original typed fixed-gauge projection with retained raw-claim clocks.
fn fixed_gauge_projection() -> AdjustmentRainFixedGaugeTargetProjection {
    let source_ids = source_ids_fixture();
    let claims = source_ids
        .iter()
        .map(
            // preserve source order in the original capture graph
            |source_id| AdjustmentRainFixedGaugeTargetClaim {
                claimId: format!("claim-{source_id}"),
                completedAt: RECEIVED_AT.to_owned(),
                stationId: source_id.parse().unwrap(),
                windowEndExclusive: WINDOW_END.to_owned(),
                windowStart: VALID_AT.to_owned(),
            },
        )
        .collect();
    let rows = source_ids
        .iter()
        .enumerate()
        .map(
            // bind every ordered source to its distinct stored-content identity
            |(index, source_id)| AdjustmentRainFixedGaugeTargetRow {
                captureMembers: vec![AdjustmentRainFixedGaugeTargetCaptureMember {
                    bodySha256: "b".repeat(64),
                    claimId: format!("claim-{source_id}"),
                    completedAt: RECEIVED_AT.to_owned(),
                }],
                intervals: Vec::new(),
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
                    receivedAt: RECEIVED_AT.to_owned(),
                    sourceId: source_id.clone(),
                    sourceKind: SourceKind::PhysicalSensor,
                    validAt: VALID_AT.to_owned(),
                },
                stationId: source_id.parse().unwrap(),
                storedContentSha256: format!("{:064x}", index + 1),
            },
        )
        .collect();
    AdjustmentRainFixedGaugeTargetProjection {
        captureBodies: vec![AdjustmentRainFixedGaugeTargetCaptureBody {
            bodyBase64: "Zml4dHVyZQ==".to_owned(),
            bodySha256: "b".repeat(64),
            claims,
        }],
        contractVersion: AdjustmentRainFixedGaugeTargetProjectionContractVersionLiteral,
        family: AdjustmentRainFixedGaugeTargetProjectionFamilyLiteral,
        logicalReceivedAt: RECEIVED_AT.to_owned(),
        projectionKind: AdjustmentRainFixedGaugeTargetProjectionProjectionKindLiteral,
        rows,
        validAt: VALID_AT.to_owned(),
    }
}

/// Preserve all twelve weather pointers and one shared original logical-key digest.
#[test]
fn fixed_gauge_group_projection_keeps_ordered_weather_pointers_and_shared_hash() {
    let document = ApiMaintenanceRevisionTransportDocument::FixedGauge(fixed_gauge_projection());
    let values = Values {
        logical_key_hashes: Cell::new(0),
    };
    let evidence = project_revision_document(
        &document,
        RevisionRequestShape::Grouped,
        &CanonicalInstant {
            epoch_milliseconds: VALID_AT_EPOCH_MS,
            utc: VALID_AT.to_owned(),
        },
        &values,
    )
    .unwrap();

    assert_eq!(
        evidence.kind,
        AdjustmentRevisionProjectionKind::TargetRevision
    );
    assert_eq!(evidence.pointers.len(), 12);
    // preserve row order and exact per-row database identity under the shared logical hash
    for (index, pointer) in evidence.pointers.iter().enumerate() {
        let source_id = (index + 101).to_string();
        let AdjustmentRevisionAdmissionKey::WeatherRecord(key) = &pointer.key else {
            panic!("fixed-gauge rows must publish weather-record pointers");
        };
        assert_eq!(key.productRunAt, Nullable::Null);
        assert_eq!(key.sourceId, source_id);
        assert_eq!(
            key.sourceKind,
            AdjustmentWeatherRevisionPointerSourceKindEnum::PhysicalSensor
        );
        assert_eq!(key.storedContentSha256, format!("{:064x}", index + 1));
        assert_eq!(key.validAt, VALID_AT);
        assert_eq!(pointer.logical_key_sha256, LOGICAL_KEY_SHA256);
    }
    assert_eq!(values.logical_key_hashes.get(), 1);
}
