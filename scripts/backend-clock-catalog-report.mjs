import { constants as fsConstants } from "node:fs";
import { lstat, open, readFile, realpath } from "node:fs/promises";
import { extname, isAbsolute, relative, resolve, sep } from "node:path";

export const BACKEND_CLOCK_CATALOG_REPORT_VERSION =
  "weather-backend-clock-catalog-gap-report/v1";

const ERROR_CONTRACT_VERSION =
  "weather-backend-clock-catalog-gap-report-error/v1";
const WORKLOAD_CONTRACT_VERSION = "weather-backend-performance-workload/v2";
const WORKLOAD_DEFINITION_SHA256 =
  "5722eeecd2734a1cfe4e7887cfc887dc6ac052cb397e08f8389fd6b284c5c364";
const EXPECTED_TYPESCRIPT_VERSION = "6.0.3";
const PER_FILE_BYTE_LIMIT = 1_048_576;
const AGGREGATE_BYTE_LIMIT = 16_777_216;

// retain the closed source extension set
const SUPPORTED_EXTENSIONS = Object.freeze([
  ".cjs",
  ".cts",
  ".js",
  ".jsx",
  ".mjs",
  ".mts",
  ".sql",
  ".ts",
  ".tsx",
]);
const SUPPORTED_EXTENSION_SET = new Set(SUPPORTED_EXTENSIONS);

// retain the fixed measurement roots
const SUPPLEMENTAL_ROOTS = Object.freeze([
  "scripts/backend-host-observation.mjs",
  "scripts/backend-performance-receipts.mjs",
  "scripts/backend-performance-resources.mjs",
  "scripts/database-page-performance.mjs",
]);

// retain reviewed nested application members
const NESTED_APPLICATION_PATHS = Object.freeze([
  "scripts/research/adjustment-maintenance-runtime/node_modules/@weather/domain/dist/forecast-adjustment.js",
  "scripts/research/adjustment-maintenance-runtime/node_modules/@weather/domain/dist/forecast-anchor-record.js",
  "scripts/research/adjustment-maintenance-runtime/node_modules/@weather/domain/dist/index.js",
  "scripts/research/adjustment-maintenance-runtime/node_modules/@weather/domain/dist/ingestion.js",
  "scripts/research/adjustment-maintenance-runtime/node_modules/@weather/domain/dist/provenance.js",
  "scripts/research/adjustment-maintenance-runtime/node_modules/@weather/domain/dist/rain-collection.js",
  "scripts/research/adjustment-maintenance-runtime/node_modules/@weather/domain/dist/weather-record.js",
]);

// retain closed inventory classifications
const INVENTORY_CLASSIFICATIONS = new Set([
  "application",
  "release-control",
  "oracle",
  "build",
  "offline-research",
]);

// retain exact report-root exclusions
const FORBIDDEN_ROOTS = Object.freeze([
  ".git",
  ".omx",
  ".weather-data",
  "node_modules",
  "deploy/secrets",
]);

// retain fixed build errors
const REPORT_ERROR_CODES = new Set([
  "report.inventory-prerequisite",
  "report.workload-prerequisite",
  "report.parser-version",
  "report.source-selection",
  "report.source-read",
  "report.source-encoding",
  "report.source-limit",
  "report.unexpected",
]);

// retain frozen cardinality expectations
const EXPECTED_OPERATION_SUMMARY = Object.freeze({
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

// compare raw utf16 code units
function compareText(left, right) {
  // order smaller text first
  if (left < right) {
    return -1;
  }

  // order larger text last
  if (left > right) {
    return 1;
  }
  return 0;
}

// sort and de-duplicate text values
function sortedUnique(values) {
  const unique = new Set();

  // retain every distinct text value
  for (const value of values) {
    unique.add(value);
  }
  return [...unique].sort(compareText);
}

// freeze one detached contract tree
function deepFreeze(value) {
  // freeze only mutable objects
  if (value !== null && typeof value === "object" && !Object.isFrozen(value)) {
    // freeze every child first
    for (const child of Object.values(value)) {
      deepFreeze(child);
    }
    Object.freeze(value);
  }
  return value;
}

// create one fixed-code build error
function reportError(code) {
  const error = new Error(code);
  error.code = code;
  return error;
}

// recognize one fixed-code build error
function isReportError(error) {
  return error !== null && typeof error === "object" &&
    REPORT_ERROR_CODES.has(error.code);
}

// validate one repository-relative path
function isSafeRelativePath(sourcePath) {
  // reject nontext and absolute forms
  if (
    typeof sourcePath !== "string" ||
    sourcePath.length === 0 ||
    isAbsolute(sourcePath) ||
    /^[A-Za-z]:\//u.test(sourcePath) ||
    sourcePath.includes("\\") ||
    sourcePath.includes("\0")
  ) {
    return false;
  }
  const segments = sourcePath.split("/");

  // reject empty and traversal segments
  for (const segment of segments) {
    // reject one unsafe segment
    if (segment.length === 0 || segment === "." || segment === "..") {
      return false;
    }
  }
  return true;
}

// identify one forbidden report-root path
function isForbiddenReportPath(sourcePath) {
  // reject every unsafe relative path
  if (!isSafeRelativePath(sourcePath)) {
    return true;
  }

  // reject only root environment basenames
  if (sourcePath === ".env" || sourcePath.startsWith(".env.")) {
    return true;
  }

  // inspect each exact forbidden root
  for (const root of FORBIDDEN_ROOTS) {
    // reject the root and its descendants
    if (sourcePath === root || sourcePath.startsWith(`${root}/`)) {
      return true;
    }
  }
  return false;
}

// classify one static graph path
function classifyStaticPath(sourcePath) {
  // prioritize unsafe and sensitive roots
  if (isForbiddenReportPath(sourcePath)) {
    return "sensitive-or-forbidden-root";
  }
  const extension = extname(sourcePath);

  // admit every closed source extension
  if (SUPPORTED_EXTENSION_SET.has(extension)) {
    return null;
  }

  // retain unsupported executable languages
  if (extension === ".py" || extension === ".sh") {
    return "unsupported-executable-language";
  }

  // retain operational declarations
  if (extension === ".service" || extension === ".timer" || extension === "") {
    return "unsupported-operational-declaration";
  }

  // retain noncode configuration
  if (
    extension === ".json" ||
    extension === ".yaml" ||
    extension === ".yml" ||
    extension === ".example"
  ) {
    return "configuration-noncode";
  }
  return "unsupported-extension";
}

// prove one resolved file remains contained
function isContainedPath(rootRealPath, candidateRealPath) {
  const relativePath = relative(rootRealPath, candidateRealPath);

  // reject the root itself and every escape
  if (
    relativePath.length === 0 ||
    isAbsolute(relativePath) ||
    relativePath === ".." ||
    relativePath.startsWith(`..${sep}`)
  ) {
    return false;
  }
  return true;
}

// add one selected source origin
function addSelectedSource(selected, sourcePath, origin, classifications = []) {
  let row = selected.get(sourcePath);

  // create one detached selection row
  if (row === undefined) {
    row = {
      sourcePath,
      origins: new Set(),
      inventoryClassifications: new Set(),
    };
    selected.set(sourcePath, row);
  }
  row.origins.add(origin);

  // retain every inventory classification
  for (const classification of classifications) {
    row.inventoryClassifications.add(classification);
  }
}

// load and validate the checked inventory first
async function loadInventory(rootDir) {
  const inventoryModule = await import("./check-backend-runtime-inventory.mjs");
  const inventory = await inventoryModule.validateRuntimeInventory({
    rootDir,
    generation: "node-v1",
    phase: "baseline",
  });

  // require the exact clean checked baseline
  if (
    inventory?.ok !== true ||
    !Array.isArray(inventory.diagnostics) ||
    inventory.diagnostics.length !== 0 ||
    !Array.isArray(inventory.discovered) ||
    inventory.discovered.length !== 437 ||
    inventory.runtimeMapArtifactCount !== 499 ||
    inventory.runtimeMapBindingCount !== 2040 ||
    inventory.runtimeMapContextCount !== 174 ||
    !/^[0-9a-f]{64}$/u.test(inventory.manifestDigest) ||
    !/^[0-9a-f]{64}$/u.test(inventory.runtimeMapDigest)
  ) {
    throw reportError("report.inventory-prerequisite");
  }
  return inventory;
}

// load and validate the checked workload second
async function loadWorkload(rootDir) {
  let workloadModule;
  let definition;

  try {
    workloadModule = await import("./backend-performance-workload.mjs");
    const bytes = await readFile(
      resolve(rootDir, "config/backend-performance-workload-v2.json"),
    );
    const text = new TextDecoder("utf-8", { fatal: true }).decode(bytes);
    definition = JSON.parse(text);
  } catch {
    throw reportError("report.workload-prerequisite");
  }
  const validation =
    workloadModule.validateBackendPerformanceWorkloadDefinition(definition);

  // require the exact checked workload identity
  if (
    workloadModule.BACKEND_PERFORMANCE_WORKLOAD_VERSION !==
      WORKLOAD_CONTRACT_VERSION ||
    validation?.ok !== true ||
    !Array.isArray(validation.diagnostics) ||
    validation.diagnostics.length !== 0 ||
    validation.definitionSha256 !== WORKLOAD_DEFINITION_SHA256 ||
    definition?.contractVersion !== WORKLOAD_CONTRACT_VERSION ||
    !Array.isArray(definition.operations) ||
    definition.operations.length !== 107 ||
    !Array.isArray(definition.groups) ||
    definition.groups.length !== 59 ||
    definition.counts?.typedSlots !== 156 ||
    definition.counts?.transitions !== 10
  ) {
    throw reportError("report.workload-prerequisite");
  }
  return { definition, validation };
}

// load the pinned parser and green extractor
async function loadExtractor() {
  let typescriptModule;

  try {
    typescriptModule = await import("typescript");
  } catch {
    throw reportError("report.parser-version");
  }
  const typescript = typescriptModule.default ?? typescriptModule;

  // require the exact parser pin
  if (typescript.version !== EXPECTED_TYPESCRIPT_VERSION) {
    throw reportError("report.parser-version");
  }
  const extractorModule = await import("./backend-clock-occurrences.mjs");

  // require the fixed extractor export
  if (typeof extractorModule.extractBackendClockOccurrences !== "function") {
    throw reportError("report.unexpected");
  }
  return extractorModule.extractBackendClockOccurrences;
}

// map one literal node target conservatively
function canonicalNodeSource(target) {
  // reject nonliteral and flag targets
  if (typeof target !== "string" || target.startsWith("-")) {
    return null;
  }
  const distMatch = /^apps\/(api|worker)\/dist\/([^/]+)\.js$/u.exec(target);

  // map one closed workspace dist target
  if (distMatch !== null) {
    return `apps/${distMatch[1]}/src/${distMatch[2]}.ts`;
  }

  // retain one already supported repository path
  if (
    isSafeRelativePath(target) &&
    !isForbiddenReportPath(target) &&
    SUPPORTED_EXTENSION_SET.has(extname(target))
  ) {
    return target;
  }
  return null;
}

// probe one direct source without reading bytes
async function directSourceExists(rootRealPath, sourcePath) {
  const absolutePath = resolve(rootRealPath, sourcePath);

  try {
    const metadata = await lstat(absolutePath);

    // reject a symlink leaf
    if (metadata.isSymbolicLink()) {
      throw reportError("report.source-read");
    }
    const canonicalPath = await realpath(absolutePath);

    // reject every containment escape
    if (!isContainedPath(rootRealPath, canonicalPath)) {
      throw reportError("report.source-read");
    }

    // require one regular file
    if (!metadata.isFile()) {
      throw reportError("report.source-read");
    }
    return true;
  } catch (error) {
    // retain an absent direct target as unresolved
    if (error?.code === "ENOENT") {
      return false;
    }

    // preserve one fixed report error
    if (isReportError(error)) {
      throw error;
    }
    throw reportError("report.source-read");
  }
}

// extract unique node targets from one operation
function operationNodeTargets(operation) {
  const targets = new Set();

  // retain no targets for nonnode operations
  if (operation.nodeArgv === null) {
    return [];
  }

  // reject malformed nested node arguments
  if (!Array.isArray(operation.nodeArgv)) {
    throw reportError("report.source-selection");
  }

  // inspect every execution phase
  for (const phase of operation.nodeArgv) {
    // require the closed literal node shape
    if (
      !Array.isArray(phase?.argv) ||
      phase.argv[0] !== "node" ||
      typeof phase.argv[1] !== "string"
    ) {
      throw reportError("report.source-selection");
    }
    targets.add(phase.argv[1]);
  }
  return [...targets].sort(compareText);
}

// build all nonauthoritative operation root hints
async function buildOperationRootHints(definition, staticPaths, rootRealPath) {
  const targetsByOperation = [];
  const allTargets = new Set();

  // collect literal targets in workload order
  for (const operation of definition.operations) {
    const targets = operationNodeTargets(operation);
    targetsByOperation.push(targets);

    // retain every distinct module target
    for (const target of targets) {
      allTargets.add(target);
    }
  }
  const targetDetails = new Map();

  // resolve every literal target once
  for (const target of [...allTargets].sort(compareText)) {
    const sourcePath = canonicalNodeSource(target);
    let status = "unresolved";

    // retain one static graph root
    if (sourcePath !== null && staticPaths.has(sourcePath)) {
      status = "static-graph-root";
    } else if (
      sourcePath !== null &&
      await directSourceExists(rootRealPath, sourcePath)
    ) {
      // retain one safe direct root
      status = "direct-operation-root";
    }
    targetDetails.set(target, { sourcePath, status });
  }

  const operations = [];
  let httpOperationCount = 0;
  let nodeArgvOperationCount = 0;
  let dualTransportOperationCount = 0;
  let nodeStaticGraphOperationCount = 0;
  let nodeDirectOperationCount = 0;
  let nodeUnresolvedOperationCount = 0;

  // preserve every workload operation row
  for (let index = 0; index < definition.operations.length; index += 1) {
    const operation = definition.operations[index];
    const hasHttp = operation.exactHttp !== null;
    const hasNode = operation.nodeArgv !== null;
    const transports = [];

    // retain http transport presence
    if (hasHttp) {
      transports.push("http");
      httpOperationCount += 1;
    }

    // retain node transport presence
    if (hasNode) {
      transports.push("node-argv");
      nodeArgvOperationCount += 1;
    }

    // retain dual transport presence
    if (hasHttp && hasNode) {
      dualTransportOperationCount += 1;
    }
    let nodeRootStatus = "not-applicable";
    let nodeSourcePaths = [];

    // derive one conservative node status
    if (hasNode) {
      const targets = targetsByOperation[index];
      let sharedStatus = null;
      const resolvedPaths = new Set();

      // inspect every phase target
      for (const target of targets) {
        const details = targetDetails.get(target);

        // reject missing target metadata
        if (details === undefined) {
          throw reportError("report.source-selection");
        }

        // merge only one consistent root class
        if (sharedStatus === null) {
          sharedStatus = details.status;
        } else if (sharedStatus !== details.status) {
          sharedStatus = "unresolved";
        }

        // retain only resolved canonical paths
        if (details.status !== "unresolved" && details.sourcePath !== null) {
          resolvedPaths.add(details.sourcePath);
        }
      }
      nodeRootStatus = sharedStatus ?? "unresolved";

      // expose paths only for one resolved root class
      if (nodeRootStatus !== "unresolved") {
        nodeSourcePaths = [...resolvedPaths].sort(compareText);
      }

      // count one static root row
      if (nodeRootStatus === "static-graph-root") {
        nodeStaticGraphOperationCount += 1;
      } else if (nodeRootStatus === "direct-operation-root") {
        // count one direct root row
        nodeDirectOperationCount += 1;
      } else {
        // count one unresolved root row
        nodeUnresolvedOperationCount += 1;
      }
    }
    operations.push({
      operationId: operation.id,
      workloadOrder: index + 1,
      transports,
      httpRootStatus: hasHttp ? "unmapped-handler" : "not-applicable",
      nodeRootStatus,
      nodeSourcePaths,
      branchReachabilityProven: false,
    });
  }

  let staticGraphNodeTargetCount = 0;
  let directNodeTargetCount = 0;
  let unresolvedNodeTargetCount = 0;
  const directSourcePaths = new Set();

  // count every distinct target class
  for (const details of targetDetails.values()) {
    // count one static target
    if (details.status === "static-graph-root") {
      staticGraphNodeTargetCount += 1;
    } else if (details.status === "direct-operation-root") {
      // count one direct target
      directNodeTargetCount += 1;

      // retain one direct source path
      if (details.sourcePath !== null) {
        directSourcePaths.add(details.sourcePath);
      }
    } else {
      // count one unresolved target
      unresolvedNodeTargetCount += 1;
    }
  }
  const summary = {
    operationCount: operations.length,
    httpOperationCount,
    nodeArgvOperationCount,
    dualTransportOperationCount,
    httpUnmappedOperationCount: httpOperationCount,
    nodeStaticGraphOperationCount,
    nodeDirectOperationCount,
    nodeUnresolvedOperationCount,
    distinctNodeTargetCount: targetDetails.size,
    staticGraphNodeTargetCount,
    directNodeTargetCount,
    unresolvedNodeTargetCount,
  };

  // require every frozen root cardinality
  for (const [key, expected] of Object.entries(EXPECTED_OPERATION_SUMMARY)) {
    // reject workload or source-root drift
    if (summary[key] !== expected) {
      throw reportError("report.source-selection");
    }
  }
  return {
    operationRootHints: { summary, operations },
    directSourcePaths,
  };
}

// select the exact bounded source union
async function selectSources(inventory, definition, rootRealPath) {
  const staticRows = new Map();

  // aggregate inventory entries by source path
  for (const entry of inventory.discovered) {
    // require one closed inventory row
    if (
      typeof entry?.sourcePath !== "string" ||
      !INVENTORY_CLASSIFICATIONS.has(entry.classification)
    ) {
      throw reportError("report.source-selection");
    }
    let row = staticRows.get(entry.sourcePath);

    // create one static source row
    if (row === undefined) {
      row = new Set();
      staticRows.set(entry.sourcePath, row);
    }
    row.add(entry.classification);
  }

  // require the exact static graph path count
  if (staticRows.size !== 259) {
    throw reportError("report.source-selection");
  }
  const selected = new Map();
  const exclusions = [];

  // partition every static path once
  for (const sourcePath of [...staticRows.keys()].sort(compareText)) {
    const classifications = staticRows.get(sourcePath);
    const reason = classifyStaticPath(sourcePath);

    // retain one scanned static source
    if (reason === null) {
      addSelectedSource(selected, sourcePath, "static-graph", classifications);
    } else {
      // retain one unread static exclusion
      exclusions.push({
        sourcePath,
        origin: "static-graph",
        inventoryClassifications: sortedUnique(classifications),
        reason,
        readAttempted: false,
      });
    }
  }
  const staticScannedCount = selected.size;

  // require the exact static partition
  if (staticScannedCount !== 182 || exclusions.length !== 77) {
    throw reportError("report.source-selection");
  }

  // require all reviewed nested application sources
  for (const sourcePath of NESTED_APPLICATION_PATHS) {
    const row = selected.get(sourcePath);

    // reject one missing or misclassified nested source
    if (
      row === undefined ||
      !row.origins.has("static-graph") ||
      !row.inventoryClassifications.has("application")
    ) {
      throw reportError("report.source-selection");
    }
  }
  const roots = await buildOperationRootHints(
    definition,
    new Set(staticRows.keys()),
    rootRealPath,
  );

  // add every direct-only operation root
  for (const sourcePath of roots.directSourcePaths) {
    addSelectedSource(selected, sourcePath, "direct-operation-root");
  }
  const beforeSupplements = new Set(selected.keys());
  let supplementalUniqueAddedFileCount = 0;

  // add every fixed measurement supplement
  for (const sourcePath of SUPPLEMENTAL_ROOTS) {
    // count one newly added supplement
    if (!beforeSupplements.has(sourcePath)) {
      supplementalUniqueAddedFileCount += 1;
    }
    addSelectedSource(selected, sourcePath, "measurement-supplement");
  }

  // require the exact final occurrence-reader union
  if (
    roots.directSourcePaths.size !== 2 ||
    supplementalUniqueAddedFileCount !== 2 ||
    selected.size !== 186
  ) {
    throw reportError("report.source-selection");
  }
  return {
    selected,
    exclusions,
    operationRootHints: roots.operationRootHints,
    staticPathCount: staticRows.size,
    staticScannedCount,
    staticExcludedCount: exclusions.length,
    directOperationRootFileCount: roots.directSourcePaths.size,
    supplementalUniqueAddedFileCount,
  };
}

// read one selected source through one descriptor
async function readSelectedSource(rootRealPath, sourcePath, aggregateState) {
  try {
    // reject every unselected path form
    if (
      isForbiddenReportPath(sourcePath) ||
      !SUPPORTED_EXTENSION_SET.has(extname(sourcePath))
    ) {
      throw reportError("report.source-read");
    }
    const absolutePath = resolve(rootRealPath, sourcePath);
    const leafMetadata = await lstat(absolutePath);

    // reject one symlink leaf
    if (leafMetadata.isSymbolicLink()) {
      throw reportError("report.source-read");
    }
    const canonicalPath = await realpath(absolutePath);

    // reject every containment escape
    if (!isContainedPath(rootRealPath, canonicalPath)) {
      throw reportError("report.source-read");
    }
    const handle = await open(
      canonicalPath,
      fsConstants.O_RDONLY | fsConstants.O_NOFOLLOW,
    );
    let bytes;

    try {
      const metadata = await handle.stat();

      // require one bounded regular file
      if (!metadata.isFile()) {
        throw reportError("report.source-read");
      }

      // reject one oversized file before reading
      if (metadata.size > PER_FILE_BYTE_LIMIT) {
        throw reportError("report.source-limit");
      }

      // reject aggregate size before reading
      if (aggregateState.byteCount + metadata.size > AGGREGATE_BYTE_LIMIT) {
        throw reportError("report.source-limit");
      }
      bytes = await handle.readFile();
    } finally {
      await handle.close();
    }

    // reject post-open size growth
    if (bytes.length > PER_FILE_BYTE_LIMIT) {
      throw reportError("report.source-limit");
    }

    // reject aggregate size growth in sorted order
    if (aggregateState.byteCount + bytes.length > AGGREGATE_BYTE_LIMIT) {
      throw reportError("report.source-limit");
    }
    let sourceText;

    try {
      sourceText = new TextDecoder("utf-8", { fatal: true }).decode(bytes);
    } catch {
      throw reportError("report.source-encoding");
    }
    aggregateState.byteCount += bytes.length;
    return { bytes, sourceText };
  } catch (error) {
    // preserve one fixed reader failure
    if (isReportError(error)) {
      throw error;
    }
    throw reportError("report.source-read");
  }
}

// compare two numeric values
function compareNumber(left, right) {
  return left - right;
}

// compare two occurrence records globally
function compareOccurrences(left, right) {
  const leftDecodedStart = left.embeddedSql?.decodedRange.startOffsetUtf16 ?? -1;
  const rightDecodedStart = right.embeddedSql?.decodedRange.startOffsetUtf16 ?? -1;
  const leftDecodedEnd = left.embeddedSql?.decodedRange.endOffsetUtf16 ?? -1;
  const rightDecodedEnd = right.embeddedSql?.decodedRange.endOffsetUtf16 ?? -1;
  return compareText(left.sourcePath, right.sourcePath) ||
    compareNumber(
      left.physicalSpan.startOffsetUtf16,
      right.physicalSpan.startOffsetUtf16,
    ) ||
    compareNumber(
      left.physicalSpan.endOffsetUtf16,
      right.physicalSpan.endOffsetUtf16,
    ) ||
    compareNumber(leftDecodedStart, rightDecodedStart) ||
    compareNumber(leftDecodedEnd, rightDecodedEnd) ||
    compareText(left.surface, right.surface) ||
    compareText(left.kind, right.kind) ||
    compareText(
      left.normalizedExpressionSha256,
      right.normalizedExpressionSha256,
    ) ||
    compareNumber(left.duplicateOrdinal, right.duplicateOrdinal);
}

// compare two diagnostic records globally
function compareDiagnostics(left, right) {
  const leftStart = left.physicalSpan?.startOffsetUtf16 ?? -1;
  const rightStart = right.physicalSpan?.startOffsetUtf16 ?? -1;
  const leftEnd = left.physicalSpan?.endOffsetUtf16 ?? -1;
  const rightEnd = right.physicalSpan?.endOffsetUtf16 ?? -1;
  return compareText(left.sourcePath, right.sourcePath) ||
    compareNumber(leftStart, rightStart) ||
    compareNumber(leftEnd, rightEnd) ||
    compareText(left.code, right.code);
}

// scan the selected union through the green core
async function scanSelectedSources(
  selection,
  rootRealPath,
  extractBackendClockOccurrences,
) {
  const files = [];
  const occurrences = [];
  const diagnostics = [];
  const aggregateState = { byteCount: 0 };

  // read every unique source in sorted order
  for (const sourcePath of [...selection.selected.keys()].sort(compareText)) {
    const selectedRow = selection.selected.get(sourcePath);
    const { bytes, sourceText } = await readSelectedSource(
      rootRealPath,
      sourcePath,
      aggregateState,
    );
    const extracted = extractBackendClockOccurrences({
      sourcePath,
      sourceText,
    });

    // require one matching extractor result
    if (
      extracted?.source?.path !== sourcePath ||
      extracted.source.byteLength !== bytes.length ||
      !Array.isArray(extracted.occurrences) ||
      !Array.isArray(extracted.diagnostics)
    ) {
      throw reportError("report.unexpected");
    }
    files.push({
      sourcePath,
      origins: sortedUnique(selectedRow.origins),
      inventoryClassifications: sortedUnique(
        selectedRow.inventoryClassifications,
      ),
      language: extracted.source.language,
      byteLength: extracted.source.byteLength,
      sha256: extracted.source.sha256,
      coverage: {
        sourceParseCompleted: extracted.coverage.sourceParseCompleted,
        sqlLexingCompleted: extracted.coverage.sqlLexingCompleted,
        candidateExtractionCompleted:
          extracted.coverage.candidateExtractionCompleted,
      },
      occurrenceCount: extracted.occurrences.length,
      diagnosticCount: extracted.diagnostics.length,
    });

    // retain every extracted occurrence
    for (const occurrence of extracted.occurrences) {
      occurrences.push(occurrence);
    }

    // retain every extracted diagnostic
    for (const diagnostic of extracted.diagnostics) {
      diagnostics.push(diagnostic);
    }
  }
  occurrences.sort(compareOccurrences);
  diagnostics.sort(compareDiagnostics);
  return {
    files,
    occurrences,
    diagnostics,
    scannedByteCount: aggregateState.byteCount,
  };
}

// append one nonempty exact gap row
function appendGap(rows, code, countBasis, values) {
  const affectedIds = sortedUnique(values);

  // omit empty conditional gaps
  if (affectedIds.length === 0) {
    return;
  }
  rows.push({
    code,
    countBasis,
    count: affectedIds.length,
    affectedIds,
  });
}

// derive the complete frozen gap table
function buildGapRows(reportParts) {
  const rows = [];
  const httpOperations = new Set();
  const nodeOperations = new Set();
  const directRoots = new Set();
  const unresolvedNodes = new Set();
  const pythonPaths = new Set();
  const shellPaths = new Set();
  const operationalPaths = new Set();
  const configurationPaths = new Set();
  const unsupportedPaths = new Set();
  const sensitivePaths = new Set();
  const dynamicCodePaths = new Set();
  const dynamicFragmentPaths = new Set();
  const typedCurrentPaths = new Set();
  const dollarBodyIds = new Set();
  const parseIncompletePaths = new Set();
  const sqlLexIncompletePaths = new Set();

  // classify every operation gap
  for (const operation of reportParts.operationRootHints.operations) {
    // retain one unmapped http operation
    if (operation.httpRootStatus === "unmapped-handler") {
      httpOperations.add(operation.operationId);
    }

    // retain one node branch gap
    if (operation.transports.includes("node-argv")) {
      nodeOperations.add(operation.operationId);
    }

    // retain every direct root path
    if (operation.nodeRootStatus === "direct-operation-root") {
      // retain each direct source path
      for (const sourcePath of operation.nodeSourcePaths) {
        directRoots.add(sourcePath);
      }
    }

    // retain one unresolved node operation
    if (operation.nodeRootStatus === "unresolved") {
      unresolvedNodes.add(operation.operationId);
    }
  }

  // classify every static exclusion gap
  for (const exclusion of reportParts.exclusions) {
    // retain one python source
    if (exclusion.sourcePath.endsWith(".py")) {
      pythonPaths.add(exclusion.sourcePath);
    }

    // retain one shell source
    if (exclusion.sourcePath.endsWith(".sh")) {
      shellPaths.add(exclusion.sourcePath);
    }

    // retain one operational declaration
    if (exclusion.reason === "unsupported-operational-declaration") {
      operationalPaths.add(exclusion.sourcePath);
    }

    // retain one noncode configuration
    if (exclusion.reason === "configuration-noncode") {
      configurationPaths.add(exclusion.sourcePath);
    }

    // retain one unsupported extension
    if (exclusion.reason === "unsupported-extension") {
      unsupportedPaths.add(exclusion.sourcePath);
    }

    // retain one sensitive root
    if (exclusion.reason === "sensitive-or-forbidden-root") {
      sensitivePaths.add(exclusion.sourcePath);
    }
  }

  // classify every extractor diagnostic gap
  for (const diagnostic of reportParts.diagnostics) {
    // retain one dynamic code path
    if (diagnostic.code === "javascript.dynamic-code-unresolved") {
      dynamicCodePaths.add(diagnostic.sourcePath);
    }

    // retain one dynamic sql path
    if (diagnostic.code === "sql.dynamic-fragment-unresolved") {
      dynamicFragmentPaths.add(diagnostic.sourcePath);
    }

    // retain one typed current context path
    if (diagnostic.code === "sql.typed-current-context-unresolved") {
      typedCurrentPaths.add(diagnostic.sourcePath);
    }
  }

  // classify every occurrence gap
  for (const occurrence of reportParts.occurrences) {
    // retain one unresolved dollar body
    if (occurrence.surface === "postgres.dollar-quoted-body") {
      dollarBodyIds.add(occurrence.id);
    }
  }

  // classify every incomplete file gap
  for (const file of reportParts.files) {
    // retain one incomplete parse
    if (file.coverage.sourceParseCompleted === false) {
      parseIncompletePaths.add(file.sourcePath);
    }

    // retain one incomplete sql lex
    if (file.coverage.sqlLexingCompleted === false) {
      sqlLexIncompletePaths.add(file.sourcePath);
    }
  }

  appendGap(rows, "catalog.binding-not-performed", "report", ["report"]);
  appendGap(rows, "reference-flow.unproven", "report", ["report"]);
  appendGap(rows, "physical-coverage.unproven", "report", ["report"]);
  appendGap(rows, "operation-reachability.unproven", "report", ["report"]);
  appendGap(rows, "http.handler-roots-unmapped", "operation-id", httpOperations);
  appendGap(
    rows,
    "node.branch-reachability-unproven",
    "operation-id",
    nodeOperations,
  );
  appendGap(
    rows,
    "node.direct-root-import-closure-missing",
    "source-path",
    directRoots,
  );
  appendGap(rows, "node.root-unresolved", "operation-id", unresolvedNodes);
  appendGap(rows, "source.unsupported-python", "source-path", pythonPaths);
  appendGap(rows, "source.unsupported-shell", "source-path", shellPaths);
  appendGap(
    rows,
    "source.unsupported-operational-declaration",
    "source-path",
    operationalPaths,
  );
  appendGap(
    rows,
    "source.configuration-noncode",
    "source-path",
    configurationPaths,
  );
  appendGap(
    rows,
    "source.unsupported-extension",
    "source-path",
    unsupportedPaths,
  );
  appendGap(
    rows,
    "source.sensitive-or-forbidden-root",
    "source-path",
    sensitivePaths,
  );
  appendGap(rows, "raw-sql.roots-unmapped", "report", ["report"]);
  appendGap(rows, "runtime-opened-sources.unmapped", "report", ["report"]);
  appendGap(
    rows,
    "embedded-sql.context-policy-limited",
    "report",
    ["report"],
  );
  appendGap(rows, "dynamic-code.unresolved", "source-path", dynamicCodePaths);
  appendGap(
    rows,
    "sql.dynamic-fragments-unresolved",
    "source-path",
    dynamicFragmentPaths,
  );
  appendGap(
    rows,
    "sql.dollar-bodies-unresolved",
    "occurrence-id",
    dollarBodyIds,
  );
  appendGap(
    rows,
    "sql.typed-current-contexts-unresolved",
    "source-path",
    typedCurrentPaths,
  );
  appendGap(
    rows,
    "source.parse-incomplete",
    "source-path",
    parseIncompletePaths,
  );
  appendGap(
    rows,
    "source.sql-lex-incomplete",
    "source-path",
    sqlLexIncompletePaths,
  );
  appendGap(rows, "metrics.not-ready", "report", ["report"]);
  return rows;
}

// build the fixed-root repository gap report
export async function buildBackendClockCatalogReport(...argumentsList) {
  // reject every caller override
  if (argumentsList.length !== 0) {
    throw new TypeError("buildBackendClockCatalogReport accepts no arguments");
  }
  const rootDir = resolve(import.meta.dirname, "..");
  const inventory = await loadInventory(rootDir);
  const workload = await loadWorkload(rootDir);
  const extractBackendClockOccurrences = await loadExtractor();
  let rootRealPath;

  try {
    rootRealPath = await realpath(rootDir);
  } catch {
    throw reportError("report.source-read");
  }
  const selection = await selectSources(
    inventory,
    workload.definition,
    rootRealPath,
  );
  const scanned = await scanSelectedSources(
    selection,
    rootRealPath,
    extractBackendClockOccurrences,
  );
  const reportParts = {
    operationRootHints: selection.operationRootHints,
    exclusions: selection.exclusions,
    files: scanned.files,
    occurrences: scanned.occurrences,
    diagnostics: scanned.diagnostics,
  };
  const report = {
    contractVersion: BACKEND_CLOCK_CATALOG_REPORT_VERSION,
    reportKind: "repository-source-clock-gap-report",
    status: "incomplete",
    validationVerdict: "not-evaluated",
    inputs: {
      inventory: {
        generation: "node-v1",
        phase: "baseline",
        manifestSha256: inventory.manifestDigest,
        runtimeMapSha256: inventory.runtimeMapDigest,
        entryCount: inventory.discovered.length,
        artifactCount: inventory.runtimeMapArtifactCount,
        bindingCount: inventory.runtimeMapBindingCount,
        contextCount: inventory.runtimeMapContextCount,
      },
      workload: {
        contractVersion: WORKLOAD_CONTRACT_VERSION,
        definitionSha256: workload.validation.definitionSha256,
        operationCount: workload.definition.operations.length,
        groupCount: workload.definition.groups.length,
        typedSlotCount: workload.definition.counts.typedSlots,
        transitionCount: workload.definition.counts.transitions,
      },
    },
    scope: {
      repositoryRootPolicy: "module-relative-fixed",
      parserVersion: EXPECTED_TYPESCRIPT_VERSION,
      embeddedSqlPolicy: "bounded-context-or-anchor/v1",
      perFileByteLimit: PER_FILE_BYTE_LIMIT,
      aggregateByteLimit: AGGREGATE_BYTE_LIMIT,
      supportedExtensions: [...SUPPORTED_EXTENSIONS],
      supplementalRoots: [...SUPPLEMENTAL_ROOTS],
      staticGraphPathCount: selection.staticPathCount,
      staticGraphScannedFileCount: selection.staticScannedCount,
      staticGraphExcludedPathCount: selection.staticExcludedCount,
      directOperationRootFileCount: selection.directOperationRootFileCount,
      supplementalRootCount: SUPPLEMENTAL_ROOTS.length,
      supplementalUniqueAddedFileCount:
        selection.supplementalUniqueAddedFileCount,
      scannedUniqueFileCount: scanned.files.length,
      scannedByteCount: scanned.scannedByteCount,
      exclusionCount: selection.exclusions.length,
    },
    operationRootHints: selection.operationRootHints,
    files: scanned.files,
    exclusions: selection.exclusions,
    occurrences: scanned.occurrences,
    diagnostics: scanned.diagnostics,
    gaps: buildGapRows(reportParts),
    canonicalEligible: false,
    referenceFlowProven: false,
    physicalCoverageProven: false,
    operationReachabilityProven: false,
    readyForMetrics: false,
  };
  return deepFreeze(report);
}

// render the fixed direct cli
async function main() {
  // reject every command argument
  if (process.argv.length !== 2) {
    process.stderr.write("usage: backend-clock-catalog-report.mjs\n");
    process.exitCode = 2;
    return;
  }

  try {
    const report = await buildBackendClockCatalogReport();
    process.stdout.write(`${JSON.stringify(report, null, 2)}\n`);
    process.exitCode = 3;
  } catch (error) {
    let code = "report.unexpected";

    // retain only one fixed known error code
    if (isReportError(error)) {
      code = error.code;
    }
    const errorReport = {
      contractVersion: ERROR_CONTRACT_VERSION,
      status: "error",
      code,
    };
    process.stderr.write(`${JSON.stringify(errorReport, null, 2)}\n`);
    process.exitCode = 1;
  }
}

// run only as the direct cli
if (import.meta.main) {
  await main();
}
