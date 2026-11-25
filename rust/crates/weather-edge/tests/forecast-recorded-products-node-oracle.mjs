import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import vm from "node:vm";

import { ForecastObservationCache } from "../../../../deploy/scripts/forecast-observations.mjs";
import {
  fromSiteWallClock,
  toSiteWallClock,
} from "../../../../apps/web/dist/index.js";

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
  return sourceBlock(source, start, opening);
}

// extract one balanced executable block from a known opening brace
function sourceBlock(source, start, opening) {
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

      // return the first complete executable block
      if (depth === 0) {
        return source.slice(start, index + 1);
      }
    }
  }

  throw new Error("unterminated source block");
}

const routeMarker = 'if (requestUrl.pathname === "/api/v1/sites/ballydidean/forecast-observations" || requestUrl.pathname === "/api/v1/sites/ballydidean/solar-cloud-evidence")';
const routeStart = webSource.indexOf(routeMarker);
assert.notEqual(routeStart, -1);
const routeBlock = sourceBlock(
  webSource,
  routeStart,
  webSource.indexOf("{", routeStart + routeMarker.length),
);
const routePayload = {
  generatedAt: "2026-08-28T12:00:00.000Z",
  data: [{ recorded: "observations" }],
  solarCloudEvidence: { recorded: "solar" },
};
// compile the route with mutable fixture bindings in one shared context
const routeContext = vm.createContext({
  Buffer,
  Error,
  JSON,
  routePayload,
});
new vm.Script(`
let routeFailure = false;
const forecastObservationCache = {
  async get() {
    if (routeFailure) throw new Error("archive gap");
    return routePayload;
  },
};
function setSecurityHeaders() {}
${sourceFunction(webSource, "function sendText(response, status, body, headers = {})")}
async function serveRecorded(request, response, requestUrl) {
  ${routeBlock}
  throw new Error("recorded route did not match");
}
this.serveRecorded = serveRecorded;
this.failRoute = () => { routeFailure = true; };
`).runInContext(routeContext);

// execute one source route into a recording response adapter
async function executeRoute(method, pathname, search = "") {
  const record = { body: undefined, headers: undefined, status: undefined };
  const response = {
    // retain only protocol headers outside this policy oracle
    setHeader() {},
    // record one final status and ordinary headers
    writeHead(status, headers) {
      record.status = status;
      record.headers = headers;
    },
    // record the optional response body
    end(body) {
      record.body = body;
    },
  };
  await routeContext.serveRecorded({ method }, response, { pathname, search });
  return record;
}

// render one route response signature
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
    body,
  ].join("|");
}

// build one exact empty-record archive input for the current fixture instant
function observationInput(now) {
  const generatedAt = now.toISOString();
  const localDay = toSiteWallClock(generatedAt, "America/Los_Angeles").slice(0, 10);
  const projectionFrom = fromSiteWallClock(`${localDay}T00:00`, "America/Los_Angeles");
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

// return one complete bounded fake archive response
function archiveResponse(now) {
  return new Response(JSON.stringify(observationInput(now)), {
    headers: { "content-type": "application/json" },
    status: 200,
  });
}

const observationsPath = "/api/v1/sites/ballydidean/forecast-observations";
const solarPath = "/api/v1/sites/ballydidean/solar-cloud-evidence";
const unsupported = await executeRoute("POST", observationsPath);
const query = await executeRoute("GET", observationsPath, "?from=browser");
const observations = await executeRoute("GET", observationsPath);
const observationsHead = await executeRoute("HEAD", observationsPath);
const solar = await executeRoute("GET", solarPath);
routeContext.failRoute();
const archiveGap = await executeRoute("GET", observationsPath);

let now = new Date("2026-08-28T12:00:00.000Z");
let cacheFetches = 0;
const cache = new ForecastObservationCache({
  origin: "http://fixture.invalid",
  now: () => new Date(now),
  // provide one complete local archive input without network access
  fetcher: async () => {
    cacheFetches += 1;
    return archiveResponse(now);
  },
});
const cachedFirst = await cache.get();
const cachedSecond = await cache.get();
now = new Date(now.getTime() + 5 * 60_000 - 1);
const cachedBeforeExpiry = await cache.get();
now = new Date(now.getTime() + 1);
const cachedRefreshed = await cache.get();

now = new Date("2026-08-28T12:00:00.000Z");
let releasePending;
const pendingGate = new Promise((resolve) => {
  releasePending = resolve;
});
let pendingFetches = 0;
const pendingCache = new ForecastObservationCache({
  origin: "http://fixture.invalid",
  now: () => new Date(now),
  // block one complete fill so concurrent readers can share it
  fetcher: async () => {
    pendingFetches += 1;
    await pendingGate;
    return archiveResponse(now);
  },
});
const pendingFirstRequest = pendingCache.get();
const pendingSecondRequest = pendingCache.get();
releasePending();
const [pendingFirst, pendingSecond] = await Promise.all([
  pendingFirstRequest,
  pendingSecondRequest,
]);

now = new Date("2026-08-28T12:00:00.000Z");
let failArchive = true;
let retryFetches = 0;
const retryCache = new ForecastObservationCache({
  origin: "http://fixture.invalid",
  now: () => new Date(now),
  retryMs: 30_000,
  // fail one fill before allowing exact-boundary recovery
  fetcher: async () => {
    retryFetches += 1;
    if (failArchive) throw new Error("archive gap");
    return archiveResponse(now);
  },
});
let initialFailure;
let cooldownFailure;
try {
  await retryCache.get();
} catch (error) {
  initialFailure = error.message;
}
try {
  await retryCache.get();
} catch (error) {
  cooldownFailure = error.message;
}
failArchive = false;
now = new Date(now.getTime() + 30_000);
const retryRecovered = await retryCache.get();

now = new Date("2026-08-29T06:59:59.000Z");
let rolloverFetches = 0;
const rolloverCache = new ForecastObservationCache({
  origin: "http://fixture.invalid",
  now: () => new Date(now),
  // rebuild when the admitted farm-local day changes inside the ttl
  fetcher: async () => {
    rolloverFetches += 1;
    return archiveResponse(now);
  },
});
const rolloverBefore = await rolloverCache.get();
now = new Date(now.getTime() + 1_000);
const rolloverAfter = await rolloverCache.get();

process.stdout.write(`unsupported=${responseSignature(unsupported)}\n`);
process.stdout.write(`query=${responseSignature(query)}\n`);
process.stdout.write(`observations=${responseSignature(observations)}\n`);
process.stdout.write(`observations-head=${responseSignature(observationsHead)}\n`);
process.stdout.write(`solar=${responseSignature(solar)}\n`);
process.stdout.write(`archive-gap=${responseSignature(archiveGap)}\n`);
process.stdout.write(`cache=${cachedFirst.data.length}/${cachedFirst === cachedSecond}/${cachedFirst === cachedBeforeExpiry}/${cachedRefreshed.data.length}:fetches=${cacheFetches}\n`);
process.stdout.write(`pending=${pendingFirst.data.length}/${pendingFirst === pendingSecond}:fetches=${pendingFetches}\n`);
process.stdout.write(`retry=${initialFailure}:${cooldownFailure}:${retryRecovered.data.length}:fetches=${retryFetches}\n`);
process.stdout.write(`rollover=${rolloverBefore.generatedAt}/${rolloverAfter.generatedAt}:fetches=${rolloverFetches}\n`);
