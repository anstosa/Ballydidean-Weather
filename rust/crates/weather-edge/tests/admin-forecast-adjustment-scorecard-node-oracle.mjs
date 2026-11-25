import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import vm from "node:vm";

const repositoryRoot = new URL("../../../../", import.meta.url);
const webSource = await readFile(
  new URL("deploy/scripts/web-server.mjs", repositoryRoot),
  "utf8",
);

// extract one complete source function
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

const fixtureNow = "2026-10-07T01:30:00.000Z";

class FixtureDate extends Date {
  // fix only no-argument route time reads
  constructor(value) {
    super(value === undefined ? fixtureNow : value);
  }

  // expose the fixed route clock
  static now() {
    return Date.parse(fixtureNow);
  }
}

const context = vm.createContext({
  Buffer,
  Date: FixtureDate,
  fixtureNow,
  JSON,
});
new vm.Script(`
const adjustmentEvidenceRoot = "/admitted-fixture";
let mode = "legacy";
let observedNow = "-";
const adminStore = {
  authenticateSession() { return true; },
};
function readAdminSessionCookie() { return "admitted-session"; }
function setSecurityHeaders() {}
function scorecard(version) {
  return {
    contractVersion: version,
    generatedAt: "2026-10-07T01:00:00.000Z",
    siteKey: "ballydidean",
    validThrough: version.endsWith("/v2")
      ? "2026-10-14T01:00:00.000Z"
      : "2099-10-08T01:00:00.000Z",
  };
}
async function readInstalledForecastAdjustmentScorecard(root, now) {
  if (root !== adjustmentEvidenceRoot) throw new Error("unexpected root");
  observedNow = now;
  if (mode === "error") throw new Error("fixture unavailable");
  if (mode === "expired") {
    const value = scorecard("forecast-adjustment-scorecard/v1");
    value.validThrough = fixtureNow;
    if (Date.parse(value.validThrough) <= Date.parse(now)) throw new Error("fixture stale");
  }
  if (mode === "pending") {
    return { publicationState: "pending_unapplied", scorecard: scorecard("forecast-adjustment-scorecard/v2") };
  }
  if (mode === "current") {
    return { publicationState: "current", scorecard: scorecard("forecast-adjustment-scorecard/v2") };
  }
  return { publicationState: "legacy_display", scorecard: scorecard("forecast-adjustment-scorecard/v1") };
}
${sourceFunction("function sendText(response, status, body, headers = {})")}
${sourceFunction("function sendJson(response, status, value, head = false, headers = {})")}
${sourceFunction("function sendAdminUnauthorized(response)")}
${sourceFunction("async function serveForecastAdjustmentScorecard(request, response, requestUrl)")}
this.serveForecastAdjustmentScorecard = serveForecastAdjustmentScorecard;
this.setFixture = (nextMode) => {
  mode = nextMode;
  observedNow = "-";
};
this.observedNow = () => observedNow;
`).runInContext(context);

// execute one actual route call into a response recorder
async function execute(method, search = "", urlSuffix = search, mode = "legacy") {
  context.setFixture(mode);
  const record = { body: undefined, headers: undefined, status: undefined };
  const response = {
    end(body) { record.body = body; },
    setHeader() {},
    writeHead(status, headers) { record.status = status; record.headers = headers; },
  };
  const pathname = "/api/v1/admin/sites/ballydidean/forecast-adjustment-scorecard";
  await context.serveForecastAdjustmentScorecard(
    { headers: { cookie: "fixture" }, method, url: pathname + urlSuffix },
    response,
    { pathname, search },
  );
  return { ...record, observedNow: context.observedNow() };
}

// render one exact route response without duplicating its body
function signature(response) {
  return [
    response.status,
    response.headers["Cache-Control"],
    response.headers["Content-Type"],
    response.headers["Content-Length"],
    response.headers.Allow ?? "-",
    response.headers["Retry-After"] ?? "-",
    response.body === undefined ? "none" : "body",
    response.observedNow,
  ].join("|");
}

const emptyQuery = await execute("GET", "", "?", "legacy");
const query = await execute("GET", "?sha256=x", "?sha256=x", "legacy");
const unsupported = await execute("POST");
const legacy = await execute("GET");
const legacyHead = await execute("HEAD");
const pending = await execute("GET", "", "", "pending");
const current = await execute("GET", "", "", "current");
const expired = await execute("GET", "", "", "expired");
const unavailable = await execute("GET", "", "", "error");

process.stdout.write(`empty-query=${signature(emptyQuery)}\n`);
process.stdout.write(`query=${signature(query)}\n`);
process.stdout.write(`unsupported=${signature(unsupported)}\n`);
process.stdout.write(`legacy=${signature(legacy)}\n`);
process.stdout.write(`legacy-head=${signature(legacyHead)}\n`);
process.stdout.write(`pending=${signature(pending)}\n`);
process.stdout.write(`current=${signature(current)}\n`);
process.stdout.write(`expired=${signature(expired)}\n`);
process.stdout.write(`unavailable=${signature(unavailable)}\n`);
process.stdout.write(`body-legacy=${Buffer.from(legacy.body).toString("utf8").trimEnd()}\n`);
process.stdout.write(`body-pending=${Buffer.from(pending.body).toString("utf8").trimEnd()}\n`);
process.stdout.write(`body-current=${Buffer.from(current.body).toString("utf8").trimEnd()}\n`);
process.stdout.write(`scorecard-legacy=${JSON.stringify(JSON.parse(Buffer.from(legacy.body)).data)}\n`);
process.stdout.write(`scorecard-v2=${JSON.stringify(JSON.parse(Buffer.from(current.body)).data)}\n`);
