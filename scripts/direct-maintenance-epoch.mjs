#!/usr/bin/env node
import { createHash } from "node:crypto";
import { lstat, readFile, realpath } from "node:fs/promises";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";

const FIXED_STORE_PATH =
  "/opt/weather/current/deploy/releases/2026.11.03-2.adjustment-evidence-store.mjs";
export const DIRECT_MAINTENANCE_EPOCH_STORE_SHA256 =
  "48a6935593a1a880846caceea5d3006aaab303e3a989befe49b98412755e3d7a";
const HASH = /^[a-f0-9]{64}$/u;
const COMMIT = /^[a-f0-9]{40}$/u;
const SERVER_IMAGE = /^ghcr\.io\/anstosa\/weather-server@(sha256:[a-f0-9]{64})$/u;
const WEB_IMAGE = /^ghcr\.io\/anstosa\/weather-web@(sha256:[a-f0-9]{64})$/u;

// hash one immutable private file
function sha256(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}

// read one owner-private canonical single-link file
async function readPrivate(path, options) {
  const absolute = resolve(path);
  const [details, canonical] = await Promise.all([lstat(absolute), realpath(absolute)]);
  // reject aliases, foreign owners and broad permissions
  if (!details.isFile() || details.isSymbolicLink() || details.nlink !== 1 ||
    details.uid !== options.expectedUid || details.gid !== options.expectedGid ||
    (details.mode & 0o777) !== 0o600 || details.size < 2 ||
    details.size > options.maximumBytes || canonical !== absolute) {
    throw new Error(`${options.label} metadata differs`);
  }
  return await readFile(absolute);
}

// validate the external immutable target identity
function validateIdentity(value) {
  const keys = ["checkRunId", "cloudflaredImage", "contractVersion", "controlPlaneSha256",
    "postgresImage", "previousRelease", "publishRunId", "revised20Sha256", "serverImage",
    "sourceEnvironmentSha256", "sourceRelease", "state", "targetCommit",
    "targetHistorySha256", "targetRelease", "webImage"];
  const server = SERVER_IMAGE.exec(value?.serverImage);
  const web = WEB_IMAGE.exec(value?.webImage);
  // admit only the fixed published direct target
  if (JSON.stringify(Object.keys(value ?? {}).sort()) !== JSON.stringify(keys.sort()) ||
    value.contractVersion !== "weather-direct-maintenance-target/v1" ||
    value.sourceRelease !== "2026.10.09-2" || value.previousRelease !== "2026.10.09-1" ||
    value.targetRelease !== "2026.11.03-2" || value.state !== "publication_ready" ||
    value.controlPlaneSha256 !==
      "6ea5cf5b770904bdfa557b29af2dd8899701d8f1b4a15b632f55ab5121032cbb" ||
    value.targetHistorySha256 !==
      "3fa9ed85218278aa3b20537d9fbe9b88ef1adee7d7b2f80fde832c45d9cb4554" ||
    !COMMIT.test(value.targetCommit) || server === null || web === null) {
    throw new Error("direct epoch target identity differs");
  }
  return Object.freeze({
    sourceCommit: value.targetCommit,
    sourceServerImageDigest: server[1],
    sourceWebImageDigest: web[1],
  });
}

// invoke the reviewed store with an exact v17 deployment identity
export async function runDirectMaintenanceEpoch(identityPath, identitySha256, options = {}) {
  const expectedUid = options.expectedUid ?? process.geteuid();
  const expectedGid = options.expectedGid ?? process.getegid();
  const storePath = options.storePath ?? FIXED_STORE_PATH;
  const storeSha256 = options.storeSha256 ?? DIRECT_MAINTENANCE_EPOCH_STORE_SHA256;
  // keep the executable entrypoint root-only
  if ((options.requireRoot ?? true) && process.geteuid() !== 0) {
    throw new Error("direct epoch initialization requires root");
  }
  if (!HASH.test(identitySha256) || !HASH.test(storeSha256) ||
    resolve(storePath) !== FIXED_STORE_PATH && options.storePath === undefined) {
    throw new Error("direct epoch immutable digest or path is not finalized");
  }
  const identityBytes = await readPrivate(identityPath, {
    expectedGid,
    expectedUid,
    label: "direct epoch target identity",
    maximumBytes: 65_536,
  });
  if (sha256(identityBytes) !== identitySha256) {
    throw new Error("direct epoch target identity digest differs");
  }
  const deployment = validateIdentity(JSON.parse(identityBytes.toString("utf8")));
  const storeBytes = await readPrivate(storePath, {
    expectedGid,
    expectedUid,
    label: "direct epoch evidence store",
    maximumBytes: 2 * 1_024 * 1_024,
  });
  if (sha256(storeBytes) !== storeSha256) {
    throw new Error("direct epoch evidence store digest differs");
  }
  const store = await import(pathToFileURL(storePath).href);
  if (typeof store.runAdjustmentArchiveCommand !== "function") {
    throw new Error("direct epoch evidence store export differs");
  }
  await store.runAdjustmentArchiveCommand(
    ["revision-capture-epoch-init-v1", "2026.11.03-2"],
    {
      captureEpochDeployment: {
        controlPlaneSha256:
          "6ea5cf5b770904bdfa557b29af2dd8899701d8f1b4a15b632f55ab5121032cbb",
        controlPlaneVersion: "17",
        sourceCommit: deployment.sourceCommit,
        sourceRelease: "2026.11.03-2",
        sourceServerImageDigest: deployment.sourceServerImageDigest,
        sourceWebImageDigest: deployment.sourceWebImageDigest,
      },
      stdin: options.stdin ?? process.stdin,
      stdout: options.stdout ?? process.stdout,
    },
  );
}

// run the fixed command-line entrypoint
async function main() {
  if (process.argv.length !== 4) {
    throw new TypeError("direct-maintenance-epoch requires IDENTITY_PATH IDENTITY_SHA256");
  }
  await runDirectMaintenanceEpoch(process.argv[2], process.argv[3]);
}

// avoid side effects when imported by tests
if (import.meta.url === pathToFileURL(process.argv[1]).href) {
  await main();
}
