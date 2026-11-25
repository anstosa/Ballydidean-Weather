import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFile } from "node:fs/promises";
import test from "node:test";

import { extractBackendClockOccurrences } from "./backend-clock-occurrences.mjs";
import { buildBackendReferenceTimeSourceProof } from "./backend-reference-time-source-proof.mjs";

const WRAPPER_KEYS = ["conditionalSeam", "rtU04", "rtU16"];
const RT_U04_KEYS = [
  "adjudications",
  "contractVersion",
  "decisionProofs",
  "diagnostics",
  "graphSha256",
  "measurementCoverage",
  "policySha256",
  "scope",
  "sourceFileHashes",
  "verdict",
];
const LEMMA_KEYS = [
  "allowedDataSinkSiteIds",
  "contractVersion",
  "diagnostics",
  "inputSourceSiteId",
  "graphSha256",
  "lemmaId",
  "policySha256",
  "safetyNonReachabilitySiteIds",
  "scope",
  "sourceFileHashes",
  "verdict",
];
const RT_U16_KEYS = [
  "allowedFlows",
  "conditionalSeamLemmaSha256",
  "consumerCoverage",
  "consumerResults",
  "contractVersion",
  "descriptorSeedSiteIds",
  "diagnostics",
  "forbiddenFlows",
  "graphSha256",
  "policySha256",
  "scope",
  "sourceFileHashes",
  "unknownFlows",
  "verdict",
];
const ADJUDICATION_KEYS = [
  "bindingStatus",
  "candidateStatus",
  "decisionId",
  "decisionSiteIds",
  "occurrenceId",
  "operationIds",
  "proofEdgeIds",
  "reachabilityStatus",
  "rootStatus",
  "semanticFlowStatus",
  "supportStatus",
];
const MEASUREMENT_KEYS = [
  "attributions",
  "conflictingAttributionOperationIds",
  "decisionId",
  "invalidAttributionEdgeIds",
  "missingOperationIds",
  "observedEdgeIds",
  "observedOperationIds",
  "observedSiteIds",
  "requiredOperationIds",
  "status",
  "unexpectedOperationIds",
];
const SELECTOR_RESULT_KEYS = [
  "allowedPathCount",
  "coveredSiteIds",
  "forbiddenPathCount",
  "groupId",
  "maximum",
  "minimum",
  "missingSiteIds",
  "resolvedSiteIds",
  "selectorId",
  "status",
  "unclassifiedPathCount",
  "unknownPathCount",
];
const SHA256_PATTERN = /^[0-9a-f]{64}$/u;

let policyPromise;
let workloadPromise;

// hash exact fixture bytes
function sha256(value) {
  return createHash("sha256").update(value).digest("hex");
}

// canonicalize proof fixture values
function canonicalJson(value) {
  // retain scalar json spelling
  if (value === null || typeof value !== "object") {
    return JSON.stringify(value);
  }
  // retain semantic array order
  if (Array.isArray(value)) {
    return `[${value.map(
      // canonicalize one array member
      (entry) => canonicalJson(entry),
    ).join(",")}]`;
  }
  const keys = Object.keys(value).sort();
  return `{${keys.map(
    // canonicalize one object member
    (key) => `${JSON.stringify(key)}:${canonicalJson(value[key])}`,
  ).join(",")}}`;
}

// frame one stable identity
function frame(domain, payload) {
  return sha256(Buffer.concat([
    Buffer.from(domain, "utf8"),
    Buffer.from([0]),
    Buffer.from(canonicalJson(payload), "utf8"),
  ]));
}

// hash one canonical document
function documentSha256(value) {
  return sha256(Buffer.from(`${canonicalJson(value)}\n`, "utf8"));
}

// load the checked policy
async function checkedPolicy() {
  // retain one immutable policy load
  if (policyPromise === undefined) {
    policyPromise = readFile(
      new URL("../config/backend-reference-time-source-policy-v1.json", import.meta.url),
      "utf8",
    ).then(
      // parse one checked policy
      (value) => JSON.parse(value),
    );
  }
  return structuredClone(await policyPromise);
}

// load the checked workload
async function checkedWorkload() {
  // retain one immutable workload load
  if (workloadPromise === undefined) {
    workloadPromise = readFile(
      new URL("../config/backend-performance-workload-v2.json", import.meta.url),
      "utf8",
    ).then(
      // parse one checked workload
      (value) => JSON.parse(value),
    );
  }
  return structuredClone(await workloadPromise);
}

// require exact key order
function assertExactKeys(value, keys) {
  assert.deepEqual(Object.keys(value), keys);
}

// require recursive immutability
function assertDeepFrozen(value, seen = new Set()) {
  // stop at scalar or repeated values
  if (value === null || typeof value !== "object" || seen.has(value)) {
    return;
  }
  seen.add(value);
  assert.equal(Object.isFrozen(value), true);
  // inspect every retained child
  for (const child of Object.values(value)) {
    assertDeepFrozen(child, seen);
  }
}

// build one public file record
function publicFile(path, language = "typescript", sourceText = "fixture\n") {
  const rawSha256 = sha256(Buffer.from(sourceText, "utf8"));
  const separator = path.lastIndexOf("/");
  return {
    byteLength: Buffer.byteLength(sourceText),
    containerSpan: null,
    derivedId: null,
    language,
    origins: ["initial-seed"],
    parentPath: null,
    parentRawSha256: null,
    path,
    rawSha256,
    resolutionBase: separator === -1 ? "" : path.slice(0, separator),
    sourceKind: "physical",
  };
}

// build one public site
function sourceSite(file, options = {}) {
  const startOffsetUtf16 = options.startOffsetUtf16 ?? 0;
  const endOffsetUtf16 = options.endOffsetUtf16 ?? startOffsetUtf16 + 1;
  const ownerSymbol = options.ownerSymbol ?? "fixture";
  const semanticRole = options.semanticRole ?? "data-edge";
  const syntaxKind = options.syntaxKind ?? "identifier";
  const siteId = frame("weather-backend-source-site/v1", {
    endUtf16: endOffsetUtf16,
    fileSha256: file.rawSha256,
    ownerSymbol,
    path: file.path,
    semanticRole,
    startUtf16: startOffsetUtf16,
    syntaxKind,
  });
  return {
    end: {
      columnUtf16: endOffsetUtf16,
      line: options.line ?? 1,
      offsetUtf16: endOffsetUtf16,
    },
    fileSha256: file.rawSha256,
    ownerSymbol,
    path: file.path,
    semanticRole,
    siteId,
    start: {
      columnUtf16: startOffsetUtf16,
      line: options.line ?? 1,
      offsetUtf16: startOffsetUtf16,
    },
    syntaxKind,
  };
}

// build one public edge
function sourceEdge(fromSiteId, toSiteId, options = {}) {
  const effect = options.effect ?? "local";
  const kind = options.kind ?? "data";
  const operationIds = options.operationIds ?? [];
  const edgeId = frame("weather-backend-source-edge/v1", {
    effect,
    fromSiteId,
    kind,
    operationIds,
    toSiteId,
  });
  return {
    edgeId,
    effect,
    fromSiteId,
    kind,
    operationIds,
    status: options.status ?? "resolved",
    toSiteId,
  };
}

// build one resolved public root
function sourceRoot(entrySiteId, operationIds, options = {}) {
  const phase = options.phase ?? "fixture";
  const rootKind = options.rootKind ?? "module-entry";
  const seedId = options.seedId ?? "workload.roots";
  const topologyRole = options.topologyRole ?? "operation-entry";
  const rootId = frame("weather-backend-source-root/v1", {
    entrySiteId,
    operationIds,
    phase,
    rootKind,
    seedId,
    topologyRole,
  });
  return {
    entrySiteId,
    operationIds,
    phase,
    rootId,
    rootKind,
    seedId,
    selectorIds: [],
    status: "resolved",
    topologyRole,
  };
}

// build exact operation rows
async function operationRows() {
  const workload = await checkedWorkload();
  return workload.operations.map(
    // retain workload order only
    (operation, workloadOrder) => ({
      entryRootIds: [],
      measurementRootIds: [],
      operationId: operation.id,
      workloadOrder,
    }),
  );
}

// build one exact graph document
async function graphDocument(overrides = {}) {
  const files = overrides.files ?? [];
  const derivedFiles = files.filter(
    // count derived sources
    (file) => file.sourceKind === "derived",
  );
  const physicalFiles = files.filter(
    // count physical sources
    (file) => file.sourceKind === "physical",
  );
  const derivedBytes = derivedFiles.reduce(
    // add one derived byte count
    (total, file) => total + file.byteLength,
    0,
  );
  const physicalBytes = physicalFiles.reduce(
    // add one physical byte count
    (total, file) => total + file.byteLength,
    0,
  );
  return {
    analysisProfiles: [],
    contractVersion: "weather-backend-operation-source-graph/v1",
    diagnostics: [],
    edges: [],
    files,
    fixedPoint: {
      derivedBytes,
      derivedSourceCount: derivedFiles.length,
      physicalBytes,
      physicalFileCount: physicalFiles.length,
      roundCount: 1,
      totalBytes: derivedBytes + physicalBytes,
    },
    operations: await operationRows(),
    readRequests: [],
    roots: [],
    sites: [],
    status: "complete",
    ...overrides,
  };
}

// project one v1 occurrence into the union
function projectV1(record) {
  const dollarContainer = record.surface === "postgres.dollar-quoted-body";
  return {
    confidence: dollarContainer ? "non-provider-container" : record.confidence,
    container: record.embeddedSql === null ? null : {
      containerOccurrenceId: null,
      containerSpan: record.embeddedSql.containerPhysicalSpan,
      decodedSpan: {
        endOffsetUtf16: record.embeddedSql.decodedRange.endOffsetUtf16,
        startOffsetUtf16: record.embeddedSql.decodedRange.startOffsetUtf16,
      },
      parentSourceLocator: record.sourcePath,
    },
    countsAsClockCandidate: dollarContainer ? false : record.countsAsClockCandidate,
    duplicateOrdinal: record.duplicateOrdinal,
    extractorOrigin: {
      record,
      syntheticSourcePath: null,
    },
    id: record.id,
    identityVersion: "physical-v1",
    kind: dollarContainer ? "non-provider-container" : record.kind,
    language: record.language,
    normalizedExpressionSha256: record.normalizedExpressionSha256,
    ownerSymbol: record.enclosingSymbol,
    parentOccurrenceId: null,
    sourceLocator: record.sourcePath,
    span: record.physicalSpan,
    surface: record.surface,
    syntaxKind: dollarContainer ? "expanded-container" : record.syntaxKind,
  };
}

// extract one physical occurrence union
function occurrencesFrom(path, sourceText) {
  return extractBackendClockOccurrences({ sourcePath: path, sourceText }).occurrences.map(
    // preserve every v1 identity
    (record) => projectV1(record),
  );
}

// build one proof input
async function proofInput(overrides = {}) {
  return {
    graph: await graphDocument(),
    occurrences: [],
    policy: await checkedPolicy(),
    scope: "current-source",
    ...overrides,
  };
}

// select one measurement record
function measurementRecord(result, decisionId) {
  const record = result.rtU04.measurementCoverage.find(
    // match one measurement identity
    (candidate) => candidate.decisionId === decisionId,
  );
  assert.notEqual(record, undefined);
  return record;
}

// select diagnostic codes
function diagnosticCodes(document) {
  return document.diagnostics.map(
    // retain one closed code
    (diagnostic) => diagnostic.code,
  );
}

test(
  "proof assembly returns only the three detached frozen child documents",
  // verify the approved in-process wrapper
  async () => {
    const input = await proofInput();
    const before = structuredClone(input);
    const result = buildBackendReferenceTimeSourceProof(input);

    assert.deepEqual(Object.keys(result).sort(), [...WRAPPER_KEYS].sort());
    assertExactKeys(result.rtU04, RT_U04_KEYS);
    assertExactKeys(result.conditionalSeam, LEMMA_KEYS);
    assertExactKeys(result.rtU16, RT_U16_KEYS);
    assert.equal(Object.hasOwn(result, "contractVersion"), false);
    assert.equal(Object.hasOwn(result, "verdict"), false);
    assert.equal(Object.hasOwn(result, "graphSha256"), false);
    assert.deepEqual(input, before);
    assertDeepFrozen(result);

    assert.throws(
      // reject a missing proof input
      () => buildBackendReferenceTimeSourceProof(),
      TypeError,
    );
    assert.throws(
      // reject a caller verdict
      () => buildBackendReferenceTimeSourceProof({ ...input, verdict: "PASS" }),
      TypeError,
    );
    assert.throws(
      // reject a caller edge override
      () => buildBackendReferenceTimeSourceProof({
        ...input,
        edges: input.graph.edges,
      }),
      TypeError,
    );
  },
);

test(
  "child hashes bind canonical graph policy and source bytes without self digests",
  // verify external hash custody
  async () => {
    const policy = await checkedPolicy();
    const file = publicFile("fixture/hash.ts", "typescript", "export const value = 1;\n");
    const graph = await graphDocument({ files: [file] });
    const result = buildBackendReferenceTimeSourceProof(await proofInput({ graph, policy }));
    const expectedGraphSha256 = documentSha256(graph);
    const expectedPolicySha256 = documentSha256(policy);

    assert.equal(result.rtU04.graphSha256, expectedGraphSha256);
    assert.equal(result.rtU04.policySha256, expectedPolicySha256);
    assert.equal(result.conditionalSeam.graphSha256, expectedGraphSha256);
    assert.equal(result.rtU16.graphSha256, expectedGraphSha256);
    assert.deepEqual(result.rtU04.sourceFileHashes, [{
      path: file.path,
      rawSha256: file.rawSha256,
    }]);
    assert.equal(Object.hasOwn(result.rtU04, "sha256"), false);
    assert.equal(Object.hasOwn(result.rtU16, "sha256"), false);
    assert.match(result.rtU16.conditionalSeamLemmaSha256, SHA256_PATTERN);
    assert.equal(
      result.rtU16.conditionalSeamLemmaSha256,
      documentSha256(result.conditionalSeam),
    );
  },
);

test(
  "checked policy retains 421 symmetric pairs strict W3 sites and separate B2 clocks",
  // verify approved source-policy overrides
  async () => {
    const policy = await checkedPolicy();
    assert.deepEqual(policy.counts, {
      calendarAdmissionConsumers: 1,
      dataReferenceConsumers: 51,
      decisions: 47,
      operations: 107,
      referenceConsumers: 52,
    });
    assert.equal(policy.referenceFlowPolicy.consumers.length, 52);
    assert.equal(policy.referenceFlowPolicy.sinkExpectationGroups.length, 78);
    assert.equal(
      policy.decisionCatalog.reduce(
        // count decision selectors
        (total, decision) => total + decision.siteSelectors.length,
        0,
      )
        + policy.prerequisitePolicy.fullEvaluatorSelectors.length
        + policy.prerequisitePolicy.referenceDescriptorSelectors.length
        + policy.referenceFlowPolicy.descriptorSeedSelectors.length
        + policy.referenceFlowPolicy.sinkExpectationGroups.reduce(
          // count sink selectors
          (total, group) => total + group.selectors.length,
          0,
        ),
      160,
    );
    const pairs = policy.decisionCatalog.flatMap(
      // retain every decision operation pair
      (decision) => decision.expectedOperations.map(
        // frame one pair identity
        (operationId) => `${decision.decisionId}\0${operationId}`,
      ),
    );
    assert.equal(pairs.length, 421);
    assert.equal(new Set(pairs).size, 421);

    const w3 = policy.decisionCatalog.find(
      // locate shared http deadline policy
      (decision) => decision.decisionId === "worker.W3.deadlineRetry",
    );
    const b2 = policy.decisionCatalog.find(
      // locate local backfill clock policy
      (decision) => decision.decisionId === "oneshot.B2.backfillOperations",
    );
    assert.notEqual(w3, undefined);
    assert.notEqual(b2, undefined);
    assert.equal(w3.expectedOperations.length, 29);
    assert.deepEqual(w3.expectedOperations.slice(-4), [
      "oneshot.tide-backfill",
      "oneshot.tempest-backfill",
      "oneshot.public-station-backfill",
      "oneshot.general-backfill",
    ]);
    const httpSelector = w3.siteSelectors.find(
      // locate the shared timer selector
      (selector) => selector.path === "packages/providers/src/http.ts",
    );
    assert.deepEqual(httpSelector.cardinality, { maximum: 3, minimum: 3 });
    assert.equal(b2.siteSelectors.length, 4);
    assert.equal(b2.expectedOperations.length, 4);
    assert.equal(
      b2.siteSelectors.every(
        // retain only local backfill default clocks
        (selector) => selector.ownerSymbol === "defaultNow"
          && selector.semanticRole === "provider-read"
          && selector.path !== "packages/providers/src/http.ts",
      ),
      true,
    );
  },
);

test(
  "current foundation derives missing evaluator descriptor and consumer blockers in every scope",
  // verify truthful incomplete prerequisites
  async () => {
    const scopes = ["current-source", "implemented-source", "physical-baseline"];
    // exercise every closed scope
    for (const scope of scopes) {
      const result = buildBackendReferenceTimeSourceProof(await proofInput({ scope }));
      assert.equal(result.rtU04.verdict, "INCOMPLETE");
      assert.equal(
        diagnosticCodes(result.rtU04).includes("prerequisite.full-evaluator-source-missing"),
        true,
      );
      assert.equal(result.rtU16.verdict, "INCOMPLETE");
      assert.equal(
        diagnosticCodes(result.rtU16).includes("prerequisite.descriptor-lineage-missing"),
        true,
      );
      assert.equal(
        diagnosticCodes(result.rtU16).includes("prerequisite.reference-consumers-incomplete"),
        true,
      );
      assert.deepEqual(result.rtU16.descriptorSeedSiteIds, []);
      assert.deepEqual(result.rtU16.consumerCoverage, {
        calendarAdmission: 0,
        covered: 0,
        dataReference: 0,
        required: 52,
      });
      assert.equal(result.rtU16.consumerResults.length, 52);
      assert.equal(
        result.rtU16.consumerResults.every(
          // retain every uncovered consumer separately
          (consumer) => consumer.status === "uncovered",
        ),
        true,
      );
    }
  },
);

test(
  "W3 resolves exactly three shared HTTP timer sites and conflicts on a fourth",
  // verify strict physical timer cardinality
  async () => {
    const policy = await checkedPolicy();
    const w3 = policy.decisionCatalog.find(
      // locate the shared deadline decision
      (decision) => decision.decisionId === "worker.W3.deadlineRetry",
    );
    const filesByPath = new Map();
    const sites = [];
    const edges = [];
    const rootFile = publicFile("fixture/w3-root.ts", "typescript", "x".repeat(4096));
    const rootSite = sourceSite(rootFile, {
      endOffsetUtf16: 1,
      ownerSymbol: "fixtureW3Root",
      semanticRole: "operation-entry",
      startOffsetUtf16: 0,
      syntaxKind: "operation-entry",
    });
    filesByPath.set(rootFile.path, rootFile);
    sites.push(rootSite);

    // materialize every reviewed selector cardinality
    for (const selector of w3.siteSelectors) {
      let file = filesByPath.get(selector.path);
      // add one source file per selector path
      if (file === undefined) {
        file = publicFile(selector.path, "typescript", "x".repeat(4096));
        filesByPath.set(selector.path, file);
      }
      // create each exact physical selector site
      for (let index = 0; index < selector.cardinality.minimum; index += 1) {
        const offset = 10 + sites.length * 2;
        const site = sourceSite(file, {
          endOffsetUtf16: offset + 1,
          ownerSymbol: selector.ownerSymbol,
          semanticRole: selector.semanticRole,
          startOffsetUtf16: offset,
          syntaxKind: "call",
        });
        sites.push(site);
        edges.push(sourceEdge(rootSite.siteId, site.siteId, {
          kind: selector.semanticRole === "timer" ? "call" : "control",
          operationIds: w3.expectedOperations,
        }));
      }
    }
    const baseGraph = await graphDocument({
      edges: edges.toSorted(
        // retain exact edge order
        (left, right) => left.edgeId.localeCompare(right.edgeId),
      ),
      files: [...filesByPath.values()].toSorted(
        // retain exact file order
        (left, right) => left.path.localeCompare(right.path),
      ),
      sites: sites.toSorted(
        // retain exact site order
        (left, right) => left.path.localeCompare(right.path)
          || left.start.offsetUtf16 - right.start.offsetUtf16
          || left.end.offsetUtf16 - right.end.offsetUtf16
          || left.siteId.localeCompare(right.siteId),
      ),
    });
    const base = buildBackendReferenceTimeSourceProof(await proofInput({
      graph: baseGraph,
      policy,
    }));
    const baseDecision = base.rtU04.decisionProofs.find(
      // locate the computed w3 decision
      (decision) => decision.decisionId === w3.decisionId,
    );
    const httpSites = baseDecision.siteIds.filter(
      // retain sites from the shared http source
      (siteId) => baseGraph.sites.find(
        // locate the matching public site
        (site) => site.siteId === siteId,
      ).path === "packages/providers/src/http.ts",
    );
    assert.equal(httpSites.length, 3);
    assert.deepEqual(baseDecision.computedOperations, w3.expectedOperations);

    const httpFile = filesByPath.get("packages/providers/src/http.ts");
    const fourth = sourceSite(httpFile, {
      endOffsetUtf16: 1001,
      ownerSymbol: "<anonymous>",
      semanticRole: "timer",
      startOffsetUtf16: 1000,
      syntaxKind: "call",
    });
    const fourthEdge = sourceEdge(rootSite.siteId, fourth.siteId, {
      kind: "call",
      operationIds: w3.expectedOperations,
    });
    const conflictGraph = await graphDocument({
      ...baseGraph,
      edges: [...baseGraph.edges, fourthEdge].toSorted(
        // retain exact edge order
        (left, right) => left.edgeId.localeCompare(right.edgeId),
      ),
      sites: [...baseGraph.sites, fourth].toSorted(
        // retain exact site order
        (left, right) => left.path.localeCompare(right.path)
          || left.start.offsetUtf16 - right.start.offsetUtf16
          || left.end.offsetUtf16 - right.end.offsetUtf16
          || left.siteId.localeCompare(right.siteId),
      ),
    });
    const conflict = buildBackendReferenceTimeSourceProof(await proofInput({
      graph: conflictGraph,
      policy,
    }));
    const conflictDecision = conflict.rtU04.decisionProofs.find(
      // locate the conflicting w3 decision
      (decision) => decision.decisionId === w3.decisionId,
    );
    assert.equal(conflictDecision.status, "source-conflict");
    assert.equal(conflict.rtU04.verdict, "FAIL");
    assert.equal(
      diagnosticCodes(conflict.rtU04).includes("binding.source-conflict"),
      true,
    );
  },
);

test(
  "six adjudication dimensions preserve parse-only and genuine unreachable clocks",
  // verify orthogonal occurrence adjudication
  async () => {
    const path = "fixture/adjudication.ts";
    const sourceText = [
      "export function main() {",
      "  const parsed = new Date('2026-01-01T00:00:00Z');",
      "  return Date.now() + parsed.getTime();",
      "}",
      "",
    ].join("\n");
    const file = publicFile(path, "typescript", sourceText);
    const occurrences = occurrencesFrom(path, sourceText);
    const sites = occurrences.map(
      // create one occurrence-backed site
      (occurrence) => sourceSite(file, {
        endOffsetUtf16: occurrence.span.endOffsetUtf16,
        ownerSymbol: occurrence.ownerSymbol,
        semanticRole: occurrence.kind === "parse-only" ? "parse-conversion" : "provider-read",
        startOffsetUtf16: occurrence.span.startOffsetUtf16,
        syntaxKind: occurrence.syntaxKind === "new-expression" ? "construct" : "call",
      }),
    );
    const rootSite = sourceSite(file, {
      endOffsetUtf16: 1,
      ownerSymbol: "<module>",
      semanticRole: "operation-entry",
      startOffsetUtf16: 0,
      syntaxKind: "operation-entry",
    });
    const clockSite = sites.find(
      // locate the real clock site
      (site) => site.semanticRole === "provider-read",
    );
    const operationIds = ["api.sites"];
    const root = sourceRoot(rootSite.siteId, operationIds);
    const operations = await operationRows();
    const operation = operations.find(
      // locate the rooted operation
      (candidate) => candidate.operationId === operationIds[0],
    );
    assert.notEqual(operation, undefined, "fixture workload must contain api.sites");
    operation.entryRootIds = [root.rootId];
    const graph = await graphDocument({
      edges: [sourceEdge(rootSite.siteId, clockSite.siteId, {
        kind: "call",
        operationIds,
        status: "operation-unreachable",
      })],
      files: [file],
      operations,
      roots: [root],
      sites: [rootSite, ...sites].toSorted(
        // preserve exact site order
        (left, right) => left.start.offsetUtf16 - right.start.offsetUtf16
          || left.end.offsetUtf16 - right.end.offsetUtf16
          || left.siteId.localeCompare(right.siteId),
      ),
    });
    const result = buildBackendReferenceTimeSourceProof(await proofInput({
      graph,
      occurrences,
    }));

    assert.equal(result.rtU04.adjudications.length, occurrences.length);
    // validate each exact adjudication record
    for (const adjudication of result.rtU04.adjudications) {
      assertExactKeys(adjudication, ADJUDICATION_KEYS);
    }
    const parseOccurrence = occurrences.find(
      // locate the supplied-date parse occurrence
      (occurrence) => occurrence.kind === "parse-only",
    );
    const parseAdjudication = result.rtU04.adjudications.find(
      // locate its adjudication
      (adjudication) => adjudication.occurrenceId === parseOccurrence.id,
    );
    assert.equal(parseAdjudication.candidateStatus, "noncandidate");
    assert.notEqual(parseAdjudication.bindingStatus, "bound-clock");

    const clockOccurrence = occurrences.find(
      // locate the real clock occurrence
      (occurrence) => occurrence.surface === "ecmascript.date-now",
    );
    const clockAdjudication = result.rtU04.adjudications.find(
      // locate its adjudication
      (adjudication) => adjudication.occurrenceId === clockOccurrence.id,
    );
    assert.equal(clockAdjudication.candidateStatus, "candidate");
    assert.equal(clockAdjudication.bindingStatus, "not-applicable");
    assert.equal(clockAdjudication.reachabilityStatus, "operation-unreachable");
    assert.equal(clockAdjudication.decisionId, null);
    assert.deepEqual(clockAdjudication.decisionSiteIds, []);
    assert.deepEqual(clockAdjudication.proofEdgeIds, []);
    assert.deepEqual(clockAdjudication.operationIds, []);
  },
);

test(
  "decision-linked measurement calls derive ordered partial coverage and roots",
  // verify appendix d positive attribution
  async () => {
    const policy = await checkedPolicy();
    const operations = await operationRows();
    const m0 = policy.decisionCatalog.find(
      // locate all-operation measurement policy
      (decision) => decision.decisionId === "harness.M0.measurement",
    );
    const selector = {
      cardinality: { maximum: 1, minimum: 1 },
      ownerSymbol: "measureAttempt",
      path: "fixture/measurement.ts",
      selectorId: "decision.harness.m0.measurement.fixture-call",
      semanticRole: "measurement-call",
      syntaxKind: "typescript-symbol",
    };
    m0.siteSelectors = [selector];

    const file = publicFile(selector.path, "typescript", "measureAttempt();\n");
    const from = sourceSite(file, {
      endOffsetUtf16: 1,
      ownerSymbol: "fixtureRoot",
      semanticRole: "operation-entry",
      startOffsetUtf16: 0,
      syntaxKind: "operation-entry",
    });
    const endpoint = sourceSite(file, {
      endOffsetUtf16: 16,
      ownerSymbol: selector.ownerSymbol,
      semanticRole: "measurement-call",
      startOffsetUtf16: 2,
      syntaxKind: "call",
    });
    const covered = operations.slice(0, 2).map(
      // retain a required operation subset
      (operation) => operation.operationId,
    );
    const edge = sourceEdge(from.siteId, endpoint.siteId, {
      kind: "measurement",
      operationIds: covered,
    });
    const measurementRoot = sourceRoot(endpoint.siteId, covered, {
      phase: "measurement",
      rootKind: "measurement-harness",
      seedId: "measurement.backend-performance-resources",
      topologyRole: "measurement-prerequisite",
    });
    const graphOperations = await operationRows();
    // attach only covered operations
    for (const operation of graphOperations.slice(0, 2)) {
      operation.measurementRootIds = [measurementRoot.rootId];
    }
    const graph = await graphDocument({
      edges: [edge],
      files: [file],
      operations: graphOperations,
      roots: [measurementRoot],
      sites: [from, endpoint],
    });
    const result = buildBackendReferenceTimeSourceProof(await proofInput({
      graph,
      policy,
    }));
    const coverage = measurementRecord(result, "harness.M0.measurement");

    assertExactKeys(coverage, MEASUREMENT_KEYS);
    assert.equal(coverage.status, "partial");
    assert.deepEqual(coverage.observedOperationIds, covered);
    assert.deepEqual(coverage.observedEdgeIds, [edge.edgeId]);
    assert.deepEqual(coverage.observedSiteIds, [endpoint.siteId]);
    assert.deepEqual(coverage.invalidAttributionEdgeIds, []);
    assert.deepEqual(coverage.unexpectedOperationIds, []);
    assert.deepEqual(coverage.attributions, [{
      edgeId: edge.edgeId,
      operationIds: covered,
      siteId: endpoint.siteId,
    }]);
    assert.deepEqual(
      coverage.missingOperationIds,
      operations.slice(2).map(
        // retain every uncovered operation
        (operation) => operation.operationId,
      ),
    );
    assert.deepEqual(graph.operations[0].measurementRootIds, [measurementRoot.rootId]);
    assert.deepEqual(graph.operations[1].measurementRootIds, [measurementRoot.rootId]);
    assert.equal(
      graph.operations.slice(2).every(
        // forbid seed-manufactured uncovered roots
        (operation) => operation.measurementRootIds.length === 0,
      ),
      true,
    );
    assert.equal(result.rtU04.verdict, "INCOMPLETE");
  },
);

test(
  "invalid measurement endpoints and operation lists fail without manufacturing roots",
  // verify appendix d hostile attribution
  async () => {
    const policy = await checkedPolicy();
    const m0 = policy.decisionCatalog.find(
      // locate all-operation measurement policy
      (decision) => decision.decisionId === "harness.M0.measurement",
    );
    m0.siteSelectors = [{
      cardinality: { maximum: 1, minimum: 1 },
      ownerSymbol: "measureAttempt",
      path: "fixture/measurement.js",
      selectorId: "decision.harness.m0.measurement.fixture-invalid",
      semanticRole: "measurement-call",
      syntaxKind: "javascript-symbol",
    }];
    const file = publicFile("fixture/measurement.js", "javascript", "measureAttempt();\n");
    const from = sourceSite(file, {
      ownerSymbol: "fixtureRoot",
      semanticRole: "operation-entry",
      syntaxKind: "operation-entry",
    });
    const endpoint = sourceSite(file, {
      endOffsetUtf16: 16,
      ownerSymbol: "measureAttempt",
      semanticRole: "measurement-call",
      startOffsetUtf16: 2,
      syntaxKind: "identifier",
    });
    const invalid = sourceEdge(from.siteId, endpoint.siteId, {
      kind: "measurement",
      operationIds: [],
    });
    const graph = await graphDocument({
      edges: [invalid],
      files: [file],
      sites: [from, endpoint],
    });
    const result = buildBackendReferenceTimeSourceProof(await proofInput({ graph, policy }));
    const coverage = measurementRecord(result, "harness.M0.measurement");

    assert.equal(coverage.status, "conflict");
    assert.deepEqual(coverage.invalidAttributionEdgeIds, [invalid.edgeId]);
    assert.deepEqual(coverage.attributions, []);
    assert.equal(result.rtU04.verdict, "FAIL");
    assert.equal(diagnosticCodes(result.rtU04).includes("binding.source-conflict"), true);
  },
);

test(
  "conjunctive consumer groups require every selector and retain legitimate multipaths",
  // verify selector-level reference coverage
  async () => {
    const policy = await checkedPolicy();
    const descriptorFile = publicFile(
      "fixture/reference.ts",
      "typescript",
      "export const reference = 1;\n",
    );
    const descriptorSelector = {
      cardinality: { maximum: 1, minimum: 1 },
      ownerSymbol: "readReference",
      path: descriptorFile.path,
      selectorId: "descriptor.fixture.reference-seed",
      semanticRole: "reference-seed",
      syntaxKind: "typescript-symbol",
    };
    policy.referenceFlowPolicy.descriptorSeedSelectors = [descriptorSelector];
    policy.prerequisitePolicy.referenceDescriptorSelectors = [{
      ...descriptorSelector,
      selectorId: "descriptor.fixture.reference-prerequisite",
    }];
    const seed = sourceSite(descriptorFile, {
      endOffsetUtf16: 8,
      ownerSymbol: descriptorSelector.ownerSymbol,
      semanticRole: "reference-seed",
      startOffsetUtf16: 1,
      syntaxKind: "call",
    });
    const files = [descriptorFile];
    const sites = [seed];
    const edgeById = new Map();
    const sitesByPredicate = new Map();

    // create every required selector site and direct path
    for (const group of policy.referenceFlowPolicy.sinkExpectationGroups) {
      // create each conjunctive selector independently
      for (const selector of group.selectors) {
        let file = files.find(
          // reuse one public file per path
          (candidate) => candidate.path === selector.path,
        );
        // add one missing public file
        if (file === undefined) {
          const language = selector.syntaxKind === "javascript-symbol"
            ? "javascript"
            : selector.syntaxKind === "sql-expression"
              ? "sql"
              : selector.syntaxKind === "shell-command"
                ? "shell"
                : selector.syntaxKind === "python-expression"
                  ? "python"
                  : "typescript";
          file = publicFile(selector.path, language, "x".repeat(4096));
          files.push(file);
        }
        const predicateKey = canonicalJson({
          ownerSymbol: selector.ownerSymbol,
          path: selector.path,
          semanticRole: selector.semanticRole,
          syntaxKind: selector.syntaxKind,
        });
        let matchedSites = sitesByPredicate.get(predicateKey);
        // create the exact selector cardinality once
        if (matchedSites === undefined) {
          matchedSites = [];
          const syntaxKind = selector.syntaxKind === "sql-expression"
            ? "sql-expression"
            : selector.syntaxKind === "shell-command"
              ? "command"
              : selector.syntaxKind === "branch-no-clock"
                ? "nonclock-evidence"
                : "call";
          // create every required physical site
          for (let index = 0; index < selector.cardinality.minimum; index += 1) {
            const offset = sites.length * 2;
            const site = sourceSite(file, {
              endOffsetUtf16: offset + 1,
              ownerSymbol: selector.ownerSymbol,
              semanticRole: selector.semanticRole,
              startOffsetUtf16: offset,
              syntaxKind,
            });
            matchedSites.push(site);
            sites.push(site);
          }
          sitesByPredicate.set(predicateKey, matchedSites);
        }
        // retain one classified path per matched site
        for (const site of matchedSites) {
          const edge = sourceEdge(seed.siteId, site.siteId, {
            kind: "data",
            operationIds: [group.consumerId],
          });
          edgeById.set(edge.edgeId, edge);
        }
      }
    }
    const edges = [...edgeById.values()];
    const graph = await graphDocument({
      edges: edges.toSorted(
        // preserve exact edge order
        (left, right) => left.edgeId.localeCompare(right.edgeId),
      ),
      files: files.toSorted(
        // preserve exact file order
        (left, right) => left.path.localeCompare(right.path),
      ),
      sites: sites.toSorted(
        // preserve exact site order
        (left, right) => left.path.localeCompare(right.path)
          || left.start.offsetUtf16 - right.start.offsetUtf16
          || left.end.offsetUtf16 - right.end.offsetUtf16
          || left.siteId.localeCompare(right.siteId),
      ),
    });
    const result = buildBackendReferenceTimeSourceProof(await proofInput({ graph, policy }));

    assert.equal(result.rtU16.descriptorSeedSiteIds.length, 1);
    assert.deepEqual(result.rtU16.consumerCoverage, {
      calendarAdmission: 1,
      covered: 52,
      dataReference: 51,
      required: 52,
    });
    assert.equal(result.rtU16.verdict, "PASS");
    assert.equal(result.rtU16.consumerResults.length, 52);
    assert.equal(
      result.rtU16.consumerResults.every(
        // require every consumer covered once
        (consumer) => consumer.status === "covered",
      ),
      true,
    );
    const selectorResults = result.rtU16.consumerResults.flatMap(
      // retain every consumer group
      (consumer) => consumer.groupResults.flatMap(
        // retain every selector result
        (group) => group.selectorResults,
      ),
    );
    assert.equal(selectorResults.length, 160);
    // validate every conjunctive selector result
    for (const selectorResult of selectorResults) {
      assertExactKeys(selectorResult, SELECTOR_RESULT_KEYS);
      assert.equal(selectorResult.status, "resolved");
      assert.equal(selectorResult.allowedPathCount >= 1, true);
      assert.deepEqual(selectorResult.missingSiteIds, []);
      assert.equal(selectorResult.forbiddenPathCount, 0);
      assert.equal(selectorResult.unknownPathCount, 0);
      assert.equal(selectorResult.unclassifiedPathCount, 0);
    }
    const calendar = result.rtU16.consumerResults.find(
      // locate the one calendar consumer
      (consumer) => consumer.referenceKind === "calendar-admission",
    );
    assert.deepEqual(
      // retain four ordered m4 classes
      calendar.groupResults.map((group) => group.requiredSinkClassId),
      policy.referenceFlowPolicy.m4SinkClassIds,
    );
  },
);

test(
  "one missing selector site cannot be covered by another allowed path",
  // verify conjunctive missing coverage
  async () => {
    const policy = await checkedPolicy();
    const existingGroupIndex = policy.referenceFlowPolicy.sinkExpectationGroups.findIndex(
      // locate one replaceable data group
      (candidate) => candidate.consumerId === "page.forecast"
        && candidate.requiredSinkClassId === null,
    );
    assert.notEqual(
      existingGroupIndex,
      -1,
      "fixture policy must contain a page.forecast data group",
    );
    const syntheticSelectors = [
      {
        cardinality: { maximum: 1, minimum: 1 },
        ownerSymbol: "firstReferenceSink",
        path: "fixture/conjunctive-first.ts",
        selectorId: "sink.fixture.conjunctive.first",
        semanticRole: "reference-sink",
        syntaxKind: "typescript-symbol",
      },
      {
        cardinality: { maximum: 1, minimum: 1 },
        ownerSymbol: "secondReferenceSink",
        path: "fixture/conjunctive-second.ts",
        selectorId: "sink.fixture.conjunctive.second",
        semanticRole: "reference-sink",
        syntaxKind: "typescript-symbol",
      },
    ];
    const syntheticSelectorIds = syntheticSelectors.map(
      // project one synthetic selector id
      (selector) => selector.selectorId,
    );
    assert.deepEqual(
      syntheticSelectorIds,
      [...syntheticSelectorIds].toSorted(),
      "fixture selector ids must use code-point order",
    );
    assert.equal(
      new Set(syntheticSelectorIds).size,
      syntheticSelectorIds.length,
      "fixture selector ids must be unique",
    );
    assert.notStrictEqual(
      syntheticSelectors[0].cardinality,
      syntheticSelectors[1].cardinality,
      "fixture selectors must own independent cardinalities",
    );
    const serializedPolicy = JSON.stringify(policy);
    // require globally new synthetic ids
    for (const syntheticId of [
      "reference-group.page.forecast.fixture.conjunctive",
      ...syntheticSelectorIds,
    ]) {
      assert.equal(
        serializedPolicy.includes(JSON.stringify(syntheticId)),
        false,
        `fixture policy must not already contain ${syntheticId}`,
      );
    }
    const group = {
      consumerId: "page.forecast",
      groupId: "reference-group.page.forecast.fixture.conjunctive",
      matchMode: "all",
      requiredSinkClassId: null,
      selectors: syntheticSelectors,
    };
    policy.referenceFlowPolicy.sinkExpectationGroups[existingGroupIndex] = group;
    const descriptorFile = publicFile("fixture/reference.ts");
    const descriptorSelector = {
      cardinality: { maximum: 1, minimum: 1 },
      ownerSymbol: "readReference",
      path: descriptorFile.path,
      selectorId: "descriptor.fixture.partial",
      semanticRole: "reference-seed",
      syntaxKind: "typescript-symbol",
    };
    policy.referenceFlowPolicy.descriptorSeedSelectors = [descriptorSelector];
    policy.prerequisitePolicy.referenceDescriptorSelectors = [{
      ...descriptorSelector,
      selectorId: "descriptor.fixture.partial-prerequisite",
    }];
    const seed = sourceSite(descriptorFile, {
      ownerSymbol: descriptorSelector.ownerSymbol,
      semanticRole: "reference-seed",
      syntaxKind: "call",
    });
    const selector = group.selectors[0];
    assert.equal(
      selector.syntaxKind,
      "typescript-symbol",
      "fixture sink selector must require a TypeScript symbol",
    );
    const sinkFile = publicFile(
      selector.path,
      "typescript",
      "firstReferenceSink();\n",
    );
    const sink = sourceSite(sinkFile, {
      endOffsetUtf16: 2,
      ownerSymbol: selector.ownerSymbol,
      semanticRole: selector.semanticRole,
      startOffsetUtf16: 1,
      syntaxKind: "call",
    });
    assert.equal(sinkFile.language, "typescript");
    assert.equal(sink.ownerSymbol, selector.ownerSymbol);
    assert.equal(sink.semanticRole, selector.semanticRole);
    assert.equal(sink.syntaxKind, "call");
    const graph = await graphDocument({
      edges: [sourceEdge(seed.siteId, sink.siteId, {
        operationIds: [group.consumerId],
      })],
      files: [descriptorFile, sinkFile].toSorted(
        // retain file order
        (left, right) => left.path.localeCompare(right.path),
      ),
      sites: [seed, sink].toSorted(
        // retain site order
        (left, right) => left.path.localeCompare(right.path),
      ),
    });
    const result = buildBackendReferenceTimeSourceProof(await proofInput({ graph, policy }));
    const consumer = result.rtU16.consumerResults.find(
      // locate the partially covered consumer
      (candidate) => candidate.consumerId === group.consumerId,
    );
    const groupResult = consumer.groupResults.find(
      // locate the partially covered group
      (candidate) => candidate.groupId === group.groupId,
    );

    assert.notEqual(groupResult.status, "resolved");
    assert.equal(groupResult.selectorResults.length, 2);
    assert.deepEqual(
      groupResult.selectorResults.map(
        // retain ordered result selector ids
        (selectorResult) => selectorResult.selectorId,
      ),
      group.selectors.map(
        // retain ordered policy selector ids
        (selector) => selector.selectorId,
      ),
    );
    assert.deepEqual(
      groupResult.selectorResults.map(
        // retain ordered selector statuses
        (selectorResult) => selectorResult.status,
      ),
      ["resolved", "missing"],
    );
    assert.equal(consumer.status, "uncovered");
    assert.equal(result.rtU16.verdict, "INCOMPLETE");
  },
);

test(
  "conditional dataAt separation stays distinct from descriptor lineage",
  // verify lemma and descriptor separation
  async () => {
    const file = publicFile(
      "apps/api/src/index.ts",
      "typescript",
      "export function createWeatherApi(dataAt, safetyAt) { return dataAt; }\n",
    );
    const dataAt = sourceSite(file, {
      endOffsetUtf16: 39,
      ownerSymbol: "createWeatherApi",
      semanticRole: "reference-seed",
      startOffsetUtf16: 33,
      syntaxKind: "parameter",
    });
    const sink = sourceSite(file, {
      endOffsetUtf16: 66,
      ownerSymbol: "createWeatherApi",
      semanticRole: "reference-sink",
      startOffsetUtf16: 60,
      syntaxKind: "return",
    });
    const safety = sourceSite(file, {
      endOffsetUtf16: 49,
      ownerSymbol: "createWeatherApi",
      semanticRole: "fixed-input",
      startOffsetUtf16: 41,
      syntaxKind: "parameter",
    });
    const graph = await graphDocument({
      edges: [sourceEdge(dataAt.siteId, sink.siteId)],
      files: [file],
      sites: [dataAt, safety, sink],
    });
    const result = buildBackendReferenceTimeSourceProof(await proofInput({ graph }));

    assert.equal(result.conditionalSeam.verdict, "PASS");
    assert.equal(result.conditionalSeam.inputSourceSiteId, dataAt.siteId);
    assert.deepEqual(result.conditionalSeam.allowedDataSinkSiteIds, [sink.siteId]);
    assert.deepEqual(result.conditionalSeam.safetyNonReachabilitySiteIds, [safety.siteId]);
    assert.equal(result.rtU16.verdict, "INCOMPLETE");
    assert.deepEqual(result.rtU16.descriptorSeedSiteIds, []);
    assert.equal(result.rtU16.consumerCoverage.covered, 0);
    assert.equal(
      result.rtU16.conditionalSeamLemmaSha256,
      documentSha256(result.conditionalSeam),
    );
  },
);

test(
  "a dataAt path into safety fails the lemma beside two incomplete global proofs",
  // verify aggregate-precedence child inputs
  async () => {
    const file = publicFile(
      "apps/api/src/index.ts",
      "typescript",
      "export function createWeatherApi(dataAt, safetyAt) { return safetyAt; }\n",
    );
    const dataAt = sourceSite(file, {
      endOffsetUtf16: 39,
      ownerSymbol: "createWeatherApi",
      semanticRole: "reference-seed",
      startOffsetUtf16: 33,
      syntaxKind: "parameter",
    });
    const safety = sourceSite(file, {
      endOffsetUtf16: 49,
      ownerSymbol: "createWeatherApi",
      semanticRole: "fixed-input",
      startOffsetUtf16: 41,
      syntaxKind: "parameter",
    });
    const graph = await graphDocument({
      edges: [sourceEdge(dataAt.siteId, safety.siteId)],
      files: [file],
      sites: [dataAt, safety],
    });
    const result = buildBackendReferenceTimeSourceProof(await proofInput({ graph }));
    assert.equal(result.conditionalSeam.verdict, "FAIL");
    assert.equal(result.rtU04.verdict, "INCOMPLETE");
    assert.equal(result.rtU16.verdict, "INCOMPLETE");
    assert.equal(
      diagnosticCodes(result.conditionalSeam).includes("reference.forbidden-flow"),
      true,
    );
  },
);
