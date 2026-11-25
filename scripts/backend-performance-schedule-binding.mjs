import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";

const FUNCTIONAL_SCHEDULE_SOURCE_URL =
  new URL("./backend-performance-functional-schedule.mjs", import.meta.url);
const FUNCTIONAL_SCHEDULE_SOURCE_PIN = Object.freeze({
  bytes: 15179,
  sha256: "268317431bf2aa78d4a4774be0a8d442970d9a842194f6ff480542b398b05110",
});
const EXPECTED_DEFINITION_SHA256 =
  "7cef887550b999edd4af2e5095778031abd2ec3a454e5a2324c67556c31f9be9";
const DIGEST_ALGORITHM = "SHA-256";
const DIGEST_ENCODING = "lowercase-hex";
const CANONICALIZATION = "recursive-json-ecmascript-code-unit-v1";
const ROLE_GENERATIONS = Object.freeze({
  baseline: "node-v1",
  candidate: "rust-v1",
});

export const BACKEND_PERFORMANCE_SCHEDULE_BINDING_VERSION =
  "weather-backend-performance-schedule-binding/v1";
export const BACKEND_PERFORMANCE_SCHEDULE_PAIR_VERSION =
  "weather-backend-performance-schedule-binding-pair/v1";

// create one coded binding error
function fail(code, message) {
  const error = new Error(message);
  error.code = code;
  throw error;
}

// require one exact binding invariant
function requireInvariant(condition, code, message) {
  // fail closed on missing or ambiguous binding data
  if (!condition) {
    fail(code, message);
  }
}

// hash one exact UTF-8 byte sequence
function sha256(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}

// compare strings by ECMAScript code-unit order
function compareText(left, right) {
  // preserve equal keys without locale behavior
  if (left === right) {
    return 0;
  }
  return left < right ? -1 : 1;
}

// serialize one closed plain JSON value
function canonicalJson(value, path = "$") {
  // preserve null as one primitive
  if (value === null) {
    return "null";
  }

  // preserve every declared array position in order
  if (Array.isArray(value)) {
    const children = [];
    // reject sparse arrays before serializing their children
    for (let index = 0; index < value.length; index += 1) {
      requireInvariant(
        Object.hasOwn(value, index),
        "schedule-binding.schema",
        `schedule binding contains an array hole at ${path}[${String(index)}]`,
      );
      children.push(canonicalJson(value[index], `${path}[${String(index)}]`));
    }
    return `[${children.join(",")}]`;
  }

  // serialize only ordinary parsed-json records
  if (typeof value === "object") {
    const prototype = Object.getPrototypeOf(value);
    requireInvariant(
      prototype === Object.prototype || prototype === null,
      "schedule-binding.schema",
      `schedule binding contains an exotic object at ${path}`,
    );
    const keys = Object.keys(value).sort(compareText);
    return `{${keys.map(
      // bind one normalized key to its normalized child
      (key) => `${JSON.stringify(key)}:${canonicalJson(value[key], `${path}.${key}`)}`,
    ).join(",")}}`;
  }

  // retain only finite JSON primitives
  if (typeof value === "string" || typeof value === "boolean"
    || (typeof value === "number" && Number.isFinite(value))) {
    return JSON.stringify(value);
  }
  fail("schedule-binding.schema", `schedule binding contains a non-json value at ${path}`);
}

// freeze one complete binding value
function deepFreeze(value) {
  // freeze every nested child before its parent
  if (value !== null && typeof value === "object" && !Object.isFrozen(value)) {
    // visit each closed child exactly once
    for (const child of Object.values(value)) {
      deepFreeze(child);
    }
    Object.freeze(value);
  }
  return value;
}

// compare one exact closed value without trusting its digest fields
function sameCanonicalValue(left, right) {
  return canonicalJson(left) === canonicalJson(right);
}

const functionalScheduleSourceBytes = readFileSync(FUNCTIONAL_SCHEDULE_SOURCE_URL);
requireInvariant(
  functionalScheduleSourceBytes.length === FUNCTIONAL_SCHEDULE_SOURCE_PIN.bytes
    && sha256(functionalScheduleSourceBytes) === FUNCTIONAL_SCHEDULE_SOURCE_PIN.sha256,
  "schedule-binding.source-pin",
  "functional schedule builder raw identity changed",
);

const functionalScheduleModule = await import(FUNCTIONAL_SCHEDULE_SOURCE_URL.href);
const {
  BACKEND_PERFORMANCE_FUNCTIONAL_DEFINITION_SHA256,
  BACKEND_PERFORMANCE_FUNCTIONAL_ORACLE_VERSION,
  BACKEND_PERFORMANCE_FUNCTIONAL_SCHEDULE_VERSION,
  buildBackendPerformanceFunctionalSchedule,
} = functionalScheduleModule;

requireInvariant(
  BACKEND_PERFORMANCE_FUNCTIONAL_DEFINITION_SHA256 === EXPECTED_DEFINITION_SHA256
    && typeof BACKEND_PERFORMANCE_FUNCTIONAL_ORACLE_VERSION === "string"
    && typeof BACKEND_PERFORMANCE_FUNCTIONAL_SCHEDULE_VERSION === "string"
    && typeof buildBackendPerformanceFunctionalSchedule === "function",
  "schedule-binding.builder-contract",
  "functional schedule builder contract changed",
);

const authoritativePlan = buildBackendPerformanceFunctionalSchedule();
const authoritativePlanJson = canonicalJson(authoritativePlan);
const AUTHORITATIVE_SCHEDULE_IDENTITY = deepFreeze({
  canonicalByteLength: Buffer.byteLength(authoritativePlanJson, "utf8"),
  canonicalization: CANONICALIZATION,
  definitionSha256: BACKEND_PERFORMANCE_FUNCTIONAL_DEFINITION_SHA256,
  digest: {
    algorithm: DIGEST_ALGORITHM,
    encoding: DIGEST_ENCODING,
    value: sha256(Buffer.from(authoritativePlanJson, "utf8")),
  },
  oracleContractVersion: BACKEND_PERFORMANCE_FUNCTIONAL_ORACLE_VERSION,
  scheduleContractVersion: BACKEND_PERFORMANCE_FUNCTIONAL_SCHEDULE_VERSION,
});

// require one exact role and generation pair
function normalizeRoleInput(input) {
  requireInvariant(
    input !== null && typeof input === "object" && !Array.isArray(input)
      && (Object.getPrototypeOf(input) === Object.prototype
        || Object.getPrototypeOf(input) === null),
    "schedule-binding.role",
    "schedule binding role input must be one plain record",
  );
  const keys = Object.keys(input).sort(compareText);
  requireInvariant(
    sameCanonicalValue(keys, ["generation", "role"]),
    "schedule-binding.role",
    "schedule binding role input must contain only generation and role",
  );
  requireInvariant(
    typeof input.role === "string" && typeof input.generation === "string"
      && Object.hasOwn(ROLE_GENERATIONS, input.role),
    "schedule-binding.role",
    "schedule binding role and generation must be exact strings",
  );
  const expectedGeneration = ROLE_GENERATIONS[input.role];
  requireInvariant(
    input.generation === expectedGeneration,
    "schedule-binding.role",
    "schedule binding role and generation do not match",
  );
  return { generation: input.generation, role: input.role };
}

// build one noncanonical companion identity for the authoritative plan
export function buildBackendPerformanceScheduleBinding(input, ...rest) {
  requireInvariant(
    rest.length === 0,
    "schedule-binding.arguments",
    "schedule binding accepts exactly one role input",
  );
  const role = normalizeRoleInput(input);
  const identity = {
    generation: role.generation,
    role: role.role,
    schedule: AUTHORITATIVE_SCHEDULE_IDENTITY,
  };
  const identityJson = canonicalJson(identity);
  return deepFreeze({
    canonicalEligible: false,
    contractVersion: BACKEND_PERFORMANCE_SCHEDULE_BINDING_VERSION,
    identity,
    identityDigest: {
      algorithm: DIGEST_ALGORITHM,
      canonicalByteLength: Buffer.byteLength(identityJson, "utf8"),
      canonicalization: CANONICALIZATION,
      encoding: DIGEST_ENCODING,
      value: sha256(Buffer.from(identityJson, "utf8")),
    },
    metadataKind: "performance-schedule-companion",
  });
}

// require one supplied binding to equal its authoritative role binding
function requireAuthoritativeBinding(binding, role, generation) {
  const expected = buildBackendPerformanceScheduleBinding({ generation, role });
  requireInvariant(
    sameCanonicalValue(binding, expected),
    "schedule-binding.mismatch",
    `${role} binding does not match the authoritative functional schedule`,
  );
  return expected;
}

// pair exact baseline and candidate companion bindings
export function pairBackendPerformanceScheduleBindings(
  baselineBinding,
  candidateBinding,
  ...rest
) {
  requireInvariant(
    rest.length === 0,
    "schedule-binding.arguments",
    "schedule binding pair accepts exactly two bindings",
  );
  const baseline = requireAuthoritativeBinding(
    baselineBinding,
    "baseline",
    ROLE_GENERATIONS.baseline,
  );
  const candidate = requireAuthoritativeBinding(
    candidateBinding,
    "candidate",
    ROLE_GENERATIONS.candidate,
  );
  requireInvariant(
    baseline.identityDigest.value !== candidate.identityDigest.value
      && baseline.identity.schedule.digest.value === candidate.identity.schedule.digest.value,
    "schedule-binding.pair",
    "baseline and candidate binding identities are not one matched schedule",
  );
  const matched = {
    baselineIdentitySha256: baseline.identityDigest.value,
    candidateIdentitySha256: candidate.identityDigest.value,
    definitionSha256: baseline.identity.schedule.definitionSha256,
    planSha256: baseline.identity.schedule.digest.value,
  };
  const matchedJson = canonicalJson(matched);
  return deepFreeze({
    canonicalEligible: false,
    contractVersion: BACKEND_PERFORMANCE_SCHEDULE_PAIR_VERSION,
    matched,
    matchedDigest: {
      algorithm: DIGEST_ALGORITHM,
      canonicalByteLength: Buffer.byteLength(matchedJson, "utf8"),
      canonicalization: CANONICALIZATION,
      encoding: DIGEST_ENCODING,
      value: sha256(Buffer.from(matchedJson, "utf8")),
    },
    metadataKind: "performance-schedule-pair-companion",
  });
}
