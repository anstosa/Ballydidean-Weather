import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFile } from "node:fs/promises";
import test from "node:test";

import { extractBackendClockOccurrences } from "./backend-clock-occurrences.mjs";
import { buildBackendOperationSourceGraph } from "./backend-operation-source-graph.mjs";

const GRAPH_KEYS = [
  "analysisProfiles",
  "contractVersion",
  "diagnostics",
  "edges",
  "files",
  "fixedPoint",
  "operations",
  "readRequests",
  "roots",
  "sites",
  "status",
];
const PUBLIC_FILE_KEYS = [
  "byteLength",
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
];
const SITE_KEYS = [
  "end",
  "fileSha256",
  "ownerSymbol",
  "path",
  "semanticRole",
  "siteId",
  "start",
  "syntaxKind",
];
const EDGE_KEYS = [
  "edgeId",
  "effect",
  "fromSiteId",
  "kind",
  "operationIds",
  "status",
  "toSiteId",
];
const REQUEST_KEYS = [
  "expectedLanguage",
  "kind",
  "originId",
  "path",
  "reason",
  "requestId",
  "required",
];
const ROOT_KEYS = [
  "entrySiteId",
  "operationIds",
  "phase",
  "rootId",
  "rootKind",
  "seedId",
  "selectorIds",
  "status",
  "topologyRole",
];
const OPERATION_KEYS = [
  "entryRootIds",
  "measurementRootIds",
  "operationId",
  "workloadOrder",
];

let repositoryInputsPromise;

// hash exact fixture bytes
function sha256(value) {
  return createHash("sha256").update(value).digest("hex");
}

// canonicalize simple framed payloads
function canonicalJson(value) {
  // retain json scalar spelling
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

// read one checked repository json input
async function readRepositoryJson(relativePath) {
  return JSON.parse(await readFile(new URL(`../${relativePath}`, import.meta.url), "utf8"));
}

// load immutable checked graph prerequisites
async function repositoryInputs() {
  // retain one shared read-only load
  if (repositoryInputsPromise === undefined) {
    repositoryInputsPromise = Promise.all([
      readRepositoryJson("config/backend-reference-time-source-policy-v1.json"),
      readRepositoryJson("config/backend-runtime-inventory.json"),
      readRepositoryJson("config/backend-runtime-live-map.json"),
      readRepositoryJson("config/backend-performance-workload-v2.json"),
    ]);
  }
  const [policy, runtimeInventory, runtimeLiveMap, workload] = await repositoryInputsPromise;
  return structuredClone({ policy, runtimeInventory, runtimeLiveMap, workload });
}

// build one physical private source
function physicalSource(language, path, sourceText, origins = ["local-module"]) {
  const separator = path.lastIndexOf("/");
  return {
    containerSpan: null,
    derivedId: null,
    language,
    origins: [...origins].sort(),
    parentPath: null,
    parentRawSha256: null,
    path,
    rawSha256: sha256(Buffer.from(sourceText, "utf8")),
    resolutionBase: separator === -1 ? "" : path.slice(0, separator),
    sourceKind: "physical",
    sourceText,
  };
}

// build one derived private source
function derivedSource(parent, sourceText, containerSpan = { endUtf16: 12, startUtf16: 0 }) {
  const rawSha256 = sha256(Buffer.from(sourceText, "utf8"));
  const payload = {
    containerSpan,
    language: "javascript",
    parentPath: parent.path,
    parentRawSha256: parent.rawSha256,
    rawSha256,
    resolutionBase: parent.resolutionBase,
  };
  const derivedId = frame("weather-backend-derived-source/v1", payload);
  return {
    containerSpan,
    derivedId,
    language: "javascript",
    origins: ["embedded-javascript"],
    parentPath: parent.path,
    parentRawSha256: parent.rawSha256,
    path: `derived:${derivedId}`,
    rawSha256,
    resolutionBase: parent.resolutionBase,
    sourceKind: "derived",
    sourceText,
  };
}

// project one frozen v1 occurrence
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

// extract one typescript occurrence union
function typescriptOccurrences(path, sourceText) {
  return extractBackendClockOccurrences({ sourcePath: path, sourceText }).occurrences.map(
    // preserve every v1 record byte
    (record) => projectV1(record),
  );
}

// build one graph input
async function graphInput(overrides = {}) {
  const prerequisites = await repositoryInputs();
  return {
    analysisProfiles: [],
    occurrences: [],
    policy: prerequisites.policy,
    privateSources: [],
    runtimeInventory: prerequisites.runtimeInventory,
    runtimeLiveMap: prerequisites.runtimeLiveMap,
    semanticFacts: [],
    workload: prerequisites.workload,
    ...overrides,
  };
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

// select one exact diagnostic code
function hasDiagnostic(graph, code) {
  return graph.diagnostics.some(
    // match one closed code
    (diagnostic) => diagnostic.code === code,
  );
}

// select graph edges by kind
function edgesOf(graph, kind) {
  return graph.edges.filter(
    // retain one closed edge kind
    (edge) => edge.kind === kind,
  );
}

test(
  "buildBackendOperationSourceGraph is synchronous pure and exact at its boundary",
  // verify the closed graph boundary
  async () => {
    const input = await graphInput();
    const before = structuredClone(input);
    const graph = buildBackendOperationSourceGraph(input);

    assert.equal(graph instanceof Promise, false);
    assert.deepEqual(input, before);
    assertExactKeys(graph, GRAPH_KEYS);
    assert.equal(graph.contractVersion, "weather-backend-operation-source-graph/v1");
    assert.equal(graph.fixedPoint.roundCount, 0);
    assertDeepFrozen(graph);

    assert.throws(
      // reject a missing input
      () => buildBackendOperationSourceGraph(),
      TypeError,
    );
    assert.throws(
      // reject a caller verdict
      () => buildBackendOperationSourceGraph({ ...input, verdict: "PASS" }),
      TypeError,
    );
    assert.throws(
      // reject a missing explicit accumulator
      () => {
        const missing = { ...input };
        // omit one required field
        delete missing.semanticFacts;
        return buildBackendOperationSourceGraph(missing);
      },
      TypeError,
    );

    let getterRead = false;
    const accessorInput = { ...input };
    // replace one field with an accessor
    delete accessorInput.policy;
    Object.defineProperty(accessorInput, "policy", {
      enumerable: true,
      // detect forbidden accessor evaluation
      get() {
        getterRead = true;
        return input.policy;
      },
    });
    assert.throws(
      // reject accessor-bearing inputs
      () => buildBackendOperationSourceGraph(accessorInput),
      TypeError,
    );
    assert.equal(getterRead, false);
  },
);

test(
  "public graph DTOs redact private source text facts and literal values",
  // verify the public privacy boundary
  async () => {
    const sentinel = "private-graph-sentinel-830bc1";
    const source = physicalSource(
      "typescript",
      "apps/api/src/main.ts",
      `const secret = ${JSON.stringify(sentinel)}; export const value = Date.now();\n`,
      ["prerequisite"],
    );
    const graph = buildBackendOperationSourceGraph(await graphInput({
      occurrences: typescriptOccurrences(source.path, source.sourceText),
      privateSources: [source],
    }));

    assert.equal(graph.files.length, 1);
    assertExactKeys(graph.files[0], PUBLIC_FILE_KEYS);
    assert.equal(Object.hasOwn(graph.files[0], "sourceText"), false);
    assert.equal(JSON.stringify(graph).includes(sentinel), false);
    assert.equal(JSON.stringify(graph).includes("factId"), false);
    assert.equal(JSON.stringify(graph).includes("literalValue"), false);
    assert.equal(graph.files[0].rawSha256, source.rawSha256);
    assert.equal(graph.files[0].byteLength, Buffer.byteLength(source.sourceText));

    // validate every emitted public site
    for (const site of graph.sites) {
      assertExactKeys(site, SITE_KEYS);
    }
    // validate every emitted public edge
    for (const edge of graph.edges) {
      assertExactKeys(edge, EDGE_KEYS);
    }
    // validate every emitted public root
    for (const root of graph.roots) {
      assertExactKeys(root, ROOT_KEYS);
    }
    // validate every operation row
    for (const operation of graph.operations) {
      assertExactKeys(operation, OPERATION_KEYS);
    }
  },
);

test(
  "literal local imports yield sorted physical typed reads without hidden I/O",
  // verify typed fixed-point requests
  async () => {
    const source = physicalSource(
      "typescript",
      "apps/api/src/main.ts",
      [
        "import './alpha.ts';",
        "const beta = require('./beta.ts');",
        "void import('./gamma.ts');",
        "void import(`./${name}.ts`);",
        "export { beta };",
        "",
      ].join("\n"),
      ["prerequisite"],
    );
    const graph = buildBackendOperationSourceGraph(await graphInput({
      privateSources: [source],
    }));
    const localRequests = graph.readRequests.filter(
      // retain local module requests
      (request) => request.kind === "local-module",
    );

    assert.deepEqual(
      // retain requested paths
      localRequests.map((request) => request.path),
      [
        "apps/api/src/alpha.ts",
        "apps/api/src/beta.ts",
        "apps/api/src/gamma.ts",
      ],
    );
    // validate every typed request
    for (const request of localRequests) {
      assertExactKeys(request, REQUEST_KEYS);
      assert.equal(request.path.startsWith("derived:"), false);
      assert.equal(request.required, true);
      assert.match(request.requestId, /^[a-z0-9._:-]{1,192}$/u);
    }
    assert.equal(hasDiagnostic(graph, "reader.dynamic-unresolved"), true);
  },
);

test(
  "derived JavaScript remains in memory while literal imports request physical files",
  // verify derived source handling
  async () => {
    const parent = physicalSource(
      "python",
      "scripts/research/fixture.py",
      "bridge = 'private'\n",
      ["runtime-opened-code"],
    );
    const derived = derivedSource(
      parent,
      "import './bridge-helper.mjs'; export const clock = Date.now();\n",
    );
    const graph = buildBackendOperationSourceGraph(await graphInput({
      privateSources: [parent, derived],
    }));

    assert.equal(
      graph.files.some(
        // retain the derived public file
        (file) => file.path === derived.path && file.sourceKind === "derived",
      ),
      true,
    );
    assert.equal(
      graph.readRequests.some(
        // forbid a derived filesystem request
        (request) => request.path.startsWith("derived:"),
      ),
      false,
    );
    assert.equal(
      graph.readRequests.some(
        // retain the resolved physical import
        (request) => request.path === "scripts/research/bridge-helper.mjs",
      ),
      true,
    );

    const deduplicated = buildBackendOperationSourceGraph(await graphInput({
      privateSources: [parent, derived, structuredClone(derived)],
    }));
    assert.equal(
      deduplicated.files.filter(
        // retain the identical derived identity
        (file) => file.path === derived.path,
      ).length,
      1,
    );

    const conflicting = {
      ...derived,
      rawSha256: sha256(Buffer.from(`${derived.sourceText} `, "utf8")),
      sourceText: `${derived.sourceText} `,
    };
    const conflictGraph = buildBackendOperationSourceGraph(await graphInput({
      privateSources: [parent, derived, conflicting],
    }));
    assert.equal(conflictGraph.status, "failed");
    assert.equal(hasDiagnostic(conflictGraph, "binding.source-conflict"), true);
  },
);

test(
  "import and call SCCs retain aliases parameters returns branches and unknown effects",
  // verify fixed-point graph semantics
  async () => {
    const sources = [
      physicalSource(
        "typescript",
        "apps/api/src/main.ts",
        "import { first } from './fixture-a.ts'; export const result = first(Date.now());\n",
        ["prerequisite"],
      ),
      physicalSource(
        "typescript",
        "apps/api/src/fixture-a.ts",
        [
          "import { second } from './fixture-b.ts';",
          "export function first(value: number) {",
          "  const alias = second;",
          "  return value > 0 ? alias(value) : 0;",
          "}",
          "",
        ].join("\n"),
      ),
      physicalSource(
        "typescript",
        "apps/api/src/fixture-b.ts",
        [
          "import { first } from './fixture-a.ts';",
          "export function second(value: number) {",
          "  return value > 1 ? first(value - 1) : value;",
          "}",
          "export const unknown = externalValue[computedMember](value);",
          "",
        ].join("\n"),
      ),
    ];
    const graph = buildBackendOperationSourceGraph(await graphInput({
      occurrences: sources.flatMap(
        // extract every fixture source occurrence
        (source) => typescriptOccurrences(source.path, source.sourceText),
      ),
      privateSources: sources,
    }));

    assert.equal(edgesOf(graph, "import").length >= 3, true);
    assert.equal(edgesOf(graph, "call").length >= 3, true);
    assert.equal(edgesOf(graph, "parameter").length > 0, true);
    assert.equal(edgesOf(graph, "return").length > 0, true);
    assert.equal(edgesOf(graph, "alias").length > 0, true);
    assert.equal(edgesOf(graph, "control").length > 0, true);
    assert.equal(edgesOf(graph, "data").length > 0, true);
    assert.equal(
      graph.edges.some(
        // retain an unresolved computed effect
        (edge) => edge.effect === "opaque-unknown" && edge.status === "unknown",
      ),
      true,
    );
    assert.equal(hasDiagnostic(graph, "effect.unknown"), true);
  },
);

test(
  "a genuine guarded main clock remains operation-unreachable rather than nonclock",
  // verify negative reachability truth
  async () => {
    const root = physicalSource(
      "typescript",
      "apps/api/src/main.ts",
      "import './unreachable-clock.ts'; export const ready = true;\n",
      ["prerequisite"],
    );
    const guarded = physicalSource(
      "typescript",
      "apps/api/src/unreachable-clock.ts",
      [
        "function main() {",
        "  return Date.now();",
        "}",
        "if (import.meta.url === process.argv[1]) {",
        "  main();",
        "}",
        "",
      ].join("\n"),
    );
    const occurrence = typescriptOccurrences(guarded.path, guarded.sourceText)[0];
    const graph = buildBackendOperationSourceGraph(await graphInput({
      occurrences: [occurrence],
      privateSources: [root, guarded],
    }));
    const clockSite = graph.sites.find(
      // locate the occurrence-backed provider site
      (site) => site.path === guarded.path
        && site.start.offsetUtf16 === occurrence.span.startOffsetUtf16,
    );
    assert.notEqual(clockSite, undefined);
    const guardedSiteIds = new Set(graph.sites.filter(
      // retain sites in the guarded module
      (site) => site.path === guarded.path,
    ).map(
      // retain one guarded site identity
      (site) => site.siteId,
    ));
    assert.equal(
      graph.edges.some(
        // retain negative guarded reachability
        (edge) => edge.status === "operation-unreachable"
          && (guardedSiteIds.has(edge.fromSiteId) || guardedSiteIds.has(edge.toSiteId)),
      ),
      true,
    );
    assert.equal(clockSite.semanticRole, "provider-read");
    assert.notEqual(clockSite.semanticRole, "fixed-input");
  },
);

test(
  "operation records preserve all workload roots and partial measurement multipaths",
  // verify root and operation ownership
  async () => {
    const graph = buildBackendOperationSourceGraph(await graphInput());
    assert.equal(graph.operations.length, 107);
    assert.deepEqual(
      // preserve workload operation order
      graph.operations.map((operation) => operation.operationId),
      graph.operations.toSorted(
        // compare by declared workload order
        (left, right) => left.workloadOrder - right.workloadOrder,
      ).map(
        // retain one operation identity
        (operation) => operation.operationId,
      ),
    );
    assert.equal(
      graph.operations.every(
        // forbid provisional or seed identities
        (operation) => [...operation.entryRootIds, ...operation.measurementRootIds].every(
          // reject internal provisional IDs
          (rootId) => !rootId.includes("provisional") && !rootId.startsWith("measurement."),
        ),
      ),
      true,
    );
    assert.equal(
      graph.roots.filter(
        // retain unresolved measurement origins
        (root) => root.rootKind === "measurement-harness"
          && root.status === "unresolved-prerequisite",
      ).length,
      4,
    );
  },
);

test(
  "graph caps fail closed without truncating files or diagnostics",
  // verify bounded explicit inputs
  async () => {
    const sources = [];
    // exceed the physical file cap by one
    for (let index = 0; index < 513; index += 1) {
      sources.push(physicalSource(
        "data",
        `fixture/cap/${String(index).padStart(3, "0")}.txt`,
        "x",
        ["runtime-data"],
      ));
    }
    const graph = buildBackendOperationSourceGraph(await graphInput({
      privateSources: sources,
    }));
    assert.equal(graph.status, "failed");
    assert.equal(hasDiagnostic(graph, "reader.bound-exceeded"), true);
    assert.equal(graph.files.length, 0);
  },
);

test(
  "graph construction never depends on cwd ambient modules or omx artifacts",
  // verify clean-checkout purity
  async () => {
    const source = physicalSource(
      "typescript",
      "apps/api/src/main.ts",
      "import ambient from 'not-installed-anywhere'; export default ambient;\n",
      ["prerequisite"],
    );
    const previousCwd = process.cwd();
    process.chdir("/");
    // always restore process cwd
    try {
      const graph = buildBackendOperationSourceGraph(await graphInput({
        privateSources: [source],
      }));
      assert.equal(hasDiagnostic(graph, "effect.unknown"), true);
      assert.equal(JSON.stringify(graph).includes(".omx"), false);
      assert.equal(
        graph.readRequests.some(
          // forbid ambient package reads
          (request) => request.path.includes("node_modules"),
        ),
        false,
      );
    } finally {
      process.chdir(previousCwd);
    }
  },
);
