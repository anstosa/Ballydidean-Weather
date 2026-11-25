import { createHash } from "node:crypto";
import { gzipSync } from "node:zlib";

import { publishRainAdjustment } from "../../../../apps/worker/dist/rain-adjustment.js";

const initializedAt = new Date("2026-09-14T00:00:00.000Z");
const completedAt = new Date("2026-09-14T06:01:00.000Z");
const hourly = {
  time: [],
  temperature_2m: [],
  relative_humidity_2m: [],
  cloud_cover: [],
  surface_pressure: [],
  wind_speed_10m: [],
  wind_direction_10m: [],
  precipitation: [],
};

// build the exact 49-value Open-Meteo source geometry consumed by the worker
for (let lead = 0; lead <= 48; lead += 1) {
  hourly.time.push(new Date(initializedAt.getTime() + lead * 3_600_000).toISOString().slice(0, 16));
  hourly.temperature_2m.push(12);
  hourly.relative_humidity_2m.push(80);
  hourly.cloud_cover.push(90);
  hourly.surface_pressure.push(1_000);
  hourly.wind_speed_10m.push(4);
  hourly.wind_direction_10m.push(180);
  hourly.precipitation.push(0.5);
}
const body = Buffer.from(JSON.stringify({ hourly, utc_offset_seconds: 0 }));
let inserted;

const client = {
  // capture the exact public-safe serving insert while emulating transaction commands
  async query(sql, parameters = []) {
    if (sql.includes("INSERT INTO rain_adjustment_runs")) {
      const hours = JSON.parse(parameters[6]);
      inserted = { claimId: parameters[3], hours };
      return {
        rows: [{
          generatedAt: new Date("2026-09-14T08:00:00.000Z"),
          hours,
          inputSha256: parameters[2],
          modelSha256: parameters[1],
          runInitializedAt: initializedAt,
          storedContentSha256: "a".repeat(64),
        }],
      };
    }
    return { rows: [] };
  },
  // retain the production pool-client surface without external effects
  release() {},
};

const pool = {
  // return the production pending selection and its exact immutable body row
  async query(sql) {
    if (sql.includes("SELECT c.run_initialized_at")) {
      return { rows: [{ run_initialized_at: initializedAt }] };
    }
    if (sql.includes("SELECT c.id, c.kind")) {
      return {
        rows: [{
          id: "current",
          kind: "forecast",
          station_id: null,
          run_initialized_at: initializedAt,
          window_start: null,
          window_end_exclusive: null,
          completed_at: completedAt,
          body_sha256: createHash("sha256").update(body).digest("hex"),
          compressed_body: gzipSync(body),
        }],
      };
    }
    throw new Error("unexpected publisher oracle query");
  },
  // open the serving transaction only after normal inference succeeds
  async connect() {
    return client;
  },
};

const published = await publishRainAdjustment(pool, new Date("2026-09-14T08:00:00.000Z"));
const first = inserted.hours[0];
const last = inserted.hours.at(-1);
process.stdout.write(`${published}|${inserted.claimId}|${inserted.hours.length}|${first.modelLeadHours}|${last.modelLeadHours}`);
