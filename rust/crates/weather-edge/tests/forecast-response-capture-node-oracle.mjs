import assert from "node:assert/strict";
import { EventEmitter } from "node:events";
import { readFileSync } from "node:fs";
import vm from "node:vm";

import {
  AdjustmentEvidenceStore,
  normalizeAdjustmentEvidenceWindow,
} from "../../../../deploy/scripts/adjustment-evidence-store.mjs";

const NOW = "2026-11-23T20:10:05.500Z";
const DUE = "capture/2026-11-23T20:10:00.000Z";
const source = readFileSync(new URL("../../../../deploy/scripts/web-server.mjs", import.meta.url), "utf8");
const start = source.indexOf("async function settleScheduledArchiveCapture(");
const end = source.indexOf("// proxy one bounded API request", start);
assert.ok(start >= 0 && end > start);

// execute the original callback and archive settlement for a normal listener fixture
async function trace({ label, search, scheduled, terminal, preparation, result, archive }) {
  const events = [];
  const store = new AdjustmentEvidenceStore({
    now: () => {
      events.push("clock");
      return new Date(NOW);
    },
  });
  // prevent any filesystem or live transport use while retaining the original callback owner
  store.commit = async (prepared, observedAt) => {
    assert.equal(prepared.material, "prepared");
    events.push(`commit:${observedAt}`);
    // preserve an ordinary post-response store error
    if (result === "write_failed") {
      throw new Error("fixture writer unavailable");
    }
    return { status: result };
  };
  const context = vm.createContext({
    scheduledArchiveTransferEnabled: archive !== "disabled",
    adjustmentEvidenceArchiveTransport: archive === "unavailable" ? null : {
      // retain only the already committed exact scheduler material
      captureCommittedSchedulerEvidence: async ({ dueKey, prepared }) => {
        assert.equal(prepared.material, "prepared");
        events.push(`append:${dueKey}`);
        // preserve the ordinary fallback after an archive writer rejection
        if (archive === "error") {
          throw new Error("fixture archive writer unavailable");
        }
        return { status: archive === "gap" ? "evidence_gap_persistence_refused" : "created" };
      },
    },
  });
  const settleArchive = vm.runInContext(
    `${source.slice(start, end)}; settleScheduledArchiveCapture`,
    context,
  );
  const url = new URL(`/api/v1/sites/ballydidean/forecast${search}`, "https://fixture.invalid");
  const window = normalizeAdjustmentEvidenceWindow(url);
  const prepared = preparation ? { material: "prepared" } : null;
  let slotResult = "-";
  const slot = scheduled ? {
    dueKey: DUE,
    settle: (output) => { slotResult = output.status; },
  } : null;
  const response = new EventEmitter();
  await new Promise((resolve) => {
    store.trackResponse(response, prepared, async (settled) => {
      events.push(`evidence:${settled.status}`);
      try {
        await settleArchive(slot, prepared, settled);
      } catch {
        slot?.settle(settled);
      }
      resolve();
    });
    events.push("headers", "body");
    response.emit(terminal);
    // ordinary HTTP completion is followed by socket close
    if (terminal === "finish") {
      response.emit("close");
    }
  });
  process.stdout.write(`${label}=${window ?? "-"}|${events.join(",")}|${slotResult}\n`);
}

for (const parameters of [
  { label: "public", search: "", scheduled: false, terminal: "finish", preparation: true, result: "created", archive: "ready" },
  { label: "scheduled_gap", search: "?window=overnight", scheduled: true, terminal: "finish", preparation: true, result: "duplicate", archive: "gap" },
  { label: "scheduled_error", search: "?days=5", scheduled: true, terminal: "finish", preparation: true, result: "created", archive: "error" },
  { label: "unavailable", search: "?days=10", scheduled: true, terminal: "finish", preparation: true, result: "created", archive: "unavailable" },
  { label: "refused", search: "?days=1", scheduled: false, terminal: "finish", preparation: false, result: "created", archive: "disabled" },
  { label: "aborted", search: "?days=1", scheduled: true, terminal: "close", preparation: true, result: "created", archive: "ready" },
  { label: "write_failed", search: "?days=5", scheduled: true, terminal: "finish", preparation: true, result: "write_failed", archive: "ready" },
]) {
  await trace(parameters);
}
