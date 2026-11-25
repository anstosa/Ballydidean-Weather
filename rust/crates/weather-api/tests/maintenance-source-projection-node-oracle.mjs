import assert from "node:assert/strict";

import {
  buildForecastMaintenanceSourceProjection, buildTemperatureMaintenanceSourceProjection,
} from "../../../../apps/api/dist/index.js";
import { createMaintenanceShadowSourceIdentity } from "../../../../packages/forecast-adjustment/dist/index.js";

const registrationSha256 = "a".repeat(64);
const candidateSha256 = "b".repeat(64);
const sourceSha256 = "c".repeat(64);
const dueKey = "capture/2026-09-01T00:35:00.000Z";
const issuedAt = "2026-09-01T00:35:00.000Z";
const referenceAt = "2026-09-01T00:00:00.000Z";
const receivedAt = "2026-09-01T00:05:00.000Z";
const windFieldOrder = [
  "validAt", "leadHours", "modelLeadHours", "referenceAt", "receivedAt", "sourceSha256",
  "contentSha256", "adapterVersion", "contractEpoch", "dataset", "providerKey", "sourceKey",
  "sourceConfigFingerprint", "upstreamModel", "revisionCount", "windSpeedMps64", "windGustMps64",
];
const temperatureFieldOrder = [
  "validAt", "leadHours", "modelLeadHours", "referenceAt", "receivedAt", "sourceSha256",
  "contentSha256", "adapterVersion", "dataset", "providerKey", "providerResponseSha256", "modelCycle",
  "upstreamModel", "rawTemperatureC64", "rawRelativeHumidityPercent64", "rawWindSpeedMps64",
  "bestMatchContentSha256", "bestMatchProductRunAt", "bestMatchSourceId", "bestMatchTemperatureC64",
];

// build one exact hourly UTC instant without locale or host-time dependencies
function validAt(leadHours) {
  const day = 1 + Math.floor(leadHours / 24);
  const hour = leadHours % 24;
  return `2026-09-${String(day).padStart(2, "0")}T${String(hour).padStart(2, "0")}:00:00.000Z`;
}

// build the complete 168-hour Best Match source expected by the Node projection owner
const rows = Array.from({ length: 168 }, (_, index) => {
  const leadHours = index + 1;
  return {
    adapterVersion: "open-meteo-forecast-daily/v4",
    contentHash: leadHours.toString(16).padStart(64, "0"),
    contractEpoch: "legacy-v4/fixture",
    firstReceivedAt: receivedAt,
    productRunAt: referenceAt,
    providerKey: "open-meteo",
    providerMetadata: { dataset: "best_match" },
    revisionCount: index,
    siteSlug: "ballydidean",
    sourceConfigFingerprint: "d".repeat(64),
    sourceId: "12",
    sourceKey: "open-meteo-forecast-v4",
    stationSlug: "open-meteo-virtual",
    upstreamModel: "best_match",
    validAt: validAt(leadHours),
    windGustMps: leadHours % 24 === 0 ? null : 2 + leadHours / 100,
    windSpeedMps: 1 + leadHours / 100,
  };
});

const temperature = process.argv[2] === "temperature";
const registration = { candidateSha256, registrationSha256, sourceSha256 };
// exercise the original independent temperature builder with a reverse-ordered comparator
const source = temperature
  ? buildTemperatureMaintenanceSourceProjection({
    bestMatchRows: rows.slice(0, 12).map(
      // preserve negative zero and prove time-key matching instead of positional pairing
      (row, index) => ({ ...row, temperatureC: index === 0 ? -0 : 10 + index / 4 }),
    ).reverse(),
    dueKey,
    issuedAt,
    registration,
    sidecar: {
      hours: rows.slice(0, 12).map(
        // retain the exact causal source order independently of the comparator
        (row, index) => ({
          contentHash: (index + 256).toString(16).padStart(64, "0"),
          modelLeadHours: index + 1,
          rawRelativeHumidityPercent: index % 4 === 0 ? null : 70 + index,
          rawTemperatureC: 11 + index / 4,
          rawWindSpeedMps: index % 3 === 0 ? null : 2,
          validAt: row.validAt,
        }),
      ),
      run: {
        adapterVersion: "open-meteo-ecmwf-single-run/v1",
        firstReceivedAt: receivedAt,
        modelCycle: "49r1",
        providerResponseSha256: "e".repeat(64),
        recentErrorState: {
          b24C: null, b72C: null, cohort: "ecmwf_single_run_hindcast",
          localDates: 0, mad72C: null, maximumSourceRunInitializedAt: null,
          maximumSourceValidAt: null, n24: 0, n72: 0, sourceKeys: [],
          supported: false, targetRunInitializedAt: referenceAt, windowEndValidAt: referenceAt,
        },
        runInitializedAt: referenceAt,
        upstreamModel: "ecmwf_ifs",
      },
    },
  })
  : buildForecastMaintenanceSourceProjection({ dueKey, family: "wind", issuedAt, registration, rows });
const projection = JSON.parse(source.toString("utf8"));
const identity = createMaintenanceShadowSourceIdentity(source);

const rowCount = temperature ? 12 : 168;
const fieldOrder = temperature ? temperatureFieldOrder : windFieldOrder;
assert.equal(projection.rowCount, rowCount);
assert.equal(projection.rows.length, rowCount);
assert.deepEqual(Object.keys(projection.rows[0]), fieldOrder);
assert.deepEqual(Object.keys(projection.rows.at(-1)), fieldOrder);
assert.equal(projection.rows[0].validAt, "2026-09-01T01:00:00.000Z");
assert.equal(projection.rows.at(-1).validAt, validAt(rowCount));
assert.equal(identity.sourceRowSha256.length, rowCount);

// expose exact canonical source, row and identity witnesses without another JSON codec
process.stdout.write(`source=${source.toString("hex")}\n`);
for (const [index, row] of projection.rows.entries()) {
  process.stdout.write(`row:${String(index)}=${Buffer.from(JSON.stringify(row)).toString("hex")}\n`);
}
process.stdout.write(`input=${identity.inputSha256}\n`);
process.stdout.write(`receipt=${identity.sourceReceiptSha256}\n`);
for (const [index, digest] of identity.sourceRowSha256.entries()) {
  process.stdout.write(`row-sha:${String(index)}=${digest}\n`);
}
