import assert from "node:assert/strict";
import { execFileSync, spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdirSync, renameSync, symlinkSync, truncateSync, writeFileSync } from "node:fs";
import { chmod, chown, link, lstat, mkdir, mkdtemp, readFile, readdir, rm, symlink, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import test from "node:test";

import * as receipts from "./backend-performance-receipts.mjs";
import * as hostObserver from "./backend-host-observation.mjs";
import { deriveRuntimeLiveMap } from "./check-backend-runtime-inventory.mjs";
import { HOST_OBSERVATION_VERSION, HOST_PRODUCER, hostEvidenceSha256 } from "./backend-host-observation.mjs";

const HASH_A = "a".repeat(64);
const HASH_B = "b".repeat(64);
const HASH_C = "c".repeat(64);
const HASH_D = "d".repeat(64);
const DEPLOYED_COMMIT = "0bccbaf28de39b68a11f24e09661f1edf842c105";
const MEASUREMENT_COMMIT = "1".repeat(40);
const SERVER_IMAGE = `ghcr.io/anstosa/weather-server@sha256:${"2".repeat(64)}`;
const SERVER_IMAGE_ID = `sha256:${"3".repeat(64)}`;
const WEB_IMAGE = `ghcr.io/anstosa/weather-web@sha256:${"4".repeat(64)}`;
const WEB_IMAGE_ID = `sha256:${"5".repeat(64)}`;
const POSTGRES_IMAGE = `postgres@sha256:${"8".repeat(64)}`;
const POSTGRES_IMAGE_ID = `sha256:${"9".repeat(64)}`;
const CLOUDFLARED_IMAGE = `cloudflare/cloudflared@sha256:${"a".repeat(64)}`;
const CLOUDFLARED_IMAGE_ID = `sha256:${"b".repeat(64)}`;
const OPERATOR_SHA256 = "86858d69ea4c10573b8e6f5c47e9a03ddc81195e30eb50f203969cbada13f22b";
const VALIDATOR_SHA256 = "969a5498f0d3727682e17d0ed5ed8b18bfb6382a339f3afe042a2642b19c1df6";
const DEPLOYED_BUNDLE_SHA256 = sha256Bytes(`${VALIDATOR_SHA256}  schema-preserving-release.mjs\n${OPERATOR_SHA256}  schema-preserving-release.sh\n`);
const KNOWN_HOSTS_SHA256 = "be8b84c8c3599bd2f0e4eaaa93f494f75e574f28d1dc3febea36b4b28986496d";
const BLUEBERRY_FINGERPRINT = "SHA256:1Tsf/CWA+NMQDo4Gw1uCoAXInZgPVEFoPrjFP+lVc98";
const KNOWN_HOSTS_BYTES = Buffer.from("|1|wvW2zM+JpMqY6TRzYTw+03ke8VE=|blN5zRiCoiWXkppny+CYwbWO/Q8= ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIPS/roZMvPzyFCux8wNkNZIL+wMYkYA0k9hZj8u4ZSw3\n");
const CAPTURE_POLICY = {
  contractVersion: "weather-backend-main-capture-policy/v1",
  knownHosts: {
    byteLength: KNOWN_HOSTS_BYTES.byteLength,
    path: "/home/ubuntu/.weather/backend-performance-evidence/v2/trust/blueberry-known-hosts",
    sha256: KNOWN_HOSTS_SHA256,
  },
  ssh: {
    fingerprint: BLUEBERRY_FINGERPRINT,
    host: "192.168.11.94",
    port: 22,
    user: "admin",
  },
};
const CAPTURE_POLICY_BYTES = receipts.canonicalReceiptBytes(CAPTURE_POLICY);
const POLICY_SHA256 = sha256Bytes(CAPTURE_POLICY_BYTES);
const NATIVE_SOURCE_ENVIRONMENT = {
  HOME: "/home/ubuntu",
  LANG: "C",
  PATH: "/home/linuxbrew/.linuxbrew/bin:/home/ubuntu/n/bin:/usr/local/bin:/usr/bin:/bin",
};
const RUNNER_UNITS = [
  "weather-adjustment-archive.service",
  "weather-adjustment-daily.service",
  "weather-adjustment-daily.timer",
  "weather-adjustment-monthly.service",
  "weather-adjustment-monthly.timer",
];
const MEASUREMENT_SOURCE_PATHS = [
  "config/backend-runtime-inventory.json",
  "config/backend-runtime-live-map.json",
  "package.json",
  "scripts/await-check.mjs",
  "scripts/backend-host-observation.mjs",
  "scripts/backend-performance-receipts.mjs",
  "scripts/backend-performance-resources.mjs",
  "scripts/database-page-performance.mjs",
  "scripts/prepare-schema-preserving-publication.mjs",
  "scripts/schema-preserving-release.mjs",
].sort();
const REPOSITORY_ROOT = new URL("..", import.meta.url);
const RECEIPT_MODULE_PATH = new URL("./backend-performance-receipts.mjs", import.meta.url);
// preserve the original retained-v1 generator commit rather than regrading historical evidence
const RETAINED_HOST_GENERATOR_COMMIT = "145dfcfb8f250d8d6651d0d0a55d2634813a59d6";
const RETAINED_HOST_FIXTURES = JSON.parse(
  await readFile(new URL("./test-fixtures/backend-host-retained-v1.json", import.meta.url), "utf8"),
);
assert.equal(RETAINED_HOST_FIXTURES.contractVersion, "weather-backend-retained-v1-test-fixture/v1");

// hash one exact byte sequence
function sha256Bytes(value) {
  return createHash("sha256").update(value).digest("hex");
}

// resolve one fixed image target without collapsing source classes
function imageFixture(target) {
  const fixtures = {
    cloudflared: { id: CLOUDFLARED_IMAGE_ID, reference: CLOUDFLARED_IMAGE },
    postgres: { id: POSTGRES_IMAGE_ID, reference: POSTGRES_IMAGE },
    server: { id: SERVER_IMAGE_ID, reference: SERVER_IMAGE },
    web: { id: WEB_IMAGE_ID, reference: WEB_IMAGE },
  };
  return fixtures[target] ?? fixtures.server;
}

// build one exact reachability declaration
function generation() {
  return { command: null, probe: null, reachable: true, replacement: null };
}

// build one checked inventory entry
function inventoryEntry({ accountingScope, id, installedPaths, kind, sourcePath }) {
  return {
    accountingScope,
    classification: "application",
    command: null,
    entryDigest: HASH_B,
    generations: {
      mixed: generation(),
      "node-v1": generation(),
      "node-v1-rollback": generation(),
      "rust-v1": generation(),
    },
    id,
    installedPaths,
    invokedRuntimes: [],
    kind,
    owner: "weather-backend",
    parents: [],
    parityTest: "scripts/backend-performance-receipts.test.mjs",
    probe: null,
    runtime: "other",
    sourceDigest: HASH_A,
    sourcePath,
  };
}

// build compact checked documents for both fixed profiles
function checkedSourceFixture() {
  const entries = [
    inventoryEntry({ accountingScope: "blueberry:compose:api", id: "compose-command:deploy/compose.yaml#api", installedPaths: ["/opt/weather/current/deploy/compose.yaml"], kind: "compose-command", sourcePath: "deploy/compose.yaml" }),
    inventoryEntry({ accountingScope: "blueberry:compose:migration", id: "compose-command:deploy/compose.yaml#migration", installedPaths: ["/opt/weather/current/deploy/compose.yaml"], kind: "compose-command", sourcePath: "deploy/compose.yaml" }),
    inventoryEntry({ accountingScope: "blueberry:compose:worker", id: "compose-command:deploy/compose.yaml#worker", installedPaths: ["/opt/weather/current/deploy/compose.yaml"], kind: "compose-command", sourcePath: "deploy/compose.yaml" }),
    inventoryEntry({ accountingScope: "blueberry-app", id: "docker-command:deploy/Dockerfile#server", installedPaths: ["image://server"], kind: "docker-command", sourcePath: "deploy/Dockerfile" }),
    inventoryEntry({ accountingScope: "blueberry-web", id: "docker-command:deploy/web.Dockerfile#web", installedPaths: ["image://web"], kind: "docker-command", sourcePath: "deploy/web.Dockerfile" }),
    inventoryEntry({ accountingScope: "blueberry-app", id: "package-script:package.json#job", installedPaths: ["/opt/weather/package.json"], kind: "package-script", sourcePath: "package.json" }),
    inventoryEntry({ accountingScope: "runner", id: "installer-allowlist:scripts/install-adjustment-maintenance-runner.sh#RUNNER_FILES:scripts/runner.mjs", installedPaths: ["${HOME}/.weather/adjustment-maintenance/runner/current/scripts/runner.mjs"], kind: "installer-allowlist", sourcePath: "scripts/install-adjustment-maintenance-runner.sh" }),
  ];
  const inventory = {
    entries,
    generationPolicies: {},
    limitations: ["bounded Gate3 structural fixture"],
    scannerVersion: 1,
    schemaVersion: "backend-runtime-inventory/v1",
  };
  const runtimeMap = deriveRuntimeLiveMap({ entries });
  return {
    inventory,
    inventoryBytes: receipts.canonicalReceiptBytes(inventory),
    runtimeMap,
    runtimeMapBytes: receipts.canonicalReceiptBytes(runtimeMap),
  };
}

// build one checked source with a native API command and exact installed executable
function generationSelectedSourceFixture() {
  const source = checkedSourceFixture();
  const command = inventoryEntry({
    accountingScope: "blueberry:compose:api",
    id: "runtime-generation-command:scripts/schema-preserving-release.mjs#api",
    installedPaths: [],
    kind: "runtime-generation-command",
    sourcePath: "scripts/schema-preserving-release.mjs",
  });
  command.command = JSON.stringify(["/opt/weather/bin/weather-api"]);
  command.runtime = "rust";
  const binary = inventoryEntry({
    accountingScope: "blueberry:compose:api",
    id: "native-binary:server-api",
    installedPaths: ["/opt/weather/bin/weather-api"],
    kind: "native-binary",
    sourcePath: "rust/crates/weather-runtime/src/bin/weather-api.rs",
  });
  binary.command = JSON.stringify(["/opt/weather/bin/weather-api"]);
  binary.invokedRuntimes = ["rust"];
  binary.parents = [{
    id: command.id,
    path: command.sourcePath,
    type: "invocation",
  }];
  binary.runtime = "rust";
  source.inventory.entries.push(command, binary);
  source.runtimeMap = deriveRuntimeLiveMap({ entries: source.inventory.entries });
  source.inventoryBytes = receipts.canonicalReceiptBytes(source.inventory);
  source.runtimeMapBytes = receipts.canonicalReceiptBytes(source.runtimeMap);
  return source;
}

// combine both full modeled Gate2 source documents
function retainedHostSourceFixture() {
  const blueberry = RETAINED_HOST_FIXTURES.blueberry;
  const workstation = RETAINED_HOST_FIXTURES.workstation;
  const inventory = {
    entries: [...blueberry.inventory.entries, ...workstation.inventory.entries],
    schemaVersion: "backend-runtime-inventory/v1",
  };
  const runtimeMap = {
    artifacts: [...blueberry.runtimeMap.artifacts, ...workstation.runtimeMap.artifacts],
    bindings: [...blueberry.runtimeMap.bindings, ...workstation.runtimeMap.bindings],
    contexts: [...blueberry.runtimeMap.contexts, ...workstation.runtimeMap.contexts],
    contractVersion: "weather-backend-runtime-live-map/v1",
    inventorySchemaVersion: "backend-runtime-inventory/v1",
  };
  return {
    inventory,
    inventoryBytes: receipts.canonicalReceiptBytes(inventory),
    runtimeMap,
    runtimeMapBytes: receipts.canonicalReceiptBytes(runtimeMap),
  };
}

// build one full retained-v1 request evidence and observation projection
function retainedRawHostFixture(profile, phase, observerSourceBytes, source, collection) {
  const modeled = structuredClone(
    profile === "blueberry-compose" ? RETAINED_HOST_FIXTURES.blueberry : RETAINED_HOST_FIXTURES.workstation,
  );
  const inventoryJson = source.inventoryBytes.toString("utf8");
  const runtimeMapJson = source.runtimeMapBytes.toString("utf8");
  const producerSha256 = sha256Bytes(observerSourceBytes);

  // align the modeled Blueberry image facts with this deployed-source fixture
  if (profile === "blueberry-compose") {
    const replacements = [
      ["sha256:image-server", SERVER_IMAGE_ID],
      ["weather-server:fixture", SERVER_IMAGE],
      [`weather-server@sha256:${"b".repeat(64)}`, SERVER_IMAGE],
      ["sha256:image-web", WEB_IMAGE_ID],
      ["weather-web:fixture", WEB_IMAGE],
      [`weather-web@sha256:${"b".repeat(64)}`, WEB_IMAGE],
    ];

    // rewrite every retained raw JSON envelope without changing its descriptor
    for (const probeValue of modeled.evidence.probes) {
      let operationJson = JSON.stringify(probeValue.operation);

      // keep derived image-inspect targets aligned with selected container facts
      for (const [from, to] of replacements) {
        operationJson = operationJson.replaceAll(from, to);
      }
      probeValue.operation = JSON.parse(operationJson);
      const raw = probeValue.raw?.stdout;

      // skip native text that carries no deployed image identity
      if (raw?.encoding !== "base64") {
        continue;
      }
      let decoded = Buffer.from(raw.dataBase64, "base64").toString("utf8");
      const before = decoded;

      // apply only the two deployed image role substitutions
      for (const [from, to] of replacements) {
        decoded = decoded.replaceAll(from, to);
      }

      // retain one exact raw envelope only when its semantic bytes changed
      if (decoded !== before) {
        const bytes = Buffer.from(decoded);
        probeValue.raw.stdout = {
          byteLength: bytes.byteLength,
          dataBase64: bytes.toString("base64"),
          encoding: "base64",
          sha256: sha256Bytes(bytes),
          truncated: false,
        };
      }
    }
  }
  modeled.evidence.startedAt = collection.startedAt;
  modeled.evidence.completedAt = collection.completedAt;

  // contain every retained probe inside the rewritten host interval
  for (const probeValue of modeled.evidence.probes) {
    probeValue.startedAt = collection.startedAt;
    probeValue.completedAt = collection.completedAt;
  }
  modeled.evidence.authority = {
    ...modeled.evidence.authority,
    canonicalEligible: false,
    inventorySha256: hostEvidenceSha256(source.inventory),
    kind: "physical-host",
    liveEligible: true,
    producerSha256,
    runtimeMapSha256: hostEvidenceSha256(source.runtimeMap),
  };
  const request = {
    contractVersion: "weather-backend-host-evidence-request/v1",
    expectedHostRole: profile === "blueberry-compose" ? "blueberry" : "workstation",
    expectedReleaseId: "2026.11.13-1",
    expectedSchemaVersion: "2026.11.13-schema",
    inventory: { json: inventoryJson, sha256: sha256Bytes(inventoryJson) },
    limits: {
      deadlineMs: 5000,
      maxFiles: 100,
      maxProbeBytes: 65536,
      maxProcesses: 100,
      maxTotalBytes: 16777216,
    },
    producerSha256,
    profile,
    runtimeMap: { json: runtimeMapJson, sha256: sha256Bytes(runtimeMapJson) },
  };
  const evidenceBytes = receipts.canonicalReceiptBytes(modeled.evidence);
  const requestBytes = receipts.canonicalReceiptBytes(request);
  const derived = hostObserver.buildRetainedBackendHostObservationV1({
    evidenceBytes,
    inventoryBytes: source.inventoryBytes,
    observerSourceBytes,
    phase,
    requestBytes,
    runtimeMapBytes: source.runtimeMapBytes,
  });
  assert.equal(derived.ok, true, JSON.stringify(derived.diagnostics));
  return { evidenceBytes, observation: derived.observation, requestBytes };
}

// build one cleared Gate2 artifact observation
function observedArtifact(source, { present = true } = {}) {
  const { artifact, bindingIds, contextIds } = source;
  const declaredPath = artifact.declaredPath ?? artifact.path ?? null;
  const resolvedPath = typeof declaredPath === "string" && declaredPath.startsWith("${HOME}/")
    ? `/home/ubuntu${declaredPath.slice("${HOME}".length)}`
    : artifact.kind === "image-config"
      ? `image://${artifact.imageTarget}`
      : declaredPath;
  const imageScoped = ["image-config", "package-metadata"].includes(artifact.kind);
  const runnerOwned = artifact.ownerPolicy === "nss-account";
  return {
    artifactId: artifact.id,
    bindingIds,
    containerId: null,
    declaredPath,
    gid: present ? runnerOwned ? 1000 : 0 : null,
    imageId: present && imageScoped ? imageFixture(artifact.imageTarget).id : null,
    kind: artifact.kind,
    mode: present ? artifact.expectedMode ?? "0644" : null,
    present,
    probeIds: contextIds.map(
      // retain one exact per-context artifact probe
      (contextId) => `artifact.${sha256Bytes(`${contextId}\0${artifact.id}`)}`,
    ),
    resolvedPath,
    scope: imageScoped ? "image" : "host",
    sha256: present ? HASH_C : null,
    symlinkTarget: null,
    type: present ? artifact.expectedType ?? (artifact.kind === "image-config" ? "image-config" : "regular-file") : null,
    uid: present ? runnerOwned ? 1000 : 0 : null,
  };
}

// derive every exact per-profile artifact and binding backlink
function observedArtifactsForProfile(runtimeMap, profile, options = {}) {
  const contexts = runtimeMap.contexts.filter(
    // select one fixed host profile
    (context) => context.profile === profile,
  );
  const contextIds = new Set(contexts.map(
    // retain one selected context id
    (context) => context.id,
  ));
  const artifactIds = new Set(contexts.flatMap(
    // retain every artifact expected in the profile
    (context) => context.artifactIds,
  ));
  return runtimeMap.artifacts.filter(
    // select every referenced profile artifact
    (artifact) => artifactIds.has(artifact.id),
  ).map(
    // derive exact backlinks for one profile artifact
    (artifact) => {
      const bindings = runtimeMap.bindings.filter(
        // select every context tuple linked to this artifact
        (binding) => contextIds.has(binding.contextId) && binding.artifactIds.includes(artifact.id),
      );
      const artifactContextIds = contexts.filter(
        // select every context that declares this artifact
        (context) => context.artifactIds.includes(artifact.id),
      ).map(
        // retain one declaring context id
        (context) => context.id,
      );
      return observedArtifact({
        artifact,
        bindingIds: bindings.map(
          // retain one exact runtime binding id
          (binding) => binding.id,
        ),
        contextIds: artifactContextIds,
      }, options);
    },
  );
}

// build one exact cleared host identity
function hostIdentity(architecture) {
  return {
    architecture,
    cgroupNamespace: "cgroup:[4026531835]",
    cgroupRoot: "/sys/fs/cgroup",
    cgroupVersion: "2",
    cpuModel: architecture === "aarch64" ? "Cortex-A76" : "fixture-cpu",
    filesystemType: "cgroup2",
    kernel: "6.12.0",
    libcAbi: "glibc 2.36",
    osId: "debian",
    osVersion: "12",
    probeIds: ["host.uname-machine"],
  };
}

// build one exact cleared systemd observation
function systemdUnit(unit, { activeState = "inactive", enabledState = "disabled", loadState = "loaded", result = "success" } = {}) {
  return {
    activeState,
    dropInPaths: [],
    enabledState,
    execMainStatus: result === "exit-code" ? 1 : 0,
    execStart: "",
    fragmentPath: loadState === "not-found" ? "" : `/etc/systemd/system/${unit}`,
    loadState,
    probeIds: [`systemd.${unit}.show`],
    result,
    subState: activeState === "failed" ? "failed" : "dead",
    unit,
    unitFiles: [],
  };
}

// build one healthy cleared API container
function apiContainer(phase) {
  const rust = phase === "post-cutover";
  return {
    actualArgv: rust ? ["/opt/weather/bin/weather-api"] : ["/usr/local/bin/node", "apps/api/dist/main.js"],
    configuredArgv: rust ? ["/opt/weather/bin/weather-api"] : ["node", "apps/api/dist/main.js"],
    containerId: "1".repeat(64),
    containerNumber: 1,
    cgroupParent: "/weather",
    exitCode: 0,
    health: "healthy",
    imageId: SERVER_IMAGE_ID,
    imageReference: SERVER_IMAGE,
    initPid: 2000,
    mainPid: 2001,
    name: "weather-api-1",
    oneOff: false,
    probeIds: ["docker.container.api.inspect"],
    readOnlyRootfs: true,
    running: true,
    service: "api",
    startedAt: "2026-11-13T18:00:00.000Z",
    status: "running",
  };
}

// build one healthy cleared worker container
function workerContainer(phase) {
  const rust = phase === "post-cutover";
  return {
    actualArgv: rust ? ["/opt/weather/bin/weather-worker"] : ["/usr/local/bin/node", "apps/worker/dist/main.js"],
    configuredArgv: rust ? ["/opt/weather/bin/weather-worker"] : ["node", "apps/worker/dist/main.js"],
    containerId: "3".repeat(64),
    containerNumber: 1,
    cgroupParent: "/weather",
    exitCode: 0,
    health: "healthy",
    imageId: SERVER_IMAGE_ID,
    imageReference: SERVER_IMAGE,
    initPid: 3000,
    mainPid: 3001,
    name: "weather-worker-1",
    oneOff: false,
    probeIds: ["docker.container.worker.inspect"],
    readOnlyRootfs: true,
    running: true,
    service: "worker",
    startedAt: "2026-11-13T18:00:00.000Z",
    status: "running",
  };
}

// build one healthy cleared infrastructure container
function infrastructureContainer(service) {
  const image = imageFixture(service);
  return {
    actualArgv: [`/usr/bin/${service}`],
    configuredArgv: [service],
    containerId: service === "web" ? "4".repeat(64) : service === "postgres" ? "5".repeat(64) : "6".repeat(64),
    containerNumber: 1,
    cgroupParent: "/weather",
    exitCode: 0,
    health: "healthy",
    imageId: image.id,
    imageReference: image.reference,
    initPid: service === "web" ? 4000 : service === "postgres" ? 5000 : 6000,
    mainPid: service === "web" ? 4001 : service === "postgres" ? 5001 : 6001,
    name: `weather-${service}-1`,
    oneOff: false,
    probeIds: [`docker.container.${service}.inspect`],
    readOnlyRootfs: true,
    running: true,
    service,
    startedAt: "2026-11-13T18:00:00.000Z",
    status: "running",
  };
}

// build one completed migration observation
function migrationContainer() {
  return {
    actualArgv: null,
    configuredArgv: ["node", "apps/worker/dist/migrate.js"],
    containerId: "2".repeat(64),
    containerNumber: 1,
    cgroupParent: "/weather",
    exitCode: 0,
    health: "none",
    imageId: SERVER_IMAGE_ID,
    imageReference: SERVER_IMAGE,
    initPid: null,
    mainPid: null,
    name: "weather-migration-run-1",
    oneOff: true,
    probeIds: ["docker.container.migration.inspect"],
    readOnlyRootfs: true,
    running: false,
    service: "migration",
    startedAt: "2026-11-13T17:59:00.000Z",
    status: "exited",
  };
}

// build one separately typed image identity
function imageIdentity(role) {
  const image = imageFixture(role);
  return {
    architecture: "arm64",
    declaredCmd: role === "web" ? ["node", "deploy/scripts/web-server.mjs"] : ["node", "apps/api/dist/main.js"],
    declaredEntrypoint: ["docker-entrypoint.sh"],
    imageId: image.id,
    os: "linux",
    probeIds: [`docker.image.${role}.inspect`],
    reference: image.reference,
    repoDigests: [image.reference],
    revision: `declared-${role}-revision`,
    role,
  };
}

// build one cleared Blueberry observation fixture
function blueberryObservation(source, phase, interval, { completedMigration = false } = {}) {
  const artifacts = observedArtifactsForProfile(source.runtimeMap, "blueberry-compose");
  const containers = [
    apiContainer(phase),
    infrastructureContainer("cloudflared"),
    infrastructureContainer("postgres"),
    infrastructureContainer("web"),
    workerContainer(phase),
  ];

  // retain completed migration only when requested
  if (completedMigration) {
    containers.push(migrationContainer());
  }
  return {
    artifacts,
    authority: { canonicalEligible: false, inventorySha256: sha256Bytes(source.inventoryBytes), kind: "injected-test", liveEligible: false, probeIds: ["docker.containers.list"], producerSha256: source.observerSha256 ?? HASH_A, runtimeMapSha256: sha256Bytes(source.runtimeMapBytes) },
    cgroups: [],
    collection: { complete: true, completedAt: interval.observationCompletedAt, startedAt: interval.observationStartedAt },
    contractVersion: HOST_OBSERVATION_VERSION,
    docker: { composeProject: "weather", containers, images: [imageIdentity("cloudflared"), imageIdentity("postgres"), imageIdentity("server"), imageIdentity("web")], probeIds: ["docker.containers.list"] },
    evidenceSha256: HASH_B,
    hostRole: "blueberry",
    identity: hostIdentity("aarch64"),
    phase,
    processes: [],
    producer: HOST_PRODUCER,
    profile: "blueberry-compose",
    release: {
      currentRelease: "2026.11.13-1",
      operator: { bundleSha256: DEPLOYED_BUNDLE_SHA256, dispatcherSha256: HASH_B, dropInSha256: HASH_C, forcedCommandSha256: HASH_B, probeIds: ["operator.installation-receipt"], receiptSha256: HASH_D, remoteOpsSha256: HASH_C, sourceCommit: null },
      previousRelease: "2026.11.12-1",
      probeIds: ["release.current", "release.previous", "release.schema"],
      schemaRelease: "2026.11.13-schema",
    },
    runner: null,
    systemd: [systemdUnit("weather-compose.service", { activeState: "failed", enabledState: "enabled", result: "exit-code" })],
  };
}

// build one cleared absent-runner observation fixture
function workstationObservation(source, phase, interval) {
  const artifacts = observedArtifactsForProfile(source.runtimeMap, "workstation-runner", { present: false });
  const units = RUNNER_UNITS.map(
    // retain every proven-absent runner unit
    (unit) => systemdUnit(unit, { enabledState: "not-found", loadState: "not-found" }),
  );
  return {
    artifacts,
    authority: { canonicalEligible: false, inventorySha256: sha256Bytes(source.inventoryBytes), kind: "injected-test", liveEligible: false, probeIds: ["nss.ubuntu.passwd"], producerSha256: source.observerSha256 ?? HASH_A, runtimeMapSha256: sha256Bytes(source.runtimeMapBytes) },
    cgroups: [],
    collection: { complete: true, completedAt: interval.observationCompletedAt, startedAt: interval.observationStartedAt },
    contractVersion: HOST_OBSERVATION_VERSION,
    docker: null,
    evidenceSha256: HASH_C,
    hostRole: "workstation",
    identity: hostIdentity("x86_64"),
    phase,
    processes: [],
    producer: HOST_PRODUCER,
    profile: "workstation-runner",
    release: { currentRelease: null, operator: { bundleSha256: null, dispatcherSha256: null, dropInSha256: null, forcedCommandSha256: null, probeIds: [], receiptSha256: null, remoteOpsSha256: null, sourceCommit: null }, previousRelease: null, probeIds: [], schemaRelease: null },
    runner: {
      account: { gid: 1000, home: "/home/ubuntu", name: "ubuntu", probeIds: ["nss.ubuntu.passwd"], uid: 1000 },
      probeIds: ["nss.ubuntu.passwd", ...RUNNER_UNITS.map(
        // retain every absent unit backlink
        (unit) => `systemd.${unit}.show`,
      )].sort(),
      processes: [],
      roots: ["/home/ubuntu/.weather/adjustment-maintenance/runner", "/home/ubuntu/.local/lib/weather-adjustment-maintenance"],
      state: "not-installed",
      units,
    },
    systemd: units,
  };
}

// derive one pair of sequential capture intervals
function captureIntervals(startMs) {
  const iso = (offset) => new Date(startMs + offset).toISOString();
  return {
    blueberry: { observationCompletedAt: iso(2000), observationStartedAt: iso(1000), transportCompletedAt: iso(3000), transportStartedAt: iso(0) },
    workstation: { observationCompletedAt: iso(6000), observationStartedAt: iso(5000), transportCompletedAt: iso(7000), transportStartedAt: iso(4000) },
    writerNow: iso(8000),
  };
}

// build one profile-owned synthetic transport
function transportFixture(profile, observationBytes, interval, producerSha256 = HASH_A) {
  const common = { completedAt: interval.transportCompletedAt, exitCode: 0, observationSha256: sha256Bytes(observationBytes), producerSha256, startedAt: interval.transportStartedAt };

  // retain SSH fields only for Blueberry
  if (profile === "blueberry-compose") {
    return { ...common, ssh: { hostKeyFingerprint: BLUEBERRY_FINGERPRINT, knownHostsSha256: KNOWN_HOSTS_SHA256, policySha256: POLICY_SHA256 } };
  }
  return { ...common, process: { egid: 0, euid: 0, executablePath: "/usr/bin/node", executableSha256: HASH_D, gid: 1000, pid: 4321, startTimeTicks: "1000", uid: 1000 } };
}

// build one cleared fixture capture
function fixtureCapture(observation, interval) {
  const observationBytes = receipts.canonicalReceiptBytes(observation);
  return { kind: "cleared-observation-fixture", observationBytes, transport: transportFixture(observation.profile, observationBytes, interval, observation.authority.producerSha256) };
}

// build one complete structural assembly input
function assemblyInput({
  completedMigration = false,
  phase = "baseline",
  previousReceiptBytes = null,
  source = checkedSourceFixture(),
  startMs = Date.parse("2026-11-13T18:00:00.000Z"),
} = {}) {
  const intervals = captureIntervals(startMs);
  const blueberry = blueberryObservation(source, phase, intervals.blueberry, { completedMigration });
  const workstation = workstationObservation(source, phase, intervals.workstation);
  return {
    input: {
      captures: [fixtureCapture(blueberry, intervals.blueberry), fixtureCapture(workstation, intervals.workstation)],
      inventoryBytes: source.inventoryBytes,
      measurementSourceReceiptBytes: receipts.canonicalReceiptBytes({ contractVersion: "weather-backend-measurement-source-receipt/v1", evaluatorSha256: HASH_A, resourceCollectorSha256: HASH_B }),
      phase,
      previousReceiptBytes,
      runtimeMapBytes: source.runtimeMapBytes,
      writerNow: intervals.writerNow,
    },
    intervals,
    source,
  };
}

// create one private controlled receipt root
async function privateRoot() {
  const rootDir = await mkdtemp(join(tmpdir(), "weather-live-receipts-v2-"));
  await chmod(rootDir, 0o700);
  return rootDir;
}

// build one retained raw byte envelope
function rawProof(bytes) {
  return {
    byteLength: bytes.byteLength,
    dataBase64: bytes.toString("base64"),
    encoding: "base64",
    sha256: sha256Bytes(bytes),
  };
}

// build one deterministic descriptor identity
function statIdentity(bytes, ino, { mode = "0600", type = "regular-file" } = {}) {
  return {
    ctimeNs: "1000",
    dev: "2049",
    gid: 0,
    ino: String(ino),
    mode,
    mtimeNs: "1000",
    nlink: "1",
    size: String(bytes.byteLength),
    type,
    uid: 0,
  };
}

// build one exact regular deployed-source proof
function regularProof(path, bytes, ino, options = {}) {
  return { identity: statIdentity(bytes, ino, options), path, raw: rawProof(bytes) };
}

// build one valid authenticated workflow summary
function workflowFixture(name, databaseId, release) {
  const value = {
    conclusion: "success",
    databaseId,
    event: "push",
    headSha: DEPLOYED_COMMIT,
    name,
    status: "completed",
    url: `https://github.com/anstosa/Ballydidean-Weather/actions/runs/${databaseId}`,
  };

  // bind only publish to its release tag
  if (name === "Publish production images") {
    value.headBranch = release;
  }
  return value;
}

// build one independently valid source publication
function publicationFixture(release, runOffset = 0) {
  const protectedPaths = [
    "Dockerfile",
    "deploy/scripts/common.sh",
    "deploy/scripts/update.sh",
    "package-lock.json",
    "packages/database/migrations/0020_adjustment_revision_frontier.sql",
    "packages/database/migrations/0023_forecast_product_read_index.sql",
    "packages/forecast-adjustment/src/maintenance-capture-epoch.ts",
  ].sort();
  const sourceTree = protectedPaths.map(
    // retain one exact protected tree member
    (path) => ({ path, sha256: HASH_A }),
  );
  return {
    check: workflowFixture("Check", 101 + runOffset, release),
    contractVersion: "weather-schema-preserving-publication/v2",
    operatorSha256: OPERATOR_SHA256,
    publish: workflowFixture("Publish production images", 102 + runOffset, release),
    serverImage: SERVER_IMAGE,
    serverImageId: SERVER_IMAGE_ID,
    sourceTree,
    targetCommit: DEPLOYED_COMMIT,
    targetRelease: release,
    targetTree: structuredClone(sourceTree),
    validatorSha256: VALIDATOR_SHA256,
    webImage: WEB_IMAGE,
    webImageId: WEB_IMAGE_ID,
  };
}

// build one exact current release environment
function releaseEnvironment(release, extraLine = null) {
  const lines = [
    `WEATHER_RELEASE=${release}`,
    `WEATHER_SERVER_IMAGE=${SERVER_IMAGE}`,
    `WEATHER_WEB_IMAGE=${WEB_IMAGE}`,
    `POSTGRES_IMAGE=${POSTGRES_IMAGE}`,
    `CLOUDFLARED_IMAGE=${CLOUDFLARED_IMAGE}`,
    "WEATHER_DATABASE_NAME=weather",
    "WEATHER_POSTGRES_DIR=/var/lib/weather/postgres",
    "WEATHER_FORECAST_ADJUSTMENT_WIND_CANARY_KILL_SWITCH=0",
    "WEATHER_FORECAST_ADJUSTMENT_TEMPERATURE_CANARY_KILL_SWITCH=1",
    `WEATHER_CONTROL_PLANE_SHA256=${HASH_C}`,
    "WEATHER_CONTROL_PLANE_VERSION=17",
  ];

  // retain one adversarial unknown field when requested
  if (extraLine !== null) {
    lines.push(extraLine);
  }
  return Buffer.from(`${lines.join("\n")}\n`);
}

// load the exact current measurement source byte set
async function repositoryMeasurementSource(
  measurementCommit = MEASUREMENT_COMMIT,
  repositoryRoot = resolve(REPOSITORY_ROOT.pathname),
) {
  const files = [];
  const bytesByPath = new Map();

  // retain every fixed measurement file byte sequence
  for (const path of MEASUREMENT_SOURCE_PATHS) {
    const bytes = await readFile(join(repositoryRoot, path));
    bytesByPath.set(path, bytes);
    files.push({ path, sha256: sha256Bytes(bytes) });
  }
  const inventoryBytes = bytesByPath.get("config/backend-runtime-inventory.json");
  const runtimeMapBytes = bytesByPath.get("config/backend-runtime-live-map.json");
  return {
    bytesByPath,
    inventory: JSON.parse(inventoryBytes),
    inventoryBytes,
    measurementCommit,
    observerSha256: sha256Bytes(bytesByPath.get("scripts/backend-host-observation.mjs")),
    repositoryRoot,
    runtimeMap: JSON.parse(runtimeMapBytes),
    runtimeMapBytes,
  };
}

// build one fixed git-show capture over the measurement source set
function measurementGitCapture(source, options = {}) {
  const startedAt = "2026-11-13T17:00:00.000Z";
  const steps = [{
    argv: ["rev-parse", `${source.measurementCommit}^{commit}`],
    completedAt: "2026-11-13T17:00:01.000Z",
    cwd: source.repositoryRoot,
    environment: options.gitEnvironment ?? NATIVE_SOURCE_ENVIRONMENT,
    exitCode: 0,
    id: "measurement.git.commit",
    program: "/usr/bin/git",
    signal: null,
    startedAt,
    stderrBase64: "",
    stdoutBase64: Buffer.from(`${source.measurementCommit}\n`).toString("base64"),
  }];
  let offset = 2;

  // retain every fixed git-show byte sequence in sorted path order
  for (const path of MEASUREMENT_SOURCE_PATHS) {
    const argv = ["show", `${source.measurementCommit}:${path}`];

    // substitute one native vector only for an adversarial fixture
    if (options.gitArgMutation === path) {
      argv[1] = `${source.measurementCommit}:caller`;
    }
    steps.push({
      argv,
      completedAt: new Date(Date.parse(startedAt) + (offset + 1) * 1000).toISOString(),
      cwd: source.repositoryRoot,
      environment: options.gitEnvironment ?? NATIVE_SOURCE_ENVIRONMENT,
      exitCode: 0,
      id: `measurement.git.show:${path}`,
      program: "/usr/bin/git",
      signal: null,
      startedAt: new Date(Date.parse(startedAt) + offset * 1000).toISOString(),
      stderrBase64: "",
      stdoutBase64: source.bytesByPath.get(path).toString("base64"),
    });
    offset += 2;
  }
  return {
    commit: source.measurementCommit,
    completedAt: new Date(Date.parse(startedAt) + offset * 1000).toISOString(),
    contractVersion: "weather-backend-measurement-git-capture/v1",
    repository: "anstosa/Ballydidean-Weather",
    startedAt,
    steps,
  };
}

// derive one later active receipt implementation with revised native vectors
async function mutatedActiveReceiptSourceBytes() {
  let source = await readFile(RECEIPT_MODULE_PATH);
  let text = source.toString("utf8");
  const workstationBefore = 'environment: { LANG: "C" },\n        id: "workstation.collect"';
  const deployedBefore = 'environment: { LANG: "C" },\n      id: "blueberry.deployed-source.collect"';
  assert.equal(text.includes(workstationBefore), true);
  assert.equal(text.includes(deployedBefore), true);
  text = text.replace(workstationBefore, 'environment: { LANG: "C.UTF-8" },\n        id: "workstation.collect"');
  text = text.replace(deployedBefore, 'environment: { LANG: "C.UTF-8" },\n      id: "blueberry.deployed-source.collect"');
  source = Buffer.from(text);
  return source;
}

// materialize one temporary later active module at the same repository root
async function createMutatedActiveReceiptModule() {
  const modulePath = join(
    REPOSITORY_ROOT.pathname,
    "scripts",
    `.backend-performance-receipts-active-${String(process.pid)}-${String(Date.now())}.mjs`,
  );
  await writeFile(modulePath, await mutatedActiveReceiptSourceBytes(), { flag: "wx", mode: 0o600 });
  return modulePath;
}

// validate retained history through one later active module
async function validateWithMutatedActiveReceiptModule(rootDir, modulePath) {
  const script = String.raw`
import { pathToFileURL } from "node:url";
const [rootDir, modulePath] = process.argv.slice(1);
const active = await import(pathToFileURL(modulePath).href);
const result = await active.validateSyntheticHistoricalCustodyChain({ rootDir });
process.stdout.write(JSON.stringify(result));
`;
  const child = spawn(
    process.execPath,
    ["--input-type=module", "-e", script, rootDir, modulePath],
    { cwd: resolve(REPOSITORY_ROOT.pathname), env: { LANG: "C" }, stdio: ["ignore", "pipe", "pipe"] },
  );
  const stdout = [];
  const stderr = [];
  // retain one bounded child stdout record
  child.stdout.on("data", (chunk) => {
    stdout.push(Buffer.from(chunk));
  });
  // retain one bounded child stderr record
  child.stderr.on("data", (chunk) => {
    stderr.push(Buffer.from(chunk));
  });
  // await one exact child terminal status
  const exitCode = await new Promise((resolveExit, rejectExit) => {
    child.once("error", rejectExit);
    child.once("close", resolveExit);
  });
  assert.equal(exitCode, 0, Buffer.concat(stderr).toString("utf8"));
  return JSON.parse(Buffer.concat(stdout));
}

// write one exact private canonical file
async function writePrivateDocument(path, value) {
  await writeFile(path, receipts.canonicalReceiptBytes(value), { flag: "wx", mode: 0o600 });
}

// construct one complete noncanonical Main custody fixture
async function writeSyntheticMainCustody(rootDir, options = {}) {
  const release = "2026.11.13-1";
  const source = await repositoryMeasurementSource(options.measurementCommit ?? MEASUREMENT_COMMIT);
  const intervals = captureIntervals(Date.parse("2026-11-13T18:00:00.000Z"));
  const blueberry = blueberryObservation(source, "baseline", intervals.blueberry);
  const workstation = workstationObservation(source, "baseline", intervals.workstation);

  // substitute one unrelated operator source class only for an adversarial fixture
  if (options.observationOperatorBundle !== undefined) {
    blueberry.release.operator.bundleSha256 = options.observationOperatorBundle;
  }
  const syntheticCaptures = new Map([
    ["blueberry-compose", fixtureCapture(blueberry, intervals.blueberry)],
    ["workstation-runner", fixtureCapture(workstation, intervals.workstation)],
  ]);
  const historicalPublication = publicationFixture(release);
  const recertifiedPublication = publicationFixture(release, 10);

  // substitute one independently valid recertified server source
  if (options.recertifiedServerImage !== undefined) {
    recertifiedPublication.serverImage = options.recertifiedServerImage;
  }
  const environmentBytes = releaseEnvironment(release, options.extraEnvironmentLine ?? null);
  const protectedDocument = {
    files: [
      "/opt/weather/current/deploy/state/schema-release",
      "/opt/weather/current/deploy/state/adjustment-candidate-catalog.json",
      "/opt/weather/current/deploy/state/adjustment-capture-epoch.json",
      "/opt/weather/current/deploy/state/adjustment-release-state/current.json",
      "/var/lib/weather/xweather/forecast-adjustment-settings.json",
    ].map(
      // retain one protected host metadata record
      (path, index) => ({ inode: `2049:${100 + index}`, mode: 33152, owner: "0:0", path, sha256: HASH_D }),
    ),
    infrastructure: "postgres|cloudflared\n",
  };
  let publicationBytes = options.historicalWriterEncoding === true
    ? Buffer.from(`${JSON.stringify(historicalPublication, null, 2)}\n`)
    : receipts.canonicalReceiptBytes(historicalPublication);
  const protectedBytes = options.historicalWriterEncoding === true
    ? Buffer.from(`${JSON.stringify(protectedDocument, null, 2)}\n`)
    : receipts.canonicalReceiptBytes(protectedDocument);

  // retain one self-consistent duplicate-key attack when requested
  if (options.historicalDuplicateKey === true) {
    publicationBytes = Buffer.from(`${JSON.stringify(historicalPublication).slice(0, -1)},"targetRelease":"${release}"}\n`);
  }
  const stage = {
    contractVersion: "weather-schema-preserving-stage/v1",
    protectedSha256: sha256Bytes(protectedBytes),
    publicationSha256: sha256Bytes(publicationBytes),
    sourceEnvironmentSha256: HASH_A,
    sourceRelease: "2026.11.12-1",
    targetEnvironmentSha256: sha256Bytes(environmentBytes),
    targetRelease: release,
  };
  const stageBytes = options.historicalWriterEncoding === true
    ? Buffer.from(`${JSON.stringify(stage, null, 2)}\n`)
    : receipts.canonicalReceiptBytes(stage);
  const readyBytes = options.readyChanged === true ? receipts.canonicalReceiptBytes({ ...stage, sourceEnvironmentSha256: HASH_B }) : stageBytes;
  const releaseRoot = `/opt/weather/current/deploy/state/schema-preserving-release-v1/${release}`;
  const currentReleaseBytes = Buffer.from(`${release}\n`);
  const currentRelease = regularProof("/opt/weather/current/deploy/state/current-release", currentReleaseBytes, 10, { mode: options.markerMode ?? "0600" });
  const releaseEnvironmentProof = regularProof(`/opt/weather/current/deploy/releases/${release}.env`, environmentBytes, 16);
  const activeLinkTarget = options.aliasTarget ?? `../releases/${release}.env`;
  const hostBundle = {
    activeAlias: {
      identity: statIdentity(Buffer.from(activeLinkTarget), 17, { mode: "0777", type: "symbolic-link" }),
      linkTarget: activeLinkTarget,
      path: "/opt/weather/current/deploy/state/active.env",
      resolvedPath: `/opt/weather/current/deploy/releases/${release}.env`,
      targetIdentity: structuredClone(releaseEnvironmentProof.identity),
      targetSha256: releaseEnvironmentProof.raw.sha256,
    },
    contractVersion: "weather-backend-deployed-source-capture/v1",
    currentRelease,
    operationId: "blueberry-deployed-source-v1",
    protected: regularProof(`${releaseRoot}/protected.json`, protectedBytes, 14),
    publication: regularProof(`${releaseRoot}/publication.json`, publicationBytes, 11),
    ready: regularProof(`${releaseRoot}/ready.json`, readyBytes, 13),
    releaseEnvironment: releaseEnvironmentProof,
    sourceRoot: {
      identity: statIdentity(Buffer.alloc(0), 9, { mode: "0755", type: "directory" }),
      path: "/opt/weather/current",
    },
    stage: regularProof(`${releaseRoot}/stage.json`, stageBytes, 12),
  };
  const hostBundleBytes = receipts.canonicalReceiptBytes(hostBundle);
  const deployedPlan = {
    contractVersion: "weather-backend-deployed-source-plan/v1",
    operationId: receipts.DEPLOYED_SOURCE_CAPTURE_PLAN.operationId,
    steps: structuredClone(receipts.DEPLOYED_SOURCE_CAPTURE_PLAN.steps),
  };

  // detach one retained deployed plan from its versioned helper contract
  if (options.deployedPlanMutation === true) {
    deployedPlan.steps[0].argv = [...deployedPlan.steps[0].argv, "--mismatched-plan"];
  }
  const deployedPlanBytes = receipts.canonicalReceiptBytes(deployedPlan);
  const plannedStep = deployedPlan.steps[0];
  const hostCapture = {
    bundleSha256: sha256Bytes(hostBundleBytes),
    contractVersion: "weather-backend-deployed-source-main-capture/v1",
    operationId: "blueberry-deployed-source-v1",
    planSha256: sha256Bytes(deployedPlanBytes),
    policySha256: POLICY_SHA256,
    steps: [{
      argv: plannedStep.argv,
      completedAt: "2026-11-13T17:45:02.000Z",
      cwd: plannedStep.cwd,
      environment: plannedStep.environment,
      exitCode: 0,
      id: plannedStep.id,
      program: options.deployedCaptureProgram ?? plannedStep.program,
      signal: null,
      startedAt: "2026-11-13T17:45:00.000Z",
      stderr: { byteLength: 0, dataBase64: "", sha256: sha256Bytes(Buffer.alloc(0)) },
      stdinKind: plannedStep.stdinKind,
      stdout: { byteLength: hostBundleBytes.byteLength, sha256: sha256Bytes(hostBundleBytes) },
    }],
  };
  const hostCaptureBytes = receipts.canonicalReceiptBytes(hostCapture);
  const recertifiedPublicationBytes = receipts.canonicalReceiptBytes(recertifiedPublication);
  const preparerCapture = {
    completedAt: "2026-11-13T17:40:02.000Z",
    contractVersion: "weather-backend-publication-preparer-capture/v1",
    result: {
      argv: [
        "/home/ubuntu/weather/scripts/prepare-schema-preserving-publication.mjs",
        release,
        "/home/ubuntu/.weather/backend-performance-evidence/v2/deployed-source/recertified-publication.json",
      ],
      completedAt: "2026-11-13T17:40:02.000Z",
      cwd: "/home/ubuntu/.weather/backend-performance-evidence/v2/deployed-source/tag-tree",
      environment: options.preparerEnvironment ?? NATIVE_SOURCE_ENVIRONMENT,
      exitCode: 0,
      program: "/home/ubuntu/n/bin/node",
      signal: null,
      startedAt: "2026-11-13T17:40:00.000Z",
      stderrBase64: "",
      stdoutBase64: "",
    },
    startedAt: "2026-11-13T17:40:00.000Z",
  };
  const preparerCaptureBytes = receipts.canonicalReceiptBytes(preparerCapture);
  const deployedProof = {
    contractVersion: "weather-backend-deployed-source-proof/v1",
    hostBundleSha256: sha256Bytes(hostBundleBytes),
    hostCaptureSha256: sha256Bytes(hostCaptureBytes),
    hostPlanSha256: sha256Bytes(deployedPlanBytes),
    preparerCaptureSha256: sha256Bytes(preparerCaptureBytes),
    recertifiedPublicationSha256: sha256Bytes(recertifiedPublicationBytes),
  };
  const gitCapture = measurementGitCapture(source, options);
  const gitCaptureBytes = receipts.canonicalReceiptBytes(gitCapture);
  const measurementReceipt = {
    captureSha256: sha256Bytes(gitCaptureBytes),
    commit: source.measurementCommit,
    contractVersion: "weather-backend-measurement-source-receipt/v2",
    files: MEASUREMENT_SOURCE_PATHS.map(
      // bind one fixed measurement file digest
      (path) => ({ path, sha256: options.measurementDigestPath === path ? HASH_D : sha256Bytes(source.bytesByPath.get(path)) }),
    ),
    repository: "anstosa/Ballydidean-Weather",
  };
  const deployedDirectory = join(rootDir, "deployed-source");
  const trustDirectory = join(rootDir, "trust");
  await mkdir(deployedDirectory, { mode: 0o700 });
  await mkdir(join(deployedDirectory, "tag-tree"), { mode: 0o700 });
  await mkdir(trustDirectory, { mode: 0o700 });
  await writePrivateDocument(join(rootDir, "measurement-source-receipt.json"), measurementReceipt);
  await writePrivateDocument(join(rootDir, "measurement-source-git-capture.json"), gitCapture);
  await writeFile(join(trustDirectory, "blueberry-known-hosts"), KNOWN_HOSTS_BYTES, { flag: "wx", mode: 0o600 });
  await writeFile(join(trustDirectory, "policy.json"), CAPTURE_POLICY_BYTES, { flag: "wx", mode: 0o600 });
  await writePrivateDocument(join(deployedDirectory, "proof.json"), deployedProof);
  await writePrivateDocument(join(deployedDirectory, "host-capture.json"), hostCapture);
  await writePrivateDocument(join(deployedDirectory, "host-bundle.json"), hostBundle);
  await writeFile(join(deployedDirectory, "host-plan.json"), deployedPlanBytes, { flag: "wx", mode: 0o600 });
  await writePrivateDocument(join(deployedDirectory, "recertified-publication.json"), recertifiedPublication);
  await writePrivateDocument(join(deployedDirectory, "preparer-capture.json"), preparerCapture);

  // retain both exact cleared synthetic profile captures
  for (const [profile, capture] of syntheticCaptures) {
    const directory = join(rootDir, "synthetic-captures", profile);
    await mkdir(directory, { mode: 0o700, recursive: true });
    await writeFile(join(directory, "observation.json"), capture.observationBytes, { flag: "wx", mode: 0o600 });
    await writePrivateDocument(join(directory, "transport.json"), capture.transport);
  }
  return {
    deployedProof,
    hostBundle,
    intervals,
    measurementReceipt,
    source,
    sourceDocumentBytes: [
      receipts.canonicalReceiptBytes(measurementReceipt),
      gitCaptureBytes,
      receipts.canonicalReceiptBytes(deployedProof),
      hostCaptureBytes,
      hostBundleBytes,
      deployedPlanBytes,
      recertifiedPublicationBytes,
      preparerCaptureBytes,
      KNOWN_HOSTS_BYTES,
      CAPTURE_POLICY_BYTES,
    ],
  };
}

// build one exact retained artifact reference
function retainedReference(bytes) {
  const sha256 = sha256Bytes(bytes);
  return { byteLength: bytes.byteLength, relativePath: `artifacts/${sha256}.bin`, sha256 };
}

// replace the synthetic capture pair for one later phase
async function rewriteSyntheticCustodyCaptures(rootDir, phase, startMs) {
  const source = await repositoryMeasurementSource();
  const intervals = captureIntervals(startMs);
  const observations = [
    blueberryObservation(source, phase, intervals.blueberry),
    workstationObservation(source, phase, intervals.workstation),
  ];

  // replace only the fixed cleared profile fixture files
  for (const observation of observations) {
    const profileInterval = observation.profile === "blueberry-compose" ? intervals.blueberry : intervals.workstation;
    const capture = fixtureCapture(observation, profileInterval);
    const directory = join(rootDir, "synthetic-captures", observation.profile);
    await writeFile(join(directory, "observation.json"), capture.observationBytes, { mode: 0o600 });
    await writeFile(join(directory, "transport.json"), receipts.canonicalReceiptBytes(capture.transport), { mode: 0o600 });
  }
  return intervals;
}

// rewrite one retained baseline to a distinct internally complete source snapshot
async function rewriteHistoricalMeasurementSnapshot(
  rootDir,
  baselinePath,
  {
    mismatchPlan = false,
    mismatchProjection = false,
    mismatchRawSemantic = false,
    mismatchTooling = false,
    rawMainCaptures = true,
  } = {},
) {
  const artifactDirectory = join(rootDir, "live-status", "artifacts");
  const historicalCommit = "2".repeat(40);
  const capture = JSON.parse(await readFile(join(rootDir, "measurement-source-git-capture.json")));
  const receipt = JSON.parse(await readFile(join(rootDir, "measurement-source-receipt.json")));
  const replacedMeasurementCaptureSha256 = receipt.captureSha256;
  capture.commit = historicalCommit;
  capture.steps[0].argv = ["rev-parse", `${historicalCommit}^{commit}`];
  capture.steps[0].stdoutBase64 = Buffer.from(`${historicalCommit}\n`).toString("base64");

  // retarget every retained git-show vector to the historical commit
  for (const step of capture.steps.slice(1)) {
    const path = step.id.slice("measurement.git.show:".length);
    step.argv = ["show", `${historicalCommit}:${path}`];
  }
  receipt.commit = historicalCommit;
  const historicalToolBytes = new Map();

  // retain syntactically valid prior tool revisions with unchanged helper contracts
  for (const path of [
    "scripts/backend-host-observation.mjs",
    "scripts/backend-performance-receipts.mjs",
    "scripts/backend-performance-resources.mjs",
    "scripts/database-page-performance.mjs",
  ]) {
    historicalToolBytes.set(path, Buffer.concat([
      await readFile(new URL(path, REPOSITORY_ROOT)),
      Buffer.from("\n// retained historical revision\n"),
    ]));
  }

  // bind every retained historical tool to its git-show proof
  for (const [path, bytes] of historicalToolBytes) {
    const changedStep = capture.steps.find(
      // select the matching retained git-show result
      (step) => step.id === `measurement.git.show:${path}`,
    );
    const changedFile = receipt.files.find(
      // select the matching retained measurement identity
      (file) => file.path === path,
    );
    assert.ok(changedStep);
    assert.ok(changedFile);
    changedStep.stdoutBase64 = bytes.toString("base64");
    changedFile.sha256 = sha256Bytes(bytes);
    await writeFile(join(artifactDirectory, `${sha256Bytes(bytes)}.bin`), bytes, { flag: "wx", mode: 0o600 });
  }
  const captureBytes = receipts.canonicalReceiptBytes(capture);
  receipt.captureSha256 = sha256Bytes(captureBytes);
  const receiptBytes = receipts.canonicalReceiptBytes(receipt);
  await writeFile(join(artifactDirectory, `${sha256Bytes(captureBytes)}.bin`), captureBytes, { flag: "wx", mode: 0o600 });
  await writeFile(join(artifactDirectory, `${sha256Bytes(receiptBytes)}.bin`), receiptBytes, { flag: "wx", mode: 0o600 });
  const baseline = JSON.parse(await readFile(baselinePath));
  const replacedDigests = [
    baseline.artifacts.measurementSourceReceipt.sha256,
    replacedMeasurementCaptureSha256,
    ...Object.values(baseline.tooling).map(
      // retain every superseded tooling sidecar identity
      (tool) => tool.artifact.sha256,
    ),
    ...baseline.captures.map(
      // retain every superseded cleared-observation sidecar identity
      (captureValue) => captureValue.source.observation.sha256,
    ),
  ];
  baseline.artifacts.measurementSourceReceipt = retainedReference(receiptBytes);
  baseline.authority.measurementSourceReceiptSha256 = sha256Bytes(receiptBytes);
  baseline.tooling.observer.artifact = retainedReference(historicalToolBytes.get("scripts/backend-host-observation.mjs"));
  baseline.tooling.receiptWriter.artifact = retainedReference(historicalToolBytes.get("scripts/backend-performance-receipts.mjs"));
  baseline.tooling.resourceCollector.artifact = retainedReference(historicalToolBytes.get("scripts/backend-performance-resources.mjs"));
  baseline.tooling.evaluator.artifact = retainedReference(historicalToolBytes.get("scripts/database-page-performance.mjs"));

  // detach one retained tool only for the cross-proof negative fixture
  if (mismatchTooling) {
    const mismatchedBytes = Buffer.from("// mismatched retained observer\n");
    await writeFile(join(artifactDirectory, `${sha256Bytes(mismatchedBytes)}.bin`), mismatchedBytes, { flag: "wx", mode: 0o600 });
    replacedDigests.push(sha256Bytes(historicalToolBytes.get("scripts/backend-host-observation.mjs")));
    baseline.tooling.observer.artifact = retainedReference(mismatchedBytes);
  }

  // replace compact fixture sources with exact retained native-plan records
  if (rawMainCaptures) {
    const previousInventorySha256 = baseline.artifacts.inventory.sha256;
    const previousRuntimeMapSha256 = baseline.artifacts.runtimeMap.sha256;
    const source = retainedHostSourceFixture();
    const observerBytes = historicalToolBytes.get("scripts/backend-host-observation.mjs");
    const priorCaptures = baseline.captures;
    const rawProfiles = priorCaptures.map(
      // rederive one full retained-v1 semantic observation per fixed profile
      (captureValue) => retainedRawHostFixture(
        captureValue.profile,
        baseline.phase,
        observerBytes,
        source,
        captureValue.observation.collection,
      ),
    );
    const clearedCaptures = rawProfiles.map(
      // retain the prior native transport interval around each rederived observation
      (raw, index) => {
        const observation = structuredClone(raw.observation);
        observation.authority = {
          ...observation.authority,
          canonicalEligible: false,
          inventorySha256: sha256Bytes(source.inventoryBytes),
          kind: "injected-test",
          liveEligible: false,
          runtimeMapSha256: sha256Bytes(source.runtimeMapBytes),
        };
        return fixtureCapture(observation, {
          observationCompletedAt: observation.collection.completedAt,
          observationStartedAt: observation.collection.startedAt,
          transportCompletedAt: priorCaptures[index].transport.completedAt,
          transportStartedAt: priorCaptures[index].transport.startedAt,
        });
      },
    );
    const projection = receipts.assembleSyntheticLiveStatusReceipt({
      captures: clearedCaptures,
      inventoryBytes: source.inventoryBytes,
      measurementSourceReceiptBytes: receiptBytes,
      phase: baseline.phase,
      previousReceiptBytes: null,
      runtimeMapBytes: source.runtimeMapBytes,
      writerNow: baseline.writtenAt,
    });

    const projectedSourceDigests = [
      projection.receipt.artifacts.inventory.sha256,
      projection.receipt.artifacts.runtimeMap.sha256,
    ];

    // persist only the two newly referenced full-source sidecars
    for (const digest of projectedSourceDigests) {
      const bytes = projection.artifactBytesBySha256.get(digest);
      const path = join(artifactDirectory, `${digest}.bin`);

      // reuse one source proof sidecar already present under the same digest
      try {
        await writeFile(path, bytes, { flag: "wx", mode: 0o600 });
      } catch (error) {
        assert.equal(error?.code, "EEXIST");
      }
    }
    baseline.artifacts.inventory = structuredClone(projection.receipt.artifacts.inventory);
    baseline.artifacts.runtimeMap = structuredClone(projection.receipt.artifacts.runtimeMap);
    baseline.bindings = structuredClone(projection.receipt.bindings);
    baseline.captures = structuredClone(projection.receipt.captures);
    replacedDigests.push(previousInventorySha256, previousRuntimeMapSha256);

    // retain one physical plan and raw source set per fixed profile
    for (let index = 0; index < baseline.captures.length; index += 1) {
      const captureValue = baseline.captures[index];
      const requestBytes = rawProfiles[index].requestBytes;
      let outputBytes = rawProfiles[index].evidenceBytes;
      const operationId = captureValue.profile === "blueberry-compose"
        ? "blueberry-compose-v1"
        : "workstation-runner-v1";

      // mutate one complete raw semantic record without using a compact dialect
      if (mismatchRawSemantic && captureValue.profile === "blueberry-compose") {
        const evidence = JSON.parse(outputBytes);
        const probeValue = evidence.probes.find(
          // select one mandatory physical identity fact
          (candidate) => candidate.id === "host.uname-machine",
        );
        const changed = Buffer.from("x86_64\n");
        probeValue.raw.stdout = {
          byteLength: changed.byteLength,
          dataBase64: changed.toString("base64"),
          encoding: "base64",
          sha256: sha256Bytes(changed),
          truncated: false,
        };
        outputBytes = receipts.canonicalReceiptBytes(evidence);
      }
      const native = mainCaptureFixture(operationId, {
        inventoryBytes: source.inventoryBytes,
        observerBytes,
        outputBytes,
        requestBytes,
        runtimeMapBytes: source.runtimeMapBytes,
      });

      // detach one self-consistent native vector from the retained helper source
      if (mismatchPlan && captureValue.profile === "blueberry-compose") {
        native.materializedPlan.steps[1].argv = [...native.materializedPlan.steps[1].argv, "--mismatched-helper"];
        native.capture.steps[1].argv = [...native.capture.steps[1].argv, "--mismatched-helper"];
        native.capture.planSha256 = sha256Bytes(receipts.canonicalReceiptBytes(native.materializedPlan));
      }
      captureValue.observation = rawProfiles[index].observation;
      captureValue.observationSha256 = receipts.receiptSha256(captureValue.observation);

      // detach one stored semantic projection from its full retained raw source
      if (mismatchProjection && captureValue.profile === "blueberry-compose") {
        captureValue.observation.identity.architecture = "x86_64";
        captureValue.observationSha256 = receipts.receiptSha256(captureValue.observation);
      }
      const nativeCaptureBytes = receipts.canonicalReceiptBytes(native.capture);
      const planBytes = receipts.canonicalReceiptBytes(native.materializedPlan);
      const collectStep = native.capture.steps.find(
        // select the one native host collection outcome
        (step) => step.id.endsWith(".collect"),
      );
      captureValue.transport = {
        captureSha256: sha256Bytes(nativeCaptureBytes),
        completedAt: collectStep.completedAt,
        exitCode: 0,
        kind: "main-fixed-native-capture",
        observationSha256: sha256Bytes(receipts.canonicalReceiptBytes(captureValue.observation)),
        operationId,
        planSha256: native.capture.planSha256,
        producerSha256: sha256Bytes(observerBytes),
        startedAt: collectStep.startedAt,
      };
      captureValue.source = {
        kind: "raw-main-custody",
        nativeCapture: retainedReference(nativeCaptureBytes),
        output: retainedReference(outputBytes),
        plan: retainedReference(planBytes),
        request: retainedReference(requestBytes),
      };

      // persist every newly referenced raw source sidecar
      for (const bytes of [nativeCaptureBytes, outputBytes, planBytes, requestBytes]) {
        const path = join(artifactDirectory, `${sha256Bytes(bytes)}.bin`);

        // reuse shared identical request or output bytes across profiles
        try {
          await writeFile(path, bytes, { flag: "wx", mode: 0o600 });
        } catch (error) {
          assert.equal(error?.code, "EEXIST");
        }
      }
    }
    const retainedIntervals = baseline.captures.map(
      // retain the two exact native collection intervals
      (captureValue) => captureValue.transport,
    );
    baseline.startedAt = retainedIntervals.map(
      // select each native start time
      (interval) => interval.startedAt,
    ).sort()[0];
    baseline.completedAt = retainedIntervals.map(
      // select each native completion time
      (interval) => interval.completedAt,
    ).sort().at(-1);
  }

  // remove only sidecars superseded by the rewritten historical receipt
  for (const digest of new Set(replacedDigests)) {
    await rm(join(artifactDirectory, `${digest}.bin`), { force: true });
  }
  const baselineBytes = receipts.canonicalReceiptBytes(baseline);
  await writeFile(baselinePath, baselineBytes, { mode: 0o600 });
}

// append one structural phase through the shared filesystem primitive
async function appendPhase(rootDir, phase, startMs, options = {}) {
  const { input, intervals } = assemblyInput({ completedMigration: options.completedMigration ?? false, phase, startMs });
  let calls = 0;
  const result = await receipts.appendSyntheticLiveStatusReceipt({
    captures: input.captures,
    clock: {
      // return one writer-owned instant exactly once
      now() {
        calls += 1;
        return intervals.writerNow;
      },
    },
    inventoryBytes: input.inventoryBytes,
    measurementSourceReceiptBytes: input.measurementSourceReceiptBytes,
    phase,
    relativeDirectory: options.relativeDirectory ?? "live-status",
    rootDir,
    runtimeMapBytes: input.runtimeMapBytes,
  });
  assert.equal(calls, 1);
  return result;
}

// require one coded asynchronous failure
async function assertRejectCode(run, codes) {
  const accepted = Array.isArray(codes) ? codes : [codes];
  await assert.rejects(run, (error) => accepted.includes(error?.code));
}

// execute one fixed Python helper in a controlled directory
async function runInvalidRootHelper(source, args, cwd, stdin = Buffer.alloc(0), afterSpawn = null) {
  const child = spawn("/usr/bin/python3", ["-I", "-B", "-S", "-c", source, ...args], {
    cwd,
    env: { LANG: "C" },
    signal: AbortSignal.timeout(5000),
    stdio: ["pipe", "pipe", "pipe"],
  });
  const stdout = [];
  const stderr = [];
  child.stdout.on("data", (chunk) => {
    stdout.push(Buffer.from(chunk));
  });
  child.stderr.on("data", (chunk) => {
    stderr.push(Buffer.from(chunk));
  });
  child.stdin.end(stdin);
  const afterSpawnPromise = afterSpawn === null ? Promise.resolve() : afterSpawn();
  const exitCode = await new Promise((resolve, reject) => {
    child.once("error", reject);
    child.once("close", resolve);
  });
  await afterSpawnPromise;
  return { exitCode, stderr: Buffer.concat(stderr), stdout: Buffer.concat(stdout) };
}

// wait for one controlled helper pathname to become observable
async function waitForControlledPath(path) {
  // bound one deterministic local synchronization loop
  for (let index = 0; index < 2000; index += 1) {
    try {
      await lstat(path);
      return;
    } catch (error) {
      // retry only a not-yet-created controlled path
      if (error?.code !== "ENOENT") {
        throw error;
      }
    }
    await new Promise(
      // yield until the helper advances its fixed copy sequence
      (resolveDelay) => setTimeout(resolveDelay, 1),
    );
  }
  assert.fail(`controlled helper did not create ${path}`);
}

// build one exact staged observer package and trusted digest vector
function controlledLifecyclePackage({ observerExitCode = 0, observerMarkerPath = null, observerPaddingBytes = 0 } = {}) {
  const observerPrelude = observerMarkerPath === null
    ? ""
    : `import { writeFileSync } from "node:fs";writeFileSync(${JSON.stringify(observerMarkerPath)},"executed\\n");`;
  const observerPadding = observerPaddingBytes === 0
    ? ""
    : `\n/*${"x".repeat(observerPaddingBytes)}*/\n`;
  const bytesByName = new Map([
    ["backend-runtime-inventory.json", Buffer.from('{"schemaVersion":"backend-runtime-inventory/v1"}\n')],
    ["backend-runtime-live-map.json", Buffer.from('{"contractVersion":"weather-backend-runtime-live-map/v1"}\n')],
    ["observer.mjs", Buffer.from(`${observerPrelude}process.stdin.resume();process.stdin.on("end",()=>{process.stdout.write("fixture output\\n");process.exit(${String(observerExitCode)});});${observerPadding}\n`)],
    ["request.json", Buffer.from('{"contractVersion":"weather-backend-host-evidence-request/v1"}\n')],
  ]);
  const files = [...bytesByName].map(
    // retain every fixed member with its exact byte identity
    ([name, bytes]) => ({
      byteLength: bytes.byteLength,
      dataBase64: bytes.toString("base64"),
      name,
      sha256: sha256Bytes(bytes),
    }),
  );
  const digests = [
    "observer.mjs",
    "backend-runtime-inventory.json",
    "backend-runtime-live-map.json",
    "request.json",
  ].map(
    // preserve the fixed root-helper digest order
    (name) => sha256Bytes(bytesByName.get(name)),
  );
  return {
    bytesByName,
    digests,
    packageBytes: Buffer.from(JSON.stringify({
      contractVersion: "weather-backend-main-stage-package/v1",
      files,
    })),
  };
}

// create one exact local workstation source tree
async function controlledWorkstationFixture({ observerPaddingBytes = 0 } = {}) {
  const rootDir = await privateRoot();
  const repositoryRoot = join(rootDir, "repository");
  const requestDirectory = join(rootDir, "incoming");
  const requestPath = join(requestDirectory, "request.json");
  const runRoot = join(rootDir, "run-stage");
  const markerPath = join(rootDir, "observer-executed");
  const fixture = controlledLifecyclePackage({ observerMarkerPath: markerPath, observerPaddingBytes });
  await mkdir(repositoryRoot, { mode: 0o700 });
  await mkdir(join(repositoryRoot, "config"), { mode: 0o755 });
  await mkdir(join(repositoryRoot, "scripts"), { mode: 0o755 });
  await mkdir(requestDirectory, { mode: 0o700 });
  const paths = {
    inventory: join(repositoryRoot, "config/backend-runtime-inventory.json"),
    observer: join(repositoryRoot, "scripts/backend-host-observation.mjs"),
    request: requestPath,
    runtimeMap: join(repositoryRoot, "config/backend-runtime-live-map.json"),
  };
  await writeFile(paths.inventory, fixture.bytesByName.get("backend-runtime-inventory.json"), { mode: 0o644 });
  await writeFile(paths.runtimeMap, fixture.bytesByName.get("backend-runtime-live-map.json"), { mode: 0o644 });
  await writeFile(paths.observer, fixture.bytesByName.get("observer.mjs"), { mode: 0o644 });
  await writeFile(paths.request, fixture.bytesByName.get("request.json"), { mode: 0o600 });
  await chmod(paths.inventory, 0o644);
  await chmod(paths.runtimeMap, 0o644);
  await chmod(paths.observer, 0o644);
  await chmod(paths.request, 0o600);
  const helper = receipts.materializeSyntheticWorkstationRootHelper({ repositoryRoot, requestPath, runRoot });
  return { fixture, helper, markerPath, paths, repositoryRoot, requestDirectory, rootDir, runRoot };
}

// require every exposed plan node to remain immutable
function assertDeepFrozen(value, seen = new Set()) {
  // stop at primitives and already visited nodes
  if (value === null || typeof value !== "object" || seen.has(value)) {
    return;
  }
  seen.add(value);
  assert.equal(Object.isFrozen(value), true);

  // inspect every nested plan node
  for (const child of Object.values(value)) {
    assertDeepFrozen(child, seen);
  }
}

// build one exact finite Main capture package without executing its plan
function mainCaptureFixture(operationId, overrides = {}) {
  const packageBytesByName = new Map([
    ["observer.mjs", overrides.observerBytes ?? Buffer.from("export const observer = true;\n")],
    ["backend-runtime-inventory.json", overrides.inventoryBytes ?? Buffer.from('{"schemaVersion":"backend-runtime-inventory/v1"}\n')],
    ["backend-runtime-live-map.json", overrides.runtimeMapBytes ?? Buffer.from('{"contractVersion":"weather-backend-runtime-live-map/v1"}\n')],
    ["request.json", overrides.requestBytes ?? Buffer.from('{"contractVersion":"weather-backend-host-evidence-request/v1"}\n')],
    ["output.json", overrides.outputBytes ?? Buffer.from('{"contractVersion":"weather-backend-host-evidence/v1"}\n')],
  ]);
  const trustedBytesByName = new Map([
    "observer.mjs",
    "backend-runtime-inventory.json",
    "backend-runtime-live-map.json",
    "request.json",
  ].map(
    // retain an independent trusted copy of each source input
    (name) => [name, Buffer.from(packageBytesByName.get(name))],
  ));
  const staticPlan = receipts.MAIN_CAPTURE_PLANS[operationId];
  assert.ok(staticPlan, operationId);
  const inputs = [...packageBytesByName].map(
    // bind one package member into the complete materialized plan
    ([name, bytes]) => ({ byteLength: bytes.byteLength, name, sha256: sha256Bytes(bytes) }),
  ).sort(
    // preserve exact lexical member order
    (left, right) => left.name.localeCompare(right.name),
  );
  const trustedDigests = [
    sha256Bytes(packageBytesByName.get("observer.mjs")),
    sha256Bytes(packageBytesByName.get("backend-runtime-inventory.json")),
    sha256Bytes(packageBytesByName.get("backend-runtime-live-map.json")),
    sha256Bytes(packageBytesByName.get("request.json")),
  ];
  const materializedSteps = staticPlan.steps.map(
    // retain one finite operation-owned vector
    (step) => ({ ...step, argv: [...step.argv] }),
  );
  // append hashes inside both fixed Blueberry remote commands
  if (operationId === "blueberry-compose-v1") {
    for (const step of materializedSteps.filter(
      // select only the privileged collect and verified cleanup vectors
      (candidate) => ["blueberry.collect", "blueberry.cleanup"].includes(candidate.id),
    )) {
      step.argv[step.argv.length - 1] = `${step.argv.at(-1)} ${trustedDigests.join(" ")}`;
    }
  } else {
    // append hashes as fixed workstation helper arguments
    materializedSteps[0].argv.push(...trustedDigests);
  }
  const materializedPlan = {
    contractVersion: "weather-backend-main-capture-plan/v1",
    inputs,
    operationId,
    profile: staticPlan.profile,
    steps: materializedSteps,
  };
  const planSha256 = sha256Bytes(receipts.canonicalReceiptBytes(materializedPlan));
  const requestBytes = packageBytesByName.get("request.json");
  const outputBytes = packageBytesByName.get("output.json");
  const baseTime = Date.parse("2026-11-13T18:00:00.000Z");
  const steps = materializedSteps.map(
    // retain one exact planned vector with independently recorded results
    (step, index) => {
      const startedAt = new Date(baseTime + index * 3000).toISOString();
      const completedAt = new Date(baseTime + index * 3000 + 2000).toISOString();
      const collecting = step.id === "blueberry.collect" || step.id === "workstation.collect";
      const stdoutBytes = collecting ? outputBytes : Buffer.alloc(0);
      return {
        argv: step.argv,
        completedAt,
        cwd: step.cwd,
        environment: step.environment,
        exitCode: 0,
        id: step.id,
        program: step.program,
        signal: null,
        startedAt,
        stderr: { byteLength: 0, dataBase64: "", sha256: sha256Bytes(Buffer.alloc(0)) },
        stdinKind: step.stdinKind,
        stdout: { byteLength: stdoutBytes.byteLength, sha256: sha256Bytes(stdoutBytes) },
      };
    },
  );
  const capture = {
    contractVersion: "weather-backend-main-capture/v1",
    operationId,
    outputSha256: sha256Bytes(outputBytes),
    planSha256,
    policySha256: POLICY_SHA256,
    requestSha256: sha256Bytes(requestBytes),
    steps,
  };
  return {
    capture,
    knownHostsBytes: Buffer.from(KNOWN_HOSTS_BYTES),
    materializedPlan,
    packageBytesByName,
    planSha256,
    trustedBytesByName,
    trustPolicyBytes: Buffer.from(CAPTURE_POLICY_BYTES),
  };
}

// expose only the Gate3 v2 receipt surface
test("receipt module deletes unpublished v1 baseline and target surfaces", () => {
  assert.equal(receipts.LIVE_STATUS_CONTRACT_VERSION, "weather-backend-live-status/v2");
  assert.equal(receipts.LIVE_STATUS_MAX_AGE_MS, 600000);
  assert.deepEqual(Object.keys(receipts).sort(), [
    "DEPLOYED_SOURCE_CAPTURE_PLAN",
    "LIVE_STATUS_CONTRACT_VERSION",
    "LIVE_STATUS_MAX_AGE_MS",
    "MAIN_CAPTURE_PLANS",
    "appendLiveStatusReceipt",
    "appendSyntheticLiveStatusReceipt",
    "appendSyntheticMainCustodyReceipt",
    "assembleSyntheticLiveStatusReceipt",
    "canonicalReceiptBytes",
    "materializeSyntheticBlueberryLifecycleHelpers",
    "materializeSyntheticWorkstationRootHelper",
    "receiptSha256",
    "validateLiveStatusReceipt",
    "validateSyntheticCanonicalChainAdmission",
    "validateSyntheticHistoricalCustodyChain",
    "validateSyntheticMainCapturePackage",
    "validateSyntheticProductionCustodyIdentity",
  ]);

  // reject every removed unpublished API
  for (const name of [
    "buildTargetContract",
    "FULL_BACKEND_BASELINE_CONTRACT_VERSION",
    "summarizeFullBackendEvidence",
    "TARGET_CONTRACT_VERSION",
    "validateFullBackendBaselineEvidence",
    "writeTargetContractCreateOnce",
  ]) {
    assert.equal(name in receipts, false, name);
  }
  assert.equal(typeof receipts.assembleSyntheticLiveStatusReceipt, "function");
  assert.equal(typeof receipts.appendSyntheticLiveStatusReceipt, "function");
  assert.equal(typeof receipts.appendSyntheticMainCustodyReceipt, "function");
  assert.equal(typeof receipts.appendLiveStatusReceipt, "function");
  assert.equal(typeof receipts.materializeSyntheticBlueberryLifecycleHelpers, "function");
  assert.equal(typeof receipts.materializeSyntheticWorkstationRootHelper, "function");
  assert.equal(typeof receipts.validateLiveStatusReceipt, "function");
  assert.equal(typeof receipts.validateSyntheticCanonicalChainAdmission, "function");
  assert.equal(typeof receipts.validateSyntheticHistoricalCustodyChain, "function");
  assert.equal(typeof receipts.validateSyntheticMainCapturePackage, "function");
  assert.equal(typeof receipts.validateSyntheticProductionCustodyIdentity, "function");
});

// bind the generated full raw fixture to its immutable historical generator
test("retained-v1 raw fixture records its generator source identity", () => {
  // read the exact committed source without executing the historical test module
  const source = execFileSync("git", [
    "show",
    `${RETAINED_HOST_GENERATOR_COMMIT}:scripts/backend-host-observation.test.mjs`,
  ], { cwd: REPOSITORY_ROOT, encoding: "utf8" });
  const firstTest = source.search(/^test\(/mu);
  assert.ok(firstTest > 0);
  assert.deepEqual(RETAINED_HOST_FIXTURES.generatedFrom, {
    path: "scripts/backend-host-observation.test.mjs",
    prefixSha256: sha256Bytes(Buffer.from(source.slice(0, firstTest))),
  });
  assert.equal(RETAINED_HOST_FIXTURES.blueberry.evidence.contractVersion, "weather-backend-host-evidence/v1");
  assert.equal(RETAINED_HOST_FIXTURES.workstation.evidence.contractVersion, "weather-backend-host-evidence/v1");
});

// expose only two immutable Main capture plans without a generic transport DSL
test("fixed Main capture plans are finite immutable and secret-free", () => {
  assert.deepEqual(Object.keys(receipts.MAIN_CAPTURE_PLANS).sort(), [
    "blueberry-compose-v1",
    "workstation-runner-v1",
  ]);
  assertDeepFrozen(receipts.MAIN_CAPTURE_PLANS);
  const serialized = JSON.stringify(receipts.MAIN_CAPTURE_PLANS);
  assert.doesNotMatch(serialized, /passwordBytes|passwordDigest|passwordLength|secret/iu);
  assert.match(serialized, /protected-sudo-password/u);
  assert.doesNotMatch(serialized, /@@[A-Z_]+@@/u);
  assert.doesNotMatch(serialized, /0\.0\.0\.0|"PATH"|ProxyCommand|StrictHostKeyChecking=no/u);
  assert.doesNotMatch(serialized, /\.omx\/evidence/u);
  assert.match(serialized, /UserKnownHostsFile=\/home\/ubuntu\/\.weather\/backend-performance-evidence\/v2\/trust\/blueberry-known-hosts/u);
  assert.deepEqual(receipts.MAIN_CAPTURE_PLANS["blueberry-compose-v1"].steps.map(
    // retain the exact finally-cleanup lifecycle
    (step) => step.id,
  ), ["blueberry.stage-package", "blueberry.collect", "blueberry.cleanup"]);
});

// prove the same finite root helper rejects malformed trusted digest vectors first
test("fixed root helper rejects malformed digest argv before collection", async (testContext) => {
  const workstation = receipts.MAIN_CAPTURE_PLANS["workstation-runner-v1"].steps[0];
  const helper = workstation.argv[9];
  const blueberryCollect = receipts.MAIN_CAPTURE_PLANS["blueberry-compose-v1"].steps.find(
    // select the fixed privileged collection vector
    (step) => step.id === "blueberry.collect",
  );
  const blueberryCommand = blueberryCollect.argv.at(-1);
  assert.equal(blueberryCommand.includes(Buffer.from(helper).toString("base64")), true);
  const rootDir = await privateRoot();

  try {
    const cases = [
      ["missing", ["workstation-runner-v1", HASH_A, HASH_B, HASH_C]],
      ["extra", ["workstation-runner-v1", HASH_A, HASH_B, HASH_C, HASH_D, HASH_A]],
      ["nonhex", ["workstation-runner-v1", HASH_A, HASH_B, HASH_C, "z".repeat(64)]],
    ];

    // execute only malformed vectors that must stop before fixed-path access
    for (const [label, args] of cases) {
      await testContext.test(label, async () => {
        const result = await runInvalidRootHelper(helper, args, rootDir);
        assert.notEqual(result.exitCode, 0);
        assert.equal(result.stdout.byteLength, 0);
        assert.ok(result.stderr.byteLength <= 65_536);
      });
    }
  } finally {
    await rm(rootDir, { force: true, recursive: true });
  }
});

// execute the fixed lifecycle only inside one controlled temporary tree
test("Blueberry lifecycle helpers clean every stage without touching preexisting roots", async (testContext) => {
  await testContext.test("two consecutive successful cycles", async () => {
    const rootDir = await privateRoot();

    try {
      const helpers = receipts.materializeSyntheticBlueberryLifecycleHelpers({ rootDir });
      const fixture = controlledLifecyclePackage();

      // prove the fixed path can be reused after verified cleanup
      for (let index = 0; index < 2; index += 1) {
        const staged = await runInvalidRootHelper(
          helpers.stageSource,
          ["blueberry-compose-v1"],
          rootDir,
          fixture.packageBytes,
        );
        assert.equal(staged.exitCode, 0, staged.stderr.toString("utf8"));
        const collected = await runInvalidRootHelper(
          helpers.rootSource,
          ["blueberry-compose-v1", ...fixture.digests],
          rootDir,
        );
        assert.equal(collected.exitCode, 0, collected.stderr.toString("utf8"));
        assert.equal(collected.stdout.toString("utf8"), "fixture output\n");
        await assert.rejects(lstat(helpers.runRoot), (error) => error?.code === "ENOENT");
        assert.equal((await lstat(helpers.stageRoot)).isDirectory(), true);
        const cleaned = await runInvalidRootHelper(
          helpers.cleanupSource,
          ["blueberry-compose-v1", ...fixture.digests],
          rootDir,
        );
        assert.equal(cleaned.exitCode, 0, cleaned.stderr.toString("utf8"));
        await assert.rejects(lstat(helpers.stageRoot), (error) => error?.code === "ENOENT");
      }
    } finally {
      await rm(rootDir, { force: true, recursive: true });
    }
  });

  await testContext.test("collect failure still permits verified cleanup", async () => {
    const rootDir = await privateRoot();

    try {
      const helpers = receipts.materializeSyntheticBlueberryLifecycleHelpers({ rootDir });
      const fixture = controlledLifecyclePackage({ observerExitCode: 1 });
      const staged = await runInvalidRootHelper(helpers.stageSource, ["blueberry-compose-v1"], rootDir, fixture.packageBytes);
      assert.equal(staged.exitCode, 0, staged.stderr.toString("utf8"));
      const collected = await runInvalidRootHelper(helpers.rootSource, ["blueberry-compose-v1", ...fixture.digests], rootDir);
      assert.notEqual(collected.exitCode, 0);
      await assert.rejects(lstat(helpers.runRoot), (error) => error?.code === "ENOENT");
      assert.equal((await lstat(helpers.stageRoot)).isDirectory(), true);
      const cleaned = await runInvalidRootHelper(helpers.cleanupSource, ["blueberry-compose-v1", ...fixture.digests], rootDir);
      assert.equal(cleaned.exitCode, 0, cleaned.stderr.toString("utf8"));
      await assert.rejects(lstat(helpers.stageRoot), (error) => error?.code === "ENOENT");
    } finally {
      await rm(rootDir, { force: true, recursive: true });
    }
  });

  await testContext.test("partial stage failure removes only its new root", async () => {
    const rootDir = await privateRoot();

    try {
      const helpers = receipts.materializeSyntheticBlueberryLifecycleHelpers({ rootDir });
      const fixture = controlledLifecyclePackage();
      const changed = JSON.parse(fixture.packageBytes);
      changed.files[1].sha256 = HASH_D;
      const staged = await runInvalidRootHelper(
        helpers.stageSource,
        ["blueberry-compose-v1"],
        rootDir,
        Buffer.from(JSON.stringify(changed)),
      );
      assert.notEqual(staged.exitCode, 0);
      await assert.rejects(lstat(helpers.stageRoot), (error) => error?.code === "ENOENT");
    } finally {
      await rm(rootDir, { force: true, recursive: true });
    }
  });

  await testContext.test("cleanup failure preserves the unverified stage", async () => {
    const rootDir = await privateRoot();

    try {
      const helpers = receipts.materializeSyntheticBlueberryLifecycleHelpers({ rootDir });
      const fixture = controlledLifecyclePackage();
      const staged = await runInvalidRootHelper(helpers.stageSource, ["blueberry-compose-v1"], rootDir, fixture.packageBytes);
      assert.equal(staged.exitCode, 0, staged.stderr.toString("utf8"));
      await writeFile(join(helpers.stageRoot, "observer.mjs"), "changed\n", { mode: 0o600 });
      const cleaned = await runInvalidRootHelper(helpers.cleanupSource, ["blueberry-compose-v1", ...fixture.digests], rootDir);
      assert.notEqual(cleaned.exitCode, 0);
      assert.equal((await lstat(helpers.stageRoot)).isDirectory(), true);
    } finally {
      await rm(rootDir, { force: true, recursive: true });
    }
  });

  await testContext.test("preexisting stage is never removed", async () => {
    const rootDir = await privateRoot();

    try {
      const helpers = receipts.materializeSyntheticBlueberryLifecycleHelpers({ rootDir });
      const fixture = controlledLifecyclePackage();
      await mkdir(helpers.stageRoot, { mode: 0o700 });
      await writeFile(join(helpers.stageRoot, "sentinel"), "owned elsewhere\n", { mode: 0o600 });
      const staged = await runInvalidRootHelper(helpers.stageSource, ["blueberry-compose-v1"], rootDir, fixture.packageBytes);
      assert.notEqual(staged.exitCode, 0);
      assert.equal((await readFile(join(helpers.stageRoot, "sentinel"))).toString("utf8"), "owned elsewhere\n");
    } finally {
      await rm(rootDir, { force: true, recursive: true });
    }
  });
});

// reject staged metadata drift before privileged observer execution
test("Blueberry root helper rejects staged metadata drift before executing Node", async (testContext) => {
  const cases = [
    {
      label: "public mode",
      // make one exact staged member group-readable
      mutate: async ({ observerPath }) => chmod(observerPath, 0o644),
    },
    {
      label: "hard link",
      // increase the staged member link count outside the stage directory
      mutate: async ({ observerPath, rootDir }) => link(observerPath, join(rootDir, "observer-link")),
    },
    {
      label: "nonregular replacement",
      // replace one staged regular file with a directory under the same name
      mutate: async ({ observerPath }) => {
        await rm(observerPath);
        await mkdir(observerPath, { mode: 0o600 });
      },
    },
  ];

  // isolate every metadata violation in its own private stage
  for (const fixtureCase of cases) {
    await testContext.test(fixtureCase.label, async () => {
      const rootDir = await privateRoot();
      const markerPath = join(rootDir, "observer-executed");

      try {
        const helpers = receipts.materializeSyntheticBlueberryLifecycleHelpers({ rootDir });
        const fixture = controlledLifecyclePackage({ observerMarkerPath: markerPath });
        const staged = await runInvalidRootHelper(
          helpers.stageSource,
          ["blueberry-compose-v1"],
          rootDir,
          fixture.packageBytes,
        );
        assert.equal(staged.exitCode, 0, staged.stderr.toString("utf8"));
        await fixtureCase.mutate({
          observerPath: join(helpers.stageRoot, "observer.mjs"),
          rootDir,
        });
        const collected = await runInvalidRootHelper(
          helpers.rootSource,
          ["blueberry-compose-v1", ...fixture.digests],
          rootDir,
        );
        assert.notEqual(collected.exitCode, 0);
        assert.equal(collected.stdout.byteLength, 0);
        await assert.rejects(lstat(markerPath), (error) => error?.code === "ENOENT");
      } finally {
        await rm(rootDir, { force: true, recursive: true });
      }
    });
  }

  await testContext.test("path replacement after source open", async () => {
    const rootDir = await privateRoot();
    const markerPath = join(rootDir, "observer-executed");

    try {
      const helpers = receipts.materializeSyntheticBlueberryLifecycleHelpers({ rootDir });
      const fixture = controlledLifecyclePackage({
        observerMarkerPath: markerPath,
        observerPaddingBytes: 8 * 1024 * 1024,
      });
      const staged = await runInvalidRootHelper(
        helpers.stageSource,
        ["blueberry-compose-v1"],
        rootDir,
        fixture.packageBytes,
      );
      assert.equal(staged.exitCode, 0, staged.stderr.toString("utf8"));
      const observerPath = join(helpers.stageRoot, "observer.mjs");
      const collected = await runInvalidRootHelper(
        helpers.rootSource,
        ["blueberry-compose-v1", ...fixture.digests],
        rootDir,
        Buffer.alloc(0),
        async () => {
          await waitForControlledPath(join(helpers.runRoot, "observer.mjs"));
          renameSync(observerPath, `${observerPath}.replaced`);
          writeFileSync(observerPath, fixture.bytesByName.get("observer.mjs"), { mode: 0o600 });
        },
      );
      assert.notEqual(collected.exitCode, 0);
      assert.equal(collected.stdout.byteLength, 0);
      await assert.rejects(lstat(markerPath), (error) => error?.code === "ENOENT");
    } finally {
      await rm(rootDir, { force: true, recursive: true });
    }
  });
});

// enforce the exact local source and private request modes before execution
test("workstation root helper accepts only fixed local source metadata", async (testContext) => {
  // accept the exact public-source and private-request split
  await testContext.test("exact source and request modes", async () => {
    const controlled = await controlledWorkstationFixture();

    try {
      const collected = await runInvalidRootHelper(
        controlled.helper.rootSource,
        ["workstation-runner-v1", ...controlled.fixture.digests],
        controlled.rootDir,
      );
      assert.equal(collected.exitCode, 0, collected.stderr.toString("utf8"));
      assert.equal(collected.stdout.toString("utf8"), "fixture output\n");
      assert.equal((await readFile(controlled.markerPath)).toString("utf8"), "executed\n");
      await assert.rejects(lstat(controlled.runRoot), (error) => error?.code === "ENOENT");
    } finally {
      await rm(controlled.rootDir, { force: true, recursive: true });
    }
  });

  const cases = [
    {
      label: "public request",
      // make the private request group-readable
      mutate: async (controlled) => chmod(controlled.paths.request, 0o644),
    },
    {
      label: "private repository source",
      // make one committed source file private
      mutate: async (controlled) => chmod(controlled.paths.observer, 0o600),
    },
    {
      label: "hard-linked repository source",
      // add one second link outside the fixed source tree
      mutate: async (controlled) => link(controlled.paths.inventory, join(controlled.rootDir, "inventory-link")),
    },
    {
      label: "nonregular private request",
      // replace the request leaf with a directory
      mutate: async (controlled) => {
        await rm(controlled.paths.request);
        await mkdir(controlled.paths.request, { mode: 0o600 });
      },
    },
  ];
  const alternateGroup = process.getgroups?.().find(
    // choose one group the unprivileged test process may assign
    (group) => group !== process.getgid?.(),
  );

  // cover a mismatched owner group when the runner exposes one
  if (alternateGroup !== undefined) {
    cases.push({
      label: "wrong repository group",
      // retain the invoking owner while changing the required group
      mutate: async (controlled) => chown(controlled.paths.runtimeMap, process.getuid(), alternateGroup),
    });
  }

  // isolate each metadata violation before the observer sentinel
  for (const fixtureCase of cases) {
    await testContext.test(fixtureCase.label, async () => {
      const controlled = await controlledWorkstationFixture();

      try {
        await fixtureCase.mutate(controlled);
        const collected = await runInvalidRootHelper(
          controlled.helper.rootSource,
          ["workstation-runner-v1", ...controlled.fixture.digests],
          controlled.rootDir,
        );
        assert.notEqual(collected.exitCode, 0);
        assert.equal(collected.stdout.byteLength, 0);
        await assert.rejects(lstat(controlled.markerPath), (error) => error?.code === "ENOENT");
      } finally {
        await rm(controlled.rootDir, { force: true, recursive: true });
      }
    });
  }
});

// reject source path substitution after a trusted local leaf is opened
test("workstation root helper closes local source paths before execution", async (testContext) => {
  const cases = [
    {
      label: "same-byte leaf replacement",
      // replace the opened observer under its pinned parent
      mutate: async (controlled) => {
        await waitForControlledPath(join(controlled.runRoot, "observer.mjs"));
        renameSync(controlled.paths.observer, `${controlled.paths.observer}.replaced`);
        await writeFile(controlled.paths.observer, controlled.fixture.bytesByName.get("observer.mjs"), { mode: 0o644 });
        await chmod(controlled.paths.observer, 0o644);
      },
    },
    {
      label: "parent replacement",
      // replace the pinned scripts directory with an equal pathname tree
      mutate: async (controlled) => {
        await waitForControlledPath(join(controlled.runRoot, "observer.mjs"));
        const scriptsPath = join(controlled.repositoryRoot, "scripts");
        renameSync(scriptsPath, `${scriptsPath}.replaced`);
        await mkdir(scriptsPath, { mode: 0o755 });
        await writeFile(controlled.paths.observer, controlled.fixture.bytesByName.get("observer.mjs"), { mode: 0o644 });
        await chmod(controlled.paths.observer, 0o644);
      },
    },
  ];

  // isolate both closing identity races
  for (const fixtureCase of cases) {
    await testContext.test(fixtureCase.label, async () => {
      const controlled = await controlledWorkstationFixture({ observerPaddingBytes: 8 * 1024 * 1024 });

      try {
        const collected = await runInvalidRootHelper(
          controlled.helper.rootSource,
          ["workstation-runner-v1", ...controlled.fixture.digests],
          controlled.rootDir,
          Buffer.alloc(0),
          () => fixtureCase.mutate(controlled),
        );
        assert.notEqual(collected.exitCode, 0);
        assert.equal(collected.stdout.byteLength, 0);
        await assert.rejects(lstat(controlled.markerPath), (error) => error?.code === "ENOENT");
      } finally {
        await rm(controlled.rootDir, { force: true, recursive: true });
      }
    });
  }
});

// validate exact Main vectors without executing SSH sudo or host collection
test("synthetic Main capture validation binds the complete finite plan", async (testContext) => {
  for (const operationId of ["blueberry-compose-v1", "workstation-runner-v1"]) {
    await testContext.test(operationId, () => {
      const fixture = mainCaptureFixture(operationId);
      const result = receipts.validateSyntheticMainCapturePackage({
        captureBytes: receipts.canonicalReceiptBytes(fixture.capture),
        knownHostsBytes: fixture.knownHostsBytes,
        operationId,
        packageBytesByName: fixture.packageBytesByName,
        trustedBytesByName: fixture.trustedBytesByName,
        trustPolicyBytes: fixture.trustPolicyBytes,
      });
      assert.equal(result.ok, true, JSON.stringify(result.diagnostics));
      assert.equal(result.canonicalEligible, false);
      assert.equal(result.planSha256, fixture.planSha256);
      const captureJson = JSON.stringify(fixture.capture);
      assert.doesNotMatch(captureJson, /@@[A-Z_]+@@/u);

      // require every trusted staged input digest in the materialized vector
      for (const name of [
        "observer.mjs",
        "backend-runtime-inventory.json",
        "backend-runtime-live-map.json",
        "request.json",
      ]) {
        assert.match(captureJson, new RegExp(sha256Bytes(fixture.packageBytesByName.get(name)), "u"));
      }
      const passwordSteps = fixture.capture.steps.filter(
        // select only password-bearing native processes
        (step) => step.stdinKind === "protected-sudo-password",
      );
      assert.ok(passwordSteps.length >= 1);

      // prohibit any password-derived evidence fields
      for (const step of passwordSteps) {
        assert.deepEqual(Object.keys(step).sort(), [
          "argv",
          "completedAt",
          "cwd",
          "environment",
          "exitCode",
          "id",
          "program",
          "signal",
          "startedAt",
          "stderr",
          "stdinKind",
          "stdout",
        ]);
      }
    });
  }
});

// reject substitutions in every authority-bearing capture family
test("synthetic Main capture validation rejects package plan and result substitutions", async (testContext) => {
  const cases = [
    {
      label: "plan digest label",
      // substitute one plausible plan label
      mutate(fixture) { fixture.capture.planSha256 = HASH_A; },
    },
    {
      label: "program",
      // substitute one executable
      mutate(fixture) { fixture.capture.steps.at(-1).program = "/bin/sh"; },
    },
    {
      label: "argv",
      // append one caller command argument
      mutate(fixture) { fixture.capture.steps.at(-1).argv = [...fixture.capture.steps.at(-1).argv, "--caller"]; },
    },
    {
      label: "trusted observer digest argv",
      // substitute one staged-input digest after plan materialization
      mutate(fixture) {
        const expected = sha256Bytes(fixture.packageBytesByName.get("observer.mjs"));
        fixture.capture.steps.at(-1).argv = fixture.capture.steps.at(-1).argv.map(
          // replace the observer digest inside the fixed helper vector
          (argument) => argument.replaceAll(expected, HASH_D),
        );
      },
    },
    {
      label: "remote trusted observer digest argv",
      operationId: "blueberry-compose-v1",
      // substitute one digest inside the fixed remote command
      mutate(fixture) {
        const expected = sha256Bytes(fixture.packageBytesByName.get("observer.mjs"));
        fixture.capture.steps.at(-1).argv = fixture.capture.steps.at(-1).argv.map(
          // replace the remote observer digest without changing the command
          (argument) => argument.replaceAll(expected, HASH_D),
        );
      },
    },
    {
      label: "missing trusted digest argv",
      // remove one exact workstation helper digest argument
      mutate(fixture) { fixture.capture.steps.at(-1).argv.pop(); },
    },
    {
      label: "nonhex trusted digest argv",
      // replace one exact workstation helper digest with nonhex bytes
      mutate(fixture) { fixture.capture.steps.at(-1).argv[fixture.capture.steps.at(-1).argv.length - 1] = "z".repeat(64); },
    },
    {
      label: "environment",
      // introduce one PATH search authority
      mutate(fixture) { fixture.capture.steps.at(-1).environment = { LANG: "C", PATH: "/tmp" }; },
    },
    {
      label: "self-consistent substituted known-hosts bytes",
      operationId: "blueberry-compose-v1",
      // substitute the trust bytes and repeat their digest labels
      mutate(fixture) {
        fixture.knownHostsBytes = Buffer.from(KNOWN_HOSTS_BYTES.toString("utf8").replace(/ZSw3\n$/u, "ZSw4\n"));
        const policy = structuredClone(CAPTURE_POLICY);
        policy.knownHosts.byteLength = fixture.knownHostsBytes.byteLength;
        policy.knownHosts.sha256 = sha256Bytes(fixture.knownHostsBytes);
        fixture.trustPolicyBytes = receipts.canonicalReceiptBytes(policy);
        fixture.capture.policySha256 = sha256Bytes(fixture.trustPolicyBytes);
      },
    },
    {
      label: "stdin kind",
      // mix the staged package with password stdin
      mutate(fixture) { fixture.capture.steps.at(-1).stdinKind = "stage-package"; },
    },
    {
      label: "password digest",
      // attach one forbidden secret-derived field
      mutate(fixture) { fixture.capture.steps.at(-1).passwordSha256 = HASH_D; },
    },
    {
      label: "cleanup nonzero exit",
      // retain one failed verified cleanup
      mutate(fixture) { fixture.capture.steps.at(-1).exitCode = 1; },
    },
    {
      label: "missing cleanup outcome",
      operationId: "blueberry-compose-v1",
      // omit the mandatory finally-cleanup result
      mutate(fixture) { fixture.capture.steps.pop(); },
    },
    {
      label: "signal",
      // retain one signaled native process
      mutate(fixture) { fixture.capture.steps.at(-1).signal = "SIGTERM"; },
    },
    {
      label: "stderr",
      // retain one unexpected error byte
      mutate(fixture) { fixture.capture.steps.at(-1).stderr = { byteLength: 1, dataBase64: "eA==", sha256: sha256Bytes("x") }; },
    },
    {
      label: "output digest",
      // detach collection stdout from output bytes
      mutate(fixture) { fixture.capture.steps.at(-1).stdout.sha256 = HASH_B; },
    },
    {
      label: "overlap",
      // overlap two native steps when Blueberry stages then collects
      mutate(fixture) { fixture.capture.steps[1].startedAt = fixture.capture.steps[0].startedAt; },
      operationId: "blueberry-compose-v1",
    },
    {
      label: "extra package member",
      // add one unplanned staged member
      mutate(fixture) { fixture.packageBytesByName.set("caller.json", Buffer.from("{}\n")); },
    },
    {
      label: "missing package member",
      // remove one fixed staged member
      mutate(fixture) { fixture.packageBytesByName.delete("request.json"); },
    },
    {
      label: "changed package bytes",
      // change one member after plan identity materialization
      mutate(fixture) { fixture.packageBytesByName.set("observer.mjs", Buffer.from("changed\n")); },
    },
    {
      label: "missing trusted member",
      // remove one independently trusted source input
      mutate(fixture) { fixture.trustedBytesByName.delete("request.json"); },
    },
    {
      label: "extra trusted member",
      // add one unplanned trusted source input
      mutate(fixture) { fixture.trustedBytesByName.set("caller.json", Buffer.from("{}\n")); },
    },
  ];

  // isolate every complete-plan substitution
  for (const fixtureCase of cases) {
    await testContext.test(fixtureCase.label, () => {
      const operationId = fixtureCase.operationId ?? "workstation-runner-v1";
      const fixture = mainCaptureFixture(operationId);
      fixtureCase.mutate(fixture);
      const result = receipts.validateSyntheticMainCapturePackage({
        captureBytes: receipts.canonicalReceiptBytes(fixture.capture),
        knownHostsBytes: fixture.knownHostsBytes,
        operationId,
        packageBytesByName: fixture.packageBytesByName,
        trustedBytesByName: fixture.trustedBytesByName,
        trustPolicyBytes: fixture.trustPolicyBytes,
      });
      assert.equal(result.ok, false);
      assert.ok(result.diagnostics.length >= 1);
      assert.equal(result.canonicalEligible, false);
    });
  }

  await testContext.test("noncanonical capture bytes", () => {
    const fixture = mainCaptureFixture("workstation-runner-v1");
    const result = receipts.validateSyntheticMainCapturePackage({
      captureBytes: Buffer.from(`${JSON.stringify(fixture.capture)} \n`),
      knownHostsBytes: fixture.knownHostsBytes,
      operationId: "workstation-runner-v1",
      packageBytesByName: fixture.packageBytesByName,
      trustedBytesByName: fixture.trustedBytesByName,
      trustPolicyBytes: fixture.trustPolicyBytes,
    });
    assert.equal(result.ok, false);
    assert.equal(result.canonicalEligible, false);
  });

  await testContext.test("collect failure retains successful cleanup outcome", () => {
    const fixture = mainCaptureFixture("blueberry-compose-v1");
    const collectIndex = fixture.capture.steps.findIndex(
      // select the fixed privileged collection result
      (step) => step.id === "blueberry.collect",
    );
    fixture.capture.steps[collectIndex].exitCode = 1;
    const result = receipts.validateSyntheticMainCapturePackage({
      captureBytes: receipts.canonicalReceiptBytes(fixture.capture),
      knownHostsBytes: fixture.knownHostsBytes,
      operationId: "blueberry-compose-v1",
      packageBytesByName: fixture.packageBytesByName,
      trustedBytesByName: fixture.trustedBytesByName,
      trustPolicyBytes: fixture.trustPolicyBytes,
    });
    assert.equal(result.ok, false);
    assert.ok(result.diagnostics.some(
      // require the failed collect result to remain visible
      (diagnostic) => diagnostic.path === `$.steps[${String(collectIndex)}]`,
    ), JSON.stringify(result.diagnostics));
    assert.equal(result.diagnostics.some(
      // preserve the exact successful cleanup result
      (diagnostic) => diagnostic.path === "$.steps[2]",
    ), false, JSON.stringify(result.diagnostics));
  });

  await testContext.test("self-labeled staged bytes differ from trusted source", () => {
    const fixture = mainCaptureFixture("workstation-runner-v1");
    const originalDigest = sha256Bytes(fixture.packageBytesByName.get("observer.mjs"));
    const changedBytes = Buffer.from("export const observer = false;\n");
    const changedDigest = sha256Bytes(changedBytes);
    fixture.packageBytesByName.set("observer.mjs", changedBytes);
    const changedPlan = structuredClone(fixture.materializedPlan);
    const observerInput = changedPlan.inputs.find(
      // select the staged observer plan input
      (input) => input.name === "observer.mjs",
    );
    observerInput.byteLength = changedBytes.byteLength;
    observerInput.sha256 = changedDigest;
    changedPlan.steps.at(-1).argv = changedPlan.steps.at(-1).argv.map(
      // make the malicious package internally self-consistent
      (argument) => argument.replaceAll(originalDigest, changedDigest),
    );
    fixture.capture.steps.at(-1).argv = fixture.capture.steps.at(-1).argv.map(
      // make the capture repeat the malicious digest label
      (argument) => argument.replaceAll(originalDigest, changedDigest),
    );
    fixture.capture.planSha256 = sha256Bytes(receipts.canonicalReceiptBytes(changedPlan));
    const result = receipts.validateSyntheticMainCapturePackage({
      captureBytes: receipts.canonicalReceiptBytes(fixture.capture),
      knownHostsBytes: fixture.knownHostsBytes,
      operationId: "workstation-runner-v1",
      packageBytesByName: fixture.packageBytesByName,
      trustedBytesByName: fixture.trustedBytesByName,
      trustPolicyBytes: fixture.trustPolicyBytes,
    });
    assert.equal(result.ok, false);
    assert.ok(result.diagnostics.some(
      // require independent trusted-source rejection
      (diagnostic) => diagnostic.code === "receipt.authority",
    ), JSON.stringify(result.diagnostics));
    assert.equal(result.canonicalEligible, false);
  });
});

// canonicalize exact bytes without ambiguous JSON structures
test("receipt v2 canonical bytes are deterministic and reject ambiguous values", () => {
  const bytes = receipts.canonicalReceiptBytes({ z: 2, a: [true, null] });
  assert.equal(bytes.toString("utf8"), '{"a":[true,null],"z":2}\n');
  assert.equal(receipts.receiptSha256(bytes), sha256Bytes(bytes));
  const sparse = [];
  sparse.length = 1;
  assert.throws(
    () => receipts.canonicalReceiptBytes(sparse),
    (error) => error?.code === "receipt.schema",
  );
  assert.throws(
    () => receipts.canonicalReceiptBytes({ value: -0 }),
    (error) => error?.code === "receipt.schema",
  );
});

// derive exact binding states without promoting fixture authority
test("synthetic v2 baseline derives bindings while remaining noncanonical", () => {
  const { input } = assemblyInput();
  const assembled = receipts.assembleSyntheticLiveStatusReceipt(input);
  assert.ok(assembled.artifactBytesBySha256 instanceof Map);
  assert.equal(assembled.receipt.contractVersion, "weather-backend-live-status/v2");
  assert.equal(Object.isFrozen(assembled.receipt), true);
  assert.equal(assembled.receipt.sequence, 0);
  assert.equal(assembled.receipt.phase, "baseline");
  assert.equal(assembled.receipt.generation, "node-v1");
  assert.deepEqual(assembled.receipt.authority, {
    blockers: [
      "gate6-authenticated-capture",
      "gate6-committed-measurement-tree",
      "gate6-deployed-source-provenance",
    ],
    canonicalEligible: false,
    custody: "same-main-uid-private-files",
    origin: "synthetic-fixture",
  });
  assert.deepEqual(assembled.receipt.captures.map(
    // retain distinct fixed profile order
    (capture) => capture.profile,
  ), ["blueberry-compose", "workstation-runner"]);
  const states = new Map(assembled.receipt.bindings.map(
    // index exact source-owned states
    (binding) => [binding.rootEntryId, binding.state],
  ));
  assert.equal(states.get("compose-command:deploy/compose.yaml#api"), "installed-active");
  assert.equal(states.get("compose-command:deploy/compose.yaml#migration"), "installed-disabled-inactive");
  assert.equal(states.get("compose-command:deploy/compose.yaml#worker"), "installed-active");
  assert.equal(states.get("docker-command:deploy/Dockerfile#server"), "installed-disabled-inactive");
  assert.equal(states.get("docker-command:deploy/web.Dockerfile#web"), "installed-disabled-inactive");
  assert.equal(states.get("package-script:package.json#job"), "installed-disabled-inactive");
  assert.equal(states.get("installer-allowlist:scripts/install-adjustment-maintenance-runner.sh#RUNNER_FILES:scripts/runner.mjs"), "not-installed");
  assert.equal(Object.hasOwn(assembled.receipt, "source"), false);
  assert.deepEqual(Object.keys(assembled.receipt.tooling).sort(), ["evaluator", "observer", "receiptWriter", "resourceCollector"]);
  assert.notEqual(
    assembled.receipt.captures[0].observation.docker.images[0].imageId,
    assembled.receipt.captures[0].observation.docker.images[1].imageId,
  );
  const validation = receipts.validateLiveStatusReceipt(assembled.receipt, {
    artifactBytesBySha256: assembled.artifactBytesBySha256,
    previousReceipt: null,
    writerNow: input.writerNow,
  });
  assert.equal(validation.ok, true, JSON.stringify(validation.diagnostics));
  assert.equal(validation.canonicalEligible, false);
});

// separate completed migration from failed supervision and healthy API state
test("completed migration stays inactive beside failed Compose and healthy API", () => {
  const { input } = assemblyInput({ completedMigration: true });
  const assembled = receipts.assembleSyntheticLiveStatusReceipt(input);
  const api = assembled.receipt.bindings.find(
    // select the persistent API binding
    (binding) => binding.rootEntryId.endsWith("#api"),
  );
  const migration = assembled.receipt.bindings.find(
    // select the one-shot migration binding
    (binding) => binding.rootEntryId.endsWith("#migration"),
  );
  assert.equal(api.state, "installed-active");
  assert.equal(migration.state, "installed-disabled-inactive");
  const blueberry = assembled.receipt.captures.find(
    // select the Compose capture
    (capture) => capture.profile === "blueberry-compose",
  );
  assert.equal(blueberry.observation.systemd[0].activeState, "failed");
  assert.equal(blueberry.observation.docker.containers.find(
    // select the independently healthy API
    (container) => container.service === "api",
  ).health, "healthy");
});

// keep canonical production custody unavailable to caller arguments
test("canonical append rejects caller capture and clock injection", async () => {
  for (const options of [
    { captures: [] },
    { clock: { now: () => "2026-11-13T18:00:00.000Z" } },
    { transport: {} },
    { adapters: {} },
  ]) {
    await assertRejectCode(
      // reject one caller-selected canonical surface
      () => receipts.appendLiveStatusReceipt(options),
      "receipt.authority",
    );
  }
});

// fail closed while actual Gate6 Main custody packages are absent
test("canonical append reports missing fixed Main custody without live work", async () => {
  await assert.rejects(
    () => receipts.appendLiveStatusReceipt(),
    (error) => error?.code === "receipt.authority" &&
      error?.path === "/home/ubuntu/.weather/backend-performance-evidence/v2" &&
      error?.cause?.code === "ENOENT",
  );
});

// keep production principal policy fixed while synthetic files use the invoking uid
test("production custody identity requires Main uid 1000 independently of test uid", () => {
  assert.deepEqual(receipts.validateSyntheticProductionCustodyIdentity({ uid: 1000 }), {
    canonicalEligible: false,
    ok: true,
  });
  assert.throws(
    () => receipts.validateSyntheticProductionCustodyIdentity({ uid: 1001 }),
    (error) => error?.code === "path.owner",
  );
  assert.throws(
    () => receipts.validateSyntheticProductionCustodyIdentity({ uid: 1000, invokingUid: process.getuid?.() }),
    (error) => error?.code === "path.owner",
  );
});

// reject caller-authored profile transport and authority substitutions
test("synthetic assembler rejects profile transport and digest substitutions", async (testContext) => {
  const cases = [
    {
      code: "receipt.schema",
      label: "caller authority",
      // add one unsupported authority field
      mutate(input) { input.authority = { canonicalEligible: true }; },
    },
    {
      code: "receipt.schema",
      label: "caller sequence",
      // add one unsupported sequence field
      mutate(input) { input.sequence = 99; },
    },
    {
      code: "receipt.schema",
      label: "caller generation",
      // add one unsupported generation field
      mutate(input) { input.generation = "rust-v1"; },
    },
    {
      code: "receipt.schema",
      label: "caller freshness limit",
      // add one unsupported freshness override
      mutate(input) { input.maxAgeMs = 3_600_000; },
    },
    {
      code: "receipt.schema",
      label: "caller global commit",
      // add one forbidden global source identity
      mutate(input) { input.source = { commit: "0".repeat(40) }; },
    },
    {
      code: "receipt.schema",
      label: "caller capture profile",
      // add one caller-selected capture profile
      mutate(input) { input.captures[0].profile = "blueberry-compose"; },
    },
    {
      code: "receipt.authority",
      label: "caller transport kind",
      // add one caller-selected transport kind
      mutate(input) { input.captures[0].transport.kind = "main-authenticated-ssh"; },
    },
    {
      code: "receipt.profile",
      label: "duplicate profile",
      // duplicate one fixed profile capture
      mutate(input) { input.captures[1] = structuredClone(input.captures[0]); },
    },
    {
      code: "receipt.authority",
      label: "local Blueberry transport",
      // substitute local process authority for SSH
      mutate(input) {
        const observation = JSON.parse(input.captures[0].observationBytes);
        input.captures[0].transport = transportFixture("workstation-runner", input.captures[0].observationBytes, {
          transportCompletedAt: observation.collection.completedAt,
          transportStartedAt: observation.collection.startedAt,
        });
      },
    },
    {
      code: "receipt.authority",
      label: "SSH workstation transport",
      // substitute SSH authority for local process custody
      mutate(input) {
        const observation = JSON.parse(input.captures[1].observationBytes);
        input.captures[1].transport = transportFixture("blueberry-compose", input.captures[1].observationBytes, {
          transportCompletedAt: observation.collection.completedAt,
          transportStartedAt: observation.collection.startedAt,
        });
      },
    },
    {
      code: "receipt.authority",
      label: "fingerprint",
      // substitute the frozen host-key fingerprint
      mutate(input) { input.captures[0].transport.ssh.hostKeyFingerprint = "SHA256:caller"; },
    },
    {
      code: "receipt.digest",
      label: "observation digest",
      // detach transport from exact observation bytes
      mutate(input) { input.captures[0].transport.observationSha256 = HASH_D; },
    },
  ];

  // isolate every caller-authored substitution
  for (const fixtureCase of cases) {
    await testContext.test(fixtureCase.label, () => {
      const { input } = assemblyInput();
      fixtureCase.mutate(input);
      assert.throws(
        () => receipts.assembleSyntheticLiveStatusReceipt(input),
        (error) => error?.code === fixtureCase.code,
      );
    });
  }
});

// reject incomplete binding evidence and offline runner transport
test("synthetic assembler rejects missing swapped and offline host evidence", async (testContext) => {
  await testContext.test("missing artifact", () => {
    const { input } = assemblyInput();
    const observation = JSON.parse(input.captures[0].observationBytes);
    observation.artifacts = observation.artifacts.filter(
      // remove the exact API artifact
      (artifact) => !artifact.bindingIds.some(
        // identify one API backlink
        (bindingId) => bindingId.includes("#api"),
      ),
    );
    input.captures[0] = fixtureCapture(observation, {
      transportCompletedAt: input.captures[0].transport.completedAt,
      transportStartedAt: input.captures[0].transport.startedAt,
    });
    assert.throws(
      () => receipts.assembleSyntheticLiveStatusReceipt(input),
      (error) => error?.code === "receipt.binding",
    );
  });

  await testContext.test("cross-context backlink", () => {
    const { input } = assemblyInput();
    const observation = JSON.parse(input.captures[0].observationBytes);
    const [first, second] = observation.artifacts;
    [first.bindingIds, second.bindingIds] = [second.bindingIds, first.bindingIds];
    input.captures[0] = fixtureCapture(observation, {
      transportCompletedAt: input.captures[0].transport.completedAt,
      transportStartedAt: input.captures[0].transport.startedAt,
    });
    assert.throws(
      () => receipts.assembleSyntheticLiveStatusReceipt(input),
      (error) => error?.code === "receipt.binding",
    );
  });

  await testContext.test("API as worker", () => {
    const { input } = assemblyInput();
    const observation = JSON.parse(input.captures[0].observationBytes);
    const worker = observation.docker.containers.find(
      // select the exact worker instance
      (container) => container.service === "worker",
    );
    worker.service = "api";
    input.captures[0] = fixtureCapture(observation, {
      transportCompletedAt: input.captures[0].transport.completedAt,
      transportStartedAt: input.captures[0].transport.startedAt,
    });
    assert.throws(
      () => receipts.assembleSyntheticLiveStatusReceipt(input),
      (error) => ["receipt.binding", "receipt.profile"].includes(error?.code),
    );
  });

  await testContext.test("server as web", () => {
    const { input } = assemblyInput();
    const observation = JSON.parse(input.captures[0].observationBytes);
    const webArtifact = observation.artifacts.find(
      // select the exact web image artifact
      (artifact) => artifact.imageId === WEB_IMAGE_ID,
    );
    assert.ok(webArtifact);
    webArtifact.imageId = SERVER_IMAGE_ID;
    input.captures[0] = fixtureCapture(observation, {
      transportCompletedAt: input.captures[0].transport.completedAt,
      transportStartedAt: input.captures[0].transport.startedAt,
    });
    assert.throws(
      () => receipts.assembleSyntheticLiveStatusReceipt(input),
      (error) => error?.code === "receipt.binding",
    );
  });

  await testContext.test("offline runner", () => {
    const { input } = assemblyInput();
    input.captures[1].transport.exitCode = 255;
    assert.throws(
      () => receipts.assembleSyntheticLiveStatusReceipt(input),
      (error) => error?.code === "receipt.authority",
    );
  });
});

// derive sequence generation and recovery from the exact predecessor graph
test("v2 phase graph derives rollback generation and repeated recovery", () => {
  const phases = [
    ["baseline", "node-v1"],
    ["pre-cutover", "mixed"],
    ["post-cutover", "rust-v1"],
    ["rollback-drill", "node-v1-rollback"],
    ["post-cutover", "rust-v1"],
    ["rollback-drill", "node-v1-rollback"],
    ["post-cutover", "rust-v1"],
  ];
  let previousReceiptBytes = null;
  let startMs = Date.parse("2026-11-13T18:00:00.000Z");

  // advance only through the fixed lifecycle graph
  for (let index = 0; index < phases.length; index += 1) {
    const [phase, expectedGeneration] = phases[index];
    const { input } = assemblyInput({ phase, previousReceiptBytes, startMs });
    const assembled = receipts.assembleSyntheticLiveStatusReceipt(input);
    assert.equal(assembled.receipt.sequence, index);
    assert.equal(assembled.receipt.generation, expectedGeneration);
    assert.equal(assembled.receipt.previousReceiptSha256, previousReceiptBytes === null
      ? null
      : receipts.receiptSha256(previousReceiptBytes));
    previousReceiptBytes = receipts.canonicalReceiptBytes(assembled.receipt);
    startMs += 60_000;
  }
  const baseline = receipts.assembleSyntheticLiveStatusReceipt(assemblyInput().input);
  const skipped = assemblyInput({
    phase: "post-cutover",
    previousReceiptBytes: receipts.canonicalReceiptBytes(baseline.receipt),
    startMs: Date.parse("2026-11-13T18:01:00.000Z"),
  });
  assert.throws(
    () => receipts.assembleSyntheticLiveStatusReceipt(skipped.input),
    (error) => error?.code === "receipt.phase",
  );
  const noncanonical = Buffer.from(`${receipts.canonicalReceiptBytes(baseline.receipt).toString("utf8").trim()} \n`);
  assert.throws(
    () => receipts.assembleSyntheticLiveStatusReceipt(assemblyInput({
      phase: "pre-cutover",
      previousReceiptBytes: noncanonical,
      startMs: Date.parse("2026-11-13T18:01:00.000Z"),
    }).input),
    (error) => ["chain.previous", "receipt.invalid", "receipt.schema"].includes(error?.code),
  );
});

// bind generation-selected native contexts to phase and observed executable identity
test("v2 generation-selected native bindings activate only after cutover", () => {
  const source = generationSelectedSourceFixture();
  const nativeEntryId = "native-binary:server-api";
  const nativeRootEntryId = "runtime-generation-command:scripts/schema-preserving-release.mjs#api";
  let previousReceiptBytes = null;
  let startMs = Date.parse("2026-11-13T18:00:00.000Z");
  const receiptsByPhase = new Map();

  // advance through the fixed lifecycle while retaining the same checked runtime map
  for (const [phase, expectedState] of [
    ["baseline", "installed-disabled-inactive"],
    ["pre-cutover", "installed-disabled-inactive"],
    ["post-cutover", "installed-active"],
    ["rollback-drill", "installed-disabled-inactive"],
  ]) {
    const { input } = assemblyInput({ phase, previousReceiptBytes, source, startMs });
    const assembled = receipts.assembleSyntheticLiveStatusReceipt(input);
    const binding = assembled.receipt.bindings.find(
      // select the native executable binding rather than the logical command row
      (candidate) => candidate.entryId === nativeEntryId &&
        candidate.rootEntryId === nativeRootEntryId,
    );
    assert.equal(binding?.state, expectedState);
    receiptsByPhase.set(phase, assembled.receipt);
    previousReceiptBytes = receipts.canonicalReceiptBytes(assembled.receipt);
    startMs += 60_000;
  }
  const preCutover = receiptsByPhase.get("pre-cutover");
  const mismatch = assemblyInput({
    phase: "post-cutover",
    previousReceiptBytes: receipts.canonicalReceiptBytes(preCutover),
    source,
    startMs: Date.parse("2026-11-13T18:02:00.000Z"),
  });
  const observation = JSON.parse(mismatch.input.captures[0].observationBytes);
  const api = observation.docker.containers.find(
    // select the production API container before substituting predecessor argv
    (container) => container.service === "api",
  );
  api.actualArgv = ["/usr/local/bin/node", "apps/api/dist/main.js"];
  api.configuredArgv = ["node", "apps/api/dist/main.js"];
  mismatch.input.captures[0] = fixtureCapture(observation, {
    transportCompletedAt: mismatch.input.captures[0].transport.completedAt,
    transportStartedAt: mismatch.input.captures[0].transport.startedAt,
  });
  assert.throws(
    () => receipts.assembleSyntheticLiveStatusReceipt(mismatch.input),
    (error) => error?.code === "receipt.binding" &&
      error.message.includes("does not execute its native artifact"),
  );
  const invalidSource = {
    ...source,
    runtimeMap: structuredClone(source.runtimeMap),
  };
  const invalidContext = invalidSource.runtimeMap.contexts.find(
    // select the exact native context before introducing a pre-cutover activation claim
    (context) => context.rootEntryId === nativeRootEntryId,
  );
  invalidContext.activationSourceByGeneration["node-v1"] = "container-inspect";
  invalidSource.runtimeMapBytes = receipts.canonicalReceiptBytes(invalidSource.runtimeMap);
  assert.throws(
    () => receipts.assembleSyntheticLiveStatusReceipt(
      assemblyInput({ source: invalidSource }).input,
    ),
    (error) => error?.code === "receipt.binding" &&
      error.message.includes("generation activation is unsupported"),
  );
});

// bind every incoming interval to writer time and predecessor chronology
test("v2 chronology rejects stale future overlap and backdating", async (testContext) => {
  const cases = [
    {
      code: "receipt.freshness",
      label: "stale workstation",
      // age one incoming host beyond ten minutes
      mutate(input) {
        const observation = JSON.parse(input.captures[1].observationBytes);
        observation.collection.startedAt = "2026-11-13T17:00:00.000Z";
        observation.collection.completedAt = "2026-11-13T17:00:01.000Z";
        input.captures[1] = fixtureCapture(observation, { transportCompletedAt: "2026-11-13T17:00:02.000Z", transportStartedAt: "2026-11-13T16:59:59.000Z" });
      },
    },
    {
      code: "receipt.freshness",
      label: "future workstation",
      // move one incoming host after writer time
      mutate(input) {
        const observation = JSON.parse(input.captures[1].observationBytes);
        observation.collection.startedAt = "2026-11-13T19:00:00.000Z";
        observation.collection.completedAt = "2026-11-13T19:00:01.000Z";
        input.captures[1] = fixtureCapture(observation, { transportCompletedAt: "2026-11-13T19:00:02.000Z", transportStartedAt: "2026-11-13T18:59:59.000Z" });
      },
    },
    {
      code: "receipt.chronology",
      label: "overlapping transports",
      // overlap the two retained capture intervals
      mutate(input) {
        const observation = JSON.parse(input.captures[1].observationBytes);
        observation.collection.startedAt = "2026-11-13T18:00:02.000Z";
        observation.collection.completedAt = "2026-11-13T18:00:03.000Z";
        input.captures[1] = fixtureCapture(observation, { transportCompletedAt: "2026-11-13T18:00:04.000Z", transportStartedAt: "2026-11-13T18:00:01.500Z" });
      },
    },
    {
      code: "receipt.chronology",
      label: "host outside transport",
      // move host collection before its transport
      mutate(input) {
        const observation = JSON.parse(input.captures[0].observationBytes);
        observation.collection.startedAt = "2026-11-13T17:59:59.000Z";
        input.captures[0] = fixtureCapture(observation, { transportCompletedAt: input.captures[0].transport.completedAt, transportStartedAt: input.captures[0].transport.startedAt });
      },
    },
  ];

  // isolate every chronology failure
  for (const fixtureCase of cases) {
    await testContext.test(fixtureCase.label, () => {
      const { input } = assemblyInput();
      fixtureCase.mutate(input);
      assert.throws(
        () => receipts.assembleSyntheticLiveStatusReceipt(input),
        (error) => error?.code === fixtureCase.code,
      );
    });
  }

  const boundary = assemblyInput();
  boundary.input.writerNow = new Date(Date.parse(boundary.input.captures[0].transport.completedAt) + receipts.LIVE_STATUS_MAX_AGE_MS).toISOString();
  assert.doesNotThrow(
    () => receipts.assembleSyntheticLiveStatusReceipt(boundary.input),
  );
  boundary.input.writerNow = new Date(Date.parse(boundary.input.writerNow) + 1).toISOString();
  assert.throws(
    () => receipts.assembleSyntheticLiveStatusReceipt(boundary.input),
    (error) => error?.code === "receipt.freshness",
  );

  const { input } = assemblyInput();
  const baseline = receipts.assembleSyntheticLiveStatusReceipt(input);
  const historical = receipts.validateLiveStatusReceipt(baseline.receipt, {
    artifactBytesBySha256: baseline.artifactBytesBySha256,
    enforceIncomingFreshness: false,
    previousReceipt: null,
    writerNow: "2026-12-13T18:00:00.000Z",
  });
  assert.equal(historical.ok, true, JSON.stringify(historical.diagnostics));

  const previous = receipts.canonicalReceiptBytes(baseline.receipt);
  const backdated = assemblyInput({
    phase: "pre-cutover",
    previousReceiptBytes: previous,
    startMs: Date.parse("2026-11-13T18:00:02.000Z"),
  });
  assert.throws(
    () => receipts.assembleSyntheticLiveStatusReceipt(backdated.input),
    (error) => error?.code === "receipt.chronology",
  );
});

// enforce reusable validation independently from assembly
test("v2 validator rechecks capture identity and interval disjointness", async (testContext) => {
  const cases = [
    {
      code: "receipt.profile",
      label: "stored profile",
      // detach the redundant profile from its retained observation
      mutate(receipt) { receipt.captures[1].profile = "blueberry-compose"; },
    },
    {
      code: "receipt.profile",
      label: "stored host role",
      // detach the redundant role from its retained observation
      mutate(receipt) { receipt.captures[1].hostRole = "blueberry"; },
    },
    {
      code: "receipt.chronology",
      label: "stored interval overlap",
      // overlap transports without changing the receipt endpoints
      mutate(receipt) { receipt.captures[1].transport.startedAt = "2026-11-13T18:00:02.500Z"; },
    },
  ];

  // isolate every reusable-validator invariant
  for (const fixtureCase of cases) {
    await testContext.test(fixtureCase.label, () => {
      const { input } = assemblyInput();
      const assembled = receipts.assembleSyntheticLiveStatusReceipt(input);
      const changed = structuredClone(assembled.receipt);
      fixtureCase.mutate(changed);
      const validation = receipts.validateLiveStatusReceipt(changed, {
        artifactBytesBySha256: assembled.artifactBytesBySha256,
        previousReceipt: null,
        writerNow: input.writerNow,
      });
      assert.equal(validation.ok, false);
      assert.ok(validation.diagnostics.some(
        // require the exact reusable-validator boundary
        (diagnostic) => diagnostic.code === fixtureCase.code,
      ), JSON.stringify(validation.diagnostics));
    });
  }
});

// reject cross-class references even when every sidecar digest exists
test("v2 validation preserves artifact source classes and exact references", async (testContext) => {
  const cases = [
    {
      label: "inventory as runtime map",
      // cross one checked-document class
      mutate(receipt) { receipt.artifacts.runtimeMap = structuredClone(receipt.artifacts.inventory); },
    },
    {
      label: "resource collector as observer",
      // cross one fixed tooling class
      mutate(receipt) { receipt.tooling.observer.artifact = structuredClone(receipt.tooling.resourceCollector.artifact); },
    },
    {
      label: "wrong observer source path",
      // detach one tool from its fixed source path
      mutate(receipt) { receipt.tooling.observer.sourcePath = receipt.tooling.resourceCollector.sourcePath; },
    },
    {
      label: "workstation source as Blueberry source",
      // cross one profile-owned observation class
      mutate(receipt) { receipt.captures[0].source = structuredClone(receipt.captures[1].source); },
    },
  ];

  // isolate every cross-class reference
  for (const fixtureCase of cases) {
    await testContext.test(fixtureCase.label, () => {
      const { input } = assemblyInput();
      const assembled = receipts.assembleSyntheticLiveStatusReceipt(input);
      const changed = structuredClone(assembled.receipt);
      fixtureCase.mutate(changed);
      const validation = receipts.validateLiveStatusReceipt(changed, {
        artifactBytesBySha256: assembled.artifactBytesBySha256,
        previousReceipt: null,
        writerNow: input.writerNow,
      });
      assert.equal(validation.ok, false);
      assert.ok(validation.diagnostics.some(
        // require an exact source or digest rejection
        (diagnostic) => ["receipt.binding", "receipt.digest", "receipt.schema"].includes(diagnostic.code),
      ), JSON.stringify(validation.diagnostics));
      assert.equal(validation.canonicalEligible, false);
    });
  }

  await testContext.test("missing sidecar", () => {
    const { input } = assemblyInput();
    const assembled = receipts.assembleSyntheticLiveStatusReceipt(input);
    const artifacts = new Map(assembled.artifactBytesBySha256);
    artifacts.delete(assembled.receipt.artifacts.inventory.sha256);
    const validation = receipts.validateLiveStatusReceipt(assembled.receipt, {
      artifactBytesBySha256: artifacts,
      previousReceipt: null,
      writerNow: input.writerNow,
    });
    assert.equal(validation.ok, false);
    assert.ok(validation.diagnostics.some(
      // require the missing content-addressed byte rejection
      (diagnostic) => diagnostic.code === "receipt.digest",
    ), JSON.stringify(validation.diagnostics));
  });
});

// keep deployed source collection separate from the two host profile plans
test("deployed source capture plan is finite separate and immutable", () => {
  assert.deepEqual(Object.keys(receipts.MAIN_CAPTURE_PLANS).sort(), [
    "blueberry-compose-v1",
    "workstation-runner-v1",
  ]);
  assert.equal(receipts.DEPLOYED_SOURCE_CAPTURE_PLAN.operationId, "blueberry-deployed-source-v1");
  assert.equal(receipts.DEPLOYED_SOURCE_CAPTURE_PLAN.steps.length, 1);
  assert.equal(receipts.DEPLOYED_SOURCE_CAPTURE_PLAN.steps[0].program, "/usr/bin/ssh");
  assert.equal(receipts.DEPLOYED_SOURCE_CAPTURE_PLAN.steps[0].stdinKind, "protected-sudo-password");
  assertDeepFrozen(receipts.DEPLOYED_SOURCE_CAPTURE_PLAN);
  const serialized = JSON.stringify(receipts.DEPLOYED_SOURCE_CAPTURE_PLAN);
  assert.doesNotMatch(serialized, /passwordBytes|passwordDigest|passwordLength|secret/iu);
  assert.match(serialized, /blueberry-deployed-source-v1/u);
});

// execute the deployed helper only through its invalid-operation preflight
test("deployed source helper rejects unknown operation before host reads", async () => {
  const command = receipts.DEPLOYED_SOURCE_CAPTURE_PLAN.steps[0].argv.at(-1);
  const encoded = /base64\.b64decode\("([A-Za-z0-9+/=]+)"\)/u.exec(command);
  assert.ok(encoded);
  const helper = Buffer.from(encoded[1], "base64").toString("utf8");
  const rootDir = await privateRoot();

  try {
    const result = await runInvalidRootHelper(helper, ["caller-operation"], rootDir);
    assert.notEqual(result.exitCode, 0);
    assert.equal(result.stdout.byteLength, 0);
    assert.ok(result.stderr.byteLength <= 65_536);
  } finally {
    await rm(rootDir, { force: true, recursive: true });
  }
});

// prove the shared custody proof and writer path without granting canonical authority
test("synthetic Main custody validates raw source proofs and appends noncanonically", async (testContext) => {
  await testContext.test("distinct typed commits", async () => {
    const rootDir = await privateRoot();

    try {
      const fixture = await writeSyntheticMainCustody(rootDir);
      let clockCalls = 0;
      const result = await receipts.appendSyntheticMainCustodyReceipt({
        clock: {
          // sample one writer-owned time after proof validation
          now() {
            clockCalls += 1;
            return fixture.intervals.writerNow;
          },
        },
        rootDir,
      });
      assert.equal(clockCalls, 1);
      assert.equal(result.canonicalEligible, false);
      assert.match(result.path, /\/live-status\/receipts\/000000-baseline\.json$/u);
      assert.equal(result.receipt.authority.canonicalEligible, false);
      assert.equal(result.receipt.authority.origin, "synthetic-main-custody");
      assert.deepEqual(result.receipt.authority.blockers, ["synthetic-custody"]);
      assert.equal(result.receipt.authority.deployedSourceProofSha256, sha256Bytes(receipts.canonicalReceiptBytes(fixture.deployedProof)));
      assert.equal(result.receipt.authority.measurementSourceReceiptSha256, sha256Bytes(receipts.canonicalReceiptBytes(fixture.measurementReceipt)));
      assert.equal(result.receipt.artifacts.capturePolicy.sha256, sha256Bytes(CAPTURE_POLICY_BYTES));
      assert.equal(result.receipt.artifacts.knownHosts.sha256, KNOWN_HOSTS_SHA256);
      assert.notEqual(fixture.measurementReceipt.commit, DEPLOYED_COMMIT);
      assert.equal(fixture.hostBundle.currentRelease.identity.mode, "0600");
      assert.equal(fixture.hostBundle.activeAlias.identity.type, "symbolic-link");
      assert.equal(Object.hasOwn(fixture.hostBundle.activeAlias, "raw"), false);
      assert.deepEqual(await readFile(result.path), receipts.canonicalReceiptBytes(result.receipt));
      assert.equal((await lstat(result.path)).mode & 0o777, 0o600);
      const retained = new Set(await readdir(join(rootDir, "live-status", "artifacts")));

      // require every raw proof document as a content-addressed sidecar
      for (const bytes of fixture.sourceDocumentBytes) {
        assert.equal(retained.has(`${sha256Bytes(bytes)}.bin`), true);
      }
    } finally {
      await rm(rootDir, { force: true, recursive: true });
    }
  });

  await testContext.test("independently equal typed commits", async () => {
    const rootDir = await privateRoot();

    try {
      const fixture = await writeSyntheticMainCustody(rootDir, { measurementCommit: DEPLOYED_COMMIT });
      const result = await receipts.appendSyntheticMainCustodyReceipt({
        clock: { now: () => fixture.intervals.writerNow },
        rootDir,
      });
      assert.equal(result.canonicalEligible, false);
      assert.equal(fixture.measurementReceipt.commit, DEPLOYED_COMMIT);
    } finally {
      await rm(rootDir, { force: true, recursive: true });
    }
  });

  await testContext.test("historical writer JSON encoding", async () => {
    const rootDir = await privateRoot();

    try {
      const fixture = await writeSyntheticMainCustody(rootDir, { historicalWriterEncoding: true });
      const result = await receipts.appendSyntheticMainCustodyReceipt({
        clock: { now: () => fixture.intervals.writerNow },
        rootDir,
      });
      assert.equal(result.canonicalEligible, false);
      assert.equal(result.receipt.authority.origin, "synthetic-main-custody");
    } finally {
      await rm(rootDir, { force: true, recursive: true });
    }
  });
});

// preserve each historical member's retained measurement proof
test("later custody append preserves a distinct retained historical source snapshot", async () => {
  const rootDir = await privateRoot();

  try {
    const fixture = await writeSyntheticMainCustody(rootDir);
    const baseline = await receipts.appendSyntheticMainCustodyReceipt({
      clock: { now: () => fixture.intervals.writerNow },
      rootDir,
    });
    await rewriteHistoricalMeasurementSnapshot(rootDir, baseline.path);
    const intervals = await rewriteSyntheticCustodyCaptures(
      rootDir,
      "pre-cutover",
      Date.parse("2026-11-13T18:01:00.000Z"),
    );
    const preCutover = await receipts.appendSyntheticMainCustodyReceipt({
      clock: { now: () => intervals.writerNow },
      rootDir,
    });
    assert.equal(preCutover.canonicalEligible, false);
    assert.equal(preCutover.receipt.sequence, 1);
    assert.equal(preCutover.receipt.phase, "pre-cutover");
    assert.equal(preCutover.receipt.previousReceiptSha256, sha256Bytes(await readFile(baseline.path)));
  } finally {
    await rm(rootDir, { force: true, recursive: true });
  }
});

// preserve retained v1 plans beside later active native descriptors
test("later active native descriptors preserve an immutable retained-v1 predecessor", async () => {
  const rootDir = await privateRoot();
  let activeModulePath = null;

  try {
    const fixture = await writeSyntheticMainCustody(rootDir);
    const baseline = await receipts.appendSyntheticMainCustodyReceipt({
      clock: { now: () => fixture.intervals.writerNow },
      rootDir,
    });
    await rewriteHistoricalMeasurementSnapshot(rootDir, baseline.path);
    activeModulePath = await createMutatedActiveReceiptModule();
    const result = await validateWithMutatedActiveReceiptModule(rootDir, activeModulePath);
    assert.deepEqual(result, {
      canonicalEligible: false,
      ok: true,
    });
  } finally {
    // remove one temporary active implementation module
    if (activeModulePath !== null) {
      await rm(activeModulePath, { force: true });
    }
    await rm(rootDir, { force: true, recursive: true });
  }
});

// reject a retained tool that no longer matches its historical measurement proof
test("later custody append rejects a historical tooling cross-proof mismatch", async () => {
  const rootDir = await privateRoot();

  try {
    const fixture = await writeSyntheticMainCustody(rootDir);
    const baseline = await receipts.appendSyntheticMainCustodyReceipt({
      clock: { now: () => fixture.intervals.writerNow },
      rootDir,
    });
    await rewriteHistoricalMeasurementSnapshot(rootDir, baseline.path, { mismatchTooling: true });
    const intervals = await rewriteSyntheticCustodyCaptures(
      rootDir,
      "pre-cutover",
      Date.parse("2026-11-13T18:01:00.000Z"),
    );
    await assertRejectCode(
      () => receipts.appendSyntheticMainCustodyReceipt({
        clock: { now: () => intervals.writerNow },
        rootDir,
      }),
      ["chain.previous", "receipt.digest"],
    );
  } finally {
    await rm(rootDir, { force: true, recursive: true });
  }
});

// reject a retained native plan detached from its historical helper source
test("later custody append rejects a historical native-plan cross-proof mismatch", async () => {
  const rootDir = await privateRoot();

  try {
    const fixture = await writeSyntheticMainCustody(rootDir);
    const baseline = await receipts.appendSyntheticMainCustodyReceipt({
      clock: { now: () => fixture.intervals.writerNow },
      rootDir,
    });
    await rewriteHistoricalMeasurementSnapshot(rootDir, baseline.path, { mismatchPlan: true });
    const intervals = await rewriteSyntheticCustodyCaptures(
      rootDir,
      "pre-cutover",
      Date.parse("2026-11-13T18:01:00.000Z"),
    );
    await assertRejectCode(
      () => receipts.appendSyntheticMainCustodyReceipt({
        clock: { now: () => intervals.writerNow },
        rootDir,
      }),
      ["chain.previous", "receipt.authority", "receipt.digest"],
    );
  } finally {
    await rm(rootDir, { force: true, recursive: true });
  }
});

// reject complete raw semantics that no longer derive the stored observation
test("later custody append rejects historical raw semantic drift", async () => {
  const rootDir = await privateRoot();

  try {
    const fixture = await writeSyntheticMainCustody(rootDir);
    const baseline = await receipts.appendSyntheticMainCustodyReceipt({
      clock: { now: () => fixture.intervals.writerNow },
      rootDir,
    });
    await rewriteHistoricalMeasurementSnapshot(rootDir, baseline.path, { mismatchRawSemantic: true });
    const intervals = await rewriteSyntheticCustodyCaptures(
      rootDir,
      "pre-cutover",
      Date.parse("2026-11-13T18:01:00.000Z"),
    );
    await assertRejectCode(
      () => receipts.appendSyntheticMainCustodyReceipt({
        clock: { now: () => intervals.writerNow },
        rootDir,
      }),
      ["chain.previous", "receipt.binding", "receipt.digest"],
    );
  } finally {
    await rm(rootDir, { force: true, recursive: true });
  }
});

// reject a stored projection detached from complete retained raw evidence
test("later custody append rejects historical observation projection drift", async () => {
  const rootDir = await privateRoot();

  try {
    const fixture = await writeSyntheticMainCustody(rootDir);
    const baseline = await receipts.appendSyntheticMainCustodyReceipt({
      clock: { now: () => fixture.intervals.writerNow },
      rootDir,
    });
    await rewriteHistoricalMeasurementSnapshot(rootDir, baseline.path, { mismatchProjection: true });
    const intervals = await rewriteSyntheticCustodyCaptures(
      rootDir,
      "pre-cutover",
      Date.parse("2026-11-13T18:01:00.000Z"),
    );
    await assertRejectCode(
      () => receipts.appendSyntheticMainCustodyReceipt({
        clock: { now: () => intervals.writerNow },
        rootDir,
      }),
      ["chain.previous", "receipt.binding", "receipt.digest"],
    );
  } finally {
    await rm(rootDir, { force: true, recursive: true });
  }
});

// reject self-consistent custody substitutions across every proof class
test("synthetic Main custody rejects source proof metadata and join substitutions", async (testContext) => {
  const cases = [
    { code: "receipt.binding", label: "marker mode", options: { markerMode: "0644" } },
    { code: "receipt.binding", label: "active alias", options: { aliasTarget: "../releases/2026.11.12-1.env" } },
    { code: "receipt.schema", label: "credential-like environment key", options: { extraEnvironmentLine: "DATABASE_PASSWORD=secret" } },
    { code: ["receipt.binding", "receipt.digest"], label: "ready differs from stage", options: { readyChanged: true } },
    { code: "receipt.binding", label: "recertified server source", options: { recertifiedServerImage: `ghcr.io/anstosa/weather-server@sha256:${"e".repeat(64)}` } },
    { code: "receipt.binding", label: "operator bundle", options: { observationOperatorBundle: HASH_A } },
    { code: "receipt.authority", label: "git show vector", options: { gitArgMutation: "package.json" } },
    { code: "receipt.authority", label: "git environment", options: { gitEnvironment: { LANG: "C" } } },
    { code: "receipt.authority", label: "preparer environment", options: { preparerEnvironment: { ...NATIVE_SOURCE_ENVIRONMENT, GITHUB_TOKEN: "secret" } } },
    { code: "receipt.digest", label: "measurement file digest", options: { measurementDigestPath: "package.json" } },
    { code: "receipt.authority", label: "deployed capture program", options: { deployedCaptureProgram: "/bin/sh" } },
    { code: "receipt.authority", label: "deployed retained plan", options: { deployedPlanMutation: true } },
    { code: "receipt.schema", label: "historical duplicate key", options: { historicalDuplicateKey: true } },
  ];

  // isolate every independently bound proof substitution
  for (const fixtureCase of cases) {
    await testContext.test(fixtureCase.label, async () => {
      const rootDir = await privateRoot();

      try {
        const fixture = await writeSyntheticMainCustody(rootDir, fixtureCase.options);
        await assertRejectCode(
          () => receipts.appendSyntheticMainCustodyReceipt({
            clock: { now: () => fixture.intervals.writerNow },
            rootDir,
          }),
          fixtureCase.code,
        );
      } finally {
        await rm(rootDir, { force: true, recursive: true });
      }
    });
  }
});

// bind custody trust inputs to exact private file metadata
test("synthetic Main custody rejects unsafe known-hosts metadata", async () => {
  const rootDir = await privateRoot();

  try {
    const fixture = await writeSyntheticMainCustody(rootDir);
    await chmod(join(rootDir, "trust", "blueberry-known-hosts"), 0o644);
    await assertRejectCode(
      () => receipts.appendSyntheticMainCustodyReceipt({
        clock: { now: () => fixture.intervals.writerNow },
        rootDir,
      }),
      ["path.mode", "receipt.authority"],
    );
  } finally {
    await rm(rootDir, { force: true, recursive: true });
  }
});

// reject a linked trust anchor outside the one private custody identity
test("synthetic Main custody rejects a hard-linked known-hosts file", async () => {
  const rootDir = await privateRoot();

  try {
    const fixture = await writeSyntheticMainCustody(rootDir);
    await link(
      join(rootDir, "trust", "blueberry-known-hosts"),
      join(rootDir, "trust", "blueberry-known-hosts.link"),
    );
    await assertRejectCode(
      () => receipts.appendSyntheticMainCustodyReceipt({
        clock: { now: () => fixture.intervals.writerNow },
        rootDir,
      }),
      ["path.type", "receipt.authority"],
    );
  } finally {
    await rm(rootDir, { force: true, recursive: true });
  }
});

// retain pinned authority inputs until the final receipt is published
test("synthetic Main custody rejects closing input replacement growth and membership drift", async (testContext) => {
  const cases = [
    {
      label: "trust parent replacement",
      // replace one admitted authority parent after its bytes are parsed
      mutate: ({ rootDir }) => {
        renameSync(join(rootDir, "trust"), join(rootDir, "trust-replaced"));
        mkdirSync(join(rootDir, "trust"), { mode: 0o700 });
      },
    },
    {
      label: "measurement leaf replacement",
      // replace one admitted proof leaf with identical bytes under a new identity
      mutate: ({ fixture, rootDir }) => {
        const path = join(rootDir, "measurement-source-receipt.json");
        renameSync(path, `${path}.replaced`);
        writeFileSync(path, receipts.canonicalReceiptBytes(fixture.measurementReceipt), { mode: 0o600 });
      },
    },
    {
      label: "incoming parent replacement",
      // replace the complete capture parent after both profile captures are loaded
      mutate: ({ rootDir }) => {
        renameSync(join(rootDir, "synthetic-captures"), join(rootDir, "synthetic-captures-replaced"));
        mkdirSync(join(rootDir, "synthetic-captures"), { mode: 0o700 });
      },
    },
    {
      label: "post-admission cap growth",
      // grow one admitted source leaf sparsely past the fixed byte cap
      mutate: ({ rootDir }) => {
        truncateSync(join(rootDir, "measurement-source-git-capture.json"), (64 * 1024 * 1024) + 1);
      },
    },
    {
      label: "trust membership drift",
      // add one undeclared trust member after exact admission
      mutate: ({ rootDir }) => {
        writeFileSync(join(rootDir, "trust", "unexpected"), "unexpected\n", { mode: 0o600 });
      },
    },
    {
      label: "custody root membership drift",
      // add one undeclared root member after writer-layout refresh
      mutate: ({ rootDir }) => {
        writeFileSync(join(rootDir, "unexpected"), "unexpected\n", { mode: 0o600 });
      },
    },
  ];

  // isolate every closing-time authority mutation
  for (const fixtureCase of cases) {
    await testContext.test(fixtureCase.label, async () => {
      const rootDir = await privateRoot();

      try {
        const fixture = await writeSyntheticMainCustody(rootDir);
        await assertRejectCode(
          () => receipts.appendSyntheticMainCustodyReceipt({
            clock: {
              // mutate only after the loader has admitted every source document
              now() {
                fixtureCase.mutate({ fixture, rootDir });
                return fixture.intervals.writerNow;
              },
            },
            rootDir,
          }),
          ["chain.member", "path.type", "receipt.authority"],
        );
      } finally {
        await rm(rootDir, { force: true, recursive: true });
      }
    });
  }
});

// require exact input-directory membership before authority derivation
test("synthetic Main custody rejects unexpected fixed input members", async (testContext) => {
  const cases = [
    ["custody root", (rootDir) => join(rootDir, "unexpected")],
    ["trust directory", (rootDir) => join(rootDir, "trust", "unexpected")],
    ["deployed source", (rootDir) => join(rootDir, "deployed-source", "unexpected")],
    ["capture profiles", (rootDir) => join(rootDir, "synthetic-captures", "unexpected")],
  ];

  // isolate every fixed directory boundary
  for (const [label, resolveUnexpected] of cases) {
    await testContext.test(label, async () => {
      const rootDir = await privateRoot();

      try {
        const fixture = await writeSyntheticMainCustody(rootDir);
        await writeFile(resolveUnexpected(rootDir), "unexpected\n", { mode: 0o600 });
        await assertRejectCode(
          () => receipts.appendSyntheticMainCustodyReceipt({
            clock: { now: () => fixture.intervals.writerNow },
            rootDir,
          }),
          ["chain.member", "receipt.authority"],
        );
      } finally {
        await rm(rootDir, { force: true, recursive: true });
      }
    });
  }
});

// close every partially acquired authority descriptor on admission failure
test("synthetic Main custody closes partial input pins after a late failure", async () => {
  const rootDir = await privateRoot();

  try {
    const fixture = await writeSyntheticMainCustody(rootDir);
    await chmod(join(rootDir, "synthetic-captures", "workstation-runner", "transport.json"), 0o644);
    const before = (await readdir("/proc/self/fd")).length;

    // repeat the late failure to expose one descriptor leak per attempt
    for (let index = 0; index < 8; index += 1) {
      await assertRejectCode(
        () => receipts.appendSyntheticMainCustodyReceipt({
          clock: { now: () => fixture.intervals.writerNow },
          rootDir,
        }),
        ["path.mode", "receipt.authority"],
      );
    }
    const after = (await readdir("/proc/self/fd")).length;
    assert.equal(after, before);
  } finally {
    await rm(rootDir, { force: true, recursive: true });
  }
});

// reject unsafe opened metadata across source trust and incoming leaves
test("synthetic Main custody rejects unsafe fixed input leaf identities", async (testContext) => {
  const cases = [
    {
      label: "source mode",
      // widen one measurement source document
      mutate: async (rootDir) => chmod(join(rootDir, "measurement-source-receipt.json"), 0o644),
    },
    {
      label: "trust link count",
      // retain one private trust leaf under a second external name
      mutate: async (rootDir) => {
        const alias = `${rootDir}-known-hosts-link`;
        await link(join(rootDir, "trust", "blueberry-known-hosts"), alias);
        return alias;
      },
    },
    {
      label: "incoming nonregular leaf",
      // replace one capture document with a directory
      mutate: async (rootDir) => {
        const path = join(rootDir, "synthetic-captures", "blueberry-compose", "observation.json");
        await rm(path);
        await mkdir(path, { mode: 0o700 });
      },
    },
    {
      label: "incoming symbolic leaf",
      // replace one capture transport with a symbolic link
      mutate: async (rootDir) => {
        const directory = join(rootDir, "synthetic-captures", "workstation-runner");
        await rm(join(directory, "transport.json"));
        await symlink("observation.json", join(directory, "transport.json"));
      },
    },
  ];

  // isolate every opened leaf identity violation
  for (const fixtureCase of cases) {
    await testContext.test(fixtureCase.label, async () => {
      const rootDir = await privateRoot();
      let externalPath = null;

      try {
        const fixture = await writeSyntheticMainCustody(rootDir);
        externalPath = await fixtureCase.mutate(rootDir) ?? null;
        await assertRejectCode(
          () => receipts.appendSyntheticMainCustodyReceipt({
            clock: { now: () => fixture.intervals.writerNow },
            rootDir,
          }),
          ["path.mode", "path.symlink", "path.type", "receipt.authority"],
        );
      } finally {
        // remove one external hard-link alias separately
        if (externalPath !== null) {
          await rm(externalPath, { force: true });
        }
        await rm(rootDir, { force: true, recursive: true });
      }
    });
  }
});

// reject every caller-selected custody authority or chain input
test("synthetic Main custody facade rejects promotion and phase inputs", async (testContext) => {
  const rootDir = await privateRoot();
  const clock = { now: () => "2026-11-13T18:00:08.000Z" };

  try {
    for (const [label, extra] of [
      ["canonical flag", { canonicalEligible: true }],
      ["phase", { phase: "baseline" }],
      ["captures", { captures: [] }],
      ["transport", { transport: {} }],
    ]) {
      await testContext.test(label, async () => {
        await assertRejectCode(
          () => receipts.appendSyntheticMainCustodyReceipt({ clock, rootDir, ...extra }),
          "receipt.authority",
        );
      });
    }
  } finally {
    await rm(rootDir, { force: true, recursive: true });
  }
});

// prevent production admission from promoting fixture history
test("canonical chain admission rejects a synthetic predecessor", async () => {
  const rootDir = await privateRoot();

  try {
    await appendPhase(rootDir, "baseline", Date.parse("2026-11-13T18:00:00.000Z"));
    await assertRejectCode(
      () => receipts.validateSyntheticCanonicalChainAdmission({ rootDir }),
      "receipt.authority",
    );
  } finally {
    await rm(rootDir, { force: true, recursive: true });
  }
});

// keep the shared admission seam nonpromotable even with empty history
test("canonical chain admission accepts empty private history without authority", async () => {
  const rootDir = await privateRoot();

  try {
    await mkdir(join(rootDir, "live-status", "artifacts"), { mode: 0o700, recursive: true });
    await mkdir(join(rootDir, "live-status", "receipts"), { mode: 0o700 });
    const result = await receipts.validateSyntheticCanonicalChainAdmission({ rootDir });
    assert.deepEqual(result, { canonicalEligible: false, ok: true });
  } finally {
    await rm(rootDir, { force: true, recursive: true });
  }
});

// persist structural documents through private create-once storage
test("synthetic append writes private canonical sidecars and contiguous receipts", async () => {
  const rootDir = await privateRoot();

  try {
    const baseline = await appendPhase(rootDir, "baseline", Date.parse("2026-11-13T18:00:00.000Z"));
    const pre = await appendPhase(rootDir, "pre-cutover", Date.parse("2026-11-13T18:01:00.000Z"));
    const post = await appendPhase(rootDir, "post-cutover", Date.parse("2026-11-13T18:02:00.000Z"));
    const rollback = await appendPhase(rootDir, "rollback-drill", Date.parse("2026-11-13T18:03:00.000Z"));
    const recovery = await appendPhase(rootDir, "post-cutover", Date.parse("2026-11-13T18:04:00.000Z"));
    const repeatedRollback = await appendPhase(rootDir, "rollback-drill", Date.parse("2026-11-13T18:05:00.000Z"));
    const repeatedRecovery = await appendPhase(rootDir, "post-cutover", Date.parse("2026-11-13T18:06:00.000Z"));
    assert.equal(baseline.canonicalEligible, false);
    assert.equal(pre.canonicalEligible, false);
    assert.match(baseline.path, /\/receipts\/000000-baseline\.json$/u);
    assert.match(pre.path, /\/receipts\/000001-pre-cutover\.json$/u);
    assert.match(post.path, /\/receipts\/000002-post-cutover\.json$/u);
    assert.match(rollback.path, /\/receipts\/000003-rollback-drill\.json$/u);
    assert.match(recovery.path, /\/receipts\/000004-post-cutover\.json$/u);
    assert.match(repeatedRollback.path, /\/receipts\/000005-rollback-drill\.json$/u);
    assert.match(repeatedRecovery.path, /\/receipts\/000006-post-cutover\.json$/u);
    assert.equal(rollback.receipt.generation, "node-v1-rollback");
    assert.equal(recovery.receipt.previousReceiptSha256, rollback.sha256);
    assert.equal(repeatedRollback.receipt.previousReceiptSha256, recovery.sha256);
    assert.equal(repeatedRecovery.receipt.previousReceiptSha256, repeatedRollback.sha256);
    assert.equal((await lstat(baseline.path)).mode & 0o777, 0o600);
    assert.equal((await lstat(pre.path)).nlink, 1);
    assert.equal((await lstat(join(rootDir, "live-status"))).mode & 0o777, 0o700);
    assert.equal((await lstat(join(rootDir, "live-status", "receipts"))).uid, process.getuid());
    assert.deepEqual(await readFile(pre.path), receipts.canonicalReceiptBytes(pre.receipt));
    const artifactDir = join(rootDir, "live-status", "artifacts");
    const names = await readdir(artifactDir);
    assert.ok(names.length >= 7);
    assert.equal(names.includes(`${baseline.sha256}.bin`), false);
    assert.equal(names.includes(`${pre.sha256}.bin`), false);

    // reverify every content-addressed sidecar
    for (const name of names) {
      const path = join(artifactDir, name);
      const bytes = await readFile(path);
      const details = await lstat(path);
      assert.equal(name, `${sha256Bytes(bytes)}.bin`);
      assert.equal(details.mode & 0o777, 0o600);
      assert.equal(details.nlink, 1);
    }
  } finally {
    await rm(rootDir, { force: true, recursive: true });
  }
});

// reject unsafe paths tampered members and concurrent forks
test("synthetic append rejects custody tampering holes and races", async (testContext) => {
  await testContext.test("traversal", async () => {
    const rootDir = await privateRoot();
    let clockCalls = 0;

    try {
      const { input } = assemblyInput();
      await assertRejectCode(
        () => receipts.appendSyntheticLiveStatusReceipt({
          captures: input.captures,
          clock: {
            // record any premature writer-time read
            now() {
              clockCalls += 1;
              return input.writerNow;
            },
          },
          inventoryBytes: input.inventoryBytes,
          measurementSourceReceiptBytes: input.measurementSourceReceiptBytes,
          phase: input.phase,
          relativeDirectory: "../escape",
          rootDir,
          runtimeMapBytes: input.runtimeMapBytes,
        }),
        "path.traversal",
      );
      assert.equal(clockCalls, 0);
    } finally {
      await rm(rootDir, { force: true, recursive: true });
    }
  });

  await testContext.test("symlink", async () => {
    const rootDir = await privateRoot();
    const outside = await privateRoot();

    try {
      await symlink(outside, join(rootDir, "live-status"));
      await assertRejectCode(
        () => appendPhase(rootDir, "baseline", Date.parse("2026-11-13T18:00:00.000Z")),
        "path.symlink",
      );
    } finally {
      await rm(rootDir, { force: true, recursive: true });
      await rm(outside, { force: true, recursive: true });
    }
  });

  await testContext.test("output parent replacement", async () => {
    const rootDir = await privateRoot();
    const detached = `${join(rootDir, "live-status")}.detached`;
    const { input } = assemblyInput();

    try {
      await assertRejectCode(
        () => receipts.appendSyntheticLiveStatusReceipt({
          captures: input.captures,
          clock: {
            // replace the admitted output tree after chain validation
            now() {
              const output = join(rootDir, "live-status");
              renameSync(output, detached);
              mkdirSync(join(output, "artifacts"), { mode: 0o700, recursive: true });
              mkdirSync(join(output, "receipts"), { mode: 0o700 });
              return input.writerNow;
            },
          },
          inventoryBytes: input.inventoryBytes,
          measurementSourceReceiptBytes: input.measurementSourceReceiptBytes,
          phase: input.phase,
          relativeDirectory: "live-status",
          rootDir,
          runtimeMapBytes: input.runtimeMapBytes,
        }),
        ["path.symlink", "path.type", "chain.member"],
      );
    } finally {
      await rm(rootDir, { force: true, recursive: true });
      await rm(detached, { force: true, recursive: true });
    }
  });

  await testContext.test("receipt parent symlink replacement", async () => {
    const rootDir = await privateRoot();
    const outside = await privateRoot();
    const { input } = assemblyInput();

    try {
      await assertRejectCode(
        () => receipts.appendSyntheticLiveStatusReceipt({
          captures: input.captures,
          clock: {
            // replace the admitted receipt directory after chain validation
            now() {
              const receiptDirectory = join(rootDir, "live-status", "receipts");
              renameSync(receiptDirectory, `${receiptDirectory}.detached`);
              symlinkSync(outside, receiptDirectory);
              return input.writerNow;
            },
          },
          inventoryBytes: input.inventoryBytes,
          measurementSourceReceiptBytes: input.measurementSourceReceiptBytes,
          phase: input.phase,
          relativeDirectory: "live-status",
          rootDir,
          runtimeMapBytes: input.runtimeMapBytes,
        }),
        ["path.symlink", "path.type", "chain.member"],
      );
    } finally {
      await rm(rootDir, { force: true, recursive: true });
      await rm(outside, { force: true, recursive: true });
    }
  });

  await testContext.test("unsafe root mode", async () => {
    const rootDir = await privateRoot();

    try {
      await chmod(rootDir, 0o755);
      await assertRejectCode(
        () => appendPhase(rootDir, "baseline", Date.parse("2026-11-13T18:00:00.000Z")),
        "path.mode",
      );
    } finally {
      await rm(rootDir, { force: true, recursive: true });
    }
  });

  await testContext.test("unexpected member", async () => {
    const rootDir = await privateRoot();

    try {
      await appendPhase(rootDir, "baseline", Date.parse("2026-11-13T18:00:00.000Z"));
      await writeFile(join(rootDir, "live-status", "receipts", "unexpected.json"), "{}\n", { mode: 0o600 });
      await assertRejectCode(
        () => appendPhase(rootDir, "pre-cutover", Date.parse("2026-11-13T18:01:00.000Z")),
        "chain.member",
      );
    } finally {
      await rm(rootDir, { force: true, recursive: true });
    }
  });

  await testContext.test("unexpected artifact member", async () => {
    const rootDir = await privateRoot();

    try {
      await appendPhase(rootDir, "baseline", Date.parse("2026-11-13T18:00:00.000Z"));
      await writeFile(
        join(rootDir, "live-status", "artifacts", `${HASH_D}.bin`),
        "unreferenced\n",
        { mode: 0o600 },
      );
      await assertRejectCode(
        () => appendPhase(rootDir, "pre-cutover", Date.parse("2026-11-13T18:01:00.000Z")),
        ["chain.member", "receipt.digest"],
      );
    } finally {
      await rm(rootDir, { force: true, recursive: true });
    }
  });

  await testContext.test("missing baseline", async () => {
    const rootDir = await privateRoot();

    try {
      await assertRejectCode(
        () => appendPhase(rootDir, "pre-cutover", Date.parse("2026-11-13T18:01:00.000Z")),
        ["chain.member", "receipt.phase"],
      );
    } finally {
      await rm(rootDir, { force: true, recursive: true });
    }
  });

  await testContext.test("predecessor hash link", async () => {
    const rootDir = await privateRoot();

    try {
      const baseline = await appendPhase(rootDir, "baseline", Date.parse("2026-11-13T18:00:00.000Z"));
      await appendPhase(rootDir, "pre-cutover", Date.parse("2026-11-13T18:01:00.000Z"));
      const changed = structuredClone(baseline.receipt);
      changed.writtenAt = "2026-11-13T18:00:09.000Z";
      await writeFile(baseline.path, receipts.canonicalReceiptBytes(changed), { mode: 0o600 });
      await assertRejectCode(
        () => appendPhase(rootDir, "post-cutover", Date.parse("2026-11-13T18:02:00.000Z")),
        ["chain.previous", "receipt.chronology", "receipt.invalid"],
      );
    } finally {
      await rm(rootDir, { force: true, recursive: true });
    }
  });

  await testContext.test("artifact bytes", async () => {
    const rootDir = await privateRoot();

    try {
      await appendPhase(rootDir, "baseline", Date.parse("2026-11-13T18:00:00.000Z"));
      const artifactDir = join(rootDir, "live-status", "artifacts");
      const [artifactName] = await readdir(artifactDir);
      await writeFile(join(artifactDir, artifactName), "changed\n", { mode: 0o600 });
      await assertRejectCode(
        () => appendPhase(rootDir, "pre-cutover", Date.parse("2026-11-13T18:01:00.000Z")),
        ["receipt.digest", "chain.member"],
      );
    } finally {
      await rm(rootDir, { force: true, recursive: true });
    }
  });

  await testContext.test("artifact symlink", async () => {
    const rootDir = await privateRoot();
    const outside = await privateRoot();

    try {
      await appendPhase(rootDir, "baseline", Date.parse("2026-11-13T18:00:00.000Z"));
      const artifactDir = join(rootDir, "live-status", "artifacts");
      const [artifactName] = await readdir(artifactDir);
      const artifactPath = join(artifactDir, artifactName);
      const outsidePath = join(outside, "artifact.bin");
      await writeFile(outsidePath, "outside\n", { mode: 0o600 });
      await rm(artifactPath);
      await symlink(outsidePath, artifactPath);
      await assertRejectCode(
        () => appendPhase(rootDir, "pre-cutover", Date.parse("2026-11-13T18:01:00.000Z")),
        ["path.symlink", "path.type", "receipt.digest"],
      );
    } finally {
      await rm(rootDir, { force: true, recursive: true });
      await rm(outside, { force: true, recursive: true });
    }
  });

  await testContext.test("predecessor mode", async () => {
    const rootDir = await privateRoot();

    try {
      const baseline = await appendPhase(rootDir, "baseline", Date.parse("2026-11-13T18:00:00.000Z"));
      await chmod(baseline.path, 0o644);
      await assertRejectCode(
        () => appendPhase(rootDir, "pre-cutover", Date.parse("2026-11-13T18:01:00.000Z")),
        "path.mode",
      );
    } finally {
      await rm(rootDir, { force: true, recursive: true });
    }
  });

  await testContext.test("predecessor hard link", async () => {
    const rootDir = await privateRoot();

    try {
      const baseline = await appendPhase(rootDir, "baseline", Date.parse("2026-11-13T18:00:00.000Z"));
      await link(baseline.path, `${baseline.path}.link`);
      await assertRejectCode(
        () => appendPhase(rootDir, "pre-cutover", Date.parse("2026-11-13T18:01:00.000Z")),
        ["chain.member", "path.type"],
      );
    } finally {
      await rm(rootDir, { force: true, recursive: true });
    }
  });

  await testContext.test("concurrent append", async () => {
    const rootDir = await privateRoot();

    try {
      const results = await Promise.allSettled([
        appendPhase(rootDir, "baseline", Date.parse("2026-11-13T18:00:00.000Z")),
        appendPhase(rootDir, "baseline", Date.parse("2026-11-13T18:00:00.000Z")),
      ]);
      assert.equal(results.filter(
        // count the one create-once winner
        (result) => result.status === "fulfilled",
      ).length, 1);
      const rejected = results.find(
        // select the losing writer
        (result) => result.status === "rejected",
      );
      assert.ok(["chain.member", "write.exists"].includes(rejected.reason.code));
    } finally {
      await rm(rootDir, { force: true, recursive: true });
    }
  });
});

// reparse raw Gate2 bytes instead of accepting an observation dialect shortcut
test("raw Gate2 capture path rejects noncanonical and incomplete evidence", () => {
  const { input } = assemblyInput();
  const requestBytes = Buffer.from('{ "contractVersion":"weather-backend-host-evidence-request/v1" }\n');
  const outputBytes = Buffer.from('{"contractVersion":"weather-backend-host-evidence/v1"}\n');
  input.captures[0] = {
    kind: "raw-gate2",
    outputBytes,
    requestBytes,
    transport: {
      completedAt: "2026-11-13T18:00:03.000Z",
      exitCode: 0,
      outputSha256: sha256Bytes(outputBytes),
      producerSha256: HASH_A,
      requestSha256: sha256Bytes(requestBytes),
      ssh: { hostKeyFingerprint: BLUEBERRY_FINGERPRINT, knownHostsSha256: KNOWN_HOSTS_SHA256, policySha256: POLICY_SHA256 },
      startedAt: "2026-11-13T18:00:00.000Z",
    },
  };
  assert.throws(
    () => receipts.assembleSyntheticLiveStatusReceipt(input),
    (error) => ["receipt.invalid", "receipt.schema"].includes(error?.code),
  );
});

// keep removed append and target CLI paths unavailable
test("receipt CLI cannot revive v1 append or target freezing", async (testContext) => {
  for (const command of ["append-live", "freeze-target"]) {
    await testContext.test(command, async () => {
      const child = spawn(process.execPath, [RECEIPT_MODULE_PATH.pathname, command], {
        cwd: REPOSITORY_ROOT.pathname,
        env: { LANG: "C", PATH: process.env.PATH },
        stdio: ["ignore", "pipe", "pipe"],
      });
      let stderr = "";
      child.stderr.setEncoding("utf8");
      child.stderr.on("data", (chunk) => {
        stderr += chunk;
      });
      const exitCode = await new Promise((resolve, reject) => {
        child.once("error", reject);
        child.once("close", resolve);
      });
      assert.notEqual(exitCode, 0);
      assert.match(stderr, /receipt\.authority|unsupported|unavailable/iu);
    });
  }
});
