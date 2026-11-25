import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { dirname, posix, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import ts from "typescript";
import { renderRetainedForecastObservationStations } from "./lib/forecast-observation-rust.mjs";

const CONFIG_PATH = "config/rust-contract-roots.json";
const RUST_OUTPUT_PATH = "rust/crates/weather-contracts/src/generated.rs";
const MANIFEST_OUTPUT_PATH = "rust/crates/weather-contracts/generated.contracts.json";
const FORECAST_OBSERVATION_SOURCE_PATH = "packages/domain/src/forecast-adjustment.ts";
const FORECAST_OBSERVATION_STATION_ROOTS = [
  "ForecastAdjustmentMetric",
  "ForecastObservationProviderFamily",
  "ForecastObservationStation",
  "ForecastObservationStationKey",
];
const RETAINED_RAIN_HURDLE_WIND_SOURCE_PATH =
  "packages/forecast-adjustment/src/rain-hurdle-wind-artifact.ts";
const RETAINED_RAIN_HURDLE_WIND_OUTPUT_PATH =
  "rust/crates/weather-contracts/retained-rain-hurdle-wind.json";
const RETAINED_RAIN_HURDLE_WIND_JSON_EXPORT = "RAIN_HURDLE_WIND_ARTIFACT_JSON";
const RETAINED_RAIN_HURDLE_WIND_SHA256_EXPORT = "RAIN_HURDLE_WIND_ARTIFACT_SHA256";
const RAIN_HURDLE_WIND_RUNTIME_SOURCE_PATH =
  "packages/forecast-adjustment/src/rain-hurdle-wind.ts";
const RAIN_HURDLE_WIND_FEATURE_NAMES_SHA256_EXPORT =
  "RAIN_HURDLE_WIND_FEATURE_NAMES_SHA256";
const RETAINED_RAIN_HURDLE_WIND_HEAD_NAMES = ["0.1", "1.0", "2.5", "amount"];
const RETAINED_RAIN_HURDLE_WIND_THRESHOLDS = [0.1, 1, 2.5];
const RETAINED_RAIN_HURDLE_WIND_PROVENANCE_NAMES = [
  "wind-freeze.json",
  "report.json",
  "predictions.npz",
  "wind-states/2026-08.json",
];
const TYPESCRIPT_VERSION = "6.0.3";
const SUBSET_VERSION = "weather-ts-rust-contract-subset/v55";
const MANIFEST_SCHEMA_VERSION = "weather-rust-contracts-manifest/v1";
const ORDERED_FINITE_NUMBER_RECORD_MAPPING = "ordered-finite-f64-record";
const ORDERED_JSON_OBJECT_MAPPING = "ordered-json-object";
const ORDERED_JSON_OBJECT_ARRAY_MAPPING = "ordered-json-object-array";
const ORDERED_JSON_SCALAR_OBJECT_MAPPING = "ordered-json-scalar-object";
const ORDERED_JSON_KEY_OBJECT_MAPPING = "ordered-json-key-object";
const ORDERED_JSON_NULLABLE_STRING_OBJECT_MAPPING = "ordered-json-nullable-string-object";
const ORDERED_JSON_SCALAR_OBJECT_ARRAY_MAPPING = "ordered-json-scalar-object-array";
const READONLY_STRING_LITERAL_RECORD_MAPPING = "readonly-string-literal-record";
const READONLY_UNSIGNED_COUNTER_RECORD_MAPPING = "readonly-unsigned-counter-record";
const READONLY_NULLABLE_METRIC_RECORD = "readonly-nullable-metric-record";
const DISABLED_REASON_EXTRACT = "disabled-reason-extract";
const FINITE_METRIC_RECORD_MAPPING = "finite-metric-record";
const FINITE_PARTIAL_METRIC_RECORD_MAPPING = "finite-partial-metric-record";
const EMPTY_METRIC_RECORD_MAPPING = "empty-metric-record";
const EMPTY_JSON_ARRAY_MAPPING = "empty-json-array";
const CONST_STRING_PROPERTY_MAPPING = "const-string-property";
const CONST_SCALAR_PROPERTY_MAPPING = "const-scalar-property";
const FROZEN_CONST_STRING_PROPERTY_MAPPING = "frozen-const-string-property";
const FROZEN_CONST_STRING_PROPERTY_UNION_MAPPING = "frozen-const-string-property-union";
const FROZEN_CONST_LITERAL_OBJECT_MAPPING = "frozen-const-literal-object";
const FROZEN_WIND_METRIC_BAND_TUPLE_MAPPING = "frozen-wind-metric-band-tuple";
const CONST_STRING_MAPPING = "const-string";
const FLAT_INTERFACE_EXTENSION_MAPPING = "flat-interface-extension";
const EXACT_NULL_MAPPING = "exact-null";
const UNSIGNED_SAFE_INTEGER_ENUM_MAPPING = "unsigned-safe-integer-enum";
const UNSIGNED_SAFE_INTEGER_LITERAL_MAPPING = "unsigned-safe-integer-literal";
const GROUPED_UNSIGNED_SAFE_INTEGER_LITERAL_MAPPING = "grouped-unsigned-safe-integer-literal";
const UNSIGNED_DECIMAL_ENUM_MAPPING = "unsigned-decimal-enum";
const READONLY_INLINE_OBJECT_MAPPING = "readonly-inline-object";
const READONLY_INLINE_OBJECT_ARRAY_MAPPING = "readonly-inline-object-array";
const RUNTIME_F64_MAPPING = "runtime-f64";
const RUNTIME_F64_ARRAY_MAPPING = "runtime-f64-array";
const RUNTIME_F64_NULLABLE_ARRAY_MAPPING = "runtime-f64-nullable-array";
const ORDERED_RUNTIME_F64_NULLABLE_ARRAY_RECORD_MAPPING = "ordered-runtime-f64-nullable-array-record";
const UINT8ARRAY_BYTES_MAPPING = "uint8array-bytes";
const FLOAT32ARRAY_VALUES_MAPPING = "float32array-values";
const NUMERIC_PAIR_TEMPLATE_MAPPING = "numeric-pair-template";
const RAIN_HURDLE_HEAD_RECORD_MAPPING = "rain-hurdle-head-record";
const JS_DATE_MAPPING = "js-date";
const JS_DATE_OR_STRING_MAPPING = "js-date-or-string";
const UTILITY_DECLARATIONS_SHA256 = "bcd24271a113971ba9eb71ff8cb01bc6b0f872a85c23fdbe5d93065b375933cd";
const UTILITY_DECLARATIONS_VIRTUAL_PATH = "/__weather_contracts__/lib.es5.d.ts";
const UTILITY_DECLARATIONS_PATH = resolve(
  dirname(fileURLToPath(import.meta.resolve("typescript"))),
  "lib.es5.d.ts",
);
const PG_MARKER_INPUTS = Object.freeze({
  "pg/package.json": "e42dd36cba6e9dd8dbb6f773a2f7be8a8c3c273e18b155e42e75961a4cb8bc28",
  "@types/pg/package.json": "1137081ebb228493e9c81dd699d0d79b7ba0019be75a10272b05399de8cb49e6",
  "@types/pg/index.d.mts": "5aea76ab98173f2c230b1f78dc010da403da622c105c468ace9fe24e3b77883c",
  "@types/pg/index.d.ts": "1f7da5b623c2b1fc4f6fbb56a19df7f6fe2f3e3e7e8b526ec6892062f0f1f82e",
});
const PG_MARKER_ENTRY = "__weather_contracts__/pg/index.d.mts";
const PG_MARKER_TARGET = "__weather_contracts__/pg/index.d.ts";
const SCALAR_RECORD_MAPPINGS = new Set([
  ORDERED_JSON_SCALAR_OBJECT_MAPPING,
  ORDERED_JSON_KEY_OBJECT_MAPPING,
  ORDERED_JSON_NULLABLE_STRING_OBJECT_MAPPING,
  ORDERED_JSON_SCALAR_OBJECT_ARRAY_MAPPING,
]);
const FIELD_MAPPING_KINDS = new Set([
  "safe-unsigned-integer",
  "safe-integer",
  "finite-f64",
  ORDERED_FINITE_NUMBER_RECORD_MAPPING,
  ORDERED_JSON_OBJECT_MAPPING,
  ORDERED_JSON_OBJECT_ARRAY_MAPPING,
  ...SCALAR_RECORD_MAPPINGS,
  READONLY_STRING_LITERAL_RECORD_MAPPING,
  READONLY_UNSIGNED_COUNTER_RECORD_MAPPING,
  FINITE_METRIC_RECORD_MAPPING,
  FINITE_PARTIAL_METRIC_RECORD_MAPPING,
  EMPTY_METRIC_RECORD_MAPPING,
  EMPTY_JSON_ARRAY_MAPPING,
  CONST_STRING_PROPERTY_MAPPING,
  CONST_SCALAR_PROPERTY_MAPPING,
  FROZEN_CONST_STRING_PROPERTY_MAPPING,
  FROZEN_CONST_STRING_PROPERTY_UNION_MAPPING,
  FROZEN_CONST_LITERAL_OBJECT_MAPPING,
  FROZEN_WIND_METRIC_BAND_TUPLE_MAPPING,
  CONST_STRING_MAPPING,
  FLAT_INTERFACE_EXTENSION_MAPPING,
  EXACT_NULL_MAPPING,
  UNSIGNED_SAFE_INTEGER_ENUM_MAPPING,
  UNSIGNED_SAFE_INTEGER_LITERAL_MAPPING,
  GROUPED_UNSIGNED_SAFE_INTEGER_LITERAL_MAPPING,
  UNSIGNED_DECIMAL_ENUM_MAPPING,
  READONLY_INLINE_OBJECT_MAPPING,
  READONLY_INLINE_OBJECT_ARRAY_MAPPING,
  RUNTIME_F64_MAPPING,
  RUNTIME_F64_ARRAY_MAPPING,
  RUNTIME_F64_NULLABLE_ARRAY_MAPPING,
  ORDERED_RUNTIME_F64_NULLABLE_ARRAY_RECORD_MAPPING,
  UINT8ARRAY_BYTES_MAPPING,
  FLOAT32ARRAY_VALUES_MAPPING,
  NUMERIC_PAIR_TEMPLATE_MAPPING,
  RAIN_HURDLE_HEAD_RECORD_MAPPING,
  JS_DATE_MAPPING,
  JS_DATE_OR_STRING_MAPPING,
  "utc-iso",
]);
const UTILITY_FIELD_MAPPING_KINDS = new Set([
  ORDERED_RUNTIME_F64_NULLABLE_ARRAY_RECORD_MAPPING,
  ORDERED_FINITE_NUMBER_RECORD_MAPPING,
  ORDERED_JSON_OBJECT_MAPPING,
  ORDERED_JSON_OBJECT_ARRAY_MAPPING,
  ...SCALAR_RECORD_MAPPINGS,
  READONLY_STRING_LITERAL_RECORD_MAPPING,
  READONLY_UNSIGNED_COUNTER_RECORD_MAPPING,
  FINITE_METRIC_RECORD_MAPPING,
  FINITE_PARTIAL_METRIC_RECORD_MAPPING,
  EMPTY_METRIC_RECORD_MAPPING,
  FROZEN_CONST_STRING_PROPERTY_MAPPING,
  FROZEN_CONST_STRING_PROPERTY_UNION_MAPPING,
  FROZEN_CONST_LITERAL_OBJECT_MAPPING,
  READONLY_INLINE_OBJECT_MAPPING,
  READONLY_INLINE_OBJECT_ARRAY_MAPPING,
  UINT8ARRAY_BYTES_MAPPING,
  FLOAT32ARRAY_VALUES_MAPPING,
  RAIN_HURDLE_HEAD_RECORD_MAPPING,
  JS_DATE_MAPPING,
  JS_DATE_OR_STRING_MAPPING,
]);
const NUMERIC_FIELD_MAPPING_KINDS = new Set([
  "safe-unsigned-integer",
  "safe-integer",
  "finite-f64",
]);
const READONLY_TAGGED_FIELD_MAPPINGS = new Set([
  "utc-iso",
  "safe-unsigned-integer",
  UNSIGNED_SAFE_INTEGER_ENUM_MAPPING,
]);
const RUST_KEYWORDS = new Set([
  "abstract",
  "as",
  "async",
  "await",
  "become",
  "box",
  "break",
  "const",
  "continue",
  "crate",
  "do",
  "dyn",
  "else",
  "enum",
  "extern",
  "false",
  "final",
  "fn",
  "for",
  "gen",
  "if",
  "impl",
  "in",
  "let",
  "loop",
  "macro",
  "match",
  "mod",
  "move",
  "mut",
  "override",
  "priv",
  "pub",
  "ref",
  "return",
  "self",
  "Self",
  "static",
  "struct",
  "super",
  "trait",
  "true",
  "try",
  "type",
  "typeof",
  "union",
  "unsafe",
  "unsized",
  "use",
  "virtual",
  "where",
  "while",
  "yield",
]);
const INVALID_RAW_RUST_IDENTIFIERS = new Set(["crate", "self", "Self", "super"]);
const GENERATED_RUST_TYPE_NAMES = new Set([
  "FieldPresence",
  "JsDate",
  "JsDateOrString",
  "Nullable",
  "OrderedFiniteNumberRecord",
  "String",
  "Vec",
  "bool",
  "f64",
  "i64",
  "str",
  "u64",
]);
// identify plain configuration records
function isPlainObject(value) {
  // reject null, arrays, and class instances
  if (value === null || typeof value !== "object" || Array.isArray(value)) {
    return false;
  }
  const prototype = Object.getPrototypeOf(value);
  return prototype === Object.prototype || prototype === null;
}

// reject misspelled or future configuration fields
function assertExactKeys(value, allowedKeys, label) {
  const allowed = new Set(allowedKeys);

  // inspect every supplied key
  for (const key of Object.keys(value)) {
    // fail closed on unknown configuration
    if (!allowed.has(key)) {
      throw new Error(`${label} has unsupported key ${JSON.stringify(key)}`);
    }
  }
}

// normalize one repository input path with a fixed suffix
function normalizeRepositoryPath(value, suffix, label) {
  // reject absolute, parent, platform-specific and wrong-kind inputs
  if (
    typeof value !== "string" ||
    value.length === 0 ||
    value.includes("\\") ||
    value.startsWith("/") ||
    !value.endsWith(suffix) ||
    (suffix === "package.json" && posix.basename(value) !== suffix) ||
    posix.normalize(value) !== value ||
    value === ".." ||
    value.startsWith("../")
  ) {
    throw new Error(`${label} must be a normalized repository-relative ${suffix} path`);
  }
  return value;
}

// normalize one explicit named object-union variant map
function normalizeObjectUnion(value, label) {
  // require one closed source-name to Rust-variant record
  if (!isPlainObject(value)) {
    throw new Error(`${label} must be a plain object`);
  }

  const sourceNames = Object.keys(value).sort();

  // require a real closed sum rather than one wrapped payload
  if (sourceNames.length < 2) {
    throw new Error(`${label} must have at least two variants`);
  }

  const rustNames = new Set();
  const objectUnion = Object.create(null);

  // sort source identities so input insertion order cannot affect canonical config
  for (const sourceName of sourceNames) {
    // constrain payload names to portable named TypeScript identifiers
    if (sourceName === "_" || !/^[A-Za-z_][A-Za-z0-9_]*$/u.test(sourceName)) {
      throw new Error(`${label} has invalid source interface name ${JSON.stringify(sourceName)}`);
    }

    const rustName = value[sourceName];

    // require an explicit Pascal-start portable Rust variant identifier
    if (typeof rustName !== "string" || !/^[A-Z][A-Za-z0-9_]*$/u.test(rustName)) {
      throw new Error(`${label}.${sourceName} must be a Pascal-start ASCII Rust identifier`);
    }

    // reject reserved Rust spellings including the non-raw Self keyword
    if (RUST_KEYWORDS.has(rustName)) {
      throw new Error(`${label}.${sourceName} is a Rust keyword`);
    }

    // keep every source arm distinguishable in Rust storage
    if (rustNames.has(rustName)) {
      throw new Error(`duplicate object union Rust variant ${rustName} at ${label}`);
    }
    rustNames.add(rustName);
    objectUnion[sourceName] = rustName;
  }

  return objectUnion;
}

// normalize one ordered list of exact direct interface field names
function normalizeConfiguredFieldNames(value, label) {
  // require at least one explicitly ordered field name
  if (!Array.isArray(value) || value.length === 0) {
    throw new Error(`${label} must be a non-empty array`);
  }

  const names = new Set();

  // preserve configured source order while rejecting ambiguous ownership
  for (const fieldName of value) {
    // constrain names to direct portable property identifiers
    if (
      typeof fieldName !== "string" ||
      fieldName === "_" ||
      !/^[A-Za-z_][A-Za-z0-9_]*$/u.test(fieldName)
    ) {
      throw new Error(`${label} must contain non-underscore ASCII identifiers`);
    }

    // reject repeated selections rather than normalizing them
    if (names.has(fieldName)) {
      throw new Error(`${label} has duplicate field ${fieldName}`);
    }
    names.add(fieldName);
  }

  return [...value];
}

// normalize one closed root-level Pick projection
function normalizePick(value, label) {
  // require the source interface and exact ordered property names
  if (!isPlainObject(value)) {
    throw new Error(`${label} must be a plain object`);
  }
  assertExactKeys(value, ["root", "fields"], label);

  // reject names that are not direct configured interface identifiers
  if (typeof value.root !== "string" || value.root === "_" || !/^[A-Za-z_][A-Za-z0-9_]*$/u.test(value.root)) {
    throw new Error(`${label}.root must be a non-underscore ASCII identifier`);
  }
  return { root: value.root, fields: normalizeConfiguredFieldNames(value.fields, `${label}.fields`) };
}

// normalize one reviewed defaulted list type parameter
function normalizeDefaultedListParameter(value, label) {
  // require only the parameter, default root, and selected list field
  if (!isPlainObject(value)) {
    throw new Error(`${label} must be a plain object`);
  }
  assertExactKeys(value, ["name", "defaultRoot", "field"], label);

  const normalized = {};

  // preserve each identifier only when it is directly usable in Rust
  for (const key of ["name", "defaultRoot", "field"]) {
    const identifier = value[key];

    // reject aliases, paths, underscores, and Rust keyword spellings
    if (
      typeof identifier !== "string" ||
      identifier === "_" ||
      !/^[A-Za-z_][A-Za-z0-9_]*$/u.test(identifier) ||
      RUST_KEYWORDS.has(identifier)
    ) {
      throw new Error(`${label}.${key} must be a non-keyword ASCII Rust identifier`);
    }
    normalized[key] = identifier;
  }

  // prevent the parameter from shadowing its configured default type
  if (normalized.name === normalized.defaultRoot) {
    throw new Error(`${label}.name must differ from defaultRoot`);
  }

  // reject the generator's fixed Rust storage and wrapper type names
  if (GENERATED_RUST_TYPE_NAMES.has(normalized.name)) {
    throw new Error(`${label}.name collides with a generated Rust type`);
  }
  return normalized;
}

// normalize one projected literal-union source
function normalizeLiteralUnion(value, label) {
  // require only the named source alias and projected field
  if (!isPlainObject(value)) {
    throw new Error(`${label} must be a plain object`);
  }
  assertExactKeys(value, ["sourceAlias", "field"], label);

  // constrain both names to direct portable TypeScript identifiers
  for (const key of ["sourceAlias", "field"]) {
    // reject qualified, computed, and implicit projection names
    if (
      typeof value[key] !== "string" ||
      value[key] === "_" ||
      !/^[A-Za-z_][A-Za-z0-9_]*$/u.test(value[key])
    ) {
      throw new Error(`${label}.${key} must be a non-underscore ASCII identifier`);
    }
  }

  return { sourceAlias: value.sourceAlias, field: value.field };
}

// normalize explicitly selected indexed-literal alias owners
function normalizeIndexedLiteralSources(value, label) {
  // require one non-empty exact alias-to-source record
  if (!isPlainObject(value) || Object.keys(value).length === 0) {
    throw new Error(`${label} must be a non-empty plain object`);
  }

  const indexedLiteralSources = Object.create(null);

  // sort alias identities so insertion order cannot affect canonical config
  for (const aliasName of Object.keys(value).sort()) {
    // constrain proof identities to exact portable TypeScript alias names
    if (aliasName === "_" || !/^[A-Za-z_][A-Za-z0-9_]*$/u.test(aliasName)) {
      throw new Error(`${label} has invalid alias name ${JSON.stringify(aliasName)}`);
    }
    indexedLiteralSources[aliasName] = normalizeRepositoryPath(
      value[aliasName],
      ".ts",
      `${label}.${aliasName}`,
    );
  }

  return indexedLiteralSources;
}

// normalize direct field projection metadata
function normalizeFieldProjections(value, label) {
  // require one closed projection record
  if (!isPlainObject(value) || Object.keys(value).length === 0) {
    throw new Error(`${label} must be a non-empty plain object`);
  }

  const fieldProjections = Object.create(null);

  // sort field paths so insertion order cannot affect normalized bytes
  for (const fieldPath of Object.keys(value).sort()) {
    // allow only the same dot-separated property paths as field mappings
    if (!/^[A-Za-z_$][A-Za-z0-9_$]*(?:\.[A-Za-z_$][A-Za-z0-9_$]*)*$/u.test(fieldPath)) {
      throw new Error(`${label} has invalid field path ${JSON.stringify(fieldPath)}`);
    }

    const projection = value[fieldPath];

    // require the exact projection grammar
    if (!isPlainObject(projection)) {
      throw new Error(`${label}.${fieldPath} must be a plain object`);
    }
    assertExactKeys(projection, ["kind", "root"], `${label}.${fieldPath}`);

    // accept only the reviewed source-only projection grammars
    if (
      projection.kind !== "const-catalog-array" &&
      projection.kind !== "return-enum" &&
      projection.kind !== "indexed-literals" &&
      projection.kind !== "exclude-literal" &&
      projection.kind !== "omit-interface-array" &&
      projection.kind !== "interface-field" &&
      projection.kind !== "indexed-interface-field-extension"
    ) {
      throw new Error(`${label}.${fieldPath}.kind is unsupported`);
    }

    // reserve nested projection paths for the closed indexed-literal grammar
    if (fieldPath.includes(".") && projection.kind !== "indexed-literals") {
      throw new Error(`dotted field projection requires indexed-literals at ${label}.${fieldPath}`);
    }

    // bind targets by their exact configured Rust root name
    if (
      typeof projection.root !== "string" ||
      projection.root === "_" ||
      !/^[A-Za-z_][A-Za-z0-9_]*$/u.test(projection.root)
    ) {
      throw new Error(`${label}.${fieldPath}.root must be a non-underscore ASCII identifier`);
    }
    fieldProjections[fieldPath] = { kind: projection.kind, root: projection.root };
  }

  return fieldProjections;
}

// bind an exact legacy payload and imported maintenance package member
function normalizeMaintenanceLoaderExtension(value, rootName, label) {
  // require one unambiguous grammar selection
  if (!isPlainObject(value)) {
    throw new Error(`${label} must be a plain object`);
  }
  assertExactKeys(value, ["legacy", "package", "bundleField"], label);

  // keep both nominated dependencies distinct and directly named
  if (
    ![value.legacy, value.package].every(
      // use the same portable identifier grammar as configured root names
      (name) => typeof name === "string" && name !== "_" &&
        name !== rootName && /^[A-Za-z_][A-Za-z0-9_]*$/u.test(name),
    ) ||
    value.legacy === value.package ||
    (value.bundleField !== "candidate" && value.bundleField !== "model")
  ) {
    throw new Error(`${label} requires distinct legacy and package roots with a candidate or model bundle field`);
  }
  return { legacy: value.legacy, package: value.package, bundleField: value.bundleField };
}

// associate one direct legacy proof with its specific loader consumer
function isMaintenanceLoaderLegacyProof(roots, consumer, owner, name) {
  return roots.some(
    // require the exact nominated legacy root for this consumer
    (root) => root.source === consumer && root.maintenanceLoaderExtension?.legacy === name,
  ) && roots.some(
    // keep the proof target an existing two-state alias selection
    (root) => root.source === owner && root.exportName === name && root.discriminator === "state",
  );
}

// normalize quoted numeric-range wire names selected for local interface fields
function normalizeWireFieldNames(value, label) {
  // require one non-empty exact path-to-identifier record
  if (!isPlainObject(value) || Object.keys(value).length === 0) {
    throw new Error(`${label} must be a non-empty plain object`);
  }

  const wireFieldNames = Object.create(null);
  const rustNames = new Set();

  // sort source paths so insertion order cannot affect canonical bytes
  for (const wirePath of Object.keys(value).sort()) {
    // admit only a nested quoted decimal-range leaf behind ordinary fields
    if (!/^[A-Za-z_$][A-Za-z0-9_$]*(?:\.[A-Za-z_$][A-Za-z0-9_$]*)*\.[0-9]+-[0-9]+$/u.test(wirePath)) {
      throw new Error(`${label} has invalid wire field path ${JSON.stringify(wirePath)}`);
    }

    const rustName = value[wirePath];

    // require one portable non-keyword Rust field identifier
    if (
      typeof rustName !== "string" ||
      rustName === "_" ||
      !/^[A-Za-z_][A-Za-z0-9_]*$/u.test(rustName) ||
      RUST_KEYWORDS.has(rustName)
    ) {
      throw new Error(`${label}.${wirePath} must be a non-keyword ASCII Rust identifier`);
    }

    // reject repeated Rust spellings instead of silently sharing a mapping
    if (rustNames.has(rustName)) {
      throw new Error(`duplicate wire field Rust name ${rustName} at ${label}`);
    }
    rustNames.add(rustName);
    wireFieldNames[wirePath] = rustName;
  }

  return wireFieldNames;
}

// report whether two exact property paths address the same storage branch
function fieldPathsOverlap(left, right) {
  return left === right || left.startsWith(`${right}.`) || right.startsWith(`${left}.`);
}

// parse one path qualified by its exact string discriminant
function parseTaggedArmFieldPath(fieldPath, allowNested = false) {
  const match = /^\[([^\[\]\r\n]*)\]\.([A-Za-z_$][A-Za-z0-9_$]*(?:\.[A-Za-z_$][A-Za-z0-9_$]*)?)$/u.exec(fieldPath);

  // reject ambiguous brackets and leave old tagged grammars direct-only
  if (match === null || (!allowNested && match[2].includes("."))) {
    return null;
  }
  return { wireValue: match[1], fieldName: match[2] };
}

// validate and normalize the public generator configuration
function normalizeConfig(config) {
  // require the documented top-level record
  if (!isPlainObject(config)) {
    throw new Error("config must be a plain object");
  }
  assertExactKeys(config, ["version", "typescriptVersion", "roots", "typeImports"], "config");

  // pin the configuration format
  if (config.version !== 1) {
    throw new Error("config version must be 1");
  }

  // pin both the requested and loaded compiler
  if (config.typescriptVersion !== TYPESCRIPT_VERSION || ts.version !== TYPESCRIPT_VERSION) {
    throw new Error(
      `TypeScript compiler version mismatch: expected ${TYPESCRIPT_VERSION}, config ${String(config.typescriptVersion)}, loaded ${ts.version}`,
    );
  }

  // require at least one explicit root
  if (!Array.isArray(config.roots) || config.roots.length === 0) {
    throw new Error("config roots must be a non-empty array");
  }

  const identities = new Set();
  const rustNames = new Set();
  const roots = [];

  // normalize roots without resolving or evaluating modules
  for (const [index, root] of config.roots.entries()) {
    const label = `config.roots[${index}]`;

    // require the documented root record
    if (!isPlainObject(root)) {
      throw new Error(`${label} must be a plain object`);
    }
    assertExactKeys(
      root,
      [
        "source",
        "exportName",
        "fields",
        "discriminator",
        "readonlyTaggedUnion",
        "base",
        "importedBase",
        "identicalImportedStringFields",
        "omitBaseFields",
        "omitInheritedBaseFields",
        "addedAfterOmit",
        "ownFieldOrderAfterOmit",
        "catalog",
        "objectUnion",
        "importedObjectUnion",
        "literalUnion",
        "recursiveJson",
        "localAlias",
        "localInterface",
        "wireFieldNames",
        "indexedLiteralSources",
        "fieldProjections",
        "defaultedListParameter",
        "pgRowNamedFields",
        "pgQueryOwners",
        "readonlyNullableMetricRecord",
        "disabledReasonExtract",
        "maintenanceLoaderExtension",
        "activeImportedConstString",
        "pick",
      ],
      label,
    );

    // constrain source identity to one repository-relative TypeScript path
    normalizeRepositoryPath(root.source, ".ts", `${label}.source`);

    // constrain root names to portable Rust type identifiers
    if (
      typeof root.exportName !== "string" ||
      root.exportName === "_" ||
      !/^[A-Za-z_][A-Za-z0-9_]*$/u.test(root.exportName)
    ) {
      throw new Error(`${label}.exportName must be a non-underscore ASCII identifier`);
    }

    // reject root names that Rust reserves in the type namespace
    if (RUST_KEYWORDS.has(root.exportName)) {
      throw new Error(`${label}.exportName is a Rust keyword`);
    }

    // prevent roots from shadowing emitted storage and helper types
    if (GENERATED_RUST_TYPE_NAMES.has(root.exportName)) {
      throw new Error(`${label}.exportName collides with a generated Rust type`);
    }

    // reject duplicate source/export identities
    const identity = `${root.source}\0${root.exportName}`;
    if (identities.has(identity)) {
      throw new Error(`duplicate root ${root.source}#${root.exportName}`);
    }
    identities.add(identity);

    // reject Rust namespace collisions across sources
    if (rustNames.has(root.exportName)) {
      throw new Error(`duplicate Rust root name ${root.exportName}`);
    }
    rustNames.add(root.exportName);

    // require a plain metadata mapping record
    if (!isPlainObject(root.fields)) {
      throw new Error(`${label}.fields must be a plain object`);
    }

    const defaultedListParameter = root.defaultedListParameter === undefined
      ? undefined
      : normalizeDefaultedListParameter(
        root.defaultedListParameter,
        `${label}.defaultedListParameter`,
      );
    const pick = root.pick === undefined ? undefined : normalizePick(root.pick, `${label}.pick`);
    const maintenanceLoaderExtension = root.maintenanceLoaderExtension === undefined
      ? undefined
      : normalizeMaintenanceLoaderExtension(
        root.maintenanceLoaderExtension, root.exportName, `${label}.maintenanceLoaderExtension`,
      );

    // keep this data-only loader grammar separate from all other root projections
    if (maintenanceLoaderExtension !== undefined) {
      assertExactKeys(root, ["source", "exportName", "fields", "maintenanceLoaderExtension"], label);
    }

    const pgQueryOwners = root.pgQueryOwners === undefined
      ? undefined
      : normalizeConfiguredFieldNames(root.pgQueryOwners, `${label}.pgQueryOwners`);

    // keep private query ownership on the explicit PostgreSQL named-field projection
    if (pgQueryOwners !== undefined && root.pgRowNamedFields !== true) {
      throw new Error(`${label}.pgQueryOwners requires pgRowNamedFields`);
    }

    // allow only an explicit named-field projection without another root grammar
    if (root.pgRowNamedFields !== undefined) {
      if (root.pgRowNamedFields !== true) {
        throw new Error(`${label}.pgRowNamedFields must be true when present`);
      }
      assertExactKeys(root, ["source", "exportName", "fields", "pgRowNamedFields", "pgQueryOwners"], label);
    }

    // bind the dedicated record root to one exact configured enum name
    const readonlyNullableMetricRecord = root.readonlyNullableMetricRecord;
    if (readonlyNullableMetricRecord !== undefined) {
      assertExactKeys(root, ["source", "exportName", "fields", "readonlyNullableMetricRecord"], label);
      // reject a missing or self-referential key and unrelated field mappings
      if (
        typeof readonlyNullableMetricRecord !== "string" ||
        readonlyNullableMetricRecord === "_" ||
        !/^[A-Za-z_][A-Za-z0-9_]*$/u.test(readonlyNullableMetricRecord) ||
        readonlyNullableMetricRecord === root.exportName ||
        Object.keys(root.fields).length !== 0
      ) {
        throw new Error(`${label}.readonlyNullableMetricRecord requires a distinct configured key root and empty fields`);
      }
    }

    // opt in only the disabled reason on a plain active-or-disabled loader union
    const disabledReasonExtract = root.disabledReasonExtract;
    if (disabledReasonExtract !== undefined) {
      assertExactKeys(root, ["source", "exportName", "fields", "discriminator", "disabledReasonExtract"], label);
      // require a distinct configured key and leave all other fields on the existing grammar
      if (
        typeof disabledReasonExtract !== "string" ||
        disabledReasonExtract === "_" ||
        !/^[A-Za-z_][A-Za-z0-9_]*$/u.test(disabledReasonExtract) ||
        disabledReasonExtract === root.exportName ||
        root.discriminator !== "state" ||
        Object.keys(root.fields).length > 0
      ) {
        throw new Error(`${label}.disabledReasonExtract requires a distinct configured enum, state discriminator, and empty fields`);
      }
    }

    const activeImportedConstString = root.activeImportedConstString;

    // limit the const proof to one direct field on a two-state loader root
    if (activeImportedConstString !== undefined) {
      assertExactKeys(root, ["source", "exportName", "fields", "discriminator", "activeImportedConstString"], label);

      // keep the selected field distinct from the original state tag
      if (
        typeof activeImportedConstString !== "string" ||
        !/^[A-Za-z_][A-Za-z0-9_]*$/u.test(activeImportedConstString) ||
        activeImportedConstString === "_" || activeImportedConstString === "state" ||
        root.discriminator !== "state" || Object.keys(root.fields).length !== 0
      ) {
        throw new Error(`${label}.activeImportedConstString requires a distinct field, state discriminator, and empty fields`);
      }
    }

    // keep the generic parameter distinct from its owning Rust type
    if (defaultedListParameter?.name === root.exportName) {
      throw new Error(`${label}.defaultedListParameter.name must differ from exportName`);
    }

    // inherit the selected fields unchanged rather than combining root rules
    if (pick !== undefined && (
      Object.keys(root.fields).length > 0 ||
      Object.keys(root).some(
        // allow only the ordinary identity and the complete Pick selection
        (key) => !["source", "exportName", "fields", "pick"].includes(key),
      )
    )) {
      throw new Error(`pick metadata cannot be combined with other root metadata at ${label}`);
    }

    const fields = Object.create(null);

    const isBaseTaggedUnion = root.base !== undefined && root.discriminator !== undefined;
    const readonlyTaggedUnion = root.readonlyTaggedUnion === true;

    // keep readonly-wrapped arms separate from ordinary and based unions
    if (
      root.readonlyTaggedUnion !== undefined &&
      (!readonlyTaggedUnion || root.discriminator === undefined || root.base !== undefined)
    ) {
      throw new Error(`${label}.readonlyTaggedUnion requires true, a discriminator, and no base`);
    }
    const useArmFieldPaths = isBaseTaggedUnion || readonlyTaggedUnion;

    // validate every explicit field mapping
    for (const [fieldPath, mapping] of Object.entries(root.fields)) {
      const isOrdinaryFieldPath =
        /^[A-Za-z_$][A-Za-z0-9_$]*(?:\.[A-Za-z_$][A-Za-z0-9_$]*)*$/u.test(fieldPath);
      // parse qualifiers only when metadata selects one renderer arm
      const taggedArmField = useArmFieldPaths
        ? parseTaggedArmFieldPath(fieldPath, readonlyTaggedUnion)
        : null;

      // keep arm qualification exclusive to the opted-in tagged grammars
      if (
        (useArmFieldPaths && taggedArmField === null) ||
        (!useArmFieldPaths && !isOrdinaryFieldPath)
      ) {
        throw new Error(`${label}.fields has invalid field path ${JSON.stringify(fieldPath)}`);
      }

      // fail closed on unknown representation rules
      if (!FIELD_MAPPING_KINDS.has(mapping)) {
        throw new Error(`${label}.fields.${fieldPath} has unsupported mapping ${JSON.stringify(mapping)}`);
      }

      // limit loader payload mappings to the original literal and readonly storage forms
      if (maintenanceLoaderExtension !== undefined &&
        mapping !== READONLY_INLINE_OBJECT_MAPPING && mapping !== UNSIGNED_SAFE_INTEGER_LITERAL_MAPPING) {
        throw new Error(`${label}.fields.${fieldPath} has unsupported maintenance loader mapping`);
      }

      // admit only the exact local-const proof on qualified tagged-arm fields
      if (isBaseTaggedUnion && mapping !== CONST_STRING_MAPPING) {
        throw new Error(`${label}.fields.${fieldPath} must use const-string`);
      }

      // admit only scalar mappings and one shallow wrapper at an arm field
      if (readonlyTaggedUnion && mapping !== READONLY_INLINE_OBJECT_MAPPING && !READONLY_TAGGED_FIELD_MAPPINGS.has(mapping)) {
        throw new Error(`${label}.fields.${fieldPath} must use utc-iso, safe-unsigned-integer, unsigned-safe-integer-enum, or readonly-inline-object`);
      }

      // require one explicit shallow wrapper before a nested arm mapping
      if (readonlyTaggedUnion && taggedArmField.fieldName.includes(".")) {
        // prevent a nested wrapper from opening another mapped level
        if (mapping === READONLY_INLINE_OBJECT_MAPPING) {
          throw new Error(`${label}.fields.${fieldPath} cannot wrap a nested tagged field`);
        }

        const parentName = taggedArmField.fieldName.split(".")[0];
        const parentPath = `[${taggedArmField.wireValue}].${parentName}`;

        // bind to the same arm's direct wrapper, even when configured later
        if (!Object.hasOwn(root.fields, parentPath) || root.fields[parentPath] !== READONLY_INLINE_OBJECT_MAPPING) {
          throw new Error(`${label}.fields.${fieldPath} requires one direct readonly-inline-object parent`);
        }
      }
      fields[fieldPath] = mapping;
    }

    // constrain an explicit tagged-union field to one portable property name
    if (
      root.discriminator !== undefined &&
      (typeof root.discriminator !== "string" ||
        !/^[A-Za-z_][A-Za-z0-9_]*$/u.test(root.discriminator))
    ) {
      throw new Error(`${label}.discriminator must be an ASCII property identifier`);
    }

    // constrain one explicitly flattened interface base
    if (
      root.base !== undefined &&
      (typeof root.base !== "string" ||
        root.base === "_" ||
        !/^[A-Za-z_][A-Za-z0-9_]*$/u.test(root.base))
    ) {
      throw new Error(`${label}.base must be a non-underscore ASCII identifier`);
    }

    const objectUnion = root.objectUnion === undefined
      ? undefined
      : normalizeObjectUnion(root.objectUnion, `${label}.objectUnion`);
    const omitBaseFields = root.omitBaseFields === undefined
      ? undefined
      : normalizeConfiguredFieldNames(root.omitBaseFields, `${label}.omitBaseFields`);
    const addedAfterOmit = root.addedAfterOmit === undefined
      ? undefined
      : normalizeConfiguredFieldNames(root.addedAfterOmit, `${label}.addedAfterOmit`);
    const ownFieldOrderAfterOmit = root.ownFieldOrderAfterOmit === undefined
      ? undefined
      : normalizeConfiguredFieldNames(root.ownFieldOrderAfterOmit, `${label}.ownFieldOrderAfterOmit`);
    const omitInheritedBaseFields = root.omitInheritedBaseFields === undefined
      ? undefined
      : normalizeConfiguredFieldNames(
        root.omitInheritedBaseFields,
        `${label}.omitInheritedBaseFields`,
      );
    const literalUnion = root.literalUnion === undefined
      ? undefined
      : normalizeLiteralUnion(root.literalUnion, `${label}.literalUnion`);
    const indexedLiteralSources = root.indexedLiteralSources === undefined
      ? undefined
      : normalizeIndexedLiteralSources(
        root.indexedLiteralSources,
        `${label}.indexedLiteralSources`,
      );
    const fieldProjections = root.fieldProjections === undefined
      ? undefined
      : normalizeFieldProjections(root.fieldProjections, `${label}.fieldProjections`);
    const localAlias = root.localAlias;
    const localInterface = root.localInterface;
    const wireFieldNames = root.wireFieldNames === undefined
      ? undefined
      : normalizeWireFieldNames(root.wireFieldNames, `${label}.wireFieldNames`);
    const importedObjectUnion = root.importedObjectUnion;
    const importedBase = root.importedBase;
    const identicalImportedStringFields = root.identicalImportedStringFields === undefined
      ? undefined
      : normalizeConfiguredFieldNames(
        root.identicalImportedStringFields, `${label}.identicalImportedStringFields`,
      );
    const recursiveJson = root.recursiveJson;
    let catalog;

    // make the private-root opt-in exact rather than truthy
    if (localAlias !== undefined && localAlias !== true) {
      throw new Error(`${label}.localAlias must be true when present`);
    }

    // make the private-interface opt-in exact rather than truthy
    if (localInterface !== undefined && localInterface !== true) {
      throw new Error(`${label}.localInterface must be true when present`);
    }

    // prevent two private declaration grammars from claiming one root
    if (localAlias !== undefined && localInterface !== undefined) {
      throw new Error(`${label} cannot combine localAlias and localInterface`);
    }

    // make imported public union support an exact opt-in
    if (importedObjectUnion !== undefined && importedObjectUnion !== true) {
      throw new Error(`${label}.importedObjectUnion must be true when present`);
    }

    // make imported interface inheritance an exact opt-in
    if (importedBase !== undefined && importedBase !== true) {
      throw new Error(`${label}.importedBase must be true when present`);
    }

    // restrict repeated storage to explicitly named unmapped imported-base strings
    if (identicalImportedStringFields !== undefined && (
      importedBase !== true || identicalImportedStringFields.some(
        // keep the inherited string's storage unchanged by field metadata
        (name) => Object.hasOwn(fields, name) || Object.hasOwn(fieldProjections ?? {}, name),
      )
    )) {
      throw new Error(`${label}.identicalImportedStringFields requires an imported base and unmapped fields`);
    }

    // bind omission metadata to one ordinary configured interface base
    if (
      omitBaseFields !== undefined &&
      (root.base === undefined || root.discriminator !== undefined)
    ) {
      throw new Error(`${label}.omitBaseFields requires an ordinary base`);
    }

    // allow only explicitly named new fields after the configured replacements
    if (addedAfterOmit !== undefined && (
      omitBaseFields === undefined || addedAfterOmit.some(
        // do not count a replacement twice through the added-field allowlist
        (name) => omitBaseFields.includes(name),
      )
    )) {
      throw new Error(`${label}.addedAfterOmit requires distinct names not in omitBaseFields`);
    }

    // bind interleaved additions and replacements to the complete own field order
    if (ownFieldOrderAfterOmit !== undefined && (
      addedAfterOmit !== undefined || omitBaseFields === undefined ||
      ownFieldOrderAfterOmit.length <= omitBaseFields.length || omitBaseFields.some(
        // retain every configured replacement exactly once
        (name) => !ownFieldOrderAfterOmit.includes(name),
      )
    )) {
      throw new Error(`${label}.ownFieldOrderAfterOmit requires omitBaseFields, all replacements, additions, and no addedAfterOmit`);
    }

    // bind inherited omissions to an ordered subset of one Omit selection
    if (omitInheritedBaseFields !== undefined) {
      if (omitBaseFields === undefined) {
        throw new Error(`${label}.omitInheritedBaseFields requires omitBaseFields`);
      }
      let priorIndex = -1;

      // preserve configured omission order without admitting unrelated names
      for (const fieldName of omitInheritedBaseFields) {
        const fieldIndex = omitBaseFields.indexOf(fieldName);

        // require a strict ordered subset of the complete omitted field list
        if (fieldIndex <= priorIndex) {
          throw new Error(`${label}.omitInheritedBaseFields must be an ordered subset of omitBaseFields`);
        }
        priorIndex = fieldIndex;
      }
    }

    // bind the recursive JSON root to one exact local primitive alias
    if (
      recursiveJson !== undefined &&
      (typeof recursiveJson !== "string" ||
        recursiveJson === "_" ||
        recursiveJson === root.exportName ||
        !/^[A-Za-z_][A-Za-z0-9_]*$/u.test(recursiveJson))
    ) {
      throw new Error(`${label}.recursiveJson must name a distinct ASCII identifier`);
    }

    // normalize only the explicit catalog extraction grammars
    if (root.catalog !== undefined) {
      // require one closed catalog metadata record
      if (!isPlainObject(root.catalog)) {
        throw new Error(`${label}.catalog must be a plain object`);
      }

      // reject catalog roots that also request unrelated root grammars
      if (root.base !== undefined || root.discriminator !== undefined) {
        throw new Error(`catalog metadata cannot be combined with base or discriminator at ${label}`);
      }

      // admit a syntactically constrained satisfies tuple catalog
      if (root.catalog.kind === "satisfies-string-tuple") {
        assertExactKeys(root.catalog, ["kind"], `${label}.catalog`);
        catalog = { kind: "satisfies-string-tuple" };
      // keep the direct type query distinct from the ordinary parenthesized form
      } else if (root.catalog.kind === "direct-const-string-tuple") {
        assertExactKeys(root.catalog, ["kind"], `${label}.catalog`);
        catalog = { kind: "direct-const-string-tuple" };
      } else if (root.catalog.kind === "unsigned-safe-integer-tuple") {
        assertExactKeys(root.catalog, ["kind"], `${label}.catalog`);
        catalog = { kind: "unsigned-safe-integer-tuple" };
      } else if (root.catalog.kind === "extend-local-string-enum") {
        assertExactKeys(root.catalog, ["kind", "base"], `${label}.catalog`);

        // constrain the base to one direct configured root name
        if (
          typeof root.catalog.base !== "string" ||
          root.catalog.base === "_" ||
          !/^[A-Za-z_][A-Za-z0-9_]*$/u.test(root.catalog.base)
        ) {
          throw new Error(`${label}.catalog.base must be a non-underscore ASCII identifier`);
        }
        catalog = { kind: "extend-local-string-enum", base: root.catalog.base };
      } else if (root.catalog.kind === "string-property") {
        assertExactKeys(root.catalog, ["kind", "key"], `${label}.catalog`);

        // constrain the selected property to a portable source identifier
        if (
          typeof root.catalog.key !== "string" ||
          !/^[A-Za-z_][A-Za-z0-9_]*$/u.test(root.catalog.key)
        ) {
          throw new Error(`${label}.catalog.key must be an ASCII property identifier`);
        }
        catalog = { kind: "string-property", key: root.catalog.key };
      } else {
        throw new Error(
          `${label}.catalog.kind must be satisfies-string-tuple, unsigned-safe-integer-tuple, extend-local-string-enum, string-property, or direct-const-string-tuple`,
        );
      }
    }

    // isolate direct imported inheritance from every alternate root grammar
    if (
      importedBase !== undefined &&
      (root.base === undefined ||
        omitBaseFields !== undefined ||
        omitInheritedBaseFields !== undefined ||
        root.discriminator !== undefined ||
        readonlyTaggedUnion ||
        catalog !== undefined ||
        objectUnion !== undefined ||
        importedObjectUnion !== undefined ||
        literalUnion !== undefined ||
        recursiveJson !== undefined ||
        localAlias !== undefined ||
        localInterface !== undefined ||
        wireFieldNames !== undefined ||
        indexedLiteralSources !== undefined)
    ) {
      throw new Error(`${label}.importedBase requires one ordinary imported interface base`);
    }

    // isolate the sole reviewed generic interface from every alternate grammar
    if (
      defaultedListParameter !== undefined &&
      (root.base !== undefined ||
        importedBase !== undefined ||
        omitBaseFields !== undefined ||
        omitInheritedBaseFields !== undefined ||
        root.discriminator !== undefined ||
        readonlyTaggedUnion ||
        catalog !== undefined ||
        objectUnion !== undefined ||
        importedObjectUnion !== undefined ||
        literalUnion !== undefined ||
        recursiveJson !== undefined ||
        localAlias !== undefined ||
        localInterface !== undefined ||
        wireFieldNames !== undefined ||
        indexedLiteralSources !== undefined ||
        fieldProjections !== undefined ||
        pick !== undefined)
    ) {
      throw new Error(`${label}.defaultedListParameter cannot combine with other root metadata`);
    }

    // keep the named object-union grammar isolated from every other root grammar
    if (
      objectUnion !== undefined &&
      (root.base !== undefined || root.catalog !== undefined || root.discriminator !== undefined)
    ) {
      throw new Error(
        `object union metadata cannot be combined with base, catalog, or discriminator at ${label}`,
      );
    }

    // reject ambiguous field paths across independently rendered payload roots
    if (objectUnion !== undefined && Object.keys(fields).length > 0) {
      throw new Error(`object union field mappings unsupported at ${label}`);
    }

    // object unions have arms rather than direct projected fields
    if (objectUnion !== undefined && fieldProjections !== undefined) {
      throw new Error(`object union field projections unsupported at ${label}`);
    }

    // private roots are admitted only for the reviewed named object-union grammar
    if (localAlias !== undefined && objectUnion === undefined) {
      throw new Error(`${label}.localAlias requires objectUnion metadata`);
    }

    // isolate private interfaces to direct field rendering and ordinary descendants
    if (
      localInterface !== undefined &&
      (root.base !== undefined ||
        root.omitBaseFields !== undefined ||
        root.catalog !== undefined ||
        root.discriminator !== undefined ||
        root.objectUnion !== undefined ||
        root.importedObjectUnion !== undefined ||
        root.literalUnion !== undefined ||
        root.recursiveJson !== undefined ||
        indexedLiteralSources !== undefined ||
        fieldProjections !== undefined)
    ) {
      throw new Error(`${label}.localInterface cannot be combined with other root metadata`);
    }

    // bind quoted wire-name selection to the sole private-interface grammar
    if (wireFieldNames !== undefined && localInterface === undefined) {
      throw new Error(`${label}.wireFieldNames requires localInterface`);
    }

    // expose imported arms only on public named object unions
    if (
      importedObjectUnion !== undefined &&
      (objectUnion === undefined || localAlias !== undefined)
    ) {
      throw new Error(`${label}.importedObjectUnion requires a public objectUnion`);
    }

    // keep composed literal unions isolated from every earlier root grammar
    if (
      literalUnion !== undefined &&
      (root.base !== undefined ||
        root.catalog !== undefined ||
        root.discriminator !== undefined ||
        root.objectUnion !== undefined)
    ) {
      throw new Error(
        `literal union metadata cannot be combined with base, catalog, discriminator, or objectUnion at ${label}`,
      );
    }

    // literal unions do not render object fields
    if (literalUnion !== undefined && Object.keys(fields).length > 0) {
      throw new Error(`literal union field mappings unsupported at ${label}`);
    }

    // literal unions do not expose direct object properties
    if (literalUnion !== undefined && fieldProjections !== undefined) {
      throw new Error(`literal union field projections unsupported at ${label}`);
    }

    // keep the recursive JSON grammar isolated from every ordinary root grammar
    if (
      recursiveJson !== undefined &&
      (root.base !== undefined ||
        root.catalog !== undefined ||
        root.discriminator !== undefined ||
        root.objectUnion !== undefined ||
        root.literalUnion !== undefined ||
        root.localAlias !== undefined ||
        root.localInterface !== undefined ||
        root.wireFieldNames !== undefined ||
        indexedLiteralSources !== undefined ||
        fieldProjections !== undefined ||
        Object.keys(fields).length > 0)
    ) {
      throw new Error(`recursiveJson metadata cannot be combined with other root metadata at ${label}`);
    }

    // projections address one direct interface or object-alias field only
    if (
      fieldProjections !== undefined &&
      (catalog !== undefined || root.discriminator !== undefined)
    ) {
      throw new Error(`field projections require a direct object root at ${label}`);
    }

    // one storage branch cannot use both representation mechanisms
    for (const fieldPath of Object.keys(fieldProjections ?? {})) {
      const projection = fieldProjections[fieldPath];

      // reject exact and ancestor/descendant metadata overlap
      if (Object.keys(fields).some((mappedPath) =>
        fieldPathsOverlap(mappedPath, fieldPath) &&
        !(
          projection.kind === "indexed-interface-field-extension" &&
          mappedPath.startsWith(`${fieldPath}.`)
        )
      )) {
        throw new Error(`field mapping conflicts with field projection at ${label}.${fieldPath}`);
      }

      // indexed projections must name one alias explicitly selected by this root
      if (
        projection.kind === "indexed-literals" &&
        !Object.hasOwn(indexedLiteralSources ?? {}, projection.root)
      ) {
        throw new Error(
          `${label}.fieldProjections.${fieldPath}.root must name an indexed literal source`,
        );
      }
    }

    // keep alias proof configuration exclusive to the indexed projection grammar
    if (
      indexedLiteralSources !== undefined &&
      !Object.values(fieldProjections ?? {}).some((projection) =>
        projection.kind === "indexed-literals"
      )
    ) {
      throw new Error(`${label}.indexedLiteralSources requires an indexed-literals projection`);
    }

    // omit absent optional configuration to preserve earlier canonical bytes
    roots.push({
      source: root.source,
      exportName: root.exportName,
      fields,
      ...(root.discriminator === undefined ? {} : { discriminator: root.discriminator }),
      ...(readonlyTaggedUnion ? { readonlyTaggedUnion: true } : {}),
      ...(root.base === undefined ? {} : { base: root.base }),
      ...(importedBase === undefined ? {} : { importedBase: true }),
      ...(identicalImportedStringFields === undefined ? {} : { identicalImportedStringFields }),
      ...(omitBaseFields === undefined ? {} : { omitBaseFields }),
      ...(omitInheritedBaseFields === undefined ? {} : { omitInheritedBaseFields }),
      ...(addedAfterOmit === undefined ? {} : { addedAfterOmit }),
      ...(ownFieldOrderAfterOmit === undefined ? {} : { ownFieldOrderAfterOmit }),
      ...(catalog === undefined ? {} : { catalog }),
      ...(objectUnion === undefined ? {} : { objectUnion }),
      ...(importedObjectUnion === undefined ? {} : { importedObjectUnion: true }),
      ...(literalUnion === undefined ? {} : { literalUnion }),
      ...(recursiveJson === undefined ? {} : { recursiveJson }),
      ...(localAlias === undefined ? {} : { localAlias: true }),
      ...(localInterface === undefined ? {} : { localInterface: true }),
      ...(wireFieldNames === undefined ? {} : { wireFieldNames }),
      ...(indexedLiteralSources === undefined ? {} : { indexedLiteralSources }),
      ...(fieldProjections === undefined ? {} : { fieldProjections }),
      ...(defaultedListParameter === undefined ? {} : { defaultedListParameter }),
      ...(root.pgRowNamedFields === undefined ? {} : { pgRowNamedFields: true }),
      ...(pgQueryOwners === undefined ? {} : { pgQueryOwners }),
      ...(readonlyNullableMetricRecord === undefined ? {} : { readonlyNullableMetricRecord }),
      ...(disabledReasonExtract === undefined ? {} : { disabledReasonExtract }),
      ...(maintenanceLoaderExtension === undefined ? {} : { maintenanceLoaderExtension }),
      ...(activeImportedConstString === undefined ? {} : { activeImportedConstString }),
      ...(pick === undefined ? {} : { pick }),
    });
  }

  const normalized = {
    version: 1,
    typescriptVersion: TYPESCRIPT_VERSION,
    roots,
  };

  // omit absent type-import configuration to preserve earlier canonical bytes
  if (config.typeImports === undefined) {
    return normalized;
  }

  // require one or more explicit, closed import graphs
  if (!Array.isArray(config.typeImports) || config.typeImports.length === 0) {
    throw new Error("config.typeImports must be a non-empty array when present");
  }

  const rootSources = new Set(roots.flatMap((root) => [
    root.source,
    ...Object.values(root.indexedLiteralSources ?? {}),
  ]));
  const rootIdentities = new Set(
    roots
      .filter((root) => root.localAlias !== true && root.localInterface !== true && root.pgQueryOwners === undefined)
      .map((root) => configuredRootIdentity(root)),
  );
  const indexedLiteralIdentities = new Set(
    roots.flatMap((root) => Object.entries(root.indexedLiteralSources ?? {})
      .map(([name, source]) => `${source}\0${name}`)),
  );
  const importIdentities = new Set();
  const typeImports = [];

  // normalize each graph without resolving package or module code
  for (const [index, typeImport] of config.typeImports.entries()) {
    const label = `config.typeImports[${index}]`;

    // require one of the two exact documented graph records
    if (!isPlainObject(typeImport)) {
      throw new Error(`${label} must be a plain object`);
    }
    const isSibling = typeof typeImport.specifier === "string" &&
      typeImport.specifier.startsWith(".");
    let consumer;
    let entry;
    let packageJson;
    let compiledSource;

    // constrain sibling imports to one direct normalized JavaScript specifier
    if (isSibling) {
      assertExactKeys(
        typeImport,
        ["consumer", "specifier", "entry", "exports", "proofExports", "tupleIdentityWrapper", "literalStringValue"],
        label,
      );
      consumer = normalizeRepositoryPath(typeImport.consumer, ".ts", `${label}.consumer`);
      entry = normalizeRepositoryPath(typeImport.entry, ".ts", `${label}.entry`);
      const siblingStem = typeImport.specifier.slice(2, -3);

      // reject empty and parent-like stems despite their lack of a slash
      if (
        !/^\.\/[A-Za-z0-9._-]+\.js$/u.test(typeImport.specifier) ||
        siblingStem === "." ||
        siblingStem === ".."
      ) {
        throw new Error(`${label}.specifier must be a direct sibling .js module`);
      }
      const expectedEntry = posix.join(
        posix.dirname(consumer),
        `${siblingStem}.ts`,
      );

      // pin the configured edge to its exact normalized sibling source
      if (entry !== expectedEntry) {
        throw new Error(`${label}.entry must match the direct sibling specifier`);
      }
    } else {
      assertExactKeys(
        typeImport,
        [
          "consumer",
          "specifier",
          "packageJson",
          "entry",
          "leaves",
          "compiledSource",
          "exports",
          "proofExports",
        ],
        label,
      );
      consumer = normalizeRepositoryPath(typeImport.consumer, ".ts", `${label}.consumer`);
      packageJson = normalizeRepositoryPath(
        typeImport.packageJson,
        "package.json",
        `${label}.packageJson`,
      );
      entry = normalizeRepositoryPath(typeImport.entry, ".ts", `${label}.entry`);

      // constrain the supported package surface to one portable scoped name
      if (
        typeof typeImport.specifier !== "string" ||
        !/^@[a-z0-9][a-z0-9._-]*\/[a-z0-9][a-z0-9._-]*$/u.test(typeImport.specifier)
      ) {
        throw new Error(`${label}.specifier must be a bare scoped package name`);
      }

      // normalize the optional source-to-compiled package proof metadata
      if (typeImport.compiledSource !== undefined) {
        if (!isPlainObject(typeImport.compiledSource)) {
          throw new Error(`${label}.compiledSource must be a plain object`);
        }
        assertExactKeys(typeImport.compiledSource, ["tsconfig", "base"], `${label}.compiledSource`);
        compiledSource = {
          tsconfig: normalizeRepositoryPath(
            typeImport.compiledSource.tsconfig,
            ".json",
            `${label}.compiledSource.tsconfig`,
          ),
          base: normalizeRepositoryPath(
            typeImport.compiledSource.base,
            ".json",
            `${label}.compiledSource.base`,
          ),
        };
        const packageDirectory = posix.dirname(packageJson);

        // keep the package tsconfig at the exact configured package root
        if (compiledSource.tsconfig !== posix.join(packageDirectory, "tsconfig.json")) {
          throw new Error(`${label}.compiledSource.tsconfig must belong to the configured package`);
        }
      }
    }

    // bind imports only from a source that also owns configured roots
    if (!rootSources.has(consumer)) {
      throw new Error(`${label}.consumer must be a configured root source`);
    }

    const importIdentity = `${consumer}\0${typeImport.specifier}`;

    // prevent ambiguous graphs for one consumer package
    if (importIdentities.has(importIdentity)) {
      throw new Error(`duplicate type import ${consumer}#${typeImport.specifier}`);
    }
    importIdentities.add(importIdentity);

    let leaves;

    // retain the existing closed package metadata and leaf inventory
    if (!isSibling) {
      // require an explicit non-empty, duplicate-free leaf inventory
      if (!Array.isArray(typeImport.leaves) || typeImport.leaves.length === 0) {
        throw new Error(`${label}.leaves must be a non-empty array`);
      }
      leaves = typeImport.leaves.map((leaf, leafIndex) =>
        normalizeRepositoryPath(leaf, ".ts", `${label}.leaves[${leafIndex}]`)
      ).sort();
      if (new Set(leaves).size !== leaves.length || leaves.includes(entry)) {
        throw new Error(`${label}.leaves must be distinct and exclude the entry`);
      }
    }

    // require a closed ordinary export record even when only proofs are selected
    if (!isPlainObject(typeImport.exports)) {
      throw new Error(`${label}.exports must be a plain object`);
    }
    const exports = Object.create(null);

    // sort export names so object insertion order cannot change generated bytes
    for (const exportName of Object.keys(typeImport.exports).sort()) {
      // reject a name assigned to both generation and proof roles
      if (Object.hasOwn(typeImport.proofExports ?? {}, exportName)) {
        throw new Error(`type import name cannot be both exported and proof-only at ${label}.${exportName}`);
      }
      const declarationOwner = normalizeRepositoryPath(
        typeImport.exports[exportName],
        ".ts",
        `${label}.exports.${exportName}`,
      );

      // constrain every name to one configured declaration owner
      if (
        exportName === "_" ||
        !/^[A-Za-z_][A-Za-z0-9_]*$/u.test(exportName) ||
        (isSibling ? declarationOwner !== entry : !leaves.includes(declarationOwner)) ||
        !rootIdentities.has(`${declarationOwner}\0${exportName}`)
      ) {
        throw new Error(
          `${label}.exports.${exportName} must name a configured ${isSibling ? "entry" : "leaf"} root`,
        );
      }
      exports[exportName] = declarationOwner;
    }

    let proofExports;

    // normalize proof-only names without admitting them as generated roots
    if (typeImport.proofExports !== undefined) {
      if (
        !isPlainObject(typeImport.proofExports) ||
        Object.keys(typeImport.proofExports).length === 0
      ) {
        throw new Error(`${label}.proofExports must be a non-empty plain object when present`);
      }
      proofExports = Object.create(null);

      // sort proof names so insertion order cannot affect canonical bytes
      for (const proofName of Object.keys(typeImport.proofExports).sort()) {
        const declarationOwner = normalizeRepositoryPath(
          typeImport.proofExports[proofName],
          ".ts",
          `${label}.proofExports.${proofName}`,
        );

        // bind proofs to one selected indexed declaration or direct legacy owner
        if (
          proofName === "_" ||
          !/^[A-Za-z_][A-Za-z0-9_]*$/u.test(proofName) ||
          (isSibling ? declarationOwner !== entry : !leaves.includes(declarationOwner)) ||
          (!indexedLiteralIdentities.has(`${declarationOwner}\0${proofName}`) &&
            !(isSibling && isMaintenanceLoaderLegacyProof(roots, consumer, declarationOwner, proofName)))
        ) {
          throw new Error(`${label}.proofExports.${proofName} must name a configured proof entry`);
        }

        // keep ordinary generated imports disjoint from proof-only identities
        if (Object.hasOwn(exports, proofName)) {
          throw new Error(`type import name cannot be both exported and proof-only at ${label}.${proofName}`);
        }
        proofExports[proofName] = declarationOwner;
      }
    }

    const tupleIdentityWrapper = typeImport.tupleIdentityWrapper;
    const literalStringValue = typeImport.literalStringValue;

    // keep the wrapper proof on a dedicated direct sibling edge
    if (
      tupleIdentityWrapper !== undefined &&
      (!isSibling || Object.keys(exports).length !== 0 || proofExports !== undefined ||
        typeof tupleIdentityWrapper !== "string" || tupleIdentityWrapper === "_" ||
        !/^[A-Za-z_][A-Za-z0-9_]*$/u.test(tupleIdentityWrapper) || literalStringValue !== undefined)
    ) {
      throw new Error(`${label}.tupleIdentityWrapper requires one named identity wrapper on a dedicated sibling edge`);
    }

    // keep the exact literal value proof on its own direct sibling edge
    if (
      literalStringValue !== undefined &&
      (!isSibling || Object.keys(exports).length !== 0 || proofExports !== undefined ||
        typeof literalStringValue !== "string" || literalStringValue === "_" ||
        !/^[A-Za-z_][A-Za-z0-9_]*$/u.test(literalStringValue))
    ) {
      throw new Error(`${label}.literalStringValue requires one named const on a dedicated sibling edge`);
    }

    // require one ordinary or explicitly selected proof name per edge
    if (Object.keys(exports).length === 0 && proofExports === undefined &&
      tupleIdentityWrapper === undefined && literalStringValue === undefined) {
      throw new Error(`${label}.exports must be non-empty unless proofExports is present`);
    }

    // omit package-only keys from direct sibling rows
    typeImports.push(
      isSibling
        ? {
          consumer,
          specifier: typeImport.specifier,
          entry,
          exports,
          ...(proofExports === undefined ? {} : { proofExports }),
          ...(tupleIdentityWrapper === undefined ? {} : { tupleIdentityWrapper }),
          ...(literalStringValue === undefined ? {} : { literalStringValue }),
        }
        : {
          consumer,
          specifier: typeImport.specifier,
          packageJson,
          entry,
          leaves,
          ...(compiledSource === undefined ? {} : { compiledSource }),
          exports,
          ...(proofExports === undefined ? {} : { proofExports }),
        },
    );
  }

  return { ...normalized, typeImports };
}

// serialize records with recursively sorted keys for stable digests
function canonicalJson(value, label = "value") {
  // retain JSON primitives exactly
  if (value === null || typeof value === "string" || typeof value === "boolean") {
    return JSON.stringify(value);
  }

  // retain only finite JSON numbers
  if (typeof value === "number") {
    // reject values JSON would silently coerce
    if (!Number.isFinite(value)) {
      throw new Error(`${label} contains a non-finite number`);
    }
    return JSON.stringify(value);
  }

  // preserve array order while normalizing members
  if (Array.isArray(value)) {
    return `[${value.map((entry, index) => canonicalJson(entry, `${label}[${index}]`)).join(",")}]`;
  }

  // sort plain record keys recursively
  if (isPlainObject(value)) {
    const keys = Object.keys(value).sort();
    return `{${keys.map((key) => `${JSON.stringify(key)}:${canonicalJson(value[key], `${label}.${key}`)}`).join(",")}}`;
  }

  throw new Error(`${label} is not canonical JSON data`);
}

// hash exact UTF-8 text bytes
function sha256(text) {
  return createHash("sha256").update(text, "utf8").digest("hex");
}

// escape exact wire strings as ordinary Rust string literals
function rustString(value) {
  let output = '"';

  // escape each Unicode scalar without JavaScript-only escapes
  for (const character of value) {
    // retain common escapes in readable form
    if (character === '"') {
      output += '\\"';
    } else if (character === "\\") {
      output += "\\\\";
    } else if (character === "\n") {
      output += "\\n";
    } else if (character === "\r") {
      output += "\\r";
    } else if (character === "\t") {
      output += "\\t";
    } else {
      const codePoint = character.codePointAt(0);

      // reject malformed UTF-16 rather than emitting replacement bytes
      if (codePoint >= 0xd800 && codePoint <= 0xdfff) {
        throw new Error("wire string contains an isolated surrogate code point");
      }

      // escape controls and line separators that would change generated layout
      if (codePoint < 0x20 || codePoint === 0x7f || codePoint === 0x2028 || codePoint === 0x2029) {
        output += `\\u{${codePoint.toString(16)}}`;
      } else {
        output += character;
      }
    }
  }

  return `${output}"`;
}

// identify one explicit named export without default-only semantics
function isExported(node) {
  const hasExport = node.modifiers?.some(
    (modifier) => modifier.kind === ts.SyntaxKind.ExportKeyword,
  ) === true;
  const hasDefault = node.modifiers?.some(
    (modifier) => modifier.kind === ts.SyntaxKind.DefaultKeyword,
  ) === true;
  return hasExport && !hasDefault;
}

// identify one explicit readonly modifier
function isReadonly(node) {
  return node.modifiers?.some((modifier) => modifier.kind === ts.SyntaxKind.ReadonlyKeyword) === true;
}

// produce a stable parser diagnostic
function parseSource(sourcePath, sourceText) {
  const sourceFile = ts.createSourceFile(
    `/${sourcePath}`,
    sourceText,
    ts.ScriptTarget.Latest,
    true,
    ts.ScriptKind.TS,
  );

  // reject malformed source before selecting declarations
  if (sourceFile.parseDiagnostics.length > 0) {
    const diagnostic = sourceFile.parseDiagnostics[0];
    const message = ts.flattenDiagnosticMessageText(diagnostic.messageText, " ");
    throw new Error(`TypeScript parse error in ${sourcePath}: TS${diagnostic.code} ${message}`);
  }

  return sourceFile;
}

// read one uniquely exported string asserted as const without evaluating source code
function readExportedConstAssertionString(sourceFile, exportName) {
  const matches = [];

  // find every top-level declaration with the retained export name
  for (const statement of sourceFile.statements) {
    if (!ts.isVariableStatement(statement)) {
      continue;
    }

    // inspect every binding so duplicate or hidden declarations fail closed
    for (const declaration of statement.declarationList.declarations) {
      if (ts.isIdentifier(declaration.name) && declaration.name.text === exportName) {
        matches.push({ declaration, statement });
      }
    }
  }

  // require one direct exported const binding with no annotation or definite assignment
  if (
    matches.length !== 1 ||
    !isExported(matches[0].statement) ||
    matches[0].statement.declarationList.declarations.length !== 1 ||
    (matches[0].statement.declarationList.flags & ts.NodeFlags.Const) === 0 ||
    matches[0].declaration.type !== undefined ||
    matches[0].declaration.exclamationToken !== undefined
  ) {
    throw new Error(`retained rain hurdle wind export unsupported: ${exportName}`);
  }

  const initializer = matches[0].declaration.initializer;

  // admit only the authoritative quoted string under an exact const assertion
  if (
    initializer === undefined ||
    !ts.isAsExpression(initializer) ||
    !ts.isStringLiteral(initializer.expression) ||
    !ts.isTypeReferenceNode(initializer.type) ||
    !ts.isIdentifier(initializer.type.typeName) ||
    initializer.type.typeName.text !== "const" ||
    initializer.type.typeArguments !== undefined
  ) {
    throw new Error(`retained rain hurdle wind export unsupported: ${exportName}`);
  }

  return initializer.expression.text;
}

// require one plain record with no missing or extension keys
function retainedRainHurdleWindRecord(value, keys, label) {
  // reject nonrecords before comparing their closed key set
  if (!isPlainObject(value) ||
      Object.keys(value).sort().join("\0") !== [...keys].sort().join("\0")) {
    throw new Error(`retained rain hurdle wind ${label} schema mismatch`);
  }
  return value;
}

// require one lowercase sha-256 identity
function retainedRainHurdleWindSha256(value, label) {
  // reject noncanonical digest values used by the frozen source
  if (typeof value !== "string" || !/^[0-9a-f]{64}$/.test(value)) {
    throw new Error(`retained rain hurdle wind ${label} SHA-256 mismatch`);
  }
  return value;
}

// emit one finite javascript number through its exact binary64 bits
function renderRetainedRainHurdleWindF64(value, label) {
  // reject values that cannot participate in the Rust numerical model
  if (typeof value !== "number" || !Number.isFinite(value)) {
    throw new Error(`retained rain hurdle wind ${label} is not finite`);
  }
  const bytes = new ArrayBuffer(8);
  const view = new DataView(bytes);
  view.setFloat64(0, value, false);
  const bits = view.getBigUint64(0, false).toString(16).padStart(16, "0");
  return `f64::from_bits(0x${bits})`;
}

// validate one tree against the exact Rust inference topology invariants
function validateRetainedRainHurdleWindTree(tree, label) {
  // require five aligned nonempty numerical vectors
  if (!Array.isArray(tree) || tree.length !== 5 ||
      tree.some((vector) => !Array.isArray(vector)) || tree[0].length === 0 ||
      tree.some((vector) => vector.length !== tree[0].length)) {
    throw new Error(`retained rain hurdle wind ${label} tree array mismatch`);
  }

  // validate every split and terminal node before emitting static tables
  for (let node = 0; node < tree[0].length; node += 1) {
    const feature = tree[0][node];
    const value = tree[1][node];
    const left = tree[2][node];
    const right = tree[3][node];
    const missingLeft = tree[4][node];
    const terminal = left === -1 && right === -1;
    const branch = Number.isInteger(left) && Number.isInteger(right) &&
      left > node && right > node && left < tree[0].length && right < tree[0].length;

    // match the finite feature, topology, and missing-direction checks used by Rust
    if (!Number.isInteger(feature) || feature < 0 || feature >= 107 ||
        typeof value !== "number" || !Number.isFinite(value) ||
        (missingLeft !== 0 && missingLeft !== 1) || (!terminal && !branch)) {
      throw new Error(`retained rain hurdle wind ${label} tree node mismatch`);
    }
  }
}

// validate the authenticated legacy v1 artifact before static Rust emission
function validateRetainedRainHurdleWindArtifact(artifact, featureNamesSha256) {
  let parsed;

  // use the ordinary JSON parser without evaluating the TypeScript module
  try {
    parsed = JSON.parse(artifact);
  } catch (error) {
    throw new Error("retained rain hurdle wind artifact JSON mismatch", { cause: error });
  }

  const root = retainedRainHurdleWindRecord(parsed, [
    "contractVersion",
    "modelMonth",
    "featureNames",
    "nativeModelSha256",
    "provenanceSha256",
    "rules",
    "categoryScales",
    "heads",
  ], "artifact");

  // bind the exact v1 month and frozen 107-name feature vector
  if (root.contractVersion !== "rain-hurdle-wind-runtime/v1" ||
      typeof root.modelMonth !== "string" ||
      !/^\d{4}-(?:0[1-9]|1[0-2])$/.test(root.modelMonth) ||
      !Array.isArray(root.featureNames) || root.featureNames.length !== 107 ||
      root.featureNames.some((name) => typeof name !== "string") ||
      sha256(JSON.stringify(root.featureNames)) !== featureNamesSha256) {
    throw new Error("retained rain hurdle wind artifact schema mismatch");
  }

  const nativeModelSha256 = retainedRainHurdleWindRecord(
    root.nativeModelSha256,
    RETAINED_RAIN_HURDLE_WIND_HEAD_NAMES,
    "native model",
  );
  const provenanceSha256 = retainedRainHurdleWindRecord(
    root.provenanceSha256,
    RETAINED_RAIN_HURDLE_WIND_PROVENANCE_NAMES,
    "provenance",
  );

  // close every retained native and provenance digest
  for (const name of RETAINED_RAIN_HURDLE_WIND_HEAD_NAMES) {
    retainedRainHurdleWindSha256(nativeModelSha256[name], `native model ${name}`);
  }
  // close every reviewed provenance member digest
  for (const name of RETAINED_RAIN_HURDLE_WIND_PROVENANCE_NAMES) {
    retainedRainHurdleWindSha256(provenanceSha256[name], `provenance ${name}`);
  }

  // require the exact three trained event rules and positive amount scales
  if (!Array.isArray(root.rules) || root.rules.length !== 3 ||
      !Array.isArray(root.categoryScales) || root.categoryScales.length !== 3) {
    throw new Error("retained rain hurdle wind rules schema mismatch");
  }
  // validate every rule in threshold order
  for (let index = 0; index < RETAINED_RAIN_HURDLE_WIND_THRESHOLDS.length; index += 1) {
    const rule = retainedRainHurdleWindRecord(
      root.rules[index],
      ["cutoff", "threshold"],
      `rule ${index}`,
    );
    const scale = root.categoryScales[index];

    // match the Rust cutoff, threshold, and category-scale invariants
    if (rule.threshold !== RETAINED_RAIN_HURDLE_WIND_THRESHOLDS[index] ||
        !(rule.cutoff === null || (typeof rule.cutoff === "number" &&
          Number.isFinite(rule.cutoff) && rule.cutoff >= 0 && rule.cutoff <= 1)) ||
        typeof scale !== "number" || !Number.isFinite(scale) || scale <= 0) {
      throw new Error(`retained rain hurdle wind rule ${index} mismatch`);
    }
  }

  const heads = retainedRainHurdleWindRecord(
    root.heads,
    RETAINED_RAIN_HURDLE_WIND_HEAD_NAMES,
    "heads",
  );
  // validate all four fixed output heads in serving order
  for (const name of RETAINED_RAIN_HURDLE_WIND_HEAD_NAMES) {
    const head = retainedRainHurdleWindRecord(
      heads[name],
      ["baseScore", "objective", "trees"],
      `head ${name}`,
    );
    const expectedObjective = name === "amount" ? "reg:gamma" : "binary:logistic";

    // require the reviewed objective, base score, and full 160-tree ensemble
    if (head.objective !== expectedObjective || typeof head.baseScore !== "number" ||
        !Number.isFinite(head.baseScore) || head.baseScore <= 0 ||
        (name !== "amount" && head.baseScore >= 1) ||
        !Array.isArray(head.trees) || head.trees.length !== 160) {
      throw new Error(`retained rain hurdle wind head ${name} mismatch`);
    }
    // validate each tree before rendering any Rust table
    for (let index = 0; index < head.trees.length; index += 1) {
      validateRetainedRainHurdleWindTree(head.trees[index], `${name}/${index}`);
    }
  }
  return root;
}

// render one frozen tree as five promoted static binary64 slices
function renderRetainedRainHurdleWindTree(tree, label, indentation) {
  const lines = [`${indentation}[`];

  // preserve the five inference vector positions exactly
  for (let vector = 0; vector < tree.length; vector += 1) {
    const values = tree[vector].map((value, index) =>
      renderRetainedRainHurdleWindF64(value, `${label}/${vector}/${index}`)
    );
    lines.push(`${indentation}    &[${values.join(", ")}],`);
  }
  lines.push(`${indentation}],`);
  return lines.join("\n");
}

// render the source-authenticated numerical artifact as one generated Rust module
function renderRetainedRainHurdleWindModule(root, expectedSha256) {
  const lines = [
    "pub mod retained_rain_hurdle_wind {",
    "    use super::{",
    "        Nullable, RainHurdleWindPortableArtifactV1HeadsObjectiveEnum,",
    "        RainHurdleWindPortableArtifactV1Rules,",
    "    };",
    "",
    "    #[derive(Debug, Clone, Copy)]",
    "    pub struct FrozenRainHead {",
    "        pub base_score: f64,",
    "        pub objective: RainHurdleWindPortableArtifactV1HeadsObjectiveEnum,",
    "        pub trees: &'static [[&'static [f64]; 5]],",
    "    }",
    "",
    `    pub const RETAINED_RAIN_MODEL_MONTH: &str = ${rustString(root.modelMonth)};`,
    `    pub const RETAINED_RAIN_MODEL_SHA256: &str = ${rustString(expectedSha256)};`,
    "",
    "    pub static RULES: [RainHurdleWindPortableArtifactV1Rules; 3] = [",
  ];

  // emit the three rules in their fixed threshold order
  for (let index = 0; index < root.rules.length; index += 1) {
    const rule = root.rules[index];
    const cutoff = rule.cutoff === null
      ? "Nullable::Null"
      : `Nullable::Value(${renderRetainedRainHurdleWindF64(rule.cutoff, `rule ${index} cutoff`)})`;
    lines.push("        RainHurdleWindPortableArtifactV1Rules {");
    lines.push(`            threshold: ${renderRetainedRainHurdleWindF64(rule.threshold, `rule ${index} threshold`)},`);
    lines.push(`            cutoff: ${cutoff},`);
    lines.push("        },");
  }
  lines.push("    ];", "", "    pub static CATEGORY_SCALES: [f64; 3] = [");
  // emit every category scale through its exact binary64 representation
  for (let index = 0; index < root.categoryScales.length; index += 1) {
    lines.push(`        ${renderRetainedRainHurdleWindF64(root.categoryScales[index], `category scale ${index}`)},`);
  }
  lines.push("    ];", "", "    pub static HEADS: [FrozenRainHead; 4] = [");
  // retain the public event-threshold and amount head order
  for (const name of RETAINED_RAIN_HURDLE_WIND_HEAD_NAMES) {
    const head = root.heads[name];
    const objective = name === "amount" ? "RegGamma" : "BinaryLogistic";
    lines.push("        FrozenRainHead {");
    lines.push(`            base_score: ${renderRetainedRainHurdleWindF64(head.baseScore, `head ${name} base score`)},`);
    lines.push(`            objective: RainHurdleWindPortableArtifactV1HeadsObjectiveEnum::${objective},`);
    lines.push("            trees: &[");
    // render all 160 trees under the owning static head
    for (let index = 0; index < head.trees.length; index += 1) {
      lines.push(renderRetainedRainHurdleWindTree(
        head.trees[index],
        `head ${name} tree ${index}`,
        "                ",
      ));
    }
    lines.push("            ],", "        },");
  }
  lines.push("    ];", "}");
  return lines.join("\n");
}

// extract, authenticate, validate, and render the retained model from pinned TypeScript syntax
function extractRetainedRainHurdleWindArtifact(sourceText, runtimeSourceText) {
  const sourceFile = parseSource(RETAINED_RAIN_HURDLE_WIND_SOURCE_PATH, sourceText);
  const runtimeSourceFile = parseSource(RAIN_HURDLE_WIND_RUNTIME_SOURCE_PATH, runtimeSourceText);
  const artifact = readExportedConstAssertionString(
    sourceFile,
    RETAINED_RAIN_HURDLE_WIND_JSON_EXPORT,
  );
  const expectedSha256 = readExportedConstAssertionString(
    sourceFile,
    RETAINED_RAIN_HURDLE_WIND_SHA256_EXPORT,
  );
  const featureNamesSha256 = readExportedConstAssertionString(
    runtimeSourceFile,
    RAIN_HURDLE_WIND_FEATURE_NAMES_SHA256_EXPORT,
  );

  // reject malformed or noncanonical retained digest literals before comparison
  if (!/^[0-9a-f]{64}$/.test(expectedSha256)) {
    throw new Error("retained rain hurdle wind SHA-256 literal is invalid");
  }

  // bind the decoded string's exact UTF-8 bytes to the exported retained identity
  if (sha256(artifact) !== expectedSha256) {
    throw new Error("retained rain hurdle wind artifact SHA-256 mismatch");
  }

  retainedRainHurdleWindSha256(featureNamesSha256, "feature names");
  const parsed = validateRetainedRainHurdleWindArtifact(artifact, featureNamesSha256);
  return {
    artifact,
    rust: renderRetainedRainHurdleWindModule(parsed, expectedSha256),
  };
}

// prove the locked node-postgres type marker contributes no named property
function verifyPgRowMarker(readDeclarationFile) {
  const files = new Map();

  // pin the installed runtime package mapping and both declaration entry files
  for (const [name, expected] of Object.entries(PG_MARKER_INPUTS)) {
    const path = name.endsWith("/package.json")
      ? fileURLToPath(import.meta.resolve(name))
      : resolve(
        dirname(fileURLToPath(import.meta.resolve("@types/pg/package.json"))),
        posix.basename(name),
      );
    const bytes = readDeclarationFile(path);

    // reject input drift without resolving or evaluating any package code
    if (createHash("sha256").update(bytes).digest("hex") !== expected) {
      throw new Error("pinned PostgreSQL row marker input mismatch at " + name);
    }

    // retain only the two declaration inputs in the closed checker
    if (name === "@types/pg/index.d.mts" || name === "@types/pg/index.d.ts") {
      const virtualName = name.endsWith(".d.mts") ? PG_MARKER_ENTRY : PG_MARKER_TARGET;
      files.set(virtualName, parseSource(virtualName, bytes.toString("utf8")));
    }
  }

  const marker = findUtilityDeclaration(files.get(PG_MARKER_TARGET), "QueryResultRow", ts.isInterfaceDeclaration);
  const index = marker.members[0];
  const key = index !== undefined && ts.isIndexSignatureDeclaration(index)
    ? index.parameters[0]
    : undefined;

  // require one exported nongeneric interface with only the open string index
  if (
    !isExported(marker) ||
    marker.typeParameters !== undefined ||
    marker.heritageClauses !== undefined ||
    marker.members.length !== 1 ||
    !ts.isIndexSignatureDeclaration(index) ||
    index.modifiers !== undefined ||
    index.parameters.length !== 1 ||
    key?.type?.kind !== ts.SyntaxKind.StringKeyword ||
    index.type?.kind !== ts.SyntaxKind.AnyKeyword
  ) {
    throw new Error("pinned PostgreSQL row marker shape unsupported");
  }
  return { files, marker };
}

// select one exact declaration of the requested kind from the pinned library
function findUtilityDeclaration(sourceFile, name, isDeclaration = ts.isTypeAliasDeclaration) {
  const matches = sourceFile.statements.filter((statement) =>
    isDeclaration(statement) && statement.name.text === name,
  );

  // fail closed if the pinned declaration shape changes
  if (matches.length !== 1) {
    throw new Error(`pinned TypeScript utility declaration ${name} is not unique`);
  }

  return matches[0];
}

// select the pinned Object value, type companion, and freeze overloads
function findUtilityObjectFreezeDeclarations(sourceFile) {
  const constructors = sourceFile.statements.filter((statement) =>
    ts.isInterfaceDeclaration(statement) && statement.name.text === "ObjectConstructor"
  );
  const objectTypes = sourceFile.statements.filter(
    // retain the exact merged type-side declaration
    (statement) => ts.isInterfaceDeclaration(statement) && statement.name.text === "Object",
  );
  const objectVariables = sourceFile.statements.flatMap((statement) =>
    ts.isVariableStatement(statement)
      ? statement.declarationList.declarations.filter((declaration) =>
        ts.isIdentifier(declaration.name) && declaration.name.text === "Object"
      )
      : []
  );

  // fail closed if the hash-pinned builtin surface is not the reviewed shape
  if (constructors.length !== 1 || objectVariables.length !== 1 || objectTypes.length !== 1) {
    throw new Error("pinned TypeScript Object.freeze declarations are not unique");
  }

  const freeze = constructors[0].members.filter((member) =>
    ts.isMethodSignature(member) &&
    ts.isIdentifier(member.name) &&
    member.name.text === "freeze"
  );

  // retain the exact three overload nodes from the pinned declaration file
  if (freeze.length !== 3) {
    throw new Error("pinned TypeScript Object.freeze declarations are unsupported");
  }

  return { Object: objectVariables[0], ObjectType: objectTypes[0], freeze };
}

// verify the utility input once per opted-in generation
function readVerifiedUtilityText(readUtilityFile) {
  const bytes = readUtilityFile(UTILITY_DECLARATIONS_PATH);
  const digest = createHash("sha256").update(bytes).digest("hex");

  // reject a changed or differently resolved compiler library
  if (digest !== UTILITY_DECLARATIONS_SHA256) {
    throw new Error(
      `TypeScript utility declarations mismatch: expected ${UTILITY_DECLARATIONS_SHA256}, loaded ${digest}`,
    );
  }

  return bytes.toString("utf8");
}

// create fresh utility declarations for one isolated checker
function parseUtilityDeclarations(text) {
  const sourceFile = ts.createSourceFile(
    UTILITY_DECLARATIONS_VIRTUAL_PATH,
    text,
    ts.ScriptTarget.Latest,
    true,
    ts.ScriptKind.TS,
  );

  // reject malformed pinned declarations before asking the checker for identity
  if (sourceFile.parseDiagnostics.length > 0) {
    const diagnostic = sourceFile.parseDiagnostics[0];
    const message = ts.flattenDiagnosticMessageText(diagnostic.messageText, " ");
    throw new Error(`TypeScript utility declarations parse error: TS${diagnostic.code} ${message}`);
  }

  const objectFreeze = findUtilityObjectFreezeDeclarations(sourceFile);

  return {
    sourceFile,
    declarations: {
      Exclude: findUtilityDeclaration(sourceFile, "Exclude"),
      Extract: findUtilityDeclaration(sourceFile, "Extract"),
      Omit: findUtilityDeclaration(sourceFile, "Omit"),
      Pick: findUtilityDeclaration(sourceFile, "Pick"),
      Partial: findUtilityDeclaration(sourceFile, "Partial"),
      Readonly: findUtilityDeclaration(sourceFile, "Readonly"),
      Record: findUtilityDeclaration(sourceFile, "Record"),
      ReturnType: findUtilityDeclaration(sourceFile, "ReturnType"),
      Uint8Array: findUtilityDeclaration(sourceFile, "Uint8Array", ts.isInterfaceDeclaration),
      Float32Array: findUtilityDeclaration(sourceFile, "Float32Array", ts.isInterfaceDeclaration),
      Date: sourceFile.statements.filter((statement) =>
        ts.isInterfaceDeclaration(statement) && statement.name.text === "Date"
      ),
      Object: objectFreeze.Object,
      ObjectType: objectFreeze.ObjectType,
      ObjectFreeze: objectFreeze.freeze,
    },
  };
}

// identify any ambient global augmentation in one selected external module
function hasGlobalAugmentation(sourceFile) {
  let found = false;

  // inspect nested namespace declarations until one augmentation is found
  function visit(node) {
    // stop after the result is known
    if (found) {
      return;
    }

    // reject any source-owned contribution to global utility identity
    if (
      ts.isModuleDeclaration(node) &&
      (node.flags & ts.NodeFlags.GlobalAugmentation) !== 0
    ) {
      found = true;
      return;
    }

    ts.forEachChild(node, visit);
  }

  visit(sourceFile);
  return found;
}

// create one isolated public checker over the already parsed source objects
function createSourceChecker(sourcePath, sourceFile, utilityDeclarations) {
  const virtualSourcePath = `/${sourcePath}`;
  const explicitSources = new Map([[virtualSourcePath, sourceFile]]);

  // expose only the pinned utility declarations for opted-in sources
  if (utilityDeclarations !== undefined) {
    explicitSources.set(UTILITY_DECLARATIONS_VIRTUAL_PATH, utilityDeclarations.sourceFile);
  }

  const options = {
    target: ts.ScriptTarget.Latest,
    noEmit: true,
    noLib: true,
    noResolve: true,
    types: [],
  };
  const host = {
    // return only explicitly selected in-memory sources
    getSourceFile(fileName) {
      return explicitSources.get(fileName);
    },
    // name an unavailable library because noLib forbids loading it
    getDefaultLibFileName() {
      return "/__weather_contracts__/unavailable-lib.d.ts";
    },
    // fail closed if an unexpected emit path is ever reached
    writeFile() {
      throw new Error(`unexpected TypeScript emit for ${sourcePath}`);
    },
    // keep virtual paths independent of the process working directory
    getCurrentDirectory() {
      return "/";
    },
    // preserve exact case-sensitive virtual identities
    getCanonicalFileName(fileName) {
      return fileName;
    },
    // use the repository's case-sensitive identity rules
    useCaseSensitiveFileNames() {
      return true;
    },
    // make compiler-created diagnostics deterministic
    getNewLine() {
      return "\n";
    },
    // expose only the explicit virtual sources as existing
    fileExists(fileName) {
      return explicitSources.has(fileName);
    },
    // expose exact in-memory text without touching the filesystem
    readFile(fileName) {
      return explicitSources.get(fileName)?.text;
    },
  };
  const program = ts.createProgram([...explicitSources.keys()], options, host);

  // require the checker to bind the exact AST object selected by the generator
  if (program.getSourceFile(virtualSourcePath) !== sourceFile) {
    throw new Error(`TypeScript checker source identity mismatch for ${sourcePath}`);
  }

  // require the checker to retain the exact pinned utility AST object
  if (
    utilityDeclarations !== undefined &&
    program.getSourceFile(UTILITY_DECLARATIONS_VIRTUAL_PATH) !== utilityDeclarations.sourceFile
  ) {
    throw new Error(`TypeScript utility declarations identity mismatch for ${sourcePath}`);
  }

  return program.getTypeChecker();
}

// create one closed checker for the exact configured import graph
function createTypeImportGraphChecker(sourceFiles, resolutions, utilityDeclarations) {
  const explicitSources = new Map(
    [...sourceFiles].map(([sourcePath, sourceFile]) => [`/${sourcePath}`, sourceFile]),
  );

  // expose one fresh pinned utility AST when a participating source opts in
  if (utilityDeclarations !== undefined) {
    explicitSources.set(UTILITY_DECLARATIONS_VIRTUAL_PATH, utilityDeclarations.sourceFile);
  }

  const options = {
    target: ts.ScriptTarget.Latest,
    module: ts.ModuleKind.ESNext,
    noEmit: true,
    noLib: true,
    noResolve: true,
    types: [],
  };
  const host = {
    // return only explicitly configured graph inputs
    getSourceFile(fileName) {
      return explicitSources.get(fileName);
    },
    // name an unavailable library because noLib forbids implicit loading
    getDefaultLibFileName() {
      return "/__weather_contracts__/unavailable-lib.d.ts";
    },
    // fail closed if an unexpected emit path is reached
    writeFile() {
      throw new Error("unexpected TypeScript emit for configured type imports");
    },
    // keep virtual paths independent of the working directory
    getCurrentDirectory() {
      return "/";
    },
    // preserve exact case-sensitive graph identities
    getCanonicalFileName(fileName) {
      return fileName;
    },
    // match repository case sensitivity
    useCaseSensitiveFileNames() {
      return true;
    },
    // make compiler-created diagnostics deterministic
    getNewLine() {
      return "\n";
    },
    // expose only configured graph and utility files
    fileExists(fileName) {
      return explicitSources.has(fileName);
    },
    // expose exact in-memory source text only
    readFile(fileName) {
      return explicitSources.get(fileName)?.text;
    },
    // resolve only edges proven by the configured package and barrel graph
    resolveModuleNames(moduleNames, containingFile) {
      return moduleNames.map((moduleName) => {
        const target = resolutions.get(`${containingFile}\0${moduleName}`);

        // leave every unrelated application import unresolved
        if (target === undefined) {
          return undefined;
        }
        return {
          resolvedFileName: `/${target}`,
          extension: target === PG_MARKER_ENTRY ? ts.Extension.Dmts
            : target === PG_MARKER_TARGET ? ts.Extension.Dts : ts.Extension.Ts,
          isExternalLibraryImport: !moduleName.startsWith("."),
        };
      });
    },
  };
  const program = ts.createProgram([...explicitSources.keys()], options, host);

  // require every participating AST to be the exact parsed input object
  for (const [sourcePath, sourceFile] of sourceFiles) {
    if (program.getSourceFile(`/${sourcePath}`) !== sourceFile) {
      throw new Error(`TypeScript import checker source identity mismatch for ${sourcePath}`);
    }
  }

  // keep utility identity within the same isolated graph program
  if (
    utilityDeclarations !== undefined &&
    program.getSourceFile(UTILITY_DECLARATIONS_VIRTUAL_PATH) !== utilityDeclarations.sourceFile
  ) {
    throw new Error("TypeScript utility declarations identity mismatch for configured type imports");
  }
  return program.getTypeChecker();
}

// reuse one isolated checker lazily for each selected source
function getSourceChecker(checkers, sourcePath, sourceFile, utilityDeclarations) {
  let checker = checkers.get(sourcePath);

  // bind a source only when a supported symbolic form needs proof
  if (checker === undefined) {
    checker = createSourceChecker(sourcePath, sourceFile, utilityDeclarations);
    checkers.set(sourcePath, checker);
  }

  return checker;
}

// form the private dependency identity without changing manifest configuration
function configuredRootIdentity(root) {
  return `${root.source}\0${root.exportName}`;
}

// select exactly one exported interface or type alias
function findRootDeclaration(sourceFile, root) {
  const named = sourceFile.statements.filter((statement) =>
    (ts.isInterfaceDeclaration(statement) || ts.isTypeAliasDeclaration(statement)) &&
    statement.name.text === root.exportName,
  );

  // reject merged or repeated declarations
  if (named.length > 1) {
    throw new Error(`duplicate root declaration ${root.source}#${root.exportName}`);
  }

  // reject missing or inferred value declarations
  if (named.length === 0) {
    throw new Error(`explicit type root not found ${root.source}#${root.exportName}`);
  }

  const declaration = named[0];
  const privateInterface = root.localInterface === true || root.pgQueryOwners !== undefined;

  // require ordinary roots to be public source contracts
  if (root.localAlias !== true && !privateInterface && !isExported(declaration)) {
    throw new Error(`root must be a named export ${root.source}#${root.exportName}`);
  }

  // require private roots to be absent from the effective module export surface
  if (root.localAlias === true || privateInterface) {
    const directExport = sourceFile.statements.some((statement) =>
      ts.isExportDeclaration(statement) &&
      statement.moduleSpecifier === undefined &&
      statement.exportClause !== undefined &&
      ts.isNamedExports(statement.exportClause) &&
      statement.exportClause.elements.some((element) =>
        (element.propertyName ?? element.name).text === root.exportName
      )
    );

    // reject modifier exports and every direct export-list reuse of the declaration
    if (isExported(declaration) || directExport) {
      const label = root.localAlias === true ? "local alias"
        : root.pgQueryOwners !== undefined ? "private PostgreSQL query row"
        : "local interface";
      throw new Error(`${label} must not be exported ${root.source}#${root.exportName}`);
    }
  }

  // keep the alias opt-in on one module-scoped type alias declaration
  if (root.localAlias === true) {
    if (
      !ts.isTypeAliasDeclaration(declaration) ||
      declaration.parent !== sourceFile ||
      !ts.isExternalModule(sourceFile)
    ) {
      throw new Error(`local alias declaration unsupported ${root.source}#${root.exportName}`);
    }
  }

  // keep the interface opt-in on one module-scoped nongeneric interface
  if (
    privateInterface &&
    (!ts.isInterfaceDeclaration(declaration) ||
      declaration.parent !== sourceFile ||
      !ts.isExternalModule(sourceFile) ||
      (declaration.typeParameters?.length ?? 0) > 0)
  ) {
    throw new Error(`local interface declaration unsupported ${root.source}#${root.exportName}`);
  }

  return declaration;
}

// select one exact configured indexed-literal declaration
function findIndexedLiteralDeclaration(sourceFile, sourcePath, aliasName) {
  const matches = sourceFile.statements.filter((statement) =>
    (ts.isTypeAliasDeclaration(statement) || ts.isInterfaceDeclaration(statement)) &&
    statement.name.text === aliasName
  );

  // reject missing, merged, and nested proof inputs before traversal
  if (
    matches.length !== 1 ||
    matches[0].parent !== sourceFile ||
    !ts.isExternalModule(sourceFile)
  ) {
    throw new Error(`indexed literal alias declaration unsupported ${sourcePath}#${aliasName}`);
  }
  return matches[0];
}

// resolve one manifest-relative path without filesystem traversal
function resolvePackageTarget(packageJsonPath, target, label) {
  // require one explicit relative package target
  if (typeof target !== "string" || !target.startsWith("./") || target.includes("\\")) {
    throw new Error(`type import package metadata unsupported at ${label}`);
  }
  const directory = posix.dirname(packageJsonPath);
  const resolved = posix.normalize(posix.join(directory, target.slice(2)));
  const relative = posix.relative(directory, resolved);

  // keep every declared target within its configured package directory
  if (relative.length === 0 || relative === ".." || relative.startsWith("../")) {
    throw new Error(`type import package metadata unsupported at ${label}`);
  }
  return resolved;
}

// resolve one explicit repository-relative config inheritance edge
function resolveRepositoryReference(containingPath, target, label) {
  // require a relative portable reference without filesystem traversal
  if (typeof target !== "string" || !target.startsWith(".") || target.includes("\\")) {
    throw new Error(`compiled source config reference unsupported at ${label}`);
  }
  const resolved = posix.normalize(posix.join(posix.dirname(containingPath), target));

  // retain the reference inside the repository root
  if (resolved === ".." || resolved.startsWith("../") || resolved.startsWith("/")) {
    throw new Error(`compiled source config reference unsupported at ${label}`);
  }
  return resolved;
}

// parse one explicitly selected compiled-source JSON input
function parseCompiledSourceJson(sources, sourcePath, label) {
  try {
    const value = JSON.parse(sources.get(sourcePath));

    // retain only ordinary JSON records as metadata roots
    if (!isPlainObject(value)) {
      throw new Error("not a plain object");
    }
    return value;
  } catch {
    throw new Error(`compiled source ${label} unsupported at ${sourcePath}`);
  }
}

// validate one package's exact source-to-declaration compilation identity
function validateCompiledSource(typeImport, packageValue, sources) {
  const packageDirectory = posix.dirname(typeImport.packageJson);
  const expectedEntry = posix.join(packageDirectory, "src/index.ts");
  const expectedRuntime = posix.join(packageDirectory, "dist/index.js");
  const expectedTypes = posix.join(packageDirectory, "dist/index.d.ts");
  const expectedRuntimeTarget = `./${posix.relative(packageDirectory, expectedRuntime)}`;
  const expectedTypesTarget = `./${posix.relative(packageDirectory, expectedTypes)}`;

  // derive both public compiled targets from the same selected source entry
  if (
    typeImport.entry !== expectedEntry ||
    !isPlainObject(packageValue) ||
    packageValue.name !== typeImport.specifier ||
    packageValue.type !== "module" ||
    Object.hasOwn(packageValue, "typesVersions") ||
    packageValue.exports !== expectedRuntimeTarget ||
    packageValue.types !== expectedTypesTarget
  ) {
    throw new Error(`compiled source package metadata unsupported at ${typeImport.specifier}`);
  }

  const tsconfig = parseCompiledSourceJson(
    sources,
    typeImport.compiledSource.tsconfig,
    "tsconfig",
  );
  const base = parseCompiledSourceJson(sources, typeImport.compiledSource.base, "base config");
  const expectedCompilerOptions = {
    declaration: true,
    outDir: "dist",
    rootDir: "src",
    types: ["node"],
  };

  // require the package config to contain only the reviewed emission surface
  if (
    Object.keys(tsconfig).sort().join("\0") !==
      ["compilerOptions", "extends", "include"].sort().join("\0") ||
    !isPlainObject(tsconfig.compilerOptions) ||
    !Array.isArray(tsconfig.include) ||
    resolveRepositoryReference(
      typeImport.compiledSource.tsconfig,
      tsconfig.extends,
      typeImport.compiledSource.tsconfig,
    ) !== typeImport.compiledSource.base ||
    canonicalJson(tsconfig.compilerOptions) !== canonicalJson(expectedCompilerOptions) ||
    canonicalJson(tsconfig.include) !== canonicalJson(["src/**/*.ts"])
  ) {
    throw new Error(`compiled source tsconfig unsupported at ${typeImport.compiledSource.tsconfig}`);
  }

  const expectedBase = {
    compilerOptions: {
      allowJs: false,
      exactOptionalPropertyTypes: true,
      forceConsistentCasingInFileNames: true,
      isolatedModules: true,
      module: "NodeNext",
      moduleResolution: "NodeNext",
      noFallthroughCasesInSwitch: true,
      noImplicitOverride: true,
      noUncheckedIndexedAccess: true,
      resolveJsonModule: true,
      skipLibCheck: true,
      strict: true,
      target: "ES2023",
      verbatimModuleSyntax: true,
    },
  };

  // pin the inherited compiler surface so resolution and emission cannot drift
  if (canonicalJson(base) !== canonicalJson(expectedBase)) {
    throw new Error(`compiled source base config unsupported at ${typeImport.compiledSource.base}`);
  }
}

// identify a source-owned external module augmentation
function hasExternalModuleAugmentation(sourceFile) {
  let found = false;

  // inspect nested declarations without treating ordinary string literals as modules
  function visit(node) {
    // stop after the result is known
    if (found) {
      return;
    }

    // reject any ambient contribution to a package export surface
    if (ts.isModuleDeclaration(node) && ts.isStringLiteralLike(node.name)) {
      found = true;
      return;
    }
    ts.forEachChild(node, visit);
  }

  visit(sourceFile);
  return found;
}

// find one exact exported symbol from an isolated source checker
function configuredLeafExport(checker, sourceFile, exportName) {
  const moduleSymbol = checker.getSymbolAtLocation(sourceFile);

  // require the leaf to remain an external module
  if (moduleSymbol === undefined) {
    return null;
  }
  const matches = checker.getExportsOfModule(moduleSymbol).filter(
    (symbol) => symbol.getName() === exportName,
  );
  return matches.length === 1 ? matches[0] : null;
}

// resolve one checker symbol to its exact declaration owner
function resolvedSymbolDeclaration(checker, symbol) {
  // follow only the compiler's exact alias edge
  const target = (symbol.flags & ts.SymbolFlags.Alias) === 0
    ? symbol
    : checker.getAliasedSymbol(symbol);
  return target.declarations?.length === 1 ? target.declarations[0] : null;
}

// verify closed package export graphs and their configured type identities
function validateTypeImportGraphs(
  config,
  sources,
  sourceFiles,
  selectedRoots,
  selectedIndexedLiteralDeclarations,
  checkers,
  utilityDeclarationsBySource,
  pgRowMarker,
) {
  const byConsumer = new Map();
  const verified = [];
  const prepared = [];
  const graphPaths = new Set();
  const resolutions = new Map();

  // add one exact resolution without allowing conflicting graph edges
  function addResolution(containingPath, moduleName, targetPath) {
    const key = `/${containingPath}\0${moduleName}`;
    const prior = resolutions.get(key);

    // reject two configured graphs that disagree about one import edge
    if (prior !== undefined && prior !== targetPath) {
      throw new Error(`conflicting type import resolution at ${containingPath}#${moduleName}`);
    }
    resolutions.set(key, targetPath);
  }

  // validate every declared package and build the bounded resolution table
  for (const typeImport of config.typeImports ?? []) {
    // add one exact direct sibling edge without package metadata or a barrel hop
    if (typeImport.packageJson === undefined) {
      const consumerSource = sourceFiles.get(typeImport.consumer);
      const entrySource = sourceFiles.get(typeImport.entry);

      // keep both endpoints free of ambient module extensions
      if (hasExternalModuleAugmentation(consumerSource)) {
        throw new Error(`type import module augmentation unsupported at ${typeImport.consumer}`);
      }
      if (hasExternalModuleAugmentation(entrySource)) {
        throw new Error(`type import module augmentation unsupported at ${typeImport.entry}`);
      }

      // require the sibling to own declarations rather than forward exports
      if (entrySource.statements.some((statement) => ts.isExportDeclaration(statement))) {
        throw new Error(`type import entry re-export unsupported at ${typeImport.entry}`);
      }
      graphPaths.add(typeImport.consumer);
      graphPaths.add(typeImport.entry);
      addResolution(typeImport.consumer, typeImport.specifier, typeImport.entry);
      prepared.push(typeImport);
      continue;
    }

    let packageValue;

    // parse only the explicitly configured package metadata input
    try {
      packageValue = JSON.parse(sources.get(typeImport.packageJson));
    } catch {
      throw new Error(
        `${typeImport.compiledSource === undefined ? "type import" : "compiled source"} package metadata unsupported at ${typeImport.specifier}`,
      );
    }
    const rootExport = packageValue?.exports?.["."];

    // select the reviewed source-compiled or declaration-source package shape
    if (typeImport.compiledSource !== undefined) {
      validateCompiledSource(typeImport, packageValue, sources);
    } else {
      // require the compiled-source opt-in for packages exposing built files
      if (typeof packageValue?.exports === "string") {
        throw new Error(`compiled source package metadata unsupported at ${typeImport.specifier}`);
      }

      // pin the package name, ESM mode, direct types path and condition order
      if (
        !isPlainObject(packageValue) ||
        packageValue.name !== typeImport.specifier ||
        packageValue.type !== "module" ||
        Object.hasOwn(packageValue, "typesVersions") ||
        resolvePackageTarget(typeImport.packageJson, packageValue.types, typeImport.specifier) !==
          typeImport.entry ||
        !isPlainObject(packageValue.exports) ||
        !isPlainObject(rootExport) ||
        Object.keys(rootExport).length !== 2 ||
        Object.keys(rootExport)[0] !== "types" ||
        Object.keys(rootExport)[1] !== "default" ||
        resolvePackageTarget(typeImport.packageJson, rootExport.types, typeImport.specifier) !==
          typeImport.entry
      ) {
        throw new Error(`type import package metadata unsupported at ${typeImport.specifier}`);
      }
      resolvePackageTarget(typeImport.packageJson, rootExport.default, typeImport.specifier);
    }

    const consumerSource = sourceFiles.get(typeImport.consumer);
    const entrySource = sourceFiles.get(typeImport.entry);
    const entryLeaves = [];

    // exclude ambient extensions of the configured consumer or package graph
    if (hasExternalModuleAugmentation(consumerSource)) {
      throw new Error(`type import module augmentation unsupported at ${typeImport.consumer}`);
    }
    if (hasExternalModuleAugmentation(entrySource)) {
      throw new Error(`type import module augmentation unsupported at ${typeImport.entry}`);
    }

    // accept only direct export-star clauses from the package types entry
    for (const statement of entrySource.statements) {
      if (
        !ts.isExportDeclaration(statement) ||
        statement.exportClause !== undefined ||
        statement.attributes !== undefined ||
        statement.assertClause !== undefined ||
        statement.moduleSpecifier === undefined ||
        !ts.isStringLiteralLike(statement.moduleSpecifier) ||
        !/^\.\/[A-Za-z0-9._-]+\.js$/u.test(statement.moduleSpecifier.text)
      ) {
        throw new Error(`type import entry unsupported at ${typeImport.entry}`);
      }
      const leaf = posix.join(
        posix.dirname(typeImport.entry),
        statement.moduleSpecifier.text.slice(2, -3) + ".ts",
      );
      entryLeaves.push(leaf);
      addResolution(typeImport.entry, statement.moduleSpecifier.text, leaf);
    }

    // bind the complete one-hop entry rather than a hand-picked subset
    if (
      entryLeaves.length === 0 ||
      entryLeaves.length !== typeImport.leaves.length ||
      new Set(entryLeaves).size !== entryLeaves.length ||
      entryLeaves.sort().some((leaf, index) => leaf !== typeImport.leaves[index])
    ) {
      throw new Error(`type import leaves mismatch at ${typeImport.entry}`);
    }

    // keep leaves as declaration owners rather than permitting another hop
    for (const leaf of typeImport.leaves) {
      const leafSource = sourceFiles.get(leaf);

      // reject ambient additions and any second export hop
      if (hasExternalModuleAugmentation(leafSource)) {
        throw new Error(`type import module augmentation unsupported at ${leaf}`);
      }
      if (leafSource.statements.some((statement) => ts.isExportDeclaration(statement))) {
        throw new Error(`type import leaf re-export unsupported at ${leaf}`);
      }
      graphPaths.add(leaf);
    }
    graphPaths.add(typeImport.consumer);
    graphPaths.add(typeImport.entry);
    addResolution(typeImport.consumer, typeImport.specifier, typeImport.entry);
    prepared.push(typeImport);
  }

  // resolve the marker through only its pinned esm facade and declaration owner
  if (pgRowMarker !== undefined) {
    addResolution(PG_MARKER_ENTRY, "./index.js", PG_MARKER_TARGET);

    // expose pg only in sources with the explicit named-field projection
    for (const root of config.roots) {
      if (root.pgRowNamedFields) {
        graphPaths.add(root.source);
        addResolution(root.source, "pg", PG_MARKER_ENTRY);
      }
    }
  }

  // preserve isolated checkers when no closed graph is needed
  if (graphPaths.size === 0) {
    return { byConsumer, verified };
  }

  // isolate utility globals from scripts and explicit global augmentations
  for (const sourcePath of graphPaths) {
    const sourceFile = sourceFiles.get(sourcePath);

    // require every graph participant to keep its declarations module-scoped
    if (!ts.isExternalModule(sourceFile) || hasGlobalAugmentation(sourceFile)) {
      throw new Error(`type import graph source unsupported at ${sourcePath}`);
    }
  }

  const graphSources = new Map(
    [...graphPaths].sort().map((sourcePath) => [sourcePath, sourceFiles.get(sourcePath)]),
  );
  // reserve the private paths and bind the exact installed pg AST objects
  for (const [sourcePath, sourceFile] of pgRowMarker?.files ?? []) {
    if (sourceFiles.has(sourcePath)) {
      throw new Error("source collides with PostgreSQL marker declarations at " + sourcePath);
    }
    graphSources.set(sourcePath, sourceFile);
  }
  const utilitySource = [...graphPaths].find((sourcePath) =>
    utilityDeclarationsBySource.has(sourcePath)
  );
  const graphUtilityDeclarations = utilitySource === undefined
    ? undefined
    : utilityDeclarationsBySource.get(utilitySource);
  const graphChecker = createTypeImportGraphChecker(
    graphSources,
    resolutions,
    graphUtilityDeclarations,
  );

  // seed the checker cache so no participating AST enters another program
  for (const sourcePath of graphPaths) {
    if (checkers.has(sourcePath)) {
      throw new Error(`duplicate TypeScript checker ownership for ${sourcePath}`);
    }
    checkers.set(sourcePath, graphChecker);

    // point opted-in utility rendering at the AST bound by this graph checker
    if (utilityDeclarationsBySource.has(sourcePath)) {
      utilityDeclarationsBySource.set(sourcePath, graphUtilityDeclarations);
    }
  }

  // prove leaf and effective barrel exports against the same closed checker
  for (const typeImport of prepared) {
    const exported = new Map();
    const proofExported = new Map();

    // prove each configured package name has one exact leaf and entry declaration
    for (const [exportName, leaf] of Object.entries(typeImport.exports)) {
      const configured = selectedRoots.get(`${leaf}\0${exportName}`);
      const leafMatches = [];

      // detect the same export name appearing in any configured declaration owner
      for (const candidateLeaf of typeImport.leaves ?? [typeImport.entry]) {
        const candidateSource = sourceFiles.get(candidateLeaf);
        const symbol = configuredLeafExport(graphChecker, candidateSource, exportName);

        // retain only genuinely exported names
        if (symbol !== null) {
          leafMatches.push({
            leaf: candidateLeaf,
            declaration: resolvedSymbolDeclaration(graphChecker, symbol),
          });
        }
      }
      const entrySymbol = configuredLeafExport(
        graphChecker,
        sourceFiles.get(typeImport.entry),
        exportName,
      );
      const leafMatch = leafMatches[0];

      // reject missing, ambiguous, merged, or wrong-owner effective exports
      if (
        configured === undefined ||
        leafMatches.length !== 1 ||
        leafMatch.leaf !== leaf ||
        leafMatch.declaration !== configured.declaration ||
        entrySymbol === null ||
        resolvedSymbolDeclaration(graphChecker, entrySymbol) !== configured.declaration
      ) {
        throw new Error(
          `type import export identity unsupported for ${exportName} from ${typeImport.specifier}`,
        );
      }
      exported.set(exportName, configured);
    }

    // prove each explicit named declaration without widening ordinary import permissions
    for (const [proofName, leaf] of Object.entries(typeImport.proofExports ?? {})) {
      const identity = `${leaf}\0${proofName}`;
      const indexedDeclaration = selectedIndexedLiteralDeclarations.get(identity);
      const legacyDeclaration = typeImport.packageJson === undefined &&
        isMaintenanceLoaderLegacyProof(config.roots, typeImport.consumer, leaf, proofName)
        ? selectedRoots.get(identity)?.declaration
        : undefined;

      // reconcile overlapping indexed and legacy proofs against the same declaration
      if (indexedDeclaration !== undefined && legacyDeclaration !== undefined &&
        indexedDeclaration !== legacyDeclaration) {
        throw new Error(`conflicting type import proof declaration for ${proofName} from ${typeImport.specifier}`);
      }
      const expectedDeclaration = indexedDeclaration ?? legacyDeclaration;
      const leafMatches = [];

      // detect every effective owner for the proof name across the graph
      for (const candidateLeaf of typeImport.leaves ?? [typeImport.entry]) {
        const candidateSource = sourceFiles.get(candidateLeaf);
        const symbol = configuredLeafExport(graphChecker, candidateSource, proofName);

        // retain only exact exported declaration identities
        if (symbol !== null) {
          leafMatches.push({
            leaf: candidateLeaf,
            declaration: resolvedSymbolDeclaration(graphChecker, symbol),
          });
        }
      }
      const entrySymbol = configuredLeafExport(
        graphChecker,
        sourceFiles.get(typeImport.entry),
        proofName,
      );
      const leafMatch = leafMatches[0];

      // require one configured owner and one identical effective entry export
      if (
        expectedDeclaration === undefined ||
        leafMatches.length !== 1 ||
        leafMatch.leaf !== leaf ||
        leafMatch.declaration !== expectedDeclaration ||
        entrySymbol === null ||
        resolvedSymbolDeclaration(graphChecker, entrySymbol) !== expectedDeclaration
      ) {
        throw new Error(
          `type import proof identity unsupported for ${proofName} from ${typeImport.specifier}`,
        );
      }
      proofExported.set(proofName, expectedDeclaration);
    }

    const binding = {
      ...typeImport,
      exported,
      proofExported,
      used: new Set(),
      proofUsed: new Set(),
      tupleIdentityWrapperUsed: false,
      literalStringValueUsed: false,
      checker: graphChecker,
      sourceFiles,
    };
    let consumerBindings = byConsumer.get(typeImport.consumer);

    // group configured packages by their exact consumer source
    if (consumerBindings === undefined) {
      consumerBindings = new Map();
      byConsumer.set(typeImport.consumer, consumerBindings);
    }
    consumerBindings.set(typeImport.specifier, binding);
    verified.push(typeImport);
  }

  return { byConsumer, verified };
}

// resolve one exact configured named type-only import
function resolveApprovedTypeImport(identifier, state, pathLabel) {
  const checker = getSourceChecker(
    state.checkers,
    state.sourcePath,
    state.sourceFile,
    state.utilityDeclarations,
  );
  const symbol = checker.getSymbolAtLocation(identifier);
  const declaration = symbol?.declarations?.[0];

  // require one compiler alias bound to one named import specifier
  if (
    symbol === undefined ||
    (symbol.flags & ts.SymbolFlags.Alias) === 0 ||
    symbol.declarations?.length !== 1 ||
    !ts.isImportSpecifier(declaration) ||
    state.typeImportBindings === undefined
  ) {
    throw new Error(`configured type import identity unsupported at ${pathLabel}`);
  }

  const namedImports = declaration.parent;
  const importClause = namedImports.parent;
  const importDeclaration = importClause.parent;

  // accept only an explicit type-only named binding from a proven graph
  if (
    !ts.isNamedImports(namedImports) ||
    !ts.isImportClause(importClause) ||
    !ts.isImportDeclaration(importDeclaration) ||
    (!declaration.isTypeOnly && !importClause.isTypeOnly) ||
    !ts.isStringLiteralLike(importDeclaration.moduleSpecifier)
  ) {
    throw new Error(`configured type import identity unsupported at ${pathLabel}`);
  }

  const binding = state.typeImportBindings.get(importDeclaration.moduleSpecifier.text);
  const exportName = (declaration.propertyName ?? declaration.name).text;
  const configured = binding?.exported.get(exportName);

  // join the compiler alias edge to the exact configured declaration
  if (
    binding === undefined ||
    configured === undefined ||
    declaration.name.text !== identifier.text ||
    resolvedSymbolDeclaration(checker, symbol) !== configured.declaration
  ) {
    throw new Error(`configured type import identity unsupported at ${pathLabel}`);
  }

  return { binding, checker, configured, exportName };
}

// resolve one named value import only for its configured proof role
function resolveApprovedValueImport(identifier, state, pathLabel, proofRole) {
  const checker = getSourceChecker(
    state.checkers,
    state.sourcePath,
    state.sourceFile,
    state.utilityDeclarations,
  );
  const symbol = checker.getSymbolAtLocation(identifier);
  const importSpecifier = symbol?.declarations?.[0];

  // require one exact compiler-bound named import alias
  if (
    symbol === undefined ||
    (symbol.flags & ts.SymbolFlags.Alias) === 0 ||
    symbol.declarations?.length !== 1 ||
    !ts.isImportSpecifier(importSpecifier) ||
    state.typeImportBindings === undefined
  ) {
    throw new Error(`configured value import identity unsupported at ${pathLabel}`);
  }

  const namedImports = importSpecifier.parent;
  const importClause = namedImports.parent;
  const importDeclaration = importClause.parent;

  // reject type-only, namespace, default, and unconfigured value bindings
  if (
    !ts.isNamedImports(namedImports) ||
    !ts.isImportClause(importClause) ||
    !ts.isImportDeclaration(importDeclaration) ||
    importSpecifier.propertyName !== undefined ||
    importSpecifier.isTypeOnly ||
    importClause.isTypeOnly ||
    !ts.isStringLiteralLike(importDeclaration.moduleSpecifier)
  ) {
    throw new Error(`configured value import identity unsupported at ${pathLabel}`);
  }

  const binding = state.typeImportBindings.get(importDeclaration.moduleSpecifier.text);
  const exportName = (importSpecifier.propertyName ?? importSpecifier.name).text;
  const targetDeclaration = resolvedSymbolDeclaration(checker, symbol);
  const dedicatedProof = binding?.tupleIdentityWrapper ?? binding?.literalStringValue;
  const selectedProof = proofRole === "tupleIdentityWrapper" ? binding?.tupleIdentityWrapper
    : proofRole === "literalStringValue" ? binding?.literalStringValue
    : undefined;

  // never promote an indexed proof-only identity through a value projection
  if (binding?.proofExported.has(exportName)) {
    throw new Error(`proof-only type import used as a value at ${pathLabel}`);
  }

  // bind the local spelling and require the closed graph checker itself
  if (
    binding === undefined ||
    binding.checker !== checker ||
    importSpecifier.name.text !== identifier.text ||
    targetDeclaration === null ||
    (proofRole === "ordinary" ? dedicatedProof !== undefined : selectedProof !== exportName)
  ) {
    throw new Error(`configured value import identity unsupported at ${pathLabel}`);
  }

  const leafMatches = [];

  // prove one unique declaration owner across every graph leaf
  for (const candidatePath of binding.leaves ?? [binding.entry]) {
    const candidateSymbol = configuredLeafExport(
      checker,
      binding.sourceFiles.get(candidatePath),
      exportName,
    );

    // retain only effective exports from exact configured ASTs
    if (candidateSymbol !== null) {
      leafMatches.push(resolvedSymbolDeclaration(checker, candidateSymbol));
    }
  }

  const entrySymbol = configuredLeafExport(
    checker,
    binding.sourceFiles.get(binding.entry),
    exportName,
  );
  const topLevelBindings = findTopLevelValueBindings(state.sourceFile, identifier.text);

  // require one leaf, one effective entry export, and one consumer binding
  if (
    leafMatches.length !== 1 ||
    leafMatches[0] !== targetDeclaration ||
    entrySymbol === null ||
    resolvedSymbolDeclaration(checker, entrySymbol) !== targetDeclaration ||
    topLevelBindings.length !== 1 ||
    topLevelBindings[0] !== importSpecifier
  ) {
    throw new Error(`configured value import provenance unsupported at ${pathLabel}`);
  }

  return { checker, declaration: targetDeclaration, exportName, binding };
}

// convert a TypeScript field name into the same Rust wire field
function rustFieldIdentifier(fieldName, pathLabel) {
  // reject names that need a wire rename
  if (fieldName === "_" || !/^[A-Za-z_][A-Za-z0-9_]*$/u.test(fieldName)) {
    throw new Error(`unsupported property name ${JSON.stringify(fieldName)} at ${pathLabel}`);
  }

  // use raw identifiers for ordinary Rust keywords
  if (RUST_KEYWORDS.has(fieldName)) {
    // reject the four keywords Rust does not permit as raw identifiers
    if (INVALID_RAW_RUST_IDENTIFIERS.has(fieldName)) {
      throw new Error(`Rust field identifier unsupported at ${pathLabel}`);
    }
    return `r#${fieldName}`;
  }

  return fieldName;
}

// convert a wire token or field segment into PascalCase
function pascalCase(value, label) {
  const pieces = value
    .replace(/([a-z0-9])([A-Z])/gu, "$1 $2")
    .split(/[^A-Za-z0-9]+/u)
    .filter((piece) => piece.length > 0);

  // reject values that cannot become a stable Rust identifier
  if (pieces.length === 0) {
    throw new Error(`cannot derive Rust identifier from ${label} ${JSON.stringify(value)}`);
  }

  let result = pieces
    .map((piece) => `${piece[0].toUpperCase()}${piece.slice(1)}`)
    .join("");

  // prefix identifiers that would begin with a digit
  if (/^[0-9]/u.test(result)) {
    result = `Value${result}`;
  }

  // avoid generated variants or types that Rust reserves
  if (RUST_KEYWORDS.has(result)) {
    result = `Value${result}`;
  }

  return result;
}

// derive a unique auxiliary type name from its owning field path
function fieldTypeName(state, suffix = "") {
  // isolate tagged-arm helper names without changing their source field paths
  const typePath = state.typePath ?? state.fieldPath;
  const pathName = typePath.map((segment) => pascalCase(segment, "field segment")).join("");
  return `${state.rootName}${pathName}${suffix}`;
}

// reserve one generated Rust type name
function reserveGeneratedType(state, typeName) {
  // reject collisions with roots or prior auxiliary declarations
  if (state.generatedTypeNames.has(typeName)) {
    throw new Error(`generated Rust type collision ${typeName}`);
  }
  state.generatedTypeNames.add(typeName);
}

// look up and consume metadata for the current field leaf
function consumeFieldMapping(state, acceptedMappings, typeLabel) {
  const fieldPath = state.fieldPath.join(".");
  const mapping = state.fieldMappings.get(fieldPath);

  // allow types that do not require metadata
  if (mapping === undefined) {
    return null;
  }

  // reject metadata attached to the wrong storage type
  if (!acceptedMappings.has(mapping)) {
    throw new Error(`wrong field mapping ${mapping} for ${typeLabel} at ${state.rootName}.${fieldPath}`);
  }

  // reject one mapping consumed by multiple leaves
  if (state.usedFieldMappings.has(fieldPath)) {
    throw new Error(`duplicate field mapping use at ${state.rootName}.${fieldPath}`);
  }
  state.usedFieldMappings.add(fieldPath);
  return mapping;
}

// describe the reviewed cross-language storage boundary
function fieldMappingDoc(mapping) {
  // document fixed records of required unsigned counts
  if (mapping === READONLY_UNSIGNED_COUNTER_RECORD_MAPPING) {
    return "wire rule: readonly-unsigned-counter-record, all named keys required with safe unsigned integer counts";
  }

  // document one decimal source integer grouped by thousands
  if (mapping === GROUPED_UNSIGNED_SAFE_INTEGER_LITERAL_MAPPING) {
    return "wire rule: grouped-unsigned-safe-integer-literal, one source integer grouped by thousands with an exact safe unsigned value";
  }

  // document a closed finite number set with fractional members
  if (mapping === UNSIGNED_DECIMAL_ENUM_MAPPING) {
    return "wire rule: unsigned-decimal-enum, exact nonnegative binary64 literal values";
  }

  // document one exact canonical unsigned integer literal
  if (mapping === UNSIGNED_SAFE_INTEGER_LITERAL_MAPPING) {
    return "wire rule: unsigned-safe-integer-literal, one canonical decimal value in 0..=9007199254740991";
  }

  // document one closed unsigned integer literal set
  if (mapping === UNSIGNED_SAFE_INTEGER_ENUM_MAPPING) {
    return "wire rule: unsigned-safe-integer-enum, canonical decimal values in 0..=9007199254740991";
  }

  // document the exact JavaScript safe unsigned range
  if (mapping === "safe-unsigned-integer") {
    return "wire rule: safe-unsigned-integer, range 0..=9007199254740991";
  }

  // document the exact JavaScript safe signed range
  if (mapping === "safe-integer") {
    return "wire rule: safe-integer, range -9007199254740991..=9007199254740991";
  }

  // document the non-NaN and non-infinite requirement
  if (mapping === "finite-f64") {
    return "wire rule: finite-f64, NaN and infinities excluded";
  }

  // distinguish private runtime storage from finite JSON numbers
  if (mapping === RUNTIME_F64_MAPPING) {
    return "storage rule: runtime-f64, JavaScript number including non-finite values; not a JSON wire type";
  }

  // preserve binary64 values and sequence without fixed cardinality or JSON semantics
  if (mapping === RUNTIME_F64_ARRAY_MAPPING) {
    return "storage rule: runtime-f64-array, ordered binary64 values; not a JSON wire type";
  }

  // retain the exact fixed head keys and five ordered tree vectors
  if (mapping === RAIN_HURDLE_HEAD_RECORD_MAPPING) {
    return "storage rule: rain-hurdle-head-record, four exact head keys and five ordered binary64 arrays per tree";
  }

  // preserve the positions of nullable binary64 samples
  if (mapping === RUNTIME_F64_NULLABLE_ARRAY_MAPPING) {
    return "storage rule: runtime-f64-nullable-array, ordered binary64 slots with explicit nulls; not a JSON wire type";
  }

  // preserve raw keys and the independent nullable slots of every sample series
  if (mapping === ORDERED_RUNTIME_F64_NULLABLE_ARRAY_RECORD_MAPPING) {
    return "storage rule: ordered-runtime-f64-nullable-array-record, ordered UTF-16 keys and ordered nullable binary64 slots; not a JSON wire type";
  }

  // document ordered bytes without specifying a JSON encoding
  if (mapping === UINT8ARRAY_BYTES_MAPPING) {
    return "storage rule: uint8array-bytes, owned Uint8Array bytes; not a JSON wire type";
  }

  // retain binary32 sample storage without adding JSON serialization
  if (mapping === FLOAT32ARRAY_VALUES_MAPPING) {
    return "storage rule: float32array-values, owned binary32 values; not a JSON wire type";
  }

  // record the source template without implying a calendar check
  if (mapping === NUMERIC_PAIR_TEMPLATE_MAPPING) {
    return "storage rule: numeric-pair-template, source uses two number placeholders separated by a hyphen; not a calendar validator";
  }

  // preserve the Date time value without admitting a string arm
  if (mapping === JS_DATE_MAPPING) {
    return "storage rule: js-date, valid epoch milliseconds or invalid Date; not a JSON wire type";
  }

  // retain the date and text alternatives before any caller normalization
  if (mapping === JS_DATE_OR_STRING_MAPPING) {
    return "storage rule: js-date-or-string, valid epoch milliseconds, invalid Date, or raw UTF-16 text";
  }

  // document the exact ordered dynamic-key representation
  if (mapping === ORDERED_FINITE_NUMBER_RECORD_MAPPING) {
    return "wire rule: ordered-finite-f64-record, ordered unique string keys and finite values";
  }

  // document the recursive JSON object representation without promising validation
  if (mapping === ORDERED_JSON_OBJECT_MAPPING) {
    return "wire rule: ordered-json-object, ordered unique UTF-16 keys and ordinary finite JSON numbers; storage does not validate";
  }

  // retain the independent order of recursive JSON objects and their entries
  if (mapping === ORDERED_JSON_OBJECT_ARRAY_MAPPING) {
    return "wire rule: ordered-json-object-array, ordered array of objects with unique UTF-16 keys and ordinary finite JSON numbers; storage does not validate";
  }

  // preserve scalar alternatives without admitting recursive JSON values
  if (mapping === ORDERED_JSON_SCALAR_OBJECT_MAPPING) {
    return "wire rule: ordered-json-scalar-object, ordered unique UTF-16 keys and scalar values; finite numbers only";
  }

  // keep logical key alternatives narrower than general scalar rows
  if (mapping === ORDERED_JSON_KEY_OBJECT_MAPPING) {
    return "wire rule: ordered-json-key-object, ordered unique UTF-16 keys and string, finite number, or null values";
  }

  // preserve exact null on a string-valued source record
  if (mapping === ORDERED_JSON_NULLABLE_STRING_OBJECT_MAPPING) {
    return "wire rule: ordered-json-nullable-string-object, ordered unique UTF-16 keys and nullable UTF-16 strings";
  }

  // retain the independent order of rows and their scalar entries
  if (mapping === ORDERED_JSON_SCALAR_OBJECT_ARRAY_MAPPING) {
    return "wire rule: ordered-json-scalar-object-array, ordered array of scalar objects with UTF-16 keys and finite numbers";
  }

  // retain a fixed set of required string-valued properties
  if (mapping === READONLY_STRING_LITERAL_RECORD_MAPPING) {
    return "wire rule: readonly-string-literal-record, exact string values at declared keys in source order";
  }

  // document the configured-key finite metric object boundary
  if (mapping === FINITE_METRIC_RECORD_MAPPING) {
    return "wire rule: finite-metric-record, ordered unique configured metric keys and finite values";
  }

  // document the direct Partial configured-key boundary
  if (mapping === FINITE_PARTIAL_METRIC_RECORD_MAPPING) {
    return "wire rule: finite-partial-metric-record, ordered unique configured metric keys and finite values";
  }

  // document the exact empty metric object boundary
  if (mapping === EMPTY_METRIC_RECORD_MAPPING) {
    return "wire rule: empty-metric-record, empty JSON object";
  }

  // document the exact empty JSON array boundary
  if (mapping === EMPTY_JSON_ARRAY_MAPPING) {
    return "wire rule: empty-json-array, empty JSON array";
  }

  // document one local const-backed string literal
  if (mapping === CONST_STRING_PROPERTY_MAPPING) {
    return "wire rule: const-string-property, exact local const string value";
  }

  // document one local const-backed string or finite binary64 literal
  if (mapping === CONST_SCALAR_PROPERTY_MAPPING) {
    return "wire rule: const-scalar-property, exact local const string or finite binary64 value";
  }

  // document one builtin-frozen local const-backed string literal
  if (mapping === FROZEN_CONST_STRING_PROPERTY_MAPPING) {
    return "wire rule: frozen-const-string-property, exact builtin-frozen local const string value";
  }

  // document one exact union of two builtin-frozen local const string literals
  if (mapping === FROZEN_CONST_STRING_PROPERTY_UNION_MAPPING) {
    return "wire rule: frozen-const-string-property-union, two distinct builtin-frozen local const string values";
  }

  // document a closed source identity object without evaluating its initializer
  if (mapping === FROZEN_CONST_LITERAL_OBJECT_MAPPING) {
    return "wire rule: frozen-const-literal-object, exact local strings and unsigned safe integers in source order";
  }

  // describe the fixed registry pairs independently of JSON serialization
  if (mapping === FROZEN_WIND_METRIC_BAND_TUPLE_MAPPING) {
    return "wire rule: frozen-wind-metric-band-tuple, exact ordered leadBand and metric pairs";
  }

  // document one direct local const-backed string literal
  if (mapping === CONST_STRING_MAPPING) {
    return "wire rule: const-string, exact local const string value";
  }

  // document one flattened configured interface extension
  if (mapping === FLAT_INTERFACE_EXTENSION_MAPPING) {
    return "wire rule: flat-interface-extension, flattened configured interface fields";
  }

  // document one required exact null value
  if (mapping === EXACT_NULL_MAPPING) {
    return "wire rule: exact-null, only JSON null";
  }

  // document the shallow pinned readonly inline object boundary
  if (mapping === READONLY_INLINE_OBJECT_MAPPING) {
    return "wire rule: readonly-inline-object, pinned shallow Readonly wrapper over explicit properties";
  }

  // preserve the source array's order and shallow element shape
  if (mapping === READONLY_INLINE_OBJECT_ARRAY_MAPPING) {
    return "wire rule: readonly-inline-object-array, ordered shallow Readonly inline objects";
  }

  return "wire rule: utc-iso, canonical UTC ISO-8601 string";
}

// identify the direct null type syntax without resolving aliases
function isExactNullType(typeNode) {
  return ts.isLiteralTypeNode(typeNode) && typeNode.literal.kind === ts.SyntaxKind.NullKeyword;
}

// extract the value only from a direct two-arm union with one null
function directNullableValue(typeNode) {
  // reject wider unions and non-union expressions
  if (!ts.isUnionTypeNode(typeNode) || typeNode.types.length !== 2) {
    return undefined;
  }
  const [first, second] = typeNode.types;
  const firstNull = isExactNullType(first);
  const secondNull = isExactNullType(second);
  // require exactly one null arm
  if (firstNull === secondNull) {
    return undefined;
  }
  return firstNull ? second : first;
}

// preserve unrestricted binary64 only in required scalar storage
function renderRuntimeF64(member, state, pathLabel) {
  const typeNode = member.type;
  const nullable = directNullableValue(typeNode)?.kind === ts.SyntaxKind.NumberKeyword;

  // exclude aliases, collections, optional fields, and widened unions
  if (member.questionToken !== undefined || (typeNode.kind !== ts.SyntaxKind.NumberKeyword && !nullable)) {
    throw new Error(`runtime-f64 requires a required direct number or number | null field at ${pathLabel}`);
  }

  consumeFieldMapping(state, new Set([RUNTIME_F64_MAPPING]), "runtime number");

  // keep explicit null independent of the number value
  if (nullable) {
    state.needsNullable.value = true;
    return "Nullable<f64>";
  }

  return "f64";
}

// recognize only one direct readonly array of primitive numbers
function isReadonlyNumberArray(typeNode) {
  return ts.isTypeOperatorNode(typeNode) && typeNode.operator === ts.SyntaxKind.ReadonlyKeyword &&
    ts.isArrayTypeNode(typeNode.type) && typeNode.type.elementType.kind === ts.SyntaxKind.NumberKeyword;
}

// retain one required ordered binary64 vector without narrowing its values
function renderRuntimeF64Array(member, state, pathLabel) {
  // keep this opt-in on the exact direct source array
  if (member.questionToken !== undefined || !isReadonlyNumberArray(member.type)) {
    throw new Error(`runtime-f64-array requires a required direct readonly number[] field at ${pathLabel}`);
  }
  consumeFieldMapping(state, new Set([RUNTIME_F64_ARRAY_MAPPING]), "runtime number array");
  return "Vec<f64>";
}

// preserve owned bytes and exact null independently of optional field presence
function renderUint8ArrayBytes(sourceType, state, pathLabel) {
  const nullableValue = directNullableValue(sourceType);
  const typeNode = nullableValue ?? sourceType;

  // restrict the mapped type to one direct builtin reference
  if (
    !ts.isTypeReferenceNode(typeNode) ||
    !ts.isIdentifier(typeNode.typeName) ||
    typeNode.typeName.text !== "Uint8Array" ||
    typeNode.typeArguments !== undefined
  ) {
    throw new Error(`uint8array-bytes requires a direct Uint8Array or Uint8Array | null field at ${pathLabel}`);
  }

  provePinnedTypedArray(typeNode, state, pathLabel, UINT8ARRAY_BYTES_MAPPING, "Uint8Array");
  consumeFieldMapping(state, new Set([UINT8ARRAY_BYTES_MAPPING]), "byte array");

  // distinguish explicit null from an empty owned vector
  if (nullableValue !== undefined) {
    state.needsNullable.value = true;
    return "Nullable<Vec<u8>>";
  }
  return "Vec<u8>";
}

// bind one direct typed-array reference to the pinned standard library interface
function provePinnedTypedArray(typeNode, state, pathLabel, mapping, name) {
  const library = state.utilityDeclarations;

  // use only the isolated pinned standard-library binding
  if (library === undefined || !ts.isExternalModule(state.sourceFile) || hasGlobalAugmentation(state.sourceFile)) {
    throw new Error(`${mapping} builtin identity unsupported at ${pathLabel}`);
  }

  const checker = getSourceChecker(state.checkers, state.sourcePath, state.sourceFile, library);
  const symbol = checker.getSymbolAtLocation(typeNode.typeName);
  const declarations = symbol?.declarations;

  // require the exact interface and permit only its pinned merged value declaration
  if (
    symbol === undefined ||
    (symbol.flags & ts.SymbolFlags.Alias) !== 0 ||
    !declarations?.includes(library.declarations[name]) ||
    declarations.some(
      // reject any additional declaration outside the exact pinned library
      (declaration) => declaration !== library.declarations[name] &&
        (declaration !== symbol.valueDeclaration || declaration.getSourceFile() !== library.sourceFile),
    )
  ) {
    throw new Error(`${mapping} builtin identity unsupported at ${pathLabel}`);
  }
}

// retain a required owned binary32 feature vector without widening it
function renderFloat32ArrayValues(member, state, pathLabel) {
  const typeNode = member.type;

  // require the original direct required Float32Array type
  if (member.questionToken !== undefined || !ts.isTypeReferenceNode(typeNode) ||
    !ts.isIdentifier(typeNode.typeName) || typeNode.typeName.text !== "Float32Array" ||
    typeNode.typeArguments !== undefined) {
    throw new Error(`float32array-values requires a required direct Float32Array field at ${pathLabel}`);
  }
  provePinnedTypedArray(typeNode, state, pathLabel, FLOAT32ARRAY_VALUES_MAPPING, "Float32Array");
  consumeFieldMapping(state, new Set([FLOAT32ARRAY_VALUES_MAPPING]), "binary32 array");
  return "Vec<f32>";
}

// preserve the original numeric template as text without enforcing a calendar
function renderNumericPairTemplate(member, state, pathLabel) {
  const typeNode = member.type;

  // require exactly two direct number placeholders separated by one hyphen
  if (member.questionToken !== undefined || !ts.isTemplateLiteralTypeNode(typeNode) ||
    (typeNode.head.rawText ?? typeNode.head.text) !== "" || typeNode.templateSpans.length !== 2 ||
    typeNode.templateSpans[0].type.kind !== ts.SyntaxKind.NumberKeyword ||
    typeNode.templateSpans[0].literal.kind !== ts.SyntaxKind.TemplateMiddle ||
    (typeNode.templateSpans[0].literal.rawText ?? typeNode.templateSpans[0].literal.text) !== "-" ||
    typeNode.templateSpans[1].type.kind !== ts.SyntaxKind.NumberKeyword ||
    typeNode.templateSpans[1].literal.kind !== ts.SyntaxKind.TemplateTail ||
    (typeNode.templateSpans[1].literal.rawText ?? typeNode.templateSpans[1].literal.text) !== "") {
    throw new Error(`numeric-pair-template requires a required direct two-number hyphen template at ${pathLabel}`);
  }
  consumeFieldMapping(state, new Set([NUMERIC_PAIR_TEMPLATE_MAPPING]), "numeric pair template");
  return "String";
}

// admit only a direct unparameterized Date type reference
function isDirectDateType(typeNode) {
  return typeNode !== undefined &&
    ts.isTypeReferenceNode(typeNode) &&
    ts.isIdentifier(typeNode.typeName) &&
    typeNode.typeName.text === "Date" &&
    typeNode.typeArguments === undefined;
}

// prove Date identity inside the exact pinned compiler program
function provePinnedDate(date, state, pathLabel, mapping) {
  const library = state.utilityDeclarations;

  // keep global identity out of script or module augmentation scope
  if (library === undefined || !ts.isExternalModule(state.sourceFile) || hasGlobalAugmentation(state.sourceFile)) {
    throw new Error(mapping + " builtin identity unsupported at " + pathLabel);
  }

  const checker = getSourceChecker(state.checkers, state.sourcePath, state.sourceFile, library);
  const symbol = checker.getSymbolAtLocation(date.typeName);
  const declarations = symbol?.declarations;
  const dateInterfaces = library.declarations.Date;

  // require both pinned Date interfaces and only the pinned companion value if merged
  if (
    symbol === undefined ||
    (symbol.flags & ts.SymbolFlags.Alias) !== 0 ||
    declarations === undefined ||
    dateInterfaces.length !== 2 ||
    !dateInterfaces.every((declaration) => declarations.includes(declaration)) ||
    declarations.some(
      // do not admit a user-defined contribution to the date type
      (declaration) => !dateInterfaces.includes(declaration) &&
        (declaration !== symbol.valueDeclaration || declaration.getSourceFile() !== library.sourceFile),
    )
  ) {
    throw new Error(mapping + " builtin identity unsupported at " + pathLabel);
  }
}

// preserve only the original Date value of one required field
function renderJsDate(member, state, pathLabel) {
  // exclude all optional, alias, union, and collection shapes
  if (member.questionToken !== undefined || !isDirectDateType(member.type)) {
    throw new Error("js-date requires a required direct Date field at " + pathLabel);
  }
  provePinnedDate(member.type, state, pathLabel, JS_DATE_MAPPING);
  consumeFieldMapping(state, new Set([JS_DATE_MAPPING]), "Date");
  state.needsJsDate.value = true;
  return "JsDate";
}

// distinguish a builtin date from original text and optional explicit null
function renderJsDateOrString(member, state, pathLabel) {
  const typeNode = member.type;
  const arms = ts.isUnionTypeNode(typeNode) ? [...typeNode.types] : [];
  const values = arms.filter((arm) => !isExactNullType(arm));
  const date = values.find((arm) => arm.kind !== ts.SyntaxKind.StringKeyword);
  const nullable = arms.length === 3;

  // admit only direct builtin Date, primitive string, and one possible null arm
  if (
    member.questionToken !== undefined ||
    (arms.length !== 2 && !nullable) ||
    values.length !== 2 ||
    !values.some((arm) => arm.kind === ts.SyntaxKind.StringKeyword) ||
    !isDirectDateType(date)
  ) {
    throw new Error("js-date-or-string requires a required direct Date | string field " +
      "with optional null at " + pathLabel);
  }

  provePinnedDate(date, state, pathLabel, JS_DATE_OR_STRING_MAPPING);

  consumeFieldMapping(state, new Set([JS_DATE_OR_STRING_MAPPING]), "Date or string");
  state.needsJsDateOrString.value = true;

  // keep a missing database timestamp distinct from an invalid date
  if (nullable) {
    state.needsNullable.value = true;
    return "Nullable<JsDateOrString>";
  }
  return "JsDateOrString";
}

// add one literal marker declaration that cannot hold another wire value
function renderLiteralMarker(state, literalValue, literalKind) {
  const typeName = fieldTypeName(state, "Literal");
  reserveGeneratedType(state, typeName);
  const rustType = literalKind === "string" ? "&'static str" : "bool";
  const rustValue = literalKind === "string" ? rustString(literalValue) : String(literalValue);
  state.auxiliaryDefinitions.push([
    `#[doc = ${rustString(`exact ${literalKind} wire literal: ${String(literalValue)}`)}]`,
    "#[derive(Debug, Clone, Copy, PartialEq, Eq)]",
    `pub struct ${typeName};`,
    `impl ${typeName} {`,
    `    pub const WIRE_VALUE: ${rustType} = ${rustValue};`,
    "}",
  ].join("\n"));
  return typeName;
}

// encode the exact javascript binary64 bits
function f64LiteralBits(literalValue) {
  const bytes = Buffer.allocUnsafe(8);
  bytes.writeDoubleBE(literalValue);
  return bytes.readBigUInt64BE().toString(16).padStart(16, "0");
}

// add one exact finite binary64 marker declaration
function renderF64LiteralMarker(state, literalValue) {
  const typeName = fieldTypeName(state, "Literal");
  reserveGeneratedType(state, typeName);
  const bits = f64LiteralBits(literalValue);
  // retain negative zero in the human-readable marker metadata
  const displayValue = Object.is(literalValue, -0) ? "-0" : String(literalValue);
  state.auxiliaryDefinitions.push([
    `#[doc = ${rustString(`exact number wire literal: ${displayValue}`)}]`,
    "#[derive(Debug, Clone, Copy, PartialEq)]",
    `pub struct ${typeName};`,
    `impl ${typeName} {`,
    `    pub const WIRE_VALUE: f64 = f64::from_bits(0x${bits});`,
    "}",
  ].join("\n"));
  return typeName;
}

// add one null marker declaration that cannot hold a non-null wire value
function renderNullMarker(state) {
  const typeName = fieldTypeName(state, "Null");
  reserveGeneratedType(state, typeName);
  state.auxiliaryDefinitions.push([
    "#[doc = \"exact null wire literal\"]",
    "#[derive(Debug, Clone, Copy, PartialEq, Eq)]",
    `pub struct ${typeName};`,
  ].join("\n"));
  return typeName;
}

// render one exact ordered set of string wire values
function renderStringEnumValues(typeName, values, pathLabel) {
  const seenValues = new Set();
  const seenVariants = new Set();
  const variants = [];

  // preserve declaration order and exact wire values
  for (const value of values) {
    // reject duplicate semantic alternatives
    if (seenValues.has(value)) {
      throw new Error(`duplicate string literal ${JSON.stringify(value)} at ${pathLabel}`);
    }
    seenValues.add(value);
    const variant = pascalCase(value, "string literal");

    // reject two wire values that collapse to one Rust variant
    if (seenVariants.has(variant)) {
      throw new Error(`Rust enum variant collision ${variant} at ${pathLabel}`);
    }
    seenVariants.add(variant);
    variants.push({ variant, value });
  }

  const lines = [
    "#[derive(Debug, Clone, Copy, PartialEq, Eq)]",
    `pub enum ${typeName} {`,
  ];

  // emit explicit metadata next to every variant
  for (const { variant, value } of variants) {
    lines.push(`    #[doc = ${rustString(`wire value: ${value}`)}]`);
    lines.push(`    ${variant},`);
  }
  lines.push("}", `impl ${typeName} {`, "    pub const WIRE_VALUES: &'static [(&'static str, Self)] = &[");

  // emit a machine-usable exact wire mapping without a serializer dependency
  for (const { variant, value } of variants) {
    lines.push(`        (${rustString(value)}, Self::${variant}),`);
  }
  lines.push("    ];", "}");
  return lines.join("\n");
}

// render one exact string-literal union
function renderStringEnum(typeName, typeNodes, pathLabel) {
  const values = [];

  // collect only exact string literal alternatives
  for (const typeNode of typeNodes) {
    // reject mixed unions rather than broadening them to String
    if (!ts.isLiteralTypeNode(typeNode) || !ts.isStringLiteralLike(typeNode.literal)) {
      throw new Error(`unsupported string literal union at ${pathLabel}`);
    }
    values.push(typeNode.literal.text);
  }

  return renderStringEnumValues(typeName, values, pathLabel);
}

// identify numeric syntax without accepting its spelling
function isNumericLiteralSyntax(typeNode) {
  // require the literal type wrapper used by TypeScript aliases
  if (!ts.isLiteralTypeNode(typeNode)) {
    return false;
  }

  // recognize ordinary and signed numeric syntax for closed rejection
  return ts.isNumericLiteral(typeNode.literal) ||
    (ts.isPrefixUnaryExpression(typeNode.literal) && ts.isNumericLiteral(typeNode.literal.operand));
}

// validate one canonical unsigned safe-integer token
function parseUnsignedIntegerToken(token, rootName) {
  // accept only canonical unsigned ordinary decimal tokens
  if (!/^(?:0|[1-9][0-9]*)$/u.test(token)) {
    throw new Error(`unsigned decimal integer literal unsupported ${JSON.stringify(token)} at root ${rootName}`);
  }

  const value = Number(token);

  // constrain values to the exact JavaScript safe-integer range
  if (!Number.isSafeInteger(value)) {
    throw new Error(`unsafe integer literal ${JSON.stringify(token)} at root ${rootName}`);
  }
  return value;
}

// validate one ordered set of unsigned safe-integer source entries
function collectUnsignedIntegerVariants(rootName, entries, readToken) {
  const seenValues = new Set();
  const variants = [];

  // validate each entry fully before advancing to the next source member
  for (const entry of entries) {
    const token = readToken(entry);
    const value = parseUnsignedIntegerToken(token, rootName);

    // reject repeated semantic alternatives
    if (seenValues.has(value)) {
      throw new Error(`duplicate numeric literal ${JSON.stringify(token)} at root ${rootName}`);
    }
    seenValues.add(value);
    variants.push({ token, variant: `Value${token}` });
  }
  return variants;
}

// render one ordered set of numeric variants with exact wire values
function renderNumericVariants(rootName, variants, wireType = "u64") {
  const lines = [
    "#[derive(Debug, Clone, Copy, PartialEq, Eq)]",
    `pub enum ${rootName} {`,
  ];

  // emit explicit metadata next to every numeric variant
  for (const { token, variant } of variants) {
    lines.push(`    #[doc = ${rustString(`wire value: ${token}`)}]`);
    lines.push(`    ${variant},`);
  }
  lines.push("}", `impl ${rootName} {`, `    pub const WIRE_VALUES: &'static [(${wireType}, Self)] = &[`);

  // emit a typed exact mapping without a serializer dependency
  for (const { token, variant, expression = token } of variants) {
    lines.push(`        (${expression}, Self::${variant}),`);
  }
  lines.push("    ];", "}");
  return lines.join("\n");
}

// render one root unsigned safe-integer literal union
function renderUnsignedIntegerEnum(rootName, unionNode) {
  return renderNumericVariants(
    rootName,
    collectUnsignedIntegerVariants(rootName, unionNode.types, (typeNode) => {
      // reject mixed unions at their original source position
      if (!isNumericLiteralSyntax(typeNode)) {
        throw new Error(`unsigned decimal integer union unsupported at root ${rootName}`);
      }
      return typeNode.literal.getText();
    }),
  );
}

// render one required direct unsigned safe-integer field union
function renderUnsignedSafeIntegerFieldEnum(typeNode, state, pathLabel) {
  // retain only a direct union instead of evaluating aliases or grouped types
  if (!ts.isUnionTypeNode(typeNode)) {
    throw new Error(
      `unsigned-safe-integer-enum requires a required direct numeric-literal union at ${pathLabel}`,
    );
  }

  consumeFieldMapping(
    state,
    new Set([UNSIGNED_SAFE_INTEGER_ENUM_MAPPING]),
    "unsigned safe integer enum",
  );
  const typeName = fieldTypeName(state, "Enum");
  reserveGeneratedType(state, typeName);
  state.auxiliaryDefinitions.push(renderUnsignedIntegerEnum(typeName, typeNode));
  return typeName;
}

// render one required direct canonical unsigned safe-integer literal
function renderUnsignedSafeIntegerLiteral(typeNode, state, pathLabel) {
  // reject aliases, signed forms, grouping, unions, and nonnumeric literals
  if (!ts.isLiteralTypeNode(typeNode) || !ts.isNumericLiteral(typeNode.literal)) {
    throw new Error(
      `unsigned-safe-integer-literal requires a required direct canonical decimal integer literal at ${pathLabel}`,
    );
  }

  const token = typeNode.literal.getText();
  parseUnsignedIntegerToken(token, state.rootName);
  consumeFieldMapping(
    state,
    new Set([UNSIGNED_SAFE_INTEGER_LITERAL_MAPPING]),
    "unsigned safe integer literal",
  );
  return renderUnsignedIntegerMarker(state, token);
}

// render a direct source integer grouped by thousands
function renderGroupedUnsignedIntegerLiteral(member, state, pathLabel) {
  const error = `grouped-unsigned-safe-integer-literal requires a required direct grouped unsigned safe integer literal at ${pathLabel}`;
  const typeNode = member.type;

  // reject optional or indirect syntax
  if (member.questionToken !== undefined || !ts.isLiteralTypeNode(typeNode) || !ts.isNumericLiteral(typeNode.literal)) {
    throw new Error(error);
  }

  const token = typeNode.literal.getText();
  const decimal = token.replaceAll("_", "");
  // keep only canonical thousand groups in the safe integer range
  if (!/^[1-9][0-9]{0,2}(?:_[0-9]{3})+$/u.test(token) || !Number.isSafeInteger(Number(decimal))) {
    throw new Error(error);
  }

  consumeFieldMapping(state, new Set([GROUPED_UNSIGNED_SAFE_INTEGER_LITERAL_MAPPING]), "grouped unsigned integer literal");
  return renderUnsignedIntegerMarker(state, decimal);
}

// render a direct decimal union without widening it to f64
function renderUnsignedDecimalEnum(member, state, pathLabel) {
  const error = `unsigned-decimal-enum requires a required direct canonical unsigned decimal union with a fractional member at ${pathLabel}`;
  const typeNode = member.type;

  // reject optional fields and indirect union syntax
  if (member.questionToken !== undefined || !ts.isUnionTypeNode(typeNode)) {
    throw new Error(error);
  }

  const seen = new Set();
  const variants = [];
  let hasFraction = false;

  // retain the original literal order
  for (const entry of typeNode.types) {
    // reject nonnumeric members and signs before reading the token
    if (!ts.isLiteralTypeNode(entry) || !ts.isNumericLiteral(entry.literal)) {
      throw new Error(error);
    }

    const token = entry.literal.getText();
    const value = Number(token);
    // reject alternate spelling, lossy decimals and repeated values
    if (
      !/^(?:0|[1-9][0-9]*)(?:\.[0-9]+)?$/u.test(token) ||
      !Number.isFinite(value) || value > Number.MAX_SAFE_INTEGER || String(value) !== token ||
      seen.has(value)
    ) {
      throw new Error(error);
    }
    seen.add(value);
    hasFraction ||= !Number.isInteger(value);
    variants.push({
      token,
      variant: `Value${token.replace(".", "Point")}`,
      expression: `f64::from_bits(0x${f64LiteralBits(value)})`,
    });
  }

  // keep all-integer unions on their existing integer mapping
  if (!hasFraction) {
    throw new Error(error);
  }

  consumeFieldMapping(state, new Set([UNSIGNED_DECIMAL_ENUM_MAPPING]), "unsigned decimal enum");
  const typeName = fieldTypeName(state, "Enum");
  reserveGeneratedType(state, typeName);
  state.auxiliaryDefinitions.push(renderNumericVariants(typeName, variants, "f64"));
  return typeName;
}

// emit one exact numeric marker shared by type and const literal forms
function renderUnsignedIntegerMarker(state, token) {
  const typeName = fieldTypeName(state, "Literal");
  reserveGeneratedType(state, typeName);
  state.auxiliaryDefinitions.push([
    `#[doc = ${rustString(`exact unsigned integer wire literal: ${token}`)}]`,
    "#[derive(Debug, Clone, Copy, PartialEq, Eq)]",
    `pub struct ${typeName};`,
    `impl ${typeName} {`,
    `    pub const WIRE_VALUE: u64 = ${token};`,
    "}",
  ].join("\n"));
  return typeName;
}

// render a homogeneous string-literal field union
function renderStringFieldEnum(typeNodes, state, pathLabel) {
  consumeFieldMapping(state, new Set(), "string literal union");
  const typeName = fieldTypeName(state, "Enum");
  reserveGeneratedType(state, typeName);
  state.auxiliaryDefinitions.push(renderStringEnum(typeName, typeNodes, pathLabel));
  return typeName;
}

// render a named tuple member after rejecting optional and rest semantics
function unwrapTupleElement(typeNode, pathLabel) {
  // preserve a named tuple member's underlying type
  if (ts.isNamedTupleMember(typeNode)) {
    // reject optional and rest tuple slots
    if (typeNode.questionToken !== undefined || typeNode.dotDotDotToken !== undefined) {
      throw new Error(`optional or rest tuple element unsupported at ${pathLabel}`);
    }
    return typeNode.type;
  }

  // reject anonymous optional and rest tuple slots
  if (ts.isOptionalTypeNode(typeNode) || ts.isRestTypeNode(typeNode)) {
    throw new Error(`optional or rest tuple element unsupported at ${pathLabel}`);
  }

  return typeNode;
}

// render a readonly tuple as a Rust tuple
function renderTupleType(typeNode, state) {
  const types = [];

  // retain tuple order and fixed arity
  for (const [index, element] of typeNode.elements.entries()) {
    const unwrapped = unwrapTupleElement(element, `${state.rootName}.${state.fieldPath.join(".")}[${index}]`);
    types.push(renderRustType(unwrapped, state));
  }

  // retain Rust's required comma for a one-element tuple
  if (types.length === 1) {
    return `(${types[0]},)`;
  }
  return `(${types.join(", ")})`;
}

// prove and render one ordered finite-number record utility field
function renderOrderedFiniteNumberRecord(typeNode, state, pathLabel) {
  // require the direct outer Readonly utility syntax
  if (
    !ts.isTypeReferenceNode(typeNode) ||
    !ts.isIdentifier(typeNode.typeName) ||
    typeNode.typeName.text !== "Readonly" ||
    typeNode.typeArguments?.length !== 1
  ) {
    throw new Error(
      `ordered finite number record requires Readonly<Record<string, number>> at ${pathLabel}`,
    );
  }

  const recordType = typeNode.typeArguments[0];

  // require the direct inner Record with exact primitive arguments
  if (
    !ts.isTypeReferenceNode(recordType) ||
    !ts.isIdentifier(recordType.typeName) ||
    recordType.typeName.text !== "Record" ||
    recordType.typeArguments?.length !== 2 ||
    recordType.typeArguments[0].kind !== ts.SyntaxKind.StringKeyword ||
    recordType.typeArguments[1].kind !== ts.SyntaxKind.NumberKeyword
  ) {
    throw new Error(
      `ordered finite number record requires Readonly<Record<string, number>> at ${pathLabel}`,
    );
  }

  // require isolated module scope and reject source-owned global merging
  if (
    state.utilityDeclarations === undefined ||
    !ts.isExternalModule(state.sourceFile) ||
    hasGlobalAugmentation(state.sourceFile)
  ) {
    throw new Error(`ordered finite number record utility identity unsupported at ${pathLabel}`);
  }

  const checker = getSourceChecker(
    state.checkers,
    state.sourcePath,
    state.sourceFile,
    state.utilityDeclarations,
  );
  const readonlySymbol = checker.getSymbolAtLocation(typeNode.typeName);
  const recordSymbol = checker.getSymbolAtLocation(recordType.typeName);
  const readonlyDeclarations = readonlySymbol?.declarations;
  const recordDeclarations = recordSymbol?.declarations;

  // require both names to resolve only to the pinned TypeScript aliases
  if (
    readonlyDeclarations?.length !== 1 ||
    readonlyDeclarations[0] !== state.utilityDeclarations.declarations.Readonly ||
    recordDeclarations?.length !== 1 ||
    recordDeclarations[0] !== state.utilityDeclarations.declarations.Record
  ) {
    throw new Error(`ordered finite number record utility identity unsupported at ${pathLabel}`);
  }

  consumeFieldMapping(
    state,
    new Set([ORDERED_FINITE_NUMBER_RECORD_MAPPING]),
    "ordered finite number record",
  );
  state.needsOrderedFiniteNumberRecord.value = true;
  return "OrderedFiniteNumberRecord";
}

// resolve one configured recursive JSON value root for an ordered object field
function resolveOrderedJsonValueRoot(typeNode, state, pathLabel) {
  // admit only one argument-free named reference
  if (
    !ts.isTypeReferenceNode(typeNode) ||
    !ts.isIdentifier(typeNode.typeName) ||
    typeNode.typeArguments !== undefined
  ) {
    throw new Error(`ordered-json-object value root unsupported at ${pathLabel}`);
  }

  const checker = getSourceChecker(
    state.checkers,
    state.sourcePath,
    state.sourceFile,
    state.utilityDeclarations,
  );
  const symbol = checker.getSymbolAtLocation(typeNode.typeName);
  let configured;
  let binding;
  let exportName;

  // reuse only one already-proven type-only import edge
  if (symbol !== undefined && (symbol.flags & ts.SymbolFlags.Alias) !== 0) {
    try {
      ({ binding, configured, exportName } = resolveApprovedTypeImport(
        typeNode.typeName,
        state,
        pathLabel,
      ));
    } catch {
      throw new Error(`ordered-json-object value root unsupported at ${pathLabel}`);
    }
  } else {
    const declarations = symbol?.declarations;

    // keep a local reference on one exact configured declaration object
    if (
      symbol === undefined ||
      declarations?.length !== 1 ||
      declarations[0].getSourceFile() !== state.sourceFile
    ) {
      throw new Error(`ordered-json-object value root unsupported at ${pathLabel}`);
    }
    configured = state.configuredDeclarations.get(declarations[0]);
  }

  // require the selected configured root to own the reviewed recursive grammar
  if (
    configured === undefined ||
    configured.root.recursiveJson === undefined ||
    configured.root.exportName !== (exportName ?? typeNode.typeName.text)
  ) {
    throw new Error(`ordered-json-object value root unsupported at ${pathLabel}`);
  }

  state.dependencies.get(state.rootIdentity).add(configured.identity);
  const rendered = state.renderConfiguredRoot(configured.identity);

  // consume the import only after its target proves the recursive JSON shape
  if (rendered.kind !== "recursive-json-value") {
    throw new Error(`ordered-json-object value root unsupported at ${pathLabel}`);
  }

  // mark only a successfully rendered imported contract as used
  if (binding !== undefined) {
    binding.used.add(exportName);
  }
  return configured.root.exportName;
}

// return the key and value operands from a proven pinned readonly record
function readPinnedReadonlyRecord(typeNode, state, pathLabel, mapping) {
  const fixedKeys = mapping === READONLY_STRING_LITERAL_RECORD_MAPPING;
  const metricKeys = mapping === READONLY_NULLABLE_METRIC_RECORD;
  const rainHeadKeys = mapping === RAIN_HURDLE_HEAD_RECORD_MAPPING;
  const counterKeys = mapping === READONLY_UNSIGNED_COUNTER_RECORD_MAPPING;
  const shape = metricKeys
    ? "Readonly<Record<Key, number | null>>"
    : rainHeadKeys ? "Readonly<Record<ExactHeadKeys, NativeHead>>"
    : counterKeys ? "a required Readonly<Record<ConfiguredKey, number>>"
    : fixedKeys ? 'Readonly<Record<"field" | ..., string>>' : "Readonly<Record<string, Value>>";

  // require the direct wrapper spelling
  if (
    !ts.isTypeReferenceNode(typeNode) ||
    !ts.isIdentifier(typeNode.typeName) ||
    typeNode.typeName.text !== "Readonly" ||
    typeNode.typeArguments?.length !== 1
  ) {
    throw new Error(`${mapping} requires ${shape} at ${pathLabel}`);
  }
  const recordType = typeNode.typeArguments[0];

  // keep dynamic object keys and value identity explicit
  if (
    !ts.isTypeReferenceNode(recordType) ||
    !ts.isIdentifier(recordType.typeName) ||
    recordType.typeName.text !== "Record" ||
    recordType.typeArguments?.length !== 2 ||
    (!metricKeys && !rainHeadKeys && !counterKeys &&
      recordType.typeArguments[fixedKeys ? 1 : 0].kind !== ts.SyntaxKind.StringKeyword)
  ) {
    throw new Error(`${mapping} requires ${shape} at ${pathLabel}`);
  }

  // prove both utilities against the pinned TypeScript declaration surface
  if (
    state.utilityDeclarations === undefined ||
    !ts.isExternalModule(state.sourceFile) ||
    hasGlobalAugmentation(state.sourceFile)
  ) {
    throw new Error(`${mapping} utility identity unsupported at ${pathLabel}`);
  }
  const checker = getSourceChecker(
    state.checkers,
    state.sourcePath,
    state.sourceFile,
    state.utilityDeclarations,
  );
  const readonlyDeclarations = checker.getSymbolAtLocation(typeNode.typeName)?.declarations;
  const recordDeclarations = checker.getSymbolAtLocation(recordType.typeName)?.declarations;

  // reject source-owned or changed utility aliases
  if (
    readonlyDeclarations?.length !== 1 ||
    readonlyDeclarations[0] !== state.utilityDeclarations.declarations.Readonly ||
    recordDeclarations?.length !== 1 ||
    recordDeclarations[0] !== state.utilityDeclarations.declarations.Record
  ) {
    throw new Error(`${mapping} utility identity unsupported at ${pathLabel}`);
  }

  return recordType.typeArguments;
}

// prove one exact ordered recursive JSON object utility field
function renderOrderedJsonObject(typeNode, state, pathLabel) {
  const nullableValue = directNullableValue(typeNode);
  const [, valueType] = readPinnedReadonlyRecord(
    nullableValue ?? typeNode,
    state,
    pathLabel,
    ORDERED_JSON_OBJECT_MAPPING,
  );
  const valueRoot = resolveOrderedJsonValueRoot(
    valueType,
    state,
    pathLabel,
  );
  consumeFieldMapping(state, new Set([ORDERED_JSON_OBJECT_MAPPING]), "ordered JSON object");
  const rustType = `Vec<(Vec<u16>, ${valueRoot})>`;

  // preserve required null independently of the object value
  if (nullableValue !== undefined) {
    state.needsNullable.value = true;
    return `Nullable<${rustType}>`;
  }

  return rustType;
}

// preserve a required readonly array of exact recursive JSON object records
function renderOrderedJsonObjectArray(member, state, pathLabel) {
  const mapping = ORDERED_JSON_OBJECT_ARRAY_MAPPING;
  const typeNode = member.type;

  // require one present direct readonly array rather than an alias or mutable collection
  if (
    member.questionToken !== undefined ||
    !ts.isTypeOperatorNode(typeNode) ||
    typeNode.operator !== ts.SyntaxKind.ReadonlyKeyword ||
    !ts.isArrayTypeNode(typeNode.type)
  ) {
    throw new Error(
      `${mapping} requires readonly Readonly<Record<string, JsonValue>>[] at ${pathLabel}`,
    );
  }

  const [, valueType] = readPinnedReadonlyRecord(
    typeNode.type.elementType,
    state,
    pathLabel,
    mapping,
  );
  const valueRoot = resolveOrderedJsonValueRoot(
    valueType,
    state,
    pathLabel,
  );
  consumeFieldMapping(state, new Set([mapping]), "ordered JSON object array");
  return `Vec<Vec<(Vec<u16>, ${valueRoot})>>`;
}

// preserve direct readonly nullable binary64 series and their pinned record form
function renderRuntimeF64NullableArray(member, state, pathLabel, mapping) {
  const isRecord = mapping === ORDERED_RUNTIME_F64_NULLABLE_ARRAY_RECORD_MAPPING;
  const nullableValue = directNullableValue(member.type);
  const payload = nullableValue ?? member.type;
  const seriesShape = "readonly (number | null)[]";
  const shape = isRecord ? `Readonly<Record<string, ${seriesShape}>>` : seriesShape;
  const error = `${mapping} requires a required direct ${shape} field with optional null at ${pathLabel}`;

  // keep optional field presence outside the admitted storage forms
  if (member.questionToken !== undefined) {
    throw new Error(error);
  }
  const type = isRecord
    ? readPinnedReadonlyRecord(payload, state, pathLabel, mapping)[1]
    : payload;

  // require the direct readonly array operator rather than a mutable array or alias
  if (
    !ts.isTypeOperatorNode(type) ||
    type.operator !== ts.SyntaxKind.ReadonlyKeyword ||
    !ts.isArrayTypeNode(type.type)
  ) {
    throw new Error(error);
  }
  const { elementType } = type.type;

  // require exactly the primitive number and null arms inside the array element
  if (
    !ts.isParenthesizedTypeNode(elementType) ||
    directNullableValue(elementType.type)?.kind !== ts.SyntaxKind.NumberKeyword
  ) {
    throw new Error(error);
  }

  consumeFieldMapping(state, new Set([mapping]), "nullable binary64 series");
  state.needsNullable.value = true;
  const series = "Vec<Nullable<f64>>";
  const storage = isRecord ? `Vec<(Vec<u16>, ${series})>` : series;
  return nullableValue === undefined ? storage : `Nullable<${storage}>`;
}

// keep ordered JSON records limited to the exact configured scalar alternatives
function renderOrderedJsonScalarObject(member, state, pathLabel, mapping) {
  // require present data at this mapping boundary
  if (member.questionToken !== undefined) {
    throw new Error(`${mapping} requires a required field at ${pathLabel}`);
  }
  const isArray = mapping === ORDERED_JSON_SCALAR_OBJECT_ARRAY_MAPPING;
  const hasNumber = mapping !== ORDERED_JSON_NULLABLE_STRING_OBJECT_MAPPING;
  const hasBoolean = isArray || mapping === ORDERED_JSON_SCALAR_OBJECT_MAPPING;
  let record = member.type;

  // admit an ordered array only through direct readonly array syntax
  if (isArray) {
    // reject mutable arrays, utility aliases, and widened outer alternatives
    if (!ts.isTypeOperatorNode(record) ||
      record.operator !== ts.SyntaxKind.ReadonlyKeyword ||
      !ts.isArrayTypeNode(record.type)) {
      throw new Error(`${mapping} requires readonly Readonly<Record<string, Value>>[] at ${pathLabel}`);
    }
    record = record.type.elementType;
  }

  const [, value] = readPinnedReadonlyRecord(
    record,
    state,
    pathLabel,
    mapping,
  );

  const expected = [
    ts.SyntaxKind.StringKeyword,
    ...(hasNumber ? [ts.SyntaxKind.NumberKeyword] : []),
    ...(hasBoolean ? [ts.SyntaxKind.BooleanKeyword] : []),
    ts.SyntaxKind.NullKeyword,
  ];

  // admit only the selected direct distinct scalar alternatives
  const actual = ts.isUnionTypeNode(value) && value.types.length === expected.length
    ? new Set(value.types.map(
      // distinguish the exact null literal from other literal types
      (arm) => isExactNullType(arm) ? ts.SyntaxKind.NullKeyword : arm.kind,
    ))
    : new Set();
  // reject aliases, repeated members, and widened or recursive alternatives
  if (!expected.every(
    // require each scalar alternative exactly once
    (kind) => actual.has(kind),
  )) {
    const shape = ["string", ...(hasNumber ? ["number"] : []), ...(hasBoolean ? ["boolean"] : []), "null"];
    throw new Error(`${mapping} requires exactly ${shape.join(" | ")} at ${pathLabel}`);
  }

  consumeFieldMapping(state, new Set([mapping]), "ordered scalar JSON object");

  // reuse the existing exact nullable storage for string-only values
  if (!hasNumber) {
    state.needsNullable.value = true;
    return "Vec<(Vec<u16>, Nullable<Vec<u16>>)>";
  }

  const valueName = fieldTypeName(state, "Value");
  reserveGeneratedType(state, valueName);
  state.auxiliaryDefinitions.push([
    "#[derive(Debug, Clone, PartialEq)]",
    `pub enum ${valueName} {`,
    "    Null,",
    ...(hasBoolean ? ["    Boolean(bool),"] : []),
    "    Number(f64),",
    "    String(Vec<u16>),",
    "}",
  ].join("\n"));
  const storage = `Vec<(Vec<u16>, ${valueName})>`;
  return isArray ? `Vec<${storage}>` : storage;
}

// bind one direct private type reference to its exact local declaration
function resolvePrivateRainArtifactType(typeNode, state, pathLabel, expectedName) {
  const mapping = RAIN_HURDLE_HEAD_RECORD_MAPPING;

  // require an ordinary identifier reference without type arguments
  if (!ts.isTypeReferenceNode(typeNode) || !ts.isIdentifier(typeNode.typeName) ||
    typeNode.typeName.text !== expectedName || typeNode.typeArguments !== undefined) {
    throw new Error(`${mapping} private type identity unsupported at ${pathLabel}`);
  }
  const checker = getSourceChecker(state.checkers, state.sourcePath, state.sourceFile, state.utilityDeclarations);
  const symbol = checker.getSymbolAtLocation(typeNode.typeName);
  const declaration = symbol?.declarations?.[0];
  const moduleSymbol = checker.getSymbolAtLocation(state.sourceFile);

  // keep the referenced declaration in this module and off its exported type surface
  if (symbol === undefined || (symbol.flags & ts.SymbolFlags.Alias) !== 0 ||
    symbol.declarations?.length !== 1 || declaration.parent !== state.sourceFile ||
    (!ts.isTypeAliasDeclaration(declaration) && !ts.isInterfaceDeclaration(declaration)) ||
    declaration.name.text !== expectedName ||
    declaration.modifiers !== undefined || declaration.typeParameters !== undefined ||
    moduleSymbol === undefined || checker.getExportsOfModule(moduleSymbol).some(
      // include a renamed export of the same declaration
      (entry) => resolvedSymbolDeclaration(checker, entry) === declaration,
    )) {
    throw new Error(`${mapping} private type identity unsupported at ${pathLabel}`);
  }
  return declaration;
}

// retain the source's fixed head record and the original tree vector geometry
function renderRainHurdleHeadRecord(member, state, pathLabel) {
  const mapping = RAIN_HURDLE_HEAD_RECORD_MAPPING;

  // restrict the artifact proof to a direct required heads property
  if (!ts.isInterfaceDeclaration(state.rootDeclaration) || member.parent !== state.rootDeclaration ||
    state.fieldPath.length !== 1 || state.fieldPath[0] !== "heads" || member.questionToken !== undefined ||
    !isReadonly(member)) {
    throw new Error(`${mapping} requires a required direct readonly heads field at ${pathLabel}`);
  }
  const [key, value] = readPinnedReadonlyRecord(member.type, state, pathLabel, mapping);
  const keys = ["0.1", "1.0", "2.5", "amount"];

  // preserve all four required literal keys in declaration order
  if (!ts.isUnionTypeNode(key) || key.types.length !== keys.length ||
    !key.types.every(
      // reject key aliases, additional variants, and changed order
      (entry, index) => ts.isLiteralTypeNode(entry) && ts.isStringLiteral(entry.literal) &&
        entry.literal.text === keys[index],
    )) {
    throw new Error(`${mapping} requires exact ordered head keys at ${pathLabel}`);
  }
  const head = resolvePrivateRainArtifactType(value, state, pathLabel, "NativeHead");
  const fields = ["objective", "baseScore", "trees"];

  // validate the direct private readonly head layout before assigning storage
  if (!ts.isInterfaceDeclaration(head) || head.heritageClauses !== undefined ||
    head.members.length !== fields.length || !head.members.every(
      // bind each required field to its original slot
      (property, index) => ts.isPropertySignature(property) && ts.isIdentifier(property.name) &&
        property.name.text === fields[index] && isReadonly(property) &&
        property.type !== undefined && property.questionToken === undefined,
    )) {
    throw new Error(`${mapping} private head fields unsupported at ${pathLabel}`);
  }
  const [objective, baseScore, trees] = head.members.map(
    // take only the validated direct property types
    (property) => property.type,
  );
  const objectives = ["binary:logistic", "reg:gamma"];

  // retain the exact two head objective literals and direct number and array types
  if (!ts.isUnionTypeNode(objective) || objective.types.length !== objectives.length ||
    !objective.types.every(
      // keep objective order and values distinct
      (entry, index) => ts.isLiteralTypeNode(entry) && ts.isStringLiteral(entry.literal) &&
        entry.literal.text === objectives[index],
    ) || baseScore.kind !== ts.SyntaxKind.NumberKeyword || !ts.isTypeOperatorNode(trees) ||
    trees.operator !== ts.SyntaxKind.ReadonlyKeyword || !ts.isArrayTypeNode(trees.type)) {
    throw new Error(`${mapping} private head types unsupported at ${pathLabel}`);
  }
  const tree = resolvePrivateRainArtifactType(trees.type.elementType, state, pathLabel, "Tree");
  const tuple = ts.isTypeAliasDeclaration(tree) ? tree.type : undefined;

  // keep the exact five-array tuple without inferring inner vector lengths
  if (tuple === undefined || !ts.isTypeOperatorNode(tuple) ||
    tuple.operator !== ts.SyntaxKind.ReadonlyKeyword || !ts.isTupleTypeNode(tuple.type) ||
    tuple.type.elements.length !== 5 || !tuple.type.elements.every(isReadonlyNumberArray)) {
    throw new Error(`${mapping} requires five direct readonly number[] tree members at ${pathLabel}`);
  }

  consumeFieldMapping(state, new Set([mapping]), "rain artifact heads");
  const headsName = fieldTypeName(state);
  const objectiveName = headsName + "ObjectiveEnum";
  const headName = headsName + "NativeHead";
  reserveGeneratedType(state, objectiveName);
  reserveGeneratedType(state, headName);
  reserveGeneratedType(state, headsName);
  state.auxiliaryDefinitions.push(
    renderStringEnumValues(objectiveName, objectives, pathLabel + ".objective"),
    [
      "#[allow(non_snake_case)]",
      "#[derive(Debug, Clone, PartialEq)]",
      `pub struct ${headName} {`,
      `    pub objective: ${objectiveName},`,
      "    pub baseScore: f64,",
      "    pub trees: Vec<[Vec<f64>; 5]>,",
      "}",
    ].join("\n"),
    [
      "#[allow(non_snake_case)]",
      "#[derive(Debug, Clone, PartialEq)]",
      `pub struct ${headsName} {`,
      `    pub atLeast0_1: ${headName},`,
      `    pub atLeast1_0: ${headName},`,
      `    pub atLeast2_5: ${headName},`,
      `    pub amount: ${headName},`,
      "}",
      `impl ${headsName} {`,
      `    pub const WIRE_KEYS: [&'static str; 4] = [${keys.map(rustString).join(", ")}];`,
      "}",
    ].join("\n"),
  );
  return headsName;
}

// render fixed string keys as required fields of one nonrecursive struct
function renderReadonlyStringLiteralRecord(member, state, pathLabel) {
  const mapping = READONLY_STRING_LITERAL_RECORD_MAPPING;

  // preserve a required direct record rather than implicit optionality
  if (member.questionToken !== undefined) {
    throw new Error(`${mapping} requires a required field at ${pathLabel}`);
  }
  const [key] = readPinnedReadonlyRecord(member.type, state, pathLabel, mapping);
  const keys = ts.isUnionTypeNode(key) ? key.types : [key];
  const names = new Set();
  const fields = [];

  // preserve each literal key in the order declared by the source
  for (const entry of keys) {
    // reject widened, repeated, aliased, and nonportable property keys
    if (
      !ts.isLiteralTypeNode(entry) ||
      !ts.isStringLiteral(entry.literal) ||
      entry.literal.text === "_" ||
      !/^[A-Za-z_][A-Za-z0-9_]*$/u.test(entry.literal.text) ||
      names.has(entry.literal.text)
    ) {
      throw new Error(`${mapping} requires distinct direct string-literal keys at ${pathLabel}`);
    }
    const name = entry.literal.text;
    names.add(name);
    fields.push(`    pub ${rustFieldIdentifier(name, `${pathLabel}.${name}`)}: String,`);
  }

  consumeFieldMapping(state, new Set([mapping]), "fixed string record");
  const typeName = fieldTypeName(state);
  reserveGeneratedType(state, typeName);
  state.auxiliaryDefinitions.push([
    "#[allow(non_snake_case)]",
    "#[derive(Debug, Clone, PartialEq)]",
    `pub struct ${typeName} {`,
    ...fields,
    "}",
  ].join("\n"));
  return typeName;
}

// prove one direct configured string enum and retain its ordered wire values
function resolveConfiguredStringEnum(typeNode, state, pathLabel, mapping) {
  // require one argument-free identifier rather than a utility or alias expression
  if (
    !ts.isTypeReferenceNode(typeNode) ||
    !ts.isIdentifier(typeNode.typeName) ||
    typeNode.typeArguments !== undefined
  ) {
    throw new Error(`${mapping} key identity unsupported at ${pathLabel}`);
  }

  const checker = getSourceChecker(
    state.checkers,
    state.sourcePath,
    state.sourceFile,
    state.utilityDeclarations,
  );
  const symbol = checker.getSymbolAtLocation(typeNode.typeName);
  const declarations = symbol?.declarations;

  // admit one already-proven type-only import of a configured enum key
  if (
    (mapping === FINITE_PARTIAL_METRIC_RECORD_MAPPING ||
      mapping === READONLY_NULLABLE_METRIC_RECORD ||
      mapping === DISABLED_REASON_EXTRACT) &&
    symbol !== undefined &&
    (symbol.flags & ts.SymbolFlags.Alias) !== 0
  ) {
    const { binding, configured, exportName } = resolveApprovedTypeImport(
      typeNode.typeName,
      state,
      pathLabel,
    );
    binding.used.add(exportName);
    state.dependencies.get(state.rootIdentity).add(configured.identity);
    const rendered = state.renderConfiguredRoot(configured.identity);

    // keep imported keys on the same nominal finite enum boundary
    if (rendered.kind !== "string-literal-enum") {
      throw new Error(`${mapping} key root must render string-literal-enum at ${pathLabel}`);
    }

    return { name: configured.root.exportName, values: rendered.wireValues };
  }

  // reject local aliases, unresolved names, and merged bindings
  if (
    symbol === undefined ||
    (symbol.flags & ts.SymbolFlags.Alias) !== 0 ||
    declarations?.length !== 1
  ) {
    throw new Error(`${mapping} key identity unsupported at ${pathLabel}`);
  }

  const declaration = declarations[0];
  const configured = state.configuredDeclarations.get(declaration);

  // require the exact configured non-generic declaration in this source module
  if (
    configured === undefined ||
    configured.root.source !== state.sourcePath ||
    declaration.getSourceFile() !== state.sourceFile ||
    declaration.parent !== state.sourceFile ||
    !ts.isExternalModule(state.sourceFile) ||
    (declaration.typeParameters?.length ?? 0) > 0
  ) {
    throw new Error(`${mapping} key identity unsupported at ${pathLabel}`);
  }

  state.dependencies.get(state.rootIdentity).add(configured.identity);
  const rendered = state.renderConfiguredRoot(configured.identity);

  // constrain metric keys to one already supported finite string enum root
  if (rendered.kind !== "string-literal-enum") {
    throw new Error(`${mapping} key root must render string-literal-enum at ${pathLabel}`);
  }

  return { name: configured.root.exportName, values: rendered.wireValues };
}

// render every closed counter key as a required unsigned field
function renderReadonlyUnsignedCounterRecord(member, state, pathLabel) {
  const mapping = READONLY_UNSIGNED_COUNTER_RECORD_MAPPING;

  // reject optional storage independently of the key proof
  if (member.questionToken !== undefined) {
    throw new Error(`${mapping} requires a required Readonly<Record<ConfiguredKey, number>> at ${pathLabel}`);
  }
  const [key, value] = readPinnedReadonlyRecord(member.type, state, pathLabel, mapping);

  // keep counters direct rather than allowing nullable or widened values
  if (value.kind !== ts.SyntaxKind.NumberKeyword) {
    throw new Error(`${mapping} requires a required Readonly<Record<ConfiguredKey, number>> at ${pathLabel}`);
  }

  const { values } = resolveConfiguredStringEnum(key, state, pathLabel, mapping);
  const fields = values.map(
    // preserve exact names and the configured key declaration order
    (name) => `    pub ${rustFieldIdentifier(name, `${pathLabel}.${name}`)}: u64,`,
  );
  consumeFieldMapping(state, new Set([mapping]), "fixed unsigned counter record");
  const typeName = fieldTypeName(state);
  reserveGeneratedType(state, typeName);
  state.auxiliaryDefinitions.push([
    "#[allow(non_snake_case)]",
    "#[derive(Debug, Clone, Copy, PartialEq, Eq)]",
    `pub struct ${typeName} {`,
    ...fields,
    "}",
  ].join("\n"));
  return typeName;
}

// preserve an exact source subset of the pinned loader reason enum
function renderDisabledReasonExtract(typeNode, state, pathLabel) {
  const mapping = DISABLED_REASON_EXTRACT;

  // require the direct utility rather than expanded or inferred reason types
  if (
    !ts.isTypeReferenceNode(typeNode) ||
    !ts.isIdentifier(typeNode.typeName) ||
    typeNode.typeName.text !== "Extract" ||
    typeNode.typeArguments?.length !== 2
  ) {
    throw new Error(mapping + " requires Extract<ConfiguredEnum, string literals> at " + pathLabel);
  }

  const library = state.utilityDeclarations;

  // resolve the utility in the exact same program as the source arm
  if (library === undefined || !ts.isExternalModule(state.sourceFile) || hasGlobalAugmentation(state.sourceFile)) {
    throw new Error(mapping + " utility identity unsupported at " + pathLabel);
  }
  const checker = getSourceChecker(state.checkers, state.sourcePath, state.sourceFile, library);
  const utility = checker.getSymbolAtLocation(typeNode.typeName);

  // reject a local shadow or any other utility declaration
  if (
    utility === undefined ||
    (utility.flags & ts.SymbolFlags.Alias) !== 0 ||
    utility.declarations?.length !== 1 ||
    utility.declarations[0] !== library.declarations.Extract
  ) {
    throw new Error(mapping + " utility identity unsupported at " + pathLabel);
  }

  const [source, subset] = typeNode.typeArguments;
  const { name, values } = resolveConfiguredStringEnum(source, state, pathLabel, mapping);

  // bind the subset to one configured enum rather than structural assignability
  if (name !== state.disabledReasonExtract || !Array.isArray(values)) {
    throw new Error(mapping + " enum root does not match the configured reason at " + pathLabel);
  }
  const declarations = ts.isUnionTypeNode(subset) ? subset.types : [subset];
  const allowed = new Set(values);
  const reasons = new Set();

  // keep the exact subset order while rejecting aliases, duplicates, or external reasons
  for (const declaration of declarations) {
    if (
      !ts.isLiteralTypeNode(declaration) ||
      !ts.isStringLiteral(declaration.literal) ||
      !allowed.has(declaration.literal.text) ||
      reasons.has(declaration.literal.text)
    ) {
      throw new Error(mapping + " reason subset unsupported at " + pathLabel);
    }
    reasons.add(declaration.literal.text);
  }
  const typeName = fieldTypeName(state, "Enum");
  reserveGeneratedType(state, typeName);
  state.auxiliaryDefinitions.push(renderStringEnumValues(typeName, [...reasons], pathLabel));
  return typeName;
}

// emit every configured metric as a required field with a distinct null value
function renderReadonlyNullableMetricRecordRoot(declaration, state) {
  const mapping = READONLY_NULLABLE_METRIC_RECORD;
  const pathLabel = "root " + state.rootName;

  // admit the named record alias rather than interfaces or other root shapes
  if (!ts.isTypeAliasDeclaration(declaration)) {
    throw new Error(mapping + " requires a type alias at " + pathLabel);
  }
  const [key, value] = readPinnedReadonlyRecord(declaration.type, state, pathLabel, mapping);

  // distinguish required nullable values from optional or wider records
  if (directNullableValue(value)?.kind !== ts.SyntaxKind.NumberKeyword) {
    throw new Error(mapping + " requires Readonly<Record<Key, number | null>> at " + pathLabel);
  }
  const { name, values } = resolveConfiguredStringEnum(key, state, pathLabel, mapping);

  // require the exact configured key name and its finite validated catalog
  if (name !== state.readonlyNullableMetricRecord || !Array.isArray(values) || values.length === 0) {
    throw new Error(mapping + " key root does not match configured record at " + pathLabel);
  }

  // retain each required property in the original key catalog order
  const fields = values.map((metric) => {
    // reject keys that cannot be exact ordinary Rust fields
    if (metric === "_" || !/^[A-Za-z_][A-Za-z0-9_]*$/u.test(metric)) {
      throw new Error(mapping + " key is not a portable identifier at " + pathLabel);
    }
    return "    pub " + rustFieldIdentifier(metric, pathLabel + "." + metric) + ": Nullable<f64>,";
  });

  state.needsNullable.value = true;
  return {
    kind: READONLY_NULLABLE_METRIC_RECORD,
    definition: [
      "#[doc = \"storage rule: readonly-nullable-metric-record, all catalog keys required; binary64 values or explicit null\"]",
      "#[allow(non_snake_case)]",
      "#[derive(Debug, Clone, PartialEq)]",
      "pub struct " + state.rootName + " {",
      ...fields,
      "}",
    ].join("\n"),
  };
}

// prove one exact configured-key metric record and emit its field nominal
function renderMetricRecord(typeNode, state, pathLabel, mapping) {
  const isDirectPartial = mapping === FINITE_PARTIAL_METRIC_RECORD_MAPPING;
  const expectedValueKind = mapping === FINITE_METRIC_RECORD_MAPPING || isDirectPartial
    ? ts.SyntaxKind.NumberKeyword
    : ts.SyntaxKind.NeverKeyword;
  const expectedValueText = expectedValueKind === ts.SyntaxKind.NumberKeyword ? "number" : "never";
  const expectedShape = isDirectPartial
    ? `Partial<Record<Key, ${expectedValueText}>>`
    : `Readonly<Partial<Record<Key, ${expectedValueText}>>>`;
  let partialType;

  // select only the exact reviewed outer utility spelling
  if (isDirectPartial) {
    partialType = typeNode;
  } else {
    // retain the existing direct Readonly utility layer
    if (
      !ts.isTypeReferenceNode(typeNode) ||
      !ts.isIdentifier(typeNode.typeName) ||
      typeNode.typeName.text !== "Readonly" ||
      typeNode.typeArguments?.length !== 1
    ) {
      throw new Error(`${mapping} requires ${expectedShape} at ${pathLabel}`);
    }
    partialType = typeNode.typeArguments[0];
  }

  // require the exact direct Partial utility layer
  if (
    !ts.isTypeReferenceNode(partialType) ||
    !ts.isIdentifier(partialType.typeName) ||
    partialType.typeName.text !== "Partial" ||
    partialType.typeArguments?.length !== 1
  ) {
    throw new Error(`${mapping} requires ${expectedShape} at ${pathLabel}`);
  }

  const recordType = partialType.typeArguments[0];

  // require the exact direct Record utility and value primitive
  if (
    !ts.isTypeReferenceNode(recordType) ||
    !ts.isIdentifier(recordType.typeName) ||
    recordType.typeName.text !== "Record" ||
    recordType.typeArguments?.length !== 2 ||
    recordType.typeArguments[1].kind !== expectedValueKind
  ) {
    throw new Error(`${mapping} requires ${expectedShape} at ${pathLabel}`);
  }

  // require isolated module scope and reject source-owned global merging
  if (
    state.utilityDeclarations === undefined ||
    !ts.isExternalModule(state.sourceFile) ||
    hasGlobalAugmentation(state.sourceFile)
  ) {
    throw new Error(`${mapping} utility identity unsupported at ${pathLabel}`);
  }

  const checker = getSourceChecker(
    state.checkers,
    state.sourcePath,
    state.sourceFile,
    state.utilityDeclarations,
  );
  const readonlyDeclarations = isDirectPartial
    ? undefined
    : checker.getSymbolAtLocation(typeNode.typeName)?.declarations;
  const partialDeclarations = checker.getSymbolAtLocation(partialType.typeName)?.declarations;
  const recordDeclarations = checker.getSymbolAtLocation(recordType.typeName)?.declarations;

  // require every utility name to resolve only to its pinned TypeScript alias
  if (
    (!isDirectPartial && (
      readonlyDeclarations?.length !== 1 ||
      readonlyDeclarations[0] !== state.utilityDeclarations.declarations.Readonly
    )) ||
    partialDeclarations?.length !== 1 ||
    partialDeclarations[0] !== state.utilityDeclarations.declarations.Partial ||
    recordDeclarations?.length !== 1 ||
    recordDeclarations[0] !== state.utilityDeclarations.declarations.Record
  ) {
    throw new Error(`${mapping} utility identity unsupported at ${pathLabel}`);
  }

  const { name: keyRustName } = resolveConfiguredStringEnum(
    recordType.typeArguments[0],
    state,
    pathLabel,
    mapping,
  );
  consumeFieldMapping(state, new Set([mapping]), mapping);
  const typeName = fieldTypeName(state, "Record");
  reserveGeneratedType(state, typeName);

  // retain finite present entries in deterministic producer order
  if (mapping === FINITE_METRIC_RECORD_MAPPING || isDirectPartial) {
    state.auxiliaryDefinitions.push([
      `#[doc = ${rustString(`source shape: ${isDirectPartial ? "" : "Readonly<"}Partial<Record<${keyRustName}, number>>${isDirectPartial ? "" : ">"}; wire shape: JSON object`)}]`,
      "#[doc = \"producer validation: ordered unique keys and finite values\"]",
      "#[derive(Debug, Clone, PartialEq)]",
      `pub struct ${typeName}(pub Vec<(${keyRustName}, f64)>);`,
    ].join("\n"));
    return typeName;
  }

  state.auxiliaryDefinitions.push([
    `#[doc = ${rustString(`source shape: Readonly<Partial<Record<${keyRustName}, never>>>; wire shape: empty JSON object`)}]`,
    "#[derive(Debug, Clone, Copy, PartialEq, Eq)]",
    `pub struct ${typeName};`,
  ].join("\n"));
  return typeName;
}

// prove one exact empty readonly tuple and emit its distinct field nominal
function renderEmptyJsonArray(typeNode, state, pathLabel) {
  // require the direct source spelling without admitting other tuple shapes
  if (
    !ts.isTypeOperatorNode(typeNode) ||
    typeNode.operator !== ts.SyntaxKind.ReadonlyKeyword ||
    !ts.isTupleTypeNode(typeNode.type) ||
    typeNode.type.elements.length !== 0
  ) {
    throw new Error(`empty-json-array requires readonly [] at ${pathLabel}`);
  }

  consumeFieldMapping(state, new Set([EMPTY_JSON_ARRAY_MAPPING]), "empty JSON array");
  const typeName = fieldTypeName(state, "EmptyArray");
  reserveGeneratedType(state, typeName);
  state.auxiliaryDefinitions.push([
    "#[doc = \"source shape: readonly []; wire shape: empty JSON array\"]",
    "#[derive(Debug, Clone, Copy, PartialEq, Eq)]",
    `pub struct ${typeName};`,
  ].join("\n"));
  return typeName;
}

// recognize one direct primitive literal admitted inside a frozen policy object
function isFrozenConstPropertyLiteral(expression) {
  // admit ordinary string and numeric literal tokens without evaluation
  if (ts.isStringLiteral(expression) || ts.isNumericLiteral(expression)) {
    return true;
  }

  // admit boolean literal keywords as exact source tokens
  if (
    expression.kind === ts.SyntaxKind.TrueKeyword ||
    expression.kind === ts.SyntaxKind.FalseKeyword
  ) {
    return true;
  }

  // admit only a signed numeric literal for the one compound literal syntax
  return ts.isPrefixUnaryExpression(expression) &&
    (expression.operator === ts.SyntaxKind.PlusToken ||
      expression.operator === ts.SyntaxKind.MinusToken) &&
    ts.isNumericLiteral(expression.operand);
}

// prove one builtin-frozen local const object without evaluating its initializer
function readPinnedFrozenConstObject(queryName, state, pathLabel, label) {
  const initializer = resolveLocalConstCatalogInitializer(queryName, state, label);
  // keep diagnostics local to the exact mapped source
  const sourceError = () => new Error(`${label} source unsupported at root ${state.rootName}`);
  // identify a mismatch in the pinned utility declaration
  const utilityError = () => new Error(`${label} utility identity unsupported at ${pathLabel}`);

  // require the exact direct Object.freeze({ ... } as const) call shape
  if (
    !ts.isCallExpression(initializer) ||
    initializer.questionDotToken !== undefined ||
    initializer.typeArguments !== undefined ||
    initializer.arguments.length !== 1 ||
    !ts.isPropertyAccessExpression(initializer.expression) ||
    initializer.expression.questionDotToken !== undefined ||
    !ts.isIdentifier(initializer.expression.expression) ||
    initializer.expression.expression.text !== "Object" ||
    initializer.expression.name.text !== "freeze"
  ) {
    throw sourceError();
  }

  const argument = initializer.arguments[0];

  // require one direct literal object under an exact const assertion
  if (
    !ts.isAsExpression(argument) ||
    !ts.isObjectLiteralExpression(argument.expression) ||
    !ts.isTypeReferenceNode(argument.type) ||
    !ts.isIdentifier(argument.type.typeName) ||
    argument.type.typeName.text !== "const" ||
    argument.type.typeArguments !== undefined
  ) {
    throw sourceError();
  }

  // require the exact hash-pinned builtin Object value and all freeze overloads
  if (state.utilityDeclarations === undefined) {
    throw utilityError();
  }
  const checker = getSourceChecker(
    state.checkers,
    state.sourcePath,
    state.sourceFile,
    state.utilityDeclarations,
  );
  const objectSymbol = checker.getSymbolAtLocation(initializer.expression.expression);
  const freezeSymbol = checker.getSymbolAtLocation(initializer.expression.name);
  const expectedFreeze = state.utilityDeclarations.declarations.ObjectFreeze;
  const expectedObject = state.utilityDeclarations.declarations.Object;
  const expectedObjectType = state.utilityDeclarations.declarations.ObjectType;

  // reject local values, aliases, augmentations, and changed builtin overload identity
  if (
    objectSymbol === undefined ||
    (objectSymbol.flags & ts.SymbolFlags.Alias) !== 0 ||
    objectSymbol.valueDeclaration !== expectedObject ||
    !objectSymbol.declarations?.includes(expectedObject) ||
    !objectSymbol.declarations.every(
      // permit only the pinned type companion of the builtin value
      (declaration) => declaration === expectedObject || declaration === expectedObjectType,
    ) ||
    freezeSymbol === undefined ||
    (freezeSymbol.flags & ts.SymbolFlags.Alias) !== 0 ||
    freezeSymbol.declarations?.length !== expectedFreeze.length ||
    freezeSymbol.declarations.some((declaration, index) => declaration !== expectedFreeze[index])
  ) {
    throw utilityError();
  }

  const properties = new Map();

  // validate every own property without evaluating the frozen object
  for (const property of argument.expression.properties) {
    // admit only unique ordinary identifier data properties with primitive literals
    if (
      !ts.isPropertyAssignment(property) ||
      !ts.isIdentifier(property.name) ||
      property.name.text === "__proto__" ||
      properties.has(property.name.text) ||
      !isFrozenConstPropertyLiteral(property.initializer)
    ) {
      throw sourceError();
    }
    properties.set(property.name.text, property);
  }
  return { checker, properties };
}

// prove one exact typeof query into a builtin-frozen local const object
function resolveFrozenConstStringProperty(typeNode, state, pathLabel) {
  // require typeof LOCAL_CONST.member with a bare qualifier and identifier member
  if (
    !ts.isTypeQueryNode(typeNode) ||
    typeNode.typeArguments !== undefined ||
    !ts.isQualifiedName(typeNode.exprName) ||
    !ts.isIdentifier(typeNode.exprName.left) ||
    !ts.isIdentifier(typeNode.exprName.right)
  ) {
    throw new Error(
      `frozen const string property requires typeof LOCAL_CONST.member at ${pathLabel}`,
    );
  }

  const { left: queryName, right: selectedProperty } = typeNode.exprName;
  const { checker, properties } = readPinnedFrozenConstObject(
    queryName, state, pathLabel, "frozen const string property",
  );
  const selectedNode = properties.get(selectedProperty.text);
  const selectedSymbol = checker.getSymbolAtLocation(selectedProperty);

  // bind the queried member to the exact selected property declaration and string value
  if (
    selectedNode === undefined ||
    !ts.isStringLiteral(selectedNode.initializer) ||
    selectedSymbol?.declarations?.length !== 1 ||
    selectedSymbol.declarations[0] !== selectedNode
  ) {
    throw new Error(`frozen const string property source unsupported at root ${state.rootName}`);
  }

  return selectedNode.initializer.text;
}

// prove one builtin-frozen local const property and emit its exact string marker
function renderFrozenConstStringProperty(typeNode, state, pathLabel) {
  const value = resolveFrozenConstStringProperty(typeNode, state, pathLabel);
  consumeFieldMapping(
    state,
    new Set([FROZEN_CONST_STRING_PROPERTY_MAPPING]),
    "frozen const string property",
  );
  return renderLiteralMarker(state, value, "string");
}

// prove an exact two-member builtin-frozen string-property union
function renderFrozenConstStringPropertyUnion(typeNode, state, pathLabel) {
  // require the direct two-arm union without grouping or alias expansion
  if (!ts.isUnionTypeNode(typeNode) || typeNode.types.length !== 2) {
    throw new Error(
      `frozen-const-string-property-union requires two direct typeof queries at ${pathLabel}`,
    );
  }

  // preserve the two exact source query values in declaration order
  const values = typeNode.types.map((member) =>
    resolveFrozenConstStringProperty(member, state, pathLabel)
  );

  // reject two source queries that collapse to one wire value
  if (values[0] === values[1]) {
    throw new Error(
      `duplicate frozen const string property value ${JSON.stringify(values[0])} at ${pathLabel}`,
    );
  }

  consumeFieldMapping(
    state,
    new Set([FROZEN_CONST_STRING_PROPERTY_UNION_MAPPING]),
    "frozen const string property union",
  );
  const typeName = fieldTypeName(state, "Enum");
  reserveGeneratedType(state, typeName);
  state.auxiliaryDefinitions.push(renderStringEnumValues(typeName, values, pathLabel));
  return typeName;
}

// emit a complete fixed identity object from a proven local constant type
function renderFrozenConstLiteralObject(member, state, pathLabel) {
  const mapping = FROZEN_CONST_LITERAL_OBJECT_MAPPING;
  const query = member.type;

  // keep the rule on a direct required readonly interface field
  if (
    !ts.isInterfaceDeclaration(state.rootDeclaration) ||
    member.parent !== state.rootDeclaration ||
    state.fieldPath.length !== 1 ||
    member.questionToken !== undefined ||
    !isReadonly(member) ||
    !ts.isTypeQueryNode(query) ||
    query.typeArguments !== undefined ||
    !ts.isIdentifier(query.exprName)
  ) {
    throw new Error(`${mapping} requires a required readonly typeof LOCAL_CONST field at ${pathLabel}`);
  }

  const { properties } = readPinnedFrozenConstObject(query.exprName, state, pathLabel, mapping);

  // require an actual nonempty record of scalar literals
  if (properties.size === 0) {
    throw new Error(`${mapping} source unsupported at root ${state.rootName}`);
  }
  consumeFieldMapping(state, new Set([mapping]), "frozen const literal object");
  const typeName = fieldTypeName(state, "Object");
  reserveGeneratedType(state, typeName);
  const fields = [];

  // preserve every declared key and its exact literal marker in order
  for (const [name, { initializer }] of properties) {
    const fieldState = { ...state, fieldPath: [...state.fieldPath, name] };
    let fieldType;

    // keep the declared text or one canonical safe integer without widening
    if (ts.isStringLiteral(initializer)) {
      fieldType = renderLiteralMarker(fieldState, initializer.text, "string");
    } else if (ts.isNumericLiteral(initializer)) {
      const token = initializer.getText(state.sourceFile);
      parseUnsignedIntegerToken(token, state.rootName);
      fieldType = renderUnsignedIntegerMarker(fieldState, token);
    } else {
      throw new Error(`${mapping} source unsupported at root ${state.rootName}`);
    }

    fields.push(`    pub ${rustFieldIdentifier(name, `${pathLabel}.${name}`)}: ${fieldType},`);
  }
  state.auxiliaryDefinitions.push([
    "#[allow(non_snake_case)]",
    "#[derive(Debug, Clone, PartialEq)]",
    `pub struct ${typeName} {`,
    ...fields,
    "}",
  ].join("\n"));
  return typeName;
}

// prove the annotated identity signature without importing the helper at runtime
function proveTupleIdentityWrapper(call, state, pathLabel, mapping) {
  const { checker, declaration, binding } = resolveApprovedValueImport(
    call.expression, state, pathLabel, "tupleIdentityWrapper",
  );
  const parameter = declaration.parameters?.[0];
  const typeParameter = declaration.typeParameters?.[0];

  // require one direct named sibling export with a single generic identity parameter
  if (
    !ts.isFunctionDeclaration(declaration) || declaration.body === undefined ||
    declaration.getSourceFile() !== binding.sourceFiles.get(binding.entry) ||
    declaration.parent !== declaration.getSourceFile() ||
    !isExported(declaration) || declaration.name?.text !== binding.tupleIdentityWrapper ||
    declaration.asteriskToken !== undefined ||
    ts.getModifiers(declaration)?.some(
      // retain only the direct named export modifier
      (modifier) => modifier.kind !== ts.SyntaxKind.ExportKeyword,
    ) ||
    declaration.parameters.length !== 1 || declaration.typeParameters?.length !== 1 ||
    !ts.isIdentifier(parameter.name) || parameter.name.text === "this" ||
    parameter.dotDotDotToken !== undefined || parameter.initializer !== undefined ||
    parameter.questionToken !== undefined || parameter.modifiers !== undefined ||
    typeParameter.constraint !== undefined || typeParameter.default !== undefined ||
    typeParameter.modifiers !== undefined ||
    checker.getResolvedSignature(call)?.getDeclaration() !== declaration
  ) {
    throw new Error(`${mapping} identity wrapper signature unsupported at ${pathLabel}`);
  }

  const typeSymbol = checker.getSymbolAtLocation(typeParameter.name);

  // bind both type references to the same compiler-resolved type parameter
  if (
    typeSymbol?.declarations?.length !== 1 || typeSymbol.declarations[0] !== typeParameter ||
    ![parameter.type, declaration.type].every(
      // reject widened, qualified, generic, or inferred annotations
      (annotation) => annotation !== undefined && ts.isTypeReferenceNode(annotation) &&
        ts.isIdentifier(annotation.typeName) && annotation.typeArguments === undefined &&
        checker.getSymbolAtLocation(annotation.typeName) === typeSymbol,
    )
  ) {
    throw new Error(`${mapping} identity wrapper signature unsupported at ${pathLabel}`);
  }
  return binding;
}

// read a fixed pair tuple straight from its original literal source
function renderFrozenWindMetricBandTuple(member, state, pathLabel) {
  const mapping = FROZEN_WIND_METRIC_BAND_TUPLE_MAPPING;
  const query = member.type;

  // limit the opt-in to a required top-level readonly typeof property
  if (
    !ts.isInterfaceDeclaration(state.rootDeclaration) ||
    member.parent !== state.rootDeclaration ||
    state.fieldPath.length !== 1 || member.questionToken !== undefined ||
    !isReadonly(member) || !ts.isTypeQueryNode(query) ||
    query.typeArguments !== undefined || !ts.isIdentifier(query.exprName)
  ) {
    throw new Error(`${mapping} requires a required readonly typeof LOCAL_CONST field at ${pathLabel}`);
  }
  const call = resolveLocalConstCatalogInitializer(query.exprName, state, mapping);

  // admit exactly one direct call to the proof-bound wrapper
  if (
    !ts.isCallExpression(call) || call.questionDotToken !== undefined ||
    call.typeArguments !== undefined || !ts.isIdentifier(call.expression) ||
    call.arguments.length !== 1
  ) {
    throw new Error(`${mapping} requires a direct wrapped as-const tuple at ${pathLabel}`);
  }
  const tuple = call.arguments[0];

  // preserve nonempty literal arity without evaluating an initializer
  if (
    !ts.isAsExpression(tuple) || !ts.isArrayLiteralExpression(tuple.expression) ||
    tuple.expression.elements.length === 0 || !ts.isTypeReferenceNode(tuple.type) ||
    !ts.isIdentifier(tuple.type.typeName) || tuple.type.typeName.text !== "const" ||
    tuple.type.typeArguments !== undefined
  ) {
    throw new Error(`${mapping} requires a direct wrapped as-const tuple at ${pathLabel}`);
  }
  const binding = proveTupleIdentityWrapper(call, state, pathLabel, mapping);
  const keys = ["leadBand", "metric"];
  const rows = [];

  // retain every row and both named fields in exactly their original order
  for (const row of tuple.expression.elements) {
    // accept only two direct string-literal data properties
    if (
      !ts.isObjectLiteralExpression(row) || row.properties.length !== keys.length ||
      !row.properties.every(
        // preserve property spelling and order without normalizing the row
        (property, index) => ts.isPropertyAssignment(property) &&
          ts.isIdentifier(property.name) && property.name.text === keys[index] &&
          ts.isStringLiteral(property.initializer),
      )
    ) {
      throw new Error(`${mapping} requires exact leadBand and metric literal rows at ${pathLabel}`);
    }
    rows.push(`        (${rustString(row.properties[0].initializer.text)}, ${rustString(row.properties[1].initializer.text)}),`);
  }

  consumeFieldMapping(state, new Set([mapping]), "frozen wind metric band tuple");
  binding.tupleIdentityWrapperUsed = true;
  const typeName = fieldTypeName(state, "Tuple");
  reserveGeneratedType(state, typeName);
  state.auxiliaryDefinitions.push([
    "#[derive(Debug, Clone, Copy, PartialEq, Eq)]",
    `pub struct ${typeName};`,
    `impl ${typeName} {`,
    `    pub const WIRE_KEYS: [&'static str; 2] = [${rustString(keys[0])}, ${rustString(keys[1])}];`,
    `    pub const WIRE_VALUES: &'static [(&'static str, &'static str); ${rows.length}] = &[`,
    ...rows,
    "    ];",
    "}",
  ].join("\n"));
  return typeName;
}

// prove one local const object property and emit its exact string marker
function renderConstStringProperty(typeNode, state, pathLabel) {
  // require typeof LOCAL_CONST.member with a bare qualifier and identifier member
  if (
    !ts.isTypeQueryNode(typeNode) ||
    typeNode.typeArguments !== undefined ||
    !ts.isQualifiedName(typeNode.exprName) ||
    !ts.isIdentifier(typeNode.exprName.left) ||
    !ts.isIdentifier(typeNode.exprName.right)
  ) {
    throw new Error(`const-string-property requires typeof LOCAL_CONST.member at ${pathLabel}`);
  }

  const queryName = typeNode.exprName.left;
  const selectedProperty = typeNode.exprName.right.text;
  const initializer = resolveLocalConstCatalogInitializer(
    queryName,
    state,
    "const string property",
  );

  // require one direct plain object as const initializer
  if (
    !ts.isAsExpression(initializer) ||
    !ts.isObjectLiteralExpression(initializer.expression) ||
    !ts.isTypeReferenceNode(initializer.type) ||
    !ts.isIdentifier(initializer.type.typeName) ||
    initializer.type.typeName.text !== "const" ||
    initializer.type.typeArguments !== undefined
  ) {
    throw new Error(`const string property source unsupported at root ${state.rootName}`);
  }

  const propertyNames = new Set();
  let selectedValue;

  // validate every own property without evaluating the object
  for (const property of initializer.expression.properties) {
    // allow only unique ordinary identifier data properties with string literals
    if (
      !ts.isPropertyAssignment(property) ||
      !ts.isIdentifier(property.name) ||
      property.name.text === "__proto__" ||
      propertyNames.has(property.name.text) ||
      !ts.isStringLiteral(property.initializer)
    ) {
      throw new Error(`const string property source unsupported at root ${state.rootName}`);
    }
    propertyNames.add(property.name.text);

    // retain only the selected member's exact string token value
    if (property.name.text === selectedProperty) {
      selectedValue = property.initializer.text;
    }
  }

  // require the referenced property to exist in the closed object
  if (selectedValue === undefined) {
    throw new Error(`const string property source unsupported at root ${state.rootName}`);
  }

  consumeFieldMapping(
    state,
    new Set([CONST_STRING_PROPERTY_MAPPING]),
    "const string property",
  );
  return renderLiteralMarker(state, selectedValue, "string");
}

// read one direct string or finite ordinary decimal literal
function readConstScalarPropertyValue(initializer, state) {
  // preserve ordinary string literal text without evaluating it
  if (ts.isStringLiteral(initializer)) {
    return { kind: "string", value: initializer.text };
  }

  let token;

  // admit one unsigned ordinary decimal token directly
  if (ts.isNumericLiteral(initializer)) {
    token = initializer.getText(state.sourceFile);
  // admit only unary minus around one direct ordinary decimal token
  } else if (
    ts.isPrefixUnaryExpression(initializer) &&
    initializer.operator === ts.SyntaxKind.MinusToken &&
    ts.isNumericLiteral(initializer.operand)
  ) {
    token = `-${initializer.operand.getText(state.sourceFile)}`;
  } else {
    throw new Error(`const scalar property source unsupported at root ${state.rootName}`);
  }

  // exclude exponents, separators, prefixed bases, signs, and noncanonical decimals
  if (!/^-?(?:0|[1-9][0-9]*)(?:\.[0-9]+)?$/u.test(token)) {
    throw new Error(`const scalar property source unsupported at root ${state.rootName}`);
  }
  const value = Number(token);

  // exclude overflow and every non-finite JavaScript number
  if (!Number.isFinite(value)) {
    throw new Error(`const scalar property source unsupported at root ${state.rootName}`);
  }
  return { kind: "number", value };
}

// prove one local const object property and emit its exact scalar marker
function renderConstScalarProperty(typeNode, state, pathLabel) {
  // require typeof LOCAL_CONST.member with a bare qualifier and identifier member
  if (
    !ts.isTypeQueryNode(typeNode) ||
    typeNode.typeArguments !== undefined ||
    !ts.isQualifiedName(typeNode.exprName) ||
    !ts.isIdentifier(typeNode.exprName.left) ||
    !ts.isIdentifier(typeNode.exprName.right)
  ) {
    throw new Error(`const-scalar-property requires typeof LOCAL_CONST.member at ${pathLabel}`);
  }

  const queryName = typeNode.exprName.left;
  const selectedProperty = typeNode.exprName.right.text;
  const initializer = resolveLocalConstCatalogInitializer(
    queryName,
    state,
    "const scalar property",
  );

  // require one direct plain object as const initializer
  if (
    !ts.isAsExpression(initializer) ||
    !ts.isObjectLiteralExpression(initializer.expression) ||
    !ts.isTypeReferenceNode(initializer.type) ||
    !ts.isIdentifier(initializer.type.typeName) ||
    initializer.type.typeName.text !== "const" ||
    initializer.type.typeArguments !== undefined
  ) {
    throw new Error(`const scalar property source unsupported at root ${state.rootName}`);
  }

  const propertyNames = new Set();
  let selectedValue;

  // validate every own property under the closed scalar grammar
  for (const property of initializer.expression.properties) {
    // allow only unique ordinary identifier data properties
    if (
      !ts.isPropertyAssignment(property) ||
      !ts.isIdentifier(property.name) ||
      property.name.text === "__proto__" ||
      propertyNames.has(property.name.text)
    ) {
      throw new Error(`const scalar property source unsupported at root ${state.rootName}`);
    }
    propertyNames.add(property.name.text);
    const value = readConstScalarPropertyValue(property.initializer, state);

    // retain only the selected member's exact scalar value
    if (property.name.text === selectedProperty) {
      selectedValue = value;
    }
  }

  // require the referenced property to exist in the closed object
  if (selectedValue === undefined) {
    throw new Error(`const scalar property source unsupported at root ${state.rootName}`);
  }

  consumeFieldMapping(
    state,
    new Set([CONST_SCALAR_PROPERTY_MAPPING]),
    "const scalar property",
  );

  // reuse the existing exact string marker representation
  if (selectedValue.kind === "string") {
    return renderLiteralMarker(state, selectedValue.value, "string");
  }
  return renderF64LiteralMarker(state, selectedValue.value);
}

// prove one direct local const string and emit its exact marker
function renderConstString(typeNode, state, pathLabel) {
  // require a bare typeof query without a qualified member or type arguments
  if (
    !ts.isTypeQueryNode(typeNode) ||
    typeNode.typeArguments !== undefined ||
    !ts.isIdentifier(typeNode.exprName)
  ) {
    throw new Error(`const-string requires typeof LOCAL_CONST at ${pathLabel}`);
  }

  const initializer = resolveLocalConstCatalogInitializer(
    typeNode.exprName,
    state,
    "const string",
  );

  const text = readDirectConstString(initializer);

  // require one direct quoted string in the already proven unannotated const
  if (text === undefined) {
    throw new Error(`const string source unsupported at root ${state.rootName}`);
  }

  consumeFieldMapping(state, new Set([CONST_STRING_MAPPING]), "const string");
  return renderLiteralMarker(state, text, "string");
}

// read one quoted string through at most a direct const assertion
function readDirectConstString(initializer) {
  const literal = ts.isAsExpression(initializer) &&
    ts.isTypeReferenceNode(initializer.type) &&
    ts.isIdentifier(initializer.type.typeName) &&
    initializer.type.typeName.text === "const" &&
    initializer.type.typeArguments === undefined
    ? initializer.expression
    : initializer;
  return ts.isStringLiteral(literal) ? literal.text : undefined;
}

// prove one imported artifact identity string without evaluating its module
function renderActiveImportedConstString(typeNode, state, pathLabel) {
  const label = "active-imported-const-string";

  // admit only a bare typeof query for the configured direct binding
  if (!ts.isTypeQueryNode(typeNode) || typeNode.typeArguments !== undefined ||
    !ts.isIdentifier(typeNode.exprName)) {
    throw new Error(`${label} requires typeof IMPORTED_CONST at ${pathLabel}`);
  }
  const { binding, declaration } = resolveApprovedValueImport(
    typeNode.exprName, state, pathLabel, "literalStringValue",
  );
  const sourceFile = state.sourceFiles.get(binding.entry);
  const statement = declaration.parent?.parent;

  // require the direct named const export in the exact sibling AST
  if (!ts.isVariableDeclaration(declaration) || !ts.isIdentifier(declaration.name) ||
    declaration.name.text !== binding.literalStringValue || statement === undefined ||
    !ts.isVariableStatement(statement) || !isExported(statement) ||
    ts.getModifiers(statement)?.length !== 1 ||
    statement.declarationList.declarations.length !== 1 ||
    statement.parent !== sourceFile || declaration.getSourceFile() !== sourceFile) {
    throw new Error(`${label} requires a direct exported const at ${pathLabel}`);
  }
  const initializer = resolveLocalConstCatalogInitializer(declaration.name, {
    ...state, sourcePath: binding.entry, sourceFile,
  }, label);
  const text = readDirectConstString(initializer);

  // reject widened, computed, and nonstring const initializers
  if (text === undefined) {
    throw new Error(`${label} requires an exact string literal at ${pathLabel}`);
  }
  binding.literalStringValueUsed = true;
  return renderLiteralMarker(state, text, "string");
}

// render one exact flattened interface extension field
function renderFlatInterfaceExtension(typeNode, state, pathLabel) {
  let intersection = typeNode;
  let isNullable = false;

  // allow only one outer null wrapper around the direct intersection
  if (ts.isUnionTypeNode(typeNode)) {
    const nullMembers = typeNode.types.filter((member) =>
      ts.isLiteralTypeNode(member) && member.literal.kind === ts.SyntaxKind.NullKeyword,
    );
    const valueMembers = typeNode.types.filter((member) =>
      !ts.isLiteralTypeNode(member) || member.literal.kind !== ts.SyntaxKind.NullKeyword,
    );

    // reject extra, undefined, or repeated wrapper alternatives
    if (nullMembers.length !== 1 || valueMembers.length !== 1) {
      throw new Error(
        `flat-interface-extension requires ExactConfiguredInterface & { ... } or null union at ${pathLabel}`,
      );
    }
    intersection = valueMembers[0];
    isNullable = true;
  }

  // require the base reference first and one direct inline extension second
  if (
    !ts.isIntersectionTypeNode(intersection) ||
    intersection.types.length !== 2 ||
    !ts.isTypeReferenceNode(intersection.types[0]) ||
    !ts.isIdentifier(intersection.types[0].typeName) ||
    intersection.types[0].typeArguments !== undefined ||
    !ts.isTypeLiteralNode(intersection.types[1]) ||
    intersection.types[1].members.length === 0
  ) {
    throw new Error(
      `flat-interface-extension requires ExactConfiguredInterface & { ... } or null union at ${pathLabel}`,
    );
  }

  const baseReference = intersection.types[0];
  const extension = intersection.types[1];
  const checker = getSourceChecker(
    state.checkers,
    state.sourcePath,
    state.sourceFile,
    state.utilityDeclarations,
  );
  const symbol = checker.getSymbolAtLocation(baseReference.typeName);
  const declarations = symbol?.declarations;

  // require one direct non-alias binding in the current module
  if (
    symbol === undefined ||
    (symbol.flags & ts.SymbolFlags.Alias) !== 0 ||
    declarations?.length !== 1
  ) {
    throw new Error(`flat-interface-extension base identity unsupported at ${pathLabel}`);
  }

  const declaration = declarations[0];
  const configured = state.configuredDeclarations.get(declaration);

  // admit only an exact configured same-source non-generic interface root
  if (
    configured === undefined ||
    configured.root.source !== state.sourcePath ||
    configured.root.exportName !== baseReference.typeName.text ||
    configured.root.localInterface === true ||
    declaration.getSourceFile() !== state.sourceFile ||
    declaration.parent !== state.sourceFile ||
    !ts.isExternalModule(state.sourceFile) ||
    !ts.isInterfaceDeclaration(declaration) ||
    (declaration.typeParameters?.length ?? 0) > 0
  ) {
    throw new Error(`flat-interface-extension base identity unsupported at ${pathLabel}`);
  }

  state.dependencies.get(state.rootIdentity).add(configured.identity);
  const baseRendered = state.renderConfiguredRoot(configured.identity);

  // reuse only the validated flattened interface field descriptors
  if (
    baseRendered.kind !== "readonly-interface" ||
    baseRendered.interfaceFields === undefined ||
    baseRendered.interfaceFieldNames === undefined
  ) {
    throw new Error(`flat-interface-extension base identity unsupported at ${pathLabel}`);
  }

  consumeFieldMapping(
    state,
    new Set([FLAT_INTERFACE_EXTENSION_MAPPING]),
    "flat interface extension",
  );
  const typeName = fieldTypeName(state);
  reserveGeneratedType(state, typeName);
  const ownFields = renderObjectProperties(
    extension.members,
    state,
    baseRendered.interfaceFieldNames,
  );
  state.auxiliaryDefinitions.push([
    "#[allow(non_snake_case)]",
    "#[derive(Debug, Clone, PartialEq)]",
    `pub struct ${typeName} {`,
    ...baseRendered.interfaceFields,
    ...ownFields,
    "}",
  ].join("\n"));

  // preserve the sole outer null alternative independently
  if (isNullable) {
    state.needsNullable.value = true;
    return `Nullable<${typeName}>`;
  }
  return typeName;
}

// prove one direct pinned Readonly wrapper and return its inline object
function readPinnedReadonlyInlineObject(wrapper, state, pathLabel, label, shape) {
  // require the exact direct wrapper and a nonempty inline property set
  if (
    !ts.isTypeReferenceNode(wrapper) ||
    !ts.isIdentifier(wrapper.typeName) ||
    wrapper.typeName.text !== "Readonly" ||
    wrapper.typeArguments?.length !== 1 ||
    !ts.isTypeLiteralNode(wrapper.typeArguments[0]) ||
    wrapper.typeArguments[0].members.length === 0
  ) {
    throw new Error(`${label} requires ${shape} at ${pathLabel}`);
  }

  // require isolated module scope and the pinned compiler utility declaration
  if (
    state.utilityDeclarations === undefined ||
    !ts.isExternalModule(state.sourceFile) ||
    hasGlobalAugmentation(state.sourceFile)
  ) {
    throw new Error(`${label} utility identity unsupported at ${pathLabel}`);
  }

  const checker = getSourceChecker(
    state.checkers,
    state.sourcePath,
    state.sourceFile,
    state.utilityDeclarations,
  );
  const readonlySymbol = checker.getSymbolAtLocation(wrapper.typeName);
  const readonlyDeclarations = readonlySymbol?.declarations;

  // reject aliases, local shadows, and changed compiler declarations
  if (
    readonlySymbol === undefined ||
    (readonlySymbol.flags & ts.SymbolFlags.Alias) !== 0 ||
    readonlyDeclarations?.length !== 1 ||
    readonlyDeclarations[0] !== state.utilityDeclarations.declarations.Readonly
  ) {
    throw new Error(`${label} utility identity unsupported at ${pathLabel}`);
  }

  return wrapper.typeArguments[0];
}

// project readonly only onto the exact inline object's direct members
function renderShallowReadonlyObject(object, state) {
  return renderNestedObject(object, {
    ...state,
    readonlyProjectionMembers: object.members,
  });
}

// render one pinned Readonly object with explicit null when declared
function renderReadonlyInlineObject(typeNode, state, pathLabel) {
  const nullableValue = directNullableValue(typeNode);
  const object = readPinnedReadonlyInlineObject(
    nullableValue ?? typeNode,
    state,
    pathLabel,
    READONLY_INLINE_OBJECT_MAPPING,
    "Readonly<{ ... }> or exact null union",
  );
  consumeFieldMapping(
    state,
    new Set([READONLY_INLINE_OBJECT_MAPPING]),
    "readonly inline object",
  );
  const rustType = renderShallowReadonlyObject(object, state);

  // preserve explicit null independently from the object value
  if (nullableValue !== undefined) {
    state.needsNullable.value = true;
    return `Nullable<${rustType}>`;
  }
  return rustType;
}

// render a required readonly array of exact pinned inline Readonly objects
function renderReadonlyInlineObjectArray(member, state, pathLabel) {
  const typeNode = member.type;

  // admit only the direct readonly array form without optional or nullable storage
  if (
    !ts.isInterfaceDeclaration(state.rootDeclaration) ||
    member.parent !== state.rootDeclaration ||
    state.fieldPath.length !== 1 ||
    member.questionToken !== undefined ||
    !ts.isTypeOperatorNode(typeNode) ||
    typeNode.operator !== ts.SyntaxKind.ReadonlyKeyword ||
    !ts.isArrayTypeNode(typeNode.type)
  ) {
    throw new Error(`readonly-inline-object-array requires a required readonly Readonly<{ ... }>[] field at ${pathLabel}`);
  }
  const object = readPinnedReadonlyInlineObject(
    typeNode.type.elementType,
    state,
    pathLabel,
    READONLY_INLINE_OBJECT_ARRAY_MAPPING,
    "Readonly<{ ... }> elements",
  );
  consumeFieldMapping(
    state,
    new Set([READONLY_INLINE_OBJECT_ARRAY_MAPPING]),
    "readonly inline object array",
  );
  return `Vec<${renderShallowReadonlyObject(object, state)}>`;
}

// prove one direct null literal and emit its existing field marker
function renderExactNull(typeNode, state, pathLabel) {
  // admit no unions or other null-like source spellings
  if (!ts.isLiteralTypeNode(typeNode) || typeNode.literal.kind !== ts.SyntaxKind.NullKeyword) {
    throw new Error(`exact-null requires null literal at ${pathLabel}`);
  }

  consumeFieldMapping(state, new Set([EXACT_NULL_MAPPING]), "exact null");
  return renderNullMarker(state);
}

// render a readonly object's property declarations
function renderObjectProperties(
  members,
  state,
  inheritedFieldNames = new Set(),
  renderedFieldBlocks = undefined,
) {
  const lines = [];
  const fieldNames = new Set(inheritedFieldNames);
  // seed Rust collision checks from the already validated inherited fields
  const rustFieldNames = new Set(
    [...inheritedFieldNames].map((fieldName) =>
      rustFieldIdentifier(fieldName, `${state.rootName}.${fieldName}`)
    ),
  );

  // inspect every object member without erasing methods or indexes
  for (const member of members) {
    // reject index signatures explicitly
    if (ts.isIndexSignatureDeclaration(member)) {
      throw new Error(`index signature unsupported at ${state.rootName}.${state.fieldPath.join(".")}`);
    }

    // allow only data properties in the source-only subset
    if (!ts.isPropertySignature(member)) {
      throw new Error(`object member unsupported at ${state.rootName}.${state.fieldPath.join(".")}`);
    }

    // require explicit readonly outside the exact shallow utility projection
    if (state.readonlyProjectionMembers !== members && !isReadonly(member)) {
      throw new Error(`non-readonly property unsupported at ${state.rootName}`);
    }

    // reject inferred property types
    if (member.type === undefined) {
      throw new Error(`inferred property type unsupported at ${state.rootName}`);
    }

    // admit ordinary identifiers plus explicitly mapped quoted numeric ranges
    if (
      !ts.isIdentifier(member.name) &&
      !(ts.isStringLiteral(member.name) && /^[0-9]+-[0-9]+$/u.test(member.name.text))
    ) {
      throw new Error(`unsupported property name at ${state.rootName}`);
    }

    const fieldName = member.name.text;
    const lineStart = lines.length;
    const fieldPath = [...state.fieldPath, fieldName];
    const pathLabel = `${state.rootName}.${fieldPath.join(".")}`;
    const wireFieldPath = fieldPath.join(".");
    const configuredRustFieldName = state.wireFieldNames.get(wireFieldPath);

    // require every quoted numeric-range property to be selected explicitly
    if (!ts.isIdentifier(member.name) && configuredRustFieldName === undefined) {
      throw new Error(`unsupported property name at ${state.rootName}`);
    }

    // keep wire-name metadata exclusive to the selected quoted property syntax
    if (ts.isIdentifier(member.name) && configuredRustFieldName !== undefined) {
      throw new Error(`wire field mapping requires quoted numeric-range property at ${pathLabel}`);
    }

    // reject duplicate object properties
    if (fieldNames.has(fieldName)) {
      // distinguish an inherited wire-field collision from a local duplicate
      if (inheritedFieldNames.has(fieldName)) {
        throw new Error(`inherited property redeclaration unsupported at ${pathLabel}`);
      }
      throw new Error(`duplicate property ${pathLabel}`);
    }
    fieldNames.add(fieldName);
    const rustFieldName = rustFieldIdentifier(
      configuredRustFieldName ?? fieldName,
      pathLabel,
    );

    // reject wire renames that collide with any inherited or local Rust field
    if (rustFieldNames.has(rustFieldName)) {
      throw new Error(`Rust field name collision ${rustFieldName} at ${pathLabel}`);
    }
    rustFieldNames.add(rustFieldName);

    // consume one mapped quoted wire name at its exact original field path
    if (configuredRustFieldName !== undefined) {
      if (state.usedWireFieldNames.has(wireFieldPath)) {
        throw new Error(`duplicate wire field mapping use at ${pathLabel}`);
      }
      state.usedWireFieldNames.add(wireFieldPath);
    }

    // extend synthetic tagged-arm names separately from metadata field paths
    const childState = {
      ...state,
      fieldPath,
      ...(state.typePath === undefined
        ? {}
        : { typePath: [...state.typePath, fieldName] }),
    };
    const mapping = state.fieldMappings.get(wireFieldPath);
    const projectionPath = wireFieldPath;
    const projection = state.fieldProjections.get(projectionPath);
    let rustType;

    // project the imported string only on the selected active field
    if (state.activeImportedConstMember === member) {
      rustType = renderActiveImportedConstString(member.type, childState, pathLabel);
    // apply the selected utility only to the disabled arm's direct reason
    } else if (state.disabledReasonExtract !== undefined && state.fieldPath.length === 0 && fieldName === "reasonCode") {
      rustType = renderDisabledReasonExtract(member.type, childState, pathLabel);
    // render only the checker-proven field owned by the reviewed generic root
    } else if (state.defaultedListParameterProof?.field === member) {
      consumeFieldMapping(childState, new Set(), "defaulted list parameter field");
      rustType = `Vec<${state.defaultedListParameterProof.name}>`;
    // consume one exact configured projection path before ordinary type rendering
    } else if (projection !== undefined) {
      state.usedFieldProjections.add(projectionPath);
      rustType = renderFieldProjection(member, childState, projection, pathLabel);
    // admit the ordered dynamic-key record only through explicit field metadata
    } else if (mapping === ORDERED_FINITE_NUMBER_RECORD_MAPPING) {
      rustType = renderOrderedFiniteNumberRecord(member.type, childState, pathLabel);
    // preserve one explicitly ordered object of recursive JSON values
    } else if (mapping === ORDERED_JSON_OBJECT_MAPPING) {
      rustType = renderOrderedJsonObject(member.type, childState, pathLabel);
    // preserve a required readonly array of recursive JSON objects
    } else if (mapping === ORDERED_JSON_OBJECT_ARRAY_MAPPING) {
      rustType = renderOrderedJsonObjectArray(member, childState, pathLabel);
    // preserve a JSON object whose entries cannot contain nested values
    } else if (SCALAR_RECORD_MAPPINGS.has(mapping)) {
      rustType = renderOrderedJsonScalarObject(member, childState, pathLabel, mapping);
    // preserve each fixed string literal key as a required struct field
    } else if (mapping === READONLY_STRING_LITERAL_RECORD_MAPPING) {
      rustType = renderReadonlyStringLiteralRecord(member, childState, pathLabel);
    // keep closed named counters in fixed required fields
    } else if (mapping === READONLY_UNSIGNED_COUNTER_RECORD_MAPPING) {
      rustType = renderReadonlyUnsignedCounterRecord(member, childState, pathLabel);
    // keep the portable rain model's exact required head and tree storage
    } else if (mapping === RAIN_HURDLE_HEAD_RECORD_MAPPING) {
      rustType = renderRainHurdleHeadRecord(member, childState, pathLabel);
    // admit configured-key finite records only through their exact mapping
    } else if (mapping === FINITE_METRIC_RECORD_MAPPING) {
      rustType = renderMetricRecord(member.type, childState, pathLabel, mapping);
    // admit the direct Partial configured-key form only through its exact mapping
    } else if (mapping === FINITE_PARTIAL_METRIC_RECORD_MAPPING) {
      rustType = renderMetricRecord(member.type, childState, pathLabel, mapping);
    // admit configured-key empty records only through their exact mapping
    } else if (mapping === EMPTY_METRIC_RECORD_MAPPING) {
      rustType = renderMetricRecord(member.type, childState, pathLabel, mapping);
    // keep an empty JSON array distinct from every ordinary tuple
    } else if (mapping === EMPTY_JSON_ARRAY_MAPPING) {
      rustType = renderEmptyJsonArray(member.type, childState, pathLabel);
    // preserve one exact local const-backed string field
    } else if (mapping === CONST_STRING_PROPERTY_MAPPING) {
      rustType = renderConstStringProperty(member.type, childState, pathLabel);
    // preserve one exact local const-backed string or finite binary64 field
    } else if (mapping === CONST_SCALAR_PROPERTY_MAPPING) {
      rustType = renderConstScalarProperty(member.type, childState, pathLabel);
    // preserve one exact builtin-frozen local const-backed string field
    } else if (mapping === FROZEN_CONST_STRING_PROPERTY_MAPPING) {
      rustType = renderFrozenConstStringProperty(member.type, childState, pathLabel);
    // preserve one direct union of two builtin-frozen local const-backed strings
    } else if (mapping === FROZEN_CONST_STRING_PROPERTY_UNION_MAPPING) {
      rustType = renderFrozenConstStringPropertyUnion(member.type, childState, pathLabel);
    // retain an exact builtin-frozen local source identity object
    } else if (mapping === FROZEN_CONST_LITERAL_OBJECT_MAPPING) {
      rustType = renderFrozenConstLiteralObject(member, childState, pathLabel);
    // keep the fixed wind registry pair sequence on a zero-storage marker
    } else if (mapping === FROZEN_WIND_METRIC_BAND_TUPLE_MAPPING) {
      rustType = renderFrozenWindMetricBandTuple(member, childState, pathLabel);
    // preserve one exact direct local const-backed string field
    } else if (mapping === CONST_STRING_MAPPING) {
      rustType = renderConstString(member.type, childState, pathLabel);
    // flatten one exact configured interface plus an inline extension
    } else if (mapping === FLAT_INTERFACE_EXTENSION_MAPPING) {
      rustType = renderFlatInterfaceExtension(member.type, childState, pathLabel);
    // preserve one exact required null field outside tagged payloads
    } else if (mapping === EXACT_NULL_MAPPING) {
      rustType = renderExactNull(member.type, childState, pathLabel);
    // keep internal runtime numbers distinct from finite wire values
    } else if (mapping === RUNTIME_F64_MAPPING) {
      rustType = renderRuntimeF64(member, childState, pathLabel);
    // preserve an ordered direct array of unrestricted binary64 values
    } else if (mapping === RUNTIME_F64_ARRAY_MAPPING) {
      rustType = renderRuntimeF64Array(member, childState, pathLabel);
    // keep raw sample arrays and per-key arrays distinct from scalar numbers
    } else if (
      mapping === RUNTIME_F64_NULLABLE_ARRAY_MAPPING ||
      mapping === ORDERED_RUNTIME_F64_NULLABLE_ARRAY_RECORD_MAPPING
    ) {
      rustType = renderRuntimeF64NullableArray(member, childState, pathLabel, mapping);
    // preserve raw bytes without making a serialization decision
    } else if (mapping === UINT8ARRAY_BYTES_MAPPING) {
      rustType = renderUint8ArrayBytes(member.type, childState, pathLabel);
    // retain Float32Array feature values in their original binary32 width
    } else if (mapping === FLOAT32ARRAY_VALUES_MAPPING) {
      rustType = renderFloat32ArrayValues(member, childState, pathLabel);
    // retain source model month text without introducing validation
    } else if (mapping === NUMERIC_PAIR_TEMPLATE_MAPPING) {
      rustType = renderNumericPairTemplate(member, childState, pathLabel);
    // preserve the original Date value without broadening to text
    } else if (mapping === JS_DATE_MAPPING) {
      rustType = renderJsDate(member, childState, pathLabel);
    // preserve dates and unparsed text as different storage alternatives
    } else if (mapping === JS_DATE_OR_STRING_MAPPING) {
      rustType = renderJsDateOrString(member, childState, pathLabel);
    // preserve a required source integer with canonical thousand grouping
    } else if (mapping === GROUPED_UNSIGNED_SAFE_INTEGER_LITERAL_MAPPING) {
      rustType = renderGroupedUnsignedIntegerLiteral(member, childState, pathLabel);
    // retain the exact set of unsigned decimals instead of accepting free f64
    } else if (mapping === UNSIGNED_DECIMAL_ENUM_MAPPING) {
      rustType = renderUnsignedDecimalEnum(member, childState, pathLabel);
    // preserve one required exact unsigned integer literal
    } else if (mapping === UNSIGNED_SAFE_INTEGER_LITERAL_MAPPING) {
      // reject optional storage before generating its exact marker type
      if (member.questionToken !== undefined) {
        throw new Error(
          `unsigned-safe-integer-literal requires a required direct canonical decimal integer literal at ${pathLabel}`,
        );
      }
      rustType = renderUnsignedSafeIntegerLiteral(member.type, childState, pathLabel);
    // preserve one required direct unsigned integer literal union
    } else if (mapping === UNSIGNED_SAFE_INTEGER_ENUM_MAPPING) {
      // reject optional storage before generating its closed helper type
      if (member.questionToken !== undefined) {
        throw new Error(
          `unsigned-safe-integer-enum requires a required direct numeric-literal union at ${pathLabel}`,
        );
      }
      rustType = renderUnsignedSafeIntegerFieldEnum(member.type, childState, pathLabel);
    // preserve one pinned readonly object with ordinary outer field presence
    } else if (mapping === READONLY_INLINE_OBJECT_MAPPING) {
      rustType = renderReadonlyInlineObject(member.type, childState, pathLabel);
    // retain ordered elements from the one exact shallow array grammar
    } else if (mapping === READONLY_INLINE_OBJECT_ARRAY_MAPPING) {
      rustType = renderReadonlyInlineObjectArray(member, childState, pathLabel);
    } else {
      rustType = renderRustType(member.type, childState);
    }

    // preserve missing independently from null
    if (member.questionToken !== undefined) {
      state.needsPresence.value = true;
      rustType = `FieldPresence<${rustType}>`;
    }

    // document the explicit representation rule on its field
    if (mapping !== undefined) {
      lines.push(`    #[doc = ${rustString(fieldMappingDoc(mapping))}]`);
    }
    // document the exact configured projection nominal
    if (projection !== undefined) {
      lines.push(
        `    #[doc = ${rustString(`field projection: ${projection.kind} -> ${projection.root}`)}]`,
      );
    }
    // retain the source wire name next to every renamed Rust field
    if (configuredRustFieldName !== undefined) {
      lines.push(`    #[doc = ${rustString(`wire name: ${fieldName}`)}]`);
    }
    lines.push(`    pub ${rustFieldName}: ${rustType},`);

    // retain every annotation with the exact field it describes
    if (renderedFieldBlocks !== undefined) {
      renderedFieldBlocks.push({ name: fieldName, lines: lines.slice(lineStart), rustType });
    }
  }

  return lines;
}

// render one nested readonly object as a named Rust struct
function renderNestedObject(typeNode, state) {
  const typeName = fieldTypeName(state);
  reserveGeneratedType(state, typeName);
  const fieldBlocks = [];
  const fields = renderObjectProperties(typeNode.members, state, new Set(), fieldBlocks);
  const path = state.fieldPath.join(".");

  // keep one root-owned descriptor for this exact AST and field path
  if (
    state.nestedObjectDescriptors.byNode.has(typeNode) ||
    state.nestedObjectDescriptors.byPath.has(path)
  ) {
    throw new Error(`nested object metadata collision at ${state.rootName}.${path}`);
  }
  const descriptor = {
    typeNode,
    path,
    typeName,
    fieldBlocks,
    fieldNames: new Set(fieldBlocks.map((field) => field.name)),
  };
  state.nestedObjectDescriptors.byNode.set(typeNode, descriptor);
  state.nestedObjectDescriptors.byPath.set(path, descriptor);
  state.auxiliaryDefinitions.push([
    "#[allow(non_snake_case)]",
    "#[derive(Debug, Clone, PartialEq)]",
    `pub struct ${typeName} {`,
    ...fields,
    "}",
  ].join("\n"));
  return typeName;
}

// resolve one field reference to an exact configured declaration in this source
function renderConfiguredTypeReference(typeNode, state, pathLabel) {
  // admit only an argument-free simple identifier spelling
  if (
    !ts.isIdentifier(typeNode.typeName) ||
    typeNode.typeArguments !== undefined
  ) {
    throw new Error(`configured type reference unsupported at ${pathLabel}`);
  }

  const checker = getSourceChecker(
    state.checkers,
    state.sourcePath,
    state.sourceFile,
    state.utilityDeclarations,
  );
  const symbol = checker.getSymbolAtLocation(typeNode.typeName);

  // reject unresolved names before classifying local versus imported identity
  if (symbol === undefined) {
    throw new Error(`configured type reference identity unsupported at ${pathLabel}`);
  }

  const declarations = symbol.declarations;

  // reject merged symbols instead of choosing among declarations
  if (declarations === undefined || declarations.length !== 1) {
    throw new Error(`configured type reference identity unsupported at ${pathLabel}`);
  }

  const declaration = declarations[0];

  // admit only one explicitly configured named type import alias
  if ((symbol.flags & ts.SymbolFlags.Alias) !== 0) {
    // retain the previous error for sources that did not opt into imports
    if (state.typeImportBindings === undefined) {
      throw new Error(`configured type reference identity unsupported at ${pathLabel}`);
    }

    // distinguish an unsupported alias in an opted-in import source
    if (!ts.isImportSpecifier(declaration)) {
      throw new Error(`configured type import identity unsupported at ${pathLabel}`);
    }
    const namedImports = declaration.parent;
    const importClause = namedImports.parent;
    const importDeclaration = importClause.parent;

    // require a syntactically type-only named import from a configured package
    if (
      !ts.isNamedImports(namedImports) ||
      !ts.isImportClause(importClause) ||
      !ts.isImportDeclaration(importDeclaration) ||
      (!declaration.isTypeOnly && !importClause.isTypeOnly) ||
      !ts.isStringLiteralLike(importDeclaration.moduleSpecifier)
    ) {
      throw new Error(`configured type import identity unsupported at ${pathLabel}`);
    }
    const binding = state.typeImportBindings.get(importDeclaration.moduleSpecifier.text);
    const exportName = (declaration.propertyName ?? declaration.name).text;
    const configured = binding?.exported.get(exportName);

    // join the compiler alias edge to the exact configured leaf declaration
    if (
      binding === undefined ||
      configured === undefined ||
      declaration.name.text !== typeNode.typeName.text ||
      resolvedSymbolDeclaration(checker, symbol) !== configured.declaration
    ) {
      throw new Error(`configured type import identity unsupported at ${pathLabel}`);
    }

    // never erase a configured imported root's generic parameters
    if ((configured.declaration.typeParameters?.length ?? 0) > 0) {
      throw new Error(`generic configured type import unsupported at ${pathLabel}`);
    }

    consumeFieldMapping(state, new Set(), "configured type import");
    binding.used.add(exportName);
    state.dependencies.get(state.rootIdentity).add(configured.identity);
    return configured.root.exportName;
  }

  const configured = state.configuredDeclarations.get(declaration);

  // require the exact configured top-level AST object from this external module
  if (
    configured === undefined ||
    configured.root.localInterface === true ||
    configured.root.pgQueryOwners !== undefined ||
    declaration.getSourceFile() !== state.sourceFile ||
    declaration.parent !== state.sourceFile ||
    !ts.isExternalModule(state.sourceFile)
  ) {
    throw new Error(`configured type reference identity unsupported at ${pathLabel}`);
  }

  // reject generic target declarations rather than erasing their parameters
  if (declaration.typeParameters !== undefined && declaration.typeParameters.length > 0) {
    throw new Error(`generic configured type reference unsupported at ${pathLabel}`);
  }

  consumeFieldMapping(state, new Set(), "configured type reference");
  state.dependencies.get(state.rootIdentity).add(configured.identity);
  return configured.root.exportName;
}

// detect one identifier anywhere inside a variable binding name
function bindingNameDeclaresIdentifier(bindingName, identifierText) {
  // compare one direct identifier binding
  if (ts.isIdentifier(bindingName)) {
    return bindingName.text === identifierText;
  }

  // inspect every nested object or array binding element
  for (const element of bindingName.elements) {
    // ignore array binding holes because they introduce no name
    if (ts.isOmittedExpression(element)) {
      continue;
    }

    // retain any nested binding of the selected catalog name
    if (bindingNameDeclaresIdentifier(element.name, identifierText)) {
      return true;
    }
  }

  return false;
}

// collect every top-level value binding that could claim one local name
function findTopLevelValueBindings(sourceFile, identifierText) {
  const bindings = [];

  // inspect only module-level statements so nested shadows remain irrelevant
  for (const statement of sourceFile.statements) {
    // count explicit and destructured variable bindings
    if (ts.isVariableStatement(statement)) {
      // inspect every declaration in the shared declaration list
      for (const declaration of statement.declarationList.declarations) {
        // record the owning declaration for exact resolved-node comparison
        if (bindingNameDeclaresIdentifier(declaration.name, identifierText)) {
          bindings.push(declaration);
        }
      }
      continue;
    }

    // count default, namespace, and named import aliases conservatively
    if (ts.isImportDeclaration(statement)) {
      const importClause = statement.importClause;

      // ignore side-effect-only imports because they introduce no local name
      if (importClause === undefined) {
        continue;
      }

      // count one default import binding
      if (importClause.name?.text === identifierText) {
        bindings.push(importClause);
      }

      const namedBindings = importClause.namedBindings;

      // count one namespace import binding
      if (namedBindings !== undefined && ts.isNamespaceImport(namedBindings)) {
        // retain the namespace import only when its local name matches
        if (namedBindings.name.text === identifierText) {
          bindings.push(namedBindings);
        }
      }

      // count every matching named import alias
      if (namedBindings !== undefined && ts.isNamedImports(namedBindings)) {
        // inspect each local import name independently
        for (const element of namedBindings.elements) {
          // retain the local alias rather than its foreign exported name
          if (element.name.text === identifierText) {
            bindings.push(element);
          }
        }
      }
      continue;
    }

    // count legacy import-equals aliases
    if (ts.isImportEqualsDeclaration(statement)) {
      // retain only an exact local alias match
      if (statement.name.text === identifierText) {
        bindings.push(statement);
      }
      continue;
    }

    // count every other named declaration that also creates a value binding
    if (
      (ts.isFunctionDeclaration(statement) ||
        ts.isClassDeclaration(statement) ||
        ts.isEnumDeclaration(statement) ||
        ts.isModuleDeclaration(statement)) &&
      statement.name !== undefined &&
      ts.isIdentifier(statement.name) &&
      statement.name.text === identifierText
    ) {
      bindings.push(statement);
    }
  }

  return bindings;
}

// resolve one uniquely bound local top-level const catalog initializer
function resolveLocalConstCatalogInitializer(queryName, state, sourceLabel = "const string tuple") {
  const checker = getSourceChecker(
    state.checkers,
    state.sourcePath,
    state.sourceFile,
    state.utilityDeclarations,
  );
  const symbol = checker.getSymbolAtLocation(queryName);

  // reject unresolved names and every alias symbol, including imports
  if (symbol === undefined || (symbol.flags & ts.SymbolFlags.Alias) !== 0) {
    throw new Error(`${sourceLabel} identity unsupported at root ${state.rootName}`);
  }

  const declarations = symbol.declarations;

  // reject merged values instead of choosing among declarations
  if (declarations === undefined || declarations.length !== 1) {
    throw new Error(`${sourceLabel} identity unsupported at root ${state.rootName}`);
  }

  const declaration = declarations[0];

  // require one same-source variable declaration with its exact identifier
  if (
    !ts.isVariableDeclaration(declaration) ||
    declaration.getSourceFile() !== state.sourceFile ||
    !ts.isIdentifier(declaration.name) ||
    declaration.name.text !== queryName.text
  ) {
    throw new Error(`${sourceLabel} identity unsupported at root ${state.rootName}`);
  }

  const topLevelBindings = findTopLevelValueBindings(state.sourceFile, queryName.text);

  // require the resolved declaration to be the sole top-level value binding
  if (topLevelBindings.length !== 1 || topLevelBindings[0] !== declaration) {
    throw new Error(`${sourceLabel} identity unsupported at root ${state.rootName}`);
  }

  const declarationList = declaration.parent;
  const statement = declarationList.parent;

  // require a local top-level const declaration in this exact external module
  if (
    !ts.isVariableDeclarationList(declarationList) ||
    (declarationList.flags & ts.NodeFlags.Const) === 0 ||
    !ts.isVariableStatement(statement) ||
    statement.parent !== state.sourceFile ||
    !ts.isExternalModule(state.sourceFile)
  ) {
    throw new Error(`${sourceLabel} source unsupported at root ${state.rootName}`);
  }

  const initializer = declaration.initializer;

  // reject annotations, definite assignment, and missing catalog values
  if (
    declaration.type !== undefined ||
    declaration.exclamationToken !== undefined ||
    initializer === undefined
  ) {
    throw new Error(`${sourceLabel} source unsupported at root ${state.rootName}`);
  }

  return initializer;
}

// identify the exact parenthesized typeof-name and number index syntax
function indexedConstCatalogName(typeNode) {
  // reject every non-indexed root expression before reading its children
  if (!ts.isIndexedAccessTypeNode(typeNode)) {
    return null;
  }

  const objectType = typeNode.objectType;

  // return the local query identifier only for the closed indexed shape
  if (
    ts.isParenthesizedTypeNode(objectType) &&
    ts.isTypeQueryNode(objectType.type) &&
    ts.isIdentifier(objectType.type.exprName) &&
    objectType.type.typeArguments === undefined &&
    typeNode.indexType.kind === ts.SyntaxKind.NumberKeyword
  ) {
    return objectType.type.exprName;
  }

  return null;
}

// remove parser-only grouping around one exact type expression
function unwrapParenthesizedType(typeNode) {
  let unwrapped = typeNode;

  // grouping never changes the admitted semantic shape
  while (ts.isParenthesizedTypeNode(unwrapped)) {
    unwrapped = unwrapped.type;
  }

  return unwrapped;
}

// select one configured enum projection target and record its dependency
function resolveFieldProjectionTarget(state, projection, pathLabel) {
  const configured = state.configuredRootsByName.get(projection.root);

  // require one exact configured public target name
  if (
    configured === undefined ||
    configured.root.localAlias === true ||
    configured.root.localInterface === true
  ) {
    throw new Error(`field projection target unsupported at ${pathLabel}`);
  }

  state.dependencies.get(state.rootIdentity).add(configured.identity);
  const rendered = state.renderConfiguredRoot(configured.identity);

  // projections may reuse only an existing finite string enum nominal
  if (rendered.kind !== "string-literal-enum") {
    throw new Error(`field projection target must render string-literal-enum at ${pathLabel}`);
  }

  return configured;
}

// prove one readonly imported const-catalog array projection
function renderConstCatalogArrayProjection(typeNode, state, projection, pathLabel) {
  const outerType = unwrapParenthesizedType(typeNode);

  // require readonly (typeof Catalog)[number][] without widening arrays
  if (
    !ts.isTypeOperatorNode(outerType) ||
    outerType.operator !== ts.SyntaxKind.ReadonlyKeyword ||
    !ts.isArrayTypeNode(outerType.type)
  ) {
    throw new Error(`const-catalog-array projection shape unsupported at ${pathLabel}`);
  }

  const elementType = unwrapParenthesizedType(outerType.type.elementType);
  const queryName = indexedConstCatalogName(elementType);

  // retain only the exact indexed imported value query
  if (queryName === null) {
    throw new Error(`const-catalog-array projection shape unsupported at ${pathLabel}`);
  }

  const imported = resolveApprovedValueImport(queryName, state, pathLabel, "ordinary");
  const configured = resolveFieldProjectionTarget(state, projection, pathLabel);
  const targetDeclaration = configured.declaration;

  // require one exact named alias with the same indexed const query
  if (!ts.isTypeAliasDeclaration(targetDeclaration)) {
    throw new Error(`const-catalog-array projection target unsupported at ${pathLabel}`);
  }
  const targetQueryName = indexedConstCatalogName(
    unwrapParenthesizedType(targetDeclaration.type),
  );

  // keep the target query on the same graph checker and declaration object
  if (targetQueryName === null) {
    throw new Error(`const-catalog-array projection target unsupported at ${pathLabel}`);
  }
  const targetChecker = getSourceChecker(
    state.checkers,
    configured.root.source,
    targetDeclaration.getSourceFile(),
    state.utilityDeclarationsBySource.get(configured.root.source),
  );
  const targetSymbol = targetChecker.getSymbolAtLocation(targetQueryName);

  // reject copied values, unrelated catalogs, and cross-program identities
  if (
    targetChecker !== imported.checker ||
    targetSymbol === undefined ||
    resolvedSymbolDeclaration(targetChecker, targetSymbol) !== imported.declaration
  ) {
    throw new Error(`const-catalog-array projection catalog identity unsupported at ${pathLabel}`);
  }

  return `Vec<${configured.root.exportName}>`;
}

// prove one pinned ReturnType projection to a configured enum
function renderReturnEnumProjection(typeNode, state, projection, pathLabel) {
  const returnType = unwrapParenthesizedType(typeNode);

  // require exact ReturnType<typeof FunctionName> syntax
  if (
    !ts.isTypeReferenceNode(returnType) ||
    !ts.isIdentifier(returnType.typeName) ||
    returnType.typeName.text !== "ReturnType" ||
    returnType.typeArguments?.length !== 1
  ) {
    throw new Error(`return-enum projection shape unsupported at ${pathLabel}`);
  }
  const queryType = unwrapParenthesizedType(returnType.typeArguments[0]);

  // retain one bare value query without type arguments
  if (
    !ts.isTypeQueryNode(queryType) ||
    !ts.isIdentifier(queryType.exprName) ||
    queryType.typeArguments !== undefined
  ) {
    throw new Error(`return-enum projection shape unsupported at ${pathLabel}`);
  }

  // require the pinned compiler utility alias in this exact checker
  if (state.utilityDeclarations === undefined) {
    throw new Error(`return-enum utility identity unsupported at ${pathLabel}`);
  }
  const imported = resolveApprovedValueImport(queryType.exprName, state, pathLabel, "ordinary");
  const returnTypeDeclarations = imported.checker.getSymbolAtLocation(returnType.typeName)?.declarations;

  // reject local utility shadows and changed compiler declarations
  if (
    returnTypeDeclarations?.length !== 1 ||
    returnTypeDeclarations[0] !== state.utilityDeclarations.declarations.ReturnType
  ) {
    throw new Error(`return-enum utility identity unsupported at ${pathLabel}`);
  }

  const configured = resolveFieldProjectionTarget(state, projection, pathLabel);
  const functionDeclaration = imported.declaration;

  // admit one sole explicit nongeneric top-level function declaration
  if (
    !ts.isFunctionDeclaration(functionDeclaration) ||
    functionDeclaration.parent !== functionDeclaration.getSourceFile() ||
    functionDeclaration.name === undefined ||
    functionDeclaration.body === undefined ||
    (functionDeclaration.typeParameters?.length ?? 0) > 0 ||
    functionDeclaration.type === undefined ||
    !ts.isTypeReferenceNode(functionDeclaration.type) ||
    !ts.isIdentifier(functionDeclaration.type.typeName) ||
    functionDeclaration.type.typeArguments !== undefined
  ) {
    throw new Error(`return-enum function declaration unsupported at ${pathLabel}`);
  }

  const functionSymbol = imported.checker.getSymbolAtLocation(functionDeclaration.name);
  const returnSymbol = imported.checker.getSymbolAtLocation(functionDeclaration.type.typeName);

  // reject overloads and bind the declared return directly to the target alias
  if (
    functionSymbol?.declarations?.length !== 1 ||
    functionSymbol.declarations[0] !== functionDeclaration ||
    returnSymbol === undefined ||
    (returnSymbol.flags & ts.SymbolFlags.Alias) !== 0 ||
    configured.declaration.getSourceFile() !== functionDeclaration.getSourceFile() ||
    resolvedSymbolDeclaration(imported.checker, returnSymbol) !== configured.declaration
  ) {
    throw new Error(`return-enum function return identity unsupported at ${pathLabel}`);
  }

  return configured.root.exportName;
}

// prove one pinned Exclude projection from a configured finite string enum
function renderExcludeLiteralProjection(typeNode, state, projection, pathLabel) {
  // require one direct named enum and one ordinary string literal argument
  if (
    !ts.isTypeReferenceNode(typeNode) ||
    !ts.isIdentifier(typeNode.typeName) ||
    typeNode.typeName.text !== "Exclude" ||
    typeNode.typeArguments?.length !== 2 ||
    !ts.isTypeReferenceNode(typeNode.typeArguments[0]) ||
    !ts.isIdentifier(typeNode.typeArguments[0].typeName) ||
    typeNode.typeArguments[0].typeArguments !== undefined ||
    !ts.isLiteralTypeNode(typeNode.typeArguments[1]) ||
    !ts.isStringLiteral(typeNode.typeArguments[1].literal)
  ) {
    throw new Error(`exclude-literal projection shape unsupported at ${pathLabel}`);
  }

  // require the pinned compiler utility alias in this exact checker
  if (state.utilityDeclarations === undefined) {
    throw new Error(`exclude-literal utility identity unsupported at ${pathLabel}`);
  }
  const checker = getSourceChecker(
    state.checkers,
    state.sourcePath,
    state.sourceFile,
    state.utilityDeclarations,
  );
  const excludeDeclarations = checker.getSymbolAtLocation(typeNode.typeName)?.declarations;

  // reject local utility shadows and changed compiler declarations
  if (
    excludeDeclarations?.length !== 1 ||
    excludeDeclarations[0] !== state.utilityDeclarations.declarations.Exclude
  ) {
    throw new Error(`exclude-literal utility identity unsupported at ${pathLabel}`);
  }

  let enumRustName;

  // collapse unsupported references to the projection's finite-enum boundary
  try {
    enumRustName = renderConfiguredTypeReference(
      typeNode.typeArguments[0],
      state,
      pathLabel,
    );
  } catch {
    throw new Error(`exclude-literal enum identity unsupported at ${pathLabel}`);
  }

  const configured = resolveFieldProjectionTarget(state, projection, pathLabel);
  const rendered = state.renderConfiguredRoot(configured.identity);

  // bind the source subject to the exact configured finite string enum
  if (
    enumRustName !== configured.root.exportName ||
    rendered.kind !== "string-literal-enum" ||
    !Array.isArray(rendered.wireValues) ||
    rendered.wireValues.length === 0
  ) {
    throw new Error(`exclude-literal enum identity unsupported at ${pathLabel}`);
  }

  const excludedValue = typeNode.typeArguments[1].literal.text;

  // require the literal to remove one actual source enum alternative
  if (!rendered.wireValues.includes(excludedValue)) {
    throw new Error(
      `exclude-literal value ${JSON.stringify(excludedValue)} is outside configured enum at ${pathLabel}`,
    );
  }

  // preserve the configured enum's original source order
  const values = rendered.wireValues.filter((value) => value !== excludedValue);

  // retain at least one exact wire alternative after exclusion
  if (values.length === 0) {
    throw new Error(`exclude-literal projection removes every value at ${pathLabel}`);
  }

  const typeName = fieldTypeName(state, "Enum");
  reserveGeneratedType(state, typeName);
  state.auxiliaryDefinitions.push(renderStringEnumValues(typeName, values, pathLabel));
  return typeName;
}

// reuse one direct field type from an exact configured public interface
function renderInterfaceFieldProjection(typeNode, state, projection, pathLabel) {
  // require the exact Root["currentField"] syntax without grouping or aliases
  if (
    !ts.isIndexedAccessTypeNode(typeNode) ||
    !ts.isTypeReferenceNode(typeNode.objectType) ||
    !ts.isIdentifier(typeNode.objectType.typeName) ||
    typeNode.objectType.typeName.text !== projection.root ||
    typeNode.objectType.typeArguments !== undefined ||
    !ts.isLiteralTypeNode(typeNode.indexType) ||
    !ts.isStringLiteral(typeNode.indexType.literal) ||
    typeNode.indexType.literal.text !== state.fieldPath.at(-1)
  ) {
    throw new Error(`interface-field projection shape unsupported at ${pathLabel}`);
  }

  const checker = getSourceChecker(
    state.checkers,
    state.sourcePath,
    state.sourceFile,
    state.utilityDeclarations,
  );
  // admit a pinned imported interface only from the dedicated loader renderer
  const imported = projection.imported === true
    ? resolveApprovedTypeImport(typeNode.objectType.typeName, state, pathLabel)
    : undefined;
  const configured = imported?.configured ?? state.configuredRootsByName.get(projection.root);
  const bindingSymbol = checker.getSymbolAtLocation(typeNode.objectType.typeName);
  const rootSymbol = imported === undefined ? bindingSymbol : checker.getAliasedSymbol(bindingSymbol);
  const rootDeclarations = rootSymbol?.declarations;
  const rootDeclaration = rootDeclarations?.[0];
  const rootSource = configured === undefined ? undefined : state.sourceFiles.get(configured.root.source);

  // bind the source spelling to one exact configured public interface declaration
  if (
    configured === undefined ||
    configured.root.exportName !== projection.root ||
    (imported === undefined ? rootSource !== state.sourceFile : rootSource === state.sourceFile) ||
    configured.root.localAlias === true ||
    configured.root.localInterface === true ||
    rootSymbol === undefined ||
    (rootSymbol.flags & ts.SymbolFlags.Alias) !== 0 ||
    rootDeclarations?.length !== 1 ||
    rootDeclaration !== configured.declaration ||
    state.configuredDeclarationsBySource.get(configured.root.source)?.get(rootDeclaration)?.identity !== configured.identity ||
    rootDeclaration.getSourceFile() !== rootSource ||
    rootDeclaration.parent !== rootSource ||
    !ts.isExternalModule(rootSource) ||
    !isExported(rootDeclaration) ||
    !ts.isInterfaceDeclaration(rootDeclaration) ||
    (rootDeclaration.typeParameters?.length ?? 0) > 0
  ) {
    throw new Error(`interface-field target identity unsupported at ${pathLabel}`);
  }

  const fieldName = state.fieldPath.at(-1);
  const directFields = rootDeclaration.members.filter((member) =>
    ts.isPropertySignature(member) &&
    ts.isIdentifier(member.name) &&
    member.name.text === fieldName
  );
  const directField = directFields[0];

  // require one direct required readonly field with an explicit source type
  if (
    directFields.length !== 1 ||
    directField.questionToken !== undefined ||
    !isReadonly(directField) ||
    directField.type === undefined
  ) {
    throw new Error(`interface-field target field unsupported at ${pathLabel}`);
  }

  const declaredType = checker.getDeclaredTypeOfSymbol(rootSymbol);
  const propertySymbol = checker.getPropertyOfType(declaredType, fieldName);
  const directFieldSymbol = checker.getSymbolAtLocation(directField.name);

  // pin the indexed field to the sole direct compiler property declaration
  if (
    propertySymbol === undefined ||
    directFieldSymbol === undefined ||
    propertySymbol !== directFieldSymbol ||
    propertySymbol.declarations?.length !== 1 ||
    propertySymbol.declarations[0] !== directField
  ) {
    throw new Error(`interface-field target field identity unsupported at ${pathLabel}`);
  }

  // record the dependency before the shared renderer applies its cycle guard
  state.dependencies.get(state.rootIdentity).add(configured.identity);
  const rendered = state.renderConfiguredRoot(configured.identity);
  const renderedFields = rendered.interfaceFieldBlocks?.filter(
    (field) => field.name === fieldName,
  );

  // reuse only the already validated structured Rust field type
  if (
    rendered.kind !== "readonly-interface" ||
    renderedFields?.length !== 1 ||
    typeof renderedFields[0].rustType !== "string"
  ) {
    throw new Error(`interface-field target unsupported at ${pathLabel}`);
  }

  // charge the exact package edge only to a successfully projected field
  if (imported !== undefined) {
    imported.binding.used.add(imported.exportName);
  }
  return renderedFields[0].rustType;
}

// extend one indexed field from an exact imported configured interface
function renderIndexedInterfaceFieldExtensionProjection(member, state, projection, pathLabel) {
  const typeNode = member.type;
  const fieldName = state.fieldPath.at(-1);

  // require one direct required readonly top-level projected field
  if (
    member.questionToken !== undefined ||
    !isReadonly(member) ||
    !ts.isIdentifier(member.name) ||
    member.name.text !== fieldName ||
    !ts.isIntersectionTypeNode(typeNode) ||
    typeNode.types.length !== 2
  ) {
    throw new Error(`indexed interface field extension shape unsupported at ${pathLabel}`);
  }
  const [indexedType, extensionType] = typeNode.types;

  // require ImportedRoot["sameField"] followed by one inline extension
  if (
    !ts.isIndexedAccessTypeNode(indexedType) ||
    !ts.isTypeReferenceNode(indexedType.objectType) ||
    !ts.isIdentifier(indexedType.objectType.typeName) ||
    indexedType.objectType.typeName.text !== projection.root ||
    indexedType.objectType.typeArguments !== undefined ||
    !ts.isLiteralTypeNode(indexedType.indexType) ||
    !ts.isStringLiteral(indexedType.indexType.literal) ||
    indexedType.indexType.literal.text !== fieldName ||
    !ts.isTypeLiteralNode(extensionType) ||
    extensionType.members.length !== 1
  ) {
    throw new Error(`indexed interface field extension shape unsupported at ${pathLabel}`);
  }

  const extensionField = extensionType.members[0];

  // admit exactly one direct required readonly identifier property
  if (
    !ts.isPropertySignature(extensionField) ||
    !ts.isIdentifier(extensionField.name) ||
    extensionField.questionToken !== undefined ||
    !isReadonly(extensionField) ||
    extensionField.type === undefined
  ) {
    throw new Error(`indexed interface field extension member unsupported at ${pathLabel}`);
  }

  const imported = resolveApprovedTypeImport(
    indexedType.objectType.typeName,
    state,
    pathLabel,
  );
  const { checker, configured } = imported;
  const rootDeclaration = configured.declaration;

  // bind only an exact foreign configured public plain interface root
  if (
    imported.exportName !== projection.root ||
    configured.root.exportName !== projection.root ||
    configured.root.source === state.sourcePath ||
    configured.root.localAlias === true ||
    configured.root.localInterface === true ||
    configured.root.base !== undefined ||
    configured.root.importedBase !== undefined ||
    configured.root.omitBaseFields !== undefined ||
    configured.root.omitInheritedBaseFields !== undefined ||
    configured.root.catalog !== undefined ||
    configured.root.discriminator !== undefined ||
    configured.root.readonlyTaggedUnion !== undefined ||
    configured.root.objectUnion !== undefined ||
    configured.root.importedObjectUnion !== undefined ||
    configured.root.literalUnion !== undefined ||
    configured.root.recursiveJson !== undefined ||
    configured.root.indexedLiteralSources !== undefined ||
    configured.root.fieldProjections !== undefined ||
    configured.root.wireFieldNames !== undefined ||
    configured.root.pick !== undefined ||
    rootDeclaration.getSourceFile() === state.sourceFile ||
    rootDeclaration.parent !== rootDeclaration.getSourceFile() ||
    !ts.isExternalModule(rootDeclaration.getSourceFile()) ||
    !isExported(rootDeclaration) ||
    !ts.isInterfaceDeclaration(rootDeclaration) ||
    (rootDeclaration.typeParameters?.length ?? 0) > 0 ||
    (rootDeclaration.heritageClauses?.length ?? 0) > 0
  ) {
    throw new Error(`indexed interface field extension target unsupported at ${pathLabel}`);
  }

  const rootSymbol = checker.getSymbolAtLocation(rootDeclaration.name);
  const directFields = rootDeclaration.members.filter((candidate) =>
    ts.isPropertySignature(candidate) &&
    ts.isIdentifier(candidate.name) &&
    candidate.name.text === fieldName
  );
  const directField = directFields[0];

  // require one direct required readonly inline-object source field
  if (
    rootSymbol === undefined ||
    (rootSymbol.flags & ts.SymbolFlags.Alias) !== 0 ||
    rootSymbol.declarations?.length !== 1 ||
    rootSymbol.declarations[0] !== rootDeclaration ||
    directFields.length !== 1 ||
    directField.questionToken !== undefined ||
    !isReadonly(directField) ||
    directField.type === undefined ||
    !ts.isTypeLiteralNode(directField.type)
  ) {
    throw new Error(`indexed interface field extension field unsupported at ${pathLabel}`);
  }

  const declaredType = checker.getDeclaredTypeOfSymbol(rootSymbol);
  const propertySymbol = checker.getPropertyOfType(declaredType, fieldName);
  const directFieldSymbol = checker.getSymbolAtLocation(directField.name);

  // pin the indexed key to the sole direct compiler property declaration
  if (
    propertySymbol === undefined ||
    directFieldSymbol === undefined ||
    propertySymbol !== directFieldSymbol ||
    propertySymbol.declarations?.length !== 1 ||
    propertySymbol.declarations[0] !== directField
  ) {
    throw new Error(`indexed interface field extension field identity unsupported at ${pathLabel}`);
  }

  state.dependencies.get(state.rootIdentity).add(configured.identity);
  const rendered = state.renderConfiguredRoot(configured.identity);
  const descriptorByNode = rendered.interfaceNestedObjects?.byNode.get(directField.type);
  const descriptorByPath = rendered.interfaceNestedObjects?.byPath.get(fieldName);
  const renderedFields = rendered.interfaceFieldBlocks?.filter(
    (field) => field.name === fieldName,
  );

  // reuse only the original renderer's exact named nested-field descriptor
  if (
    rendered.kind !== "readonly-interface" ||
    descriptorByNode === undefined ||
    descriptorByNode !== descriptorByPath ||
    descriptorByNode.path !== fieldName ||
    renderedFields?.length !== 1 ||
    renderedFields[0].rustType !== descriptorByNode.typeName
  ) {
    throw new Error(`indexed interface field extension metadata unsupported at ${pathLabel}`);
  }

  const typeName = fieldTypeName(state);
  reserveGeneratedType(state, typeName);
  const extensionBlocks = [];
  const extensionFields = renderObjectProperties(
    extensionType.members,
    state,
    descriptorByNode.fieldNames,
    extensionBlocks,
  );

  // require the shared renderer to retain exactly the one extension field
  if (extensionBlocks.length !== 1 || extensionBlocks[0].name !== extensionField.name.text) {
    throw new Error(`indexed interface field extension metadata unsupported at ${pathLabel}`);
  }

  const inheritedFields = descriptorByNode.fieldBlocks.flatMap(
    // copy each original declaration together with its annotations
    (field) => field.lines,
  );
  imported.binding.used.add(imported.exportName);
  state.auxiliaryDefinitions.push([
    "#[allow(non_snake_case)]",
    "#[derive(Debug, Clone, PartialEq)]",
    `pub struct ${typeName} {`,
    ...inheritedFields,
    ...extensionFields,
    "}",
  ].join("\n"));
  return typeName;
}

// prove one pinned Omit array projection from a configured local interface
function renderOmitInterfaceArrayProjection(typeNode, state, projection, pathLabel) {
  // require exactly readonly Omit<NamedInterface, "field">[]
  if (
    !ts.isTypeOperatorNode(typeNode) ||
    typeNode.operator !== ts.SyntaxKind.ReadonlyKeyword ||
    !ts.isArrayTypeNode(typeNode.type) ||
    !ts.isTypeReferenceNode(typeNode.type.elementType) ||
    !ts.isIdentifier(typeNode.type.elementType.typeName) ||
    typeNode.type.elementType.typeName.text !== "Omit" ||
    typeNode.type.elementType.typeArguments?.length !== 2 ||
    !ts.isTypeReferenceNode(typeNode.type.elementType.typeArguments[0]) ||
    !ts.isIdentifier(typeNode.type.elementType.typeArguments[0].typeName) ||
    typeNode.type.elementType.typeArguments[0].typeArguments !== undefined ||
    !ts.isLiteralTypeNode(typeNode.type.elementType.typeArguments[1]) ||
    !ts.isStringLiteral(typeNode.type.elementType.typeArguments[1].literal)
  ) {
    throw new Error(`omit-interface-array projection shape unsupported at ${pathLabel}`);
  }

  const omitType = typeNode.type.elementType;
  const baseType = omitType.typeArguments[0];
  const omittedName = omitType.typeArguments[1].literal.text;

  // require the pinned compiler utility alias in this exact checker
  if (state.utilityDeclarations === undefined) {
    throw new Error(`omit-interface-array utility identity unsupported at ${pathLabel}`);
  }
  const checker = getSourceChecker(
    state.checkers,
    state.sourcePath,
    state.sourceFile,
    state.utilityDeclarations,
  );
  const omitSymbol = checker.getSymbolAtLocation(omitType.typeName);
  const omitDeclarations = omitSymbol?.declarations;

  // reject local utility shadows and changed compiler declarations
  if (
    omitSymbol === undefined ||
    (omitSymbol.flags & ts.SymbolFlags.Alias) !== 0 ||
    omitDeclarations?.length !== 1 ||
    omitDeclarations[0] !== state.utilityDeclarations.declarations.Omit
  ) {
    throw new Error(`omit-interface-array utility identity unsupported at ${pathLabel}`);
  }

  const configured = state.configuredRootsByName.get(projection.root);
  const baseSymbol = checker.getSymbolAtLocation(baseType.typeName);
  const baseDeclarations = baseSymbol?.declarations;
  const baseDeclaration = baseDeclarations?.[0];

  // bind the source name directly to one same-module configured interface
  if (
    baseType.typeName.text !== projection.root ||
    configured === undefined ||
    configured.root.source !== state.sourcePath ||
    configured.root.localAlias === true ||
    configured.root.localInterface === true ||
    baseSymbol === undefined ||
    (baseSymbol.flags & ts.SymbolFlags.Alias) !== 0 ||
    baseDeclarations?.length !== 1 ||
    baseDeclaration !== configured.declaration ||
    state.configuredDeclarations.get(baseDeclaration)?.identity !== configured.identity ||
    baseDeclaration.getSourceFile() !== state.sourceFile ||
    baseDeclaration.parent !== state.sourceFile ||
    !ts.isInterfaceDeclaration(baseDeclaration) ||
    (baseDeclaration.typeParameters?.length ?? 0) > 0
  ) {
    throw new Error(`omit-interface-array base identity unsupported at ${pathLabel}`);
  }

  // select only direct identifier properties matching the exact omitted key
  const directOmittedFields = baseDeclaration.members.filter((member) =>
    ts.isPropertySignature(member) &&
    ts.isIdentifier(member.name) &&
    member.name.text === omittedName
  );

  // require the omitted key to name one direct property on the selected declaration
  if (directOmittedFields.length !== 1) {
    throw new Error(`omit-interface-array field unsupported at ${pathLabel}`);
  }
  const omittedFieldSymbol = checker.getSymbolAtLocation(directOmittedFields[0].name);

  // pin the omitted property symbol to the latest parsed declaration object
  if (
    omittedFieldSymbol === undefined ||
    omittedFieldSymbol.declarations?.length !== 1 ||
    omittedFieldSymbol.declarations[0] !== directOmittedFields[0]
  ) {
    throw new Error(`omit-interface-array field unsupported at ${pathLabel}`);
  }

  // record the dependency before the shared renderer applies its cycle guard
  state.dependencies.get(state.rootIdentity).add(configured.identity);
  const rendered = state.renderConfiguredRoot(configured.identity);

  // reuse only the fully validated structured interface representation
  if (
    rendered.kind !== "readonly-interface" ||
    rendered.interfaceFieldBlocks === undefined ||
    rendered.interfaceFieldNames === undefined ||
    !rendered.interfaceFieldNames.has(omittedName)
  ) {
    throw new Error(`omit-interface-array target unsupported at ${pathLabel}`);
  }

  // retain complete annotated blocks in the configured interface field order
  const retainedFieldBlocks = rendered.interfaceFieldBlocks.filter(
    (field) => field.name !== omittedName,
  );

  // reject empty helpers and any incomplete omission from the rendered fields
  if (
    retainedFieldBlocks.length === 0 ||
    retainedFieldBlocks.length + 1 !== rendered.interfaceFieldBlocks.length ||
    retainedFieldBlocks.some((field) => field.name === omittedName)
  ) {
    throw new Error(`omit-interface-array projection removes every field at ${pathLabel}`);
  }

  const typeName = fieldTypeName(state, "Entry");
  reserveGeneratedType(state, typeName);
  // preserve every retained field annotation in the generated helper
  const retainedFields = retainedFieldBlocks.flatMap((field) => field.lines);
  state.auxiliaryDefinitions.push([
    "#[allow(non_snake_case)]",
    "#[derive(Debug, Clone, PartialEq)]",
    `pub struct ${typeName} {`,
    ...retainedFields,
    "}",
  ].join("\n"));
  return `Vec<${typeName}>`;
}

// switch indexed-literal proof to one exact configured source
function indexedLiteralSourceState(state, sourcePath) {
  return {
    ...state,
    sourcePath,
    sourceFile: state.sourceFiles.get(sourcePath),
    configuredDeclarations: state.configuredDeclarationsBySource.get(sourcePath),
    utilityDeclarations: state.utilityDeclarationsBySource.get(sourcePath),
    typeImportBindings: state.typeImportBindingsByConsumer.get(sourcePath),
  };
}

// prove one configured proof-only named type import
function resolveIndexedLiteralTypeImport(identifier, state, expectedDeclaration, pathLabel, requireExplicitProof = false) {
  const checker = getSourceChecker(
    state.checkers,
    state.sourcePath,
    state.sourceFile,
    state.utilityDeclarations,
  );
  const symbol = checker.getSymbolAtLocation(identifier);
  const importSpecifier = symbol?.declarations?.[0];

  // require one compiler alias bound to an exact named import specifier
  if (
    symbol === undefined ||
    (symbol.flags & ts.SymbolFlags.Alias) === 0 ||
    symbol.declarations?.length !== 1 ||
    !ts.isImportSpecifier(importSpecifier) ||
    state.typeImportBindings === undefined
  ) {
    throw new Error(`indexed literal alias identity unsupported at ${pathLabel}`);
  }

  const namedImports = importSpecifier.parent;
  const importClause = namedImports.parent;
  const importDeclaration = importClause.parent;

  // exclude renamed, namespace, default, and malformed imports
  if (
    !ts.isNamedImports(namedImports) ||
    !ts.isImportClause(importClause) ||
    !ts.isImportDeclaration(importDeclaration) ||
    importSpecifier.propertyName !== undefined ||
    !ts.isStringLiteralLike(importDeclaration.moduleSpecifier)
  ) {
    throw new Error(`indexed literal alias identity unsupported at ${pathLabel}`);
  }

  const binding = state.typeImportBindings.get(importDeclaration.moduleSpecifier.text);
  const targetDeclaration = resolvedSymbolDeclaration(checker, symbol);
  const configuredProof = binding?.proofExported.get(identifier.text);

  // distinguish an explicitly proof-only name imported as a runtime value
  if (!importSpecifier.isTypeOnly && !importClause.isTypeOnly) {
    // report the stricter role violation for configured proof names
    if (configuredProof !== undefined) {
      throw new Error(`proof-only type import used as a value at ${pathLabel}`);
    }
    throw new Error(`indexed literal alias identity unsupported at ${pathLabel}`);
  }

  // bind the local spelling to the configured declaration in this graph checker
  if (
    binding === undefined ||
    binding.checker !== checker ||
    importSpecifier.name.text !== identifier.text ||
    ((requireExplicitProof || binding.proofExports !== undefined) && configuredProof !== expectedDeclaration) ||
    targetDeclaration !== expectedDeclaration
  ) {
    throw new Error(`indexed literal alias identity unsupported at ${pathLabel}`);
  }

  const leafMatches = [];

  // prove one unique declaration owner across the configured graph leaves
  for (const candidatePath of binding.leaves ?? [binding.entry]) {
    const candidateSymbol = configuredLeafExport(
      checker,
      binding.sourceFiles.get(candidatePath),
      identifier.text,
    );

    // retain only effective exports from exact selected source objects
    if (candidateSymbol !== null) {
      leafMatches.push(resolvedSymbolDeclaration(checker, candidateSymbol));
    }
  }

  const entrySymbol = configuredLeafExport(
    checker,
    binding.sourceFiles.get(binding.entry),
    identifier.text,
  );

  // require one leaf owner and the same effective package export
  if (
    leafMatches.length !== 1 ||
    leafMatches[0] !== expectedDeclaration ||
    entrySymbol === null ||
    resolvedSymbolDeclaration(checker, entrySymbol) !== expectedDeclaration
  ) {
    throw new Error(`indexed literal alias provenance unsupported at ${pathLabel}`);
  }

  // consume only explicitly configured proof names while retaining v14 graphs
  if (configuredProof !== undefined) {
    binding.proofUsed.add(identifier.text);
  }
  return expectedDeclaration;
}

// resolve one exact configured indexed-literal alias reference
function resolveIndexedLiteralAlias(identifier, state, pathLabel) {
  const expectedSource = state.indexedLiteralSources.get(identifier.text);
  const expectedDeclaration = state.indexedLiteralDeclarations.get(identifier.text);

  // require every traversed alias to be explicitly selected by name and source
  if (expectedSource === undefined || expectedDeclaration === undefined) {
    throw new Error(`indexed literal alias is not configured at ${pathLabel}: ${identifier.text}`);
  }

  const checker = getSourceChecker(
    state.checkers,
    state.sourcePath,
    state.sourceFile,
    state.utilityDeclarations,
  );
  const symbol = checker.getSymbolAtLocation(identifier);
  let declaration;

  // prove imported identities through the configured closed graph
  if (symbol !== undefined && (symbol.flags & ts.SymbolFlags.Alias) !== 0) {
    declaration = resolveIndexedLiteralTypeImport(
      identifier,
      state,
      expectedDeclaration,
      pathLabel,
    );
  } else {
    const declarations = symbol?.declarations;

    // require the exact local declaration object without merging or inference
    if (
      symbol === undefined ||
      declarations?.length !== 1 ||
      declarations[0] !== expectedDeclaration ||
      expectedSource !== state.sourcePath
    ) {
      throw new Error(`indexed literal alias identity unsupported at ${pathLabel}`);
    }
    declaration = declarations[0];
  }

  state.usedIndexedLiteralSources.add(identifier.text);
  return {
    declaration,
    sourceState: indexedLiteralSourceState(state, expectedSource),
  };
}

// collect exact direct string literals from one Extract constraint
function collectExtractConstraintValues(typeNode, pathLabel) {
  const members = ts.isUnionTypeNode(typeNode) ? typeNode.types : [typeNode];
  const values = [];

  // accept only ordinary direct string literal alternatives
  for (const member of members) {
    if (!ts.isLiteralTypeNode(member) || !ts.isStringLiteralLike(member.literal)) {
      throw new Error(`indexed literal Extract constraint unsupported at ${pathLabel}`);
    }
    values.push(member.literal.text);
  }

  return values;
}

// collect one selected property type without evaluating unrelated fields
function collectIndexedLiteralPropertyValues(
  typeNode,
  state,
  selectedField,
  pathLabel,
  activeAliases,
) {
  const node = unwrapParenthesizedType(typeNode);

  // preserve direct union order across literals and Extract expressions
  if (ts.isUnionTypeNode(node)) {
    const collected = { values: [], hasNull: false };

    // collect each direct member without admitting aliases or conditional evaluation
    for (const member of node.types) {
      const part = collectIndexedLiteralPropertyValues(
        member,
        state,
        selectedField,
        pathLabel,
        activeAliases,
      );
      collected.values.push(...part.values);
      collected.hasNull ||= part.hasNull;
    }
    return collected;
  }

  // retain exact string and null literals as distinct wire alternatives
  if (ts.isLiteralTypeNode(node)) {
    if (ts.isStringLiteralLike(node.literal)) {
      return { values: [node.literal.text], hasNull: false };
    }
    if (node.literal.kind === ts.SyntaxKind.NullKeyword) {
      return { values: [], hasNull: true };
    }
    throw new Error(`indexed literal property type unsupported at ${pathLabel}`);
  }

  // follow one exact same-source ConfiguredType["selectedField"] edge
  if (ts.isIndexedAccessTypeNode(node)) {
    if (
      !ts.isTypeReferenceNode(node.objectType) ||
      !ts.isIdentifier(node.objectType.typeName) ||
      node.objectType.typeArguments !== undefined ||
      !ts.isLiteralTypeNode(node.indexType) ||
      !ts.isStringLiteral(node.indexType.literal) ||
      node.indexType.literal.text !== selectedField
    ) {
      throw new Error(`indexed literal indexed access unsupported at ${pathLabel}`);
    }
    const resolved = resolveIndexedLiteralAlias(node.objectType.typeName, state, pathLabel);

    // prohibit an indexed property proof from crossing another source edge
    if (resolved.sourceState.sourcePath !== state.sourcePath) {
      throw new Error(`indexed literal indexed access unsupported at ${pathLabel}`);
    }
    return collectIndexedLiteralDeclarationValues(
      resolved.declaration,
      resolved.sourceState,
      selectedField,
      pathLabel,
      activeAliases,
    );
  }

  // admit only the pinned builtin Extract<ConfiguredEnum, DirectStrings>
  if (
    !ts.isTypeReferenceNode(node) ||
    !ts.isIdentifier(node.typeName) ||
    node.typeName.text !== "Extract" ||
    node.typeArguments?.length !== 2 ||
    state.utilityDeclarations === undefined
  ) {
    throw new Error(`indexed literal property type unsupported at ${pathLabel}`);
  }

  const checker = getSourceChecker(
    state.checkers,
    state.sourcePath,
    state.sourceFile,
    state.utilityDeclarations,
  );
  const extractDeclarations = checker.getSymbolAtLocation(node.typeName)?.declarations;

  // reject utility shadows and compiler declaration drift
  if (
    extractDeclarations?.length !== 1 ||
    extractDeclarations[0] !== state.utilityDeclarations.declarations.Extract
  ) {
    throw new Error(`indexed literal Extract utility identity unsupported at ${pathLabel}`);
  }

  const enumReference = node.typeArguments[0];

  // constrain the subject to one direct configured finite enum reference
  if (
    !ts.isTypeReferenceNode(enumReference) ||
    !ts.isIdentifier(enumReference.typeName) ||
    enumReference.typeArguments !== undefined
  ) {
    throw new Error(`indexed literal Extract enum unsupported at ${pathLabel}`);
  }

  const enumRustName = renderConfiguredTypeReference(enumReference, state, pathLabel);
  const configured = state.configuredRootsByName.get(enumRustName);
  let rendered;

  // collapse any unsupported configured target shape to this projection boundary
  if (configured !== undefined) {
    try {
      rendered = state.renderConfiguredRoot(configured.identity);
    } catch {
      throw new Error(`indexed literal Extract enum unsupported at ${pathLabel}`);
    }
  }

  // require a nonnullable finite string enum with retained exact wire values
  if (
    configured === undefined ||
    rendered?.kind !== "string-literal-enum" ||
    !Array.isArray(rendered.wireValues) ||
    rendered.wireValues.length === 0
  ) {
    throw new Error(`indexed literal Extract enum unsupported at ${pathLabel}`);
  }

  const values = collectExtractConstraintValues(node.typeArguments[1], pathLabel);
  const enumValues = new Set(rendered.wireValues);

  // prove the syntactic filter is a real subset of the configured enum
  for (const value of values) {
    if (!enumValues.has(value)) {
      throw new Error(
        `indexed literal Extract value ${JSON.stringify(value)} is outside configured enum at ${pathLabel}`,
      );
    }
  }

  return { values, hasNull: false };
}

// validate one closed readonly literal object and select one field
function collectIndexedLiteralObject(
  typeNode,
  state,
  selectedField,
  pathLabel,
  augmentation,
  activeAliases,
) {
  const memberNames = new Set();
  let selectedMember;

  // validate member closure while deliberately ignoring unrelated property types
  for (const member of typeNode.members) {
    if (
      !ts.isPropertySignature(member) ||
      !ts.isIdentifier(member.name) ||
      memberNames.has(member.name.text) ||
      !isReadonly(member) ||
      member.type === undefined
    ) {
      throw new Error(`indexed literal object arm unsupported at ${pathLabel}`);
    }
    memberNames.add(member.name.text);

    // retain only the exact configured property
    if (member.name.text === selectedField) {
      selectedMember = member;
    }
  }

  // augmentation objects may add fields but may not reinterpret the projection
  if (augmentation) {
    if (typeNode.members.length === 0) {
      throw new Error(`indexed literal intersection unsupported at ${pathLabel}`);
    }

    // preserve the base alias as the sole owner of the selected field
    if (selectedMember !== undefined) {
      throw new Error(`indexed literal intersection touches selected field at ${pathLabel}`);
    }
    return { values: [], hasNull: false };
  }

  // require one explicit readonly required property in every materialized arm
  if (selectedMember === undefined || selectedMember.questionToken !== undefined) {
    throw new Error(`indexed literal selected field unsupported at ${pathLabel}`);
  }

  return collectIndexedLiteralPropertyValues(
    selectedMember.type,
    state,
    selectedField,
    pathLabel,
    activeAliases,
  );
}

// traverse one configured alias or interface declaration with one cycle guard
function collectIndexedLiteralDeclarationValues(
  declaration,
  state,
  selectedField,
  pathLabel,
  activeAliases,
) {
  // retain only exact top-level nongeneric declarations from selected modules
  if (
    declaration.parent !== declaration.getSourceFile() ||
    !ts.isExternalModule(declaration.getSourceFile()) ||
    (declaration.typeParameters?.length ?? 0) > 0 ||
    (!ts.isTypeAliasDeclaration(declaration) && !ts.isInterfaceDeclaration(declaration)) ||
    (ts.isInterfaceDeclaration(declaration) &&
      (!isExported(declaration) || (declaration.heritageClauses?.length ?? 0) > 0))
  ) {
    throw new Error(`indexed literal alias declaration unsupported at ${pathLabel}`);
  }

  // stop configured alias and interface cycles before descending again
  if (activeAliases.has(declaration)) {
    throw new Error(`indexed literal alias cycle at ${pathLabel}: ${declaration.name.text}`);
  }
  activeAliases.add(declaration);

  try {
    // inspect only the selected member of an admitted direct interface
    if (ts.isInterfaceDeclaration(declaration)) {
      return collectIndexedLiteralObject(
        declaration,
        state,
        selectedField,
        pathLabel,
        false,
        activeAliases,
      );
    }
    return collectIndexedLiteralAliasType(
      declaration.type,
      state,
      selectedField,
      pathLabel,
      activeAliases,
    );
  } finally {
    activeAliases.delete(declaration);
  }
}

// traverse one configured closed alias expression
function collectIndexedLiteralAliasType(typeNode, state, selectedField, pathLabel, activeAliases) {
  const node = unwrapParenthesizedType(typeNode);

  // preserve direct source union order across every closed arm
  if (ts.isUnionTypeNode(node)) {
    const collected = { values: [], hasNull: false };

    // visit each arm without evaluating TypeScript conditional semantics
    for (const arm of node.types) {
      const part = collectIndexedLiteralAliasType(
        arm,
        state,
        selectedField,
        pathLabel,
        activeAliases,
      );
      collected.values.push(...part.values);
      collected.hasNull ||= part.hasNull;
    }
    return collected;
  }

  // read only the selected property from exact readonly literal objects
  if (ts.isTypeLiteralNode(node)) {
    return collectIndexedLiteralObject(
      node,
      state,
      selectedField,
      pathLabel,
      false,
      activeAliases,
    );
  }

  // allow one exact Alias & { readonly additions } wrapper
  if (ts.isIntersectionTypeNode(node)) {
    if (
      node.types.length !== 2 ||
      !ts.isTypeReferenceNode(unwrapParenthesizedType(node.types[0])) ||
      !ts.isTypeLiteralNode(unwrapParenthesizedType(node.types[1]))
    ) {
      throw new Error(`indexed literal intersection unsupported at ${pathLabel}`);
    }
    const aliasReference = unwrapParenthesizedType(node.types[0]);
    const augmentation = unwrapParenthesizedType(node.types[1]);
    collectIndexedLiteralObject(
      augmentation,
      state,
      selectedField,
      pathLabel,
      true,
      activeAliases,
    );
    return collectIndexedLiteralAliasType(
      aliasReference,
      state,
      selectedField,
      pathLabel,
      activeAliases,
    );
  }

  // follow only explicitly configured nongeneric direct alias references
  if (
    !ts.isTypeReferenceNode(node) ||
    !ts.isIdentifier(node.typeName) ||
    node.typeArguments !== undefined
  ) {
    throw new Error(`indexed literal alias arm unsupported at ${pathLabel}`);
  }

  const resolved = resolveIndexedLiteralAlias(node.typeName, state, pathLabel);
  return collectIndexedLiteralDeclarationValues(
    resolved.declaration,
    resolved.sourceState,
    selectedField,
    pathLabel,
    activeAliases,
  );
}

// prove one exact Alias["field"] and emit its finite field enum
function renderIndexedLiteralsProjection(typeNode, state, projection, pathLabel) {
  // bind the source spelling to the configured direct field and root alias
  if (
    !ts.isIndexedAccessTypeNode(typeNode) ||
    !ts.isTypeReferenceNode(typeNode.objectType) ||
    !ts.isIdentifier(typeNode.objectType.typeName) ||
    typeNode.objectType.typeName.text !== projection.root ||
    typeNode.objectType.typeArguments !== undefined ||
    !ts.isLiteralTypeNode(typeNode.indexType) ||
    !ts.isStringLiteral(typeNode.indexType.literal) ||
    typeNode.indexType.literal.text !== state.fieldPath.at(-1)
  ) {
    throw new Error(`indexed-literals projection shape unsupported at ${pathLabel}`);
  }

  const collected = collectIndexedLiteralAliasType(
    typeNode.objectType,
    state,
    state.fieldPath.at(-1),
    pathLabel,
    new Set(),
  );
  const values = [];
  const seenValues = new Set();

  // retain each semantic wire value at its first composed occurrence
  for (const value of collected.values) {
    if (!seenValues.has(value)) {
      seenValues.add(value);
      values.push(value);
    }
  }

  // every projected field must have at least one finite string alternative
  if (values.length === 0) {
    throw new Error(`indexed literal projection has no string values at ${pathLabel}`);
  }

  const typeName = fieldTypeName(state, "Enum");
  reserveGeneratedType(state, typeName);
  state.auxiliaryDefinitions.push(renderStringEnumValues(typeName, values, pathLabel));

  // preserve exact null independently from every string alternative
  if (collected.hasNull) {
    state.needsNullable.value = true;
    return `Nullable<${typeName}>`;
  }
  return typeName;
}

// dispatch one consumed direct field projection
function renderFieldProjection(member, state, projection, pathLabel) {
  const typeNode = member.type;

  // extend one exact indexed imported interface field structurally
  if (projection.kind === "indexed-interface-field-extension") {
    return renderIndexedInterfaceFieldExtensionProjection(
      member,
      state,
      projection,
      pathLabel,
    );
  }

  // select the exact configured projection kind
  if (projection.kind === "const-catalog-array") {
    return renderConstCatalogArrayProjection(typeNode, state, projection, pathLabel);
  }

  // derive a finite enum from one explicitly selected indexed alias graph
  if (projection.kind === "indexed-literals") {
    return renderIndexedLiteralsProjection(typeNode, state, projection, pathLabel);
  }

  // subtract one exact literal from one configured finite enum
  if (projection.kind === "exclude-literal") {
    return renderExcludeLiteralProjection(typeNode, state, projection, pathLabel);
  }

  // reuse one exact field type from a configured public interface
  if (projection.kind === "interface-field") {
    return renderInterfaceFieldProjection(typeNode, state, projection, pathLabel);
  }

  // omit one direct field from each element of a configured interface array
  if (projection.kind === "omit-interface-array") {
    return renderOmitInterfaceArrayProjection(typeNode, state, projection, pathLabel);
  }

  return renderReturnEnumProjection(typeNode, state, projection, pathLabel);
}

// identify the opted-in direct typeof-name and number index syntax
function directIndexedConstCatalogName(typeNode) {
  // leave parenthesized queries to the existing catalog path
  if (!ts.isIndexedAccessTypeNode(typeNode) || !ts.isTypeQueryNode(typeNode.objectType) ||
    !ts.isIdentifier(typeNode.objectType.exprName) || typeNode.objectType.typeArguments !== undefined ||
    typeNode.indexType.kind !== ts.SyntaxKind.NumberKeyword) {
    return null;
  }
  return typeNode.objectType.exprName;
}

// render one checker-proven closed const string tuple as a root enum
function renderConstStringTupleRoot(typeNode, state, direct = false) {
  const queryName = direct ? directIndexedConstCatalogName(typeNode) : indexedConstCatalogName(typeNode);

  // require the exact query syntax selected by the root
  if (queryName === null) {
    throw new Error(
      `const string tuple root unsupported at ${state.rootName}: expected ${direct ? "typeof LOCAL_CONST[number]" : "(typeof LOCAL_CONST)[number]"}`,
    );
  }

  const initializer = resolveLocalConstCatalogInitializer(queryName, state);

  // reject assertions, aliases, and every initializer but array as const
  if (
    !ts.isAsExpression(initializer) ||
    !ts.isArrayLiteralExpression(initializer.expression) ||
    !ts.isTypeReferenceNode(initializer.type) ||
    !ts.isIdentifier(initializer.type.typeName) ||
    initializer.type.typeName.text !== "const" ||
    initializer.type.typeArguments !== undefined
  ) {
    throw new Error(`const string tuple source unsupported at root ${state.rootName}`);
  }

  const values = [];

  // reject an empty tuple because it has no string wire value
  if (initializer.expression.elements.length === 0) {
    throw new Error(`const string tuple source unsupported at root ${state.rootName}`);
  }

  // retain only ordinary string literals in exact tuple order
  for (const element of initializer.expression.elements) {
    // reject spreads, elisions, templates, and non-string expressions
    if (!ts.isStringLiteral(element)) {
      throw new Error(`const string tuple source unsupported at root ${state.rootName}`);
    }
    values.push(element.text);
  }

  return {
    definition: renderStringEnumValues(state.rootName, values, `root ${state.rootName}`),
    wireValues: values,
  };
}

// render one explicitly opted-in unsigned safe-integer tuple
function renderUnsignedSafeIntegerTupleCatalogRoot(typeNode, state) {
  const queryName = indexedConstCatalogName(typeNode);

  // keep numeric catalog extraction on the exact indexed query grammar
  if (queryName === null) {
    throw new Error(`unsigned safe integer tuple type unsupported at root ${state.rootName}`);
  }
  const initializer = resolveLocalConstCatalogInitializer(
    queryName,
    state,
    "unsigned safe integer tuple",
  );

  // require one direct nonempty array asserted as const
  if (
    !ts.isAsExpression(initializer) ||
    !ts.isArrayLiteralExpression(initializer.expression) ||
    !ts.isTypeReferenceNode(initializer.type) ||
    !ts.isIdentifier(initializer.type.typeName) ||
    initializer.type.typeName.text !== "const" ||
    initializer.type.typeArguments !== undefined ||
    initializer.expression.elements.length === 0
  ) {
    throw new Error(`unsigned safe integer tuple source unsupported at root ${state.rootName}`);
  }
  return {
    definition: renderNumericVariants(
      state.rootName,
      collectUnsignedIntegerVariants(
        state.rootName,
        initializer.expression.elements,
        (element) => {
          // reject nonordinary members at their original source position
          if (!ts.isNumericLiteral(element)) {
            throw new Error(`unsigned safe integer tuple source unsupported at root ${state.rootName}`);
          }
          return element.getText();
        },
      ),
    ),
  };
}

// render one explicitly opted-in satisfies-constrained const string tuple
function renderSatisfiesStringTupleCatalogRoot(typeNode, state) {
  const queryName = indexedConstCatalogName(typeNode);

  // keep catalog extraction limited to one direct indexed type query
  if (queryName === null) {
    throw new Error(`catalog type unsupported at root ${state.rootName}`);
  }

  const initializer = resolveLocalConstCatalogInitializer(queryName, state);

  // require array as const satisfies readonly NamedSimpleType[] syntax
  if (
    !ts.isSatisfiesExpression(initializer) ||
    !ts.isAsExpression(initializer.expression) ||
    !ts.isArrayLiteralExpression(initializer.expression.expression) ||
    !ts.isTypeReferenceNode(initializer.expression.type) ||
    !ts.isIdentifier(initializer.expression.type.typeName) ||
    initializer.expression.type.typeName.text !== "const" ||
    initializer.expression.type.typeArguments !== undefined ||
    !ts.isTypeOperatorNode(initializer.type) ||
    initializer.type.operator !== ts.SyntaxKind.ReadonlyKeyword ||
    !ts.isArrayTypeNode(initializer.type.type) ||
    !ts.isTypeReferenceNode(initializer.type.type.elementType) ||
    !ts.isIdentifier(initializer.type.type.elementType.typeName) ||
    initializer.type.type.elementType.typeArguments !== undefined
  ) {
    throw new Error(`satisfies string tuple source unsupported at root ${state.rootName}`);
  }

  const elements = initializer.expression.expression.elements;
  const values = [];

  // reject an empty catalog because it has no string wire value
  if (elements.length === 0) {
    throw new Error(`satisfies string tuple source unsupported at root ${state.rootName}`);
  }

  // retain only ordinary string literals in exact tuple order
  for (const element of elements) {
    // reject spreads, elisions, templates, and non-string expressions
    if (!ts.isStringLiteral(element)) {
      throw new Error(`satisfies string tuple source unsupported at root ${state.rootName}`);
    }
    values.push(element.text);
  }

  return {
    definition: renderStringEnumValues(state.rootName, values, `root ${state.rootName}`),
    wireValues: values,
  };
}

// extend one exact configured local string enum by one literal
function renderExtendedLocalStringEnumCatalogRoot(typeNode, state) {
  // require the direct base-first two-arm union without grouping
  if (!ts.isUnionTypeNode(typeNode) || typeNode.types.length !== 2) {
    throw new Error(`extended local string enum shape unsupported at root ${state.rootName}`);
  }
  const [baseType, extensionType] = typeNode.types;

  // require ConfiguredBase | "new-value" in that exact order
  if (
    !ts.isTypeReferenceNode(baseType) ||
    !ts.isIdentifier(baseType.typeName) ||
    baseType.typeName.text !== state.catalog.base ||
    baseType.typeArguments !== undefined ||
    !ts.isLiteralTypeNode(extensionType) ||
    !ts.isStringLiteral(extensionType.literal)
  ) {
    throw new Error(`extended local string enum shape unsupported at root ${state.rootName}`);
  }

  const configured = state.configuredRootsByName.get(state.catalog.base);
  const checker = getSourceChecker(
    state.checkers,
    state.sourcePath,
    state.sourceFile,
    state.utilityDeclarations,
  );
  const referenceSymbol = checker.getSymbolAtLocation(baseType.typeName);
  const exportSymbol = configuredLeafExport(checker, state.sourceFile, state.catalog.base);
  const baseDeclaration = configured?.declaration;

  // bind the name to one direct same-source configured exported alias
  if (
    configured === undefined ||
    configured.identity === state.rootIdentity ||
    configured.root.source !== state.sourcePath ||
    referenceSymbol === undefined ||
    (referenceSymbol.flags & ts.SymbolFlags.Alias) !== 0 ||
    referenceSymbol.declarations?.length !== 1 ||
    referenceSymbol.declarations[0] !== baseDeclaration ||
    exportSymbol === null ||
    (exportSymbol.flags & ts.SymbolFlags.Alias) !== 0 ||
    exportSymbol.declarations?.length !== 1 ||
    exportSymbol.declarations[0] !== baseDeclaration ||
    !ts.isTypeAliasDeclaration(baseDeclaration) ||
    baseDeclaration.parent !== state.sourceFile ||
    !isExported(baseDeclaration) ||
    (baseDeclaration.typeParameters?.length ?? 0) > 0 ||
    !ts.isUnionTypeNode(baseDeclaration.type) ||
    baseDeclaration.type.types.length < 2
  ) {
    throw new Error(`extended local string enum base unsupported at root ${state.rootName}`);
  }

  const baseValues = [];

  // retain only direct ordinary string literals in base declaration order
  for (const member of baseDeclaration.type.types) {
    // reject templates, references, aliases, and nested unions
    if (!ts.isLiteralTypeNode(member) || !ts.isStringLiteral(member.literal)) {
      throw new Error(`extended local string enum base unsupported at root ${state.rootName}`);
    }
    baseValues.push(member.literal.text);
  }

  state.dependencies.get(state.rootIdentity).add(configured.identity);
  const baseRendered = state.renderConfiguredRoot(configured.identity);

  // require the configured base to retain this exact enum value order
  if (
    baseRendered.kind !== "string-literal-enum" ||
    baseRendered.wireValues?.length !== baseValues.length ||
    !baseValues.every(
      // compare each already validated direct base literal
      (value, index) => baseRendered.wireValues[index] === value,
    )
  ) {
    throw new Error(`extended local string enum base unsupported at root ${state.rootName}`);
  }

  const values = [...baseValues, extensionType.literal.text];
  return {
    definition: renderStringEnumValues(state.rootName, values, `root ${state.rootName}`),
    wireValues: values,
  };
}

// render one selected string property from a closed const object catalog
function renderStringPropertyCatalogRoot(typeNode, state) {
  const selectedKey = state.catalog.key;

  // require exact (typeof LOCAL_CONST)[number]["configuredKey"] syntax
  if (
    !ts.isIndexedAccessTypeNode(typeNode) ||
    !ts.isLiteralTypeNode(typeNode.indexType) ||
    !ts.isStringLiteral(typeNode.indexType.literal) ||
    typeNode.indexType.literal.text !== selectedKey ||
    !ts.isIndexedAccessTypeNode(typeNode.objectType)
  ) {
    throw new Error(`catalog type unsupported at root ${state.rootName}`);
  }

  const queryName = indexedConstCatalogName(typeNode.objectType);

  // require the inner indexed access to name one local const catalog
  if (queryName === null) {
    throw new Error(`catalog type unsupported at root ${state.rootName}`);
  }

  const initializer = resolveLocalConstCatalogInitializer(queryName, state);

  // require one direct non-empty object array as const
  if (
    !ts.isAsExpression(initializer) ||
    !ts.isArrayLiteralExpression(initializer.expression) ||
    !ts.isTypeReferenceNode(initializer.type) ||
    !ts.isIdentifier(initializer.type.typeName) ||
    initializer.type.typeName.text !== "const" ||
    initializer.type.typeArguments !== undefined ||
    initializer.expression.elements.length === 0
  ) {
    throw new Error(`string property catalog source unsupported at root ${state.rootName}`);
  }

  const values = [];

  // validate every row without interpreting or resolving nonselected values
  for (const row of initializer.expression.elements) {
    // admit only ordinary non-empty object literal rows
    if (!ts.isObjectLiteralExpression(row) || row.properties.length === 0) {
      throw new Error(`string property catalog source unsupported at root ${state.rootName}`);
    }

    const propertyNames = new Set();
    let selectedValue;

    // require uniquely named own simple data properties in every row
    for (const property of row.properties) {
      // reject spreads, shorthand, methods, accessors, computed names, and __proto__
      if (
        !ts.isPropertyAssignment(property) ||
        !ts.isIdentifier(property.name) ||
        property.name.text === "__proto__" ||
        propertyNames.has(property.name.text)
      ) {
        throw new Error(`string property catalog source unsupported at root ${state.rootName}`);
      }
      propertyNames.add(property.name.text);

      const value = property.initializer;

      // admit only ordinary string literals or canonical safe nonnegative integers
      if (ts.isStringLiteral(value)) {
        // retain the configured string property exactly once per row
        if (property.name.text === selectedKey) {
          selectedValue = value.text;
        }
        continue;
      }

      // reject every other value syntax and noncanonical numeric spelling
      if (!ts.isNumericLiteral(value)) {
        throw new Error(`string property catalog source unsupported at root ${state.rootName}`);
      }
      const token = value.getText();

      // constrain numeric metadata to canonical safe nonnegative integers
      if (!/^(?:0|[1-9][0-9]*)$/u.test(token) || !Number.isSafeInteger(Number(token))) {
        throw new Error(`string property catalog source unsupported at root ${state.rootName}`);
      }

      // reject a numeric value for the configured string property
      if (property.name.text === selectedKey) {
        throw new Error(`string property catalog source unsupported at root ${state.rootName}`);
      }
    }

    // require the configured property exactly once as an ordinary string literal
    if (selectedValue === undefined) {
      throw new Error(`string property catalog source unsupported at root ${state.rootName}`);
    }
    values.push(selectedValue);
  }

  return {
    definition: renderStringEnumValues(state.rootName, values, `root ${state.rootName}`),
    wireValues: values,
  };
}

// render one supported TypeScript type node without semantic evaluation
function renderRustType(typeNode, state) {
  const pathLabel = `${state.rootName}.${state.fieldPath.join(".")}`;

  // preserve primitive string storage and optional UTC metadata
  if (typeNode.kind === ts.SyntaxKind.StringKeyword) {
    consumeFieldMapping(state, new Set(["utc-iso"]), "string");
    return "String";
  }

  // preserve primitive boolean storage
  if (typeNode.kind === ts.SyntaxKind.BooleanKeyword) {
    consumeFieldMapping(state, new Set(), "boolean");
    return "bool";
  }

  // require an explicit safe numeric representation rule
  if (typeNode.kind === ts.SyntaxKind.NumberKeyword) {
    const mapping = state.fieldMappings.get(state.fieldPath.join("."));

    // reject JavaScript number fields without a reviewed representation
    if (mapping === undefined) {
      throw new Error(`unmapped numeric field at ${pathLabel}`);
    }
    consumeFieldMapping(state, NUMERIC_FIELD_MAPPING_KINDS, "number");

    // select the exact reviewed Rust storage
    if (mapping === "safe-unsigned-integer") {
      return "u64";
    }

    // preserve signed safe integer intent
    if (mapping === "safe-integer") {
      return "i64";
    }

    return "f64";
  }

  // preserve exact string and boolean literals with zero-sized marker types
  if (ts.isLiteralTypeNode(typeNode)) {
    // preserve one exact null literal only within an opted-in tagged payload
    if (state.allowExactNullLiteral && typeNode.literal.kind === ts.SyntaxKind.NullKeyword) {
      consumeFieldMapping(state, new Set(), "null literal");
      return renderNullMarker(state);
    }

    // preserve one exact string literal
    if (ts.isStringLiteralLike(typeNode.literal)) {
      consumeFieldMapping(state, new Set(), "string literal");
      return renderLiteralMarker(state, typeNode.literal.text, "string");
    }

    // preserve one exact true or false literal
    if (
      typeNode.literal.kind === ts.SyntaxKind.TrueKeyword ||
      typeNode.literal.kind === ts.SyntaxKind.FalseKeyword
    ) {
      consumeFieldMapping(state, new Set(), "boolean literal");
      return renderLiteralMarker(
        state,
        typeNode.literal.kind === ts.SyntaxKind.TrueKeyword,
        "boolean",
      );
    }

    throw new Error(`literal type unsupported at ${pathLabel}`);
  }

  // preserve null separately from missing with one named wrapper
  if (ts.isUnionTypeNode(typeNode)) {
    const nullMembers = typeNode.types.filter((member) =>
      ts.isLiteralTypeNode(member) && member.literal.kind === ts.SyntaxKind.NullKeyword,
    );
    const undefinedMembers = typeNode.types.filter((member) => member.kind === ts.SyntaxKind.UndefinedKeyword);

    // reject explicit undefined rather than collapsing it into missing
    if (undefinedMembers.length > 0) {
      throw new Error(`undefined type unsupported at ${pathLabel}`);
    }

    const valueMembers = typeNode.types.filter((member) =>
      !ts.isLiteralTypeNode(member) || member.literal.kind !== ts.SyntaxKind.NullKeyword,
    );

    // classify only unions whose value alternatives are exact strings
    const isStringLiteralUnion = valueMembers.length >= 2 && valueMembers.every((member) =>
      ts.isLiteralTypeNode(member) && ts.isStringLiteralLike(member.literal),
    );

    // preserve every exact alternative in a homogeneous finite string union
    if (isStringLiteralUnion) {
      // reject repeated null alternatives instead of normalizing them
      if (nullMembers.length > 1) {
        throw new Error(`union type unsupported at ${pathLabel}`);
      }

      const enumType = renderStringFieldEnum(valueMembers, state, pathLabel);

      // preserve required null separately from every enum value
      if (nullMembers.length === 1) {
        state.needsNullable.value = true;
        return `Nullable<${enumType}>`;
      }

      return enumType;
    }

    // accept only one value alternative plus one null alternative
    if (nullMembers.length === 1 && valueMembers.length === 1) {
      state.needsNullable.value = true;
      return `Nullable<${renderRustType(valueMembers[0], state)}>`;
    }

    throw new Error(`union type unsupported at ${pathLabel}`);
  }

  // support only readonly array and tuple operators
  if (ts.isTypeOperatorNode(typeNode) && typeNode.operator === ts.SyntaxKind.ReadonlyKeyword) {
    // render a readonly array as owned immutable-contract storage
    if (ts.isArrayTypeNode(typeNode.type)) {
      let element = typeNode.type.elementType;

      // remove grouping without widening the supported element types
      while (ts.isParenthesizedTypeNode(element)) {
        element = element.type;
      }
      return `Vec<${renderRustType(element, state)}>`;
    }

    // render a readonly fixed tuple without losing arity
    if (ts.isTupleTypeNode(typeNode.type)) {
      return renderTupleType(typeNode.type, state);
    }

    throw new Error(`readonly operator unsupported at ${pathLabel}`);
  }

  // reject mutable array and tuple contracts
  if (ts.isArrayTypeNode(typeNode) || ts.isTupleTypeNode(typeNode)) {
    throw new Error(`mutable array or tuple unsupported at ${pathLabel}`);
  }

  // support explicit readonly type literals
  if (ts.isTypeLiteralNode(typeNode)) {
    return renderNestedObject(typeNode, state);
  }

  // admit only checker-proven references to configured roots in this source
  if (ts.isTypeReferenceNode(typeNode)) {
    return renderConfiguredTypeReference(typeNode, state, pathLabel);
  }

  throw new Error(`TypeScript type unsupported at ${pathLabel}: ${ts.SyntaxKind[typeNode.kind]}`);
}

// prove the one reviewed defaulted generic list parameter root
function resolveDefaultedListParameter(declaration, state) {
  const metadata = state.defaultedListParameter;

  // prevent one generic parameter from shadowing any configured or emitted type
  if (state.generatedTypeNames.has(metadata.name)) {
    throw new Error(`defaulted list parameter Rust type collision ${metadata.name}`);
  }
  state.generatedTypeNames.add(metadata.name);

  // require one exported direct non-inheriting interface parameter declaration
  if (
    !ts.isInterfaceDeclaration(declaration) ||
    declaration.parent !== state.sourceFile ||
    !isExported(declaration) ||
    (declaration.heritageClauses?.length ?? 0) > 0 ||
    declaration.typeParameters?.length !== 1
  ) {
    throw new Error(`defaulted list parameter root unsupported at ${state.rootName}`);
  }
  const parameter = declaration.typeParameters[0];
  const defaultType = parameter.default;

  // require one unconstrained unmodified parameter with one direct imported default
  if (
    parameter.name.text !== metadata.name ||
    parameter.constraint !== undefined ||
    (parameter.modifiers?.length ?? 0) > 0 ||
    defaultType === undefined ||
    !ts.isTypeReferenceNode(defaultType) ||
    !ts.isIdentifier(defaultType.typeName) ||
    defaultType.typeName.text !== metadata.defaultRoot ||
    defaultType.typeArguments !== undefined
  ) {
    throw new Error(`defaulted list parameter declaration unsupported at ${state.rootName}`);
  }

  const directFields = declaration.members.filter((member) =>
    ts.isPropertySignature(member) &&
    ts.isIdentifier(member.name) &&
    member.name.text === metadata.field
  );
  const field = directFields[0];
  const fieldType = field?.type;
  const elementType = fieldType !== undefined &&
    ts.isTypeOperatorNode(fieldType) &&
    fieldType.operator === ts.SyntaxKind.ReadonlyKeyword &&
    ts.isArrayTypeNode(fieldType.type)
    ? fieldType.type.elementType
    : undefined;

  // require one direct required readonly field with exact readonly Parameter[] syntax
  if (
    directFields.length !== 1 ||
    field.questionToken !== undefined ||
    !isReadonly(field) ||
    elementType === undefined ||
    !ts.isTypeReferenceNode(elementType) ||
    !ts.isIdentifier(elementType.typeName) ||
    elementType.typeName.text !== metadata.name ||
    elementType.typeArguments !== undefined
  ) {
    throw new Error(`defaulted list parameter field unsupported at ${state.rootName}.${metadata.field}`);
  }

  const checker = getSourceChecker(
    state.checkers,
    state.sourcePath,
    state.sourceFile,
    state.utilityDeclarations,
  );
  const parameterSymbol = checker.getSymbolAtLocation(parameter.name);
  const elementSymbol = checker.getSymbolAtLocation(elementType.typeName);

  // bind the list element to the exact sole type parameter declaration
  if (
    parameterSymbol === undefined ||
    (parameterSymbol.flags & ts.SymbolFlags.TypeParameter) === 0 ||
    parameterSymbol.declarations?.length !== 1 ||
    parameterSymbol.declarations[0] !== parameter ||
    elementSymbol !== parameterSymbol
  ) {
    throw new Error(`defaulted list parameter identity unsupported at ${state.rootName}`);
  }

  const imported = resolveApprovedTypeImport(defaultType.typeName, state, `root ${state.rootName}`);
  const configured = imported.configured;
  const defaultDeclaration = configured.declaration;

  // bind the default to one differently owned configured public nongeneric interface
  if (
    imported.exportName !== metadata.defaultRoot ||
    configured.root.exportName !== metadata.defaultRoot ||
    configured.root.source === state.sourcePath ||
    configured.root.localAlias === true ||
    configured.root.localInterface === true ||
    defaultDeclaration.getSourceFile() === state.sourceFile ||
    defaultDeclaration.parent !== defaultDeclaration.getSourceFile() ||
    !ts.isExternalModule(defaultDeclaration.getSourceFile()) ||
    !isExported(defaultDeclaration) ||
    !ts.isInterfaceDeclaration(defaultDeclaration) ||
    (defaultDeclaration.typeParameters?.length ?? 0) > 0
  ) {
    throw new Error(`defaulted list parameter default root unsupported at ${state.rootName}`);
  }

  state.dependencies.get(state.rootIdentity).add(configured.identity);
  const renderedDefault = state.renderConfiguredRoot(configured.identity);

  // require the default interface to pass the existing readonly renderer unchanged
  if (
    renderedDefault.kind !== "readonly-interface" ||
    renderedDefault.interfaceFieldBlocks === undefined
  ) {
    throw new Error(`defaulted list parameter default root unsupported at ${state.rootName}`);
  }
  imported.binding.used.add(imported.exportName);
  return { name: metadata.name, defaultRoot: metadata.defaultRoot, field };
}

// prove the single imported marker base before rendering named row fields
function provePgRowNamedFields(heritageClauses, state) {
  const clause = heritageClauses[0];
  const base = clause?.types[0];

  // admit only a source-owned exact argument-free extends QueryResultRow clause
  if (
    !ts.isExternalModule(state.sourceFile) ||
    hasGlobalAugmentation(state.sourceFile) ||
    hasExternalModuleAugmentation(state.sourceFile) ||
    heritageClauses.length !== 1 ||
    clause.token !== ts.SyntaxKind.ExtendsKeyword ||
    clause.types.length !== 1 ||
    !ts.isIdentifier(base.expression) ||
    base.expression.text !== "QueryResultRow" ||
    base.typeArguments !== undefined
  ) {
    throw new Error("PostgreSQL row marker base unsupported at root " + state.rootName);
  }

  const checker = getSourceChecker(
    state.checkers,
    state.sourcePath,
    state.sourceFile,
    state.utilityDeclarations,
  );
  const symbol = checker.getSymbolAtLocation(base.expression);
  const imported = symbol?.declarations?.[0];

  // prove the local edge without exposing unrelated pg types to the checker
  if (
    symbol === undefined ||
    (symbol.flags & ts.SymbolFlags.Alias) === 0 ||
    symbol.declarations?.length !== 1 ||
    !ts.isImportSpecifier(imported) ||
    imported.propertyName !== undefined ||
    imported.name.text !== "QueryResultRow"
  ) {
    throw new Error("PostgreSQL row marker import unsupported at root " + state.rootName);
  }
  const importClause = imported.parent.parent;
  const importDeclaration = importClause.parent;

  // keep the marker bound to an ordinary unaliased pg type import
  if (
    importClause.isTypeOnly !== true ||
    importClause.name !== undefined ||
    importDeclaration.getSourceFile() !== state.sourceFile ||
    importDeclaration.attributes !== undefined ||
    !ts.isStringLiteral(importDeclaration.moduleSpecifier) ||
    importDeclaration.moduleSpecifier.text !== "pg" ||
    resolvedSymbolDeclaration(checker, symbol) !== state.pgRowMarkerDeclaration
  ) {
    throw new Error("PostgreSQL row marker import unsupported at root " + state.rootName);
  }
}

// prove an exact type-only pg Pool declaration at the first parameter
function isPinnedPgPoolParameter(parameter, pool, checker) {
  const type = parameter?.type;

  // require a direct required parameter with the original named annotation
  if (parameter === undefined || !ts.isIdentifier(parameter.name) ||
    parameter.questionToken !== undefined || parameter.initializer !== undefined ||
    parameter.dotDotDotToken !== undefined || type === undefined ||
    !ts.isTypeReferenceNode(type) || !ts.isIdentifier(type.typeName) ||
    type.typeName.text !== "Pool" || type.typeArguments !== undefined) {
    return false;
  }
  const symbol = checker.getSymbolAtLocation(type.typeName);
  const imported = symbol?.declarations?.[0];

  // keep the annotation on the exact named imported standard pg type
  if (symbol === undefined || (symbol.flags & ts.SymbolFlags.Alias) === 0 ||
    symbol.declarations?.length !== 1 || !ts.isImportSpecifier(imported) ||
    imported.propertyName !== undefined || resolvedSymbolDeclaration(checker, symbol) !== pool) {
    return false;
  }
  const clause = imported.parent.parent;
  return (clause.isTypeOnly || imported.isTypeOnly) &&
    ts.isImportDeclaration(clause.parent) &&
    ts.isStringLiteral(clause.parent.moduleSpecifier) && clause.parent.moduleSpecifier.text === "pg";
}

// compare the closed SELECT projection to this exact row's named fields
function provePrivatePgSelectFields(sql, row, state) {
  const error = `private PostgreSQL row SELECT fields unsupported at ${state.rootName}`;
  const projection = /^\s*SELECT\s+([\s\S]+?)\s+FROM\s+[A-Za-z_][A-Za-z0-9_]*\b/iu.exec(sql);

  // require one explicit projection list before the original FROM clause
  if (projection === null) {
    throw new Error(error);
  }
  const outputs = new Set();

  // admit only qualified columns and the original aliased jsonb text form
  for (const item of projection[1].split(",")) {
    const column = /^[A-Za-z_][A-Za-z0-9_]*\.([A-Za-z_][A-Za-z0-9_]*)(?:\s*->>\s*'([A-Za-z_][A-Za-z0-9_]*)')?(?:\s+AS\s+(?:"([A-Za-z_][A-Za-z0-9_]*)"|([A-Za-z_][A-Za-z0-9_]*)))?$/iu.exec(item.trim());

    // require aliases for expressions and reject ambiguous non-column syntax
    if (column === null || (column[2] !== undefined && column[3] === undefined && column[4] === undefined)) {
      throw new Error(error);
    }
    const output = column[3] ?? (column[4] ?? column[1]).toLowerCase();

    // preserve one unambiguous SQL result name per selected expression
    if (outputs.has(output)) {
      throw new Error(error);
    }
    outputs.add(output);
  }

  // compare the complete output set while retaining original TS field order in Rust
  if (outputs.size !== row.members.length || !row.members.every(
    // consume every matching declared identifier exactly once
    (member) => ts.isPropertySignature(member) && ts.isIdentifier(member.name) && outputs.delete(member.name.text),
  )) {
    throw new Error(error);
  }
}

// compare exact numbered placeholders to the source binding slots without evaluating them
function provePrivatePgSelectBindings(sql, bindings, state) {
  const error = `private PostgreSQL row SELECT bindings unsupported at ${state.rootName}`;
  const unquoted = sql.replace(/'(?:[^'\\]|'')*'|"(?:[^"\\]|"")*"/gu, " ");
  const placeholders = new Set(Array.from(unquoted.matchAll(/\$([0-9]+)/gu),
    // retain the exact placeholder spelling rather than normalizing zero or leading zeros
    (match) => match[1]));

  // reject unsupported quoting/comment forms and variable binding-list shapes
  if (/(?:--|\/\*|['";\\]|\$(?![0-9]))/u.test(unquoted) || bindings.elements.length === 0 ||
    bindings.elements.some(
      // require one actual binding expression in every literal array slot
      (element) => ts.isSpreadElement(element) || ts.isOmittedExpression(element),
    ) || placeholders.size !== bindings.elements.length ||
    !bindings.elements.every(
      // require contiguous one-based placeholders for the exact number of bindings
      (_, index) => placeholders.has(String(index + 1)),
    )) {
    throw new Error(error);
  }
}

// bind one selected query call to its direct exported async function
function privatePgQueryOwner(call, row, pool, state, checker) {
  const error = `private PostgreSQL row query site unsupported at ${state.rootName}`;
  const query = call.expression;
  const rowType = call.typeArguments[0];
  const sql = call.arguments[0];
  const bindings = call.arguments[1];
  const awaited = call.parent;
  const target = awaited?.parent;
  const list = target?.parent;
  const statement = list?.parent;
  const block = statement?.parent;
  const owner = block?.parent;

  // admit only a direct awaited SELECT literal with an ordinary binding array
  if (call.questionDotToken !== undefined || query.questionDotToken !== undefined ||
    !ts.isIdentifier(query.expression) || call.typeArguments.length !== 1 ||
    rowType.typeArguments !== undefined ||
    checker.getSymbolAtLocation(rowType.typeName)?.declarations?.[0] !== row ||
    call.arguments.length !== 2 ||
    !(ts.isStringLiteral(sql) || ts.isNoSubstitutionTemplateLiteral(sql)) ||
    !/^\s*SELECT\b/iu.test(sql.text) || !ts.isArrayLiteralExpression(bindings) ||
    !ts.isAwaitExpression(awaited) || target === undefined || !ts.isVariableDeclaration(target) ||
    target.initializer !== awaited || !ts.isIdentifier(target.name) ||
    list === undefined || !ts.isVariableDeclarationList(list) || (list.flags & ts.NodeFlags.Const) === 0 ||
    list.declarations.length !== 1 || statement === undefined || !ts.isVariableStatement(statement) ||
    block === undefined || !ts.isBlock(block) || owner === undefined || !ts.isFunctionDeclaration(owner) ||
    owner.parent !== state.sourceFile || owner.body !== block || owner.name === undefined ||
    owner.asteriskToken !== undefined || owner.typeParameters !== undefined ||
    !isExported(owner) || ts.getModifiers(owner)?.length !== 2 || !ts.getModifiers(owner).some(
      // require the direct async declaration that owns the awaited read
      (modifier) => modifier.kind === ts.SyntaxKind.AsyncKeyword,
    )) {
    throw new Error(error);
  }
  const parameter = owner.parameters[0];
  const ownerSymbol = configuredLeafExport(checker, state.sourceFile, owner.name.text);
  const receiver = checker.getSymbolAtLocation(query.expression);
  const method = checker.getResolvedSignature(call)?.getDeclaration();
  const rowConstraint = method?.typeParameters?.[0]?.constraint;

  // bind both the exported owner and the receiver to their exact declarations
  if (ownerSymbol === null || resolvedSymbolDeclaration(checker, ownerSymbol) !== owner ||
    !isPinnedPgPoolParameter(parameter, pool, checker) ||
    receiver?.declarations?.length !== 1 || receiver.declarations[0] !== parameter ||
    checker.getSymbolAtLocation(parameter.name) !== receiver ||
    method === undefined || !ts.isMethodDeclaration(method) || method.parent !== pool ||
    !ts.isIdentifier(method.name) || method.name.text !== "query" || method.parameters.length !== 2 ||
    !ts.isIdentifier(method.parameters[0].name) || method.parameters[0].name.text !== "queryTextOrConfig" ||
    !ts.isIdentifier(method.parameters[1].name) || method.parameters[1].name.text !== "values" ||
    rowConstraint === undefined || !ts.isTypeReferenceNode(rowConstraint) ||
    checker.getSymbolAtLocation(rowConstraint.typeName)?.declarations?.[0] !== state.pgRowMarkerDeclaration) {
    throw new Error(error);
  }
  provePrivatePgSelectFields(sql.text, row, state);
  provePrivatePgSelectBindings(sql.text, bindings, state);
  return owner.name.text;
}

// prove every scoped private row use against the explicit original owner list
function provePrivatePgQueryOwners(declaration, state) {
  const checker = getSourceChecker(state.checkers, state.sourcePath, state.sourceFile, state.utilityDeclarations);
  const moduleSymbol = checker.getSymbolAtLocation(state.sourceFile);
  const symbol = checker.getSymbolAtLocation(declaration.name);

  // reject merged or effectively exported names before selecting their query uses
  if (declaration.modifiers !== undefined ||
    symbol?.declarations?.length !== 1 || symbol.declarations[0] !== declaration ||
    moduleSymbol === undefined || checker.getExportsOfModule(moduleSymbol).some(
      // include a renamed export of the same interface
      (entry) => resolvedSymbolDeclaration(checker, entry) === declaration,
    )) {
    throw new Error(`private PostgreSQL row declaration unsupported at ${state.rootName}`);
  }
  const pool = findUtilityDeclaration(
    state.pgRowMarkerDeclaration.getSourceFile(), "Pool", ts.isClassDeclaration,
  );
  const owners = [];

  // inspect syntax only and keep matching query owners in source order
  function visit(node) {
    // select direct query calls that explicitly name this row as their type argument
    if (ts.isCallExpression(node) && ts.isPropertyAccessExpression(node.expression) &&
      node.expression.name.text === "query" && node.typeArguments?.length > 0 &&
      ts.isTypeReferenceNode(node.typeArguments[0]) &&
      ts.isIdentifier(node.typeArguments[0].typeName) &&
      node.typeArguments[0].typeName.text === state.rootName) {
      owners.push(privatePgQueryOwner(node, declaration, pool, state, checker));
    }
    ts.forEachChild(node, visit);
  }
  visit(state.sourceFile);

  // require exactly one proven query for each configured owner and no others
  if (owners.length !== state.pgQueryOwners.length || owners.some(
    // retain the configured source owner order
    (owner, index) => owner !== state.pgQueryOwners[index],
  )) {
    throw new Error(`private PostgreSQL row query owners unsupported at ${state.rootName}`);
  }
}

// prove a direct required readonly primitive string on one exact interface
function directReadonlyStringField(declaration, name, checker) {
  const matches = declaration.members.filter(
    // count every direct ordinary property with the configured name
    (member) => member.name !== undefined && ts.isIdentifier(member.name) && member.name.text === name,
  );
  const member = matches[0];

  // reject aliases, optionality, mutability, and duplicate declarations
  if (matches.length !== 1 || !ts.isPropertySignature(member) ||
    member.questionToken !== undefined || !isReadonly(member) || member.modifiers?.length !== 1 ||
    member.type?.kind !== ts.SyntaxKind.StringKeyword) {
    return undefined;
  }
  const symbol = checker.getSymbolAtLocation(member.name);
  const parentSymbol = checker.getSymbolAtLocation(declaration.name);

  // bind the property to the exact own declaration and primitive compiler type
  if (symbol?.declarations?.length !== 1 || symbol.declarations[0] !== member ||
    parentSymbol === undefined || checker.getPropertyOfType(checker.getDeclaredTypeOfSymbol(parentSymbol), name) !== symbol ||
    checker.getTypeAtLocation(member.type).flags !== ts.TypeFlags.String) {
    return undefined;
  }
  return member;
}

// retain one proven inherited string field instead of emitting the duplicate
function withoutIdenticalImportedStrings(declaration, base, inheritedBlocks, checker, state) {
  const repeated = new Set();

  // admit only the direct storage-identical property pairs named by the root
  for (const name of state.identicalImportedStringFields) {
    const own = directReadonlyStringField(declaration, name, checker);
    const baseMember = directReadonlyStringField(base, name, checker);
    const blocks = inheritedBlocks.filter(
      // check the exact inherited field block before omitting its duplicate
      (entry) => entry.name === name,
    );
    const pathLabel = `${state.rootName}.${name}`;
    const fieldName = rustFieldIdentifier(name, pathLabel);

    // reject missing, mapped, or transformed storage on either side
    if (own === undefined || baseMember === undefined || blocks.length !== 1 ||
      blocks[0].rustType !== "String" || blocks[0].lines.length !== 1 ||
      blocks[0].lines[0] !== `    pub ${fieldName}: String,`) {
      throw new Error(`identical imported readonly string unsupported at ${pathLabel}`);
    }
    repeated.add(own);
  }

  // preserve every other source member and its original order
  return declaration.members.filter((member) => !repeated.has(member));
}

// render one interface with an optional exact flattened base
function renderInterfaceRoot(declaration, state) {
  const heritageClauses = declaration.heritageClauses ?? [];
  const defaultedListParameterProof = state.defaultedListParameter === undefined
    ? undefined
    : resolveDefaultedListParameter(declaration, state);
  const interfaceState = defaultedListParameterProof === undefined
    ? state
    : { ...state, defaultedListParameterProof };

  // retain the historical closed boundary without explicit base metadata
  if (state.pgRowNamedFields) {
    provePgRowNamedFields(heritageClauses, state);

    // private rows require exact source query ownership before rendering
    if (state.pgQueryOwners !== undefined) {
      provePrivatePgQueryOwners(declaration, state);
    }
  } else if (state.base === undefined && heritageClauses.length > 0) {
    throw new Error(`interface inheritance unsupported at root ${state.rootName}`);
  }

  let inheritedFields = [];
  let inheritedFieldNames = new Set();
  let inheritedFieldBlocks = [];
  let ownMembers = declaration.members;

  // validate and flatten one explicitly configured base
  if (state.base !== undefined) {
    const clause = heritageClauses[0];
    const heritageType = clause?.types[0];

    const checker = getSourceChecker(
      state.checkers,
      state.sourcePath,
      state.sourceFile,
      state.utilityDeclarations,
    );
    let configured;
    let baseDeclaration;
    let importedBase;

    // admit one exact configured type-only imported interface base
    if (state.importedBase) {
      // require exactly one direct argument-free imported extends clause
      if (
        heritageClauses.length !== 1 ||
        clause.token !== ts.SyntaxKind.ExtendsKeyword ||
        clause.types.length !== 1 ||
        heritageType === undefined ||
        !ts.isIdentifier(heritageType.expression) ||
        heritageType.expression.text !== state.base ||
        heritageType.typeArguments !== undefined
      ) {
        throw new Error(`configured imported interface base unsupported at root ${state.rootName}`);
      }

      importedBase = resolveApprovedTypeImport(
        heritageType.expression,
        state,
        `root ${state.rootName}`,
      );
      configured = importedBase.configured;
      baseDeclaration = configured.declaration;

      // bind expression, export, config, and declaration to one foreign public interface
      if (
        importedBase.exportName !== state.base ||
        configured.root.exportName !== state.base ||
        configured.root.source === state.sourcePath ||
        configured.root.localAlias === true ||
        configured.root.localInterface === true ||
        baseDeclaration.getSourceFile() === state.sourceFile ||
        baseDeclaration.parent !== baseDeclaration.getSourceFile() ||
        !ts.isExternalModule(baseDeclaration.getSourceFile()) ||
        !isExported(baseDeclaration) ||
        !ts.isInterfaceDeclaration(baseDeclaration) ||
        (baseDeclaration.typeParameters?.length ?? 0) > 0
      ) {
        throw new Error(`configured imported interface base identity unsupported at root ${state.rootName}`);
      }

      // consume the exact approved import only after every base proof succeeds
      importedBase.binding.used.add(importedBase.exportName);
    // retain the ordinary direct same-source base grammar unchanged
    } else if (state.omitBaseFields === undefined) {
      // require exactly one direct argument-free extends clause
      if (
        heritageClauses.length !== 1 ||
        clause.token !== ts.SyntaxKind.ExtendsKeyword ||
        clause.types.length !== 1 ||
        heritageType === undefined ||
        !ts.isIdentifier(heritageType.expression) ||
        heritageType.typeArguments !== undefined
      ) {
        throw new Error(`configured interface base unsupported at root ${state.rootName}`);
      }

      const symbol = checker.getSymbolAtLocation(heritageType.expression);
      const declarations = symbol?.declarations;

      // require one non-alias symbol bound to the configured declaration object
      if (
        symbol === undefined ||
        (symbol.flags & ts.SymbolFlags.Alias) !== 0 ||
        declarations?.length !== 1
      ) {
        throw new Error(`configured interface base identity unsupported at root ${state.rootName}`);
      }

      baseDeclaration = declarations[0];
      configured = state.configuredDeclarations.get(baseDeclaration);

      // keep ordinary inheritance inside its prior exact source graph
      if (
        configured === undefined ||
        configured.root.source !== state.sourcePath ||
        configured.root.exportName !== state.base ||
        heritageType.expression.text !== state.base ||
        baseDeclaration.getSourceFile() !== state.sourceFile ||
        baseDeclaration.parent !== state.sourceFile ||
        !ts.isInterfaceDeclaration(baseDeclaration) ||
        (baseDeclaration.typeParameters?.length ?? 0) > 0
      ) {
        throw new Error(`configured interface base identity unsupported at root ${state.rootName}`);
      }
    } else {
      // require exactly one standard Omit heritage expression
      if (
        heritageClauses.length !== 1 ||
        clause.token !== ts.SyntaxKind.ExtendsKeyword ||
        clause.types.length !== 1 ||
        heritageType === undefined ||
        !ts.isIdentifier(heritageType.expression) ||
        heritageType.expression.text !== "Omit" ||
        heritageType.typeArguments?.length !== 2
      ) {
        throw new Error(`configured omitted interface base unsupported at root ${state.rootName}`);
      }

      const omitSymbol = checker.getSymbolAtLocation(heritageType.expression);
      const omitDeclarations = omitSymbol?.declarations;

      // pin Omit to the verified standard TypeScript declaration
      if (
        state.utilityDeclarations === undefined ||
        omitSymbol === undefined ||
        (omitSymbol.flags & ts.SymbolFlags.Alias) !== 0 ||
        omitDeclarations?.length !== 1 ||
        omitDeclarations[0] !== state.utilityDeclarations.declarations.Omit
      ) {
        throw new Error(
          `configured omitted interface utility identity unsupported at root ${state.rootName}`,
        );
      }

      const baseType = heritageType.typeArguments[0];
      const omittedType = heritageType.typeArguments[1];

      // require one direct argument-free base identifier
      if (
        !ts.isTypeReferenceNode(baseType) ||
        !ts.isIdentifier(baseType.typeName) ||
        baseType.typeArguments !== undefined ||
        baseType.typeName.text !== state.base
      ) {
        throw new Error(
          `configured omitted interface base identity unsupported at root ${state.rootName}`,
        );
      }

      const omittedTypes = ts.isUnionTypeNode(omittedType)
        ? omittedType.types
        : [omittedType];
      const omittedNames = [];

      // admit only exact string literal keys in source order
      for (const omitted of omittedTypes) {
        // reject keyof, aliases, templates, and non-string literals
        if (!ts.isLiteralTypeNode(omitted) || !ts.isStringLiteral(omitted.literal)) {
          throw new Error(
            `configured omitted interface fields unsupported at root ${state.rootName}`,
          );
        }
        omittedNames.push(omitted.literal.text);
      }

      // require exact configured keys without normalization or extras
      if (
        omittedNames.length !== state.omitBaseFields.length ||
        omittedNames.some((name, index) => name !== state.omitBaseFields[index]) ||
        new Set(omittedNames).size !== omittedNames.length
      ) {
        throw new Error(
          `configured omitted interface fields unsupported at root ${state.rootName}`,
        );
      }

      const baseSymbol = checker.getSymbolAtLocation(baseType.typeName);
      const baseDeclarations = baseSymbol?.declarations;

      // resolve only an exact approved named type import or exact local symbol
      if (baseSymbol !== undefined && (baseSymbol.flags & ts.SymbolFlags.Alias) !== 0) {
        importedBase = resolveApprovedTypeImport(
          baseType.typeName,
          state,
          `root ${state.rootName}`,
        );
        configured = importedBase.configured;
        baseDeclaration = configured.declaration;
      } else {
        // reject ambiguous, missing, and indirect local base symbols
        if (baseSymbol === undefined || baseDeclarations?.length !== 1) {
          throw new Error(
            `configured omitted interface base identity unsupported at root ${state.rootName}`,
          );
        }
        baseDeclaration = baseDeclarations[0];
        configured = state.configuredDeclarations.get(baseDeclaration);
      }

      const isImported = importedBase !== undefined;

      // bind the base to one exported nongeneric configured interface
      if (
        configured === undefined ||
        configured.root.exportName !== state.base ||
        configured.root.localAlias === true ||
        configured.root.localInterface === true ||
        baseDeclaration.parent !== baseDeclaration.getSourceFile() ||
        !ts.isExternalModule(baseDeclaration.getSourceFile()) ||
        !isExported(baseDeclaration) ||
        !ts.isInterfaceDeclaration(baseDeclaration) ||
        (baseDeclaration.typeParameters?.length ?? 0) > 0 ||
        (isImported && (
          importedBase.exportName !== state.base ||
          configured.root.source === state.sourcePath ||
          baseDeclaration.getSourceFile() === state.sourceFile
        )) ||
        (!isImported && (
          configured.root.source !== state.sourcePath ||
          baseDeclaration.getSourceFile() !== state.sourceFile
        ))
      ) {
        throw new Error(
          `configured omitted interface base identity unsupported at root ${state.rootName}`,
        );
      }

      const baseOwnFieldNames = new Set(
        baseDeclaration.members
          .filter((member) => ts.isPropertySignature(member) && ts.isIdentifier(member.name))
          .map((member) => member.name.text),
      );
      const nonOwnOmittedNames = state.omitBaseFields.filter(
        (fieldName) => !baseOwnFieldNames.has(fieldName),
      );
      const expectedInheritedNames = state.omitInheritedBaseFields ?? [];

      // require exactly the opted-in names to come from flattened base inheritance
      if (
        nonOwnOmittedNames.length !== expectedInheritedNames.length ||
        nonOwnOmittedNames.some((fieldName, index) =>
          fieldName !== expectedInheritedNames[index]
        )
      ) {
        throw new Error(
          `configured omitted interface fields unsupported at root ${state.rootName}`,
        );
      }

      // consume the approved import only after the full base identity is proven
      if (importedBase !== undefined) {
        importedBase.binding.used.add(importedBase.exportName);
      }
    }

    state.dependencies.get(state.rootIdentity).add(configured.identity);
    const baseRendered = state.renderConfiguredRoot(configured.identity);

    // require reusable structured interface field descriptors from the base
    if (
      baseRendered.kind !== "readonly-interface" ||
      baseRendered.interfaceFields === undefined ||
      baseRendered.interfaceFieldNames === undefined ||
      baseRendered.interfaceFieldBlocks === undefined
    ) {
      throw new Error(`configured interface base unsupported at root ${state.rootName}`);
    }

    // prove every opted-in inherited omission exists once in flattened base output
    for (const fieldName of state.omitInheritedBaseFields ?? []) {
      const renderedMatches = baseRendered.interfaceFieldBlocks.filter(
        (field) => field.name === fieldName,
      );

      // reject absent or ambiguous inherited fields before filtering their blocks
      if (renderedMatches.length !== 1) {
        throw new Error(
          `configured inherited omitted interface field unsupported at ${state.rootName}.${fieldName}`,
        );
      }
    }

    // retain ordinary base rendering byte-for-byte
    if (state.omitBaseFields === undefined) {
      inheritedFieldBlocks = baseRendered.interfaceFieldBlocks;
    } else {
      const omittedNames = new Set(state.omitBaseFields);

      // retain complete annotated blocks in exact base order
      inheritedFieldBlocks = baseRendered.interfaceFieldBlocks.filter(
        (field) => !omittedNames.has(field.name),
      );
    }
    inheritedFields = inheritedFieldBlocks.flatMap((field) => field.lines);
    inheritedFieldNames = new Set(inheritedFieldBlocks.map((field) => field.name));

    // fold only the exact configured direct duplicates on this imported base
    if (state.identicalImportedStringFields !== undefined) {
      ownMembers = withoutIdenticalImportedStrings(declaration, baseDeclaration, inheritedFieldBlocks, checker, state);
    }
  }

  const ownFieldBlocks = [];
  const ownFields = renderObjectProperties(
    ownMembers,
    interfaceState,
    inheritedFieldNames,
    ownFieldBlocks,
  );
  // retain the exact validated field names for descendant collision checks
  const ownFieldNames = ownMembers.map((member) => member.name.text);
  const orderedOwnFields = state.ownFieldOrderAfterOmit ?? (state.addedAfterOmit === undefined
    ? undefined
    : [...state.omitBaseFields, ...state.addedAfterOmit]);

  // retain exact override-only behavior unless ordered added fields are configured
  if (
    state.omitBaseFields !== undefined &&
    (orderedOwnFields === undefined
      ? ownFieldNames.length !== state.omitBaseFields.length ||
        ownFieldNames.some(
          // admit only the originally named overrides in ordinary omission roots
          (name) => !state.omitBaseFields.includes(name),
        )
      : ownFieldNames.length !== orderedOwnFields.length ||
        ownFieldNames.some(
          // match the source replacement and added-field order exactly
          (name, index) => name !== orderedOwnFields[index],
        ))
  ) {
    throw new Error(`configured omitted interface overrides unsupported at root ${state.rootName}`);
  }

  const fields = [...inheritedFields, ...ownFields];
  const fieldBlocks = [...inheritedFieldBlocks, ...ownFieldBlocks];
  const rustRootName = defaultedListParameterProof === undefined
    ? state.rootName
    : `${state.rootName}<${defaultedListParameterProof.name} = ${defaultedListParameterProof.defaultRoot}>`;
  return {
    definition: [
      "#[allow(non_snake_case)]",
      "#[derive(Debug, Clone, PartialEq)]",
      `pub struct ${rustRootName} {`,
      ...fields,
      "}",
    ].join("\n"),
    interfaceFields: fields,
    interfaceFieldBlocks: fieldBlocks,
    interfaceFieldNames: new Set([...inheritedFieldNames, ...ownFieldNames]),
    interfaceNestedObjects: state.nestedObjectDescriptors,
  };
}

// recognize loader aliases already proven by their dedicated root renderers
function isConfiguredLoaderUnionArm(root, rendered) {
  return (
    root.disabledReasonExtract !== undefined &&
    rendered.kind === "tagged-readonly-object-union" &&
    rendered.discriminator?.field === "state"
  ) || (
    root.maintenanceLoaderExtension !== undefined &&
    rendered.kind === "maintenance-loader-extension-union"
  );
}

// render one explicitly configured union of named readonly interface roots
function renderNamedReadonlyObjectUnionRoot(typeNode, state) {
  // require a direct union rather than following aliases or inferred shapes
  if (!ts.isUnionTypeNode(typeNode) || typeNode.types.length < 2) {
    throw new Error(
      `named object union requires at least two direct type reference arms at root ${state.rootName}`,
    );
  }

  const checker = getSourceChecker(
    state.checkers,
    state.sourcePath,
    state.sourceFile,
    state.utilityDeclarations,
  );
  const seenSourceNames = new Set();
  const usedMappings = new Set();
  const variants = [];
  let usedImportedArm = false;

  // preserve exact source arm order while proving every configured payload identity
  for (const arm of typeNode.types) {
    // reject inline, qualified, generic, primitive, intersection, and mixed arms
    if (
      !ts.isTypeReferenceNode(arm) ||
      !ts.isIdentifier(arm.typeName) ||
      arm.typeArguments !== undefined
    ) {
      throw new Error(
        `named object union arm must be an argument-free identifier reference at root ${state.rootName}`,
      );
    }

    const sourceName = arm.typeName.text;

    // reject repeated payload identities instead of making duplicate variants
    if (seenSourceNames.has(sourceName)) {
      throw new Error(`duplicate named object union arm ${sourceName} at root ${state.rootName}`);
    }
    seenSourceNames.add(sourceName);

    const symbol = checker.getSymbolAtLocation(arm.typeName);
    const declarations = symbol?.declarations;
    let configured;
    let declaration;
    let imported;

    // opted-in unions may reuse one exact approved type-only import arm
    if (
      (state.localAlias || state.importedObjectUnion) &&
      symbol !== undefined &&
      (symbol.flags & ts.SymbolFlags.Alias) !== 0
    ) {
      imported = resolveApprovedTypeImport(arm.typeName, state, `root ${state.rootName}`);
      configured = imported.configured;
      declaration = configured.declaration;
    } else {
      // retain the prior direct non-alias requirement for ordinary object unions
      if (
        symbol === undefined ||
        (symbol.flags & ts.SymbolFlags.Alias) !== 0 ||
        declarations === undefined ||
        declarations.length !== 1
      ) {
        throw new Error(
          `named object union arm identity unsupported for ${sourceName} at root ${state.rootName}`,
        );
      }
      declaration = declarations[0];
      configured = state.configuredDeclarations.get(declaration);
    }

    const isPublicLocalInterface =
      ts.isInterfaceDeclaration(declaration) &&
      configured?.root.source === state.sourcePath &&
      declaration.getSourceFile() === state.sourceFile &&
      imported === undefined;
    const isPublicImportedInterface =
      state.importedObjectUnion &&
      ts.isInterfaceDeclaration(declaration) &&
      configured?.root.source !== state.sourcePath &&
      declaration.getSourceFile() !== state.sourceFile &&
      imported !== undefined;
    const isPublicImportedNamedObjectUnion =
      state.importedObjectUnion &&
      ts.isTypeAliasDeclaration(declaration) &&
      configured?.root.objectUnion !== undefined &&
      configured.root.localAlias !== true &&
      configured.root.source !== state.sourcePath &&
      declaration.getSourceFile() !== state.sourceFile &&
      imported !== undefined;

    // keep every arm on one exact configured, exported, nongeneric declaration
    if (
      configured === undefined ||
      configured.root.exportName !== sourceName ||
      configured.root.localAlias === true ||
      configured.root.localInterface === true ||
      declaration.parent !== declaration.getSourceFile() ||
      !ts.isExternalModule(declaration.getSourceFile()) ||
      !isExported(declaration) ||
      (declaration.typeParameters?.length ?? 0) > 0 ||
      (!state.localAlias && !isPublicLocalInterface &&
        !isPublicImportedInterface && !isPublicImportedNamedObjectUnion)
    ) {
      throw new Error(
        `named object union arm identity unsupported for ${sourceName} at root ${state.rootName}`,
      );
    }

    // consume an approved import only after its payload identity is proven
    if (imported !== undefined) {
      imported.binding.used.add(imported.exportName);
      usedImportedArm = true;
    }

    state.dependencies.get(state.rootIdentity).add(configured.identity);

    // private unions wrap existing nominal payloads without flattening them
    if (state.localAlias) {
      const renderedArm = state.renderConfiguredRoot(configured.identity);

      // admit imported loader aliases only when their dedicated grammar was proven
      if (!(
        (ts.isInterfaceDeclaration(declaration) && renderedArm.kind === "readonly-interface") ||
        (ts.isTypeAliasDeclaration(declaration) &&
          ((configured.root.objectUnion !== undefined &&
            renderedArm.kind === "named-readonly-object-union") ||
            (imported !== undefined && isConfiguredLoaderUnionArm(configured.root, renderedArm))))
      )) {
        throw new Error(
          `local alias object union arm unsupported for ${sourceName} at root ${state.rootName}`,
        );
      }
    }

    const rustName = state.objectUnion[sourceName];

    // require an exact explicit variant for every source arm
    if (rustName === undefined) {
      throw new Error(`missing named object union mapping for ${sourceName} at root ${state.rootName}`);
    }

    usedMappings.add(sourceName);
    variants.push({ rustName, payloadRustName: configured.root.exportName });
  }

  // reject stale configured variants that are absent from the direct source union
  for (const sourceName of Object.keys(state.objectUnion)) {
    // report the first unused normalized mapping deterministically
    if (!usedMappings.has(sourceName)) {
      throw new Error(`unused named object union mapping ${sourceName} at root ${state.rootName}`);
    }
  }

  // reject a stale imported-arm opt-in with no imported payload
  if (state.importedObjectUnion && !usedImportedArm) {
    throw new Error(`imported object union has no imported arm at root ${state.rootName}`);
  }

  const lines = [
    "#[derive(Debug, Clone, PartialEq)]",
    `pub enum ${state.rootName} {`,
  ];

  // bind each explicit Rust variant to its independently rendered interface root
  for (const variant of variants) {
    lines.push(`    ${variant.rustName}(${variant.payloadRustName}),`);
  }
  lines.push("}");

  return {
    kind: "named-readonly-object-union",
    definition: lines.join("\n"),
    objectUnion: { variants },
  };
}

// collect exact string and null literals from one direct type expression
function collectLiteralUnionValues(typeNode, label) {
  const members = ts.isUnionTypeNode(typeNode) ? typeNode.types : [typeNode];
  const values = [];
  let hasNull = false;

  // admit only direct literal arms without resolving another alias
  for (const member of members) {
    // retain one exact string wire value
    if (ts.isLiteralTypeNode(member) && ts.isStringLiteralLike(member.literal)) {
      values.push(member.literal.text);
      continue;
    }

    // retain null independently from every string spelling
    if (ts.isLiteralTypeNode(member) && member.literal.kind === ts.SyntaxKind.NullKeyword) {
      hasNull = true;
      continue;
    }

    throw new Error(`${label} must contain only direct string literals and null`);
  }

  return { values, hasNull };
}

// resolve one configured simple string-literal alias arm
function resolveLiteralUnionAlias(typeNode, state) {
  const label = `literal union referenced alias at root ${state.rootName}`;

  // require one argument-free named reference
  if (
    !ts.isTypeReferenceNode(typeNode) ||
    !ts.isIdentifier(typeNode.typeName) ||
    typeNode.typeArguments !== undefined
  ) {
    throw new Error(`${label} unsupported`);
  }

  const checker = getSourceChecker(
    state.checkers,
    state.sourcePath,
    state.sourceFile,
    state.utilityDeclarations,
  );
  const symbol = checker.getSymbolAtLocation(typeNode.typeName);

  // reject unresolved or merged bindings before classifying the reference
  if (symbol === undefined || symbol.declarations?.length !== 1) {
    throw new Error(`${label} identity unsupported`);
  }

  const bindingDeclaration = symbol.declarations[0];
  let configured;

  // admit one already-approved type-only named import alias
  if ((symbol.flags & ts.SymbolFlags.Alias) !== 0) {
    if (
      state.typeImportBindings === undefined ||
      !ts.isImportSpecifier(bindingDeclaration)
    ) {
      throw new Error(`${label} identity unsupported`);
    }
    const namedImports = bindingDeclaration.parent;
    const importClause = namedImports.parent;
    const importDeclaration = importClause.parent;

    // retain the existing exact named type-import grammar
    if (
      !ts.isNamedImports(namedImports) ||
      !ts.isImportClause(importClause) ||
      !ts.isImportDeclaration(importDeclaration) ||
      (!bindingDeclaration.isTypeOnly && !importClause.isTypeOnly) ||
      !ts.isStringLiteralLike(importDeclaration.moduleSpecifier)
    ) {
      throw new Error(`${label} identity unsupported`);
    }
    const binding = state.typeImportBindings.get(importDeclaration.moduleSpecifier.text);
    const exportName = (bindingDeclaration.propertyName ?? bindingDeclaration.name).text;
    configured = binding?.exported.get(exportName);

    // join the compiler alias edge to the configured declaration owner
    if (
      binding === undefined ||
      configured === undefined ||
      bindingDeclaration.name.text !== typeNode.typeName.text ||
      resolvedSymbolDeclaration(checker, symbol) !== configured.declaration
    ) {
      throw new Error(`${label} identity unsupported`);
    }
    binding.used.add(exportName);
  } else {
    configured = state.configuredDeclarations.get(bindingDeclaration);

    // keep local references on exact configured declarations in this module
    if (
      configured === undefined ||
      bindingDeclaration.getSourceFile() !== state.sourceFile ||
      bindingDeclaration.parent !== state.sourceFile ||
      !ts.isExternalModule(state.sourceFile)
    ) {
      throw new Error(`${label} identity unsupported`);
    }
  }

  const declaration = configured.declaration ?? bindingDeclaration;

  // constrain the referenced root to one exported nongeneric direct string union
  if (
    !ts.isTypeAliasDeclaration(declaration) ||
    (declaration.typeParameters?.length ?? 0) > 0 ||
    Object.keys(configured.root.fields).length !== 0 ||
    configured.root.base !== undefined ||
    configured.root.catalog !== undefined ||
    configured.root.discriminator !== undefined ||
    configured.root.objectUnion !== undefined ||
    configured.root.literalUnion !== undefined ||
    !ts.isUnionTypeNode(declaration.type) ||
    declaration.type.types.length < 2 ||
    !declaration.type.types.every((member) =>
      ts.isLiteralTypeNode(member) && ts.isStringLiteralLike(member.literal)
    )
  ) {
    throw new Error(`${label} unsupported`);
  }

  state.dependencies.get(state.rootIdentity).add(configured.identity);
  return declaration.type.types.map((member) => member.literal.text);
}

// extract one configured field from a closed inline object union alias
function collectLiteralUnionProjection(typeNode, state) {
  const { sourceAlias, field } = state.literalUnion;

  // require the exact SourceAlias["field"] syntax from configuration
  if (
    !ts.isIndexedAccessTypeNode(typeNode) ||
    !ts.isTypeReferenceNode(typeNode.objectType) ||
    !ts.isIdentifier(typeNode.objectType.typeName) ||
    typeNode.objectType.typeName.text !== sourceAlias ||
    typeNode.objectType.typeArguments !== undefined ||
    !ts.isLiteralTypeNode(typeNode.indexType) ||
    !ts.isStringLiteral(typeNode.indexType.literal) ||
    typeNode.indexType.literal.text !== field
  ) {
    throw new Error(`literal union source projection unsupported at root ${state.rootName}`);
  }

  const checker = getSourceChecker(
    state.checkers,
    state.sourcePath,
    state.sourceFile,
    state.utilityDeclarations,
  );
  const symbol = checker.getSymbolAtLocation(typeNode.objectType.typeName);
  const declarations = symbol?.declarations;

  // require a unique direct same-module alias symbol
  if (
    symbol === undefined ||
    (symbol.flags & ts.SymbolFlags.Alias) !== 0 ||
    declarations?.length !== 1
  ) {
    throw new Error(`literal union source alias identity unsupported at root ${state.rootName}`);
  }

  const declaration = declarations[0];
  const configured = state.configuredDeclarations.get(declaration);
  const isConfiguredTaggedUnion =
    configured !== undefined &&
    configured.root.source === state.sourcePath &&
    configured.root.exportName === sourceAlias &&
    configured.root.localAlias !== true &&
    configured.root.localInterface !== true &&
    configured.root.discriminator !== undefined &&
    configured.root.base === undefined;

  // exclude ordinary configured aliases, generics, nonexports, and indirect source shapes
  if (
    !ts.isTypeAliasDeclaration(declaration) ||
    declaration.name.text !== sourceAlias ||
    declaration.getSourceFile() !== state.sourceFile ||
    declaration.parent !== state.sourceFile ||
    !ts.isExternalModule(state.sourceFile) ||
    !isExported(declaration) ||
    (declaration.typeParameters?.length ?? 0) > 0 ||
    (configured !== undefined && !isConfiguredTaggedUnion) ||
    !ts.isUnionTypeNode(declaration.type) ||
    declaration.type.types.length < 2
  ) {
    throw new Error(`literal union source alias identity unsupported at root ${state.rootName}`);
  }

  // retain cycle proof when the closed source union is also a configured tagged root
  if (isConfiguredTaggedUnion) {
    state.dependencies.get(state.rootIdentity).add(configured.identity);
  }

  const values = [];
  let hasNull = false;

  // read only the configured property from every closed inline object arm
  for (const arm of declaration.type.types) {
    // reject referenced, intersected, primitive, and open union arms
    if (!ts.isTypeLiteralNode(arm)) {
      throw new Error(`literal union source alias arm unsupported at root ${state.rootName}`);
    }

    const memberNames = new Set();
    let selectedMember;

    // prove a closed property-only object without translating adjacent values
    for (const member of arm.members) {
      if (
        !ts.isPropertySignature(member) ||
        !ts.isIdentifier(member.name) ||
        memberNames.has(member.name.text)
      ) {
        throw new Error(`literal union source alias arm unsupported at root ${state.rootName}`);
      }
      memberNames.add(member.name.text);

      // retain exactly one ordinary configured property
      if (member.name.text === field) {
        selectedMember = member;
      }
    }

    // require one readonly, required, explicitly typed projection in every arm
    if (
      selectedMember === undefined ||
      selectedMember.questionToken !== undefined ||
      !isReadonly(selectedMember) ||
      selectedMember.type === undefined
    ) {
      throw new Error(`literal union source alias field unsupported at root ${state.rootName}`);
    }

    const projected = collectLiteralUnionValues(
      selectedMember.type,
      `literal union source alias field at root ${state.rootName}`,
    );
    values.push(...projected.values);
    hasNull ||= projected.hasNull;
  }

  return { values, hasNull };
}

// render one explicitly composed nullable string-literal root
function renderComposedLiteralUnionRoot(typeNode, state) {
  // require direct source arms rather than a wrapped alias or utility
  if (!ts.isUnionTypeNode(typeNode) || typeNode.types.length < 2) {
    throw new Error(`literal union requires a direct union at root ${state.rootName}`);
  }

  const values = [];
  let hasNull = false;
  let projectionCount = 0;

  // preserve first TypeScript order across literals, aliases, and the projection
  for (const arm of typeNode.types) {
    // retain direct literal arms without widening their syntax
    if (ts.isLiteralTypeNode(arm)) {
      const direct = collectLiteralUnionValues(
        arm,
        `literal union arm at root ${state.rootName}`,
      );
      values.push(...direct.values);
      hasNull ||= direct.hasNull;
      continue;
    }

    // expand only exact configured simple string-literal aliases
    if (ts.isTypeReferenceNode(arm)) {
      values.push(...resolveLiteralUnionAlias(arm, state));
      continue;
    }

    // admit exactly the configured source projection syntax
    if (ts.isIndexedAccessTypeNode(arm)) {
      projectionCount += 1;
      const projected = collectLiteralUnionProjection(arm, state);
      values.push(...projected.values);
      hasNull ||= projected.hasNull;
      continue;
    }

    throw new Error(`literal union arm unsupported at root ${state.rootName}`);
  }

  // require the configured projection and genuine nullable string storage
  if (projectionCount !== 1 || !hasNull || values.length === 0) {
    throw new Error(`literal union composition unsupported at root ${state.rootName}`);
  }

  const uniqueValues = [];
  const seenValues = new Set();

  // deduplicate semantic wire values at their first TypeScript occurrence
  for (const value of values) {
    // omit only later occurrences without reordering any first occurrence
    if (!seenValues.has(value)) {
      seenValues.add(value);
      uniqueValues.push(value);
    }
  }

  const valueTypeName = `${state.rootName}Value`;
  reserveGeneratedType(state, valueTypeName);
  state.needsNullable.value = true;
  return {
    kind: "nullable-string-literal-enum",
    definition: [
      renderStringEnumValues(valueTypeName, uniqueValues, `root ${state.rootName}`),
      `pub type ${state.rootName} = Nullable<${valueTypeName}>;`,
    ].join("\n\n"),
  };
}

// derive one tagged-union Rust variant from an exact wire discriminant
function taggedUnionVariantName(wireValue) {
  // give the null tag its explicit Rust identity
  if (wireValue === null) {
    return "Null";
  }

  return pascalCase(wireValue, "tagged union wire value");
}

// select one required exact discriminant property from an inline object arm
function readTaggedUnionDiscriminator(typeNode, state) {
  const matchingMembers = typeNode.members.filter((member) =>
    member.name !== undefined &&
    ts.isIdentifier(member.name) &&
    member.name.text === state.discriminator,
  );
  const pathLabel = `${state.rootName}.${state.discriminator}`;

  // require one unambiguous property declaration for the configured field
  if (matchingMembers.length !== 1 || !ts.isPropertySignature(matchingMembers[0])) {
    throw new Error(`tagged union requires exactly one discriminator property at ${pathLabel}`);
  }

  const member = matchingMembers[0];

  // preserve the source contract's required-field semantics
  if (member.questionToken !== undefined) {
    throw new Error(`tagged union discriminator must be required at ${pathLabel}`);
  }

  // require explicit readonly outside the proven shallow wrapper
  if (state.readonlyProjectionMembers !== typeNode.members && !isReadonly(member)) {
    throw new Error(`tagged union discriminator must be readonly at ${pathLabel}`);
  }

  // reject inferred or widened discriminator declarations
  if (member.type === undefined || !ts.isLiteralTypeNode(member.type)) {
    throw new Error(`tagged union discriminator must be one exact string literal or null at ${pathLabel}`);
  }

  // retain one exact string wire value without semantic widening
  if (ts.isStringLiteralLike(member.type.literal)) {
    return member.type.literal.text;
  }

  // retain exact null distinctly from the string spelling "null"
  if (member.type.literal.kind === ts.SyntaxKind.NullKeyword) {
    return null;
  }

  throw new Error(`tagged union discriminator must be one exact string literal or null at ${pathLabel}`);
}

// keep disabled reason extraction on the original loader payload shapes
function validateDisabledReasonExtractArm(arm, wireValue, rootName) {
  const active = wireValue === "active";
  const required = new Set(["bundle", "reasonCode", "state"]);
  const seen = new Set();
  const error = DISABLED_REASON_EXTRACT + " requires exact active and disabled loader payloads at root " + rootName;

  // reject unrelated discriminants
  if (!active && wireValue !== "disabled") {
    throw new Error(error);
  }

  // check the exact own fields without reordering the source declarations
  for (const member of arm.members) {
    // require direct readonly named properties with explicit types
    if (!ts.isPropertySignature(member) || !ts.isIdentifier(member.name) || !isReadonly(member) || member.type === undefined) {
      throw new Error(error);
    }
    const name = member.name.text;
    const comparator = active && name === "comparatorAuthority";

    // keep the comparator optional and confined to the active arm
    if ((!required.has(name) && !comparator) || seen.has(name) || comparator !== (member.questionToken !== undefined)) {
      throw new Error(error);
    }
    seen.add(name);

    // keep the active bundle nominal and the disabled bundle exactly null
    if (name === "bundle" && (active ? !ts.isTypeReferenceNode(member.type) : !isExactNullType(member.type))) {
      throw new Error(error);
    }

    // keep the active reason exactly null
    if (active && name === "reasonCode" && !isExactNullType(member.type)) {
      throw new Error(error);
    }
    required.delete(name);
  }

  // reject any missing required field
  if (required.size !== 0) {
    throw new Error(error);
  }
}

// prove and render one exact configured interface intersected with tagged arms
function renderTaggedUnionBase(typeNode, state) {
  // leave the historical direct-union grammar unchanged without base metadata
  if (state.base === undefined) {
    return {
      unionType: typeNode,
      inheritedFields: [],
      inheritedFieldNames: new Set(),
    };
  }

  const baseType = typeNode.types?.[0];
  const groupedUnion = typeNode.types?.[1];

  // require configured base and grouped inline arms with no generic arguments
  if (
    !ts.isIntersectionTypeNode(typeNode) ||
    typeNode.types.length !== 2 ||
    baseType === undefined ||
    !ts.isTypeReferenceNode(baseType) ||
    !ts.isIdentifier(baseType.typeName) ||
    baseType.typeArguments !== undefined ||
    groupedUnion === undefined ||
    !ts.isParenthesizedTypeNode(groupedUnion) ||
    !ts.isUnionTypeNode(groupedUnion.type)
  ) {
    throw new Error(`configured tagged union base shape unsupported at root ${state.rootName}`);
  }

  const checker = getSourceChecker(
    state.checkers,
    state.sourcePath,
    state.sourceFile,
    state.utilityDeclarations,
  );
  const symbol = checker.getSymbolAtLocation(baseType.typeName);
  const declarations = symbol?.declarations;

  // require one direct non-alias declaration for the configured base spelling
  if (
    symbol === undefined ||
    (symbol.flags & ts.SymbolFlags.Alias) !== 0 ||
    declarations?.length !== 1
  ) {
    throw new Error(`configured tagged union base identity unsupported at root ${state.rootName}`);
  }

  const baseDeclaration = declarations[0];
  const configured = state.configuredDeclarations.get(baseDeclaration);

  // bind the base to one local configured nongeneric getter-free interface
  if (
    configured === undefined ||
    configured.root.source !== state.sourcePath ||
    configured.root.exportName !== state.base ||
    configured.root.localInterface === true ||
    baseType.typeName.text !== state.base ||
    baseDeclaration.getSourceFile() !== state.sourceFile ||
    baseDeclaration.parent !== state.sourceFile ||
    !ts.isExternalModule(state.sourceFile) ||
    !ts.isInterfaceDeclaration(baseDeclaration) ||
    (baseDeclaration.typeParameters?.length ?? 0) > 0 ||
    !baseDeclaration.members.every((member) => ts.isPropertySignature(member))
  ) {
    throw new Error(`configured tagged union base identity unsupported at root ${state.rootName}`);
  }

  state.dependencies.get(state.rootIdentity).add(configured.identity);
  const baseRendered = state.renderConfiguredRoot(configured.identity);

  // reuse only the base renderer's validated flattened descriptors and names
  if (
    baseRendered.kind !== "readonly-interface" ||
    baseRendered.interfaceFields === undefined ||
    baseRendered.interfaceFieldNames === undefined
  ) {
    throw new Error(`configured tagged union base unsupported at root ${state.rootName}`);
  }

  return {
    unionType: groupedUnion.type,
    inheritedFields: baseRendered.interfaceFields,
    inheritedFieldNames: baseRendered.interfaceFieldNames,
  };
}

// route qualified metadata only into the arm with the exact string wire value
function taggedArmFieldMappings(state, wireValue) {
  const fieldMappings = new Map();
  const sourcePaths = new Map();

  // retain exact configured order while removing the arm qualifier locally
  for (const [sourcePath, mapping] of state.fieldMappings) {
    const parsed = parseTaggedArmFieldPath(sourcePath, state.readonlyTaggedUnion);

    // normalized arm-specific paths always use the qualified grammar
    if (parsed === null) {
      throw new Error(`invalid tagged union field mapping at ${state.rootName}.${sourcePath}`);
    }

    // never expose one arm's metadata to another payload
    if (parsed.wireValue !== wireValue) {
      continue;
    }

    // reject two source paths collapsing onto one payload field
    if (fieldMappings.has(parsed.fieldName)) {
      throw new Error(`duplicate tagged union field mapping at ${state.rootName}.${sourcePath}`);
    }
    fieldMappings.set(parsed.fieldName, mapping);
    sourcePaths.set(parsed.fieldName, sourcePath);
  }

  return { fieldMappings, sourcePaths };
}

// prove one required direct field for the two-state const selection
function validateActiveImportedConstStringArm(arm, wireValue, state) {
  const members = arm.members.filter(
    // select only the configured direct readonly field
    (member) => ts.isPropertySignature(member) && ts.isIdentifier(member.name) &&
      member.name.text === state.activeImportedConstString,
  );
  const member = members[0];

  // keep disabled null distinct from the proven active literal
  if (
    (wireValue !== "active" && wireValue !== "disabled") ||
    members.length !== 1 || member.questionToken !== undefined ||
    !isReadonly(member) || member.type === undefined ||
    (wireValue === "disabled" && !isExactNullType(member.type))
  ) {
    throw new Error(`active-imported-const-string requires active const and disabled null at root ${state.rootName}`);
  }
  return wireValue === "active" ? member : undefined;
}

// render one explicitly configured tagged object union in source order
function renderTaggedObjectUnionRoot(typeNode, state) {
  const useArmFieldPaths = state.base !== undefined || state.readonlyTaggedUnion;

  // retain the historical rejection for ordinary unbased tagged roots
  if (!useArmFieldPaths && state.fieldMappings.size > 0) {
    throw new Error(`tagged union field mappings unsupported at root ${state.rootName}`);
  }

  const {
    unionType,
    inheritedFields,
    inheritedFieldNames,
  } = renderTaggedUnionBase(typeNode, state);

  // require a real union rather than treating one object as a tagged sum
  if (!ts.isUnionTypeNode(unionType) || unionType.types.length < 2) {
    throw new Error(`tagged union requires at least two inline object arms at root ${state.rootName}`);
  }

  // keep disabled reason extraction on the exact two-arm loader form
  if (state.disabledReasonExtract !== undefined && unionType.types.length !== 2) {
    throw new Error(DISABLED_REASON_EXTRACT + " requires active and disabled arms at root " + state.rootName);
  }

  // keep the imported const rule on one exact active and disabled union
  if (state.activeImportedConstString !== undefined && unionType.types.length !== 2) {
    throw new Error(`active-imported-const-string requires active and disabled arms at root ${state.rootName}`);
  }

  const seenWireValues = new Set();
  const seenVariants = new Set();
  const variants = [];

  // preserve exact source arm order while validating each closed payload
  for (const sourceArm of unionType.types) {
    const arm = state.readonlyTaggedUnion
      ? readPinnedReadonlyInlineObject(
        sourceArm,
        state,
        state.rootName,
        "readonly-tagged-union",
        "Readonly<{ ... }> arms",
      )
      : sourceArm;

    // reject references, intersections, and mixed primitive arms
    if (!ts.isTypeLiteralNode(arm)) {
      throw new Error(`tagged union arm must be an inline object at root ${state.rootName}`);
    }

    const armState = state.readonlyTaggedUnion
      ? { ...state, readonlyProjectionMembers: arm.members }
      : state;
    const wireValue = readTaggedUnionDiscriminator(arm, armState);

    // prove the original loader arm before routing the disabled subset rule
    if (state.disabledReasonExtract !== undefined) {
      validateDisabledReasonExtractArm(arm, wireValue, state.rootName);
    }

    // prove required presence and keep only the validated active member identity
    const activeImportedConstMember = state.activeImportedConstString === undefined
      ? undefined
      : validateActiveImportedConstStringArm(arm, wireValue, state);

    // qualified metadata cannot represent null distinctly from a string path
    if (state.base !== undefined && wireValue === null) {
      throw new Error(`base-tagged union discriminator must be one exact string literal at root ${state.rootName}`);
    }

    // require a real string tag for each readonly-wrapped arm
    if (state.readonlyTaggedUnion && wireValue === null) {
      throw new Error(`readonly-tagged union discriminator must be one exact string literal at root ${state.rootName}`);
    }
    // keep null distinct from every possible exact string tag
    const wireIdentity = wireValue === null ? "null" : `string:${wireValue}`;

    // reject overlapping tags instead of making source order meaningful
    if (seenWireValues.has(wireIdentity)) {
      throw new Error(`duplicate tagged union wire value ${JSON.stringify(wireValue)} at root ${state.rootName}`);
    }
    seenWireValues.add(wireIdentity);

    const rustName = taggedUnionVariantName(wireValue);

    // reject distinct wire values that normalize to one Rust variant
    if (seenVariants.has(rustName)) {
      throw new Error(`tagged union Rust variant collision ${rustName} at root ${state.rootName}`);
    }
    seenVariants.add(rustName);

    const payloadRustName = `${state.rootName}${rustName}`;
    reserveGeneratedType(state, payloadRustName);
    // keep historical empty metadata on ordinary unbased payloads
    const routed = useArmFieldPaths
      ? taggedArmFieldMappings(state, wireValue)
      : { fieldMappings: state.fieldMappings, sourcePaths: new Map() };
    const armUsedFieldMappings = useArmFieldPaths ? new Set() : state.usedFieldMappings;
    const payloadState = {
      ...armState,
      allowExactNullLiteral: true,
      fieldPath: [],
      disabledReasonExtract: wireValue === "disabled" ? state.disabledReasonExtract : undefined,
      activeImportedConstMember,
      fieldMappings: routed.fieldMappings,
      usedFieldMappings: armUsedFieldMappings,
      typePath: [rustName],
    };
    const fields = renderObjectProperties(
      arm.members,
      payloadState,
      inheritedFieldNames,
    );

    // translate every consumed local path back to its qualified config path
    for (const fieldName of armUsedFieldMappings) {
      const sourcePath = routed.sourcePaths.get(fieldName);

      // retain the consumed mapping's exact qualified source identity
      if (sourcePath !== undefined) {
        state.usedFieldMappings.add(sourcePath);
      }
    }
    state.auxiliaryDefinitions.push([
      "#[allow(non_snake_case)]",
      "#[derive(Debug, Clone, PartialEq)]",
      `pub struct ${payloadRustName} {`,
      ...inheritedFields,
      ...fields,
      "}",
    ].join("\n"));
    variants.push({ rustName, payloadRustName, wireValue });
  }

  // diagnose qualified paths that name no arm separately from missing fields
  for (const sourcePath of state.fieldMappings.keys()) {
    // skip every exact mapping consumed by its selected payload field
    if (state.usedFieldMappings.has(sourcePath)) {
      continue;
    }
    const parsed = parseTaggedArmFieldPath(sourcePath, state.readonlyTaggedUnion);

    // preserve a fail-closed diagnostic if normalized state is ever bypassed
    if (parsed === null) {
      throw new Error(`invalid tagged union field mapping at ${state.rootName}.${sourcePath}`);
    }
    const wireIdentity = `string:${parsed.wireValue}`;

    // distinguish a nonexistent wire tag from a misspelled payload property
    if (!seenWireValues.has(wireIdentity)) {
      throw new Error(`tagged union field mapping targets no arm at ${state.rootName}.${sourcePath}`);
    }
    throw new Error(`tagged union field mapping targets no field at ${state.rootName}.${sourcePath}`);
  }

  const lines = [
    "#[derive(Debug, Clone, PartialEq)]",
    `pub enum ${state.rootName} {`,
  ];

  // bind every Rust variant to its exact discriminant and payload type
  for (const variant of variants) {
    lines.push(
      `    #[doc = ${rustString(`wire discriminant ${state.discriminator}: ${JSON.stringify(variant.wireValue)}`)}]`,
    );
    lines.push(`    ${variant.rustName}(${variant.payloadRustName}),`);
  }
  lines.push(
    "}",
    `impl ${state.rootName} {`,
    `    pub const DISCRIMINATOR_FIELD: &'static str = ${rustString(state.discriminator)};`,
    "}",
  );

  return {
    kind: "tagged-readonly-object-union",
    definition: lines.join("\n"),
    discriminator: {
      field: state.discriminator,
      variants,
    },
  };
}

// prove the already-generated two-arm loader without merging extension properties
function resolveMaintenanceLoaderLegacy(typeNode, state, pathLabel) {
  const { legacy } = state.maintenanceLoaderExtension;
  const configured = state.configuredRootsByName.get(legacy);
  const declaration = configured?.declaration;

  // bind the intersection to the selected public direct tagged alias
  if (
    !ts.isTypeReferenceNode(typeNode) || !ts.isIdentifier(typeNode.typeName) ||
    typeNode.typeName.text !== legacy || typeNode.typeArguments !== undefined ||
    configured?.root.discriminator !== "state" || configured.root.readonlyTaggedUnion !== undefined ||
    declaration === undefined || !ts.isTypeAliasDeclaration(declaration) ||
    !ts.isUnionTypeNode(declaration.type) || declaration.type.types.length !== 2
  ) {
    throw new Error(`maintenance loader legacy alias unsupported at ${pathLabel}`);
  }
  const checker = getSourceChecker(
    state.checkers, state.sourcePath, state.sourceFile, state.utilityDeclarations,
  );
  const symbol = checker.getSymbolAtLocation(typeNode.typeName);

  // accept a cross-module alias only through its exact existing proof-only binding
  if (configured.root.source !== state.sourcePath) {
    resolveIndexedLiteralTypeImport(typeNode.typeName, state, declaration, pathLabel, true);
  // retain one exact local declaration identity for the same-module loader
  } else if (
    symbol === undefined || (symbol.flags & ts.SymbolFlags.Alias) !== 0 ||
    symbol.declarations?.length !== 1 || symbol.declarations[0] !== declaration
  ) {
    throw new Error(`maintenance loader legacy alias identity unsupported at ${pathLabel}`);
  }

  // prevent an implicit intersection with an existing comparator property
  for (const arm of declaration.type.types) {
    // admit only direct object arms without redeclaring the extension key
    if (!ts.isTypeLiteralNode(arm) || arm.members.some(
      // check the property on both original legacy states
      (property) => property.name !== undefined && ts.isIdentifier(property.name) &&
        property.name.text === "comparatorAuthority",
    )) {
      throw new Error(`maintenance loader legacy comparator collision at ${pathLabel}`);
    }
  }
  state.dependencies.get(state.rootIdentity).add(configured.identity);
  const { kind, discriminator } = state.renderConfiguredRoot(configured.identity);

  // retain the original active and disabled nominal payload identities
  if (
    kind !== "tagged-readonly-object-union" || discriminator?.field !== "state" ||
    discriminator.variants.length !== 2 || discriminator.variants[0].wireValue !== "active" ||
    discriminator.variants[1].wireValue !== "disabled"
  ) {
    throw new Error(`maintenance loader legacy states unsupported at ${pathLabel}`);
  }
  return legacy;
}

// require one readonly comparator reference with its original presence rule
function isMaintenanceLoaderComparator(member, optional) {
  return member !== undefined && ts.isPropertySignature(member) && ts.isIdentifier(member.name) &&
    member.name.text === "comparatorAuthority" && isReadonly(member) &&
    (member.questionToken !== undefined) === optional && member.type !== undefined &&
    ts.isTypeReferenceNode(member.type) && ts.isIdentifier(member.type.typeName) &&
    member.type.typeArguments === undefined;
}

// read the distinct maintenance payloads without using state as a union-wide tag
function renderMaintenanceLoaderPayload(arm, state, comparator, active, pathLabel) {
  const wireState = active ? "active" : "disabled";
  const variant = active ? "MaintenanceActive" : "MaintenanceDisabled";
  const names = ["bundle", "comparatorAuthority", "reasonCode", "state"];

  // close the arm to the four direct readonly required payload fields
  if (!ts.isTypeLiteralNode(arm) || arm.members.length !== names.length || !arm.members.every(
    // require original source members to stay explicit data properties
    (member) => ts.isPropertySignature(member) && ts.isIdentifier(member.name) &&
      isReadonly(member) && member.type !== undefined && member.questionToken === undefined,
  )) {
    throw new Error(`maintenance loader ${wireState} payload unsupported at ${pathLabel}`);
  }
  const members = new Map(arm.members.map(
    // preserve member declarations for exact field and tag matching
    (member) => [member.name.text, member],
  ));
  const bundle = members.get("bundle")?.type;
  const reason = members.get("reasonCode")?.type;
  const authority = members.get("comparatorAuthority");

  // preserve the exact active or raw policy payload without adding a wire tag
  if (
    // require the same four members in each branch
    names.some((name) => !members.has(name)) || bundle === undefined || reason === undefined ||
    !isMaintenanceLoaderComparator(authority, false) ||
    authority.type.typeName.text !== comparator.type.typeName.text ||
    readTaggedUnionDiscriminator(arm, { ...state, discriminator: "state" }) !== wireState ||
    (active
      ? !ts.isTypeLiteralNode(bundle) || !isExactNullType(reason)
      : !isExactNullType(bundle) || !ts.isLiteralTypeNode(reason) ||
        !ts.isStringLiteral(reason.literal) || reason.literal.text !== "policy_raw")
  ) {
    throw new Error(`maintenance loader ${wireState} payload unsupported at ${pathLabel}`);
  }

  const { package: packageName, bundleField } = state.maintenanceLoaderExtension;
  const projectedPath = `bundle.${bundleField}`;
  const usedFieldProjections = new Set();
  const fieldProjections = new Map();

  // route only the configured active-bundle member to its exact imported interface field
  if (active) {
    fieldProjections.set(projectedPath, { kind: "interface-field", root: packageName, imported: true });
  }
  const payloadName = state.rootName + variant;
  reserveGeneratedType(state, payloadName);
  const fields = renderObjectProperties(arm.members, {
    ...state, fieldProjections, usedFieldProjections, allowExactNullLiteral: true, typePath: [variant],
  });

  // reject source changes that remove or rename the selected imported member
  if (active && !usedFieldProjections.has(projectedPath)) {
    throw new Error(`maintenance loader active package field missing at ${pathLabel}.${projectedPath}`);
  }
  state.auxiliaryDefinitions.push([
    "#[allow(non_snake_case)]",
    "#[derive(Debug, Clone, PartialEq)]",
    `pub struct ${payloadName} {`, ...fields, "}",
  ].join("\n"));
  return payloadName;
}

// represent the original legacy intersection and two maintenance arms without wire tags
function renderMaintenanceLoaderExtension(declaration, state) {
  const pathLabel = "root " + state.rootName;

  // require exactly a legacy intersection and the ordered active and disabled objects
  if (!ts.isTypeAliasDeclaration(declaration) || !ts.isUnionTypeNode(declaration.type) ||
    declaration.type.types.length !== 3) {
    throw new Error(`maintenance loader extension requires three direct arms at ${pathLabel}`);
  }
  const [legacyArm, active, disabled] = declaration.type.types;
  const intersection = unwrapParenthesizedType(legacyArm);

  // keep the legacy extension limited to the one direct optional field
  if (!ts.isIntersectionTypeNode(intersection) || intersection.types.length !== 2 ||
    !ts.isTypeLiteralNode(intersection.types[1]) || intersection.types[1].members.length !== 1 ||
    !isMaintenanceLoaderComparator(intersection.types[1].members[0], true)) {
    throw new Error(`maintenance loader legacy intersection unsupported at ${pathLabel}`);
  }
  const [base, extension] = intersection.types;
  const legacy = resolveMaintenanceLoaderLegacy(base, state, pathLabel);
  const legacyFields = renderObjectProperties(extension.members, { ...state, typePath: ["Legacy"] });
  const comparator = extension.members[0];
  const activeName = renderMaintenanceLoaderPayload(active, state, comparator, true, pathLabel);
  const disabledName = renderMaintenanceLoaderPayload(disabled, state, comparator, false, pathLabel);

  return {
    kind: "maintenance-loader-extension-union",
    definition: [
      "#[allow(non_snake_case)]",
      "#[derive(Debug, Clone, PartialEq)]",
      `pub enum ${state.rootName} {`,
      "    Legacy {",
      `        value: ${legacy},`,
      // retain the actual comparator field while keeping the Rust wrapper storage-only
      ...legacyFields.map((line) => `    ${line.replace(/^    pub /u, "    ")}`),
      "    },",
      `    MaintenanceActive(${activeName}),`,
      `    MaintenanceDisabled(${disabledName}),`,
      "}",
    ].join("\n"),
  };
}

// report whether one reference resolves to one exact local alias declaration
function isExactRecursiveJsonReference(typeNode, checker, declaration, expectedName) {
  // require one direct argument-free identifier reference
  if (
    !ts.isTypeReferenceNode(typeNode) ||
    !ts.isIdentifier(typeNode.typeName) ||
    typeNode.typeName.text !== expectedName ||
    typeNode.typeArguments !== undefined
  ) {
    return false;
  }
  const symbol = checker.getSymbolAtLocation(typeNode.typeName);
  return symbol !== undefined &&
    (symbol.flags & ts.SymbolFlags.Alias) === 0 &&
    symbol.declarations?.length === 1 &&
    symbol.declarations[0] === declaration;
}

// render the one reviewed recursive JSON alias grammar
function renderRecursiveJsonRoot(declaration, state) {
  const sourceFile = state.sourceFile;
  const primitiveName = state.recursiveJson;
  const primitiveMatches = sourceFile.statements.filter((statement) =>
    ts.isTypeAliasDeclaration(statement) && statement.name.text === primitiveName
  );

  // require one distinct exported nongeneric primitive alias in this module
  if (
    !ts.isTypeAliasDeclaration(declaration) ||
    (declaration.typeParameters?.length ?? 0) > 0 ||
    primitiveMatches.length !== 1 ||
    primitiveMatches[0] === declaration ||
    primitiveMatches[0].parent !== sourceFile ||
    !isExported(primitiveMatches[0]) ||
    (primitiveMatches[0].typeParameters?.length ?? 0) > 0 ||
    !ts.isExternalModule(sourceFile)
  ) {
    throw new Error(`recursive JSON primitive alias unsupported at root ${state.rootName}`);
  }
  const primitiveDeclaration = primitiveMatches[0];
  const checker = getSourceChecker(
    state.checkers,
    state.sourcePath,
    sourceFile,
    state.utilityDeclarations,
  );
  const primitiveExport = configuredLeafExport(checker, sourceFile, primitiveName);

  // bind the source export and local compiler symbol to the exact primitive alias
  if (
    primitiveExport === null ||
    resolvedSymbolDeclaration(checker, primitiveExport) !== primitiveDeclaration ||
    !ts.isUnionTypeNode(primitiveDeclaration.type) ||
    primitiveDeclaration.type.types.length !== 4 ||
    primitiveDeclaration.type.types[0].kind !== ts.SyntaxKind.BooleanKeyword ||
    primitiveDeclaration.type.types[1].kind !== ts.SyntaxKind.NumberKeyword ||
    primitiveDeclaration.type.types[2].kind !== ts.SyntaxKind.StringKeyword ||
    !ts.isLiteralTypeNode(primitiveDeclaration.type.types[3]) ||
    primitiveDeclaration.type.types[3].literal.kind !== ts.SyntaxKind.NullKeyword
  ) {
    throw new Error(`recursive JSON primitive alias unsupported at root ${state.rootName}`);
  }

  // require exactly Primitive | readonly Self[] | { readonly [key: string]: Self }
  if (!ts.isUnionTypeNode(declaration.type) || declaration.type.types.length !== 3) {
    throw new Error(`recursive JSON shape unsupported at root ${state.rootName}`);
  }
  const [primitiveArm, arrayArm, objectArm] = declaration.type.types;

  // bind the first arm to the selected primitive alias
  if (!isExactRecursiveJsonReference(
    primitiveArm,
    checker,
    primitiveDeclaration,
    primitiveName,
  )) {
    throw new Error(`recursive JSON primitive alias unsupported at root ${state.rootName}`);
  }

  // bind the second arm to one readonly self array
  if (
    !ts.isTypeOperatorNode(arrayArm) ||
    arrayArm.operator !== ts.SyntaxKind.ReadonlyKeyword ||
    !ts.isArrayTypeNode(arrayArm.type) ||
    !isExactRecursiveJsonReference(
      arrayArm.type.elementType,
      checker,
      declaration,
      state.rootName,
    )
  ) {
    throw new Error(`recursive JSON shape unsupported at root ${state.rootName}`);
  }

  // keep the final arm on one inline object only
  if (!ts.isTypeLiteralNode(objectArm) || objectArm.members.length !== 1) {
    throw new Error(`recursive JSON shape unsupported at root ${state.rootName}`);
  }
  const index = objectArm.members[0];
  const parameter = ts.isIndexSignatureDeclaration(index) && index.parameters.length === 1
    ? index.parameters[0]
    : undefined;

  // keep the object arm on one readonly string index and the exact self alias
  if (
    !ts.isIndexSignatureDeclaration(index) ||
    index.modifiers?.length !== 1 ||
    !isReadonly(index) ||
    index.questionToken !== undefined ||
    index.type === undefined ||
    parameter === undefined ||
    parameter.modifiers !== undefined ||
    parameter.dotDotDotToken !== undefined ||
    parameter.questionToken !== undefined ||
    !ts.isIdentifier(parameter.name) ||
    parameter.type?.kind !== ts.SyntaxKind.StringKeyword ||
    !isExactRecursiveJsonReference(index.type, checker, declaration, state.rootName)
  ) {
    throw new Error(`recursive JSON shape unsupported at root ${state.rootName}`);
  }

  return {
    kind: "recursive-json-value",
    definition: [
      "#[doc = \"dependency-free JSON storage; validation and canonicalization remain in TypeScript\"]",
      "#[derive(Debug, Clone, PartialEq)]",
      `pub enum ${state.rootName} {`,
      "    Boolean(bool),",
      "    Number(f64),",
      "    String(Vec<u16>),",
      "    Null,",
      `    Array(Vec<${state.rootName}>),`,
      `    Object(Vec<(Vec<u16>, ${state.rootName})>),`,
      "}",
    ].join("\n"),
  };
}

// render one exact root-level Pick from an existing configured interface
function renderPickRoot(declaration, state) {
  const { root, fields } = state.pick;
  const typeNode = ts.isTypeAliasDeclaration(declaration) ? declaration.type : undefined;
  const baseType = typeNode?.typeArguments?.[0];

  // require the direct two-argument utility and a bare interface reference
  if (
    typeNode === undefined ||
    !ts.isTypeReferenceNode(typeNode) ||
    !ts.isIdentifier(typeNode.typeName) ||
    typeNode.typeName.text !== "Pick" ||
    typeNode.typeArguments?.length !== 2 ||
    !ts.isTypeReferenceNode(baseType) ||
    !ts.isIdentifier(baseType.typeName) ||
    baseType.typeArguments !== undefined
  ) {
    throw new Error(`pick requires Pick<ConfiguredInterface, "field" | ...> at root ${state.rootName}`);
  }

  const library = state.utilityDeclarations;

  // prove the utility binding in the isolated pinned compiler surface
  if (library === undefined || !ts.isExternalModule(state.sourceFile) || hasGlobalAugmentation(state.sourceFile)) {
    throw new Error(`pick utility identity unsupported at root ${state.rootName}`);
  }
  const checker = getSourceChecker(state.checkers, state.sourcePath, state.sourceFile, library);
  const utility = checker.getSymbolAtLocation(typeNode.typeName);

  // reject aliases and shadows rather than resolving them to a lookalike
  if (
    utility === undefined ||
    (utility.flags & ts.SymbolFlags.Alias) !== 0 ||
    utility.declarations?.length !== 1 ||
    utility.declarations[0] !== library.declarations.Pick
  ) {
    throw new Error(`pick utility identity unsupported at root ${state.rootName}`);
  }

  const configured = state.configuredRootsByName.get(root);
  const base = checker.getSymbolAtLocation(baseType.typeName);
  const baseDeclaration = base?.declarations?.[0];

  // bind only one direct public configured interface from this source
  if (
    baseType.typeName.text !== root ||
    configured === undefined ||
    configured.root.source !== state.sourcePath ||
    configured.root.localInterface === true ||
    base === undefined ||
    (base.flags & ts.SymbolFlags.Alias) !== 0 ||
    base.declarations?.length !== 1 ||
    baseDeclaration !== configured.declaration ||
    !ts.isInterfaceDeclaration(baseDeclaration) ||
    baseDeclaration.parent !== state.sourceFile ||
    !isExported(baseDeclaration) ||
    (baseDeclaration.typeParameters?.length ?? 0) > 0 ||
    (baseDeclaration.heritageClauses?.length ?? 0) > 0
  ) {
    throw new Error(`pick base identity unsupported at root ${state.rootName}`);
  }

  const key = typeNode.typeArguments[1];
  const keys = ts.isUnionTypeNode(key) ? key.types : [key];

  // require the same ordered keys directly in the source and configuration
  if (keys.length !== fields.length || !keys.every(
    // accept only a matching ordinary string literal at each source position
    (item, index) => ts.isLiteralTypeNode(item) && ts.isStringLiteral(item.literal) &&
      item.literal.text === fields[index],
  )) {
    throw new Error(`pick fields mismatch at root ${state.rootName}`);
  }

  state.dependencies.get(state.rootIdentity).add(configured.identity);
  const rendered = state.renderConfiguredRoot(configured.identity);

  // reuse only validated structured fields from the original base
  if (rendered.kind !== "readonly-interface" || rendered.interfaceFieldBlocks === undefined) {
    throw new Error(`pick base identity unsupported at root ${state.rootName}`);
  }
  const baseFields = new Map(rendered.interfaceFieldBlocks.map(
    // index each of the already validated direct source fields
    (field) => [field.name, field],
  ));
  const selected = fields.flatMap(
    // copy the complete annotation and type of one selected field
    (fieldName) => {
      const field = baseFields.get(fieldName);

      // reject any selection that the original interface did not declare
      if (field === undefined) {
        throw new Error(`pick field unsupported at ${state.rootName}.${fieldName}`);
      }
      return field.lines;
    },
  );

  return {
    kind: "picked-readonly-interface",
    definition: [
      "#[allow(non_snake_case)]",
      "#[derive(Debug, Clone, PartialEq)]",
      `pub struct ${state.rootName} {`,
      ...selected,
      "}",
    ].join("\n"),
  };
}

// render one selected root and report its manifest kind
function renderRoot(declaration, state) {
  // keep the marker projection on an exact exported interface
  if (state.pgRowNamedFields && !ts.isInterfaceDeclaration(declaration)) {
    throw new Error("PostgreSQL row projection requires an interface at root " + state.rootName);
  }

  // select the dedicated required record alias only through explicit metadata
  if (state.readonlyNullableMetricRecord !== undefined) {
    return renderReadonlyNullableMetricRecordRoot(declaration, state);
  }

  // select the legacy and maintenance result grammar only by explicit metadata
  if (state.maintenanceLoaderExtension !== undefined) {
    return renderMaintenanceLoaderExtension(declaration, state);
  }

  // select only the explicitly configured Pick grammar
  if (state.pick !== undefined) {
    return renderPickRoot(declaration, state);
  }

  // select only the explicitly configured defaulted list parameter grammar
  if (state.defaultedListParameter !== undefined) {
    // reject aliases, classes, and every other generic declaration shape
    if (!ts.isInterfaceDeclaration(declaration)) {
      throw new Error(`defaulted list parameter root unsupported at ${state.rootName}`);
    }
    const rendered = renderInterfaceRoot(declaration, state);
    return {
      kind: "readonly-interface",
      ...rendered,
    };
  }

  // bind the opted-in readonly union to the exact alias grammar
  if (state.readonlyTaggedUnion) {
    // reject interfaces and other non-alias declarations
    if (!ts.isTypeAliasDeclaration(declaration)) {
      throw new Error(`readonly-tagged union requires a type alias at root ${state.rootName}`);
    }
    return renderTaggedObjectUnionRoot(declaration.type, state);
  }

  // admit base metadata only on interfaces or explicit tagged intersections
  if (
    state.base !== undefined &&
    !ts.isInterfaceDeclaration(declaration) &&
    !(state.discriminator !== undefined &&
      ts.isTypeAliasDeclaration(declaration) &&
      ts.isIntersectionTypeNode(declaration.type))
  ) {
    throw new Error(`configured interface base unsupported at root ${state.rootName}`);
  }

  // admit only the explicit recursive JSON root grammar
  if (state.recursiveJson !== undefined) {
    return renderRecursiveJsonRoot(declaration, state);
  }

  // enable composed nullable literal unions only through explicit metadata
  if (state.literalUnion !== undefined) {
    // keep composition on one exact named nongeneric alias declaration
    if (!ts.isTypeAliasDeclaration(declaration)) {
      throw new Error(`literal union requires a type alias at root ${state.rootName}`);
    }
    return renderComposedLiteralUnionRoot(declaration.type, state);
  }

  // enable named object unions only through explicit root configuration
  if (state.objectUnion !== undefined) {
    // limit this syntax to one named alias with a direct union declaration
    if (!ts.isTypeAliasDeclaration(declaration)) {
      throw new Error(`named object union requires a type alias at root ${state.rootName}`);
    }
    return renderNamedReadonlyObjectUnionRoot(declaration.type, state);
  }

  // enable tagged object unions only through explicit root configuration
  if (state.discriminator !== undefined) {
    // limit this syntax to named aliases with direct union declarations
    if (!ts.isTypeAliasDeclaration(declaration)) {
      throw new Error(`tagged union requires a type alias at root ${state.rootName}`);
    }
    return renderTaggedObjectUnionRoot(declaration.type, state);
  }

  // enable catalog extraction only through explicit root configuration
  if (state.catalog !== undefined) {
    // keep catalog roots limited to named aliases with direct type syntax
    if (!ts.isTypeAliasDeclaration(declaration)) {
      throw new Error(`catalog root requires a type alias at root ${state.rootName}`);
    }

    // select the exact configured catalog grammar without fallback widening
    if (state.catalog.kind === "satisfies-string-tuple") {
      const rendered = renderSatisfiesStringTupleCatalogRoot(declaration.type, state);
      return {
        kind: "string-literal-enum",
        ...rendered,
      };
    }

    // select only the explicitly configured direct typeof tuple syntax
    if (state.catalog.kind === "direct-const-string-tuple") {
      const rendered = renderConstStringTupleRoot(declaration.type, state, true);
      return {
        kind: "string-literal-enum",
        ...rendered,
      };
    }

    // preserve one exact numeric tuple as the existing unsigned enum shape
    if (state.catalog.kind === "unsigned-safe-integer-tuple") {
      const rendered = renderUnsignedSafeIntegerTupleCatalogRoot(declaration.type, state);
      return {
        kind: "unsigned-safe-integer-literal-enum",
        ...rendered,
      };
    }

    // extend one exact configured local string enum by one direct literal
    if (state.catalog.kind === "extend-local-string-enum") {
      const rendered = renderExtendedLocalStringEnumCatalogRoot(declaration.type, state);
      return {
        kind: "string-literal-enum",
        ...rendered,
      };
    }

    const rendered = renderStringPropertyCatalogRoot(declaration.type, state);
    return {
      kind: "string-literal-enum",
      ...rendered,
    };
  }

  // render an explicit readonly interface
  if (ts.isInterfaceDeclaration(declaration)) {
    const rendered = renderInterfaceRoot(declaration, state);
    return {
      kind: "readonly-interface",
      ...rendered,
    };
  }

  const typeNode = declaration.type;

  // reject root references for the same unresolved-identity reason as fields
  if (ts.isTypeReferenceNode(typeNode)) {
    throw new Error(`configured type reference unsupported at root ${state.rootName}`);
  }

  // render only the exact checker-proven const string tuple indexed form
  if (ts.isIndexedAccessTypeNode(typeNode)) {
    const rendered = renderConstStringTupleRoot(typeNode, state);
    return {
      kind: "string-literal-enum",
      ...rendered,
    };
  }

  // render a standalone exact string literal or direct string-literal union
  const stringLiteralArms = ts.isUnionTypeNode(typeNode) ? typeNode.types : [typeNode];
  if (stringLiteralArms.every(
    // exclude aliases and widened values
    (member) => ts.isLiteralTypeNode(member) && ts.isStringLiteralLike(member.literal),
  )) {
    const wireValues = stringLiteralArms.map(
      // preserve the declared order and wire spelling
      (member) => member.literal.text,
    );
    return {
      kind: "string-literal-enum",
      definition: renderStringEnumValues(state.rootName, wireValues, `root ${state.rootName}`),
      wireValues,
    };
  }

  // render only root unions containing explicit numeric syntax
  if (
    ts.isUnionTypeNode(typeNode) &&
    typeNode.types.some((member) => isNumericLiteralSyntax(member))
  ) {
    return {
      kind: "unsigned-safe-integer-literal-enum",
      definition: renderUnsignedIntegerEnum(state.rootName, typeNode),
    };
  }

  // render aliases whose properties are each explicitly readonly
  if (ts.isTypeLiteralNode(typeNode)) {
    const fields = renderObjectProperties(typeNode.members, state);
    return {
      kind: "readonly-object-alias",
      definition: [
        "#[allow(non_snake_case)]",
        "#[derive(Debug, Clone, PartialEq)]",
        `pub struct ${state.rootName} {`,
        ...fields,
        "}",
      ].join("\n"),
    };
  }

  throw new Error(`root type unsupported at ${state.rootName}`);
}

// reject every direct or transitive configured type cycle
function assertAcyclicConfiguredReferences(dependencies, rootNames) {
  const visitStates = new Map();
  const stack = [];

  // visit one configured root in deterministic dependency order
  function visit(rootIdentity) {
    visitStates.set(rootIdentity, "visiting");
    stack.push(rootIdentity);

    // inspect every exact dependency recorded while rendering fields
    for (const dependencyIdentity of dependencies.get(rootIdentity)) {
      const dependencyState = visitStates.get(dependencyIdentity);

      // report the closed cycle from its first repeated root
      if (dependencyState === "visiting") {
        const cycleStart = stack.indexOf(dependencyIdentity);
        const cycle = [...stack.slice(cycleStart), dependencyIdentity]
          .map((identity) => rootNames.get(identity))
          .join(" -> ");
        throw new Error(`recursive configured type reference cycle: ${cycle}`);
      }

      // visit each dependency only once after it is proven acyclic
      if (dependencyState !== "visited") {
        visit(dependencyIdentity);
      }
    }

    stack.pop();
    visitStates.set(rootIdentity, "visited");
  }

  // start from roots in explicit configuration order
  for (const rootIdentity of dependencies.keys()) {
    // skip roots already covered by an earlier traversal
    if (visitStates.get(rootIdentity) !== "visited") {
      visit(rootIdentity);
    }
  }
}

// require every private configured declaration to serve its reviewed public grammar
function assertPrivateRootsReachable(config, dependencies) {
  const reachable = new Set();
  const rootsByIdentity = new Map(
    config.roots.map((root) => [configuredRootIdentity(root), root]),
  );

  // traverse dependencies from every exported configured root
  function visit(identity) {
    // stop when an earlier public path already covered this root
    if (reachable.has(identity)) {
      return;
    }
    reachable.add(identity);

    // follow only dependencies proven during rendering
    for (const dependency of dependencies.get(identity)) {
      visit(dependency);
    }
  }

  // seed traversal from all ordinary public roots
  for (const root of config.roots) {
    if (root.localAlias !== true && root.localInterface !== true && root.pgQueryOwners === undefined) {
      visit(configuredRootIdentity(root));
    }
  }

  // reject private roots that exist only as standalone generation requests
  for (const root of config.roots) {
    if (root.localAlias === true && !reachable.has(configuredRootIdentity(root))) {
      throw new Error(`local alias is not reachable from an exported root ${root.source}#${root.exportName}`);
    }
  }

  // bind every private interface only to direct ordinary exported inheritance
  for (const root of config.roots) {
    // leave the established private-alias grammar to transitive reachability above
    if (root.localInterface !== true) {
      continue;
    }

    const identity = configuredRootIdentity(root);
    const consumers = [];

    // inspect every proven dependency edge into this private interface
    for (const [consumerIdentity, consumerDependencies] of dependencies) {
      // ignore roots that did not reference this interface
      if (!consumerDependencies.has(identity)) {
        continue;
      }

      const consumer = rootsByIdentity.get(consumerIdentity);
      const isOrdinaryExportedBase =
        consumer !== undefined &&
        consumer.localAlias !== true &&
        consumer.localInterface !== true &&
        consumer.source === root.source &&
        consumer.base === root.exportName &&
        consumer.discriminator === undefined &&
        consumer.omitBaseFields === undefined;

      // reject field, union, tagged, omitted, imported, or private reuse
      if (!isOrdinaryExportedBase) {
        throw new Error(
          `local interface reuse unsupported ${root.source}#${root.exportName}`,
        );
      }
      consumers.push(consumerIdentity);
    }

    // require at least one configured exported interface to flatten the base
    if (consumers.length === 0) {
      throw new Error(
        `local interface is not an ordinary base of an exported root ${root.source}#${root.exportName}`,
      );
    }
  }
}

// reject projections placed on fields owned by configured interface bases
function assertNoInheritedFieldProjections(config, selectedRoots) {
  // inspect only roots that declare both a base and projection paths
  for (const root of config.roots) {
    if (root.base === undefined || root.fieldProjections === undefined) {
      continue;
    }
    const inheritedNames = new Set();
    const seenBases = new Set();
    let baseName = root.base;

    // collect direct members from the closed configured base chain
    while (baseName !== undefined) {
      const baseIdentity = `${root.source}\0${baseName}`;

      // stop malformed or cyclic bases for the ordinary renderer to diagnose
      if (seenBases.has(baseIdentity)) {
        break;
      }
      seenBases.add(baseIdentity);
      const selectedBase = selectedRoots.get(baseIdentity);

      // leave missing and non-interface bases to existing root validation
      if (selectedBase === undefined || !ts.isInterfaceDeclaration(selectedBase.declaration)) {
        break;
      }

      // retain only identifier properties that can own rendered wire fields
      for (const member of selectedBase.declaration.members) {
        if (ts.isPropertySignature(member) && ts.isIdentifier(member.name)) {
          inheritedNames.add(member.name.text);
        }
      }
      baseName = selectedBase.root.base;
    }

    // reject every projection whose leading field is inherited
    for (const fieldPath of Object.keys(root.fieldProjections)) {
      const leadingField = fieldPath.split(".", 1)[0];

      // omitted base fields are owned by the replacement declaration
      if (
        inheritedNames.has(leadingField) &&
        !root.omitBaseFields?.includes(leadingField)
      ) {
        throw new Error(`unused field projection at ${root.exportName}.${fieldPath}`);
      }
    }
  }
}

// render wrappers only when selected roots require them
function renderHelpers(
  needsNullable,
  needsPresence,
  needsOrderedFiniteNumberRecord,
  needsJsDateOrString,
  needsJsDate,
) {
  const definitions = [];

  // preserve dynamic JSON object entry order without choosing a serializer
  if (needsOrderedFiniteNumberRecord) {
    definitions.push([
      "#[doc = \"wire rule: ordered-finite-f64-record, ordered unique string keys and finite values\"]",
      "#[derive(Debug, Clone, PartialEq)]",
      "pub struct OrderedFiniteNumberRecord(pub Vec<(String, f64)>);",
    ].join("\n"));
  }

  // preserve required null independently from a value
  if (needsNullable) {
    definitions.push([
      "#[derive(Debug, Clone, PartialEq)]",
      "pub enum Nullable<T> {",
      "    Null,",
      "    Value(T),",
      "}",
    ].join("\n"));
  }

  // preserve an optional missing field independently from its value
  if (needsPresence) {
    definitions.push([
      "#[derive(Debug, Clone, PartialEq)]",
      "pub enum FieldPresence<T> {",
      "    Missing,",
      "    Value(T),",
      "}",
    ].join("\n"));
  }

  // preserve the three observable timestamp payloads without parsing or formatting
  if (needsJsDateOrString) {
    definitions.push([
      "#[derive(Debug, Clone, PartialEq, Eq)]",
      "pub enum JsDateOrString {",
      "    #[doc = \"Date getTime() as integer milliseconds in -8640000000000000..=8640000000000000\"]",
      "    DateEpochMilliseconds(i64),",
      "    InvalidDate,",
      "    #[doc = \"Raw JavaScript string code units without timestamp normalization\"]",
      "    String(Vec<u16>),",
      "}",
    ].join("\n"));
  }

  // keep the Date-only storage independent from its string-bearing counterpart
  if (needsJsDate) {
    definitions.push([
      "#[derive(Debug, Clone, Copy, PartialEq, Eq)]",
      "pub enum JsDate {",
      "    #[doc = \"Date getTime() as integer milliseconds in -8640000000000000..=8640000000000000\"]",
      "    EpochMilliseconds(i64),",
      "    InvalidDate,",
      "}",
    ].join("\n"));
  }

  return definitions;
}

// validate source inputs against the exact selected source set
function normalizeSources(config, sources) {
  // require an ordinary string dictionary
  if (!isPlainObject(sources)) {
    throw new Error("sources must be a plain object");
  }

  const selectedPaths = [...new Set([
    ...config.roots.map((root) => root.source),
    ...config.roots.flatMap((root) => Object.values(root.indexedLiteralSources ?? {})),
    ...(config.typeImports ?? []).flatMap((typeImport) =>
      typeImport.packageJson === undefined
        ? [typeImport.entry]
        : [
          typeImport.packageJson,
          typeImport.entry,
          ...typeImport.leaves,
          ...Object.values(typeImport.compiledSource ?? {}),
        ]
    ),
  ])].sort();
  const suppliedPaths = Object.keys(sources).sort();

  // reject omitted or unrelated source bytes
  if (
    selectedPaths.length !== suppliedPaths.length ||
    selectedPaths.some((sourcePath, index) => sourcePath !== suppliedPaths[index])
  ) {
    throw new Error("sources must contain exactly the selected config source paths");
  }

  const normalized = new Map();

  // retain exact source text for parsing and hashing
  for (const sourcePath of selectedPaths) {
    // reject buffers and implicit string conversion
    if (typeof sources[sourcePath] !== "string") {
      throw new Error(`source ${sourcePath} must be a string`);
    }
    normalized.set(sourcePath, sources[sourcePath]);
  }

  return normalized;
}

// generate dependency-free Rust declarations and their deterministic manifest
export function generateRustContracts(
  config,
  sources,
  readDeclarationFile = readFileSync,
  retainedRainHurdleWindRust = undefined,
) {
  const normalizedConfig = normalizeConfig(config);
  const normalizedSources = normalizeSources(normalizedConfig, sources);
  const needsPgRowMarker = normalizedConfig.roots.some((root) => root.pgRowNamedFields === true);

  const pgRowMarker = needsPgRowMarker ? verifyPgRowMarker(readDeclarationFile) : undefined;

  // locate every source that needs the pinned utility checker surface
  const utilitySources = new Set(
    normalizedConfig.roots.flatMap((root) => {
      const projectionKinds = Object.values(root.fieldProjections ?? {}).map(
        (projection) => projection.kind,
      );
      const rootUsesUtility = Object.values(root.fields).some((mapping) =>
        UTILITY_FIELD_MAPPING_KINDS.has(mapping)
      ) || projectionKinds.includes("return-enum") ||
        projectionKinds.includes("indexed-literals") ||
        projectionKinds.includes("exclude-literal") ||
        projectionKinds.includes("omit-interface-array") ||
        root.omitBaseFields !== undefined ||
        root.readonlyTaggedUnion === true ||
        root.pick !== undefined ||
        root.readonlyNullableMetricRecord !== undefined ||
        root.disabledReasonExtract !== undefined;

      // indexed alias graphs may use Extract in any explicitly selected source
      if (projectionKinds.includes("indexed-literals")) {
        return [root.source, ...Object.values(root.indexedLiteralSources ?? {})];
      }
      return rootUsesUtility ? [root.source] : [];
    }),
  );
  const utilityDeclarationsBySource = new Map();

  // reverify each opted-in call and isolate the compiler's mutable ASTs
  if (utilitySources.size > 0) {
    const utilityText = readVerifiedUtilityText(readDeclarationFile);

    // bind one fresh library AST in each selected source program
    for (const sourcePath of utilitySources) {
      utilityDeclarationsBySource.set(sourcePath, parseUtilityDeclarations(utilityText));
    }
  }
  const sourceFiles = new Map();

  // parse each selected TypeScript source once while retaining JSON bytes only
  for (const [sourcePath, sourceText] of normalizedSources) {
    if (sourcePath.endsWith(".ts")) {
      sourceFiles.set(sourcePath, parseSource(sourcePath, sourceText));
    }
  }

  const configuredDeclarationsBySource = new Map();
  const indexedLiteralDeclarationsByRoot = new Map();
  const selectedIndexedLiteralDeclarations = new Map();
  const selectedRoots = new Map();
  const configuredRootsByName = new Map();
  const dependencies = new Map();
  const dependencyRootNames = new Map();
  const checkers = new Map();

  // create declaration identity maps before rendering any reference
  for (const sourcePath of sourceFiles.keys()) {
    configuredDeclarationsBySource.set(sourcePath, new Map());
  }

  // select every configured declaration before resolving field references
  for (const root of normalizedConfig.roots) {
    const declaration = findRootDeclaration(sourceFiles.get(root.source), root);
    const identity = configuredRootIdentity(root);
    const configured = { root, identity };
    configuredDeclarationsBySource.get(root.source).set(declaration, configured);
    selectedRoots.set(identity, { ...configured, declaration });
    configuredRootsByName.set(root.exportName, { ...configured, declaration });
    dependencies.set(identity, new Set());
    dependencyRootNames.set(identity, root.exportName);

    const indexedLiteralDeclarations = new Map();

    // select every proof-only alias before any projection traversal begins
    for (const [aliasName, sourcePath] of Object.entries(root.indexedLiteralSources ?? {})) {
      const proofIdentity = `${sourcePath}\0${aliasName}`;
      const declaration = findIndexedLiteralDeclaration(
        sourceFiles.get(sourcePath),
        sourcePath,
        aliasName,
      );
      const priorDeclaration = selectedIndexedLiteralDeclarations.get(proofIdentity);

      // require repeated root selections to retain one exact declaration object
      if (priorDeclaration !== undefined && priorDeclaration !== declaration) {
        throw new Error(`indexed literal declaration identity conflict ${sourcePath}#${aliasName}`);
      }
      selectedIndexedLiteralDeclarations.set(proofIdentity, declaration);
      indexedLiteralDeclarations.set(aliasName, declaration);
    }
    indexedLiteralDeclarationsByRoot.set(identity, indexedLiteralDeclarations);
  }

  assertNoInheritedFieldProjections(normalizedConfig, selectedRoots);

  const typeImportGraph = validateTypeImportGraphs(
    normalizedConfig,
    normalizedSources,
    sourceFiles,
    selectedRoots,
    selectedIndexedLiteralDeclarations,
    checkers,
    utilityDeclarationsBySource,
    pgRowMarker,
  );

  const generatedTypeNames = new Set([
    ...GENERATED_RUST_TYPE_NAMES,
    ...normalizedConfig.roots.map((root) => root.exportName),
  ]);
  const auxiliaryDefinitions = [];
  const rootDefinitions = [];
  const rootManifest = [];
  const needsNullable = { value: false };
  const needsPresence = { value: false };
  const needsOrderedFiniteNumberRecord = { value: false };
  const needsJsDateOrString = { value: false };
  const needsJsDate = { value: false };
  const renderedRoots = new Map();
  const renderingRoots = [];

  // render one configured root once, resolving base roots before descendants
  function renderConfiguredRoot(rootIdentity) {
    const cached = renderedRoots.get(rootIdentity);

    // reuse exact field descriptors and auxiliary type identities
    if (cached !== undefined) {
      return cached;
    }

    const cycleStart = renderingRoots.indexOf(rootIdentity);

    // stop recursive base chains before entering an unbounded render
    if (cycleStart !== -1) {
      const cycle = [...renderingRoots.slice(cycleStart), rootIdentity]
        .map((identity) => dependencyRootNames.get(identity))
        .join(" -> ");
      throw new Error(`recursive configured type reference cycle: ${cycle}`);
    }

    const selected = selectedRoots.get(rootIdentity);
    const { root, declaration } = selected;
    const sourceFile = sourceFiles.get(root.source);
    const fieldMappings = new Map(Object.entries(root.fields));
    const usedFieldMappings = new Set();
    const fieldProjections = new Map(Object.entries(root.fieldProjections ?? {}));
    const usedFieldProjections = new Set();
    const wireFieldNames = new Map(Object.entries(root.wireFieldNames ?? {}));
    const usedWireFieldNames = new Set();
    const indexedLiteralSources = new Map(Object.entries(root.indexedLiteralSources ?? {}));
    const usedIndexedLiteralSources = new Set();
    const nestedObjectDescriptors = { byNode: new Map(), byPath: new Map() };

    // reject every generic root outside the one explicit list-parameter grammar
    if (
      declaration.typeParameters !== undefined &&
      declaration.typeParameters.length > 0 &&
      root.defaultedListParameter === undefined
    ) {
      throw new Error(`generic type unsupported at root ${root.exportName}`);
    }

    const state = {
      rootName: root.exportName,
      rootIdentity,
      rootDeclaration: declaration,
      sourcePath: root.source,
      sourceFile,
      fieldPath: [],
      fieldMappings,
      usedFieldMappings,
      fieldProjections,
      usedFieldProjections,
      wireFieldNames,
      usedWireFieldNames,
      configuredDeclarations: configuredDeclarationsBySource.get(root.source),
      configuredDeclarationsBySource,
      configuredRootsByName,
      dependencies,
      checkers,
      utilityDeclarations: utilityDeclarationsBySource.get(root.source),
      utilityDeclarationsBySource,
      typeImportBindings: typeImportGraph.byConsumer.get(root.source),
      typeImportBindingsByConsumer: typeImportGraph.byConsumer,
      sourceFiles,
      indexedLiteralSources,
      indexedLiteralDeclarations: indexedLiteralDeclarationsByRoot.get(rootIdentity),
      usedIndexedLiteralSources,
      nestedObjectDescriptors,
      generatedTypeNames,
      auxiliaryDefinitions,
      needsNullable,
      needsPresence,
      needsOrderedFiniteNumberRecord,
      needsJsDateOrString,
      needsJsDate,
      discriminator: root.discriminator,
      readonlyTaggedUnion: root.readonlyTaggedUnion === true,
      pick: root.pick,
      base: root.base,
      importedBase: root.importedBase === true,
      identicalImportedStringFields: root.identicalImportedStringFields,
      omitBaseFields: root.omitBaseFields,
      omitInheritedBaseFields: root.omitInheritedBaseFields,
      addedAfterOmit: root.addedAfterOmit,
      ownFieldOrderAfterOmit: root.ownFieldOrderAfterOmit,
      catalog: root.catalog,
      objectUnion: root.objectUnion,
      importedObjectUnion: root.importedObjectUnion === true,
      literalUnion: root.literalUnion,
      recursiveJson: root.recursiveJson,
      localAlias: root.localAlias === true,
      defaultedListParameter: root.defaultedListParameter,
      pgRowNamedFields: root.pgRowNamedFields === true,
      pgQueryOwners: root.pgQueryOwners,
      readonlyNullableMetricRecord: root.readonlyNullableMetricRecord,
      maintenanceLoaderExtension: root.maintenanceLoaderExtension,
      disabledReasonExtract: root.disabledReasonExtract,
      activeImportedConstString: root.activeImportedConstString,
      pgRowMarkerDeclaration: pgRowMarker?.marker,
      renderConfiguredRoot,
    };
    renderingRoots.push(rootIdentity);
    let rendered;

    // always restore the traversal stack after one bounded render
    try {
      rendered = renderRoot(declaration, state);
    } finally {
      renderingRoots.pop();
    }

    // reject stale or misspelled field metadata
    for (const fieldPath of fieldMappings.keys()) {
      // report the first unused mapping deterministically
      if (!usedFieldMappings.has(fieldPath)) {
        throw new Error(`unused field mapping at ${root.exportName}.${fieldPath}`);
      }
    }


    // reject stale, inherited, or misspelled field projection paths
    for (const fieldPath of fieldProjections.keys()) {
      if (!usedFieldProjections.has(fieldPath)) {
        throw new Error(`unused field projection at ${root.exportName}.${fieldPath}`);
      }
    }

    // reject stale or misspelled quoted wire-name metadata
    for (const fieldPath of wireFieldNames.keys()) {
      // report the first unused normalized mapping deterministically
      if (!usedWireFieldNames.has(fieldPath)) {
        throw new Error(`unused wire field mapping at ${root.exportName}.${fieldPath}`);
      }
    }

    // reject stale, misspelled, or unreachable indexed alias proof inputs
    for (const aliasName of indexedLiteralSources.keys()) {
      if (!usedIndexedLiteralSources.has(aliasName)) {
        throw new Error(`unused indexed literal source at ${root.exportName}.${aliasName}`);
      }
    }

    renderedRoots.set(rootIdentity, rendered);
    return rendered;
  }

  // render roots in explicit configuration order
  for (const root of normalizedConfig.roots) {
    const rootIdentity = configuredRootIdentity(root);
    const rendered = renderConfiguredRoot(rootIdentity);

    rootDefinitions.push(rendered.definition);
    // add only explicitly configured root metadata
    rootManifest.push({
      source: root.source,
      exportName: root.exportName,
      rustName: root.exportName,
      kind: rendered.kind,
      ...(root.readonlyTaggedUnion ? { readonlyTaggedUnion: true } : {}),
      ...(root.pick === undefined ? {} : { pick: root.pick }),
      ...(root.base === undefined ? {} : { base: root.base }),
      ...(root.importedBase === undefined ? {} : { importedBase: true }),
      ...(root.identicalImportedStringFields === undefined
        ? {}
        : { identicalImportedStringFields: root.identicalImportedStringFields }),
      ...(root.omitBaseFields === undefined
        ? {}
        : { omitBaseFields: root.omitBaseFields }),
      ...(root.omitInheritedBaseFields === undefined
        ? {}
        : { omitInheritedBaseFields: root.omitInheritedBaseFields }),
      ...(root.addedAfterOmit === undefined ? {} : { addedAfterOmit: root.addedAfterOmit }),
      ...(root.ownFieldOrderAfterOmit === undefined ? {} : { ownFieldOrderAfterOmit: root.ownFieldOrderAfterOmit }),
      ...(root.catalog === undefined ? {} : { catalog: root.catalog }),
      ...(root.literalUnion === undefined ? {} : { literalUnion: root.literalUnion }),
      ...(root.recursiveJson === undefined ? {} : { recursiveJson: root.recursiveJson }),
      ...(root.localAlias === undefined ? {} : { localAlias: true }),
      ...(root.localInterface === undefined ? {} : { localInterface: true }),
      ...(root.wireFieldNames === undefined ? {} : { wireFieldNames: root.wireFieldNames }),
      ...(root.importedObjectUnion === undefined ? {} : { importedObjectUnion: true }),
      ...(root.indexedLiteralSources === undefined
        ? {}
        : { indexedLiteralSources: root.indexedLiteralSources }),
      ...(root.fieldProjections === undefined
        ? {}
        : { fieldProjections: root.fieldProjections }),
      ...(root.defaultedListParameter === undefined
        ? {}
        : { defaultedListParameter: root.defaultedListParameter }),
      ...(root.pgRowNamedFields ? { pgRowNamedFields: true } : {}),
      ...(root.pgQueryOwners === undefined ? {} : { pgQueryOwners: root.pgQueryOwners }),
      ...(root.readonlyNullableMetricRecord === undefined
        ? {}
        : { readonlyNullableMetricRecord: root.readonlyNullableMetricRecord }),
      ...(root.disabledReasonExtract === undefined ? {} : { disabledReasonExtract: root.disabledReasonExtract }),
      ...(root.activeImportedConstString === undefined
        ? {}
        : { activeImportedConstString: root.activeImportedConstString }),
      ...(root.maintenanceLoaderExtension === undefined
        ? {}
        : { maintenanceLoaderExtension: root.maintenanceLoaderExtension }),
      ...(rendered.objectUnion === undefined ? {} : { objectUnion: rendered.objectUnion }),
      ...(rendered.discriminator === undefined
        ? {}
        : { discriminator: rendered.discriminator }),
    });
  }

  // reject configured package exports that no selected consumer root used
  for (const typeImport of typeImportGraph.verified) {
    const binding = typeImportGraph.byConsumer
      .get(typeImport.consumer)
      .get(typeImport.specifier);

    // reject an edge that proves no consumed frozen tuple wrapper
    if (typeImport.tupleIdentityWrapper !== undefined && !binding.tupleIdentityWrapperUsed) {
      throw new Error(`unused configured tuple identity wrapper ${typeImport.specifier}#${typeImport.tupleIdentityWrapper} for ${typeImport.consumer}`);
    }

    // reject unused proof edges for imported string literals
    if (typeImport.literalStringValue !== undefined && !binding.literalStringValueUsed) {
      throw new Error(`unused configured literal string value ${typeImport.specifier}#${typeImport.literalStringValue} for ${typeImport.consumer}`);
    }

    // require every proof name to participate in the generated dependency graph
    for (const exportName of Object.keys(typeImport.exports)) {
      if (!binding.used.has(exportName)) {
        throw new Error(
          `unused configured type import ${typeImport.specifier}#${exportName} for ${typeImport.consumer}`,
        );
      }
    }

    // require every proof-only name to participate in a configured proof
    for (const proofName of Object.keys(typeImport.proofExports ?? {})) {
      if (!binding.proofUsed.has(proofName)) {
        throw new Error(
          `unused configured proof export ${typeImport.specifier}#${proofName} for ${typeImport.consumer}`,
        );
      }
    }
  }

  assertAcyclicConfiguredReferences(dependencies, dependencyRootNames);
  assertPrivateRootsReachable(normalizedConfig, dependencies);

  // bind the fixed station material only when the original type graph is selected complete
  const retainedForecastObservation = FORECAST_OBSERVATION_STATION_ROOTS.every(
    (name) => normalizedConfig.roots.some(
      (root) => root.source === FORECAST_OBSERVATION_SOURCE_PATH && root.exportName === name,
    ),
  ) ? renderRetainedForecastObservationStations(sourceFiles.get(FORECAST_OBSERVATION_SOURCE_PATH), {
    variant: (wire) => pascalCase(wire, "string literal"),
    rustString,
  }) : undefined;
  const definitions = [
    ...renderHelpers(
      needsNullable.value,
      needsPresence.value,
      needsOrderedFiniteNumberRecord.value,
      needsJsDateOrString.value,
      needsJsDate.value,
    ),
    ...auxiliaryDefinitions,
    ...rootDefinitions,
    ...(retainedForecastObservation === undefined ? [] : [retainedForecastObservation]),
    ...(retainedRainHurdleWindRust === undefined ? [] : [retainedRainHurdleWindRust]),
  ];
  const rust = [
    "// generated by scripts/generate-rust-contracts.mjs; do not edit",
    "// source-only subset; no TypeScript module code was evaluated",
    "",
    definitions.join("\n\n"),
    "",
  ].join("\n");
  const manifestValue = {
    schemaVersion: MANIFEST_SCHEMA_VERSION,
    generator: {
      typescriptVersion: TYPESCRIPT_VERSION,
      subsetVersion: SUBSET_VERSION,
      ...(needsPgRowMarker ? { pgRowMarkerInputSha256: PG_MARKER_INPUTS } : {}),
      ...(utilityDeclarationsBySource.size === 0
        ? {}
        : { utilityDeclarationsSha256: UTILITY_DECLARATIONS_SHA256 }),
    },
    config: {
      sha256: sha256(canonicalJson(normalizedConfig, "config")),
    },
    sources: [...normalizedSources].map(([sourcePath, sourceText]) => ({
      path: sourcePath,
      sha256: sha256(sourceText),
    })),
    roots: rootManifest,
    ...(typeImportGraph.verified.length === 0
      ? {}
      : { typeImports: typeImportGraph.verified }),
    output: {
      path: RUST_OUTPUT_PATH,
      sha256: sha256(rust),
    },
  };
  const manifest = `${JSON.stringify(manifestValue, null, 2)}\n`;
  return { rust, manifest };
}

// reject malformed utf-8 instead of hashing replacement characters
async function readContractInput(repositoryRoot, inputPath) {
  const bytes = await readFile(resolve(repositoryRoot, inputPath));

  // keep initial bom handling identical to the previous node string read
  try {
    return new TextDecoder("utf-8", { fatal: true, ignoreBOM: true }).decode(bytes);
  } catch (error) {
    throw new Error(`contract input is not valid UTF-8: ${inputPath}`, { cause: error });
  }
}

// load selected source text from the fixed repository root
async function loadRepositoryInputs(repositoryRoot, config) {
  const normalizedConfig = normalizeConfig(config);
  const sourcePaths = [...new Set([
    ...normalizedConfig.roots.map((root) => root.source),
    ...normalizedConfig.roots.flatMap((root) =>
      Object.values(root.indexedLiteralSources ?? {})
    ),
    ...(normalizedConfig.typeImports ?? []).flatMap((typeImport) =>
      typeImport.packageJson === undefined
        ? [typeImport.entry]
        : [
          typeImport.packageJson,
          typeImport.entry,
          ...typeImport.leaves,
          ...Object.values(typeImport.compiledSource ?? {}),
        ]
    ),
  ])].sort();
  const sources = {};

  // read each selected source as exact UTF-8 text
  for (const sourcePath of sourcePaths) {
    sources[sourcePath] = await readContractInput(repositoryRoot, sourcePath);
  }

  return sources;
}

// write generated artifacts at their fixed repository paths
async function writeOutputs(repositoryRoot, generated, retainedRainHurdleWindArtifact) {
  const rustPath = resolve(repositoryRoot, RUST_OUTPUT_PATH);
  const manifestPath = resolve(repositoryRoot, MANIFEST_OUTPUT_PATH);
  await mkdir(dirname(rustPath), { recursive: true });
  await writeFile(rustPath, generated.rust, "utf8");
  await writeFile(manifestPath, generated.manifest, "utf8");

  // production inputs additionally retain the authenticated model bytes
  if (retainedRainHurdleWindArtifact !== undefined) {
    await writeFile(
      resolve(repositoryRoot, RETAINED_RAIN_HURDLE_WIND_OUTPUT_PATH),
      retainedRainHurdleWindArtifact,
      "utf8",
    );
  }
}

// compare generated artifacts without mutating the repository
async function checkOutputs(repositoryRoot, generated, retainedRainHurdleWindArtifact) {
  const outputs = [
    [RUST_OUTPUT_PATH, generated.rust],
    [MANIFEST_OUTPUT_PATH, generated.manifest],
  ];
  const drifted = [];

  // preserve scratch-generator behavior while checking the production retained artifact
  if (retainedRainHurdleWindArtifact !== undefined) {
    outputs.push([
      RETAINED_RAIN_HURDLE_WIND_OUTPUT_PATH,
      retainedRainHurdleWindArtifact,
    ]);
  }

  // compare exact UTF-8 bytes for each generated output
  for (const [outputPath, expected] of outputs) {
    let actual;

    // report missing outputs as ordinary drift
    try {
      actual = await readFile(resolve(repositoryRoot, outputPath));
    } catch (error) {
      // retain non-missing read failures as actionable diagnostics
      if (error?.code !== "ENOENT") {
        throw error;
      }
      actual = null;
    }

    // collect every drifted output before failing
    if (actual === null || !actual.equals(Buffer.from(expected, "utf8"))) {
      drifted.push(outputPath);
    }
  }

  // keep check mode read-only and fail with one remediation command
  if (drifted.length > 0) {
    throw new Error(`generated contract drift: ${drifted.join(", ")}; run node scripts/generate-rust-contracts.mjs --write`);
  }
}

// run the fixed repository CLI
async function main(argv) {
  // accept exactly one documented mode
  if (argv.length !== 1 || (argv[0] !== "--write" && argv[0] !== "--check")) {
    throw new Error("usage: node scripts/generate-rust-contracts.mjs --write|--check");
  }

  const repositoryRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
  const config = JSON.parse(await readContractInput(repositoryRoot, CONFIG_PATH));
  const sources = await loadRepositoryInputs(repositoryRoot, config);
  const retainedRainHurdleWind = sources[RETAINED_RAIN_HURDLE_WIND_SOURCE_PATH] ===
    undefined
    ? undefined
    : extractRetainedRainHurdleWindArtifact(
      sources[RETAINED_RAIN_HURDLE_WIND_SOURCE_PATH],
      sources[RAIN_HURDLE_WIND_RUNTIME_SOURCE_PATH],
    );
  const generated = generateRustContracts(
    config,
    sources,
    readFileSync,
    retainedRainHurdleWind?.rust,
  );
  const retainedRainHurdleWindArtifact = retainedRainHurdleWind?.artifact;

  // write only in the explicit mutation mode
  if (argv[0] === "--write") {
    await writeOutputs(repositoryRoot, generated, retainedRainHurdleWindArtifact);
    const retainedOutput = retainedRainHurdleWindArtifact === undefined
      ? ""
      : ` and ${RETAINED_RAIN_HURDLE_WIND_OUTPUT_PATH}`;
    process.stdout.write(
      `wrote ${RUST_OUTPUT_PATH} and ${MANIFEST_OUTPUT_PATH}${retainedOutput}\n`,
    );
    return;
  }

  await checkOutputs(repositoryRoot, generated, retainedRainHurdleWindArtifact);
  process.stdout.write("generated Rust contracts are current\n");
}

// avoid CLI side effects when imported by tests or build tooling
if (
  process.argv[1] !== undefined &&
  resolve(process.argv[1]) === fileURLToPath(import.meta.url)
) {
  main(process.argv.slice(2)).catch((error) => {
    process.stderr.write(`generate-rust-contracts: ${error instanceof Error ? error.message : String(error)}\n`);
    process.exitCode = 1;
  });
}
