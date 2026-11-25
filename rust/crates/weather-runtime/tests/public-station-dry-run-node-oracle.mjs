import assert from "node:assert/strict";
import { fileURLToPath } from "node:url";

import {
  executePublicStationBackfill,
  resolvePublicStationBackfillSources,
} from "../../../../apps/worker/dist/public-stations-backfill-cli.js";
import { loadPublicStationConfiguration } from "../../../../packages/database/dist/public-stations-config.js";
import {
  ProviderFailure,
  fetchPurpleAirRange,
} from "../../../../packages/providers/dist/index.js";

const catalogPath = fileURLToPath(
  new URL("../../../../config/public-stations/stations.json", import.meta.url),
);
const selectedSourceKey = "wunderground-maxweather-history-v1";
const requestedFrom = "2026-08-19";
const requestedTo = "2026-08-25";
const archiveNow = new Date("2026-11-22T00:00:00.000Z");
const archiveChecksum = "b".repeat(64);

// retain only fields consumed during a bound historical request
const archiveMaterialKeys = {
  "ambient-weather": ["deviceId", "macAddress"],
  "weather-underground": ["stationId", "publicApiKey"],
  netatmo: ["deviceId", "outdoorModuleId", "rainModuleId", "windModuleId"],
  purpleair: ["mapVersion", "sensorIndex"],
};

// collect the active historical catalog entries in declared order
function collectHistoricalEntries(configuration, sourceKeys) {
  const entries = [];
  const selected = new Set(sourceKeys);
  // retain station and source declaration order
  for (const station of configuration.stations) {
    // retain source declaration order within each station
    for (const source of station.sources) {
      // mirror the compiled resolver selection contract
      if (
        source.active
        && source.capabilities.includes("historical")
        && (selected.size === 0 || selected.has(source.key))
      ) {
        entries.push({ source, station });
      }
    }
  }
  return entries;
}

// synthesize the checked database identity for one catalog source
function createIdentityRow(configuration, entry) {
  return {
    materialProviderConfig: entry.source.adapterConfig,
    providerKey: entry.source.providerKey,
    siteSlug: configuration.siteKey,
    sourceConfigFingerprint: entry.source.fingerprint,
    sourceId: `source-${entry.source.key}`,
    sourceKey: entry.source.key,
    sourceKind: "physical_sensor",
    stationSlug: entry.station.key,
    timezone: entry.station.timezone,
  };
}

// expose only the resolver's exact read query without host access
function createReadPool(configuration, sourceKeys) {
  const entries = collectHistoricalEntries(configuration, sourceKeys);
  const expectedKeys = [];
  const rows = [];
  // derive every fake row from the checked catalog
  for (const entry of entries) {
    expectedKeys.push(entry.source.key);
    rows.unshift(createIdentityRow(configuration, entry));
  }
  return {
    // validate the compiled resolver query and bind order
    async query(sql, binds) {
      assert.match(sql, /s\.source_key = ANY\(\$2::text\[\]\)/u);
      assert.match(sql, /ORDER BY s\.source_key/u);
      assert.deepEqual(binds, [configuration.siteKey, expectedKeys]);
      return { rows };
    },
  };
}

// reject every dry-run path that would acquire or write through a repository
function unexpectedRepositoryCall(name) {
  throw new Error(`unexpected dry-run repository call: ${name}`);
}

// execute one compiled dry-run and capture every resume lookup
async function executeDryRun(pool, configuration, sources, sourceKeys) {
  const lookups = [];
  const repository = {
    // dry-run must not recover ingestion runs
    async abandonExpiredRuns() {
      unexpectedRepositoryCall("abandonExpiredRuns");
    },
    // dry-run must not acquire source sessions
    async acquireSourceSession() {
      unexpectedRepositoryCall("acquireSourceSession");
    },
    // dry-run must not complete ingestion
    async completeBackfillIngestion() {
      unexpectedRepositoryCall("completeBackfillIngestion");
    },
    // dry-run must not record failures
    async failIngestionRun() {
      unexpectedRepositoryCall("failIngestionRun");
    },
    // capture the exact resume identity before dry-run skips the chunk
    async hasSuccessfulBackfillChunk(_pool, identity) {
      lookups.push(identity);
      return false;
    },
    // dry-run must not start ingestion
    async startIngestionRun() {
      unexpectedRepositoryCall("startIngestionRun");
    },
  };
  const report = await executePublicStationBackfill(
    pool,
    {
      dryRun: true,
      from: requestedFrom,
      reportPath: null,
      resume: true,
      site: configuration.siteKey,
      sourceKeys,
      to: requestedTo,
    },
    configuration,
    sources,
    {
      // reject provider access during dry-run
      async fetchRange() {
        throw new Error("unexpected dry-run provider call");
      },
      repository,
    },
  );
  return { lookups, report };
}

// format one resolved source for the differential Rust fixture
function formatSource(source) {
  const historyEnd = source.source.historyEndDate;
  const historyEndEpoch = historyEnd === null
    ? "-"
    : String(Date.parse(`${historyEnd}T00:00:00.000Z`));
  return [
    "source",
    source.source.adapter,
    source.id,
    source.source.key,
    source.source.fingerprint,
    source.source.historyStartDate,
    String(Date.parse(`${source.source.historyStartDate}T00:00:00.000Z`)),
    historyEnd ?? "-",
    historyEndEpoch,
    String(source.source.maximumChunkDays),
  ].join("\t");
}

// format the stable per-source report signature
function formatReportRows(report) {
  const rows = [];
  // retain compiled execution order
  for (const source of report.sources) {
    rows.push([
      source.source,
      source.from,
      source.to,
      source.plannedChunks,
      source.skippedChunks,
      source.completedChunks,
      source.records,
      source.exitCode,
    ].join("|"));
  }
  return rows.join(";");
}

// format the stable resume-lookup identity signature
function formatLookups(lookups) {
  const rows = [];
  // retain compiled chunk planning order
  for (const identity of lookups) {
    rows.push([
      identity.sourceId,
      identity.adapterVersion,
      identity.sourceConfigFingerprint,
      identity.chunkPlanVersion,
      identity.intervalStart,
      identity.intervalEndExclusive,
      identity.requestedFromDate,
      identity.requestedToDate,
    ].join("|"));
  }
  return rows.join(";");
}

// exercise mixed provider writes through the original command without I/O
async function executeArchive(pool, configuration, sources) {
  const trace = [];
  const byId = new Map(sources.map((source) => [source.id, source]));
  const repository = {
    // skip the first Weather Underground chunk and one intermediate PurpleAir chunk
    async hasSuccessfulBackfillChunk(_pool, identity) {
      const source = byId.get(identity.sourceId);
      const epoch = Date.parse(identity.intervalStart);
      trace.push(`archive:lookup|${source.source.adapter}|${epoch}`);
      const day = identity.intervalStart.slice(0, 10);
      return (source.source.adapter === "weather-underground" && day === requestedFrom)
        || (source.source.adapter === "purpleair" && day === "2026-08-21");
    },
    // model the exact acquired session through release
    async acquireSourceSession(_pool, id) {
      const { adapter } = byId.get(id).source;
      trace.push(`archive:acquire|${adapter}`);
      return {
        adapter,
        // release after the successful or failed row is durable
        async release() {
          trace.push(`archive:release|${adapter}`);
        },
      };
    },
    // capture recovery against the same source session
    async abandonExpiredRuns(session, now) {
      trace.push(`archive:abandon|${session.adapter}|${Date.parse(now)}`);
    },
    // capture the original run metadata, interval and absolute deadline
    async startIngestionRun(session, start) {
      assert.equal(start.mode, "backfill");
      trace.push([
        "archive:start", session.adapter, start.requestMetadata.endpoint,
        start.requestMetadata.station, Date.parse(start.requestedStart),
        Date.parse(start.requestedEndExclusive), Date.parse(start.deadlineAt),
      ].join("|"));
      return { id: "archive-run" };
    },
    // capture the attempt and exact successful chunk product
    async completeBackfillIngestion(session, completion) {
      trace.push([
        "archive:complete", session.adapter, completion.runId,
        completion.attempts, completion.records.length,
        completion.responseMetadata.provider, completion.upstreamResponseChecksum,
      ].join("|"));
    },
    // retain classified failure instead of a successful receipt
    async failIngestionRun(session, failure) {
      trace.push([
        "archive:fail", session.adapter, failure.runId, failure.attempts,
        failure.error.classification, failure.error.code, failure.error.message,
        Date.parse(failure.backfillIdentity.intervalStart),
      ].join("|"));
    },
  };
  const report = await executePublicStationBackfill(
    pool,
    {
      dryRun: false, from: requestedFrom, reportPath: null, resume: true,
      site: configuration.siteKey, sourceKeys: [], to: requestedTo,
    },
    configuration,
    sources,
    {
      // use the exact fixed clock for run and provider deadlines
      now: () => archiveNow,
      fetchOptions: {
        // record the intentional pacing without waiting
        async sleep(delay) {
          trace.push(`archive:pace|${delay}`);
        },
      },
      // check checked-catalog request shape and inject a committed source failure
      async fetchRange(request, options) {
        const source = byId.get(request.sourceId);
        assert.equal(request.timezone, source.station.timezone);
        assert.equal(request.model, source.station.model);
        assert.equal(request.serial, source.station.serial);
        // require every provider-specific field from the generated catalog
        for (const field of archiveMaterialKeys[request.adapter]) {
          assert.equal(request[field], source.source.adapterConfig[field]);
        }
        trace.push([
          "archive:fetch", request.adapter, request.sourceId, request.timezone,
          request.model ?? "-", request.serial ?? "-", Date.parse(request.start),
          Date.parse(request.endExclusive), Date.parse(options.deadlineAt),
        ].join("|"));
        // stop only Weather Underground after its first committed failure
        if (request.adapter === "weather-underground") {
          throw new ProviderFailure({
            classification: "rate_limited", code: "fixture_rate_limited",
            message: "fixture rate limit",
          }, { attempts: 2 });
        }
        return {
          attempts: 2, records: [], checksum: archiveChecksum,
          responseMetadata: { provider: source.source.providerKey },
        };
      },
      repository,
    },
  );
  return { report, trace };
}

// encode the already decoded catalog subset required by the Rust fixture
function formatArchiveCatalog(source) {
  return [
    "catalog", source.sourceKey, source.station.key, source.station.timezone,
    source.station.model ?? "-", source.station.serial ?? "-",
    ...archiveMaterialKeys[source.source.adapter].flatMap(
      (field) => [field, source.source.adapterConfig[field]],
    ),
  ].join("\t");
}

// run the real token parser against a malformed decoded response
async function malformedPurpleAirToken(source) {
  let calls = 0;
  try {
    await fetchPurpleAirRange({
      adapter: "purpleair",
      start: "2026-08-19T00:00:00.000Z",
      endExclusive: "2026-08-20T00:00:00.000Z",
      sourceId: source.id,
      timezone: source.station.timezone,
      model: source.station.model,
      serial: source.station.serial,
      mapVersion: source.source.adapterConfig.mapVersion,
      sensorIndex: source.source.adapterConfig.sensorIndex,
    }, {
      // return one malformed token and forbid a subsequent history request
      async fetch(url) {
        calls += 1;
        assert.equal(new URL(url).pathname, "/v1/token");
        return new Response("<html>bad</html>", { status: 200 });
      },
    });
  } catch (error) {
    assert(error instanceof ProviderFailure);
    assert.equal(calls, 1);
    return [
      error.attempts, error.ingestionError.classification,
      error.ingestionError.code, error.ingestionError.message,
    ].join("|");
  }
  throw new Error("malformed PurpleAir token was accepted");
}

const configuration = await loadPublicStationConfiguration(catalogPath);
const allPool = createReadPool(configuration, []);
const selectedPool = createReadPool(configuration, [selectedSourceKey]);
const allSources = await resolvePublicStationBackfillSources(
  allPool,
  configuration,
  [],
);
const selectedSources = await resolvePublicStationBackfillSources(
  selectedPool,
  configuration,
  [selectedSourceKey],
);
assert.equal(allSources.length, 4);
assert.equal(selectedSources.length, 1);
assert.equal(selectedSources[0]?.sourceKey, selectedSourceKey);

const all = await executeDryRun(allPool, configuration, allSources, []);
const selected = await executeDryRun(
  selectedPool,
  configuration,
  selectedSources,
  [selectedSourceKey],
);
const lines = [];
// emit source material in checked catalog order
for (const source of allSources) {
  lines.push(formatSource(source));
}
lines.push(`report\tall\t${formatReportRows(all.report)}`);
lines.push(`report\tselected\t${formatReportRows(selected.report)}`);
lines.push(`lookups\tall\t${formatLookups(all.lookups)}`);
lines.push(`lookups\tselected\t${formatLookups(selected.lookups)}`);
// keep previous dry-run output unchanged unless the write fixture was requested
if (process.argv.includes("--archive")) {
  const { report, trace } = await executeArchive(allPool, configuration, allSources);
  assert.equal(report.exitCode, 1);
  assert.deepEqual(report.sources[1].failedChunk, {
    errorCode: "fixture_rate_limited", startDate: "2026-08-20",
  });
  // retain every checked catalog source and transactional trace in the comparison
  for (const source of allSources) {
    lines.push(formatArchiveCatalog(source));
  }
  lines.push(`archive-report\t${formatReportRows(report)}`, ...trace);
}
// expose only bounded derived failure evidence for the Rust operation test
if (process.argv.includes("--malformed-token")) {
  const source = allSources.find((entry) => entry.source.adapter === "purpleair");
  lines.push(`malformed-token\t${await malformedPurpleAirToken(source)}`);
}
console.log(lines.join("\n"));
