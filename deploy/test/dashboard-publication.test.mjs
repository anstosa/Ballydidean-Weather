import assert from "node:assert/strict";
import test from "node:test";
import { verifyDashboardWebGitRelease } from "../scripts/adjustment-evaluation-package.mjs";

const core = "2026.10.09-1";
const source = "56b327d9c750946f6f6963b6fe1fa5c9bba791ca";
const targetRelease = "2026.10.09-4";
const target = "a".repeat(40);

// expose only complete same-repository immutable publication evidence
function publicationFixture(release = targetRelease, commit = target) {
  const repository = {
    fork: false,
    full_name: "anstosa/Ballydidean-Weather",
    id: 1_342_404_160,
  };
  // retain workflow identity, tag and independent completion clocks
  const run = (filename, id, createdAt, updatedAt) => ({
    conclusion: "success", created_at: createdAt, event: "push", head_branch: release,
    head_repository: { ...repository }, head_sha: commit, id,
    path: `.github/workflows/${filename}`, repository: { ...repository }, run_attempt: 1,
    status: "completed", updated_at: updatedAt,
  });
  const responses = new Map([
    [`git/ref/tags/${core}`, { object: { sha: source, type: "commit" } }],
    [`git/ref/tags/${release}`, { object: { sha: commit, type: "commit" } }],
    [`compare/${source}...${commit}?per_page=1`, {
      ahead_by: 8, base_commit: { sha: source }, behind_by: 0, status: "ahead",
    }],
    [`actions/workflows/check.yml/runs?head_sha=${commit}&event=push&per_page=100`, {
      total_count: 1, workflow_runs: [run("check.yml", 10, "2026-10-10T05:00:00Z", "2026-10-10T05:20:00Z")],
    }],
    [`actions/workflows/publish-images.yml/runs?head_sha=${commit}&event=push&per_page=100`, {
      total_count: 1, workflow_runs: [run("publish-images.yml", 11, "2026-10-10T05:21:00Z", "2026-10-10T05:30:00Z")],
    }],
  ]);
  // reject any unexpected public metadata lookup in the fixture
  const readJson = async (path) => {
    assert.ok(responses.has(path), path);
    return structuredClone(responses.get(path));
  };
  return { readJson, responses };
}

// multi-commit UI work must remain disjoint from the pinned maintenance handoff
test("dashboard publication binds exact core, descendant tag and Check-before-Publish", async () => {
  const { readJson } = publicationFixture();
  const result = await verifyDashboardWebGitRelease(targetRelease, core, readJson);
  assert.deepEqual(result, {
    checkRunId: 10, contractVersion: "dashboard-web-release-proof/v1", coreRelease: core,
    publishRunId: 11, sourceCommit: source, state: "publication_ready", targetCommit: target,
    targetRelease,
  });
  await assert.rejects(verifyDashboardWebGitRelease("2026.10.09-2", core, readJson), /unsupported/u);
  await assert.rejects(verifyDashboardWebGitRelease("2026.10.09-3", core, readJson), /unsupported/u);
  await assert.rejects(verifyDashboardWebGitRelease(targetRelease, "2026.10.07-3", readJson), /unsupported/u);
});

// compensation reuses the published core without fabricating a new ancestry
test("dashboard rollback proves the unchanged original core publication", async () => {
  const { readJson } = publicationFixture(core, source);
  const result = await verifyDashboardWebGitRelease(core, core, readJson);
  assert.equal(result.targetCommit, source);
  assert.equal(result.targetRelease, core);
});

// a green unrelated branch cannot substitute the retained deployed source
test("dashboard publication rejects moved core tags and divergent ancestry", async () => {
  const { readJson, responses } = publicationFixture();
  responses.get(`git/ref/tags/${core}`).object.sha = "b".repeat(40);
  await assert.rejects(verifyDashboardWebGitRelease(targetRelease, core, readJson), /Git identity/u);
  responses.get(`git/ref/tags/${core}`).object.sha = source;
  const compare = responses.get(`compare/${source}...${target}?per_page=1`);
  compare.behind_by = 1;
  await assert.rejects(verifyDashboardWebGitRelease(targetRelease, core, readJson), /ancestry/u);
  compare.behind_by = 0;
  compare.base_commit.sha = "b".repeat(40);
  await assert.rejects(verifyDashboardWebGitRelease(targetRelease, core, readJson), /ancestry/u);
});

// stale successful runs and another tag cannot authorize the selected image
test("dashboard publication rejects failed latest runs, incomplete pages and mismatched tags", async () => {
  const { readJson, responses } = publicationFixture();
  const check = responses.get(`actions/workflows/check.yml/runs?head_sha=${target}&event=push&per_page=100`);
  check.workflow_runs.push({ ...check.workflow_runs[0], conclusion: "failure", created_at: "2026-10-10T05:22:00Z", id: 12 });
  check.total_count = 2;
  await assert.rejects(verifyDashboardWebGitRelease(targetRelease, core, readJson), /not successful/u);
  check.workflow_runs.pop();
  await assert.rejects(verifyDashboardWebGitRelease(targetRelease, core, readJson), /incomplete/u);
  check.total_count = 1;
  const publish = responses.get(`actions/workflows/publish-images.yml/runs?head_sha=${target}&event=push&per_page=100`);
  publish.workflow_runs[0].head_branch = "2026.10.09-5";
  await assert.rejects(verifyDashboardWebGitRelease(targetRelease, core, readJson), /another tag/u);
  publish.workflow_runs[0].head_branch = targetRelease;
  check.workflow_runs[0].updated_at = "2026-10-10T05:22:00Z";
  await assert.rejects(verifyDashboardWebGitRelease(targetRelease, core, readJson), /preceded/u);
});

// reject the former repository name and every fork or head-repository substitution
test("dashboard publication requires canonical repository and head identities", async () => {
  const { readJson, responses } = publicationFixture();
  const check = responses.get(
    `actions/workflows/check.yml/runs?head_sha=${target}&event=push&per_page=100`,
  );
  check.workflow_runs[0].repository.full_name = "anstosa/weather";
  await assert.rejects(verifyDashboardWebGitRelease(targetRelease, core, readJson),
    /identity differs/u);
  check.workflow_runs[0].repository.full_name = "anstosa/Ballydidean-Weather";
  check.workflow_runs[0].head_repository.fork = true;
  await assert.rejects(verifyDashboardWebGitRelease(targetRelease, core, readJson),
    /identity differs/u);
  check.workflow_runs[0].head_repository.fork = false;
  check.workflow_runs[0].head_repository.id += 1;
  await assert.rejects(verifyDashboardWebGitRelease(targetRelease, core, readJson),
    /identity differs/u);
});
