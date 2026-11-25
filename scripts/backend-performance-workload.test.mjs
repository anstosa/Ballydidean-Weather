import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFile } from "node:fs/promises";
import test from "node:test";

import {
  BACKEND_PERFORMANCE_SCHEDULE_VERSION,
  BACKEND_PERFORMANCE_WORKLOAD_VERSION,
  buildBackendPerformanceWorkloadPlan,
  classifyBackendPerformanceBindingSlots,
  selectBackendPerformanceAdminLayout,
  selectBackendPerformanceAdminSettings,
  selectBackendPerformanceMapFrame,
  validateBackendPerformanceWorkloadDefinition,
} from "./backend-performance-workload.mjs";

const DEFINITION_PATH = new URL("../config/backend-performance-workload-v2.json", import.meta.url);
const RUN_SLOT_IDS = [
  "run-slot-01",
  "run-slot-02",
  "run-slot-03",
  "run-slot-04",
  "run-slot-05",
];
const DEFINITION_ROOT_KEYS = [
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
];
const EXPECTED_DEFINITION_SHA256 =
  "5722eeecd2734a1cfe4e7887cfc887dc6ac052cb397e08f8389fd6b284c5c364";
const PREVIOUS_DEFINITION_SHA256 =
  "cd54ccc05498ba2a3618f7a225d5f07faaeb7b34f91da661159a620ab7deecce";
const REFERENCE_CONSUMER_IDS = [
  "page.forecast",
  "api.sites",
  "api.health",
  "api.current",
  "api.daily-precipitation",
  "api.history-25",
  "api.forecast-default",
  "api.forecast-1d",
  "api.forecast-5d",
  "api.forecast-10d",
  "api.forecast-overnight",
  "api.rain-collection",
  "api.trends",
  "api.tides",
  "edge.widget-forecast-v1",
  "edge.widget-forecast-v2",
  "edge.widget-forecast-v3",
  "edge.forecast-observations",
  "edge.solar-cloud-evidence",
  "api.private-forecast-observation-input",
  "admin.live.read-current",
  "map.tile.cache-miss",
  "map.tile.cache-hit",
  "provider.open-meteo-current.success",
  "provider.open-meteo-current.upstream-error",
  "provider.open-meteo-current.slow-timeout",
  "provider.open-meteo-current.interruption-resume",
  "provider.open-meteo-forecast.success",
  "provider.open-meteo-forecast.upstream-error",
  "provider.open-meteo-forecast.slow-timeout",
  "provider.open-meteo-forecast.interruption-resume",
  "provider.ecowitt.success",
  "provider.ecowitt.upstream-error",
  "provider.ecowitt.slow-timeout",
  "provider.ecowitt.interruption-resume",
  "provider.tempest.success",
  "provider.tempest.upstream-error",
  "provider.tempest.slow-timeout",
  "provider.tempest.interruption-resume",
  "provider.public-station.success",
  "provider.public-station.upstream-error",
  "provider.public-station.slow-timeout",
  "provider.public-station.interruption-resume",
  "provider.noaa-tide.success",
  "provider.noaa-tide.upstream-error",
  "provider.noaa-tide.slow-timeout",
  "provider.noaa-tide.interruption-resume",
  "inference.temperature.load-serve",
  "inference.wind.load-serve",
  "inference.rain.load-serve",
  "inference.rain.publish",
  "oneshot.maintenance-monthly",
];
const MONTHLY_SLOT = "oneshot.maintenance.monthlyInputs";
const MONTHLY_CONTRACT = {
  contractVersion: "weather-benchmark-monthly-inputs/v1",
  sourceOwnedRoster: {
    source: "scripts/research/adjustment_maintenance_controller.mjs:163 FAMILIES and :1260-1271 monthly due keys",
    exactOrder: ["temperature", "wind", "rain"],
    cardinality: 3,
  },
  exactKeys: [
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
  ],
  orderedChildExactKeys: [
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
  ],
  rules: [
    "children are exactly three and index-equal to familyOrder temperature/wind/rain; no implementation selects cardinality",
    "each child is exactly weather-benchmark-monthly-input-child/v1 with orderedChildExactKeys; no childInputSha256 or other per-child digest field exists",
    "expectedCompletedWorkCount is an integer greater than zero for every child and is derived before baseline/candidate execution from the admitted corpus",
    "familyInputAggregateRootSha256 is SHA-256 over its literal versioned domain plus canonical familyOrder, the three complete immutable child core bytes, preStateProjectionSha256 and expectedRestoredStateProjectionSha256; it excludes itself and every phase receipt",
    "monthlyInputsSha256 is an external digest of the complete immutable slot bytes including familyInputAggregateRootSha256; it is never embedded in that slot",
    "if the admitted corpus cannot satisfy all three exact children/outcomes/nonempty counts, binding fails before metrics; no child is skipped or weakened",
    "one existing operation, one validated-one-shot-execution completed work unit and one atomic-group restore remain; child counts and aggregate roots are evidence, not operations, attempts, work units, slots or transitions",
  ],
};

// read the checked source-owned workload definition
async function readDefinition() {
  return JSON.parse(await readFile(DEFINITION_PATH, "utf8"));
}

// build one exact admitted site projection
function admittedSite(overrides = {}) {
  return {
    id: "ballydidean",
    latitude: 0,
    longitude: 0,
    version: 1,
    ...overrides,
  };
}

// build one exact persisted layout entry
function layoutEntry(sensorKey, overrides = {}) {
  return {
    displayName: `Sensor ${sensorKey}`,
    icon: "temperature",
    latitude: 0,
    longitude: 0,
    sensorKey,
    updatedAt: "2026-11-13T20:00:00.000Z",
    ...overrides,
  };
}

// require one closed selector failure
function assertSelectorError(select, input) {
  assert.throws(
    // invoke one pure selector boundary
    () => select(input),
    { code: "workload.selector" },
  );
}

// hash one already canonical test value
function sha256Json(value) {
  return createHash("sha256").update(JSON.stringify(value)).digest("hex");
}

// encode parsed definition values with sorted object keys
function canonicalDefinitionJson(value) {
  // encode arrays in their source order
  if (Array.isArray(value)) {
    return `[${value.map(
      // encode one ordered member
      (item) => canonicalDefinitionJson(item),
    ).join(",")}]`;
  }
  // encode object members in lexical key order
  if (value !== null && typeof value === "object") {
    return `{${Object.keys(value).sort().map(
      // encode one sorted member
      (key) => `${JSON.stringify(key)}:${canonicalDefinitionJson(value[key])}`,
    ).join(",")}}`;
  }
  return JSON.stringify(value);
}

// hash one parsed definition independently from the product validator
function canonicalDefinitionSha256(definition) {
  return createHash("sha256")
    .update(canonicalDefinitionJson(definition))
    .digest("hex");
}

// find one exact operation without accepting a missing identity
function operation(definition, operationId) {
  const selected = definition.operations.find(
    // match one source-owned operation identity
    (candidate) => candidate.id === operationId,
  );
  assert.notEqual(selected, undefined, operationId);
  return selected;
}

// collect exact operation consumers for one binding slot
function consumers(definition, bindingSlot) {
  return definition.operations.filter(
    // retain operations declaring this slot
    (candidate) => candidate.bindingSlots.includes(bindingSlot),
  );
}

// require one deterministic validator diagnostic family
function assertDiagnostic(result, code, pathPrefix, label = "diagnostic") {
  assert.equal(result.ok, false);
  assert.equal(result.canonicalEligible, false);
  assert.ok(result.diagnostics.some(
    // match the expected failure family and location
    (diagnostic) => diagnostic.code === code && diagnostic.path.startsWith(pathPrefix),
  ), `${label}: ${JSON.stringify(result.diagnostics)}`);
}

// reverse only the approved reference-definition delta
function reverseReferenceDefinitionMigration(definition) {
  const reversed = structuredClone(definition);
  delete reversed.bindingSlotContracts;
  delete reversed.bindingSlotTypes["reference.*"];

  // remove only the two newly generalized slot declarations
  for (const candidate of reversed.operations) {
    candidate.bindingSlots = candidate.bindingSlots.filter(
      // retain every preexisting binding slot
      (slot) => slot !== "reference.benchmarkDataEpoch" &&
        slot !== "provenance.phaseSafetyEnvelope",
    );
  }
  const verifier = operation(reversed, "oneshot.performance-verify-scorecard");
  verifier.bindingSlots = verifier.bindingSlots.map(
    // restore the old prebound scorecard slot
    (slot) => slot === "transition.oneshot.performanceScorecardOutput"
      ? "oneshot.performance.scorecardInput"
      : slot,
  );
  verifier.nodeArgv[0].argv = verifier.nodeArgv[0].argv.map(
    // restore the old scorecard argv placeholder
    (argument) => argument === "${binding:transition.oneshot.performanceScorecardOutput}"
      ? "${binding:oneshot.performance.scorecardInput}"
      : argument,
  );
  delete reversed.transitionProducers["transition.oneshot.performanceScorecardOutput"];
  reversed.counts.transitions = 9;
  reversed.counts.typedSlots = 154;
  return reversed;
}

// resolve one slot against exact-first trailing-wildcard types
function matchingBindingTypes(slot, bindingSlotTypes) {
  // give one exact descriptor precedence over every wildcard
  if (Object.hasOwn(bindingSlotTypes, slot)) {
    return [slot];
  }
  return Object.keys(bindingSlotTypes).filter(
    // retain every matching trailing wildcard
    (key) => key.endsWith(".*") && slot.startsWith(key.slice(0, -1)),
  );
}

// select one operation deadline from its closed timeout policy
function operationDeadline(definition, operation) {
  const policy = definition.timeoutPolicies[operation.timeoutPolicy];
  return policy.attemptDeadlineMs ?? policy.operationDeadlineMs;
}

// expand one group into its exact deterministic attempt projection
function expectedGroupAttempts(definition, group) {
  const operationById = new Map(definition.operations.map(
    // index each closed operation once
    (operation) => [operation.id, operation],
  ));
  const operations = group.operationIds.map(
    // preserve the source-owned group order
    (operationId) => operationById.get(operationId),
  );
  const expected = [];

  // expand the public HTTP strata in the frozen order
  if (group.id === "group.http-public-matrix") {
    // retain every warm attempt for each ordered operation
    for (const operation of operations) {
      // retain the fixed forty warm ordinals
      for (let ordinal = 1; ordinal <= 40; ordinal += 1) {
        expected.push({ operation, stratum: "warm" });
      }
    }
    // retain only the three proven application-cache-cold routes
    for (const operation of operations) {
      if (operation.count.coldAttemptsPerRun["application-cache-cold"] === 1) {
        expected.push({ operation, stratum: "application-cache-cold" });
      }
    }
    // retain every process and pool cold attempt
    for (const operation of operations) {
      expected.push({ operation, stratum: "process-pool-cold" });
    }
    // retain every database and OS cold attempt
    for (const operation of operations) {
      expected.push({ operation, stratum: "db-os-cold" });
    }
    return expected;
  }

  // expand every nonmatrix operation exactly once per slot
  for (const operation of operations) {
    expected.push({ operation, stratum: "once-per-run" });
  }
  return expected;
}

// validate the complete checked manifest cardinalities
test("source-owned backend workload definition is exact closed and noncanonical", async () => {
  const definition = await readDefinition();
  assert.equal(BACKEND_PERFORMANCE_WORKLOAD_VERSION, "weather-backend-performance-workload/v2");
  assert.equal(BACKEND_PERFORMANCE_SCHEDULE_VERSION, "weather-backend-performance-planned-schedule/v2");
  assert.deepEqual(Object.keys(definition).sort(), DEFINITION_ROOT_KEYS);
  assert.equal(definition.contractVersion, BACKEND_PERFORMANCE_WORKLOAD_VERSION);
  assert.deepEqual(definition.runPolicy.orderedRunSlots, RUN_SLOT_IDS);
  assert.equal(definition.operations.length, 107);
  assert.equal(definition.groups.length, 59);
  assert.equal(new Set(definition.operations.map(
    // retain every unique operation identity
    (operation) => operation.id,
  )).size, 107);
  assert.equal(new Set(definition.groups.map(
    // retain every unique group identity
    (group) => group.id,
  )).size, 59);
  assert.equal(new Set(definition.operations.flatMap(
    // retain every referenced typed binding slot
    (operation) => operation.bindingSlots,
  )).size, 156);
  assert.equal(Object.keys(definition.transitionProducers).length, 10);
  assert.deepEqual(definition.counts, {
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
  assert.deepEqual(definition.operations.filter(
    // select only proven application-cache-cold operations
    (operation) => operation.count.coldAttemptsPerRun?.["application-cache-cold"] === 1,
  ).map(
    // retain their exact route identities
    (operation) => operation.id,
  ), [
    "edge.viewer-context",
    "edge.forecast-observations",
    "edge.solar-cloud-evidence",
  ]);
  const validation = validateBackendPerformanceWorkloadDefinition(definition);
  assert.equal(validation.ok, true, JSON.stringify(validation.diagnostics));
  assert.equal(validation.canonicalEligible, false);
  assert.equal(validation.definitionSha256, EXPECTED_DEFINITION_SHA256);
  assert.deepEqual(validation.diagnostics, []);
  assert.equal(Object.isFrozen(validation), true);
  assert.equal(Object.isFrozen(validation.diagnostics), true);
});

// bind the approved consumer rosters schema and scorecard edge exactly
test("reference definition migration is exact and reverses to the old canonical definition", async () => {
  const definition = await readDefinition();
  const operationIds = definition.operations.map(
    // retain the complete source-owned operation order
    (candidate) => candidate.id,
  );
  const referenceConsumers = consumers(
    definition,
    "reference.benchmarkDataEpoch",
  ).map(
    // retain one exact reference consumer identity
    (candidate) => candidate.id,
  );
  const phaseConsumers = consumers(
    definition,
    "provenance.phaseSafetyEnvelope",
  ).map(
    // retain one exact phase consumer identity
    (candidate) => candidate.id,
  );
  const scorecardConsumers = consumers(
    definition,
    "transition.oneshot.performanceScorecardOutput",
  ).map(
    // retain one exact transition consumer identity
    (candidate) => candidate.id,
  );

  assert.equal(canonicalDefinitionSha256(definition), EXPECTED_DEFINITION_SHA256);
  assert.equal(REFERENCE_CONSUMER_IDS.length, 52);
  assert.equal(new Set(REFERENCE_CONSUMER_IDS).size, 52);
  assert.deepEqual(referenceConsumers.toSorted(), REFERENCE_CONSUMER_IDS.toSorted());
  assert.equal(referenceConsumers.length, 52);
  assert.deepEqual(phaseConsumers, operationIds);
  assert.equal(phaseConsumers.length, 107);
  assert.deepEqual(scorecardConsumers, ["oneshot.performance-verify-scorecard"]);

  // reject duplicate binding membership in every operation roster
  for (const candidate of definition.operations) {
    assert.equal(
      new Set(candidate.bindingSlots).size,
      candidate.bindingSlots.length,
      candidate.id,
    );
  }

  assert.deepEqual(Object.keys(definition.bindingSlotContracts), [MONTHLY_SLOT]);
  assert.deepEqual(
    Object.keys(definition.bindingSlotContracts[MONTHLY_SLOT]).sort(),
    [
      "contractVersion",
      "exactKeys",
      "orderedChildExactKeys",
      "rules",
      "sourceOwnedRoster",
    ],
  );
  assert.deepEqual(definition.bindingSlotContracts[MONTHLY_SLOT], MONTHLY_CONTRACT);
  assert.equal(MONTHLY_CONTRACT.exactKeys.length, 10);
  assert.equal(MONTHLY_CONTRACT.orderedChildExactKeys.length, 11);
  assert.equal(MONTHLY_CONTRACT.rules.length, 7);
  assert.equal(MONTHLY_CONTRACT.exactKeys.includes("childInputSha256"), false);
  assert.equal(
    MONTHLY_CONTRACT.orderedChildExactKeys.includes("childInputSha256"),
    false,
  );
  assert.deepEqual(operation(definition, "oneshot.maintenance-monthly").bindingSlots, [
    "oneshot.maintenance.installedRunnerIdentity",
    "oneshot.maintenance.monthlyInputs",
    "reference.benchmarkDataEpoch",
    "provenance.phaseSafetyEnvelope",
  ]);

  const producer = operation(definition, "oneshot.performance-scorecard");
  const verifier = operation(definition, "oneshot.performance-verify-scorecard");
  const producerGroup = definition.groups.find(
    // locate the source-owned producer group
    (group) => group.id === producer.atomicGroupId,
  );
  const verifierGroup = definition.groups.find(
    // locate the source-owned consumer group
    (group) => group.id === verifier.atomicGroupId,
  );
  assert.equal(definition.transitionProducers[
    "transition.oneshot.performanceScorecardOutput"
  ], producer.id);
  assert.equal(producerGroup.globalOrder, 54);
  assert.equal(verifierGroup.globalOrder, 55);
  assert.deepEqual(verifier.bindingSlots, [
    "transition.oneshot.performanceScorecardOutput",
    "provenance.phaseSafetyEnvelope",
  ]);
  assert.deepEqual(verifier.nodeArgv[0].argv.filter(
    // retain only binding placeholders
    (argument) => argument.startsWith("${binding:"),
  ), ["${binding:transition.oneshot.performanceScorecardOutput}"]);
  assert.doesNotMatch(JSON.stringify(definition), /oneshot\.performance\.scorecardInput/u);

  const reversed = reverseReferenceDefinitionMigration(definition);
  const reverseValidation = validateBackendPerformanceWorkloadDefinition(reversed);
  assert.equal(reverseValidation.ok, false);
  assert.equal(reverseValidation.canonicalEligible, false);
  assert.equal(reverseValidation.definitionSha256, PREVIOUS_DEFINITION_SHA256);
  assert.equal(canonicalDefinitionSha256(reversed), PREVIOUS_DEFINITION_SHA256);
});

// reject every monthly registry weakening without trusting runtime planning files
test("monthly slot contract rejects unknown missing reordered weakened and malformed shapes", async () => {
  const definition = await readDefinition();
  const base = structuredClone(definition);
  // keep legacy RED candidates total before the registry exists
  base.bindingSlotContracts ??= {
    [MONTHLY_SLOT]: structuredClone(MONTHLY_CONTRACT),
  };
  const cases = [];

  const unknown = structuredClone(base);
  unknown.bindingSlotContracts = { "oneshot.maintenance.unknown": MONTHLY_CONTRACT };
  cases.push(["unknown slot", unknown]);

  const missing = structuredClone(base);
  missing.bindingSlotContracts = {};
  cases.push(["missing slot", missing]);

  const reordered = structuredClone(base);
  reordered.bindingSlotContracts[MONTHLY_SLOT].sourceOwnedRoster.exactOrder = [
    "wind",
    "temperature",
    "rain",
  ];
  cases.push(["reordered roster", reordered]);

  const missingRain = structuredClone(base);
  missingRain.bindingSlotContracts[MONTHLY_SLOT].sourceOwnedRoster.exactOrder = [
    "temperature",
    "wind",
  ];
  cases.push(["missing rain", missingRain]);

  const wrongCardinality = structuredClone(base);
  wrongCardinality.bindingSlotContracts[MONTHLY_SLOT].sourceOwnedRoster.cardinality = 2;
  cases.push(["wrong cardinality", wrongCardinality]);

  const extraKey = structuredClone(base);
  extraKey.bindingSlotContracts[MONTHLY_SLOT].extension = true;
  cases.push(["extra contract key", extraKey]);

  const childDigest = structuredClone(base);
  childDigest.bindingSlotContracts[MONTHLY_SLOT].orderedChildExactKeys.push(
    "childInputSha256",
  );
  cases.push(["embedded child digest", childDigest]);

  const zeroWork = structuredClone(base);
  zeroWork.bindingSlotContracts[MONTHLY_SLOT].rules[2] =
    "expectedCompletedWorkCount may be zero";
  cases.push(["zero work", zeroWork]);

  const embeddedMonthlyDigest = structuredClone(base);
  embeddedMonthlyDigest.bindingSlotContracts[MONTHLY_SLOT].rules[4] =
    "monthlyInputsSha256 is embedded in the slot";
  cases.push(["embedded monthly digest", embeddedMonthlyDigest]);

  const malformed = structuredClone(base);
  malformed.bindingSlotContracts = [];
  cases.push(["malformed registry", malformed]);

  // isolate every closed monthly-contract failure
  for (const [label, candidate] of cases) {
    const result = validateBackendPerformanceWorkloadDefinition(candidate);
    assertDiagnostic(
      result,
      "workload.monthly-contract",
      "$.bindingSlotContracts",
      label,
    );
  }

  let getterExecutions = 0;
  const accessor = structuredClone(base);
  Object.defineProperty(accessor.bindingSlotContracts, MONTHLY_SLOT, {
    enumerable: true,
    // fail if validation executes an untrusted monthly getter
    get() {
      getterExecutions += 1;
      throw new Error("monthly getter executed");
    },
  });
  const accessorResult = validateBackendPerformanceWorkloadDefinition(accessor);
  assert.equal(accessorResult.ok, false);
  assert.equal(accessorResult.canonicalEligible, false);
  assert.equal(accessorResult.definitionSha256, null);
  assert.equal(getterExecutions, 0);
});

// require exact-first and otherwise unambiguous slot typing
test("all used binding slots have one effective type and mutations diagnose coverage", async () => {
  const definition = await readDefinition();
  const slots = [...new Set(definition.operations.flatMap(
    // retain every source-owned used slot
    (candidate) => candidate.bindingSlots,
  ))].sort();
  assert.equal(slots.length, 156);

  // require one effective descriptor for every used slot
  for (const slot of slots) {
    assert.equal(
      matchingBindingTypes(slot, definition.bindingSlotTypes).length,
      1,
      slot,
    );
  }
  assert.equal(consumers(definition, "identity.measurementCommit").length, 0);

  const untyped = structuredClone(definition);
  untyped.operations[0].bindingSlots[0] = "unknown.untyped";
  assertDiagnostic(
    validateBackendPerformanceWorkloadDefinition(untyped),
    "workload.binding-type",
    "$.operations",
  );

  const ambiguous = structuredClone(definition);
  ambiguous.bindingSlotTypes["provider.open-meteo-current.*"] =
    "ambiguous nested wildcard";
  assertDiagnostic(
    validateBackendPerformanceWorkloadDefinition(ambiguous),
    "workload.binding-type",
    "$.operations",
  );

  const exactFirst = structuredClone(definition);
  exactFirst.bindingSlotTypes["provider.open-meteo-current.sourceIdentity"] =
    "exact descriptor wins";
  const exactFirstResult = validateBackendPerformanceWorkloadDefinition(exactFirst);
  assert.equal(exactFirstResult.ok, false);
  assert.equal(exactFirstResult.diagnostics.some(
    // forbid a false ambiguity when one exact type exists
    (diagnostic) => diagnostic.code === "workload.binding-type",
  ), false);
});

// reject globally invalid producer order and duplicate transition consumption
test("transition topology rejects self later and duplicate consumer edges", async () => {
  const definition = await readDefinition();
  const transition = "transition.oneshot.performanceScorecardOutput";
  const cases = [];

  const self = structuredClone(definition);
  self.transitionProducers[transition] = "oneshot.performance-verify-scorecard";
  cases.push(["self producer", self]);

  const later = structuredClone(definition);
  later.transitionProducers[transition] = "oneshot.migration";
  cases.push(["later producer", later]);

  const earlierConsumer = structuredClone(definition);
  operation(earlierConsumer, "page.home").bindingSlots.push(transition);
  cases.push(["earlier consumer", earlierConsumer]);

  const duplicateConsumer = structuredClone(definition);
  operation(
    duplicateConsumer,
    "oneshot.performance-verify-scorecard",
  ).bindingSlots.push(transition);
  cases.push(["duplicate consumer slot", duplicateConsumer]);

  // isolate every invalid global transition topology
  for (const [label, candidate] of cases) {
    assertDiagnostic(
      validateBackendPerformanceWorkloadDefinition(candidate),
      "workload.transition",
      "$.transitionProducers",
      label,
    );
  }
});

// keep validation total for malformed untrusted candidates
test("workload definition validator returns structured failures without executing accessors", async (testContext) => {
  const definition = await readDefinition();
  const cyclic = {};
  cyclic.self = cyclic;
  const malformedOperations = structuredClone(definition);
  malformedOperations.operations = [null, [], "operation", { id: 42 }];
  const malformedGroups = structuredClone(definition);
  malformedGroups.groups = [null, [], "group", { id: 42 }];
  const malformedBindingTypes = structuredClone(definition);
  malformedBindingTypes.bindingSlotTypes = null;
  const malformedContracts = structuredClone(definition);
  malformedContracts.bindingSlotContracts = null;
  const cases = [
    { candidate: undefined, digest: null, label: "undefined" },
    { candidate: null, digest: null, label: "null" },
    { candidate: {}, digest: "sha256", label: "empty record" },
    {
      candidate: { contractVersion: BACKEND_PERFORMANCE_WORKLOAD_VERSION },
      digest: "sha256",
      label: "partial root",
    },
    { candidate: [], digest: "sha256", label: "array root" },
    { candidate: 1n, digest: null, label: "non-JSON bigint" },
    { candidate: cyclic, digest: null, label: "cyclic record" },
    { candidate: malformedOperations, digest: "sha256", label: "malformed operations" },
    { candidate: malformedGroups, digest: "sha256", label: "malformed groups" },
    { candidate: malformedBindingTypes, digest: "sha256", label: "malformed binding types" },
    { candidate: malformedContracts, digest: "sha256", label: "malformed binding contracts" },
  ];

  // isolate every malformed candidate family
  for (const fixtureCase of cases) {
    await testContext.test(fixtureCase.label, () => {
      const result = validateBackendPerformanceWorkloadDefinition(fixtureCase.candidate);
      assert.deepEqual(Object.keys(result).sort(), [
        "canonicalEligible",
        "definitionSha256",
        "diagnostics",
        "ok",
      ]);
      assert.equal(result.ok, false);
      assert.equal(result.canonicalEligible, false);
      assert.ok(result.diagnostics.length > 0);

      // distinguish encodable invalid inputs from non-JSON inputs
      if (fixtureCase.digest === null) {
        assert.equal(result.definitionSha256, null);
      } else {
        assert.match(result.definitionSha256, /^[a-f0-9]{64}$/u);
      }
    });
  }

  let rootGetterExecutions = 0;
  const accessorRoot = {};
  Object.defineProperty(accessorRoot, "contractVersion", {
    enumerable: true,
    // fail if validation executes an untrusted root getter
    get() {
      rootGetterExecutions += 1;
      throw new Error("root getter executed");
    },
  });
  const rootResult = validateBackendPerformanceWorkloadDefinition(accessorRoot);
  assert.equal(rootResult.ok, false);
  assert.equal(rootResult.canonicalEligible, false);
  assert.equal(rootResult.definitionSha256, null);
  assert.ok(rootResult.diagnostics.length > 0);
  assert.equal(rootGetterExecutions, 0);

  let nestedGetterExecutions = 0;
  const accessorOperation = {};
  Object.defineProperty(accessorOperation, "id", {
    enumerable: true,
    // fail if validation executes an untrusted nested getter
    get() {
      nestedGetterExecutions += 1;
      throw new Error("nested getter executed");
    },
  });
  const accessorNested = structuredClone(definition);
  accessorNested.operations[0] = accessorOperation;
  const nestedResult = validateBackendPerformanceWorkloadDefinition(accessorNested);
  assert.equal(nestedResult.ok, false);
  assert.equal(nestedResult.canonicalEligible, false);
  assert.equal(nestedResult.definitionSha256, null);
  assert.ok(nestedResult.diagnostics.length > 0);
  assert.equal(nestedGetterExecutions, 0);

  let bindingTypeGetterExecutions = 0;
  const accessorBindingType = structuredClone(definition);
  Object.defineProperty(accessorBindingType.bindingSlotTypes, "provider.*", {
    enumerable: true,
    // fail if validation executes an untrusted binding-type getter
    get() {
      bindingTypeGetterExecutions += 1;
      throw new Error("binding type getter executed");
    },
  });
  const bindingTypeResult = validateBackendPerformanceWorkloadDefinition(
    accessorBindingType,
  );
  assert.equal(bindingTypeResult.ok, false);
  assert.equal(bindingTypeResult.canonicalEligible, false);
  assert.equal(bindingTypeResult.definitionSha256, null);
  assert.equal(bindingTypeGetterExecutions, 0);
});

// reject every caller-visible manifest drift family
test("workload definition rejects operation count order target slot and schema drift", async (testContext) => {
  const definition = await readDefinition();
  const cases = [
    { label: "operation identity", path: ["operations", 0, "id"], value: "page.caller-added" },
    { label: "warm count", path: ["operations", 0, "count", "warmAttemptsPerRun"], value: 41 },
    { label: "operation order", path: ["operations", 0, "orderWithinGroup"], value: 2 },
    { label: "HTTP target", path: ["operations", 0, "exactHttp", "target"], value: "/caller" },
    { label: "binding slot", path: ["operations", 0, "bindingSlots", 0], value: "topology.caller" },
    { label: "run slot", path: ["runPolicy", "orderedRunSlots", 0], value: "run-slot-caller" },
    { label: "group order", path: ["groups", 1, "globalOrder"], value: 99 },
    {
      label: "transition producer",
      path: ["transitionProducers", "transition.admin.liveSessionCookie"],
      value: "admin.live.logout",
    },
  ];

  // isolate every closed-definition drift family
  for (const fixtureCase of cases) {
    await testContext.test(fixtureCase.label, () => {
      const candidate = structuredClone(definition);
      let parent = candidate;

      // descend to the selected closed field
      for (const component of fixtureCase.path.slice(0, -1)) {
        parent = parent[component];
      }
      parent[fixtureCase.path.at(-1)] = fixtureCase.value;
      const result = validateBackendPerformanceWorkloadDefinition(candidate);
      assert.equal(result.ok, false);
      assert.equal(result.canonicalEligible, false);
      assert.ok(result.diagnostics.length > 0);
    });
  }

  await testContext.test("missing root field", () => {
    const candidate = structuredClone(definition);
    delete candidate.bindingPrerequisites;
    const result = validateBackendPerformanceWorkloadDefinition(candidate);
    assert.equal(result.ok, false);
    assert.equal(result.canonicalEligible, false);
    assert.ok(result.diagnostics.length > 0);
  });

  await testContext.test("additional operation", () => {
    const candidate = structuredClone(definition);
    candidate.operations.push(structuredClone(candidate.operations[0]));
    const result = validateBackendPerformanceWorkloadDefinition(candidate);
    assert.equal(result.ok, false);
    assert.equal(result.canonicalEligible, false);
    assert.ok(result.diagnostics.length > 0);
  });
});

// require one deterministic five-slot plan with mandatory restoration
test("planned workload expands exact attempts groups deadlines and restore gates", async () => {
  const definition = await readDefinition();
  const first = buildBackendPerformanceWorkloadPlan();
  const second = buildBackendPerformanceWorkloadPlan();
  assert.deepEqual(second, first);
  assert.throws(
    // reject caller-selected definitions
    () => buildBackendPerformanceWorkloadPlan(definition),
    TypeError,
  );
  assert.deepEqual(Object.keys(first).sort(), [
    "canonicalEligible",
    "contractVersion",
    "definition",
    "runSlots",
    "totals",
    "transitions",
  ]);
  assert.equal(first.canonicalEligible, false);
  assert.equal(Object.isFrozen(first), true);
  assert.equal(Object.isFrozen(first.definition), true);
  assert.equal(Object.isFrozen(first.runSlots), true);
  assert.equal(Object.isFrozen(first.totals), true);
  assert.equal(Object.isFrozen(first.transitions), true);
  assert.equal(first.contractVersion, BACKEND_PERFORMANCE_SCHEDULE_VERSION);
  assert.equal(first.definition.contractVersion, BACKEND_PERFORMANCE_WORKLOAD_VERSION);
  assert.equal(first.definition.sha256, EXPECTED_DEFINITION_SHA256);
  assert.deepEqual(first.runSlots.map(
    // retain the exact ordered slot identities
    (slot) => slot.id,
  ), RUN_SLOT_IDS);
  assert.deepEqual(first.totals, {
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
  });
  const groupById = new Map(definition.groups.map(
    // index every fixed group definition
    (group) => [group.id, group],
  ));
  const operationById = new Map(definition.operations.map(
    // index every fixed operation definition
    (operation) => [operation.id, operation],
  ));
  const attemptIds = new Set();

  // validate every slot against the same closed definition
  for (const [slotIndex, slot] of first.runSlots.entries()) {
    assert.deepEqual(Object.keys(slot).sort(), ["groups", "id", "order"]);
    assert.equal(slot.order, slotIndex + 1);
    assert.equal(slot.groups.length, 59);
    let nextOrdinal = 1;

    // validate the exact atomic group chain
    for (const [groupIndex, group] of slot.groups.entries()) {
      const sourceGroup = groupById.get(group.id);
      assert.deepEqual(Object.keys(group).sort(), [
        "attempts",
        "dependsOnGroupId",
        "finalRestore",
        "id",
        "order",
        "preState",
      ]);
      assert.equal(group.order, groupIndex + 1);
      assert.equal(group.id, definition.runPolicy.globalAtomicGroupOrder[groupIndex]);
      assert.equal(group.dependsOnGroupId, groupIndex === 0 ? null : slot.groups[groupIndex - 1].id);
      assert.deepEqual(group.preState, {
        contractVersion: "weather-backend-group-prestate/v1",
        id: `control.prestate::${group.id}`,
        requiredBeforeAttempts: true,
        sequence: "before-attempts",
      });
      assert.deepEqual(group.finalRestore, {
        contractVersion: "weather-backend-group-restore/v1",
        id: sourceGroup.finalRestoreOperation.id,
        requiredBeforeNextGroup: true,
        sequence: "finally-after-attempts",
      });
      const expectedAttempts = expectedGroupAttempts(definition, sourceGroup);
      assert.equal(group.attempts.length, expectedAttempts.length);

      // validate every attempt projection and deadline
      for (const [attemptIndex, attempt] of group.attempts.entries()) {
        const expected = expectedAttempts[attemptIndex];
        assert.deepEqual(Object.keys(attempt).sort(), [
          "attemptId",
          "deadlineMs",
          "groupId",
          "operationId",
          "ordinal",
          "runSlotId",
          "stratum",
          "timeoutPolicy",
        ]);
        assert.equal(attempt.runSlotId, slot.id);
        assert.equal(attempt.groupId, group.id);
        assert.equal(attempt.operationId, expected.operation.id);
        assert.equal(attempt.stratum, expected.stratum);
        assert.equal(attempt.timeoutPolicy, expected.operation.timeoutPolicy);
        assert.equal(attempt.deadlineMs, operationDeadline(definition, expected.operation));
        assert.equal(attempt.ordinal, nextOrdinal);
        assert.equal(typeof attempt.attemptId, "string");
        assert.ok(attempt.attemptId.length > 0);
        assert.equal(attemptIds.has(attempt.attemptId), false);
        attemptIds.add(attempt.attemptId);
        nextOrdinal += 1;
      }
    }
    assert.equal(nextOrdinal, 1259);

    // require every same-slot transition producer before all consumers
    const attempts = slot.groups.flatMap(
      // retain the planned execution order
      (group) => group.attempts,
    );
    for (const transition of first.transitions) {
      const producerIndex = attempts.findIndex(
        // locate the declared same-run producer
        (attempt) => attempt.operationId === transition.producerOperationId,
      );
      assert.ok(producerIndex >= 0, transition.bindingSlot);
      const consumers = definition.operations.filter(
        // select every operation consuming this transition
        (operation) => operation.bindingSlots.includes(transition.bindingSlot),
      );

      // require every consumer after its producer
      for (const consumer of consumers) {
        const consumerIndex = attempts.findIndex(
          // locate the first same-run consumer
          (attempt) => attempt.operationId === consumer.id,
        );
        assert.ok(consumerIndex > producerIndex, `${transition.bindingSlot}:${consumer.id}`);
      }
    }
  }
  assert.equal(attemptIds.size, 6290);
  assert.equal(first.runSlots.every(
    // require every schedule slot to remain immutable
    (slot) => Object.isFrozen(slot) && Object.isFrozen(slot.groups) &&
      slot.groups.every(
        // require every group and attempt projection immutable
        (group) => Object.isFrozen(group) && Object.isFrozen(group.attempts) &&
          group.attempts.every(Object.isFrozen),
      ),
  ), true);
  assert.equal(first.transitions.length, 10);
  assert.deepEqual(first.transitions, Object.entries(definition.transitionProducers).map(
    // render the exact sorted transition edges
    ([bindingSlot, producerOperationId]) => ({ bindingSlot, producerOperationId }),
  ).sort(
    // sort edges by typed binding slot
    (left, right) => left.bindingSlot.localeCompare(right.bindingSlot),
  ));
});

// classify every typed slot without retaining binding values
test("binding slot classification is closed sorted frozen and value-free", async () => {
  const definition = await readDefinition();
  assert.equal(classifyBackendPerformanceBindingSlots.length, 0);
  assert.throws(
    // reject caller-selected slot definitions
    () => classifyBackendPerformanceBindingSlots(definition),
    { code: "workload.selector" },
  );
  const classification = classifyBackendPerformanceBindingSlots();
  const allSlots = [...new Set(definition.operations.flatMap(
    // retain every source-owned typed slot name
    (operation) => operation.bindingSlots,
  ))].sort();
  const expectedPhase = allSlots.filter(
    // select phase-specific provenance slots
    (slot) => slot.startsWith("provenance."),
  );
  const expectedSecrets = allSlots.filter(
    // select out-of-band secret handle slots
    (slot) => slot.startsWith("secret."),
  );
  const expectedTransitions = allSlots.filter(
    // select same-run transition slots
    (slot) => slot.startsWith("transition."),
  );
  const excluded = new Set([...expectedPhase, ...expectedSecrets, ...expectedTransitions]);
  const expectedShared = allSlots.filter(
    // retain every immutable shared slot
    (slot) => !excluded.has(slot),
  );
  assert.deepEqual(Object.keys(classification).sort(), [
    "canonicalEligible",
    "contractVersion",
    "counts",
    "definition",
    "immutableShared",
    "phaseProvenance",
    "secretHandles",
    "transitions",
  ]);
  assert.equal(classification.canonicalEligible, false);
  assert.equal(
    classification.contractVersion,
    "weather-backend-performance-binding-slot-classification/v2",
  );
  assert.deepEqual(classification.definition, {
    contractVersion: BACKEND_PERFORMANCE_WORKLOAD_VERSION,
    sha256: EXPECTED_DEFINITION_SHA256,
  });
  assert.deepEqual(classification.counts, {
    immutableShared: 142,
    phaseProvenance: 2,
    secretHandles: 2,
    total: 156,
    transitions: 10,
  });
  assert.deepEqual(classification.immutableShared, expectedShared);
  assert.deepEqual(classification.phaseProvenance, expectedPhase);
  assert.deepEqual(classification.secretHandles, expectedSecrets);
  assert.deepEqual(classification.transitions, expectedTransitions);
  assert.deepEqual(expectedPhase, [
    "provenance.applicationSourceRevision",
    "provenance.phaseSafetyEnvelope",
  ]);
  assert.deepEqual(expectedSecrets, ["secret.adminPassword", "secret.xweatherCredentialHandle"]);
  assert.deepEqual(expectedTransitions, [
    "transition.admin.disposableSavedLayoutEntry",
    "transition.admin.disposableSessionCookie",
    "transition.admin.liveSessionCookie",
    "transition.admin.liveSessionRevoked",
    "transition.maintenance.fixedGaugeGapKey",
    "transition.maintenance.rainControlStageReceipt",
    "transition.maintenance.revisionStageReceipt",
    "transition.maintenance.shadowStageReceipt",
    "transition.map.usageLedgerAfterMiss",
    "transition.oneshot.performanceScorecardOutput",
  ]);
  assert.equal(Object.isFrozen(classification), true);
  assert.equal(Object.isFrozen(classification.definition), true);
  assert.equal(Object.isFrozen(classification.counts), true);

  // require every returned slot collection to remain immutable
  for (const slots of [
    classification.immutableShared,
    classification.phaseProvenance,
    classification.secretHandles,
    classification.transitions,
  ]) {
    assert.equal(Object.isFrozen(slots), true);
    assert.ok(slots.every(
      // retain slot names only without values
      (slot) => typeof slot === "string",
    ));
  }
});

// toggle only rain through the exact settings selector
test("admin settings selector preserves independent switches and emits canonical bytes", () => {
  assert.equal(selectBackendPerformanceAdminSettings.length, 1);
  const input = { wind: false, rain: true, version: 1, temperature: true };
  const first = selectBackendPerformanceAdminSettings(input);
  const second = selectBackendPerformanceAdminSettings({
    temperature: true,
    version: 1,
    rain: true,
    wind: false,
  });
  assert.deepEqual(first, second);
  assert.deepEqual(Object.keys(first).sort(), [
    "canonicalEligible",
    "inputSha256",
    "outputSha256",
    "selection",
    "selectorId",
  ]);
  assert.equal(first.canonicalEligible, false);
  assert.equal(first.selectorId, "admin-settings");
  assert.match(first.inputSha256, /^[a-f0-9]{64}$/u);
  assert.match(first.outputSha256, /^[a-f0-9]{64}$/u);
  assert.deepEqual(first.selection, {
    canonicalJson: '{"rain":false,"temperature":true,"version":1,"wind":false}',
    value: { rain: false, temperature: true, version: 1, wind: false },
  });
  assert.equal(first.inputSha256, sha256Json({ rain: true, temperature: true, version: 1, wind: false }));
  assert.equal(first.outputSha256, sha256Json(first.selection));
  assert.equal(Object.isFrozen(first), true);
  assert.equal(Object.isFrozen(first.selection), true);
  assert.equal(Object.isFrozen(first.selection.value), true);
  assert.deepEqual(
    selectBackendPerformanceAdminSettings({ rain: false, temperature: false, version: 1, wind: true }).selection.value,
    { rain: true, temperature: false, version: 1, wind: true },
  );
});

// reject malformed settings and arbitrary selector inputs
test("admin settings selector rejects partial typed and extra inputs", () => {
  const cases = [
    undefined,
    null,
    [],
    {},
    { version: 1, temperature: true, wind: true },
    { version: 2, temperature: true, wind: true, rain: true },
    { version: 1, temperature: 1, wind: true, rain: true },
    { version: 1, temperature: true, wind: "true", rain: true },
    { version: 1, temperature: true, wind: true, rain: null },
    { version: 1, temperature: true, wind: true, rain: true, body: {} },
    { version: 1, temperature: true, wind: true, rain: true, slot: "admin.settings" },
    { version: 1, temperature: true, wind: true, rain: true, url: "https://caller.invalid" },
  ];

  // reject every malformed settings shape
  for (const candidate of cases) {
    assertSelectorError(selectBackendPerformanceAdminSettings, candidate);
  }
});

// choose the lexicographically first inclusive-boundary layout entry
test("admin layout selector validates all entries and creates one closed write payload", () => {
  assert.equal(selectBackendPerformanceAdminLayout.length, 1);
  const input = {
    layout: {
      sensors: [
        layoutEntry("zeta-sensor", { displayName: "  accepted name  ", icon: "rain" }),
        layoutEntry("alpha-sensor", { icon: "wind", latitude: 0.025, longitude: 0.04 }),
        layoutEntry("outside-sensor", { latitude: 0.025_001, longitude: 0 }),
      ],
      version: 1,
    },
    site: admittedSite(),
  };
  const first = selectBackendPerformanceAdminLayout(input);
  const second = selectBackendPerformanceAdminLayout(structuredClone(input));
  assert.deepEqual(first, second);
  assert.equal(first.canonicalEligible, false);
  assert.equal(first.selectorId, "admin-layout");
  assert.match(first.inputSha256, /^[a-f0-9]{64}$/u);
  assert.match(first.outputSha256, /^[a-f0-9]{64}$/u);
  assert.deepEqual(first.selection, {
    sensorKey: "alpha-sensor",
    writePayload: {
      displayName: "G002 benchmark",
      icon: "wind",
      latitude: 0.025,
      longitude: 0.04,
    },
  });
  assert.equal(first.inputSha256, sha256Json(input));
  assert.equal(first.outputSha256, sha256Json(first.selection));
  assert.deepEqual(Object.keys(first.selection.writePayload).sort(), [
    "displayName",
    "icon",
    "latitude",
    "longitude",
  ]);
  assert.equal(Object.isFrozen(first), true);
  assert.equal(Object.isFrozen(first.selection), true);
  assert.equal(Object.isFrozen(first.selection.writePayload), true);

  const toggled = structuredClone(input);
  toggled.layout.sensors[1].displayName = "G002 benchmark";
  assert.equal(
    selectBackendPerformanceAdminLayout(toggled).selection.writePayload.displayName,
    "G002 benchmark 2",
  );

  const legacy = structuredClone(input);
  delete legacy.layout.sensors[1].icon;
  assert.equal(
    selectBackendPerformanceAdminLayout(legacy).selection.writePayload.icon,
    "temperature",
  );
  legacy.layout.sensors[1].icon = null;
  assert.equal(
    selectBackendPerformanceAdminLayout(legacy).selection.writePayload.icon,
    "temperature",
  );
});

// reject invalid layouts including entries beyond the old truncation boundary
test("admin layout selector rejects invalid duplicate extra and ineligible entries", () => {
  const valid = {
    layout: { sensors: [layoutEntry("alpha-sensor")], version: 1 },
    site: admittedSite(),
  };
  const sixtyFive = [];

  // build sixty-four valid entries before one invalid trailing entry
  for (let index = 0; index < 64; index += 1) {
    sixtyFive.push(layoutEntry(`sensor-${String(index)}`, {
      latitude: index === 0 ? 0 : 1,
      longitude: index === 0 ? 0 : 1,
    }));
  }
  sixtyFive.push(layoutEntry("sensor-64", { icon: "invalid" }));
  const cases = [
    { ...valid, extra: true },
    { layout: { ...valid.layout, extra: true }, site: valid.site },
    { layout: valid.layout, site: { ...valid.site, extra: true } },
    {
      layout: { sensors: [{ ...valid.layout.sensors[0], extra: true }], version: 1 },
      site: valid.site,
    },
    {
      layout: { sensors: [layoutEntry("duplicate"), layoutEntry("duplicate")], version: 1 },
      site: valid.site,
    },
    { layout: { sensors: sixtyFive, version: 1 }, site: valid.site },
    {
      layout: { sensors: [layoutEntry("alpha-sensor", { latitude: Number.NaN })], version: 1 },
      site: valid.site,
    },
    {
      layout: { sensors: [layoutEntry("alpha-sensor", { longitude: Number.POSITIVE_INFINITY })], version: 1 },
      site: valid.site,
    },
    {
      layout: { sensors: [layoutEntry("alpha-sensor", { latitude: -0 })], version: 1 },
      site: valid.site,
    },
    {
      layout: { sensors: [layoutEntry("alpha-sensor", { latitude: 85.001 })], version: 1 },
      site: valid.site,
    },
    {
      layout: { sensors: [layoutEntry("alpha-sensor", { longitude: 180.001 })], version: 1 },
      site: valid.site,
    },
    { layout: { sensors: [layoutEntry("A-invalid")], version: 1 }, site: valid.site },
    { layout: { sensors: [layoutEntry("a".repeat(65))], version: 1 }, site: valid.site },
    {
      layout: { sensors: [layoutEntry("alpha-sensor", { displayName: " " })], version: 1 },
      site: valid.site,
    },
    {
      layout: { sensors: [layoutEntry("alpha-sensor", { displayName: "x".repeat(81) })], version: 1 },
      site: valid.site,
    },
    {
      layout: { sensors: [layoutEntry("alpha-sensor", { displayName: "bad\u0000name" })], version: 1 },
      site: valid.site,
    },
    {
      layout: { sensors: [layoutEntry("alpha-sensor", { icon: "soil" })], version: 1 },
      site: valid.site,
    },
    {
      layout: { sensors: [layoutEntry("alpha-sensor", { updatedAt: "2026-11-13" })], version: 1 },
      site: valid.site,
    },
    {
      layout: { sensors: [layoutEntry("alpha-sensor", { latitude: 0.025_001 })], version: 1 },
      site: valid.site,
    },
    { layout: valid.layout, site: admittedSite({ id: "caller-site" }) },
    { layout: valid.layout, site: admittedSite({ version: 2 }) },
    { layout: valid.layout, site: admittedSite({ latitude: -0 }) },
    { layout: valid.layout, site: admittedSite({ longitude: Number.NaN }) },
    { layout: valid.layout, site: admittedSite({ latitude: 85.001 }) },
    { layout: valid.layout, site: admittedSite({ longitude: 180.001 }) },
  ];

  // reject every invalid layout family
  for (const candidate of cases) {
    assertSelectorError(selectBackendPerformanceAdminLayout, candidate);
  }

  const boundedKey = structuredClone(valid);
  boundedKey.layout.sensors[0].sensorKey = "a".repeat(64);
  assert.equal(
    selectBackendPerformanceAdminLayout(boundedKey).selection.sensorKey,
    "a".repeat(64),
  );
  const boundedName = structuredClone(valid);
  boundedName.layout.sensors[0].displayName = "x".repeat(80);
  assert.equal(
    selectBackendPerformanceAdminLayout(boundedName).selection.writePayload.displayName,
    "G002 benchmark",
  );
});

// select the strictly previous closed provider frame across UTC boundaries
test("map frame selector derives fixed history radar geometry and strict UTC frames", () => {
  assert.equal(selectBackendPerformanceMapFrame.length, 1);
  const site = admittedSite({
    latitude: 47.950429954185445,
    longitude: -122.42797012608193,
  });
  const cases = [
    ["2026-11-14T00:10:00.000Z", "20261114000000"],
    ["2026-11-14T00:10:00.001Z", "20261114001000"],
    ["2026-01-01T00:00:00.000Z", "20251231235000"],
    ["2028-03-01T00:00:00.000Z", "20280229235000"],
  ];

  // verify each exact UTC edge independently
  for (const [bindingCreatedAt, validTime] of cases) {
    const input = { bindingCreatedAt, site };
    const first = selectBackendPerformanceMapFrame(input);
    const second = selectBackendPerformanceMapFrame(structuredClone(input));
    assert.deepEqual(first, second);
    assert.equal(first.canonicalEligible, false);
    assert.equal(first.selectorId, "map-frame");
    assert.match(first.inputSha256, /^[a-f0-9]{64}$/u);
    assert.match(first.outputSha256, /^[a-f0-9]{64}$/u);
    assert.deepEqual(first.selection, {
      height: 168,
      kind: "frame",
      latitude: "47.950430",
      layer: "radar",
      longitude: "-122.427970",
      phase: "history",
      target: `/maps/xweather/history/radar/${validTime}/10/256x168/47.950430,-122.427970.png`,
      validTime,
      width: 256,
      zoom: 10,
    });
    assert.equal(first.inputSha256, sha256Json(input));
    assert.equal(first.outputSha256, sha256Json(first.selection));
    assert.equal(Object.isFrozen(first), true);
    assert.equal(Object.isFrozen(first.selection), true);
  }
});

// match the deployed parser's minimum four-digit frame year
test("map frame selector rejects derived years below 0100", () => {
  const site = admittedSite({ latitude: 47.95, longitude: -122.42 });
  assertSelectorError(selectBackendPerformanceMapFrame, {
    bindingCreatedAt: "0099-12-31T23:59:59.999Z",
    site,
  });
  assertSelectorError(selectBackendPerformanceMapFrame, {
    bindingCreatedAt: "0100-01-01T00:00:00.000Z",
    site,
  });
  const firstSupported = selectBackendPerformanceMapFrame({
    bindingCreatedAt: "0100-01-01T00:00:00.001Z",
    site,
  });
  assert.equal(firstSupported.selection.validTime, "01000101000000");
  assert.equal(
    firstSupported.selection.target,
    "/maps/xweather/history/radar/01000101000000/10/256x168/47.950000,-122.420000.png",
  );
});

// reject arbitrary map targets and invalid site or time projections
test("map frame selector rejects invalid coordinates dates and extra caller fields", () => {
  const valid = {
    bindingCreatedAt: "2026-11-14T00:10:00.000Z",
    site: admittedSite({ latitude: 47.95, longitude: -122.42 }),
  };
  const cases = [
    undefined,
    null,
    {},
    { ...valid, target: "/caller" },
    { ...valid, url: "https://caller.invalid" },
    { ...valid, body: {} },
    { ...valid, bindingCreatedAt: "2026-11-14" },
    { ...valid, bindingCreatedAt: "2026-11-14T00:10:00Z" },
    { ...valid, bindingCreatedAt: "invalid" },
    { ...valid, site: admittedSite({ id: "caller-site" }) },
    { ...valid, site: admittedSite({ version: 2 }) },
    { ...valid, site: admittedSite({ latitude: -0 }) },
    { ...valid, site: admittedSite({ longitude: -0 }) },
    { ...valid, site: admittedSite({ latitude: Number.NaN }) },
    { ...valid, site: admittedSite({ longitude: Number.POSITIVE_INFINITY }) },
    { ...valid, site: admittedSite({ latitude: 85.001 }) },
    { ...valid, site: admittedSite({ longitude: 180.001 }) },
    { ...valid, site: { ...valid.site, timezone: "America/Los_Angeles" } },
    { ...valid, site: { site: valid.site, version: 1 } },
  ];

  // reject every invalid map selector family
  for (const candidate of cases) {
    assertSelectorError(selectBackendPerformanceMapFrame, candidate);
  }
});
