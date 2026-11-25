import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import vm from "node:vm";

const repositoryRoot = new URL("../../../../", import.meta.url);
const serverSource = await readFile(
  new URL("deploy/scripts/web-server.mjs", repositoryRoot),
  "utf8",
);
const storeSource = await readFile(
  new URL("deploy/scripts/weather-admin-store.mjs", repositoryRoot),
  "utf8",
);

// extract one source-owned function through its following comment boundary
function sourceSection(source, start, end) {
  const first = source.indexOf(start);
  const last = source.indexOf(end, first);
  assert.notEqual(first, -1);
  assert.notEqual(last, -1);
  return source.slice(first, last);
}

const parser = sourceSection(
  storeSource,
  "function parseAdjustmentSettings(value)",
  "// parse one immutable auth record",
);
const objectGuard = storeSource.slice(storeSource.indexOf("function isObject(value)"));
const handler = sourceSection(
  serverSource,
  "async function serveForecastAdjustmentSettings(request, response, admin)",
  "// serve one authenticated aggregate-only adjustment scorecard",
);

const context = vm.createContext({
  Error,
  RangeError,
  SyntaxError,
  // admit the already-verified fixture session
  readAdminSessionCookie() {
    return "verified-operator";
  },
  // admit the already-verified same-origin mutation
  isSameOriginMutation() {
    return true;
  },
  // return the decoded fixture object without adding a JSON codec
  async readRequestJson(request) {
    return request.body;
  },
  // capture one typed JSON response
  sendJson(response, status, value, head) {
    Object.assign(response, { head, kind: "json", status, value });
  },
  // capture one typed text outcome
  sendText(response, status, text, headers = {}) {
    Object.assign(response, { headers, kind: "text", status, text });
  },
});

new vm.Script(`
const ADJUSTMENT_SETTINGS_VERSION = 1;
${objectGuard}
${parser}
${handler}
this.parseAdjustmentSettings = parseAdjustmentSettings;
this.serveForecastAdjustmentSettings = serveForecastAdjustmentSettings;
`).runInContext(context);

// retain settings state in memory while executing the actual source handler
class FixtureStore {
  constructor(status, writeFailure = false) {
    this.status = status;
    this.writeFailure = writeFailure;
  }

  // accept only the fixture's pre-admitted operator
  authenticateSession(operator) {
    assert.equal(operator, "verified-operator");
    return true;
  }

  // return one in-memory settings status
  async readAdjustmentSettingsStatus() {
    return this.status;
  }

  // validate first and then replace the whole snapshot atomically
  async writeAdjustmentSettings(input) {
    const settings = context.parseAdjustmentSettings(input);

    // retain the prior snapshot on an ordinary persistence failure
    if (this.writeFailure) {
      throw new Error("write failed");
    }

    this.status = { error: null, settings };
    return settings;
  }
}

// execute one actual source handler request without transport or filesystem I/O
async function execute(store, method, body, admin = true) {
  context.adminStore = store;
  const request = {
    body,
    headers: { cookie: "verified-operator" },
    method,
  };
  const response = {};
  await context.serveForecastAdjustmentSettings(request, response, admin);
  return response;
}

// render one stable typed response signature
function responseSignature(response) {
  // retain JSON body omission and optional admin diagnostics
  if (response.kind === "json") {
    const settings = response.value.data;
    const switches = `${settings.version}:${Number(settings.temperature)}${Number(settings.wind)}${Number(settings.rain)}`;
    const body = response.head ? "head" : switches;
    return `${response.status}:${body}:${response.value.error ?? "-"}:-`;
  }

  const text = response.text.trim().replaceAll(" ", "_");
  const allow = response.headers.Allow?.replaceAll(" ", "_") ?? "-";
  return `${response.status}:${text}:-:${allow}`;
}

const defaults = {
  error: null,
  settings: { version: 1, temperature: true, wind: true, rain: true },
};
const store = new FixtureStore(defaults);
const read = await execute(store, "GET");
const head = await execute(store, "HEAD");
const updateInput = { version: 1, temperature: true, wind: false, rain: false };
const update = await execute(store, "PUT", updateInput);
const readAfter = await execute(store, "GET");
const invalid = await execute(store, "PUT", { ...updateInput, extra: true });
const readAfterInvalid = await execute(store, "GET");
const unavailable = new FixtureStore({
  error: "adjustment_settings_unavailable",
  settings: { version: 1, temperature: false, wind: false, rain: false },
});
const unavailableAdmin = await execute(unavailable, "GET");
const unavailablePublic = await execute(unavailable, "GET", undefined, false);
const unsupported = await execute(store, "POST");
const failingStore = new FixtureStore(defaults, true);
const failedWrite = await execute(failingStore, "PUT", updateInput);
const readAfterFailedWrite = await execute(failingStore, "GET");

process.stdout.write(`read=${responseSignature(read)}\n`);
process.stdout.write(`head=${responseSignature(head)}\n`);
process.stdout.write(`update=${responseSignature(update)}\n`);
process.stdout.write(`read-after=${responseSignature(readAfter)}\n`);
process.stdout.write(`invalid=${responseSignature(invalid)}\n`);
process.stdout.write(`read-after-invalid=${responseSignature(readAfterInvalid)}\n`);
process.stdout.write(`unavailable-admin=${responseSignature(unavailableAdmin)}\n`);
process.stdout.write(`unavailable-public=${responseSignature(unavailablePublic)}\n`);
process.stdout.write(`unsupported=${responseSignature(unsupported)}\n`);
process.stdout.write(`failed-write=${responseSignature(failedWrite)}\n`);
process.stdout.write(`read-after-failed-write=${responseSignature(readAfterFailedWrite)}\n`);
