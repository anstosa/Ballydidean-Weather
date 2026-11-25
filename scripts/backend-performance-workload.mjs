import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";

export const BACKEND_PERFORMANCE_WORKLOAD_VERSION = "weather-backend-performance-workload/v2";
export const BACKEND_PERFORMANCE_SCHEDULE_VERSION = "weather-backend-performance-planned-schedule/v2";

const DEFINITION_URL = new URL("../config/backend-performance-workload-v2.json", import.meta.url);
const DEFINITION_SHA256 = "5722eeecd2734a1cfe4e7887cfc887dc6ac052cb397e08f8389fd6b284c5c364";
const DEFINITION_KEYS = Object.freeze([
  "bindingPrerequisites",
  "bindingSlotContracts",
  "bindingSlotTypes",
  "closedSelectors",
  "coldPolicy",
  "contractVersion",
  "counts",
  "groups",
  "logicalGenerationRoles",
  "operations",
  "redactionPolicy",
  "runPolicy",
  "timeoutPolicies",
  "transitionProducers",
]);
const EXPECTED_COUNTS = Object.freeze({
  applicationCacheColdAttempts: 15,
  dbOsColdAttempts: 140,
  executableOneShots: 18,
  groups: 59,
  oncePerRunAttempts: 395,
  operations: 107,
  processPoolColdAttempts: 140,
  providerCases: 24,
  publicGets: 28,
  runSlots: 5,
  totalAttempts: 6290,
  transitions: 10,
  typedSlots: 156,
  warmAttempts: 5600,
});
const EXPECTED_CLASSIFICATION_COUNTS = Object.freeze({
  immutableShared: 142,
  phaseProvenance: 2,
  secretHandles: 2,
  total: 156,
  transitions: 10,
});
const EXPECTED_RUN_SLOTS = Object.freeze([
  "run-slot-01",
  "run-slot-02",
  "run-slot-03",
  "run-slot-04",
  "run-slot-05",
]);
const EXPECTED_MONTHLY_SLOT = "oneshot.maintenance.monthlyInputs";
const EXPECTED_MONTHLY_CONTRACT_KEYS = Object.freeze([
  "contractVersion",
  "sourceOwnedRoster",
  "exactKeys",
  "orderedChildExactKeys",
  "rules",
]);
const EXPECTED_MONTHLY_ROSTER = Object.freeze({
  source: "scripts/research/adjustment_maintenance_controller.mjs:163 FAMILIES and :1260-1271 monthly due keys",
  exactOrder: Object.freeze(["temperature", "wind", "rain"]),
  cardinality: 3,
});
const EXPECTED_MONTHLY_PARENT_KEYS = Object.freeze([
  "contractVersion",
  "referenceReceiptSha256",
  "referenceMonth",
  "calendarMonthStartAt",
  "operationId",
  "familyOrder",
  "children",
  "familyInputAggregateRootSha256",
  "preStateProjectionSha256",
  "expectedRestoredStateProjectionSha256",
]);
const EXPECTED_MONTHLY_CHILD_KEYS = Object.freeze([
  "contractVersion",
  "family",
  "dueKey",
  "fitMonth",
  "inputManifestSha256",
  "historicalMemberRootSha256",
  "modelArtifactSha256",
  "enabledBranches",
  "expectedTerminalOutcome",
  "expectedCompletedWorkCount",
  "expectedStateEffectProjectionSha256",
]);
const EXPECTED_MONTHLY_RULES = Object.freeze([
  "children are exactly three and index-equal to familyOrder temperature/wind/rain; no implementation selects cardinality",
  "each child is exactly weather-benchmark-monthly-input-child/v1 with orderedChildExactKeys; no childInputSha256 or other per-child digest field exists",
  "expectedCompletedWorkCount is an integer greater than zero for every child and is derived before baseline/candidate execution from the admitted corpus",
  "familyInputAggregateRootSha256 is SHA-256 over its literal versioned domain plus canonical familyOrder, the three complete immutable child core bytes, preStateProjectionSha256 and expectedRestoredStateProjectionSha256; it excludes itself and every phase receipt",
  "monthlyInputsSha256 is an external digest of the complete immutable slot bytes including familyInputAggregateRootSha256; it is never embedded in that slot",
  "if the admitted corpus cannot satisfy all three exact children/outcomes/nonempty counts, binding fails before metrics; no child is skipped or weakened",
  "one existing operation, one validated-one-shot-execution completed work unit and one atomic-group restore remain; child counts and aggregate roots are evidence, not operations, attempts, work units, slots or transitions",
]);
const EXPECTED_MONTHLY_SLOT_CONTRACT = Object.freeze({
  contractVersion: "weather-benchmark-monthly-inputs/v1",
  sourceOwnedRoster: EXPECTED_MONTHLY_ROSTER,
  exactKeys: EXPECTED_MONTHLY_PARENT_KEYS,
  orderedChildExactKeys: EXPECTED_MONTHLY_CHILD_KEYS,
  rules: EXPECTED_MONTHLY_RULES,
});
const EXPECTED_ROOT = JSON.parse(readFileSync(DEFINITION_URL, "utf8"));

// identify plain records only
function isRecord(value) {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

// encode one unambiguous canonical JSON value
function canonicalJson(value, seen = new Set()) {
  // encode JSON scalars without coercion
  if (value === null || typeof value === "string" || typeof value === "boolean") {
    return JSON.stringify(value);
  }
  // retain finite fractions and safe nonnegative-zero integers
  if (typeof value === "number") {
    if (!Number.isFinite(value) || Object.is(value, -0) ||
      (Number.isInteger(value) && !Number.isSafeInteger(value))) {
      throw new TypeError("workload definition contains an unsafe number");
    }
    return JSON.stringify(value);
  }
  // reject non-JSON or cyclic values
  if (typeof value !== "object" || seen.has(value)) {
    throw new TypeError("workload definition contains a non-JSON value");
  }
  seen.add(value);
  try {
    // encode dense arrays without custom properties
    if (Array.isArray(value)) {
      if (Object.keys(value).length !== value.length || Object.getOwnPropertySymbols(value).length !== 0) {
        throw new TypeError("workload definition contains an ambiguous array");
      }
      return `[${value.map(
        // encode each ordered array member
        (item) => canonicalJson(item, seen),
      ).join(",")}]`;
    }
    const prototype = Object.getPrototypeOf(value);

    // retain plain data records only
    if ((prototype !== Object.prototype && prototype !== null) ||
      Object.getOwnPropertySymbols(value).length !== 0) {
      throw new TypeError("workload definition contains an ambiguous record");
    }
    const descriptors = Object.getOwnPropertyDescriptors(value);
    const keys = Object.keys(value).sort();

    // reject hidden or accessor-backed fields
    if (Reflect.ownKeys(descriptors).length !== keys.length || keys.some(
      // inspect one complete data descriptor
      (key) => descriptors[key]?.enumerable !== true || !("value" in descriptors[key]),
    )) {
      throw new TypeError("workload definition contains hidden structure");
    }
    return `{${keys.map(
      // encode each sorted record member
      (key) => `${JSON.stringify(key)}:${canonicalJson(value[key], seen)}`,
    ).join(",")}}`;
  } finally {
    seen.delete(value);
  }
}

// hash one canonical definition
function definitionSha256(definition) {
  return createHash("sha256").update(canonicalJson(definition)).digest("hex");
}

// append one deterministic diagnostic
function addDiagnostic(diagnostics, code, path, message) {
  diagnostics.push({ code, message, path });
}

// compare one exact closed key set
function hasExactKeys(value, expected) {
  return isRecord(value) && canonicalJson(Object.keys(value).sort()) === canonicalJson([...expected].sort());
}

// validate the sole closed monthly slot contract
function validateMonthlySlotContracts(definition, diagnostics) {
  const contracts = definition?.bindingSlotContracts;

  // require one literal source-owned registry entry
  if (!hasExactKeys(contracts, [EXPECTED_MONTHLY_SLOT])) {
    addDiagnostic(diagnostics, "workload.monthly-contract", "$.bindingSlotContracts", "monthly slot contract registry is invalid");
    return;
  }
  const contract = contracts[EXPECTED_MONTHLY_SLOT];

  // require every approved roster key and rule verbatim
  if (!hasExactKeys(contract, EXPECTED_MONTHLY_CONTRACT_KEYS) ||
    canonicalJson(contract) !== canonicalJson(EXPECTED_MONTHLY_SLOT_CONTRACT)) {
    addDiagnostic(diagnostics, "workload.monthly-contract", `$.bindingSlotContracts.${EXPECTED_MONTHLY_SLOT}`, "monthly slot contract is invalid");
  }
}

// validate exact-first source-owned binding types
function validateBindingSlotTypes(definition, operations, diagnostics) {
  const bindingSlotTypes = definition?.bindingSlotTypes;

  // reject malformed type registries before slot traversal
  if (!isRecord(bindingSlotTypes)) {
    addDiagnostic(diagnostics, "workload.binding-type", "$.bindingSlotTypes", "binding slot type registry is invalid");
    return;
  }
  const typeKeys = Object.keys(bindingSlotTypes);

  // inspect every operation without accepting unused caller slots
  for (let operationIndex = 0; operationIndex < operations.length; operationIndex += 1) {
    const operation = operations[operationIndex];
    const slots = Array.isArray(operation?.bindingSlots) ? operation.bindingSlots : [];

    // require one effective descriptor for each used slot
    for (let slotIndex = 0; slotIndex < slots.length; slotIndex += 1) {
      const slot = slots[slotIndex];
      // prefer one exact descriptor over wildcard matches
      const matches = typeof slot === "string" && Object.hasOwn(bindingSlotTypes, slot)
        ? [slot]
        : typeKeys.filter(
            // retain only matching trailing-wildcard descriptors
            (key) => typeof slot === "string" && key.endsWith(".*") &&
              slot.startsWith(key.slice(0, -1)),
          );

      // reject missing ambiguous or malformed effective descriptors
      if (matches.length !== 1 || typeof bindingSlotTypes[matches[0]] !== "string") {
        addDiagnostic(
          diagnostics,
          "workload.binding-type",
          `$.operations[${String(operationIndex)}].bindingSlots[${String(slotIndex)}]`,
          "binding slot does not have one effective type",
        );
      }
    }
  }
}

// validate the fixed source-owned catalog relationships
function validateDefinitionRelationships(definition, diagnostics) {
  const operations = Array.isArray(definition?.operations) ? definition.operations : [];
  const groups = Array.isArray(definition?.groups) ? definition.groups : [];
  const operationIds = new Set();
  const groupIds = new Set();
  const operationById = new Map();
  const groupById = new Map();

  // index every unique operation identity
  for (const operation of operations) {
    if (!isRecord(operation) || typeof operation.id !== "string" || operationIds.has(operation.id)) {
      addDiagnostic(diagnostics, "workload.catalog", "$.operations", "operation identities must be unique records");
      continue;
    }
    operationIds.add(operation.id);
    operationById.set(operation.id, operation);
  }
  // validate the exact ordered dependency chain and operation membership
  for (let index = 0; index < groups.length; index += 1) {
    const group = groups[index];
    const expectedDependency = index === 0 ? null : groups[index - 1]?.id;

    if (!isRecord(group) || typeof group.id !== "string" || groupIds.has(group.id) ||
      group.globalOrder !== index + 1 || group.dependsOnGroupId !== expectedDependency ||
      !Array.isArray(group.operationIds) || group.operationIds.length === 0) {
      addDiagnostic(diagnostics, "workload.group", `$.groups[${String(index)}]`, "group order or dependency is invalid");
      continue;
    }
    groupIds.add(group.id);
    groupById.set(group.id, group);
    // require each operation to belong to this group exactly once
    for (let operationIndex = 0; operationIndex < group.operationIds.length; operationIndex += 1) {
      const operationId = group.operationIds[operationIndex];
      const operation = operationById.get(operationId);

      if (operation?.atomicGroupId !== group.id || operation?.orderWithinGroup !== operationIndex + 1) {
        addDiagnostic(diagnostics, "workload.group", `$.groups[${String(index)}].operationIds`, "group operation order is invalid");
      }
    }
  }
  // require every operation to reference one declared group
  for (const operation of operations) {
    if (!groupIds.has(operation?.atomicGroupId)) {
      addDiagnostic(diagnostics, "workload.catalog", `$.operations.${String(operation?.id)}`, "operation group is missing");
    }
    if (!Object.hasOwn(definition?.timeoutPolicies ?? {}, operation?.timeoutPolicy)) {
      addDiagnostic(diagnostics, "workload.timeout", `$.operations.${String(operation?.id)}`, "operation timeout policy is missing");
    }
  }
  const typedSlots = new Set(operations.flatMap(
    // retain every source-declared typed binding slot
    (operation) => Array.isArray(operation?.bindingSlots) ? operation.bindingSlots : [],
  ));
  validateBindingSlotTypes(definition, operations, diagnostics);
  // retain only one plain producer registry
  const transitionProducers = isRecord(definition?.transitionProducers)
    ? definition.transitionProducers
    : {};
  const transitions = Object.entries(transitionProducers).sort(
    // keep transition validation byte-stable
    ([left], [right]) => left.localeCompare(right),
  );

  // require one declared producer for every used transition slot
  for (const bindingSlot of [...typedSlots].filter(
    // retain only source-owned transition consumers
    (slot) => typeof slot === "string" && slot.startsWith("transition."),
  )) {
    // diagnose missing transition map entries explicitly
    if (!Object.hasOwn(transitionProducers, bindingSlot)) {
      addDiagnostic(diagnostics, "workload.transition", `$.transitionProducers.${bindingSlot}`, "transition producer is missing");
    }
  }

  // require each transition producer before every consumer
  for (const [bindingSlot, producerOperationId] of transitions) {
    const producer = operationById.get(producerOperationId);
    const consumers = operations.filter(
      // select every declared transition consumer
      (operation) => Array.isArray(operation?.bindingSlots) && operation.bindingSlots.includes(bindingSlot),
    );
    const producerGroup = groupById.get(producer?.atomicGroupId);
    const invalidConsumer = consumers.some(
      // compare complete stable execution ranks and membership
      (consumer) => {
        const consumerGroup = groupById.get(consumer?.atomicGroupId);
        const occurrences = consumer.bindingSlots.filter(
          // count this exact transition membership
          (slot) => slot === bindingSlot,
        ).length;
        const producerBeforeConsumer = producerGroup !== undefined && consumerGroup !== undefined &&
          (producerGroup.globalOrder < consumerGroup.globalOrder ||
            (producerGroup.globalOrder === consumerGroup.globalOrder &&
              producer.orderWithinGroup < consumer.orderWithinGroup));
        return occurrences !== 1 || !producerBeforeConsumer;
      },
    );

    // reject unknown producers consumers groups and nonforward edges
    if (typeof bindingSlot !== "string" || typeof producerOperationId !== "string" ||
      producer === undefined || producerGroup === undefined || consumers.length === 0 || invalidConsumer) {
      addDiagnostic(diagnostics, "workload.transition", `$.transitionProducers.${bindingSlot}`, "transition topology is invalid");
    }
  }

  // bind all reviewed cardinalities to their exact source rows
  if (operations.length !== 107 || groups.length !== 59 ||
    typedSlots.size !== EXPECTED_COUNTS.typedSlots ||
    transitions.length !== EXPECTED_COUNTS.transitions) {
    addDiagnostic(diagnostics, "workload.count", "$.counts", "definition cardinalities differ from the reviewed workload");
  }
}

// finish one total validator result deterministically
function finishDefinitionValidation(digest, diagnostics) {
  diagnostics.sort(
    // keep result order deterministic
    (left, right) => canonicalJson(left).localeCompare(canonicalJson(right)),
  );
  return deepFreeze({ canonicalEligible: false, definitionSha256: digest, diagnostics, ok: diagnostics.length === 0 });
}

// validate one candidate against the exact checked definition
export function validateBackendPerformanceWorkloadDefinition(definition) {
  const diagnostics = [];
  let digest = null;

  // reject absent roots without assigning them a definition digest
  if (definition === null || definition === undefined) {
    addDiagnostic(diagnostics, "workload.schema", "$", "workload definition root is invalid");
    return finishDefinitionValidation(null, diagnostics);
  }

  try {
    digest = definitionSha256(definition);
  } catch (error) {
    addDiagnostic(diagnostics, "workload.schema", "$", error instanceof Error ? error.message : "definition is not canonical JSON");
    return finishDefinitionValidation(null, diagnostics);
  }

  // validate the closed root and fixed contract before catalog traversal
  if (!hasExactKeys(definition, DEFINITION_KEYS)) {
    addDiagnostic(diagnostics, "workload.schema", "$", "workload definition root is invalid");
    return finishDefinitionValidation(digest, diagnostics);
  }
  try {
    // require the fixed contract identity
    if (definition.contractVersion !== BACKEND_PERFORMANCE_WORKLOAD_VERSION) {
      addDiagnostic(diagnostics, "workload.schema", "$.contractVersion", "workload definition contract is invalid");
    }
    // require the complete reviewed count declaration
    if (!isRecord(definition.counts) || canonicalJson(definition.counts) !== canonicalJson(EXPECTED_COUNTS)) {
      addDiagnostic(diagnostics, "workload.count", "$.counts", "workload counts differ from the reviewed definition");
    }
    // require five fixed ordered planning slots and concurrency one
    if (canonicalJson(definition.runPolicy?.orderedRunSlots) !== canonicalJson(EXPECTED_RUN_SLOTS) ||
      definition.runPolicy?.nonOverlap !== true) {
      addDiagnostic(diagnostics, "workload.schedule", "$.runPolicy", "run policy differs from the reviewed definition");
    }
    validateMonthlySlotContracts(definition, diagnostics);
    validateDefinitionRelationships(definition, diagnostics);

    // reject all self-consistent drift from the checked source-owned definition
    if (digest !== DEFINITION_SHA256 || canonicalJson(definition) !== canonicalJson(EXPECTED_ROOT)) {
      addDiagnostic(diagnostics, "workload.definition", "$", "workload definition differs from the checked source bytes");
    }
  } catch (error) {
    // contain every malformed nested shape as structured diagnostics
    addDiagnostic(diagnostics, "workload.schema", "$", error instanceof Error ? error.message : "workload definition is invalid");
  }
  return finishDefinitionValidation(digest, diagnostics);
}

// create one stable selector error
function selectorError(message) {
  const error = new Error(message);
  error.code = "workload.selector";
  return error;
}

// require one canonical selector source value
function requireCanonicalSelectorInput(input) {
  try {
    return canonicalJson(input);
  } catch {
    throw selectorError("selector input is not canonical JSON");
  }
}

// freeze one pure selector result recursively
function freezeSelectorResult(value) {
  // freeze children before their containing record
  if (value !== null && typeof value === "object" && !Object.isFrozen(value)) {
    for (const child of Object.values(value)) {
      freezeSelectorResult(child);
    }
    Object.freeze(value);
  }
  return value;
}

// return one hashed non-evidence selector projection
function selectorResult(selectorId, input, selection) {
  return freezeSelectorResult({
    canonicalEligible: false,
    inputSha256: createHash("sha256").update(canonicalJson(input)).digest("hex"),
    outputSha256: createHash("sha256").update(canonicalJson(selection)).digest("hex"),
    selection,
    selectorId,
  });
}

// classify every fixed typed slot without accepting runtime slot input
export function classifyBackendPerformanceBindingSlots(...argumentsList) {
  // reject caller-selected definition or slot values
  if (argumentsList.length !== 0) {
    throw selectorError("binding slot classification accepts no runtime input");
  }
  const slots = [...new Set(EXPECTED_ROOT.operations.flatMap(
    // retain every exact source-owned binding slot
    (operation) => operation.bindingSlots,
  ))].sort();
  const phaseProvenance = slots.filter(
    // isolate phase-specific application provenance
    (slot) => slot.startsWith("provenance."),
  );
  const secretHandles = slots.filter(
    // isolate protected runtime secret handles
    (slot) => slot.startsWith("secret."),
  );
  const transitions = slots.filter(
    // isolate same-run predecessor transitions
    (slot) => slot.startsWith("transition."),
  );
  const classified = new Set([...phaseProvenance, ...secretHandles, ...transitions]);
  const immutableShared = slots.filter(
    // retain every remaining source-owned shared slot
    (slot) => !classified.has(slot),
  );

  // fail closed if the reviewed classification changes
  if (slots.length !== EXPECTED_CLASSIFICATION_COUNTS.total ||
    immutableShared.length !== EXPECTED_CLASSIFICATION_COUNTS.immutableShared ||
    phaseProvenance.length !== EXPECTED_CLASSIFICATION_COUNTS.phaseProvenance ||
    secretHandles.length !== EXPECTED_CLASSIFICATION_COUNTS.secretHandles ||
    transitions.length !== EXPECTED_CLASSIFICATION_COUNTS.transitions) {
    throw selectorError("binding slot classification differs from the reviewed definition");
  }
  return freezeSelectorResult({
    canonicalEligible: false,
    contractVersion: "weather-backend-performance-binding-slot-classification/v2",
    counts: { ...EXPECTED_CLASSIFICATION_COUNTS },
    definition: {
      contractVersion: BACKEND_PERFORMANCE_WORKLOAD_VERSION,
      sha256: DEFINITION_SHA256,
    },
    immutableShared,
    phaseProvenance,
    secretHandles,
    transitions,
  });
}

// require one exact plain record key set
function requireSelectorKeys(value, expected, label) {
  // reject nonrecords before key inspection
  if (!isRecord(value) || canonicalJson(Object.keys(value).sort()) !== canonicalJson([...expected].sort())) {
    throw selectorError(`${label} has invalid fields`);
  }
}

// validate one source-format coordinate pair
function validateSelectorCoordinate(value, label) {
  requireSelectorKeys(value, new Set(["latitude", "longitude"]), label);

  // require finite source coordinate ranges
  if (typeof value.latitude !== "number" || !Number.isFinite(value.latitude) ||
    value.latitude < -85 || value.latitude > 85 || Object.is(value.latitude, -0) ||
    typeof value.longitude !== "number" || !Number.isFinite(value.longitude) ||
    value.longitude < -180 || value.longitude > 180 || Object.is(value.longitude, -0)) {
    throw selectorError(`${label} coordinates are invalid`);
  }
  return { latitude: value.latitude, longitude: value.longitude };
}

// validate one exact bound site-center projection
function validateSelectorSite(value) {
  requireSelectorKeys(value, new Set(["id", "latitude", "longitude", "version"]), "site");

  // require the fixed site and projection version
  if (value.id !== "ballydidean" || value.version !== 1) {
    throw selectorError("site identity is invalid");
  }
  const coordinate = validateSelectorCoordinate(
    { latitude: value.latitude, longitude: value.longitude },
    "site",
  );
  return { id: value.id, ...coordinate, version: value.version };
}

// derive the fixed settings toggle from persisted source format
export function selectBackendPerformanceAdminSettings(input) {
  requireCanonicalSelectorInput(input);
  requireSelectorKeys(input, new Set(["rain", "temperature", "version", "wind"]), "admin settings");

  // require the current independent-switch persistence contract
  if (input.version !== 1 || typeof input.temperature !== "boolean" ||
    typeof input.wind !== "boolean" || typeof input.rain !== "boolean") {
    throw selectorError("admin settings are invalid");
  }
  const value = {
    rain: !input.rain,
    temperature: input.temperature,
    version: 1,
    wind: input.wind,
  };
  const selection = { canonicalJson: canonicalJson(value), value };
  return selectorResult("admin-settings", input, selection);
}

// validate one complete persisted layout entry
function validateLayoutEntry(value, index) {
  const required = ["displayName", "latitude", "longitude", "sensorKey", "updatedAt"];
  const keys = Object.keys(value ?? {}).sort();
  const expected = value?.icon === undefined ? required : [...required, "icon"];

  // reject missing extra or nonrecord entry fields
  if (!isRecord(value) || canonicalJson(keys) !== canonicalJson(expected.sort())) {
    throw selectorError(`layout sensor ${String(index)} has invalid fields`);
  }
  const coordinate = validateSelectorCoordinate(
    { latitude: value.latitude, longitude: value.longitude },
    `layout sensor ${String(index)}`,
  );
  const displayName = typeof value.displayName === "string" ? value.displayName.trim() : "";

  // require one bounded persisted display name
  if (displayName.length < 1 || displayName.length > 80 || /[\u0000-\u001f\u007f]/u.test(value.displayName)) {
    throw selectorError(`layout sensor ${String(index)} displayName is invalid`);
  }
  // require one stable bounded sensor key
  if (typeof value.sensorKey !== "string" || value.sensorKey.length > 64 ||
    !/^[a-z0-9]+(?:-[a-z0-9]+)*$/u.test(value.sensorKey)) {
    throw selectorError(`layout sensor ${String(index)} sensorKey is invalid`);
  }
  let updatedAt;

  try {
    updatedAt = new Date(value.updatedAt).toISOString();
  } catch {
    throw selectorError(`layout sensor ${String(index)} updatedAt is invalid`);
  }
  // require one canonical persisted update instant
  if (typeof value.updatedAt !== "string" || updatedAt !== value.updatedAt) {
    throw selectorError(`layout sensor ${String(index)} updatedAt is invalid`);
  }
  const icon = value.icon ?? null;

  // retain only source-supported icon categories or a legacy null
  if (icon !== null && !["air-quality", "rain", "temperature", "wind"].includes(icon)) {
    throw selectorError(`layout sensor ${String(index)} icon is invalid`);
  }
  return { displayName, icon, ...coordinate, sensorKey: value.sensorKey, updatedAt };
}

// derive one exact disposable layout write projection
export function selectBackendPerformanceAdminLayout(input) {
  requireCanonicalSelectorInput(input);
  requireSelectorKeys(input, new Set(["layout", "site"]), "admin layout selector");
  const site = validateSelectorSite(input.site);
  requireSelectorKeys(input.layout, new Set(["sensors", "version"]), "admin layout");

  // require the current complete layout persistence contract
  if (input.layout.version !== 1 || !Array.isArray(input.layout.sensors)) {
    throw selectorError("admin layout is invalid");
  }
  const sensors = input.layout.sensors.map(
    // validate every entry without source truncation
    (sensor, index) => validateLayoutEntry(sensor, index),
  );
  const sensorKeys = new Set(sensors.map(
    // retain each stable key for duplicate detection
    (sensor) => sensor.sensorKey,
  ));

  // reject duplicate persisted sensor identities
  if (sensorKeys.size !== sensors.length) {
    throw selectorError("admin layout contains duplicate sensor keys");
  }
  const eligible = sensors.filter(
    // apply the inclusive reviewed center window
    (sensor) => Math.abs(sensor.latitude - site.latitude) <= 0.025 &&
      Math.abs(sensor.longitude - site.longitude) <= 0.04,
  ).sort(
    // select by byte-stable lowercase sensor key order
    (left, right) => left.sensorKey < right.sensorKey ? -1 : left.sensorKey > right.sensorKey ? 1 : 0,
  );

  // require one source-backed eligible entry
  if (eligible.length === 0) {
    throw selectorError("admin layout has no eligible sensor");
  }
  const selected = eligible[0];
  const selection = {
    sensorKey: selected.sensorKey,
    writePayload: {
      displayName: selected.displayName === "G002 benchmark" ? "G002 benchmark 2" : "G002 benchmark",
      icon: selected.icon ?? "temperature",
      latitude: selected.latitude,
      longitude: selected.longitude,
    },
  };
  return selectorResult("admin-layout", input, selection);
}

// format one fixed provider UTC frame identifier
function formatMapValidTime(frameMs) {
  const instant = new Date(frameMs);
  return [
    String(instant.getUTCFullYear()).padStart(4, "0"),
    String(instant.getUTCMonth() + 1).padStart(2, "0"),
    String(instant.getUTCDate()).padStart(2, "0"),
    String(instant.getUTCHours()).padStart(2, "0"),
    String(instant.getUTCMinutes()).padStart(2, "0"),
    String(instant.getUTCSeconds()).padStart(2, "0"),
  ].join("");
}

// derive the latest fully closed fixed map frame
export function selectBackendPerformanceMapFrame(input) {
  requireCanonicalSelectorInput(input);
  requireSelectorKeys(input, new Set(["bindingCreatedAt", "site"]), "map frame selector");
  const site = validateSelectorSite(input.site);
  let bindingCreatedAt;

  try {
    bindingCreatedAt = new Date(input.bindingCreatedAt);
  } catch {
    throw selectorError("bindingCreatedAt is invalid");
  }
  // require one canonical writer-supplied instant
  if (typeof input.bindingCreatedAt !== "string" ||
    !Number.isFinite(bindingCreatedAt.getTime()) ||
    bindingCreatedAt.toISOString() !== input.bindingCreatedAt) {
    throw selectorError("bindingCreatedAt is invalid");
  }
  const frameMs = Math.floor((bindingCreatedAt.getTime() - 1) / 600_000) * 600_000;
  const frameYear = new Date(frameMs).getUTCFullYear();

  // reject years the deployed parser would reinterpret through Date.UTC
  if (frameYear < 100) {
    throw selectorError("map frame year is unsupported");
  }
  const validTime = formatMapValidTime(frameMs);
  const latitude = site.latitude.toFixed(6);
  const longitude = site.longitude.toFixed(6);

  // reject dates outside the fixed fourteen-digit provider format
  if (!/^[0-9]{14}$/u.test(validTime)) {
    throw selectorError("map frame instant is out of range");
  }
  const selection = {
    height: 168,
    kind: "frame",
    latitude,
    layer: "radar",
    longitude,
    phase: "history",
    target: `/maps/xweather/history/radar/${validTime}/10/256x168/${latitude},${longitude}.png`,
    validTime,
    width: 256,
    zoom: 10,
  };
  return selectorResult("map-frame", input, selection);
}

// resolve one operation deadline from its fixed policy
function operationDeadlineMs(definition, operation) {
  const policy = definition.timeoutPolicies[operation.timeoutPolicy];
  const deadline = policy?.attemptDeadlineMs ?? policy?.operationDeadlineMs;

  // refuse incomplete policy definitions
  if (!Number.isSafeInteger(deadline) || deadline <= 0) {
    throw new Error(`operation ${operation.id} has no fixed deadline`);
  }
  return deadline;
}

// append one deterministic planned attempt
function appendAttempt(target, operation, group, runSlotId, stratum, ordinal, definition) {
  const attemptId = `planned:${runSlotId}:${String(ordinal).padStart(4, "0")}:${operation.id}:${stratum}`;
  target.push({
    attemptId,
    deadlineMs: operationDeadlineMs(definition, operation),
    groupId: group.id,
    operationId: operation.id,
    ordinal,
    runSlotId,
    stratum,
    timeoutPolicy: operation.timeoutPolicy,
  });
}

// expand one fixed group without accepting runtime selectors
function expandGroupAttempts(definition, group, runSlotId, startingOrdinal) {
  const operationById = new Map(definition.operations.map(
    // index the exact fixed operation table
    (operation) => [operation.id, operation],
  ));
  const operations = group.operationIds.map(
    // preserve group-declared operation order
    (operationId) => operationById.get(operationId),
  );
  const attempts = [];
  let ordinal = startingOrdinal;
  const warm = operations.some(
    // identify the single fixed HTTP matrix group
    (operation) => Number.isSafeInteger(operation?.count?.warmAttemptsPerRun),
  );

  // expand the HTTP matrix by complete strata
  if (warm) {
    for (const stratum of ["warm", "application-cache-cold", "process-pool-cold", "db-os-cold"]) {
      for (const operation of operations) {
        const repetitions = stratum === "warm"
          ? operation.count.warmAttemptsPerRun
          : operation.count.coldAttemptsPerRun?.[stratum] ?? 0;

        // retain every reviewed repetition in source order
        for (let repetition = 0; repetition < repetitions; repetition += 1) {
          appendAttempt(attempts, operation, group, runSlotId, stratum, ordinal, definition);
          ordinal += 1;
        }
      }
    }
  } else {
    // expand every non-HTTP operation exactly once per slot
    for (const operation of operations) {
      appendAttempt(attempts, operation, group, runSlotId, "once-per-run", ordinal, definition);
      ordinal += 1;
    }
  }
  return { attempts, nextOrdinal: ordinal };
}

// recursively freeze one detached plan value
function deepFreeze(value) {
  // freeze every child before its parent
  if (value !== null && typeof value === "object" && !Object.isFrozen(value)) {
    for (const child of Object.values(value)) {
      deepFreeze(child);
    }
    Object.freeze(value);
  }
  return value;
}

// build the deterministic non-evidence schedule from the fixed checked file
export function buildBackendPerformanceWorkloadPlan(...argumentsList) {
  // reject runtime definition or operation overrides
  if (argumentsList.length !== 0) {
    throw new TypeError("workload plan accepts no runtime input");
  }
  const validation = validateBackendPerformanceWorkloadDefinition(EXPECTED_ROOT);

  // fail closed if the checked file no longer matches its reviewed digest
  if (!validation.ok) {
    const error = new Error("checked workload definition is invalid");
    error.code = "workload.definition";
    error.diagnostics = validation.diagnostics;
    throw error;
  }
  const runSlots = EXPECTED_RUN_SLOTS.map(
    // expand one complete ordered planning slot
    (runSlotId, slotIndex) => {
      let ordinal = 1;
      const groups = EXPECTED_ROOT.groups.map(
        // expand one globally ordered atomic group
        (group) => {
          const expanded = expandGroupAttempts(EXPECTED_ROOT, group, runSlotId, ordinal);
          ordinal = expanded.nextOrdinal;
          return {
            attempts: expanded.attempts,
            dependsOnGroupId: group.dependsOnGroupId,
            finalRestore: {
              contractVersion: "weather-backend-group-restore/v1",
              id: group.finalRestoreOperation.id,
              requiredBeforeNextGroup: true,
              sequence: "finally-after-attempts",
            },
            id: group.id,
            order: group.globalOrder,
            preState: {
              contractVersion: "weather-backend-group-prestate/v1",
              id: `control.prestate::${group.id}`,
              requiredBeforeAttempts: true,
              sequence: "before-attempts",
            },
          };
        },
      );
      return { groups, id: runSlotId, order: slotIndex + 1 };
    },
  );
  const transitions = Object.entries(EXPECTED_ROOT.transitionProducers).sort(
    // retain transition slot order independently from object insertion order
    ([left], [right]) => left.localeCompare(right),
  ).map(
    // expose only fixed producer topology
    ([bindingSlot, producerOperationId]) => ({ bindingSlot, producerOperationId }),
  );
  return deepFreeze({
    canonicalEligible: false,
    contractVersion: BACKEND_PERFORMANCE_SCHEDULE_VERSION,
    definition: {
      contractVersion: BACKEND_PERFORMANCE_WORKLOAD_VERSION,
      sha256: DEFINITION_SHA256,
    },
    runSlots,
    totals: {
      applicationCacheColdAttempts: 15,
      dbOsColdAttempts: 140,
      groupInstances: 295,
      oncePerRunAttempts: 395,
      plannedAttempts: 6290,
      preStateReceipts: 295,
      processPoolColdAttempts: 140,
      restoreReceipts: 295,
      runSlots: 5,
      warmAttempts: 5600,
    },
    transitions,
  });
}
