import assert from "node:assert/strict";
import test from "node:test";
import {
  queryCheckState,
  selectLatestPushCheck,
  selectLatestPushCheckEvidence,
  waitForSuccessfulCheck,
} from "./await-check.mjs";

const sha = "a".repeat(40);
const repo = { id: 42, full_name: "owner/weather" };

// construct a same-repository branch push
function run(overrides = {}) {
  return {
    id: 12,
    run_attempt: 1,
    event: "push",
    head_sha: sha,
    head_branch: "release/faster-ci",
    head_repository: repo,
    repository: repo,
    path: ".github/workflows/check.yml@refs/heads/release/faster-ci",
    status: "completed",
    conclusion: "success",
    created_at: "2026-09-14T00:00:00Z",
    updated_at: "2026-09-14T00:00:00Z",
    ...overrides,
  };
}

// require exact commit and same-repository push identity
test("only an exact SHA same-repository branch push is evidence", () => {
  assert.equal(selectLatestPushCheck([run()], { sha, repository: repo }), "success");
  assert.equal(selectLatestPushCheck([run({ path: ".github/workflows/check.yml" })], { sha, repository: repo }), "success");
  assert.equal(selectLatestPushCheck([run({ event: "pull_request" })], { sha, repository: repo }), "missing");
  assert.equal(selectLatestPushCheck([run({ head_sha: "b".repeat(40) })], { sha, repository: repo }), "missing");
  assert.equal(selectLatestPushCheck([run({ head_repository: { id: 9, full_name: "fork/weather" } })], { sha, repository: repo }), "missing");
  assert.equal(selectLatestPushCheck([run({ head_branch: null })], { sha, repository: repo }), "failure");
  assert.equal(selectLatestPushCheck([run({ path: ".github/workflows/other.yml@main" })], { sha, repository: repo }), "failure");
  assert.equal(selectLatestPushCheck([run({ repository: { id: 9, full_name: "fork/weather" } })], { sha, repository: repo }), "failure");
  assert.equal(selectLatestPushCheck([run({ created_at: "invalid" })], { sha, repository: repo }), "failure");
});

// late old success cannot hide a newer pending or failed run
test("latest rerun or attempt overrides an earlier success", () => {
  const earlier = run({ id: 12, updated_at: "2026-09-14T00:10:00Z" });
  const newer = run({ id: 13, status: "in_progress", conclusion: null, created_at: "2026-09-14T00:05:00Z" });
  assert.equal(selectLatestPushCheck([earlier, newer], { sha, repository: repo }), "pending");
  assert.equal(selectLatestPushCheck([earlier, run({ id: 13, conclusion: "failure", created_at: "2026-09-14T00:05:00Z" })], { sha, repository: repo }), "failure");
  assert.equal(selectLatestPushCheck([run({ conclusion: "skipped" })], { sha, repository: repo }), "failure");
  assert.equal(selectLatestPushCheck([run({ run_attempt: 2, conclusion: "failure" })], { sha, repository: repo }), "failure");
});

// choose the larger run id when creation timestamps tie
test("latest successful Check selection is deterministic", () => {
  const document = {
    total_count: 2,
    workflow_runs: [run({ id: 12 }), run({ id: 13 })],
  };
  const evidence = selectLatestPushCheckEvidence(document, { sha, repository: repo });
  assert.equal(evidence.state, "success");
  assert.equal(evidence.run.id, 13);
});

// reject unavailable and malformed API evidence
test("API failure and incomplete result never certify a Check", async () => {
  const originalFetch = globalThis.fetch;
  const lookup = { sha, repository: repo, token: "test-only", apiUrl: "https://api.github.test" };

  try {
    // reject forbidden lookup
    globalThis.fetch = async () => new Response("denied", { status: 403 });
    await assert.rejects(queryCheckState(lookup), /HTTP 403/u);
    // reject non-JSON lookup
    globalThis.fetch = async () => new Response("not json", { status: 200 });
    await assert.rejects(queryCheckState(lookup), SyntaxError);
    // reject truncated lookup
    globalThis.fetch = async () => Response.json({ total_count: 2, workflow_runs: [run()] });
    await assert.rejects(queryCheckState(lookup), /incomplete evidence/u);
    // accept one complete matching lookup
    globalThis.fetch = async () => Response.json({ total_count: 1, workflow_runs: [run()] });
    assert.equal(await queryCheckState(lookup), "success");
    // reject malformed run metadata
    globalThis.fetch = async () => Response.json({ total_count: 1, workflow_runs: [run({ path: "bad" })] });
    assert.equal(await queryCheckState(lookup), "failure");
  } finally {
    globalThis.fetch = originalFetch;
  }
});

// release wait is bounded and fail closed
test("release wait rejects failure and deadline but accepts later success", async () => {
  await assert.rejects(
    waitForSuccessfulCheck(async () => "failure", { timeoutMs: 1_000 }),
    /failed/u,
  );
  let time = 0;
  await assert.rejects(
    waitForSuccessfulCheck(async () => "missing", {
      now: () => time,
      sleep: async () => { time += 10; },
      timeoutMs: 20,
      intervalMs: 10,
    }),
    /before deadline/u,
  );
  const states = ["pending", "success"];
  await waitForSuccessfulCheck(async () => states.shift(), {
    now: () => 0,
    sleep: async () => {},
    timeoutMs: 20,
  });
});
