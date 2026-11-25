import assert from "node:assert/strict";

import {
  executePublicStationBackfill,
  resolvePublicStationBackfillSources,
} from "../../../../apps/worker/dist/index.js";

const fingerprint = "a".repeat(64);
const material = {
  contractVersion: "ambient-device-data/v1",
  deviceId: "5fe7d16a25083b000171f8e5",
  macAddress: "F0:08:D1:07:B3:9E",
  sample: "every-distinct-provider-observation",
};
const sourceConfiguration = {
  active: true,
  adapter: "ambient-weather",
  adapterConfig: material,
  cadenceSeconds: 3600,
  capabilities: ["current", "historical"],
  fingerprint,
  historyEndDate: null,
  historyStartDate: "2026-09-01",
  key: "ambient-merlin-observations-v1",
  maximumChunkDays: 7,
  providerKey: "ambient-weather-network",
};
const station = {
  active: true,
  displayName: "French Rd & Lapis Ln",
  key: "ambient-merlin",
  latitude: 47.955,
  longitude: -122.418,
  model: "AMBWeatherV4.3.6",
  serial: "5fe7d16a25083b000171f8e5",
  sources: [sourceConfiguration],
  timezone: "America/Los_Angeles",
  vendor: "Ambient Weather",
};
const configuration = {
  providers: [],
  siteKey: "ballydidean",
  stations: [station],
  version: 1,
};
const associationTrace = [];
const pool = {
  // capture the exact compiled source-association query and bind order
  async query(sql, binds) {
    associationTrace.push("associate");
    assert.match(sql, /s\.source_key = ANY\(\$2::text\[\]\)/u);
    assert.match(sql, /ORDER BY s\.source_key/u);
    assert.deepEqual(binds, [
      "ballydidean",
      ["ambient-merlin-observations-v1"],
    ]);
    return {
      rows: [{
        materialProviderConfig: material,
        providerKey: "ambient-weather-network",
        siteSlug: "ballydidean",
        sourceConfigFingerprint: fingerprint,
        sourceId: "source-ambient",
        sourceKey: "ambient-merlin-observations-v1",
        sourceKind: "physical_sensor",
        stationSlug: "ambient-merlin",
        timezone: "America/Los_Angeles",
      }],
    };
  },
};
const sources = await resolvePublicStationBackfillSources(pool, configuration, []);
assert.equal(sources.length, 1);
assert.equal(sources[0].id, "source-ambient");
assert.deepEqual(associationTrace, ["associate"]);

const arguments_ = {
  dryRun: false,
  from: null,
  reportPath: null,
  resume: true,
  site: "ballydidean",
  sourceKeys: [],
  to: "2026-09-01",
};

// prove exact successful-receipt resume skips provider and session work
const skipTrace = [];
const skipped = await executePublicStationBackfill(
  pool,
  arguments_,
  configuration,
  sources,
  {
    repository: {
      async hasSuccessfulBackfillChunk(_pool, identity) {
        skipTrace.push(`resume:${identity.intervalStart}`);
        return true;
      },
    },
  },
);
assert.deepEqual(skipTrace, ["resume:2026-09-01T00:00:00.000Z"]);
assert.equal(skipped.sources[0].skippedChunks, 1);
assert.equal(skipped.sources[0].completedChunks, 0);

const trace = [];
const session = {
  // retain release as the final source-session operation
  async release() {
    trace.push("release");
  },
};
const repository = {
  // return no prior successful receipt for the executable fixture
  async hasSuccessfulBackfillChunk(_pool, identity) {
    assert.equal(identity.sourceId, "source-ambient");
    assert.equal(identity.adapterVersion, "ambient-device-data/v1");
    assert.equal(identity.chunkPlanVersion, "public-station-calendar-range/v1");
    trace.push("resume");
    return false;
  },
  // retain the exact source lock identity
  async acquireSourceSession(_pool, sourceId) {
    assert.equal(sourceId, "source-ambient");
    trace.push("acquire");
    return session;
  },
  // recover expired work before the running receipt
  async abandonExpiredRuns(actualSession, asOf) {
    assert.equal(actualSession, session);
    assert.equal(asOf, "2026-09-03T01:00:00.000Z");
    trace.push("abandon");
  },
  // capture the durable running receipt inputs
  async startIngestionRun(actualSession, input) {
    assert.equal(actualSession, session);
    assert.equal(input.mode, "backfill");
    assert.deepEqual(input.requestMetadata, {
      endpoint: "ambient-weather",
      station: "ambient-merlin",
    });
    trace.push("start-receipt");
    return { id: "run-ambient" };
  },
  // capture the atomic records, successful receipt, and provenance
  async completeBackfillIngestion(actualSession, input) {
    assert.equal(actualSession, session);
    assert.equal(input.runId, "run-ambient");
    assert.equal(input.records.length, 1);
    assert.equal(input.identity.requestedFromDate, "2026-09-01");
    assert.equal(input.identity.requestedToDate, "2026-09-01");
    assert.equal(input.upstreamResponseChecksum, "fixture-checksum");
    trace.push("complete-receipt");
  },
  // the normal fixture must not finalize failure
  async failIngestionRun() {
    throw new Error("unexpected failure finalization");
  },
};
const completed = await executePublicStationBackfill(
  pool,
  arguments_,
  configuration,
  sources,
  {
    // freeze the request and deadline clock without changing the CLI behavior
    now: () => new Date("2026-09-03T01:00:00.000Z"),
    repository,
    // return one real in-window record-shaped provider result
    async fetchRange(request) {
      assert.equal(request.adapter, "ambient-weather");
      assert.equal(request.deviceId, material.deviceId);
      assert.equal(request.macAddress, material.macAddress);
      trace.push("provider");
      return {
        attempts: 2,
        checksum: "fixture-checksum",
        records: [{ validAt: "2026-09-01T00:06:00.000Z" }],
        responseMetadata: {
          normalized_record_count: 1,
          provider: "ambient-weather-network",
          raw_observation_count: 1,
        },
      };
    },
  },
);
assert.deepEqual(trace, [
  "resume",
  "acquire",
  "abandon",
  "start-receipt",
  "provider",
  "complete-receipt",
  "release",
]);
assert.equal(completed.exitCode, 0);
assert.equal(completed.sources[0].completedChunks, 1);
assert.equal(completed.sources[0].records, 1);
console.log(`node-ambient-backfill=${trace.join(",")}`);
