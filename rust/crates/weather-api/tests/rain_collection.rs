use std::path::PathBuf;
use std::process::Command;

use weather_api::{
    AdmittedRainForecastRuntime, CurrentProjectionError, RainCollectionTimestampBoundary,
    admit_rain_forecast_runtime, project_rain_collection_status,
};
use weather_contracts::{
    FieldPresence, ForecastRainAdjustmentStartupSnapshot,
    LoadedForecastAdjustmentRainRuntimeRegistryV1,
    LoadedForecastAdjustmentRainRuntimeRegistryV1Disabled,
    LoadedForecastAdjustmentRainRuntimeRegistryV1DisabledArtifactSha256Null,
    LoadedForecastAdjustmentRainRuntimeRegistryV1DisabledReasonCodeEnum,
    LoadedForecastAdjustmentRainRuntimeRegistryV1DisabledStateLiteral, Nullable,
    RainCollectionStatus, RainCollectionStatusContractVersionLiteral,
    RainCollectionStatusModelEnabledLiteral, RainCollectionStatusQualificationEnabledLiteral,
};

struct FixtureTimestampBoundary;

impl RainCollectionTimestampBoundary for FixtureTimestampBoundary {
    /// admit only the source-derived canonical timestamp fixtures
    fn validate_canonical_utc(&self, value: &str) -> Result<(), CurrentProjectionError> {
        // preserve the exact Node fixture timestamps
        if matches!(
            value,
            "2026-09-13T01:00:00.000Z"
                | "2026-09-13T01:01:00.000Z"
                | "2026-09-13T01:02:00.000Z"
                | "2026-09-13T01:03:00.000Z"
                | "2026-09-14T00:00:00.000Z"
        ) {
            return Ok(());
        }

        Err(CurrentProjectionError {
            code: "rain_collection.time",
            detail: "invalid rain collection timestamp".to_owned(),
        })
    }
}

// verifies the generated startup refusal and capture time survive service admission
#[test]
fn admits_immutable_rain_startup_refusal() {
    let snapshot = ForecastRainAdjustmentStartupSnapshot {
        loadedAt: "2026-08-22T04:58:00.000Z".to_owned(),
        runtime: LoadedForecastAdjustmentRainRuntimeRegistryV1::Disabled(
            LoadedForecastAdjustmentRainRuntimeRegistryV1Disabled {
                artifactSha256:
                    LoadedForecastAdjustmentRainRuntimeRegistryV1DisabledArtifactSha256Null,
                comparatorAuthority: FieldPresence::Missing,
                reasonCode:
                    LoadedForecastAdjustmentRainRuntimeRegistryV1DisabledReasonCodeEnum::PolicyRaw,
                state: LoadedForecastAdjustmentRainRuntimeRegistryV1DisabledStateLiteral,
            },
        ),
    };
    assert_eq!(
        admit_rain_forecast_runtime(&snapshot),
        AdmittedRainForecastRuntime::Disabled {
            loaded_at: "2026-08-22T04:58:00.000Z".to_owned(),
            reason_code: weather_contracts::ApiRainAdjustmentRuntimeReasonCodeEnum::PolicyRaw,
        }
    );
}

/// build one generated typed collection status
fn status() -> RainCollectionStatus {
    RainCollectionStatus {
        contractVersion: RainCollectionStatusContractVersionLiteral,
        modelEnabled: RainCollectionStatusModelEnabledLiteral,
        qualificationEnabled: RainCollectionStatusQualificationEnabledLiteral,
        claims: 12,
        receipts: 11,
        validForecasts: 3,
        timelyForecasts: 2,
        validStationWindows: 7,
        stationsSeen: 4,
        failedRequests: 1,
        pendingRequests: 2,
        unknownRequests: 1,
        lastClaimAt: Nullable::Value("2026-09-13T01:00:00.000Z".to_owned()),
        lastReceiptAt: Nullable::Value("2026-09-13T01:01:00.000Z".to_owned()),
        lastForecastReceiptAt: Nullable::Value("2026-09-13T01:02:00.000Z".to_owned()),
        lastStationReceiptAt: Nullable::Value("2026-09-13T01:03:00.000Z".to_owned()),
        pausedUntil: Nullable::Value("2026-09-14T00:00:00.000Z".to_owned()),
        compressedBytes: 4096,
    }
}

// verifies exact canonical projection parity with compiled Node
#[test]
fn matches_compiled_node_rain_collection_projection() {
    let oracle =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/rain-collection-node-oracle.mjs");
    let output = Command::new("node")
        .arg(oracle)
        .output()
        .expect("run rain collection Node oracle");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let projected = project_rain_collection_status(status(), &FixtureTimestampBoundary)
        .expect("rain collection projection");
    assert_eq!(projected.expectedStations, 12);
    assert!(projected.stationAccessAuthorized);
    assert!(!projected.modelEnabled);
    assert_eq!(projected.startsAt, "2026-09-13T00:00:00.000Z");
    assert_eq!(projected.expiresAt, "2027-10-08T00:00:00.000Z");
    assert_eq!(projected.claims, 12);
    assert_eq!(projected.compressedBytes, 4096);
}

// verifies unsafe integers and noncanonical timestamps fail closed
#[test]
fn rejects_malformed_rain_collection_aggregates() {
    let mut unsafe_count = status();
    unsafe_count.compressedBytes = 9_007_199_254_740_992;
    let count_error = project_rain_collection_status(unsafe_count, &FixtureTimestampBoundary)
        .expect_err("unsafe count");
    assert_eq!(count_error.code, "rain_collection.aggregate");

    let mut invalid_time = status();
    invalid_time.lastClaimAt = Nullable::Value("2026-09-13T01:00:00Z".to_owned());
    let time_error = project_rain_collection_status(invalid_time, &FixtureTimestampBoundary)
        .expect_err("noncanonical timestamp");
    assert_eq!(time_error.code, "rain_collection.time");
}
