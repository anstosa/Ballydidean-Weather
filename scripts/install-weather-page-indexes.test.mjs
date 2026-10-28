import assert from "node:assert/strict";
import { execFileSync, spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import {
  chmodSync,
  cpSync,
  lstatSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  symlinkSync,
  unlinkSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import test from "node:test";

const script = new URL("./install-weather-page-indexes.sh", import.meta.url).pathname;
const release = "2026.10.28-1";
const commit = "a".repeat(40);
const tree = "b".repeat(40);
const restoreCommit = "c".repeat(40);
const restoreTree = "d".repeat(40);
const migration = "packages/database/migrations/0001_fixture.sql";
const indexManifestTemplate = JSON.parse(
  readFileSync(new URL("./weather-page-indexes.json", import.meta.url), "utf8"),
);

// hash one byte sequence
function sha256(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}

// serialize recursive sorted keys
function stableJson(value) {
  // preserve array order
  if (Array.isArray(value)) {
    return `[${value.map((entry) => stableJson(entry)).join(",")}]`;
  }

  // sort object fields
  if (value !== null && typeof value === "object") {
    return `{${Object.keys(value).sort().map((key) =>
      `${JSON.stringify(key)}:${stableJson(value[key])}`).join(",")}}`;
  }
  return JSON.stringify(value);
}

// write one fixture file
function write(root, path, content, mode = 0o644) {
  const destination = join(root, path);
  mkdirSync(dirname(destination), { recursive: true });
  writeFileSync(destination, content);
  chmodSync(destination, mode);
}

// run one fixture command
function run(command, args, options = {}) {
  return execFileSync(command, args, {
    cwd: options.cwd,
    encoding: options.encoding ?? "utf8",
    env: { ...process.env, LC_ALL: "C" },
    maxBuffer: 32 * 1024 * 1024,
  });
}

// build one deterministic fixture archive
function archive(root, output, paths, format = "ustar") {
  run("tar", [
    `--format=${format}`,
    "--mtime=@0",
    "--owner=0",
    "--group=0",
    "--numeric-owner",
    "-czf",
    output,
    ...paths,
  ], { cwd: root });
}

// create one valid package pair
function createPackage(root, mutateIndexManifest = undefined) {
  const restoreRoot = join(root, "restore");
  const restorePaths = [
    "deploy/compose.yaml",
    "deploy/postgres/runtime-acl-v2.sql",
    "deploy/scripts/common.sh",
    "deploy/scripts/restore.sh",
    migration,
  ];
  write(restoreRoot, "deploy/compose.yaml", "services: {}\n");
  write(restoreRoot, "deploy/postgres/runtime-acl-v2.sql", "select 1;\n");
  write(restoreRoot, "deploy/scripts/common.sh", "common\n", 0o755);
  write(restoreRoot, "deploy/scripts/restore.sh", "restore\n", 0o755);
  write(restoreRoot, migration, "select 1;\n");
  const restoreFiles = restorePaths.map((path) => {
    const content = readFileSync(join(restoreRoot, path));
    const mode = path.endsWith(".sh") ? "100755" : "100644";
    return {
      path,
      mode,
      blob: sha256(Buffer.from(`blob:${path}`)).slice(0, 40),
      digest: sha256(content),
      size: content.length,
    };
  });
  const restoreArchiveName = `restore-context-${restoreCommit.slice(0, 12)}.tar.gz`;
  const restoreArchive = join(root, restoreArchiveName);
  archive(restoreRoot, restoreArchive, restorePaths);
  const restoreArchiveBytes = readFileSync(restoreArchive);
  const byPath = new Map(restoreFiles.map((entry) => [entry.path, entry]));

  // retain finite critical fields
  const critical = (path) => ({ path, digest: byPath.get(path).digest });
  const restoreDescriptor = {
    schema: "weather-page-index-restore-context/v1",
    repository: "fixture/weather",
    tag: "2026.10.27-1",
    commit: restoreCommit,
    tree: restoreTree,
    fileCount: restoreFiles.length,
    decodedBytes: restoreFiles.reduce((total, entry) => total + entry.size, 0),
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
    forbiddenProjectNames: ["weather"],
    forbiddenDatabaseNames: ["weather"],
    forbiddenDataMountSources: ["/var/lib/weather/postgres"],
    filesDigest: sha256(Buffer.from(stableJson(restoreFiles))),
    archive: {
      name: restoreArchiveName,
      digest: sha256(restoreArchiveBytes),
      size: restoreArchiveBytes.length,
    },
    files: restoreFiles,
    critical: {
      restore: critical("deploy/scripts/restore.sh"),
      common: critical("deploy/scripts/common.sh"),
      compose: critical("deploy/compose.yaml"),
      acl: critical("deploy/postgres/runtime-acl-v2.sql"),
      migrations: [critical(migration)],
    },
  };
  const packageRoot = join(root, "package");
  const helper = Buffer.from("export const fixture = true;\n");
  const indexManifest = structuredClone(indexManifestTemplate);
  indexManifest.restoreContext = restoreDescriptor;

  // apply one hostile fixture mutation
  if (mutateIndexManifest !== undefined) {
    mutateIndexManifest(indexManifest);
  }
  const index = Buffer.from(`${JSON.stringify(indexManifest, null, 2)}\n`);
  write(packageRoot, "weather-page-indexes.mjs", helper);
  write(packageRoot, "weather-page-indexes.json", index);
  const releaseManifest = {
    schema: "weather-page-index-release/v1",
    repository: "fixture/weather",
    tag: release,
    commit,
    tree,
    files: [
      {
        path: "weather-page-indexes.mjs",
        digest: sha256(helper),
        size: helper.length,
        mode: "100644",
      },
      {
        path: "weather-page-indexes.json",
        digest: sha256(index),
        size: index.length,
        mode: "100644",
      },
    ],
    restoreContext: restoreDescriptor,
  };
  write(
    packageRoot,
    "release-manifest.json",
    `${JSON.stringify(releaseManifest, null, 2)}\n`,
  );
  const packageArchiveName = `weather-page-indexes-${release}-${commit.slice(0, 12)}.tar.gz`;
  const packageArchive = join(root, packageArchiveName);
  archive(packageRoot, packageArchive, [
    "release-manifest.json",
    "weather-page-indexes.json",
    "weather-page-indexes.mjs",
  ]);
  return {
    packageRoot,
    packageArchive,
    packageHash: sha256(readFileSync(packageArchive)),
    restoreRoot,
    restoreArchive,
    restoreHash: sha256(restoreArchiveBytes),
  };
}

// invoke the read-only verifier
function verifyPackage(fixture, overrides = {}) {
  const archivePath = overrides.packageArchive ?? fixture.packageArchive;
  const restorePath = overrides.restoreArchive ?? fixture.restoreArchive;
  return spawnSync("bash", [
    script,
    "verify",
    "--archive",
    archivePath,
    "--archive-sha256",
    overrides.packageHash ?? sha256(readFileSync(archivePath)),
    "--restore-archive",
    restorePath,
    "--restore-sha256",
    overrides.restoreHash ?? sha256(readFileSync(restorePath)),
    "--release",
    release,
    "--commit",
    commit,
  ], { encoding: "utf8" });
}

// clean one isolated fixture
async function withFixture(callback) {
  const root = mkdtempSync(join(tmpdir(), "weather-page-installer-test-"));

  try {
    await callback(root);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
}

// verify the complete finite package
test("verify accepts exact regular archives without installing anything", async () => {
  await withFixture(async (root) => {
    const fixture = createPackage(root);
    const result = verifyPackage(fixture);
    assert.equal(result.status, 0, result.stderr);
    assert.equal(result.stderr, "");
    assert.match(result.stdout, /package verified/u);
    assert.equal(lstatSync(fixture.packageArchive).isFile(), true);
    assert.equal(lstatSync(fixture.restoreArchive).isFile(), true);
  });
});

// reject removed additive and altered replacement manifests
test("verify accepts only the closed B1 replacement manifest", async () => {
  await withFixture(async (root) => {
    const additiveRoot = join(root, "additive");
    mkdirSync(additiveRoot);
    const additive = createPackage(additiveRoot, (manifest) => {
      // replace with the removed additive shape
      manifest.schema = "weather-page-indexes/v1";
      manifest.indexes = [manifest.replacement.target];
      delete manifest.replacement;
    });
    const additiveResult = verifyPackage(additive);
    assert.notEqual(additiveResult.status, 0);
    assert.match(additiveResult.stderr, /index manifest fields differ|closed B1 replacement/u);
    const alteredRoot = join(root, "altered");
    mkdirSync(alteredRoot);
    const altered = createPackage(alteredRoot, (manifest) => {
      // remove the required partial predicate
      manifest.replacement.target.predicate = null;
    });
    const alteredResult = verifyPackage(altered);
    assert.notEqual(alteredResult.status, 0);
    assert.match(alteredResult.stderr, /B1 replacement manifest differs/u);
  });
});

// reject package extras and transformed bytes
test("verify rejects extra members and payload hash mismatches", async () => {
  await withFixture(async (root) => {
    const fixture = createPackage(root);
    write(fixture.packageRoot, "extra", "extra\n");
    const extraArchive = join(root, "extra.tar.gz");
    archive(fixture.packageRoot, extraArchive, [
      "release-manifest.json",
      "weather-page-indexes.json",
      "weather-page-indexes.mjs",
      "extra",
    ]);
    const extra = verifyPackage(fixture, { packageArchive: extraArchive });
    assert.notEqual(extra.status, 0);
    assert.match(extra.stderr, /aggregate bounds|members differ/u);
    write(fixture.packageRoot, "weather-page-indexes.mjs", "substituted\n");
    const changedArchive = join(root, "changed.tar.gz");
    archive(fixture.packageRoot, changedArchive, [
      "release-manifest.json",
      "weather-page-indexes.json",
      "weather-page-indexes.mjs",
    ]);
    const changed = verifyPackage(fixture, { packageArchive: changedArchive });
    assert.notEqual(changed.status, 0);
    assert.match(changed.stderr, /release file differs/u);
  });
});

// reject links and extension headers
test("verify rejects symlink and pax package members", async () => {
  await withFixture(async (root) => {
    const fixture = createPackage(root);
    unlinkSync(join(fixture.packageRoot, "weather-page-indexes.mjs"));
    symlinkSync("weather-page-indexes.json", join(fixture.packageRoot, "weather-page-indexes.mjs"));
    const linkArchive = join(root, "link.tar.gz");
    archive(fixture.packageRoot, linkArchive, [
      "release-manifest.json",
      "weather-page-indexes.json",
      "weather-page-indexes.mjs",
    ]);
    const linked = verifyPackage(fixture, { packageArchive: linkArchive });
    assert.notEqual(linked.status, 0);
    assert.match(linked.stderr, /plain regular file/u);
    const paxRoot = join(root, "pax-package");
    cpSync(fixture.packageRoot, paxRoot, { recursive: true, dereference: true });
    const paxArchive = join(root, "pax.tar.gz");
    archive(paxRoot, paxArchive, [
      "release-manifest.json",
      "weather-page-indexes.json",
      "weather-page-indexes.mjs",
    ], "pax");
    const pax = verifyPackage(fixture, { packageArchive: paxArchive });
    assert.notEqual(pax.status, 0);
    assert.match(pax.stderr, /plain regular file|plain ustar/u);
  });
});

// reject restore descriptor and archive drift
test("verify binds restore archive bytes to the complete descriptor", async () => {
  await withFixture(async (root) => {
    const fixture = createPackage(root);
    write(fixture.restoreRoot, migration, "changed\n");
    const changedRestore = join(root, `changed-restore-context-${restoreCommit.slice(0, 12)}.tar.gz`);
    archive(fixture.restoreRoot, changedRestore, [
      "deploy/compose.yaml",
      "deploy/postgres/runtime-acl-v2.sql",
      "deploy/scripts/common.sh",
      "deploy/scripts/restore.sh",
      migration,
    ]);
    const result = verifyPackage(fixture, { restoreArchive: changedRestore });
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /archive identity differs|member differs/u);
  });
});

// require root before installation mutation
test("install refuses non-root authority before touching host paths", () => {
  const result = spawnSync("bash", [script, "install"], { encoding: "utf8" });

  // verify the ordinary non-root test environment
  if (process.getuid() !== 0) {
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /requires root/u);
  } else {
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /arguments are incomplete/u);
  }
});

// retain fixed security boundaries
test("installer source freezes private no-overwrite release installation", () => {
  const source = readFileSync(script, "utf8");
  const mode = lstatSync(script).mode & 0o777;
  assert.equal(mode, 0o755);
  assert.match(source, /\/var\/lib\/weather\/maintenance\/page-read-index-v1/u);
  assert.match(source, /--no-clobber --no-target-directory/u);
  assert.match(source, /installed release already exists with different bytes/u);
  assert.match(source, /\/usr\/bin\/sync -f/u);
  assert.doesNotMatch(source, /deploy\/control|release\.env|current\.env/u);
});
