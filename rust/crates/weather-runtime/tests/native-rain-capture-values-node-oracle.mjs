import { createHash } from "node:crypto";
import { weatherRecordContent } from "../../../../packages/domain/dist/weather-record.js";
import { readFile } from "node:fs/promises";
import { normalizeTempestObservationPayload } from "../../../../packages/providers/dist/tempest.js";
import { rainForecastProfile } from "../../../../apps/worker/dist/rain-adjustment.js";

// execute the actual predecessor normalization on its checked provider response
const fixture = JSON.parse(await readFile(new URL("../../../../packages/providers/test/fixtures/tempest/observations.json", import.meta.url), "utf8"));
const receivedAt = "2026-08-20T12:00:00.000Z";
const records = normalizeTempestObservationPayload(fixture, {
  deviceId: 470937, locationId: 203055, serial: "ST-00198967", sourceId: "source-tempest",
  timezone: "America/Los_Angeles", start: new Date(1787184000000).toISOString(), endExclusive: new Date(1787191200000).toISOString(),
}, receivedAt);
const tempestHashes = records.map(record => createHash("sha256").update(weatherRecordContent(record)).digest("hex"));
const tempest = records.map(record => [Date.parse(record.validAt), record.metrics.temperatureC,record.metrics.precipitationMm,record.metrics.precipitationRateMmPerHour,record.metrics.windSpeedMps,record.metrics.windDirectionDegrees,record.metrics.uvIndex,record.metadata.provider.report_interval_minutes]);
// preserve unused initialization and extra cells while varying the 48 used leads
const initialized = Date.parse("2026-09-14T00:00:00.000Z");
const hourly = {time: Array.from({length:49}, (_,i)=>new Date(initialized+i*3600000).toISOString().slice(0,16))};
for (const [i, key] of ["precipitation", "temperature_2m", "relative_humidity_2m", "cloud_cover", "surface_pressure", "wind_speed_10m", "wind_direction_10m"].entries()) {
  hourly[key]=Array.from({length:49},(_,lead)=>lead%7===0 ? null : i+lead/4);
}
const body = JSON.stringify({utc_offset_seconds:0,hourly});
const capture = {kind:"forecast",runInitializedAt:new Date(initialized).toISOString(),completedAt:new Date(initialized+3600000).toISOString(),body:Buffer.from(body)};
const profile = rainForecastProfile(capture);
const forecast = profile.hours.map(hour=>[hour.leadHours,hour.precipitationMm,hour.temperatureC,hour.relativeHumidityPercent,hour.cloudCoverPercent,hour.pressureHpa,hour.windSpeedMps,hour.windDirectionDegrees]);
process.stdout.write(JSON.stringify({tempest,tempestHashes,body,forecast})+"\n");
