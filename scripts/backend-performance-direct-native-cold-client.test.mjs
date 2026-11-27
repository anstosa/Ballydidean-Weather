import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdtemp, rm, writeFile, chmod } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import {
  buildBackendPerformanceDirectNativeColdTunnelArguments,
  runBackendPerformanceDirectNativeColdClient,
} from "./backend-performance-direct-native-cold-client.mjs";
import { buildBackendPerformanceFunctionalSchedule } from
  "./backend-performance-functional-schedule.mjs";
import { canonicalReceiptBytes, receiptSha256 } from
  "./backend-performance-receipts.mjs";

// select one exact source attempt from one stratum
function attemptFor(stratum) {
  return buildBackendPerformanceFunctionalSchedule().runSlots.flatMap(
    // retain run-slot order
    (runSlot) => runSlot.groups.flatMap(
      // retain group order
      (group) => group.attempts,
    ),
  ).find(
    // select the first exact stratum member
    (attempt) => attempt.stratum === stratum,
  );
}

// select one exact operation from one stratum
function attemptForOperation(stratum, operationId) {
  return buildBackendPerformanceFunctionalSchedule().runSlots.flatMap(
    // retain run-slot order
    (runSlot) => runSlot.groups.flatMap(
      // retain group order
      (group) => group.attempts,
    ),
  ).find(
    // select the exact source operation and stratum
    (attempt) => attempt.stratum === stratum && attempt.operationId === operationId,
  );
}

// build one sanitized actual-shape D-loop proof
function coldProof(fixtureId) {
  const root = `/var/lib/weather-native-benchmark/${fixtureId}`;
  const volumeNames = Array.from(
    { length: 5 },
    // retain the exact five-slot namespace
    (_unused, index) => `weather-native-benchmark-${fixtureId}-slot-${String(index + 1).padStart(2, "0")}`,
  );
  return {
    completedAt: "2026-11-27T00:00:00.000Z",
    contractVersion: "weather-page-d-loop-cold-proof/v1",
    inner: {
      completedAt: "2026-11-26T23:59:59.000Z",
      completedFiles: 1_502,
      fadviseCompleted: true,
      fallbackUsed: false,
      mincoreCompleted: true,
      startedAt: "2026-11-26T23:59:58.000Z",
      totalPages: 3_308_765,
      totalResidentAfter: 0,
    },
    outer: {
      backingFileBytes: 19_864_223_744,
      completedAt: "2026-11-27T00:00:00.000Z",
      completedFiles: 1,
      fadviseCompleted: true,
      fallbackUsed: false,
      mincoreCompleted: true,
      startedAt: "2026-11-26T23:59:59.000Z",
      totalPages: 4_849_664,
      totalResidentAfter: 0,
    },
    startedAt: "2026-11-26T23:59:58.000Z",
    storageIdentity: {
      backingFile: `${root}/fixture.ext4`,
      backingFileBytes: 19_864_223_744,
      loopDevice: "/dev/loop1",
      loopUuid: "e2386e28-d95b-40b5-978f-59d5161a1db3",
      mount: `${root}/mnt`,
      mountSource: "/dev/loop1",
      volumeNames,
    },
    target: {
      fileCount: 1_502,
      inventoryAfterSha256: "e".repeat(64),
      inventoryBeforeSha256: "e".repeat(64),
      pgdataRoot: `${root}/mnt/docker-volumes/${volumeNames[0]}`,
      postmasterPidAbsent: true,
      totalBytes: 13_552_629_161,
      volumeName: volumeNames[0],
    },
    volumes: volumeNames.map(
      // retain one detached record per exact slot
      (name) => ({ name, runningAttachments: [] }),
    ),
  };
}

// encode one exact proof byte sequence
function proofBytes(proof) {
  return canonicalReceiptBytes(proof);
}

// create one exact initial protocol packet
function startPacket(knownHostsFile, attemptId, fixtureId = "rust-rewrite-0123456789abcdef") {
  const bytes = proofBytes(coldProof(fixtureId));
  return {
    attemptId,
    coldProofBase64: bytes.toString("base64"),
    coldProofSha256: createHash("sha256").update(bytes).digest("hex"),
    fixtureId,
    knownHostsFile,
    sourceReceiptSha256: "a".repeat(64),
    type: "start",
  };
}

// create one reviewed prepared packet
function preparedPacket(
  attemptId,
  fixtureId = "rust-rewrite-0123456789abcdef",
  port = 3_001,
) {
  return {
    api: {
      bridgeIpv4: "172.20.0.3",
      cleanupIdentitySha256: "b".repeat(64),
      endpointIdSha256: "c".repeat(64),
      networkIdSha256: "d".repeat(64),
      port,
      processIdentitySha256: "f".repeat(64),
      readyAt: "2026-11-27T00:00:02.000Z",
      startedAt: "2026-11-27T00:00:01.000Z",
    },
    attemptId,
    fixtureId,
    sourceReceiptSha256: "a".repeat(64),
    type: "prepared",
  };
}

// create one exact cleanup acknowledgement
function cleanedPacket(attemptId, fixtureId = "rust-rewrite-0123456789abcdef") {
  return {
    attemptId,
    cleanupIdentitySha256: "b".repeat(64),
    fixtureId,
    stoppedAt: "2026-11-27T00:00:03.000Z",
    type: "cleaned",
  };
}

// create one deterministic in-memory packet transport
function transport(packets) {
  const readTimeouts = [];
  const writes = [];
  return {
    packets: [...packets],
    readTimeouts,
    writes,
    // read one queued controller packet
    async readLine(timeoutMs) {
      readTimeouts.push(timeoutMs);
      return this.packets.shift() ?? null;
    },
    // retain one client packet
    async writeLine(packet) {
      writes.push(packet);
    },
  };
}

// create one private pinned known-hosts file
async function knownHostsFixture() {
  const directory = await mkdtemp(join(tmpdir(), "weather-native-cold-client-"));
  const path = join(directory, "known_hosts");
  await writeFile(path, "192.168.11.94 ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAITestOnly\n");
  await chmod(path, 0o600);
  return { directory, path };
}

test("runs one exact db-os-cold request through the reviewed protocol", async () => {
  const fixture = await knownHostsFixture();
  try {
    const attempt = attemptForOperation("db-os-cold", "api.current");
    const lineTransport = transport([
      startPacket(fixture.path, attempt.attemptId),
      preparedPacket(attempt.attemptId),
      cleanedPacket(attempt.attemptId),
    ]);
    const tunnels = [];
    let fetches = 0;
    const terminal = await runBackendPerformanceDirectNativeColdClient(lineTransport, {
      async closeTunnel(child) {
        child.closed = true;
      },
      async fetch(url) {
        fetches += 1;
        assert.equal(String(url).startsWith("http://127.0.0.1:13001/"), true);
        return new Response('{"ok":true}', {
          headers: { "content-type": "application/json" },
          status: 200,
        });
      },
      monotonicMilliseconds: () => 1_000,
      spawnTunnel(knownHostsFile, bridgeIpv4, remotePort) {
        const child = { bridgeIpv4, closed: false, knownHostsFile, remotePort };
        tunnels.push(child);
        return child;
      },
      async waitTunnel() {},
    });

    assert.equal(terminal.type, "terminal");
    assert.equal(terminal.result.status, "complete");
    assert.equal(terminal.result.nativeEligibility, "requires-external-proof-adjudication");
    assert.equal(terminal.result.operationReceipts[0].nativeEligible, false);
    assert.deepEqual(terminal.cleanup, { code: null, message: null, status: "complete" });
    assert.equal(terminal.coldProofSha256, receiptSha256(coldProof(
      "rust-rewrite-0123456789abcdef",
    )));
    assert.deepEqual(terminal.controllerEvidence, {
      cleanedPacketSha256: receiptSha256(cleanedPacket(attempt.attemptId)),
      endpointIdSha256: "c".repeat(64),
      knownHostsSha256: createHash("sha256").update(
        "192.168.11.94 ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAITestOnly\n",
      ).digest("hex"),
      networkIdSha256: "d".repeat(64),
      preparedPacketSha256: receiptSha256(preparedPacket(attempt.attemptId)),
      remotePort: 3_001,
    });
    assert.equal(fetches, 1);
    assert.equal(tunnels.length, 1);
    assert.equal(tunnels[0].bridgeIpv4, "172.20.0.3");
    assert.equal(tunnels[0].knownHostsFile, fixture.path);
    assert.equal(tunnels[0].remotePort, 3_001);
    assert.equal(tunnels[0].closed, true);
    assert.deepEqual(lineTransport.writes.map(
      // retain protocol state order
      (packet) => packet.type,
    ), ["prepare", "cleanup", "terminal"]);
    assert.equal(lineTransport.writes[0].attemptId, attempt.attemptId);
    assert.equal(lineTransport.writes[1].cleanupIdentitySha256, "b".repeat(64));
  } finally {
    await rm(fixture.directory, { force: true, recursive: true });
  }
});

test("constructs only the fixed strict-host-key no-command tunnel", () => {
  const apiArguments = buildBackendPerformanceDirectNativeColdTunnelArguments(
    "/tmp/pinned-known-hosts",
    "172.20.0.3",
  );

  assert.deepEqual(apiArguments, [
    "-N",
    "-T",
    "-o", "BatchMode=yes",
    "-o", "ExitOnForwardFailure=yes",
    "-o", "IdentitiesOnly=yes",
    "-o", "StrictHostKeyChecking=yes",
    "-o", "UserKnownHostsFile=/tmp/pinned-known-hosts",
    "-L", "127.0.0.1:13001:172.20.0.3:3001",
    "admin@192.168.11.94",
  ]);
  const webArguments = buildBackendPerformanceDirectNativeColdTunnelArguments(
    "/tmp/pinned-known-hosts",
    "172.20.0.4",
    3_000,
  );
  assert.equal(webArguments.at(-2), "127.0.0.1:13001:172.20.0.4:3000");
  assert.throws(
    () => buildBackendPerformanceDirectNativeColdTunnelArguments(
      "/tmp/pinned-known-hosts",
      "172.20.0.4",
      3_002,
    ),
    { code: "native-cold-client.tunnel" },
  );
});

test("requires immutable page edge and API service port profiles", async () => {
  const fixture = await knownHostsFixture();
  try {
    const cases = [
      [attemptForOperation("db-os-cold", "page.home"), 3_001],
      [attemptForOperation("db-os-cold", "edge.viewer-context"), 3_001],
      [attemptForOperation("db-os-cold", "api.current"), 3_000],
    ];
    // reject each cross-profile port before opening SSH
    for (const [attempt, port] of cases) {
      const lineTransport = transport([
        startPacket(fixture.path, attempt.attemptId),
        preparedPacket(attempt.attemptId, "rust-rewrite-0123456789abcdef", port),
      ]);
      let tunnels = 0;
      const terminal = await runBackendPerformanceDirectNativeColdClient(lineTransport, {
        monotonicMilliseconds: () => 1_000,
        spawnTunnel() {
          tunnels += 1;
          return {};
        },
      });

      assert.equal(terminal.result.status, "stopped");
      assert.equal(terminal.result.attempts[0].errorCategory, "process");
      assert.equal(tunnels, 0);
    }
  } finally {
    await rm(fixture.directory, { force: true, recursive: true });
  }
});

test("permits reviewed page and edge profiles only on fixed web port 3000", async () => {
  const fixture = await knownHostsFixture();
  try {
    const operations = ["page.home", "edge.viewer-context"];
    // exercise both immutable web operation categories
    for (const operationId of operations) {
      const attempt = attemptForOperation("db-os-cold", operationId);
      const lineTransport = transport([
        startPacket(fixture.path, attempt.attemptId),
        preparedPacket(attempt.attemptId, "rust-rewrite-0123456789abcdef", 3_000),
        cleanedPacket(attempt.attemptId),
      ]);
      const ports = [];
      const terminal = await runBackendPerformanceDirectNativeColdClient(lineTransport, {
        async closeTunnel() {},
        async fetch() {
          // return the exact response family expected by the operation category
          if (operationId.startsWith("page.")) {
            return new Response("<!doctype html><html></html>", {
              headers: { "content-type": "text/html" },
              status: 200,
            });
          }
          return new Response('{"ok":true}', {
            headers: { "content-type": "application/json" },
            status: 200,
          });
        },
        monotonicMilliseconds: () => 1_000,
        spawnTunnel(_knownHostsFile, _bridgeIpv4, remotePort) {
          ports.push(remotePort);
          return {};
        },
        async waitTunnel() {},
      });

      assert.equal(terminal.result.status, "complete");
      assert.equal(terminal.controllerEvidence.remotePort, 3_000);
      assert.deepEqual(ports, [3_000]);
    }
  } finally {
    await rm(fixture.directory, { force: true, recursive: true });
  }
});

test("rejects any non-db-os-cold source attempt before preparation", async () => {
  const fixture = await knownHostsFixture();
  try {
    const attempt = attemptFor("warm");
    const lineTransport = transport([startPacket(fixture.path, attempt.attemptId)]);

    await assert.rejects(
      runBackendPerformanceDirectNativeColdClient(lineTransport),
      { code: "native-http-client.fixture-scope" },
    );
    assert.deepEqual(lineTransport.writes, []);
  } finally {
    await rm(fixture.directory, { force: true, recursive: true });
  }
});

test("rejects a public prepared endpoint without opening a tunnel", async () => {
  const fixture = await knownHostsFixture();
  try {
    const attempt = attemptForOperation("db-os-cold", "api.current");
    const prepared = preparedPacket(attempt.attemptId);
    prepared.api.bridgeIpv4 = "8.8.8.8";
    const lineTransport = transport([
      startPacket(fixture.path, attempt.attemptId),
      prepared,
    ]);
    let tunnels = 0;
    const terminal = await runBackendPerformanceDirectNativeColdClient(lineTransport, {
      monotonicMilliseconds: () => 1_000,
      spawnTunnel() {
        tunnels += 1;
        return {};
      },
    });

    assert.equal(terminal.result.status, "stopped");
    assert.equal(terminal.result.attempts[0].errorCategory, "process");
    assert.equal(terminal.result.operationReceipts.length, 0);
    assert.equal(terminal.controllerEvidence, null);
    assert.equal(terminal.cleanup.status, "failed");
    assert.match(terminal.cleanup.message, /controller-owned cleanup is unconfirmed/u);
    assert.equal(tunnels, 0);
    assert.deepEqual(lineTransport.writes.map(
      // retain fail-closed protocol output
      (packet) => packet.type,
    ), ["prepare", "terminal"]);
  } finally {
    await rm(fixture.directory, { force: true, recursive: true });
  }
});

test("retains unconfirmed cleanup when the prepare response is lost", async () => {
  const fixture = await knownHostsFixture();
  try {
    const attempt = attemptForOperation("db-os-cold", "api.current");
    const lineTransport = transport([startPacket(fixture.path, attempt.attemptId)]);
    const readInitial = lineTransport.readLine;
    // simulate a controller that may have allocated the fixture but lost its handoff
    lineTransport.readLine = async function (timeoutMs) {
      // lose only the preparation response
      if (this.packets.length === 0) {
        throw new Error("controller packet timeout");
      }
      return readInitial.call(this, timeoutMs);
    };
    const terminal = await runBackendPerformanceDirectNativeColdClient(lineTransport, {
      monotonicMilliseconds: () => 1_000,
      // fail on any unadmitted client tunnel
      spawnTunnel() {
        assert.fail("must not open a tunnel without validated preparation");
      },
    });
    assert.equal(terminal.result.status, "stopped");
    assert.equal(terminal.result.attempts.length, 1);
    assert.equal(terminal.result.operationReceipts.length, 0);
    assert.equal(terminal.cleanup.status, "failed");
    assert.match(terminal.cleanup.message, /controller-owned cleanup is unconfirmed/u);
    assert.deepEqual(lineTransport.writes.map(
      // retain the terminal handoff without inventing a cleanup identity
      (packet) => packet.type,
    ), ["prepare", "terminal"]);
  } finally {
    await rm(fixture.directory, { force: true, recursive: true });
  }
});

test("fails a mismatched cleanup acknowledgement without native credit", async () => {
  const fixture = await knownHostsFixture();
  try {
    const attempt = attemptForOperation("db-os-cold", "api.current");
    const cleaned = cleanedPacket(attempt.attemptId);
    cleaned.cleanupIdentitySha256 = "0".repeat(64);
    const lineTransport = transport([
      startPacket(fixture.path, attempt.attemptId),
      preparedPacket(attempt.attemptId),
      cleaned,
    ]);
    const terminal = await runBackendPerformanceDirectNativeColdClient(lineTransport, {
      async closeTunnel() {},
      async fetch() {
        return new Response('{"ok":true}', {
          headers: { "content-type": "application/json" },
          status: 200,
        });
      },
      monotonicMilliseconds: () => 1_000,
      spawnTunnel() {
        return {};
      },
      async waitTunnel() {},
    });

    assert.equal(terminal.result.status, "stopped");
    assert.equal(terminal.result.operationReceipts.length, 0);
    assert.equal(terminal.result.attempts.length, 1);
    assert.equal(terminal.result.terminalCounts.completed, 1);
    assert.equal(terminal.result.nativeEligibility, "requires-external-proof-adjudication");
    assert.equal(terminal.cleanup.status, "failed");
    assert.match(terminal.result.stopReason, /^fixture cleanup failed:/u);
  } finally {
    await rm(fixture.directory, { force: true, recursive: true });
  }
});

test("uses a fresh cleanup deadline after the request deadline elapsed", async () => {
  const fixture = await knownHostsFixture();
  try {
    const attempt = attemptForOperation("db-os-cold", "api.current");
    const lineTransport = transport([
      startPacket(fixture.path, attempt.attemptId),
      preparedPacket(attempt.attemptId),
      cleanedPacket(attempt.attemptId),
    ]);
    const readings = [0, 0, 40_000, 40_000];
    const terminal = await runBackendPerformanceDirectNativeColdClient(lineTransport, {
      async closeTunnel() {},
      async fetch() {
        return new Response('{"error":"ordinary"}', {
          headers: { "content-type": "application/json" },
          status: 503,
        });
      },
      monotonicMilliseconds() {
        return readings.shift() ?? 40_000;
      },
      spawnTunnel() {
        return {};
      },
      async waitTunnel() {},
    });

    assert.equal(terminal.result.status, "complete");
    assert.equal(terminal.result.attempts.length, 1);
    assert.equal(terminal.result.attempts[0].errorCategory, "http");
    assert.deepEqual(terminal.result.terminalCounts, { completed: 0, error: 1, timeout: 0 });
    assert.equal(terminal.cleanup.status, "complete");
    assert.deepEqual(lineTransport.readTimeouts, [60_000, 30_000, 120_000]);
    assert.equal(lineTransport.writes.at(-1).type, "terminal");
  } finally {
    await rm(fixture.directory, { force: true, recursive: true });
  }
});

test("retains one timeout denominator row through acknowledged cleanup", async (context) => {
  const fixture = await knownHostsFixture();
  const nativeSetTimeout = globalThis.setTimeout;
  context.mock.method(
    globalThis,
    "setTimeout",
    // accelerate only the immutable 30-second evaluator deadline
    (callback, delay, ...argumentsList) => nativeSetTimeout(
      callback,
      delay === 30_000 ? 0 : delay,
      ...argumentsList,
    ),
  );
  try {
    const attempt = attemptForOperation("db-os-cold", "api.current");
    const lineTransport = transport([
      startPacket(fixture.path, attempt.attemptId),
      preparedPacket(attempt.attemptId),
      cleanedPacket(attempt.attemptId),
    ]);
    const terminal = await runBackendPerformanceDirectNativeColdClient(lineTransport, {
      async closeTunnel() {},
      async fetch(_url, options) {
        await new Promise((resolve, reject) => {
          // reject only when the evaluator aborts this request
          if (options.signal.aborted) {
            reject(new Error("aborted"));
            return;
          }
          options.signal.addEventListener("abort", () => reject(new Error("aborted")), {
            once: true,
          });
        });
      },
      monotonicMilliseconds: () => 1_000,
      spawnTunnel() {
        return {};
      },
      async waitTunnel() {},
    });

    assert.equal(terminal.result.status, "stopped");
    assert.equal(terminal.result.attempts.length, 1);
    assert.equal(terminal.result.attempts[0].terminalCategory, "timeout");
    assert.deepEqual(terminal.result.terminalCounts, { completed: 0, error: 0, timeout: 1 });
    assert.equal(terminal.cleanup.status, "complete");
    assert.equal(lineTransport.writes.at(-1).type, "terminal");
  } finally {
    await rm(fixture.directory, { force: true, recursive: true });
  }
});

test("retains the request row but denies completion on cleanup timeout", async () => {
  const fixture = await knownHostsFixture();
  try {
    const attempt = attemptForOperation("db-os-cold", "api.current");
    const packets = [
      startPacket(fixture.path, attempt.attemptId),
      preparedPacket(attempt.attemptId),
    ];
    const lineTransport = transport([]);
    let reads = 0;
    const readTimeouts = [];
    lineTransport.readLine = async (timeoutMs) => {
      reads += 1;
      readTimeouts.push(timeoutMs);
      // simulate the bounded controller acknowledgement timeout
      if (reads === 3) {
        throw new Error("controller packet timeout");
      }
      return packets.shift();
    };
    const terminal = await runBackendPerformanceDirectNativeColdClient(lineTransport, {
      async closeTunnel() {},
      async fetch() {
        return new Response('{"ok":true}', {
          headers: { "content-type": "application/json" },
          status: 200,
        });
      },
      monotonicMilliseconds: () => 1_000,
      spawnTunnel() {
        return {};
      },
      async waitTunnel() {},
    });

    assert.equal(terminal.result.status, "stopped");
    assert.equal(terminal.result.attempts.length, 1);
    assert.equal(terminal.result.terminalCounts.completed, 1);
    assert.deepEqual(terminal.result.operationReceipts, []);
    assert.equal(terminal.cleanup.status, "failed");
    assert.match(terminal.cleanup.message, /controller packet timeout/u);
    assert.deepEqual(readTimeouts, [60_000, 30_000, 120_000]);
    assert.equal(lineTransport.writes.at(-1).type, "terminal");
  } finally {
    await rm(fixture.directory, { force: true, recursive: true });
  }
});

test("requires one canonical owner-private pinned known-hosts file", async () => {
  const fixture = await knownHostsFixture();
  try {
    await chmod(fixture.path, 0o644);
    const attempt = attemptFor("db-os-cold");
    const lineTransport = transport([startPacket(fixture.path, attempt.attemptId)]);

    await assert.rejects(
      runBackendPerformanceDirectNativeColdClient(lineTransport),
      { code: "native-cold-client.known-hosts" },
    );
    assert.deepEqual(lineTransport.writes, []);
  } finally {
    await rm(fixture.directory, { force: true, recursive: true });
  }
});
