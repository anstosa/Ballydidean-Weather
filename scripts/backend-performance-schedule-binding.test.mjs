import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import test from "node:test";

import {
  BACKEND_PERFORMANCE_SCHEDULE_BINDING_VERSION,
  BACKEND_PERFORMANCE_SCHEDULE_PAIR_VERSION,
  buildBackendPerformanceScheduleBinding,
  pairBackendPerformanceScheduleBindings,
} from "./backend-performance-schedule-binding.mjs";
import {
  BACKEND_PERFORMANCE_FUNCTIONAL_DEFINITION_SHA256,
  BACKEND_PERFORMANCE_FUNCTIONAL_ORACLE_VERSION,
  BACKEND_PERFORMANCE_FUNCTIONAL_SCHEDULE_VERSION,
  buildBackendPerformanceFunctionalSchedule,
} from "./backend-performance-functional-schedule.mjs";

// compare strings by code-unit order
function compareText(left, right) {
  // preserve equal text
  if (left === right) {
    return 0;
  }
  return left < right ? -1 : 1;
}

// serialize one closed test value
function canonicalJson(value) {
  // preserve null
  if (value === null) {
    return "null";
  }

  // preserve array order
  if (Array.isArray(value)) {
    return `[${value.map(
      // serialize one array child
      (child) => canonicalJson(child),
    ).join(",")}]`;
  }

  // normalize object key order
  if (typeof value === "object") {
    return `{${Object.keys(value).sort(compareText).map(
      // serialize one object child
      (key) => `${JSON.stringify(key)}:${canonicalJson(value[key])}`,
    ).join(",")}}`;
  }
  return JSON.stringify(value);
}

// hash one canonical test value
function canonicalSha256(value) {
  return createHash("sha256").update(canonicalJson(value), "utf8").digest("hex");
}

// clone one binding for a negative assertion
function clone(value) {
  return JSON.parse(JSON.stringify(value));
}

// assert recursive immutability
function assertDeepFrozen(value) {
  // inspect every nested record and array
  if (value !== null && typeof value === "object") {
    assert.equal(Object.isFrozen(value), true);
    // recurse through every child
    for (const child of Object.values(value)) {
      assertDeepFrozen(child);
    }
  }
}

// build the two exact generation roles
function exactBindings() {
  return {
    baseline: buildBackendPerformanceScheduleBinding({
      generation: "node-v1",
      role: "baseline",
    }),
    candidate: buildBackendPerformanceScheduleBinding({
      generation: "rust-v1",
      role: "candidate",
    }),
  };
}

// verify both exact role bindings
test("binds both roles to the complete authoritative plan digest", () => {
  const plan = buildBackendPerformanceFunctionalSchedule();
  const planJson = canonicalJson(plan);
  const expectedPlanSha256 = createHash("sha256").update(planJson, "utf8").digest("hex");
  const { baseline, candidate } = exactBindings();
  assert.equal(baseline.contractVersion, BACKEND_PERFORMANCE_SCHEDULE_BINDING_VERSION);
  assert.equal(baseline.canonicalEligible, false);
  assert.equal(baseline.metadataKind, "performance-schedule-companion");
  assert.equal(baseline.identity.role, "baseline");
  assert.equal(baseline.identity.generation, "node-v1");
  assert.equal(candidate.identity.role, "candidate");
  assert.equal(candidate.identity.generation, "rust-v1");
  assert.equal(baseline.identity.schedule.digest.algorithm, "SHA-256");
  assert.equal(baseline.identity.schedule.digest.encoding, "lowercase-hex");
  assert.equal(baseline.identity.schedule.digest.value, expectedPlanSha256);
  assert.equal(candidate.identity.schedule.digest.value, expectedPlanSha256);
  assert.equal(baseline.identity.schedule.canonicalByteLength, Buffer.byteLength(planJson, "utf8"));
  assert.equal(
    baseline.identity.schedule.definitionSha256,
    BACKEND_PERFORMANCE_FUNCTIONAL_DEFINITION_SHA256,
  );
  assert.equal(
    baseline.identity.schedule.scheduleContractVersion,
    BACKEND_PERFORMANCE_FUNCTIONAL_SCHEDULE_VERSION,
  );
  assert.equal(
    baseline.identity.schedule.oracleContractVersion,
    BACKEND_PERFORMANCE_FUNCTIONAL_ORACLE_VERSION,
  );
  assert.notEqual(baseline.identityDigest.value, candidate.identityDigest.value);
  assertDeepFrozen(baseline);
  assertDeepFrozen(candidate);
});

// verify deterministic closed builder inputs
test("builds deterministic role identities without schedule overrides", () => {
  const first = buildBackendPerformanceScheduleBinding({
    generation: "node-v1",
    role: "baseline",
  });
  const second = buildBackendPerformanceScheduleBinding({
    generation: "node-v1",
    role: "baseline",
  });
  assert.notStrictEqual(first, second);
  assert.deepEqual(first, second);
  assert.equal(first.identityDigest.algorithm, "SHA-256");
  assert.equal(first.identityDigest.encoding, "lowercase-hex");
  assert.equal(first.identityDigest.value, canonicalSha256(first.identity));
  assert.throws(
    // omit the required role input
    () => buildBackendPerformanceScheduleBinding(),
    /one plain record/,
  );
  assert.throws(
    // mismatch the fixed role generation
    () => buildBackendPerformanceScheduleBinding({
      generation: "rust-v1",
      role: "baseline",
    }),
    /role and generation/,
  );
  assert.throws(
    // reject parsed-json array coercion
    () => buildBackendPerformanceScheduleBinding({
      generation: "node-v1",
      role: ["baseline"],
    }),
    /exact strings/,
  );
  assert.throws(
    // reject inherited map property names
    () => buildBackendPerformanceScheduleBinding({
      generation: "node-v1",
      role: "toString",
    }),
    /exact strings/,
  );
  assert.throws(
    // add an embedded schedule override
    () => buildBackendPerformanceScheduleBinding({
      generation: "node-v1",
      role: "baseline",
      schedule: "legacy-v3",
    }),
    /only generation and role/,
  );
  assert.throws(
    // add a positional schedule override
    () => buildBackendPerformanceScheduleBinding(
      { generation: "node-v1", role: "baseline" },
      { schedule: "override" },
    ),
    /exactly one role input/,
  );
});

// verify one immutable matched pair
test("pairs exact baseline and candidate bindings", () => {
  const { baseline, candidate } = exactBindings();
  const paired = pairBackendPerformanceScheduleBindings(baseline, candidate);
  assert.equal(paired.contractVersion, BACKEND_PERFORMANCE_SCHEDULE_PAIR_VERSION);
  assert.equal(paired.canonicalEligible, false);
  assert.equal(paired.metadataKind, "performance-schedule-pair-companion");
  assert.equal(paired.matched.baselineIdentitySha256, baseline.identityDigest.value);
  assert.equal(paired.matched.candidateIdentitySha256, candidate.identityDigest.value);
  assert.equal(paired.matched.definitionSha256, baseline.identity.schedule.definitionSha256);
  assert.equal(paired.matched.planSha256, baseline.identity.schedule.digest.value);
  assert.equal(paired.matchedDigest.algorithm, "SHA-256");
  assert.equal(paired.matchedDigest.encoding, "lowercase-hex");
  assert.equal(paired.matchedDigest.value, canonicalSha256(paired.matched));
  assertDeepFrozen(paired);
});

// verify closed pair rejection paths
test("rejects swapped, fabricated, legacy, missing, and extra bindings", () => {
  const { baseline, candidate } = exactBindings();
  assert.throws(
    // swap the fixed comparison roles
    () => pairBackendPerformanceScheduleBindings(candidate, baseline),
    /baseline binding/,
  );

  const fabricatedBaseline = clone(baseline);
  const fabricatedCandidate = clone(candidate);
  fabricatedBaseline.identity.schedule.digest.value = "0".repeat(64);
  fabricatedCandidate.identity.schedule.digest.value = "0".repeat(64);
  fabricatedBaseline.identityDigest.value = canonicalSha256(fabricatedBaseline.identity);
  fabricatedCandidate.identityDigest.value = canonicalSha256(fabricatedCandidate.identity);
  assert.throws(
    // reuse one fabricated plan digest
    () => pairBackendPerformanceScheduleBindings(fabricatedBaseline, fabricatedCandidate),
    /baseline binding/,
  );

  const wrongDefinition = clone(candidate);
  wrongDefinition.identity.schedule.definitionSha256 = "1".repeat(64);
  wrongDefinition.identityDigest.value = canonicalSha256(wrongDefinition.identity);
  assert.throws(
    // mismatch the candidate definition
    () => pairBackendPerformanceScheduleBindings(baseline, wrongDefinition),
    /candidate binding/,
  );

  const legacy = clone(candidate);
  legacy.identity.schedule.scheduleContractVersion =
    "weather-backend-performance-planned-schedule/v3";
  legacy.identityDigest.value = canonicalSha256(legacy.identity);
  assert.throws(
    // substitute the accepted legacy v3 plan
    () => pairBackendPerformanceScheduleBindings(baseline, legacy),
    /candidate binding/,
  );

  const missing = clone(candidate);
  delete missing.identity.schedule.oracleContractVersion;
  missing.identityDigest.value = canonicalSha256(missing.identity);
  assert.throws(
    // omit one required oracle identity
    () => pairBackendPerformanceScheduleBindings(baseline, missing),
    /candidate binding/,
  );

  const inexact = clone(candidate);
  inexact.identity.schedule.digest.encoding = "hex";
  inexact.identityDigest.value = canonicalSha256(inexact.identity);
  assert.throws(
    // weaken the exact digest encoding
    () => pairBackendPerformanceScheduleBindings(baseline, inexact),
    /candidate binding/,
  );

  const extra = clone(candidate);
  extra.scheduleOverride = { attemptId: "override" };
  assert.throws(
    // add one binding schedule override
    () => pairBackendPerformanceScheduleBindings(baseline, extra),
    /candidate binding/,
  );
  assert.throws(
    // add one positional pair override
    () => pairBackendPerformanceScheduleBindings(
      baseline,
      candidate,
      { schedule: "override" },
    ),
    /exactly two bindings/,
  );
});
