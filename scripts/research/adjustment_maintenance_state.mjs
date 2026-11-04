import { constants as fsConstants } from "node:fs";
import {
  lstat,
  open,
  readFile,
  realpath,
  rename,
  statfs,
  unlink,
} from "node:fs/promises";
import { createHash, randomUUID } from "node:crypto";
import { spawn } from "node:child_process";
import { homedir } from "node:os";
import { dirname, join, resolve } from "node:path";
import {
  validateAdjustmentRevisionColdCustodyAcknowledgementV2,
} from "../../deploy/scripts/adjustment-evidence-store.mjs";
import { ensureAdjustmentPrivateDirectory } from "./adjustment_private_directory.mjs";

export const ADJUSTMENT_JOURNAL_CONTRACT_VERSION =
  "forecast-adjustment-maintenance-journal/v2";
export const ADJUSTMENT_LIFECYCLE_LEDGER_CONTRACT_VERSION =
  "forecast-adjustment-lifecycle-ledger/v2";
export const ADJUSTMENT_MAINTENANCE_STATE_ROOT_KIND = "home_native_ext4_state";
export const ADJUSTMENT_DEFAULT_STATE_ROOT = join(
  homedir(),
  ".weather",
  "adjustment-maintenance",
  "v2",
  "state",
);
export const ADJUSTMENT_MAXIMUM_FENCING_TOKEN = 0xffff_ffff_ffff_ffffn;
export const ADJUSTMENT_CONFIRMATION_CHUNK_PART_V3_VERSION =
  "adjustment-confirmation-chunk-part/v3";
export const ADJUSTMENT_CONFIRMATION_LOGICAL_CHUNK_V3_VERSION =
  "adjustment-confirmation-logical-chunk/v3";

const SHA256_PATTERN = /^[a-f0-9]{64}$/u;
const INSTANT_PATTERN = /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z$/u;
const LOCAL_DATE_PATTERN = /^\d{4}-\d{2}-\d{2}$/u;
const CLOCK_ROLLBACK_TOLERANCE_MS = 5 * 60 * 1_000;
const MAXIMUM_JOURNAL_BYTES = 64 * 1_024 * 1_024;
const EXT4_MAGIC = 0xef53n;
const LEASE_MILLISECONDS = new Map([
  ["archive", 60 * 60 * 1_000],
  ["daily", 3 * 60 * 60 * 1_000],
  ["confirmation", 3 * 60 * 60 * 1_000],
  ["monthly", 6 * 60 * 60 * 1_000],
  ["release", 8 * 60 * 60 * 1_000],
]);
const FAMILY_CANDIDATE_KINDS = new Map([
  ["temperature", "temperature-delayed-mos/v1"],
  ["wind", "wind-robust-hierarchical-median/v1"],
  ["rain", "rain-hurdle-wind-occurrence-amount/v1"],
]);
const CONFIRMATION_CHUNK_COUNTS = new Map([
  ["temperature", 27],
  ["wind", 27],
  ["rain", 24],
]);
const CONFIRMATION_INTERVAL_DAYS = new Map([
  ["temperature", 366],
  ["wind", 366],
  ["rain", 334],
]);
const ACTION_VERIFICATION_OUTCOMES = new Set([
  "verified",
  "deployed_operator_off",
  "failed",
]);
const ACTION_ACKNOWLEDGEMENT_OUTCOMES = new Set([
  "active",
  "deployed_operator_off",
]);
const ACTION_COMPENSATION_STATES = new Map([
  ["unapplied", "expired_unapplied"],
  ["incumbent_restored", "failed"],
  ["raw_installed", "raw"],
]);
const MODEL_ACTION_KEYS = [
  "actionKind", "candidateGraphSha256", "candidateSha256", "contractVersion",
  "createdAt", "expectedInstalledReceiptSha256", "expectedSettingsSha256",
  "expectedSourceCommit", "expectedSourceRelease", "family", "fencingToken",
  "fullMemberRootSha256", "lifecycleHeadSha256", "policyDecision",
  "policyReportSha256", "predecessorActionSha256", "reason", "reportCreatedAt",
  "siteKey", "validThrough",
];
const FAMILY_RELEASE_REQUEST_KEYS = [
  "actionSha256", "compensatingRelease", "expectedCurrentRelease",
  "expectedSettingsSha256", "expectedSourceRelease", "family", "fencingToken",
  "reportSha256", "targetRelease",
];
const FORBIDDEN_VALUE_KEYS = /(?:^|_)(?:body|bytes|payload|row|rows|value|values)(?:$|_)/iu;
const ADJUSTMENT_ROLLING_MIGRATION_HISTORY_SHA256 =
  "6de5c8c7efaa448aeb12bf1a9debe6fe7d4d4d1003ee0e21ab619ffa624c3424";

// create the fixed-root journal or an injected test journal
export function createMaintenanceJournal(options = {}) {
  // prohibit configurable production state roots
  if (Object.hasOwn(options, "root")) {
    throw new TypeError("maintenance state root override is prohibited");
  }
  return new MaintenanceJournal(options.store ?? new FixedRootJournalStore());
}

// own append-only journal and lifecycle transitions
export class MaintenanceJournal {
  #store;

  // retain only one exclusive durable store
  constructor(store) {
    // require the journal storage port
    if (store === null || typeof store !== "object" ||
      typeof store.initialize !== "function" || typeof store.withLock !== "function") {
      throw new TypeError("maintenance journal store is invalid");
    }
    this.#store = store;
  }

  // initialize and reconcile the durable head slots
  async initialize() {
    const identity = await this.#store.initialize();

    // reject path disclosure or an unexpected root identity
    if (identity.rootKind !== ADJUSTMENT_MAINTENANCE_STATE_ROOT_KIND ||
      Object.hasOwn(identity, "path")) {
      throw new TypeError("maintenance state identity is invalid");
    }

    return await this.#store.withLock(
      // verify the full journal and repair only bounded head projections
      async (transaction) => {
        const records = verifyJournalBytes(await transaction.readJournal());
        await transaction.writeHeads(headsForRecords(records));
        return { head: records.at(-1)?.recordSha256 ?? null, records: records.length };
      },
    );
  }

  // return one path-free verified journal projection
  async status() {
    return await this.#read(
      // project bounded authority state
      (records) => {
        const activeLeases = deriveActiveLeases(records);
        const maxSeenUtc = deriveMaxSeenUtc(records);
        const fencingToken = deriveMaximumFencingToken(records);
        return {
          headSha256: records.at(-1)?.recordSha256 ?? null,
          generation: records.at(-1)?.generation ?? "0",
          maxSeenUtc,
          fencingToken: fencingToken.toString(),
          activeLeases: [...activeLeases.values()].map(
            // omit private payload details from status
            (lease) => ({
              dueKey: lease.payload.dueKey,
              expiresAt: lease.payload.expiresAt,
              runId: lease.payload.runId,
              scope: lease.payload.scope,
            }),
          ),
        };
      },
    );
  }

  // read one bounded value-blind confirmation lifecycle projection
  async readConfirmationLifecycle(input) {
    requireExactKeys(input, ["family", "registrationSha256"],
      "confirmation lifecycle read");
    requireFamily(input.family);
    requireSha256(input.registrationSha256, "registrationSha256");
    return await this.#read(
      // project only immutable metadata already admitted by the journal
      (records) => projectConfirmationLifecycle(
        records,
        findRegistration(records, input.family, input.registrationSha256),
      ),
    );
  }

  // read the sole active confirmation without trusting a database registration alias
  async readActiveConfirmation(input) {
    requireExactKeys(input, ["family"], "active confirmation read");
    requireFamily(input.family);
    return await this.#read(
      // select and project only the journal-owned active member
      (records) => projectConfirmationLifecycle(
        records,
        findActiveConfirmation(confirmationRecords(records, input.family)),
      ),
    );
  }

  // read one retained registration by its immutable candidate identity
  async readConfirmationByCandidate(input) {
    requireExactKeys(input, ["candidateSha256", "family"],
      "candidate confirmation read");
    requireFamily(input.family);
    requireSha256(input.candidateSha256, "candidateSha256");
    return await this.#read(
      // retain terminal members for exact owner-reconciliation retries
      (records) => {
        const matches = confirmationRecords(records, input.family).filter(
          // select only registrations for the requested immutable candidate
          (record) => record.kind === "confirmation_registered" &&
            record.payload.candidateSha256 === input.candidateSha256,
        );

        // reject ambiguous candidate reuse instead of choosing by append order
        if (matches.length > 1) {
          throw stateError("confirmation_candidate_ambiguous");
        }
        return projectConfirmationLifecycle(records, matches[0] ?? null);
      },
    );
  }

  // read the root-acknowledged archive identities for the active development shadow
  async readActiveDevelopmentMaterial(input) {
    requireExactKeys(input, ["family"], "active development material read");
    requireFamily(input.family);
    return await this.#read(
      // bind the installed material to the sole active local registration
      (records) => {
        const registration = findActiveConfirmation(confirmationRecords(records, input.family));

        // preserve an empty family slot without guessing a candidate graph
        if (registration === undefined) {
          return null;
        }
        const installed = records.findLast(
          // select only the matching immutable installed shadow event
          (record) => record.kind === "confirmation_development_installed" &&
            record.payload.candidateSha256 === registration.payload.candidateSha256,
        );
        return installed?.payload ?? null;
      },
    );
  }

  // read the latest owner-reconciled terminal tombstone for one family
  async readOwnerTerminalTombstone(input) {
    requireExactKeys(input, ["family"], "owner terminal tombstone read");
    requireFamily(input.family);
    return await this.#read(
      // project only a tombstone returned by the authenticated owner bridge
      (records) => records.findLast(
        (record) => record.kind === "confirmation_owner_terminal_tombstone" &&
          record.payload.family === input.family,
      )?.payload.tombstone ?? null,
    );
  }

  // read one immutable qualified release recovery transaction
  async readQualifiedModelReleaseTransaction(input) {
    requireExactKeys(input, ["family", "registrationSha256"],
      "qualified model release transaction read");
    requireFamily(input.family);
    requireSha256(input.registrationSha256, "registrationSha256");
    return await this.#read(
      // project only the exact registration-scoped recovery document
      (records) => {
        const matches = records.filter(
          // reject ambiguous recovery authority instead of choosing append order
          (record) => record.kind === "qualified_model_release_transaction" &&
            record.payload.family === input.family &&
            record.payload.registrationSha256 === input.registrationSha256,
        );

        // preserve absence before public release publication
        if (matches.length === 0) {
          return null;
        }
        if (matches.length !== 1) {
          throw stateError("qualified_model_release_transaction_ambiguous");
        }
        return structuredClone(matches[0].payload);
      },
    );
  }

  // read one immutable qualified C/T/F recovery intent
  async readQualifiedTerminalAuthority(input) {
    requireExactKeys(input, ["family", "registrationSha256"],
      "qualified terminal authority read");
    requireFamily(input.family);
    requireSha256(input.registrationSha256, "registrationSha256");
    return await this.#read(
      // select only one registration-scoped authority document
      (records) => {
        const payload = records.find(
          (record) => record.kind === "qualified_terminal_authority" &&
            record.payload.family === input.family &&
            record.payload.registrationSha256 === input.registrationSha256,
        )?.payload;
        return payload === undefined ? null : structuredClone(payload);
      },
    );
  }

  // read the exact original release lease retained by one prepared action
  async readConfirmationActionLease(input) {
    requireExactKeys(input, ["family", "now", "registrationSha256"],
      "confirmation action lease read");
    requireFamily(input.family);
    requireInstant(input.now, "now");
    requireSha256(input.registrationSha256, "registrationSha256");
    return await this.#read(
      // bind the prepared action to the currently live release lease
      (records) => {
        const prepared = findConfirmationActionKind(
          records,
          input.registrationSha256,
          "confirmation_action_prepared",
        );

        // preserve absence before action preparation
        if (prepared === undefined || prepared.payload.family !== input.family) {
          return null;
        }
        const active = deriveActiveLeases(records).get("release");
        return projectConfirmationActionLease(prepared, active, input.now);
      },
    );
  }

  // extend one expired prepared action lease without changing its fence
  async continueConfirmationActionLease(input) {
    requireExactKeys(input, ["family", "now", "registrationSha256"],
      "confirmation action lease continuation");
    requireFamily(input.family);
    requireInstant(input.now, "now");
    requireSha256(input.registrationSha256, "registrationSha256");
    return await this.#mutate(
      // continue only the exact durable action intent under its original fence
      (records) => {
        enforceTrustedClock(records, input.now, { allowExpired: true });
        const prepared = findConfirmationActionKind(
          records,
          input.registrationSha256,
          "confirmation_action_prepared",
        );
        const authority = records.find(
          (record) => record.kind === "qualified_terminal_authority" &&
            record.payload.registrationSha256 === input.registrationSha256,
        );
        const active = deriveActiveLeases(records).get("release");

        // reject naked or cross-family continuation attempts
        if (prepared === undefined || authority === undefined ||
          prepared.payload.family !== input.family ||
          authority.payload.family !== input.family ||
          prepared.payload.actionIdentitySha256 !==
            sha256(canonicalJsonBytes(authority.payload.action))) {
          throw stateError("confirmation_action_lease_continuation_refused");
        }

        // permit owner-only retry after an already-terminal lease was released
        if (active === undefined) {
          if (!isConfirmationTerminal(records, input.registrationSha256)) {
            throw stateError("confirmation_action_lease_continuation_refused");
          }
          return {
            records: [],
            result: projectConfirmationActionLease(prepared, undefined, input.now),
          };
        }

        // reject a replaced release lease instead of adopting its fence
        if (
          active.payload.scope !== "release" ||
          active.payload.dueKey !== prepared.payload.releaseDueKey ||
          active.payload.runId !== prepared.payload.releaseRunId ||
          active.payload.fencingToken !== prepared.payload.fencingToken) {
          throw stateError("confirmation_action_lease_continuation_refused");
        }

        // preserve a still-live exact lease without another journal event
        if (Date.parse(active.payload.expiresAt) > Date.parse(input.now)) {
          return {
            records: [],
            result: projectConfirmationActionLease(prepared, active, input.now),
          };
        }
        const payload = {
          acquiredAt: input.now,
          dueKey: active.payload.dueKey,
          expiresAt: new Date(
            Date.parse(input.now) + LEASE_MILLISECONDS.get("release"),
          ).toISOString(),
          fencingToken: active.payload.fencingToken,
          inputHeadSha256: active.payload.inputHeadSha256,
          predecessorLeaseRecordSha256: active.recordSha256,
          runId: active.payload.runId,
          scope: "release",
        };
        const event = {
          dueKey: payload.dueKey,
          kind: "lease_continued",
          payload,
          recordKey: `lease/release/${payload.runId}/continued/${active.recordSha256}`,
          recordedAt: input.now,
        };
        return {
          records: [event],
          result: {
            dueKey: payload.dueKey,
            fencingToken: payload.fencingToken,
            live: true,
            runId: payload.runId,
          },
        };
      },
    );
  }

  // read one immutable no-action C/T/F recovery intent
  async readNoActionTerminalAuthority(input) {
    requireExactKeys(input, ["family", "registrationSha256"],
      "no-action terminal authority read");
    requireFamily(input.family);
    requireSha256(input.registrationSha256, "registrationSha256");
    return await this.#read(
      // select only the exact registration-scoped no-action authority
      (records) => {
        const payload = records.find(
          (record) => record.kind === "no_action_terminal_authority" &&
            record.payload.family === input.family &&
            record.payload.registrationSha256 === input.registrationSha256,
        )?.payload;
        return payload === undefined ? null : structuredClone(payload);
      },
    );
  }

  // retain one owner-reconciled tombstone before database slot retirement
  async recordOwnerTerminalTombstone(input) {
    requireExactKeys(input, ["family", "now", "tombstone"],
      "owner terminal tombstone record");
    requireFamily(input.family);
    requireInstant(input.now, "now");
    validateOwnerTerminalTombstone(input.tombstone);
    return await this.#mutate(
      // append or reuse the exact immutable family reconciliation
      (records) => {
        const payload = {
          family: input.family,
          tombstone: { ...input.tombstone },
        };
        const mutation = uniqueMutation(records, {
          dueKey: `confirmation/${input.family}/${input.tombstone.registrationSha256}`,
          kind: "confirmation_owner_terminal_tombstone",
          payload,
          recordKey: `confirmation-owner-terminal/${input.family}/` +
            `${input.tombstone.registrationSha256}`,
          recordedAt: input.now,
        });
        return mutation.records.length === 0
          ? { records: [], result: mutation.result.payload.tombstone }
          : { records: mutation.records, result: payload.tombstone };
      },
    );
  }

  // inspect a bounded caller-derived due-key set
  async inspectDueKeys(input) {
    requireExactKeys(input, ["dueKeys"], "due inspection");

    // require one bounded unique due-key query
    if (!Array.isArray(input.dueKeys) || input.dueKeys.length > 4_096 ||
      new Set(input.dueKeys).size !== input.dueKeys.length) {
      throw new TypeError("dueKeys are invalid");
    }

    // validate every key before reading journal state
    for (const dueKey of input.dueKeys) {
      requireDueKey(dueKey);
    }
    return await this.#read(
      // project only immutable due registration and completion hashes
      (records) => input.dueKeys.map((dueKey) => {
        const registration = findDueRegistration(records, dueKey);
        const completion = findDueCompletion(records, dueKey);
        return {
          dueKey,
          inputHeadSha256: registration?.payload.inputHeadSha256 ?? null,
          outputSha256: completion?.payload.outputSha256 ?? null,
          status: completion === undefined
            ? registration === undefined ? "absent" : "registered"
            : "complete",
        };
      }),
    );
  }

  // read one terminal due outcome retained before owner retirement
  async readDueTerminalOutcome(input) {
    requireExactKeys(input, ["dueKey"], "due terminal outcome read");
    requireDueKey(input.dueKey);
    return await this.#read(
      // project the sole exact due-scoped terminal outcome
      (records) => {
        const payload = records.find(
          (record) => record.kind === "due_terminal_outcome" &&
            record.dueKey === input.dueKey,
        )?.payload;
        return payload === undefined ? null : structuredClone(payload.outcome);
      },
    );
  }

  // read one exact owner-retirement request retained before slot mutation
  async readDueTerminalRetirement(input) {
    requireExactKeys(input, ["dueKey"], "due terminal retirement read");
    requireDueKey(input.dueKey);
    return await this.#read(
      // return the immutable request with its durable completion state
      (records) => {
        const retained = records.find(
          (record) => record.kind === "due_terminal_retirement" &&
            record.dueKey === input.dueKey,
        );

        // preserve absence before owner reconciliation
        if (retained === undefined) {
          return null;
        }
        const completed = records.some(
          (record) => record.kind === "due_terminal_retirement_completed" &&
            record.dueKey === input.dueKey &&
            record.payload.requestSha256 === retained.payload.requestSha256,
        );
        return {
          completed,
          kind: retained.payload.kind,
          request: structuredClone(retained.payload.request),
          requestSha256: retained.payload.requestSha256,
        };
      },
    );
  }

  // read one frozen due attempt context across report-publication retries
  async readDueAttemptContext(input) {
    requireExactKeys(input, ["dueKey"], "due attempt context read");
    requireDueKey(input.dueKey);
    return await this.#read(
      // project one original input and lifecycle head pair
      (records) => {
        const payload = records.find(
          (record) => record.kind === "due_attempt_context" &&
            record.dueKey === input.dueKey,
        )?.payload;
        return payload === undefined ? null : structuredClone(payload);
      },
    );
  }

  // freeze one original due attempt context before irreversible work
  async recordDueAttemptContext(input) {
    requireExactKeys(input, [
      "dueKey", "inputHeadSha256", "lifecycleHeadSha256", "now",
    ], "due attempt context record");
    requireDueKey(input.dueKey);
    requireSha256(input.inputHeadSha256, "inputHeadSha256");
    requireSha256(input.lifecycleHeadSha256, "lifecycleHeadSha256");
    requireInstant(input.now, "now");
    return await this.#mutate(
      // retain only the first immutable attempt context for this original due
      (records) => {
        enforceTrustedClock(records, input.now);
        const due = findDueRegistration(records, input.dueKey);

        // bind the frozen attempt to the registered semantic input
        if (due?.payload.inputHeadSha256 !== input.inputHeadSha256) {
          throw stateError("due_attempt_context_unbound");
        }
        const payload = {
          inputHeadSha256: input.inputHeadSha256,
          lifecycleHeadSha256: input.lifecycleHeadSha256,
        };
        const mutation = uniqueMutation(records, {
          dueKey: input.dueKey,
          kind: "due_attempt_context",
          payload,
          recordKey: `due/${input.dueKey}/attempt-context`,
          recordedAt: input.now,
        });
        return mutation.records.length === 0
          ? { records: [], result: structuredClone(mutation.result.payload) }
          : { records: mutation.records, result: payload };
      },
    );
  }

  // retain one exact terminal attempt outcome before owner retirement
  async recordDueTerminalOutcome(input) {
    requireExactKeys(input, ["dueKey", "now", "outcome"],
      "due terminal outcome record");
    requireDueKey(input.dueKey);
    requireInstant(input.now, "now");
    validateDueTerminalOutcome(input.outcome);
    return await this.#mutate(
      // bind the outcome only to its already-registered original due
      (records) => {
        enforceTrustedClock(records, input.now);
        if (findDueRegistration(records, input.dueKey) === undefined) {
          throw stateError("due_terminal_outcome_unregistered");
        }
        const payload = { outcome: structuredClone(input.outcome) };
        const mutation = uniqueMutation(records, {
          dueKey: input.dueKey,
          kind: "due_terminal_outcome",
          payload,
          recordKey: `due/${input.dueKey}/terminal-outcome`,
          recordedAt: input.now,
        });
        return mutation.records.length === 0
          ? { records: [], result: structuredClone(mutation.result.payload.outcome) }
          : { records: mutation.records, result: structuredClone(input.outcome) };
      },
    );
  }

  // retain the exact owner-retirement command before releasing the family slot
  async recordDueTerminalRetirement(input) {
    requireExactKeys(input, ["dueKey", "kind", "now", "request"],
      "due terminal retirement record");
    requireDueKey(input.dueKey);
    requireInstant(input.now, "now");
    requirePlainObject(input.request, "due terminal retirement request");
    if (!new Set(["qualified_v3", "unsupported_v1"]).has(input.kind)) {
      throw new TypeError("due terminal retirement kind is invalid");
    }
    const requestSha256 = sha256(canonicalJsonBytes(input.request));
    return await this.#mutate(
      // bind the command to the already registered original due
      (records) => {
        enforceTrustedClock(records, input.now);
        if (findDueRegistration(records, input.dueKey) === undefined) {
          throw stateError("due_terminal_retirement_unregistered");
        }
        const payload = {
          kind: input.kind,
          request: structuredClone(input.request),
          requestSha256,
        };
        const mutation = uniqueMutation(records, {
          dueKey: input.dueKey,
          kind: "due_terminal_retirement",
          payload,
          recordKey: `due/${input.dueKey}/terminal-retirement`,
          recordedAt: input.now,
        });
        return mutation.records.length === 0
          ? { records: [], result: structuredClone(mutation.result.payload) }
          : { records: mutation.records, result: payload };
      },
    );
  }

  // acknowledge only the exact retained owner-retirement command
  async completeDueTerminalRetirement(input) {
    requireExactKeys(input, ["dueKey", "now", "requestSha256"],
      "due terminal retirement completion");
    requireDueKey(input.dueKey);
    requireInstant(input.now, "now");
    requireSha256(input.requestSha256, "requestSha256");
    return await this.#mutate(
      // close the retained command after the authenticated owner response
      (records) => {
        enforceTrustedClock(records, input.now);
        const retained = records.find(
          (record) => record.kind === "due_terminal_retirement" &&
            record.dueKey === input.dueKey,
        );

        // reject naked or changed completion identities
        if (retained?.payload.requestSha256 !== input.requestSha256) {
          throw stateError("due_terminal_retirement_unbound");
        }
        const payload = { requestSha256: input.requestSha256 };
        return uniqueMutation(records, {
          dueKey: input.dueKey,
          kind: "due_terminal_retirement_completed",
          payload,
          recordKey: `due/${input.dueKey}/terminal-retirement-completed`,
          recordedAt: input.now,
        });
      },
    );
  }

  // acquire one fixed-duration fenced lease
  async acquireLease(input) {
    requireExactKeys(input, [
      "dueKey",
      "inputHeadSha256",
      "now",
      "runId",
      "scope",
    ], "lease input");
    requireDueKey(input.dueKey);
    requireSha256(input.inputHeadSha256, "inputHeadSha256");
    requireInstant(input.now, "now");
    requireRunId(input.runId);
    const scopeClass = leaseScopeClass(input.scope);
    const durationMs = LEASE_MILLISECONDS.get(scopeClass);

    return await this.#mutate(
      // enforce due identity, clock and lease fencing under one lock
      (records) => {
        enforceTrustedClock(records, input.now);
        const activeLeases = deriveActiveLeases(records);
        const active = activeLeases.get(input.scope);
        const due = findDueRegistration(records, input.dueKey);

        // reject due-key reuse with different immutable inputs
        if (due !== undefined && due.payload.inputHeadSha256 !== input.inputHeadSha256) {
          throw stateError("due_key_collision");
        }

        // require reconciliation before expired lease reuse
        if (active !== undefined && Date.parse(active.payload.expiresAt) <= Date.parse(input.now)) {
          throw stateError("reconciliation_required");
        }

        // serialize one active lease per exact scope
        if (active !== undefined) {
          // return only an exact idempotent acquisition
          if (active.payload.runId === input.runId && active.payload.dueKey === input.dueKey &&
            active.payload.inputHeadSha256 === input.inputHeadSha256) {
            return { records: [], result: active.payload };
          }
          throw stateError("lease_occupied");
        }

        // return terminal due work without replay
        if (findDueCompletion(records, input.dueKey) !== undefined) {
          return { records: [], result: { status: "already_complete", dueKey: input.dueKey } };
        }

        const fencingToken = scopeClass === "release"
          ? allocateFencingToken(records)
          : null;
        const expiresAt = new Date(Date.parse(input.now) + durationMs).toISOString();
        const payload = {
          scope: input.scope,
          runId: input.runId,
          dueKey: input.dueKey,
          acquiredAt: input.now,
          expiresAt,
          inputHeadSha256: input.inputHeadSha256,
          fencingToken: fencingToken === null ? null : fencingToken.toString(),
        };
        const additions = [];

        // register one immutable due input before its lease
        if (due === undefined) {
          additions.push({
            dueKey: input.dueKey,
            kind: "due_registered",
            payload: { inputHeadSha256: input.inputHeadSha256 },
            recordKey: `due/${input.dueKey}`,
            recordedAt: input.now,
          });
        }
        additions.push({
          dueKey: input.dueKey,
          kind: "lease_acquired",
          payload,
          recordKey: `lease/${input.scope}/${input.runId}/acquired`,
          recordedAt: input.now,
        });
        return { records: additions, result: payload };
      },
    );
  }

  // release one exact live lease without deleting history
  async releaseLease(input) {
    requireExactKeys(input, ["dueKey", "now", "runId", "scope"], "lease release");
    requireDueKey(input.dueKey);
    requireInstant(input.now, "now");
    requireRunId(input.runId);
    leaseScopeClass(input.scope);

    return await this.#mutate(
      // bind one release to the current active lease
      (records) => {
        enforceTrustedClock(records, input.now);
        const active = deriveActiveLeases(records).get(input.scope);

        // reject release of an unknown or replaced lease
        if (active === undefined || active.payload.runId !== input.runId ||
          active.payload.dueKey !== input.dueKey) {
          throw stateError("unknown_lease");
        }

        // require expiry reconciliation instead of ordinary release
        if (Date.parse(active.payload.expiresAt) <= Date.parse(input.now)) {
          throw stateError("reconciliation_required");
        }

        // retain a release fence until its action reconciles
        if (input.scope === "release" && hasUnreconciledPreparedAction(records, active)) {
          throw stateError("confirmation_action_reconciliation_required");
        }
        return {
          records: [{
            dueKey: input.dueKey,
            kind: "lease_released",
            payload: {
              scope: input.scope,
              runId: input.runId,
              dueKey: input.dueKey,
              releasedAt: input.now,
            },
            recordKey: `lease/${input.scope}/${input.runId}/released`,
            recordedAt: input.now,
          }],
          result: { status: "released" },
        };
      },
    );
  }

  // reconcile one expired lease against immutable external evidence
  async reconcileExpiredLease(input) {
    requireExactKeys(input, [
      "dueKey",
      "immutableOutputSha256",
      "now",
      "remoteStateSha256",
      "resolution",
      "runId",
      "scope",
    ], "lease reconciliation");
    requireDueKey(input.dueKey);
    requireInstant(input.now, "now");
    requireRunId(input.runId);
    leaseScopeClass(input.scope);
    requireNullableSha256(input.immutableOutputSha256, "immutableOutputSha256");
    requireNullableSha256(input.remoteStateSha256, "remoteStateSha256");

    // require one closed reconciliation outcome
    if (!new Set(["blocked", "complete", "resume"]).has(input.resolution)) {
      throw new TypeError("reconciliation resolution is invalid");
    }

    return await this.#mutate(
      // resolve only the exact known expired lease
      (records) => {
        enforceTrustedClock(records, input.now, { allowExpired: true });
        const active = deriveActiveLeases(records).get(input.scope);

        // require one exact expired lease
        if (active === undefined || active.payload.runId !== input.runId ||
          active.payload.dueKey !== input.dueKey ||
          Date.parse(active.payload.expiresAt) > Date.parse(input.now)) {
          throw stateError("expired_lease_not_found");
        }
        // retain an expired release fence until its action reconciles
        if (input.scope === "release" && hasUnreconciledPreparedAction(records, active)) {
          throw stateError("confirmation_action_reconciliation_required");
        }
        return {
          records: [{
            dueKey: input.dueKey,
            kind: "lease_reconciled",
            payload: {
              scope: input.scope,
              runId: input.runId,
              dueKey: input.dueKey,
              reconciledAt: input.now,
              resolution: input.resolution,
              immutableOutputSha256: input.immutableOutputSha256,
              remoteStateSha256: input.remoteStateSha256,
            },
            recordKey: `lease/${input.scope}/${input.runId}/reconciled`,
            recordedAt: input.now,
          }],
          result: { status: "reconciled", resolution: input.resolution },
        };
      },
    );
  }

  // mark one exact due key terminal
  async completeDue(input) {
    requireExactKeys(input, ["dueKey", "now", "outputSha256"], "due completion");
    requireDueKey(input.dueKey);
    requireInstant(input.now, "now");
    requireSha256(input.outputSha256, "outputSha256");

    return await this.#mutate(
      // append one immutable due completion
      (records) => {
        const existing = findDueCompletion(records, input.dueKey);

        // prohibit terminal output without immutable due registration
        if (findDueRegistration(records, input.dueKey) === undefined) {
          throw stateError("due_key_unregistered");
        }

        // reuse only one exact completion
        if (existing !== undefined) {
          // reject changing one terminal due output
          if (existing.payload.outputSha256 !== input.outputSha256) {
            throw stateError("due_completion_collision");
          }
          return { records: [], result: { status: "already_complete" } };
        }
        return {
          records: [{
            dueKey: input.dueKey,
            kind: "due_completed",
            payload: { outputSha256: input.outputSha256 },
            recordKey: `due/${input.dueKey}/complete`,
            recordedAt: input.now,
          }],
          result: { status: "complete" },
        };
      },
    );
  }

  // bind immutable v1 history into one ledger-v2 genesis
  async initializeLifecycle(input) {
    requireExactKeys(input, ["now", "v1LedgerSha256", "v1TailSha256"], "lifecycle genesis");
    requireInstant(input.now, "now");
    requireSha256(input.v1LedgerSha256, "v1LedgerSha256");
    requireSha256(input.v1TailSha256, "v1TailSha256");

    return await this.#appendUnique({
      dueKey: null,
      kind: "lifecycle_genesis",
      payload: {
        contractVersion: ADJUSTMENT_LIFECYCLE_LEDGER_CONTRACT_VERSION,
        v1LedgerSha256: input.v1LedgerSha256,
        v1TailSha256: input.v1TailSha256,
      },
      recordKey: "lifecycle/genesis",
      recordedAt: input.now,
    });
  }

  // bind a root-authenticated future-only capture epoch without legacy fiction
  async initializeFutureOnlyLifecycle(input) {
    requireExactKeys(input, ["now", "witness"], "future-only lifecycle genesis");
    requireInstant(input.now, "now");
    const witness = validateAdjustmentFutureOnlyEpochWitness(input.witness);

    // never record the local journal event before the authenticated server epoch
    if (Date.parse(input.now) < Date.parse(witness.epochAt)) {
      throw new TypeError("future-only lifecycle genesis predates its epoch");
    }
    return await this.#appendUnique({
      dueKey: null,
      kind: "lifecycle_genesis",
      payload: {
        contractVersion: ADJUSTMENT_LIFECYCLE_LEDGER_CONTRACT_VERSION,
        legacyHistoryStatus: "unavailable",
        sourceKind: "future_only_verified_capture_epoch",
        witness,
      },
      recordKey: "lifecycle/genesis",
      recordedAt: input.now,
    });
  }

  // read the latest server-frontier cursor retained after durable graph publication
  async readRevisionCursor() {
    return await this.#read(
      // derive the cursor only from the authenticated epoch and monotonic advances
      (records) => deriveRevisionCursor(records),
    );
  }

  // list bounded page-to-graph mappings needed to restore cold semantic members
  async listRevisionCatalogPages() {
    return await this.#read(
      // project only immutable content identities, never payload bytes
      (records) => [
        ...records.filter((record) => record.kind === "revision_cursor_advanced" &&
          Object.hasOwn(record.payload, "graphManifestSha256")).map((record) => ({
          graphManifestSha256: record.payload.graphManifestSha256,
          pageSha256: record.payload.pageSha256,
          startSha256: record.payload.startSha256,
        })),
        ...records.filter((record) => record.kind === "revision_custody_pack_sealed")
          .flatMap((record) => record.payload.pages.map((page) => ({
            graphManifestSha256: record.payload.graphManifestSha256,
            pageSha256: page.pageSha256,
            startSha256: page.startSha256,
          }))),
      ],
    );
  }

  // record one hash-bound custody request before advancing its local cursor
  async recordRevisionCustodyIntent(input) {
    requireExactKeys(input, [
      "nextArchiveCommitOrdinal", "nextFrontierSha256", "now", "request",
    ], "revision custody intent");
    const request = validateRevisionCustodyRequest(input.request);
    requireArchiveOrdinal(input.nextArchiveCommitOrdinal, "nextArchiveCommitOrdinal");
    requireSha256(input.nextFrontierSha256, "nextFrontierSha256");
    requireInstant(input.now, "now");

    // require one strict page-bounded frontier transition
    if (BigInt(input.nextArchiveCommitOrdinal) <= BigInt(request.afterArchiveCommitOrdinal) ||
      BigInt(input.nextArchiveCommitOrdinal) > BigInt(request.watermarkArchiveCommitOrdinal)) {
      throw new TypeError("revision custody intent ordinal is invalid");
    }
    return await this.#mutate(
      // bind intent to the exact locally completed graph publication
      (records) => {
        enforceTrustedClock(records, input.now);
        // retain v1 compatibility only for an already immutable page graph
        if (Object.hasOwn(request, "graphManifestSha256")) {
          const dueKey = `archive/revision-cold-page/${request.pageSha256}`;
          const publication = findDueCompletion(records, dueKey);

          // refuse a naked v1 page or incomplete graph publication
          if (publication?.payload.outputSha256 !== request.graphManifestSha256) {
            throw stateError("revision_custody_graph_unavailable");
          }
        }
        const expectedPayload = {
          nextArchiveCommitOrdinal: input.nextArchiveCommitOrdinal,
          nextFrontierSha256: input.nextFrontierSha256,
          request,
        };
        const existing = records.find(
          // reuse only the exact page-bound pending or completed intent
          (record) => record.kind === "revision_custody_intent" &&
            record.payload.request.pageSha256 === request.pageSha256,
        );

        // reject page identity reuse with different custody roots
        if (existing !== undefined) {
          if (!canonicalJsonBytes(existing.payload).equals(canonicalJsonBytes(expectedPayload))) {
            throw stateError("revision_custody_intent_collision");
          }
          return { records: [], result: projectRevisionCustodyIntent(existing.payload) };
        }
        const pending = derivePendingRevisionCustody(records);
        const cursor = deriveRevisionCursor(records);

        // serialize custody and bind it to the retained predecessor cursor
        if (pending !== null || cursor === null ||
          cursor.archiveCommitOrdinal !== request.afterArchiveCommitOrdinal ||
          cursor.frontierSha256 !== request.afterFrontierSha256) {
          throw stateError("revision_custody_intent_compare_and_swap_failed");
        }
        return {
          records: [{
            dueKey: null,
            kind: "revision_custody_intent",
            payload: expectedPayload,
            recordKey: `revision/custody/intent/${request.pageSha256}`,
            recordedAt: input.now,
          }],
          result: projectRevisionCustodyIntent(expectedPayload),
        };
      },
    );
  }

  // read the sole unfinished successful-page custody request
  async readPendingRevisionCustody() {
    return await this.#read(
      // expose only the exact remote request and intended successor cursor
      (records) => {
        const pending = derivePendingRevisionCustody(records);
        return pending === null ? null : projectRevisionCustodyIntent(pending.payload);
      },
    );
  }

  // close one custody intent only after the exact remote acknowledgement validates
  async completeRevisionCustody(input) {
    requireExactKeys(input, [
      "acknowledgementSha256", "now", "pageSha256",
    ], "revision custody completion");
    requireSha256(input.acknowledgementSha256, "acknowledgementSha256");
    requireSha256(input.pageSha256, "pageSha256");
    requireInstant(input.now, "now");
    return await this.#mutate(
      // bind acknowledgement to the intended page after durable cursor advance
      (records) => {
        enforceTrustedClock(records, input.now);
        const expectedPayload = {
          acknowledgementSha256: input.acknowledgementSha256,
          pageSha256: input.pageSha256,
        };
        const completed = records.find(
          // reuse one exact durable acknowledgement
          (record) => record.kind === "revision_custody_completed" &&
            record.payload.pageSha256 === input.pageSha256,
        );

        // reject acknowledgement identity reuse
        if (completed !== undefined) {
          if (!canonicalJsonBytes(completed.payload).equals(canonicalJsonBytes(expectedPayload))) {
            throw stateError("revision_custody_completion_collision");
          }
          return { records: [], result: { status: "completed" } };
        }
        const pending = derivePendingRevisionCustody(records);
        const cursor = deriveRevisionCursor(records);

        // complete only the exact pending page after its successor cursor is durable
        if (pending === null || pending.payload.request.pageSha256 !== input.pageSha256 ||
          cursor === null || cursor.archiveCommitOrdinal !==
            pending.payload.nextArchiveCommitOrdinal ||
          cursor.frontierSha256 !== pending.payload.nextFrontierSha256) {
          throw stateError("revision_custody_completion_unavailable");
        }
        return {
          records: [{
            dueKey: null,
            kind: "revision_custody_completed",
            payload: expectedPayload,
            recordKey: `revision/custody/completed/${input.pageSha256}`,
            recordedAt: input.now,
          }],
          result: { status: "completed" },
        };
      },
    );
  }

  // close one packed custody intent while retaining its complete server proof
  async completeRevisionCustodyV2(input) {
    requireExactKeys(input, ["acknowledgement", "now"],
      "revision custody v2 completion");
    const acknowledgement = validateAdjustmentRevisionColdCustodyAcknowledgementV2(
      input.acknowledgement,
    );
    requireInstant(input.now, "now");
    return await this.#mutate(
      // atomically bind the full acknowledgement to its pending page and cursor
      (records) => {
        enforceTrustedClock(records, input.now);
        const expectedPayload = { acknowledgement };
        const completed = records.find(
          // converge only the exact already-retained v2 acknowledgement
          (record) => record.kind === "revision_custody_completed_v2" &&
            record.payload.acknowledgement.pageSha256 === acknowledgement.pageSha256,
        );

        // reject another acknowledgement for the same immutable page
        if (completed !== undefined) {
          if (!canonicalJsonBytes(completed.payload).equals(canonicalJsonBytes(expectedPayload))) {
            throw stateError("revision_custody_completion_collision");
          }
          return { records: [], result: { status: "completed" } };
        }
        const pending = derivePendingRevisionCustody(records);
        const cursor = deriveRevisionCursor(records);
        const expectedBindings = pending === null ? null : {
          ...pending.payload.request,
          nextArchiveCommitOrdinal: pending.payload.nextArchiveCommitOrdinal,
          nextFrontierSha256: pending.payload.nextFrontierSha256,
        };

        // complete only the exact packed request after its durable successor cursor
        if (pending === null || cursor === null ||
          cursor.archiveCommitOrdinal !== pending.payload.nextArchiveCommitOrdinal ||
          cursor.frontierSha256 !== pending.payload.nextFrontierSha256 ||
          Object.entries(expectedBindings).some(([name, value]) =>
            acknowledgement[name] !== value)) {
          throw stateError("revision_custody_completion_unavailable");
        }
        return {
          records: [{
            dueKey: null,
            kind: "revision_custody_completed_v2",
            payload: expectedPayload,
            recordKey: `revision/custody/completed-v2/${acknowledgement.pageSha256}`,
            recordedAt: input.now,
          }],
          result: { status: "completed" },
        };
      },
    );
  }

  // read the latest complete packed custody authority for development anchoring
  async readLatestRevisionCustodyAcknowledgement() {
    return await this.#read(
      // expose one exact immutable server document without journal metadata
      (records) => records.findLast(
        (record) => record.kind === "revision_custody_completed_v2",
      )?.payload.acknowledgement ?? null,
    );
  }

  // bind every checkpointed page to one later immutable packed graph
  async recordRevisionCustodyPackSeal(input) {
    requireExactKeys(input, [
      "checkpointSha256", "graphManifestSha256", "now", "pages",
    ], "revision custody pack seal");
    requireSha256(input.checkpointSha256, "checkpointSha256");
    requireSha256(input.graphManifestSha256, "graphManifestSha256");
    requireInstant(input.now, "now");

    // retain only one bounded ordered open-pack population
    if (!Array.isArray(input.pages) || input.pages.length < 1 || input.pages.length > 19) {
      throw new TypeError("revision custody pack pages are invalid");
    }
    const pages = input.pages.map(
      // close every page checkpoint before journal mutation
      (page) => validateRevisionCustodyPackPage(page),
    );
    return await this.#mutate(
      // prove completed custody and durable graph publication under one lock
      (records) => {
        enforceTrustedClock(records, input.now);
        const expectedPayload = {
          checkpointSha256: input.checkpointSha256,
          graphManifestSha256: input.graphManifestSha256,
          pages,
        };
        const existing = records.find(
          // converge one exact immutable pack seal
          (record) => record.kind === "revision_custody_pack_sealed" &&
            record.payload.checkpointSha256 === input.checkpointSha256,
        );

        // reject checkpoint identity reuse with another graph
        if (existing !== undefined) {
          if (!canonicalJsonBytes(existing.payload).equals(canonicalJsonBytes(expectedPayload))) {
            throw stateError("revision_custody_pack_seal_collision");
          }
          return { records: [], result: { status: "sealed" } };
        }
        const publication = findDueCompletion(
          records,
          `archive/revision-custody-pack/${input.checkpointSha256}`,
        );

        // require the one verified archive graph before mapping any page
        if (publication?.payload.outputSha256 !== input.graphManifestSha256) {
          throw stateError("revision_custody_pack_graph_unavailable");
        }
        const seen = new Set();

        // bind every page to its completed custody roots in append order
        for (const page of pages) {
          if (seen.has(page.pageSha256) || records.some(
            // prohibit mapping a page into multiple immutable packs
            (record) => record.kind === "revision_custody_pack_sealed" &&
              record.payload.pages.some((prior) => prior.pageSha256 === page.pageSha256),
          )) {
            throw stateError("revision_custody_pack_page_reused");
          }
          seen.add(page.pageSha256);
          const intent = records.find(
            // locate the exact earlier private-checkpoint custody intent
            (record) => record.kind === "revision_custody_intent" &&
              record.payload.request.pageSha256 === page.pageSha256,
          );
          const completed = records.find(
            // require its matching server acknowledgement
            (record) => revisionCustodyCompletionPageSha256(record) === page.pageSha256,
          );

          // preserve every start and member root from the private checkpoint
          if (intent === undefined || completed === undefined ||
            !Object.hasOwn(intent.payload.request, "custodyCheckpointSha256") ||
            intent.payload.request.memberRootSha256 !== page.memberRootSha256 ||
            intent.payload.request.startMemberSha256 !== page.startMemberSha256 ||
            intent.payload.request.startSha256 !== page.startSha256) {
            throw stateError("revision_custody_pack_page_unavailable");
          }
        }
        const last = pages.at(-1);
        const lastIntent = records.find(
          // bind the final pack hash to its most recent acknowledged page
          (record) => record.kind === "revision_custody_intent" &&
            record.payload.request.pageSha256 === last.pageSha256,
        );

        // earlier checkpoint hashes are prefixes, while the last is the sealed pack
        if (lastIntent.payload.request.custodyCheckpointSha256 !== input.checkpointSha256) {
          throw stateError("revision_custody_pack_checkpoint_differs");
        }
        return {
          records: [{
            dueKey: null,
            kind: "revision_custody_pack_sealed",
            payload: expectedPayload,
            recordKey: `revision/custody/pack/${input.checkpointSha256}`,
            recordedAt: input.now,
          }],
          result: { status: "sealed" },
        };
      },
    );
  }

  // advance one exact cold frontier only after its page graph is durable
  async advanceRevisionCursor(input) {
    const proofKey = Object.hasOwn(input, "custodyCheckpointSha256")
      ? "custodyCheckpointSha256" : "graphManifestSha256";
    requireExactKeys(input, [
      "afterArchiveCommitOrdinal", "afterFrontierSha256", proofKey,
      "nextArchiveCommitOrdinal", "nextFrontierSha256", "now", "pageSha256",
      "startSha256",
    ], "revision cursor advance");
    for (const name of ["afterArchiveCommitOrdinal", "nextArchiveCommitOrdinal"]) {
      requireArchiveOrdinal(input[name], name);
    }
    for (const name of [
      "afterFrontierSha256", proofKey, "nextFrontierSha256", "pageSha256",
      "startSha256",
    ]) {
      requireSha256(input[name], name);
    }
    requireInstant(input.now, "now");
    return await this.#mutate(
      // compare and swap the retained frontier under the journal lock
      (records) => {
        enforceTrustedClock(records, input.now);
        const current = deriveRevisionCursor(records);
        const pending = derivePendingRevisionCustody(records);
        const expectedPayload = {
          afterArchiveCommitOrdinal: input.afterArchiveCommitOrdinal,
          afterFrontierSha256: input.afterFrontierSha256,
          [proofKey]: input[proofKey],
          nextArchiveCommitOrdinal: input.nextArchiveCommitOrdinal,
          nextFrontierSha256: input.nextFrontierSha256,
          pageSha256: input.pageSha256,
          startSha256: input.startSha256,
        };
        const existing = records.find(
          // reuse only the exact page-derived cursor transition
          (record) => record.kind === "revision_cursor_advanced" &&
            record.payload.pageSha256 === input.pageSha256,
        );

        // reject page identity reuse with a different frontier transition
        if (existing !== undefined) {
          if (!canonicalJsonBytes(existing.payload).equals(canonicalJsonBytes(expectedPayload))) {
            throw stateError("revision_cursor_collision");
          }
          return { records: [], result: {
            archiveCommitOrdinal: existing.payload.nextArchiveCommitOrdinal,
            frontierSha256: existing.payload.nextFrontierSha256,
          } };
        }

        // require the exact pending custody intent and retained predecessor cursor
        if (pending === null || pending.payload.request.pageSha256 !== input.pageSha256 ||
          pending.payload.request[proofKey] !== input[proofKey] ||
          pending.payload.request.startSha256 !== input.startSha256 ||
          pending.payload.nextArchiveCommitOrdinal !== input.nextArchiveCommitOrdinal ||
          pending.payload.nextFrontierSha256 !== input.nextFrontierSha256 ||
          current === null || current.archiveCommitOrdinal !==
          input.afterArchiveCommitOrdinal || current.frontierSha256 !==
          input.afterFrontierSha256 || BigInt(input.nextArchiveCommitOrdinal) <=
          BigInt(input.afterArchiveCommitOrdinal)) {
          throw stateError("revision_cursor_compare_and_swap_failed");
        }
        return {
          records: [{
            dueKey: null,
            kind: "revision_cursor_advanced",
            payload: expectedPayload,
            recordKey: `revision/cursor/${input.nextArchiveCommitOrdinal}/${input.nextFrontierSha256}`,
            recordedAt: input.now,
          }],
          result: {
            archiveCommitOrdinal: input.nextArchiveCommitOrdinal,
            frontierSha256: input.nextFrontierSha256,
          },
        };
      },
    );
  }

  // preregister one blinded action-bearing confirmation member
  async preregisterConfirmation(input) {
    requireExactKeys(input, [
      "candidateKind",
      "candidateSha256",
      "cohortLineageSha256",
      "family",
      "firstTargetAt",
      "gateManifestSha256",
      "inputHeadSha256",
      "intervalEndExclusiveLocalDate",
      "intervalStartLocalDate",
      "now",
      "reservedKeySha256",
      "sourceLineageSha256",
      "terminalAccessAt",
    ], "confirmation registration");
    validateConfirmationRegistrationInput(input);

    return await this.#mutate(
      // enforce genesis, embargo and one active member per family
      (records) => {
        requireLifecycleGenesis(records);
        const familyRecords = confirmationRecords(records, input.family);
        const active = findActiveConfirmation(familyRecords);
        const registrationPayload = {
          contractVersion: ADJUSTMENT_LIFECYCLE_LEDGER_CONTRACT_VERSION,
          family: input.family,
          candidateKind: input.candidateKind,
          candidateSha256: input.candidateSha256,
          cohortLineageSha256: input.cohortLineageSha256,
          sourceLineageSha256: input.sourceLineageSha256,
          reservedKeySha256: input.reservedKeySha256,
          intervalStartLocalDate: input.intervalStartLocalDate,
          intervalEndExclusiveLocalDate: input.intervalEndExclusiveLocalDate,
          firstTargetAt: input.firstTargetAt,
          terminalAccessAt: input.terminalAccessAt,
          gateManifestSha256: input.gateManifestSha256,
          inputHeadSha256: input.inputHeadSha256,
        };
        const registrationSha256 = sha256(canonicalJsonBytes(registrationPayload));

        // make an exact active preregistration retry idempotent
        if (active !== null) {
          if (active.payload.registrationSha256 === registrationSha256) {
            return { records: [], result: {
              registrationSha256,
              status: "already_registered",
            } };
          }
          throw stateError("confirmation_slot_occupied");
        }

        const embargo = findLatestKind(familyRecords, "confirmation_result");

        // enforce the full post-disposition embargo
        if (embargo !== undefined &&
          Date.parse(input.now) < Date.parse(embargo.payload.nextConfirmationEligibleAt)) {
          throw stateError("confirmation_embargo_active");
        }

        const priorRegistrations = records.filter(
          // select every retained confirmation registration
          (record) => record.kind === "confirmation_registered",
        );
        const reused = priorRegistrations.some(
          // reject reserved-key reuse and overlapping same-lineage members
          (record) => record.payload.reservedKeySha256 === input.reservedKeySha256 ||
            (record.payload.sourceLineageSha256 === input.sourceLineageSha256 &&
              localDateIntervalsOverlap(
                record.payload.intervalStartLocalDate,
                record.payload.intervalEndExclusiveLocalDate,
                input.intervalStartLocalDate,
                input.intervalEndExclusiveLocalDate,
              )),
        );

        // prohibit reused or overlapping designated members
        if (reused) {
          throw stateError("confirmation_member_reused");
        }

        const lifecyclePayload = {
          ...registrationPayload,
          registrationSha256,
          candidateReportSha256: null,
          actionIdentitySha256: null,
          accessState: "registered",
          actionState: "none",
        };
        return {
          records: [{
            dueKey: `confirmation/${input.family}/${input.candidateSha256}`,
            kind: "confirmation_registered",
            payload: lifecyclePayload,
            recordKey: `confirmation/${input.family}/${registrationSha256}/registered`,
            recordedAt: input.now,
          }],
          result: { registrationSha256, status: "registered" },
        };
      },
    );
  }

  // retain one acknowledged inactive shadow archive binding for later cold replay
  async recordDevelopmentCandidateInstalled(input) {
    requireExactKeys(input, [
      "actionSha256", "artifactSha256", "candidateGraphSha256", "candidateSha256",
      "family", "now", "registrationSha256", "shadowRegistrationSha256",
    ], "development candidate installation");
    requireFamily(input.family);
    requireInstant(input.now, "now");
    for (const field of [
      "actionSha256", "artifactSha256", "candidateGraphSha256", "candidateSha256",
      "registrationSha256", "shadowRegistrationSha256",
    ]) {
      requireSha256(input[field], field);
    }
    return await this.#mutate(
      // append only after the matching local preregistration exists
      (records) => {
        enforceTrustedClock(records, input.now);
        const registration = findRegistration(
          records,
          input.family,
          input.registrationSha256,
        );

        // prohibit relabelling an acknowledged release onto another candidate
        if (registration === undefined || registration.payload.candidateSha256 !==
            input.candidateSha256) {
          throw stateError("confirmation_registration_unknown");
        }
        const payload = {
          actionSha256: input.actionSha256,
          artifactSha256: input.artifactSha256,
          candidateGraphSha256: input.candidateGraphSha256,
          candidateSha256: input.candidateSha256,
          family: input.family,
          registrationSha256: input.registrationSha256,
          shadowRegistrationSha256: input.shadowRegistrationSha256,
        };
        const existing = records.find(
          // locate an exact acknowledged development retry
          (record) => record.kind === "confirmation_development_installed" &&
            record.payload.candidateSha256 === input.candidateSha256,
        );

        // reuse only byte-identical installed material
        if (existing !== undefined) {
          if (!canonicalJsonBytes(existing.payload).equals(canonicalJsonBytes(payload))) {
            throw stateError("confirmation_development_installation_collision");
          }
          return { records: [], result: existing.payload };
        }
        return {
          records: [{
            dueKey: registration.dueKey,
            kind: "confirmation_development_installed",
            payload,
            recordKey: `confirmation/${input.family}/${input.registrationSha256}/development-installed`,
            recordedAt: input.now,
          }],
          result: payload,
        };
      },
    );
  }

  // record one value-blind immutable comparator snapshot
  async recordRevisionSnapshot(input) {
    requireExactKeys(input, [
      "entryCount",
      "expectedKeySetSha256",
      "family",
      "now",
      "registrationSha256",
      "revisionCatalogWatermarkSha256",
      "snapshotRootSha256",
      "targetCutoffAt",
    ], "revision snapshot");
    requireFamily(input.family);
    requireInstant(input.now, "now");
    requireInstant(input.targetCutoffAt, "targetCutoffAt");
    requireSha256(input.registrationSha256, "registrationSha256");
    requireSha256(input.expectedKeySetSha256, "expectedKeySetSha256");
    requireSha256(input.revisionCatalogWatermarkSha256, "revisionCatalogWatermarkSha256");
    requireSha256(input.snapshotRootSha256, "snapshotRootSha256");

    // require one bounded value-free key count
    if (!Number.isSafeInteger(input.entryCount) || input.entryCount < 0 ||
      input.entryCount > 9_500_000) {
      throw new TypeError("snapshot entryCount is invalid");
    }
    assertValueBlind(input);
    return await this.#mutate(
      // append one value-blind lifecycle snapshot record
      (records) => {
        const registration = findRegistration(records, input.family, input.registrationSha256);

        // reject unknown or cross-family registrations
        if (registration === undefined) {
          throw stateError("confirmation_registration_unknown");
        }
        return uniqueMutation(records, {
          dueKey: registration.dueKey,
          kind: "confirmation_revision_snapshot",
          payload: {
            ...lifecycleContext(registration.payload, "snapshot_frozen", "none"),
            entryCount: input.entryCount,
            expectedKeySetSha256: input.expectedKeySetSha256,
            revisionCatalogWatermarkSha256: input.revisionCatalogWatermarkSha256,
            snapshotRootSha256: input.snapshotRootSha256,
            targetCutoffAt: input.targetCutoffAt,
          },
          recordKey: `confirmation/${input.family}/${input.registrationSha256}/confirmation_revision_snapshot`,
          recordedAt: input.now,
        });
      },
    );
  }

  // burn one registered member after terminal access
  async burnConfirmation(input) {
    requireExactKeys(input, ["family", "now", "registrationSha256"], "confirmation burn");
    requireFamily(input.family);
    requireInstant(input.now, "now");
    requireSha256(input.registrationSha256, "registrationSha256");

    return await this.#mutate(
      // bind access to the exact preregistration and frozen snapshot
      (records) => {
        const registration = findRegistration(records, input.family, input.registrationSha256);
        const snapshot = findConfirmationKind(
          records,
          input.registrationSha256,
          "confirmation_revision_snapshot",
        );

        // require exact registration and snapshot before access
        if (registration === undefined || snapshot === undefined) {
          throw stateError("confirmation_snapshot_unavailable");
        }

        // permanently refuse premature designated-member access
        if (Date.parse(input.now) < Date.parse(registration.payload.terminalAccessAt)) {
          throw stateError("confirmation_access_premature");
        }

        const existing = findConfirmationKind(
          records,
          input.registrationSha256,
          "confirmation_burned",
        );

        // preserve one immutable burn identity
        if (existing !== undefined) {
          return { records: [], result: existing.payload };
        }

        const payload = {
          ...lifecycleContext(registration.payload, "burned", "none"),
          targetComparatorSnapshotRootSha256: snapshot.payload.snapshotRootSha256,
          revisionCatalogWatermarkSha256: snapshot.payload.revisionCatalogWatermarkSha256,
          expectedKeySetSha256: snapshot.payload.expectedKeySetSha256,
          targetCutoffAt: snapshot.payload.targetCutoffAt,
          accessedAt: input.now,
        };
        const accessSha256 = sha256(canonicalJsonBytes(payload));
        return {
          records: [{
            dueKey: registration.dueKey,
            kind: "confirmation_burned",
            payload: { ...payload, accessSha256 },
            recordKey: `confirmation/${input.family}/${input.registrationSha256}/burned`,
            recordedAt: input.now,
          }],
          result: { ...payload, accessSha256 },
        };
      },
    );
  }

  // append one deterministic immutable confirmation chunk
  async appendConfirmationChunk(input) {
    requireExactKeys(input, ["chunk", "family", "now", "registrationSha256"], "confirmation chunk input");
    requireFamily(input.family);
    requireInstant(input.now, "now");
    requireSha256(input.registrationSha256, "registrationSha256");
    const normalizedChunk = validateConfirmationChunk(input.chunk, input.family);
    assertValueBlind(normalizedChunk);

    return await this.#mutate(
      // append only the first absent ordered chunk
      (records) => {
        const registration = findRegistration(records, input.family, input.registrationSha256);
        const burn = findConfirmationKind(records, input.registrationSha256, "confirmation_burned");

        // require durable burn before chunk assembly
        if (registration === undefined || burn === undefined) {
          throw stateError("confirmation_not_burned");
        }

        const chunks = confirmationChunks(records, input.registrationSha256);
        const existing = chunks.find(
          // locate an exact chunk retry
          (record) => confirmationChunkKey(record.payload.chunk) ===
            confirmationChunkKey(normalizedChunk),
        );

        // reuse only byte-identical chunk metadata
        if (existing !== undefined) {
          // reject changing one immutable chunk
          if (!canonicalJsonBytes(existing.payload.chunk).equals(canonicalJsonBytes(normalizedChunk))) {
            throw stateError("confirmation_chunk_collision");
          }
          return { records: [], result: { status: "already_present" } };
        }

        // require deterministic first-absent resume order across logical parts
        if (!confirmationChunkFollows(chunks.at(-1)?.payload.chunk ?? null, normalizedChunk)) {
          throw stateError("confirmation_chunk_order_refused");
        }
        return {
          records: [{
            dueKey: registration.dueKey,
            kind: "confirmation_chunk",
            payload: {
              ...lifecycleContext(registration.payload, "burned", "none"),
              chunk: normalizedChunk,
            },
            recordKey: `confirmation/${input.family}/${input.registrationSha256}/chunk/` +
              confirmationChunkKey(normalizedChunk),
            recordedAt: input.now,
          }],
          result: {
            ...(Object.hasOwn(normalizedChunk, "chunkIndex")
              ? { chunkIndex: normalizedChunk.chunkIndex }
              : { chunkKey: confirmationChunkKey(normalizedChunk) }),
            status: "appended",
          },
        };
      },
    );
  }

  // finalize one complete 27/27/24 member manifest
  async finalizeConfirmationMember(input) {
    requireExactKeys(input, ["family", "now", "registrationSha256"], "confirmation finalization");
    requireFamily(input.family);
    requireInstant(input.now, "now");
    requireSha256(input.registrationSha256, "registrationSha256");

    return await this.#mutate(
      // assemble only from immutable registration, burn and chunks
      (records) => {
        const registration = findRegistration(records, input.family, input.registrationSha256);
        const burn = findConfirmationKind(records, input.registrationSha256, "confirmation_burned");
        const chunks = confirmationChunks(records, input.registrationSha256).map(
          // select immutable chunk metadata
          (record) => record.payload.chunk,
        );

        // require exact chronology records
        if (registration === undefined || burn === undefined) {
          throw stateError("confirmation_not_burned");
        }

        const manifest = assembleConfirmationManifest({
          access: burn.payload,
          chunks,
          registration: registration.payload,
        });
        const completePayload = {
          ...lifecycleContext(registration.payload, "member_complete", "none"),
          ...manifest,
        };
        const existing = findConfirmationKind(
          records,
          input.registrationSha256,
          "confirmation_member_complete",
        );

        // reuse only the exact deterministic full root
        if (existing !== undefined) {
          // reject a divergent repeat assembly
          if (existing.payload.fullMemberRootSha256 !== manifest.fullMemberRootSha256) {
            throw stateError("confirmation_manifest_collision");
          }
          return { records: [], result: existing.payload };
        }
        return {
          records: [{
            dueKey: registration.dueKey,
            kind: "confirmation_member_complete",
            payload: completePayload,
            recordKey: `confirmation/${input.family}/${input.registrationSha256}/complete`,
            recordedAt: input.now,
          }],
          result: completePayload,
        };
      },
    );
  }

  // record exactly one terminal family result and embargo
  async recordConfirmationResult(input) {
    requireExactKeys(input, [
      "actionIdentitySha256",
      "candidateReportSha256",
      "disposition",
      "family",
      "nextConfirmationEligibleAt",
      "now",
      "registrationSha256",
    ], "confirmation result");
    requireFamily(input.family);
    requireInstant(input.now, "now");
    requireInstant(input.nextConfirmationEligibleAt, "nextConfirmationEligibleAt");
    requireSha256(input.registrationSha256, "registrationSha256");
    requireSha256(input.candidateReportSha256, "candidateReportSha256");
    requireNullableSha256(input.actionIdentitySha256, "actionIdentitySha256");

    // require one closed terminal disposition
    if (!new Set(["promoted", "rejected", "resource_refused", "support_failed"]).has(input.disposition)) {
      throw new TypeError("confirmation disposition is invalid");
    }

    // bind action identity only to promotions
    if ((input.disposition === "promoted") !== (input.actionIdentitySha256 !== null)) {
      throw stateError("confirmation_action_identity_invalid");
    }

    // require at least the seven-day post-disposition embargo
    if (Date.parse(input.nextConfirmationEligibleAt) < Date.parse(input.now) + 7 * 86_400_000) {
      throw stateError("confirmation_embargo_invalid");
    }

    return await this.#mutate(
      // score only one complete immutable member
      (records) => {
        const registration = findRegistration(records, input.family, input.registrationSha256);
        const complete = findConfirmationKind(
          records,
          input.registrationSha256,
          "confirmation_member_complete",
        );

        // prohibit partial, cross-family or repeated-look scoring
        if (registration === undefined || complete === undefined) {
          throw stateError("confirmation_member_incomplete");
        }

        // prohibit cross-member action identity reuse
        if (input.actionIdentitySha256 !== null && records.some(
          // find another promoted action identity
          (record) => record.kind === "confirmation_result" &&
            record.payload.actionIdentitySha256 === input.actionIdentitySha256 &&
            record.payload.registrationSha256 !== input.registrationSha256,
        )) {
          throw stateError("confirmation_action_identity_reused");
        }

        const existing = findConfirmationKind(records, input.registrationSha256, "confirmation_result");
        const payload = {
          ...lifecycleContext(
            registration.payload,
            "opened",
            input.disposition === "promoted" ? "action_pending" : "terminal_no_action",
          ),
          fullMemberRootSha256: complete.payload.fullMemberRootSha256,
          candidateReportSha256: input.candidateReportSha256,
          actionIdentitySha256: input.actionIdentitySha256,
          disposition: input.disposition,
          nextConfirmationEligibleAt: input.nextConfirmationEligibleAt,
        };

        // reuse only one exact terminal result
        if (existing !== undefined) {
          // reject rescoring or redisposition
          if (!canonicalJsonBytes(existing.payload).equals(canonicalJsonBytes(payload))) {
            throw stateError("confirmation_result_collision");
          }
          return { records: [], result: existing.payload };
        }
        return {
          records: [{
            dueKey: complete.dueKey,
            kind: "confirmation_result",
            payload,
            recordKey: `confirmation/${input.family}/${input.registrationSha256}/result`,
            recordedAt: input.now,
          }],
          result: payload,
        };
      },
    );
  }

  // bind one promoted member to the live global release fence
  async prepareConfirmationAction(input) {
    requireExactKeys(input, [
      "actionIdentitySha256",
      "actionManifestSha256",
      "family",
      "fencingToken",
      "now",
      "registrationSha256",
      "releaseDueKey",
      "releaseRunId",
    ], "confirmation action preparation");
    validateActionIdentityInput(input);
    requireSha256(input.actionManifestSha256, "actionManifestSha256");
    requireDueKey(input.releaseDueKey);
    requireRunId(input.releaseRunId);

    return await this.#mutate(
      // prepare only one exact fenced promotion
      (records) => {
        const context = requirePromotedAction(records, input);
        const payload = {
          ...actionLifecycleContext(context.registration.payload, context.result.payload, {
            actionState: "release_pending",
            fencingToken: input.fencingToken,
          }),
          actionManifestSha256: input.actionManifestSha256,
          releaseDueKey: input.releaseDueKey,
          releaseRunId: input.releaseRunId,
        };
        const event = confirmationActionEvent(
          context.registration,
          "confirmation_action_prepared",
          "prepared",
          payload,
          input.now,
        );
        const existing = findConfirmationActionKind(
          records,
          input.registrationSha256,
          "confirmation_action_prepared",
        );

        // permit exact retry after lease reconciliation
        if (existing !== undefined) {
          return exactActionRetry(existing, event);
        }

        // bind promotion work only to its confirmation due key
        if (input.releaseDueKey !== context.registration.dueKey) {
          throw stateError("confirmation_action_due_mismatch");
        }

        enforceTrustedClock(records, input.now);
        const lease = deriveActiveLeases(records).get("release");

        // require the exact current global release lease
        if (lease === undefined || lease.payload.runId !== input.releaseRunId ||
          lease.payload.dueKey !== input.releaseDueKey ||
          lease.payload.fencingToken !== input.fencingToken) {
          throw stateError("release_lease_unavailable");
        }

        // prohibit one fence from authorizing another action
        if (records.some(
          // find a differently bound prepared fence
          (record) => record.kind === "confirmation_action_prepared" &&
            record.payload.fencingToken === input.fencingToken &&
            record.payload.actionIdentitySha256 !== input.actionIdentitySha256,
        )) {
          throw stateError("confirmation_action_fence_reused");
        }

        const modelRelease = records.find(
          // locate a model release already bound to this fence
          (record) => record.kind === "model_release_prepared" &&
            record.payload.fencingToken === input.fencingToken,
        );

        // require both release projections to name the same family action
        if (modelRelease !== undefined &&
          (modelRelease.payload.actionSha256 !== input.actionManifestSha256 ||
            modelRelease.payload.family !== input.family)) {
          throw stateError("confirmation_action_fence_reused");
        }
        return { records: [event], result: payload };
      },
    );
  }

  // record one remote apply receipt after preparation
  async recordConfirmationActionApplied(input) {
    requireExactKeys(input, [
      "actionIdentitySha256",
      "applyReceiptSha256",
      "family",
      "fencingToken",
      "now",
      "registrationSha256",
    ], "confirmation action apply");
    validateActionIdentityInput(input);
    requireSha256(input.applyReceiptSha256, "applyReceiptSha256");

    return await this.#mutate(
      // advance only the exact prepared action
      (records) => {
        const context = requirePreparedAction(records, input);
        const payload = {
          ...actionLifecycleContext(context.registration.payload, context.result.payload, {
            actionState: "deploying",
            fencingToken: input.fencingToken,
          }),
          actionManifestSha256: context.prepared.payload.actionManifestSha256,
          applyReceiptSha256: input.applyReceiptSha256,
        };
        return appendConfirmationActionTransition(records, {
          input,
          kind: "confirmation_action_applied",
          payload,
          suffix: "applied",
        });
      },
    );
  }

  // record one closed live verification outcome
  async recordConfirmationActionVerified(input) {
    requireExactKeys(input, [
      "actionIdentitySha256",
      "family",
      "fencingToken",
      "now",
      "registrationSha256",
      "verificationOutcome",
      "verificationSha256",
    ], "confirmation action verification");
    validateActionIdentityInput(input);
    requireSha256(input.verificationSha256, "verificationSha256");

    // require one closed remote verification result
    if (!ACTION_VERIFICATION_OUTCOMES.has(input.verificationOutcome)) {
      throw new TypeError("verificationOutcome is invalid");
    }

    return await this.#mutate(
      // verify only one applied action
      (records) => {
        const context = requireAppliedAction(records, input);
        const payload = {
          ...actionLifecycleContext(context.registration.payload, context.result.payload, {
            actionState: input.verificationOutcome === "failed" ? "failed" : "deploying",
            fencingToken: input.fencingToken,
          }),
          applyReceiptSha256: context.applied.payload.applyReceiptSha256,
          verificationOutcome: input.verificationOutcome,
          verificationSha256: input.verificationSha256,
        };
        return appendConfirmationActionTransition(records, {
          input,
          kind: "confirmation_action_verified",
          payload,
          suffix: "verified",
        });
      },
    );
  }

  // acknowledge one verified family-local release
  async acknowledgeConfirmationAction(input) {
    requireExactKeys(input, [
      "acknowledgementSha256",
      "actionIdentitySha256",
      "family",
      "fencingToken",
      "now",
      "outcome",
      "registrationSha256",
    ], "confirmation action acknowledgement");
    validateActionIdentityInput(input);
    requireSha256(input.acknowledgementSha256, "acknowledgementSha256");

    // require one successful closed acknowledgement
    if (!ACTION_ACKNOWLEDGEMENT_OUTCOMES.has(input.outcome)) {
      throw new TypeError("action acknowledgement outcome is invalid");
    }

    return await this.#mutate(
      // acknowledge only the matching verified outcome
      (records) => {
        const context = requireVerifiedAction(records, input);
        const expectedOutcome = context.verified.payload.verificationOutcome === "verified"
          ? "active"
          : "deployed_operator_off";

        // prohibit failed or mismatched verification acknowledgement
        if (input.outcome !== expectedOutcome) {
          throw stateError("confirmation_action_acknowledgement_refused");
        }
        const payload = {
          ...actionLifecycleContext(context.registration.payload, context.result.payload, {
            actionState: input.outcome,
            fencingToken: input.fencingToken,
          }),
          acknowledgementSha256: input.acknowledgementSha256,
          outcome: input.outcome,
          verificationSha256: context.verified.payload.verificationSha256,
        };
        return appendConfirmationActionTransition(records, {
          input,
          kind: "confirmation_action_acknowledged",
          payload,
          suffix: "acknowledged",
        });
      },
    );
  }

  // record one durable safe-state compensation result
  async recordConfirmationCompensationResult(input) {
    requireExactKeys(input, [
      "actionIdentitySha256",
      "compensationActionIdentitySha256",
      "compensationReportSha256",
      "family",
      "fencingToken",
      "now",
      "outcome",
      "registrationSha256",
    ], "confirmation action compensation");
    validateActionIdentityInput(input);
    requireSha256(input.compensationActionIdentitySha256, "compensationActionIdentitySha256");
    requireSha256(input.compensationReportSha256, "compensationReportSha256");
    const actionState = ACTION_COMPENSATION_STATES.get(input.outcome);

    // require a distinct immutable compensation identity
    if (input.compensationActionIdentitySha256 === input.actionIdentitySha256) {
      throw stateError("confirmation_compensation_identity_invalid");
    }

    // require one closed family-safe reconciliation outcome
    if (actionState === undefined) {
      throw new TypeError("action compensation outcome is invalid");
    }

    return await this.#mutate(
      // finalize only a prepared unacknowledged action
      (records) => {
        const context = requirePreparedAction(records, input);
        const acknowledged = findConfirmationActionKind(
          records,
          input.registrationSha256,
          "confirmation_action_acknowledged",
        );
        const applied = findConfirmationActionKind(
          records,
          input.registrationSha256,
          "confirmation_action_applied",
        );
        const verified = findConfirmationActionKind(
          records,
          input.registrationSha256,
          "confirmation_action_verified",
        );

        // prohibit compensation after successful acknowledgement
        if (acknowledged !== undefined) {
          throw stateError("confirmation_action_already_acknowledged");
        }

        // bind unapplied only before apply and restore only after failed verification
        if ((input.outcome === "unapplied" && applied !== undefined) ||
          (input.outcome !== "unapplied" &&
            (applied === undefined || verified?.payload.verificationOutcome !== "failed"))) {
          throw stateError("confirmation_action_compensation_refused");
        }

        // prohibit reusing any promotion or compensation identity
        if (records.some(
          // find a conflicting action identity
          (record) => (record.kind === "confirmation_result" &&
            record.payload.actionIdentitySha256 === input.compensationActionIdentitySha256) ||
            (record.kind === "confirmation_action_compensation_result" &&
              record.payload.compensationActionIdentitySha256 ===
                input.compensationActionIdentitySha256 &&
              record.payload.registrationSha256 !== input.registrationSha256),
        )) {
          throw stateError("confirmation_compensation_identity_reused");
        }
        const payload = {
          ...actionLifecycleContext(context.registration.payload, context.result.payload, {
            actionState,
            fencingToken: input.fencingToken,
          }),
          compensationActionIdentitySha256: input.compensationActionIdentitySha256,
          compensationReportSha256: input.compensationReportSha256,
          outcome: input.outcome,
        };
        return appendConfirmationActionTransition(records, {
          input,
          kind: "confirmation_action_compensation_result",
          payload,
          suffix: "compensation-result",
        });
      },
    );
  }

  // bind one local model commit before any remote ref publication
  async prepareModelRelease(input) {
    requireExactKeys(input, [
      "actionSha256",
      "actionTag",
      "branch",
      "commitSha",
      "expectedSourceCommit",
      "family",
      "fencingToken",
      "now",
      "packageRootSha256",
      "releaseDueKey",
      "releaseRunId",
      "releaseTag",
    ], "model release preparation");
    requireSha256(input.actionSha256, "actionSha256");
    requireGitCommit(input.commitSha, "commitSha");
    requireGitCommit(input.expectedSourceCommit, "expectedSourceCommit");
    requireFamily(input.family);
    requireFencingToken(input.fencingToken);
    requireInstant(input.now, "now");
    requireSha256(input.packageRootSha256, "packageRootSha256");
    requireDueKey(input.releaseDueKey);
    requireRunId(input.releaseRunId);
    requireReleaseRef(input.branch, "branch");
    requireReleaseRef(input.actionTag, "actionTag");
    requireReleaseTag(input.releaseTag);

    return await this.#mutate(
      // authorize only the exact live release fence
      (records) => {
        enforceTrustedClock(records, input.now);
        const lease = deriveActiveLeases(records).get("release");

        // require one matching global release lease
        if (lease === undefined || lease.payload.runId !== input.releaseRunId ||
          lease.payload.dueKey !== input.releaseDueKey ||
          lease.payload.fencingToken !== input.fencingToken) {
          throw stateError("release_lease_unavailable");
        }
        const confirmation = records.find(
          // locate confirmation authority already bound to this fence
          (record) => record.kind === "confirmation_action_prepared" &&
            record.payload.fencingToken === input.fencingToken,
        );

        // bind journal and git release identities when confirmation exists
        if (confirmation !== undefined &&
          (confirmation.payload.actionManifestSha256 !== input.actionSha256 ||
            confirmation.payload.family !== input.family)) {
          throw stateError("model_release_fence_reused");
        }
        const payload = {
          actionSha256: input.actionSha256,
          actionTag: input.actionTag,
          branch: input.branch,
          commitSha: input.commitSha,
          expectedSourceCommit: input.expectedSourceCommit,
          family: input.family,
          fencingToken: input.fencingToken,
          packageRootSha256: input.packageRootSha256,
          releaseDueKey: input.releaseDueKey,
          releaseRunId: input.releaseRunId,
          releaseTag: input.releaseTag,
        };
        const event = {
          dueKey: input.releaseDueKey,
          kind: "model_release_prepared",
          payload,
          recordKey: `model-release/${input.actionSha256}/prepared`,
          recordedAt: input.now,
        };

        // prohibit reuse of any immutable remote identity
        for (const record of records) {
          // inspect only earlier release preparations
          if (!isModelReleasePreparation(record) ||
            record.payload.actionSha256 === input.actionSha256) {
            continue;
          }

          // reject branch, tag, commit, or fence aliasing
          if (record.payload.branch === input.branch ||
            record.payload.actionTag === input.actionTag ||
            record.payload.releaseTag === input.releaseTag ||
            record.payload.commitSha === input.commitSha ||
            (record.payload.fencingToken === input.fencingToken &&
              !(record.kind === "model_compensating_release_prepared" &&
                record.payload.predecessorActionSha256 === input.actionSha256))) {
            throw stateError("model_release_identity_reused");
          }
        }
        const mutation = uniqueMutation(records, event);
        return mutation.records.length === 0
          ? { records: [], result: mutation.result.payload }
          : { records: mutation.records, result: payload };
      },
    );
  }

  // bind one compensation commit to its already prepared target release
  async prepareCompensatingModelRelease(input) {
    requireExactKeys(input, [
      "actionSha256",
      "actionTag",
      "branch",
      "commitSha",
      "expectedSourceCommit",
      "family",
      "fencingToken",
      "now",
      "packageRootSha256",
      "predecessorActionSha256",
      "releaseDueKey",
      "releaseRunId",
      "releaseTag",
    ], "compensating model release preparation");
    requireSha256(input.actionSha256, "actionSha256");
    requireSha256(input.predecessorActionSha256, "predecessorActionSha256");
    requireGitCommit(input.commitSha, "commitSha");
    requireGitCommit(input.expectedSourceCommit, "expectedSourceCommit");
    requireFamily(input.family);
    requireFencingToken(input.fencingToken);
    requireInstant(input.now, "now");
    requireSha256(input.packageRootSha256, "packageRootSha256");
    requireDueKey(input.releaseDueKey);
    requireRunId(input.releaseRunId);
    requireReleaseRef(input.branch, "branch");
    requireReleaseRef(input.actionTag, "actionTag");
    requireReleaseTag(input.releaseTag);

    return await this.#mutate(
      // authorize compensation only under the target's live release fence
      (records) => {
        enforceTrustedClock(records, input.now);
        const lease = deriveActiveLeases(records).get("release");
        const target = findModelReleasePreparation(records, input.predecessorActionSha256);

        // bind the compensation base and lease to the exact prepared target
        if (lease === undefined || target === undefined ||
          target.kind !== "model_release_prepared" ||
          lease.payload.runId !== input.releaseRunId ||
          lease.payload.dueKey !== input.releaseDueKey ||
          lease.payload.fencingToken !== input.fencingToken ||
          target.payload.family !== input.family ||
          target.payload.fencingToken !== input.fencingToken ||
          target.payload.releaseDueKey !== input.releaseDueKey ||
          target.payload.releaseRunId !== input.releaseRunId ||
          target.payload.commitSha !== input.expectedSourceCommit ||
          input.actionSha256 === input.predecessorActionSha256) {
          throw stateError("model_compensation_target_unavailable");
        }
        const payload = {
          actionSha256: input.actionSha256,
          actionTag: input.actionTag,
          branch: input.branch,
          commitSha: input.commitSha,
          expectedSourceCommit: input.expectedSourceCommit,
          family: input.family,
          fencingToken: input.fencingToken,
          packageRootSha256: input.packageRootSha256,
          predecessorActionSha256: input.predecessorActionSha256,
          releaseDueKey: input.releaseDueKey,
          releaseRunId: input.releaseRunId,
          releaseTag: input.releaseTag,
        };

        // prohibit every remote identity alias except the intentional shared fence
        for (const record of records) {
          // inspect only earlier release preparations
          if (!isModelReleasePreparation(record) ||
            record.payload.actionSha256 === input.actionSha256) {
            continue;
          }

          // retain distinct commits, branches, and immutable tags
          if (record.payload.branch === input.branch ||
            record.payload.actionTag === input.actionTag ||
            record.payload.releaseTag === input.releaseTag ||
            record.payload.commitSha === input.commitSha) {
            throw stateError("model_release_identity_reused");
          }
        }
        const mutation = uniqueMutation(records, {
          dueKey: input.releaseDueKey,
          kind: "model_compensating_release_prepared",
          payload,
          recordKey: `model-release/${input.actionSha256}/prepared`,
          recordedAt: input.now,
        });
        return mutation.records.length === 0
          ? { records: [], result: mutation.result.payload }
          : { records: mutation.records, result: payload };
      },
    );
  }

  // bind two published immutable releases before any privileged family mutation
  async recordModelReleasePair(input) {
    requireExactKeys(input, [
      "compensationActionSha256",
      "compensationCommitSha",
      "compensationReleaseTag",
      "family",
      "fencingToken",
      "now",
      "targetActionSha256",
      "targetCommitSha",
      "targetReleaseTag",
    ], "model release pair");
    requireSha256(input.compensationActionSha256, "compensationActionSha256");
    requireGitCommit(input.compensationCommitSha, "compensationCommitSha");
    requireReleaseTag(input.compensationReleaseTag);
    requireFamily(input.family);
    requireFencingToken(input.fencingToken);
    requireInstant(input.now, "now");
    requireSha256(input.targetActionSha256, "targetActionSha256");
    requireGitCommit(input.targetCommitSha, "targetCommitSha");
    requireReleaseTag(input.targetReleaseTag);

    return await this.#mutate(
      // close the exact target and compensation mapping atomically
      (records) => {
        enforceTrustedClock(records, input.now);
        const target = findModelReleasePreparation(records, input.targetActionSha256);
        const compensation = findModelReleasePreparation(
          records,
          input.compensationActionSha256,
        );
        const lease = deriveActiveLeases(records).get("release");

        // require the compensation commit to descend from this target mapping
        if (lease === undefined || target?.kind !== "model_release_prepared" ||
          compensation?.kind !== "model_compensating_release_prepared" ||
          compensation.payload.predecessorActionSha256 !== input.targetActionSha256 ||
          compensation.payload.expectedSourceCommit !== input.targetCommitSha ||
          target.payload.commitSha !== input.targetCommitSha ||
          compensation.payload.commitSha !== input.compensationCommitSha ||
          target.payload.family !== input.family || compensation.payload.family !== input.family ||
          target.payload.fencingToken !== input.fencingToken ||
          compensation.payload.fencingToken !== input.fencingToken ||
          lease.payload.fencingToken !== input.fencingToken ||
          lease.payload.dueKey !== target.payload.releaseDueKey ||
          lease.payload.runId !== target.payload.releaseRunId ||
          compensation.payload.releaseDueKey !== target.payload.releaseDueKey ||
          compensation.payload.releaseRunId !== target.payload.releaseRunId ||
          finalModelReleaseTag(records, target) !== input.targetReleaseTag ||
          finalModelReleaseTag(records, compensation) !== input.compensationReleaseTag) {
          throw stateError("model_release_pair_unbound");
        }
        const payload = {
          compensationActionSha256: input.compensationActionSha256,
          compensationCommitSha: input.compensationCommitSha,
          compensationReleaseTag: input.compensationReleaseTag,
          family: input.family,
          fencingToken: input.fencingToken,
          targetActionSha256: input.targetActionSha256,
          targetCommitSha: input.targetCommitSha,
          targetReleaseTag: input.targetReleaseTag,
        };
        return uniqueMutation(records, {
          dueKey: target.dueKey,
          kind: "model_release_pair_prepared",
          payload,
          recordKey: `model-release/${input.targetActionSha256}/pair/` +
            input.compensationActionSha256,
          recordedAt: input.now,
        });
      },
    );
  }

  // retain exact action manifests and root request before privileged mutation
  async recordQualifiedModelReleaseTransaction(input) {
    validateQualifiedModelReleaseTransactionInput(input);
    return await this.#mutate(
      // bind recovery only to the already-published immutable release pair
      (records) => {
        enforceTrustedClock(records, input.now);
        const targetActionSha256 = sha256(canonicalJsonBytes(input.targetAction));
        const compensationActionSha256 = sha256(
          canonicalJsonBytes(input.compensationAction),
        );
        const prepared = findConfirmationActionKind(
          records,
          input.registrationSha256,
          "confirmation_action_prepared",
        );
        const pair = records.find(
          // locate the exact target and compensation publication mapping
          (record) => record.kind === "model_release_pair_prepared" &&
            record.payload.targetActionSha256 === targetActionSha256 &&
            record.payload.compensationActionSha256 === compensationActionSha256,
        );

        // require local action, immutable refs and root request to agree
        if (prepared === undefined || pair === undefined ||
          prepared.payload.actionIdentitySha256 !== targetActionSha256 ||
          prepared.payload.actionManifestSha256 !== targetActionSha256 ||
          prepared.payload.fencingToken !== input.targetAction.fencingToken ||
          pair.payload.family !== input.family ||
          pair.payload.fencingToken !== input.targetAction.fencingToken ||
          pair.payload.targetCommitSha !== input.target.commitSha ||
          pair.payload.targetReleaseTag !== input.target.releaseTag ||
          pair.payload.compensationCommitSha !== input.compensation.commitSha ||
          pair.payload.compensationReleaseTag !== input.compensation.releaseTag ||
          input.releaseRequest.actionSha256 !== targetActionSha256 ||
          input.releaseRequest.targetRelease !== input.target.releaseTag ||
          input.releaseRequest.compensatingRelease !== input.compensation.releaseTag) {
          throw stateError("qualified_model_release_transaction_unbound");
        }
        const payload = {
          compensation: structuredClone(input.compensation),
          compensationAction: structuredClone(input.compensationAction),
          family: input.family,
          finalizationProof: structuredClone(input.finalizationProof),
          registrationSha256: input.registrationSha256,
          releaseRequest: structuredClone(input.releaseRequest),
          target: structuredClone(input.target),
          targetAction: structuredClone(input.targetAction),
        };
        return uniqueMutation(records, {
          dueKey: prepared.dueKey,
          kind: "qualified_model_release_transaction",
          payload,
          recordKey: `model-release/${targetActionSha256}/qualified-transaction`,
          recordedAt: input.now,
        });
      },
    );
  }

  // retain original C/T/F documents before the first root authority transition
  async recordQualifiedTerminalAuthority(input) {
    requireExactKeys(input, [
      "action", "anchor", "candidateReportSha256", "current", "family",
      "finalizationProof", "nextConfirmationEligibleAt", "now", "registrationSha256",
      "releaseDueKey", "releaseRunId", "seal",
    ], "qualified terminal authority");
    requirePlainObject(input.action, "qualified action");
    requirePlainObject(input.anchor, "qualified anchor");
    requirePlainObject(input.current, "qualified current source");
    requirePlainObject(input.finalizationProof, "qualified finalization proof");
    requirePlainObject(input.seal, "qualified input seal");
    requireExactKeys(input.action, MODEL_ACTION_KEYS, "qualified action");
    requireFamily(input.family);
    requireInstant(input.now, "now");
    requireInstant(input.nextConfirmationEligibleAt, "nextConfirmationEligibleAt");
    requireSha256(input.registrationSha256, "registrationSha256");
    requireSha256(input.candidateReportSha256, "candidateReportSha256");
    requireDueKey(input.releaseDueKey);
    requireRunId(input.releaseRunId);
    const actionSha256 = sha256(canonicalJsonBytes(input.action));

    // crossbind the staged root documents to the exact promotion action
    if (input.action.actionKind !== "promote" || input.action.family !== input.family ||
      input.anchor.actionSha256 !== actionSha256 ||
      input.anchor.inputSealSha256 !== sha256(canonicalJsonBytes(input.seal)) ||
      input.finalizationProof.actionSha256 !== actionSha256 ||
      input.finalizationProof.transferredAnchorSha256 !==
        sha256(canonicalJsonBytes(input.anchor)) ||
      input.finalizationProof.graphManifestSha256 !== input.seal.graphManifestSha256 ||
      input.current.commit !== input.action.expectedSourceCommit ||
      input.current.release !== input.action.expectedSourceRelease ||
      input.current.settingsSha256 !== input.action.expectedSettingsSha256) {
      throw new TypeError("qualified terminal authority identity differs");
    }
    return await this.#mutate(
      // append the result, action preparation and root authority atomically
      (records) => {
        enforceTrustedClock(records, input.now);
        const registration = findRegistration(
          records,
          input.family,
          input.registrationSha256,
        );
        const complete = findConfirmationKind(
          records,
          input.registrationSha256,
          "confirmation_member_complete",
        );
        const lease = deriveActiveLeases(records).get("release");

        // bind all three records to one complete member and live original fence
        if (registration === undefined || complete === undefined ||
          input.action.candidateSha256 !== registration.payload.candidateSha256 ||
          input.action.fullMemberRootSha256 !== complete.payload.fullMemberRootSha256 ||
          input.action.policyReportSha256 !== input.candidateReportSha256 ||
          input.releaseDueKey !== registration.dueKey || lease === undefined ||
          lease.payload.dueKey !== input.releaseDueKey ||
          lease.payload.runId !== input.releaseRunId ||
          lease.payload.fencingToken !== input.action.fencingToken) {
          throw stateError("qualified_terminal_authority_unbound");
        }
        if (Date.parse(input.nextConfirmationEligibleAt) <
          Date.parse(input.action.reportCreatedAt) + 7 * 86_400_000) {
          throw stateError("confirmation_embargo_invalid");
        }

        // prohibit action and fence reuse across another registration
        if (records.some(
          (record) => record.kind === "confirmation_result" &&
            record.payload.actionIdentitySha256 === actionSha256 &&
            record.payload.registrationSha256 !== input.registrationSha256,
        ) || records.some(
          (record) => record.kind === "confirmation_action_prepared" &&
            record.payload.fencingToken === input.action.fencingToken &&
            record.payload.actionIdentitySha256 !== actionSha256,
        )) {
          throw stateError("confirmation_action_identity_reused");
        }
        const modelRelease = records.find(
          (record) => record.kind === "model_release_prepared" &&
            record.payload.fencingToken === input.action.fencingToken,
        );

        // keep an already-used release fence bound to this exact action and family
        if (modelRelease !== undefined &&
          (modelRelease.payload.actionSha256 !== actionSha256 ||
            modelRelease.payload.family !== input.family)) {
          throw stateError("confirmation_action_fence_reused");
        }
        const localResult = {
          ...lifecycleContext(registration.payload, "opened", "action_pending"),
          fullMemberRootSha256: complete.payload.fullMemberRootSha256,
          candidateReportSha256: input.candidateReportSha256,
          actionIdentitySha256: actionSha256,
          disposition: "promoted",
          nextConfirmationEligibleAt: input.nextConfirmationEligibleAt,
        };
        const preparedPayload = {
          ...actionLifecycleContext(registration.payload, localResult, {
            actionState: "release_pending",
            fencingToken: input.action.fencingToken,
          }),
          actionManifestSha256: actionSha256,
          releaseDueKey: input.releaseDueKey,
          releaseRunId: input.releaseRunId,
        };
        const payload = {
          action: structuredClone(input.action),
          anchor: structuredClone(input.anchor),
          current: structuredClone(input.current),
          family: input.family,
          finalizationProof: structuredClone(input.finalizationProof),
          registrationSha256: input.registrationSha256,
          seal: structuredClone(input.seal),
        };
        const resultEvent = {
          dueKey: complete.dueKey,
          kind: "confirmation_result",
          payload: localResult,
          recordKey: `confirmation/${input.family}/${input.registrationSha256}/result`,
          recordedAt: input.now,
        };
        const preparedEvent = confirmationActionEvent(
          registration,
          "confirmation_action_prepared",
          "prepared",
          preparedPayload,
          input.now,
        );
        const authorityEvent = {
          dueKey: registration.dueKey,
          kind: "qualified_terminal_authority",
          payload,
          recordKey: `confirmation/${input.family}/${input.registrationSha256}/` +
            `qualified-authority`,
          recordedAt: input.now,
        };
        const additions = [];
        let working = records;

        // reuse only exact all-or-nothing retry records
        for (const event of [resultEvent, preparedEvent, authorityEvent]) {
          const mutation = uniqueMutation(working, event);
          additions.push(...mutation.records);
          working = [...working, ...mutation.records];
        }
        return {
          records: additions,
          result: { authority: payload, localResult },
        };
      },
    );
  }

  // retain original no-action C/T/F documents before the first root transition
  async recordNoActionTerminalAuthority(input) {
    requireExactKeys(input, [
      "actionIdentity", "anchor", "current", "disposition", "family",
      "finalizationProof", "now", "registrationSha256", "seal",
    ], "no-action terminal authority");
    requirePlainObject(input.actionIdentity, "no-action identity");
    requirePlainObject(input.anchor, "no-action anchor");
    requirePlainObject(input.current, "no-action current source");
    requirePlainObject(input.finalizationProof, "no-action finalization proof");
    requirePlainObject(input.seal, "no-action input seal");
    requireFamily(input.family);
    requireInstant(input.now, "now");
    requireSha256(input.registrationSha256, "registrationSha256");
    const actionSha256 = sha256(canonicalJsonBytes(input.actionIdentity));

    // bind the immutable no-action identity to one complete C/T/F successor
    if (!new Set(["rejected", "support_failed"]).has(input.disposition) ||
      input.actionIdentity.contractVersion !==
        "adjustment-terminal-no-action-identity/v3" ||
      input.actionIdentity.disposition !== input.disposition ||
      input.anchor.actionSha256 !== actionSha256 ||
      input.anchor.inputSealSha256 !== sha256(canonicalJsonBytes(input.seal)) ||
      input.finalizationProof.actionSha256 !== actionSha256 ||
      input.finalizationProof.transferredAnchorSha256 !==
        sha256(canonicalJsonBytes(input.anchor)) ||
      input.finalizationProof.graphManifestSha256 !== input.seal.graphManifestSha256 ||
      input.current.commit !== input.seal.sourceCommit) {
      throw new TypeError("no-action terminal authority identity differs");
    }
    return await this.#mutate(
      // require one complete unscored registration before root transition
      (records) => {
        enforceTrustedClock(records, input.now);
        const complete = findConfirmationKind(
          records,
          input.registrationSha256,
          "confirmation_member_complete",
        );
        const result = findConfirmationKind(
          records,
          input.registrationSha256,
          "confirmation_result",
        );

        // persist before result, while allowing an exact retry after result
        if (complete === undefined || result !== undefined &&
          result.payload.disposition !== input.disposition) {
          throw stateError("no_action_terminal_authority_unbound");
        }
        const payload = {
          actionIdentity: structuredClone(input.actionIdentity),
          anchor: structuredClone(input.anchor),
          current: structuredClone(input.current),
          disposition: input.disposition,
          family: input.family,
          finalizationProof: structuredClone(input.finalizationProof),
          registrationSha256: input.registrationSha256,
          seal: structuredClone(input.seal),
        };
        return uniqueMutation(records, {
          dueKey: complete.dueKey,
          kind: "no_action_terminal_authority",
          payload,
          recordKey: `confirmation/${input.family}/${input.registrationSha256}/` +
            `no-action-authority`,
          recordedAt: input.now,
        });
      },
    );
  }

  // retain an immutable release-tag race and its absent successor
  async recordModelReleaseTagCollision(input) {
    requireExactKeys(input, [
      "actionSha256",
      "attemptedReleaseTag",
      "collisionCommitSha",
      "family",
      "fencingToken",
      "now",
      "successorReleaseTag",
    ], "model release tag collision");
    requireSha256(input.actionSha256, "actionSha256");
    requireReleaseTag(input.attemptedReleaseTag);
    requireGitCommit(input.collisionCommitSha, "collisionCommitSha");
    requireFamily(input.family);
    requireFencingToken(input.fencingToken);
    requireInstant(input.now, "now");
    requireReleaseTag(input.successorReleaseTag);

    return await this.#mutate(
      // bind a collision only to its prepared release
      (records) => {
        const prepared = findModelReleasePreparation(records, input.actionSha256);

        const collisions = records.filter(
          // retain ordered collisions for this action only
          (record) => record.kind === "model_release_tag_collision" &&
            record.payload.actionSha256 === input.actionSha256,
        );
        const expectedAttemptedTag = collisions.at(-1)?.payload.successorReleaseTag ??
          prepared?.payload.releaseTag;

        // require exact family, fence, and attempted tag ancestry
        if (prepared === undefined || prepared.payload.family !== input.family ||
          prepared.payload.fencingToken !== input.fencingToken ||
          expectedAttemptedTag !== input.attemptedReleaseTag ||
          input.successorReleaseTag === input.attemptedReleaseTag) {
          throw stateError("model_release_collision_unbound");
        }
        const payload = {
          actionSha256: input.actionSha256,
          attemptedReleaseTag: input.attemptedReleaseTag,
          collisionCommitSha: input.collisionCommitSha,
          family: input.family,
          fencingToken: input.fencingToken,
          successorReleaseTag: input.successorReleaseTag,
        };
        return uniqueMutation(records, {
          dueKey: prepared.dueKey,
          kind: "model_release_tag_collision",
          payload,
          recordKey: `model-release/${input.actionSha256}/collision/${input.attemptedReleaseTag}`,
          recordedAt: input.now,
        });
      },
    );
  }

  // append one exact unique journal event
  async #appendUnique(event) {
    return await this.#mutate(
      // enforce record-key idempotence
      (records) => uniqueMutation(records, event),
    );
  }

  // run one exclusive append transaction
  async #mutate(builder) {
    return await this.#store.withLock(
      // verify, decide and durably append under one held lock
      async (transaction) => {
        const existing = verifyJournalBytes(await transaction.readJournal());
        const mutation = builder(existing);

        // return idempotent results without filesystem mutation
        if (mutation.records.length === 0) {
          return mutation.result;
        }

        const appended = appendJournalRecords(existing, mutation.records);
        const newRecords = appended.slice(existing.length);
        await transaction.append(
          Buffer.concat(newRecords.map(
            // encode one append-only journal line
            (record) => canonicalJsonBytes(record),
          )),
          headsForRecords(appended),
        );
        return mutation.result;
      },
    );
  }

  // read one verified journal projection
  async #read(project) {
    return await this.#store.withLock(
      // read under the same exclusive journal lock
      async (transaction) => project(verifyJournalBytes(await transaction.readJournal())),
    );
  }
}

// validate the exact root and database authenticated zero-frontier witness
export function validateAdjustmentFutureOnlyEpochWitness(value) {
  requireExactKeys(value, [
    "activationKind",
    "archiveCommitOrdinal",
    "catalogFrontierSha256",
    "contractVersion",
    "controlPlaneSha256",
    "controlPlaneVersion",
    "databaseMigrationHistorySha256",
    "epochAt",
    "servingSnapshotSha256",
    "sourceCommit",
    "sourceRelease",
    "sourceServerImageDigest",
    "sourceWebImageDigest",
    "witnessSha256",
  ], "future-only capture epoch witness");
  for (const name of [
    "catalogFrontierSha256",
    "controlPlaneSha256",
    "databaseMigrationHistorySha256",
    "servingSnapshotSha256",
    "witnessSha256",
  ]) {
    requireSha256(value[name], name);
  }
  requireInstant(value.epochAt, "epochAt");
  requireGitCommit(value.sourceCommit, "sourceCommit");
  const genesisFrontierSha256 = sha256(Buffer.from("adjustment-revision-frontier/v1\n0\n"));
  const unsigned = { ...value };
  delete unsigned.witnessSha256;

  // accept only the empty authoritative frontier established by deployed v14
  if (value.activationKind !== "inert_v14_pre_activation" ||
    value.contractVersion !== "adjustment-revision-capture-epoch-witness/v1" ||
    value.archiveCommitOrdinal !== "0" ||
    value.catalogFrontierSha256 !== genesisFrontierSha256 ||
    value.controlPlaneVersion !== "14" ||
    value.databaseMigrationHistorySha256 !== ADJUSTMENT_ROLLING_MIGRATION_HISTORY_SHA256 ||
    typeof value.sourceRelease !== "string" ||
    !/^\d{4}\.\d{2}\.\d{2}-[1-9]\d{0,2}$/u.test(value.sourceRelease) ||
    typeof value.sourceServerImageDigest !== "string" ||
    !/^sha256:[a-f0-9]{64}$/u.test(value.sourceServerImageDigest) ||
    typeof value.sourceWebImageDigest !== "string" ||
    !/^sha256:[a-f0-9]{64}$/u.test(value.sourceWebImageDigest) ||
    value.witnessSha256 !== sha256(canonicalJsonBytes(unsigned))) {
    throw new TypeError("future-only capture epoch witness is invalid");
  }
  return Object.freeze(structuredClone(value));
}

// deterministically assemble one full confirmation manifest
export function assembleConfirmationManifest(input) {
  requireExactKeys(input, ["access", "chunks", "registration"], "confirmation assembly");
  const registration = input.registration;
  const access = input.access;
  requireFamily(registration.family);
  requireSha256(registration.registrationSha256, "registrationSha256");
  requireSha256(access.accessSha256, "accessSha256");
  requireSha256(access.targetComparatorSnapshotRootSha256, "targetComparatorSnapshotRootSha256");
  requireSha256(access.revisionCatalogWatermarkSha256, "revisionCatalogWatermarkSha256");
  requireSha256(access.expectedKeySetSha256, "expectedKeySetSha256");
  requireInstant(access.targetCutoffAt, "targetCutoffAt");
  const chunkCount = CONFIRMATION_CHUNK_COUNTS.get(registration.family);
  const usesParts = Array.isArray(input.chunks) && input.chunks.some(
    // detect only the additive logical-part grammar
    (chunk) => Object.hasOwn(chunk, "contractVersion"),
  );

  // assemble additive physical parts into the frozen logical window count
  if (usesParts) {
    return assembleConfirmationPartManifest({
      access,
      chunkCount,
      chunks: input.chunks,
      registration,
    });
  }

  // require the exact complete family chunk count
  if (!Array.isArray(input.chunks) || input.chunks.length !== chunkCount) {
    throw stateError("confirmation_chunk_count_incomplete");
  }

  const chunks = input.chunks.map(
    // validate each ordered immutable chunk
    (chunk, index) => {
      const normalized = validateConfirmationChunk(chunk, registration.family);

      // require consecutive chunk order
      if (normalized.chunkIndex !== index) {
        throw stateError("confirmation_chunk_order_refused");
      }
      return normalized;
    },
  );
  let expectedStart = registration.intervalStartLocalDate;

  // prove exact no-gap local-date partitions
  for (let index = 0; index < chunks.length; index += 1) {
    const chunk = chunks[index];
    const expectedDays = index === chunks.length - 1
      ? CONFIRMATION_INTERVAL_DAYS.get(registration.family) - 14 * (chunks.length - 1)
      : 14;
    const expectedEnd = addLocalDates(expectedStart, expectedDays);

    // reject a gap, overlap or wrong final partition
    if (chunk.fromLocalDate !== expectedStart ||
      chunk.toLocalDateExclusive !== expectedEnd) {
      throw stateError("confirmation_partition_invalid");
    }
    expectedStart = expectedEnd;
  }

  // require exact registration interval coverage
  if (expectedStart !== registration.intervalEndExclusiveLocalDate) {
    throw stateError("confirmation_interval_invalid");
  }

  const projectedChunks = chunks.map(
    // bind only immutable ordered chunk metadata
    (chunk) => ({
      chunkIndex: chunk.chunkIndex,
      fromLocalDate: chunk.fromLocalDate,
      toLocalDateExclusive: chunk.toLocalDateExclusive,
      chunkSha256: chunk.chunkSha256,
      recordCount: chunk.recordCount,
      expectedKeySubsetSha256: chunk.expectedKeySubsetSha256,
    }),
  );
  const eligiblePredictionSetSha256 = sha256(canonicalJsonBytes(chunks.map(
    // bind every ordered eligible subset root
    (chunk) => chunk.eligiblePredictionSubsetSha256,
  )));
  const missingKeySetSha256 = sha256(canonicalJsonBytes(chunks.map(
    // bind every ordered missing subset root
    (chunk) => chunk.missingKeySubsetSha256,
  )));
  const manifestWithoutRoot = {
    contractVersion: "adjustment-confirmation-member/v2",
    family: registration.family,
    registrationSha256: registration.registrationSha256,
    accessSha256: access.accessSha256,
    intervalStartLocalDate: registration.intervalStartLocalDate,
    intervalEndExclusiveLocalDate: registration.intervalEndExclusiveLocalDate,
    targetCutoffAt: access.targetCutoffAt,
    targetComparatorSnapshotRootSha256: access.targetComparatorSnapshotRootSha256,
    revisionCatalogWatermarkSha256: access.revisionCatalogWatermarkSha256,
    expectedKeySetSha256: access.expectedKeySetSha256,
    eligiblePredictionSetSha256,
    missingKeySetSha256,
    chunkCount,
    chunks: projectedChunks,
  };
  return {
    ...manifestWithoutRoot,
    fullMemberRootSha256: sha256(canonicalJsonBytes(manifestWithoutRoot)),
  };
}

// assemble bounded physical parts into exact logical confirmation windows
function assembleConfirmationPartManifest(input) {
  if (!Array.isArray(input.chunks) || input.chunks.length < input.chunkCount) {
    throw stateError("confirmation_chunk_count_incomplete");
  }
  const parts = input.chunks.map(
    // validate every immutable value-blind part before grouping
    (chunk) => validateConfirmationChunk(chunk, input.registration.family),
  );

  // prohibit mixing the legacy and additive hash domains
  if (parts.some((part) => part.contractVersion !==
      ADJUSTMENT_CONFIRMATION_CHUNK_PART_V3_VERSION)) {
    throw stateError("confirmation_chunk_version_mixed");
  }
  const grouped = new Map();

  // retain physical order while collecting each logical window
  for (const part of parts) {
    const group = grouped.get(part.logicalChunkIndex) ?? [];
    group.push(part);
    grouped.set(part.logicalChunkIndex, group);
  }
  if (grouped.size !== input.chunkCount) {
    throw stateError("confirmation_chunk_count_incomplete");
  }
  let expectedStart = input.registration.intervalStartLocalDate;
  const logicalChunks = [];

  // prove every logical window and all of its declared physical parts
  for (let logicalChunkIndex = 0; logicalChunkIndex < input.chunkCount;
    logicalChunkIndex += 1) {
    const group = grouped.get(logicalChunkIndex);
    const expectedDays = logicalChunkIndex === input.chunkCount - 1
      ? CONFIRMATION_INTERVAL_DAYS.get(input.registration.family) -
        14 * (input.chunkCount - 1)
      : 14;
    const expectedEnd = addLocalDates(expectedStart, expectedDays);

    // reject absent parts and incomplete logical windows
    if (group === undefined || group.length < 1 || group.length !== group[0].partCount) {
      throw stateError("confirmation_chunk_count_incomplete");
    }
    for (const [partIndex, part] of group.entries()) {
      // require one complete consecutive part sequence and exact logical date block
      if (part.partIndex !== partIndex || part.partCount !== group.length ||
        part.logicalChunkIndex !== logicalChunkIndex || part.fromLocalDate !== expectedStart ||
        part.toLocalDateExclusive !== expectedEnd ||
        (partIndex > 0 && part.captureLocalDate <= group[partIndex - 1].captureLocalDate)) {
        throw stateError("confirmation_partition_invalid");
      }
    }
    logicalChunks.push(Object.freeze({
      contractVersion: ADJUSTMENT_CONFIRMATION_LOGICAL_CHUNK_V3_VERSION,
      eligiblePredictionSubsetSha256: sha256(canonicalJsonBytes(group.map(
        // retain every ordered eligible subset root
        (part) => part.eligiblePredictionSubsetSha256,
      ))),
      expectedKeyCount: group.reduce(
        // retain the complete preregistered cell count
        (count, part) => count + part.expectedKeyCount,
        0,
      ),
      expectedKeySubsetSha256: sha256(canonicalJsonBytes(group.map(
        // retain every ordered expected subset root
        (part) => part.expectedKeySubsetSha256,
      ))),
      fromLocalDate: expectedStart,
      logicalChunkIndex,
      missingKeyCount: group.reduce(
        // retain the exact unavailable cell count
        (count, part) => count + part.missingKeyCount,
        0,
      ),
      missingKeySubsetSha256: sha256(canonicalJsonBytes(group.map(
        // retain every ordered missing subset root
        (part) => part.missingKeySubsetSha256,
      ))),
      partCount: group.length,
      partRootSha256: sha256(canonicalJsonBytes(group.map(
        // bind every exact value-bearing part identity
        (part) => part.partSha256,
      ))),
      recordCount: group.reduce(
        // retain the exact eligible record count
        (count, part) => count + part.recordCount,
        0,
      ),
      toLocalDateExclusive: expectedEnd,
    }));
    expectedStart = expectedEnd;
  }

  // require exact registration interval coverage after the final logical window
  if (expectedStart !== input.registration.intervalEndExclusiveLocalDate) {
    throw stateError("confirmation_interval_invalid");
  }
  const expectedKeySetSha256 = sha256(canonicalJsonBytes(logicalChunks.map(
    // bind the preregistered complete logical key population
    (chunk) => chunk.expectedKeySubsetSha256,
  )));

  // require burn authority to name this exact hierarchical expected population
  if (input.access.expectedKeySetSha256 !== expectedKeySetSha256 ||
    input.registration.reservedKeySha256 !== expectedKeySetSha256) {
    throw stateError("confirmation_expected_key_set_invalid");
  }
  const manifestWithoutRoot = {
    accessSha256: input.access.accessSha256,
    chunkCount: input.chunkCount,
    chunks: logicalChunks,
    contractVersion: "adjustment-confirmation-member/v3",
    eligiblePredictionSetSha256: sha256(canonicalJsonBytes(logicalChunks.map(
      // bind each logical eligible population
      (chunk) => chunk.eligiblePredictionSubsetSha256,
    ))),
    expectedKeySetSha256,
    family: input.registration.family,
    intervalEndExclusiveLocalDate: input.registration.intervalEndExclusiveLocalDate,
    intervalStartLocalDate: input.registration.intervalStartLocalDate,
    missingKeySetSha256: sha256(canonicalJsonBytes(logicalChunks.map(
      // bind each logical missing population
      (chunk) => chunk.missingKeySubsetSha256,
    ))),
    registrationSha256: input.registration.registrationSha256,
    revisionCatalogWatermarkSha256: input.access.revisionCatalogWatermarkSha256,
    targetComparatorSnapshotRootSha256: input.access.targetComparatorSnapshotRootSha256,
    targetCutoffAt: input.access.targetCutoffAt,
  };
  return Object.freeze({
    ...manifestWithoutRoot,
    fullMemberRootSha256: sha256(canonicalJsonBytes(manifestWithoutRoot)),
  });
}

// hold the one fixed production journal root
class FixedRootJournalStore {
  #root = ADJUSTMENT_DEFAULT_STATE_ROOT;

  // create and prove the owner-private state root
  async initialize() {
    await ensureAdjustmentPrivateDirectory("state");
    await assertPrivateDirectory(this.#root);
    const resolvedRoot = await realpath(this.#root);

    // require the exact fixed HOME root without symlink drift
    if (resolvedRoot !== resolve(this.#root)) {
      throw stateError("state_path_refused");
    }
    const filesystem = await statfs(resolvedRoot, { bigint: true });

    // require the same native ext4 boundary as the archive
    if (BigInt(filesystem.type) !== EXT4_MAGIC) {
      throw stateError("state_filesystem_refused");
    }
    return { rootKind: ADJUSTMENT_MAINTENANCE_STATE_ROOT_KIND };
  }

  // execute one callback while a kernel flock holder remains alive
  async withLock(callback) {
    const lockPath = join(this.#root, "journal.lock");
    const lockHandle = await open(
      lockPath,
      fsConstants.O_CREAT | fsConstants.O_RDWR | fsConstants.O_NOFOLLOW,
      0o600,
    );
    const holder = spawn(
      "flock",
      ["--exclusive", "--nonblock", lockPath, "sh", "-c", "printf ready; cat >/dev/null"],
      { stdio: ["pipe", "pipe", "pipe"] },
    );

    try {
      const ready = await waitForLockHolder(holder);

      // refuse an unavailable exclusive lock
      if (ready !== "ready") {
        throw stateError("journal_lock_occupied");
      }

      const transaction = this.#transaction();
      return await callback(transaction);
    } finally {
      holder.stdin.end();
      await waitForExit(holder).catch(() => undefined);
      await lockHandle.close();
    }
  }

  // create one lock-scoped durable transaction port
  #transaction() {
    return {
      // read the complete bounded append-only journal
      readJournal: async () => {
        const path = join(this.#root, "journal.jsonl");

        try {
          const details = await assertPrivateFile(path);

          // reject an unbounded journal read
          if (details.size > MAXIMUM_JOURNAL_BYTES) {
            throw stateError("journal_size_refused");
          }
          return await readFile(path);
        } catch (error) {
          // treat only absence as genesis
          if (error?.code === "ENOENT") {
            return Buffer.alloc(0);
          }
          throw error;
        }
      },
      // append, fsync and rotate both head slots
      append: async (bytes, heads) => {
        const path = join(this.#root, "journal.jsonl");
        const handle = await open(
          path,
          fsConstants.O_CREAT | fsConstants.O_APPEND | fsConstants.O_WRONLY | fsConstants.O_NOFOLLOW,
          0o600,
        );

        try {
          const details = await handle.stat();

          // reject linked, foreign, broad or oversized append targets
          if (!details.isFile() || details.uid !== process.getuid() ||
            (details.mode & 0o777) !== 0o600 || details.nlink !== 1 ||
            details.size + bytes.length > MAXIMUM_JOURNAL_BYTES) {
            throw stateError("journal_append_refused");
          }
          await handle.writeFile(bytes);
          await handle.sync();
        } finally {
          await handle.close();
        }
        await this.#writeHeads(heads);
      },
      // reconcile bounded head projections
      writeHeads: async (heads) => await this.#writeHeads(heads),
    };
  }

  // atomically write current and previous journal heads
  async #writeHeads(heads) {
    const currentPath = join(this.#root, "head.current");
    const previousPath = join(this.#root, "head.previous");
    await writeAtomicPrivateFile(currentPath, canonicalJsonBytes(heads.current));

    // write or remove only the bounded previous projection
    if (heads.previous === null) {
      try {
        await unlink(previousPath);
      } catch (error) {
        // ignore only an absent genesis slot
        if (error?.code !== "ENOENT") {
          throw error;
        }
      }
    } else {
      await writeAtomicPrivateFile(previousPath, canonicalJsonBytes(heads.previous));
    }
    await syncDirectory(this.#root);
  }
}

// append canonical hash-chained journal records
function appendJournalRecords(existing, additions) {
  const records = [...existing];
  let previousSha256 = records.at(-1)?.recordSha256 ?? null;
  let generation = BigInt(records.at(-1)?.generation ?? "0");

  // append every requested record in one deterministic order
  for (const addition of additions) {
    validateJournalAddition(addition);
    generation += 1n;

    // stop rather than wrap the unsigned generation
    if (generation > ADJUSTMENT_MAXIMUM_FENCING_TOKEN) {
      throw stateError("journal_generation_exhausted");
    }
    const payloadSha256 = sha256(canonicalJsonBytes(addition.payload));
    const withoutHash = {
      contractVersion: ADJUSTMENT_JOURNAL_CONTRACT_VERSION,
      generation: generation.toString(),
      kind: addition.kind,
      recordKey: addition.recordKey,
      dueKey: addition.dueKey,
      recordedAt: addition.recordedAt,
      previousSha256,
      payloadSha256,
      payload: addition.payload,
    };
    const record = { ...withoutHash, recordSha256: sha256(canonicalJsonBytes(withoutHash)) };
    records.push(record);
    previousSha256 = record.recordSha256;
  }
  return records;
}

// verify the complete append-only journal chain
export function verifyJournalBytes(bytes) {
  // require one bounded immutable byte buffer
  if (!Buffer.isBuffer(bytes) || bytes.length > MAXIMUM_JOURNAL_BYTES) {
    throw new TypeError("journal bytes are invalid");
  }

  // return the exact empty genesis journal
  if (bytes.length === 0) {
    return [];
  }

  // require one newline-terminated canonical journal
  if (bytes.at(-1) !== 0x0a) {
    throw new TypeError("journal is not newline terminated");
  }
  const lines = bytes.toString("utf8").slice(0, -1).split("\n");
  let previousSha256 = null;
  const records = [];

  // verify every generation and predecessor hash
  for (let index = 0; index < lines.length; index += 1) {
    let record;

    try {
      record = JSON.parse(lines[index]);
    } catch {
      throw new TypeError("journal JSON is invalid");
    }
    validateJournalRecord(record, index + 1, previousSha256);

    // require exact canonical line bytes
    if (canonicalJsonBytes(record).toString("utf8").slice(0, -1) !== lines[index]) {
      throw new TypeError("journal record is not canonical");
    }
    records.push(record);
    previousSha256 = record.recordSha256;
  }
  return records;
}

// validate one complete journal record
function validateJournalRecord(record, expectedGeneration, previousSha256) {
  requireExactKeys(record, [
    "contractVersion",
    "generation",
    "kind",
    "recordKey",
    "dueKey",
    "recordedAt",
    "previousSha256",
    "payloadSha256",
    "payload",
    "recordSha256",
  ], "journal record");

  // require exact contract, generation and predecessor
  if (record.contractVersion !== ADJUSTMENT_JOURNAL_CONTRACT_VERSION ||
    record.generation !== String(expectedGeneration) ||
    record.previousSha256 !== previousSha256) {
    throw new TypeError("journal chain is invalid");
  }
  requireRecordKey(record.recordKey);
  requireNullableDueKey(record.dueKey);
  requireInstant(record.recordedAt, "recordedAt");
  requireSha256(record.payloadSha256, "payloadSha256");
  requireSha256(record.recordSha256, "recordSha256");
  requirePlainObject(record.payload, "journal payload");

  // bind the exact payload bytes
  if (sha256(canonicalJsonBytes(record.payload)) !== record.payloadSha256) {
    throw new TypeError("journal payload hash is invalid");
  }
  const withoutHash = { ...record };
  delete withoutHash.recordSha256;

  // bind the exact record bytes
  if (sha256(canonicalJsonBytes(withoutHash)) !== record.recordSha256) {
    throw new TypeError("journal record hash is invalid");
  }
}

// validate one pending journal addition
function validateJournalAddition(addition) {
  requireExactKeys(addition, ["dueKey", "kind", "payload", "recordKey", "recordedAt"], "journal addition");
  requireNullableDueKey(addition.dueKey);
  requireRecordKey(addition.recordKey);
  requireInstant(addition.recordedAt, "recordedAt");
  requirePlainObject(addition.payload, "journal payload");

  // require one bounded closed event kind
  if (!/^[a-z][a-z0-9_]{0,63}$/u.test(addition.kind)) {
    throw new TypeError("journal kind is invalid");
  }
}

// derive bounded current and previous head projections
function headsForRecords(records) {
  const current = records.at(-1);
  const previous = records.at(-2);
  return {
    current: current === undefined
      ? { generation: "0", recordSha256: null }
      : { generation: current.generation, recordSha256: current.recordSha256 },
    previous: previous === undefined
      ? null
      : { generation: previous.generation, recordSha256: previous.recordSha256 },
  };
}

// derive active leases without removing unknown history
function deriveActiveLeases(records) {
  const active = new Map();

  // apply append-only lease transitions in order
  for (const record of records) {
    // open one exact lease scope
    if (record.kind === "lease_acquired") {
      active.set(record.payload.scope, record);
    }

    // replace one expired lease only through its exact durable predecessor
    if (record.kind === "lease_continued") {
      const current = active.get(record.payload.scope);

      // ignore malformed history here so later authority checks fail closed
      if (current !== undefined && current.recordSha256 ===
        record.payload.predecessorLeaseRecordSha256 &&
        current.payload.runId === record.payload.runId &&
        current.payload.dueKey === record.payload.dueKey &&
        current.payload.fencingToken === record.payload.fencingToken) {
        active.set(record.payload.scope, record);
      }
    }

    // close only the matching retained lease
    if (record.kind === "lease_released" || record.kind === "lease_reconciled") {
      const current = active.get(record.payload.scope);

      // retain unrelated or mismatched lease evidence
      if (current !== undefined && current.payload.runId === record.payload.runId &&
        current.payload.dueKey === record.payload.dueKey) {
        active.delete(record.payload.scope);
      }
    }
  }
  return active;
}

// project one prepared action against its current release lease
function projectConfirmationActionLease(prepared, active, now) {
  return {
    dueKey: prepared.payload.releaseDueKey,
    fencingToken: prepared.payload.fencingToken,
    live: active?.payload.dueKey === prepared.payload.releaseDueKey &&
      active.payload.runId === prepared.payload.releaseRunId &&
      active.payload.fencingToken === prepared.payload.fencingToken &&
      Date.parse(active.payload.expiresAt) > Date.parse(now),
    runId: prepared.payload.releaseRunId,
  };
}

// derive the authenticated zero cursor and every later durable page advance
function deriveRevisionCursor(records) {
  const advanced = records.findLast(
    // select the latest append-only frontier transition
    (record) => record.kind === "revision_cursor_advanced",
  );

  // prefer the most recent successfully archived page cursor
  if (advanced !== undefined) {
    return {
      archiveCommitOrdinal: advanced.payload.nextArchiveCommitOrdinal,
      frontierSha256: advanced.payload.nextFrontierSha256,
    };
  }
  const genesis = records.find(
    // accept only the explicit future-only lifecycle witness as cursor genesis
    (record) => record.kind === "lifecycle_genesis" &&
      record.payload.sourceKind === "future_only_verified_capture_epoch",
  );
  return genesis === undefined ? null : {
    archiveCommitOrdinal: genesis.payload.witness.archiveCommitOrdinal,
    frontierSha256: genesis.payload.witness.catalogFrontierSha256,
  };
}

// derive the sole custody intent without a matching acknowledgement
function derivePendingRevisionCustody(records) {
  const intents = new Map();

  // replay only the two closed custody transitions
  for (const record of records) {
    // retain every immutable page intent until its acknowledgement appears
    if (record.kind === "revision_custody_intent") {
      intents.set(record.payload.request.pageSha256, record);
    }

    // close only the exact acknowledged page
    if (record.kind === "revision_custody_completed" ||
      record.kind === "revision_custody_completed_v2") {
      intents.delete(revisionCustodyCompletionPageSha256(record));
    }
  }

  // reject journal history that claims concurrent page custody
  if (intents.size > 1) {
    throw stateError("revision_custody_history_invalid");
  }
  return intents.values().next().value ?? null;
}

// project either immutable custody completion version onto its page identity
function revisionCustodyCompletionPageSha256(record) {
  if (record.kind === "revision_custody_completed") {
    return record.payload.pageSha256;
  }
  if (record.kind === "revision_custody_completed_v2") {
    return record.payload.acknowledgement.pageSha256;
  }
  return null;
}

// project one immutable custody intent without journal metadata
function projectRevisionCustodyIntent(payload) {
  return {
    nextArchiveCommitOrdinal: payload.nextArchiveCommitOrdinal,
    nextFrontierSha256: payload.nextFrontierSha256,
    request: Object.freeze({ ...payload.request }),
  };
}

// validate one exact ten-operand successful-page acknowledgement request
function validateRevisionCustodyRequest(value) {
  const proofKey = Object.hasOwn(value, "custodyCheckpointSha256")
    ? "custodyCheckpointSha256" : "graphManifestSha256";
  requireExactKeys(value, [
    "afterArchiveCommitOrdinal", "afterFrontierSha256", proofKey,
    "memberRootSha256", "pageSha256", "previousPageSha256", "startMemberSha256",
    "startSha256", "watermarkArchiveCommitOrdinal", "watermarkFrontierSha256",
  ], "revision custody request");
  for (const name of ["afterArchiveCommitOrdinal", "watermarkArchiveCommitOrdinal"]) {
    requireArchiveOrdinal(value[name], name);
  }
  for (const name of [
    "afterFrontierSha256", proofKey, "memberRootSha256", "pageSha256",
    "previousPageSha256", "startSha256", "watermarkFrontierSha256",
  ]) {
    requireSha256(value[name], name);
  }
  requireNullableSha256(value.startMemberSha256, "startMemberSha256");

  // keep the requested predecessor within its immutable transfer watermark
  if (BigInt(value.afterArchiveCommitOrdinal) > BigInt(value.watermarkArchiveCommitOrdinal)) {
    throw new TypeError("revision custody request watermark is invalid");
  }
  return Object.freeze({ ...value });
}

// validate one packed page checkpoint without admitting payload bytes
function validateRevisionCustodyPackPage(value) {
  requireExactKeys(value, [
    "memberRootSha256", "pageSha256", "startMemberSha256", "startSha256",
  ], "revision custody pack page");
  for (const name of ["memberRootSha256", "pageSha256", "startSha256"]) {
    requireSha256(value[name], name);
  }
  requireNullableSha256(value.startMemberSha256, "startMemberSha256");
  return Object.freeze({ ...value });
}

// detect one prepared action still bound to a release lease
function hasUnreconciledPreparedAction(records, lease) {
  const prepared = records.find(
    // bind preparation to the exact lease run and due key
    (record) => record.kind === "confirmation_action_prepared" &&
      record.payload.releaseRunId === lease.payload.runId &&
      record.payload.releaseDueKey === lease.payload.dueKey &&
      record.payload.fencingToken === lease.payload.fencingToken,
  );

  // treat absence as an ordinary nonaction release
  if (prepared === undefined) {
    return false;
  }
  return !isConfirmationTerminal(records, prepared.payload.registrationSha256);
}

// enforce monotonic clock trust and expired reconciliation
function enforceTrustedClock(records, now, options = {}) {
  const maxSeenUtc = deriveMaxSeenUtc(records);

  // stop acquisitions after a material clock rollback
  if (maxSeenUtc !== null &&
    Date.parse(now) < Date.parse(maxSeenUtc) - CLOCK_ROLLBACK_TOLERANCE_MS) {
    throw stateError("clock_untrusted");
  }

  // force reconciliation for any expired active lease after a forward jump
  if (options.allowExpired !== true) {
    const expired = [...deriveActiveLeases(records).values()].some(
      // detect any lease whose maximum interval passed
      (record) => Date.parse(record.payload.expiresAt) <= Date.parse(now),
    );

    // refuse blind takeover after a forward jump
    if (expired) {
      throw stateError("reconciliation_required");
    }
  }
}

// derive the greatest observed journal instant
function deriveMaxSeenUtc(records) {
  return records.reduce(
    // retain the later canonical timestamp
    (maximum, record) => maximum === null || record.recordedAt > maximum
      ? record.recordedAt
      : maximum,
    null,
  );
}

// allocate one monotonic unsigned release fencing token
function allocateFencingToken(records) {
  const maximum = deriveMaximumFencingToken(records);

  // stop permanently rather than wrap
  if (maximum === ADJUSTMENT_MAXIMUM_FENCING_TOKEN) {
    throw stateError("fencing_token_exhausted");
  }
  return maximum + 1n;
}

// derive the greatest durable fencing token
function deriveMaximumFencingToken(records) {
  return records.reduce(
    // retain only release lease token maxima
    (maximum, record) => record.kind === "lease_acquired" &&
      record.payload.fencingToken !== null && BigInt(record.payload.fencingToken) > maximum
      ? BigInt(record.payload.fencingToken)
      : maximum,
    0n,
  );
}

// return one idempotent unique mutation
function uniqueMutation(records, event) {
  const existing = records.find(
    // find one exact logical record key
    (record) => record.recordKey === event.recordKey,
  );

  // reuse only an exact immutable event
  if (existing !== undefined) {
    const payloadSha256 = sha256(canonicalJsonBytes(event.payload));

    // reject record-key collision
    if (existing.kind !== event.kind || existing.dueKey !== event.dueKey ||
      existing.payloadSha256 !== payloadSha256) {
      throw stateError("journal_record_collision");
    }
    return { records: [], result: existing };
  }
  return { records: [event], result: { status: "appended" } };
}

// recognize either target or compensation release preparation
function isModelReleasePreparation(record) {
  return record.kind === "model_release_prepared" ||
    record.kind === "model_compensating_release_prepared";
}

// find one immutable target or compensation action mapping
function findModelReleasePreparation(records, actionSha256) {
  return records.find(
    // bind the exact action identity across both release roles
    (record) => isModelReleasePreparation(record) &&
      record.payload.actionSha256 === actionSha256,
  );
}

// derive the final collision-adjusted release tag for one preparation
function finalModelReleaseTag(records, prepared) {
  const collisions = records.filter(
    // retain ordered collisions for the exact prepared action
    (record) => record.kind === "model_release_tag_collision" &&
      record.payload.actionSha256 === prepared.payload.actionSha256,
  );
  return collisions.at(-1)?.payload.successorReleaseTag ?? prepared.payload.releaseTag;
}

// find one due registration
function findDueRegistration(records, dueKey) {
  return records.find(
    // match one immutable due key
    (record) => record.kind === "due_registered" && record.dueKey === dueKey,
  );
}

// find one due completion
function findDueCompletion(records, dueKey) {
  return records.find(
    // match one terminal due key
    (record) => record.kind === "due_completed" && record.dueKey === dueKey,
  );
}

// require one lifecycle genesis
function requireLifecycleGenesis(records) {
  // refuse a v2 lifecycle without its immutable v1 tail
  if (!records.some((record) => record.kind === "lifecycle_genesis")) {
    throw stateError("lifecycle_genesis_missing");
  }
}

// select one family's confirmation records
function confirmationRecords(records, family) {
  return records.filter(
    // retain records carrying the exact family registration
    (record) => record.payload.family === family ||
      record.dueKey?.startsWith(`confirmation/${family}/`) === true,
  );
}

// find one active family registration
function findActiveConfirmation(records) {
  const registrations = records.filter(
    // select preregistered confirmation members
    (record) => record.kind === "confirmation_registered",
  );

  return registrations.find(
    // retain a registration without a reconciled terminal outcome
    (registration) => !isConfirmationTerminal(records, registration.payload.registrationSha256),
  ) ?? null;
}

// recognize only durable no-action or reconciled action terminals
function isConfirmationTerminal(records, registrationSha256) {
  const result = findConfirmationKind(records, registrationSha256, "confirmation_result");

  // retain every member without a final policy result
  if (result === undefined) {
    return false;
  }

  // close genuine no-action results immediately
  if (result.payload.disposition !== "promoted") {
    return result.payload.actionIdentitySha256 === null &&
      result.payload.actionState === "terminal_no_action";
  }

  // retain promotion ownership until ack or compensation
  return findConfirmationActionKind(
    records,
    registrationSha256,
    "confirmation_action_acknowledged",
  ) !== undefined || findConfirmationActionKind(
    records,
    registrationSha256,
    "confirmation_action_compensation_result",
  ) !== undefined;
}

// find one exact registration
function findRegistration(records, family, registrationSha256) {
  return records.find(
    // bind family and registration identity
    (record) => record.kind === "confirmation_registered" &&
      record.payload.family === family &&
      record.payload.registrationSha256 === registrationSha256,
  );
}

// project one journal-owned confirmation lifecycle without exposing value payloads
function projectConfirmationLifecycle(records, registration) {
  // preserve an unknown registration as an explicit absent projection
  if (registration === undefined || registration === null) {
    return null;
  }
  const registrationSha256 = registration.payload.registrationSha256;
  const burn = findConfirmationKind(records, registrationSha256, "confirmation_burned");
  const chunks = confirmationChunks(records, registrationSha256).map(
    // expose only the value-free chunk metadata
    (record) => structuredClone(record.payload.chunk),
  );
  const complete = findConfirmationKind(
    records,
    registrationSha256,
    "confirmation_member_complete",
  );
  const result = findConfirmationKind(records, registrationSha256, "confirmation_result");
  const fullManifest = complete === undefined || burn === undefined
    ? null
    : assembleConfirmationManifest({
        access: burn.payload,
        chunks,
        registration: registration.payload,
      });
  return Object.freeze({
    access: burn === undefined ? null : structuredClone(burn.payload),
    chunks: Object.freeze(chunks),
    fullManifest,
    registration: structuredClone(registration.payload),
    result: result === undefined ? null : structuredClone(result.payload),
    state: result !== undefined
      ? "terminal"
      : complete !== undefined
        ? "complete"
        : burn !== undefined ? "burned" : "registered",
  });
}

// find one confirmation-scoped kind
function findConfirmationKind(records, registrationSha256, kind) {
  return records.find(
    // bind kind and registration identity
    (record) => record.kind === kind &&
      record.payload.registrationSha256 === registrationSha256,
  );
}

// find one action transition bound to a registration
function findConfirmationActionKind(records, registrationSha256, kind) {
  return records.find(
    // bind the exact action transition
    (record) => record.kind === kind &&
      record.payload.registrationSha256 === registrationSha256,
  );
}

// validate one action-stage identity envelope
function validateActionIdentityInput(input) {
  requireFamily(input.family);
  requireInstant(input.now, "now");
  requireSha256(input.registrationSha256, "registrationSha256");
  requireSha256(input.actionIdentitySha256, "actionIdentitySha256");
  requireFencingToken(input.fencingToken);
}

// require one promoted member and exact action identity
function requirePromotedAction(records, input) {
  const registration = findRegistration(records, input.family, input.registrationSha256);
  const result = findConfirmationKind(records, input.registrationSha256, "confirmation_result");

  // prohibit unknown, cross-family or nonpromotion actions
  if (registration === undefined || result === undefined ||
    result.payload.family !== input.family || result.payload.disposition !== "promoted" ||
    result.payload.actionIdentitySha256 !== input.actionIdentitySha256) {
    throw stateError("confirmation_action_unknown");
  }
  return { registration, result };
}

// require one exact prepared action and fence
function requirePreparedAction(records, input) {
  const context = requirePromotedAction(records, input);
  const prepared = findConfirmationActionKind(
    records,
    input.registrationSha256,
    "confirmation_action_prepared",
  );

  // bind all later transitions to the original fence
  if (prepared === undefined || prepared.payload.actionIdentitySha256 !== input.actionIdentitySha256 ||
    prepared.payload.fencingToken !== input.fencingToken) {
    throw stateError("confirmation_action_not_prepared");
  }
  return { ...context, prepared };
}

// require one exact applied action
function requireAppliedAction(records, input) {
  const context = requirePreparedAction(records, input);
  const applied = findConfirmationActionKind(
    records,
    input.registrationSha256,
    "confirmation_action_applied",
  );

  // prohibit verification before apply
  if (applied === undefined) {
    throw stateError("confirmation_action_not_applied");
  }
  return { ...context, applied };
}

// require one exact verified action
function requireVerifiedAction(records, input) {
  const context = requireAppliedAction(records, input);
  const verified = findConfirmationActionKind(
    records,
    input.registrationSha256,
    "confirmation_action_verified",
  );

  // prohibit acknowledgement before verification
  if (verified === undefined) {
    throw stateError("confirmation_action_not_verified");
  }
  return { ...context, verified };
}

// project immutable promotion identity into one action stage
function actionLifecycleContext(registration, result, action) {
  return {
    ...lifecycleContext(registration, "opened", action.actionState),
    fullMemberRootSha256: result.fullMemberRootSha256,
    candidateReportSha256: result.candidateReportSha256,
    actionIdentitySha256: result.actionIdentitySha256,
    disposition: result.disposition,
    nextConfirmationEligibleAt: result.nextConfirmationEligibleAt,
    fencingToken: action.fencingToken,
  };
}

// create one registration-scoped action event
function confirmationActionEvent(registration, kind, suffix, payload, now) {
  return {
    dueKey: registration.dueKey,
    kind,
    payload,
    recordKey: `confirmation/${payload.family}/${payload.registrationSha256}/action/${payload.actionIdentitySha256}/${suffix}`,
    recordedAt: now,
  };
}

// append or reuse one exact action transition
function appendConfirmationActionTransition(records, options) {
  const registration = findRegistration(
    records,
    options.input.family,
    options.input.registrationSha256,
  );
  const event = confirmationActionEvent(
    registration,
    options.kind,
    options.suffix,
    options.payload,
    options.input.now,
  );
  const existing = findConfirmationActionKind(
    records,
    options.input.registrationSha256,
    options.kind,
  );

  // preserve one immutable transition per action stage
  if (existing !== undefined) {
    return exactActionRetry(existing, event);
  }

  // prohibit new stages after durable reconciliation
  if (findConfirmationActionKind(
    records,
    options.input.registrationSha256,
    "confirmation_action_acknowledged",
  ) !== undefined || findConfirmationActionKind(
    records,
    options.input.registrationSha256,
    "confirmation_action_compensation_result",
  ) !== undefined) {
    throw stateError("confirmation_action_already_reconciled");
  }
  return { records: [event], result: options.payload };
}

// reuse only one exact action event payload
function exactActionRetry(existing, event) {
  const payloadSha256 = sha256(canonicalJsonBytes(event.payload));

  // reject altered evidence on an idempotent retry
  if (existing.kind !== event.kind || existing.dueKey !== event.dueKey ||
    existing.payloadSha256 !== payloadSha256) {
    throw stateError("confirmation_action_transition_collision");
  }
  return { records: [], result: existing.payload };
}

// list ordered confirmation chunk records
function confirmationChunks(records, registrationSha256) {
  return records.filter(
    // select exact registration chunks
    (record) => record.kind === "confirmation_chunk" &&
      record.payload.registrationSha256 === registrationSha256,
  ).sort(
    // restore deterministic chunk order
    (left, right) => compareConfirmationChunks(left.payload.chunk, right.payload.chunk),
  );
}

// project one stable journal key for either closed chunk grammar
function confirmationChunkKey(chunk) {
  return Object.hasOwn(chunk, "contractVersion")
    ? `${chunk.logicalChunkIndex}/${chunk.partIndex}`
    : String(chunk.chunkIndex);
}

// compare logical and physical part coordinates without mixing versions
function compareConfirmationChunks(left, right) {
  const leftV3 = Object.hasOwn(left, "contractVersion");
  const rightV3 = Object.hasOwn(right, "contractVersion");

  // keep mixed histories visibly invalid rather than interleaving them
  if (leftV3 !== rightV3) {
    return leftV3 ? 1 : -1;
  }
  return leftV3
    ? left.logicalChunkIndex - right.logicalChunkIndex || left.partIndex - right.partIndex
    : left.chunkIndex - right.chunkIndex;
}

// require the exact next coordinate under one immutable chunk grammar
function confirmationChunkFollows(previous, current) {
  const currentV3 = Object.hasOwn(current, "contractVersion");

  // start only at the first logical and physical coordinate
  if (previous === null) {
    return currentV3
      ? current.logicalChunkIndex === 0 && current.partIndex === 0
      : current.chunkIndex === 0;
  }
  const previousV3 = Object.hasOwn(previous, "contractVersion");

  // never splice additive parts into a legacy confirmation member
  if (previousV3 !== currentV3) {
    return false;
  }
  if (!currentV3) {
    return current.chunkIndex === previous.chunkIndex + 1;
  }
  const sameLogicalChunk = current.logicalChunkIndex === previous.logicalChunkIndex;

  // finish every declared part before advancing the logical window
  if (sameLogicalChunk) {
    return current.partCount === previous.partCount &&
      current.partIndex === previous.partIndex + 1 &&
      current.captureLocalDate > previous.captureLocalDate &&
      current.fromLocalDate === previous.fromLocalDate &&
      current.toLocalDateExclusive === previous.toLocalDateExclusive;
  }
  return previous.partIndex === previous.partCount - 1 && current.partIndex === 0 &&
    current.logicalChunkIndex === previous.logicalChunkIndex + 1;
}

// find the latest record of one kind
function findLatestKind(records, kind) {
  return records.findLast(
    // match one event kind
    (record) => record.kind === kind,
  );
}

// project one complete ledger-v2 lifecycle context
function lifecycleContext(registration, accessState, actionState) {
  return {
    contractVersion: ADJUSTMENT_LIFECYCLE_LEDGER_CONTRACT_VERSION,
    family: registration.family,
    candidateKind: registration.candidateKind,
    candidateSha256: registration.candidateSha256,
    cohortLineageSha256: registration.cohortLineageSha256,
    sourceLineageSha256: registration.sourceLineageSha256,
    reservedKeySha256: registration.reservedKeySha256,
    intervalStartLocalDate: registration.intervalStartLocalDate,
    intervalEndExclusiveLocalDate: registration.intervalEndExclusiveLocalDate,
    firstTargetAt: registration.firstTargetAt,
    terminalAccessAt: registration.terminalAccessAt,
    gateManifestSha256: registration.gateManifestSha256,
    inputHeadSha256: registration.inputHeadSha256,
    registrationSha256: registration.registrationSha256,
    candidateReportSha256: null,
    actionIdentitySha256: null,
    accessState,
    actionState,
  };
}

// validate registration chronology and identities
function validateConfirmationRegistrationInput(input) {
  requireFamily(input.family);

  // require the one family-specific candidate kind
  if (input.candidateKind !== FAMILY_CANDIDATE_KINDS.get(input.family)) {
    throw new TypeError("candidateKind is invalid");
  }
  requireSha256(input.candidateSha256, "candidateSha256");
  requireSha256(input.cohortLineageSha256, "cohortLineageSha256");
  requireSha256(input.gateManifestSha256, "gateManifestSha256");
  requireSha256(input.inputHeadSha256, "inputHeadSha256");
  requireSha256(input.reservedKeySha256, "reservedKeySha256");
  requireSha256(input.sourceLineageSha256, "sourceLineageSha256");
  requireInstant(input.firstTargetAt, "firstTargetAt");
  requireInstant(input.now, "now");
  requireInstant(input.terminalAccessAt, "terminalAccessAt");
  requireLocalDate(input.intervalStartLocalDate, "intervalStartLocalDate");
  requireLocalDate(input.intervalEndExclusiveLocalDate, "intervalEndExclusiveLocalDate");

  // require preregistration before first target and access after it
  if (Date.parse(input.now) >= Date.parse(input.firstTargetAt) ||
    Date.parse(input.terminalAccessAt) <= Date.parse(input.firstTargetAt) ||
    daysBetween(input.intervalStartLocalDate, input.intervalEndExclusiveLocalDate) !==
      CONFIRMATION_INTERVAL_DAYS.get(input.family)) {
    throw stateError("confirmation_chronology_invalid");
  }

}

// validate one value-blind chunk manifest
function validateConfirmationChunk(chunk, family) {
  // retain the exact legacy one-payload logical chunk grammar
  if (!Object.hasOwn(chunk, "contractVersion")) {
    requireExactKeys(chunk, [
      "chunkIndex",
      "fromLocalDate",
      "toLocalDateExclusive",
      "chunkSha256",
      "recordCount",
      "expectedKeySubsetSha256",
      "eligiblePredictionSubsetSha256",
      "missingKeySubsetSha256",
    ], "confirmation chunk");
    const count = CONFIRMATION_CHUNK_COUNTS.get(family);

    // require one exact legacy family chunk index and bounded reference count
    if (!Number.isInteger(chunk.chunkIndex) || chunk.chunkIndex < 0 ||
      chunk.chunkIndex >= count || !Number.isSafeInteger(chunk.recordCount) ||
      chunk.recordCount < 0 || chunk.recordCount > 8_192) {
      throw new TypeError("confirmation chunk bounds are invalid");
    }
    requireLocalDate(chunk.fromLocalDate, "fromLocalDate");
    requireLocalDate(chunk.toLocalDateExclusive, "toLocalDateExclusive");
    requireSha256(chunk.chunkSha256, "chunkSha256");
    requireSha256(chunk.expectedKeySubsetSha256, "expectedKeySubsetSha256");
    requireSha256(chunk.eligiblePredictionSubsetSha256, "eligiblePredictionSubsetSha256");
    requireSha256(chunk.missingKeySubsetSha256, "missingKeySubsetSha256");
    return { ...chunk };
  }
  requireExactKeys(chunk, [
    "captureLocalDate",
    "contractVersion",
    "eligiblePredictionSubsetSha256",
    "expectedKeyCount",
    "expectedKeySubsetSha256",
    "fromLocalDate",
    "logicalChunkIndex",
    "missingKeyCount",
    "missingKeySubsetSha256",
    "partCount",
    "partIndex",
    "partSha256",
    "recordCount",
    "toLocalDateExclusive",
  ], "confirmation chunk part");
  const count = CONFIRMATION_CHUNK_COUNTS.get(family);

  // require one exact bounded part without selecting evidence from its outcome
  if (chunk.contractVersion !== ADJUSTMENT_CONFIRMATION_CHUNK_PART_V3_VERSION ||
    !Number.isInteger(chunk.logicalChunkIndex) || chunk.logicalChunkIndex < 0 ||
    chunk.logicalChunkIndex >= count || !Number.isInteger(chunk.partIndex) ||
    chunk.partIndex < 0 || !Number.isInteger(chunk.partCount) || chunk.partCount < 1 ||
    chunk.partCount > 24 || chunk.partIndex >= chunk.partCount ||
    !Number.isSafeInteger(chunk.recordCount) || chunk.recordCount < 0 ||
    chunk.recordCount > 8_192 || !Number.isSafeInteger(chunk.expectedKeyCount) ||
    chunk.expectedKeyCount < 1 || chunk.expectedKeyCount > 8_192 ||
    !Number.isSafeInteger(chunk.missingKeyCount) || chunk.missingKeyCount < 0 ||
    chunk.recordCount + chunk.missingKeyCount !== chunk.expectedKeyCount) {
    throw new TypeError("confirmation chunk bounds are invalid");
  }
  requireLocalDate(chunk.captureLocalDate, "captureLocalDate");
  requireLocalDate(chunk.fromLocalDate, "fromLocalDate");
  requireLocalDate(chunk.toLocalDateExclusive, "toLocalDateExclusive");
  requireSha256(chunk.partSha256, "partSha256");
  requireSha256(chunk.expectedKeySubsetSha256, "expectedKeySubsetSha256");
  requireSha256(chunk.eligiblePredictionSubsetSha256, "eligiblePredictionSubsetSha256");
  requireSha256(chunk.missingKeySubsetSha256, "missingKeySubsetSha256");
  return { ...chunk };
}

// reject value-bearing fields from blinded journal events
function assertValueBlind(value) {
  const stack = [value];

  // inspect every nested object without parsing designated values
  while (stack.length > 0) {
    const current = stack.pop();

    // traverse array containers only
    if (Array.isArray(current)) {
      stack.push(...current);
      continue;
    }

    // ignore scalar identity metadata
    if (current === null || typeof current !== "object") {
      continue;
    }

    // reject forbidden keys and traverse metadata values
    for (const [key, nested] of Object.entries(current)) {
      // reject body, row, payload and value exposure
      if (FORBIDDEN_VALUE_KEYS.test(key)) {
        throw stateError("blinded_value_exposure_refused");
      }
      stack.push(nested);
    }
  }
}

// classify one lease scope into a fixed duration
function leaseScopeClass(scope) {
  // require one exact supported scope grammar
  if (scope === "archive" || scope === "daily" || scope === "release") {
    return scope;
  }

  // recognize family-scoped monthly and confirmation leases
  if (/^monthly\/(?:temperature|wind|rain)$/u.test(scope)) {
    return "monthly";
  }

  // recognize one confirmation family scope
  if (/^confirmation\/(?:temperature|wind|rain)$/u.test(scope)) {
    return "confirmation";
  }
  throw new TypeError("lease scope is invalid");
}

// wait until the flock-held command confirms acquisition
async function waitForLockHolder(child) {
  return await new Promise(
    // settle on readiness, error or early exit
    (resolvePromise, rejectPromise) => {
      let output = "";

      // collect the bounded readiness word
      child.stdout.on("data", (chunk) => {
        output += chunk.toString("ascii");

        // resolve only the exact lock readiness marker
        if (output === "ready") {
          resolvePromise(output);
        }
      });
      child.once("error", rejectPromise);
      child.once("exit", () => resolvePromise(output));
    },
  );
}

// wait for one child process exit
async function waitForExit(child) {
  // return immediately after an already observed exit
  if (child.exitCode !== null) {
    return child.exitCode;
  }
  return await new Promise(
    // resolve one process exit
    (resolvePromise, rejectPromise) => {
      child.once("error", rejectPromise);
      child.once("exit", resolvePromise);
    },
  );
}

// atomically replace one owner-private state file
async function writeAtomicPrivateFile(path, bytes) {
  const temporaryPath = join(dirname(path), `.state-${randomUUID()}.tmp`);
  const handle = await open(
    temporaryPath,
    fsConstants.O_CREAT | fsConstants.O_EXCL | fsConstants.O_WRONLY | fsConstants.O_NOFOLLOW,
    0o600,
  );

  try {
    await handle.writeFile(bytes);
    await handle.sync();
  } finally {
    await handle.close();
  }
  await rename(temporaryPath, path);
}

// fsync one held private directory
async function syncDirectory(path) {
  const handle = await open(
    path,
    fsConstants.O_RDONLY | fsConstants.O_DIRECTORY | fsConstants.O_NOFOLLOW,
  );

  try {
    await handle.sync();
  } finally {
    await handle.close();
  }
}

// prove one owner-private directory
async function assertPrivateDirectory(path) {
  const details = await lstat(path);

  // reject links, foreign ownership and broad access
  if (!details.isDirectory() || details.isSymbolicLink() ||
    details.uid !== process.getuid() || (details.mode & 0o777) !== 0o700) {
    throw stateError("state_directory_refused");
  }
}

// prove one owner-private single-link file
async function assertPrivateFile(path) {
  const details = await lstat(path);

  // reject links, special files, foreign ownership and broad access
  if (!details.isFile() || details.isSymbolicLink() || details.nlink !== 1 ||
    details.uid !== process.getuid() || (details.mode & 0o777) !== 0o600) {
    throw stateError("state_file_refused");
  }
  return details;
}

// calculate one local-date delta
function daysBetween(start, end) {
  return (Date.parse(`${end}T00:00:00.000Z`) - Date.parse(`${start}T00:00:00.000Z`)) /
    (24 * 60 * 60 * 1_000);
}

// add exact calendar days to one local-date label
function addLocalDates(start, days) {
  return new Date(Date.parse(`${start}T00:00:00.000Z`) + days * 24 * 60 * 60 * 1_000)
    .toISOString().slice(0, 10);
}

// detect overlap between two local-date intervals
function localDateIntervalsOverlap(leftStart, leftEnd, rightStart, rightEnd) {
  return leftStart < rightEnd && rightStart < leftEnd;
}

// validate one fully published qualified release recovery transaction
function validateQualifiedModelReleaseTransactionInput(input) {
  requireExactKeys(input, [
    "compensation", "compensationAction", "family", "finalizationProof", "now",
    "registrationSha256", "releaseRequest", "target", "targetAction",
  ], "qualified model release transaction");
  requireFamily(input.family);
  requireInstant(input.now, "now");
  requireSha256(input.registrationSha256, "registrationSha256");
  requirePlainObject(input.targetAction, "targetAction");
  requirePlainObject(input.compensationAction, "compensationAction");
  requirePlainObject(input.finalizationProof, "finalizationProof");
  requireExactKeys(input.targetAction, MODEL_ACTION_KEYS, "target action");
  requireExactKeys(input.compensationAction, MODEL_ACTION_KEYS, "compensation action");
  requireExactKeys(input.target, ["actionSha256", "commitSha", "releaseTag"],
    "qualified target release");
  requireExactKeys(input.compensation, ["actionSha256", "commitSha", "releaseTag"],
    "qualified compensation release");
  requireExactKeys(input.releaseRequest, FAMILY_RELEASE_REQUEST_KEYS,
    "qualified family release request");
  const targetActionSha256 = sha256(canonicalJsonBytes(input.targetAction));
  const compensationActionSha256 = sha256(canonicalJsonBytes(input.compensationAction));

  // bind the two manifests, immutable refs and finalization authority
  if (input.targetAction.contractVersion !== "forecast-adjustment-model-action/v1" ||
    input.targetAction.actionKind !== "promote" ||
    input.targetAction.family !== input.family ||
    input.targetAction.predecessorActionSha256 !== null ||
    input.compensationAction.contractVersion !== "forecast-adjustment-model-action/v1" ||
    !["compensate_incumbent", "compensate_raw"].includes(
      input.compensationAction.actionKind,
    ) || input.compensationAction.family !== input.family ||
    input.compensationAction.predecessorActionSha256 !== targetActionSha256 ||
    input.compensationAction.fencingToken !== input.targetAction.fencingToken ||
    input.target.actionSha256 !== targetActionSha256 ||
    input.compensation.actionSha256 !== compensationActionSha256 ||
    input.compensationAction.expectedSourceCommit !== input.target.commitSha ||
    input.compensationAction.expectedSourceRelease !== input.target.releaseTag ||
    input.finalizationProof.contractVersion !==
      "adjustment-maintenance-finalization-proof/v3" ||
    input.finalizationProof.actionSha256 !== targetActionSha256 ||
    input.finalizationProof.family !== input.family ||
    input.finalizationProof.sourceCommit !== input.targetAction.expectedSourceCommit ||
    input.releaseRequest.family !== input.family ||
    input.releaseRequest.fencingToken !== input.targetAction.fencingToken ||
    input.releaseRequest.reportSha256 !== input.targetAction.policyReportSha256 ||
    input.releaseRequest.expectedCurrentRelease !==
      input.targetAction.expectedSourceRelease ||
    input.releaseRequest.expectedSourceRelease !==
      input.targetAction.expectedSourceRelease ||
    input.releaseRequest.expectedSettingsSha256 !==
      input.targetAction.expectedSettingsSha256) {
    throw new TypeError("qualified model release transaction identity differs");
  }
  for (const value of [input.target.actionSha256, input.compensation.actionSha256,
    input.releaseRequest.actionSha256, input.releaseRequest.reportSha256,
    input.releaseRequest.expectedSettingsSha256]) {
    requireSha256(value, "qualified release identity");
  }
  for (const value of [input.target.commitSha, input.compensation.commitSha,
    input.targetAction.expectedSourceCommit, input.compensationAction.expectedSourceCommit]) {
    requireGitCommit(value, "qualified release commit");
  }
  for (const value of [input.target.releaseTag, input.compensation.releaseTag,
    input.releaseRequest.targetRelease, input.releaseRequest.compensatingRelease,
    input.releaseRequest.expectedCurrentRelease, input.releaseRequest.expectedSourceRelease]) {
    requireReleaseTag(value);
  }
  requireFencingToken(input.targetAction.fencingToken);
}

// validate one value-free completed daily outcome
function validateDueTerminalOutcome(value) {
  requireExactKeys(value, [
    "actionEligible", "candidateGraphSha256", "candidateSha256", "fitReceiptSha256",
    "reason", "semanticInputSha256", "servingChanged", "state",
  ], "due terminal outcome");
  requireSha256(value.semanticInputSha256, "semanticInputSha256");

  // retain only final daily outcomes with no candidate-value exposure
  if (value.state !== "completed" || value.candidateGraphSha256 !== null ||
    value.candidateSha256 !== null || value.fitReceiptSha256 !== null ||
    typeof value.actionEligible !== "boolean" ||
    typeof value.servingChanged !== "boolean" ||
    !new Set([
      "daily_candidate_promoted", "daily_candidate_rejected", "daily_support_failed",
    ]).has(value.reason) ||
    (value.reason === "daily_candidate_promoted") !== value.actionEligible ||
    value.reason !== "daily_candidate_promoted" && value.servingChanged) {
    throw new TypeError("due terminal outcome is invalid");
  }
}

// encode recursively sorted canonical JSON
function canonicalJsonBytes(value) {
  return Buffer.from(`${JSON.stringify(canonicalValue(value))}\n`, "utf8");
}

// recursively normalize canonical JSON values
function canonicalValue(value) {
  // retain JSON scalar values
  if (value === null || typeof value === "string" || typeof value === "boolean") {
    return value;
  }

  // retain only finite nonnegative-zero numbers
  if (typeof value === "number") {
    // reject nonfinite and negative-zero encodings
    if (!Number.isFinite(value) || Object.is(value, -0)) {
      throw new TypeError("canonical JSON number is invalid");
    }
    return value;
  }

  // preserve semantic array order
  if (Array.isArray(value)) {
    return value.map(
      // normalize one array member
      (entry) => canonicalValue(entry),
    );
  }

  requirePlainObject(value, "canonical JSON object");
  return Object.fromEntries(Object.keys(value).sort().map(
    // sort and normalize each object field
    (key) => [key, canonicalValue(value[key])],
  ));
}

// calculate one lowercase SHA-256 digest
function sha256(value) {
  return createHash("sha256").update(value).digest("hex");
}

// create one categorical state refusal
function stateError(reason) {
  const error = new Error(reason);
  error.code = "adjustment_state_refused";
  error.reason = reason;
  return error;
}

// require one family
function requireFamily(value) {
  // reject cross-family aliases
  if (!FAMILY_CANDIDATE_KINDS.has(value)) {
    throw new TypeError("family is invalid");
  }
}

// require one canonical local date
function requireLocalDate(value, label) {
  // reject noncanonical or impossible dates
  if (typeof value !== "string" || !LOCAL_DATE_PATTERN.test(value) ||
    new Date(`${value}T00:00:00.000Z`).toISOString().slice(0, 10) !== value) {
    throw new TypeError(`${label} is invalid`);
  }
}

// require one canonical UTC millisecond instant
function requireInstant(value, label) {
  // reject noncanonical or impossible instants
  if (typeof value !== "string" || !INSTANT_PATTERN.test(value) ||
    new Date(value).toISOString() !== value) {
    throw new TypeError(`${label} is invalid`);
  }
}

// require one lowercase SHA-256 value
function requireSha256(value, label) {
  // reject noncanonical hashes
  if (typeof value !== "string" || !SHA256_PATTERN.test(value)) {
    throw new TypeError(`${label} is invalid`);
  }
}

// require one nullable SHA-256 value
function requireNullableSha256(value, label) {
  // validate only present hashes
  if (value !== null) {
    requireSha256(value, label);
  }
}

// require one canonical positive unsigned 64-bit fence
function requireFencingToken(value) {
  // reject zero, leading zeros and unsigned overflow
  if (typeof value !== "string" || !/^[1-9][0-9]{0,19}$/u.test(value) ||
    BigInt(value) > ADJUSTMENT_MAXIMUM_FENCING_TOKEN) {
    throw new TypeError("fencingToken is invalid");
  }
}

// require one canonical due key
function requireDueKey(value) {
  // reject path escape and unbounded due keys
  if (typeof value !== "string" || value.length > 256 ||
    !/^(?:archive|capture|daily|monthly|control-reference|confirmation|release|rollback)\/[a-zA-Z0-9:._/-]+$/u.test(value) ||
    value.includes("..") || value.includes("//")) {
    throw new TypeError("dueKey is invalid");
  }
}

// validate one root-returned terminal tombstone without accepting extra authority
function validateOwnerTerminalTombstone(value) {
  requireExactKeys(value, [
    "contractVersion", "reconciliationSha256", "registrationSha256",
    "terminalResultSha256",
  ], "owner terminal tombstone");
  for (const field of [
    "reconciliationSha256", "registrationSha256", "terminalResultSha256",
  ]) {
    requireSha256(value[field], `owner terminal tombstone ${field}`);
  }

  // retain only the owner contract consumed by the next terminal transaction
  if (value.contractVersion !== "adjustment-shadow-terminal-tombstone/v3") {
    throw new TypeError("owner terminal tombstone contract is invalid");
  }
  return value;
}

// require one exact lowercase git object id
function requireGitCommit(value, label) {
  // reject abbreviated or uppercase commit identities
  if (typeof value !== "string" || !/^[a-f0-9]{40}$/u.test(value)) {
    throw new TypeError(`${label} is invalid`);
  }
}

// require one closed automation ref suffix
function requireReleaseRef(value, label) {
  // reject ambiguous, unsafe, or caller-expanded refs
  if (typeof value !== "string" || value.length > 200 ||
    !/^[a-zA-Z0-9][a-zA-Z0-9._/-]*$/u.test(value) || value.includes("..") ||
    value.includes("//") || value.endsWith("/") || value.endsWith(".")) {
    throw new TypeError(`${label} is invalid`);
  }
}

// require one immutable calendar release tag
function requireReleaseTag(value) {
  // accept only the repository release namespace
  if (typeof value !== "string" || !/^\d{4}\.\d{2}\.\d{2}-[1-9][0-9]*$/u.test(value)) {
    throw new TypeError("releaseTag is invalid");
  }
}

// require one nullable due key
function requireNullableDueKey(value) {
  // validate only present due keys
  if (value !== null) {
    requireDueKey(value);
  }
}

// require one bounded run identity
function requireRunId(value) {
  // reject ambiguous run identifiers
  if (typeof value !== "string" || !/^[a-zA-Z0-9][a-zA-Z0-9._-]{0,127}$/u.test(value)) {
    throw new TypeError("runId is invalid");
  }
}

// require one canonical database archive ordinal within bigint range
function requireArchiveOrdinal(value, label) {
  if (typeof value !== "string" || !/^(?:0|[1-9]\d{0,18})$/u.test(value) ||
    BigInt(value) > 0x7fff_ffff_ffff_ffffn) {
    throw new TypeError(`${label} is invalid`);
  }
}

// require one bounded journal record key
function requireRecordKey(value) {
  // reject path traversal and unbounded keys
  if (typeof value !== "string" || value.length > 512 || value.includes("..") ||
    !/^[a-zA-Z0-9][a-zA-Z0-9:._/-]*$/u.test(value)) {
    throw new TypeError("recordKey is invalid");
  }
}

// require one plain object
function requirePlainObject(value, label) {
  // reject null, arrays and custom prototypes
  if (value === null || Array.isArray(value) || typeof value !== "object" ||
    Object.getPrototypeOf(value) !== Object.prototype) {
    throw new TypeError(`${label} must be a plain object`);
  }
}

// require one exact closed key set
function requireExactKeys(value, keys, label) {
  requirePlainObject(value, label);
  const actual = Object.keys(value).sort();
  const expected = [...keys].sort();

  // reject unknown or missing fields
  if (actual.length !== expected.length ||
    actual.some((key, index) => key !== expected[index])) {
    throw new TypeError(`${label} has invalid keys`);
  }
}
