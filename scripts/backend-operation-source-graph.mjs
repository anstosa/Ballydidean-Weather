import { createHash } from "node:crypto";

const INPUT_KEYS = Object.freeze([
  "analysisProfiles",
  "occurrences",
  "policy",
  "privateSources",
  "runtimeInventory",
  "runtimeLiveMap",
  "semanticFacts",
  "workload",
]);
const PRIVATE_SOURCE_KEYS = Object.freeze([
  "containerSpan",
  "derivedId",
  "language",
  "origins",
  "parentPath",
  "parentRawSha256",
  "path",
  "rawSha256",
  "resolutionBase",
  "sourceKind",
  "sourceText",
]);
const LANGUAGES = new Set([
  "typescript",
  "javascript",
  "python",
  "shell",
  "sql",
  "json",
  "data",
]);
const ORIGINS = new Set([
  "prerequisite",
  "initial-seed",
  "local-module",
  "runtime-opened-code",
  "raw-sql",
  "database-definition",
  "shell-source",
  "embedded-javascript",
  "runtime-data",
]);
const SHA256_PATTERN = /^[0-9a-f]{64}$/u;
const PHYSICAL_FILE_CAP = 512;
const PHYSICAL_BYTE_CAP = 16_777_216;
const PER_FILE_BYTE_CAP = 1_048_576;
const DERIVED_SOURCE_CAP = 128;
const DERIVED_BYTE_CAP = 2_097_152;
const DERIVED_PER_SOURCE_BYTE_CAP = 262_144;

// hash one exact byte sequence
function sha256(value) {
  return createHash("sha256").update(value).digest("hex");
}

// compare two text values by code point
function compareText(left, right) {
  // order unequal values deterministically
  if (left < right) {
    return -1;
  }
  // order unequal values deterministically
  if (left > right) {
    return 1;
  }
  return 0;
}

// read one exact plain data record
function exactDataRecord(value, expectedKeys, label) {
  // require one ordinary record
  if (
    value === null ||
    typeof value !== "object" ||
    (Object.getPrototypeOf(value) !== Object.prototype &&
      Object.getPrototypeOf(value) !== null)
  ) {
    throw new TypeError(`${label} must be a plain record`);
  }
  const ownKeys = Reflect.ownKeys(value);

  // require the exact reviewed keys
  if (
    ownKeys.length !== expectedKeys.length ||
    expectedKeys.some(
      // locate one missing reviewed key
      (key) => !ownKeys.includes(key),
    )
  ) {
    throw new TypeError(`${label} keys are invalid`);
  }
  const result = Object.create(null);

  // read only enumerable data properties
  for (const key of expectedKeys) {
    const descriptor = Object.getOwnPropertyDescriptor(value, key);

    // reject accessors and hidden values
    if (
      descriptor === undefined ||
      descriptor.enumerable !== true ||
      !("value" in descriptor)
    ) {
      throw new TypeError(`${label} fields are invalid`);
    }
    result[key] = descriptor.value;
  }
  return result;
}

// read one dense ordinary array
function denseArray(value, label) {
  // require the built-in array shape
  if (!Array.isArray(value) || Object.getPrototypeOf(value) !== Array.prototype) {
    throw new TypeError(`${label} must be an array`);
  }
  const ownKeys = Reflect.ownKeys(value);

  // reject extra properties and holes
  if (ownKeys.length !== value.length + 1 || !ownKeys.includes("length")) {
    throw new TypeError(`${label} array shape is invalid`);
  }
  const result = [];

  // read every explicit data element
  for (let index = 0; index < value.length; index += 1) {
    const descriptor = Object.getOwnPropertyDescriptor(value, String(index));

    // reject holes and accessors
    if (
      descriptor === undefined ||
      descriptor.enumerable !== true ||
      !("value" in descriptor)
    ) {
      throw new TypeError(`${label} array shape is invalid`);
    }
    result.push(descriptor.value);
  }
  return result;
}

// recognize one contained repository path
function isRepositoryPath(value) {
  // reject unsafe outer path forms
  if (
    typeof value !== "string" ||
    value.length === 0 ||
    value.startsWith("/") ||
    value.endsWith("/") ||
    value.includes("\\") ||
    value.includes("\0") ||
    value.includes("%")
  ) {
    return false;
  }
  const segments = value.split("/");

  // reject aliases and empty segments
  for (const segment of segments) {
    // reject one unsafe segment
    if (segment.length === 0 || segment === "." || segment === "..") {
      return false;
    }
  }
  return true;
}

// validate one sorted source-origin array
function validateOrigins(value) {
  const origins = denseArray(value, "source origins");

  // require one or more origins
  if (origins.length === 0) {
    throw new TypeError("source origins must not be empty");
  }
  let previous = null;

  // validate every sorted unique origin
  for (const origin of origins) {
    // reject unknown or repeated origins
    if (
      typeof origin !== "string" ||
      !ORIGINS.has(origin) ||
      (previous !== null && compareText(previous, origin) >= 0)
    ) {
      throw new TypeError("source origins are invalid");
    }
    previous = origin;
  }
  return origins;
}

// validate one private source record
function validatePrivateSource(value) {
  const source = exactDataRecord(value, PRIVATE_SOURCE_KEYS, "private source");
  const origins = validateOrigins(source.origins);

  // require the closed source language
  if (!LANGUAGES.has(source.language)) {
    throw new TypeError("private source language is invalid");
  }
  // require exact textual byte custody
  if (
    typeof source.sourceText !== "string" ||
    source.sourceText.includes("\0") ||
    typeof source.rawSha256 !== "string" ||
    !SHA256_PATTERN.test(source.rawSha256)
  ) {
    throw new TypeError("private source bytes are invalid");
  }
  const byteLength = Buffer.byteLength(source.sourceText, "utf8");

  // require the exact source hash
  if (sha256(Buffer.from(source.sourceText, "utf8")) !== source.rawSha256) {
    throw new TypeError("private source hash is invalid");
  }
  // require a contained resolution base
  if (
    typeof source.resolutionBase !== "string" ||
    (source.resolutionBase !== "" && !isRepositoryPath(source.resolutionBase))
  ) {
    throw new TypeError("private source resolution base is invalid");
  }

  let containerSpan = null;

  // validate physical source custody
  if (source.sourceKind === "physical") {
    // require physical path and null derived fields
    if (
      !isRepositoryPath(source.path) ||
      source.containerSpan !== null ||
      source.derivedId !== null ||
      source.parentPath !== null ||
      source.parentRawSha256 !== null
    ) {
      throw new TypeError("physical source custody is invalid");
    }
  } else if (source.sourceKind === "derived") {
    const span = exactDataRecord(
      source.containerSpan,
      ["endUtf16", "startUtf16"],
      "derived container span",
    );

    // require derived identity and parent custody
    if (
      typeof source.derivedId !== "string" ||
      !SHA256_PATTERN.test(source.derivedId) ||
      source.path !== `derived:${source.derivedId}` ||
      typeof source.parentPath !== "string" ||
      (!isRepositoryPath(source.parentPath) &&
        !/^derived:[0-9a-f]{64}$/u.test(source.parentPath)) ||
      typeof source.parentRawSha256 !== "string" ||
      !SHA256_PATTERN.test(source.parentRawSha256) ||
      !Number.isSafeInteger(span.startUtf16) ||
      !Number.isSafeInteger(span.endUtf16) ||
      span.startUtf16 < 0 ||
      span.endUtf16 < span.startUtf16
    ) {
      throw new TypeError("derived source custody is invalid");
    }
    containerSpan = {
      endUtf16: span.endUtf16,
      startUtf16: span.startUtf16,
    };
  } else {
    throw new TypeError("private source kind is invalid");
  }

  return {
    byteLength,
    containerSpan,
    derivedId: source.derivedId,
    language: source.language,
    origins,
    parentPath: source.parentPath,
    parentRawSha256: source.parentRawSha256,
    path: source.path,
    rawSha256: source.rawSha256,
    resolutionBase: source.resolutionBase,
    sourceKind: source.sourceKind,
    sourceText: source.sourceText,
  };
}

// clone one accessor-free json value
function cloneJsonData(value, label) {
  // retain supported scalar values
  if (
    value === null ||
    typeof value === "string" ||
    typeof value === "boolean"
  ) {
    return value;
  }
  // retain safe finite numbers
  if (typeof value === "number") {
    // reject ambiguous numeric values
    if (!Number.isFinite(value) || !Number.isSafeInteger(value)) {
      throw new TypeError(`${label} number is invalid`);
    }
    return value;
  }
  // clone dense arrays without evaluating accessors
  if (Array.isArray(value)) {
    const entries = denseArray(value, label);
    const result = [];

    // clone every explicit array member
    for (const entry of entries) {
      result.push(cloneJsonData(entry, label));
    }
    return result;
  }
  // reject non-record values
  if (
    typeof value !== "object" ||
    (Object.getPrototypeOf(value) !== Object.prototype &&
      Object.getPrototypeOf(value) !== null)
  ) {
    throw new TypeError(`${label} value is invalid`);
  }
  const ownKeys = Reflect.ownKeys(value);
  const result = {};

  // clone every enumerable string data field
  for (const key of ownKeys) {
    const descriptor = Object.getOwnPropertyDescriptor(value, key);

    // reject symbols accessors and hidden fields
    if (
      typeof key !== "string" ||
      key === "__proto__" ||
      descriptor === undefined ||
      descriptor.enumerable !== true ||
      !("value" in descriptor)
    ) {
      throw new TypeError(`${label} record is invalid`);
    }
    Object.defineProperty(result, key, {
      configurable: true,
      enumerable: true,
      value: cloneJsonData(descriptor.value, label),
      writable: true,
    });
  }
  return result;
}

// compare two public source files
function compareFiles(left, right) {
  return compareText(left.path, right.path);
}

// compare two safe analysis profiles
function compareProfiles(left, right) {
  // place missing scaffold identities first
  return compareText(left.batchInputSha256 ?? "", right.batchInputSha256 ?? "");
}

// recursively freeze one detached graph
function freezeRecursively(value) {
  // stop at scalars and frozen objects
  if (value === null || typeof value !== "object" || Object.isFrozen(value)) {
    return value;
  }

  // freeze every child before its parent
  for (const child of Object.values(value)) {
    freezeRecursively(child);
  }
  return Object.freeze(value);
}

// create one exact public diagnostic
function diagnostic(code, severity) {
  // derive the closed diagnostic dimension
  const dimension = code.startsWith("reader.")
    ? "reader"
    : code.startsWith("binding.")
      ? "binding"
      : "frontend";
  return {
    code,
    dimension,
    ids: [],
    messageId: code,
    severity,
    sourceSiteId: null,
  };
}

// build one exact graph scaffold
function graphResult(analysisProfiles, files, fixedPoint, diagnostics, status) {
  return freezeRecursively({
    analysisProfiles,
    contractVersion: "weather-backend-operation-source-graph/v1",
    diagnostics,
    edges: [],
    files,
    fixedPoint,
    operations: [],
    readRequests: [],
    roots: [],
    sites: [],
    status,
  });
}

// build one pure redacted graph scaffold from explicit inputs
export function buildBackendOperationSourceGraph(...argumentsList) {
  // require exactly one graph envelope
  if (argumentsList.length !== 1) {
    throw new TypeError("graph accepts one input record");
  }
  const input = exactDataRecord(argumentsList[0], INPUT_KEYS, "graph input");
  const analysisProfileValues = denseArray(
    input.analysisProfiles,
    "graph analysis profiles",
  );
  denseArray(input.occurrences, "graph occurrences");
  denseArray(input.semanticFacts, "graph semantic facts");

  // require plain checked prerequisite records
  for (const prerequisite of [
    input.policy,
    input.runtimeInventory,
    input.runtimeLiveMap,
    input.workload,
  ]) {
    // reject exotic prerequisite records
    if (
      prerequisite === null ||
      typeof prerequisite !== "object" ||
      (Object.getPrototypeOf(prerequisite) !== Object.prototype &&
        Object.getPrototypeOf(prerequisite) !== null)
    ) {
      throw new TypeError("graph prerequisite is invalid");
    }
  }

  const analysisProfiles = [];

  // detach every safe frontend profile
  for (const profile of analysisProfileValues) {
    analysisProfiles.push(cloneJsonData(profile, "analysis profile"));
  }
  analysisProfiles.sort(compareProfiles);

  const sourceValues = denseArray(input.privateSources, "graph private sources");
  const sourcesByPath = new Map();
  let conflict = false;

  // validate and coalesce identical explicit source custody
  for (const value of sourceValues) {
    const source = validatePrivateSource(value);
    const existing = sourcesByPath.get(source.path);

    // retain the first source for one locator
    if (existing === undefined) {
      sourcesByPath.set(source.path, source);
      continue;
    }
    // reject conflicting custody for one locator
    if (JSON.stringify(existing) !== JSON.stringify(source)) {
      conflict = true;
    }
  }

  const sources = [...sourcesByPath.values()];
  let physicalBytes = 0;
  let physicalFileCount = 0;
  let derivedBytes = 0;
  let derivedSourceCount = 0;
  let boundCode = null;

  // account for every admitted explicit source
  for (const source of sources) {
    // account for one physical source
    if (source.sourceKind === "physical") {
      physicalFileCount += 1;
      physicalBytes += source.byteLength;

      // retain one physical bound failure
      if (source.byteLength > PER_FILE_BYTE_CAP) {
        boundCode = "reader.bound-exceeded";
      }
    } else {
      derivedSourceCount += 1;
      derivedBytes += source.byteLength;

      // retain one derived bound failure
      if (source.byteLength > DERIVED_PER_SOURCE_BYTE_CAP) {
        boundCode = "frontend.bound-exceeded";
      }
    }
  }

  // retain aggregate physical bound failure
  if (
    physicalFileCount > PHYSICAL_FILE_CAP ||
    physicalBytes > PHYSICAL_BYTE_CAP
  ) {
    boundCode = "reader.bound-exceeded";
  }
  // retain aggregate derived bound failure
  if (
    derivedSourceCount > DERIVED_SOURCE_CAP ||
    derivedBytes > DERIVED_BYTE_CAP
  ) {
    boundCode = "frontend.bound-exceeded";
  }

  // fail closed on custody conflicts
  if (conflict) {
    return graphResult(
      analysisProfiles,
      [],
      {
        derivedBytes: 0,
        derivedSourceCount: 0,
        physicalBytes: 0,
        physicalFileCount: 0,
        roundCount: 0,
        totalBytes: 0,
      },
      [diagnostic("binding.source-conflict", "error")],
      "failed",
    );
  }
  // fail closed on explicit source bounds
  if (boundCode !== null) {
    return graphResult(
      analysisProfiles,
      [],
      {
        derivedBytes: 0,
        derivedSourceCount: 0,
        physicalBytes: 0,
        physicalFileCount: 0,
        roundCount: 0,
        totalBytes: 0,
      },
      [diagnostic(boundCode, "error")],
      "failed",
    );
  }

  const files = [];

  // redact every admitted private source
  for (const source of sources) {
    files.push({
      byteLength: source.byteLength,
      containerSpan: source.containerSpan,
      derivedId: source.derivedId,
      language: source.language,
      origins: [...source.origins],
      parentPath: source.parentPath,
      parentRawSha256: source.parentRawSha256,
      path: source.path,
      rawSha256: source.rawSha256,
      resolutionBase: source.resolutionBase,
      sourceKind: source.sourceKind,
    });
  }
  files.sort(compareFiles);
  return graphResult(
    analysisProfiles,
    files,
    {
      derivedBytes,
      derivedSourceCount,
      physicalBytes,
      physicalFileCount,
      roundCount: 0,
      totalBytes: derivedBytes + physicalBytes,
    },
    [diagnostic("frontend.unsupported", "gap")],
    "incomplete",
  );
}
