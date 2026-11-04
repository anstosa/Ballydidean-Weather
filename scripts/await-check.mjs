import { appendFileSync } from "node:fs";
import { setTimeout as delay } from "node:timers/promises";
import { pathToFileURL } from "node:url";

const shaPattern = /^[a-f0-9]{40}$/u;
const pollIntervalMs = 15_000;
const deadlineMs = 30 * 60_000;

// retain the latest trusted run and its conservative state
function latestPushCheckEvidence(runs, { sha, repository }) {
  // non-array evidence cannot certify a Check
  if (!Array.isArray(runs)) {
    return { run: null, state: "failure" };
  }
  // scope to the exact same-repository push commit
  const candidates = runs.filter((run) =>
    run?.event === "push" &&
    run?.head_sha === sha &&
    run?.head_repository?.id === repository.id &&
    run?.head_repository?.full_name === repository.full_name);

  // no branch push is not validation evidence
  if (candidates.length === 0) {
    return { run: null, state: "missing" };
  }

  // malformed same-repository evidence must not expose an older success
  // inspect every candidate before selecting the latest
  if (candidates.some((run) =>
    typeof run.head_branch !== "string" ||
    run.head_branch.length === 0 ||
    run.repository?.id !== repository.id ||
    run.repository?.full_name !== repository.full_name ||
    !/^\.github\/workflows\/check\.yml(?:@[^\s]+)?$/u.test(run.path ?? "") ||
    !Number.isSafeInteger(run.id) || run.id <= 0 ||
    !Number.isSafeInteger(run.run_attempt) || run.run_attempt <= 0 ||
    !Number.isFinite(Date.parse(run.created_at ?? "")))) {
    return { run: null, state: "failure" };
  }

  // order by run creation, not late completion of an older run
  candidates.sort((left, right) =>
    Date.parse(right.created_at) - Date.parse(left.created_at) ||
    right.id - left.id);
  const latest = candidates[0];

  // only completed success is sufficient for publication
  if (latest.status === "completed") {
    return {
      run: latest,
      state: latest.conclusion === "success" ? "success" : "failure",
    };
  }

  // wait for the latest attempt; unknown states fail closed
  if (["queued", "in_progress", "requested", "waiting", "pending"].includes(latest.status)) {
    return { run: latest, state: "pending" };
  }
  return { run: latest, state: "failure" };
}

// trust only the latest same-repository branch-push run for the exact commit
export function selectLatestPushCheck(runs, context) {
  return latestPushCheckEvidence(runs, context).state;
}

// require a complete workflow-run catalog before exposing selected evidence
export function selectLatestPushCheckEvidence(document, context) {
  // truncated result sets could conceal a later rerun
  if (
    !Number.isInteger(document?.total_count) ||
    document.total_count > 100 ||
    !Array.isArray(document.workflow_runs) ||
    document.workflow_runs.length !== document.total_count
  ) {
    throw new Error("Check lookup returned incomplete evidence");
  }
  return latestPushCheckEvidence(document.workflow_runs, context);
}

// fetch scoped workflow runs with Actions read permission
export async function queryCheckState({ sha, repository, token, apiUrl }) {
  const url = new URL(
    `/repos/${repository.full_name}/actions/workflows/check.yml/runs`,
    apiUrl,
  );
  url.searchParams.set("head_sha", sha);
  url.searchParams.set("event", "push");
  url.searchParams.set("per_page", "100");
  const response = await fetch(url, {
    headers: {
      Accept: "application/vnd.github+json",
      Authorization: `Bearer ${token}`,
      "X-GitHub-Api-Version": "2026-03-10",
    },
    signal: AbortSignal.timeout(10_000),
  });

  // API failures cannot serve as successful evidence
  if (!response.ok) {
    throw new Error(`Check lookup failed with HTTP ${response.status}`);
  }
  const document = await response.json();

  return selectLatestPushCheckEvidence(document, { sha, repository }).state;
}

// refuse to infer identity from a tag, PR or caller-provided branch label
function context() {
  const sha = process.env.CI_SHA;
  const fullName = process.env.GITHUB_REPOSITORY;
  const id = Number(process.env.CI_REPOSITORY_ID);
  const token = process.env.GITHUB_TOKEN;

  // require authenticated repository identity and an exact commit
  if (
    !shaPattern.test(sha ?? "") ||
    !/^[a-zA-Z0-9_.-]+\/[a-zA-Z0-9_.-]+$/u.test(fullName ?? "") ||
    !Number.isSafeInteger(id) || id <= 0 || !token
  ) {
    throw new Error("Check lookup lacks exact repository or commit identity");
  }
  return {
    sha,
    repository: { id, full_name: fullName },
    token,
    apiUrl: process.env.GITHUB_API_URL ?? "https://api.github.com",
  };
}

// emit one conservative baseline verdict without blocking normal CI
async function checkBaseline() {
  let verified = false;

  try {
    verified = await queryCheckState(context()) === "success";
  } catch (error) {
    process.stderr.write(`baseline Check unavailable: ${error.message}\n`);
  }
  const line = `baseline_verified=${verified}`;
  process.stdout.write(`${line}\n`);

  // output only a boolean to the subsequent classification step
  if (process.env.GITHUB_OUTPUT) {
    appendFileSync(process.env.GITHUB_OUTPUT, `${line}\n`);
  }
}

// wait for a current exact-SHA success without trusting stale attempts
export async function waitForSuccessfulCheck(read, {
  now = Date.now,
  sleep = delay,
  timeoutMs = deadlineMs,
  intervalMs = pollIntervalMs,
} = {}) {
  const started = now();

  // bounded wait for an in-flight exact-SHA branch run
  while (now() - started < timeoutMs) {
    const state = await read();

    // publish only after complete success
    if (state === "success") {
      return;
    }

    // a failed or skipped latest attempt cannot be replaced by stale success
    if (state === "failure") {
      throw new Error("latest exact-SHA push Check failed");
    }
    process.stdout.write(`exact-SHA push Check ${state}; waiting\n`);
    await sleep(intervalMs);
  }
  throw new Error("no successful exact-SHA push Check before deadline");
}

// wait for the tag target's successful branch-push Check, never rerun tests
async function awaitReleaseCheck() {
  const lookup = context();
  await waitForSuccessfulCheck(() => queryCheckState(lookup));
  process.stdout.write(`exact-SHA push Check passed: ${lookup.sha}\n`);
}

// avoid network calls when unit tests import the verdict function
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const mode = process.env.CI_CHECK_MODE;

  // mode is explicit so baseline failures cannot masquerade as publish success
  if (mode === "baseline") {
    await checkBaseline();
  } else if (mode === "release") {
    await awaitReleaseCheck();
  } else {
    throw new Error("CI_CHECK_MODE must be baseline or release");
  }
}
