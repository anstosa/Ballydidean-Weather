import assert from "node:assert/strict";
import { mkdtemp, rm, symlink, writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import test from "node:test";

import {
  captureLocalForecastIssue,
  captureRetrospectiveDevelopmentHistory,
  configuredRemoteCaptureBaseUrl,
  discoverLocalForecastSources,
  fetchBoundedPublicJson,
  initializeLocalForecastCorrectionPanel,
  localForecastCaptureInputHash,
  normalizeLocalForecastWeatherRecord,
  prepareLocalForecastCaptureRoot,
  runFiniteLocalForecastCapture,
  sealLocalForecastLabels,
} from "./capture_local_forecast_corrections.mjs";
import {
  LOCAL_FORECAST_CORRECTION_PRIMARY_CELLS,
  createLocalForecastCorrectionPanel,
  localForecastCorrectionPolicyHash,
  validateLocalForecastPhysicalSource,
  validateLocalForecastIssueSnapshot,
  validateLocalForecastLabelSeal,
} from "./local_forecast_correction_contract.mjs";

const ORIGIN = "https://weather.test/api/v1";
const SITE = Object.freeze({
  latitude: 47.950429954185445,
  longitude: -122.42797012608193,
  siteKey: "ballydidean",
  timeZone: "America/Los_Angeles",
});
const SOURCE_IDS = Object.freeze(["10", "20"]);

// create one complete public weather dto
function weatherRecord(overrides = {}) {
  return {
    freshness: { ageSeconds: 0, label: "Weather value is current", status: "fresh" },
    id: "100",
    metadata: {
      device: { model: "GW2000", serial: "private-serial", vendor: "Ecowitt" },
      provider: { dataset: "forecast", elevationM: 10, gridCell: "private-cell", propertySensors: [{ key: "private" }] },
      quality: { confidencePercent: 100, flags: [], interpolation: null, status: "accepted" },
      upstream: { model: "best_match", timezone: "GMT" },
    },
    metrics: {
      apparentTemperatureC: 10,
      blackGlobeTemperatureC: null,
      cloudCoverPercent: 40,
      pm25MicrogramsPerCubicMeter: 2,
      precipitationMm: 0,
      precipitationRateMmPerHour: 0,
      pressureHpa: 1000,
      relativeHumidityPercent: 50,
      soilElectricalConductivityMicrosiemensPerCm: null,
      soilMoisturePercent: null,
      solarRadiationWm2: 500,
      temperatureC: 11,
      uvIndex: 4,
      wetBulbGlobeTemperatureC: null,
      windDirectionDegrees: 180,
      windGustMps: 4,
      windSpeedMps: 2,
    },
    productRunAt: "2026-10-31T06:00:00.000Z",
    provenance: {
      attribution: { label: "Weather data", url: "https://weather.test/source" },
      label: "forecast",
      providerKey: "open-meteo",
      sourceId: "30",
      sourceKey: "open-meteo-forecast-v4",
      sourceKind: "forecast",
      stationSlug: "open-meteo-virtual",
    },
    receivedAt: "2026-10-31T06:01:00.000Z",
    revisionCount: 1,
    validAt: "2026-10-31T08:00:00.000Z",
    adjustment: { private: "drop" },
    ...overrides,
  };
}

// create one public site catalog envelope
function siteEnvelope() {
  return {
    data: [{
      latitude: SITE.latitude,
      longitude: SITE.longitude,
      name: "Ballydidean",
      slug: SITE.siteKey,
      stations: [
        {
          kind: "physical", latitude: SITE.latitude, longitude: SITE.longitude,
          name: "Farm", slug: "ballydidean-ecowitt",
          sources: [
            { attribution: { label: "Ecowitt", url: "https://example.test" }, id: "10", key: "ecowitt-88f15505d89f-local-live-v1", kind: "physical_sensor", providerKey: "ecowitt-local", providerName: "Ecowitt", provenanceLabel: "physical" },
            { attribution: { label: "Ecowitt", url: "https://example.test" }, id: "11", key: "ecowitt-legacy-v1", kind: "physical_sensor", providerKey: "ecowitt-local", providerName: "Ecowitt", provenanceLabel: "physical" },
            { attribution: { label: "Rain", url: "https://example.test" }, id: "12", key: "rain-target-derived-v1", kind: "physical_sensor", providerKey: "ecowitt-local", providerName: "Rain", provenanceLabel: "physical" },
          ],
        },
        {
          kind: "physical", latitude: 48, longitude: -122,
          name: "Neighbor", slug: "tempest-neighbor",
          sources: [{ attribution: { label: "Tempest", url: "https://example.test" }, id: "20", key: "tempest-live-v1", kind: "physical_sensor", providerKey: "weatherflow-tempest", providerName: "Tempest", provenanceLabel: "physical" }],
        },
        {
          kind: "virtual", latitude: SITE.latitude, longitude: SITE.longitude,
          name: "Model", slug: "open-meteo-virtual",
          sources: [{ attribution: { label: "Open-Meteo", url: "https://example.test" }, id: "30", key: "open-meteo-reanalysis-v1", kind: "reanalysis", providerKey: "open-meteo", providerName: "Open-Meteo", provenanceLabel: "reanalysis" }],
        },
      ],
      timezone: SITE.timeZone,
    }],
  };
}

// create one frozen capture panel fixture
function panel() {
  const cells = LOCAL_FORECAST_CORRECTION_PRIMARY_CELLS.map((cell) => ({
    ...cell,
    policy: { horizonHours: 3, strength: 0 },
  }));
  return createLocalForecastCorrectionPanel({
    candidateFrozenAt: "2026-10-31T03:34:59.999Z",
    captureMinute: 5,
    cells,
    claims: { candidateOutput: "prospective_causal_counterfactual", inputProduct: "as_available_captured_regional_anchor", servingOutput: "regional_zero" },
    codeHash: "a".repeat(64),
    createdAt: "2026-10-31T03:34:59.999Z",
    developmentLatestAt: "2026-10-30T00:35:00.000Z",
    earliestValidationInputAt: "2026-10-31T03:35:00.000Z",
    inputHash: localForecastCaptureInputHash(ORIGIN, SITE, SOURCE_IDS),
    policyHash: localForecastCorrectionPolicyHash(cells),
    servingPolicyState: "zero",
    site: SITE,
    sourceAllowlist: SOURCE_IDS,
    sourceCatalog: [
      { providerKey: "ecowitt-local", sourceId: "10", sourceKey: "ecowitt-88f15505d89f-local-live-v1", stationSlug: "ballydidean-ecowitt" },
      { providerKey: "weatherflow-tempest", sourceId: "20", sourceKey: "tempest-live-v1", stationSlug: "tempest-neighbor" },
    ],
    sourceCatalogReceipt: {
      envelopeSha256: "c".repeat(64),
      receivedAt: "2026-10-31T03:34:59.999Z",
    },
    targetDateCount: 14,
    targetStartLocalDate: "2026-11-01",
  });
}

// create one deterministic injected clock
function fixedClock(at) {
  return { now: () => Date.parse(at), sleep: async () => {} };
}

// create one json response with an optional status
function jsonResponse(body, status = 200) {
  return new Response(JSON.stringify(body), {
    headers: { "content-type": "application/json" },
    status,
  });
}

// create and clean one private capture test root
async function withCaptureRoot(run) {
  const parent = resolve(".weather-data");
  await prepareLocalForecastCaptureRoot(parent);
  const root = await mkdtemp(resolve(parent, "capture-unit-"));
  try {
    return await run(root);
  } finally {
    await rm(root, { force: true, recursive: true });
  }
}

test("source discovery freezes only eligible public physical sources", () => {
  const discovered = discoverLocalForecastSources(siteEnvelope(), SITE);
  assert.deepEqual(discovered.sourceAllowlist, SOURCE_IDS);
  assert.equal(discovered.site.siteKey, SITE.siteKey);
});

test("frozen physical source tuples reject substituted keys providers and stations", () => {
  const catalog = panel().sourceCatalog;
  const exact = catalog[0];
  assert.equal(validateLocalForecastPhysicalSource(exact, catalog), exact);
  for (const change of [
    { sourceKey: "ecowitt-legacy-v1" },
    { providerKey: "substituted-provider" },
    { stationSlug: "substituted-station" },
    { sourceId: "11" },
  ]) {
    // every tuple component remains part of the immutable physical identity
    assert.throws(() => validateLocalForecastPhysicalSource({ ...exact, ...change }, catalog), /frozen source catalog/u);
  }
});

test("configured remote capture base fixes the cli to the checked api target", () => {
  assert.equal(configuredRemoteCaptureBaseUrl({
    remote: { tunnel: { origin: "https://weather.ballydidean.farm" } },
  }), "https://weather.ballydidean.farm/api/v1");
  assert.throws(() => configuredRemoteCaptureBaseUrl({
    remote: { tunnel: { origin: "https://weather.ballydidean.farm/other" } },
  }), /https origin/u);
});

test("normalization retains canonical dto fields while stripping private metadata", () => {
  const normalized = normalizeLocalForecastWeatherRecord(weatherRecord({
    metrics: {
      ...weatherRecord().metrics,
      soilElectricalConductivityMicrosiemensPerCm: 500,
      soilMoisturePercent: 42,
    },
    revisionCount: 0,
  }), "2026-10-31T06:05:00.000Z");
  assert.equal(normalized.metadata.device.serial, null);
  assert.equal(normalized.metadata.provider.gridCell, null);
  assert.equal(normalized.metadata.provider.propertySensors, null);
  assert.equal(normalized.metadata.provider.dataset, "forecast");
  assert.equal(Object.hasOwn(normalized, "adjustment"), false);
  assert.equal(normalized.metrics.uvIndex, 4);
  assert.equal(normalized.metrics.soilElectricalConductivityMicrosiemensPerCm, null);
  assert.equal(normalized.metrics.soilMoisturePercent, null);
  assert.equal(normalized.revisionCount, 0);
});

test("bounded public fetch retries transport once and rejects oversized bodies", async () => {
  let calls = 0;
  const fetched = await fetchBoundedPublicJson(`${ORIGIN}/sites`, {
    baseUrl: ORIGIN,
    clock: fixedClock("2026-10-31T06:05:00.000Z"),
    fetcher: async () => {
      calls += 1;
      if (calls === 1) {
        throw new Error("temporary network failure");
      }
      return jsonResponse({ data: [] });
    },
    maximumBytes: 1_024,
    sleep: async () => {},
  });
  assert.equal(calls, 2);
  assert.deepEqual(fetched.value, { data: [] });
  await assert.rejects(() => fetchBoundedPublicJson(`${ORIGIN}/sites`, {
    baseUrl: ORIGIN,
    clock: fixedClock("2026-10-31T06:05:00.000Z"),
    fetcher: async () => jsonResponse({ value: "x".repeat(200) }),
    maximumBytes: 64,
    sleep: async () => {},
  }), /byte limit/u);
  let exhaustedCalls = 0;
  await assert.rejects(() => fetchBoundedPublicJson(`${ORIGIN}/sites`, {
    baseUrl: ORIGIN,
    clock: fixedClock("2026-10-31T06:05:00.000Z"),
    fetcher: async () => {
      exhaustedCalls += 1;
      throw new Error("offline");
    },
    maximumBytes: 1_024,
    sleep: async () => {},
  }), /offline/u);
  assert.equal(exhaustedCalls, 3);
  let stalledCalls = 0;
  await assert.rejects(() => fetchBoundedPublicJson(`${ORIGIN}/sites`, {
    baseUrl: ORIGIN,
    clock: fixedClock("2026-10-31T06:05:00.000Z"),
    fetcher: async () => {
      stalledCalls += 1;
      return new Response(new ReadableStream({
        // emit one partial chunk and never finish the response body
        start(controller) {
          controller.enqueue(Buffer.from("{"));
        },
      }), { headers: { "content-type": "application/json" } });
    },
    maximumBytes: 1_024,
    sleep: async () => {},
    timeoutMs: 20,
  }), /timed out/u);
  assert.equal(stalledCalls, 3);
  await assert.rejects(() => fetchBoundedPublicJson("https://other.test/api/v1/sites", {
    baseUrl: ORIGIN,
    clock: fixedClock("2026-10-31T06:05:00.000Z"),
    fetcher: async () => jsonResponse({}),
    maximumBytes: 1_024,
    sleep: async () => {},
  }), /escaped/u);
});

test("panel initialization discovers once and binds origin site sources and code", async () => {
  let calls = 0;
  const cells = LOCAL_FORECAST_CORRECTION_PRIMARY_CELLS.map((cell) => ({ ...cell, policy: { horizonHours: 3, strength: 0 } }));
  const initialized = await initializeLocalForecastCorrectionPanel({
    baseUrl: ORIGIN,
    clock: fixedClock("2026-10-31T03:34:59.999Z"),
    fetcher: async () => {
      calls += 1;
      return jsonResponse(siteEnvelope());
    },
    input: {
      captureMinute: 5,
      cells,
      claims: { candidateOutput: "prospective_causal_counterfactual", inputProduct: "as_available_captured_regional_anchor", servingOutput: "regional_zero" },
      developmentLatestAt: "2026-10-30T00:35:00.000Z",
      earliestValidationInputAt: "2026-10-31T03:35:00.000Z",
      servingPolicyState: "zero",
      site: SITE,
      targetDateCount: 14,
      targetStartLocalDate: "2026-11-01",
    },
    sleep: async () => {},
  });
  assert.equal(calls, 1);
  assert.deepEqual(initialized.sourceAllowlist, SOURCE_IDS);
  assert.match(initialized.codeHash, /^[a-f0-9]{64}$/u);
  assert.equal(initialized.inputHash, localForecastCaptureInputHash(ORIGIN, SITE, SOURCE_IDS));
  assert.equal(initialized.createdAt, "2026-10-31T03:34:59.999Z");
  assert.equal(initialized.candidateFrozenAt, initialized.createdAt);
  assert.equal(initialized.sourceCatalogReceipt.receivedAt, initialized.createdAt);
  await assert.rejects(() => initializeLocalForecastCorrectionPanel({
    baseUrl: ORIGIN,
    clock: fixedClock("2026-10-31T03:34:59.999Z"),
    fetcher: async () => jsonResponse(siteEnvelope()),
    input: {
      candidateFrozenAt: "2026-10-31T03:34:59.998Z",
      captureMinute: 5,
      cells,
      claims: { candidateOutput: "prospective_causal_counterfactual", inputProduct: "as_available_captured_regional_anchor", servingOutput: "regional_zero" },
      developmentLatestAt: "2026-10-30T00:35:00.000Z",
      earliestValidationInputAt: "2026-10-31T03:35:00.000Z",
      servingPolicyState: "zero",
      site: SITE,
      targetDateCount: 14,
      targetStartLocalDate: "2026-11-01",
    },
    sleep: async () => {},
  }), /differs from manifest creation/u);
});

test("issue capture publishes one causal normalized snapshot and is idempotent", async () => withCaptureRoot(async (root) => {
  const manifest = panel();
  const scheduledAt = manifest.issueSchedule[0];
  const evaluatedAt = new Date(Date.parse(scheduledAt) + 60_000).toISOString();
  const current = weatherRecord({
    id: "current",
    pressureChange3hHpa: 0.5,
    productRunAt: null,
    provenance: { ...weatherRecord().provenance, providerKey: "ecowitt-local", sourceId: "10", sourceKey: "ecowitt-88f15505d89f-local-live-v1", sourceKind: "physical_sensor", stationSlug: "ballydidean-ecowitt" },
    receivedAt: scheduledAt,
    validAt: scheduledAt,
  });
  const forecast = weatherRecord({ productRunAt: scheduledAt, receivedAt: scheduledAt, validAt: new Date(Date.parse(scheduledAt) + 2 * 3_600_000).toISOString() });
  const regionalCurrent = weatherRecord({
    id: "regional-current",
    productRunAt: null,
    provenance: { ...weatherRecord().provenance, sourceKind: "model_current" },
    receivedAt: scheduledAt,
    validAt: scheduledAt,
  });
  const propertyOnly = weatherRecord({
    id: "property-only",
    metrics: {
      ...weatherRecord().metrics,
      pm25MicrogramsPerCubicMeter: null,
      pressureHpa: null,
      relativeHumidityPercent: null,
      soilElectricalConductivityMicrosiemensPerCm: 400,
      soilMoisturePercent: 31,
      solarRadiationWm2: null,
      uvIndex: null,
    },
    pressureChange3hHpa: null,
    productRunAt: null,
    provenance: { ...current.provenance, sourceId: "property", sourceKey: "ecowitt-property-v1" },
    receivedAt: scheduledAt,
    revisionCount: 0,
    validAt: scheduledAt,
  });
  let calls = 0;
  const fetcher = async (url) => {
    calls += 1;
    if (url.endsWith("/current")) {
      return jsonResponse({ data: [current, propertyOnly, regionalCurrent], generatedAt: scheduledAt, site: siteEnvelope().data[0] });
    }
    if (url.includes("/forecast?days=5")) {
      return jsonResponse({ data: [forecast], generatedAt: scheduledAt, pressureContext: [], site: siteEnvelope().data[0] });
    }
    return jsonResponse({ contractVersion: "solar-cloud-evidence/v1", evidence: null, generatedAt: scheduledAt });
  };
  const snapshot = await captureLocalForecastIssue({ baseUrl: ORIGIN, clock: fixedClock(evaluatedAt), fetcher, panel: manifest, root, scheduledAt, sleep: async () => {} });
  assert.equal(calls, 3);
  assert.equal(validateLocalForecastIssueSnapshot(snapshot), snapshot);
  assert.equal(snapshot.current[0].pressureChange3hHpa, 0.5);
  assert.deepEqual(snapshot.current.map((row) => row.id), ["current", "regional-current"]);
  const repeated = await captureLocalForecastIssue({ baseUrl: ORIGIN, clock: fixedClock(evaluatedAt), fetcher: async () => { throw new Error("must not refetch"); }, panel: manifest, root, scheduledAt, sleep: async () => {} });
  assert.equal(repeated.normalizedDigest, snapshot.normalizedDigest);
}));

test("issue capture rejects a substituted ws90 tuple while allowing regional model current", async () => withCaptureRoot(async (root) => {
  const manifest = panel();
  const scheduledAt = manifest.issueSchedule[0];
  await assert.rejects(() => captureLocalForecastIssue({
    baseUrl: ORIGIN,
    clock: fixedClock(scheduledAt),
    fetcher: async (url) => {
      if (url.endsWith("/current")) {
        return jsonResponse({ data: [], generatedAt: scheduledAt, site: siteEnvelope().data[0] });
      }
      if (url.includes("/forecast?days=5")) {
        return jsonResponse({ data: [], generatedAt: scheduledAt, pressureContext: [], site: siteEnvelope().data[0] });
      }
      return jsonResponse({
        contractVersion: "solar-cloud-evidence/v1",
        evidence: {
          evaluatedAt: scheduledAt,
          regional: {
            providerKey: "open-meteo",
            receivedAt: scheduledAt,
            recordId: "regional",
            sourceId: "regional-public",
            sourceKey: "open-meteo-current-v1",
            sourceKind: "model_current",
            stationSlug: "open-meteo-virtual",
            validAt: scheduledAt,
            value: 500,
          },
          ws90: {
            ...manifest.sourceCatalog[1],
            receivedAt: scheduledAt,
            recordId: "ws90",
            sourceKind: "physical_sensor",
            validAt: scheduledAt,
            value: 400,
          },
        },
        generatedAt: scheduledAt,
      });
    },
    panel: manifest,
    root,
    scheduledAt,
    sleep: async () => {},
  }), /canonical farm live source/u);
}));

test("a missed issue burns the slot without making network reads", async () => withCaptureRoot(async (root) => {
  const manifest = panel();
  const scheduledAt = manifest.issueSchedule[0];
  const late = new Date(Date.parse(scheduledAt) + 6 * 60_000).toISOString();
  await assert.rejects(() => captureLocalForecastIssue({
    baseUrl: ORIGIN,
    clock: fixedClock(late),
    fetcher: async () => { throw new Error("must not fetch"); },
    panel: manifest,
    root,
    scheduledAt,
    sleep: async () => {},
  }), /missed/u);
  await assert.rejects(() => captureLocalForecastIssue({
    baseUrl: ORIGIN,
    clock: fixedClock(late),
    fetcher: async () => jsonResponse({}),
    panel: manifest,
    root,
    scheduledAt,
    sleep: async () => {},
  }), /terminal/u);
}));

test("an oversized current product fails the whole burned issue without schema retries", async () => withCaptureRoot(async (root) => {
  const manifest = panel();
  const scheduledAt = manifest.issueSchedule[0];
  const evaluatedAt = new Date(Date.parse(scheduledAt) + 60_000).toISOString();
  let calls = 0;
  const fetcher = async (url) => {
    calls += 1;
    if (url.endsWith("/current")) {
      return jsonResponse({ data: Array.from({ length: 129 }, (_unused, index) => weatherRecord({ id: String(index + 1) })), generatedAt: scheduledAt, site: siteEnvelope().data[0] });
    }
    if (url.includes("/forecast?days=5")) {
      return jsonResponse({ data: [], generatedAt: scheduledAt, pressureContext: [], site: siteEnvelope().data[0] });
    }
    return jsonResponse({ contractVersion: "solar-cloud-evidence/v1", evidence: null, generatedAt: scheduledAt });
  };
  await assert.rejects(() => captureLocalForecastIssue({ baseUrl: ORIGIN, clock: fixedClock(evaluatedAt), fetcher, panel: manifest, root, scheduledAt, sleep: async () => {} }), /weather data/u);
  assert.equal(calls, 3);
  await assert.rejects(() => captureLocalForecastIssue({ baseUrl: ORIGIN, clock: fixedClock(evaluatedAt), fetcher, panel: manifest, root, scheduledAt, sleep: async () => {} }), /terminal/u);
  assert.equal(calls, 3);
}));

test("finite runner continues only witnessed http slot failures and rethrows structural errors", async () => withCaptureRoot(async (root) => {
  const manifest = panel();
  let now = Date.parse(manifest.issueSchedule[0]);
  const clock = { now: () => now };
  const sleep = async (milliseconds) => {
    // advance the injected finite schedule clock without real waits
    now += milliseconds;
  };
  let issueCalls = 0;
  const result = await runFiniteLocalForecastCapture({
    baseUrl: ORIGIN,
    captureIssue: async (options) => {
      issueCalls += 1;
      // exercise one real exhausted transport slot then stub later successful publications
      if (issueCalls === 1) {
        return captureLocalForecastIssue(options);
      }
      return {};
    },
    clock,
    fetcher: async () => { throw new Error("offline"); },
    panel: manifest,
    root,
    sealLabels: async ({ targetDate }) => ({ status: "complete", targetDate }),
    sleep,
  });
  assert.equal(result.issueFailed, 1);
  assert.equal(result.issuePublished, manifest.issueSchedule.length - 1);
  assert.deepEqual(result.issueFailureReasons, { offline: 1 });
  await assert.rejects(() => runFiniteLocalForecastCapture({
    baseUrl: ORIGIN,
    captureIssue: async () => { throw new Error("structural filesystem failure"); },
    clock: fixedClock(manifest.issueSchedule[0]),
    panel: manifest,
    root,
    sealLabels: async ({ targetDate }) => ({ status: "complete", targetDate }),
    sleep: async () => {},
  }), /structural filesystem failure/u);
}));

test("label sealing pages each frozen source and excludes revisions received after cutoff", async () => withCaptureRoot(async (root) => {
  const manifest = panel();
  const schedule = manifest.labelSchedule[0];
  const receivedAt = new Date(Date.parse(schedule.sealAt) + 60_000).toISOString();
  const rowAt = new Date(Date.parse(schedule.from) + 4 * 3_600_000).toISOString();
  let calls = 0;
  const fetcher = async (url) => {
    calls += 1;
    const sourceId = new URL(url).searchParams.get("source");
    const row = weatherRecord({
      id: `${sourceId}-row`,
      productRunAt: null,
      provenance: { ...weatherRecord().provenance, providerKey: sourceId === "10" ? "ecowitt-local" : "weatherflow-tempest", sourceId, sourceKey: sourceId === "10" ? "ecowitt-88f15505d89f-local-live-v1" : "tempest-live-v1", sourceKind: "physical_sensor", stationSlug: sourceId === "10" ? "ballydidean-ecowitt" : "tempest-neighbor" },
      receivedAt: sourceId === "10" ? schedule.sealAt : new Date(Date.parse(schedule.sealAt) + 1).toISOString(),
      validAt: rowAt,
    });
    return jsonResponse({ data: [row], generatedAt: schedule.sealAt, page: { limit: 250, nextCursor: null }, site: siteEnvelope().data[0] });
  };
  const labels = await sealLocalForecastLabels({ baseUrl: ORIGIN, clock: fixedClock(receivedAt), fetcher, panel: manifest, root, sleep: async () => {}, targetDate: schedule.targetDate });
  assert.equal(calls, 2);
  assert.equal(validateLocalForecastLabelSeal(labels), labels);
  assert.equal(labels.status, "complete", labels.reasonCode);
  assert.deepEqual(labels.rows.map((row) => row.provenance.sourceId), ["10"]);
  assert.deepEqual(labels.receipts.map((receipt) => receipt.sourceId), SOURCE_IDS);
}));

test("a cursor cycle seals one unsupported tombstone without partial rows", async () => withCaptureRoot(async (root) => {
  const manifest = panel();
  const schedule = manifest.labelSchedule[0];
  const receiptAt = new Date(Date.parse(schedule.sealAt) + 60_000).toISOString();
  let calls = 0;
  const labels = await sealLocalForecastLabels({
    baseUrl: ORIGIN,
    clock: fixedClock(receiptAt),
    fetcher: async () => {
      calls += 1;
      return jsonResponse({ data: [], generatedAt: schedule.sealAt, page: { limit: 250, nextCursor: "repeat" }, site: siteEnvelope().data[0] });
    },
    panel: manifest,
    root,
    sleep: async () => {},
    targetDate: schedule.targetDate,
  });
  assert.equal(calls, 2);
  assert.equal(labels.status, "unsupported");
  assert.deepEqual(labels.rows, []);
  assert.match(labels.reasonCode, /cursor/u);
  assert.equal(validateLocalForecastLabelSeal(labels), labels);
}));

test("sealed labels are immutable and never refreshed", async () => withCaptureRoot(async (root) => {
  const manifest = panel();
  const schedule = manifest.labelSchedule[0];
  const receiptAt = new Date(Date.parse(schedule.sealAt) + 60_000).toISOString();
  const first = await sealLocalForecastLabels({
    baseUrl: ORIGIN,
    clock: fixedClock(receiptAt),
    fetcher: async () => jsonResponse({ data: [], generatedAt: schedule.sealAt, page: { limit: 250, nextCursor: null }, site: siteEnvelope().data[0] }),
    panel: manifest,
    root,
    sleep: async () => {},
    targetDate: schedule.targetDate,
  });
  const second = await sealLocalForecastLabels({
    baseUrl: ORIGIN,
    clock: fixedClock(receiptAt),
    fetcher: async () => { throw new Error("must not refetch"); },
    panel: manifest,
    root,
    sleep: async () => {},
    targetDate: schedule.targetDate,
  });
  assert.equal(first.normalizedDigest, second.normalizedDigest);
}));

test("a burned label claim recovers after its deadline without another http read", async () => withCaptureRoot(async (root) => {
  const manifest = panel();
  const schedule = manifest.labelSchedule[0];
  await assert.rejects(() => sealLocalForecastLabels({
    baseUrl: ORIGIN,
    clock: fixedClock(new Date(Date.parse(schedule.sealAt) - 1).toISOString()),
    fetcher: async () => { throw new Error("must not fetch"); },
    panel: manifest,
    root,
    sleep: async () => {},
    targetDate: schedule.targetDate,
  }), /not due/u);
  await writeFile(resolve(root, "attempts", "labels", `${schedule.targetDate}.claim.json`), JSON.stringify({
    contractVersion: "weather-local-correction-capture/v1",
    panelDigest: manifest.panelDigest,
    sealAt: schedule.sealAt,
    startedAt: schedule.sealAt,
    targetDate: schedule.targetDate,
    type: "label_attempt",
  }), { mode: 0o600 });
  const recovered = await sealLocalForecastLabels({
    baseUrl: ORIGIN,
    clock: fixedClock(new Date(Date.parse(schedule.sealAt) + 5 * 60_000).toISOString()),
    fetcher: async () => { throw new Error("must not fetch"); },
    panel: manifest,
    root,
    sleep: async () => {},
    targetDate: schedule.targetDate,
  });
  assert.equal(recovered.status, "unsupported");
  assert.equal(recovered.reasonCode, "burned_claim_recovered_after_deadline");
  assert.equal(recovered.panelDigest, manifest.panelDigest);
}));

test("retrospective development history uses actual retrieval receipts without seal claims", async () => {
  const from = "2026-09-01T00:00:00.000Z";
  const to = "2026-09-02T12:00:00.000Z";
  const retrievedAt = "2026-10-31T06:05:00.000Z";
  let calls = 0;
  const result = await captureRetrospectiveDevelopmentHistory({
    baseUrl: ORIGIN,
    clock: fixedClock(retrievedAt),
    fetcher: async (url) => {
      calls += 1;
      const sourceId = new URL(url).searchParams.get("source");
      return jsonResponse({
        data: [weatherRecord({
          id: `history-${sourceId}`,
          productRunAt: null,
          provenance: {
            ...weatherRecord().provenance,
            providerKey: sourceId === "10" ? "ecowitt-local" : "weatherflow-tempest",
            sourceId,
            sourceKey: sourceId === "10" ? "ecowitt-88f15505d89f-local-live-v1" : "tempest-live-v1",
            sourceKind: "physical_sensor",
            stationSlug: sourceId === "10" ? "ballydidean-ecowitt" : "tempest-neighbor",
          },
          receivedAt: "2026-09-01T08:01:00.000Z",
          validAt: "2026-09-01T08:00:00.000Z",
        })],
        generatedAt: retrievedAt,
        page: { limit: 250, nextCursor: null },
        site: siteEnvelope().data[0],
      });
    },
    from,
    site: SITE,
    sleep: async () => {},
    sourceCatalog: panel().sourceCatalog,
    to,
  });
  assert.equal(calls, 2);
  assert.equal(result.retrievedAt, retrievedAt);
  assert.equal(result.historyRows.length, 2);
  assert.equal(result.receipts.length, 2);
  assert.equal(Object.hasOwn(result, "sealAt"), false);
  await assert.rejects(() => captureRetrospectiveDevelopmentHistory({
    baseUrl: ORIGIN,
    clock: fixedClock(retrievedAt),
    fetcher: async () => { throw new Error("must not fetch"); },
    from,
    site: SITE,
    sleep: async () => {},
    sourceCatalog: panel().sourceCatalog,
    to: "2026-09-02T12:00:00.001Z",
  }), /thirty-six hours/u);
});

test("private root preparation rejects symlink ancestors", async () => {
  await prepareLocalForecastCaptureRoot(resolve(".weather-data"));
  const target = await mkdtemp(resolve(".weather-data", "capture-target-"));
  const link = resolve(".weather-data", `capture-link-${process.pid}-${Date.now()}`);
  await symlink(target, link, "dir");
  try {
    await assert.rejects(() => prepareLocalForecastCaptureRoot(resolve(link, "nested")), /symbolic link/u);
  } finally {
    await rm(link, { force: true });
    await rm(target, { force: true, recursive: true });
  }
});
