import {
  buildOpenMeteoPreviousRunsRequest,
  fetchOpenMeteoPreviousRuns,
} from "../../../../packages/providers/dist/open-meteo-previous-runs.js";

const bases = [
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
const hours = Array.from(
  { length: 24 },
  (_unused, hour) => `2026-03-08T${String(hour).padStart(2, "0")}:00`,
);
const hourly = { time: hours };
const hourlyUnits = { time: "iso8601" };

// materialize all 63 original base-major fixed-lead fields
for (const [base, unit] of bases) {
  for (let day = 1; day <= 7; day += 1) {
    const field = `${base}_previous_day${day}`;
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

const body = {
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
    sourceConfigFingerprint: "a".repeat(64),
    sourceId: "source-previous-runs",
  }],
  startDate: "2026-03-08",
};
const plan = buildOpenMeteoPreviousRunsRequest(request);
const batch = await fetchOpenMeteoPreviousRuns(request, {
  fetch: async () => new Response(JSON.stringify(body), { status: 201 }),
  now: () => new Date("2026-08-22T05:20:00.000Z"),
});

// expose one compact stable witness for the Rust differential fixture
if (process.argv[2] === "--summary") {
  process.stdout.write([
    `parameters=${[...plan.url.searchParams.entries()].length}`,
    `hourlyFields=${plan.url.searchParams.get("hourly").split(",").length}`,
    `attempts=${batch.attempts}`,
    `recordCount=${batch.records.length}`,
    `requested=${batch.responseMetadata.requested_anchor_count}`,
    `populated=${batch.responseMetadata.populated_anchor_count}`,
    `status=${batch.responseMetadata.http_status}`,
    `generationMs=${batch.responseMetadata.generation_ms}`,
    `first=${batch.records[0].leadHours}|${batch.records[0].metrics.temperatureC}|${batch.records[0].metrics.windDirectionDegrees}|${batch.records[0].validAt}`,
    "",
  ].join("\n"));
  process.exit(0);
}

process.stdout.write(JSON.stringify({
  attempts: batch.attempts,
  first: {
    leadHours: batch.records[0].leadHours,
    temperatureC: batch.records[0].metrics.temperatureC,
    validAt: batch.records[0].validAt,
    windDirectionDegrees: batch.records[0].metrics.windDirectionDegrees,
  },
  generationMs: batch.responseMetadata.generation_ms,
  hourlyFields: plan.url.searchParams.get("hourly").split(","),
  parameters: [...plan.url.searchParams.entries()],
  populated: batch.responseMetadata.populated_anchor_count,
  recordCount: batch.records.length,
  requested: batch.responseMetadata.requested_anchor_count,
  status: batch.responseMetadata.http_status,
}));
