import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import test from "node:test";

import * as reportModule from "./backend-clock-catalog-report.mjs";

const {
  BACKEND_CLOCK_CATALOG_REPORT_VERSION,
  buildBackendClockCatalogReport,
} = reportModule;

const REPOSITORY_ROOT = fileURLToPath(new URL("..", import.meta.url));
const REPORT_CLI = fileURLToPath(
  new URL("./backend-clock-catalog-report.mjs", import.meta.url),
);
const TOP_LEVEL_KEYS = [
  "contractVersion",
  "reportKind",
  "status",
  "validationVerdict",
  "inputs",
  "scope",
  "operationRootHints",
  "files",
  "exclusions",
  "occurrences",
  "diagnostics",
  "gaps",
  "canonicalEligible",
  "referenceFlowProven",
  "physicalCoverageProven",
  "operationReachabilityProven",
  "readyForMetrics",
];
const FILE_KEYS = [
  "sourcePath",
  "origins",
  "inventoryClassifications",
  "language",
  "byteLength",
  "sha256",
  "coverage",
  "occurrenceCount",
  "diagnosticCount",
];
const EXCLUSION_KEYS = [
  "sourcePath",
  "origin",
  "inventoryClassifications",
  "reason",
  "readAttempted",
];
const OPERATION_KEYS = [
  "operationId",
  "workloadOrder",
  "transports",
  "httpRootStatus",
  "nodeRootStatus",
  "nodeSourcePaths",
  "branchReachabilityProven",
];
const GAP_KEYS = ["code", "countBasis", "count", "affectedIds"];
const NESTED_APPLICATION_PATHS = [
  "scripts/research/adjustment-maintenance-runtime/node_modules/@weather/domain/dist/forecast-adjustment.js",
  "scripts/research/adjustment-maintenance-runtime/node_modules/@weather/domain/dist/forecast-anchor-record.js",
  "scripts/research/adjustment-maintenance-runtime/node_modules/@weather/domain/dist/index.js",
  "scripts/research/adjustment-maintenance-runtime/node_modules/@weather/domain/dist/ingestion.js",
  "scripts/research/adjustment-maintenance-runtime/node_modules/@weather/domain/dist/provenance.js",
  "scripts/research/adjustment-maintenance-runtime/node_modules/@weather/domain/dist/rain-collection.js",
  "scripts/research/adjustment-maintenance-runtime/node_modules/@weather/domain/dist/weather-record.js",
];
const DIRECT_OPERATION_ROOTS = [
  "apps/worker/src/forecast-adjustment-maintenance-cli.ts",
  "apps/worker/src/forecast-adjustment-wind-refresh-cli.ts",
];
const SUPPLEMENTAL_ROOTS = [
  "scripts/backend-host-observation.mjs",
  "scripts/backend-performance-receipts.mjs",
  "scripts/backend-performance-resources.mjs",
  "scripts/database-page-performance.mjs",
];
const UNCONDITIONAL_GAPS = [
  "catalog.binding-not-performed",
  "reference-flow.unproven",
  "physical-coverage.unproven",
  "operation-reachability.unproven",
  "raw-sql.roots-unmapped",
  "runtime-opened-sources.unmapped",
  "embedded-sql.context-policy-limited",
  "metrics.not-ready",
];
const GAP_BASES = new Map([
  ["catalog.binding-not-performed", "report"],
  ["reference-flow.unproven", "report"],
  ["physical-coverage.unproven", "report"],
  ["operation-reachability.unproven", "report"],
  ["http.handler-roots-unmapped", "operation-id"],
  ["node.branch-reachability-unproven", "operation-id"],
  ["node.direct-root-import-closure-missing", "source-path"],
  ["node.root-unresolved", "operation-id"],
  ["source.unsupported-python", "source-path"],
  ["source.unsupported-shell", "source-path"],
  ["source.unsupported-operational-declaration", "source-path"],
  ["source.configuration-noncode", "source-path"],
  ["source.unsupported-extension", "source-path"],
  ["source.sensitive-or-forbidden-root", "source-path"],
  ["raw-sql.roots-unmapped", "report"],
  ["runtime-opened-sources.unmapped", "report"],
  ["embedded-sql.context-policy-limited", "report"],
  ["dynamic-code.unresolved", "source-path"],
  ["sql.dynamic-fragments-unresolved", "source-path"],
  ["sql.dollar-bodies-unresolved", "occurrence-id"],
  ["sql.typed-current-contexts-unresolved", "source-path"],
  ["source.parse-incomplete", "source-path"],
  ["source.sql-lex-incomplete", "source-path"],
  ["metrics.not-ready", "report"],
]);

let cachedReportPromise;

// build one shared actual-repository report
function actualReport() {
  // retain one expensive read-only build
  if (cachedReportPromise === undefined) {
    cachedReportPromise = buildBackendClockCatalogReport();
  }
  return cachedReportPromise;
}

// require exact key order
function assertExactKeys(value, keys) {
  assert.deepEqual(Object.keys(value), keys);
}

// require recursive immutable values
function assertDeepFrozen(value, seen = new Set()) {
  // stop at primitives and repeated values
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

// require lexical uniqueness
function assertSortedUnique(values) {
  assert.deepEqual(values, [...new Set(values)].sort());
}

// retain sorted unique projected values
function sortedUnique(values) {
  return [...new Set(values)].sort();
}

// run only the known report command
function runReportCli(argumentsList, nodeArguments = []) {
  return spawnSync(
    process.execPath,
    [...nodeArguments, REPORT_CLI, ...argumentsList],
    {
      cwd: REPOSITORY_ROOT,
      encoding: "utf8",
      maxBuffer: 64 * 1024 * 1024,
    },
  );
}

// select one required file row
function fileRow(report, sourcePath) {
  const row = report.files.find(
    // match one source path
    (candidate) => candidate.sourcePath === sourcePath,
  );
  assert.notEqual(row, undefined, sourcePath);
  return row;
}

// select one required gap row
function gapRow(report, code) {
  const row = report.gaps.find(
    // match one gap code
    (candidate) => candidate.code === code,
  );
  assert.notEqual(row, undefined, code);
  return row;
}

// derive every conditional affected set independently
function expectedConditionalGaps(report) {
  const operations = report.operationRootHints.operations;
  const diagnostics = report.diagnostics;
  const exclusions = report.exclusions;
  const files = report.files;
  return new Map([
    [
      "http.handler-roots-unmapped",
      sortedUnique(operations.filter(
        // retain unmapped http operations
        (operation) => operation.httpRootStatus === "unmapped-handler",
      ).map(
        // retain operation identity
        (operation) => operation.operationId,
      )),
    ],
    [
      "node.branch-reachability-unproven",
      sortedUnique(operations.filter(
        // retain node transports
        (operation) => operation.transports.includes("node-argv"),
      ).map(
        // retain operation identity
        (operation) => operation.operationId,
      )),
    ],
    [
      "node.direct-root-import-closure-missing",
      sortedUnique(operations.filter(
        // retain direct roots
        (operation) => operation.nodeRootStatus === "direct-operation-root",
      ).flatMap(
        // retain direct source paths
        (operation) => operation.nodeSourcePaths,
      )),
    ],
    [
      "node.root-unresolved",
      sortedUnique(operations.filter(
        // retain unresolved node roots
        (operation) => operation.nodeRootStatus === "unresolved",
      ).map(
        // retain operation identity
        (operation) => operation.operationId,
      )),
    ],
    [
      "source.unsupported-python",
      sortedUnique(exclusions.filter(
        // retain python sources
        (exclusion) => exclusion.sourcePath.endsWith(".py"),
      ).map(
        // retain excluded path
        (exclusion) => exclusion.sourcePath,
      )),
    ],
    [
      "source.unsupported-shell",
      sortedUnique(exclusions.filter(
        // retain shell sources
        (exclusion) => exclusion.sourcePath.endsWith(".sh"),
      ).map(
        // retain excluded path
        (exclusion) => exclusion.sourcePath,
      )),
    ],
    [
      "source.unsupported-operational-declaration",
      sortedUnique(exclusions.filter(
        // retain operational declarations
        (exclusion) => exclusion.reason === "unsupported-operational-declaration",
      ).map(
        // retain excluded path
        (exclusion) => exclusion.sourcePath,
      )),
    ],
    [
      "source.configuration-noncode",
      sortedUnique(exclusions.filter(
        // retain noncode configuration
        (exclusion) => exclusion.reason === "configuration-noncode",
      ).map(
        // retain excluded path
        (exclusion) => exclusion.sourcePath,
      )),
    ],
    [
      "source.unsupported-extension",
      sortedUnique(exclusions.filter(
        // retain unsupported extensions
        (exclusion) => exclusion.reason === "unsupported-extension",
      ).map(
        // retain excluded path
        (exclusion) => exclusion.sourcePath,
      )),
    ],
    [
      "source.sensitive-or-forbidden-root",
      sortedUnique(exclusions.filter(
        // retain forbidden roots
        (exclusion) => exclusion.reason === "sensitive-or-forbidden-root",
      ).map(
        // retain excluded path
        (exclusion) => exclusion.sourcePath,
      )),
    ],
    [
      "dynamic-code.unresolved",
      sortedUnique(diagnostics.filter(
        // retain dynamic code gaps
        (diagnostic) => diagnostic.code === "javascript.dynamic-code-unresolved",
      ).map(
        // retain diagnostic path
        (diagnostic) => diagnostic.sourcePath,
      )),
    ],
    [
      "sql.dynamic-fragments-unresolved",
      sortedUnique(diagnostics.filter(
        // retain dynamic sql gaps
        (diagnostic) => diagnostic.code === "sql.dynamic-fragment-unresolved",
      ).map(
        // retain diagnostic path
        (diagnostic) => diagnostic.sourcePath,
      )),
    ],
    [
      "sql.dollar-bodies-unresolved",
      sortedUnique(report.occurrences.filter(
        // retain unresolved dollar bodies
        (occurrence) => occurrence.surface === "postgres.dollar-quoted-body",
      ).map(
        // retain occurrence identity
        (occurrence) => occurrence.id,
      )),
    ],
    [
      "sql.typed-current-contexts-unresolved",
      sortedUnique(diagnostics.filter(
        // retain typed current context gaps
        (diagnostic) => diagnostic.code === "sql.typed-current-context-unresolved",
      ).map(
        // retain diagnostic path
        (diagnostic) => diagnostic.sourcePath,
      )),
    ],
    [
      "source.parse-incomplete",
      sortedUnique(files.filter(
        // retain incomplete source parses
        (file) => file.coverage.sourceParseCompleted === false,
      ).map(
        // retain incomplete file path
        (file) => file.sourcePath,
      )),
    ],
    [
      "source.sql-lex-incomplete",
      sortedUnique(files.filter(
        // retain incomplete sql lexing
        (file) => file.coverage.sqlLexingCompleted === false,
      ).map(
        // retain incomplete file path
        (file) => file.sourcePath,
      )),
    ],
  ]);
}

test(
  "exports only the fixed zero-argument report API",
  // verify the closed public boundary
  async () => {
    assert.deepEqual(Object.keys(reportModule), [
      "BACKEND_CLOCK_CATALOG_REPORT_VERSION",
      "buildBackendClockCatalogReport",
    ]);
    assert.equal(
      BACKEND_CLOCK_CATALOG_REPORT_VERSION,
      "weather-backend-clock-catalog-gap-report/v1",
    );
    assert.equal(typeof buildBackendClockCatalogReport, "function");
    await assert.rejects(
      // reject a caller-supplied root or option
      () => buildBackendClockCatalogReport({ rootDir: REPOSITORY_ROOT }),
      TypeError,
    );
  },
);

test(
  "returns the exact frozen visibly incomplete report contract",
  // verify nonauthority and input contracts
  async () => {
    const report = await actualReport();
    assertExactKeys(report, TOP_LEVEL_KEYS);
    assert.equal(report.contractVersion, BACKEND_CLOCK_CATALOG_REPORT_VERSION);
    assert.equal(report.reportKind, "repository-source-clock-gap-report");
    assert.equal(report.status, "incomplete");
    assert.equal(report.validationVerdict, "not-evaluated");
    assert.equal(report.canonicalEligible, false);
    assert.equal(report.referenceFlowProven, false);
    assert.equal(report.physicalCoverageProven, false);
    assert.equal(report.operationReachabilityProven, false);
    assert.equal(report.readyForMetrics, false);
    assertExactKeys(report.inputs, ["inventory", "workload"]);
    assertExactKeys(report.inputs.inventory, [
      "generation",
      "phase",
      "manifestSha256",
      "runtimeMapSha256",
      "entryCount",
      "artifactCount",
      "bindingCount",
      "contextCount",
    ]);
    assert.deepEqual(
      {
        generation: report.inputs.inventory.generation,
        phase: report.inputs.inventory.phase,
        entryCount: report.inputs.inventory.entryCount,
        artifactCount: report.inputs.inventory.artifactCount,
        bindingCount: report.inputs.inventory.bindingCount,
        contextCount: report.inputs.inventory.contextCount,
      },
      {
        generation: "node-v1",
        phase: "baseline",
        entryCount: 437,
        artifactCount: 499,
        bindingCount: 2040,
        contextCount: 174,
      },
    );
    assert.match(report.inputs.inventory.manifestSha256, /^[0-9a-f]{64}$/);
    assert.match(report.inputs.inventory.runtimeMapSha256, /^[0-9a-f]{64}$/);
    assertExactKeys(report.inputs.workload, [
      "contractVersion",
      "definitionSha256",
      "operationCount",
      "groupCount",
      "typedSlotCount",
      "transitionCount",
    ]);
    assert.deepEqual(report.inputs.workload, {
      contractVersion: "weather-backend-performance-workload/v2",
      definitionSha256: "5722eeecd2734a1cfe4e7887cfc887dc6ac052cb397e08f8389fd6b284c5c364",
      operationCount: 107,
      groupCount: 59,
      typedSlotCount: 156,
      transitionCount: 10,
    });
    assertDeepFrozen(report);
  },
);

test(
  "partitions all static paths once and reads the exact 186-file union",
  // verify bounded source selection
  async () => {
    const report = await actualReport();
    assertExactKeys(report.scope, [
      "repositoryRootPolicy",
      "parserVersion",
      "embeddedSqlPolicy",
      "perFileByteLimit",
      "aggregateByteLimit",
      "supportedExtensions",
      "supplementalRoots",
      "staticGraphPathCount",
      "staticGraphScannedFileCount",
      "staticGraphExcludedPathCount",
      "directOperationRootFileCount",
      "supplementalRootCount",
      "supplementalUniqueAddedFileCount",
      "scannedUniqueFileCount",
      "scannedByteCount",
      "exclusionCount",
    ]);
    assert.deepEqual(
      {
        repositoryRootPolicy: report.scope.repositoryRootPolicy,
        parserVersion: report.scope.parserVersion,
        embeddedSqlPolicy: report.scope.embeddedSqlPolicy,
        perFileByteLimit: report.scope.perFileByteLimit,
        aggregateByteLimit: report.scope.aggregateByteLimit,
        supportedExtensions: report.scope.supportedExtensions,
        supplementalRoots: report.scope.supplementalRoots,
        staticGraphPathCount: report.scope.staticGraphPathCount,
        staticGraphScannedFileCount: report.scope.staticGraphScannedFileCount,
        staticGraphExcludedPathCount: report.scope.staticGraphExcludedPathCount,
        directOperationRootFileCount: report.scope.directOperationRootFileCount,
        supplementalRootCount: report.scope.supplementalRootCount,
        supplementalUniqueAddedFileCount: report.scope.supplementalUniqueAddedFileCount,
        scannedUniqueFileCount: report.scope.scannedUniqueFileCount,
        exclusionCount: report.scope.exclusionCount,
      },
      {
        repositoryRootPolicy: "module-relative-fixed",
        parserVersion: "6.0.3",
        embeddedSqlPolicy: "bounded-context-or-anchor/v1",
        perFileByteLimit: 1_048_576,
        aggregateByteLimit: 16_777_216,
        supportedExtensions: [
          ".cjs",
          ".cts",
          ".js",
          ".jsx",
          ".mjs",
          ".mts",
          ".sql",
          ".ts",
          ".tsx",
        ],
        supplementalRoots: SUPPLEMENTAL_ROOTS,
        staticGraphPathCount: 259,
        staticGraphScannedFileCount: 182,
        staticGraphExcludedPathCount: 77,
        directOperationRootFileCount: 2,
        supplementalRootCount: 4,
        supplementalUniqueAddedFileCount: 2,
        scannedUniqueFileCount: 186,
        exclusionCount: 77,
      },
    );
    assert.ok(report.scope.scannedByteCount > 0);
    assert.ok(report.scope.scannedByteCount <= report.scope.aggregateByteLimit);
    assert.equal(report.files.length, 186);
    assert.equal(report.exclusions.length, 77);
    assertSortedUnique(report.files.map(
      // retain scanned source paths
      (file) => file.sourcePath,
    ));
    assertSortedUnique(report.exclusions.map(
      // retain excluded source paths
      (exclusion) => exclusion.sourcePath,
    ));

    const staticFiles = report.files.filter(
      // retain static graph members
      (file) => file.origins.includes("static-graph"),
    );
    assert.equal(staticFiles.length, 182);
    const staticPartition = [
      ...staticFiles.map(
        // retain scanned static paths
        (file) => file.sourcePath,
      ),
      ...report.exclusions.map(
        // retain excluded static paths
        (exclusion) => exclusion.sourcePath,
      ),
    ];
    assert.equal(staticPartition.length, 259);
    assert.equal(new Set(staticPartition).size, 259);
    assert.deepEqual(
      sortedUnique(report.files.filter(
        // retain direct operation roots
        (file) => file.origins.includes("direct-operation-root"),
      ).map(
        // retain direct source paths
        (file) => file.sourcePath,
      )),
      DIRECT_OPERATION_ROOTS,
    );
    assert.deepEqual(
      sortedUnique(report.files.filter(
        // retain measurement supplements
        (file) => file.origins.includes("measurement-supplement"),
      ).map(
        // retain supplement paths
        (file) => file.sourcePath,
      )),
      SUPPLEMENTAL_ROOTS,
    );

    // validate every scanned file row
    for (const file of report.files) {
      assertExactKeys(file, FILE_KEYS);
      assertSortedUnique(file.origins);
      assertSortedUnique(file.inventoryClassifications);
      assert.equal(["javascript", "typescript", "sql"].includes(file.language), true);
      assert.ok(file.byteLength >= 0);
      assert.ok(file.byteLength <= report.scope.perFileByteLimit);
      assert.match(file.sha256, /^[0-9a-f]{64}$/);
      assertExactKeys(file.coverage, [
        "sourceParseCompleted",
        "sqlLexingCompleted",
        "candidateExtractionCompleted",
      ]);
      assert.ok(file.occurrenceCount >= 0);
      assert.ok(file.diagnosticCount >= 0);
    }
    // validate every excluded path was not reread
    for (const exclusion of report.exclusions) {
      assertExactKeys(exclusion, EXCLUSION_KEYS);
      assert.equal(exclusion.origin, "static-graph");
      assert.equal(exclusion.readAttempted, false);
      assertSortedUnique(exclusion.inventoryClassifications);
      assert.equal([
        "sensitive-or-forbidden-root",
        "unsupported-executable-language",
        "unsupported-operational-declaration",
        "configuration-noncode",
        "unsupported-extension",
      ].includes(exclusion.reason), true);
      assert.equal(
        report.files.some(
          // reject any scanned and excluded overlap
          (file) => file.sourcePath === exclusion.sourcePath,
        ),
        false,
      );
    }
  },
);

test(
  "includes all seven reviewed nested application sources despite pathname words",
  // verify graph membership overrides heuristic exclusions
  async () => {
    const report = await actualReport();
    // validate every nested application source
    for (const sourcePath of NESTED_APPLICATION_PATHS) {
      const file = fileRow(report, sourcePath);
      assert.deepEqual(file.origins, ["static-graph"]);
      assert.equal(file.inventoryClassifications.includes("application"), true);
      assert.equal(
        report.exclusions.some(
          // reject a duplicate exclusion
          (exclusion) => exclusion.sourcePath === sourcePath,
        ),
        false,
      );
    }
  },
);

test(
  "preserves all workload operations as nonauthoritative root hints",
  // verify exact transport and root summaries
  async () => {
    const report = await actualReport();
    assertExactKeys(report.operationRootHints, ["summary", "operations"]);
    assert.deepEqual(report.operationRootHints.summary, {
      operationCount: 107,
      httpOperationCount: 64,
      nodeArgvOperationCount: 46,
      dualTransportOperationCount: 3,
      httpUnmappedOperationCount: 64,
      nodeStaticGraphOperationCount: 43,
      nodeDirectOperationCount: 3,
      nodeUnresolvedOperationCount: 0,
      distinctNodeTargetCount: 14,
      staticGraphNodeTargetCount: 12,
      directNodeTargetCount: 2,
      unresolvedNodeTargetCount: 0,
    });
    assert.equal(report.operationRootHints.operations.length, 107);
    assert.deepEqual(
      report.operationRootHints.operations.map(
        // retain workload order
        (operation) => operation.workloadOrder,
      ),
      Array.from(
        { length: 107 },
        // build one-based workload positions
        (_, index) => index + 1,
      ),
    );

    const staticTargets = new Set();
    const directTargets = new Set();
    // validate every root-hint row
    for (const operation of report.operationRootHints.operations) {
      assertExactKeys(operation, OPERATION_KEYS);
      assert.deepEqual(
        operation.transports,
        ["http", "node-argv"].filter(
          // retain only declared transports
          (transport) => operation.transports.includes(transport),
        ),
      );
      assert.equal(
        ["unmapped-handler", "not-applicable"].includes(operation.httpRootStatus),
        true,
      );
      assert.equal([
        "static-graph-root",
        "direct-operation-root",
        "unresolved",
        "not-applicable",
      ].includes(operation.nodeRootStatus), true);
      assertSortedUnique(operation.nodeSourcePaths);
      assert.equal(operation.branchReachabilityProven, false);
      // collect static root targets
      if (operation.nodeRootStatus === "static-graph-root") {
        for (const sourcePath of operation.nodeSourcePaths) {
          staticTargets.add(sourcePath);
        }
      }
      // collect direct root targets
      if (operation.nodeRootStatus === "direct-operation-root") {
        for (const sourcePath of operation.nodeSourcePaths) {
          directTargets.add(sourcePath);
        }
      }
    }
    assert.equal(staticTargets.size, 12);
    assert.deepEqual([...directTargets].sort(), DIRECT_OPERATION_ROOTS);
    assert.equal(
      report.operationRootHints.operations.filter(
        // retain http contracts
        (operation) => operation.httpRootStatus === "unmapped-handler",
      ).length,
      64,
    );
    assert.equal(
      report.operationRootHints.operations.filter(
        // retain node contracts
        (operation) => operation.transports.includes("node-argv"),
      ).length,
      46,
    );
  },
);

test(
  "reports grounded source sentinels without assigning decision or operation authority",
  // verify actual repository detector integration
  async () => {
    const report = await actualReport();
    assert.ok(report.occurrences.length > 0);
    assert.equal(
      report.occurrences.some(
        // find the api constructor sentinel
        (occurrence) => occurrence.sourcePath === "apps/api/src/index.ts"
          && occurrence.surface === "ecmascript.date-construct-current",
      ),
      true,
    );
    assert.equal(
      report.occurrences.some(
        // find the worker direct clock sentinel
        (occurrence) => occurrence.sourcePath === "apps/worker/src/worker.ts"
          && occurrence.surface === "ecmascript.date-now",
      ),
      true,
    );
    assert.equal(
      report.occurrences.some(
        // find the worker wrapper sentinel
        (occurrence) => occurrence.sourcePath === "apps/worker/src/worker.ts"
          && occurrence.surface.startsWith("ecmascript.wrapper-"),
      ),
      true,
    );
    assert.equal(
      report.occurrences.some(
        // find the resource wall-clock sentinel
        (occurrence) => occurrence.sourcePath === "scripts/backend-performance-resources.mjs"
          && occurrence.surface === "ecmascript.date-now",
      ),
      true,
    );
    assert.equal(
      report.occurrences.some(
        // find the resource relative-clock sentinel
        (occurrence) => occurrence.sourcePath === "scripts/backend-performance-resources.mjs"
          && occurrence.surface === "node.performance-now",
      ),
      true,
    );
    assert.equal(
      report.occurrences.some(
        // find the embedded transaction sentinel
        (occurrence) => occurrence.sourcePath === "deploy/scripts/adjustment-evaluation-package.mjs"
          && occurrence.surface === "postgres.transaction-timestamp",
          ),
      true,
    );
    // forbid later authority fields on occurrence records
    for (const occurrence of report.occurrences) {
      assert.equal("decisionId" in occurrence, false);
      assert.equal("operationId" in occurrence, false);
      assert.equal("clockDomain" in occurrence, false);
      assert.equal("providerAllowlist" in occurrence, false);
    }
  },
);

test(
  "emits exact gap bases affected sets and no empty conditional rows",
  // verify finite nonauthority gaps
  async () => {
    const report = await actualReport();
    const gapCodes = report.gaps.map(
      // retain one gap code
      (gap) => gap.code,
    );
    assert.equal(new Set(gapCodes).size, gapCodes.length);
    // validate every unconditional report marker
    for (const code of UNCONDITIONAL_GAPS) {
      const gap = gapRow(report, code);
      assert.deepEqual(gap, {
        code,
        countBasis: "report",
        count: 1,
        affectedIds: ["report"],
      });
    }

    const conditional = expectedConditionalGaps(report);
    // validate every conditional affected set
    for (const [code, affectedIds] of conditional) {
      const gap = report.gaps.find(
        // match one conditional row
        (candidate) => candidate.code === code,
      );
      // omit empty conditional sets
      if (affectedIds.length === 0) {
        assert.equal(gap, undefined, code);
        continue;
      }
      assert.notEqual(gap, undefined, code);
      assert.deepEqual(gap, {
        code,
        countBasis: GAP_BASES.get(code),
        count: affectedIds.length,
        affectedIds,
      });
    }

    // validate every emitted row against the frozen code table
    for (const gap of report.gaps) {
      assertExactKeys(gap, GAP_KEYS);
      assert.equal(GAP_BASES.has(gap.code), true, gap.code);
      assert.equal(gap.countBasis, GAP_BASES.get(gap.code));
      assert.ok(gap.count > 0);
      assert.equal(gap.count, gap.affectedIds.length);
      assertSortedUnique(gap.affectedIds);
    }
    assert.equal(gapRow(report, "http.handler-roots-unmapped").count, 64);
    assert.equal(gapRow(report, "node.branch-reachability-unproven").count, 46);
    assert.deepEqual(
      gapRow(report, "node.direct-root-import-closure-missing").affectedIds,
      DIRECT_OPERATION_ROOTS,
    );
  },
);

test(
  "builds stable bytes and resists input or result mutation",
  // verify deterministic repeatability without a giant snapshot
  async () => {
    const first = await actualReport();
    const firstBytes = `${JSON.stringify(first, null, 2)}\n`;
    assert.throws(
      // reject top-level mutation
      () => {
        first.status = "complete";
      },
      TypeError,
    );
    assert.throws(
      // reject nested mutation
      () => first.files.push(null),
      TypeError,
    );
    const second = await buildBackendClockCatalogReport();
    assert.equal(`${JSON.stringify(first, null, 2)}\n`, firstBytes);
    assert.equal(`${JSON.stringify(second, null, 2)}\n`, firstBytes);
  },
);

test(
  "review repair: source structure checks aggregate descriptor size before reading and growth after",
  // verify the private descriptor boundary without adding an injection seam
  async () => {
    const sourceText = await readFile(
      new URL("./backend-clock-catalog-report.mjs", import.meta.url),
      "utf8",
    );
    const functionStart = sourceText.indexOf(
      "async function readSelectedSource(rootRealPath, sourcePath, aggregateState)",
    );
    const functionEnd = sourceText.indexOf(
      "\n// compare two numeric values",
      functionStart,
    );
    assert.ok(functionStart >= 0);
    assert.ok(functionEnd > functionStart);
    const functionSource = sourceText.slice(functionStart, functionEnd);
    const descriptorStat = functionSource.indexOf("await handle.stat()");
    const descriptorAggregateCheck = /aggregateState\.byteCount\s*\+\s*metadata\.size\s*>\s*AGGREGATE_BYTE_LIMIT/u.exec(functionSource);
    const descriptorRead = functionSource.indexOf("await handle.readFile()");
    const postReadAggregateCheck = /aggregateState\.byteCount\s*\+\s*bytes\.length\s*>\s*AGGREGATE_BYTE_LIMIT/u.exec(functionSource);

    assert.ok(descriptorStat >= 0);
    assert.notEqual(descriptorAggregateCheck, null);
    assert.ok(descriptorAggregateCheck.index > descriptorStat);
    assert.ok(descriptorRead > descriptorAggregateCheck.index);
    assert.notEqual(postReadAggregateCheck, null);
    assert.ok(postReadAggregateCheck.index > descriptorRead);
  },
);

test(
  "uses only exit 3 incomplete exit 2 usage and sanitized exit 1 error CLI outcomes",
  // verify direct command semantics and permission boundaries
  async () => {
    const report = await actualReport();
    const success = runReportCli([], [
      "--permission",
      `--allow-fs-read=${REPOSITORY_ROOT}`,
    ]);
    assert.equal(success.error, undefined);
    assert.equal(success.status, 3);
    assert.equal(success.signal, null);
    assert.equal(success.stderr, "");
    assert.equal(success.stdout, `${JSON.stringify(report, null, 2)}\n`);

    const usage = runReportCli(["unexpected"]);
    assert.equal(usage.error, undefined);
    assert.equal(usage.status, 2);
    assert.equal(usage.stdout, "");
    assert.equal(usage.stderr, "usage: backend-clock-catalog-report.mjs\n");

    const denied = runReportCli([], [
      "--permission",
      `--allow-fs-read=${fileURLToPath(new URL("./", import.meta.url))}`,
      `--allow-fs-read=${fileURLToPath(new URL("../node_modules/", import.meta.url))}`,
    ]);
    assert.equal(denied.error, undefined);
    assert.equal(denied.status, 1);
    assert.equal(denied.stdout, "");
    assert.deepEqual(JSON.parse(denied.stderr), {
      contractVersion: "weather-backend-clock-catalog-gap-report-error/v1",
      status: "error",
      code: "report.inventory-prerequisite",
    });
    assert.equal([success.status, usage.status, denied.status].includes(0), false);
  },
);
