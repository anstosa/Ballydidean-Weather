use weather_contracts::{
    ApiRainCollectionResponseData, ApiRainCollectionResponseDataContractVersionLiteral,
    ApiRainCollectionResponseDataQualificationEnabledLiteral, Nullable, RainCollectionStatus,
};

use crate::CurrentProjectionError;

const MAX_SAFE_UNSIGNED_INTEGER: u64 = 9_007_199_254_740_991;
const EXPECTED_STATIONS: u64 = 12;
const COLLECTION_STARTS_AT: &str = "2026-09-13T00:00:00.000Z";
const COLLECTION_EXPIRES_AT: &str = "2027-10-08T00:00:00.000Z";

/// admitted canonical UTC validation for retained collection timestamps
pub trait RainCollectionTimestampBoundary {
    /// validate one already decoded canonical UTC string without normalizing it
    fn validate_canonical_utc(&self, value: &str) -> Result<(), CurrentProjectionError>;
}

/// project bounded aggregate collection evidence without raw provider data
pub fn project_rain_collection_status(
    status: RainCollectionStatus,
    boundary: &dyn RainCollectionTimestampBoundary,
) -> Result<ApiRainCollectionResponseData, CurrentProjectionError> {
    let counts = [
        status.claims,
        status.receipts,
        status.validForecasts,
        status.timelyForecasts,
        status.validStationWindows,
        status.stationsSeen,
        status.failedRequests,
        status.pendingRequests,
        status.unknownRequests,
        status.compressedBytes,
    ];

    // retain only JavaScript-safe nonnegative aggregate integers
    if counts
        .into_iter()
        .any(|value| value > MAX_SAFE_UNSIGNED_INTEGER)
    {
        return Err(CurrentProjectionError {
            code: "rain_collection.aggregate",
            detail: "invalid rain collection aggregate".to_owned(),
        });
    }

    // validate every present timestamp through the admitted shared boundary
    for timestamp in [
        &status.lastClaimAt,
        &status.lastReceiptAt,
        &status.lastForecastReceiptAt,
        &status.lastStationReceiptAt,
        &status.pausedUntil,
    ] {
        // keep missing evidence explicit
        if let Nullable::Value(value) = timestamp {
            boundary.validate_canonical_utc(value)?;
        }
    }

    Ok(ApiRainCollectionResponseData {
        contractVersion: ApiRainCollectionResponseDataContractVersionLiteral,
        modelEnabled: false,
        qualificationEnabled: ApiRainCollectionResponseDataQualificationEnabledLiteral,
        stationAccessAuthorized: true,
        expectedStations: EXPECTED_STATIONS,
        startsAt: COLLECTION_STARTS_AT.to_owned(),
        expiresAt: COLLECTION_EXPIRES_AT.to_owned(),
        claims: status.claims,
        receipts: status.receipts,
        validForecasts: status.validForecasts,
        timelyForecasts: status.timelyForecasts,
        validStationWindows: status.validStationWindows,
        stationsSeen: status.stationsSeen,
        failedRequests: status.failedRequests,
        pendingRequests: status.pendingRequests,
        unknownRequests: status.unknownRequests,
        compressedBytes: status.compressedBytes,
        lastClaimAt: status.lastClaimAt,
        lastReceiptAt: status.lastReceiptAt,
        lastForecastReceiptAt: status.lastForecastReceiptAt,
        lastStationReceiptAt: status.lastStationReceiptAt,
        pausedUntil: status.pausedUntil,
    })
}
