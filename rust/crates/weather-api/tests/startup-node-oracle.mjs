import assert from "node:assert/strict";

import { startWeatherApi } from "../../../../apps/api/dist/main.js";

// build one inert listener that records rather than opening a socket
function recordingServer(events, name) {
  return {
    // retain exact listener ordering and arguments
    listen(port, host) {
      events.push(`listen:${name}:${host}:${String(port)}`);
      return this;
    },
  };
}

const activeEvents = [];
const activeWind = {
  bundle: { artifactKind: "wind_transfer_canary_runtime_bundle" },
  reasonCode: null,
  state: "active",
};
const activeStarted = await startWeatherApi({
  // select the active canary without qualified fallback
  async loadForecastAdjustmentWindCanaryRuntime() {
    activeEvents.push("wind");
    return activeWind;
  },
  // detect an invalid qualified load after active canary selection
  async loadForecastAdjustmentRuntime() {
    activeEvents.push("qualified");
    throw new Error("qualified loader must remain lazy");
  },
  // prove temperature failure remains independently contained
  async loadForecastAdjustmentTemperatureCanaryRuntime() {
    activeEvents.push("temperature");
    throw new Error("temperature loader failure");
  },
  // prove rain failure remains independently contained
  async loadForecastAdjustmentRainRuntime() {
    activeEvents.push("rain");
    throw new Error("rain loader failure");
  },
  // expose the one shared startup clock capture
  now() {
    activeEvents.push("clock");
    return new Date("2026-09-02T01:02:03.000Z");
  },
  // prepare both inert listeners from immutable snapshots
  async prepareServer(adjustment, temperature, rain) {
    activeEvents.push("prepare");
    assert.equal(adjustment.loadedAt, temperature.loadedAt);
    assert.equal(adjustment.loadedAt, rain.loadedAt);
    assert.equal(adjustment.runtime, activeWind);
    assert.equal(temperature.runtime.reasonCode, "bundle_invalid");
    assert.equal(rain.runtime.reasonCode, "registry_invalid");
    return {
      maintenancePort: 8081,
      maintenanceServer: recordingServer(activeEvents, "maintenance"),
      port: 8080,
      server: recordingServer(activeEvents, "public"),
    };
  },
});

assert.equal(activeStarted.adjustment.runtime, activeWind);
assert.notEqual(activeStarted.maintenanceServer, null);
assert.deepEqual(activeEvents, [
  "wind",
  "temperature",
  "rain",
  "clock",
  "prepare",
  "listen:public:0.0.0.0:8080",
  "listen:maintenance:0.0.0.0:8081",
]);

const fallbackEvents = [];
const qualifiedWind = {
  bundle: null,
  reasonCode: "bundle_missing",
  state: "disabled",
};
const fallbackStarted = await startWeatherApi({
  // allow qualified fallback only for exact inactive registry state
  async loadForecastAdjustmentWindCanaryRuntime() {
    fallbackEvents.push("wind");
    return { bundle: null, reasonCode: "registry_inactive", state: "disabled" };
  },
  // retain the qualified startup snapshot
  async loadForecastAdjustmentRuntime() {
    fallbackEvents.push("qualified");
    return qualifiedWind;
  },
  // retain independently admitted raw family selections
  async loadForecastAdjustmentTemperatureCanaryRuntime() {
    fallbackEvents.push("temperature");
    return { bundle: null, reasonCode: "registry_inactive", state: "disabled" };
  },
  // retain independently admitted raw rain selection
  async loadForecastAdjustmentRainRuntime() {
    fallbackEvents.push("rain");
    return { artifactSha256: null, reasonCode: "policy_raw", state: "disabled" };
  },
  // expose one shared fallback snapshot clock
  now() {
    fallbackEvents.push("clock");
    return new Date("2026-09-02T01:02:03.000Z");
  },
  // prepare only the inert public listener
  async prepareServer(adjustment) {
    fallbackEvents.push("prepare");
    assert.equal(adjustment.runtime, qualifiedWind);
    return {
      port: 8080,
      server: recordingServer(fallbackEvents, "public"),
    };
  },
});

assert.equal(fallbackStarted.adjustment.runtime, qualifiedWind);
assert.equal(fallbackStarted.maintenanceServer, null);
assert.deepEqual(fallbackEvents, [
  "wind",
  "qualified",
  "temperature",
  "rain",
  "clock",
  "prepare",
  "listen:public:0.0.0.0:8080",
]);

const terminalEvents = [];
const terminalWind = {
  bundle: null,
  reasonCode: "canary_killed",
  state: "disabled",
};
const terminalStarted = await startWeatherApi({
  // retain an explicit terminal canary result
  async loadForecastAdjustmentWindCanaryRuntime() {
    terminalEvents.push("wind");
    return terminalWind;
  },
  // detect unsafe fallback after a terminal canary result
  async loadForecastAdjustmentRuntime() {
    terminalEvents.push("qualified");
    throw new Error("terminal canary must not fall through");
  },
  // retain an independent raw temperature result
  async loadForecastAdjustmentTemperatureCanaryRuntime() {
    terminalEvents.push("temperature");
    return { bundle: null, reasonCode: "registry_inactive", state: "disabled" };
  },
  // retain an independent raw rain result
  async loadForecastAdjustmentRainRuntime() {
    terminalEvents.push("rain");
    return { artifactSha256: null, reasonCode: "policy_raw", state: "disabled" };
  },
  // expose the one terminal startup clock capture
  now() {
    terminalEvents.push("clock");
    return new Date("2026-09-02T01:02:03.000Z");
  },
  // prepare only the inert public listener
  async prepareServer(adjustment) {
    terminalEvents.push("prepare");
    assert.equal(adjustment.runtime, terminalWind);
    return {
      port: 8080,
      server: recordingServer(terminalEvents, "public"),
    };
  },
});

assert.equal(terminalStarted.adjustment.runtime, terminalWind);
assert.deepEqual(terminalEvents, [
  "wind",
  "temperature",
  "rain",
  "clock",
  "prepare",
  "listen:public:0.0.0.0:8080",
]);

process.stdout.write("weather API startup Node oracle passed\n");
