import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import vm from "node:vm";

import { XweatherTileMemoryCache } from "../../../../deploy/scripts/xweather-tile-cache.mjs";
import { XweatherUsageBudget } from "../../../../deploy/scripts/xweather-usage-budget.mjs";

const repositoryRoot = new URL("../../../../", import.meta.url);
const webSource = await readFile(
  new URL("deploy/scripts/web-server.mjs", repositoryRoot),
  "utf8",
);

// extract one complete source function
function sourceFunction(source, signature) {
  const start = source.indexOf(signature);
  assert.notEqual(start, -1);
  const opening = source.indexOf("{", start + signature.length);
  let depth = 0;
  let quote = null;
  let escaped = false;

  // scan strings while balancing every executable block
  for (let index = opening; index < source.length; index += 1) {
    const character = source[index];

    // retain escaped string characters without balancing braces
    if (quote !== null) {
      // advance the active string state
      if (escaped) {
        escaped = false;
      } else if (character === "\\") {
        escaped = true;
      } else if (character === quote) {
        quote = null;
      }
      continue;
    }

    // enter one ordinary source string
    if (character === "\"" || character === "'" || character === "`") {
      quote = character;
      continue;
    }

    // balance the executable block
    if (character === "{") {
      depth += 1;
    } else if (character === "}") {
      depth -= 1;

      // return the first complete function block
      if (depth === 0) {
        return source.slice(start, index + 1);
      }
    }
  }

  throw new Error(`unterminated source function: ${signature}`);
}

let now = Date.parse("2026-08-28T12:00:00Z");
let providerCalls = 0;
const budget = new XweatherUsageBudget({
  dailyLimit: 3,
  monthlyLimit: 5,
  now: () => now,
});
const cache = new XweatherTileMemoryCache({
  forecastFreshnessMs: 60 * 60 * 1_000,
  maximumBytes: 1_024,
  now: () => now,
  // reserve before one fake provider attempt
  async loadTile(tile) {
    const units = Math.ceil(tile.width / 256) * Math.ceil(tile.height / 256);
    budget.reserve(units);
    providerCalls += 1;

    // inject one ordinary upstream failure after consuming its attempt
    if (tile.layer === "wind") {
      throw new Error("provider failed");
    }

    return { body: Buffer.from(`${tile.phase}-${tile.layer}`) };
  },
});

class FixtureDate extends Date {
  // fix only no-argument construction at the admitted request clock
  constructor(value) {
    super(value === undefined ? now : value);
  }

  // expose the admitted request clock to the source parser
  static now() {
    return now;
  }
}

const context = vm.createContext({
  Buffer,
  Date: FixtureDate,
  Math,
  Number,
  String,
  cache,
});
new vm.Script(`
const XWEATHER_FRAME_PATTERN = /^\\/maps\\/xweather\\/(history|forecast)\\/(radar|clouds|precipitation|wind)\\/(\\d{14})\\/(\\d{1,2})\\/(\\d{3,4})x(\\d{3,4})\\/(-?\\d{1,2}(?:\\.\\d{1,6})?),(-?\\d{1,3}(?:\\.\\d{1,6})?)\\.png$/u;
const forecastMapPreloadSite = { latitude: 47.95, longitude: -122.43 };
const xweatherTileCache = cache;
let xweatherCredentials = null;
function setSecurityHeaders() {}
${sourceFunction(webSource, "function secondsUntilUtcDay()")}
${sourceFunction(webSource, "function parseXweatherMapImage(pathname)")}
${sourceFunction(webSource, "function parseXweatherFrame(pathname)")}
${sourceFunction(webSource, "function parseXweatherValidTime(value)")}
${sourceFunction(webSource, "function sendText(response, status, body, headers = {})")}
${sourceFunction(webSource, "async function proxyXweatherMap(request, response, requestUrl)")}
this.enableProvider = () => { xweatherCredentials = {}; };
this.proxy = proxyXweatherMap;
`).runInContext(context);

// execute one source proxy request into a recording response adapter
async function execute(method, pathname) {
  const record = { body: undefined, headers: undefined, status: undefined };
  const response = {
    // retain only protocol headers outside this policy oracle
    setHeader() {},
    // record one final response status and ordinary headers
    writeHead(status, headers) {
      record.status = status;
      record.headers = headers;
    },
    // record the optional response body
    end(body) {
      record.body = body;
    },
  };
  await context.proxy({ method }, response, { pathname });
  return record;
}

// render one protocol-independent response signature
function responseSignature(response) {
  const headers = response.headers;
  const body = response.body === undefined ? "-" : Buffer.from(response.body).toString("utf8").trimEnd();
  return [
    response.status,
    headers["Cache-Control"],
    headers["Content-Type"],
    headers["Content-Length"],
    headers.Allow ?? "-",
    headers["Retry-After"] ?? "-",
    headers["X-Weather-Tile-Age"] ?? "-",
    headers["X-Weather-Tile-Cache"] ?? "-",
    body,
  ].join("|");
}

const forecastRadar = "/maps/xweather/forecast/radar/20260828130000/10/256x168/47.95,-122.43.png";
const historyRadar = "/maps/xweather/history/radar/20260828115000/10/256x168/47.95,-122.43.png";
const forecastWind = "/maps/xweather/forecast/wind/20260828130000/10/256x168/47.95,-122.43.png";
const forecastClouds = "/maps/xweather/forecast/clouds/20260828130000/10/256x168/47.95,-122.43.png";
const unsupported = await execute("POST", forecastRadar);
const missing = await execute("GET", "/maps/xweather/not-a-frame");
const invalidGeometry = await execute("GET", forecastRadar.replace("256x168", "257x168"));
const invalidCenter = await execute("GET", forecastRadar.replace("47.95,-122.43", "47.96,-122.43"));
const invalidTime = await execute("GET", forecastRadar.replace("20260828130000", "20260830150000"));
const unavailable = await execute("GET", forecastRadar);
context.enableProvider();
const forecastMiss = await execute("GET", forecastRadar);
now += 2_000;
const forecastHead = await execute("HEAD", forecastRadar);
const historyMiss = await execute("GET", historyRadar);
now += 3_000;
const historyHit = await execute("GET", historyRadar);
const upstreamFailure = await execute("GET", forecastWind);
const exhausted = await execute("GET", forecastClouds);
now += 1_000;
const cachedAfterExhaustion = await execute("GET", forecastRadar);
const snapshot = budget.snapshot();

process.stdout.write(`unsupported=${responseSignature(unsupported)}\n`);
process.stdout.write(`missing=${responseSignature(missing)}\n`);
process.stdout.write(`invalid-geometry=${responseSignature(invalidGeometry)}\n`);
process.stdout.write(`invalid-center=${responseSignature(invalidCenter)}\n`);
process.stdout.write(`invalid-time=${responseSignature(invalidTime)}\n`);
process.stdout.write(`unavailable=${responseSignature(unavailable)}\n`);
process.stdout.write(`forecast-miss=${responseSignature(forecastMiss)}\n`);
process.stdout.write(`forecast-head=${responseSignature(forecastHead)}\n`);
process.stdout.write(`history-miss=${responseSignature(historyMiss)}\n`);
process.stdout.write(`history-hit=${responseSignature(historyHit)}\n`);
process.stdout.write(`upstream-failure=${responseSignature(upstreamFailure)}\n`);
process.stdout.write(`exhausted=${responseSignature(exhausted)}\n`);
process.stdout.write(`cached-after-exhaustion=${responseSignature(cachedAfterExhaustion)}\n`);
process.stdout.write(`usage=${snapshot.dayUnits}/${snapshot.monthUnits}:provider-calls=${providerCalls}\n`);
