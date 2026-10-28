import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import {
  chmodSync,
  mkdtempSync,
  mkdirSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import test from "node:test";
import {
  buildWeatherPageIndexPackage,
  inspectUstarArchive,
  requireReleaseTag,
  resolvePublishedRepositoryIdentity,
  selectPublishedWorkflowRun,
  stableJson,
  validateB1IndexManifest,
  WEATHER_PAGE_INDEX_MANIFEST_SCHEMA,
  WEATHER_PAGE_INDEX_RELEASE_SCHEMA,
  WEATHER_PAGE_INDEX_REPLACEMENT_SCHEMA,
  WEATHER_PAGE_INDEX_RESTORE_SCHEMA,
} from "./package-weather-page-indexes.mjs";
import { validateReleaseManifest } from "./weather-page-indexes.mjs";

const release = "2026.10.28-1";
const restoreTag = "2026.10.27-1";
const repositoryName = "fixture/weather";
const migration = "packages/database/migrations/0001_fixture.sql";
const indexManifestTemplate = JSON.parse(
  readFileSync(new URL("./weather-page-indexes.json", import.meta.url), "utf8"),
);

// run one fixture command
function run(command, args, options = {}) {
  return execFileSync(command, args, {
    cwd: options.cwd,
    encoding: options.encoding ?? "utf8",
    env: { ...process.env, LC_ALL: "C" },
    maxBuffer: 32 * 1024 * 1024,
  });
}

// write one fixture member
function write(root, path, content, mode = 0o644) {
  const destination = join(root, path);
  mkdirSync(dirname(destination), { recursive: true });
  writeFileSync(destination, content);
  chmodSync(destination, mode);
}

// resolve fixture tree totals
function treeTotals(repository, commit) {
  const paths = run("git", ["ls-tree", "-r", "--name-only", commit], {
    cwd: repository,
  }).trim().split("\n");
  let decodedBytes = 0;

  // count immutable blob bytes
  for (const path of paths) {
    decodedBytes += run("git", ["cat-file", "-s", `${commit}:${path}`], {
      cwd: repository,
    }).trim() * 1;
  }
  return { fileCount: paths.length, decodedBytes };
}

// create two annotated fixture releases
function createRepository(
  root,
  installerSource = "#!/usr/bin/env bash\nexit 0\n",
  mutateIndexManifest = undefined,
) {
  const repository = join(root, "repository");
  mkdirSync(root, { recursive: true });
  mkdirSync(repository);
  run("git", ["init", "--quiet"], { cwd: repository });
  run("git", ["config", "user.name", "Package Test"], { cwd: repository });
  run("git", ["config", "user.email", "package@example.test"], { cwd: repository });
  run("git", ["remote", "add", "origin", `git@github.com:${repositoryName}.git`], {
    cwd: repository,
  });
  write(repository, "deploy/scripts/restore.sh", "restore fixture\n", 0o755);
  write(repository, "deploy/scripts/common.sh", "common fixture\n", 0o755);
  write(repository, "deploy/compose.yaml", "services: {}\n");
  write(repository, "deploy/postgres/runtime-acl-v2.sql", "select 1;\n");
  write(repository, migration, "select 1;\n");
  run("git", ["add", "."], { cwd: repository });
  run("git", ["commit", "--quiet", "-m", "restore fixture"], { cwd: repository });
  run("git", ["tag", "-a", restoreTag, "-m", restoreTag], { cwd: repository });
  const restoreCommit = run("git", ["rev-parse", `${restoreTag}^{commit}`], {
    cwd: repository,
  }).trim();
  const restoreTree = run("git", ["rev-parse", `${restoreTag}^{tree}`], {
    cwd: repository,
  }).trim();
  const totals = treeTotals(repository, restoreCommit);
  const restoreContext = {
    schema: "weather-page-index-restore-context/v1",
    repository: repositoryName,
    tag: restoreTag,
    commit: restoreCommit,
    tree: restoreTree,
    ...totals,
    maximumFileCount: 2048,
    maximumDecodedBytes: 64 * 1024 * 1024,
    criticalPaths: [
      "deploy/scripts/restore.sh",
      "deploy/scripts/common.sh",
      "deploy/compose.yaml",
      "deploy/postgres/runtime-acl-v2.sql",
    ],
    migrations: [migration],
    candidatePrefixMaximumLength: 32,
  };
  write(repository, "scripts/weather-page-indexes.mjs", "export const tagged = true;\n");
  const indexManifest = structuredClone(indexManifestTemplate);
  indexManifest.restoreContext = restoreContext;

  // apply one hostile fixture mutation
  if (mutateIndexManifest !== undefined) {
    mutateIndexManifest(indexManifest);
  }
  write(
    repository,
    "scripts/weather-page-indexes.json",
    `${JSON.stringify(indexManifest, null, 2)}\n`,
  );
  write(repository, "scripts/install-weather-page-indexes.sh", installerSource, 0o755);
  run("git", ["add", "."], { cwd: repository });
  run("git", ["commit", "--quiet", "-m", "release fixture"], { cwd: repository });
  run("git", ["tag", "-a", release, "-m", release], { cwd: repository });
  const commit = run("git", ["rev-parse", `${release}^{commit}`], {
    cwd: repository,
  }).trim();
  const tree = run("git", ["rev-parse", `${release}^{tree}`], {
    cwd: repository,
  }).trim();
  return {
    repository,
    commit,
    tree,
    restoreIdentity: {
      repository: repositoryName,
      tag: restoreTag,
      commit: restoreCommit,
      tree: restoreTree,
      ...totals,
      migrations: [migration],
    },
  };
}

// provide exact publication evidence
function publicationVerifier(calls) {
  // bind one verifier call
  return async (identity) => {
    calls.push(identity);
    return {
      repository: identity.repository,
      canonicalRepository: identity.repository,
      repositoryId: 99,
      tag: identity.tag,
      tagObject: identity.tagObject,
      commit: identity.commit,
      tree: identity.tree,
      checkRunId: 41,
      checkRunAttempt: 2,
      publicationRunId: 42,
      publicationRunAttempt: 1,
    };
  };
}

// clean each isolated fixture
async function withFixture(callback) {
  const root = mkdtempSync(join(tmpdir(), "weather-page-package-test-"));

  try {
    await callback(root);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
}

// package only immutable tagged bytes
test("builder emits the exact bounded package and restore context from Git objects", async () => {
  await withFixture(async (root) => {
    const fixture = createRepository(root);
    const output = join(root, "output");
    const calls = [];
    write(
      fixture.repository,
      "scripts/weather-page-indexes.mjs",
      "throw new Error('dirty worktree must not package');\n",
    );
    const result = await buildWeatherPageIndexPackage({
      repositoryPath: fixture.repository,
      release,
      outputDirectory: output,
      verifyPublication: publicationVerifier(calls),
      restoreIdentity: fixture.restoreIdentity,
    });
    assert.equal(calls.length, 1);
    assert.equal(calls[0].commit, fixture.commit);
    assert.equal(calls[0].tree, fixture.tree);
    const archiveBytes = readFileSync(join(output, result.archive.name));
    assert.equal(result.archive.digest.length, 64);
    assert.ok(result.archive.size <= 1024 * 1024);
    assert.equal(result.publication.historicalRepository, repositoryName);
    assert.equal(result.publication.canonicalRepository, repositoryName);
    assert.equal(result.publication.repositoryId, 99);
    const inspected = inspectUstarArchive(archiveBytes, {
      expectedNames: [
        "release-manifest.json",
        "weather-page-indexes.json",
        "weather-page-indexes.mjs",
      ],
      maximumEntries: 3,
      maximumMemberBytes: 256 * 1024,
      maximumDecodedBytes: 768 * 1024,
    });
    const byName = new Map(inspected.entries.map((entry) => [entry.name, entry]));
    assert.equal(
      byName.get("weather-page-indexes.mjs").content.toString("utf8"),
      "export const tagged = true;\n",
    );
    const releaseManifest = JSON.parse(
      byName.get("release-manifest.json").content.toString("utf8"),
    );
    assert.equal(releaseManifest.schema, WEATHER_PAGE_INDEX_RELEASE_SCHEMA);
    assert.equal(releaseManifest.repository, repositoryName);
    assert.equal(releaseManifest.tag, release);
    assert.equal(releaseManifest.commit, fixture.commit);
    assert.equal(releaseManifest.tree, fixture.tree);
    assert.deepEqual(
      releaseManifest.files.map((entry) => entry.path),
      ["weather-page-indexes.mjs", "weather-page-indexes.json"],
    );
    assert.ok(releaseManifest.files.every((entry) =>
      entry.mode === "100644" || entry.mode === "100755"));
    const indexManifest = JSON.parse(
      byName.get("weather-page-indexes.json").content.toString("utf8"),
    );
    assert.equal(indexManifest.schema, WEATHER_PAGE_INDEX_MANIFEST_SCHEMA);
    assert.equal(indexManifest.replacement.schema, WEATHER_PAGE_INDEX_REPLACEMENT_SCHEMA);
    assert.equal(validateB1IndexManifest(indexManifest), indexManifest);
    const restore = indexManifest.restoreContext;
    assert.deepEqual(releaseManifest.restoreContext, restore);
    assert.equal(validateReleaseManifest(releaseManifest, indexManifest), releaseManifest);
    assert.equal(restore.schema, WEATHER_PAGE_INDEX_RESTORE_SCHEMA);
    assert.equal(restore.commit, fixture.restoreIdentity.commit);
    assert.equal(restore.tree, fixture.restoreIdentity.tree);
    assert.equal(restore.fileCount, fixture.restoreIdentity.fileCount);
    assert.equal(restore.decodedBytes, fixture.restoreIdentity.decodedBytes);
    assert.equal(restore.archive.name, result.restoreContext.name);
    assert.equal(restore.archive.digest, result.restoreContext.digest);
    assert.equal(
      restore.filesDigest,
      createHash("sha256").update(stableJson(restore.files)).digest("hex"),
    );
    assert.ok(byName.get("weather-page-indexes.json").size <= 256 * 1024);
    assert.deepEqual(
      restore.critical.migrations.map((entry) => entry.path),
      [migration],
    );
    const restoreArchive = readFileSync(join(output, result.restoreContext.name));
    const inspectedRestore = inspectUstarArchive(restoreArchive, {
      expectedNames: restore.files.map((entry) => entry.path),
      allowNested: true,
      maximumEntries: 2048,
      maximumDecodedBytes: 64 * 1024 * 1024,
    });
    assert.equal(inspectedRestore.entries.length, restore.fileCount);
    assert.equal(inspectedRestore.decodedBytes, restore.decodedBytes);
    assert.match(result.installer.name, new RegExp(`${result.installer.digest}\\.sh$`, "u"));
    assert.equal(
      readFileSync(join(output, `${result.archive.name}.sha256`), "utf8"),
      `${result.archive.digest}  ${result.archive.name}\n`,
    );
    assert.equal(
      readFileSync(join(output, `${result.installer.name}.sha256`), "utf8"),
      `${result.installer.digest}  ${result.installer.name}\n`,
    );
    const firstArchive = Buffer.from(archiveBytes);
    await buildWeatherPageIndexPackage({
      repositoryPath: fixture.repository,
      release,
      outputDirectory: output,
      verifyPublication: publicationVerifier(calls),
      restoreIdentity: fixture.restoreIdentity,
    });
    assert.deepEqual(readFileSync(join(output, result.archive.name)), firstArchive);
  });
});

// accept only the reviewed historical repository rename
test("repository publication identity binds the exact canonical rename", () => {
  assert.deepEqual(
    resolvePublishedRepositoryIdentity("anstosa/weather", {
      id: 1_342_404_160,
      full_name: "anstosa/Ballydidean-Weather",
      fork: false,
    }),
    {
      repository: "anstosa/weather",
      canonicalRepository: "anstosa/Ballydidean-Weather",
      repositoryId: 1_342_404_160,
    },
  );
  assert.deepEqual(
    resolvePublishedRepositoryIdentity(repositoryName, {
      id: 99,
      full_name: repositoryName,
      fork: false,
    }),
    {
      repository: repositoryName,
      canonicalRepository: repositoryName,
      repositoryId: 99,
    },
  );
  assert.throws(
    () => resolvePublishedRepositoryIdentity("anstosa/weather", {
      id: 1_342_404_159,
      full_name: "anstosa/Ballydidean-Weather",
      fork: false,
    }),
    /identity differs/u,
  );
  assert.throws(
    () => resolvePublishedRepositoryIdentity("anstosa/weather", {
      id: 1_342_404_160,
      full_name: "anstosa/Ballydidean-Weather",
      fork: true,
    }),
    /identity differs/u,
  );
  assert.throws(
    () => resolvePublishedRepositoryIdentity("anstosa/weather", {
      id: 1_342_404_160,
      full_name: "anstosa/weather-fork",
      fork: false,
    }),
    /identity differs/u,
  );
});

// reject legacy additive and altered replacement payloads
test("builder accepts only the closed B1 replacement manifest", async () => {
  await withFixture(async (root) => {
    const additive = createRepository(
      join(root, "additive"),
      undefined,
      (manifest) => {
        // replace with the removed additive shape
        manifest.schema = "weather-page-indexes/v1";
        manifest.indexes = [manifest.replacement.target];
        delete manifest.replacement;
      },
    );
    await assert.rejects(
      buildWeatherPageIndexPackage({
        repositoryPath: additive.repository,
        release,
        outputDirectory: join(root, "additive-output"),
        verifyPublication: publicationVerifier([]),
        restoreIdentity: additive.restoreIdentity,
      }),
      /closed B1 replacement/u,
    );
    const altered = createRepository(
      join(root, "altered"),
      undefined,
      (manifest) => {
        // remove the required partial predicate
        manifest.replacement.target.predicate = null;
      },
    );
    await assert.rejects(
      buildWeatherPageIndexPackage({
        repositoryPath: altered.repository,
        release,
        outputDirectory: join(root, "altered-output"),
        verifyPublication: publicationVerifier([]),
        restoreIdentity: altered.restoreIdentity,
      }),
      /B1 replacement manifest differs/u,
    );
  });
});

// reject verifier and output identity drift
test("builder fails closed on incomplete publication evidence and output replacement", async () => {
  await withFixture(async (root) => {
    const fixture = createRepository(root);
    const output = join(root, "output");
    await assert.rejects(
      buildWeatherPageIndexPackage({
        repositoryPath: fixture.repository,
        release,
        outputDirectory: output,
        verifyPublication: async (identity) => ({
          ...identity,
          checkRunId: 1,
        }),
        restoreIdentity: fixture.restoreIdentity,
      }),
      /complete publication evidence/u,
    );
    const result = await buildWeatherPageIndexPackage({
      repositoryPath: fixture.repository,
      release,
      outputDirectory: output,
      verifyPublication: publicationVerifier([]),
      restoreIdentity: fixture.restoreIdentity,
    });
    writeFileSync(join(output, result.archive.name), "replacement");
    await assert.rejects(
      buildWeatherPageIndexPackage({
        repositoryPath: fixture.repository,
        release,
        outputDirectory: output,
        verifyPublication: publicationVerifier([]),
        restoreIdentity: fixture.restoreIdentity,
      }),
      /different bytes/u,
    );
  });
});

// reject archive links and pax extensions
test("ustar inspector rejects links, traversal, extensions, and member overflow", async () => {
  await withFixture(async (root) => {
    write(root, "safe", "safe\n");
    write(root, "large", "x".repeat(32));
    run("ln", ["-s", "safe", "linked"], { cwd: root });
    run("tar", ["--format=ustar", "-czf", "link.tar.gz", "linked"], { cwd: root });
    assert.throws(
      () => inspectUstarArchive(readFileSync(join(root, "link.tar.gz"))),
      /not regular/u,
    );
    run("tar", ["--format=pax", "-czf", "pax.tar.gz", "safe"], { cwd: root });
    assert.throws(
      () => inspectUstarArchive(readFileSync(join(root, "pax.tar.gz"))),
      /not regular|not ustar/u,
    );
    run("tar", [
      "--format=ustar",
      "--transform=s|safe|../outside|",
      "-czf",
      "traversal.tar.gz",
      "safe",
    ], { cwd: root });
    assert.throws(
      () => inspectUstarArchive(readFileSync(join(root, "traversal.tar.gz")), {
        allowNested: true,
      }),
      /unsafe archive path/u,
    );
    run("tar", ["--format=ustar", "-czf", "large.tar.gz", "large"], { cwd: root });
    assert.throws(
      () => inspectUstarArchive(readFileSync(join(root, "large.tar.gz")), {
        maximumMemberBytes: 16,
      }),
      /size limit/u,
    );
    run("tar", [
      "--format=ustar",
      "--hard-dereference",
      "-czf",
      "duplicate.tar.gz",
      "safe",
      "safe",
    ], { cwd: root });
    assert.throws(
      () => inspectUstarArchive(readFileSync(join(root, "duplicate.tar.gz"))),
      /duplicate tar member/u,
    );
  });
});

// construct one workflow record
function workflowRun({ workflow, branch, id = 10, overrides = {} }) {
  const repository = { id: 99, full_name: repositoryName };
  return {
    id,
    run_attempt: 1,
    event: "push",
    head_sha: "a".repeat(40),
    head_branch: branch,
    head_repository: repository,
    repository,
    path: `.github/workflows/${workflow}.yml@refs/${branch === release ? "tags" : "heads"}/${branch}`,
    status: "completed",
    conclusion: "success",
    created_at: `2026-10-28T00:00:${String(id).padStart(2, "0")}Z`,
    ...overrides,
  };
}

// select only latest exact successful runs
test("workflow evidence binds branch Check and tag publication identities", () => {
  const repository = { id: 99, full_name: repositoryName };
  const base = {
    repository,
    commit: "a".repeat(40),
    tag: release,
  };
  const check = workflowRun({ workflow: "check", branch: "release/page-indexes" });
  assert.equal(selectPublishedWorkflowRun({
    total_count: 1,
    workflow_runs: [check],
  }, { ...base, workflow: "check" }).id, check.id);
  const publication = workflowRun({ workflow: "publish-images", branch: release });
  assert.equal(selectPublishedWorkflowRun({
    total_count: 1,
    workflow_runs: [publication],
  }, { ...base, workflow: "publish-images" }).id, publication.id);
  assert.throws(
    () => selectPublishedWorkflowRun({
      total_count: 2,
      workflow_runs: [
        check,
        workflowRun({
          workflow: "check",
          branch: "release/page-indexes",
          id: 11,
          overrides: { conclusion: "failure" },
        }),
      ],
    }, { ...base, workflow: "check" }),
    /not successful/u,
  );
  assert.throws(
    () => selectPublishedWorkflowRun({
      total_count: 1,
      workflow_runs: [workflowRun({ workflow: "check", branch: release })],
    }, { ...base, workflow: "check" }),
    /malformed exact evidence|tag push/u,
  );
});

// validate immutable release tag syntax
test("release tags use valid repository dates", () => {
  assert.equal(requireReleaseTag("2026.10.28-1"), "2026.10.28-1");
  assert.throws(() => requireReleaseTag("2026.02.30-1"), /invalid date/u);
  assert.throws(() => requireReleaseTag("latest"), /YYYY\.MM\.DD-N/u);
});
