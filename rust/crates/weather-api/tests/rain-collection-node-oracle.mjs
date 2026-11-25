import assert from "node:assert/strict";

import { projectRainCollectionStatus } from "../../../../apps/api/dist/rain-collection.js";

const status = {
  claims: 12,
  compressedBytes: 4096,
  contractVersion: "rain-prospective-capture/v1",
  failedRequests: 1,
  lastClaimAt: "2026-09-13T01:00:00.000Z",
  lastForecastReceiptAt: "2026-09-13T01:02:00.000Z",
  lastReceiptAt: "2026-09-13T01:01:00.000Z",
  lastStationReceiptAt: "2026-09-13T01:03:00.000Z",
  modelEnabled: false,
  pausedUntil: "2026-09-14T00:00:00.000Z",
  pendingRequests: 2,
  qualificationEnabled: false,
  receipts: 11,
  stationsSeen: 4,
  timelyForecasts: 2,
  unknownRequests: 1,
  validForecasts: 3,
  validStationWindows: 7,
};

const projected = projectRainCollectionStatus(status);
assert.deepEqual(projected, {
  ...status,
  expectedStations: 12,
  expiresAt: "2027-10-08T00:00:00.000Z",
  startsAt: "2026-09-13T00:00:00.000Z",
  stationAccessAuthorized: true,
});
assert.throws(
  () => projectRainCollectionStatus({
    ...status,
    compressedBytes: Number.MAX_SAFE_INTEGER + 1,
  }),
  /invalid rain collection aggregate/u,
);
assert.throws(
  () => projectRainCollectionStatus({
    ...status,
    lastClaimAt: "2026-09-13T01:00:00Z",
  }),
  /invalid rain collection timestamp/u,
);

process.stdout.write("rain collection projection Node oracle passed\n");
