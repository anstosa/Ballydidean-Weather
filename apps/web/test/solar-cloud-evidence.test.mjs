import assert from "node:assert/strict";
import test from "node:test";
import { buildSolarCloudEvidence, parseSolarCloudEvidenceResponse, solarCloudBiasFromEvidence } from "../dist/index.js";

const site = { latitude: 47.950429954185445, longitude: -122.42797012608193 };
const day = "2026-06-21T20:00:00.000Z";
const night = new Date("2026-06-22T05:00:00.000Z");

// construct only original normalized fields consumed by the historical selector
function record(kind, validAt = day, overrides = {}) {
  const physical = kind === "physical_sensor";
  return {
    id: physical ? "ws90" : "regional", validAt, receivedAt: validAt,
    freshness: { status: "stale" },
    metrics: { solarRadiationWm2: physical ? 800 : null, cloudCoverPercent: physical ? null : 80 },
    provenance: { providerKey: physical ? "ecowitt-local" : "open-meteo", sourceKind: kind,
      sourceId: physical ? "sensor" : "model", sourceKey: physical ? "ecowitt-88f15505d89f-local-live-v1" : "open-meteo-current-v1",
      stationSlug: physical ? "ballydidean-ecowitt" : "open-meteo-virtual" },
    ...overrides,
  };
}

// retain stale archive labels without reinterpreting the real original measurement times
test("historical daylight evidence rehydrates at night with fixed original expiry and no metadata", () => {
  const rows = [record("physical_sensor"), record("model_current", "2026-06-21T19:55:00.000Z")];
  const before = JSON.stringify(rows);
  const evidence = buildSolarCloudEvidence(rows, site, night.toISOString());
  assert.equal(evidence.evaluatedAt, day);
  assert.deepEqual(Object.keys(evidence.ws90).sort(), ["providerKey", "receivedAt", "recordId", "sourceId", "sourceKey", "sourceKind", "stationSlug", "validAt", "value"].sort());
  const bias = solarCloudBiasFromEvidence(evidence, site, night);
  assert.equal(bias.biasPercentPoints, -80);
  assert.equal(bias.observedAt, day);
  assert.equal(bias.evaluatedAt, day);
  assert.equal(bias.expiresAt, "2026-06-22T20:00:00.000Z");
  assert.equal(solarCloudBiasFromEvidence(evidence, site, new Date(bias.expiresAt)), null);
  assert.equal(JSON.stringify(rows), before);
  assert.deepEqual(parseSolarCloudEvidenceResponse({ contractVersion: "solar-cloud-evidence/v1", generatedAt: night.toISOString(), evidence }, site, night), evidence);
});

// preserve conservative source daylight chronology and original freshness gates
test("invalid archived evidence fails raw instead of accepting spoofed evaluation or freshness", () => {
  const evidence = buildSolarCloudEvidence([record("physical_sensor"), record("model_current")], site, night.toISOString());
  const invalid = [
    { ...evidence, evaluatedAt: night.toISOString() },
    { ...evidence, ws90: { ...evidence.ws90, receivedAt: "2026-06-21T20:06:00.000Z" }, evaluatedAt: "2026-06-21T20:06:00.000Z" },
    { ...evidence, ws90: { ...evidence.ws90, receivedAt: "2026-06-21T19:59:00.000Z" } },
    { ...evidence, ws90: { ...evidence.ws90, sourceKey: "maintenance-rain-target" } },
    { ...evidence, ws90: { ...evidence.ws90, providerKey: "ecowitt-cloud" } },
    { ...evidence, ws90: { ...evidence.ws90, stationSlug: "neighbor" } },
    { ...evidence, ws90: { ...evidence.ws90, value: 0 } },
    { ...evidence, ws90: { ...evidence.ws90, value: Infinity } },
    { ...evidence, regional: { ...evidence.regional, sourceKind: "forecast" } },
    { ...evidence, regional: { ...evidence.regional, recordId: "" } },
    { ...evidence, regional: { ...evidence.regional, validAt: "2026-06-21T19:39:00.000Z" } },
    { ...evidence, ws90: { ...evidence.ws90, validAt: "2026-06-21T20:00:00" } },
    { evaluatedAt: night.toISOString(), ws90: { ...evidence.ws90, validAt: night.toISOString(), receivedAt: night.toISOString() }, regional: { ...evidence.regional, validAt: night.toISOString(), receivedAt: night.toISOString() } },
  ];
  // every forged or ineligible pair independently retires to raw
  for (const value of invalid) {
    assert.equal(solarCloudBiasFromEvidence(value, site, night), null);
  }
  assert.equal(solarCloudBiasFromEvidence(evidence, site, new Date("2026-06-21T19:59:00.000Z")), null);
  assert.equal(buildSolarCloudEvidence([record("physical_sensor", day, { receivedAt: "2026-06-22T06:00:00.000Z" }), record("model_current")], site, night.toISOString()), null);
});

// a cache refresh carries original evidence rather than renewing its qualification window
test("versioned envelopes cannot renew expiry and reject invalid generation times", () => {
  const evidence = buildSolarCloudEvidence([record("physical_sensor"), record("model_current")], site, night.toISOString());
  const envelope = { contractVersion: "solar-cloud-evidence/v1", generatedAt: night.toISOString(), evidence };
  assert.equal(parseSolarCloudEvidenceResponse({ ...envelope, evidence: null }, site, night), null);
  assert.equal(parseSolarCloudEvidenceResponse({ ...envelope, generatedAt: "2026-06-21T19:59:00.000Z" }, site, night), null);
  const expiry = new Date("2026-06-22T20:00:00.000Z");
  assert.equal(parseSolarCloudEvidenceResponse({ ...envelope, generatedAt: expiry.toISOString() }, site, expiry), null);
  // malformed envelopes do not cross the optional browser boundary
  for (const raw of [{ ...envelope, contractVersion: "other" }, { ...envelope, generatedAt: "2026-06-22T05:00:01.000Z" }, { ...envelope, generatedAt: "not-a-date" }]) {
    assert.throws(() => parseSolarCloudEvidenceResponse(raw, site, night), /invalid/u);
  }
});

// latest valid zero bias must not resurrect a previous large correction
test("newest original observation and revision supersede an older nonzero bias", () => {
  const later = "2026-06-21T21:00:00.000Z";
  const rows = [record("physical_sensor"), record("model_current"),
    record("physical_sensor", later, { id: "new-ws90" }),
    record("model_current", later, { id: "new-regional", metrics: { cloudCoverPercent: 0 } })];
  const evidence = buildSolarCloudEvidence(rows.reverse(), site, night.toISOString());
  assert.equal(evidence.ws90.recordId, "new-ws90");
  assert.equal(solarCloudBiasFromEvidence(evidence, site, night).biasPercentPoints, 0);
  const revised = record("physical_sensor", later, { id: "revised", receivedAt: "2026-06-21T21:01:00.000Z", metrics: { solarRadiationWm2: 100 } });
  assert.equal(buildSolarCloudEvidence([...rows, revised], site, night.toISOString()).ws90.recordId, "revised");
});

// include an older regional sample in the pairing halo at the near-expiry boundary
test("live 24-hour evidence can pair with regional history in the twenty-minute halo", () => {
  const through = "2026-06-22T19:59:00.000Z";
  const evidence = buildSolarCloudEvidence([record("physical_sensor"), record("model_current", "2026-06-21T19:40:00.000Z")], site, through);
  assert.ok(evidence);
  assert.equal(evidence.regional.validAt, "2026-06-21T19:40:00.000Z");
});

// pathological revision windows cannot create a quadratic unbounded scan
test("historical pairing rejects excessive rows and comparison growth without a partial result", () => {
  assert.throws(() => buildSolarCloudEvidence(Array(20_001).fill(record("physical_sensor")), site, night.toISOString()), /limit/u);
  const physical = Array.from({ length: 257 }, (_, index) => record("physical_sensor", day, { id: `ws-${index}`, metrics: { solarRadiationWm2: 0 } }));
  const regional = Array.from({ length: 250 }, (_, index) => record("model_current", day, { id: `model-${index}` }));
  assert.throws(() => buildSolarCloudEvidence([...physical, ...regional], site, night.toISOString()), /pairing limit/u);
});
