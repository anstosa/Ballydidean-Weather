import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import vm from "node:vm";

const repositoryRoot = new URL("../../../../", import.meta.url);
const webSource = await readFile(
  new URL("deploy/scripts/web-server.mjs", repositoryRoot),
  "utf8",
);

// extract one source span between stable declarations
function sourceSpan(startMarker, endMarker) {
  const start = webSource.indexOf(startMarker);
  const end = webSource.indexOf(endMarker, start);

  // fail when either production boundary moved
  if (start < 0 || end < 0) {
    throw new Error(`unable to extract ${startMarker}`);
  }
  return webSource.slice(start, end);
}

// extract one complete named source function
function sourceFunction(signature) {
  const start = webSource.indexOf(signature);
  assert.notEqual(start, -1);
  const opening = webSource.indexOf("{", start + signature.length);
  let depth = 0;
  let quote = null;
  let escaped = false;

  // scan ordinary strings while balancing executable blocks
  for (let index = opening; index < webSource.length; index += 1) {
    const character = webSource[index];

    // retain escaped characters inside the active string
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

    // balance the current executable block
    if (character === "{") {
      depth += 1;
    } else if (character === "}") {
      depth -= 1;

      // return the first complete function block
      if (depth === 0) {
        return webSource.slice(start, index + 1);
      }
    }
  }

  throw new Error(`unterminated source function: ${signature}`);
}

const fixtureNow = "2026-09-12T15:15:01.000Z";

class FixtureDate extends Date {
  // fix only no-argument source time reads
  constructor(value) {
    super(value === undefined ? fixtureNow : value);
  }

  // expose the fixed edge clock
  static now() {
    return Date.parse(fixtureNow);
  }
}

const context = vm.createContext({
  Buffer,
  Date: FixtureDate,
  JSON,
  URL,
});
new vm.Script(`
const release = "2026.11.23-1";
const publicRoot = "public";
const compiledRoot = "compiled";
const webRevision = "fixture-revision";
const shellAssetDigests = { "/assets/2026.11.23-1/client.js": "sha256-fixture" };
const forecastMapPreloadSite = { latitude: 47.950429954185445, longitude: -122.42797012608193 };
const apiOrigin = new URL("http://fixture.invalid");
const maximumApiBytes = 1024 * 1024;
const maximumTrendsApiBytes = 2 * 1024 * 1024;
let mode = "ok";
let upstreamMethod = "-";
let upstreamTarget = "-";
let upstreamTimeout = "-";
const fixtureAssets = new Map([
  ["public/index.html", Buffer.from("version=__WEATHER_ASSET_VERSION__;revision=__WEATHER_GIT_REVISION__;digests=__WEATHER_SHELL_ASSET_DIGESTS__;admin=__WEATHER_ADMIN__;analytics=__WEATHER_ANALYTICS__;preload=__WEATHER_ROUTE_PRELOAD__")],
  ["public/privacy.html", Buffer.from("privacy __WEATHER_ASSET_VERSION__ __WEATHER_ANALYTICS__")],
  ["public/brand/favicon.svg", Buffer.from("<svg>fixture</svg>")],
  ["compiled/client.js", Buffer.from("console.log('fixture');\\n")],
]);
const adminStore = {
  async readAdjustmentSettingsStatus() {
    return { settings: { rain: true, temperature: true, wind: true } };
  },
};
const adjustmentEvidenceScheduler = { status() { return { activation: "inactive" }; } };
const adjustmentEvidenceStore = {
  prepareForecast() { return null; },
  prepareForecastV2() { return null; },
  trackResponse() {},
};
const AbortSignal = {
  timeout(value) { upstreamTimeout = String(value); return {}; },
};
function setSecurityHeaders() {}
function takeScheduledCaptureRequest() { return null; }
function normalizeAdjustmentEvidenceWindow() { return {}; }
async function settleScheduledArchiveCapture() {}
function filterForecastResponse() {
  const body = Buffer.from('{"forecast":"filtered"}');
  return { body, forecast: { data: [] } };
}
function filterRainCollectionStatus() {
  return Buffer.from('{"rain":"filtered"}');
}
function formatXweatherValidTime(instant) {
  if (instant.getTime() !== Date.parse("2026-09-12T15:10:00.000Z")) throw new Error("unexpected preload frame");
  return "20260912151000";
}
function join(...parts) { return parts.join("/"); }
function apiResponse() {
  let status = 200;
  let contentType = "application/x.weather+json";
  let contentLength = null;
  let body = Buffer.from('{"ok":true}');
  if (mode === "raw-error") { status = 418; contentType = "text/plain"; body = Buffer.from("teapot"); }
  if (mode === "fallback") { contentType = null; }
  if (mode === "oversize") { body = Buffer.alloc(maximumApiBytes + 1, 120); }
  if (mode === "head-oversize") { contentLength = String(maximumApiBytes + 1); }
  if (mode === "trends-large") { body = Buffer.alloc(1536 * 1024, 116); }
  const chunks = [body];
  return {
    body: {
      async cancel() {},
      getReader() {
        return {
          async cancel() {},
          async read() {
            return chunks.length === 0
              ? { done: true, value: undefined }
              : { done: false, value: chunks.shift() };
          },
        };
      },
    },
    headers: {
      get(name) {
        if (name === "content-length") return contentLength;
        if (name === "content-type") return contentType;
        return null;
      },
    },
    ok: status >= 200 && status < 300,
    status,
  };
}
async function fetch(target, options) {
  upstreamMethod = options.method;
  upstreamTarget = target.pathname + target.search;
  if (mode === "fetch-failure") throw new Error("fixture fetch failure");
  return apiResponse();
}
${sourceSpan("const assetPrefix =", "// bind every immutable executable and stylesheet")}
${sourceFunction("function resolveAsset(pathname)")}
${sourceFunction("function renderHtmlTemplate(source, pathname, isAdmin, analyticsEnabled)")}
${sourceFunction("function forecastMapPreloadLink(pathname)")}
${sourceFunction("function apiCacheControl(pathname)")}
${sourceFunction("function apiRequestTimeoutMs(pathname)")}
${sourceFunction("function boundedContentLength(value, maximumBytes = maximumApiBytes)")}
${sourceFunction("async function readBoundedBody(upstream, maximumBytes, description)")}
${sourceFunction("function sendText(response, status, body, headers = {})")}
${sourceFunction("async function proxyApi(request, response, requestUrl)")}
this.resolveAsset = resolveAsset;
this.renderHtmlTemplate = renderHtmlTemplate;
this.proxyApi = proxyApi;
this.readFixtureAsset = (path) => {
  if (mode === "read-failure") throw new Error("fixture read failure");
  const value = fixtureAssets.get(path);
  if (value === undefined) throw new Error("fixture missing asset");
  return Buffer.from(value);
};
this.setMode = (value) => {
  mode = value;
  upstreamMethod = "-";
  upstreamTarget = "-";
  upstreamTimeout = "-";
};
this.upstreamTrace = () => [upstreamMethod, upstreamTarget, upstreamTimeout];
`).runInContext(context);

// create one protocol-shaped response recorder
function responseRecorder() {
  const record = { body: undefined, headers: undefined, status: undefined };
  return {
    record,
    response: {
      end(body) { record.body = body; },
      setHeader() {},
      writeHead(status, headers) { record.status = status; record.headers = headers; },
    },
  };
}

// execute the source-equivalent static branch over actual resolver and renderer functions
async function executeStatic(method, pathname, mode = "ok") {
  context.setMode(mode);
  const { record, response } = responseRecorder();

  try {
    const asset = context.resolveAsset(pathname);

    // retain exact missing and normalized-traversal rejection
    if (asset === undefined || pathname.includes("\\")) {
      const content = Buffer.from("not found\n");
      response.writeHead(404, {
        "Cache-Control": "no-store",
        "Content-Length": String(content.byteLength),
        "Content-Type": "text/plain; charset=utf-8",
      });
      response.end(content);
      return record;
    }
    const source = context.readFixtureAsset(asset.path);
    const isHtmlTemplate = asset.template === true && asset.type === "text/html; charset=utf-8";
    const analyticsEnabled = isHtmlTemplate && asset.analytics !== false;
    const body = asset.template === true
      ? Buffer.from(context.renderHtmlTemplate(source.toString("utf8"), pathname, false, analyticsEnabled))
      : source;
    response.writeHead(200, {
      "Cache-Control": asset.cache,
      "Content-Length": String(body.byteLength),
      "Content-Type": asset.type,
      ...(isHtmlTemplate ? { Vary: "Cookie" } : {}),
    });
    response.end(method === "HEAD" ? undefined : body);
  } catch {
    const content = Buffer.from("internal server error\n");
    response.writeHead(500, {
      "Cache-Control": "no-store",
      "Content-Length": String(content.byteLength),
      "Content-Type": "text/plain; charset=utf-8",
    });
    response.end(content);
  }
  return record;
}

// execute the actual source API proxy into the response recorder
async function executeProxy(method, pathname, search, mode = "ok", accept = undefined) {
  context.setMode(mode);
  const { record, response } = responseRecorder();
  const url = new URL(`${pathname}${search}`, "http://weather.invalid");
  await context.proxyApi({ headers: { accept }, method }, response, url);
  return { ...record, trace: context.upstreamTrace() };
}

// render response policy without copying potentially large bodies
function signature(record, trace = ["-", "-", "-"]) {
  return [
    record.status,
    record.headers["Cache-Control"],
    record.headers["Content-Type"],
    record.headers["Content-Length"],
    record.headers.Vary ?? "-",
    record.headers.Allow ?? "-",
    record.body === undefined ? "none" : "body",
    ...trace,
  ].join("|");
}

const staticMissing = await executeStatic("GET", "/missing");
const staticSvg = await executeStatic("GET", "/brand/favicon.svg");
const staticSvgHead = await executeStatic("HEAD", "/brand/favicon.svg");
const staticHtml = await executeStatic("GET", "/");
const staticHtmlHead = await executeStatic("HEAD", "/");
const staticForecast = await executeStatic("GET", "/forecast");
const staticAsset = await executeStatic("GET", "/assets/2026.11.23-1/client.js");
const staticReadFailure = await executeStatic("GET", "/brand/favicon.svg", "read-failure");
const proxyMethod = await executeProxy("POST", "/api/v1/sites", "");
const proxyCurrent = await executeProxy("GET", "/api/v1/sites/ballydidean/current", "?unit=metric", "ok", "application/vnd.weather+json");
const proxyCurrentHead = await executeProxy("HEAD", "/api/v1/sites/ballydidean/current", "");
const proxyForecastHead = await executeProxy("HEAD", "/api/v1/sites/ballydidean/forecast", "");
const proxyForecastGet = await executeProxy("GET", "/api/v1/sites/ballydidean/forecast", "?days=1");
const proxyRawError = await executeProxy("GET", "/api/v1/sites/ballydidean/current", "", "raw-error");
const proxyFallback = await executeProxy("GET", "/api/v1/sites/ballydidean/current", "", "fallback");
const proxyOversize = await executeProxy("GET", "/api/v1/sites/ballydidean/current", "", "oversize");
const proxyHeadOversize = await executeProxy("HEAD", "/api/v1/sites/ballydidean/current", "", "head-oversize");
const proxyTrends = await executeProxy("GET", "/api/v1/sites/ballydidean/trends", "", "trends-large");
const proxyFetchFailure = await executeProxy("GET", "/api/v1/sites/ballydidean/current", "", "fetch-failure");

process.stdout.write(`static-missing=${signature(staticMissing)}\n`);
process.stdout.write(`static-svg=${signature(staticSvg)}\n`);
process.stdout.write(`static-svg-head=${signature(staticSvgHead)}\n`);
process.stdout.write(`static-html=${signature(staticHtml)}\n`);
process.stdout.write(`static-html-head=${signature(staticHtmlHead)}\n`);
process.stdout.write(`static-forecast=${signature(staticForecast)}\n`);
process.stdout.write(`static-asset=${signature(staticAsset)}\n`);
process.stdout.write(`static-read-failure=${signature(staticReadFailure)}\n`);
process.stdout.write(`proxy-method=${signature(proxyMethod, proxyMethod.trace)}\n`);
process.stdout.write(`proxy-current=${signature(proxyCurrent, proxyCurrent.trace)}\n`);
process.stdout.write(`proxy-current-head=${signature(proxyCurrentHead, proxyCurrentHead.trace)}\n`);
process.stdout.write(`proxy-forecast-head=${signature(proxyForecastHead, proxyForecastHead.trace)}\n`);
process.stdout.write(`proxy-forecast-get=${signature(proxyForecastGet, proxyForecastGet.trace)}\n`);
process.stdout.write(`proxy-raw-error=${signature(proxyRawError, proxyRawError.trace)}\n`);
process.stdout.write(`proxy-fallback=${signature(proxyFallback, proxyFallback.trace)}\n`);
process.stdout.write(`proxy-oversize=${signature(proxyOversize, proxyOversize.trace)}\n`);
process.stdout.write(`proxy-head-oversize=${signature(proxyHeadOversize, proxyHeadOversize.trace)}\n`);
process.stdout.write(`proxy-trends=${signature(proxyTrends, proxyTrends.trace)}\n`);
process.stdout.write(`proxy-fetch-failure=${signature(proxyFetchFailure, proxyFetchFailure.trace)}\n`);
process.stdout.write(`body-static-html=${Buffer.from(staticHtml.body).toString("utf8")}\n`);
process.stdout.write(`body-static-forecast=${Buffer.from(staticForecast.body).toString("utf8")}\n`);
process.stdout.write(`body-static-svg=${Buffer.from(staticSvg.body).toString("utf8")}\n`);
process.stdout.write(`body-static-asset-trimmed=${Buffer.from(staticAsset.body).toString("utf8").trimEnd()}\n`);
process.stdout.write(`body-proxy-current=${Buffer.from(proxyCurrent.body).toString("utf8")}\n`);
process.stdout.write(`body-proxy-raw-error=${Buffer.from(proxyRawError.body).toString("utf8")}\n`);
process.stdout.write(`body-proxy-forecast=${Buffer.from(proxyForecastGet.body).toString("utf8")}\n`);
process.stdout.write(`body-proxy-forecast-head-length=${proxyForecastHead.headers["Content-Length"]}\n`);
