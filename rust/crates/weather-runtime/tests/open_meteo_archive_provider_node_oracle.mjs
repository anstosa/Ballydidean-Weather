import {
  buildOpenMeteoArchiveRequest,
  fetchOpenMeteoArchive,
} from "../../../../packages/providers/dist/open-meteo.js";

const hours = [
  "2026-03-08T07:00",
  "2026-03-08T08:00",
  "2026-03-09T06:00",
  "2026-03-09T07:00",
];
const fields = [
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
const hourly = { time: hours };
const hourlyUnits = {};
// include zero, null, circular north and upstream padding in their original positions
for (const [key, unit] of fields) {
  hourly[key] = Array(hours.length).fill(null);
  hourlyUnits[key] = unit;
}
hourly.temperature_2m = [5, 0, null, 8];
hourly.wind_direction_10m[1] = 360;
hourly.precipitation[2] = 1.25;
const payload = {
  elevation: 12.5,
  generationtime_ms: -1.25,
  hourly,
  hourly_units: hourlyUnits,
  latitude: 48.1,
  longitude: -122.7,
  timezone: "UTC",
  utc_offset_seconds: 0,
};
const request = {
  startDate: "2026-03-08",
  endDate: "2026-03-08",
  sourceId: "source-reanalysis",
  latitude: 48.1,
  longitude: -122.7,
  timezone: "America/Los_Angeles",
};
const plan = buildOpenMeteoArchiveRequest(request);
const batch = await fetchOpenMeteoArchive(request, {
  fetch: async () => new Response(JSON.stringify(payload), { status: 201 }),
  now: () => new Date("2026-08-22T05:20:00.000Z"),
});
const rows = [
  [plan.url.origin + plan.url.pathname],
  ...plan.url.searchParams.entries(),
  [batch.attempts, batch.providerCursor, batch.responseMetadata.http_status, batch.responseMetadata.generation_ms],
  ...batch.records.map(
    // compare original metadata and canonical record fields without a rust-side codec
    (record) => [
      record.validAt, record.receivedAt, record.sourceKind, record.sourceId,
      record.productRunAt, record.metadata.model, record.metadata.provider.dataset,
      record.metadata.provider.elevation_m, record.metadata.provider.grid_cell,
      record.metadata.upstreamTimezone, record.metrics.temperatureC,
      record.metrics.windDirectionDegrees, record.metrics.precipitationMm,
      record.metrics.precipitationRateMmPerHour,
    ],
  ),
];
process.stdout.write(rows.map((row) => row.map(String).join("\t")).join("\n"));
