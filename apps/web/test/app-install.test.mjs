import assert from "node:assert/strict";
import test from "node:test";
import { appInstallPlatform, ANDROID_STORE_URL, ANDROID_TESTING_URL } from "../dist/app-install.js";

// cover mobile identities without mistaking installed applications for browsers
test("installation targets ios and android browsers, including desktop-mode ipads", () => {
  assert.equal(appInstallPlatform("iPhone", "iPhone", 5, false), "ios");
  assert.equal(appInstallPlatform("iPad", "iPad", 5, false), "ios");
  assert.equal(appInstallPlatform("Macintosh Safari", "MacIntel", 5, false), "ios");
  assert.equal(appInstallPlatform("Android Chrome", "Linux", 5, false), "android");
  assert.equal(appInstallPlatform("Macintosh Safari", "MacIntel", 0, false), null);
  assert.equal(appInstallPlatform("Windows Chrome", "Win32", 0, false), null);
  assert.equal(appInstallPlatform("Android; wv)", "Linux", 5, false), null);
  assert.equal(appInstallPlatform("Android Chrome", "Linux", 5, true), null);
  assert.equal(appInstallPlatform("iPhone", "iPhone", 5, true), null);
});

// keep both google play destinations isolated to the weather package
test("android installation and testing links use the weather app identity", () => {
  assert.equal(new URL(ANDROID_STORE_URL).searchParams.get("id"), "farm.ballydidean.weather");
  assert.equal(new URL(ANDROID_TESTING_URL).pathname, "/apps/testing/farm.ballydidean.weather");
});
