//! Lossless generated-type decoding after the existing canonical revision admission.

use weather_contracts::{
    AdjustmentRainFixedGaugeTargetCaptureBody, AdjustmentRainFixedGaugeTargetCaptureMember,
    AdjustmentRainFixedGaugeTargetClaim, AdjustmentRainFixedGaugeTargetInterval,
    AdjustmentRainFixedGaugeTargetProjection,
    AdjustmentRainFixedGaugeTargetProjectionContractVersionLiteral,
    AdjustmentRainFixedGaugeTargetProjectionFamilyLiteral,
    AdjustmentRainFixedGaugeTargetProjectionProjectionKindLiteral,
    AdjustmentRainFixedGaugeTargetRow, AdjustmentRainGateControlProjection,
    AdjustmentRainGateControlProjectionContractVersionLiteral,
    AdjustmentRainGateControlProjectionFamilyLiteral,
    AdjustmentRainGateControlProjectionLogicalKey,
    AdjustmentRainGateControlProjectionProjectionKindLiteral,
    AdjustmentRainGateControlProjectionRows,
    AdjustmentRainGateControlProjectionRowsIncumbentProbability,
    AdjustmentRainGateControlProjectionRowsNativeSourceProbability,
    AdjustmentRainGateControlProjectionRowsPersistenceReasonEnum,
    AdjustmentRainGateFeatureProjection, AdjustmentRainGateFeatureProjectionContractVersionLiteral,
    AdjustmentRainGateFeatureProjectionFamilyLiteral,
    AdjustmentRainGateFeatureProjectionLogicalKey,
    AdjustmentRainGateFeatureProjectionProjectionKindLiteral,
    AdjustmentRainGateFeatureProjectionRows, AdjustmentRevisionBatchProjection,
    AdjustmentRevisionBatchProjectionContractVersionLiteral,
    AdjustmentRevisionBatchProjectionFamilyEnum, AdjustmentRevisionBatchProjectionLogicalKeyValue,
    AdjustmentRevisionBatchProjectionProjectionKindEnum,
    AdjustmentRevisionBatchProjectionRowsValue, AdjustmentRevisionProjection,
    AdjustmentRevisionProjectionContractVersionLiteral, AdjustmentRevisionProjectionDocument,
    AdjustmentRevisionProjectionFamily, AdjustmentRevisionProjectionKind,
    AdjustmentRevisionProjectionLogicalKeyValue, AdjustmentRevisionProjectionRowsValue,
    AdjustmentTemperatureNativeSourceProjection,
    AdjustmentTemperatureNativeSourceProjectionContractVersionLiteral,
    AdjustmentTemperatureNativeSourceProjectionFamilyLiteral,
    AdjustmentTemperatureNativeSourceProjectionProjectionKindLiteral, CanonicalWeatherMetrics,
    DeviceMetadata, FieldPresence, JsonValue, MaintenanceShadowTemperatureRecentErrorState,
    MaintenanceShadowTemperatureRecentErrorStateCohortLiteral, NormalizedWeatherRecord, Nullable,
    RainMaintenancePersistenceCaptureMember, RainMaintenancePersistenceTarget,
    RainMaintenancePersistenceTargetContractVersionLiteral,
    RainMaintenancePersistenceTargetReasonEnum, RainMaintenancePersistenceTargetRow,
    RainMaintenancePersistenceTargetRowCountLiteral, SourceKind, WeatherRecordMetadata,
};
use weather_edge::{ArchiveWireCodec, ArchiveWirePrimitives, admit_archive_revision_projection};

use crate::{
    ApiMaintenanceRevisionTransportDocument, NativeModelEvidenceValues,
    NativePrivateRevisionDocumentDecoder,
};

/// Zero-state decoder that retains the complete generated document after canonical admission.
#[derive(Clone, Copy, Debug, Default)]
pub struct NativeApiRevisionDocumentDecoder;

impl NativePrivateRevisionDocumentDecoder for NativeApiRevisionDocumentDecoder {
    /// Admit exact producer bytes first, then losslessly copy their generated fields.
    fn decode_revision_document(
        &self,
        projection: &[u8],
    ) -> Option<ApiMaintenanceRevisionTransportDocument> {
        decode_native_api_revision_document(projection)
    }
}

/// Decode every original revision projection without replacing the archive's canonical validator.
pub fn decode_native_api_revision_document(
    projection: &[u8],
) -> Option<ApiMaintenanceRevisionTransportDocument> {
    let values = NativeModelEvidenceValues::new();
    let codec = ArchiveWireCodec::new(values);
    let admitted = admit_archive_revision_projection(&codec, projection)?;
    let document = values.decode_json(projection)?;
    let contract = text(field(&document, "contractVersion")?)?;
    let decoded = match contract.as_str() {
        AdjustmentRevisionProjectionContractVersionLiteral::WIRE_VALUE => {
            ApiMaintenanceRevisionTransportDocument::Standard(Box::new(
                AdjustmentRevisionProjectionDocument::Single(decode_single(&document)?),
            ))
        }
        AdjustmentRevisionBatchProjectionContractVersionLiteral::WIRE_VALUE => {
            ApiMaintenanceRevisionTransportDocument::Standard(Box::new(
                AdjustmentRevisionProjectionDocument::Batch(decode_batch(&document)?),
            ))
        }
        AdjustmentTemperatureNativeSourceProjectionContractVersionLiteral::WIRE_VALUE => {
            ApiMaintenanceRevisionTransportDocument::Standard(Box::new(
                AdjustmentRevisionProjectionDocument::NativeTemperature(decode_native_temperature(
                    &document,
                )?),
            ))
        }
        AdjustmentRainGateFeatureProjectionContractVersionLiteral::WIRE_VALUE => {
            ApiMaintenanceRevisionTransportDocument::Standard(Box::new(
                AdjustmentRevisionProjectionDocument::RainFeature(decode_rain_feature(&document)?),
            ))
        }
        AdjustmentRainGateControlProjectionContractVersionLiteral::WIRE_VALUE => {
            ApiMaintenanceRevisionTransportDocument::Standard(Box::new(
                AdjustmentRevisionProjectionDocument::RainControl(decode_rain_control(&document)?),
            ))
        }
        AdjustmentRainFixedGaugeTargetProjectionContractVersionLiteral::WIRE_VALUE => {
            ApiMaintenanceRevisionTransportDocument::FixedGauge(decode_fixed_gauge(&document)?)
        }
        _ => return None,
    };
    // bind the second-stage discriminator to the already admitted canonical family
    (projection_kind(&decoded) == admitted.projection_kind).then_some(decoded)
}

/// Decode one ordinary single-pointer projection after its full edge admission.
fn decode_single(value: &JsonValue) -> Option<AdjustmentRevisionProjection> {
    Some(AdjustmentRevisionProjection {
        contractVersion: AdjustmentRevisionProjectionContractVersionLiteral,
        family: enum_value(
            field(value, "family")?,
            AdjustmentRevisionProjectionFamily::WIRE_VALUES,
        )?,
        logicalKey: logical_key(field(value, "logicalKey")?)?,
        logicalReceivedAt: text(field(value, "logicalReceivedAt")?)?,
        projectionKind: enum_value(
            field(value, "projectionKind")?,
            AdjustmentRevisionProjectionKind::WIRE_VALUES,
        )?,
        rows: scalar_rows(field(value, "rows")?)?,
        source: nullable_string_object(field(value, "source")?)?,
        storedContentSha256: text(field(value, "storedContentSha256")?)?,
    })
}

/// Decode one grouped projection while preserving every ordered logical and row member.
fn decode_batch(value: &JsonValue) -> Option<AdjustmentRevisionBatchProjection> {
    Some(AdjustmentRevisionBatchProjection {
        contractVersion: AdjustmentRevisionBatchProjectionContractVersionLiteral,
        family: enum_value(
            field(value, "family")?,
            AdjustmentRevisionBatchProjectionFamilyEnum::WIRE_VALUES,
        )?,
        logicalKey: batch_logical_key(field(value, "logicalKey")?)?,
        logicalReceivedAt: text(field(value, "logicalReceivedAt")?)?,
        projectionKind: enum_value(
            field(value, "projectionKind")?,
            AdjustmentRevisionBatchProjectionProjectionKindEnum::WIRE_VALUES,
        )?,
        rows: batch_scalar_rows(field(value, "rows")?)?,
        source: nullable_string_object(field(value, "source")?)?,
        storedContentSha256: text(field(value, "storedContentSha256")?)?,
    })
}

/// Decode the complete native-temperature projection and its recent-error state.
fn decode_native_temperature(
    value: &JsonValue,
) -> Option<AdjustmentTemperatureNativeSourceProjection> {
    Some(AdjustmentTemperatureNativeSourceProjection {
        contractVersion: AdjustmentTemperatureNativeSourceProjectionContractVersionLiteral,
        family: AdjustmentTemperatureNativeSourceProjectionFamilyLiteral,
        logicalKey: logical_key(field(value, "logicalKey")?)?,
        logicalReceivedAt: text(field(value, "logicalReceivedAt")?)?,
        projectionKind: AdjustmentTemperatureNativeSourceProjectionProjectionKindLiteral,
        recentErrorState: decode_recent_error_state(field(value, "recentErrorState")?)?,
        recentErrorStateSha256: text(field(value, "recentErrorStateSha256")?)?,
        rows: scalar_rows(field(value, "rows")?)?,
        source: nullable_string_object(field(value, "source")?)?,
        storedContentSha256: text(field(value, "storedContentSha256")?)?,
    })
}

/// Decode every field of the source-backed temperature recent-error state.
fn decode_recent_error_state(
    value: &JsonValue,
) -> Option<MaintenanceShadowTemperatureRecentErrorState> {
    Some(MaintenanceShadowTemperatureRecentErrorState {
        b24C: nullable_number(field(value, "b24C")?)?,
        b72C: nullable_number(field(value, "b72C")?)?,
        cohort: MaintenanceShadowTemperatureRecentErrorStateCohortLiteral,
        localDates: unsigned(field(value, "localDates")?)?,
        mad72C: nullable_number(field(value, "mad72C")?)?,
        maximumSourceRunInitializedAt: nullable_text(field(
            value,
            "maximumSourceRunInitializedAt",
        )?)?,
        maximumSourceValidAt: nullable_text(field(value, "maximumSourceValidAt")?)?,
        n24: unsigned(field(value, "n24")?)?,
        n72: unsigned(field(value, "n72")?)?,
        sourceKeys: string_array(field(value, "sourceKeys")?)?,
        supported: boolean(field(value, "supported")?)?,
        targetRunInitializedAt: text(field(value, "targetRunInitializedAt")?)?,
        windowEndValidAt: text(field(value, "windowEndValidAt")?)?,
    })
}

/// Decode all rain feature inputs after the edge has validated their vector geometry.
fn decode_rain_feature(value: &JsonValue) -> Option<AdjustmentRainGateFeatureProjection> {
    let key = field(value, "logicalKey")?;
    Some(AdjustmentRainGateFeatureProjection {
        contractVersion: AdjustmentRainGateFeatureProjectionContractVersionLiteral,
        family: AdjustmentRainGateFeatureProjectionFamilyLiteral,
        logicalKey: AdjustmentRainGateFeatureProjectionLogicalKey {
            inputSha256: text(field(key, "inputSha256")?)?,
            modelSha256: text(field(key, "modelSha256")?)?,
            runInitializedAt: text(field(key, "runInitializedAt")?)?,
        },
        logicalReceivedAt: text(field(value, "logicalReceivedAt")?)?,
        projectionKind: AdjustmentRainGateFeatureProjectionProjectionKindLiteral,
        rows: array(field(value, "rows")?)?
            .iter()
            .map(decode_rain_feature_row)
            .collect::<Option<Vec<_>>>()?,
        source: nullable_string_object(field(value, "source")?)?,
        storedContentSha256: text(field(value, "storedContentSha256")?)?,
    })
}

/// Decode one complete rain feature vector row.
fn decode_rain_feature_row(value: &JsonValue) -> Option<AdjustmentRainGateFeatureProjectionRows> {
    Some(AdjustmentRainGateFeatureProjectionRows {
        features64: nullable_string_array(field(value, "features64")?)?,
        modelLeadHours: unsigned(field(value, "modelLeadHours")?)?,
        rawPrecipitationMm64: text(field(value, "rawPrecipitationMm64")?)?,
        rawTargetHourTemperatureC64: text(field(value, "rawTargetHourTemperatureC64")?)?,
        validAt: text(field(value, "validAt")?)?,
    })
}

/// Decode all rain control fields including persistence evidence and probability vectors.
fn decode_rain_control(value: &JsonValue) -> Option<AdjustmentRainGateControlProjection> {
    let key = field(value, "logicalKey")?;
    Some(AdjustmentRainGateControlProjection {
        contractVersion: AdjustmentRainGateControlProjectionContractVersionLiteral,
        family: AdjustmentRainGateControlProjectionFamilyLiteral,
        logicalKey: AdjustmentRainGateControlProjectionLogicalKey {
            inputSha256: text(field(key, "inputSha256")?)?,
            modelSha256: text(field(key, "modelSha256")?)?,
            runInitializedAt: text(field(key, "runInitializedAt")?)?,
        },
        logicalReceivedAt: text(field(value, "logicalReceivedAt")?)?,
        ordinalArtifactSha256: text(field(value, "ordinalArtifactSha256")?)?,
        persistenceTarget: decode_persistence_target(field(value, "persistenceTarget")?)?,
        projectionKind: AdjustmentRainGateControlProjectionProjectionKindLiteral,
        rows: array(field(value, "rows")?)?
            .iter()
            .map(decode_rain_control_row)
            .collect::<Option<Vec<_>>>()?,
        source: nullable_string_object(field(value, "source")?)?,
        stateSha256: text(field(value, "stateSha256")?)?,
        stateStageReceiptSha256: text(field(value, "stateStageReceiptSha256")?)?,
        storedContentSha256: text(field(value, "storedContentSha256")?)?,
    })
}

/// Decode the causal fixed-station persistence target retained by the control projection.
fn decode_persistence_target(value: &JsonValue) -> Option<RainMaintenancePersistenceTarget> {
    Some(RainMaintenancePersistenceTarget {
        contractVersion: RainMaintenancePersistenceTargetContractVersionLiteral,
        decisionAt: text(field(value, "decisionAt")?)?,
        prediction64: nullable_text(field(value, "prediction64")?)?,
        reason: enum_value(
            field(value, "reason")?,
            RainMaintenancePersistenceTargetReasonEnum::WIRE_VALUES,
        )?,
        rowCount: RainMaintenancePersistenceTargetRowCountLiteral,
        rows: array(field(value, "rows")?)?
            .iter()
            .map(decode_persistence_row)
            .collect::<Option<Vec<_>>>()?,
        targetMemberSha256: text(field(value, "targetMemberSha256")?)?,
        validAt: text(field(value, "validAt")?)?,
    })
}

/// Decode one physical-station row and its exact ordered capture members.
fn decode_persistence_row(value: &JsonValue) -> Option<RainMaintenancePersistenceTargetRow> {
    Some(RainMaintenancePersistenceTargetRow {
        captureMembers: array(field(value, "captureMembers")?)?
            .iter()
            .map(|member| {
                Some(RainMaintenancePersistenceCaptureMember {
                    bodySha256: text(field(member, "bodySha256")?)?,
                    claimId: text(field(member, "claimId")?)?,
                    completedAt: text(field(member, "completedAt")?)?,
                })
            })
            .collect::<Option<Vec<_>>>()?,
        precipitationMm64: nullable_text(field(value, "precipitationMm64")?)?,
        receivedAt: nullable_text(field(value, "receivedAt")?)?,
        stationId: unsigned(field(value, "stationId")?)?,
    })
}

/// Decode one rain-control row without recalculating already admitted feature evidence.
fn decode_rain_control_row(value: &JsonValue) -> Option<AdjustmentRainGateControlProjectionRows> {
    Some(AdjustmentRainGateControlProjectionRows {
        features64: nullable_string_array(field(value, "features64")?)?,
        incumbentArtifactIdentitySha256: nullable_text(field(
            value,
            "incumbentArtifactIdentitySha256",
        )?)?,
        incumbentPrediction64: text(field(value, "incumbentPrediction64")?)?,
        incumbentProbability: decode_incumbent_probability(field(value, "incumbentProbability")?)?,
        incumbentReceiptMemberSha256: text(field(value, "incumbentReceiptMemberSha256")?)?,
        modelLeadHours: unsigned(field(value, "modelLeadHours")?)?,
        nativeSourceProbability: decode_native_probability(field(
            value,
            "nativeSourceProbability",
        )?)?,
        persistencePrediction64: text(field(value, "persistencePrediction64")?)?,
        persistenceReason: enum_value(
            field(value, "persistenceReason")?,
            AdjustmentRainGateControlProjectionRowsPersistenceReasonEnum::WIRE_VALUES,
        )?,
        persistenceTargetMemberSha256: nullable_text(field(
            value,
            "persistenceTargetMemberSha256",
        )?)?,
        rawPrecipitationMm64: text(field(value, "rawPrecipitationMm64")?)?,
        rawTargetHourTemperatureC64: text(field(value, "rawTargetHourTemperatureC64")?)?,
        recentVolumeScalePrediction64: text(field(value, "recentVolumeScalePrediction64")?)?,
        sameWindowVolumeScalePrediction64: text(field(
            value,
            "sameWindowVolumeScalePrediction64",
        )?)?,
        sourceRowSha256: text(field(value, "sourceRowSha256")?)?,
        unchangedOrdinalPrediction64: text(field(value, "unchangedOrdinalPrediction64")?)?,
        validAt: text(field(value, "validAt")?)?,
        volumeScalePrediction64: text(field(value, "volumeScalePrediction64")?)?,
    })
}

/// Decode the incumbent probability record in its generated field order.
fn decode_incumbent_probability(
    value: &JsonValue,
) -> Option<AdjustmentRainGateControlProjectionRowsIncumbentProbability> {
    Some(
        AdjustmentRainGateControlProjectionRowsIncumbentProbability {
            atLeast0_1: text(field(value, "atLeast0_1")?)?,
            atLeast1_0: text(field(value, "atLeast1_0")?)?,
            atLeast2_5: text(field(value, "atLeast2_5")?)?,
        },
    )
}

/// Decode the native-source probability record in its generated field order.
fn decode_native_probability(
    value: &JsonValue,
) -> Option<AdjustmentRainGateControlProjectionRowsNativeSourceProbability> {
    Some(
        AdjustmentRainGateControlProjectionRowsNativeSourceProbability {
            atLeast0_1: text(field(value, "atLeast0_1")?)?,
            atLeast1_0: text(field(value, "atLeast1_0")?)?,
            atLeast2_5: text(field(value, "atLeast2_5")?)?,
        },
    )
}

/// Decode the complete fixed-gauge projection after its large canonical graph is validated.
fn decode_fixed_gauge(value: &JsonValue) -> Option<AdjustmentRainFixedGaugeTargetProjection> {
    Some(AdjustmentRainFixedGaugeTargetProjection {
        captureBodies: array(field(value, "captureBodies")?)?
            .iter()
            .map(decode_capture_body)
            .collect::<Option<Vec<_>>>()?,
        contractVersion: AdjustmentRainFixedGaugeTargetProjectionContractVersionLiteral,
        family: AdjustmentRainFixedGaugeTargetProjectionFamilyLiteral,
        logicalReceivedAt: text(field(value, "logicalReceivedAt")?)?,
        projectionKind: AdjustmentRainFixedGaugeTargetProjectionProjectionKindLiteral,
        rows: array(field(value, "rows")?)?
            .iter()
            .map(decode_fixed_gauge_row)
            .collect::<Option<Vec<_>>>()?,
        validAt: text(field(value, "validAt")?)?,
    })
}

/// Decode one authenticated raw capture body and all of its claims.
fn decode_capture_body(value: &JsonValue) -> Option<AdjustmentRainFixedGaugeTargetCaptureBody> {
    Some(AdjustmentRainFixedGaugeTargetCaptureBody {
        bodyBase64: text(field(value, "bodyBase64")?)?,
        bodySha256: text(field(value, "bodySha256")?)?,
        claims: array(field(value, "claims")?)?
            .iter()
            .map(|claim| {
                Some(AdjustmentRainFixedGaugeTargetClaim {
                    claimId: text(field(claim, "claimId")?)?,
                    completedAt: text(field(claim, "completedAt")?)?,
                    stationId: unsigned(field(claim, "stationId")?)?,
                    windowEndExclusive: text(field(claim, "windowEndExclusive")?)?,
                    windowStart: text(field(claim, "windowStart")?)?,
                })
            })
            .collect::<Option<Vec<_>>>()?,
    })
}

/// Decode one fixed gauge row, preserving raw members, intervals, and normalized weather content.
fn decode_fixed_gauge_row(value: &JsonValue) -> Option<AdjustmentRainFixedGaugeTargetRow> {
    Some(AdjustmentRainFixedGaugeTargetRow {
        captureMembers: array(field(value, "captureMembers")?)?
            .iter()
            .map(|member| {
                Some(AdjustmentRainFixedGaugeTargetCaptureMember {
                    bodySha256: text(field(member, "bodySha256")?)?,
                    claimId: text(field(member, "claimId")?)?,
                    completedAt: text(field(member, "completedAt")?)?,
                })
            })
            .collect::<Option<Vec<_>>>()?,
        intervals: array(field(value, "intervals")?)?
            .iter()
            .map(decode_fixed_gauge_interval)
            .collect::<Option<Vec<_>>>()?,
        normalizedRecord: decode_normalized_record(field(value, "normalizedRecord")?)?,
        stationId: unsigned(field(value, "stationId")?)?,
        storedContentSha256: text(field(value, "storedContentSha256")?)?,
    })
}

/// Decode one exact fixed-gauge interval and retain its binary64 strings verbatim.
fn decode_fixed_gauge_interval(
    value: &JsonValue,
) -> Option<AdjustmentRainFixedGaugeTargetInterval> {
    Some(AdjustmentRainFixedGaugeTargetInterval {
        bodySha256: text(field(value, "bodySha256")?)?,
        claimId: text(field(value, "claimId")?)?,
        completedAt: text(field(value, "completedAt")?)?,
        durationMinutes: unsigned(field(value, "durationMinutes")?)?,
        precipitationMm64: text(field(value, "precipitationMm64")?)?,
        temperatureC64: nullable_text(field(value, "temperatureC64")?)?,
        validAt: text(field(value, "validAt")?)?,
    })
}

/// Decode the full normalized weather record used by the fixed-gauge target row.
fn decode_normalized_record(value: &JsonValue) -> Option<NormalizedWeatherRecord> {
    Some(NormalizedWeatherRecord {
        metadata: decode_weather_metadata(field(value, "metadata")?)?,
        metrics: decode_weather_metrics(field(value, "metrics")?)?,
        productRunAt: nullable_text(field(value, "productRunAt")?)?,
        receivedAt: text(field(value, "receivedAt")?)?,
        sourceId: text(field(value, "sourceId")?)?,
        sourceKind: enum_value(field(value, "sourceKind")?, SourceKind::WIRE_VALUES)?,
        validAt: text(field(value, "validAt")?)?,
    })
}

/// Decode complete record metadata without normalizing provider or quality object order.
fn decode_weather_metadata(value: &JsonValue) -> Option<WeatherRecordMetadata> {
    Some(WeatherRecordMetadata {
        device: match field(value, "device")? {
            JsonValue::Null => Nullable::Null,
            device => Nullable::Value(DeviceMetadata {
                model: optional_text_field(device, "model")?,
                serial: optional_text_field(device, "serial")?,
                vendor: optional_text_field(device, "vendor")?,
            }),
        },
        model: nullable_text(field(value, "model")?)?,
        provider: nullable_object(field(value, "provider")?)?,
        quality: nullable_object(field(value, "quality")?)?,
        upstreamTimezone: text(field(value, "upstreamTimezone")?)?,
    })
}

/// Decode every canonical metric with explicit null occupancy intact.
fn decode_weather_metrics(value: &JsonValue) -> Option<CanonicalWeatherMetrics> {
    Some(CanonicalWeatherMetrics {
        apparentTemperatureC: nullable_number(field(value, "apparentTemperatureC")?)?,
        blackGlobeTemperatureC: nullable_number(field(value, "blackGlobeTemperatureC")?)?,
        cloudCoverPercent: nullable_number(field(value, "cloudCoverPercent")?)?,
        pm25MicrogramsPerCubicMeter: nullable_number(field(value, "pm25MicrogramsPerCubicMeter")?)?,
        precipitationMm: nullable_number(field(value, "precipitationMm")?)?,
        precipitationRateMmPerHour: nullable_number(field(value, "precipitationRateMmPerHour")?)?,
        pressureHpa: nullable_number(field(value, "pressureHpa")?)?,
        relativeHumidityPercent: nullable_number(field(value, "relativeHumidityPercent")?)?,
        soilElectricalConductivityMicrosiemensPerCm: nullable_number(field(
            value,
            "soilElectricalConductivityMicrosiemensPerCm",
        )?)?,
        soilMoisturePercent: nullable_number(field(value, "soilMoisturePercent")?)?,
        solarRadiationWm2: nullable_number(field(value, "solarRadiationWm2")?)?,
        temperatureC: nullable_number(field(value, "temperatureC")?)?,
        uvIndex: nullable_number(field(value, "uvIndex")?)?,
        waterLevelM: nullable_number(field(value, "waterLevelM")?)?,
        windDirectionDegrees: nullable_number(field(value, "windDirectionDegrees")?)?,
        windGustMps: nullable_number(field(value, "windGustMps")?)?,
        windSpeedMps: nullable_number(field(value, "windSpeedMps")?)?,
        wetBulbGlobeTemperatureC: nullable_number(field(value, "wetBulbGlobeTemperatureC")?)?,
    })
}

/// Return the generated projection kind retained by the decoded transport variant.
fn projection_kind(
    document: &ApiMaintenanceRevisionTransportDocument,
) -> AdjustmentRevisionProjectionKind {
    match document {
        ApiMaintenanceRevisionTransportDocument::Standard(document) => match document.as_ref() {
            AdjustmentRevisionProjectionDocument::Single(value) => value.projectionKind,
            AdjustmentRevisionProjectionDocument::Batch(value) => match value.projectionKind {
                AdjustmentRevisionBatchProjectionProjectionKindEnum::ActualBestMatch => {
                    AdjustmentRevisionProjectionKind::ActualBestMatch
                }
                AdjustmentRevisionBatchProjectionProjectionKindEnum::TargetRevision => {
                    AdjustmentRevisionProjectionKind::TargetRevision
                }
            },
            AdjustmentRevisionProjectionDocument::NativeTemperature(_) => {
                AdjustmentRevisionProjectionKind::NativeSource
            }
            AdjustmentRevisionProjectionDocument::RainFeature(_)
            | AdjustmentRevisionProjectionDocument::RainControl(_) => {
                AdjustmentRevisionProjectionKind::RainGateInput
            }
        },
        ApiMaintenanceRevisionTransportDocument::FixedGauge(_) => {
            AdjustmentRevisionProjectionKind::TargetRevision
        }
    }
}

/// Copy one ordered single-projection logical key into its generated scalar domain.
fn logical_key(
    value: &JsonValue,
) -> Option<Vec<(Vec<u16>, AdjustmentRevisionProjectionLogicalKeyValue)>> {
    object(value)?
        .iter()
        .map(|(key, value)| {
            Some((
                key.clone(),
                match value {
                    JsonValue::Null => AdjustmentRevisionProjectionLogicalKeyValue::Null,
                    JsonValue::Number(value) => {
                        AdjustmentRevisionProjectionLogicalKeyValue::Number(*value)
                    }
                    JsonValue::String(value) => {
                        AdjustmentRevisionProjectionLogicalKeyValue::String(value.clone())
                    }
                    _ => return None,
                },
            ))
        })
        .collect()
}

/// Copy one ordered batch logical key into its generated scalar domain.
fn batch_logical_key(
    value: &JsonValue,
) -> Option<Vec<(Vec<u16>, AdjustmentRevisionBatchProjectionLogicalKeyValue)>> {
    object(value)?
        .iter()
        .map(|(key, value)| {
            Some((
                key.clone(),
                match value {
                    JsonValue::Null => AdjustmentRevisionBatchProjectionLogicalKeyValue::Null,
                    JsonValue::Number(value) => {
                        AdjustmentRevisionBatchProjectionLogicalKeyValue::Number(*value)
                    }
                    JsonValue::String(value) => {
                        AdjustmentRevisionBatchProjectionLogicalKeyValue::String(value.clone())
                    }
                    _ => return None,
                },
            ))
        })
        .collect()
}

type OrderedRevisionObject<Value> = Vec<(Vec<u16>, Value)>;
type OrderedRevisionRows<Value> = Vec<OrderedRevisionObject<Value>>;

/// Copy every ordered single-projection scalar row without converting source numbers.
fn scalar_rows(
    value: &JsonValue,
) -> Option<OrderedRevisionRows<AdjustmentRevisionProjectionRowsValue>> {
    array(value)?
        .iter()
        .map(|row| {
            object(row)?
                .iter()
                .map(|(key, value)| {
                    Some((
                        key.clone(),
                        match value {
                            JsonValue::Null => AdjustmentRevisionProjectionRowsValue::Null,
                            JsonValue::Boolean(value) => {
                                AdjustmentRevisionProjectionRowsValue::Boolean(*value)
                            }
                            JsonValue::Number(value) => {
                                AdjustmentRevisionProjectionRowsValue::Number(*value)
                            }
                            JsonValue::String(value) => {
                                AdjustmentRevisionProjectionRowsValue::String(value.clone())
                            }
                            _ => return None,
                        },
                    ))
                })
                .collect::<Option<Vec<_>>>()
        })
        .collect()
}

/// Copy every ordered grouped-projection scalar row without changing its values.
fn batch_scalar_rows(
    value: &JsonValue,
) -> Option<OrderedRevisionRows<AdjustmentRevisionBatchProjectionRowsValue>> {
    array(value)?
        .iter()
        .map(|row| {
            object(row)?
                .iter()
                .map(|(key, value)| {
                    Some((
                        key.clone(),
                        match value {
                            JsonValue::Null => AdjustmentRevisionBatchProjectionRowsValue::Null,
                            JsonValue::Boolean(value) => {
                                AdjustmentRevisionBatchProjectionRowsValue::Boolean(*value)
                            }
                            JsonValue::Number(value) => {
                                AdjustmentRevisionBatchProjectionRowsValue::Number(*value)
                            }
                            JsonValue::String(value) => {
                                AdjustmentRevisionBatchProjectionRowsValue::String(value.clone())
                            }
                            _ => return None,
                        },
                    ))
                })
                .collect::<Option<Vec<_>>>()
        })
        .collect()
}

/// Copy one ordered object whose values are exactly null or UTF-16 strings.
fn nullable_string_object(value: &JsonValue) -> Option<OrderedRevisionObject<Nullable<Vec<u16>>>> {
    object(value)?
        .iter()
        .map(|(key, value)| {
            Some((
                key.clone(),
                match value {
                    JsonValue::Null => Nullable::Null,
                    JsonValue::String(value) => Nullable::Value(value.clone()),
                    _ => return None,
                },
            ))
        })
        .collect()
}

/// Decode one enum from its generated wire table.
fn enum_value<Value: Copy>(value: &JsonValue, wire: &[(&str, Value)]) -> Option<Value> {
    let value = text(value)?;
    wire.iter()
        .find_map(|(candidate, decoded)| (*candidate == value).then_some(*decoded))
}

/// Read one named object member without changing its retained value.
fn field<'a>(value: &'a JsonValue, name: &str) -> Option<&'a JsonValue> {
    let name = name.encode_utf16().collect::<Vec<_>>();
    object(value)?
        .iter()
        .find_map(|(key, value)| (*key == name).then_some(value))
}

/// Borrow one JSON object after canonical admission has closed its member grammar.
fn object(value: &JsonValue) -> Option<&[(Vec<u16>, JsonValue)]> {
    match value {
        JsonValue::Object(fields) => Some(fields),
        _ => None,
    }
}

/// Borrow one JSON array after canonical admission has closed its element grammar.
fn array(value: &JsonValue) -> Option<&[JsonValue]> {
    match value {
        JsonValue::Array(values) => Some(values),
        _ => None,
    }
}

/// Decode one UTF-16 JSON string without lossy substitution.
fn text(value: &JsonValue) -> Option<String> {
    match value {
        JsonValue::String(value) => String::from_utf16(value).ok(),
        _ => None,
    }
}

/// Decode one already admitted finite number.
fn number(value: &JsonValue) -> Option<f64> {
    match value {
        JsonValue::Number(value) if value.is_finite() => Some(*value),
        _ => None,
    }
}

/// Decode one already admitted nonnegative safe integer without rounding.
fn unsigned(value: &JsonValue) -> Option<u64> {
    let value = number(value)?;
    (value >= 0.0 && value.fract() == 0.0 && value <= 9_007_199_254_740_991.0)
        .then_some(value as u64)
}

/// Decode one ordinary JSON boolean.
fn boolean(value: &JsonValue) -> Option<bool> {
    match value {
        JsonValue::Boolean(value) => Some(*value),
        _ => None,
    }
}

/// Preserve explicit null versus one finite number.
fn nullable_number(value: &JsonValue) -> Option<Nullable<f64>> {
    Some(match value {
        JsonValue::Null => Nullable::Null,
        _ => Nullable::Value(number(value)?),
    })
}

/// Preserve explicit null versus one exact UTF-16 string.
fn nullable_text(value: &JsonValue) -> Option<Nullable<String>> {
    Some(match value {
        JsonValue::Null => Nullable::Null,
        _ => Nullable::Value(text(value)?),
    })
}

/// Preserve explicit null versus one ordered arbitrary JSON object.
fn nullable_object(value: &JsonValue) -> Option<Nullable<OrderedRevisionObject<JsonValue>>> {
    Some(match value {
        JsonValue::Null => Nullable::Null,
        JsonValue::Object(fields) => Nullable::Value(fields.clone()),
        _ => return None,
    })
}

/// Preserve a missing metadata member separately from an explicitly present string.
fn optional_text_field(value: &JsonValue, name: &str) -> Option<FieldPresence<String>> {
    Some(match field(value, name) {
        Some(value) => FieldPresence::Value(text(value)?),
        None => FieldPresence::Missing,
    })
}

/// Decode one exact string array.
fn string_array(value: &JsonValue) -> Option<Vec<String>> {
    array(value)?.iter().map(text).collect()
}

/// Decode one array retaining explicit null and exact string occupancy.
fn nullable_string_array(value: &JsonValue) -> Option<Vec<Nullable<String>>> {
    array(value)?.iter().map(nullable_text).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Refuse a typed-looking document that has not passed exact canonical producer admission.
    #[test]
    fn decoder_requires_complete_canonical_projection() {
        let partial = b"{\"contractVersion\":\"adjustment-revision-projection/v1\"}\n";
        assert_eq!(decode_native_api_revision_document(partial), None);
    }
}

#[cfg(test)]
#[path = "native_api_revision_document_decode_test.rs"]
mod positive_tests;
