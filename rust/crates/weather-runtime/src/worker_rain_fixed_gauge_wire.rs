//! Adapt fixed-gauge worker values to the retained generated persistence and archive contracts.

use weather_contracts::{
    AdjustmentRevisionCommitReceipt, AdjustmentRevisionCommitReceiptContractVersionLiteral,
    AdjustmentRevisionCommitReceiptProjectionKindEnum, AdjustmentRevisionGap,
    AdjustmentRevisionGapInput, AdjustmentRevisionGapReason, AdjustmentRevisionGapReasonEnum,
    AdjustmentRevisionProjectionKind, CanonicalWeatherMetrics, DeviceMetadata, FieldPresence,
    JsonValue, NormalizedWeatherRecord, Nullable, RainFixedGaugeTargetRevisionGroup,
    RainFixedGaugeTargetRevisionRow, SourceKind, WeatherRecordMetadata,
};
use weather_worker::{
    RAIN_COLLECTION_STATIONS, RainFixedGaugeCommitReceipt, RainFixedGaugeGapReason,
    RainFixedGaugeNormalizedRecord, RainFixedGaugeRevisionGap, RainFixedGaugeRevisionGroup,
};

use crate::worker_rain_shadow::string;

/// Rebuild the fixed physical-sensor provenance alongside the original narrow target metrics.
pub(crate) fn target_record(
    station_id: u64,
    record: &RainFixedGaugeNormalizedRecord,
) -> Option<NormalizedWeatherRecord> {
    let station = RAIN_COLLECTION_STATIONS
        .iter()
        .find(|station| station.location_id == station_id)?;
    let metrics = record.metrics;
    let nullable = |value: Option<f64>| value.map_or(Nullable::Null, Nullable::Value);
    let quality = if metrics.precipitation_mm.is_some() {
        "complete_interval_coverage"
    } else {
        "incomplete_interval_coverage"
    };
    Some(NormalizedWeatherRecord {
        metadata: WeatherRecordMetadata {
            device: Nullable::Value(DeviceMetadata {
                model: FieldPresence::Value("Tempest".to_owned()),
                serial: FieldPresence::Value(station.serial.to_owned()),
                vendor: FieldPresence::Value("WeatherFlow".to_owned()),
            }),
            model: Nullable::Null,
            provider: Nullable::Value(fields([
                ("dataset", string("rain_fixed_gauge_hourly")),
                ("device_id", JsonValue::Number(station.device_id as f64)),
                ("location_id", JsonValue::Number(station_id as f64)),
                ("report_interval_minutes", JsonValue::Number(60.0)),
            ])),
            quality: Nullable::Value(fields([
                ("sampling", string("backward_exact_interval_tiling")),
                ("status", string(quality)),
            ])),
            upstreamTimezone: "America/Los_Angeles".to_owned(),
        },
        metrics: CanonicalWeatherMetrics {
            apparentTemperatureC: nullable(metrics.apparent_temperature_c),
            blackGlobeTemperatureC: nullable(metrics.black_globe_temperature_c),
            cloudCoverPercent: nullable(metrics.cloud_cover_percent),
            pm25MicrogramsPerCubicMeter: nullable(metrics.pm25_micrograms_per_cubic_meter),
            precipitationMm: nullable(metrics.precipitation_mm),
            precipitationRateMmPerHour: nullable(metrics.precipitation_rate_mm_per_hour),
            pressureHpa: nullable(metrics.pressure_hpa),
            relativeHumidityPercent: nullable(metrics.relative_humidity_percent),
            soilElectricalConductivityMicrosiemensPerCm: nullable(
                metrics.soil_electrical_conductivity_microsiemens_per_cm,
            ),
            soilMoisturePercent: nullable(metrics.soil_moisture_percent),
            solarRadiationWm2: nullable(metrics.solar_radiation_wm2),
            temperatureC: nullable(metrics.temperature_c),
            uvIndex: nullable(metrics.uv_index),
            waterLevelM: nullable(metrics.water_level_m),
            windDirectionDegrees: nullable(metrics.wind_direction_degrees),
            windGustMps: nullable(metrics.wind_gust_mps),
            windSpeedMps: nullable(metrics.wind_speed_mps),
            wetBulbGlobeTemperatureC: nullable(metrics.wet_bulb_globe_temperature_c),
        },
        productRunAt: Nullable::Null,
        receivedAt: record.received_at.clone(),
        sourceId: record.source_id.clone(),
        sourceKind: SourceKind::PhysicalSensor,
        validAt: record.valid_at.clone(),
    })
}

/// Preserve the original typed JSON-object property order.
fn fields<const N: usize>(entries: [(&str, JsonValue); N]) -> Vec<(Vec<u16>, JsonValue)> {
    entries
        .into_iter()
        .map(|(key, value)| (key.encode_utf16().collect(), value))
        .collect()
}

/// Bind each normalized row and content digest to the same complete ordered group.
pub(crate) fn target_group(
    group: &RainFixedGaugeRevisionGroup,
) -> Option<RainFixedGaugeTargetRevisionGroup> {
    let rows = group
        .rows
        .iter()
        .map(|row| {
            Some(RainFixedGaugeTargetRevisionRow {
                normalizedRecord: target_record(row.station_id, &row.normalized_record)?,
                stationId: row.station_id,
                storedContentSha256: row.stored_content_sha256.clone(),
            })
        })
        .collect::<Option<Vec<_>>>()?;
    Some(RainFixedGaugeTargetRevisionGroup {
        logicalKeySha256: group.logical_key_sha256.clone(),
        projectionIdentitySha256: group.projection_identity_sha256.clone(),
        projectionSha256: group.projection_sha256.clone(),
        rows,
        stageReceiptSha256: group
            .stage_receipt_sha256
            .clone()
            .map_or(Nullable::Null, Nullable::Value),
        validAt: group.valid_at.clone(),
    })
}

/// Retain the full committed receipt; the PostgreSQL bigint ordinal fits its worker carrier.
pub(crate) fn received_target_receipt(
    value: AdjustmentRevisionCommitReceipt,
) -> Option<RainFixedGaugeCommitReceipt> {
    // prevent another projection family or noncanonical ordinal from entering this group
    if value.projectionKind != AdjustmentRevisionCommitReceiptProjectionKindEnum::TargetRevision
        || !value
            .archiveCommitOrdinal
            .as_bytes()
            .first()
            .is_some_and(|b| (b'1'..=b'9').contains(b))
        || !value
            .archiveCommitOrdinal
            .bytes()
            .all(|b| b.is_ascii_digit())
    {
        return None;
    }
    Some(RainFixedGaugeCommitReceipt {
        archive_commit_ordinal: value.archiveCommitOrdinal.parse().ok()?,
        archive_committed_at: value.archiveCommittedAt,
        frontier_sha256: value.frontierSha256,
        predecessor_frontier_sha256: value.predecessorFrontierSha256,
        projection_identity_sha256: value.projectionIdentitySha256,
        projection_sha256: value.projectionSha256,
        receipt_sha256: value.receiptSha256,
        stage_receipt_sha256: value.stageReceiptSha256,
    })
}

/// Restore the original wire receipt without discarding any ordinal or frontier evidence.
pub(crate) fn outbound_target_receipt(
    value: &RainFixedGaugeCommitReceipt,
) -> AdjustmentRevisionCommitReceipt {
    AdjustmentRevisionCommitReceipt {
        archiveCommitOrdinal: value.archive_commit_ordinal.to_string(),
        archiveCommittedAt: value.archive_committed_at.clone(),
        contractVersion: AdjustmentRevisionCommitReceiptContractVersionLiteral,
        frontierSha256: value.frontier_sha256.clone(),
        predecessorFrontierSha256: value.predecessor_frontier_sha256.clone(),
        projectionIdentitySha256: value.projection_identity_sha256.clone(),
        projectionKind: AdjustmentRevisionCommitReceiptProjectionKindEnum::TargetRevision,
        projectionSha256: value.projection_sha256.clone(),
        receiptSha256: value.receipt_sha256.clone(),
        stageReceiptSha256: value.stage_receipt_sha256.clone(),
    }
}

/// Retain only a matching database gap for the original target revision family.
pub(crate) fn received_target_gap(gap: AdjustmentRevisionGap) -> Option<RainFixedGaugeRevisionGap> {
    // this producer never emits a body-bound gap for another revision kind
    if gap.projectionKind != AdjustmentRevisionCommitReceiptProjectionKindEnum::TargetRevision {
        return None;
    }
    let (Nullable::Value(identity), Nullable::Value(projection)) =
        (gap.projectionIdentitySha256, gap.projectionSha256)
    else {
        return None;
    };
    let reason = match gap.reason {
        AdjustmentRevisionGapReasonEnum::ArchiveStageFailed => {
            RainFixedGaugeGapReason::ArchiveStageFailed
        }
        AdjustmentRevisionGapReasonEnum::DatabaseBindFailed => {
            RainFixedGaugeGapReason::DatabaseBindFailed
        }
        AdjustmentRevisionGapReasonEnum::ArchivePublishFailed => {
            RainFixedGaugeGapReason::ArchivePublishFailed
        }
        AdjustmentRevisionGapReasonEnum::DatabaseAdmissionFailed => {
            RainFixedGaugeGapReason::DatabaseAdmissionFailed
        }
    };
    Some(RainFixedGaugeRevisionGap {
        logical_key_sha256: gap.logicalKeySha256,
        projection_identity_sha256: identity,
        projection_sha256: projection,
        reason,
    })
}

/// Bind one categorical marked target gap to the private request.
pub(crate) fn outbound_target_gap(
    gap: &RainFixedGaugeRevisionGap,
) -> Option<AdjustmentRevisionGapInput> {
    let reason = match gap.reason {
        RainFixedGaugeGapReason::ArchiveStageFailed => {
            AdjustmentRevisionGapReason::ArchiveStageFailed
        }
        RainFixedGaugeGapReason::DatabaseBindFailed => {
            AdjustmentRevisionGapReason::DatabaseBindFailed
        }
        RainFixedGaugeGapReason::ArchivePublishFailed => {
            AdjustmentRevisionGapReason::ArchivePublishFailed
        }
        RainFixedGaugeGapReason::DatabaseAdmissionFailed => {
            AdjustmentRevisionGapReason::DatabaseAdmissionFailed
        }
        RainFixedGaugeGapReason::TargetSourceOversized => return None,
    };
    Some(AdjustmentRevisionGapInput {
        logicalKeySha256: gap.logical_key_sha256.clone(),
        projectionIdentitySha256: Nullable::Value(gap.projection_identity_sha256.clone()),
        projectionKind: AdjustmentRevisionProjectionKind::TargetRevision,
        projectionSha256: Nullable::Value(gap.projection_sha256.clone()),
        reason,
    })
}
