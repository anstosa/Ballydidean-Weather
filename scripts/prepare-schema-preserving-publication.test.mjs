import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import test from "node:test";
import { preparePublication } from "./prepare-schema-preserving-publication.mjs";
import { readPrivateJson, validatePublication } from "./schema-preserving-release.mjs";

const RELEASE = "2026.11.03-4";
const TARGET_COMMIT = "c".repeat(40);
const SOURCE_COMMIT = "7e045d5bc69969e9247440ce3b70744e1f9ef6c4";
const HASH_A = "a".repeat(64);
const HASH_B = "b".repeat(64);
const HASH_C = "c".repeat(64);
const HASH_D = "d".repeat(64);
const REPOSITORY = { id: 42, full_name: "anstosa/Ballydidean-Weather" };
const TREE_PATHS = [
  "Dockerfile",
  "deploy/scripts/common.sh",
  "deploy/scripts/update.sh",
  "package-lock.json",
  "packages/database/migrations/0020_adjustment_revision_frontier.sql",
  "packages/database/migrations/0023_forecast_product_read_index.sql",
  "packages/forecast-adjustment/src/maintenance-capture-epoch.ts",
];

// encode one fake command response
function bytes(value) {
  return Buffer.from(typeof value === "string" ? value : JSON.stringify(value));
}

// build isolated command evidence
function fixture(options = {}) {
  const root = mkdtempSync(join(tmpdir(), "prepare-schema-publication-test-"));
  const scripts = join(root, "scripts");
  mkdirSync(scripts);
  const operator = bytes("operator bundle\n");
  const validator = bytes("validator bundle\n");
  writeFileSync(join(scripts, "schema-preserving-release.sh"), operator);
  writeFileSync(join(scripts, "schema-preserving-release.mjs"), validator);
  const commands = [];
  const temporaryRoots = [];
  const check = {
    conclusion: options.checkConclusion ?? "success",
    created_at: "2026-11-03T00:00:00Z",
    event: "push",
    head_branch: "main",
    head_repository: REPOSITORY,
    head_sha: TARGET_COMMIT,
    html_url: "https://github.com/anstosa/Ballydidean-Weather/actions/runs/101",
    id: 101,
    name: "Check",
    path: ".github/workflows/check.yml@refs/heads/main",
    repository: REPOSITORY,
    run_attempt: 1,
    status: "completed",
  };
  const checkEvidence = { total_count: 1, workflow_runs: [check] };
  const publish = {
    conclusion: "success",
    databaseId: 102,
    event: "push",
    headBranch: RELEASE,
    headSha: TARGET_COMMIT,
    name: "Publish production images",
    status: "completed",
    url: "https://github.com/anstosa/Ballydidean-Weather/actions/runs/102",
  };

  // emulate only expected local tools
  function run(command, args) {
    commands.push([command, ...args]);
    // resolve local commits
    if (command === "git" && args[0] === "rev-parse") {
      return bytes(`${TARGET_COMMIT}\n`);
    }
    // list protected paths
    if (command === "git" && args[0] === "ls-tree") {
      return bytes(`${TREE_PATHS.join("\0")}\0`);
    }
    // return committed bytes
    if (command === "git" && args[0] === "show") {
      const [commit, path] = args[1].split(":");
      // bind bundle fixtures
      if (path === "scripts/schema-preserving-release.sh") {
        return options.bundleDrift ? bytes("changed operator\n") : operator;
      }
      // bind validator fixtures
      if (path === "scripts/schema-preserving-release.mjs") {
        return validator;
      }
      // optionally drift one target blob
      if (options.treeDrift && commit === TARGET_COMMIT && path === "Dockerfile") {
        return bytes("changed target Dockerfile\n");
      }
      return bytes(`protected:${path}\n`);
    }
    // resolve github api evidence
    if (command === "gh" && args[0] === "api") {
      const endpoint = args[3];
      // return authenticated repository identity
      if (endpoint === `repos/${REPOSITORY.full_name}`) {
        return bytes(REPOSITORY);
      }
      // return complete exact-sha Check evidence
      if (endpoint.includes("/actions/workflows/check.yml/runs")) {
        return bytes(checkEvidence);
      }
      // return the bounded artifact catalog
      if (endpoint.includes("/actions/runs/")) {
        return bytes({
          artifacts: [
            {
              expired: false,
              name: "server-image-digest",
              size_in_bytes: options.artifactSize ?? 128,
            },
            { expired: false, name: "web-image-digest", size_in_bytes: 128 },
          ],
        });
      }
      // return the exact remote tag
      return bytes({
        object: {
          sha: options.remoteCommit ?? TARGET_COMMIT,
          type: "commit",
        },
      });
    }
    // list workflow evidence
    if (command === "gh" && args[0] === "run" && args[1] === "list") {
      const workflow = args[args.indexOf("--workflow") + 1];
      return bytes(workflow === "publish-images.yml" ? [publish] : []);
    }
    // inspect the boundary step
    if (command === "gh" && args[0] === "run" && args[1] === "view") {
      return bytes({
        jobs: [{
          steps: [{
            conclusion: options.stepConclusion ?? "success",
            name: "Verify exact production image boundary",
            status: "completed",
          }],
        }],
      });
    }
    // download one digest artifact
    if (command === "gh" && args[0] === "run" && args[1] === "download") {
      const name = args[args.indexOf("--name") + 1];
      const directory = args[args.indexOf("--dir") + 1];
      temporaryRoots.push(dirname(directory));
      const target = name.startsWith("server") ? "server" : "web";
      const digest = target === "server" ? HASH_C : HASH_D;
      const reference = options.badArtifact === target
        ? `registry.example/weather-${target}@sha256:${digest}`
        : `ghcr.io/anstosa/weather-${target}@sha256:${digest}`;
      writeFileSync(
        join(directory, `${target}-image-digest.env`),
        `WEATHER_${target.toUpperCase()}_IMAGE=${reference}\n`,
      );
      return Buffer.alloc(0);
    }
    // inspect remote image manifests
    if (command === "docker" && args[0] === "buildx") {
      const target = args[3].includes("weather-server") ? "server" : "web";
      return bytes({ config: { digest: `sha256:${target === "server" ? HASH_A : HASH_B}` } });
    }
    throw new Error(`unexpected command: ${command} ${args.join(" ")}`);
  }

  return {
    checkEvidence,
    commands,
    output: join(root, "publication.json"),
    root,
    run,
    temporaryRoots,
  };
}

// build a complete publication from derived evidence
test("prepares one validated private publication without image pulls", () => {
  const setup = fixture();
  const publication = preparePublication(RELEASE, setup.output, {
    cwd: setup.root,
    run: setup.run,
  });
  assert.equal(validatePublication(publication, RELEASE), publication);
  assert.deepEqual(readPrivateJson(setup.output), publication);
  assert.equal(statSync(setup.output).mode & 0o777, 0o600);
  assert.equal(statSync(setup.output).nlink, 1);
  assert.equal(
    publication.operatorSha256,
    createHash("sha256").update("operator bundle\n").digest("hex"),
  );
  assert.equal(publication.sourceTree.length, TREE_PATHS.length);
  assert.equal(publication.targetTree.length, TREE_PATHS.length);
  assert.equal(
    setup.commands.some((command) => command[0] === "git"
      && command[1] === "ls-tree" && command.includes(SOURCE_COMMIT)),
    true,
  );
  assert.equal(setup.commands.some((command) => command[0] === "docker" && command[1] !== "buildx"), false);
  assert.deepEqual(
    setup.commands.filter((command) => command[0] === "gh" && command[2] === "download")
      .map((command) => command[command.indexOf("--name") + 1]),
    ["server-image-digest", "web-image-digest"],
  );
  // verify owned temporary cleanup
  for (const temporaryRoot of new Set(setup.temporaryRoots)) {
    assert.equal(existsSync(temporaryRoot), false);
  }
});

// reject disagreement between local and github tags
test("rejects a remote tag that differs from local HEAD", () => {
  const setup = fixture({ remoteCommit: "d".repeat(40) });
  assert.throws(
    () => preparePublication(RELEASE, setup.output, { cwd: setup.root, run: setup.run }),
    /local tag, remote tag and HEAD/u,
  );
  assert.equal(existsSync(setup.output), false);
});

// reject unsuccessful workflow evidence
test("rejects failed Check and image-boundary workflow evidence", () => {
  const failedCheck = fixture({ checkConclusion: "failure" });
  assert.throws(
    () => preparePublication(RELEASE, failedCheck.output, {
      cwd: failedCheck.root,
      run: failedCheck.run,
    }),
    /Check evidence is missing/u,
  );
  const failedStep = fixture({ stepConclusion: "failure" });
  assert.throws(
    () => preparePublication(RELEASE, failedStep.output, {
      cwd: failedStep.root,
      run: failedStep.run,
    }),
    /image boundary was not proven/u,
  );
});

// select the latest successful exact-sha branch Check
test("selects the latest of multiple successful Check runs", () => {
  const setup = fixture();
  const earlier = setup.checkEvidence.workflow_runs[0];
  setup.checkEvidence.workflow_runs.push({
    ...earlier,
    created_at: "2026-11-03T00:05:00Z",
    head_branch: "release/prepare-publication",
    html_url: "https://github.com/anstosa/Ballydidean-Weather/actions/runs/103",
    id: 103,
    path: ".github/workflows/check.yml@refs/heads/release/prepare-publication",
  });
  setup.checkEvidence.total_count = 2;
  const publication = preparePublication(RELEASE, setup.output, {
    cwd: setup.root,
    run: setup.run,
  });
  assert.equal(publication.check.databaseId, 103);
});

// reject stale success behind a newer failed or pending Check
test("rejects an older Check success when the latest run is not successful", () => {
  const failed = fixture();
  const failedEarlier = failed.checkEvidence.workflow_runs[0];
  failed.checkEvidence.workflow_runs.push({
    ...failedEarlier,
    conclusion: "failure",
    created_at: "2026-11-03T00:05:00Z",
    id: 103,
  });
  failed.checkEvidence.total_count = 2;
  assert.throws(
    () => preparePublication(RELEASE, failed.output, { cwd: failed.root, run: failed.run }),
    /Check evidence is missing/u,
  );

  const pending = fixture();
  const pendingEarlier = pending.checkEvidence.workflow_runs[0];
  pending.checkEvidence.workflow_runs.push({
    ...pendingEarlier,
    conclusion: null,
    created_at: "2026-11-03T00:05:00Z",
    id: 103,
    status: "in_progress",
  });
  pending.checkEvidence.total_count = 2;
  assert.throws(
    () => preparePublication(RELEASE, pending.output, { cwd: pending.root, run: pending.run }),
    /Check evidence is missing/u,
  );
});

// fail closed for malformed candidates and truncated catalogs
test("rejects malformed or incomplete Check evidence", () => {
  const malformed = fixture();
  malformed.checkEvidence.workflow_runs[0].path = ".github/workflows/other.yml@refs/heads/main";
  assert.throws(
    () => preparePublication(RELEASE, malformed.output, {
      cwd: malformed.root,
      run: malformed.run,
    }),
    /Check evidence is missing/u,
  );

  const truncated = fixture();
  truncated.checkEvidence.total_count = 2;
  assert.throws(
    () => preparePublication(RELEASE, truncated.output, {
      cwd: truncated.root,
      run: truncated.run,
    }),
    /incomplete evidence/u,
  );
});

// break creation-time ties by the larger workflow run id
test("selects Check runs deterministically when timestamps tie", () => {
  const setup = fixture();
  const earlier = setup.checkEvidence.workflow_runs[0];
  setup.checkEvidence.workflow_runs.push({
    ...earlier,
    html_url: "https://github.com/anstosa/Ballydidean-Weather/actions/runs/103",
    id: 103,
  });
  setup.checkEvidence.total_count = 2;
  const publication = preparePublication(RELEASE, setup.output, {
    cwd: setup.root,
    run: setup.run,
  });
  assert.equal(publication.check.databaseId, 103);
});

// reject protected source drift and local bundle drift
test("rejects tree drift and uncommitted critical bundle bytes", () => {
  const treeDrift = fixture({ treeDrift: true });
  assert.throws(
    () => preparePublication(RELEASE, treeDrift.output, {
      cwd: treeDrift.root,
      run: treeDrift.run,
    }),
    /protected tree drifted/u,
  );
  const bundleDrift = fixture({ bundleDrift: true });
  assert.throws(
    () => preparePublication(RELEASE, bundleDrift.output, {
      cwd: bundleDrift.root,
      run: bundleDrift.run,
    }),
    /differs from the target commit/u,
  );
});

// reject artifacts outside the exact ghcr digest boundary
test("rejects invalid downloaded image proof", () => {
  const oversized = fixture({ artifactSize: 8192 });
  assert.throws(
    () => preparePublication(RELEASE, oversized.output, {
      cwd: oversized.root,
      run: oversized.run,
    }),
    /artifact is unavailable or oversized/u,
  );
  assert.equal(
    oversized.commands.some((command) => command[0] === "gh" && command[2] === "download"),
    false,
  );
  const setup = fixture({ badArtifact: "server" });
  assert.throws(
    () => preparePublication(RELEASE, setup.output, { cwd: setup.root, run: setup.run }),
    /server artifact reference is invalid/u,
  );
  assert.equal(existsSync(setup.output), false);
  // verify owned temporary cleanup on failure
  for (const temporaryRoot of new Set(setup.temporaryRoots)) {
    assert.equal(existsSync(temporaryRoot), false);
  }
});
