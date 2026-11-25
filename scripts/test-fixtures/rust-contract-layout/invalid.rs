use weather_contracts_generated::{
    ApiPropertySensorSnapshot, DailyPrecipitationRow, FieldPresence,
    ForecastAdjustmentBootstrapSummary,
    ForecastAdjustmentBootstrapSummaryContractVersionLiteral, ForecastAdjustmentRainThresholdSummary,
    ForecastAdjustmentRainThresholdSummaryRawBrierNull,
    ForecastAdjustmentRainThresholdSummaryThresholdMmPerHourEnum, ForecastDays,
    InstalledMaintenanceShadowReceiptV1, InstalledMaintenanceShadowReceiptV2,
    InstalledMaintenanceShadowReceiptV2ActionKindEnum,
    InstalledMaintenanceShadowReceiptV2ContractVersionLiteral,
    InstalledMaintenanceShadowReceiptV2PolicyDecisionEnum, InstalledRainMaintenanceControlReference,
    JsonValue, Nullable, OrderedFiniteNumberRecord,
};

// rejects a numeric air quality payload
pub fn invalid_air_quality() {
    let _ = ApiPropertySensorSnapshot {
        airQuality: FieldPresence::Value(Nullable::Value(7_u64)), // mismatch:invalid-air-quality
        channel: Nullable::Null,
        key: String::new(),
        model: String::new(),
        readings: OrderedFiniteNumberRecord(Vec::new()),
    };
}

// rejects an omitted required channel
pub fn missing_channel() {
    let _ = ApiPropertySensorSnapshot { airQuality: FieldPresence::Missing, key: String::new(), model: String::new(), readings: OrderedFiniteNumberRecord(Vec::new()) }; // mismatch:missing-channel
}

// rejects an unsupported forecast day
pub fn invalid_forecast_days() {
    let _ = ForecastDays::Value7; // mismatch:invalid-forecast-days
}

// rejects a non-marker bootstrap count
pub fn invalid_bootstrap_count() {
    let _ = ForecastAdjustmentBootstrapSummary {
        contractVersion: ForecastAdjustmentBootstrapSummaryContractVersionLiteral,
        lowerSkill: 0.0,
        replicates: 2_001_u64, // mismatch:invalid-bootstrap-count
        upperSkill: 1.0,
    };
}

// rejects a numeric raw brier baseline
pub fn invalid_raw_brier() {
    let _ = ForecastAdjustmentRainThresholdSummary {
        adjustedBrier: 0.0,
        csi: Nullable::Null,
        falseAlarms: 0,
        far: Nullable::Null,
        hits: 0,
        misses: 0,
        pod: Nullable::Null,
        rawBrier: 0.0_f64, // mismatch:invalid-raw-brier
        reliability: Vec::new(),
        thresholdMmPerHour:
            ForecastAdjustmentRainThresholdSummaryThresholdMmPerHourEnum::Value0Point1,
    };
}

// rejects an unsupported rain threshold
pub fn invalid_rain_threshold() {
    let _ = ForecastAdjustmentRainThresholdSummary {
        adjustedBrier: 0.0,
        csi: Nullable::Null,
        falseAlarms: 0,
        far: Nullable::Null,
        hits: 0,
        misses: 0,
        pod: Nullable::Null,
        rawBrier: ForecastAdjustmentRainThresholdSummaryRawBrierNull,
        reliability: Vec::new(),
        thresholdMmPerHour: ForecastAdjustmentRainThresholdSummaryThresholdMmPerHourEnum::Value2, // mismatch:invalid-rain-threshold
    };
}

// rejects a string control byte buffer
pub fn invalid_control_bytes() {
    let _ = InstalledRainMaintenanceControlReference {
        controlStateBytes: String::new(), // mismatch:invalid-control-bytes
        controlStateSha256: String::new(),
        ordinalArtifactBytes: Vec::new(),
        ordinalArtifactSha256: String::new(),
    };
}

// rejects a non-timestamp precipitation value
pub fn invalid_precipitation_timestamp() {
    let _ = DailyPrecipitationRow {
        accumulationMm: 0.0,
        sourceId: String::new(),
        stationSlug: String::new(),
        validThrough: true, // mismatch:invalid-precipitation-timestamp
    };
}

// rejects a nonexistent json member variant
pub fn invalid_json_member() {
    let _ = JsonValue::BigInt(1_i64); // mismatch:invalid-json-member
}

// rejects a plain unsupported receipt version
pub fn invalid_install_receipt_version() {
    let _ = InstalledMaintenanceShadowReceiptV1 {
        actionSha256: String::new(),
        bundleSha256: String::new(),
        candidateGraphSha256: String::new(),
        candidateSha256: String::new(),
        contractVersion: String::from("adjustment-installed-candidate-receipt/v3"), // mismatch:invalid-install-receipt-version
        controlPlaneSha256: String::new(),
        deployedCommit: String::new(),
        deployedImageDigest: String::new(),
        deployedRelease: String::new(),
        deployedSettingsSha256: String::new(),
        fencingToken: String::new(),
        installedAt: String::new(),
        registrationSha256: String::new(),
        sourceSha256: String::new(),
    };
}

// rejects an omitted v2 member root
pub fn missing_receipt_member_root() {
    let _ = InstalledMaintenanceShadowReceiptV2 { // mismatch:missing-receipt-member-root
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
        lifecycleHeadSha256: String::new(),
        policyDecision: InstalledMaintenanceShadowReceiptV2PolicyDecisionEnum::Pending,
        policyReportSha256: String::new(),
    };
}
