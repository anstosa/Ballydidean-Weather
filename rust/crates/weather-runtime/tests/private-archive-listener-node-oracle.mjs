import { mkdir, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { parseAdjustmentRevisionProjectionDocument } from "../../../../apps/web/src/adjustment-maintenance-contract.mjs";
import {
  AdjustmentRevisionArchiveStore,
  createAdjustmentRevisionArchiveHandler,
} from "../../../../deploy/scripts/adjustment-evidence-store.mjs";

const parent = process.argv[2];
const root = join(parent, "node");
const now = () => new Date("2026-10-08T03:00:00.000Z");
const base = "/internal/adjustment-maintenance";
const gaugeKey = { logicalHourAt: "2026-10-08T01:00:00.000Z", logicalKeySha256: "a".repeat(64) };
const gaugeGap = { ...gaugeKey, reason: "target_source_oversized" };
const shadowGap = {
  dueKey: "capture/2026-10-08T00:35:00.000Z", family: "rain",
  reason: "candidate_unavailable", registrationSha256: "b".repeat(64),
};
const revisionGap = {
  logicalKeySha256: "c".repeat(64), projectionIdentitySha256: null,
  projectionKind: "target_revision", projectionSha256: null, reason: "archive_stage_failed",
};
await mkdir(root, { mode: 0o700 });
const handler = createAdjustmentRevisionArchiveHandler({ store: new AdjustmentRevisionArchiveStore({
  root, now, parseProjection: parseAdjustmentRevisionProjectionDocument,
}) });
const cases = [
  ["gauge-absent", "/rain-fixed-gauge-target/gap/status", gaugeKey],
  ["gauge-first", "/rain-fixed-gauge-target/gap", gaugeGap],
  ["gauge-retry", "/rain-fixed-gauge-target/gap", gaugeGap],
  ["gauge-present", "/rain-fixed-gauge-target/gap/status", gaugeKey],
  ["shadow-gap-first", "/archive/gap", shadowGap],
  ["shadow-gap-retry", "/archive/gap", shadowGap],
  ["revision-gap-first", "/revision/gap", revisionGap],
  ["revision-gap-retry", "/revision/gap", revisionGap],
];

// preserve actual request values and JSON responses from the original handler
for (const [name, suffix, input] of cases) {
  const receipt = await handler(base + suffix, input);
  await writeFile(join(parent, `${name}.request.json`), JSON.stringify(input));
  await writeFile(join(parent, `${name}.response.json`), JSON.stringify(receipt) + "\n");
}
await writeFile(join(parent, "listener-cases.tsv"), cases.map(([name, suffix]) => `${name}\t${base + suffix}`).join("\n") + "\n");
