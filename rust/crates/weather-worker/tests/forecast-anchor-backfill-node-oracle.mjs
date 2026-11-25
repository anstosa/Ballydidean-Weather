import {
  executeBackfill,
  parseBackfillArguments,
  planPreviousRunsBackfillChunks,
} from "../../../../apps/worker/dist/backfill-cli.js";
import {
  fetchOpenMeteoPreviousRuns,
  normalizeOpenMeteoPreviousRunsPayload,
} from "../../../../packages/providers/dist/open-meteo-previous-runs.js";

const fingerprint = "a".repeat(64);
const sourceId = "source-previous-runs";
const adapterConfig = {
  contractEpoch: "open-meteo-previous-runs-best-match/2026-09",
  contractVersion: "previous-runs-hourly/v1",
  leadHours: [24, 48, 72, 96, 120, 144, 168],
  maximumChunkDays: 14,
  model: "best_match",
  variables: [
    "temperature",
    "apparent_temperature",
    "relative_humidity",
    "precipitation",
    "cloud_cover",
    "wind_speed",
    "wind_gust",
    "wind_direction",
    "surface_pressure",
  ],
};
const configuration = {
  active: true,
  adapterConfig,
  cadenceSeconds: null,
  capabilities: ["historical"],
  fingerprint,
  key: "open-meteo-previous-runs-v1",
  sourceKind: "forecast",
};
const site = {
  provider: { key: "open-meteo" },
  site: {
    key: "ballydidean",
    latitude: 48.1,
    longitude: -122.7,
    timezone: "America/Los_Angeles",
  },
  sources: [configuration],
  station: { key: "ballydidean-primary" },
};
const source = {
  active: true,
  cadenceSeconds: null,
  id: sourceId,
  key: configuration.key,
  latitude: site.site.latitude,
  longitude: site.site.longitude,
  materialProviderConfig: adapterConfig,
  providerKey: site.provider.key,
  siteSlug: site.site.key,
  sourceConfigFingerprint: fingerprint,
  sourceKey: configuration.key,
  sourceKind: "forecast",
  stationSlug: site.station.key,
  timezone: site.site.timezone,
};
const planned = planPreviousRunsBackfillChunks({
  chunkDays: 14,
  from: "2026-03-01",
  sourceConfigFingerprint: fingerprint,
  sourceId,
  to: "2026-03-15",
});
const cli = parseBackfillArguments(
  [
    "--site", "ballydidean",
    "--source", "open-meteo-previous-runs-v1",
    "--from", "2026-03-08",
    "--to", "2026-03-08",
    "--chunk-days", "14",
    "--resume",
    "--report", "report.json",
  ],
  new Date("2026-08-22T00:00:00.000Z"),
);
const events = [];
let started;
let completed;
const repository = {
  abandonExpiredRuns: async () => {
    events.push("abandon");
  },
  acquireSourceSession: async () => {
    events.push("lock");
    return {
      release: async () => {
        events.push("release");
      },
      sourceId,
    };
  },
  completeBackfillIngestion: async () => {
    throw new Error("wrong completion route");
  },
  completeForecastAnchorBackfillIngestion: async (_session, input) => {
    events.push("complete");
    completed = input;
  },
  failIngestionRun: async () => {
    throw new Error("unexpected failure finalization");
  },
  hasSuccessfulBackfillChunk: async () => false,
  startIngestionRun: async (_session, input) => {
    events.push("start");
    started = input;
    return { id: "run-1", startedAt: "2026-08-22T05:20:00.000Z", state: "running" };
  },
};
const report = await executeBackfill(
  {},
  {
    chunkDays: 14,
    dryRun: false,
    from: "2026-03-08",
    reportPath: null,
    resume: false,
    site: "ballydidean",
    source: configuration.key,
    to: "2026-03-08",
  },
  site,
  source,
  {
    fetchPreviousRuns: async (input) => {
      events.push("fetch");
      const location = input.locations[0];
      return {
        attempts: 2,
        checksum: "d".repeat(64),
        providerCursor: null,
        records: [{
          sourceConfigFingerprint: location.sourceConfigFingerprint,
          sourceId: location.sourceId,
          validAt: `${input.startDate}T00:00:00.000Z`,
        }],
        responseMetadata: { populated_anchor_count: 1 },
      };
    },
    now: () => new Date("2026-08-22T05:20:00.000Z"),
    repository,
  },
);
const baseVariables = [
  ["temperature_2m", "°C"],
  ["apparent_temperature", "°C"],
  ["relative_humidity_2m", "%"],
  ["precipitation", "mm"],
  ["cloud_cover", "%"],
  ["wind_speed_10m", "m/s"],
  ["wind_gusts_10m", "m/s"],
  ["wind_direction_10m", "°"],
  ["surface_pressure", "hPa"],
];
const hourly = {
  time: Array.from({ length: 24 }, (_, hour) => `2026-03-08T${String(hour).padStart(2, "0")}:00`),
};
const hourlyUnits = { time: "iso8601" };

// create every exact requested lead array and unit
for (const [variable, unit] of baseVariables) {
  // retain fixed lead order inside each base variable
  for (let day = 1; day <= 7; day += 1) {
    const field = `${variable}_previous_day${day}`;
    hourly[field] = Array(24).fill(null);
    hourlyUnits[field] = unit;
  }
}
hourly.temperature_2m_previous_day1[0] = 10;
hourly.apparent_temperature_previous_day1[0] = 9;
hourly.relative_humidity_2m_previous_day1[0] = 75;
hourly.precipitation_previous_day1[0] = 0;
hourly.cloud_cover_previous_day1[0] = 50;
hourly.wind_speed_10m_previous_day1[0] = 2;
hourly.wind_gusts_10m_previous_day1[0] = 4;
hourly.wind_direction_10m_previous_day1[0] = 360;
hourly.surface_pressure_previous_day1[0] = 1012;
const rawResponse = {
  elevation: 12.5,
  generationtime_ms: 1.25,
  hourly,
  hourly_units: hourlyUnits,
  latitude: 48.1,
  longitude: -122.7,
  timezone: "UTC",
  utc_offset_seconds: 0,
};
const request = {
  contractEpoch: "open-meteo-previous-runs-best-match/2026-09",
  endDate: "2026-03-08",
  locations: [{
    latitude: 48.1,
    longitude: -122.7,
    sourceConfigFingerprint: fingerprint,
    sourceId,
  }],
  startDate: "2026-03-08",
};
const receiptTime = "2026-08-22T05:20:00.000Z";
const normalized = normalizeOpenMeteoPreviousRunsPayload(rawResponse, request, receiptTime);

// expose exact response metadata semantics without adding a rust-side test codec
function writeMetadataFields(value, prefix) {
  // visit all original outer and per-lead membership in source order
  for (const [key, field] of Object.entries(value)) {
    const path = prefix === "" ? key : `${prefix}.${key}`;
    // walk the nested lead counters or print one scalar comparison row
    if (typeof field === "object" && field !== null) {
      writeMetadataFields(field, path);
    } else {
      process.stdout.write(`${path}\t${String(field)}\n`);
    }
  }
}

// exercise the original fetch result with disposable response bytes and no socket
if (process.argv[2] === "--response-metadata") {
  const batch = await fetchOpenMeteoPreviousRuns(request, {
    fetch: async () => new Response(JSON.stringify(rawResponse), { status: 201 }),
    now: () => new Date(receiptTime),
  });
  writeMetadataFields(batch.responseMetadata, "");
  process.exit(0);
}

process.stdout.write(JSON.stringify({
  cli,
  completeAttempts: completed.attempts,
  completeRecordCount: completed.records.length,
  events,
  firstEnd: planned[0].identity.intervalEndExclusive,
  firstStart: planned[0].identity.intervalStart,
  normalized: {
    count: normalized.length,
    leadHours: normalized[0].leadHours,
    temperatureC: normalized[0].metrics.temperatureC,
    validAt: normalized[0].validAt,
    windDirectionDegrees: normalized[0].metrics.windDirectionDegrees,
  },
  planCount: planned.length,
  secondStart: planned[1].identity.intervalStart,
  startDeadline: started.deadlineAt,
  startEndpoint: started.requestMetadata.endpoint,
  startMode: started.mode,
  statuses: report.chunks.map((chunk) => chunk.status),
}));
