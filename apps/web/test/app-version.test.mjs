import assert from "node:assert/strict";
import test from "node:test";
import { applicationVersions } from "../dist/app-version.js";

const revision = "0123456789abcdef0123456789abcdef01234567";
const native = "Mozilla/5.0 (Linux; Android 16; Pixel; wv)";

// report source identity on browsers without treating an android browser as a native app
test("web and home-screen versions use the loaded short git hash", () => {
  const expected = [{ label: "Version", value: "0123456" }];
  assert.deepEqual(applicationVersions("Chrome", revision, "2026.10.12-1"), expected);
  assert.deepEqual(applicationVersions("Android Chrome", revision, "2026.10.12-1"), expected);
  assert.deepEqual(applicationVersions("iPhone Safari", revision, "2026.10.12-1"), expected);
  assert.deepEqual(applicationVersions("Chrome BallydideanWeather/9.9.9", revision, "2026.10.12-1"), expected);
  assert.deepEqual(applicationVersions("Chrome", "development", "development"), [{ label: "Version", value: "Development" }]);
  assert.equal(applicationVersions("Chrome", "<script>", "2026.10.12-1")[0].value, "Unavailable");
});

// keep repeated semantic versions distinguishable by their installed native build code
test("android separates the installed apk version from the loaded ota version", () => {
  assert.deepEqual(applicationVersions(`${native} BallydideanWeather/0.1.2 BallydideanWeatherBuild/262852359`, revision, "2026.10.12-1"), [
    { label: "App version", value: "0.1.2 (build 262852359)" },
    { label: "OTA version", value: "2026.10.12-1 (0123456)" },
  ]);
  assert.equal(applicationVersions(`${native} BallydideanWeather/0.1.2-debug`, revision, "development")[0].value, "0.1.2-debug");
  assert.equal(applicationVersions(native, revision, "2026.10.12-1")[0].value, "Not reported by this app build");
  assert.equal(applicationVersions(`${native} BallydideanWeather/0.1.2<script>`, revision, "2026.10.12-1")[0].value, "Not reported by this app build");
  assert.equal(applicationVersions(`${native} BallydideanWeather/0.1.2 BallydideanWeatherBuild/9999999999`, revision, "2026.10.12-1")[0].value, "0.1.2");
  assert.equal(applicationVersions(native, "bad", "<script>")[1].value, "Unavailable");
});
