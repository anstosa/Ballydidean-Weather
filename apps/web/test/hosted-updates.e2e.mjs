import assert from "node:assert/strict";
import { existsSync, readdirSync } from "node:fs";
import { join } from "node:path";
import test from "node:test";
import { chromium } from "playwright";
import { androidWebViewAgent, startHostedUpdateFixture } from "./hosted-updates-fixture.mjs";

// prefer the repository's optional host browser while supporting the ci image
function chromiumPath() {
  // honor a specifically selected verification binary
  if (process.env.WEATHER_CHROMIUM_EXECUTABLE) {
    return process.env.WEATHER_CHROMIUM_EXECUTABLE;
  }
  const root = join(process.env.HOME ?? "", ".cache/ms-playwright");
  // leave container-managed browser selection to playwright
  if (!existsSync(root)) {
    return undefined;
  }
  return readdirSync(root).sort().reverse().map(
    // locate supported chromium installations
    (name) => join(root, name, "chrome-linux64/chrome"),
  ).find(
    // ignore other browser caches
    (path) => existsSync(path),
  );
}

// migrate a retained legacy registration and an already-staged new release
test("hosted OTA aligns the legacy worker URL with the current document", { timeout: 60_000 }, async () => {
  const fixture = await startHostedUpdateFixture({ legacy: true });
  const browser = await chromium.launch({ headless: true, executablePath: chromiumPath() });
  const page = await browser.newPage({ userAgent: androidWebViewAgent });
  try {
    await page.goto(`${fixture.origin}/forecast`);
    await page.waitForFunction(() => navigator.serviceWorker.controller !== null);
    await page.getByRole("button", { name: "Use Celsius" }).click();
    fixture.update("2026.10.11-2");
    await check(page);
    await waitForAsyncCondition(page, async () => (await navigator.serviceWorker.getRegistration()).waiting?.state === "installed");
    assert.equal(await page.locator("h1").textContent(), "Release 2026.10.11-1");
    // reopening the hosted page loads the new updater and migrates the stable url
    await page.reload();
    await waitForAsyncCondition(page, async () => navigator.serviceWorker.controller?.scriptURL.endsWith("/service-worker.js") && (await navigator.serviceWorker.getRegistration()).waiting === null);
    await waitForAsyncCondition(page, async () => !(await caches.keys()).includes("ballydidean-weather-shell-2026.10.11-1"));
    assert.equal(await page.locator("h1").textContent(), "Release 2026.10.11-2");
    assert.equal(await page.locator("#unit").textContent(), "Celsius");
    assert.equal(await page.locator(".hosted-update-prompt").count(), 0);
    assert.equal(await page.evaluate(async () => (await caches.keys()).includes("ballydidean-weather-shell-2026.10.11-1")), false);
  } finally {
    await browser.close();
    await fixture.close();
  }
});

// resolve asynchronous browser predicates before judging readiness
async function waitForAsyncCondition(page, predicate) {
  const deadline = Date.now() + 15_000;
  // retry only this disposable test's asynchronous lifecycle observation
  while (Date.now() < deadline) {
    try {
      // require the resolved boolean rather than a truthy promise object
      if (await page.evaluate(predicate)) {
        return;
      }
    } catch (error) {
      // allow only the expected context replacement during update reload
      if (!/Execution context was destroyed|Cannot find context with specified id/u.test(error.message)) {
        throw error;
      }
    }
    await page.waitForTimeout(50);
  }
  throw new Error("Hosted worker did not reach the expected lifecycle state");
}

// trigger an actual worker byte comparison without navigating away
async function check(page) {
  await page.evaluate(async () => (await navigator.serviceWorker.getRegistration()).update());
}

// exercise stage failure retry explicit apply and browser autoapply
for (const native of [true, false]) {
  test(`hosted OTA ${native ? "Android stages verifies retries and applies explicitly" : "browser preserves automatic updates"}`, { timeout: 90_000 }, async () => {
    const fixture = await startHostedUpdateFixture();
    const browser = await chromium.launch({ headless: true, executablePath: chromiumPath() });
    const context = await browser.newContext({ viewport: { width: 390, height: 844 }, ...(native ? { userAgent: androidWebViewAgent } : {}) });
    const page = await context.newPage();
    const errors = [];
    // collect only actual application bootstrap failures
    page.on("pageerror", (error) => errors.push(error.message));
    try {
      await page.goto(`${fixture.origin}/forecast`);
      await page.waitForFunction(() => navigator.serviceWorker.controller !== null);
      await page.getByRole("button", { name: "Use Celsius" }).click();
      // prove checksum failures preserve the usable current release
      if (native) {
        fixture.update("2026.10.11-2", { corruptAsset: true });
        await check(page);
        await page.getByRole("button", { name: "Retry download" }).waitFor();
        assert.equal(await page.locator("h1").textContent(), "Release 2026.10.11-1");
        assert.equal(await page.evaluate(async () => (await caches.keys()).includes("ballydidean-weather-shell-2026.10.11-1")), true);
        fixture.update("2026.10.11-2", { pause: true });
        await page.getByRole("button", { name: "Retry download" }).click();
      } else {
        fixture.update("2026.10.11-2", { pause: true });
        await check(page);
      }
      // hold a real download before declaring the release ready
      if (native) {
        await page.getByText("Downloading app update…", { exact: true }).waitFor();
        await page.waitForFunction(() => document.querySelector("progress")?.value > 0);
        assert.equal(await page.getByRole("button", { name: "Reload to apply" }).count(), 0);
      }
      fixture.finishDownload();
      // defer activation until the Android user chooses reload
      if (native) {
        await page.getByRole("button", { name: "Reload to apply" }).waitFor();
        await page.waitForTimeout(300);
        assert.equal(await page.locator("h1").textContent(), "Release 2026.10.11-1");
        assert.equal(await page.evaluate(async () => (await navigator.serviceWorker.getRegistration()).waiting !== null), true);
        assert.equal(await page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth), 0);
        await page.getByRole("button", { name: "Reload to apply" }).click();
      }
      await page.waitForFunction(() => document.body.dataset.booted === "2026.10.11-2");
      assert.equal(await page.locator("#unit").textContent(), "Celsius");
      assert.equal(await page.locator(".hosted-update-prompt").count(), 0);
      assert.deepEqual(errors, []);
      assert.equal(await page.evaluate(async () => (await caches.keys()).includes("ballydidean-weather-shell-2026.10.11-1")), false);
      // verify the applied complete shell still boots with transport and http cache unavailable
      const devtools = await context.newCDPSession(page);
      await devtools.send("Network.setCacheDisabled", { cacheDisabled: true });
      await context.setOffline(true);
      await page.reload();
      await page.waitForFunction(() => document.body.dataset.booted === "2026.10.11-2");
      assert.equal(await page.locator("#unit").textContent(), "Celsius");
      assert.deepEqual(errors, []);
    } finally {
      await browser.close();
      await fixture.close();
    }
  });
}
