use weather_contracts_generated::{
    ApiAirQualityHourlyContext, ApiPropertySensorSnapshot, DailyPrecipitationRow, FieldPresence,
    ForecastAdjustmentBootstrapSummary, ForecastAdjustmentBootstrapSummaryContractVersionLiteral,
    ForecastAdjustmentBootstrapSummaryReplicatesLiteral, ForecastAdjustmentReliabilityBin,
    ForecastAdjustmentRainThresholdSummary, ForecastAdjustmentRainThresholdSummaryRawBrierNull,
    ForecastAdjustmentRainThresholdSummaryThresholdMmPerHourEnum, ForecastDays,
    InstalledMaintenanceShadowReceipt, InstalledMaintenanceShadowReceiptV1,
    InstalledMaintenanceShadowReceiptV1ContractVersionLiteral,
    InstalledMaintenanceShadowReceiptV2, InstalledMaintenanceShadowReceiptV2ActionKindEnum,
    InstalledMaintenanceShadowReceiptV2ContractVersionLiteral,
    InstalledMaintenanceShadowReceiptV2PolicyDecisionEnum, InstalledRainMaintenanceControlReference,
    JsDateOrString, JsonValue, Nullable, OrderedFiniteNumberRecord,
};

// proves the exact forecast day mapping
const _: () = {
    assert!(ForecastDays::WIRE_VALUES.len() == 3);
    assert!(ForecastDays::WIRE_VALUES[0].0 == 1);
    assert!(ForecastDays::WIRE_VALUES[1].0 == 5);
    assert!(ForecastDays::WIRE_VALUES[2].0 == 10);
};

// proves the exact bootstrap count mapping
const _: () = {
    assert!(ForecastAdjustmentBootstrapSummaryReplicatesLiteral::WIRE_VALUE == 2_000);
};

// proves the exact rain threshold bit mappings
const _: () = {
    assert!(ForecastAdjustmentRainThresholdSummaryThresholdMmPerHourEnum::WIRE_VALUES.len() == 3);
    assert!(
        ForecastAdjustmentRainThresholdSummaryThresholdMmPerHourEnum::WIRE_VALUES[0]
            .0
            .to_bits()
            == 0x3fb999999999999a
    );
    assert!(
        ForecastAdjustmentRainThresholdSummaryThresholdMmPerHourEnum::WIRE_VALUES[1]
            .0
            .to_bits()
            == 0x3ff0000000000000
    );
    assert!(
        ForecastAdjustmentRainThresholdSummaryThresholdMmPerHourEnum::WIRE_VALUES[2]
            .0
            .to_bits()
            == 0x4004000000000000
    );
};

pub const BOOTSTRAP_CONTRACT_VERSION: &str =
    ForecastAdjustmentBootstrapSummaryContractVersionLiteral::WIRE_VALUE;

// constructs each accepted air quality presence shape
pub fn sensor_snapshots() -> [ApiPropertySensorSnapshot; 3] {
    [
        ApiPropertySensorSnapshot {
            airQuality: FieldPresence::Missing,
            channel: Nullable::Null,
            key: String::new(),
            model: String::new(),
            readings: OrderedFiniteNumberRecord(Vec::new()),
        },
        ApiPropertySensorSnapshot {
            airQuality: FieldPresence::Value(Nullable::Null),
            channel: Nullable::Value(0),
            key: String::new(),
            model: String::new(),
            readings: OrderedFiniteNumberRecord(Vec::new()),
        },
        ApiPropertySensorSnapshot {
            airQuality: FieldPresence::Value(Nullable::Value(ApiAirQualityHourlyContext {
                hourlyPm25: vec![Nullable::Null, Nullable::Value(12.5)],
                through: String::new(),
            })),
            channel: Nullable::Value(7),
            key: String::new(),
            model: String::new(),
            readings: OrderedFiniteNumberRecord(Vec::new()),
        },
    ]
}

// constructs every accepted forecast day literal
pub fn forecast_days() -> [ForecastDays; 3] {
    [
        ForecastDays::Value1,
        ForecastDays::Value5,
        ForecastDays::Value10,
    ]
}

// constructs the exact bootstrap marker pair
pub fn bootstrap_summary() -> ForecastAdjustmentBootstrapSummary {
    ForecastAdjustmentBootstrapSummary {
        contractVersion: ForecastAdjustmentBootstrapSummaryContractVersionLiteral,
        lowerSkill: 0.0,
        replicates: ForecastAdjustmentBootstrapSummaryReplicatesLiteral,
        upperSkill: 1.0,
    }
}

// constructs one rain threshold summary
pub fn rain_threshold_summary(
    threshold: ForecastAdjustmentRainThresholdSummaryThresholdMmPerHourEnum,
) -> ForecastAdjustmentRainThresholdSummary {
    ForecastAdjustmentRainThresholdSummary {
        adjustedBrier: 0.0,
        csi: Nullable::Null,
        falseAlarms: 0,
        far: Nullable::Null,
        hits: 1,
        misses: 0,
        pod: Nullable::Value(1.0),
        rawBrier: ForecastAdjustmentRainThresholdSummaryRawBrierNull,
        reliability: vec![ForecastAdjustmentReliabilityBin {
            count: 1,
            meanProbability: Nullable::Value(0.5),
            observedFrequency: Nullable::Value(1.0),
        }],
        thresholdMmPerHour: threshold,
    }
}

// constructs every accepted rain threshold literal
pub fn rain_threshold_summaries() -> [ForecastAdjustmentRainThresholdSummary; 3] {
    [
        rain_threshold_summary(
            ForecastAdjustmentRainThresholdSummaryThresholdMmPerHourEnum::Value0Point1,
        ),
        rain_threshold_summary(
            ForecastAdjustmentRainThresholdSummaryThresholdMmPerHourEnum::Value1,
        ),
        rain_threshold_summary(
            ForecastAdjustmentRainThresholdSummaryThresholdMmPerHourEnum::Value2Point5,
        ),
    ]
}

// constructs both installed control byte buffers
pub fn installed_control_reference() -> InstalledRainMaintenanceControlReference {
    InstalledRainMaintenanceControlReference {
        controlStateBytes: vec![0, 255],
        controlStateSha256: String::new(),
        ordinalArtifactBytes: vec![1, 254],
        ordinalArtifactSha256: String::new(),
    }
}

// constructs each supported precipitation timestamp storage shape
pub fn daily_precipitation_rows() -> [DailyPrecipitationRow; 3] {
    [
        DailyPrecipitationRow {
            accumulationMm: 0.0,
            sourceId: String::new(),
            stationSlug: String::new(),
            validThrough: JsDateOrString::DateEpochMilliseconds(0),
        },
        DailyPrecipitationRow {
            accumulationMm: 0.0,
            sourceId: String::new(),
            stationSlug: String::new(),
            validThrough: JsDateOrString::InvalidDate,
        },
        DailyPrecipitationRow {
            accumulationMm: 0.0,
            sourceId: String::new(),
            stationSlug: String::new(),
            validThrough: JsDateOrString::String(vec![0xd800]),
        },
    ]
}

// constructs recursive json with raw utf16 keys and strings
pub fn recursive_json_value() -> JsonValue {
    JsonValue::Object(vec![(
        vec![0x006b, 0xd800],
        JsonValue::Array(vec![
            JsonValue::String(vec![0xd800]),
            JsonValue::Object(vec![(vec![0x006e], JsonValue::Null)]),
        ]),
    )])
}

// constructs the installed receipt v1 payload
pub fn installed_receipt_v1() -> InstalledMaintenanceShadowReceiptV1 {
    InstalledMaintenanceShadowReceiptV1 {
        actionSha256: String::new(),
        bundleSha256: String::new(),
        candidateGraphSha256: String::new(),
        candidateSha256: String::new(),
        contractVersion: InstalledMaintenanceShadowReceiptV1ContractVersionLiteral,
        controlPlaneSha256: String::new(),
        deployedCommit: String::new(),
        deployedImageDigest: String::new(),
        deployedRelease: String::new(),
        deployedSettingsSha256: String::new(),
        fencingToken: String::new(),
        installedAt: String::new(),
        registrationSha256: String::new(),
        sourceSha256: String::new(),
    }
}

// constructs the installed receipt v2 payload
pub fn installed_receipt_v2(
    full_member_root_sha256: Nullable<String>,
) -> InstalledMaintenanceShadowReceiptV2 {
    InstalledMaintenanceShadowReceiptV2 {
        actionSha256: String::new(),
        bundleSha256: String::new(),
        candidateGraphSha256: String::new(),
        candidateSha256: String::new(),
        controlPlaneSha256: String::new(),
        deployedCommit: String::new(),
        deployedImageDigest: String::new(),
        deployedRelease: String::new(),
        deployedSettingsSha256: String::new(),
        fencingToken: String::new(),
        installedAt: String::new(),
        registrationSha256: String::new(),
        sourceSha256: String::new(),
        actionKind: InstalledMaintenanceShadowReceiptV2ActionKindEnum::Shadow,
        contractVersion: InstalledMaintenanceShadowReceiptV2ContractVersionLiteral,
        fullMemberRootSha256: full_member_root_sha256,
        lifecycleHeadSha256: String::new(),
        policyDecision: InstalledMaintenanceShadowReceiptV2PolicyDecisionEnum::Pending,
        policyReportSha256: String::new(),
    }
}

// constructs both receipt arms and both v2 member root states
pub fn installed_receipts() -> Vec<InstalledMaintenanceShadowReceipt> {
    vec![
        InstalledMaintenanceShadowReceipt::V1(installed_receipt_v1()),
        InstalledMaintenanceShadowReceipt::V2(installed_receipt_v2(Nullable::Null)),
        InstalledMaintenanceShadowReceipt::V2(installed_receipt_v2(Nullable::Value(
            String::new(),
        ))),
    ]
}
