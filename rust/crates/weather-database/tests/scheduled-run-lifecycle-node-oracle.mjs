import assert from "node:assert/strict";

import {
  SourceSession,
  failIngestionRun,
  getScheduledCheckpoint,
  startIngestionRun,
} from "../../../../packages/database/dist/index.js";

const requestedStart = "2027-01-10T10:00:00.000Z";
const requestedEndExclusive = "2027-01-10T11:00:00.000Z";
const deadlineAt = "2027-01-10T11:15:00.000Z";
const lastValidAt = "2027-01-10T10:55:00.000Z";
const fingerprint = "b".repeat(64);

// create one no-host scheduled-start queryable
function startClient(existingRunId = null) {
  const trace = [];
  return {
    trace,
    // emulate the two source-owned scheduled-start statements
    async query(text, values = []) {
      // expose the running-row lookup
      if (text.includes("SELECT id, deadline_at")) {
        assert.deepEqual(values, ["source-1"]);
        trace.push("lookup:source-1");
        return existingRunId === null
          ? { rowCount: 0, rows: [] }
          : {
              rowCount: 1,
              rows: [{ deadline_at: new Date(deadlineAt), id: existingRunId }],
            };
      }

      // expose the scheduled running-row insert binds
      if (text.includes("INSERT INTO ingestion_runs")) {
        assert.deepEqual(values, [
          "source-1",
          "scheduled",
          requestedStart,
          requestedEndExclusive,
          fingerprint,
          "open-meteo/v1",
          null,
          deadlineAt,
          2,
          '{"requestId":"request-7"}',
        ]);
        trace.push(`insert:${values[1]}:${values[8]}`);
        return {
          rowCount: 1,
          rows: [{ id: "run-1", startedAt: requestedStart, state: "running" }],
        };
      }

      throw new Error(`unexpected scheduled-start SQL: ${text}`);
    },
  };
}

// execute one compiled scheduled-start fixture
async function startFixture(existingRunId = null) {
  const client = startClient(existingRunId);
  const session = new SourceSession(client, "source-1");

  // return either the running row or ordinary conflict text
  try {
    const row = await startIngestionRun(session, {
      adapterVersion: "open-meteo/v1",
      attempts: 2,
      deadlineAt,
      mode: "scheduled",
      requestMetadata: { requestId: "request-7" },
      requestedEndExclusive,
      requestedStart,
      sourceConfigFingerprint: fingerprint,
    });
    return { error: null, row, trace: client.trace };
  } catch (error) {
    return { error: error.message, row: null, trace: client.trace };
  }
}

// create one no-host checkpoint queryable
function checkpointClient(includeRow) {
  const trace = [];
  return {
    trace,
    // return one decoded PostgreSQL checkpoint row
    async query(text, values = []) {
      assert.ok(text.includes("FROM ingestion_checkpoints"));
      assert.deepEqual(values, ["source-1"]);
      trace.push("read:source-1");
      return includeRow
        ? {
            rowCount: 1,
            rows: [{
              last_committed_at: new Date("2027-01-10T10:56:00.000Z"),
              last_valid_at: new Date(lastValidAt),
              provider_cursor: { cursor: "cursor-8" },
              source_id: "source-1",
              version: "7",
              window_end_exclusive: new Date(requestedEndExclusive),
              window_start: new Date(requestedStart),
            }],
          }
        : { rowCount: 0, rows: [] };
    },
  };
}

// execute one compiled checkpoint read fixture
async function checkpointFixture(includeRow) {
  const client = checkpointClient(includeRow);
  const session = new SourceSession(client, "source-1");
  const row = await getScheduledCheckpoint(session);
  return { row, trace: client.trace };
}

// create one no-host failed-run transaction queryable
function failureClient(assertionRowCount) {
  const trace = [];
  return {
    trace,
    // emulate the scheduled failure transaction statements
    async query(text, values = []) {
      // preserve transaction start order
      if (text === "BEGIN") {
        trace.push("begin");
        return { rowCount: null, rows: [] };
      }

      // preserve transaction completion order
      if (text === "COMMIT" || text === "ROLLBACK") {
        trace.push(text.toLowerCase());
        return { rowCount: null, rows: [] };
      }

      // expose the scheduled running-mode guard
      if (text.includes("SELECT 1") && text.includes("mode = $3")) {
        assert.deepEqual(values, ["run-1", "source-1", "scheduled"]);
        trace.push(`assert:${values.join(":")}`);
        return { rowCount: assertionRowCount, rows: assertionRowCount === 1 ? [{}] : [] };
      }

      // expose the failed-run durable evidence
      if (text.includes("state = 'failed'")) {
        assert.deepEqual(values, [
          "run-1",
          "source-1",
          3,
          '{"requestId":"response-8"}',
          "retryable",
          "network_timeout",
          "provider timed out",
        ]);
        trace.push(
          `finalize:${values[2]}:${values[3]}:${values[4]}:${values[5]}:${values[6]}`,
        );
        return { rowCount: 1, rows: [] };
      }

      throw new Error(`unexpected scheduled-failure SQL: ${text}`);
    },
  };
}

// execute one compiled scheduled failure fixture
async function failureFixture(assertionRowCount) {
  const client = failureClient(assertionRowCount);
  const session = new SourceSession(client, "source-1");

  // return ordinary guarded-write error text and exact command order
  try {
    await failIngestionRun(session, {
      attempts: 3,
      error: {
        classification: "retryable",
        code: "network_timeout",
        message: "provider timed out",
      },
      responseMetadata: { requestId: "response-8" },
      runId: "run-1",
    });
    return { error: null, trace: client.trace };
  } catch (error) {
    return { error: error.message, trace: client.trace };
  }
}

const started = await startFixture();
assert.equal(started.error, null);
assert.deepEqual(started.row, {
  id: "run-1",
  startedAt: requestedStart,
  state: "running",
});
assert.deepEqual(started.trace, ["lookup:source-1", "insert:scheduled:2"]);

const existing = await startFixture("run-existing");
assert.equal(existing.error, "source already has a running ingestion: run-existing");
assert.deepEqual(existing.trace, ["lookup:source-1"]);

const checkpoint = await checkpointFixture(true);
assert.deepEqual(checkpoint.trace, ["read:source-1"]);
assert.deepEqual(checkpoint.row, {
  lastCommittedAt: "2027-01-10T10:56:00.000Z",
  lastValidAt,
  providerCursor: { cursor: "cursor-8" },
  sourceId: "source-1",
  version: 7,
  windowEndExclusive: requestedEndExclusive,
  windowStart: requestedStart,
});

const missingCheckpoint = await checkpointFixture(false);
assert.deepEqual(missingCheckpoint.trace, ["read:source-1"]);
assert.equal(missingCheckpoint.row, null);

const failed = await failureFixture(1);
assert.equal(failed.error, null);
assert.deepEqual(failed.trace, [
  "begin",
  "assert:run-1:source-1:scheduled",
  'finalize:3:{"requestId":"response-8"}:retryable:network_timeout:provider timed out',
  "commit",
]);

const failureMismatch = await failureFixture(0);
assert.equal(
  failureMismatch.error,
  "running ingestion mode check did not affect exactly one row",
);
assert.deepEqual(failureMismatch.trace, [
  "begin",
  "assert:run-1:source-1:scheduled",
  "rollback",
]);

process.stdout.write(`start=${started.trace.join("|")}\n`);
process.stdout.write(`start-existing=${existing.trace.join("|")}\n`);
process.stdout.write(`start-error=${existing.error}\n`);
process.stdout.write(`checkpoint=${checkpoint.trace.join("|")}\n`);
process.stdout.write(`checkpoint-missing=${missingCheckpoint.trace.join("|")}\n`);
process.stdout.write(`failure=${failed.trace.join("|")}\n`);
process.stdout.write(`failure-mismatch=${failureMismatch.trace.join("|")}\n`);
process.stdout.write(`failure-error=${failureMismatch.error}\n`);
