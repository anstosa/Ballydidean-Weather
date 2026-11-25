import assert from "node:assert/strict";

import { ForecastObservationCache } from "../../../../deploy/scripts/forecast-observations.mjs";
import {
  fromSiteWallClock,
  toSiteWallClock,
} from "../../../../apps/web/dist/index.js";

const now = new Date("2026-08-28T12:00:00.000Z");

// Build the exact valid empty private envelope for the fixed farm day.
function input() {
  const generatedAt = now.toISOString();
  const localDay = toSiteWallClock(generatedAt, "America/Los_Angeles").slice(0, 10);
  const projectionFrom = fromSiteWallClock(
    `${localDay}T00:00`,
    "America/Los_Angeles",
  );
  const queryFrom = new Date(Math.min(
    Date.parse(projectionFrom) - 3 * 3_600_000,
    Date.parse(generatedAt) - 24 * 3_600_000 - 20 * 60_000,
  )).toISOString();
  return {
    contractVersion: "forecast-observation-input/v1",
    generatedAt,
    projectionFrom,
    queryFrom,
    records: [],
    site: { slug: "ballydidean", timezone: "America/Los_Angeles" },
    through: generatedAt,
  };
}

let request;
const cache = new ForecastObservationCache({
  origin: "http://fixture.invalid",
  now: () => new Date(now),
  // Capture the real fixed request and return a disposable in-memory response.
  fetcher: async (url, options) => {
    request = { options, url: new URL(url) };
    return new Response(JSON.stringify(input()), { status: 200 });
  },
});
const product = await cache.get();
assert.equal(request.url.pathname, "/internal/v1/forecast-observation-input");
assert.equal(request.options.method ?? "GET", "GET");
assert.equal(request.options.headers.accept, "application/json");
assert.equal(request.options.redirect, "error");
assert.equal(product.generatedAt, now.toISOString());
assert.equal(product.data.length, 5);
assert.equal(product.solarCloudEvidence, null);

let capError;
const capped = new ForecastObservationCache({
  origin: "http://fixture.invalid",
  maxBytes: 1,
  now: () => new Date(now),
  // Return a valid envelope whose first chunk exceeds the configured cap.
  fetcher: async () => new Response(JSON.stringify(input()), { status: 200 }),
});
try {
  await capped.get();
} catch (error) {
  capError = error.message;
}
assert.equal(capError, "Forecast observation input limit exceeded");

process.stdout.write(
  `node-fill=${request.url.pathname}|${request.options.method ?? "GET"}|${request.options.headers.accept}|${request.options.redirect}|${product.data.length}|${product.solarCloudEvidence === null}\n`,
);
process.stdout.write(`node-cap=${capError}\n`);
