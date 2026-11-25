import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import {
  canonicalJsonBytes,
  canonicalObjectSha256,
  canonicalSha256,
  FORECAST_ADJUSTMENT_WIND_CANARY_RAW_REGISTRY_BYTES,
  FORECAST_ADJUSTMENT_WIND_CANARY_RAW_REGISTRY_SHA256,
  FORECAST_ADJUSTMENT_WIND_MAINTENANCE_METRIC_BANDS,
  validateForecastAdjustmentWindCanaryRuntimeBundleLinks,
  verifyForecastAdjustmentWindCanaryRuntimeBundle,
} from "../../../../packages/forecast-adjustment/dist/index.js";

const REPOSITORY_ROOT = join(dirname(fileURLToPath(import.meta.url)), "../../../..");
const CONFIG_ROOT = join(REPOSITORY_ROOT, "config/forecast-adjustments");
const REGISTRY_PATH = join(CONFIG_ROOT, "ballydidean-wind-canary.json");
const FINITE_BUNDLE_SHA256 =
  "5e8b2e3932111621af6785a1b16dfd22edc0a2d26059c6e396654c70119abbe1";

// decode one checked-in JSON member without changing its source bytes
function readJson(path) {
  const bytes = readFileSync(path);
  return { bytes, value: JSON.parse(bytes.toString("utf8")) };
}

const checkedInRegistry = readJson(REGISTRY_PATH);
const active = checkedInRegistry.value.activeBundle;
assert.notEqual(active, null);
const permanentBundlePath = join(CONFIG_ROOT, "ballydidean", active.path);
const permanentBundle = readJson(permanentBundlePath);
verifyForecastAdjustmentWindCanaryRuntimeBundle(permanentBundle.value);
validateForecastAdjustmentWindCanaryRuntimeBundleLinks(
  checkedInRegistry.value,
  permanentBundle.value,
);

const finiteBundle = readJson(join(
  CONFIG_ROOT,
  "ballydidean/wind-canary-bundles",
  `sha256-${FINITE_BUNDLE_SHA256}.json`,
));
verifyForecastAdjustmentWindCanaryRuntimeBundle(finiteBundle.value);
const finiteRegistry = {
  activeBundle: {
    authorizationSha256: finiteBundle.value.authorization.authorizationSha256,
    bundleSha256: finiteBundle.value.bundleSha256,
    candidateArtifactSha256: finiteBundle.value.candidate.candidateArtifactSha256,
    path: `wind-canary-bundles/sha256-${finiteBundle.value.bundleSha256}.json`,
    transferReportSha256: finiteBundle.value.transferReport.transferReportSha256,
  },
  contractVersion: "forecast-adjustment-wind-canary-registry/v1",
};
validateForecastAdjustmentWindCanaryRuntimeBundleLinks(
  finiteRegistry,
  finiteBundle.value,
);

const inactiveRegistry = {
  activeBundle: null,
  contractVersion: "forecast-adjustment-wind-canary-registry/v1",
};
const rawRegistry = {
  activeBundle: null,
  contractVersion: "forecast-adjustment-wind-canary-registry/v2",
  enabledMetricBands: FORECAST_ADJUSTMENT_WIND_MAINTENANCE_METRIC_BANDS,
  rawReason: "policy_raw",
  siteKey: "ballydidean",
};
const rawRegistryBytes = Buffer.from(canonicalJsonBytes(rawRegistry));
assert.equal(rawRegistryBytes.length, FORECAST_ADJUSTMENT_WIND_CANARY_RAW_REGISTRY_BYTES);
assert.equal(canonicalSha256(rawRegistry), FORECAST_ADJUSTMENT_WIND_CANARY_RAW_REGISTRY_SHA256);

// retain the exact checked-in canonical bytes as the differential source of truth
for (const member of [
  checkedInRegistry,
  permanentBundle,
  finiteBundle,
]) {
  assert.deepEqual(member.bytes, Buffer.from(canonicalJsonBytes(member.value)));
}

// collect only immutable hash preimages exercised by the native legacy verifier
const hashes = [];
for (const bundle of [permanentBundle.value, finiteBundle.value]) {
  for (const [member, omitted] of [
    [bundle, "bundleSha256"],
    [bundle.candidate, "candidateArtifactSha256"],
    [bundle.transferReport, "transferReportSha256"],
    [bundle.authorization, "authorizationSha256"],
  ]) {
    const { [omitted]: expected, ...unsigned } = member;
    assert.equal(canonicalObjectSha256(member, omitted), expected);
    hashes.push([unsigned, expected]);
  }
  // preserve the source-defined nested lineage and fitted material hashes
  for (const key of [
    "coefficients",
    "servedForecastIdentity",
    "trainingForecastIdentity",
    "trainingProvenance",
  ]) {
    hashes.push([bundle.candidate[key], canonicalSha256(bundle.candidate[key])]);
  }
}

// admit only clocks that occur in the original checked-in documents
const clocks = {};
function collectClocks(value) {
  // retain canonical UTC spellings only
  if (typeof value === "string") {
    const epoch = Date.parse(value);
    if (Number.isFinite(epoch) && new Date(epoch).toISOString() === value) {
      clocks[value] = epoch;
    }
    return;
  }
  // traverse the closed fixture without synthesizing additional values
  if (value !== null && typeof value === "object") {
    for (const nested of Object.values(value)) {
      collectClocks(nested);
    }
  }
}
collectClocks(permanentBundle.value);
collectClocks(finiteBundle.value);

// use the same bounded value stream as the existing runtime differential fixtures
const lines = [];
function frame(value) {
  // distinguish every JSON primitive before descending into containers
  if (value === null) {
    lines.push("z");
  } else if (typeof value === "string") {
    lines.push(`s${Buffer.from(value, "utf16le").toString("hex")}`);
  } else if (typeof value === "number") {
    lines.push(`n${String(value)}`);
  } else if (typeof value === "boolean") {
    lines.push(value ? "t" : "f");
  } else if (Array.isArray(value)) {
    lines.push(`a${value.length}`);
    // preserve array order from the original typed value
    for (const member of value) {
      frame(member);
    }
  } else {
    const fields = Object.entries(value)
      .sort(([left], [right]) => left < right ? -1 : Number(left > right));
    lines.push(`o${fields.length}`);
    // sort object fields exactly as canonical JSON does
    for (const [name, member] of fields) {
      frame(name);
      frame(member);
    }
  }
}

const permanentBytes = permanentBundle.bytes;
const authorizationBytes = Buffer.from(
  canonicalJsonBytes(permanentBundle.value.authorization),
);
const installed = {
  authorizationBase64: authorizationBytes.toString("base64"),
  authorizationBytes: [...authorizationBytes],
  authorizationMemberSha256: canonicalSha256(permanentBundle.value.authorization),
  bundleBase64: permanentBytes.toString("base64"),
  bundleBytes: [...permanentBytes],
  bundleMemberSha256: canonicalSha256(permanentBundle.value),
  finiteBundleBytes: [...finiteBundle.bytes],
  finiteRegistry,
  finiteRegistryBytes: [...Buffer.from(canonicalJsonBytes(finiteRegistry))],
  inactiveRegistry,
  inactiveRegistryBytes: [...Buffer.from(canonicalJsonBytes(inactiveRegistry))],
  rawRegistry,
  rawRegistryBase64: rawRegistryBytes.toString("base64"),
  rawRegistryBytes: [...rawRegistryBytes],
  rawRegistryMemberSha256: canonicalSha256(rawRegistry),
  registry: checkedInRegistry.value,
  registryBytes: [...checkedInRegistry.bytes],
};
// include raw member hashes because the comparator binds exact admitted bytes
hashes.push([permanentBundle.value, installed.bundleMemberSha256]);
hashes.push([
  permanentBundle.value.authorization,
  installed.authorizationMemberSha256,
]);

frame([
  permanentBundle.value,
  finiteBundle.value,
  hashes,
  clocks,
  installed,
]);
process.stdout.write(`${lines.join("\n")}\n`);
