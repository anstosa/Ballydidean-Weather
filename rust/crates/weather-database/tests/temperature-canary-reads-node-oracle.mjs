import assert from "node:assert/strict";

import {
  getEcmwfTemperatureCanarySidecar,
  getEcmwfTemperatureCanaryStatus,
} from "../../../../packages/database/dist/index.js";

const runInitializedAt = "2026-09-07T12:00:00.000Z";
const sidecarAsOf = "2026-09-07T18:03:00.000Z";
const statusAsOf = "2026-09-08T00:03:00.000Z";
const from = "2026-09-07T12:00:00.000Z";
const to = "2026-09-09T00:00:00.000Z";
const run = {
  adapterVersion: "open-meteo-ecmwf-single-run/v1",
  contentHash: "b".repeat(64),
  firstReceivedAt: "2026-09-07T17:59:00.000Z",
  id: "run-1",
  modelCycle: "50r1",
  providerResponseSha256: "a".repeat(64),
  recentErrorState: {
    b24C: null,
    b72C: null,
    cohort: "ecmwf_single_run_hindcast",
    localDates: 0,
    mad72C: null,
    maximumSourceRunInitializedAt: null,
    maximumSourceValidAt: null,
    n24: 0,
    n72: 0,
    sourceKeys: [],
    supported: false,
    targetRunInitializedAt: runInitializedAt,
    windowEndValidAt: "2026-09-07T05:00:00.000Z",
  },
  recentErrorStateSha256: "c".repeat(64),
  runInitializedAt,
  stateReason: "no_causal_forecast_observation_pairs",
  stateStatus: "cold",
  upstreamModel: "ecmwf_ifs",
};
const hours = [
  {
    contentHash: "d".repeat(64),
    modelLeadHours: 7,
    rawRelativeHumidityPercent: null,
    rawTemperatureC: 10.5,
    rawWindSpeedMps: 3.5,
    validAt: "2026-09-07T19:00:00.000Z",
  },
  {
    contentHash: "e".repeat(64),
    modelLeadHours: 8,
    rawRelativeHumidityPercent: 80,
    rawTemperatureC: 11,
    rawWindSpeedMps: null,
    validAt: "2026-09-07T20:00:00.000Z",
  },
];
const captured = [];
const pool = {
  // Return rows for the three exact production statements.
  async query(text, values) {
    captured.push({ text, values });
    // Status deliberately has no six-hour run cutoff.
    if (text.includes("count(hour.run_id)::integer")) {
      return { rows: [{ ...run, hourCount: "18" }] };
    }
    // Hours remain scoped to the run selected by the first query.
    if (text.includes("model_lead_hours BETWEEN 7 AND 18")) {
      return { rows: hours };
    }
    // The sidecar run selection includes the serving delay.
    if (text.includes("run_initialized_at + interval '6 hours'")) {
      return { rows: [run] };
    }
    throw new Error("unexpected ECMWF temperature canary query");
  },
};

const sidecar = await getEcmwfTemperatureCanarySidecar(pool, {
  asOf: sidecarAsOf,
  from,
  siteSlug: "ballydidean",
  to,
});
const status = await getEcmwfTemperatureCanaryStatus(pool, {
  asOf: statusAsOf,
  siteSlug: "ballydidean",
});
assert.notEqual(sidecar, null);
assert.notEqual(status, null);

const missingCounts = [];
const missingPool = {
  // Prove no hour query follows a missing run.
  async query() {
    missingCounts[missingCounts.length - 1] += 1;
    return { rows: [] };
  },
};
missingCounts.push(0);
const missingSidecar = await getEcmwfTemperatureCanarySidecar(missingPool, {
  asOf: sidecarAsOf,
  from,
  siteSlug: "missing",
  to,
});
missingCounts.push(0);
const missingStatus = await getEcmwfTemperatureCanaryStatus(missingPool, {
  asOf: statusAsOf,
  siteSlug: "missing",
});

// Collapse irrelevant whitespace while retaining every query token.
function normalizeSql(text) {
  return text.trim().split(/\s+/u).join(" ");
}

// Preserve fake-driver scalar encoding without a JSON codec.
function renderBinds(values) {
  return values.join("|");
}

console.log(`sidecar-run-sql=${normalizeSql(captured[0].text)}`);
console.log(`sidecar-run-binds=${renderBinds(captured[0].values)}`);
console.log(`sidecar-hours-sql=${normalizeSql(captured[1].text)}`);
console.log(`sidecar-hours-binds=${renderBinds(captured[1].values)}`);
console.log(`status-sql=${normalizeSql(captured[2].text)}`);
console.log(`status-binds=${renderBinds(captured[2].values)}`);
console.log([
  "sidecar",
  sidecar.run.id,
  sidecar.run.stateStatus,
  sidecar.hours.length,
  sidecar.hours[0].modelLeadHours,
  sidecar.hours[0].rawRelativeHumidityPercent ?? "null",
  sidecar.hours[0].rawWindSpeedMps ?? "null",
  sidecar.hours[1].modelLeadHours,
  sidecar.hours[1].rawRelativeHumidityPercent ?? "null",
  sidecar.hours[1].rawWindSpeedMps ?? "null",
].join("="));
console.log([
  "status",
  status.firstReceivedAt,
  status.hourCount,
  status.latestRunInitializedAt,
  status.stateStatus,
  status.stateReason,
].join("="));
console.log([
  "missing",
  missingSidecar === null,
  missingStatus === null,
  ...missingCounts,
].join("="));
console.log([
  "epochs",
  Date.parse(from),
  Date.parse(to),
].join("="));
