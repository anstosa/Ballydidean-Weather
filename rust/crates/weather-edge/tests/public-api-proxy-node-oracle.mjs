import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import vm from "node:vm";

const source = await readFile(
  new URL("../../../../deploy/scripts/web-server.mjs", import.meta.url),
  "utf8",
);

// Extract one complete named production function.
function sourceFunction(signature) {
  const start = source.indexOf(signature);
  assert.notEqual(start, -1);
  const opening = source.indexOf("{", start + signature.length);
  let depth = 0;
  let quote = null;
  let escaped = false;

  // Balance executable braces while retaining ordinary source strings.
  for (let index = opening; index < source.length; index += 1) {
    const character = source[index];

    // Ignore braces inside the active source string.
    if (quote !== null) {
      // Advance escaped and closing string state.
      if (escaped) {
        escaped = false;
      } else if (character === "\\") {
        escaped = true;
      } else if (character === quote) {
        quote = null;
      }
      continue;
    }

    // Enter one ordinary string.
    if (character === '"' || character === "'" || character === "`") {
      quote = character;
      continue;
    }

    // Return the first balanced function block.
    if (character === "{") {
      depth += 1;
    } else if (character === "}") {
      depth -= 1;
      // Stop at the function's closing brace.
      if (depth === 0) {
        return source.slice(start, index + 1);
      }
    }
  }
  throw new Error(`unterminated ${signature}`);
}

const context = vm.createContext({ Buffer, JSON, URL });
new vm.Script(`
const apiOrigin = new URL("http://api:4000/");
const maximumApiBytes = 1024 * 1024;
const maximumTrendsApiBytes = 2 * 1024 * 1024;
let mode = "forecast";
let trace = [];
const adminStore = {
  async readAdjustmentSettingsStatus() {
    trace.push("prepare");
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
  timeout(value) { trace.push("timeout:" + String(value)); return {}; },
};
function setSecurityHeaders() {}
function takeScheduledCaptureRequest() { return null; }
function normalizeAdjustmentEvidenceWindow() { return {}; }
async function settleScheduledArchiveCapture() {}
function filterForecastResponse(body) {
  // Require the complete source body before projecting it.
  if (body.toString("utf8") !== '{"data":[]}') throw new Error("unexpected forecast body");
  trace.push("project");
  return { body: Buffer.from("projected"), forecast: { data: [] } };
}
function filterRainCollectionStatus() { throw new Error("unused rain projection"); }
function upstreamResponse() {
  const status = mode === "raw" ? 418 : 200;
  const contentType = mode === "raw" ? "text/plain" : "application/json";
  const contentLength = mode === "head" ? "17" : null;
  const chunks = mode === "oversize"
    ? [Buffer.alloc(maximumApiBytes, 120), Buffer.from("y")]
    : [Buffer.from(mode === "raw" ? "teapot" : '{"data":[]}')];
  return {
    body: {
      getReader() {
        return {
          async cancel() { trace.push("cancel"); },
          async read() {
            trace.push("read");
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
  trace.push([
    "fetch", options.method, options.redirect, options.headers.Accept,
    target.origin, target.pathname + target.search,
  ].join(":"));
  return upstreamResponse();
}
${sourceFunction("function apiCacheControl(pathname)")}
${sourceFunction("function apiRequestTimeoutMs(pathname)")}
${sourceFunction("function boundedContentLength(value, maximumBytes = maximumApiBytes)")}
${sourceFunction("async function readBoundedBody(upstream, maximumBytes, description)")}
${sourceFunction("function sendText(response, status, body, headers = {})")}
${sourceFunction("async function proxyApi(request, response, requestUrl)")}
this.setMode = (value) => { mode = value; trace = []; };
this.getTrace = () => trace;
this.proxyApi = proxyApi;
`).runInContext(context);

// Execute the actual proxy function into a disposable response recorder.
async function execute(method, pathname, search, mode, accept) {
  context.setMode(mode);
  const record = { body: undefined, headers: undefined, status: undefined };
  const response = {
    end(body) { record.body = body; },
    setHeader() {},
    writeHead(status, headers) { record.status = status; record.headers = headers; },
  };
  await context.proxyApi(
    { headers: { accept }, method },
    response,
    new URL(`${pathname}${search}`, "https://weather.invalid"),
  );
  return { ...record, trace: context.getTrace() };
}

const forecast = await execute(
  "HEAD",
  "/api/v1/sites/ballydidean/forecast",
  "?days=1",
  "forecast",
  undefined,
);
assert.equal(
  forecast.trace.slice(0, 3).join("\n"),
  [
    "prepare",
    "timeout:5000",
    "fetch:GET:manual:application/json:http://api:4000:/api/v1/sites/ballydidean/forecast?days=1",
  ].join("\n"),
);
assert.ok(forecast.trace.includes("project"));

const head = await execute(
  "HEAD",
  "/api/v1/sites/ballydidean/current",
  "",
  "head",
  undefined,
);
assert.ok(!head.trace.includes("read"));

const raw = await execute(
  "GET",
  "/api/v1/sites/ballydidean/current",
  "",
  "raw",
  "application/vnd.weather+json",
);
const oversize = await execute(
  "GET",
  "/api/v1/sites/ballydidean/current",
  "",
  "oversize",
  undefined,
);

process.stdout.write(
  `forecast=${forecast.status}|GET|manual|application/json|${forecast.headers["Content-Length"] === "9" ? "projected" : "unexpected"}|${forecast.body === undefined ? "none" : "body"}\n`,
);
process.stdout.write(
  `head=${head.status}|HEAD|manual|${head.headers["Content-Length"]}|${head.body === undefined ? "none" : "body"}\n`,
);
process.stdout.write(
  `raw=${raw.status}|GET|${Buffer.from(raw.body).toString("utf8")}\n`,
);
process.stdout.write(
  `oversize=${oversize.status}|${Buffer.from(oversize.body).toString("utf8").trimEnd()}\n`,
);
