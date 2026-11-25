import assert from "node:assert/strict";
import { readFile, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { createHttpAdjustmentMaintenanceArchiveRelay } from "../../../../apps/api/dist/adjustment-maintenance-shadow.js";
import { parseAdjustmentRevisionProjectionDocument } from "../../../../apps/web/src/adjustment-maintenance-contract.mjs";
import { AdjustmentRevisionArchiveStore, createAdjustmentRevisionArchiveHandler } from "../../../../deploy/scripts/adjustment-evidence-store.mjs";

const parent = process.argv[2];
const stage = JSON.parse(await readFile(join(parent, "publication-temperature-v1-stage.request.json")));
const stageReceipt = JSON.parse(await readFile(join(parent, "publication-temperature-v1-stage.response.json")));
const compact = JSON.parse(await readFile(join(parent, "publication-temperature-v1-publish.request.json")));
const handler = createAdjustmentRevisionArchiveHandler({ store: new AdjustmentRevisionArchiveStore({
  root: join(parent, "node-publication-temperature-v1"), parseProjection: parseAdjustmentRevisionProjectionDocument,
}) });
const relay = createHttpAdjustmentMaintenanceArchiveRelay("http://web:8080/");
const originalFetch = globalThis.fetch;
let refused = false;
// exercise the actual Node relay and actual archive handler in memory without opening a listener
globalThis.fetch = async (url, options) => {
  assert.equal(options.method, "POST");
  await writeFile(join(parent, "api-relay-v1-forwarded.json"), options.body);
  try {
    const result = await handler(url.pathname, JSON.parse(options.body));
    return new Response(JSON.stringify(result), { status: 200 });
  } catch {
    refused = true;
    return new Response('{"error":"revision_archive_rejected"}', { status: 409 });
  }
};
try {
  await assert.rejects(relay.publish({ body: Buffer.from(stage.bodyBase64, "base64"), metadata: stage.metadata,
    revisionReceipt: compact.revisionReceipt, sourceProjection: Buffer.from(stage.sourceProjectionBase64, "base64"),
    sourceProjectionSha256: stage.sourceProjectionSha256, stageReceipt }));
  assert.ok(refused, "original full v1 relay is refused by the compact archive grammar");
} finally {
  globalThis.fetch = originalFetch;
}
