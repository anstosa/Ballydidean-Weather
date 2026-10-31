import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import test from "node:test";
import {
  ADJUSTMENT_MAINTENANCE_STATE_ROOT_KIND,
  MaintenanceJournal,
  assembleConfirmationManifest,
  createMaintenanceJournal,
  validateAdjustmentFutureOnlyEpochWitness,
  verifyJournalBytes,
} from "./adjustment_maintenance_state.mjs";
import { canonicalJsonBytes } from "./adjustment_plaintext_archive.mjs";

const HASH_A = "a".repeat(64);
const HASH_B = "b".repeat(64);
const HASH_C = "c".repeat(64);
const HASH_D = "d".repeat(64);
const HASH_E = "e".repeat(64);
const HASH_F = "f".repeat(64);

// hold one append-only journal for deterministic tests
class MemoryJournalStore {
  constructor() {
    this.bytes = Buffer.alloc(0);
    this.heads = null;
    this.queue = Promise.resolve();
    this.failAppend = null;
  }

  // return one path-free state identity
  async initialize() {
    return { rootKind: ADJUSTMENT_MAINTENANCE_STATE_ROOT_KIND };
  }

  // serialize one exclusive transaction
  async withLock(callback) {
    const operation = this.queue.then(
      // execute inside the injected exclusive lock
      async () => await callback({
        // read exact journal bytes
        readJournal: async () => Buffer.from(this.bytes),
        // append and fsync one transaction
        append: async (bytes, heads) => {
          // inject a crash before journal mutation
          if (this.failAppend === "before") {
            this.failAppend = null;
            throw new Error("journal crash before append");
          }
          this.bytes = Buffer.concat([this.bytes, bytes]);

          // inject a crash after journal fsync but before head rotation
          if (this.failAppend === "after_journal") {
            this.failAppend = null;
            throw new Error("journal crash after append");
          }
          this.heads = structuredClone(heads);
        },
        // reconcile bounded head slots
        writeHeads: async (heads) => {
          this.heads = structuredClone(heads);
        },
      }),
    );
    this.queue = operation.catch(
      // keep later lock acquisition live after failure
      () => undefined,
    );
    return await operation;
  }
}

// add calendar days to one test date
function addDate(localDate, days) {
  return new Date(Date.parse(`${localDate}T00:00:00.000Z`) + days * 86_400_000)
    .toISOString().slice(0, 10);
}

// build one deterministic confirmation chunk array
function buildChunks(family, start, count, intervalDays) {
  let current = start;
  return Array.from({ length: count },
    // create one exact fourteen-day or final partition
    (_, index) => {
      const days = index === count - 1 ? intervalDays - 14 * (count - 1) : 14;
      const end = addDate(current, days);
      const chunk = {
        chunkIndex: index,
        fromLocalDate: current,
        toLocalDateExclusive: end,
        chunkSha256: index.toString(16).padStart(64, "0"),
        recordCount: family === "wind" ? 224 : 64,
        expectedKeySubsetSha256: (index + 100).toString(16).padStart(64, "0"),
        eligiblePredictionSubsetSha256: (index + 200).toString(16).padStart(64, "0"),
        missingKeySubsetSha256: (index + 300).toString(16).padStart(64, "0"),
      };
      current = end;
      return chunk;
    },
  );
}

// hash one canonical test document
function hashDocument(value) {
  return createHash("sha256").update(canonicalJsonBytes(value)).digest("hex");
}

test("owner terminal tombstones persist the exact family predecessor", async () => {
  const store = new MemoryJournalStore();
  const journal = new MaintenanceJournal(store);
  const tombstone = {
    contractVersion: "adjustment-shadow-terminal-tombstone/v3",
    reconciliationSha256: HASH_A,
    registrationSha256: HASH_B,
    terminalResultSha256: HASH_C,
  };
  assert.equal(await journal.readOwnerTerminalTombstone({ family: "rain" }), null);
  assert.deepEqual(await journal.recordOwnerTerminalTombstone({
    family: "rain",
    now: "2028-02-09T09:12:00.000Z",
    tombstone,
  }), tombstone);
  assert.deepEqual(await journal.readOwnerTerminalTombstone({ family: "rain" }),
    tombstone);
  assert.deepEqual(await journal.recordOwnerTerminalTombstone({
    family: "rain",
    now: "2028-02-09T09:13:00.000Z",
    tombstone,
  }), tombstone);
  await assert.rejects(journal.recordOwnerTerminalTombstone({
    family: "rain",
    now: "2028-02-09T09:13:00.000Z",
    tombstone: { ...tombstone, reconciliationSha256: "not-a-hash" },
  }), /reconciliationSha256/u);
});

test("terminal due outcome retains the exact owner retirement across report crashes", async () => {
  const { journal } = await createLifecycleJournal();
  const dueKey = "daily/2027-01-10";
  const inputHeadSha256 = (await journal.status()).headSha256;
  await journal.acquireLease({
    dueKey,
    inputHeadSha256,
    now: "2027-01-10T08:00:00.000Z",
    runId: "daily-terminal-crash",
    scope: "daily",
  });
  const request = {
    contractVersion: "fixture-terminal-retirement/v1",
    registrationSha256: HASH_A,
  };
  const retained = await journal.recordDueTerminalRetirement({
    dueKey,
    kind: "qualified_v3",
    now: "2027-01-10T08:01:00.000Z",
    request,
  });
  const outcome = {
    actionEligible: true,
    candidateGraphSha256: null,
    candidateSha256: null,
    fitReceiptSha256: null,
    reason: "daily_candidate_promoted",
    semanticInputSha256: HASH_B,
    servingChanged: true,
    state: "completed",
  };
  await journal.recordDueTerminalOutcome({
    dueKey,
    now: "2027-01-10T08:02:00.000Z",
    outcome,
  });
  assert.deepEqual(await journal.readDueTerminalOutcome({ dueKey }), outcome);
  assert.deepEqual(await journal.readDueTerminalRetirement({ dueKey }), {
    completed: false,
    ...retained,
  });
  const retry = await journal.recordDueTerminalRetirement({
    dueKey,
    kind: "qualified_v3",
    now: "2027-01-10T08:03:00.000Z",
    request,
  });
  assert.deepEqual(retry, retained);
  await journal.completeDueTerminalRetirement({
    dueKey,
    now: "2027-01-10T08:04:00.000Z",
    requestSha256: retained.requestSha256,
  });
  assert.equal((await journal.readDueTerminalRetirement({ dueKey })).completed, true);
  await assert.rejects(journal.recordDueTerminalRetirement({
    dueKey,
    kind: "unsupported_v1",
    now: "2027-01-10T08:05:00.000Z",
    request,
  }), (error) => error.reason === "journal_record_collision");
});

// build one complete bounded physical-part population
function buildPartedChunks(family, start, count, intervalDays) {
  let current = start;
  const chunks = [];
  const logicalExpectedRoots = [];

  // build every exact logical date window
  for (let logicalChunkIndex = 0; logicalChunkIndex < count; logicalChunkIndex += 1) {
    const days = logicalChunkIndex === count - 1
      ? intervalDays - 14 * (count - 1)
      : 14;
    const end = addDate(current, days);
    const partCount = logicalChunkIndex === 0 ? 2 : 1;
    const partExpectedRoots = [];

    // split the first logical window to exercise durable part order
    for (let partIndex = 0; partIndex < partCount; partIndex += 1) {
      const expectedKeySubsetSha256 = hashDocument([
        `${family}/${logicalChunkIndex}/${partIndex}`,
      ]);
      partExpectedRoots.push(expectedKeySubsetSha256);
      chunks.push({
        captureLocalDate: addDate(current, partIndex - 7),
        contractVersion: "adjustment-confirmation-chunk-part/v3",
        eligiblePredictionSubsetSha256: hashDocument([
          `eligible/${family}/${logicalChunkIndex}/${partIndex}`,
        ]),
        expectedKeyCount: 1,
        expectedKeySubsetSha256,
        fromLocalDate: current,
        logicalChunkIndex,
        missingKeyCount: 1,
        missingKeySubsetSha256: hashDocument([
          `${family}/${logicalChunkIndex}/${partIndex}`,
        ]),
        partCount,
        partIndex,
        partSha256: hashDocument({ family, logicalChunkIndex, partIndex }),
        recordCount: 0,
        toLocalDateExclusive: end,
      });
    }
    logicalExpectedRoots.push(hashDocument(partExpectedRoots));
    current = end;
  }
  return {
    chunks,
    expectedKeySetSha256: hashDocument(logicalExpectedRoots),
  };
}

// create one complete unscored temperature confirmation
async function createCompleteTemperatureMember(journal) {
  const registrationInput = {
    candidateKind: "temperature-delayed-mos/v1",
    candidateSha256: HASH_C,
    cohortLineageSha256: HASH_B,
    family: "temperature",
    firstTargetAt: "2026-01-02T08:00:00.000Z",
    gateManifestSha256: HASH_D,
    inputHeadSha256: HASH_A,
    intervalEndExclusiveLocalDate: "2027-01-03",
    intervalStartLocalDate: "2026-01-02",
    now: "2026-01-01T00:01:00.000Z",
    reservedKeySha256: HASH_B,
    sourceLineageSha256: HASH_A,
    terminalAccessAt: "2027-01-10T08:00:00.000Z",
  };
  const registration = await journal.preregisterConfirmation(registrationInput);
  await journal.recordRevisionSnapshot({
    entryCount: 100,
    expectedKeySetSha256: HASH_A,
    family: "temperature",
    now: "2027-01-10T07:59:00.000Z",
    registrationSha256: registration.registrationSha256,
    revisionCatalogWatermarkSha256: HASH_B,
    snapshotRootSha256: HASH_C,
    targetCutoffAt: "2027-01-09T08:00:00.000Z",
  });
  await journal.burnConfirmation({
    family: "temperature",
    now: "2027-01-10T08:00:00.000Z",
    registrationSha256: registration.registrationSha256,
  });

  // append the exact complete member
  for (const chunk of buildChunks("temperature", "2026-01-02", 27, 366)) {
    await journal.appendConfirmationChunk({
      chunk,
      family: "temperature",
      now: "2027-01-10T08:01:00.000Z",
      registrationSha256: registration.registrationSha256,
    });
  }
  await journal.finalizeConfirmationMember({
    family: "temperature",
    now: "2027-01-10T08:02:00.000Z",
    registrationSha256: registration.registrationSha256,
  });
  return { registration, registrationInput };
}

// initialize one isolated lifecycle journal
async function createLifecycleJournal() {
  const store = new MemoryJournalStore();
  const journal = new MaintenanceJournal(store);
  await journal.initialize();
  await journal.initializeLifecycle({
    now: "2026-01-01T00:00:00.000Z",
    v1LedgerSha256: HASH_A,
    v1TailSha256: HASH_B,
  });
  return { journal, store };
}

// build one canonical root-authenticated future-only epoch witness
function futureOnlyEpochWitness() {
  const unsigned = {
    activationKind: "inert_v14_pre_activation",
    archiveCommitOrdinal: "0",
    catalogFrontierSha256: createHash("sha256")
      .update("adjustment-revision-frontier/v1\n0\n").digest("hex"),
    contractVersion: "adjustment-revision-capture-epoch-witness/v1",
    controlPlaneSha256: HASH_A,
    controlPlaneVersion: "14",
    databaseMigrationHistorySha256:
      "6de5c8c7efaa448aeb12bf1a9debe6fe7d4d4d1003ee0e21ab619ffa624c3424",
    epochAt: "2026-10-10T08:00:00.000Z",
    servingSnapshotSha256: HASH_C,
    sourceCommit: "1".repeat(40),
    sourceRelease: "2026.10.10-1",
    sourceServerImageDigest: `sha256:${HASH_D}`,
    sourceWebImageDigest: `sha256:${HASH_E}`,
  };
  return {
    ...unsigned,
    witnessSha256: createHash("sha256").update(`${JSON.stringify(unsigned)}\n`).digest("hex"),
  };
}

test("future-only lifecycle genesis binds only the exact rolling capture ledger", async () => {
  const store = new MemoryJournalStore();
  const journal = new MaintenanceJournal(store);
  const witness = futureOnlyEpochWitness();
  await journal.initialize();
  assert.deepEqual(validateAdjustmentFutureOnlyEpochWitness(witness), witness);
  const catalogUnsigned = {
    ...witness,
    databaseMigrationHistorySha256:
      "422dac5e0547295b7fd973f8e1b234a013dd629b04001215a044bee928b77b6e",
  };
  delete catalogUnsigned.witnessSha256;
  const catalogWitness = {
    ...catalogUnsigned,
    witnessSha256: createHash("sha256")
      .update(`${JSON.stringify(catalogUnsigned)}\n`).digest("hex"),
  };
  assert.throws(() => validateAdjustmentFutureOnlyEpochWitness(catalogWitness));
  const first = await journal.initializeFutureOnlyLifecycle({
    now: "2026-10-10T08:00:01.000Z",
    witness,
  });
  assert.equal(first.status, "appended");
  assert.deepEqual(await journal.readRevisionCursor(), {
    archiveCommitOrdinal: "0",
    frontierSha256: witness.catalogFrontierSha256,
  });
  const archiveDueKey = `archive/revision-cold-page/${HASH_A}`;
  const archiveInputHeadSha256 = (await journal.status()).headSha256;
  await journal.acquireLease({
    dueKey: archiveDueKey,
    inputHeadSha256: archiveInputHeadSha256,
    now: "2026-10-10T08:00:01.100Z",
    runId: `archive-segment-${HASH_A}`,
    scope: "archive",
  });
  await journal.completeDue({
    dueKey: archiveDueKey,
    now: "2026-10-10T08:00:01.200Z",
    outputSha256: HASH_C,
  });
  await journal.releaseLease({
    dueKey: archiveDueKey,
    now: "2026-10-10T08:00:01.300Z",
    runId: `archive-segment-${HASH_A}`,
    scope: "archive",
  });
  const custodyRequest = {
    afterArchiveCommitOrdinal: "0",
    afterFrontierSha256: witness.catalogFrontierSha256,
    graphManifestSha256: HASH_C,
    memberRootSha256: HASH_D,
    pageSha256: HASH_A,
    previousPageSha256: HASH_B,
    startMemberSha256: HASH_E,
    startSha256: HASH_B,
    watermarkArchiveCommitOrdinal: "1",
    watermarkFrontierSha256: HASH_F,
  };
  const custody = await journal.recordRevisionCustodyIntent({
    nextArchiveCommitOrdinal: "1",
    nextFrontierSha256: HASH_F,
    now: "2026-10-10T08:00:01.400Z",
    request: custodyRequest,
  });
  assert.deepEqual(await journal.readPendingRevisionCustody(), custody);
  const advanced = await journal.advanceRevisionCursor({
    afterArchiveCommitOrdinal: "0",
    afterFrontierSha256: witness.catalogFrontierSha256,
    graphManifestSha256: HASH_C,
    nextArchiveCommitOrdinal: "1",
    nextFrontierSha256: HASH_F,
    now: "2026-10-10T08:00:01.500Z",
    pageSha256: HASH_A,
    startSha256: HASH_B,
  });
  assert.deepEqual(advanced, {
    archiveCommitOrdinal: "1",
    frontierSha256: HASH_F,
  });
  assert.deepEqual(await journal.advanceRevisionCursor({
    afterArchiveCommitOrdinal: "0",
    afterFrontierSha256: witness.catalogFrontierSha256,
    graphManifestSha256: HASH_C,
    nextArchiveCommitOrdinal: "1",
    nextFrontierSha256: HASH_F,
    now: "2026-10-10T08:00:01.500Z",
    pageSha256: HASH_A,
    startSha256: HASH_B,
  }), advanced);
  assert.deepEqual(await journal.readRevisionCursor(), advanced);
  assert.deepEqual(await journal.completeRevisionCustody({
    acknowledgementSha256: HASH_B,
    now: "2026-10-10T08:00:01.600Z",
    pageSha256: HASH_A,
  }), { status: "completed" });
  assert.equal(await journal.readPendingRevisionCustody(), null);
  assert.deepEqual(await journal.completeRevisionCustody({
    acknowledgementSha256: HASH_B,
    now: "2026-10-10T08:00:01.600Z",
    pageSha256: HASH_A,
  }), { status: "completed" });
  assert.deepEqual(await journal.listRevisionCatalogPages(), [{
    graphManifestSha256: HASH_C,
    pageSha256: HASH_A,
    startSha256: HASH_B,
  }]);
  const replay = await journal.initializeFutureOnlyLifecycle({
    now: "2026-10-10T08:00:01.000Z",
    witness,
  });
  assert.equal(replay.payload.sourceKind, "future_only_verified_capture_epoch");
  assert.match(store.bytes.toString("utf8"), /future_only_verified_capture_epoch/u);
  assert.doesNotMatch(store.bytes.toString("utf8"), /v1LedgerSha256|v1TailSha256/u);
  await assert.rejects(journal.initializeLifecycle({
    now: "2026-10-10T08:00:02.000Z",
    v1LedgerSha256: HASH_E,
    v1TailSha256: HASH_F,
  }), /journal_record_collision/u);
  assert.throws(
    // reject caller-selected clocks under an unchanged witness identity
    () => validateAdjustmentFutureOnlyEpochWitness({
      ...witness,
      epochAt: "2026-10-10T07:59:59.000Z",
    }),
    /witness is invalid/u,
  );
});

test("revision custody refuses naked graphs and mismatched completion", async () => {
  const store = new MemoryJournalStore();
  const journal = new MaintenanceJournal(store);
  const witness = futureOnlyEpochWitness();
  await journal.initialize();
  await journal.initializeFutureOnlyLifecycle({
    now: "2026-10-10T08:00:01.000Z",
    witness,
  });
  const request = {
    afterArchiveCommitOrdinal: "0",
    afterFrontierSha256: witness.catalogFrontierSha256,
    graphManifestSha256: HASH_C,
    memberRootSha256: HASH_D,
    pageSha256: HASH_A,
    previousPageSha256: HASH_B,
    startMemberSha256: null,
    startSha256: HASH_B,
    watermarkArchiveCommitOrdinal: "1",
    watermarkFrontierSha256: HASH_F,
  };
  await assert.rejects(
    journal.recordRevisionCustodyIntent({
      nextArchiveCommitOrdinal: "1",
      nextFrontierSha256: HASH_F,
      now: "2026-10-10T08:00:01.100Z",
      request,
    }),
    (error) => error.reason === "revision_custody_graph_unavailable",
  );
  await assert.rejects(
    journal.advanceRevisionCursor({
      afterArchiveCommitOrdinal: "0",
      afterFrontierSha256: witness.catalogFrontierSha256,
      graphManifestSha256: HASH_C,
      nextArchiveCommitOrdinal: "1",
      nextFrontierSha256: HASH_F,
      now: "2026-10-10T08:00:01.200Z",
      pageSha256: HASH_A,
      startSha256: HASH_B,
    }),
    (error) => error.reason === "revision_cursor_compare_and_swap_failed",
  );
  await assert.rejects(
    journal.completeRevisionCustody({
      acknowledgementSha256: HASH_B,
      now: "2026-10-10T08:00:01.300Z",
      pageSha256: HASH_A,
    }),
    (error) => error.reason === "revision_custody_completion_unavailable",
  );
});

test("checkpoint custody advances before later immutable pack sealing", async () => {
  const store = new MemoryJournalStore();
  const journal = new MaintenanceJournal(store);
  const witness = futureOnlyEpochWitness();
  await journal.initialize();
  await journal.initializeFutureOnlyLifecycle({
    now: "2026-10-10T08:00:01.000Z",
    witness,
  });
  const request = {
    afterArchiveCommitOrdinal: "0",
    afterFrontierSha256: witness.catalogFrontierSha256,
    custodyCheckpointSha256: HASH_C,
    memberRootSha256: HASH_D,
    pageSha256: HASH_A,
    previousPageSha256: HASH_B,
    startMemberSha256: HASH_E,
    startSha256: HASH_B,
    watermarkArchiveCommitOrdinal: "1",
    watermarkFrontierSha256: HASH_F,
  };
  assert.deepEqual(await journal.recordRevisionCustodyIntent({
    nextArchiveCommitOrdinal: "1",
    nextFrontierSha256: HASH_F,
    now: "2026-10-10T08:00:01.100Z",
    request,
  }), {
    nextArchiveCommitOrdinal: "1",
    nextFrontierSha256: HASH_F,
    request,
  });
  assert.deepEqual(await journal.listRevisionCatalogPages(), []);
  assert.deepEqual(await journal.advanceRevisionCursor({
    afterArchiveCommitOrdinal: "0",
    afterFrontierSha256: witness.catalogFrontierSha256,
    custodyCheckpointSha256: HASH_C,
    nextArchiveCommitOrdinal: "1",
    nextFrontierSha256: HASH_F,
    now: "2026-10-10T08:00:01.200Z",
    pageSha256: HASH_A,
    startSha256: HASH_B,
  }), {
    archiveCommitOrdinal: "1",
    frontierSha256: HASH_F,
  });
  const retirementEntries = [
    { fileSha256: HASH_A, identitySha256: HASH_A, kind: "revision_commit_receipt" },
    { fileSha256: HASH_B, identitySha256: HASH_B, kind: "revision_frontier_successor" },
    { fileSha256: HASH_C, identitySha256: HASH_C, kind: "revision_projection" },
    { fileSha256: HASH_D, identitySha256: HASH_D, kind: "revision_stage_receipt" },
  ];
  const acknowledgementUnsigned = {
    acknowledgedAt: "2026-10-10T08:00:01.250Z",
    afterArchiveCommitOrdinal: request.afterArchiveCommitOrdinal,
    afterFrontierSha256: request.afterFrontierSha256,
    authority: "cold_custody_only",
    contractVersion: "adjustment-revision-custody-acknowledgement/v2",
    custodyCheckpointSha256: request.custodyCheckpointSha256,
    memberRootSha256: request.memberRootSha256,
    nextArchiveCommitOrdinal: "1",
    nextFrontierSha256: HASH_F,
    pageSha256: request.pageSha256,
    previousAcknowledgementSha256: null,
    previousPageSha256: request.previousPageSha256,
    retirementEntries,
    startMemberSha256: request.startMemberSha256,
    startSha256: request.startSha256,
    watermarkArchiveCommitOrdinal: request.watermarkArchiveCommitOrdinal,
    watermarkFrontierSha256: request.watermarkFrontierSha256,
  };
  const acknowledgement = {
    ...acknowledgementUnsigned,
    acknowledgementSha256: createHash("sha256")
      .update(canonicalJsonBytes(acknowledgementUnsigned)).digest("hex"),
  };
  await journal.completeRevisionCustodyV2({
    acknowledgement,
    now: "2026-10-10T08:00:01.300Z",
  });
  assert.deepEqual(await journal.readLatestRevisionCustodyAcknowledgement(), acknowledgement);
  const dueKey = `archive/revision-custody-pack/${HASH_C}`;
  const inputHeadSha256 = (await journal.status()).headSha256;
  await journal.acquireLease({
    dueKey,
    inputHeadSha256,
    now: "2026-10-10T08:00:01.400Z",
    runId: `archive-segment-${HASH_C}`,
    scope: "archive",
  });
  await journal.completeDue({
    dueKey,
    now: "2026-10-10T08:00:01.500Z",
    outputSha256: HASH_E,
  });
  await journal.releaseLease({
    dueKey,
    now: "2026-10-10T08:00:01.600Z",
    runId: `archive-segment-${HASH_C}`,
    scope: "archive",
  });
  assert.deepEqual(await journal.recordRevisionCustodyPackSeal({
    checkpointSha256: HASH_C,
    graphManifestSha256: HASH_E,
    now: "2026-10-10T08:00:01.700Z",
    pages: [{
      memberRootSha256: HASH_D,
      pageSha256: HASH_A,
      startMemberSha256: HASH_E,
      startSha256: HASH_B,
    }],
  }), { status: "sealed" });
  assert.deepEqual(await journal.listRevisionCatalogPages(), [{
    graphManifestSha256: HASH_E,
    pageSha256: HASH_A,
    startSha256: HASH_B,
  }]);
});

// build one later nonoverlapping temperature registration
function laterTemperatureRegistration(registrationInput) {
  return {
    ...registrationInput,
    candidateSha256: HASH_D,
    cohortLineageSha256: HASH_E,
    firstTargetAt: "2027-01-20T08:00:00.000Z",
    gateManifestSha256: HASH_F,
    inputHeadSha256: HASH_E,
    intervalEndExclusiveLocalDate: "2028-01-21",
    intervalStartLocalDate: "2027-01-20",
    now: "2027-01-19T08:00:00.000Z",
    reservedKeySha256: HASH_F,
    sourceLineageSha256: HASH_E,
    terminalAccessAt: "2028-01-28T08:00:00.000Z",
  };
}

test("leases enforce durations, due collisions, expiry reconciliation and clock fencing", async () => {
  const store = new MemoryJournalStore();
  const journal = new MaintenanceJournal(store);
  assert.deepEqual(await journal.initialize(), { head: null, records: 0 });
  const acquired = await journal.acquireLease({
    dueKey: "daily/2026-10-08",
    inputHeadSha256: HASH_A,
    now: "2026-10-08T00:00:00.000Z",
    runId: "daily-1",
    scope: "daily",
  });
  assert.equal(acquired.expiresAt, "2026-10-08T03:00:00.000Z");
  assert.equal(acquired.fencingToken, null);
  const retry = await journal.acquireLease({
    dueKey: "daily/2026-10-08",
    inputHeadSha256: HASH_A,
    now: "2026-10-08T00:01:00.000Z",
    runId: "daily-1",
    scope: "daily",
  });
  assert.equal(retry.runId, "daily-1");
  assert.deepEqual(await journal.inspectDueKeys({
    dueKeys: ["daily/2026-10-08", "monthly/wind/2026-10"],
  }), [{
    dueKey: "daily/2026-10-08",
    inputHeadSha256: HASH_A,
    outputSha256: null,
    status: "registered",
  }, {
    dueKey: "monthly/wind/2026-10",
    inputHeadSha256: null,
    outputSha256: null,
    status: "absent",
  }]);
  await assert.rejects(
    // refuse duplicate keys and unbounded caller scans
    journal.inspectDueKeys({
      dueKeys: ["daily/2026-10-08", "daily/2026-10-08"],
    }),
    /dueKeys are invalid/u,
  );
  await assert.rejects(
    // reject the same due key with different immutable inputs
    journal.acquireLease({
      dueKey: "daily/2026-10-08",
      inputHeadSha256: HASH_B,
      now: "2026-10-08T00:02:00.000Z",
      runId: "daily-2",
      scope: "daily",
    }),
    (error) => error.reason === "lease_occupied" || error.reason === "due_key_collision",
  );
  await journal.releaseLease({
    dueKey: "daily/2026-10-08",
    now: "2026-10-08T00:03:00.000Z",
    runId: "daily-1",
    scope: "daily",
  });

  await assert.rejects(
    // reject a wall-clock rollback greater than five minutes
    journal.acquireLease({
      dueKey: "daily/2026-10-09",
      inputHeadSha256: HASH_A,
      now: "2026-10-07T23:00:00.000Z",
      runId: "daily-rollback",
      scope: "daily",
    }),
    (error) => error.reason === "clock_untrusted",
  );

  await journal.acquireLease({
    dueKey: "capture/2026-10-08T06:35:00.000Z",
    inputHeadSha256: HASH_A,
    now: "2026-10-08T06:35:00.000Z",
    runId: "archive-1",
    scope: "archive",
  });
  await assert.rejects(
    // force reconciliation after expiry instead of blind takeover
    journal.acquireLease({
      dueKey: "daily/2026-10-09",
      inputHeadSha256: HASH_A,
      now: "2026-10-08T08:00:00.000Z",
      runId: "daily-forward",
      scope: "daily",
    }),
    (error) => error.reason === "reconciliation_required",
  );
  const reconciliation = await journal.reconcileExpiredLease({
    dueKey: "capture/2026-10-08T06:35:00.000Z",
    immutableOutputSha256: null,
    now: "2026-10-08T08:00:00.000Z",
    remoteStateSha256: HASH_B,
    resolution: "resume",
    runId: "archive-1",
    scope: "archive",
  });
  assert.deepEqual(reconciliation, { status: "reconciled", resolution: "resume" });
  assert.ok(verifyJournalBytes(store.bytes).length >= 5);
});

test("release fencing tokens increase and exact completed due keys never replay", async () => {
  const store = new MemoryJournalStore();
  const journal = new MaintenanceJournal(store);
  await journal.initialize();
  const first = await journal.acquireLease({
    dueKey: "rollback/temperature/1",
    inputHeadSha256: HASH_A,
    now: "2026-10-08T00:00:00.000Z",
    runId: "release-1",
    scope: "release",
  });
  assert.equal(first.expiresAt, "2026-10-08T08:00:00.000Z");
  assert.equal(first.fencingToken, "1");
  await journal.releaseLease({
    dueKey: "rollback/temperature/1",
    now: "2026-10-08T00:01:00.000Z",
    runId: "release-1",
    scope: "release",
  });
  const second = await journal.acquireLease({
    dueKey: "rollback/temperature/2",
    inputHeadSha256: HASH_B,
    now: "2026-10-08T00:02:00.000Z",
    runId: "release-2",
    scope: "release",
  });
  assert.equal(second.fencingToken, "2");
  await journal.completeDue({
    dueKey: "rollback/temperature/2",
    now: "2026-10-08T00:03:00.000Z",
    outputSha256: HASH_C,
  });
  const completed = await journal.acquireLease({
    dueKey: "rollback/temperature/2",
    inputHeadSha256: HASH_B,
    now: "2026-10-08T00:04:00.000Z",
    runId: "release-2",
    scope: "release",
  });
  assert.equal(completed.runId, "release-2");
  assert.equal((await journal.status()).fencingToken, "2");
});

test("journal crash retry reconciles an appended tail without duplicate authority", async () => {
  const store = new MemoryJournalStore();
  const journal = new MaintenanceJournal(store);
  await journal.initialize();
  store.failAppend = "before";
  await assert.rejects(
    // crash before any append-only mutation
    journal.acquireLease({
      dueKey: "daily/2026-10-08",
      inputHeadSha256: HASH_A,
      now: "2026-10-08T00:00:00.000Z",
      runId: "daily-crash",
      scope: "daily",
    }),
    /before append/u,
  );
  assert.equal(verifyJournalBytes(store.bytes).length, 0);

  store.failAppend = "after_journal";
  await assert.rejects(
    // crash after journal fsync but before bounded head rotation
    journal.acquireLease({
      dueKey: "daily/2026-10-08",
      inputHeadSha256: HASH_A,
      now: "2026-10-08T00:00:00.000Z",
      runId: "daily-crash",
      scope: "daily",
    }),
    /after append/u,
  );
  assert.equal(verifyJournalBytes(store.bytes).length, 2);
  const initialized = await journal.initialize();
  assert.equal(initialized.records, 2);
  const retry = await journal.acquireLease({
    dueKey: "daily/2026-10-08",
    inputHeadSha256: HASH_A,
    now: "2026-10-08T00:00:00.000Z",
    runId: "daily-crash",
    scope: "daily",
  });
  assert.equal(retry.runId, "daily-crash");
  assert.equal(verifyJournalBytes(store.bytes).length, 2);
});

test("27/27/24 confirmation members assemble deterministically without values", () => {
  const configurations = [
    ["temperature", 27, 366],
    ["wind", 27, 366],
    ["rain", 24, 334],
  ];

  // prove each family-specific complete member shape
  for (const [family, count, intervalDays] of configurations) {
    const start = "2026-10-31";
    const end = addDate(start, intervalDays);
    const registration = {
      family,
      registrationSha256: HASH_A,
      intervalStartLocalDate: start,
      intervalEndExclusiveLocalDate: end,
    };
    const access = {
      accessSha256: HASH_B,
      expectedKeySetSha256: HASH_A,
      revisionCatalogWatermarkSha256: HASH_C,
      targetComparatorSnapshotRootSha256: HASH_D,
      targetCutoffAt: "2027-10-07T07:00:00.000Z",
    };
    const chunks = buildChunks(family, start, count, intervalDays);
    const first = assembleConfirmationManifest({ access, chunks, registration });
    const second = assembleConfirmationManifest({ access, chunks, registration });
    assert.equal(first.chunkCount, count);
    assert.equal(first.intervalEndExclusiveLocalDate, end);
    assert.equal(first.fullMemberRootSha256, second.fullMemberRootSha256);
    assert.equal(JSON.stringify(first).includes("values"), false);

    const broken = structuredClone(chunks);
    broken.at(-1).fromLocalDate = addDate(broken.at(-1).fromLocalDate, 1);
    assert.throws(
      // reject a final partition gap
      () => assembleConfirmationManifest({ access, chunks: broken, registration }),
      /partition/u,
    );
  }
});

test("journal appends bounded v3 parts in order and finalizes logical windows", async () => {
  const { journal } = await createLifecycleJournal();
  const population = buildPartedChunks("temperature", "2026-01-02", 27, 366);
  const registration = await journal.preregisterConfirmation({
    candidateKind: "temperature-delayed-mos/v1",
    candidateSha256: HASH_C,
    cohortLineageSha256: HASH_B,
    family: "temperature",
    firstTargetAt: "2026-01-02T08:00:00.000Z",
    gateManifestSha256: HASH_D,
    inputHeadSha256: HASH_A,
    intervalEndExclusiveLocalDate: "2027-01-03",
    intervalStartLocalDate: "2026-01-02",
    now: "2026-01-01T00:01:00.000Z",
    reservedKeySha256: population.expectedKeySetSha256,
    sourceLineageSha256: HASH_A,
    terminalAccessAt: "2027-01-10T08:00:00.000Z",
  });
  await journal.recordRevisionSnapshot({
    entryCount: population.chunks.length,
    expectedKeySetSha256: population.expectedKeySetSha256,
    family: "temperature",
    now: "2027-01-10T07:59:00.000Z",
    registrationSha256: registration.registrationSha256,
    revisionCatalogWatermarkSha256: HASH_B,
    snapshotRootSha256: HASH_C,
    targetCutoffAt: "2027-01-09T08:00:00.000Z",
  });
  await journal.burnConfirmation({
    family: "temperature",
    now: "2027-01-10T08:00:00.000Z",
    registrationSha256: registration.registrationSha256,
  });
  await assert.rejects(
    // reject a later part before the first absent part
    journal.appendConfirmationChunk({
      chunk: population.chunks[1],
      family: "temperature",
      now: "2027-01-10T08:01:00.000Z",
      registrationSha256: registration.registrationSha256,
    }),
    (error) => error.reason === "confirmation_chunk_order_refused",
  );

  // append every physical part once
  for (const chunk of population.chunks) {
    const appended = await journal.appendConfirmationChunk({
      chunk,
      family: "temperature",
      now: "2027-01-10T08:01:00.000Z",
      registrationSha256: registration.registrationSha256,
    });
    assert.equal(appended.chunkKey,
      `${chunk.logicalChunkIndex}/${chunk.partIndex}`);
  }
  const retry = await journal.appendConfirmationChunk({
    chunk: population.chunks[0],
    family: "temperature",
    now: "2027-01-10T08:01:01.000Z",
    registrationSha256: registration.registrationSha256,
  });
  assert.equal(retry.status, "already_present");
  const fullMember = await journal.finalizeConfirmationMember({
    family: "temperature",
    now: "2027-01-10T08:02:00.000Z",
    registrationSha256: registration.registrationSha256,
  });
  assert.equal(fullMember.contractVersion, "adjustment-confirmation-member/v3");
  assert.equal(fullMember.chunkCount, 27);
  assert.equal(fullMember.chunks[0].partCount, 2);
  assert.equal(fullMember.expectedKeySetSha256, population.expectedKeySetSha256);
});

test("lifecycle preregistration, snapshot, burn, chunks, result and embargo are append only", async () => {
  const store = new MemoryJournalStore();
  const journal = new MaintenanceJournal(store);
  await journal.initialize();
  await journal.initializeLifecycle({
    now: "2026-01-01T00:00:00.000Z",
    v1LedgerSha256: HASH_A,
    v1TailSha256: HASH_B,
  });
  const registrationInput = {
    candidateKind: "temperature-delayed-mos/v1",
    candidateSha256: HASH_C,
    cohortLineageSha256: HASH_B,
    family: "temperature",
    firstTargetAt: "2026-01-02T08:00:00.000Z",
    gateManifestSha256: HASH_D,
    inputHeadSha256: HASH_A,
    intervalEndExclusiveLocalDate: "2027-01-03",
    intervalStartLocalDate: "2026-01-02",
    now: "2026-01-01T00:01:00.000Z",
    reservedKeySha256: HASH_B,
    sourceLineageSha256: HASH_A,
    terminalAccessAt: "2027-01-10T08:00:00.000Z",
  };
  const registration = await journal.preregisterConfirmation(registrationInput);
  const registeredProjection = await journal.readConfirmationLifecycle({
    family: "temperature",
    registrationSha256: registration.registrationSha256,
  });
  assert.equal(registeredProjection.state, "registered");
  assert.equal(registeredProjection.access, null);
  assert.deepEqual(registeredProjection.chunks, []);
  assert.deepEqual(await journal.readActiveConfirmation({ family: "temperature" }),
    registeredProjection);
  assert.equal(await journal.readActiveConfirmation({ family: "wind" }), null);
  assert.equal(await journal.readActiveDevelopmentMaterial({ family: "temperature" }), null);
  const installed = await journal.recordDevelopmentCandidateInstalled({
    actionSha256: HASH_D,
    artifactSha256: HASH_A,
    candidateGraphSha256: HASH_B,
    candidateSha256: HASH_C,
    family: "temperature",
    now: "2026-01-01T00:01:30.000Z",
    registrationSha256: registration.registrationSha256,
    shadowRegistrationSha256: HASH_D,
  });
  assert.deepEqual(await journal.readActiveDevelopmentMaterial({ family: "temperature" }),
    installed);
  assert.deepEqual(await journal.recordDevelopmentCandidateInstalled({
    ...installed,
    now: "2026-01-01T00:01:31.000Z",
  }), installed);
  assert.deepEqual(await journal.preregisterConfirmation(registrationInput), {
    registrationSha256: registration.registrationSha256,
    status: "already_registered",
  });
  await assert.rejects(
    // prohibit a second active same-family member
    journal.preregisterConfirmation({
      ...registrationInput,
      candidateSha256: HASH_D,
      now: "2026-01-01T00:02:00.000Z",
    }),
    (error) => error.reason === "confirmation_slot_occupied",
  );
  await journal.recordRevisionSnapshot({
    entryCount: 100,
    expectedKeySetSha256: HASH_A,
    family: "temperature",
    now: "2027-01-10T07:59:00.000Z",
    registrationSha256: registration.registrationSha256,
    revisionCatalogWatermarkSha256: HASH_B,
    snapshotRootSha256: HASH_C,
    targetCutoffAt: "2027-01-09T08:00:00.000Z",
  });
  await journal.recordRevisionSnapshot({
    entryCount: 100,
    expectedKeySetSha256: HASH_A,
    family: "temperature",
    now: "2027-01-10T07:59:30.000Z",
    registrationSha256: registration.registrationSha256,
    revisionCatalogWatermarkSha256: HASH_B,
    snapshotRootSha256: HASH_C,
    targetCutoffAt: "2027-01-09T08:00:00.000Z",
  });
  await assert.rejects(
    // reject a later revision-root substitution for the same registration
    journal.recordRevisionSnapshot({
      entryCount: 100,
      expectedKeySetSha256: HASH_A,
      family: "temperature",
      now: "2027-01-10T07:59:31.000Z",
      registrationSha256: registration.registrationSha256,
      revisionCatalogWatermarkSha256: HASH_B,
      snapshotRootSha256: HASH_D,
      targetCutoffAt: "2027-01-09T08:00:00.000Z",
    }),
    (error) => error.reason === "journal_record_collision",
  );
  await assert.rejects(
    // make premature access permanently unavailable to action code
    journal.burnConfirmation({
      family: "temperature",
      now: "2027-01-10T07:59:59.000Z",
      registrationSha256: registration.registrationSha256,
    }),
    (error) => error.reason === "confirmation_access_premature",
  );
  const burn = await journal.burnConfirmation({
    family: "temperature",
    now: "2027-01-10T08:00:00.000Z",
    registrationSha256: registration.registrationSha256,
  });
  assert.match(burn.accessSha256, /^[a-f0-9]{64}$/u);
  const burnedProjection = await journal.readConfirmationLifecycle({
    family: "temperature",
    registrationSha256: registration.registrationSha256,
  });
  assert.equal(burnedProjection.state, "burned");
  assert.equal(burnedProjection.access.accessSha256, burn.accessSha256);
  const chunks = buildChunks("temperature", "2026-01-02", 27, 366);

  // append every immutable chunk once
  for (const chunk of chunks) {
    await journal.appendConfirmationChunk({
      chunk,
      family: "temperature",
      now: "2027-01-10T08:01:00.000Z",
      registrationSha256: registration.registrationSha256,
    });
  }

  const complete = await journal.finalizeConfirmationMember({
    family: "temperature",
    now: "2027-01-10T08:02:00.000Z",
    registrationSha256: registration.registrationSha256,
  });
  assert.equal(complete.chunkCount, 27);
  const completeProjection = await journal.readConfirmationLifecycle({
    family: "temperature",
    registrationSha256: registration.registrationSha256,
  });
  assert.equal(completeProjection.state, "complete");
  assert.equal(completeProjection.chunks.length, 27);
  assert.equal(completeProjection.fullManifest.fullMemberRootSha256,
    complete.fullMemberRootSha256);
  const result = await journal.recordConfirmationResult({
    actionIdentitySha256: null,
    candidateReportSha256: HASH_D,
    disposition: "support_failed",
    family: "temperature",
    nextConfirmationEligibleAt: "2027-01-18T08:00:00.000Z",
    now: "2027-01-10T08:03:00.000Z",
    registrationSha256: registration.registrationSha256,
  });
  assert.equal(result.fullMemberRootSha256, complete.fullMemberRootSha256);
  const terminalProjection = await journal.readConfirmationLifecycle({
    family: "temperature",
    registrationSha256: registration.registrationSha256,
  });
  assert.equal(terminalProjection.state, "terminal");
  assert.equal(terminalProjection.result.disposition, "support_failed");
  assert.equal(await journal.readActiveConfirmation({ family: "temperature" }), null);
  assert.deepEqual(await journal.readConfirmationByCandidate({
    candidateSha256: HASH_C,
    family: "temperature",
  }), terminalProjection);
  assert.equal(await journal.readConfirmationLifecycle({
    family: "wind",
    registrationSha256: HASH_F,
  }), null);
  await assert.rejects(
    // enforce the seven-full-local-date embargo projection
    journal.preregisterConfirmation({
      ...registrationInput,
      candidateSha256: HASH_D,
      firstTargetAt: "2027-01-12T08:00:00.000Z",
      intervalStartLocalDate: "2027-01-12",
      intervalEndExclusiveLocalDate: "2028-01-13",
      now: "2027-01-11T00:00:00.000Z",
      terminalAccessAt: "2028-01-20T08:00:00.000Z",
    }),
    (error) => error.reason === "confirmation_embargo_active",
  );
});

test("promoted confirmation keeps its family slot through fenced action acknowledgement", async () => {
  const { journal, store } = await createLifecycleJournal();
  const { registration, registrationInput } = await createCompleteTemperatureMember(journal);
  const resultInput = {
    actionIdentitySha256: HASH_E,
    candidateReportSha256: HASH_D,
    disposition: "promoted",
    family: "temperature",
    nextConfirmationEligibleAt: "2027-01-18T08:00:00.000Z",
    now: "2027-01-10T08:03:00.000Z",
    registrationSha256: registration.registrationSha256,
  };
  await assert.rejects(
    // require an immutable action identity for promotion
    journal.recordConfirmationResult({ ...resultInput, actionIdentitySha256: null }),
    (error) => error.reason === "confirmation_action_identity_invalid",
  );
  await assert.rejects(
    // prohibit action identity on a no-action result
    journal.recordConfirmationResult({ ...resultInput, disposition: "rejected" }),
    (error) => error.reason === "confirmation_action_identity_invalid",
  );
  await journal.recordConfirmationResult(resultInput);
  await assert.rejects(
    // retain the family slot after promoted scoring
    journal.preregisterConfirmation(laterTemperatureRegistration(registrationInput)),
    (error) => error.reason === "confirmation_slot_occupied",
  );
  await journal.acquireLease({
    dueKey: "daily/2027-01-10",
    inputHeadSha256: HASH_A,
    now: "2027-01-10T08:04:00.000Z",
    runId: "daily-not-action-authority",
    scope: "daily",
  });
  const preparationInput = {
    actionIdentitySha256: HASH_E,
    actionManifestSha256: HASH_F,
    family: "temperature",
    fencingToken: "1",
    now: "2027-01-10T08:05:00.000Z",
    registrationSha256: registration.registrationSha256,
    releaseDueKey: `confirmation/temperature/${HASH_C}`,
    releaseRunId: "release-temperature-promotion",
  };
  await assert.rejects(
    // refuse daily leases as action authority
    journal.prepareConfirmationAction(preparationInput),
    (error) => error.reason === "release_lease_unavailable",
  );
  await journal.releaseLease({
    dueKey: "daily/2027-01-10",
    now: "2027-01-10T08:06:00.000Z",
    runId: "daily-not-action-authority",
    scope: "daily",
  });
  const release = await journal.acquireLease({
    dueKey: `confirmation/temperature/${HASH_C}`,
    inputHeadSha256: HASH_A,
    now: "2027-01-10T08:07:00.000Z",
    runId: "release-temperature-promotion",
    scope: "release",
  });
  assert.equal(release.fencingToken, "1");
  await assert.rejects(
    // prohibit a daily due key from authorizing promotion
    journal.prepareConfirmationAction({
      ...preparationInput,
      now: "2027-01-10T08:07:30.000Z",
      releaseDueKey: "daily/2027-01-10",
    }),
    (error) => error.reason === "confirmation_action_due_mismatch",
  );
  const prepared = await journal.prepareConfirmationAction({
    ...preparationInput,
    now: "2027-01-10T08:08:00.000Z",
  });
  assert.equal(prepared.actionState, "release_pending");
  const countAfterPrepare = verifyJournalBytes(store.bytes).length;
  await journal.prepareConfirmationAction({
    ...preparationInput,
    now: "2027-01-10T08:09:00.000Z",
  });
  assert.equal(verifyJournalBytes(store.bytes).length, countAfterPrepare);
  const releaseMapping = {
    actionSha256: HASH_F,
    actionTag: `adjustment-action/temperature/${HASH_F}`,
    branch: `automation/adjustment-temperature-${HASH_C.slice(0, 12)}-${HASH_F.slice(0, 12)}`,
    commitSha: "1".repeat(40),
    expectedSourceCommit: "2".repeat(40),
    family: "temperature",
    fencingToken: "1",
    now: "2027-01-10T08:09:10.000Z",
    packageRootSha256: HASH_D,
    releaseDueKey: `confirmation/temperature/${HASH_C}`,
    releaseRunId: "release-temperature-promotion",
    releaseTag: "2027.01.10-1",
  };
  await assert.rejects(
    // prohibit git release identity drift from confirmation authority
    journal.prepareModelRelease({ ...releaseMapping, actionSha256: HASH_A }),
    (error) => error.reason === "model_release_fence_reused",
  );
  await journal.prepareModelRelease(releaseMapping);
  await assert.rejects(
    // retain the global fence through action reconciliation
    journal.releaseLease({
      dueKey: `confirmation/temperature/${HASH_C}`,
      now: "2027-01-10T08:09:30.000Z",
      runId: "release-temperature-promotion",
      scope: "release",
    }),
    (error) => error.reason === "confirmation_action_reconciliation_required",
  );
  await assert.rejects(
    // enforce apply before verification
    journal.recordConfirmationActionVerified({
      actionIdentitySha256: HASH_E,
      family: "temperature",
      fencingToken: "1",
      now: "2027-01-10T08:10:00.000Z",
      registrationSha256: registration.registrationSha256,
      verificationOutcome: "verified",
      verificationSha256: HASH_B,
    }),
    (error) => error.reason === "confirmation_action_not_applied",
  );
  const applyInput = {
    actionIdentitySha256: HASH_E,
    applyReceiptSha256: HASH_A,
    family: "temperature",
    fencingToken: "1",
    now: "2027-01-10T08:11:00.000Z",
    registrationSha256: registration.registrationSha256,
  };
  await journal.recordConfirmationActionApplied(applyInput);
  await journal.recordConfirmationActionApplied({
    ...applyInput,
    now: "2027-01-10T08:12:00.000Z",
  });
  await assert.rejects(
    // reject altered apply evidence
    journal.recordConfirmationActionApplied({
      ...applyInput,
      applyReceiptSha256: HASH_B,
      now: "2027-01-10T08:13:00.000Z",
    }),
    (error) => error.reason === "confirmation_action_transition_collision",
  );
  const verificationInput = {
    actionIdentitySha256: HASH_E,
    family: "temperature",
    fencingToken: "1",
    now: "2027-01-10T08:14:00.000Z",
    registrationSha256: registration.registrationSha256,
    verificationOutcome: "verified",
    verificationSha256: HASH_B,
  };
  await journal.recordConfirmationActionVerified(verificationInput);
  const countAfterVerification = verifyJournalBytes(store.bytes).length;
  await journal.recordConfirmationActionVerified({
    ...verificationInput,
    now: "2027-01-10T08:14:30.000Z",
  });
  assert.equal(verifyJournalBytes(store.bytes).length, countAfterVerification);
  await assert.rejects(
    // bind acknowledgement to the verified operator outcome
    journal.acknowledgeConfirmationAction({
      acknowledgementSha256: HASH_C,
      actionIdentitySha256: HASH_E,
      family: "temperature",
      fencingToken: "1",
      now: "2027-01-10T08:15:00.000Z",
      outcome: "deployed_operator_off",
      registrationSha256: registration.registrationSha256,
    }),
    (error) => error.reason === "confirmation_action_acknowledgement_refused",
  );
  const acknowledgementInput = {
    acknowledgementSha256: HASH_C,
    actionIdentitySha256: HASH_E,
    family: "temperature",
    fencingToken: "1",
    now: "2027-01-10T08:16:00.000Z",
    outcome: "active",
    registrationSha256: registration.registrationSha256,
  };
  const acknowledgement = await journal.acknowledgeConfirmationAction(acknowledgementInput);
  assert.equal(acknowledgement.actionState, "active");
  const countAfterAcknowledgement = verifyJournalBytes(store.bytes).length;
  await journal.acknowledgeConfirmationAction({
    ...acknowledgementInput,
    now: "2027-01-10T08:16:30.000Z",
  });
  assert.equal(verifyJournalBytes(store.bytes).length, countAfterAcknowledgement);
  await journal.releaseLease({
    dueKey: `confirmation/temperature/${HASH_C}`,
    now: "2027-01-10T08:17:00.000Z",
    runId: "release-temperature-promotion",
    scope: "release",
  });
  const next = await journal.preregisterConfirmation(
    laterTemperatureRegistration(registrationInput),
  );
  assert.equal(next.status, "registered");
  const actionKinds = verifyJournalBytes(store.bytes).filter(
    // retain only action lifecycle records
    (record) => record.kind.startsWith("confirmation_action_"),
  ).map(
    // project exact transition kinds
    (record) => record.kind,
  );
  assert.deepEqual(actionKinds, [
    "confirmation_action_prepared",
    "confirmation_action_applied",
    "confirmation_action_verified",
    "confirmation_action_acknowledged",
  ]);
});

test("qualified recovery documents retain one original action fence and release pair", async () => {
  const { journal, store } = await createLifecycleJournal();
  const { registration } = await createCompleteTemperatureMember(journal);
  const lifecycle = await journal.readConfirmationLifecycle({
    family: "temperature",
    registrationSha256: registration.registrationSha256,
  });
  const sourceCommit = "1".repeat(40);
  const targetAction = {
    actionKind: "promote",
    candidateGraphSha256: HASH_A,
    candidateSha256: HASH_C,
    contractVersion: "forecast-adjustment-model-action/v1",
    createdAt: "2027-01-10T08:03:00.000Z",
    expectedInstalledReceiptSha256: null,
    expectedSettingsSha256: HASH_B,
    expectedSourceCommit: sourceCommit,
    expectedSourceRelease: "2027.01.09-1",
    family: "temperature",
    fencingToken: "1",
    fullMemberRootSha256: lifecycle.fullManifest.fullMemberRootSha256,
    lifecycleHeadSha256: HASH_D,
    policyDecision: "qualified",
    policyReportSha256: HASH_E,
    predecessorActionSha256: null,
    reason: "qualified_candidate",
    reportCreatedAt: "2027-01-10T08:03:00.000Z",
    siteKey: "ballydidean",
    validThrough: "2027-01-17T08:03:00.000Z",
  };
  const targetActionSha256 = hashDocument(targetAction);
  const dueKey = `confirmation/temperature/${HASH_C}`;
  const runId = `release-promote-${HASH_C.slice(0, 32)}`;
  const lease = await journal.acquireLease({
    dueKey,
    inputHeadSha256: (await journal.status()).headSha256,
    now: "2027-01-10T08:04:00.000Z",
    runId,
    scope: "release",
  });
  assert.equal(lease.fencingToken, targetAction.fencingToken);
  const seal = {
    graphManifestSha256: HASH_F,
    sourceCommit,
  };
  const anchor = {
    actionSha256: targetActionSha256,
    inputSealSha256: hashDocument(seal),
  };
  const finalizationProof = {
    actionSha256: targetActionSha256,
    contractVersion: "adjustment-maintenance-finalization-proof/v3",
    family: "temperature",
    graphManifestSha256: HASH_F,
    sourceCommit,
    transferredAnchorSha256: hashDocument(anchor),
  };
  const current = {
    commit: sourceCommit,
    release: targetAction.expectedSourceRelease,
    settingsSha256: targetAction.expectedSettingsSha256,
  };
  const authorityInput = {
    action: targetAction,
    anchor,
    candidateReportSha256: HASH_E,
    current,
    family: "temperature",
    finalizationProof,
    nextConfirmationEligibleAt: "2027-01-17T08:03:00.000Z",
    now: "2027-01-10T08:06:00.000Z",
    registrationSha256: registration.registrationSha256,
    releaseDueKey: dueKey,
    releaseRunId: runId,
    seal,
  };
  store.failAppend = "after_journal";
  await assert.rejects(
    journal.recordQualifiedTerminalAuthority(authorityInput),
    /journal crash after append/u,
  );
  const atomicKinds = verifyJournalBytes(store.bytes).slice(-3).map(
    // prove one append retained every promotion authority component
    (record) => record.kind,
  );
  assert.deepEqual(atomicKinds, [
    "confirmation_result",
    "confirmation_action_prepared",
    "qualified_terminal_authority",
  ]);
  const staged = await journal.recordQualifiedTerminalAuthority({
    ...authorityInput,
    now: "2027-01-10T08:06:30.000Z",
  });
  assert.equal(staged.localResult.actionIdentitySha256, targetActionSha256);
  assert.deepEqual(await journal.readConfirmationActionLease({
    family: "temperature",
    now: "2027-01-10T08:06:30.000Z",
    registrationSha256: registration.registrationSha256,
  }), {
    dueKey,
    fencingToken: lease.fencingToken,
    live: true,
    runId,
  });
  assert.equal((await journal.readConfirmationActionLease({
    family: "temperature",
    now: "2027-01-10T17:00:00.000Z",
    registrationSha256: registration.registrationSha256,
  })).live, false);
  const continued = await journal.continueConfirmationActionLease({
    family: "temperature",
    now: "2027-01-10T17:00:00.000Z",
    registrationSha256: registration.registrationSha256,
  });
  assert.deepEqual(continued, {
    dueKey,
    fencingToken: lease.fencingToken,
    live: true,
    runId,
  });
  const target = {
    actionSha256: targetActionSha256,
    commitSha: "2".repeat(40),
    releaseTag: "2027.01.10-1",
  };
  await journal.prepareModelRelease({
    actionSha256: target.actionSha256,
    actionTag: `adjustment-action/temperature/${target.actionSha256}`,
    branch: "automation/adjustment-temperature-qualified-recovery",
    commitSha: target.commitSha,
    expectedSourceCommit: sourceCommit,
    family: "temperature",
    fencingToken: lease.fencingToken,
    now: "2027-01-10T17:01:00.000Z",
    packageRootSha256: HASH_A,
    releaseDueKey: dueKey,
    releaseRunId: runId,
    releaseTag: target.releaseTag,
  });
  const compensationAction = {
    ...targetAction,
    actionKind: "compensate_raw",
    candidateGraphSha256: null,
    candidateSha256: null,
    expectedSourceCommit: target.commitSha,
    expectedSourceRelease: target.releaseTag,
    predecessorActionSha256: targetActionSha256,
    reason: "invalid_incumbent",
  };
  const compensation = {
    actionSha256: hashDocument(compensationAction),
    commitSha: "3".repeat(40),
    releaseTag: "2027.01.10-2",
  };
  await journal.prepareCompensatingModelRelease({
    actionSha256: compensation.actionSha256,
    actionTag: `adjustment-action/temperature/${compensation.actionSha256}`,
    branch: "automation/adjustment-temperature-qualified-compensation",
    commitSha: compensation.commitSha,
    expectedSourceCommit: target.commitSha,
    family: "temperature",
    fencingToken: lease.fencingToken,
    now: "2027-01-10T17:02:00.000Z",
    packageRootSha256: HASH_B,
    predecessorActionSha256: targetActionSha256,
    releaseDueKey: dueKey,
    releaseRunId: runId,
    releaseTag: compensation.releaseTag,
  });
  await journal.recordModelReleasePair({
    compensationActionSha256: compensation.actionSha256,
    compensationCommitSha: compensation.commitSha,
    compensationReleaseTag: compensation.releaseTag,
    family: "temperature",
    fencingToken: lease.fencingToken,
    now: "2027-01-10T17:03:00.000Z",
    targetActionSha256,
    targetCommitSha: target.commitSha,
    targetReleaseTag: target.releaseTag,
  });
  const releaseRequest = {
    actionSha256: targetActionSha256,
    compensatingRelease: compensation.releaseTag,
    expectedCurrentRelease: targetAction.expectedSourceRelease,
    expectedSettingsSha256: targetAction.expectedSettingsSha256,
    expectedSourceRelease: targetAction.expectedSourceRelease,
    family: "temperature",
    fencingToken: lease.fencingToken,
    reportSha256: targetAction.policyReportSha256,
    targetRelease: target.releaseTag,
  };
  const transactionInput = {
    compensation,
    compensationAction,
    family: "temperature",
    finalizationProof,
    now: "2027-01-10T17:04:00.000Z",
    registrationSha256: registration.registrationSha256,
    releaseRequest,
    target,
    targetAction,
  };
  await journal.recordQualifiedModelReleaseTransaction(transactionInput);
  assert.deepEqual(await journal.readQualifiedTerminalAuthority({
    family: "temperature",
    registrationSha256: registration.registrationSha256,
  }), {
    action: targetAction,
    anchor,
    current,
    family: "temperature",
    finalizationProof,
    registrationSha256: registration.registrationSha256,
    seal,
  });
  assert.deepEqual(await journal.readQualifiedModelReleaseTransaction({
    family: "temperature",
    registrationSha256: registration.registrationSha256,
  }), {
    compensation,
    compensationAction,
    family: "temperature",
    finalizationProof,
    registrationSha256: registration.registrationSha256,
    releaseRequest,
    target,
    targetAction,
  });
  await assert.rejects(journal.recordQualifiedModelReleaseTransaction({
    ...transactionInput,
    now: "2027-01-10T17:05:00.000Z",
    targetAction: { ...targetAction, fencingToken: "2" },
  }), /identity differs|unbound/u);
  const dailyDueKey = "daily/2027-01-10";
  const dailyRunId = "daily-qualified-recovery";
  await journal.acquireLease({
    dueKey: dailyDueKey,
    inputHeadSha256: (await journal.status()).headSha256,
    now: "2027-01-10T17:06:00.000Z",
    runId: dailyRunId,
    scope: "daily",
  });
  await journal.releaseLease({
    dueKey: dailyDueKey,
    now: "2027-01-10T17:07:00.000Z",
    runId: dailyRunId,
    scope: "daily",
  });
  assert.equal((await journal.readConfirmationActionLease({
    family: "temperature",
    now: "2027-01-11T02:00:00.000Z",
    registrationSha256: registration.registrationSha256,
  })).live, false);
  const lateContinuation = await journal.continueConfirmationActionLease({
    family: "temperature",
    now: "2027-01-11T02:00:00.000Z",
    registrationSha256: registration.registrationSha256,
  });
  assert.equal(lateContinuation.fencingToken, lease.fencingToken);
  await journal.recordConfirmationActionApplied({
    actionIdentitySha256: targetActionSha256,
    applyReceiptSha256: HASH_A,
    family: "temperature",
    fencingToken: lease.fencingToken,
    now: "2027-01-11T02:01:00.000Z",
    registrationSha256: registration.registrationSha256,
  });
  await journal.recordConfirmationActionVerified({
    actionIdentitySha256: targetActionSha256,
    family: "temperature",
    fencingToken: lease.fencingToken,
    now: "2027-01-11T02:02:00.000Z",
    registrationSha256: registration.registrationSha256,
    verificationOutcome: "verified",
    verificationSha256: HASH_B,
  });
  await journal.acknowledgeConfirmationAction({
    acknowledgementSha256: HASH_C,
    actionIdentitySha256: targetActionSha256,
    family: "temperature",
    fencingToken: lease.fencingToken,
    now: "2027-01-11T02:03:00.000Z",
    outcome: "active",
    registrationSha256: registration.registrationSha256,
  });
  await journal.releaseLease({
    dueKey,
    now: "2027-01-11T02:03:30.000Z",
    runId,
    scope: "release",
  });
  const ownerOnlyRecovery = await journal.continueConfirmationActionLease({
    family: "temperature",
    now: "2027-01-11T02:04:00.000Z",
    registrationSha256: registration.registrationSha256,
  });
  assert.equal(ownerOnlyRecovery.live, false);
  const retirement = await journal.recordDueTerminalRetirement({
    dueKey: dailyDueKey,
    kind: "qualified_v3",
    now: "2027-01-11T02:05:00.000Z",
    request: { fixtureSha256: HASH_D },
  });
  await journal.recordDueTerminalOutcome({
    dueKey: dailyDueKey,
    now: "2027-01-11T02:06:00.000Z",
    outcome: {
      actionEligible: true,
      candidateGraphSha256: null,
      candidateSha256: null,
      fitReceiptSha256: null,
      reason: "daily_candidate_promoted",
      semanticInputSha256: HASH_E,
      servingChanged: true,
      state: "completed",
    },
  });
  await journal.completeDueTerminalRetirement({
    dueKey: dailyDueKey,
    now: "2027-01-11T02:07:00.000Z",
    requestSha256: retirement.requestSha256,
  });
});

test("model release mapping is durable before remote publication", async () => {
  const store = new MemoryJournalStore();
  const journal = new MaintenanceJournal(store);
  await journal.initialize();
  const lease = await journal.acquireLease({
    dueKey: `release/temperature/${HASH_A}`,
    inputHeadSha256: HASH_B,
    now: "2027-01-10T08:00:00.000Z",
    runId: "release-temperature-model",
    scope: "release",
  });
  const input = {
    actionSha256: HASH_A,
    actionTag: `adjustment-action/temperature/${HASH_A}`,
    branch: `automation/adjustment-temperature-${HASH_B.slice(0, 12)}-${HASH_A.slice(0, 12)}`,
    commitSha: "1".repeat(40),
    expectedSourceCommit: "2".repeat(40),
    family: "temperature",
    fencingToken: lease.fencingToken,
    now: "2027-01-10T08:01:00.000Z",
    packageRootSha256: HASH_C,
    releaseDueKey: `release/temperature/${HASH_A}`,
    releaseRunId: "release-temperature-model",
    releaseTag: "2027.01.10-1",
  };
  const prepared = await journal.prepareModelRelease(input);
  assert.equal(prepared.commitSha, input.commitSha);
  const count = verifyJournalBytes(store.bytes).length;
  await journal.prepareModelRelease({ ...input, now: "2027-01-10T08:02:00.000Z" });
  assert.equal(verifyJournalBytes(store.bytes).length, count);
  await assert.rejects(
    // reject changing a prepared commit under one action
    journal.prepareModelRelease({
      ...input,
      commitSha: "3".repeat(40),
      now: "2027-01-10T08:03:00.000Z",
    }),
    (error) => error.reason === "journal_record_collision",
  );
  const collision = await journal.recordModelReleaseTagCollision({
    actionSha256: HASH_A,
    attemptedReleaseTag: "2027.01.10-1",
    collisionCommitSha: "4".repeat(40),
    family: "temperature",
    fencingToken: lease.fencingToken,
    now: "2027-01-10T08:04:00.000Z",
    successorReleaseTag: "2027.01.10-2",
  });
  assert.equal(collision.status, "appended");
  const successorCollision = await journal.recordModelReleaseTagCollision({
    actionSha256: HASH_A,
    attemptedReleaseTag: "2027.01.10-2",
    collisionCommitSha: "5".repeat(40),
    family: "temperature",
    fencingToken: lease.fencingToken,
    now: "2027-01-10T08:05:00.000Z",
    successorReleaseTag: "2027.01.10-3",
  });
  assert.equal(successorCollision.status, "appended");
});

test("target and compensation releases share one fenced immutable pair", async () => {
  const store = new MemoryJournalStore();
  const journal = new MaintenanceJournal(store);
  await journal.initialize();
  const dueKey = `release/temperature/${HASH_A}`;
  const runId = "release-temperature-pair";
  const lease = await journal.acquireLease({
    dueKey,
    inputHeadSha256: HASH_B,
    now: "2027-01-10T08:00:00.000Z",
    runId,
    scope: "release",
  });
  const target = {
    actionSha256: HASH_A,
    actionTag: `adjustment-action/temperature/${HASH_A}`,
    branch: "automation/adjustment-temperature-target",
    commitSha: "1".repeat(40),
    expectedSourceCommit: "2".repeat(40),
    family: "temperature",
    fencingToken: lease.fencingToken,
    now: "2027-01-10T08:01:00.000Z",
    packageRootSha256: HASH_C,
    releaseDueKey: dueKey,
    releaseRunId: runId,
    releaseTag: "2027.01.10-1",
  };
  await journal.prepareModelRelease(target);
  const compensation = {
    actionSha256: HASH_D,
    actionTag: `adjustment-action/temperature/${HASH_D}`,
    branch: "automation/adjustment-temperature-compensation",
    commitSha: "3".repeat(40),
    expectedSourceCommit: target.commitSha,
    family: "temperature",
    fencingToken: lease.fencingToken,
    now: "2027-01-10T08:02:00.000Z",
    packageRootSha256: HASH_E,
    predecessorActionSha256: target.actionSha256,
    releaseDueKey: dueKey,
    releaseRunId: runId,
    releaseTag: "2027.01.10-2",
  };
  await journal.prepareCompensatingModelRelease(compensation);
  await journal.recordModelReleaseTagCollision({
    actionSha256: compensation.actionSha256,
    attemptedReleaseTag: compensation.releaseTag,
    collisionCommitSha: "4".repeat(40),
    family: "temperature",
    fencingToken: lease.fencingToken,
    now: "2027-01-10T08:03:00.000Z",
    successorReleaseTag: "2027.01.10-3",
  });
  const pair = {
    compensationActionSha256: compensation.actionSha256,
    compensationCommitSha: compensation.commitSha,
    compensationReleaseTag: "2027.01.10-3",
    family: "temperature",
    fencingToken: lease.fencingToken,
    now: "2027-01-10T08:04:00.000Z",
    targetActionSha256: target.actionSha256,
    targetCommitSha: target.commitSha,
    targetReleaseTag: target.releaseTag,
  };
  const recorded = await journal.recordModelReleasePair(pair);
  assert.equal(recorded.status, "appended");
  const count = verifyJournalBytes(store.bytes).length;
  await journal.prepareModelRelease({ ...target, now: "2027-01-10T08:05:00.000Z" });
  await journal.prepareCompensatingModelRelease({
    ...compensation,
    now: "2027-01-10T08:05:00.000Z",
  });
  await journal.recordModelReleasePair({ ...pair, now: "2027-01-10T08:05:00.000Z" });
  assert.equal(verifyJournalBytes(store.bytes).length, count);
  await assert.rejects(
    // reject a compensation commit that is not based on its target commit
    journal.prepareCompensatingModelRelease({
      ...compensation,
      actionSha256: HASH_F,
      actionTag: `adjustment-action/temperature/${HASH_F}`,
      branch: "automation/adjustment-temperature-invalid-compensation",
      commitSha: "5".repeat(40),
      expectedSourceCommit: "6".repeat(40),
      now: "2027-01-10T08:06:00.000Z",
      releaseTag: "2027.01.10-4",
    }),
    (error) => error.reason === "model_compensation_target_unavailable",
  );
});

test("archive due keys share the bounded durable journal grammar", async () => {
  const store = new MemoryJournalStore();
  const journal = new MaintenanceJournal(store);
  await journal.initialize();
  const acquired = await journal.acquireLease({
    dueKey: `archive/report/${HASH_A}`,
    inputHeadSha256: HASH_B,
    now: "2027-01-10T08:00:00.000Z",
    runId: `archive-report-${HASH_A}`,
    scope: "archive",
  });
  assert.equal(acquired.dueKey, `archive/report/${HASH_A}`);
});

test("failed confirmation action releases its slot only after durable compensation", async () => {
  const { journal, store } = await createLifecycleJournal();
  const { registration, registrationInput } = await createCompleteTemperatureMember(journal);
  await journal.recordConfirmationResult({
    actionIdentitySha256: HASH_E,
    candidateReportSha256: HASH_D,
    disposition: "promoted",
    family: "temperature",
    nextConfirmationEligibleAt: "2027-01-18T08:00:00.000Z",
    now: "2027-01-10T08:03:00.000Z",
    registrationSha256: registration.registrationSha256,
  });
  await journal.acquireLease({
    dueKey: `confirmation/temperature/${HASH_C}`,
    inputHeadSha256: HASH_A,
    now: "2027-01-10T08:04:00.000Z",
    runId: "release-temperature-compensation",
    scope: "release",
  });
  await journal.prepareConfirmationAction({
    actionIdentitySha256: HASH_E,
    actionManifestSha256: HASH_F,
    family: "temperature",
    fencingToken: "1",
    now: "2027-01-10T08:05:00.000Z",
    registrationSha256: registration.registrationSha256,
    releaseDueKey: `confirmation/temperature/${HASH_C}`,
    releaseRunId: "release-temperature-compensation",
  });
  await journal.recordConfirmationActionApplied({
    actionIdentitySha256: HASH_E,
    applyReceiptSha256: HASH_A,
    family: "temperature",
    fencingToken: "1",
    now: "2027-01-10T08:06:00.000Z",
    registrationSha256: registration.registrationSha256,
  });
  await journal.recordConfirmationActionVerified({
    actionIdentitySha256: HASH_E,
    family: "temperature",
    fencingToken: "1",
    now: "2027-01-10T08:07:00.000Z",
    registrationSha256: registration.registrationSha256,
    verificationOutcome: "failed",
    verificationSha256: HASH_B,
  });
  await assert.rejects(
    // prohibit acknowledgement of failed verification
    journal.acknowledgeConfirmationAction({
      acknowledgementSha256: HASH_C,
      actionIdentitySha256: HASH_E,
      family: "temperature",
      fencingToken: "1",
      now: "2027-01-10T08:08:00.000Z",
      outcome: "active",
      registrationSha256: registration.registrationSha256,
    }),
    (error) => error.reason === "confirmation_action_acknowledgement_refused",
  );
  await assert.rejects(
    // retain the family through failed deployment
    journal.preregisterConfirmation(laterTemperatureRegistration(registrationInput)),
    (error) => error.reason === "confirmation_slot_occupied",
  );
  const compensationInput = {
    actionIdentitySha256: HASH_E,
    compensationActionIdentitySha256: HASH_F,
    compensationReportSha256: HASH_C,
    family: "temperature",
    fencingToken: "1",
    now: "2027-01-10T08:09:00.000Z",
    outcome: "incumbent_restored",
    registrationSha256: registration.registrationSha256,
  };
  const compensated = await journal.recordConfirmationCompensationResult(compensationInput);
  assert.equal(compensated.actionState, "failed");
  const countAfterCompensation = verifyJournalBytes(store.bytes).length;
  await journal.recordConfirmationCompensationResult({
    ...compensationInput,
    now: "2027-01-10T08:10:00.000Z",
  });
  assert.equal(verifyJournalBytes(store.bytes).length, countAfterCompensation);
  await journal.releaseLease({
    dueKey: `confirmation/temperature/${HASH_C}`,
    now: "2027-01-10T08:11:00.000Z",
    runId: "release-temperature-compensation",
    scope: "release",
  });
  const next = await journal.preregisterConfirmation(
    laterTemperatureRegistration(registrationInput),
  );
  assert.equal(next.status, "registered");
});

test("operator-off unapplied action releases its slot only after durable terminal result", async () => {
  const { journal } = await createLifecycleJournal();
  const { registration, registrationInput } = await createCompleteTemperatureMember(journal);
  await journal.recordConfirmationResult({
    actionIdentitySha256: HASH_E,
    candidateReportSha256: HASH_D,
    disposition: "promoted",
    family: "temperature",
    nextConfirmationEligibleAt: "2027-01-18T08:00:00.000Z",
    now: "2027-01-10T08:03:00.000Z",
    registrationSha256: registration.registrationSha256,
  });
  await journal.acquireLease({
    dueKey: `confirmation/temperature/${HASH_C}`,
    inputHeadSha256: HASH_A,
    now: "2027-01-10T08:04:00.000Z",
    runId: "release-temperature-operator-off",
    scope: "release",
  });
  await journal.prepareConfirmationAction({
    actionIdentitySha256: HASH_E,
    actionManifestSha256: HASH_E,
    family: "temperature",
    fencingToken: "1",
    now: "2027-01-10T08:05:00.000Z",
    registrationSha256: registration.registrationSha256,
    releaseDueKey: `confirmation/temperature/${HASH_C}`,
    releaseRunId: "release-temperature-operator-off",
  });
  await assert.rejects(
    journal.preregisterConfirmation(laterTemperatureRegistration(registrationInput)),
    (error) => error.reason === "confirmation_slot_occupied",
  );
  const terminal = await journal.recordConfirmationCompensationResult({
    actionIdentitySha256: HASH_E,
    compensationActionIdentitySha256: HASH_F,
    compensationReportSha256: HASH_D,
    family: "temperature",
    fencingToken: "1",
    now: "2027-01-10T08:06:00.000Z",
    outcome: "unapplied",
    registrationSha256: registration.registrationSha256,
  });
  assert.equal(terminal.actionState, "expired_unapplied");
  await journal.releaseLease({
    dueKey: `confirmation/temperature/${HASH_C}`,
    now: "2027-01-10T08:07:00.000Z",
    runId: "release-temperature-operator-off",
    scope: "release",
  });
  const next = await journal.preregisterConfirmation(
    laterTemperatureRegistration(registrationInput),
  );
  assert.equal(next.status, "registered");
});

test("journal corruption fails closed and production root cannot be overridden", async () => {
  const store = new MemoryJournalStore();
  const journal = new MaintenanceJournal(store);
  await journal.initialize();
  await journal.initializeLifecycle({
    now: "2026-01-01T00:00:00.000Z",
    v1LedgerSha256: HASH_A,
    v1TailSha256: HASH_B,
  });
  const corrupted = Buffer.from(store.bytes);
  corrupted[20] ^= 1;
  assert.throws(
    // reject one corrupted append-only byte
    () => verifyJournalBytes(corrupted),
  );
  assert.throws(
    // reject an unsafe caller-selected state root
    () => createMaintenanceJournal({ root: "/tmp/state" }),
    /override is prohibited/u,
  );
});
