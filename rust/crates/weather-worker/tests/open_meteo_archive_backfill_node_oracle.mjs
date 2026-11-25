import {
  executeBackfill,
  planBackfillChunks,
} from "../../../../apps/worker/dist/backfill-cli.js";

const fingerprint = "a".repeat(64);
const sourceId = "00000000-0000-4000-8000-000000000001";
const adapterConfig = {
  contractVersion: "archive-hourly/v1",
  maximumChunkDays: 14,
};
const configuredSource = {
  active: true,
  adapterConfig,
  cadenceSeconds: null,
  capabilities: ["historical"],
  fingerprint,
  key: "open-meteo-reanalysis-v1",
  sourceKind: "reanalysis",
};
const site = {
  provider: { key: "open-meteo" },
  site: {
    key: "ballydidean",
    latitude: 47.950429954185445,
    longitude: -122.42797012608193,
    timezone: "America/Los_Angeles",
  },
  sources: [configuredSource],
  station: { key: "open-meteo-virtual" },
};
const source = {
  id: sourceId,
  key: configuredSource.key,
  latitude: site.site.latitude,
  longitude: site.site.longitude,
  materialProviderConfig: adapterConfig,
  providerKey: site.provider.key,
  siteSlug: site.site.key,
  sourceConfigFingerprint: fingerprint,
  sourceKey: configuredSource.key,
  sourceKind: configuredSource.sourceKind,
  stationSlug: site.station.key,
  timezone: site.site.timezone,
};
const now = () => new Date("2026-08-22T05:20:00.000Z");

// build one exact original CLI argument record
function argumentsFor(overrides = {}) {
  return {
    chunkDays: 14,
    dryRun: false,
    from: "2026-03-01",
    reportPath: null,
    resume: false,
    site: site.site.key,
    source: null,
    to: "2026-03-15",
    ...overrides,
  };
}

// project one identity without introducing a document codec
function identityFields(identity) {
  return [
    identity.adapterVersion,
    identity.chunkPlanVersion,
    identity.intervalStart,
    identity.intervalEndExclusive,
    identity.requestedFromDate,
    identity.requestedToDate,
    identity.sourceConfigFingerprint,
    identity.sourceId,
  ];
}

// retain one stable tab-separated oracle row
function row(label, ...fields) {
  return [label, ...fields.map(String)].join("\t");
}

const rows = [];
const planned = planBackfillChunks({
  chunkDays: 14,
  from: "2026-03-01",
  sourceConfigFingerprint: fingerprint,
  sourceId,
  timezone: site.site.timezone,
  to: "2026-03-15",
});

// expose original planning across the spring offset transition
rows.push(...planned.map((chunk, index) => row(
  "PLAN",
  index,
  chunk.startDate,
  chunk.endDate,
  ...identityFields(chunk.identity),
)));
const springDay = planBackfillChunks({
  chunkDays: 1,
  from: "2026-03-08",
  sourceConfigFingerprint: fingerprint,
  sourceId,
  timezone: site.site.timezone,
  to: "2026-03-08",
})[0];
rows.push(row(
  "SPRING_DAY",
  springDay.startDate,
  springDay.endDate,
  ...identityFields(springDay.identity),
));

const dryEvents = [];
const dryRepository = {
  // reject writes during dry-run
  abandonExpiredRuns: async () => { throw new Error("dry-run wrote abandon"); },
  // reject locks during dry-run
  acquireSourceSession: async () => { throw new Error("dry-run acquired lock"); },
  // reject completion during dry-run
  completeBackfillIngestion: async () => { throw new Error("dry-run completed"); },
  // reject finalization during dry-run
  failIngestionRun: async () => { throw new Error("dry-run finalized"); },
  // record each exact-success read before planning
  hasSuccessfulBackfillChunk: async (_pool, identity) => {
    dryEvents.push(`resume:${identity.intervalStart}`);
    return false;
  },
  // reject starts during dry-run
  startIngestionRun: async () => { throw new Error("dry-run started"); },
};
const dryReport = await executeBackfill(
  {},
  argumentsFor({ dryRun: true, resume: true }),
  site,
  source,
  {
    // reject provider calls during dry-run
    fetchArchive: async () => { throw new Error("dry-run fetched"); },
    now,
    repository: dryRepository,
  },
);
rows.push(row(
  "DRY_REPORT",
  dryReport.exitCode,
  dryReport.dryRun,
  dryReport.resume,
  dryReport.from,
  dryReport.to,
  dryReport.site,
  dryReport.source,
  dryReport.chunks.map((chunk) => chunk.status).join(","),
));
rows.push(row("DRY_EVENTS", dryEvents.join("|")));

const lifecycleEvents = [];
let resumeRead = 0;
let run = 0;
let providerCall = 0;
const lifecycleRepository = {
  // retain cleanup before every attempted run
  abandonExpiredRuns: async (_session, startedAt) => {
    lifecycleEvents.push(`abandon:${Date.parse(startedAt)}`);
    return [];
  },
  // retain one scoped source lock per attempted chunk
  acquireSourceSession: async (_pool, selectedSourceId) => {
    lifecycleEvents.push(`lock:${selectedSourceId}`);
    return {
      // retain release after success and failure
      release: async () => {
        lifecycleEvents.push("release");
        throw new Error("cleanup delayed");
      },
    };
  },
  // retain successful provider attempts and exact identity
  completeBackfillIngestion: async (_session, input) => {
    lifecycleEvents.push([
      "complete",
      input.attempts,
      input.identity.intervalStart,
      input.runId,
      input.upstreamResponseChecksum,
    ].join(":"));
  },
  // retain failed provider attempts and exact identity
  failIngestionRun: async (_session, input) => {
    lifecycleEvents.push([
      "fail",
      input.attempts,
      input.backfillIdentity.intervalStart,
      input.error.code,
      input.runId,
      input.error.message,
    ].join(":"));
    throw new Error("finalization timed out");
  },
  // resume only the first exact chunk
  hasSuccessfulBackfillChunk: async (_pool, identity) => {
    resumeRead += 1;
    lifecycleEvents.push(`resume:${identity.intervalStart}`);
    return resumeRead === 1;
  },
  // retain global interval labels beside per-chunk request dates
  startIngestionRun: async (_session, input) => {
    run += 1;
    lifecycleEvents.push([
      "start",
      input.requestedStart,
      input.requestedEndExclusive,
      input.requestMetadata.start_date,
      input.requestMetadata.end_date,
      input.requestMetadata.endpoint,
      input.adapterVersion,
      input.chunkPlanVersion,
      Date.parse(input.deadlineAt),
    ].join(":"));
    return {
      id: `run-${run}`,
      startedAt: "2026-08-22T05:20:00.000Z",
      state: "running",
    };
  },
};
const lifecycleReport = await executeBackfill(
  {},
  argumentsFor({
    from: "2026-03-01",
    resume: true,
    to: "2026-04-12",
  }),
  site,
  source,
  {
    // complete one chunk then fail the next with retained attempts
    fetchArchive: async (input, options) => {
      providerCall += 1;
      lifecycleEvents.push([
        "fetch",
        providerCall,
        input.startDate,
        input.endDate,
        input.timezone,
        input.sourceId,
        Date.parse(options.deadlineAt),
      ].join(":"));
      // stop after one ordinary successful provider batch
      if (providerCall === 1) {
        return {
          attempts: 2,
          checksum: "d".repeat(64),
          records: [{ validAt: "2026-03-15T07:00:00.000Z" }],
          responseMetadata: { fixture: "archive" },
        };
      }
      return {
        attempts: 3,
        checksum: "d".repeat(64),
        records: [],
        responseMetadata: {},
      };
    },
    now,
    repository: lifecycleRepository,
  },
);
rows.push(row(
  "LIFECYCLE_REPORT",
  lifecycleReport.exitCode,
  lifecycleReport.dryRun,
  lifecycleReport.resume,
  lifecycleReport.from,
  lifecycleReport.to,
  lifecycleReport.site,
  lifecycleReport.source,
  lifecycleReport.chunks.map((chunk) => chunk.status).join(","),
  lifecycleReport.chunks.map((chunk) => chunk.errorCode ?? "<null>").join(","),
  lifecycleReport.chunks.map((chunk) => chunk.secondaryError ?? "<null>").join(","),
));
rows.push(...lifecycleReport.chunks.map((chunk, index) => row(
  "LIFECYCLE_CHUNK",
  index,
  chunk.status,
  chunk.errorCode ?? "<null>",
  ...identityFields(chunk.identity),
)));
rows.push(row("LIFECYCLE_EVENTS", lifecycleEvents.join("|")));

process.stdout.write(`${rows.join("\n")}\n`);
