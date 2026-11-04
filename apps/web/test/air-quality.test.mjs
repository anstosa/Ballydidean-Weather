import assert from "node:assert/strict";
import test from "node:test";

import {
  airQualityDayKey,
  dailyForecastAqi,
  pm25NowCastAqi,
  pm25ToAqi,
} from "../dist/units.js";

const HOUR_MS = 3_600_000;

// build one newest-first hourly concentration context
function context(hourlyPm25, through = "2026-11-04T18:00:00.000Z") {
  return { hourlyPm25, through };
}

test("PM2.5 averages convert through the current EPA AQI breakpoints", () => {
  assert.equal(pm25ToAqi(null), null);
  assert.equal(pm25ToAqi(undefined), null);
  assert.equal(pm25ToAqi(Number.NaN), null);
  assert.equal(pm25ToAqi(-0.1), null);
  assert.equal(pm25ToAqi(9), 50);
  assert.equal(pm25ToAqi(9.09), 50);
  assert.equal(pm25ToAqi(9.1), 51);
  assert.equal(pm25ToAqi(15), 62);
  assert.equal(pm25ToAqi(35.4), 100);
  assert.equal(pm25ToAqi(35.5), 101);
  assert.equal(pm25ToAqi(55.4), 150);
  assert.equal(pm25ToAqi(55.5), 151);
  assert.equal(pm25ToAqi(125.4), 200);
  assert.equal(pm25ToAqi(125.5), 201);
  assert.equal(pm25ToAqi(225.4), 300);
  assert.equal(pm25ToAqi(225.5), 301);
  assert.equal(pm25ToAqi(325.4), 500);
  assert.equal(pm25ToAqi(325.5), 500);
  assert.equal(pm25ToAqi(325.7), 501);
});

test("PM2.5 NowCast retains wall-clock gaps and requires two recent hours", () => {
  const now = Date.parse("2026-11-04T19:30:00.000Z");
  assert.equal(pm25NowCastAqi(context(Array(12).fill(10)), now), 53);
  assert.equal(pm25NowCastAqi(context([
    21,
    null,
    35,
    49.2,
    48.6,
    53.7,
    66.2,
    69.2,
    64.9,
    50,
    43,
    34.9,
  ]), now), 87);
  assert.equal(pm25NowCastAqi(context([10, null, null, 10, 10]), now), null);
  assert.equal(pm25NowCastAqi(context([10, null, 10]), now), 53);
  assert.equal(pm25NowCastAqi(context(Array(12).fill(0)), now), 0);
  assert.equal(pm25NowCastAqi(context([10, Number.NaN, 10]), now), 53);
  assert.equal(pm25NowCastAqi(context([10, -1, null, 10]), now), null);
});

test("PM2.5 NowCast stays anchored to the latest completed hour", () => {
  const now = Date.parse("2026-11-04T19:30:00.000Z");
  assert.equal(pm25NowCastAqi(context([10, 10], "2026-11-04T16:00:00.000Z"), now), null);
  assert.equal(pm25NowCastAqi(context([10, 10], "2026-11-04T17:00:00.000Z"), now), 53);
  assert.equal(pm25NowCastAqi(context(Array(12).fill(10), "2026-11-04T18:30:00.000Z"), now), null);
  assert.equal(pm25NowCastAqi(context(Array(12).fill(10), "2026-11-04T19:00:00.000Z"), now), null);
  assert.equal(pm25NowCastAqi(context(Array(12).fill(10), "2026-11-04T06:00:00.000Z"), now), null);
});

test("AQI daily keys use fixed Pacific standard time across daylight saving", () => {
  assert.equal(airQualityDayKey("2026-01-15T07:30:00.000Z", "America/Los_Angeles"), "2026-01-14");
  assert.equal(airQualityDayKey("2026-01-15T08:00:00.000Z", "America/Los_Angeles"), "2026-01-15");
  assert.equal(airQualityDayKey("2026-07-01T07:30:00.000Z", "America/Los_Angeles"), "2026-06-30");
  assert.equal(airQualityDayKey("2026-07-01T08:00:00.000Z", "America/Los_Angeles"), "2026-07-01");
});

test("forecast AQI requires one complete 24-hour local-standard-time day", () => {
  const start = Date.parse("2026-07-01T08:00:00.000Z");
  const full = Array.from({ length: 24 },
    // populate one fixed-offset daily forecast block
    (_, index) => ({ concentration: 15, validAt: new Date(start + index * HOUR_MS).toISOString() }),
  );
  assert.deepEqual([...dailyForecastAqi(full, "America/Los_Angeles")], [["2026-07-01", 62]]);
  assert.deepEqual([...dailyForecastAqi(full.slice(0, 23), "America/Los_Angeles")], []);
  assert.deepEqual([...dailyForecastAqi([
    ...full,
    { concentration: 99, validAt: full[0].validAt },
  ], "America/Los_Angeles")], [["2026-07-01", 69]]);
  assert.deepEqual([...dailyForecastAqi(full.map(
    // invalidate one hour without silently averaging the remaining hours
    (sample, index) => index === 12 ? { ...sample, concentration: null } : sample,
  ), "America/Los_Angeles")], []);
});
