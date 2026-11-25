import assert from "node:assert/strict";
import { dirname, join } from "node:path";
import {
  mkdir,
  mkdtemp,
  readFile,
  readdir,
  rename,
  rm,
  stat,
  writeFile,
} from "node:fs/promises";
import { tmpdir } from "node:os";
import vm from "node:vm";

const repositoryRoot = new URL("../../../../", import.meta.url);
const storeSource = await readFile(
  new URL("deploy/scripts/weather-admin-store.mjs", repositoryRoot),
  "utf8",
);
const UPDATED_AT = "2026-11-23T12:34:56.000Z";

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
  "async function readOptionalJson(path)",
  "async function readOptionalText(path)",
  "async function atomicWriteJson(path, value)",
  "async function atomicWriteText(path, content)",
].map((signature) => sourceFunction(storeSource, signature)).join("\n");
const methods = [
  "async readLayout()",
  "async upsertSensor(sensorKey, input)",
].map((signature) => sourceFunction(storeSource, signature)).join("\n");

const root = await mkdtemp(join(tmpdir(), "weather-edge-layout-node-"));
let temporaryCounter = 0;

class FixedDate extends Date {
  // retain ordinary parsing while fixing only the no-argument update clock
  constructor(value) {
    super(value === undefined ? UPDATED_AT : value);
  }
}

const context = vm.createContext({
  Date: FixedDate,
  Error,
  JSON,
  RangeError,
  dirname,
  mkdir,
  process,
  readFile,
  rename,
  writeFile,
  // inject bounded deterministic temporary names without implementing crypto
  randomBytes(size) {
    temporaryCounter += 1;
    return Buffer.alloc(size, temporaryCounter);
  },
});

new vm.Script(`
const LAYOUT_VERSION = 1;
const SENSOR_KEY_PATTERN = /^[a-z0-9]+(?:-[a-z0-9]+)*$/u;
const SENSOR_ICONS = new Set(["air-quality", "rain", "temperature", "wind"]);
${functions}
class FileLayoutStore {
  #center = { latitude: 47.95, longitude: -122.43 };
  #layoutPath;

  constructor(path) {
    this.#layoutPath = path;
  }

  ${methods}
}
this.FileLayoutStore = FileLayoutStore;
`).runInContext(context);

// build one exact source fixture placement
function sensor(index) {
  return {
    displayName: `Sensor ${index}`,
    icon: index % 2 === 0 ? "temperature" : "wind",
    latitude: 47.95,
    longitude: -122.43,
    sensorKey: `sensor-${String(index).padStart(2, "0")}`,
    updatedAt: "2026-11-22T10:00:00.000Z",
  };
}

// render one typed placement without another serialization contract
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

// read one ordinary permission mode
async function mode(path) {
  return ((await stat(path)).mode & 0o777).toString(8);
}

// report whether one fixture path currently exists
async function exists(path) {
  try {
    await stat(path);
    return true;
  } catch (error) {
    // retain only the ordinary missing-file result
    if (error?.code === "ENOENT") return false;
    throw error;
  }
}

// capture one exact ordinary failure message
async function failureMessage(operation) {
  try {
    await operation();
  } catch (error) {
    return error.message;
  }
  throw new Error("expected fixture failure");
}

try {
  const layoutPath = join(root, "state", "property-sensor-layout.json");
  const store = new context.FileLayoutStore(layoutPath);
  const missing = await store.readLayout();
  const missingExists = await exists(layoutPath);
  await mkdir(dirname(layoutPath), { mode: 0o700, recursive: true });
  const initialState = {
    sensors: [
      {
        displayName: "Wind mast",
        icon: "wind",
        latitude: 47.951,
        longitude: -122.431,
        sensorKey: "wind-1",
        updatedAt: "2026-11-22T10:00:00.000Z",
      },
      {
        displayName: "Old soil",
        icon: "rain",
        latitude: 47.9504,
        longitude: -122.4282,
        sensorKey: "soil-1",
        updatedAt: "2026-11-22T11:00:00.000Z",
      },
      {
        displayName: "Legacy air",
        latitude: 47.949,
        longitude: -122.429,
        sensorKey: "air-quality-1",
        updatedAt: "2026-11-22T12:00:00.000Z",
      },
    ],
    version: 1,
  };
  await writeFile(layoutPath, `${JSON.stringify(initialState)}\n`, { mode: 0o600 });
  const before = await store.readLayout();
  const saved = await store.upsertSensor("soil-1", {
    displayName: "  Orchard soil  ",
    icon: "temperature",
    latitude: 47.9505,
    longitude: -122.4281,
  });
  const after = await store.readLayout();
  const written = await readFile(layoutPath, "utf8");
  const fixtureModes = `${await mode(dirname(layoutPath))}/${await mode(layoutPath)}`;
  const leftovers = (await readdir(dirname(layoutPath)))
    .filter((name) => name.endsWith(".tmp"))
    .length;

  const boundedState = { sensors: Array.from({ length: 66 }, (_, index) => sensor(index)), version: 1 };
  await writeFile(layoutPath, `${JSON.stringify(boundedState)}\n`, { mode: 0o600 });
  const bounded = await store.readLayout();

  await writeFile(layoutPath, '{"sensors":[],"version":2}\n', { mode: 0o600 });
  const invalidEnvelope = await failureMessage(() => store.readLayout());
  await writeFile(layoutPath, '{"sensors":[{"displayName":"Bad","icon":"wind","latitude":90,"longitude":0,"sensorKey":"bad-1","updatedAt":"2026-11-22T10:00:00.000Z"}],"version":1}\n', { mode: 0o600 });
  const invalidCoordinate = await failureMessage(() => store.readLayout());

  process.stdout.write(`missing=${missing.length}:${missingExists}\n`);
  process.stdout.write(`before=${before.map(entrySignature).join(",")}\n`);
  process.stdout.write(`saved=${entrySignature(saved)}\n`);
  process.stdout.write(`after=${after.map(entrySignature).join(",")}\n`);
  process.stdout.write(`written=${written.trimEnd()}:newline=${written.endsWith("\n")}\n`);
  process.stdout.write(`modes=${fixtureModes}:temporary=${leftovers}\n`);
  process.stdout.write(`bounded=${bounded.length}:${entrySignature(bounded[0])}:${entrySignature(bounded.at(-1))}\n`);
  process.stdout.write(`invalid-envelope=${invalidEnvelope}\n`);
  process.stdout.write(`invalid-coordinate=${invalidCoordinate}\n`);
} finally {
  await rm(root, { force: true, recursive: true });
}
