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
const fixedNow = "2026-11-23T12:34:56.000Z";
const sequenceTrace = [];
const layoutReadFailure = { value: false };

// extract one complete source function or class method
function sourceFunction(source, signature) {
  const start = source.indexOf(signature);
  assert.notEqual(start, -1);
  const opening = source.indexOf("{", start);
  let depth = 0;
  let quote = null;
  let escaped = false;

  // scan through strings while balancing every executable block
  for (let index = opening; index < source.length; index += 1) {
    const character = source[index];

    // retain escaped string characters without interpreting braces
    if (quote !== null) {
      // advance the active string state without balancing its characters
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

const functions = [
  "function parseLayoutState(value)",
  "function parseLayoutEntry(value)",
  "function requireSensorIcon(value)",
  "function optionalSensorIcon(value)",
  "function validateCoordinate(value, field)",
  "function validateSensorKey(value)",
  "function requireDisplayName(value)",
  "function requireIsoInstant(value)",
  "function isObject(value)",
].map((signature) => sourceFunction(storeSource, signature)).join("\n");
const upsertSensor = sourceFunction(
  storeSource,
  "async upsertSensor(sensorKey, input)",
);
const serveLayout = sourceFunction(
  serverSource,
  "async function servePropertySensorLayout(request, response)",
);
const updateLayout = sourceFunction(
  serverSource,
  "async function updatePropertySensorLayout(request, response, pathname)",
);

const memory = new Map();
let failWrite = false;

class FixedDate extends Date {
  // preserve explicit parse inputs while fixing the source update clock
  constructor(...values) {
    super(values.length === 0 ? fixedNow : values[0]);

    // record only the source-owned current update clock
    if (values.length === 0) {
      sequenceTrace.push("clock");
    }
  }
}

const context = vm.createContext({
  Date: FixedDate,
  Error,
  RangeError,
  SyntaxError,
  URIError,
  memory,
  layoutReadFailure,
  sequenceTrace,
  // atomically publish the full layout only after admitted success
  async atomicWriteJson(path, value) {
    // retain prior state on an ordinary write failure
    if (failWrite) {
      throw new Error("write failed");
    }

    sequenceTrace.push("replace");
    memory.set(path, structuredClone(value));
  },
  // admit the already-verified fixture session
  readAdminSessionCookie() {
    return "verified-operator";
  },
  // return the decoded fixture object without adding a JSON codec
  async readRequestJson(request) {
    return request.body;
  },
  // capture one typed JSON response and the source cache override
  sendJson(response, status, value, head = false, headers = {}) {
    Object.assign(response, {
      cache: headers["Cache-Control"] ?? "no-store",
      head,
      kind: "json",
      status,
      value,
    });
  },
  // capture one typed text outcome
  sendText(response, status, text, headers = {}) {
    Object.assign(response, {
      cache: "no-store",
      headers,
      kind: "text",
      status,
      text,
    });
  },
  structuredClone,
});

new vm.Script(`
const LAYOUT_VERSION = 1;
const SENSOR_KEY_PATTERN = /^[a-z0-9]+(?:-[a-z0-9]+)*$/u;
const SENSOR_ICONS = new Set(["air-quality", "rain", "temperature", "wind"]);
${functions}
class InMemoryAdminStore {
  #center;
  #layoutPath;

  constructor(center, layoutPath) {
    this.#center = center;
    this.#layoutPath = layoutPath;
  }

  authenticateSession(operator) {
    return operator === "verified-operator";
  }

  async readLayout() {
    sequenceTrace.push("read");

    // inject one ordinary current-layout read failure before the clock
    if (layoutReadFailure.value) throw new Error("layout read failed");
    const state = memory.get(this.#layoutPath) ?? null;
    return state === null ? [] : parseLayoutState(state);
  }

  ${upsertSensor}
}
${serveLayout}
${updateLayout}
this.InMemoryAdminStore = InMemoryAdminStore;
this.servePropertySensorLayout = servePropertySensorLayout;
this.updatePropertySensorLayout = updatePropertySensorLayout;
`).runInContext(context);

// execute one actual public source handler request
async function readLayout(store, method = "GET") {
  context.adminStore = store;
  const response = {};
  await context.servePropertySensorLayout({ method }, response);
  return response;
}

// execute one actual admin source handler update
async function updateSensor(store, sensorKey, body, method = "PUT") {
  context.adminStore = store;
  const response = {};
  const pathname = `/api/v1/admin/sites/ballydidean/property-sensor-layout/${encodeURIComponent(sensorKey)}`;
  await context.updatePropertySensorLayout({
    body,
    headers: { cookie: "verified-operator" },
    method,
  }, response, pathname);
  return response;
}

// render one exact placement without serializing the response protocol
function entrySignature(entry) {
  return [
    entry.sensorKey,
    entry.displayName,
    entry.icon ?? "null",
    entry.latitude,
    entry.longitude,
    entry.updatedAt,
  ].join("~");
}

// render one stable typed handler signature
function responseSignature(response) {
  // retain read arrays, saved entries, and HEAD body omission
  if (response.kind === "json") {
    const body = response.head
      ? "head"
      : Array.isArray(response.value.data)
        ? response.value.data.map(entrySignature).join(",") || "empty"
        : entrySignature(response.value.data);
    return `${response.status}:${body}:${response.cache}:-`;
  }

  const text = response.text.trim().replaceAll(" ", "_");
  const allow = response.headers.Allow?.replaceAll(" ", "_") ?? "-";
  return `${response.status}:${text}:${response.cache}:${allow}`;
}

const center = { latitude: 47.95, longitude: -122.43 };
const missingStore = new context.InMemoryAdminStore(center, "missing");
const missing = await readLayout(missingStore);
const missingHead = await readLayout(missingStore, "HEAD");
const publicUnsupported = await readLayout(missingStore, "POST");
memory.set("layout", {
  sensors: [{
    displayName: "Wind mast",
    icon: "wind",
    latitude: 47.951,
    longitude: -122.431,
    sensorKey: "wind-1",
    updatedAt: "2026-11-22T10:00:00.000Z",
  }, {
    displayName: "Old soil",
    icon: "rain",
    latitude: 47.9504,
    longitude: -122.4282,
    sensorKey: "soil-1",
    updatedAt: "2026-11-22T11:00:00.000Z",
  }, {
    displayName: "Legacy air",
    latitude: 47.949,
    longitude: -122.429,
    sensorKey: "air-quality-1",
    updatedAt: "2026-11-22T12:00:00.000Z",
  }],
  version: 1,
});
const store = new context.InMemoryAdminStore(center, "layout");
const before = await readLayout(store);
const updateInput = {
  displayName: "  Orchard soil  ",
  icon: "temperature",
  ignored: "not persisted",
  latitude: 47.9505,
  longitude: -122.4281,
};
const update = await updateSensor(store, "soil-1", updateInput);
const after = await readLayout(store);
const repeated = await updateSensor(store, "soil-1", updateInput);
const afterRepeated = await readLayout(store);
const invalidKey = await updateSensor(store, "../soil", updateInput);
const invalidName = await updateSensor(store, "soil-2", { ...updateInput, displayName: " " });
const outside = await updateSensor(store, "soil-2", {
  ...updateInput,
  latitude: center.latitude + 0.026,
});
const afterInvalid = await readLayout(store);
const adminUnsupported = await updateSensor(store, "soil-1", updateInput, "POST");
memory.set("failing", structuredClone(memory.get("layout")));
const failingStore = new context.InMemoryAdminStore(center, "failing");
failWrite = true;
const failedWrite = await updateSensor(failingStore, "soil-2", {
  ...updateInput,
  displayName: "North soil",
});
failWrite = false;
const afterFailedWrite = await readLayout(failingStore);

const sequenceStore = new context.InMemoryAdminStore(center, "sequence");
sequenceTrace.length = 0;
const sequenceUpdate = await updateSensor(sequenceStore, "soil-1", updateInput);
assert.equal(sequenceUpdate.status, 200);
assert.deepEqual(sequenceTrace, ["read", "clock", "replace"]);
sequenceTrace.length = 0;
layoutReadFailure.value = true;
const sequenceReadFailure = await updateSensor(sequenceStore, "soil-2", updateInput);
layoutReadFailure.value = false;
assert.equal(sequenceReadFailure.status, 500);
assert.deepEqual(sequenceTrace, ["read"]);

process.stdout.write(`missing=${responseSignature(missing)}\n`);
process.stdout.write(`missing-head=${responseSignature(missingHead)}\n`);
process.stdout.write(`public-unsupported=${responseSignature(publicUnsupported)}\n`);
process.stdout.write(`before=${responseSignature(before)}\n`);
process.stdout.write(`update=${responseSignature(update)}\n`);
process.stdout.write(`after=${responseSignature(after)}\n`);
process.stdout.write(`repeated=${responseSignature(repeated)}\n`);
process.stdout.write(`after-repeated=${responseSignature(afterRepeated)}\n`);
process.stdout.write(`invalid-key=${responseSignature(invalidKey)}\n`);
process.stdout.write(`invalid-name=${responseSignature(invalidName)}\n`);
process.stdout.write(`outside=${responseSignature(outside)}\n`);
process.stdout.write(`after-invalid=${responseSignature(afterInvalid)}\n`);
process.stdout.write(`admin-unsupported=${responseSignature(adminUnsupported)}\n`);
process.stdout.write(`failed-write=${responseSignature(failedWrite)}\n`);
process.stdout.write(`after-failed-write=${responseSignature(afterFailedWrite)}\n`);
