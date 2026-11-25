#!/usr/bin/env node

import { createHash } from "node:crypto";
import { constants as fsConstants } from "node:fs";
import {
  access,
  chmod,
  mkdir,
  mkdtemp,
  readFile,
  readdir,
  readlink,
  rm,
  writeFile,
} from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { spawn, spawnSync } from "node:child_process";

import {
  AdjustmentCyclePageDiskPorts,
  AdjustmentEvidenceArchiveTransport,
  AdjustmentEvidenceStore,
  runAdjustmentArchiveCommand,
  validateAdjustmentFutureOnlyInputSeal,
  validateAdjustmentMaintenanceAnchorV3,
  validateAdjustmentMaintenanceFinalizationProofV3,
} from "../deploy/scripts/adjustment-evidence-store.mjs";

const SCRIPT_PATH = fileURLToPath(import.meta.url);
const REPO_ROOT = resolve(dirname(SCRIPT_PATH), "..");
const DEADLINE_MS = 20_000;
const SUCCESS = "native-evidence-operator-smoke: ok\n";
const CONTROL_SHA256 = "b".repeat(64);
const CONTROL_VERSION = "14";

// recursively normalize object keys while preserving semantic array order
function canonicalValue(value) {
  // preserve scalar values directly
  if (value === null || typeof value !== "object") {
    return value;
  }
  // retain array order while normalizing nested records
  if (Array.isArray(value)) {
    return value.map(canonicalValue);
  }
  return Object.fromEntries(
    Object.keys(value)
      .sort()
      // normalize every recursively sorted object field
      .map((key) => [key, canonicalValue(value[key])]),
  );
}

// encode the source canonical JSON document with its required trailing line feed
function canonicalBytes(value) {
  return Buffer.from(`${JSON.stringify(canonicalValue(value))}\n`, "utf8");
}

// hash exact canonical or retained bytes into the source lowercase identity
function sha256(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}

// assemble one complete source-admitted seal, transferred anchor, and finalization proof
function fixtureMaterials() {
  const candidate = "6".repeat(64);
  const emptyRootSha256 = sha256(canonicalBytes([]));
  const populatedRootSha256 = sha256(canonicalBytes(["7".repeat(64)]));
  const candidateRootSha256 = sha256(canonicalBytes([candidate]));
  const inputClasses = Object.fromEntries(
    [
      "actual_best_match",
      "artifact",
      "candidate",
      "comparator",
      "native_source",
      "rain_gate_input",
      "shadow_body",
      "shadow_source",
      "target",
      "target_revision",
    ].map(
      // declare each exact family population without synthesizing a rain input
      (name) => [
        name,
        name === "rain_gate_input"
          ? { count: 0, rootSha256: emptyRootSha256 }
          : {
              count: 1,
              rootSha256: name === "candidate" ? candidateRootSha256 : populatedRootSha256,
            },
      ],
    ),
  );
  const requiredInputRootSha256 = sha256(canonicalBytes(inputClasses));
  const seal = validateAdjustmentFutureOnlyInputSeal({
    archiveCommitOrdinal: "42",
    burnSha256: "8".repeat(64),
    candidateArtifactRootSha256: sha256(
      canonicalBytes([candidateRootSha256, populatedRootSha256]),
    ),
    candidateReportSha256: "9".repeat(64),
    captureEpochWitnessSha256: "a".repeat(64),
    comparatorRootSha256: populatedRootSha256,
    confirmationAccessSha256: "c".repeat(64),
    confirmationChunkCount: 27,
    contractVersion: "adjustment-future-only-input-seal/v2",
    custodyCheckpointSha256: "d".repeat(64),
    dueKey: `confirmation/temperature/${candidate}`,
    family: "temperature",
    frontierSha256: "e".repeat(64),
    fullMemberCount: 1,
    fullMemberRootSha256: "f".repeat(64),
    graphManifestSha256: "0".repeat(64),
    inputClasses,
    lifecycleLedgerRootSha256: "1".repeat(64),
    pageSha256: "2".repeat(64),
    policyReportSha256: "3".repeat(64),
    predecessorSealSha256: null,
    requiredInputRootSha256,
    sealedAt: "2026-10-08T02:11:00.000Z",
    sequence: "0",
    sourceCommit: "9".repeat(40),
    targetRootSha256: populatedRootSha256,
    workstationJournalHeadSha256: "4".repeat(64),
  });
  const sealBytes = canonicalBytes(seal);
  const sealSha256 = sha256(sealBytes);
  const anchor = validateAdjustmentMaintenanceAnchorV3({
    actionSha256: "5".repeat(64),
    archiveCommitOrdinal: seal.archiveCommitOrdinal,
    burnSha256: seal.burnSha256,
    candidateArtifactRootSha256: seal.candidateArtifactRootSha256,
    candidateReportSha256: seal.candidateReportSha256,
    captureEpochWitnessSha256: seal.captureEpochWitnessSha256,
    confirmationAccessSha256: seal.confirmationAccessSha256,
    confirmationChunkCount: seal.confirmationChunkCount,
    contractVersion: "adjustment-maintenance-anchor/v3",
    controlSha256: CONTROL_SHA256,
    controlVersion: CONTROL_VERSION,
    ctfState: "transferred",
    custodyCheckpointSha256: seal.custodyCheckpointSha256,
    dueKey: seal.dueKey,
    family: seal.family,
    frontierSha256: seal.frontierSha256,
    fullGraphVerifiedAt: "2026-10-08T02:12:00.000Z",
    fullMemberRootSha256: seal.fullMemberRootSha256,
    graphManifestSha256: seal.graphManifestSha256,
    inputSealSha256: sealSha256,
    lifecycleLedgerRootSha256: seal.lifecycleLedgerRootSha256,
    pageSha256: seal.pageSha256,
    policyReportSha256: seal.policyReportSha256,
    predecessorAnchorSha256: null,
    publishedAt: "2026-10-08T02:13:00.000Z",
    requiredInputRootSha256: seal.requiredInputRootSha256,
    sequence: seal.sequence,
    sourceCommit: seal.sourceCommit,
    workstationJournalHeadSha256: seal.workstationJournalHeadSha256,
  });
  const anchorBytes = canonicalBytes(anchor);
  const anchorSha256 = sha256(anchorBytes);
  const { contractVersion: ignoredVersion, ctfState: ignoredState, ...proofFields } = anchor;
  // guard the fixture transformation before deriving the source proof document
  if (
    ignoredVersion !== "adjustment-maintenance-anchor/v3" ||
    ignoredState !== "transferred"
  ) {
    throw new Error("source v3 transferred fixture is invalid");
  }
  const proof = validateAdjustmentMaintenanceFinalizationProofV3({
    ...proofFields,
    contractVersion: "adjustment-maintenance-finalization-proof/v3",
    finalizedAt: "2026-10-08T02:14:00.000Z",
    transferredAnchorSha256: anchorSha256,
  });
  const proofBytes = canonicalBytes(proof);
  return {
    anchorBytes,
    anchorSha256,
    proofBytes,
    proofSha256: sha256(proofBytes),
    sealBytes,
  };
}

const hashes = {
  rain: "a".repeat(64),
  temperature: "b".repeat(64),
  temperatureAuthorization: "c".repeat(64),
  wind: "d".repeat(64),
  windAuthorization: "e".repeat(64),
  windCandidate: "f".repeat(64),
};

// build one public filtered forecast row
function forecastRow(index = 0, overrides = {}) {
  const validAt = new Date(Date.parse("2026-10-07T08:00:00.000Z") + index * 3_600_000).toISOString();
  return {
    adjustment: {
      adjustedMetrics: { windGustMps: 3.5, windSpeedMps: 2.25 },
      appliedMetrics: ["windGustMps", "windSpeedMps"],
      authorizationSha256: hashes.windAuthorization,
      candidateArtifactSha256: hashes.windCandidate,
      leadBand: "001-024",
      reasonCode: null,
      state: "active",
    },
    freshness: { ageSeconds: 1, label: "fresh", status: "fresh" },
    id: String(index + 1),
    metadata: {
      provider: { dataset: "best_match" },
      upstream: { model: "best_match", timezone: "America/Los_Angeles" },
    },
    metrics: {
      precipitationMm: 0.2,
      temperatureC: 12.5,
      windGustMps: 4,
      windSpeedMps: 2.5,
    },
    productRunAt: "2026-10-07T00:00:00.000Z",
    provenance: {
      providerKey: "open-meteo",
      sourceId: "12",
      sourceKey: "open-meteo-forecast-v4",
    },
    rainAdjustment: {
      bundleSha256: hashes.rain,
      correctedPrecipitationMm: 0.15,
      rawBestMatchPrecipitationMm: 0.2,
      reasonCode: null,
      sourceForecast: {
        decisionAt: "2026-10-07T02:00:00.000Z",
        firstReceivedAt: "2026-10-07T00:05:00.000Z",
        modelLeadHours: 8 + index,
        providerKey: "open-meteo",
        rawPrecipitationMm: 0.25,
        runInitializedAt: "2026-10-07T00:00:00.000Z",
        upstreamModel: "ecmwf_ifs",
        validAt,
      },
      state: "active",
    },
    receivedAt: "2026-10-07T00:06:00.000Z",
    revisionCount: 2,
    temperatureAdjustment: {
      branch: "direct",
      bundleSha256: hashes.temperature,
      correctedTemperatureC: 12,
      rawBestMatchTemperatureC: 12.5,
      reasonCode: null,
      sourceForecast: {
        adapterVersion: "v1",
        dataset: "single_run",
        firstReceivedAt: "2026-10-07T00:04:00.000Z",
        modelCycle: "50r1",
        modelLeadHours: 8 + index,
        operationalHorizonHours: 2 + index,
        providerKey: "open-meteo",
        providerResponseSha256: "1".repeat(64),
        rawRelativeHumidityPercent: 75,
        rawTemperatureC: 11.5,
        rawWindSpeedMps: 3,
        runInitializedAt: "2026-10-07T00:00:00.000Z",
        upstreamModel: "ecmwf_ifs",
        validAt,
      },
      state: "active",
    },
    validAt,
    ...overrides,
  };
}

// build one post-settings response body
function forecastBody(rows = [forecastRow()], overrides = {}) {
  return Buffer.from(`${JSON.stringify({
    adjustmentRuntime: {
      activeBundle: hashes.wind,
      authorizationSha256: hashes.windAuthorization,
      candidateArtifactSha256: hashes.windCandidate,
      loadedAt: "2026-10-07T00:07:00.000Z",
      state: "active",
    },
    adjustmentSettings: { rain: true, temperature: true, version: 1, wind: true },
    data: rows,
    days: 1,
    generatedAt: "2026-10-07T00:08:00.000Z",
    rainAdjustmentRuntime: {
      activeBundle: hashes.rain,
      loadedAt: "2026-10-07T00:07:00.000Z",
      state: "active",
    },
    temperatureAdjustmentRuntime: {
      activeBundle: hashes.temperature,
      authorizationSha256: hashes.temperatureAuthorization,
      loadedAt: "2026-10-07T00:07:00.000Z",
      state: "active",
    },
    ...overrides,
  })}\n`);
}

// seed both disposable roots independently through the genuine source forecast store
async function seedArchivePage(root) {
  const now = () => new Date("2026-10-08T00:40:00.123Z");
  const store = await new AdjustmentEvidenceStore({now, root}).initialize();
  const prepared = store.prepareV2(forecastBody(), "days=10", "scheduler_request");
  // require archive material from the actual source forecast preparation
  if (prepared === null || (await store.commit(prepared, now().toISOString())).status !== "created") {
    throw new Error("source candidate forecast preparation failed");
  }
  const transport = await new AdjustmentEvidenceArchiveTransport({now, root}).initialize();
  const captured = await transport.captureCommittedSchedulerEvidence({
    dueKey: "capture/2026-10-08T00:35:00.000Z", prepared,
  });
  // retain a real committed online page rather than a hand-authored envelope
  if (captured.status !== "page_pending_acknowledgement") {
    throw new Error(`source candidate archive page not pending: ${captured.status}`);
  }
  const page = await transport.next();
  return {now, prepared, page, transport};
}

// compare the actual installed candidate archive verbs with source custody
async function compareCandidateArchive(binary, nativeRoot, sourceRoot) {
  const nativeSeed = await seedArchivePage(nativeRoot);
  const sourceSeed = await seedArchivePage(sourceRoot);
  const sourceBytes = [];
  await runAdjustmentArchiveCommand(["archive-next"], {
    root: sourceRoot, now: sourceSeed.now, stdout: {write: (bytes) => sourceBytes.push(Buffer.from(bytes))},
  });
  const nativeBytes = runNative(binary, "archive-next");
  // require exact canonical transfer bytes including the source-compressed retained forecast
  if (!nativeBytes.equals(Buffer.concat(sourceBytes)) ||
      !nativeBytes.equals(canonicalBytes(nativeSeed.page)) || nativeSeed.page.kind !== "page") {
    throw new Error("candidate archive page differs from exact Node predecessor bytes");
  }
  const checkpoint = "9".repeat(64);
  // simulate the same durable checkpoint made before either command releases its slot
  for (const [root, seed] of [[nativeRoot, nativeSeed], [sourceRoot, sourceSeed]]) {
    const ports = await new AdjustmentCyclePageDiskPorts({now: seed.now, root}).initialize();
    await ports.persistTransferAcknowledgement({
      acknowledgedAt: "2026-10-08T00:41:00.456Z",
      checkpointSha256: checkpoint,
      contractVersion: "adjustment-archive-transfer-ack/v1",
      header: seed.page.header,
      pageSha256: seed.page.pageSha256,
      payloadLength: Buffer.from(seed.page.payloadBase64, "base64").length,
      projections: seed.page.projections,
    });
  }
  const ackArguments = [nativeSeed.page.pageSha256, checkpoint];
  await runAdjustmentArchiveCommand(["archive-ack", ...ackArguments], {root: sourceRoot});
  // require the installed no-output acknowledgement and exact subsequent idle state
  if (runNative(binary, "archive-ack", ackArguments).length !== 0 ||
      !runNative(binary, "archive-next").equals(canonicalBytes(await sourceSeed.transport.next()))) {
    throw new Error("candidate archive page acknowledgement differs from source");
  }
  // close the same seven producer classes through the genuine seal authority in both roots
  const emptyRoot = sha256(canonicalBytes([]));
  for (const seed of [nativeSeed, sourceSeed]) {
    const requiredInputs = Object.fromEntries([
      "actual_best_match", "native_source", "rain_gate_input", "serving_evidence",
      "shadow_body", "shadow_metadata", "target_revision",
    ].map((kind) => [kind, kind === "serving_evidence"
      ? {count: 0, rootSha256: emptyRoot}
      : {count: 1, rootSha256: sha256(kind)}]));
    requiredInputs.serving_evidence = {
      count: 1, rootSha256: seed.prepared.edgeReceiptIdentitySha256,
    };
    await seed.transport.sealCycleInputs({
      dueKey: seed.page.header.dueKey, generation: seed.page.header.generation, requiredInputs,
    });
  }
  const expectedFinal = await sourceSeed.transport.next();
  // compare the full native final manifest envelope without attempting a live graph publication
  if (expectedFinal.kind !== "final" ||
      !runNative(binary, "archive-next").equals(canonicalBytes(expectedFinal))) {
    throw new Error("candidate archive final differs from source");
  }
  const graph = "7".repeat(64);
  const finalArguments = [expectedFinal.manifestSha256, graph];
  await runAdjustmentArchiveCommand(["archive-ack-final", ...finalArguments], {root: sourceRoot});
  // preserve the source no-output final ack and exact idle representation after graph binding
  if (runNative(binary, "archive-ack-final", finalArguments).length !== 0 ||
      !runNative(binary, "archive-next").equals(canonicalBytes(await sourceSeed.transport.next()))) {
    throw new Error("candidate archive final acknowledgement differs from source");
  }
  process.stdout.write("candidate archive page, checkpoint, final and final ack match source\n");
}

// run one namespace setup command without inheriting input
function requireCommand(command, arguments_) {
  const result = spawnSync(command, arguments_, {
    encoding: "utf8",
    stdio: ["ignore", "pipe", "pipe"],
  });
  // stop before touching the private fixture when namespace preparation differs
  if (result.status !== 0) {
    throw new Error(`${command} failed: ${result.stderr.trim()}`);
  }
}

// execute the supplied artifact with only the exact operator control environment
function runNative(binary, action, operands = [], input = Buffer.alloc(0)) {
  const result = spawnSync(
    "/usr/bin/env",
    [
      "-i",
      `WEATHER_ANCHOR_CONTROL_SHA256=${CONTROL_SHA256}`,
      `WEATHER_ANCHOR_CONTROL_VERSION=${CONTROL_VERSION}`,
      binary,
      action,
      ...operands,
    ],
    {
      cwd: REPO_ROOT,
      input,
      maxBuffer: 1024 * 1024,
      stdio: ["pipe", "pipe", "pipe"],
    },
  );
  // retain the exact binary diagnostic only when the requested command failed
  if (result.status !== 0) {
    throw new Error(
      `weather-evidence ${action} failed: ${result.stderr.toString("utf8").trim()}`,
    );
  }
  return result.stdout;
}

// execute the actual source command against only its explicitly selected disposable root
async function runSource(root, action, digest, input) {
  const chunks = [];
  await runAdjustmentArchiveCommand([action, digest], {
    expectedControlSha256: CONTROL_SHA256,
    expectedControlVersion: CONTROL_VERSION,
    root,
    stdinBytes: input,
    stdout: { write: (bytes) => chunks.push(Buffer.from(bytes)) },
  });
  return Buffer.concat(chunks);
}

// require two retained slot files to remain byte-identical across implementations
async function requireEqualSlots(nativeRoot, sourceRoot) {
  // compare both final authority and its retained direct predecessor
  for (const slot of ["current.json", "previous.json"]) {
    const relative = join("maintenance-anchors", slot);
    const [nativeBytes, sourceBytes] = await Promise.all([
      readFile(join(nativeRoot, relative)),
      readFile(join(sourceRoot, relative)),
    ]);
    // reject any canonical or transaction-rotation difference
    if (!nativeBytes.equals(sourceBytes)) {
      throw new Error(`native evidence ${slot} differs from source`);
    }
  }
  const [nativeNames, sourceNames] = await Promise.all([
    readdir(join(nativeRoot, "maintenance-anchors")),
    readdir(join(sourceRoot, "maintenance-anchors")),
  ]);
  nativeNames.sort();
  sourceNames.sort();
  // require both transactions to retire pending state and retain the same fixed inventory
  if (
    JSON.stringify(nativeNames) !== JSON.stringify(["current.json", "previous.json"]) ||
    JSON.stringify(sourceNames) !== JSON.stringify(nativeNames)
  ) {
    throw new Error("native evidence anchor inventory differs from source");
  }
}

// run all binary and source effects only after entering distinct private namespaces
async function runInner(binary, fixture, hostNetworkNamespace, hostMountNamespace, candidate) {
  const networkNamespace = await readlink("/proc/self/ns/net");
  const mountNamespace = await readlink("/proc/self/ns/mnt");
  // refuse the bind before both namespace identities differ from the outer process
  if (networkNamespace === hostNetworkNamespace || mountNamespace === hostMountNamespace) {
    throw new Error("weather-evidence smoke requires private network and mount namespaces");
  }
  requireCommand("mount", ["--bind", join(fixture, "var-lib"), "/var/lib"]);

  const nativeRoot = "/var/lib/weather/xweather/adjustment-evidence";
  const sourceRoot = join(fixture, "source-evidence");
  // exercise the newly installed archive family without rerunning the previously accepted custody baseline
  if (candidate) {
    await compareCandidateArchive(binary, nativeRoot, sourceRoot);
    return;
  }
  const anchorBytes = await readFile(join(fixture, "anchor.json"));
  const proofBytes = await readFile(join(fixture, "proof.json"));
  const anchorSha256 = sha256(anchorBytes);
  const proofSha256 = sha256(proofBytes);

  const emptyProof = runNative(binary, "unsupported-terminal-proof-current-v1");
  // require the genuine absent-root source representation before any anchor writes
  if (!emptyProof.equals(Buffer.from("null\n"))) {
    throw new Error(`unexpected absent unsupported proof output: ${emptyProof.toString("utf8")}`);
  }

  const sourceInstall = await runSource(
    sourceRoot,
    "maintenance-anchor-install-v3",
    anchorSha256,
    anchorBytes,
  );
  const nativeInstall = runNative(
    binary,
    "maintenance-anchor-install-v3",
    [anchorSha256],
    anchorBytes,
  );
  // require exact installation receipt and current T bytes before finalization
  if (!nativeInstall.equals(sourceInstall)) {
    throw new Error("native v3 transferred installation receipt differs from source");
  }
  const [nativeTransferred, sourceTransferred] = await Promise.all([
    readFile(join(nativeRoot, "maintenance-anchors/current.json")),
    readFile(join(sourceRoot, "maintenance-anchors/current.json")),
  ]);
  // preserve the exact supplied T document on both independent roots
  if (
    !nativeTransferred.equals(anchorBytes) ||
    !sourceTransferred.equals(anchorBytes)
  ) {
    throw new Error("v3 transferred current bytes differ from the supplied anchor");
  }

  let firstReceipt;
  // compare first finalization and its exact idempotent retry through both real owners
  for (let attempt = 0; attempt < 2; attempt += 1) {
    const sourceReceipt = await runSource(
      sourceRoot,
      "maintenance-anchor-finalize-v3",
      proofSha256,
      proofBytes,
    );
    const nativeReceipt = runNative(
      binary,
      "maintenance-anchor-finalize-v3",
      [proofSha256],
      proofBytes,
    );
    // require source-identical receipts on both initial commit and retry
    if (!nativeReceipt.equals(sourceReceipt)) {
      throw new Error(`native v3 finalization receipt ${attempt} differs from source`);
    }
    // require the retry to return the same receipt without rotating new state
    if (firstReceipt !== undefined && !nativeReceipt.equals(firstReceipt)) {
      throw new Error("native v3 finalization retry receipt changed");
    }
    firstReceipt = nativeReceipt;
    await requireEqualSlots(nativeRoot, sourceRoot);
  }
}

// wait for one namespace child without allowing an isolated smoke to hang
function awaitExit(child, deadlineMilliseconds) {
  // settle on the first child close or fixed deadline
  return new Promise((resolveExit, rejectExit) => {
    const timeout = setTimeout(
      () => rejectExit(new Error("weather-evidence smoke deadline expired")),
      deadlineMilliseconds,
    );
    // cancel the deadline after every process resource closes
    child.once("close", (code, signal) => {
      clearTimeout(timeout);
      resolveExit({ code, signal });
    });
  });
}

// create both disposable roots and enter the isolated namespace worker
async function runOuter(binaryArgument, candidate) {
  const binary = resolve(binaryArgument);
  await access(binary, fsConstants.X_OK);
  const fixture = await mkdtemp(join(tmpdir(), "weather-native-evidence-smoke-"));
  let child;

  try {
    const nativeRoot = join(fixture, "var-lib", "weather", "xweather", "adjustment-evidence");
    const sourceRoot = join(fixture, "source-evidence");
    const materials = fixtureMaterials();
    // create only the two explicit private evidence and raw-seal roots
    for (const root of [nativeRoot, sourceRoot]) {
      await mkdir(root, { mode: 0o700, recursive: true });
      await chmod(root, 0o700);
      await mkdir(join(root, "future-input-seals"), { mode: 0o700 });
      await writeFile(
        join(root, "future-input-seals", "current.json"),
        materials.sealBytes,
        { mode: 0o600 },
      );
    }
    await writeFile(join(fixture, "anchor.json"), materials.anchorBytes, { mode: 0o600 });
    await writeFile(join(fixture, "proof.json"), materials.proofBytes, { mode: 0o600 });
    const hostNetworkNamespace = await readlink("/proc/self/ns/net");
    const hostMountNamespace = await readlink("/proc/self/ns/mnt");
    child = spawn(
      "unshare",
      [
        "-Urnm",
        "--propagation",
        "private",
        "--",
        process.execPath,
        SCRIPT_PATH,
        "--inner",
        binary,
        fixture,
        hostNetworkNamespace,
        hostMountNamespace,
        ...(candidate ? ["--candidate-gzip"] : []),
      ],
      { detached: true, stdio: ["ignore", "pipe", "pipe"] },
    );
    const stdout = [];
    const stderr = [];
    // retain only the isolated worker output for bounded failure reporting
    child.stdout.on("data", (chunk) => stdout.push(chunk));
    // retain only the isolated worker diagnostics for bounded failure reporting
    child.stderr.on("data", (chunk) => stderr.push(chunk));
    const exit = await awaitExit(child, DEADLINE_MS);
    // expose the isolated diagnostic only when the smoke owner failed
    if (exit.code !== 0 || exit.signal !== null) {
      throw new Error(
        `isolated weather-evidence smoke failed: code=${exit.code} signal=${exit.signal}\n` +
          Buffer.concat(stderr).toString("utf8") +
          Buffer.concat(stdout).toString("utf8"),
      );
    }
    process.stdout.write(SUCCESS);
  } finally {
    // terminate the complete private namespace group only after failure or timeout
    if (child !== undefined && child.exitCode === null && child.signalCode === null) {
      process.kill(-child.pid, "SIGKILL");
      // ignore only the cleanup wait after the retained failure
      await awaitExit(child, 2_000).catch(() => undefined);
    }
    await rm(fixture, { force: true, recursive: true });
  }
}

// select the private namespace worker without adding a binary fixture mode
if (process.argv[2] === "--inner") {
  // require the outer owner to supply both original namespace identities
  if (![7, 8].includes(process.argv.length) || (process.argv.length === 8 && process.argv[7] !== "--candidate-gzip")) {
    throw new Error("private namespace smoke arguments are incomplete");
  }
  const [, , , binary, fixture, hostNetworkNamespace, hostMountNamespace] = process.argv;
  await runInner(binary, fixture, hostNetworkNamespace, hostMountNamespace, process.argv[7] === "--candidate-gzip");
} else {
  const binary = process.argv[2];
  // require the exact built artifact instead of compiling inside the smoke test
  if (binary === undefined || ![3, 4].includes(process.argv.length) ||
      (process.argv.length === 4 && process.argv[3] !== "--candidate-gzip")) {
    throw new Error(`usage: ${process.argv[1]} /absolute/or/relative/path/to/weather-evidence`);
  }
  await runOuter(binary, process.argv[3] === "--candidate-gzip");
}
