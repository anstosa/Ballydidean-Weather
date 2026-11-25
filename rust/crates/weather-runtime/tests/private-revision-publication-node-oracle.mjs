import { createHash } from "node:crypto";
import { execFile } from "node:child_process";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { promisify } from "node:util";
import { join } from "node:path";

import {
  parseAdjustmentRainFixedGaugeTargetProjection,
  parseAdjustmentRevisionProjectionDocument,
} from "../../../../apps/web/src/adjustment-maintenance-contract.mjs";
import {
  AdjustmentRevisionArchiveStore,
  createAdjustmentRevisionArchiveHandler,
} from "../../../../deploy/scripts/adjustment-evidence-store.mjs";

const ACK_TIME = "2026-10-08T03:00:00.000Z";
const COMMIT_TIME = "2026-10-08T01:00:00.000Z";
const PRODUCER_ORACLE = fileURLToPath(
  new URL("./producer-projection-node-oracle.mjs", import.meta.url),
);
const execFileAsync = promisify(execFile);

// retain the source canonical JSON spelling for requests and expected responses
function canonical(value) {
  // retain array order while canonicalizing its values
  if (Array.isArray(value)) return value.map(canonical);
  // sort every object level exactly as the archive writer does
  if (value !== null && typeof value === "object") {
    return Object.fromEntries(
      Object.keys(value).sort().map((key) => [key, canonical(value[key])]),
    );
  }
  return value;
}

// encode one newline-terminated canonical JSON document
function bytes(value) {
  return Buffer.from(`${JSON.stringify(canonical(value))}\n`);
}

// hash one original receipt identity preimage
function sha(value) {
  return createHash("sha256").update(value).digest("hex");
}

// route through the exact checked-in producer parser selected by contract version
function parseProjection(raw) {
  const version = JSON.parse(raw.toString("utf8")).contractVersion;
  return version === "adjustment-rain-fixed-gauge-target-projection/v1"
    ? parseAdjustmentRainFixedGaugeTargetProjection(raw)
    : parseAdjustmentRevisionProjectionDocument(raw);
}

// synthesize only the database receipt identity from its original fixed preimages
function receiptFor(stage, ordinal, predecessor) {
  const receipt = {
    archiveCommitOrdinal: String(ordinal),
    archiveCommittedAt: new Date(
      Date.parse(COMMIT_TIME) + ordinal * 60_000,
    ).toISOString(),
    contractVersion: "adjustment-revision-commit-receipt/v1",
    frontierSha256: "",
    predecessorFrontierSha256: predecessor,
    projectionIdentitySha256: stage.projectionIdentitySha256,
    projectionKind: stage.projectionKind,
    projectionSha256: stage.projectionSha256,
    receiptSha256: "",
    stageReceiptSha256: stage.stageReceiptSha256,
  };
  receipt.receiptSha256 = sha([
    receipt.contractVersion,
    receipt.archiveCommitOrdinal,
    receipt.archiveCommittedAt,
    receipt.projectionKind,
    receipt.projectionIdentitySha256,
    receipt.projectionSha256,
    receipt.stageReceiptSha256,
    receipt.predecessorFrontierSha256,
  ].join("\n"));
  receipt.frontierSha256 = sha([
    "adjustment-revision-frontier/v1",
    predecessor,
    receipt.archiveCommitOrdinal,
    receipt.receiptSha256,
  ].join("\n"));
  return receipt;
}

// invoke one actual handler request and retain its raw request plus canonical value result
async function recordCase(parent, cases, handler, rootName, name, target, input) {
  const request = Buffer.from(JSON.stringify(input));
  const response = await handler(target, JSON.parse(request.toString("utf8")));
  await writeFile(join(parent, `${name}.request.json`), request);
  await writeFile(join(parent, `${name}.response.json`), bytes(response));
  cases.push([rootName, name, target].join("\t"));
  return response;
}

// publish one ordinary or grouped producer fixture through first writes and exact retries
async function publishFixture(parent, cases, fixtureRoot, rootName, fixtureName) {
  const root = join(parent, `node-${rootName}`);
  await mkdir(root, { mode: 0o700 });
  const store = new AdjustmentRevisionArchiveStore({
    now: () => new Date(ACK_TIME),
    parseProjection,
    root,
  });
  const handler = createAdjustmentRevisionArchiveHandler({ store });
  const projection = await readFile(join(fixtureRoot, `${fixtureName}.json`));
  const stageInput = { projectionBase64: projection.toString("base64") };
  const stage = await recordCase(
    parent,
    cases,
    handler,
    rootName,
    `${rootName}-stage-first`,
    "/internal/adjustment-maintenance/revision/stage",
    stageInput,
  );
  await recordCase(
    parent,
    cases,
    handler,
    rootName,
    `${rootName}-stage-retry`,
    "/internal/adjustment-maintenance/revision/stage",
    stageInput,
  );
  const rowCount = parseProjection(projection).rows.length;
  let predecessor = sha("adjustment-revision-frontier/v1\n0\n");
  const receipts = [];
  // reproduce the database's direct global frontier chain in row order
  for (let index = 0; index < rowCount; index += 1) {
    const receipt = receiptFor(stage, index + 1, predecessor);
    receipts.push(receipt);
    predecessor = receipt.frontierSha256;
  }
  const publishInput = receipts.length === 1
    ? { revisionReceipt: receipts[0], stageReceipt: stage }
    : { revisionReceipts: receipts, stageReceipt: stage };
  await recordCase(
    parent,
    cases,
    handler,
    rootName,
    `${rootName}-publish-first`,
    "/internal/adjustment-maintenance/revision/publish",
    publishInput,
  );
  await recordCase(
    parent,
    cases,
    handler,
    rootName,
    `${rootName}-publish-retry`,
    "/internal/adjustment-maintenance/revision/publish",
    publishInput,
  );
}

const parent = process.argv[2];
const fixtureRoot = join(parent, "producer-fixtures");
await mkdir(fixtureRoot, { mode: 0o700 });
await execFileAsync(process.execPath, [PRODUCER_ORACLE, fixtureRoot]);
const cases = [];
await publishFixture(parent, cases, fixtureRoot, "single", "actual");
await publishFixture(parent, cases, fixtureRoot, "grouped", "grouped");
await writeFile(join(parent, "publication-cases.tsv"), `${cases.join("\n")}\n`);
