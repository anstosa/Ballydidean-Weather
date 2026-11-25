import {
  RAIN_COLLECTION_POLICY,
  RAIN_COLLECTION_STATIONS,
  type RainCollectionStatus,
} from "@weather/domain";

// project only bounded aggregate fields and never raw provider evidence
export function projectRainCollectionStatus(status: RainCollectionStatus) {
  const counts = {
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
  };
  const timestamps = {
    lastClaimAt: status.lastClaimAt,
    lastReceiptAt: status.lastReceiptAt,
    lastForecastReceiptAt: status.lastForecastReceiptAt,
    lastStationReceiptAt: status.lastStationReceiptAt,
    pausedUntil: status.pausedUntil,
  };

  // reject malformed database aggregates instead of reporting collection success
  for (const value of Object.values(counts)) {
    // reject impossible counts
    if (!Number.isSafeInteger(value) || value < 0) {
      throw new Error("invalid rain collection aggregate");
    }
  }
  // accept only actual canonical receipt timestamps or explicit missing values
  for (const value of Object.values(timestamps)) {
    // keep missing evidence explicit
    if (value !== null && (!Number.isFinite(Date.parse(value)) || new Date(value).toISOString() !== value)) {
      throw new Error("invalid rain collection timestamp");
    }
  }

  return {
    contractVersion: RAIN_COLLECTION_POLICY.contractVersion,
    modelEnabled: false,
    qualificationEnabled: false as const,
    stationAccessAuthorized: RAIN_COLLECTION_POLICY.stationAccessAuthorized,
    expectedStations: RAIN_COLLECTION_STATIONS.length,
    startsAt: RAIN_COLLECTION_POLICY.startsAt,
    expiresAt: RAIN_COLLECTION_POLICY.expiresAt,
    ...counts,
    ...timestamps,
  };
}
