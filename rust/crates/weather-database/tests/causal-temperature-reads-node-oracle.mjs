import assert from "node:assert/strict";

import {
  listCausalEcmwfTemperatureCanaryPriorHours,
  listCausalForecastObservationHourlyStations,
} from "../../../../packages/database/dist/index.js";

const from = "2026-09-04T18:00:00.000Z";
const observationTo = "2026-09-07T18:00:00.000Z";
const priorToInclusive = "2026-09-07T17:00:00.000Z";
const targetRunInitializedAt = "2026-09-08T00:00:00.000Z";
const captured = [];
const pool = {
  // Capture both exact statements and return one representative prior forecast.
  async query(text, values) {
    captured.push({ text, values });
    if (text.includes("ecmwf_temperature_canary_hours")) {
      return {
        rows: [{
          contentHash: "a".repeat(64),
          modelLeadHours: 11,
          rawRelativeHumidityPercent: null,
          rawTemperatureC: 10.25,
          rawWindSpeedMps: 3.5,
          runInitializedAt: "2026-09-07T06:00:00.000Z",
          validAt: priorToInclusive,
        }],
      };
    }
    return { rows: [] };
  },
};

const observations = await listCausalForecastObservationHourlyStations(pool, {
  asOf: targetRunInitializedAt,
  from,
  siteSlug: "ballydidean",
  to: observationTo,
});
const priors = await listCausalEcmwfTemperatureCanaryPriorHours(pool, {
  from,
  siteSlug: "ballydidean",
  targetRunInitializedAt,
  toInclusive: priorToInclusive,
});
assert.equal(observations.length, 72 * 11);
assert.equal(priors.length, 1);

// Collapse irrelevant whitespace while retaining every SQL token and predicate.
function normalizeSql(text) {
  return text.trim().split(/\s+/u).join(" ");
}

// Encode scalar and source-key-array binds without JSON serialization.
function renderBinds(values) {
  return values.map((value) => Array.isArray(value) ? value.join(",") : value).join("|");
}

const first = observations[0];
const last = observations.at(-1);
const prior = priors[0];
console.log(`observation-sql=${normalizeSql(captured[0].text)}`);
console.log(`observation-binds=${renderBinds(captured[0].values)}`);
console.log([
  "observation-rows",
  observations.length,
  first.physicalStationKey,
  first.metrics.temperatureC ?? "null",
  first.validAt,
  last.physicalStationKey,
  last.metrics.temperatureC ?? "null",
  last.validAt,
].join("="));
console.log(`prior-sql=${normalizeSql(captured[1].text)}`);
console.log(`prior-binds=${renderBinds(captured[1].values)}`);
console.log([
  "prior-row",
  prior.key,
  prior.contentHash,
  prior.modelLeadHours,
  prior.rawRelativeHumidityPercent ?? "null",
  prior.rawTemperatureC,
  prior.rawWindSpeedMps ?? "null",
  prior.runInitializedAt,
  prior.validAt,
].join("="));
console.log([
  "epochs",
  Date.parse(from),
  Date.parse(observationTo),
  Date.parse(priorToInclusive),
].join("="));
