import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import vm from "node:vm";

import { projectWidgetForecast as actualV1 } from "../../../../apps/web/dist/widget-forecast.js";
import { projectWidgetForecastV2 as actualV2 } from "../../../../apps/web/dist/widget-forecast-v2.js";
import { projectWidgetForecastV3 as actualV3 } from "../../../../apps/web/dist/widget-forecast-v3.js";
import { createForecastFixture } from "../../../../scripts/widget-fixtures.mjs";

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

const fixture = createForecastFixture({
  generic: true,
  rain: true,
  temperature: true,
  // retain one amount-only legacy rain fallback and one explicit null row
  row(index, record) {
    if (index === 0) {
      return {
        ...record,
        metrics: {
          ...record.metrics,
          precipitationMm: 0.7,
          precipitationRateMmPerHour: null,
        },
      };
    }
    if (index === 1) {
      return {
        ...record,
        metrics: {
          ...record.metrics,
          precipitationMm: null,
          precipitationRateMmPerHour: null,
          temperatureC: null,
        },
      };
    }
    return record;
  },
});
let mode = "ok";
let upstreamPath = "-";

class FixtureDate extends Date {
  // fix only no-argument edge receipt time
  constructor(value) {
    super(value === undefined ? fixture.receivedAt : value);
  }

  // expose the fixed edge receipt clock
  static now() {
    return Date.parse(fixture.receivedAt);
  }
}

const context = vm.createContext({
  AbortSignal: { timeout: () => ({}) },
  Buffer,
  Date: FixtureDate,
  Error,
  JSON,
  structuredClone,
  URL,
  actualV1,
  actualV2,
  actualV3,
  fixture,
});
new vm.Script(`
const widgetForecastPath = "/api/v1/sites/ballydidean/widget-forecast";
const widgetForecastV2Path = "/api/v2/sites/ballydidean/widget-forecast";
const widgetForecastV3Path = "/api/v3/sites/ballydidean/widget-forecast";
const apiOrigin = new URL("http://fixture.invalid");
const maximumApiBytes = 1024 * 1024;
const WIDGET_FORECAST_MAX_BYTES = 128 * 1024;
let mode = "ok";
let upstreamPath = "-";
const adminStore = {
  async readAdjustmentSettingsStatus() {
    return { settings: { version: 1, temperature: true, wind: true, rain: true } };
  },
};
function apiRequestTimeoutMs() { return 5000; }
async function fetch(target) {
  upstreamPath = target.pathname + target.search;
  return {
    body: { async cancel() {} },
    ok: mode !== "upstream-failure",
  };
}
async function readBoundedBody() {
  const input = structuredClone(fixture.input);
  if (mode === "wrong-site") input.site.slug = "other";
  return input;
}
function filterForecastResponse(value) { return { forecast: value }; }
function projectWidgetForecast(value, receivedAt) {
  if (mode === "oversize") return { huge: "x".repeat(128 * 1024) };
  return actualV1(value, receivedAt);
}
function projectWidgetForecastV2(value, receivedAt) {
  if (mode === "oversize") return { huge: "x".repeat(128 * 1024) };
  return actualV2(value, receivedAt);
}
function projectWidgetForecastV3(value, receivedAt) {
  if (mode === "oversize") return { huge: "x".repeat(128 * 1024) };
  return actualV3(value, receivedAt);
}
function setSecurityHeaders() {}
${sourceFunction(webSource, "function sendText(response, status, body, headers = {})")}
${sourceFunction(webSource, "async function serveWidgetForecast(request, response, requestUrl)")}
this.serveWidgetForecast = serveWidgetForecast;
this.setMode = (value) => { mode = value; upstreamPath = "-"; };
this.upstreamPath = () => upstreamPath;
`).runInContext(context);

// execute one source widget request into a recording response adapter
async function execute(method, pathname, search = "", url = pathname) {
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
  await context.serveWidgetForecast({ method, url }, response, { pathname, search });
  return { ...record, upstreamPath: context.upstreamPath() };
}

// render one response without its potentially large json body
function responseSignature(response) {
  const headers = response.headers;
  return [
    response.status,
    headers["Cache-Control"],
    headers["Content-Type"],
    headers["Content-Length"],
    headers.Allow ?? "-",
    response.body === undefined ? "none" : "body",
    response.upstreamPath,
  ].join("|");
}

const v1Path = "/api/v1/sites/ballydidean/widget-forecast";
const v2Path = "/api/v2/sites/ballydidean/widget-forecast";
const v3Path = "/api/v3/sites/ballydidean/widget-forecast";
const notFound = await execute("GET", "/api/v1/sites/other/widget-forecast");
const query = await execute("GET", v1Path, "?date=2026-09-12", `${v1Path}?date=2026-09-12`);
const unsupported = await execute("POST", v1Path);
const v1 = await execute("GET", v1Path);
const v1Head = await execute("HEAD", v1Path);
const v2 = await execute("GET", v2Path);
const v3 = await execute("GET", v3Path);
context.setMode("upstream-failure");
const upstreamFailure = await execute("GET", v1Path);
context.setMode("wrong-site");
const projectionFailure = await execute("GET", v1Path);
context.setMode("oversize");
const oversize = await execute("GET", v1Path);
const v1Body = Buffer.from(v1.body).toString("utf8").trimEnd();
const v2Body = Buffer.from(v2.body).toString("utf8").trimEnd();
const v3Body = Buffer.from(v3.body).toString("utf8").trimEnd();
const v1Snapshot = JSON.parse(v1Body);
const v2Snapshot = JSON.parse(v2Body);
const v3Snapshot = JSON.parse(v3Body);

process.stdout.write(`not-found=${responseSignature(notFound)}:${Buffer.from(notFound.body).toString("utf8").trimEnd()}\n`);
process.stdout.write(`query=${responseSignature(query)}:${Buffer.from(query.body).toString("utf8").trimEnd()}\n`);
process.stdout.write(`unsupported=${responseSignature(unsupported)}:${Buffer.from(unsupported.body).toString("utf8").trimEnd()}\n`);
process.stdout.write(`v1=${responseSignature(v1)}\n`);
process.stdout.write(`v1-head=${responseSignature(v1Head)}\n`);
process.stdout.write(`v2=${responseSignature(v2)}\n`);
process.stdout.write(`v3=${responseSignature(v3)}\n`);
process.stdout.write(`upstream-failure=${responseSignature(upstreamFailure)}:${Buffer.from(upstreamFailure.body).toString("utf8").trimEnd()}\n`);
process.stdout.write(`projection-failure=${responseSignature(projectionFailure)}:${Buffer.from(projectionFailure.body).toString("utf8").trimEnd()}\n`);
process.stdout.write(`oversize=${responseSignature(oversize)}:${Buffer.from(oversize.body).toString("utf8").trimEnd()}\n`);
process.stdout.write(`meta=${v1Snapshot.schemaVersion}/${v2Snapshot.schemaVersion}/${v3Snapshot.schemaVersion}:${v1Snapshot.site.slug}/${v1Snapshot.site.latitude}/${v1Snapshot.site.longitude}:${v1Snapshot.hours.length}/${v3Snapshot.hours.length}:${v1Snapshot.hours[0].rainMmPerHour.selected}:${v1Snapshot.hours[1].rainMmPerHour.selected}/${v1Snapshot.hours[1].temperatureC.selected}\n`);
process.stdout.write(`body-v1=${v1Body}\n`);
process.stdout.write(`body-v2=${v2Body}\n`);
process.stdout.write(`body-v3=${v3Body}\n`);
