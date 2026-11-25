import assert from "node:assert/strict";
import { mkdtemp, mkdir, readFile, rm, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

import {
  canonicalJsonBytes,
  canonicalSha256,
  createForecastAdjustmentRainRuntimeRegistryLoaderForRoot,
  createForecastAdjustmentTemperatureCanaryRuntimeLoaderForRoot,
} from "../../../../packages/forecast-adjustment/dist/index.js";

const root = fileURLToPath(new URL("../../../../config/forecast-adjustments", import.meta.url));
const registryName = "ballydidean-temperature-canary.json";
const finiteSha = "3e82073a266ca88c15f492f86bbefbca8b8cda029520af6cc78e0a0062ee50dd";
const bundleDirectory = "temperature-canary-bundles";

// read one real checked-in document without replacing its canonical bytes
async function readCanonical(path) {
  const bytes = await readFile(path, "utf8");
  const value = JSON.parse(bytes);
  assert.equal(canonicalJsonBytes(value), bytes);
  return value;
}

// instantiate the original loader with one fixed authorization clock
function load(rootPath, now, environmentKillSwitch) {
  return createForecastAdjustmentTemperatureCanaryRuntimeLoaderForRoot(rootPath, {
    environmentKillSwitch,
    now: () => now,
  }).load();
}

const registry = await readCanonical(join(root, registryName));
const permanent = await readCanonical(join(root, "ballydidean", registry.activeBundle.path));
const finite = await readCanonical(join(root, "ballydidean", bundleDirectory, `sha256-${finiteSha}.json`));
const finiteRegistry = {
  activeBundle: {
    authorizationSha256: finite.authorization.authorizationSha256,
    bundleSha256: finite.bundleSha256,
    modelSourceSha256: finite.evidence.modelSourceSha256,
    path: `${bundleDirectory}/sha256-${finiteSha}.json`,
    strengthSourceSha256: finite.evidence.strengthSourceSha256,
  },
  contractVersion: "forecast-adjustment-temperature-canary-registry/v1",
};
const rawRegistry = {
  activeBundle: null,
  contractVersion: "forecast-adjustment-temperature-canary-registry/v2",
  rawReason: "policy_raw",
  siteKey: "ballydidean",
};
const active = await load(root, permanent.authorization.activatedAt, "0");
assert.equal(active.state, "active");
assert.equal(active.bundle.bundleSha256, permanent.bundleSha256);
assert.equal((await load(root, permanent.authorization.activatedAt)).reasonCode, "canary_killed");
const rain = await createForecastAdjustmentRainRuntimeRegistryLoaderForRoot(root).load();
assert.equal(rain.state, "active");
const rainName = "ballydidean-rain-runtime.json";
assert.equal(rain.comparatorAuthority.receiptMemberSha256, canonicalSha256(await readCanonical(join(root, rainName))));

const temporary = await mkdtemp("/var/tmp/weather-temperature-original-");
let finiteActive;
let raw;
try {
  const registryPath = join(temporary, registryName);
  await writeFile(registryPath, canonicalJsonBytes(finiteRegistry));
  assert.equal((await load(temporary, finite.authorization.activatedAt, "0")).reasonCode, "bundle_missing");
  await mkdir(join(temporary, "ballydidean", bundleDirectory), { recursive: true });
  await writeFile(join(temporary, "ballydidean", finiteRegistry.activeBundle.path), canonicalJsonBytes(finite));
  finiteActive = await load(temporary, finite.authorization.activatedAt, "0");
  assert.equal(finiteActive.state, "active");
  assert.equal((await load(temporary, finite.authorization.expiresAt, "0")).reasonCode, "canary_expired");
  await writeFile(registryPath, canonicalJsonBytes(rawRegistry));
  raw = await load(temporary, finite.authorization.activatedAt, "0");
  assert.equal(raw.reasonCode, "policy_raw");
  const rawRainRegistry = {
    activeArtifact: null,
    contractVersion: "forecast-adjustment-rain-runtime-registry/v1",
    rawReason: "policy_raw",
    siteKey: "ballydidean",
  };
  await writeFile(join(temporary, rainName), canonicalJsonBytes(rawRainRegistry));
  const rawRain = await createForecastAdjustmentRainRuntimeRegistryLoaderForRoot(temporary).load();
  assert.equal(rawRain.reasonCode, "policy_raw");
  assert.equal(rawRain.comparatorAuthority.receiptMemberSha256, canonicalSha256(rawRainRegistry));
} finally {
  await rm(temporary, { recursive: true, force: true });
}

const documents = [registry, permanent, finiteRegistry, finite, rawRegistry];
const clocks = {};

// retain only original canonical clocks exercised by the native graph verifier
function collectClocks(value) {
  if (typeof value === "string") {
    const time = Date.parse(value);
    if (Number.isFinite(time) && new Date(time).toISOString() === value) {
      clocks[value] = time;
    }
  } else if (value !== null && typeof value === "object") {
    Object.values(value).forEach(collectClocks);
  }
}

// retain each exact preimage, canonical member and authorization used at startup
for (const bundle of [permanent, finite]) {
  collectClocks(bundle);
  const { bundleSha256, ...unsignedBundle } = bundle;
  const { authorizationSha256, ...unsignedAuthorization } = bundle.authorization;
  assert.equal(canonicalSha256(unsignedBundle), bundleSha256);
  assert.equal(canonicalSha256(unsignedAuthorization), authorizationSha256);
  documents.push(unsignedBundle, bundle.authorization, unsignedAuthorization);
}

// send ordered native JSON values through the existing style of test-only value frames
function frame(value, output) {
  if (value === null) {
    output.push("z");
  } else if (typeof value === "string") {
    output.push(`s${Buffer.from(value, "utf16le").toString("hex")}`);
  } else if (typeof value === "number") {
    output.push(`n${String(value)}`);
  } else if (typeof value === "boolean") {
    output.push(value ? "t" : "f");
  } else if (Array.isArray(value)) {
    output.push(`a${value.length}`);
    value.forEach((member) => frame(member, output));
  } else {
    const entries = Object.entries(value).sort(([a], [b]) => a < b ? -1 : Number(a > b));
    output.push(`o${entries.length}`);
    for (const [key, member] of entries) {
      frame(key, output);
      frame(member, output);
    }
  }
}

const output = [];
frame({
  active: {
    authority: active.comparatorAuthority,
    bundleSha256: active.bundle.bundleSha256,
  },
  clocks,
  finiteActiveAuthority: finiteActive.comparatorAuthority,
  materials: documents.map((value) => ({
    base64: Buffer.from(canonicalJsonBytes(value)).toString("base64"),
    bytes: canonicalJsonBytes(value),
    sha256: canonicalSha256(value),
    value,
  })),
  rawAuthority: raw.comparatorAuthority,
}, output);
process.stdout.write(`${output.join("\n")}\n`);
