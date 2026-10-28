import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import test from "node:test";

import {
  projectWidgetForecast,
  WIDGET_FORECAST_MAX_BYTES,
} from "../../apps/web/dist/widget-forecast.js";
import {
  projectWidgetForecastV2,
  WIDGET_FORECAST_V2_SCHEMA_VERSION,
} from "../../apps/web/dist/widget-forecast-v2.js";
import {
  projectWidgetForecastV3,
  WIDGET_FORECAST_V3_SCHEMA_VERSION,
} from "../../apps/web/dist/widget-forecast-v3.js";
import { createForecastFixture } from "../../scripts/widget-fixtures.mjs";
import { freezeAdjustmentEvidenceSnapshot } from "../scripts/adjustment-evidence-store.mjs";

const repoRoot = resolve(import.meta.dirname, "../..");
const widgetPath = "/api/v1/sites/ballydidean/widget-forecast";
const widgetV2Path = "/api/v2/sites/ballydidean/widget-forecast";
const widgetV3Path = "/api/v3/sites/ballydidean/widget-forecast";
const forecastPath = "/api/v1/sites/ballydidean/forecast?days=1";
const overnightForecastPath = "/api/v1/sites/ballydidean/forecast?window=overnight";
const maximumApiBytes = 1024 * 1024;
const enabledSettings = Object.freeze({
  rain: true,
  temperature: true,
  version: 1,
  wind: true,
});
const disabledSettings = Object.freeze({
  rain: false,
  temperature: false,
  version: 1,
  wind: false,
});

// reserve one disposable loopback port
async function reservePort() {
  const listener = createServer();
  listener.listen(0, "127.0.0.1");
  await once(listener, "listening");
  const address = listener.address();

  // require one tcp listener
  if (address === null || typeof address === "string") {
    throw new Error("failed to reserve a tcp port");
  }

  listener.close();
  await once(listener, "close");
  return address.port;
}

// wait for one bounded local startup
async function waitForServer(url) {
  // retry only the disposable edge process
  for (let attempt = 0; attempt < 100; attempt += 1) {
    try {
      const response = await fetch(url);

      // accept one complete http response
      if (response.status > 0) {
        return;
      }
    } catch {
      await new Promise(
        // bound one startup retry delay
        (resolveWait) => setTimeout(resolveWait, 25),
      );
    }
  }

  throw new Error(`server did not start: ${url}`);
}

// close one disposable listener
async function closeServer(server) {
  await new Promise((resolveClose, rejectClose) => {
    server.close((error) => {
      // surface unexpected close failures
      if (error !== undefined) {
        rejectClose(error);
      } else {
        resolveClose();
      }
    });
  });
}

// stop one disposable child process
async function stopProcess(child) {
  // preserve an already completed process
  if (child.exitCode !== null || child.signalCode !== null) {
    return;
  }

  child.kill("SIGTERM");
  await once(child, "exit");
}

// build one exact-size allowlisted upstream body
function paddedForecastBody(forecast) {
  const empty = JSON.stringify({ ...forecast, edgePadding: "" });
  const paddingLength = maximumApiBytes - Buffer.byteLength(empty);

  // require room for one deterministic padding field
  if (paddingLength < 0) {
    throw new Error("forecast fixture already exceeds the api limit");
  }

  const body = Buffer.from(JSON.stringify({
    ...forecast,
    edgePadding: "x".repeat(paddingLength),
  }));
  assert.equal(body.byteLength, maximumApiBytes);
  return body;
}

// require one head error to retain get metadata without a body
async function assertHeadErrorParity(url, status, body) {
  const getResponse = await fetch(url);
  const getBody = await getResponse.text();
  const headResponse = await fetch(url, { method: "HEAD" });
  const headBody = Buffer.from(await headResponse.arrayBuffer());
  assert.equal(getResponse.status, status);
  assert.equal(headResponse.status, status);
  assert.equal(getBody, body);
  assert.equal(Number(getResponse.headers.get("content-length")), Buffer.byteLength(body));
  assert.equal(headResponse.headers.get("content-length"), getResponse.headers.get("content-length"));
  assert.equal(headResponse.headers.get("cache-control"), "no-store");
  assert.equal(headResponse.headers.get("cross-origin-resource-policy"), "same-origin");
  assert.equal(headResponse.headers.get("x-content-type-options"), "nosniff");
  assert.equal(headBody.byteLength, 0);
}

// exercise the real router with deterministic upstream and settings state
test("widget forecast edge preserves filtering and rejects untrusted inputs", {
  timeout: 30_000,
}, async () => {
  const fixtureRoot = await mkdtemp(join(tmpdir(), "weather-widget-edge-"));
  const stateRoot = join(fixtureRoot, "state");
  const settingsPath = join(stateRoot, "forecast-adjustment-settings.json");
  const markerPath = `${settingsPath}.initialized`;
  const evidenceRoot = join(fixtureRoot, "adjustment-evidence");
  const layoutPath = join(stateRoot, "property-sensor-layout.json");
  const apiPort = await reservePort();
  const edgePort = await reservePort();
  const fixture = createForecastFixture({
    generic: true,
    rain: true,
    // retain real condition values through the v2 edge projection
    row(index, record) {
      return {
        ...record,
        metrics: {
          ...record.metrics,
          cloudCoverPercent: 30 + index,
          windSpeedMps: 4 + index / 10,
        },
      };
    },
    temperature: true,
  });
  const normalBody = Buffer.from(JSON.stringify(fixture.input));
  const maximumBody = paddedForecastBody(fixture.input);
  const requests = [];
  let mode = "normal";
  let replaceSettings = false;
  await mkdir(stateRoot, { recursive: true });

  // persist one complete settings snapshot
  async function writeSettings(settings) {
    await writeFile(settingsPath, `${JSON.stringify(settings)}\n`);
  }

  await writeSettings(enabledSettings);
  await writeFile(markerPath, "forecast-adjustment-settings/v1\n");

  // emulate one deterministic internal forecast service
  const upstream = createServer(async (request, response) => {
    const path = request.url ?? "/";
    requests.push({ headers: request.headers, method: request.method, path });

    // retain one hanging request until the edge aborts it
    if (mode === "timeout") {
      return;
    }

    // expose one redirect target without allowing it to be followed
    if (mode === "redirect") {
      response.writeHead(302, { Location: "/redirected" });
      response.end("redirect-secret\n");
      return;
    }

    // expose one upstream error without leaking its body
    if (mode === "error") {
      response.writeHead(503, { "Content-Type": "text/plain" });
      response.end("upstream-error-secret\n");
      return;
    }

    // expose malformed json at the trusted route
    if (mode === "malformed") {
      response.writeHead(200, { "Content-Type": "application/json" });
      response.end("{invalid json\n");
      return;
    }

    // reject any unexpected fixed-route drift
    if (path !== forecastPath && !path.startsWith("/api/v1/sites/ballydidean/forecast?")) {
      response.writeHead(404, { "Content-Type": "text/plain" });
      response.end("internal route not found\n");
      return;
    }

    // change persistence only after the edge captured its request snapshot
    if (replaceSettings) {
      replaceSettings = false;
      await writeSettings(disabledSettings);
    }

    const body = mode === "maximum"
      ? maximumBody
      : mode === "oversized"
        ? Buffer.alloc(maximumApiBytes + 1, "x")
        : normalBody;
    response.writeHead(200, {
      "Content-Length": String(body.byteLength),
      "Content-Type": "application/json",
    });
    response.end(body);
  });
  upstream.listen(apiPort, "127.0.0.1");
  await once(upstream, "listening");

  const edge = spawn(process.execPath, [join(repoRoot, "deploy/scripts/web-server.mjs")], {
    cwd: repoRoot,
    env: {
      ...process.env,
      PORT: String(edgePort),
      WEATHER_ADMIN_AUTH_PATH: join(stateRoot, "admin-auth.json"),
      WEATHER_ADMIN_BOOTSTRAP_TOKEN_PATH: join(stateRoot, "admin-bootstrap-token"),
      WEATHER_ADJUSTMENT_EVIDENCE_ROOT: evidenceRoot,
      WEATHER_API_ORIGIN: `http://127.0.0.1:${apiPort}`,
      WEATHER_PROPERTY_SENSOR_LAYOUT_PATH: layoutPath,
      WEATHER_RELEASE: "2026.09.20-1",
    },
    stdio: ["ignore", "ignore", "pipe"],
  });
  const diagnostics = [];
  edge.stderr.on(
    "data",
    // retain bounded startup diagnostics for failures
    (chunk) => diagnostics.push(String(chunk)),
  );
  const edgeOrigin = `http://127.0.0.1:${edgePort}`;

  try {
    await waitForServer(`${edgeOrigin}/`);

    // compare all switch combinations through the existing browser resolver
    for (let mask = 0; mask < 8; mask += 1) {
      const settings = {
        rain: Boolean(mask & 4),
        temperature: Boolean(mask & 1),
        version: 1,
        wind: Boolean(mask & 2),
      };
      await writeSettings(settings);
      const browserResponse = await fetch(`${edgeOrigin}${forecastPath}`);
      const browserForecast = await browserResponse.json();
      const widgetResponse = await fetch(`${edgeOrigin}${widgetPath}`);
      const widget = await widgetResponse.json();
      const expected = projectWidgetForecast(browserForecast, widget.receivedAt);
      const widgetV2Response = await fetch(`${edgeOrigin}${widgetV2Path}`);
      const widgetV2 = await widgetV2Response.json();
      const expectedV2 = projectWidgetForecastV2(browserForecast, widgetV2.receivedAt);
      const widgetV3Response = await fetch(`${edgeOrigin}${widgetV3Path}`);
      const widgetV3 = await widgetV3Response.json();
      const expectedV3 = projectWidgetForecastV3(browserForecast, widgetV3.receivedAt);
      assert.equal(browserResponse.status, 200);
      assert.equal(widgetResponse.status, 200);
      assert.deepEqual(widget, expected);
      assert.equal(widgetV2Response.status, 200);
      assert.deepEqual(widgetV2, expectedV2);
      assert.equal(widgetV2.schemaVersion, WIDGET_FORECAST_V2_SCHEMA_VERSION);
      assert.equal(widgetV3Response.status, 200);
      assert.deepEqual(widgetV3, expectedV3);
      assert.equal(widgetV3.schemaVersion, WIDGET_FORECAST_V3_SCHEMA_VERSION);
      assert.equal(widgetV3.hours.at(-1).end, widgetV3.calendar.overnightEnd);
      assert.ok(widgetV2.hours.every(
        // retain real condition metrics without derived defaults
        (hour) => hour.cloudCoverPercent.mode === "raw" &&
          hour.windSpeedMps.mode === "raw",
      ));
      assert.equal(
        widget.hours[0].temperatureC.mode,
        settings.temperature ? "adjusted" : "raw",
      );
      assert.equal(widget.hours[0].rainMmPerHour.mode, settings.rain ? "adjusted" : "raw");
    }

    const forecastGetResponse = await fetch(`${edgeOrigin}${forecastPath}`);
    const forecastGetBody = Buffer.from(await forecastGetResponse.arrayBuffer());
    const forecastHeadResponse = await fetch(`${edgeOrigin}${forecastPath}`, {
      method: "HEAD",
    });
    const forecastHeadBody = Buffer.from(await forecastHeadResponse.arrayBuffer());
    assert.equal(forecastGetResponse.status, 200);
    assert.equal(forecastHeadResponse.status, 200);
    assert.equal(forecastGetBody.at(-1), 10);
    assert.equal(
      forecastHeadResponse.headers.get("content-length"),
      forecastGetResponse.headers.get("content-length"),
    );
    assert.equal(forecastHeadBody.byteLength, 0);
    await new Promise(
      // allow the response-finish evidence queue to settle
      (resolveWait) => setTimeout(resolveWait, 100),
    );
    assert.equal((await freezeAdjustmentEvidenceSnapshot({ root: evidenceRoot })).entries.length, 8);

    await writeSettings(enabledSettings);
    const requestStart = requests.length;
    const getResponse = await fetch(`${edgeOrigin}${widgetPath}`, {
      headers: {
        Authorization: "Bearer private-edge-token",
        Cookie: "private_cookie=edge-secret",
        Origin: "https://foreign.example",
      },
    });
    const getBody = Buffer.from(await getResponse.arrayBuffer());
    const headResponse = await fetch(`${edgeOrigin}${widgetPath}`, {
      headers: {
        Authorization: "Bearer private-head-token",
        Cookie: "private_cookie=head-secret",
      },
      method: "HEAD",
    });
    const headBody = Buffer.from(await headResponse.arrayBuffer());
    const widgetRequests = requests.slice(requestStart);
    assert.equal(getResponse.status, 200);
    assert.equal(headResponse.status, 200);
    assert.equal(getResponse.headers.get("content-type"), "application/json; charset=utf-8");
    assert.equal(getResponse.headers.get("cache-control"), "no-store");
    assert.equal(getResponse.headers.get("cross-origin-resource-policy"), "same-origin");
    assert.equal(getResponse.headers.get("referrer-policy"), "strict-origin-when-cross-origin");
    assert.equal(getResponse.headers.get("x-content-type-options"), "nosniff");
    assert.match(getResponse.headers.get("content-security-policy"), /default-src 'self'/u);
    assert.equal(getResponse.headers.get("access-control-allow-origin"), null);
    assert.equal(Number(getResponse.headers.get("content-length")), getBody.byteLength);
    assert.equal(Number(headResponse.headers.get("content-length")), getBody.byteLength);
    assert.equal(headBody.byteLength, 0);
    assert.ok(getBody.byteLength <= WIDGET_FORECAST_MAX_BYTES);
    assert.equal(widgetRequests.length, 2);

    // require fixed credential-free get requests upstream
    for (const upstreamRequest of widgetRequests) {
      assert.equal(upstreamRequest.method, "GET");
      assert.equal(upstreamRequest.path, forecastPath);
      assert.equal(upstreamRequest.headers.accept, "application/json");
      assert.equal(upstreamRequest.headers.authorization, undefined);
      assert.equal(upstreamRequest.headers.cookie, undefined);
      assert.equal(upstreamRequest.headers.origin, undefined);
    }

    const v2Response = await fetch(`${edgeOrigin}${widgetV2Path}`);
    const v2 = await v2Response.json();
    assert.equal(v2Response.status, 200);
    assert.equal(v2Response.headers.get("cache-control"), "no-store");
    assert.equal(v2Response.headers.get("access-control-allow-origin"), null);
    assert.equal(v2.schemaVersion, WIDGET_FORECAST_V2_SCHEMA_VERSION);
    assert.ok(Buffer.byteLength(JSON.stringify(v2)) <= WIDGET_FORECAST_MAX_BYTES);

    const v3RequestStart = requests.length;
    const v3GetResponse = await fetch(`${edgeOrigin}${widgetV3Path}`, {
      headers: {
        Authorization: "Bearer private-v3-token",
        Cookie: "private_cookie=v3-secret",
        Origin: "https://foreign.example",
      },
    });
    const v3GetBody = Buffer.from(await v3GetResponse.arrayBuffer());
    const v3HeadResponse = await fetch(`${edgeOrigin}${widgetV3Path}`, {
      headers: {
        Authorization: "Bearer private-v3-head-token",
        Cookie: "private_cookie=v3-head-secret",
      },
      method: "HEAD",
    });
    const v3HeadBody = Buffer.from(await v3HeadResponse.arrayBuffer());
    const v3Requests = requests.slice(v3RequestStart);
    assert.equal(v3GetResponse.status, 200);
    assert.equal(v3HeadResponse.status, 200);
    assert.equal(v3GetResponse.headers.get("cache-control"), "no-store");
    assert.equal(v3GetResponse.headers.get("access-control-allow-origin"), null);
    assert.equal(
      Number(v3HeadResponse.headers.get("content-length")),
      v3GetBody.byteLength,
    );
    assert.equal(v3HeadBody.byteLength, 0);
    assert.ok(v3GetBody.byteLength <= WIDGET_FORECAST_MAX_BYTES);
    assert.equal(v3Requests.length, 2);

    // require the fixed overnight query without forwarding caller credentials
    for (const upstreamRequest of v3Requests) {
      assert.equal(upstreamRequest.method, "GET");
      assert.equal(upstreamRequest.path, overnightForecastPath);
      assert.equal(upstreamRequest.headers.accept, "application/json");
      assert.equal(upstreamRequest.headers.authorization, undefined);
      assert.equal(upstreamRequest.headers.cookie, undefined);
      assert.equal(upstreamRequest.headers.origin, undefined);
    }

    const serialized = `${getBody.toString("utf8")}${v3GetBody.toString("utf8")}`;

    // reject private adjustment and provider internals from public bytes
    for (const forbidden of [
      "activeBundle",
      "authorizationSha256",
      "bundleSha256",
      "candidateArtifactSha256",
      "edgePadding",
      "sourceForecast",
      "stationSlug",
    ]) {
      assert.equal(serialized.includes(forbidden), false, forbidden);
    }

    const beforeRejected = requests.length;

    // reject every mutation with one exact allow header
    for (const endpoint of [widgetPath, widgetV2Path, widgetV3Path]) {
      // keep every versioned projection read-only
      for (const method of ["POST", "PUT", "PATCH", "DELETE", "OPTIONS"]) {
        const response = await fetch(`${edgeOrigin}${endpoint}`, { method });
        assert.equal(response.status, 405);
        assert.equal(response.headers.get("allow"), "GET, HEAD");
      }
    }

    // reject every caller-controlled projection query
    for (const endpoint of [widgetPath, widgetV2Path, widgetV3Path]) {
      // keep every versioned projection independent of caller input
      for (const query of ["?date=2026-09-12", "?origin=https://evil.example", "?site=other"]) {
        const response = await fetch(`${edgeOrigin}${endpoint}${query}`);
        assert.equal(response.status, 400);
      }
    }

    await assertHeadErrorParity(
      `${edgeOrigin}${widgetPath}?site=other`,
      400,
      "bad request\n",
    );
    await assertHeadErrorParity(
      `${edgeOrigin}${widgetV2Path}?site=other`,
      400,
      "bad request\n",
    );
    await assertHeadErrorParity(
      `${edgeOrigin}${widgetV3Path}?site=other`,
      400,
      "bad request\n",
    );

    // reject near-match site and path variants before the general proxy
    for (const path of [
      "/api/v1/sites/other/widget-forecast",
      `${widgetPath}/`,
      `${widgetPath}/private`,
      "/api/v2/sites/other/widget-forecast",
      `${widgetV2Path}/`,
      `${widgetV2Path}/private`,
      "/api/v3/sites/other/widget-forecast",
      `${widgetV3Path}/`,
      `${widgetV3Path}/private`,
    ]) {
      const response = await fetch(`${edgeOrigin}${path}`);
      assert.equal(response.status, 404);
    }

    assert.equal(requests.length, beforeRejected);

    // preserve existing forecast and public admin routes unchanged
    const ordinaryForecast = await fetch(
      `${edgeOrigin}/api/v1/sites/ballydidean/forecast?days=3`,
    );
    const ordinaryForecastBody = await ordinaryForecast.json();
    const publicSettings = await fetch(
      `${edgeOrigin}/api/v1/sites/ballydidean/forecast-adjustment-settings`,
    );
    assert.equal(ordinaryForecast.status, 200);
    assert.deepEqual(ordinaryForecastBody.adjustmentSettings, enabledSettings);
    assert.equal(requests.at(-1).path, "/api/v1/sites/ballydidean/forecast?days=3");
    assert.deepEqual(await publicSettings.json(), { data: enabledSettings });

    // preserve one settings snapshot even when persistence changes mid-request
    await writeSettings(enabledSettings);
    replaceSettings = true;
    const captured = await (await fetch(`${edgeOrigin}${widgetPath}`)).json();
    assert.equal(captured.status, "adjusted");
    const nextSnapshot = await (await fetch(`${edgeOrigin}${widgetPath}`)).json();
    assert.equal(nextSnapshot.status, "raw");

    // fail closed when the persisted switch snapshot is corrupt
    await writeFile(settingsPath, "{invalid json\n");
    const corruptSettingsResponse = await fetch(`${edgeOrigin}${widgetPath}`);
    const corruptSettings = await corruptSettingsResponse.json();
    const corruptBrowserResponse = await fetch(`${edgeOrigin}${forecastPath}`);
    const corruptBrowser = await corruptBrowserResponse.json();
    assert.equal(corruptSettings.status, "raw");
    assert.deepEqual(
      corruptSettings,
      projectWidgetForecast(corruptBrowser, corruptSettings.receivedAt),
    );
    assert.ok(corruptBrowser.data.every(
      // require the browser response to disable every correction family
      (record) => record.adjustment.state === "disabled" &&
        record.temperatureAdjustment.state === "disabled" &&
        record.rainAdjustment.state === "disabled",
    ));
    assert.ok(corruptSettings.hours.every(
      // require both projected fields to use raw values
      (hour) => hour.temperatureC.mode === "raw" && hour.rainMmPerHour.mode === "raw",
    ));

    // fail closed when the settings path cannot contain readable state
    await rm(settingsPath);
    await mkdir(settingsPath);
    const unreadableSettingsResponse = await fetch(`${edgeOrigin}${widgetPath}`);
    const unreadableSettings = await unreadableSettingsResponse.json();
    const unreadableBrowser = await (await fetch(`${edgeOrigin}${forecastPath}`)).json();
    assert.equal(unreadableSettings.status, "raw");
    assert.deepEqual(
      unreadableSettings,
      projectWidgetForecast(unreadableBrowser, unreadableSettings.receivedAt),
    );
    await rm(settingsPath, { recursive: true });
    await writeSettings(enabledSettings);

    // accept exactly one mebibyte before applying the smaller public cap
    mode = "maximum";
    const maximumResponse = await fetch(`${edgeOrigin}${widgetPath}`);
    const maximumOutput = Buffer.from(await maximumResponse.arrayBuffer());
    assert.equal(maximumResponse.status, 200);
    assert.ok(maximumOutput.byteLength <= WIDGET_FORECAST_MAX_BYTES);
    assert.equal(maximumOutput.toString("utf8").includes("edgePadding"), false);

    // reject one byte beyond the upstream input ceiling
    mode = "oversized";
    const oversized = await fetch(`${edgeOrigin}${widgetPath}`);
    assert.equal(oversized.status, 502);
    assert.equal(await oversized.text(), "upstream unavailable\n");

    // reject malformed upstream json without reflecting it
    mode = "malformed";
    const malformed = await fetch(`${edgeOrigin}${widgetPath}`);
    assert.equal(malformed.status, 502);
    assert.equal(await malformed.text(), "upstream unavailable\n");

    // reject upstream errors without reflecting their private body
    mode = "error";
    await assertHeadErrorParity(
      `${edgeOrigin}${widgetPath}`,
      502,
      "upstream unavailable\n",
    );

    // reject redirects without following their internal location
    mode = "redirect";
    const redirectStart = requests.length;
    const redirect = await fetch(`${edgeOrigin}${widgetPath}`);
    assert.equal(redirect.status, 502);
    assert.equal(await redirect.text(), "upstream unavailable\n");
    assert.deepEqual(
      requests.slice(redirectStart).map((request) => request.path),
      [forecastPath],
    );

    // abort one slow upstream at the existing five-second boundary
    mode = "timeout";
    const timeoutStartedAt = Date.now();
    const timeout = await fetch(`${edgeOrigin}${widgetPath}`);
    const timeoutElapsed = Date.now() - timeoutStartedAt;
    assert.equal(timeout.status, 502);
    assert.equal(await timeout.text(), "upstream unavailable\n");
    assert.ok(timeoutElapsed >= 4_500 && timeoutElapsed < 8_000, String(timeoutElapsed));
  } catch (error) {
    throw new Error(`${error.stack ?? error}\nedge diagnostics:\n${diagnostics.join("")}`, {
      cause: error,
    });
  } finally {
    await stopProcess(edge);
    await closeServer(upstream);
    await rm(fixtureRoot, { force: true, recursive: true });
  }
});
