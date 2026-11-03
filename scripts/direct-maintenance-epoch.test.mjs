import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { chmod, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { Writable } from "node:stream";
import test from "node:test";

import { runDirectMaintenanceEpoch } from "./direct-maintenance-epoch.mjs";

// hash one fixture file body
function sha256(value) {
  return createHash("sha256").update(value).digest("hex");
}

// build one exact immutable target identity
function targetIdentity() {
  return {
    checkRunId: 41,
    cloudflaredImage: `cloudflare/cloudflared@sha256:${"4".repeat(64)}`,
    contractVersion: "weather-direct-maintenance-target/v1",
    controlPlaneSha256: "6ea5cf5b770904bdfa557b29af2dd8899701d8f1b4a15b632f55ab5121032cbb",
    postgresImage: `postgres@sha256:${"3".repeat(64)}`,
    previousRelease: "2026.10.09-1",
    publishRunId: 42,
    revised20Sha256: "58d50e88fa767212865e222ec02e55283ceac5b00538aa2898b5ebc2873b28f6",
    serverImage: `ghcr.io/anstosa/weather-server@sha256:${"1".repeat(64)}`,
    sourceEnvironmentSha256: "97368a14e368dcf7fd3f3f179cd222df2f848695e1cd993abf8892e69c30d4e7",
    sourceRelease: "2026.10.09-2",
    state: "publication_ready",
    targetCommit: "5".repeat(40),
    targetHistorySha256: "3fa9ed85218278aa3b20537d9fbe9b88ef1adee7d7b2f80fde832c45d9cb4554",
    targetRelease: "2026.11.03-2",
    webImage: `ghcr.io/anstosa/weather-web@sha256:${"2".repeat(64)}`,
  };
}

test("epoch wrapper passes the exact v17 deployment identity to the private store", async () => {
  const root = await mkdtemp(join(tmpdir(), "weather-direct-epoch-"));
  const identityPath = join(root, "identity.json");
  const storePath = join(root, "store.mjs");
  const identityBytes = Buffer.from(`${JSON.stringify(targetIdentity())}\n`);
  const storeBytes = Buffer.from(`export async function runAdjustmentArchiveCommand(argv, options) {
  options.stdout.write(JSON.stringify({ argv, deployment: options.captureEpochDeployment }));
}\n`);
  await Promise.all([
    writeFile(identityPath, identityBytes, { mode: 0o600 }),
    writeFile(storePath, storeBytes, { mode: 0o600 }),
  ]);
  await Promise.all([chmod(identityPath, 0o600), chmod(storePath, 0o600)]);
  let output = "";
  const stdout = new Writable({
    // retain one wrapper result
    write(chunk, _encoding, callback) {
      output += chunk.toString();
      callback();
    },
  });
  try {
    await runDirectMaintenanceEpoch(identityPath, sha256(identityBytes), {
      expectedGid: process.getegid(),
      expectedUid: process.geteuid(),
      requireRoot: false,
      stdin: process.stdin,
      stdout,
      storePath,
      storeSha256: sha256(storeBytes),
    });
    const value = JSON.parse(output);
    assert.deepEqual(value.argv, ["revision-capture-epoch-init-v1", "2026.11.03-2"]);
    assert.deepEqual(value.deployment, {
      controlPlaneSha256: "6ea5cf5b770904bdfa557b29af2dd8899701d8f1b4a15b632f55ab5121032cbb",
      controlPlaneVersion: "17",
      sourceCommit: "5".repeat(40),
      sourceRelease: "2026.11.03-2",
      sourceServerImageDigest: `sha256:${"1".repeat(64)}`,
      sourceWebImageDigest: `sha256:${"2".repeat(64)}`,
    });
  } finally {
    await rm(root, { force: true, recursive: true });
  }
});

test("epoch wrapper rejects an unpinned private store before import", async () => {
  const root = await mkdtemp(join(tmpdir(), "weather-direct-epoch-pin-"));
  const identityPath = join(root, "identity.json");
  const storePath = join(root, "store.mjs");
  const identityBytes = Buffer.from(`${JSON.stringify(targetIdentity())}\n`);
  const storeBytes = Buffer.from("throw new Error('unsafe import');\n");
  await Promise.all([
    writeFile(identityPath, identityBytes, { mode: 0o600 }),
    writeFile(storePath, storeBytes, { mode: 0o600 }),
  ]);
  try {
    await assert.rejects(
      runDirectMaintenanceEpoch(identityPath, sha256(identityBytes), {
        expectedGid: process.getegid(),
        expectedUid: process.geteuid(),
        requireRoot: false,
        storePath,
        storeSha256: "0".repeat(64),
      }),
      /evidence store digest differs/u,
    );
  } finally {
    await rm(root, { force: true, recursive: true });
  }
});
