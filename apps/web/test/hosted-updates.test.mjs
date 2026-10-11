import assert from "node:assert/strict";
import test from "node:test";
import { isAndroidHostedShell, parseHostedUpdateState } from "../dist/hosted-updates.js";

// recognize existing and reduced android webview user agents only
test("hosted OTA prompts target Android WebView rather than Chrome or iOS", () => {
  assert.equal(isAndroidHostedShell("Mozilla/5.0 (Linux; Android 16; Pixel 4 Build/AP; wv) AppleWebKit/537.36 Version/4.0 Chrome/140.0.0.0 Mobile Safari/537.36"), true);
  assert.equal(isAndroidHostedShell("Mozilla/5.0 (Linux; Android 10; K; wv) AppleWebKit/537.36 Version/4.0 Chrome/140.0.0.0 Mobile Safari/537.36"), true);
  assert.equal(isAndroidHostedShell("Mozilla/5.0 (Linux; Android 16; Pixel 4) Chrome/140.0.0.0 Mobile Safari/537.36"), false);
  assert.equal(isAndroidHostedShell("Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X) AppleWebKit/605.1.15"), false);
});

// fail closed on malformed release labels and impossible progress
test("worker update state accepts only complete bounded metadata", () => {
  const ready = { type: "WEATHER_UPDATE_STATE", release: "2026.10.11-4", phase: "ready", completed: 39, total: 39 };
  assert.deepEqual(parseHostedUpdateState(ready), ready);
  // exercise every closed progress and identity boundary
  for (const value of [null, "ready", {}, { ...ready, type: "unrelated" }, { ...ready, release: "<script>" }, { ...ready, release: "a".repeat(65) }, { ...ready, phase: "installed" }, { ...ready, completed: 0 }, { ...ready, completed: -1 }, { ...ready, completed: 40 }, { ...ready, completed: 0.5 }, { ...ready, total: 0 }, { ...ready, total: 257 }]) {
    assert.equal(parseHostedUpdateState(value), null);
  }
  assert.ok(parseHostedUpdateState({ ...ready, phase: "downloading", completed: 1 }));
  assert.ok(parseHostedUpdateState({ ...ready, phase: "failed", completed: 1 }));
});
