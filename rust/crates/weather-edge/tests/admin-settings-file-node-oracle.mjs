import assert from "node:assert/strict";
import { dirname, join, relative } from "node:path";
import {
  link,
  mkdir,
  mkdtemp,
  readFile,
  rename as fsRename,
  rm,
  stat,
  unlink,
  writeFile,
} from "node:fs/promises";
import { tmpdir } from "node:os";
import vm from "node:vm";

const repositoryRoot = new URL("../../../../", import.meta.url);
const storeSource = await readFile(
  new URL("deploy/scripts/weather-admin-store.mjs", repositoryRoot),
  "utf8",
);

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
  "function parseAdjustmentSettings(value)",
  "function isObject(value)",
  "async function readOptionalText(path)",
  "async function atomicWriteJson(path, value)",
  "async function atomicWriteText(path, content)",
  "async function atomicCreateJsonIfMissing(path, value)",
].map((signature) => sourceFunction(storeSource, signature)).join("\n");
const methods = [
  "async readAdjustmentSettingsStatus()",
  "async writeAdjustmentSettings(input)",
  "async #initializeAdjustmentSettings()",
].map((signature) => sourceFunction(storeSource, signature)).join("\n");

const root = await mkdtemp(join(tmpdir(), "weather-edge-settings-node-"));
const renameTrace = [];
let temporaryCounter = 0;

const context = vm.createContext({
  Error,
  JSON,
  RangeError,
  dirname,
  join,
  link,
  mkdir,
  process,
  readFile,
  root,
  stat,
  unlink,
  writeFile,
  // inject bounded deterministic temporary names without implementing crypto
  randomBytes(size) {
    temporaryCounter += 1;
    return Buffer.alloc(size, temporaryCounter);
  },
  // retain exact replacement target order while using real rename mechanics
  async rename(source, target) {
    renameTrace.push(relative(root, target));
    await fsRename(source, target);
  },
});

new vm.Script(`
const ADJUSTMENT_SETTINGS_VERSION = 1;
const ADJUSTMENT_SETTINGS_MARKER = "forecast-adjustment-settings/v1";
const DEFAULT_ADJUSTMENT_SETTINGS = Object.freeze({
  version: 1, temperature: true, wind: true, rain: true,
});
const DISABLED_ADJUSTMENT_SETTINGS = Object.freeze({
  version: 1, temperature: false, wind: false, rain: false,
});
${functions}
let admittedAuthState = "admitted";
async function readOptionalJson(path) {
  // replace credential parsing with the admitted authorization boundary
  if (path === "admitted-auth") {
    // preserve the live missing state before configuration appears
    if (admittedAuthState === "missing") return null;
    return { admitted: admittedAuthState === "admitted" };
  }
  const text = await readOptionalText(path);
  return text === null ? null : JSON.parse(text);
}
function parseAuthState(value) {
  // require only the already-admitted authorization evidence
  if (value?.admitted !== true) throw new Error("admin auth state is invalid");
  return value;
}
class FileSettingsStore {
  #authPath = "admitted-auth";
  #adjustmentSettingsPath;
  #adjustmentSettingsMarkerPath;
  #adjustmentSettingsInitialized = false;

  constructor(path) {
    this.#adjustmentSettingsPath = path;
    this.#adjustmentSettingsMarkerPath = path + ".initialized";
  }

  ${methods}
}
this.FileSettingsStore = FileSettingsStore;
this.setAdmittedAuthState = (state) => { admittedAuthState = state; };
`).runInContext(context);

// render one typed status without serializing another response contract
function statusSignature(status) {
  const settings = status.settings;
  return `${settings.version}:${Number(settings.temperature)}${Number(settings.wind)}${Number(settings.rain)}:${status.error ?? "-"}`;
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

try {
  const livePath = join(root, "live", "forecast-adjustment-settings.json");
  const liveStore = new context.FileSettingsStore(livePath);
  context.setAdmittedAuthState("missing");
  const liveMissing = await liveStore.readAdjustmentSettingsStatus();
  context.setAdmittedAuthState("invalid");
  const liveInvalid = await liveStore.readAdjustmentSettingsStatus();
  const liveSettingsBefore = await exists(livePath);
  const liveMarkerBefore = await exists(`${livePath}.initialized`);
  const liveTemporaryBefore = temporaryCounter;
  context.setAdmittedAuthState("admitted");
  const liveAdmitted = await liveStore.readAdjustmentSettingsStatus();
  const settingsPath = join(root, "state", "forecast-adjustment-settings.json");
  const markerPath = `${settingsPath}.initialized`;
  const store = new context.FileSettingsStore(settingsPath);
  const fresh = await store.readAdjustmentSettingsStatus();
  const freshModes = `${await mode(dirname(settingsPath))}/${await mode(settingsPath)}/${await mode(markerPath)}`;
  const written = await store.writeAdjustmentSettings({
    version: 1, temperature: true, wind: false, rain: false,
  });
  const afterWrite = await store.readAdjustmentSettingsStatus();
  await writeFile(settingsPath, "{broken json\n", { mode: 0o600 });
  const corrupt = await store.readAdjustmentSettingsStatus();
  renameTrace.length = 0;
  await store.writeAdjustmentSettings({
    version: 1, temperature: false, wind: false, rain: true,
  });
  const repaired = await store.readAdjustmentSettingsStatus();
  const repairOrder = renameTrace.join(",");
  await writeFile(markerPath, "damaged marker\n", { mode: 0o600 });
  const damagedMarker = await store.readAdjustmentSettingsStatus();
  renameTrace.length = 0;
  await store.writeAdjustmentSettings({
    version: 1, temperature: true, wind: true, rain: false,
  });
  const markerRepaired = await store.readAdjustmentSettingsStatus();
  const markerRepairOrder = renameTrace.join(",");
  await unlink(markerPath);
  renameTrace.length = 0;
  await store.writeAdjustmentSettings({
    version: 1, temperature: false, wind: true, rain: false,
  });
  const missingMarkerRepaired = await store.readAdjustmentSettingsStatus();
  const missingMarkerRepairOrder = renameTrace.join(",");

  const existingPath = join(root, "existing", "forecast-adjustment-settings.json");
  await mkdir(dirname(existingPath), { mode: 0o700, recursive: true });
  await writeFile(existingPath, JSON.stringify({
    version: 1, temperature: true, wind: false, rain: true,
  }) + "\n", { mode: 0o600 });
  const existingStore = new context.FileSettingsStore(existingPath);
  const preservedExisting = await existingStore.readAdjustmentSettingsStatus();
  const existingModes = `${await mode(dirname(existingPath))}/${await mode(existingPath)}/${await mode(`${existingPath}.initialized`)}`;

  process.stdout.write(`live-init=${statusSignature(liveMissing)}:${statusSignature(liveInvalid)}:${statusSignature(liveAdmitted)}:${liveSettingsBefore}/${liveMarkerBefore}/${liveTemporaryBefore}\n`);
  process.stdout.write(`fresh=${statusSignature(fresh)}:${freshModes}\n`);
  process.stdout.write(`write=${statusSignature({ settings: written, error: null })}:${statusSignature(afterWrite)}\n`);
  process.stdout.write(`corrupt=${statusSignature(corrupt)}\n`);
  process.stdout.write(`repair=${statusSignature(repaired)}:${repairOrder}\n`);
  process.stdout.write(`damaged-marker=${statusSignature(damagedMarker)}\n`);
  process.stdout.write(`marker-repair=${statusSignature(markerRepaired)}:${markerRepairOrder}\n`);
  process.stdout.write(`missing-marker-repair=${statusSignature(missingMarkerRepaired)}:${missingMarkerRepairOrder}\n`);
  process.stdout.write(`preserved-existing=${statusSignature(preservedExisting)}:${existingModes}\n`);
} finally {
  await rm(root, { force: true, recursive: true });
}
