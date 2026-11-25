import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import vm from "node:vm";

import {
  executePublicStationBackfill,
  resolvePublicStationBackfillSources,
} from "../../../../apps/worker/dist/public-stations-backfill-cli.js";

const source = await readFile(
  new URL("../../../../apps/worker/dist/worker.js", import.meta.url),
  "utf8",
);
const start = source.indexOf("async function runStartupPublicStationBackfill(");
const end = source.indexOf("// calculate a bounded non-negative duration", start);
assert.ok(start >= 0 && end > start);

const trace = [];
const witnesses = [];
const configuration = { siteKey: "ballydidean" };
const pool = {};
let failResolution = false;

// supply deterministic wall and calendar observations to the actual startup function
class FixtureDate extends Date {
  // keep specified timestamps intact and freeze unqualified date reads
  constructor(value = "2026-10-20T17:00:00.000Z") {
    super(value);
    // record when the code captures the current UTC date
    if (arguments.length === 0) {
      trace.push("today");
    }
  }

  // capture each diagnostic wall-clock read
  static now() {
    trace.push("diagnostic-clock");
    return trace.length * 100;
  }
}
const {
  runStartupPublicStationBackfill,
} = vm.runInNewContext(
  `${source.slice(start, end)}\n({ runStartupPublicStationBackfill })`,
  {
    Date: FixtureDate,
    // return the in-catalog active source set only after resolving it
    async resolvePublicStationBackfillSources(actualPool, actualConfig, selectors) {
      assert.equal(actualPool, pool);
      assert.equal(actualConfig, configuration);
      assert.equal(selectors.length, 0);
      trace.push("resolve");
      // contain an ordinary repository failure before choosing the date
      if (failResolution) {
        throw new Error("fixture repository unavailable");
      }
      return [];
    },
    // capture exact resume arguments for the ordinary startup workflow
    async executePublicStationBackfill(actualPool, args, actualConfig, sources) {
      assert.equal(actualPool, pool);
      assert.equal(actualConfig, configuration);
      assert.equal(sources.length, 0);
      trace.push(`execute:${args.site}:${args.to}:${args.resume}`);
      assert.equal(args.dryRun, false);
      assert.equal(args.from, null);
      assert.equal(args.reportPath, null);
      assert.equal(args.sourceKeys.length, 0);
      return { sources: [] };
    },
    // retain the redacted production category without a separate formatter
    createWorkerDiagnostic(diagnostic) {
      return diagnostic;
    },
    // record one normal worker log result
    writeWorkerDiagnostic(diagnostic) {
      witnesses.push(diagnostic);
      trace.push(`diagnostic:${diagnostic.event}:${diagnostic.errorCode}`);
    },
  },
);

await runStartupPublicStationBackfill(pool, null, "fixture-release");
assert.deepEqual(trace, []);

await runStartupPublicStationBackfill(pool, configuration, "fixture-release");
console.log(`node-startup=${trace.join(",")}`);
assert.deepEqual(trace, [
  "diagnostic-clock", "resolve", "today", "execute:ballydidean:2026-10-19:true",
]);

trace.length = 0;
failResolution = true;
await runStartupPublicStationBackfill(pool, configuration, "fixture-release");
console.log(`node-failure=${trace.join(",")}`);
assert.deepEqual(trace, [
  "diagnostic-clock", "resolve", "diagnostic-clock",
  "diagnostic:worker_iteration:public_station_backfill_failed",
]);
assert.equal(witnesses[0].release, "fixture-release");
assert.equal(witnesses[0].runId, null);

// run the original startup owner and archive executor with a four-source catalog
async function runMixedStartup() {
  const october19 = "2026-10-19T00:00:00.000Z";
  const october20 = "2026-10-20T00:00:00.000Z";
  const configured = [
    ["ambient", "ambient-weather", {
      deviceId: "0123456789abcdef01234567",
      macAddress: "AA:BB:CC:DD:EE:FF",
    }],
    ["wunderground", "weather-underground", {
      stationId: "KWAREHOUSE1",
      publicApiKey: "fixture-key",
    }],
    ["netatmo", "netatmo", {
      deviceId: "aa:bb:cc:dd:ee:ff",
      outdoorModuleId: "11:22:33:44:55:66",
      rainModuleId: "22:33:44:55:66:77",
      windModuleId: "33:44:55:66:77:88",
    }],
    ["purple", "purpleair", { mapVersion: "1.2.3", sensorIndex: 1234 }],
  ];
  const events = [];
  let report;
  const catalog = {
    siteKey: "ballydidean",
    // keep the fixture identity and historical cutoff in the same order as Rust
    stations: configured.map(([name, adapter, material], index) => ({
      active: true,
      key: `${name}-station`,
      model: null,
      serial: null,
      timezone: "America/Los_Angeles",
      sources: [{
        active: true,
        adapter,
        adapterConfig: material,
        capabilities: ["historical"],
        fingerprint: String(index + 1).padStart(64, "0"),
        historyEndDate: null,
        historyStartDate: "2026-10-19",
        key: `${name}-history`,
        maximumChunkDays: 1,
        providerKey: adapter,
      }],
    })),
  };
  const mixedPool = {
    // resolve the actual Node query from the shared fixture pool
    async query(sql, binds) {
      assert.match(sql, /s\.source_key = ANY\(\$2::text\[\]\)/u);
      assert.deepEqual(binds, ["ballydidean", configured.map(([name]) => `${name}-history`)]);
      return {
        // return live row identity fields in reverse order
        rows: catalog.stations.map((station) => {
          const [entry] = station.sources;
          return {
            materialProviderConfig: entry.adapterConfig,
            providerKey: entry.providerKey,
            siteSlug: catalog.siteKey,
            sourceConfigFingerprint: entry.fingerprint,
            sourceId: `source-${entry.adapter}`,
            sourceKey: entry.key,
            sourceKind: "physical_sensor",
            stationSlug: station.key,
            timezone: station.timezone,
          };
        }).reverse(),
      };
    },
  };
  const repository = {
    // resume all but the last provider for the previous complete UTC day
    async hasSuccessfulBackfillChunk(actualPool, identity) {
      assert.equal(actualPool, mixedPool);
      assert.equal(identity.intervalStart, october19);
      assert.equal(identity.intervalEndExclusive, october20);
      events.push(`lookup:${identity.sourceId}`);
      return identity.sourceId !== "source-purpleair";
    },
    // reuse the pool inherited by the startup owner
    async acquireSourceSession(actualPool, sourceId) {
      assert.equal(actualPool, mixedPool);
      assert.equal(sourceId, "source-purpleair");
      events.push("acquire:purpleair");
      return {
        // release only after the successful receipt
        async release() {
          events.push("release:purpleair");
        },
      };
    },
    // recover expired runs before committing the new one
    async abandonExpiredRuns() {
      events.push("abandon:purpleair");
    },
    // record the original archive endpoint, station and interval
    async startIngestionRun(_session, start) {
      assert.deepEqual(start.requestMetadata, { endpoint: "purpleair", station: "purple-station" });
      assert.equal(start.requestedStart, october19);
      assert.equal(start.requestedEndExclusive, october20);
      events.push("start:purpleair");
      return { id: "run-purpleair" };
    },
    // commit an empty successful receipt after fetching
    async completeBackfillIngestion(_session, completion) {
      assert.equal(completion.runId, "run-purpleair");
      assert.equal(completion.records.length, 0);
      assert.equal(completion.attempts, 1);
      events.push("complete:purpleair");
    },
    // surface any unexpected archive failure rather than report a synthetic success
    async failIngestionRun(_session, failure) {
      throw new Error(`unexpected failure ${failure.error.code}`);
    },
  };
  const { runStartupPublicStationBackfill: mixedStartup } = vm.runInNewContext(
    `${source.slice(start, end)}\n({ runStartupPublicStationBackfill })`,
    {
      Date: FixtureDate,
      resolvePublicStationBackfillSources,
      // run the actual archive executor before the worker emits diagnostics
      async executePublicStationBackfill(actualPool, args, actualCatalog, sources) {
        assert.equal(actualPool, mixedPool);
        assert.equal(actualCatalog, catalog);
        assert.equal(args.to, "2026-10-19");
        assert.equal(args.resume, true);
        assert.equal(args.dryRun, false);
        report = await executePublicStationBackfill(actualPool, args, actualCatalog, sources, {
          // retain a deterministic chunk clock
          now: () => new Date("2026-10-20T00:00:01.000Z"),
          repository,
          // validate the only required provider request without contacting it
          async fetchRange(request) {
            assert.equal(request.adapter, "purpleair");
            assert.equal(request.sourceId, "source-purpleair");
            assert.equal(request.sensorIndex, 1234);
            assert.equal(request.mapVersion, "1.2.3");
            assert.equal(request.start, october19);
            assert.equal(request.endExclusive, october20);
            events.push("fetch:purpleair");
            return {
              attempts: 1, records: [], checksum: "a".repeat(64),
              responseMetadata: { provider: "purpleair" },
            };
          },
        });
        return report;
      },
      // keep the diagnostic fields visible to the comparison
      createWorkerDiagnostic(diagnostic) {
        return diagnostic;
      },
      // emit the same allowlisted outcome per source
      writeWorkerDiagnostic({ event, sourceId, count, errorCode, release }) {
        assert.equal(release, "fixture-release");
        events.push(`diagnostic:${event}:${sourceId ?? "-"}:${count}:${errorCode ?? "-"}`);
      },
    },
  );
  await mixedStartup(mixedPool, catalog, "fixture-release");
  assert.equal(report?.exitCode, 0);
  console.log(`node-mixed-events=${events.join(",")}`);
  console.log(`node-mixed-report=${report.sources.map((row) =>
    [row.source, row.from, row.to, row.plannedChunks, row.skippedChunks,
      row.completedChunks, row.records, row.exitCode].join("|")).join(";")}`);
}

// keep the original empty/failure oracle unchanged for its existing callers
if (process.argv.includes("--mixed")) {
  await runMixedStartup();
}
