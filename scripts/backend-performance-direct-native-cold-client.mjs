import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { lstat, readFile, realpath } from "node:fs/promises";
import { createConnection, isIP } from "node:net";
import { isAbsolute } from "node:path";
import { createInterface } from "node:readline";
import { pathToFileURL } from "node:url";
import { setTimeout as pause } from "node:timers/promises";

import {
  BACKEND_PERFORMANCE_DIRECT_FIXTURE_AUTHORITY_VERSION,
  BACKEND_PERFORMANCE_DIRECT_FIXTURE_LIFECYCLE_VERSION,
  BACKEND_PERFORMANCE_DIRECT_PHYSICAL_PROOF_VERSION,
  buildBackendPerformanceDirectNativeHttpPlan,
  runBackendPerformanceDirectNativeHttpClient,
} from "./backend-performance-direct-native-http-client.mjs";
import {
  canonicalReceiptBytes,
  receiptSha256,
} from "./backend-performance-receipts.mjs";

export const BACKEND_PERFORMANCE_DIRECT_NATIVE_COLD_CLIENT_VERSION =
  "weather-backend-performance-direct-native-cold-client/v1";

const BLUEBERRY_SSH_TARGET = "admin@192.168.11.94";
const LOCAL_ORIGIN = "http://127.0.0.1:13001/";
const LOCAL_PORT = 13_001;
const REMOTE_WEB_PORT = 3_000;
const REMOTE_API_PORT = 3_001;
const INITIAL_PACKET_TIMEOUT_MS = 60_000;
const CLEANUP_PACKET_TIMEOUT_MS = 120_000;
const MAXIMUM_PROOF_BYTES = 16 * 1024 * 1024;
const MAXIMUM_KNOWN_HOSTS_BYTES = 64 * 1024;
const SHA256_PATTERN = /^[a-f0-9]{64}$/u;
const FIXTURE_ID_PATTERN = /^rust-rewrite-[a-f0-9]{16}$/u;

// create one coded cold-client failure
function fail(code, message) {
  const error = new Error(message);
  error.code = code;
  throw error;
}

// require one cold-client invariant
function requireInvariant(condition, code, message) {
  // reject unsafe or ambiguous control input
  if (!condition) {
    fail(code, message);
  }
}

// identify one plain record
function isRecord(value) {
  return value !== null && typeof value === "object" && !Array.isArray(value)
    && (Object.getPrototypeOf(value) === Object.prototype
      || Object.getPrototypeOf(value) === null);
}

// require one exact record shape
function requireExactKeys(value, keys, path) {
  requireInvariant(
    isRecord(value)
      && JSON.stringify(Object.keys(value).sort()) === JSON.stringify([...keys].sort()),
    "native-cold-client.schema",
    `${path} has unexpected fields`,
  );
}

// read one monotonic millisecond clock
function monotonicMilliseconds() {
  return Number(process.hrtime.bigint() / 1_000_000n);
}

// validate one owner-private pinned known-hosts file
async function validateKnownHostsFile(path) {
  requireInvariant(
    typeof path === "string" && isAbsolute(path),
    "native-cold-client.known-hosts",
    "known-hosts file must be an absolute path",
  );
  const [resolved, metadata] = await Promise.all([realpath(path), lstat(path)]);
  const invokingUser = typeof process.getuid === "function" ? process.getuid() : null;
  requireInvariant(
    resolved === path
      && metadata.isFile()
      && (metadata.mode & 0o777) === 0o600
      && (invokingUser === null || metadata.uid === invokingUser)
      && metadata.size > 0
      && metadata.size <= MAXIMUM_KNOWN_HOSTS_BYTES,
    "native-cold-client.known-hosts",
    "known-hosts file must be canonical owner-private 0600 bounded bytes",
  );
  const bytes = await readFile(path);
  requireInvariant(
    bytes.length === metadata.size && bytes.includes(Buffer.from("192.168.11.94")),
    "native-cold-client.known-hosts",
    "known-hosts file does not pin the fixed Blueberry address",
  );
  return Object.freeze({
    changedAtMs: metadata.ctimeMs,
    device: metadata.dev,
    inode: metadata.ino,
    modifiedAtMs: metadata.mtimeMs,
    path,
    sha256: createHash("sha256").update(bytes).digest("hex"),
    size: metadata.size,
  });
}

// reject pinned host-key replacement before SSH consumes it
async function revalidateKnownHostsFile(authority) {
  const current = await validateKnownHostsFile(authority.path);
  requireInvariant(
    JSON.stringify(current) === JSON.stringify(authority),
    "native-cold-client.known-hosts",
    "known-hosts file changed after initial validation",
  );
  return current.path;
}

// parse one canonical base64 proof payload
function parseColdProof(value, expectedSha256) {
  requireInvariant(
    typeof value === "string" && value.length > 0,
    "native-cold-client.proof",
    "cold proof bytes are missing",
  );
  const bytes = Buffer.from(value, "base64");
  requireInvariant(
    bytes.length > 0
      && bytes.length <= MAXIMUM_PROOF_BYTES
      && bytes.toString("base64") === value
      && createHash("sha256").update(bytes).digest("hex") === expectedSha256
      && receiptSha256(bytes) === expectedSha256,
    "native-cold-client.proof",
    "cold proof bytes or digest are invalid",
  );
  let proof;
  try {
    proof = JSON.parse(bytes.toString("utf8"));
  } catch {
    fail("native-cold-client.proof", "cold proof bytes are not JSON");
  }
  requireInvariant(
    isRecord(proof) && proof.contractVersion === "weather-page-d-loop-cold-proof/v1",
    "native-cold-client.proof",
    "cold proof does not use the exact D-loop v1 contract",
  );
  return proof;
}

// validate one initial controller request
async function validateStartPacket(packet) {
  requireExactKeys(packet, [
    "attemptId",
    "coldProofBase64",
    "coldProofSha256",
    "fixtureId",
    "knownHostsFile",
    "sourceReceiptSha256",
    "type",
  ], "$start");
  requireInvariant(
    packet.type === "start"
      && FIXTURE_ID_PATTERN.test(packet.fixtureId ?? "")
      && SHA256_PATTERN.test(packet.sourceReceiptSha256 ?? "")
      && SHA256_PATTERN.test(packet.coldProofSha256 ?? "")
      && typeof packet.attemptId === "string"
      && packet.attemptId.length > 0,
    "native-cold-client.start",
    "start packet identity is invalid",
  );
  const [knownHosts, coldProof] = await Promise.all([
    validateKnownHostsFile(packet.knownHostsFile),
    Promise.resolve(parseColdProof(packet.coldProofBase64, packet.coldProofSha256)),
  ]);
  const fixtureAuthority = Object.freeze({
    contractVersion: BACKEND_PERFORMANCE_DIRECT_FIXTURE_AUTHORITY_VERSION,
    fixtureId: packet.fixtureId,
    origin: LOCAL_ORIGIN,
    sourceReceiptSha256: packet.sourceReceiptSha256,
  });
  const request = {
    attemptIds: [packet.attemptId],
    fixtureAuthority,
    generation: "node-v1",
    mode: "disposable-fixture",
    role: "baseline",
    runId: `native-cold-${packet.attemptId.replaceAll(":", ".")}`.slice(0, 128),
  };
  const plan = buildBackendPerformanceDirectNativeHttpPlan(request);
  requireInvariant(
    plan.attempts.length === 1
      && plan.attempts[0].attemptId === packet.attemptId
      && plan.attempts[0].stratum === "db-os-cold"
      && plan.attempts[0].timeoutMs === 30_000,
    "native-cold-client.attempt",
    "start packet must select one approved v4 db-os-cold HTTP attempt",
  );
  let remotePort;
  // bind the immutable operation category to one reviewed service profile
  if (plan.attempts[0].operationId.startsWith("api.")) {
    remotePort = REMOTE_API_PORT;
  } else if (plan.attempts[0].operationId.startsWith("page.")
    || plan.attempts[0].operationId.startsWith("edge.")) {
    remotePort = REMOTE_WEB_PORT;
  } else {
    fail(
      "native-cold-client.attempt",
      "cold attempt operation category has no reviewed isolated service profile",
    );
  }
  return {
    attempt: plan.attempts[0],
    coldProof,
    coldProofSha256: packet.coldProofSha256,
    fixtureAuthority,
    knownHosts,
    remotePort,
    request,
  };
}

// accept only one private bridge ipv4 address
function privateBridgeAddress(value) {
  // reject non-ipv4 values first
  if (isIP(value) !== 4) {
    return false;
  }
  const octets = value.split(".").map(Number);
  return octets[0] === 10
    || (octets[0] === 172 && octets[1] >= 16 && octets[1] <= 31)
    || (octets[0] === 192 && octets[1] === 168);
}

// validate one reviewed API lifecycle handoff
function validatePreparedPacket(packet, start) {
  requireExactKeys(packet, [
    "api",
    "attemptId",
    "fixtureId",
    "sourceReceiptSha256",
    "type",
  ], "$prepared");
  requireExactKeys(packet.api, [
    "bridgeIpv4",
    "cleanupIdentitySha256",
    "endpointIdSha256",
    "networkIdSha256",
    "port",
    "processIdentitySha256",
    "readyAt",
    "startedAt",
  ], "$prepared.api");
  const api = packet.api;
  requireInvariant(
    packet.type === "prepared"
      && packet.attemptId === start.attempt.attemptId
      && packet.fixtureId === start.fixtureAuthority.fixtureId
      && packet.sourceReceiptSha256 === start.fixtureAuthority.sourceReceiptSha256
      && privateBridgeAddress(api.bridgeIpv4)
      && api.port === start.remotePort
      && SHA256_PATTERN.test(api.cleanupIdentitySha256 ?? "")
      && SHA256_PATTERN.test(api.endpointIdSha256 ?? "")
      && SHA256_PATTERN.test(api.networkIdSha256 ?? "")
      && SHA256_PATTERN.test(api.processIdentitySha256 ?? "")
      && typeof api.startedAt === "string"
      && typeof api.readyAt === "string"
      && new Date(api.startedAt).toISOString() === api.startedAt
      && new Date(api.readyAt).toISOString() === api.readyAt
      && Date.parse(api.readyAt) >= Date.parse(api.startedAt)
      && Date.parse(api.startedAt) >= Date.parse(start.coldProof.completedAt),
    "native-cold-client.prepared",
    "prepared packet is not the exact reviewed fixture API handoff",
  );
  return packet;
}

// validate one exact controller cleanup acknowledgement
function validateCleanedPacket(packet, start, prepared) {
  requireExactKeys(packet, [
    "attemptId",
    "cleanupIdentitySha256",
    "fixtureId",
    "stoppedAt",
    "type",
  ], "$cleaned");
  requireInvariant(
    packet.type === "cleaned"
      && packet.attemptId === start.attempt.attemptId
      && packet.fixtureId === start.fixtureAuthority.fixtureId
      && packet.cleanupIdentitySha256 === prepared.api.cleanupIdentitySha256
      && typeof packet.stoppedAt === "string"
      && new Date(packet.stoppedAt).toISOString() === packet.stoppedAt
      && Date.parse(packet.stoppedAt) >= Date.parse(prepared.api.readyAt),
    "native-cold-client.cleaned",
    "cleaned packet does not acknowledge the exact fixture lifecycle",
  );
  return packet;
}

// calculate one positive remaining attempt budget
function remainingMilliseconds(
  deadlineAt,
  now,
  deadlineDescription = "30-second cold-attempt",
) {
  const remaining = Math.floor(deadlineAt - now());
  requireInvariant(
    remaining > 0,
    "native-cold-client.deadline",
    `the ${deadlineDescription} deadline was exceeded`,
  );
  return remaining;
}

// read and validate one bounded protocol packet
async function readPacket(
  transport,
  deadlineAt,
  now,
  deadlineDescription = "30-second cold-attempt",
) {
  const packet = await transport.readLine(remainingMilliseconds(
    deadlineAt,
    now,
    deadlineDescription,
  ));
  requireInvariant(
    isRecord(packet),
    "native-cold-client.protocol",
    "controller packet is missing or invalid",
  );
  return packet;
}

// construct the only permitted SSH tunnel command
export function buildBackendPerformanceDirectNativeColdTunnelArguments(
  knownHostsFile,
  bridgeIpv4,
  remotePort = REMOTE_API_PORT,
) {
  requireInvariant(
    typeof knownHostsFile === "string"
      && isAbsolute(knownHostsFile)
      && privateBridgeAddress(bridgeIpv4)
      && (remotePort === REMOTE_WEB_PORT || remotePort === REMOTE_API_PORT),
    "native-cold-client.tunnel",
    "tunnel arguments must use the pinned host file private bridge and reviewed service port",
  );
  return [
    "-N",
    "-T",
    "-o", "BatchMode=yes",
    "-o", "ExitOnForwardFailure=yes",
    "-o", "IdentitiesOnly=yes",
    "-o", "StrictHostKeyChecking=yes",
    "-o", `UserKnownHostsFile=${knownHostsFile}`,
    "-L", `127.0.0.1:${String(LOCAL_PORT)}:${bridgeIpv4}:${String(remotePort)}`,
    BLUEBERRY_SSH_TARGET,
  ];
}

// start one fixed no-command SSH tunnel
function spawnTunnel(knownHostsFile, bridgeIpv4, remotePort) {
  return spawn("ssh", buildBackendPerformanceDirectNativeColdTunnelArguments(
    knownHostsFile,
    bridgeIpv4,
    remotePort,
  ), {
    stdio: ["ignore", "ignore", "ignore"],
  });
}

// probe only the fixed local forwarded port
function connectLocalTunnel() {
  return new Promise((resolve) => {
    const socket = createConnection({ host: "127.0.0.1", port: LOCAL_PORT });
    let settled = false;
    // finish one local probe once
    function finish(ready) {
      // ignore duplicate socket events
      if (settled) {
        return;
      }
      settled = true;
      socket.destroy();
      resolve(ready);
    }
    socket.once("connect", () => finish(true));
    socket.once("error", () => finish(false));
    socket.setTimeout(250, () => finish(false));
  });
}

// wait for the exact SSH child to publish its local listener
async function waitTunnel(child, deadlineAt, now) {
  // retry only the fixed local socket
  while (remainingMilliseconds(deadlineAt, now) > 0) {
    requireInvariant(
      child.exitCode === null && child.signalCode === null,
      "native-cold-client.tunnel",
      "SSH tunnel exited before readiness",
    );
    // accept one successful fixed-port connection
    if (await connectLocalTunnel()) {
      return;
    }
    await pause(50);
  }
}

// await one child exit with a bounded timeout
function waitChildExit(child, timeoutMs) {
  // preserve an already exited child
  if (child.exitCode !== null || child.signalCode !== null) {
    return Promise.resolve();
  }
  return new Promise((resolve, reject) => {
    const timer = setTimeout(
      // fail one bounded child wait
      () => {
        child.off("exit", exited);
        reject(new Error("SSH tunnel did not exit within its cleanup timeout"));
      },
      timeoutMs,
    );
    // complete one child exit
    function exited() {
      clearTimeout(timer);
      resolve();
    }
    child.once("exit", exited);
  });
}

// close only the exact spawned SSH tunnel
async function closeTunnel(child) {
  // preserve an already exited child
  if (child.exitCode !== null || child.signalCode !== null) {
    return;
  }
  child.kill("SIGTERM");
  try {
    await waitChildExit(child, 2_000);
  } catch {
    // force only the same owned child after bounded graceful cleanup
    child.kill("SIGKILL");
    await waitChildExit(child, 1_000);
  }
}

// validate the injected execution surface
function validateAdapters(adapters) {
  requireInvariant(
    isRecord(adapters)
      && Object.keys(adapters).every((key) => [
        "closeTunnel",
        "fetch",
        "monotonicMilliseconds",
        "spawnTunnel",
        "waitTunnel",
        "wallNow",
      ].includes(key))
      && (adapters.closeTunnel === undefined || typeof adapters.closeTunnel === "function")
      && (adapters.fetch === undefined || typeof adapters.fetch === "function")
      && (adapters.monotonicMilliseconds === undefined
        || typeof adapters.monotonicMilliseconds === "function")
      && (adapters.spawnTunnel === undefined || typeof adapters.spawnTunnel === "function")
      && (adapters.waitTunnel === undefined || typeof adapters.waitTunnel === "function")
      && (adapters.wallNow === undefined || typeof adapters.wallNow === "function"),
    "native-cold-client.adapters",
    "cold-client adapters are invalid",
  );
}

// execute one source-bound native cold HTTP attempt
export async function runBackendPerformanceDirectNativeColdClient(transport, adapters = {}) {
  requireInvariant(
    isRecord(transport)
      && typeof transport.readLine === "function"
      && typeof transport.writeLine === "function",
    "native-cold-client.transport",
    "cold-client transport is invalid",
  );
  validateAdapters(adapters);
  const initial = await transport.readLine(INITIAL_PACKET_TIMEOUT_MS);
  const start = await validateStartPacket(initial);
  const now = adapters.monotonicMilliseconds ?? monotonicMilliseconds;
  const openTunnel = adapters.spawnTunnel ?? spawnTunnel;
  const awaitTunnel = adapters.waitTunnel ?? waitTunnel;
  const stopTunnel = adapters.closeTunnel ?? closeTunnel;
  const wallNow = adapters.wallNow ?? (() => new Date());
  let tunnel = null;
  let prepared = null;
  let prepareRequested = false;
  let cleanedPacketSha256 = null;
  let requestDeadlineAt = null;
  let cleanupOutcome = Object.freeze({
    code: null,
    message: null,
    status: "not-required",
  });

  // close and acknowledge one prepared fixture without erasing its request row
  async function cleanupPreparedFixture() {
    const failures = [];
    // close the client path before asking the controller to stop the service
    if (tunnel !== null) {
      try {
        await stopTunnel(tunnel);
      } catch (error) {
        failures.push(`tunnel: ${error instanceof Error ? error.message : String(error)}`);
      } finally {
        tunnel = null;
      }
    }
    try {
      await transport.writeLine({
        attemptId: start.attempt.attemptId,
        cleanupIdentitySha256: prepared.api.cleanupIdentitySha256,
        fixtureId: start.fixtureAuthority.fixtureId,
        type: "cleanup",
      });
      const cleanupDeadlineAt = now() + CLEANUP_PACKET_TIMEOUT_MS;
      const cleaned = await readPacket(
        transport,
        cleanupDeadlineAt,
        now,
        "120-second cleanup acknowledgement",
      );
      validateCleanedPacket(cleaned, start, prepared);
      cleanedPacketSha256 = receiptSha256(cleaned);
    } catch (error) {
      failures.push(`controller: ${error instanceof Error ? error.message : String(error)}`);
    }
    // retain an explicit non-crediting cleanup failure
    if (failures.length > 0) {
      cleanupOutcome = Object.freeze({
        code: "native-cold-client.cleanup",
        message: failures.join("; "),
        status: "failed",
      });
      return;
    }
    cleanupOutcome = Object.freeze({
      code: null,
      message: null,
      status: "complete",
    });
  }

  const result = await runBackendPerformanceDirectNativeHttpClient(start.request, {
    async cleanupAttempt() {
      await cleanupPreparedFixture();
    },
    fetch: adapters.fetch ?? globalThis.fetch,
    async prepareAttempt() {
      requestDeadlineAt = now() + start.attempt.timeoutMs;
      // a failed write or handoff may still have triggered controller allocation
      prepareRequested = true;
      await transport.writeLine({ type: "prepare", attemptId: start.attempt.attemptId });
      prepared = validatePreparedPacket(
        await readPacket(transport, requestDeadlineAt, now),
        start,
      );
      try {
        const knownHostsFile = await revalidateKnownHostsFile(start.knownHosts);
        tunnel = openTunnel(knownHostsFile, prepared.api.bridgeIpv4, start.remotePort);
        await awaitTunnel(tunnel, requestDeadlineAt, now);
      } catch (error) {
        // clean a partially prepared fixture on its separate custody deadline
        await cleanupPreparedFixture();
        throw error;
      }
      const lifecycle = {
        attemptId: start.attempt.attemptId,
        cleanupIdentitySha256: prepared.api.cleanupIdentitySha256,
        contractVersion: BACKEND_PERFORMANCE_DIRECT_FIXTURE_LIFECYCLE_VERSION,
        fixtureAuthoritySha256: receiptSha256(start.fixtureAuthority),
        processIdentitySha256: prepared.api.processIdentitySha256,
        readyAt: prepared.api.readyAt,
        startedAt: prepared.api.startedAt,
      };
      return {
        baseUrl: LOCAL_ORIGIN,
        classification: "fresh-disposable-db-os-cold-api",
        lifecycle,
        physicalProof: {
          attemptId: start.attempt.attemptId,
          completedAt: start.coldProof.completedAt,
          contractVersion: BACKEND_PERFORMANCE_DIRECT_PHYSICAL_PROOF_VERSION,
          sourceContractVersion: start.coldProof.contractVersion,
          sourceProof: start.coldProof,
          sourceProofSha256: receiptSha256(start.coldProof),
        },
      };
    },
    wallNow,
  });
  // retain ambiguity rather than sending cleanup with an unvalidated identity
  if (prepareRequested && prepared === null) {
    cleanupOutcome = Object.freeze({
      code: "native-cold-client.cleanup",
      message: "preparation requested without a validated handoff; controller-owned cleanup is unconfirmed",
      status: "failed",
    });
  }
  const terminalResult = cleanupOutcome.status === "failed"
    ? Object.freeze({
      ...result,
      operationReceipts: Object.freeze([]),
      status: "stopped",
      stopReason: `fixture cleanup failed: ${cleanupOutcome.message}`,
    })
    : result;
  const terminal = {
    attemptId: start.attempt.attemptId,
    cleanup: cleanupOutcome,
    coldProofSha256: start.coldProofSha256,
    contractVersion: BACKEND_PERFORMANCE_DIRECT_NATIVE_COLD_CLIENT_VERSION,
    controllerEvidence: prepared === null ? null : {
      cleanedPacketSha256,
      endpointIdSha256: prepared.api.endpointIdSha256,
      knownHostsSha256: start.knownHosts.sha256,
      networkIdSha256: prepared.api.networkIdSha256,
      preparedPacketSha256: receiptSha256(prepared),
      remotePort: start.remotePort,
    },
    result: terminalResult,
    type: "terminal",
  };
  await transport.writeLine(terminal);
  return terminal;
}

// create one queued json-lines transport
function createJsonLineTransport(input, output) {
  const reader = createInterface({ input, crlfDelay: Infinity });
  const queue = [];
  const waiters = [];
  let ended = false;
  reader.on("line", (line) => {
    const waiter = waiters.shift();
    // deliver directly to one bounded reader
    if (waiter !== undefined) {
      clearTimeout(waiter.timer);
      waiter.resolve(line);
      return;
    }
    queue.push(line);
  });
  reader.on("close", () => {
    ended = true;
    // reject every pending bounded reader
    for (const waiter of waiters.splice(0)) {
      clearTimeout(waiter.timer);
      waiter.reject(new Error("controller input closed"));
    }
  });
  return {
    // close only this input interface
    close() {
      reader.close();
    },
    // read one bounded json line
    async readLine(timeoutMs) {
      let line;
      // consume one already queued line
      if (queue.length > 0) {
        line = queue.shift();
      } else {
        requireInvariant(!ended, "native-cold-client.protocol", "controller input is closed");
        line = await new Promise((resolve, reject) => {
          const waiter = { reject, resolve, timer: null };
          waiter.timer = setTimeout(
            // remove one expired packet waiter
            () => {
              const index = waiters.indexOf(waiter);
              // remove only the same pending waiter
              if (index >= 0) {
                waiters.splice(index, 1);
              }
              reject(new Error("controller packet timeout"));
            },
            timeoutMs,
          );
          waiters.push(waiter);
        });
      }
      try {
        return JSON.parse(line);
      } catch {
        fail("native-cold-client.protocol", "controller line is not JSON");
      }
    },
    // write one complete json packet
    async writeLine(packet) {
      await new Promise((resolve, reject) => {
        output.write(`${JSON.stringify(packet)}\n`, (error) => {
          // reject one output write failure
          if (error !== null && error !== undefined) {
            reject(error);
            return;
          }
          resolve();
        });
      });
    },
  };
}

// execute the stdin/stdout controller protocol
async function runCli() {
  const transport = createJsonLineTransport(process.stdin, process.stdout);
  try {
    const terminal = await runBackendPerformanceDirectNativeColdClient(transport);
    // preserve a stopped attempt in the process status
    if (terminal.result.status !== "complete") {
      process.exitCode = 1;
    }
  } catch (error) {
    await transport.writeLine({
      code: error?.code ?? "native-cold-client.error",
      message: error instanceof Error ? error.message : String(error),
      status: "error",
      type: "terminal",
    });
    process.exitCode = 1;
  } finally {
    transport.close();
  }
}

// execute only on direct invocation
if (process.argv[1] !== undefined
  && import.meta.url === pathToFileURL(process.argv[1]).href) {
  void runCli();
}
