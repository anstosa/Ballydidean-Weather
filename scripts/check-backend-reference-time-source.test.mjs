import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import { constants as fsConstants } from "node:fs";
import {
  copyFile,
  link,
  lstat,
  mkdir,
  mkdtemp,
  readFile,
  readdir,
  readlink,
  rm,
  stat,
  symlink,
  unlink,
  writeFile,
} from "node:fs/promises";
import { isAbsolute, join, relative, sep } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

import { buildBackendClockCatalogReport } from "./backend-clock-catalog-report.mjs";
import { buildBackendReferenceTimeSourceGate } from "./check-backend-reference-time-source.mjs";
import {
  collectTestPlan,
  fastExcludedFiles,
  filterFastTests,
} from "./test-workspaces.mjs";
import { ADJUSTMENT_FIT_PUBLIC_CODE_ALLOWLIST } from "./research/adjustment_fit_inputs.mjs";

const REPOSITORY_ROOT = fileURLToPath(new URL("..", import.meta.url));
const CLI_PATH = fileURLToPath(
  new URL("./check-backend-reference-time-source.mjs", import.meta.url),
);
const USAGE = "usage: node scripts/check-backend-reference-time-source.mjs --gate <rt-u04|conditional-seam|rt-u16|all> --scope <current-source|implemented-source|physical-baseline>\n";
const OWNED_TEST_FILES = [
  "scripts/backend-nonmodule-source-analysis.test.mjs",
  "scripts/backend-operation-source-graph.test.mjs",
  "scripts/backend-reference-time-source-proof.test.mjs",
  "scripts/check-backend-reference-time-source.test.mjs",
];
const SOURCE_MODULES = [
  "scripts/backend-nonmodule-source-analysis.mjs",
  "scripts/backend-operation-source-graph.mjs",
  "scripts/backend-reference-time-source-proof.mjs",
  "scripts/check-backend-reference-time-source.mjs",
];
const PERMISSION_WARNING_PATTERN = /^\(node:[1-9][0-9]*\) SecurityWarning: The flag --allow-child-process must be used with extreme caution\. It could invalidate the permission model\.\n\(Use `node --trace-warnings \.\.\.` to show where the warning was created\)\n$/u;
const EARLY_ERROR_KEYS = ["contractVersion", "diagnostics", "stage", "verdict"];
const AGGREGATE_KEYS = [
  "conditionalSeamLemmaSha256",
  "contractVersion",
  "rtU04Sha256",
  "rtU16Sha256",
  "scope",
  "verdict",
  "verdicts",
];
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
const FIXTURE_EXCLUDED_PATHS = [
  ".cache",
  ".git",
  ".omx",
  ".weather-data",
  "coverage",
  "deploy/backups",
  "deploy/releases",
  "deploy/secrets",
  "deploy/state",
  "deploy/test",
  "dist",
  "node_modules",
];
const HARD_LINK_FALLBACK_CODES = new Set(["EMLINK", "EPERM", "EXDEV"]);
const MAX_FIXTURE_FALLBACK_COPY_BYTES = 64 * 1024 * 1024;

let currentGateRunsPromise;

// hash exact bytes
function sha256(value) {
  return createHash("sha256").update(value).digest("hex");
}

// canonicalize emitted documents
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

// require one direct gate document
function assertDirectGateDocument(value, keys, contractVersion) {
  assertExactKeys(value, keys);
  assert.equal(value.contractVersion, contractVersion);
  assert.equal(value.scope, "current-source");
  assert.equal(Object.hasOwn(value, "document"), false);
  assert.equal(Object.hasOwn(value, "exitCode"), false);
  assert.equal(Object.hasOwn(value, "graph"), false);
  assert.equal(Object.hasOwn(value, "debug"), false);
  assertDeepFrozen(value);
}

// require canonical json with one lf
function assertCanonicalDocument(bytes) {
  const text = bytes.toString("utf8");
  assert.equal(text.endsWith("\n"), true);
  assert.equal(text.endsWith("\n\n"), false);
  const value = JSON.parse(text);
  assert.deepEqual(bytes, Buffer.from(`${canonicalJson(value)}\n`, "utf8"));
  return value;
}

// require exact node child warning bytes
function assertPermissionWarning(bytes) {
  const text = bytes.toString("utf8");
  assert.deepEqual(Buffer.from(text, "utf8"), bytes);
  assert.match(text, PERMISSION_WARNING_PATTERN);
}

// run one fixed validator command
function runCli(argumentsList, options = {}) {
  const nodeArguments = [];
  // enable the reviewed permission invocation
  if (options.permission !== false) {
    nodeArguments.push(
      "--permission",
      `--allow-fs-read=${options.allowRead ?? REPOSITORY_ROOT}`,
      "--allow-child-process",
    );
  }
  return spawnSync(
    process.execPath,
    [...nodeArguments, options.cliPath ?? CLI_PATH, ...argumentsList],
    {
      cwd: options.cwd ?? REPOSITORY_ROOT,
      encoding: null,
      env: options.env ?? process.env,
      maxBuffer: 64 * 1024 * 1024,
    },
  );
}

// derive one contained fixture path
function containedRelativePath(root, path) {
  const relativePath = relative(root, path);
  // reject every root escape
  if (
    isAbsolute(relativePath)
    || relativePath === ".."
    || relativePath.startsWith(`..${sep}`)
  ) {
    throw new Error("fixture path escapes source root");
  }
  return relativePath;
}

// classify clean-checkout exclusions
function skipFixtureEntry(name, sourcePath, destinationPath) {
  // stop recursive fixture self-copy
  if (sourcePath === destinationPath || name.startsWith(".source-gate-fixture-")) {
    return true;
  }
  const relativePath = containedRelativePath(REPOSITORY_ROOT, sourcePath);
  // retain only validator-relevant repository roots
  if (!relativePath.includes("/") && !new Set([
    "Dockerfile",
    "apps",
    "config",
    "deploy",
    "package-lock.json",
    "package.json",
    "packages",
    "scripts",
    "tsconfig.base.json",
  ]).has(name)) {
    return true;
  }
  // omit non-source deployment custody
  if ([
    "deploy/backups",
    "deploy/releases",
    "deploy/secrets",
    "deploy/state",
    "deploy/test",
  ].some(
    // match one excluded deployment subtree
    (prefix) => relativePath === prefix || relativePath.startsWith(`${prefix}/`),
  )) {
    return true;
  }
  return new Set([
    ".git",
    ".omx",
    ".weather-data",
    ".cache",
    "coverage",
    "dist",
    "node_modules",
  ]).has(name);
}

// retain one fixture regular file
async function retainFixtureFile(
  sourcePath,
  destinationPath,
  fallbackCopyState,
) {
  // prefer zero-copy fixture custody
  try {
    await link(sourcePath, destinationPath);
    return;
  } catch (error) {
    // reject unrelated hard-link failures
    if (!HARD_LINK_FALLBACK_CODES.has(error?.code)) {
      throw error;
    }
  }

  const sourceStats = await stat(sourcePath);
  // require one bounded regular fallback source
  if (
    !sourceStats.isFile()
    || !Number.isSafeInteger(sourceStats.size)
    || sourceStats.size < 0
  ) {
    throw new TypeError("fixture fallback source is invalid");
  }
  const nextCopiedBytes = fallbackCopyState.copiedBytes + sourceStats.size;
  // reject fixture copy expansion before allocation
  if (
    !Number.isSafeInteger(nextCopiedBytes)
    || nextCopiedBytes > MAX_FIXTURE_FALLBACK_COPY_BYTES
  ) {
    throw new RangeError("fixture fallback copy exceeds 64 MiB");
  }
  await copyFile(sourcePath, destinationPath, fsConstants.COPYFILE_EXCL);
  const destinationStats = await stat(destinationPath);
  // reject copy size drift
  if (destinationStats.size !== sourceStats.size) {
    throw new Error("fixture fallback copy size changed");
  }
  fallbackCopyState.copiedBytes = nextCopiedBytes;
}

// build one clean fixture checkout tree
async function linkFixtureTree(
  sourceRoot,
  destinationRoot,
  sourcePath,
  fallbackCopyState,
) {
  const relativePath = containedRelativePath(sourceRoot, sourcePath);
  const destinationPath = relativePath === ""
    ? destinationRoot
    : join(destinationRoot, relativePath);
  await mkdir(destinationPath, { recursive: true });
  const entries = await readdir(sourcePath, { withFileTypes: true });
  // retain every clean source entry
  for (const entry of entries) {
    const sourceEntry = join(sourcePath, entry.name);
    const destinationEntry = join(destinationPath, entry.name);
    // omit ignored runtime material
    if (skipFixtureEntry(entry.name, sourceEntry, destinationRoot)) {
      continue;
    }
    // recurse through ordinary directories
    if (entry.isDirectory()) {
      await linkFixtureTree(
        sourceRoot,
        destinationRoot,
        sourceEntry,
        fallbackCopyState,
      );
      continue;
    }
    // preserve repository symlinks
    if (entry.isSymbolicLink()) {
      await symlink(await readlink(sourceEntry), destinationEntry);
      continue;
    }
    // retain regular bytes with bounded fallback
    if (entry.isFile()) {
      await retainFixtureFile(sourceEntry, destinationEntry, fallbackCopyState);
    }
  }
}

// require one bounded fixture layout
async function assertCleanFixtureLayout(fixtureRoot) {
  assert.equal(containedRelativePath(fixtureRoot, fixtureRoot), "");
  const validator = await lstat(
    join(fixtureRoot, "scripts/check-backend-reference-time-source.mjs"),
  );
  assert.equal(validator.isFile(), true);
  // require every runtime exclusion
  for (const relativePath of FIXTURE_EXCLUDED_PATHS) {
    await assert.rejects(
      // probe only one excluded path
      () => lstat(join(fixtureRoot, relativePath)),
      { code: "ENOENT" },
    );
  }
}

// create one clean checkout fixture
async function cleanFixtureCheckout() {
  const root = await mkdtemp(join(REPOSITORY_ROOT, ".source-gate-fixture-"));
  const fallbackCopyState = { copiedBytes: 0 };
  // remove every partial checkout failure
  try {
    await linkFixtureTree(
      REPOSITORY_ROOT,
      root,
      REPOSITORY_ROOT,
      fallbackCopyState,
    );
    await assertCleanFixtureLayout(root);
    return root;
  } catch (error) {
    // remove only the owned partial root
    await rm(root, { force: true, recursive: true });
    throw error;
  }
}

// run one disposable clean checkout fixture
async function withCleanFixture(action) {
  const fixtureRoot = await cleanFixtureCheckout();
  // always remove fixture links
  try {
    return await action(fixtureRoot);
  } finally {
    await rm(fixtureRoot, { force: true, recursive: true });
  }
}

// build one fixture cli option set
function fixtureCliOptions(fixtureRoot) {
  return {
    allowRead: [fixtureRoot, join(REPOSITORY_ROOT, "node_modules")].join(","),
    cliPath: join(fixtureRoot, "scripts/check-backend-reference-time-source.mjs"),
    cwd: fixtureRoot,
  };
}

// execute the four current-source gates once
async function currentGateRuns() {
  // retain one expensive current-source run set
  if (currentGateRunsPromise === undefined) {
    currentGateRunsPromise = Promise.resolve().then(
      // run gates in deterministic order
      () => ({
        aggregate: runCli(["--gate", "all", "--scope", "current-source"]),
        lemma: runCli(["--gate", "conditional-seam", "--scope", "current-source"]),
        rtU04: runCli(["--gate", "rt-u04", "--scope", "current-source"]),
        rtU16: runCli(["--gate", "rt-u16", "--scope", "current-source"]),
      }),
    );
  }
  return currentGateRunsPromise;
}

// project diagnostic codes
function diagnosticCodes(document) {
  return document.diagnostics.map(
    // retain one closed code
    (diagnostic) => diagnostic.code,
  );
}

test(
  "buildBackendReferenceTimeSourceGate accepts only exact gate and scope arguments",
  // verify the closed in-process boundary
  async () => {
    assert.equal(typeof buildBackendReferenceTimeSourceGate, "function");
    await assert.rejects(
      // reject an unknown gate
      () => buildBackendReferenceTimeSourceGate("unknown", "current-source"),
    );
    await assert.rejects(
      // reject an unknown scope
      () => buildBackendReferenceTimeSourceGate("rt-u04", "unknown"),
    );
    await assert.rejects(
      // reject a root override
      () => buildBackendReferenceTimeSourceGate("rt-u04", "current-source", {
        root: "/tmp/fixture",
      }),
    );
    await assert.rejects(
      // reject an omitted scope
      () => buildBackendReferenceTimeSourceGate("rt-u04"),
    );

    const rtU04 = await buildBackendReferenceTimeSourceGate("rt-u04", "current-source");
    const conditionalSeam = await buildBackendReferenceTimeSourceGate(
      "conditional-seam",
      "current-source",
    );
    const rtU16 = await buildBackendReferenceTimeSourceGate("rt-u16", "current-source");
    const selected = await buildBackendReferenceTimeSourceGate("all", "current-source");
    assertDirectGateDocument(
      rtU04,
      RT_U04_KEYS,
      "weather-backend-clock-source-closure-proof/v1",
    );
    assertDirectGateDocument(
      conditionalSeam,
      LEMMA_KEYS,
      "weather-backend-reference-seam-lemma/v1",
    );
    assertDirectGateDocument(
      rtU16,
      RT_U16_KEYS,
      "weather-backend-reference-flow-proof/v1",
    );
    assertDirectGateDocument(
      selected,
      AGGREGATE_KEYS,
      "weather-backend-reference-time-source-gates/v1",
    );
    assert.notStrictEqual(rtU04, conditionalSeam);
    assert.notStrictEqual(conditionalSeam, rtU16);
    assert.notStrictEqual(rtU16, selected);
  },
);

test(
  "CLI usage errors emit only the exact usage line with exit two",
  // verify argument misuse streams
  () => {
    const cases = [
      [],
      ["--gate", "unknown", "--scope", "current-source"],
      ["--gate", "rt-u04", "--scope", "unknown"],
      ["--gate", "rt-u04", "--scope", "current-source", "--root", "/tmp"],
    ];
    // verify every invalid argument shape
    for (const argumentsList of cases) {
      const run = runCli(argumentsList, { permission: false });
      assert.equal(run.status, 2);
      assert.deepEqual(run.stdout, Buffer.alloc(0));
      assert.deepEqual(run.stderr, Buffer.from(USAGE, "utf8"));
    }
  },
);

test(
  "Node child permission stderr accepts only the exact two-line warning",
  // verify raw warning protocol
  () => {
    const valid = Buffer.from(
      "(node:1234) SecurityWarning: The flag --allow-child-process must be used with extreme caution. It could invalidate the permission model.\n(Use `node --trace-warnings ...` to show where the warning was created)\n",
      "utf8",
    );
    assert.doesNotThrow(
      // accept only the reviewed bytes
      () => assertPermissionWarning(valid),
    );
    const mutations = [
      Buffer.alloc(0),
      Buffer.from(valid.toString("utf8").replace("1234", "0")),
      Buffer.from(valid.toString("utf8").replaceAll("\n", "\r\n")),
      Buffer.concat([valid, Buffer.from("ordinary stderr\n")]),
      Buffer.concat([valid, Buffer.from("(node:4) SecurityWarning: extra\n")]),
      Buffer.from(valid.toString("utf8").replace("must", "should")),
    ];
    // reject every changed warning byte stream
    for (const mutation of mutations) {
      assert.throws(
        // validate one hostile stderr mutation
        () => assertPermissionWarning(mutation),
      );
    }
    assert.equal(process.version, "v24.16.0");
  },
);

test(
  "reader permission denial emits the sanitized early-error envelope",
  // verify pre-proof failure streams
  () => {
    const allowed = [
      join(REPOSITORY_ROOT, "scripts"),
      join(REPOSITORY_ROOT, "config"),
      join(REPOSITORY_ROOT, "node_modules"),
      join(REPOSITORY_ROOT, "packages"),
      join(REPOSITORY_ROOT, "deploy"),
      join(REPOSITORY_ROOT, "Dockerfile"),
      join(REPOSITORY_ROOT, "package.json"),
      join(REPOSITORY_ROOT, "package-lock.json"),
      join(REPOSITORY_ROOT, "tsconfig.base.json"),
    ].join(",");
    const run = runCli(
      ["--gate", "rt-u04", "--scope", "current-source"],
      { allowRead: allowed },
    );
    assert.equal(run.status, 1);
    assertPermissionWarning(run.stderr);
    const envelope = assertCanonicalDocument(run.stdout);
    assertExactKeys(envelope, EARLY_ERROR_KEYS);
    assert.equal(envelope.stage, "reader");
    assert.equal(envelope.verdict, "FAIL");
    assert.deepEqual(diagnosticCodes(envelope), ["reader.unsafe-input"]);
    assert.equal(Object.hasOwn(envelope, "graphSha256"), false);
    assert.equal(Object.hasOwn(envelope, "policySha256"), false);
    assert.equal(JSON.stringify(envelope).includes(REPOSITORY_ROOT), false);
    assert.equal(JSON.stringify(envelope).includes("ERR_ACCESS_DENIED"), false);
  },
);

test(
  "current source emits canonical truthful child and aggregate documents",
  // verify current repository foundation results
  async () => {
    const runs = await currentGateRuns();
    assert.equal(runs.rtU04.status, 3);
    assert.equal(runs.lemma.status, 0);
    assert.equal(runs.rtU16.status, 3);
    assert.equal(runs.aggregate.status, 3);
    assertPermissionWarning(runs.rtU04.stderr);
    assertPermissionWarning(runs.lemma.stderr);
    assertPermissionWarning(runs.rtU16.stderr);
    assertPermissionWarning(runs.aggregate.stderr);

    const rtU04 = assertCanonicalDocument(runs.rtU04.stdout);
    const lemma = assertCanonicalDocument(runs.lemma.stdout);
    const rtU16 = assertCanonicalDocument(runs.rtU16.stdout);
    const aggregate = assertCanonicalDocument(runs.aggregate.stdout);
    assert.equal(rtU04.contractVersion, "weather-backend-clock-source-closure-proof/v1");
    assert.equal(lemma.contractVersion, "weather-backend-reference-seam-lemma/v1");
    assert.equal(rtU16.contractVersion, "weather-backend-reference-flow-proof/v1");
    assert.equal(aggregate.contractVersion, "weather-backend-reference-time-source-gates/v1");
    assert.equal(rtU04.verdict, "INCOMPLETE");
    assert.equal(lemma.verdict, "PASS");
    assert.equal(rtU16.verdict, "INCOMPLETE");
    assert.equal(aggregate.verdict, "INCOMPLETE");
    assert.deepEqual(aggregate.verdicts, {
      conditionalSeam: "PASS",
      rtU04: "INCOMPLETE",
      rtU16: "INCOMPLETE",
    });
    assert.deepEqual(diagnosticCodes(rtU04), [
      "prerequisite.full-evaluator-source-missing",
    ]);
    assert.deepEqual(diagnosticCodes(rtU16), [
      "prerequisite.descriptor-lineage-missing",
      "prerequisite.reference-consumers-incomplete",
    ]);
    const w3 = rtU04.decisionProofs.find(
      // locate the shared http deadline proof
      (decision) => decision.decisionId === "worker.W3.deadlineRetry",
    );
    const b2 = rtU04.decisionProofs.find(
      // locate the local backfill proof
      (decision) => decision.decisionId === "oneshot.B2.backfillOperations",
    );
    const r1 = rtU04.decisionProofs.find(
      // locate the rain reference-window proof
      (decision) => decision.decisionId === "worker.R1.rainEligibility",
    );
    assert.equal(w3.status, "matched");
    assert.equal(w3.siteIds.length, 5);
    assert.equal(w3.computedOperations.length, 29);
    assert.deepEqual(w3.computedOperations.slice(-4), [
      "oneshot.tide-backfill",
      "oneshot.tempest-backfill",
      "oneshot.public-station-backfill",
      "oneshot.general-backfill",
    ]);
    assert.equal(b2.status, "matched");
    assert.equal(b2.siteIds.length, 4);
    assert.equal(b2.computedOperations.length, 4);
    assert.equal(
      b2.occurrenceIds.some(
        // forbid shared timer dual binding
        (occurrenceId) => w3.occurrenceIds.includes(occurrenceId),
      ),
      false,
    );
    const sharedHttpOccurrenceIds = [
      "clock-occurrence-v1:0f60c2fad428825ed464bf4063e1ea70ef2c8de4784095c8e1be10f39a6f31b4",
      "clock-occurrence-v1:2823138faf536c11db392132e7e8d44d526edac74a2db13ba7cbf250f96edf7a",
      "clock-occurrence-v1:292b34729c07b166a7013789733a8adcb1b2edf06f4e2e36e40333843d66aeb5",
    ];
    const sharedHttpAdjudications = sharedHttpOccurrenceIds.map(
      // locate each physical shared timer
      (occurrenceId) => rtU04.adjudications.find(
        // match one physical occurrence identity
        (adjudication) => adjudication.occurrenceId === occurrenceId,
      ),
    );
    assert.equal(
      sharedHttpAdjudications.every(
        // bind every shared timer only to w3
        (adjudication) => adjudication.decisionId === "worker.W3.deadlineRetry",
      ),
      true,
    );
    const backfillOperationIds = [
      "oneshot.tide-backfill",
      "oneshot.tempest-backfill",
      "oneshot.public-station-backfill",
      "oneshot.general-backfill",
    ];
    assert.equal(
      backfillOperationIds.every(
        // retain unconditional line-198 reachability
        (operationId) => sharedHttpAdjudications[0].operationIds.includes(operationId),
      ),
      true,
    );
    assert.equal(
      sharedHttpAdjudications.slice(1).every(
        // forbid invented retry and stall activation
        (adjudication) => backfillOperationIds.every(
          // reject each unproved backfill operation
          (operationId) => !adjudication.operationIds.includes(operationId),
        ),
      ),
      true,
    );
    assert.equal(r1.siteIds.length, 2);
    assert.equal(
      rtU16.descriptorSeedSiteIds.some(
        // forbid current provider-to-reference relabeling
        (siteId) => r1.siteIds.includes(siteId),
      ),
      false,
    );
    const h0 = rtU04.measurementCoverage.find(
      // locate one-shot measurement coverage
      (coverage) => coverage.decisionId === "oneshot.H0.harnessDeadline",
    );
    const m0 = rtU04.measurementCoverage.find(
      // locate full-workload measurement coverage
      (coverage) => coverage.decisionId === "harness.M0.measurement",
    );
    assert.equal(h0.requiredOperationIds.length, 18);
    assert.equal(m0.requiredOperationIds.length, 107);
    assert.notEqual(h0.status, "complete");
    assert.notEqual(m0.status, "complete");
    assert.equal(aggregate.rtU04Sha256, sha256(runs.rtU04.stdout));
    assert.equal(aggregate.rtU16Sha256, sha256(runs.rtU16.stdout));
    assert.equal(aggregate.conditionalSeamLemmaSha256, sha256(runs.lemma.stdout));
    assert.equal(JSON.stringify(rtU04).includes("sourceText"), false);
    assert.equal(JSON.stringify(rtU04).includes("literalValue"), false);
    assert.equal(JSON.stringify(rtU16).includes(".omx"), false);
    const diagnosticIds = [...rtU04.diagnostics, ...rtU16.diagnostics].flatMap(
      // retain every public diagnostic identity
      (diagnostic) => diagnostic.ids,
    );
    assert.equal(
      diagnosticIds.every(
        // forbid private helper and fact identities
        (id) => !id.includes("fact")
          && !id.includes("child")
          && !id.includes("batch"),
      ),
      true,
    );

    const secondAggregate = runCli(["--gate", "all", "--scope", "current-source"]);
    assert.equal(secondAggregate.status, 3);
    assertPermissionWarning(secondAggregate.stderr);
    assert.deepEqual(secondAggregate.stdout, runs.aggregate.stdout);
  },
);

test(
  "current proof retains every approved physical V1 occurrence identity",
  // verify existing occurrence preservation
  async () => {
    const [runs, report] = await Promise.all([
      currentGateRuns(),
      buildBackendClockCatalogReport(),
    ]);
    const rtU04 = JSON.parse(runs.rtU04.stdout.toString("utf8"));
    assert.equal(report.occurrences.length, 2131);
    assert.equal(report.files.length, 186);
    assert.equal(
      report.files.some(
        // forbid python in the prior js and ts report
        (file) => file.sourcePath.endsWith(".py"),
      ),
      false,
    );
    const currentFileHashes = await Promise.all(report.files.map(
      // hash one current physical source
      async (file) => ({
        actualSha256: sha256(await readFile(new URL(`../${file.sourcePath}`, import.meta.url))),
        expectedSha256: file.sha256,
      }),
    ));
    assert.equal(
      currentFileHashes.every(
        // require every retained source hash
        (record) => record.actualSha256 === record.expectedSha256,
      ),
      true,
    );
    assert.equal(
      report.occurrences.filter(
        // retain clock candidates
        (occurrence) => occurrence.countsAsClockCandidate,
      ).length,
      287,
    );
    assert.equal(
      report.occurrences.filter(
        // retain parse-only noncandidates
        (occurrence) => !occurrence.countsAsClockCandidate,
      ).length,
      1844,
    );
    const adjudicatedIds = new Set(rtU04.adjudications.map(
      // retain each final occurrence identity
      (adjudication) => adjudication.occurrenceId,
    ));
    assert.equal(
      report.occurrences.every(
        // require every physical identity unchanged
        (occurrence) => adjudicatedIds.has(occurrence.id),
      ),
      true,
    );
    const embeddedSqlIds = [
      "clock-occurrence-v1:71f59884098f044b1dda65d89ed2e4325564d39db4f57e951da19d9e018ad882",
      "clock-occurrence-v1:0a4fe0bc0db0ddf92b9ed4163ba497f86ec64b9348ebe6a2fb3aca1359de3246",
      "clock-occurrence-v1:15b77159355e92603937c0a42a5ca7cff104092215f65df0a0c2faed5cdfafcc",
      "clock-occurrence-v1:d469c9648d0992268a66ce3bca81d887e7c61c4b24d6cc65e51bd307e392f200",
      "clock-occurrence-v1:e49f15faf3a0187764fd58ae56c98ae18cb900a041471b26be1e7c23bc69fa0f",
    ];
    const embeddedSql = embeddedSqlIds.map(
      // locate one frozen embedded sql record
      (id) => report.occurrences.find(
        // match one physical v1 identity
        (occurrence) => occurrence.id === id,
      ),
    );
    assert.equal(
      embeddedSql.every(
        // preserve the host language and custody
        (occurrence) => occurrence.language === "typescript"
          && occurrence.embeddedSql !== null
          && occurrence.surface === "postgres.clock-timestamp",
      ),
      true,
    );
    assert.deepEqual(
      // preserve frozen embedded sql owners
      embeddedSql.map((occurrence) => occurrence.enclosingSymbol),
      [
        "advanceScheduledCheckpoint",
        "upsertBackfillOutcome",
        "readPendingRainAdjustmentCaptures",
        "readPendingRainAdjustmentCaptures",
        "<anonymous>",
      ],
    );
    assert.equal(
      embeddedSql[2].normalizedExpressionSha256,
      "51e7c66204e2d4a419694870224836cac233ff925b4dd60741e13a1dd9b5ab5a",
    );
    assert.equal(
      embeddedSql[3].normalizedExpressionSha256,
      embeddedSql[2].normalizedExpressionSha256,
    );
    assert.notEqual(embeddedSql[2].duplicateOrdinal, embeddedSql[3].duplicateOrdinal);
    assert.notDeepEqual(embeddedSql[2].physicalSpan, embeddedSql[3].physicalSpan);
  },
);

test(
  "clean checkout excludes omx and reproduces identical source proof bytes",
  // verify clean checkout independence
  async () => {
    await withCleanFixture(
      // exercise one clean repository fixture
      async (fixtureRoot) => {
        await assert.rejects(
          // require absent ignored omx state
          () => lstat(join(fixtureRoot, ".omx")),
          { code: "ENOENT" },
        );
        const clean = runCli(
          ["--gate", "all", "--scope", "current-source"],
          fixtureCliOptions(fixtureRoot),
        );
        const current = (await currentGateRuns()).aggregate;
        assert.equal(clean.status, 3);
        assertPermissionWarning(clean.stderr);
        assert.deepEqual(clean.stdout, current.stdout);
      },
    );
  },
);

test(
  "reader finalization retains final frontend diagnostics without a provisional graph gap",
  // verify final diagnostic merge
  async () => {
    await withCleanFixture(
      // exercise one unsupported frontend fixture
      async (fixtureRoot) => {
        const options = fixtureCliOptions(fixtureRoot);
        const pythonVictim = join(fixtureRoot, "scripts/research/rain_refresh.py");
        const pythonSource = await readFile(pythonVictim, "utf8");
        await unlink(pythonVictim);
        await writeFile(
          pythonVictim,
          `${pythonSource}\nmatch 1:\n    case 1:\n        pass\n`,
        );
        const unsupported = runCli(
          ["--gate", "rt-u04", "--scope", "current-source"],
          options,
        );
        assert.equal(unsupported.status, 3);
        assertPermissionWarning(unsupported.stderr);
        const proof = assertCanonicalDocument(unsupported.stdout);
        assert.equal(diagnosticCodes(proof).includes("frontend.unsupported"), true);
        assert.equal(diagnosticCodes(proof).includes("reader.dynamic-unresolved"), false);
      },
    );
  },
);

test(
  "aggregate FAIL precedence preserves two incomplete children beside a failed lemma",
  // verify aggregate verdict precedence
  async () => {
    await withCleanFixture(
      // exercise one forbidden reference-flow mutant
      async (fixtureRoot) => {
        const options = fixtureCliOptions(fixtureRoot);
        const fixtureVictim = join(fixtureRoot, "apps/api/src/index.ts");
        const apiSource = await readFile(fixtureVictim, "utf8");
        const forbiddenSource = apiSource.replace(
          "validRainAdjustmentRun(run, safetyAt)",
          "validRainAdjustmentRun(run, dataAt)",
        );
        assert.notEqual(forbiddenSource, apiSource);
        await unlink(fixtureVictim);
        await writeFile(fixtureVictim, forbiddenSource);
        const precedence = runCli(
          ["--gate", "all", "--scope", "current-source"],
          options,
        );
        assert.equal(precedence.status, 1);
        assertPermissionWarning(precedence.stderr);
        const aggregate = assertCanonicalDocument(precedence.stdout);
        assert.equal(aggregate.verdict, "FAIL");
        assert.deepEqual(aggregate.verdicts, {
          conditionalSeam: "FAIL",
          rtU04: "INCOMPLETE",
          rtU16: "INCOMPLETE",
        });
      },
    );
  },
);

test(
  "reader rejects a required source symlink without leaking its real path",
  // verify reader symlink containment
  async () => {
    await withCleanFixture(
      // exercise one unsafe source fixture
      async (fixtureRoot) => {
        const options = fixtureCliOptions(fixtureRoot);
        const relativeVictim = "apps/api/src/index.ts";
        const fixtureVictim = join(fixtureRoot, relativeVictim);
        await unlink(fixtureVictim);
        await symlink(join(REPOSITORY_ROOT, relativeVictim), fixtureVictim);
        assert.equal((await lstat(fixtureVictim)).isSymbolicLink(), true);
        const unsafe = runCli(
          ["--gate", "rt-u04", "--scope", "current-source"],
          options,
        );
        assert.equal(unsafe.status, 1);
        assertPermissionWarning(unsafe.stderr);
        const envelope = assertCanonicalDocument(unsafe.stdout);
        assert.equal(envelope.verdict, "FAIL");
        assert.equal(
          diagnosticCodes(envelope).includes("reader.unsafe-input"),
          true,
        );
        assert.equal(JSON.stringify(envelope).includes(REPOSITORY_ROOT), false);
      },
    );
  },
);

test(
  "Python corpus selection derives exactly 26 paths from the exported allowlist",
  // verify source-owned python selection
  () => {
    const paths = Object.values(ADJUSTMENT_FIT_PUBLIC_CODE_ALLOWLIST).flat().map(
      // project one source-owned path
      (entry) => entry.source,
    ).filter(
      // retain only reviewed python sources
      (path) => path.endsWith(".py"),
    );
    const uniquePaths = [...new Set(paths)].sort();
    assert.equal(uniquePaths.length, 26);
    assert.equal(
      uniquePaths.every(
        // require only repository python paths
        (path) => path.endsWith(".py")
          && !path.startsWith("/")
          && !path.includes("..")
          && !path.includes(".omx"),
      ),
      true,
    );
  },
);

test(
  "source modules expose one API each and contain no omx runtime dependency",
  // verify clean-checkout module boundaries
  async () => {
    const modules = await Promise.all([
      import("./backend-nonmodule-source-analysis.mjs"),
      import("./backend-operation-source-graph.mjs"),
      import("./backend-reference-time-source-proof.mjs"),
      import("./check-backend-reference-time-source.mjs"),
    ]);
    assert.deepEqual(Object.keys(modules[0]), ["analyzeBackendNonmoduleSourcesBatch"]);
    assert.deepEqual(Object.keys(modules[1]), ["buildBackendOperationSourceGraph"]);
    assert.deepEqual(Object.keys(modules[2]), ["buildBackendReferenceTimeSourceProof"]);
    assert.deepEqual(Object.keys(modules[3]), ["buildBackendReferenceTimeSourceGate"]);

    const contents = await Promise.all(SOURCE_MODULES.map(
      // read one checked source module
      (path) => readFile(new URL(`../${path}`, import.meta.url), "utf8"),
    ));
    // inspect every source module
    for (const source of contents) {
      assert.equal(source.includes(".omx"), false);
      assert.equal(source.includes("NODE_NO_WARNINGS"), false);
      assert.equal(source.includes("--no-warnings"), false);
      assert.equal(source.includes("--disable-warning"), false);
    }
  },
);

test(
  "all four direct tests remain in full and fast workspace plans",
  // verify existing ci discovery
  async () => {
    const full = await collectTestPlan(REPOSITORY_ROOT);
    const fast = filterFastTests(full);
    const fullFiles = new Set(full.flatMap(
      // retain every full-plan file
      (group) => group.files,
    ));
    const fastFiles = new Set(fast.flatMap(
      // retain every fast-plan file
      (group) => group.files,
    ));
    // require each owned direct test
    for (const path of OWNED_TEST_FILES) {
      assert.equal(fullFiles.has(path), true, path);
      assert.equal(fastFiles.has(path), true, path);
      assert.equal(fastExcludedFiles.includes(path), false, path);
    }
  },
);
