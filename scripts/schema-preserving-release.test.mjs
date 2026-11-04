import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import {
  chmodSync,
  mkdtempSync,
  readFileSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import test from "node:test";
import {
  CONTROL_SHA256,
  CONTROL_VERSION,
  CONTRACT_VERSION,
  LEGACY_CONTRACT_VERSION,
  APP_QUERY_TRANSITION,
  HISTORY_SHA256,
  SOURCE_COMMIT,
  SOURCE_ENV_SHA256,
  SOURCE_RELEASE,
  STAGE_CONTRACT_VERSION,
  UPDATE_SHA256,
  readPrivateJson,
  validateDate,
  validateProtectedTrees,
  validatePublication,
  validateReleaseName,
  validateStageReceipt,
  writeJsonCreateOnce,
} from "./schema-preserving-release.mjs";

const SCRIPT = new URL("./schema-preserving-release.mjs", import.meta.url).pathname;
const HASH_A = "a".repeat(64);
const HASH_B = "b".repeat(64);
const TARGET_COMMIT = "c".repeat(40);

// build the minimum protected tree
function validTree() {
  return [
    { path: "Dockerfile", sha256: HASH_A },
    { path: "deploy/scripts/common.sh", sha256: HASH_A },
    { path: "deploy/scripts/update.sh", sha256: HASH_A },
    { path: "package-lock.json", sha256: HASH_A },
    {
      path: "packages/database/migrations/0020_adjustment_revision_frontier.sql",
      sha256: HASH_A,
    },
    {
      path: "packages/database/migrations/0023_forecast_product_read_index.sql",
      sha256: HASH_A,
    },
    {
      path: "packages/forecast-adjustment/src/maintenance-capture-epoch.ts",
      sha256: HASH_A,
    },
  ];
}

// build a valid publication proof
function validPublication() {
  const sourceTree = validTree();
  return {
    check: {
      conclusion: "success",
      databaseId: 123456,
      event: "push",
      headSha: TARGET_COMMIT,
      name: "Check",
      status: "completed",
      url: "https://github.com/anstosa/Ballydidean-Weather/actions/runs/123456",
    },
    contractVersion: CONTRACT_VERSION,
    operatorSha256: HASH_A,
    publish: {
      conclusion: "success",
      databaseId: 123457,
      event: "push",
      headBranch: "2026.11.03-4",
      headSha: TARGET_COMMIT,
      name: "Publish production images",
      status: "completed",
      url: "https://github.com/anstosa/Ballydidean-Weather/actions/runs/123457",
    },
    serverImage: `ghcr.io/anstosa/weather-server@sha256:${HASH_A}`,
    serverImageId: `sha256:${HASH_A}`,
    sourceTree,
    targetCommit: TARGET_COMMIT,
    targetRelease: "2026.11.03-4",
    targetTree: structuredClone(sourceTree),
    validatorSha256: HASH_B,
    webImage: `ghcr.io/anstosa/weather-web@sha256:${HASH_B}`,
    webImageId: `sha256:${HASH_B}`,
  };
}

// build a valid stage receipt
function validStageReceipt() {
  return {
    contractVersion: STAGE_CONTRACT_VERSION,
    protectedSha256: HASH_A,
    publicationSha256: HASH_B,
    sourceEnvironmentSha256: HASH_A,
    sourceRelease: SOURCE_RELEASE,
    targetEnvironmentSha256: HASH_B,
    targetRelease: "2026.11.03-4",
  };
}

// create one private json fixture
function privateJson(directory, name, value) {
  const path = join(directory, name);
  writeFileSync(path, JSON.stringify(value), { mode: 0o600 });
  chmodSync(path, 0o600);
  return path;
}

// run the command interface
function runCli(args, options = {}) {
  return spawnSync(process.execPath, [SCRIPT, ...args], {
    encoding: "utf8",
    ...options,
  });
}

// release values stay calendar-valid and ordered
test("release and anchor constants are exact", () => {
  assert.equal(SOURCE_RELEASE, "2026.11.03-3");
  assert.equal(SOURCE_COMMIT, "7e045d5bc69969e9247440ce3b70744e1f9ef6c4");
  assert.equal(
    SOURCE_ENV_SHA256,
    "7727f844ca07fdd7fbd7a717976628d5e4f860285d5fe527e990f13632eea72b",
  );
  assert.equal(CONTROL_VERSION, 17);
  assert.equal(
    CONTROL_SHA256,
    "6ea5cf5b770904bdfa557b29af2dd8899701d8f1b4a15b632f55ab5121032cbb",
  );
  assert.equal(
    UPDATE_SHA256,
    "ea1185e04e0da2439c856b5b6989ce4f87f087a63ff7bafa1781c577cf42b8a0",
  );
  assert.equal(
    HISTORY_SHA256,
    "1cfd6251eeb66d443e36d661299c9e49e73465fab55e5dab2cfa4ee86dd36ad2",
  );
  assert.equal(validateDate("2028-02-29"), "2028-02-29");
  assert.equal(validateReleaseName("2026.11.03-4"), "2026.11.03-4");
  assert.throws(() => validateDate("2027-02-29"), /real calendar date/u);
  assert.throws(() => validateReleaseName("2026.11.03-04"), /YYYY\.MM\.DD-N/u);
});

// closed publication shapes reject extra or missing evidence
test("publication requires exact closed shapes", () => {
  const publication = validPublication();
  assert.equal(validatePublication(publication, publication.targetRelease), publication);

  const extraTop = structuredClone(publication);
  extraTop.sourceCommit = SOURCE_COMMIT;
  assert.throws(() => validatePublication(extraTop), /unexpected fields/u);

  const extraWorkflow = structuredClone(publication);
  extraWorkflow.check.headBranch = publication.targetRelease;
  assert.throws(() => validatePublication(extraWorkflow), /unexpected fields/u);

  const missing = structuredClone(publication);
  delete missing.webImageId;
  assert.throws(() => validatePublication(missing), /unexpected fields/u);
});

// stage receipts bind closed immutable handoff evidence
test("stage receipt rejects contract release shape and digest drift", () => {
  const receipt = validStageReceipt();
  assert.equal(validateStageReceipt(receipt, receipt.targetRelease), receipt);

  const badContract = structuredClone(receipt);
  badContract.contractVersion = "weather-schema-preserving-stage/v2";
  assert.throws(() => validateStageReceipt(badContract, receipt.targetRelease), /contractVersion/u);

  const wrongTarget = structuredClone(receipt);
  wrongTarget.targetRelease = "2026.11.03-5";
  assert.throws(() => validateStageReceipt(wrongTarget, receipt.targetRelease), /requested release/u);

  const unknownField = structuredClone(receipt);
  unknownField.targetCommit = TARGET_COMMIT;
  assert.throws(() => validateStageReceipt(unknownField, receipt.targetRelease), /unexpected fields/u);

  const invalidSource = structuredClone(receipt);
  invalidSource.sourceRelease = "2026.02.30-1";
  assert.throws(() => validateStageReceipt(invalidSource, receipt.targetRelease), /real calendar date/u);

  const sameRelease = structuredClone(receipt);
  sameRelease.sourceRelease = receipt.targetRelease;
  assert.throws(() => validateStageReceipt(sameRelease, receipt.targetRelease), /must differ/u);

  const invalidDigest = structuredClone(receipt);
  invalidDigest.protectedSha256 = "sha256:invalid";
  assert.throws(() => validateStageReceipt(invalidDigest, receipt.targetRelease), /protectedSha256/u);
});

// workflow proof stays tied to one successful target push
test("publication rejects wrong release commit workflow and repository proof", () => {
  const wrongExpectedRelease = validPublication();
  assert.throws(
    () => validatePublication(wrongExpectedRelease, "2026.11.03-5"),
    /requested release/u,
  );

  const reusedRelease = validPublication();
  reusedRelease.targetRelease = SOURCE_RELEASE;
  reusedRelease.publish.headBranch = SOURCE_RELEASE;
  assert.throws(() => validatePublication(reusedRelease), /must follow/u);

  const reusedCommit = validPublication();
  reusedCommit.targetCommit = SOURCE_COMMIT;
  reusedCommit.check.headSha = SOURCE_COMMIT;
  reusedCommit.publish.headSha = SOURCE_COMMIT;
  assert.throws(() => validatePublication(reusedCommit), /targetCommit/u);

  const wrongHead = validPublication();
  wrongHead.check.headSha = "d".repeat(40);
  assert.throws(() => validatePublication(wrongHead), /headSha/u);

  const failed = validPublication();
  failed.check.conclusion = "failure";
  assert.throws(() => validatePublication(failed), /successful completed push/u);

  const pullRequest = validPublication();
  pullRequest.publish.event = "workflow_dispatch";
  assert.throws(() => validatePublication(pullRequest), /successful completed push/u);

  const wrongBranch = validPublication();
  wrongBranch.publish.headBranch = "main";
  assert.throws(() => validatePublication(wrongBranch), /headBranch/u);

  const wrongRepository = validPublication();
  wrongRepository.publish.url = "https://github.com/anstosa/weather/actions/runs/123457";
  assert.throws(() => validatePublication(wrongRepository), /url is invalid/u);

  const wrongRun = validPublication();
  wrongRun.check.url = "https://github.com/anstosa/Ballydidean-Weather/actions/runs/999";
  assert.throws(() => validatePublication(wrongRun), /url is invalid/u);
});

// image and bundle identities remain exact immutable digests
test("publication rejects mutable image and malformed sha identities", () => {
  const mutableImage = validPublication();
  mutableImage.serverImage = "ghcr.io/anstosa/weather-server:latest";
  assert.throws(() => validatePublication(mutableImage), /server image identity/u);

  const wrongRepository = validPublication();
  wrongRepository.webImage = `ghcr.io/anstosa/other-web@sha256:${HASH_B}`;
  assert.throws(() => validatePublication(wrongRepository), /web image identity/u);

  const shortId = validPublication();
  shortId.serverImageId = "sha256:abc";
  assert.throws(() => validatePublication(shortId), /server image identity/u);

  const badBundle = validPublication();
  badBundle.operatorSha256 = HASH_A.toUpperCase();
  assert.throws(() => validatePublication(badBundle), /bundle sha256/u);
});

// protected paths are canonical sorted unique and unchanged
test("protected tree validation rejects malformed paths and drift", () => {
  const sourceTree = validTree();
  assert.equal(validateProtectedTrees(sourceTree, structuredClone(sourceTree)), true);

  const unsorted = structuredClone(sourceTree);
  [unsorted[0], unsorted[1]] = [unsorted[1], unsorted[0]];
  assert.throws(() => validateProtectedTrees(unsorted, sourceTree), /sorted/u);

  const duplicate = structuredClone(sourceTree);
  duplicate.splice(1, 0, structuredClone(duplicate[0]));
  assert.throws(() => validateProtectedTrees(duplicate, sourceTree), /sorted/u);

  const unsafe = structuredClone(sourceTree);
  unsafe[0].path = "../Dockerfile";
  assert.throws(() => validateProtectedTrees(unsafe, sourceTree), /canonical protected/u);

  const unprotected = structuredClone(sourceTree);
  unprotected[0].path = "README.md";
  assert.throws(() => validateProtectedTrees(unprotected, sourceTree), /canonical protected/u);

  const missingSentinel = structuredClone(sourceTree);
  missingSentinel.splice(1, 1);
  assert.throws(() => validateProtectedTrees(missingSentinel, sourceTree), /missing required/u);

  const drift = structuredClone(sourceTree);
  drift[0].sha256 = HASH_B;
  assert.throws(() => validateProtectedTrees(sourceTree, drift), /drifted/u);
});

// v2 grants only the exact reviewed query transition while retained v1 stays strict
test("AQI query authorization cannot widen protected release contents", () => {
  const queryBytes = readFileSync(new URL(`../${APP_QUERY_TRANSITION.path}`, import.meta.url));
  assert.equal(createHash("sha256").update(queryBytes).digest("hex"), APP_QUERY_TRANSITION.targetSha256);
  const source = [...validTree(), { path: APP_QUERY_TRANSITION.path, sha256: APP_QUERY_TRANSITION.sourceSha256 }].sort(
    // preserve canonical tree ordering
    (left, right) => left.path < right.path ? -1 : 1,
  );
  const target = structuredClone(source);
  target.find((member) => member.path === APP_QUERY_TRANSITION.path).sha256 = APP_QUERY_TRANSITION.targetSha256;
  assert.equal(validateProtectedTrees(source, target), true);
  assert.throws(() => validateProtectedTrees(source, target, LEGACY_CONTRACT_VERSION), /drifted/u);
  const legacy = { ...validPublication(), contractVersion: LEGACY_CONTRACT_VERSION };
  assert.equal(validatePublication(legacy), legacy);
  const publication = { ...validPublication(), sourceTree: source, targetTree: target };
  assert.equal(validatePublication(publication), publication);
  assert.throws(() => validatePublication({ ...publication, contractVersion: LEGACY_CONTRACT_VERSION }), /drifted/u);
  const wrongSource = structuredClone(source);
  wrongSource.find((member) => member.path === APP_QUERY_TRANSITION.path).sha256 = HASH_A;
  assert.throws(() => validateProtectedTrees(wrongSource, target), /drifted/u);
  const wrongTarget = structuredClone(target);
  wrongTarget.find((member) => member.path === APP_QUERY_TRANSITION.path).sha256 = HASH_B;
  assert.throws(() => validateProtectedTrees(source, wrongTarget), /drifted/u);
  const secondDrift = structuredClone(target);
  secondDrift[0].sha256 = HASH_B;
  assert.throws(() => validateProtectedTrees(source, secondDrift), /drifted/u);
  const migrationDrift = structuredClone(target);
  migrationDrift.find((member) => member.path.endsWith("0023_forecast_product_read_index.sql")).sha256 = HASH_B;
  assert.throws(() => validateProtectedTrees(source, migrationDrift), /drifted/u);
  assert.throws(() => validateProtectedTrees(source, target.filter((member) => member.path !== APP_QUERY_TRANSITION.path)), /length drifted/u);
  const added = [...target, { path: "packages/database/src/unreviewed.ts", sha256: HASH_A }].sort(
    // keep added membership otherwise well formed
    (left, right) => left.path < right.path ? -1 : 1,
  );
  assert.throws(() => validateProtectedTrees(source, added), /length drifted/u);
});

// private reads enforce ownership shape and bounded metadata
test("private JSON reads require root objects and private regular files", () => {
  const directory = mkdtempSync(join(tmpdir(), "schema-publication-read-"));
  const path = privateJson(directory, "receipt.json", { ready: true });
  assert.deepEqual(readPrivateJson(path), { ready: true });

  chmodSync(path, 0o644);
  assert.throws(() => readPrivateJson(path), /mode must be 0600/u);
  chmodSync(path, 0o600);

  const arrayPath = privateJson(directory, "array.json", ["not", "a", "root", "object"]);
  assert.throws(() => readPrivateJson(arrayPath), /must be an object/u);
});

// publication validation echoes the exact accepted bytes
test("validate-publication CLI validates private proof and preserves JSON bytes", () => {
  const directory = mkdtempSync(join(tmpdir(), "schema-publication-cli-"));
  const publication = validPublication();
  const serialized = `${JSON.stringify(publication, null, 3)}\n`;
  const path = join(directory, "publication.json");
  writeFileSync(path, serialized, { mode: 0o600 });
  chmodSync(path, 0o600);

  const valid = runCli(["validate-publication", path, publication.targetRelease]);
  assert.equal(valid.status, 0, valid.stderr);
  assert.equal(valid.stdout, serialized);

  const invalid = runCli(["validate-publication", path, "2026.11.03-5"]);
  assert.notEqual(invalid.status, 0);
  assert.match(invalid.stderr, /requested release/u);
});

// validate-stage echoes exact accepted private bytes
test("validate-stage CLI enforces the expected release", () => {
  const directory = mkdtempSync(join(tmpdir(), "schema-stage-cli-"));
  const receipt = validStageReceipt();
  const serialized = `${JSON.stringify(receipt, null, 2)}\n`;
  const path = join(directory, "stage.json");
  writeFileSync(path, serialized, { mode: 0o600 });
  chmodSync(path, 0o600);

  const valid = runCli(["validate-stage", path, receipt.targetRelease]);
  assert.equal(valid.status, 0, valid.stderr);
  assert.equal(valid.stdout, serialized);
  const invalid = runCli(["validate-stage", path, "2026.11.03-5"]);
  assert.notEqual(invalid.status, 0);
  assert.match(invalid.stderr, /requested release/u);
});

// field reads generic receipts but only emits safe own scalars
test("field CLI emits exact safe own primitive values", () => {
  const directory = mkdtempSync(join(tmpdir(), "schema-publication-field-"));
  const path = privateJson(directory, "journal.json", {
    count: 42,
    nested: { value: "hidden" },
    ready: true,
    targetRelease: "2026.11.03-4",
    unsafe: "two\nlines",
  });
  assert.equal(runCli(["field", path, "targetRelease"]).stdout, "2026.11.03-4\n");
  assert.equal(runCli(["field", path, "count"]).stdout, "42\n");
  assert.equal(runCli(["field", path, "ready"]).stdout, "true\n");
  assert.notEqual(runCli(["field", path, "nested"]).status, 0);
  assert.notEqual(runCli(["field", path, "unsafe"]).status, 0);
  assert.notEqual(runCli(["field", path, "__proto__"]).status, 0);
});

// create-once publication never replaces existing bytes
test("write-json publishes canonical private JSON exactly once", () => {
  const directory = mkdtempSync(join(tmpdir(), "schema-publication-write-"));
  const directPath = join(directory, "direct.json");
  assert.equal(writeJsonCreateOnce(directPath, { first: true }), directPath);
  assert.deepEqual(JSON.parse(readFileSync(directPath, "utf8")), { first: true });
  assert.equal(statSync(directPath).mode & 0o777, 0o600);
  assert.equal(statSync(directPath).nlink, 1);
  assert.throws(() => writeJsonCreateOnce(directPath, { replaced: true }), /EEXIST/u);
  assert.deepEqual(JSON.parse(readFileSync(directPath, "utf8")), { first: true });

  const cliPath = join(directory, "cli.json");
  const created = runCli(["write-json", cliPath], { input: "{\"value\":7}" });
  assert.equal(created.status, 0, created.stderr);
  assert.equal(readFileSync(cliPath, "utf8"), "{\n  \"value\": 7\n}\n");
  const repeated = runCli(["write-json", cliPath], { input: "{\"value\":8}" });
  assert.notEqual(repeated.status, 0);
  assert.equal(readFileSync(cliPath, "utf8"), "{\n  \"value\": 7\n}\n");
});

// bounded commands reject oversized json before publication
test("write-json and private reads reject oversized input", () => {
  const directory = mkdtempSync(join(tmpdir(), "schema-publication-bounds-"));
  const output = join(directory, "oversized-output.json");
  const oversized = JSON.stringify({ value: "x".repeat(1024 * 1024) });
  const writeResult = runCli(["write-json", output], {
    input: oversized,
    maxBuffer: oversized.length + 1024,
  });
  assert.notEqual(writeResult.status, 0);
  assert.match(writeResult.stderr, /size limit/u);

  const input = join(directory, "oversized-input.json");
  writeFileSync(input, oversized, { mode: 0o600 });
  chmodSync(input, 0o600);
  assert.throws(() => readPrivateJson(input), /size is out of bounds/u);
});
