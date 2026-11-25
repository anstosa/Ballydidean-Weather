import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import vm from "node:vm";

const webSource = await readFile(new URL(
  "../../../../deploy/scripts/web-server.mjs",
  import.meta.url,
), "utf8");
const storeSource = await readFile(new URL(
  "../../../../deploy/scripts/adjustment-evidence-store.mjs",
  import.meta.url,
), "utf8");

// isolate the actual listener without starting its module side effects
const listenerStart = webSource.indexOf(
  "async function serveAdjustmentRevisionArchive(request, response)",
);
const listenerEnd = webSource.indexOf(
  "\n// admit the additive large fixed-gauge body",
  listenerStart,
);
assert.notEqual(listenerStart, -1);
assert.notEqual(listenerEnd, -1);
const listenerSource = webSource.slice(listenerStart, listenerEnd);

const listenerCalls = [];
const listenerContext = {
  // capture the original body ceiling and injected decoded input
  async readRequestJson(request, maximumBytes) {
    listenerCalls.push({ kind: "read", maximumBytes });
    if (request.bodyFailure !== undefined) {
      throw request.bodyFailure;
    }
    return request.input;
  },
  // return an injected archive receipt or failure
  async adjustmentRevisionArchiveHandler(path, input) {
    listenerCalls.push({ input, kind: "dispatch", path });
    if (input?.failure !== undefined) {
      throw input.failure;
    }
    return { operation: input.operation };
  },
  // retain the exact listener status and json body
  sendJson(response, status, body) {
    response.body = body;
    response.status = status;
  },
};
const serveAdjustmentRevisionArchive = vm.runInNewContext(
  `${listenerSource}\nserveAdjustmentRevisionArchive`,
  listenerContext,
  { filename: "web-server.mjs#serveAdjustmentRevisionArchive" },
);

// exercise one ordinary listener request with injected dependencies
async function runListenerCase(name, request) {
  const callOffset = listenerCalls.length;
  const response = {};
  await serveAdjustmentRevisionArchive(request, response);
  return {
    body: response.body,
    calls: listenerCalls.slice(callOffset),
    name,
    status: response.status,
  };
}

const stageTarget = "/internal/adjustment-maintenance/archive/stage";
const spoolFailure = Object.assign(new Error("full"), {
  code: "adjustment_revision_spool_refused",
});
const listenerCases = [
  await runListenerCase("method", {
    headers: { "content-type": "application/json" },
    input: { operation: "stage" },
    method: "GET",
    url: stageTarget,
  }),
  await runListenerCase("target", {
    headers: { "content-type": "application/json" },
    input: { operation: "stage" },
    method: "POST",
    url: "/internal/adjustment-maintenance/archive/unknown",
  }),
  await runListenerCase("media", {
    headers: { "content-type": "text/plain" },
    input: { operation: "stage" },
    method: "POST",
    url: stageTarget,
  }),
  await runListenerCase("ok", {
    headers: { "content-type": " Application/JSON ; charset=utf-8" },
    input: { operation: "stage" },
    method: "POST",
    url: stageTarget,
  }),
  await runListenerCase("body", {
    bodyFailure: new RangeError("large"),
    headers: { "content-type": "application/json" },
    input: { operation: "stage" },
    method: "POST",
    url: stageTarget,
  }),
  await runListenerCase("spool", {
    headers: { "content-type": "application/json" },
    input: { failure: spoolFailure },
    method: "POST",
    url: stageTarget,
  }),
  await runListenerCase("rejected", {
    headers: { "content-type": "application/json" },
    input: { failure: new Error("rejected") },
    method: "POST",
    url: stageTarget,
  }),
];

// isolate the actual route-neutral handler without filesystem operations
const handlerStart = storeSource.indexOf(
  "export function createAdjustmentRevisionArchiveHandler(options = {})",
);
const handlerEnd = storeSource.indexOf(
  "\nconst ADJUSTMENT_FUTURE_INPUT_CLASS_NAMES",
  handlerStart,
);
assert.notEqual(handlerStart, -1);
assert.notEqual(handlerEnd, -1);
const handlerSource = storeSource.slice(handlerStart, handlerEnd).replace(
  "export function createAdjustmentRevisionArchiveHandler",
  "function createAdjustmentRevisionArchiveHandler",
);

class AdjustmentRevisionArchiveStore {
  calls = [];

  // record one injected operation
  record(name, value) {
    this.calls.push({ name, value });
    return { name };
  }

  // stage one injected control state
  stageRainControlState(value) { return this.record("stageRainControlState", value); }
  // record one injected control gap
  recordRainControlStateGap(value) { return this.record("recordRainControlStateGap", value); }
  // record one injected fixed-gauge gap
  recordRainFixedGaugeTargetGap(value) {
    return this.record("recordRainFixedGaugeTargetGap", value);
  }
  // read one injected fixed-gauge status
  readRainFixedGaugeTargetGap(value) {
    return this.record("readRainFixedGaugeTargetGap", value);
  }
  // stage one injected shadow revision
  stageShadowRevision(value) { return this.record("stageShadowRevision", value); }
  // publish one injected shadow revision
  publishShadowRevision(value) { return this.record("publishShadowRevision", value); }
  // record one injected shadow gap
  recordShadowGap(value) { return this.record("recordShadowGap", value); }
  // stage one injected revision projection
  stageProjection(value) { return this.record("stageProjection", value); }
  // publish one injected revision
  publishRevision(value) { return this.record("publishRevision", value); }
  // record one injected revision gap
  recordGap(value) { return this.record("recordGap", value); }
}

const handlerContext = {
  AdjustmentRevisionArchiveStore,
  // preserve the handler's exact-key calls without duplicating parsing
  requireExactKeys(value, keys, label) {
    assert.deepEqual(Object.keys(value).sort(), [...keys].sort(), label);
  },
  // stand in only for the already-tested canonical base64 decoder
  decodeCanonicalBase64(value, label) {
    return { label, value };
  },
};
const createAdjustmentRevisionArchiveHandler = vm.runInNewContext(
  `${handlerSource}\ncreateAdjustmentRevisionArchiveHandler`,
  handlerContext,
  { filename: "adjustment-evidence-store.mjs#createAdjustmentRevisionArchiveHandler" },
);
const store = new AdjustmentRevisionArchiveStore();
const handler = createAdjustmentRevisionArchiveHandler({ store });
const handlerCases = [
  ["/internal/adjustment-maintenance/rain-control-state/stage", { stateBase64: "state" }],
  ["/internal/adjustment-maintenance/rain-control-state/gap", { operation: "state-gap" }],
  ["/internal/adjustment-maintenance/rain-fixed-gauge-target/gap", { operation: "fixed-gap" }],
  ["/internal/adjustment-maintenance/rain-fixed-gauge-target/gap/status", { operation: "fixed-status" }],
  ["/internal/adjustment-maintenance/archive/stage", { operation: "shadow-stage" }],
  ["/internal/adjustment-maintenance/archive/publish", { operation: "shadow-publish" }],
  ["/internal/adjustment-maintenance/archive/gap", { operation: "shadow-gap" }],
  ["/internal/adjustment-maintenance/revision/stage", { projectionBase64: "projection" }],
  ["/internal/adjustment-maintenance/revision/publish", {
    revisionReceipt: { receiptSha256: "single" },
    stageReceipt: { stageReceiptSha256: "stage" },
  }],
  ["/internal/adjustment-maintenance/revision/publish", {
    revisionReceipts: [{ receiptSha256: "batch" }],
    stageReceipt: { stageReceiptSha256: "stage" },
  }],
  ["/internal/adjustment-maintenance/revision/gap", { operation: "revision-gap" }],
];

// execute every supported operation through the extracted handler
for (const [path, input] of handlerCases) {
  await handler(path, input);
}

// read path order directly from each extracted source body
function extractPathOrder(source, pattern) {
  return [...source.matchAll(pattern)].map(
    // retain each source-ordered target
    (match) => match[1],
  );
}

// read one actual store input key list without reimplementing validation
function extractExactInputKeys(label) {
  const labelOffset = storeSource.indexOf(`"${label}"`);
  assert.notEqual(labelOffset, -1);
  const callOffset = storeSource.lastIndexOf("requireExactKeys(input, [", labelOffset);
  assert.notEqual(callOffset, -1);
  const keysOffset = storeSource.indexOf("[", callOffset);
  const keysSource = storeSource.slice(keysOffset + 1, labelOffset);
  return [...keysSource.matchAll(/"([^"]+)"/gu)].map(
    // retain each source-ordered key
    (match) => match[1],
  );
}

const listenerTargets = extractPathOrder(
  listenerSource,
  /"(\/internal\/adjustment-maintenance\/[^"]+)"/gu,
);
const handlerTargets = extractPathOrder(
  handlerSource,
  /path === "(\/internal\/adjustment-maintenance\/[^"]+)"/gu,
);
const shadowGapInputKeys = extractExactInputKeys("adjustment shadow gap input");
const revisionGapInputKeys = extractExactInputKeys("adjustment revision gap input");

// render each recorded handler name
const handlerCallNames = store.calls.map(
  // retain the called storage operation
  ({ name }) => name,
);
// render each listener outcome
const listenerLines = listenerCases.map((fixture) => {
  const result = fixture.body.error ?? fixture.body.operation;
  const calls = fixture.calls.map(
    // preserve request-read and dispatch order
    (call) => call.kind === "read"
      ? `read:${call.maximumBytes}`
      : `dispatch:${call.path}`,
  ).join(",");
  return `${fixture.name}|${fixture.status}|${result}|${calls}`;
});

// render one dependency-free parity signature for the Rust test
const lines = [
  `listener-targets|${listenerTargets.join(",")}`,
  `handler-targets|${handlerTargets.join(",")}`,
  `handler-calls|${handlerCallNames.join(",")}`,
  `gap-inputs|${shadowGapInputKeys.join(",")}|${revisionGapInputKeys.join(",")}`,
  ...listenerLines,
];
process.stdout.write(`${lines.join("\n")}\n`);
