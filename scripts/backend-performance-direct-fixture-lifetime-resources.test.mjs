import assert from "node:assert/strict";
import test from "node:test";

import {
  BACKEND_PERFORMANCE_DIRECT_FIXTURE_LIFETIME_RESOURCES_VERSION,
  reduceBackendPerformanceDirectFixtureLifetimeResources,
} from "./backend-performance-direct-fixture-lifetime-resources.mjs";

const API_ATTEMPT_ID =
  "planned-functional-v4-r2:run-slot-01:1161:api.current:db-os-cold";
const WEB_ATTEMPT_ID =
  "planned-functional-v4-r2:run-slot-01:1152:page.home:db-os-cold";
// construct synthetic raw counters with realistic kernel field order
function serviceSample(id, pid, quota, limit, usage, memory, peak) {
  const containerId = id.repeat(64);
  return {
    containerId,
    pid,
    cgroup: `/system.slice/docker-${containerId}.scope`,
    counters: {
      "cpu.max": `${quota} 100000`,
      "cpu.stat": [
        `usage_usec ${usage}`,
        `user_usec ${usage - 100}`,
        "system_usec 100",
        "nice_usec 0",
        "nr_periods 10",
        "nr_throttled 2",
        "throttled_usec 1000",
        "nr_bursts 0",
        "burst_usec 0",
      ].join("\n"),
      "memory.current": String(memory),
      "memory.events": "low 0\nhigh 0\nmax 0\noom 0\noom_kill 0\noom_group_kill 0",
      "memory.max": String(limit),
      "memory.peak": String(peak),
      "pids.current": "12",
    },
  };
}

// construct an independent synthetic fixture capture
function snapshot(capturedAt, containers) {
  return {
    capturedAt,
    containers,
    hostMeminfo: [
      "MemTotal: 8000000 kB",
      "MemFree: 3000000 kB",
      "MemAvailable: 6000000 kB",
      "Buffers: 100000 kB",
    ],
    hostProcStat: "cpu  100 0 200 300 400 0 0 0 0 0",
    scope: "fresh fixture container lifetime counters, not full application aggregate",
  };
}

// clone one closed observation value
function clone(value) {
  return structuredClone(value);
}

const API_READY = snapshot("2025-01-01T00:00:00Z", {
  apiContainerId: serviceSample("a", 101, 50_000, 268_435_456, 5000, 30_000_000, 50_000_000),
  postgresContainerId: serviceSample("b", 102, 100_000, 536_870_912, 1000, 40_000_000, 40_000_000),
});
const API_END = snapshot("2025-01-01T00:00:01Z", {
  apiContainerId: serviceSample("a", 101, 50_000, 268_435_456, 5100, 31_000_000, 51_000_000),
  postgresContainerId: serviceSample("b", 102, 100_000, 536_870_912, 1200, 41_000_000, 41_000_000),
});
const WEB_READY = snapshot("2025-01-01T00:00:00Z", {
  apiContainerId: serviceSample("d", 201, 50_000, 268_435_456, 3000, 35_000_000, 40_000_000),
  postgresContainerId: serviceSample("e", 202, 100_000, 536_870_912, 1000, 40_000_000, 40_000_000),
  webContainerId: serviceSample("f", 203, 25_000, 402_653_184, 1500, 20_000_000, 20_000_000),
});
const WEB_END = snapshot("2025-01-01T00:00:01Z", {
  apiContainerId: serviceSample("d", 201, 50_000, 268_435_456, 3200, 34_000_000, 40_000_000),
  postgresContainerId: serviceSample("e", 202, 100_000, 536_870_912, 1200, 41_000_000, 41_000_000),
  webContainerId: serviceSample("f", 203, 25_000, 402_653_184, 1600, 21_000_000, 21_000_000),
});

// reduce one api-only fixture pair
function reduceApi(readyObservation = API_READY, terminalObservation = API_END) {
  return reduceBackendPerformanceDirectFixtureLifetimeResources({
    attemptId: API_ATTEMPT_ID,
    profile: "api-only",
    readyObservation,
    terminalObservation,
  });
}

test("reduces the synthetic api-only fixture without aggregate authority", () => {
  const result = reduceApi();
  const api = result.services.find(
    // select the api service observation
    (service) => service.service === "api",
  );
  const postgres = result.services.find(
    // select the postgres service observation
    (service) => service.service === "postgres",
  );

  assert.equal(
    result.contractVersion,
    BACKEND_PERFORMANCE_DIRECT_FIXTURE_LIFETIME_RESOURCES_VERSION,
  );
  assert.deepEqual(result.source, {
    attemptId: API_ATTEMPT_ID,
    operationId: "api.current",
    profile: "api-only",
    runSlotId: "run-slot-01",
    stratum: "db-os-cold",
  });
  assert.equal(result.authority.nativeEligible, false);
  assert.equal(result.authority.officialTarget, null);
  assert.match(result.hostContext.limitation, /not a host resource interval/u);
  assert.deepEqual(result.hostContext.ready.hostMeminfo, API_READY.hostMeminfo);
  assert.equal(result.services.length, 2);
  assert.equal(api.observations.lifetimeCpuUsageUsecAtEnd, 5100);
  assert.equal(api.observations.lifetimeMemoryPeakBytesAtEnd, 51_000_000);
  assert.equal(api.observations.readyMemoryCurrentBytes, 30_000_000);
  assert.equal(api.observations.endMemoryCurrentBytes, 31_000_000);
  assert.equal(api.betweenSamples.cpuStatDelta.usage_usec, 100);
  assert.equal(postgres.observations.lifetimeCpuUsageUsecAtEnd, 1200);
  assert.equal(postgres.observations.lifetimeMemoryPeakBytesAtEnd, 41_000_000);
  assert.equal(Object.hasOwn(result, "aggregate"), false);
  assert.ok(result.services.every(
    // reject any invented steady-state service metric
    (service) => !Object.hasOwn(service, "steady"),
  ));
  assert.equal(Object.isFrozen(result.services[0].samples.end.cpuStat), true);
});

test("reduces the synthetic api-web shape as three independent services", () => {
  const result = reduceBackendPerformanceDirectFixtureLifetimeResources({
    attemptId: WEB_ATTEMPT_ID,
    profile: "api-web",
    readyObservation: WEB_READY,
    terminalObservation: WEB_END,
  });
  const web = result.services.find(
    // select the web service observation
    (service) => service.service === "web",
  );

  assert.deepEqual(result.services.map(
    // retain exact fixture service order
    (service) => service.service,
  ), ["api", "postgres", "web"]);
  assert.deepEqual(web.limits, {
    cpuMax: { periodUsec: 100_000, quotaUsec: 25_000 },
    memoryMaxBytes: 402_653_184,
  });
  assert.equal(web.observations.lifetimeCpuUsageUsecAtEnd, 1600);
  assert.equal(web.observations.lifetimeMemoryPeakBytesAtEnd, 21_000_000);
  assert.equal(web.betweenSamples.cpuStatDelta.usage_usec, 100);
  assert.equal(result.services[0].samples.end.memoryCurrentBytes, 34_000_000);
  assert.equal(result.services[0].samples.ready.memoryCurrentBytes, 35_000_000);
});

test("rejects source profile, identity, and ceiling changes", () => {
  assert.throws(
    // reject a fixture profile outside the source operation binding
    () => reduceBackendPerformanceDirectFixtureLifetimeResources({
      attemptId: API_ATTEMPT_ID,
      profile: "api-web",
      readyObservation: API_READY,
      terminalObservation: API_END,
    }),
    { code: "fixture-resource.profile-binding" },
  );

  const changedIdentity = clone(API_END);
  changedIdentity.containers.apiContainerId.pid += 1;
  assert.throws(
    // reject a changed container identity between samples
    () => reduceApi(API_READY, changedIdentity),
    { code: "fixture-resource.identity" },
  );

  const changedLimit = clone(API_END);
  changedLimit.containers.apiContainerId.counters["cpu.max"] = "60000 100000";
  assert.throws(
    // reject a fixture ceiling outside the fixed profile
    () => reduceApi(API_READY, changedLimit),
    { code: "fixture-resource.limit" },
  );
});

test("rejects inconsistent and decreasing raw counters", () => {
  const inconsistent = clone(API_END);
  inconsistent.containers.apiContainerId.counters["cpu.stat"] =
    inconsistent.containers.apiContainerId.counters["cpu.stat"].replace(
      "usage_usec 5100",
      "usage_usec 5200",
    );
  assert.throws(
    // reject internally inconsistent cpu accounting
    () => reduceApi(API_READY, inconsistent),
    { code: "fixture-resource.counter-consistency" },
  );

  const decreased = clone(API_END);
  decreased.containers.apiContainerId.counters["cpu.stat"] =
    API_READY.containers.apiContainerId.counters["cpu.stat"].replace(
      "usage_usec 5000",
      "usage_usec 4999",
    );
  assert.throws(
    // reject a decreasing lifetime counter across the sample pair
    () => reduceApi(API_READY, decreased),
    { code: "fixture-resource.counter-monotonicity" },
  );
});
