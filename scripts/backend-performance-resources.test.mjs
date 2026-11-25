import assert from "node:assert/strict";
import { mkdir, mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import {
  RESOURCE_INTERVAL_CONTRACT_VERSION,
  beginBackendResourceInterval,
} from "./backend-performance-resources.mjs";

const cgroupRoot = "/sys/fs/cgroup";
const cgroupPath = `${cgroupRoot}/weather-api`;
const pid = 321;

// construct one externally released promise
function deferred() {
  let release;
  const promise = new Promise(
    // retain one test release function
    (resolve) => { release = resolve; },
  );
  return { promise, release };
}

// wait for one real timer boundary
async function sleep(milliseconds) {
  await new Promise(
    // release after the requested delay
    (resolve) => { setTimeout(resolve, milliseconds); },
  );
}

// format one cgroup counter file
function counterFile(values) {
  return `${Object.entries(values).map(
    // encode each counter line
    ([key, value]) => `${key} ${String(value)}`,
  ).join("\n")}\n`;
}

// build one deterministic collector fixture
function collectorFixture() {
  const state = {
    boundedReadCloses: 0,
    boundedReadLengths: [],
    cgroupInode: 200n,
    cpu: {
      after: {
        nr_periods: 14,
        nr_throttled: 3,
        system_usec: 410,
        throttled_usec: 70,
        usage_usec: 1_600,
        user_usec: 1_190,
      },
      before: {
        nr_periods: 10,
        nr_throttled: 1,
        system_usec: 300,
        throttled_usec: 20,
        usage_usec: 1_000,
        user_usec: 700,
      },
      middle: {
        nr_periods: 12,
        nr_throttled: 2,
        system_usec: 350,
        throttled_usec: 35,
        usage_usec: 1_300,
        user_usec: 950,
      },
    },
    fdEntries: {
      after: ["0", "1", "2", "8", "9"],
      before: ["0", "1", "2", "8"],
      middle: ["0", "1", "2", "8", "9"],
    },
    limits: {
      cpuMax: "50000 100000\n",
      memoryMax: "268435456\n",
      pidsMax: "96\n",
    },
    memoryCurrent: {
      after: 130_000_000,
      before: 100_000_000,
      middle: 120_000_000,
    },
    memoryEvents: {
      after: { high: 1, low: 0, max: 2, oom: 1, oom_group_kill: 0, oom_kill: 1 },
      before: { high: 0, low: 0, max: 0, oom: 0, oom_group_kill: 0, oom_kill: 0 },
      middle: { high: 0, low: 0, max: 1, oom: 0, oom_group_kill: 0, oom_kill: 0 },
    },
    mountFilesystem: "cgroup2",
    namespaces: {
      cgroup: "cgroup:[4026531835]",
      mnt: "mnt:[4026531841]",
    },
    peak: {
      closed: 0,
      reads: [100_000_000, 150_000_000],
      readPositions: [],
      writeBytes: 1,
      writes: [],
    },
    phase: "before",
    process: {
      command: Buffer.from("node\0apps/api/dist/main.js\0", "utf8"),
      executableInode: 300n,
      executablePath: "/usr/local/bin/node",
      rssKiB: { after: 102_400, before: 96_000, middle: 100_000 },
      startTimeTicks: 999_999,
      threads: { after: 13, before: 12, middle: 13 },
    },
    readGate: null,
    readStarted: null,
  };
  const clock = {
    monotonicMs: 1_000,
    wallMs: Date.parse("2026-11-13T20:00:00.000Z"),
  };

  const peakHandle = {
    fd: 44,

    // close the retained descriptor
    async close() {
      state.peak.closed += 1;
    },

    // report the retained descriptor identity
    async stat() {
      return { dev: 8n, ino: 400n };
    },

    // read from one explicit descriptor position
    async read(buffer, offset, length, position) {
      state.peak.readPositions.push(position);
      const value = state.peak.reads.shift();
      const encoded = Buffer.from(`${String(value)}\n`, "utf8");
      encoded.copy(buffer, offset, 0, Math.min(length, encoded.length));
      return { buffer, bytesRead: encoded.length };
    },

    // reset the private descriptor peak
    async write(buffer, offset, length, position) {
      state.peak.writes.push({
        bytes: Buffer.from(buffer).subarray(offset, offset + length).toString("utf8"),
        position,
      });
      return { buffer, bytesWritten: state.peak.writeBytes };
    },
  };

  // produce one deterministic virtual file value
  function fixtureBytes(path) {
    const phase = state.phase;

    // serve cgroup counters and limits
    if (path === `${cgroupPath}/cpu.stat`) {
      return Buffer.from(counterFile(state.cpu[phase]), "utf8");
    }
    // serve one current memory value
    if (path === `${cgroupPath}/memory.current`) {
      return Buffer.from(`${String(state.memoryCurrent[phase])}\n`, "utf8");
    }
    // serve one memory event set
    if (path === `${cgroupPath}/memory.events`) {
      return Buffer.from(counterFile(state.memoryEvents[phase]), "utf8");
    }
    // serve one CPU limit
    if (path === `${cgroupPath}/cpu.max`) {
      return Buffer.from(state.limits.cpuMax, "utf8");
    }
    // serve one memory limit
    if (path === `${cgroupPath}/memory.max`) {
      return Buffer.from(state.limits.memoryMax, "utf8");
    }
    // serve one process limit
    if (path === `${cgroupPath}/pids.max`) {
      return Buffer.from(state.limits.pidsMax, "utf8");
    }
    // serve one controller set
    if (path === `${cgroupPath}/cgroup.controllers`) {
      return Buffer.from("cpu io memory pids\n", "utf8");
    }

    // serve process identity and usage
    if (path === "/proc/sys/kernel/random/boot_id") {
      return Buffer.from("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee\n", "utf8");
    }
    // serve one unified membership
    if (path === `/proc/${String(pid)}/cgroup`) {
      return Buffer.from("0::/weather-api\n", "utf8");
    }
    // serve one process command
    if (path === `/proc/${String(pid)}/cmdline`) {
      return state.process.command;
    }
    // serve one process start time
    if (path === `/proc/${String(pid)}/stat`) {
      return Buffer.from(`${String(pid)} (weather api) S 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 ${String(state.process.startTimeTicks)} 20\n`, "utf8");
    }
    // serve one process usage sample
    if (path === `/proc/${String(pid)}/status`) {
      return Buffer.from(`Name:\tweather\nVmRSS:\t${String(state.process.rssKiB[phase])} kB\nThreads:\t${String(state.process.threads[phase])}\n`, "utf8");
    }
    // serve one cgroup mount record
    if (path === "/proc/self/mountinfo") {
      return Buffer.from(`36 25 0:29 / ${cgroupRoot} rw,nosuid,nodev,noexec,relatime - ${state.mountFilesystem} cgroup rw\n`, "utf8");
    }
    throw new Error(`unexpected read ${path}`);
  }

  const fs = {
    // open one retained peak or bounded read descriptor
    async open(path, flags) {
      // return the retained read-write peak descriptor
      if (path === `${cgroupPath}/memory.peak`) {
        assert.equal(flags, "r+");
        return peakHandle;
      }
      assert.equal(flags, "r");
      const bytes = fixtureBytes(path);
      let cursor = 0;

      return {
        // close one bounded fixture descriptor
        async close() {
          state.boundedReadCloses += 1;
        },

        // read one bounded fixture chunk
        async read(buffer, offset, length, position) {
          state.boundedReadLengths.push(length);
          // pause one selected observation read
          if (state.readGate !== null && path === `${cgroupPath}/memory.current`) {
            state.readStarted?.release();
            const gate = state.readGate;
            state.readGate = null;
            await gate.promise;
          }
          const start = position === null ? cursor : position;
          const bytesRead = Math.min(length, bytes.length - start);

          bytes.copy(buffer, offset, start, start + bytesRead);
          // advance one sequential read cursor
          if (position === null) {
            cursor += bytesRead;
          }
          return { buffer, bytesRead };
        },
      };
    },

    // stream one process descriptor snapshot
    async opendir(path) {
      assert.equal(path, `/proc/${String(pid)}/fd`);
      const entries = state.fdEntries[state.phase];
      let index = 0;

      return {
        // close one fixture directory stream
        async close() {},

        // read one descriptor entry
        async read() {
          // return the observed directory boundary
          if (index >= entries.length) {
            return null;
          }
          const entry = { name: entries[index] };
          index += 1;
          return entry;
        },
      };
    },

    // resolve fixture paths without aliases
    async realpath(path) {
      return path;
    },

    // observe the target executable path
    async readlink(path) {
      // expose one mount namespace identity
      if (path === "/proc/self/ns/mnt") {
        return state.namespaces.mnt;
      }
      // expose one cgroup namespace identity
      if (path === "/proc/self/ns/cgroup") {
        return state.namespaces.cgroup;
      }
      assert.equal(path, `/proc/${String(pid)}/exe`);
      return state.process.executablePath;
    },

    // report stable filesystem identities
    async stat(path) {
      // identify the cgroup directories
      if (path === cgroupRoot) {
        return { dev: 8n, ino: 100n, isDirectory: () => true };
      }
      // identify the measured cgroup
      if (path === cgroupPath) {
        return { dev: 8n, ino: state.cgroupInode, isDirectory: () => true };
      }
      // identify the measured executable
      if (path === `/proc/${String(pid)}/exe`) {
        return { dev: 8n, ino: state.process.executableInode, isDirectory: () => false };
      }
      throw new Error(`unexpected stat ${path}`);
    },

    // report the native cgroup filesystem magic
    async statfs(path) {
      assert.ok(path === cgroupRoot || path === cgroupPath);
      return { type: 0x63677270n };
    },
  };

  // return one controllable clock sample
  function now() {
    return { ...clock };
  }

  // advance both wall and monotonic clocks
  function tick(milliseconds) {
    clock.monotonicMs += milliseconds;
    clock.wallMs += milliseconds;
  }

  return { fs, now, state, tick };
}

// build one valid service collection request
function serviceConfiguration(overrides = {}) {
  return {
    accountingScope: "service",
    cgroupPath,
    cgroupRoot,
    deadlineMs: 5_000,
    maximumSamples: 5,
    minimumSampleSpacingMs: 0,
    minimumSamples: 3,
    pid,
    releaseId: "2026.11.04-6",
    scopeId: "api",
    sourceIdentitySha256: "a".repeat(64),
    ...overrides,
  };
}

// verify one complete retained-descriptor interval
test("collects matched raw cgroup and process observations", async () => {
  const fixture = collectorFixture();
  const session = await beginBackendResourceInterval(serviceConfiguration(), {
    fs: fixture.fs,
    now: fixture.now,
  });

  fixture.state.phase = "middle";
  fixture.tick(50);
  await session.sample();
  fixture.state.phase = "after";
  fixture.tick(50);
  const result = await session.finish();

  assert.equal(result.contractVersion, RESOURCE_INTERVAL_CONTRACT_VERSION);
  assert.equal(result.authority.kind, "injected-test-only");
  assert.equal(result.authority.liveEligible, false);
  assert.equal(result.accountingScope, "service");
  assert.deepEqual(result.before, {
    cgroupId: result.cgroupIdentity.sha256,
    observedAt: "2026-11-13T20:00:00.000Z",
    pid,
    releaseId: "2026.11.04-6",
  });
  assert.equal(result.after.cgroupId, result.before.cgroupId);
  assert.equal(result.cpu.usageUsec, 600);
  assert.equal(result.cpu.userUsec, 490);
  assert.equal(result.cpu.systemUsec, 110);
  assert.equal(result.cpu.nrThrottled, 2);
  assert.equal(result.cpu.throttledUsec, 50);
  assert.deepEqual(result.memory.eventsDelta, {
    high: 1,
    low: 0,
    max: 2,
    oom: 1,
    oomGroupKill: 0,
    oomKill: 1,
  });
  assert.equal(result.memory.peak.scope, "private-retained-rw-fd");
  assert.equal(result.memory.peak.initializedBytes, 100_000_000);
  assert.equal(result.memory.peak.valueBytes, 150_000_000);
  assert.equal(result.memory.steady.samples.length, 3);
  assert.equal(result.memory.steady.sampledMaximumBytes, 130_000_000);
  assert.equal(result.process.samples[0].samplingMethod, "instantaneous-not-peak");
  assert.deepEqual(fixture.state.peak.writes, [{ bytes: "1", position: 0 }]);
  assert.deepEqual(fixture.state.peak.readPositions, [0, 0]);
  assert.equal(fixture.state.peak.closed, 1);
  assert.ok(fixture.state.boundedReadCloses > 0);
  assert.ok(Math.max(...fixture.state.boundedReadLengths) <= 1024 * 1024 + 1);
});

// verify unsupported scopes cannot manufacture aggregate evidence
test("supports direct process cgroups only", async () => {
  const fixture = collectorFixture();

  await assert.rejects(
    beginBackendResourceInterval(serviceConfiguration({ pid: null }), { fs: fixture.fs, now: fixture.now }),
    /service scope requires one process/u,
  );
  await assert.rejects(
    beginBackendResourceInterval(serviceConfiguration({
      accountingScope: "app-aggregate",
      commonCgroupEvidenceSha256: "b".repeat(64),
      pid: null,
    }), { fs: fixture.fs, now: fixture.now }),
    /accountingScope is invalid|unsupported resource collector field/u,
  );
  await assert.rejects(
    beginBackendResourceInterval(serviceConfiguration({
      accountingScope: "host",
      pid: null,
    }), { fs: fixture.fs, now: fixture.now }),
    /accountingScope is invalid/u,
  );
});

// verify ordinary directories cannot claim native cgroup authority
test("rejects ordinary filesystems before reading cgroup controls", async () => {
  const root = await mkdtemp(join(tmpdir(), "weather-fake-cgroup-"));
  const group = join(root, "fake-service");

  try {
    await mkdir(group);
    await assert.rejects(
      beginBackendResourceInterval(serviceConfiguration({
        cgroupPath: group,
        cgroupRoot: root,
        minimumSamples: 2,
        pid: process.pid,
      })),
      /native cgroup v2 filesystem/u,
    );
  } finally {
    await rm(root, { force: true, recursive: true });
  }
});

// verify cgroup magic alone cannot replace mount authority
test("rejects a non-cgroup mount record on an injected filesystem", async () => {
  const fixture = collectorFixture();

  fixture.state.mountFilesystem = "ext4";
  await assert.rejects(
    beginBackendResourceInterval(serviceConfiguration(), { fs: fixture.fs, now: fixture.now }),
    /one exact native cgroup v2 filesystem mount/u,
  );
  assert.equal(fixture.state.peak.closed, 0);
});

// verify the kernel peak covers every watched current value
test("rejects a retained peak below an in-window current sample", async () => {
  const fixture = collectorFixture();

  fixture.state.peak.reads = [100_000_000, 110_000_000];
  const session = await beginBackendResourceInterval(serviceConfiguration(), {
    fs: fixture.fs,
    now: fixture.now,
  });
  fixture.state.phase = "middle";
  fixture.tick(50);
  await session.sample();
  fixture.state.phase = "after";
  fixture.tick(50);
  await assert.rejects(session.finish(), /peak is below an in-window memory.current sample/u);
  assert.equal(fixture.state.peak.closed, 1);
});

// verify descriptor enumeration has one hard allocation bound
test("rejects process descriptor enumeration above the collector cap", async () => {
  const fixture = collectorFixture();

  fixture.state.fdEntries.before = Array.from(
    { length: 4_097 },
    // name one synthetic descriptor
    (_value, index) => String(index),
  );
  await assert.rejects(
    beginBackendResourceInterval(serviceConfiguration(), { fs: fixture.fs, now: fixture.now }),
    /descriptor count exceeds/u,
  );
  assert.equal(fixture.state.peak.closed, 0);
});

// verify decreasing counters fail closed and release the descriptor
test("rejects counter decreases and closes the peak descriptor", async () => {
  const fixture = collectorFixture();
  const session = await beginBackendResourceInterval(serviceConfiguration({ minimumSamples: 2 }), {
    fs: fixture.fs,
    now: fixture.now,
  });

  fixture.state.phase = "after";
  fixture.state.cpu.after.usage_usec = 999;
  fixture.tick(50);
  await assert.rejects(session.finish(), /counter decreased/u);
  assert.equal(fixture.state.peak.closed, 1);
});

// verify frozen interval identities cannot drift
test("rejects limit process and cgroup identity drift", async () => {
  const mutations = [
    {
      expected: /limits changed/u,
      // change one frozen resource limit
      mutate: (fixture) => { fixture.state.limits.memoryMax = "536870912\n"; },
    },
    {
      expected: /process identity changed/u,
      // reuse the process identifier with another start time
      mutate: (fixture) => { fixture.state.process.startTimeTicks += 1; },
    },
    {
      expected: /cgroup identity changed/u,
      // replace the measured cgroup inode
      mutate: (fixture) => { fixture.state.cgroupInode += 1n; },
    },
    {
      expected: /native cgroup authority changed/u,
      // replace the active cgroup namespace identity
      mutate: (fixture) => { fixture.state.namespaces.cgroup = "cgroup:[4026531999]"; },
    },
  ];

  // exercise every protected identity independently
  for (const mutation of mutations) {
    const fixture = collectorFixture();
    const session = await beginBackendResourceInterval(serviceConfiguration({ minimumSamples: 2 }), {
      fs: fixture.fs,
      now: fixture.now,
    });

    mutation.mutate(fixture);
    fixture.state.phase = "after";
    fixture.tick(50);
    await assert.rejects(session.finish(), mutation.expected);
    assert.equal(fixture.state.peak.closed, 1);
  }
});

// verify transient process reuse cannot hide between interval endpoints
test("rejects process identity drift in an intermediate sample", async () => {
  const fixture = collectorFixture();
  const session = await beginBackendResourceInterval(serviceConfiguration(), {
    fs: fixture.fs,
    now: fixture.now,
  });

  fixture.state.phase = "middle";
  fixture.state.process.startTimeTicks += 1;
  fixture.tick(50);
  await assert.rejects(session.sample(), /process identity changed/u);
  assert.equal(fixture.state.peak.closed, 1);
});

// verify sampling bounds terminate the session safely
test("enforces bounded samples and deadlines", async () => {
  const sampleFixture = collectorFixture();
  const sampleSession = await beginBackendResourceInterval(serviceConfiguration({
    maximumSamples: 2,
    minimumSamples: 2,
  }), { fs: sampleFixture.fs, now: sampleFixture.now });

  sampleFixture.state.phase = "middle";
  sampleFixture.tick(10);
  await sampleSession.sample();
  sampleFixture.tick(10);
  await assert.rejects(sampleSession.sample(), /sample limit/u);
  assert.equal(sampleFixture.state.peak.closed, 1);

  const deadlineFixture = collectorFixture();
  const deadlineSession = await beginBackendResourceInterval(serviceConfiguration({
    deadlineMs: 100,
    minimumSamples: 2,
  }), { fs: deadlineFixture.fs, now: deadlineFixture.now });

  deadlineFixture.state.phase = "after";
  deadlineFixture.tick(101);
  await assert.rejects(deadlineSession.finish(), /deadline/u);
  assert.equal(deadlineFixture.state.peak.closed, 1);
});

// verify an awaited abort cannot be undone by delayed sampling
test("keeps abort terminal across an in-flight sample", async () => {
  const fixture = collectorFixture();
  const session = await beginBackendResourceInterval(serviceConfiguration(), {
    fs: fixture.fs,
    now: fixture.now,
  });
  const gate = deferred();
  const started = deferred();

  fixture.state.phase = "middle";
  fixture.state.readGate = gate;
  fixture.state.readStarted = started;
  fixture.tick(50);
  const sample = session.sample();
  await started.promise;
  const abort = session.abort();
  gate.release();
  await abort;
  await assert.rejects(sample, /aborted|closed/u);
  await assert.rejects(session.sample(), /aborted|closed/u);
  assert.equal(fixture.state.peak.closed, 1);
});

// verify completion time cannot cross the configured deadline
test("rejects a sample whose I/O completes after the deadline", async () => {
  const fixture = collectorFixture();
  const session = await beginBackendResourceInterval(serviceConfiguration({
    deadlineMs: 100,
  }), { fs: fixture.fs, now: fixture.now });
  const gate = deferred();
  const started = deferred();

  fixture.state.phase = "middle";
  fixture.state.readGate = gate;
  fixture.state.readStarted = started;
  const sample = session.sample();
  await started.promise;
  fixture.tick(101);
  gate.release();
  await assert.rejects(sample, /deadline/u);
  assert.equal(fixture.state.peak.closed, 1);
});

// verify abandoned sessions release retained descriptors automatically
test("closes an abandoned descriptor at its real deadline", async () => {
  const fixture = collectorFixture();
  const session = await beginBackendResourceInterval(serviceConfiguration({
    deadlineMs: 20,
  }), { fs: fixture.fs, now: fixture.now });

  await sleep(50);
  assert.equal(fixture.state.peak.closed, 1);
  await assert.rejects(session.sample(), /deadline|closed/u);
});

// verify explicit cancellation is idempotent
test("closes the retained descriptor on explicit cancellation", async () => {
  const fixture = collectorFixture();
  const session = await beginBackendResourceInterval(serviceConfiguration(), {
    fs: fixture.fs,
    now: fixture.now,
  });

  await session.abort();
  await session.abort();
  await assert.rejects(session.finish(), /aborted|closed/u);
  assert.equal(fixture.state.peak.closed, 1);
});

// verify capability failures close a partially initialized session
test("closes the descriptor when the private peak reset is incomplete", async () => {
  const fixture = collectorFixture();

  fixture.state.peak.writeBytes = 0;
  await assert.rejects(
    beginBackendResourceInterval(serviceConfiguration(), { fs: fixture.fs, now: fixture.now }),
    /reset was incomplete/u,
  );
  assert.equal(fixture.state.peak.closed, 1);
});
