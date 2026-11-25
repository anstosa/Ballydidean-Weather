import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import assert from "node:assert/strict";

import {
  canonicalJsonBytes, canonicalObjectSha256, canonicalSha256,
  createForecastAdjustmentRuntimeBundle,
  verifyForecastAdjustmentRuntimeBundle,
} from "../../../../packages/forecast-adjustment/dist/index.js";
import { createQualifiedFixture } from "../../../../packages/forecast-adjustment/test/evidence-fixtures.mjs";

// construct the same qualified graph as the original Node runtime tests
const directory = await mkdtemp(join(tmpdir(), "weather-legacy-runtime-oracle-"));
let bundle;
try {
  bundle = createForecastAdjustmentRuntimeBundle(await createQualifiedFixture(directory));
} finally {
  await rm(directory, { force: true, recursive: true });
}

// re-address an ordinary fitted payload with insufficient root evidence
const short = structuredClone(bundle);
short.candidate.coefficients[0].effectiveEventCount = 0;
short.candidate.coefficientPayloadSha256 = canonicalSha256(short.candidate.coefficients);
short.candidate.candidateArtifactSha256 = canonicalObjectSha256(short.candidate, "candidateArtifactSha256");
short.evaluationReport.candidateArtifactSha256 = short.candidate.candidateArtifactSha256;
short.evaluationReport.evaluationReportSha256 = canonicalObjectSha256(short.evaluationReport, "evaluationReportSha256");
short.qualificationReceipt.candidateArtifactSha256 = short.candidate.candidateArtifactSha256;
short.qualificationReceipt.evaluationReportSha256 = short.evaluationReport.evaluationReportSha256;
short.qualificationReceipt.qualificationReceiptSha256 = canonicalObjectSha256(short.qualificationReceipt, "qualificationReceiptSha256");
short.bundleSha256 = canonicalObjectSha256(short, "bundleSha256");
assert.throws(() => verifyForecastAdjustmentRuntimeBundle(short));

// use a test-only typed value stream rather than implementing a JSON parser in Rust
const lines = [];
function frame(value) {
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
    for (const member of value) {
      frame(member);
    }
  } else {
    const fields = Object.entries(value).sort(([left], [right]) => left < right ? -1 : Number(left > right));
    lines.push(`o${fields.length}`);
    for (const [name, member] of fields) {
      frame(name);
      frame(member);
    }
  }
}

// provide exact checked Node hash preimages without importing crypto into the Rust crate
const hashes = [];
for (const value of [bundle, short]) {
  for (const [member, omitted] of [
    [value, "bundleSha256"],
    [value.candidate, "candidateArtifactSha256"],
    [value.evaluationReport, "evaluationReportSha256"],
    [value.qualificationReceipt, "qualificationReceiptSha256"],
  ]) {
    const { [omitted]: expected, ...unsigned } = member;
    assert.equal(canonicalSha256(unsigned), expected);
    hashes.push([unsigned, expected]);
  }
  for (const key of ["coefficients", "forecastIdentity", "trainingProvenance", "metricPolicies"]) {
    const member = value.candidate[key];
    hashes.push([member, canonicalSha256(member)]);
  }
}

// provide only the original document clocks and the two exact local-day labels at UTC midnight
const clocks = {};
function collectClocks(value) {
  if (typeof value === "string") {
    const clock = /^\d{4}-\d{2}-\d{2}$/u.test(value) ? `${value}T00:00:00.000Z` : value;
    const ms = Date.parse(clock);
    if (Number.isFinite(ms) && new Date(ms).toISOString() === clock) {
      clocks[clock] = ms;
    }
  } else if (value !== null && typeof value === "object") {
    for (const nested of Object.values(value)) {
      collectClocks(nested);
    }
  }
}
collectClocks(bundle);
const registry = {
  activeBundle: {
    bundleSha256: bundle.bundleSha256,
    candidateArtifactSha256: bundle.candidate.candidateArtifactSha256,
    evaluationReportSha256: bundle.evaluationReport.evaluationReportSha256,
    path: `bundles/sha256-${bundle.bundleSha256}.json`,
    qualificationReceiptSha256:
      bundle.qualificationReceipt.qualificationReceiptSha256,
  },
  contractVersion: "forecast-adjustment-registry/v1",
};
const bundleBytes = Buffer.from(canonicalJsonBytes(bundle));
const receiptBytes = Buffer.from(canonicalJsonBytes(bundle.qualificationReceipt));
const installed = {
  bundleBase64: bundleBytes.toString("base64"),
  bundleBytes: [...bundleBytes],
  bundleMemberSha256: canonicalSha256(bundle),
  receiptBase64: receiptBytes.toString("base64"),
  receiptBytes: [...receiptBytes],
  receiptMemberSha256: canonicalSha256(bundle.qualificationReceipt),
  inactiveRegistry: {
    activeBundle: null,
    contractVersion: "forecast-adjustment-registry/v1",
  },
  inactiveRegistryBytes: [
    ...Buffer.from(canonicalJsonBytes({
      activeBundle: null,
      contractVersion: "forecast-adjustment-registry/v1",
    })),
  ],
  registry,
  registryBytes: [...Buffer.from(canonicalJsonBytes(registry))],
};
frame([bundle, short, hashes, clocks, installed]);
process.stdout.write(lines.join("\n") + "\n");
