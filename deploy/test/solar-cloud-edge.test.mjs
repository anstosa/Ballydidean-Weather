import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { readFile } from "node:fs/promises";
import { createServer } from "node:http";
import { resolve } from "node:path";
import test from "node:test";

const repoRoot = resolve(import.meta.dirname, "../..");
const site = { slug: "ballydidean", name: "Ballydidean", latitude: 47.950429954185445, longitude: -122.42797012608193, stations: [], timezone: "America/Los_Angeles" };

// isolate the real edge from production and keep its core archive fully controllable
async function startEdge(context, archive) {
  const api = createServer(archive);
  api.listen(0, "127.0.0.1");
  await once(api, "listening");
  const reservation = createServer();
  reservation.listen(0, "127.0.0.1");
  await once(reservation, "listening");
  const port = reservation.address().port;
  reservation.close();
  await once(reservation, "close");
  const edge = spawn(process.execPath, [resolve(repoRoot, "deploy/scripts/web-server.mjs")], {
    cwd: repoRoot, stdio: ["ignore", "ignore", "pipe"],
    env: { ...process.env, NODE_ENV: "test", PORT: String(port), WEATHER_API_ORIGIN: `http://127.0.0.1:${api.address().port}`, WEATHER_RELEASE: "development", WEATHER_ADJUSTMENT_SCHEDULED_CAPTURE_ENABLED: "false" },
  });
  const exited = once(edge, "exit");
  let diagnostics = "";
  edge.stderr.on("data", (chunk) => {
    // retain disposable startup failures without touching live service logs
    diagnostics += String(chunk);
  });
  context.after(async () => {
    // terminate only this test's disposable process and listener
    if (edge.exitCode === null) {
      edge.kill("SIGTERM");
    }
    await exited;
    api.close();
    await once(api, "close");
  });
  const origin = `http://127.0.0.1:${port}`;
  // startup retries are bounded and never retry an application endpoint verdict
  for (let attempt = 0; attempt < 50; attempt += 1) {
    try {
      await fetch(`${origin}/healthz`);
      return origin;
    } catch {
      await new Promise((resolveWait) => setTimeout(resolveWait, 50));
    }
  }
  throw new Error(`edge startup failed: ${diagnostics}`);
}

// both compact read products share one completed scan but retain distinct public envelopes
test("solar cloud endpoint shares paging and preserves the recorded envelope and query guards", async (context) => {
  const requests = [];
  const origin = await startEdge(context, (request, response) => {
    // serve only the existing fixed core archive contract
    requests.push(new URL(request.url, "http://api"));
    response.setHeader("Content-Type", "application/json");
    response.end(JSON.stringify({ site, data: [], page: { limit: 250, nextCursor: null } }));
  });
  // the new transitive browser module must be served through the closed immutable asset map
  const asset = await fetch(`${origin}/assets/development/solar-cloud-evidence.js`);
  assert.equal(asset.status, 200);
  assert.equal(asset.headers.get("content-type"), "text/javascript; charset=utf-8");
  assert.match(asset.headers.get("cache-control"), /immutable/u);
  assert.equal(await asset.text(), await readFile(resolve(repoRoot, "apps/web/dist/solar-cloud-evidence.js"), "utf8"));
  const path = "/api/v1/sites/ballydidean/solar-cloud-evidence";
  assert.equal((await fetch(`${origin}${path}?from=unbounded`)).status, 400);
  const mutation = await fetch(`${origin}${path}`, { method: "POST" });
  assert.equal(mutation.status, 405);
  assert.equal(mutation.headers.get("allow"), "GET, HEAD");
  assert.equal(requests.length, 0);
  const [evidenceResponse, recordedResponse] = await Promise.all([fetch(`${origin}${path}`), fetch(`${origin}/api/v1/sites/ballydidean/forecast-observations`)]);
  assert.equal(evidenceResponse.status, 200);
  assert.equal(recordedResponse.status, 200);
  const evidence = await evidenceResponse.json();
  const recorded = await recordedResponse.json();
  assert.deepEqual(Object.keys(evidence).sort(), ["contractVersion", "generatedAt", "evidence"].sort());
  assert.equal(evidence.contractVersion, "solar-cloud-evidence/v1");
  assert.equal(evidence.evidence, null);
  assert.deepEqual(Object.keys(recorded).sort(), ["generatedAt", "data"].sort());
  assert.equal(evidence.generatedAt, recorded.generatedAt);
  assert.equal(requests.length, 2);
  assert.equal(evidenceResponse.headers.get("cache-control"), "public, max-age=60, s-maxage=60");
  assert.equal(evidenceResponse.headers.get("x-content-type-options"), "nosniff");
  const head = await fetch(`${origin}${path}`, { method: "HEAD" });
  assert.equal(head.status, 200);
  assert.equal((await head.arrayBuffer()).byteLength, 0);
  assert.equal(head.headers.get("content-length"), evidenceResponse.headers.get("content-length"));
  assert.equal(requests.length, 2);
  // every upstream read remains fixed fully paged and queryless to the viewer
  for (const url of requests) {
    assert.equal(url.pathname, "/api/v1/sites/ballydidean/history");
    assert.equal(url.searchParams.get("limit"), "250");
    const span = Date.parse(url.searchParams.get("to")) - Date.parse(url.searchParams.get("from"));
    assert.ok(span >= 24 * 3_600_000 + 20 * 60_000 && span < 29 * 3_600_000);
  }
});

// optional endpoint failures remain honest and reuse the same bounded retry cooldown
test("solar cloud archive failure returns no-store retry-after without partial evidence", async (context) => {
  let calls = 0;
  const origin = await startEdge(context, (_request, response) => {
    // refuse the entire upstream history envelope before any compact publication
    calls += 1;
    response.writeHead(503);
    response.end("unavailable");
  });
  const response = await fetch(`${origin}/api/v1/sites/ballydidean/solar-cloud-evidence`);
  assert.equal(response.status, 503);
  assert.equal(response.headers.get("retry-after"), "30");
  assert.equal(response.headers.get("cache-control"), "no-store");
  assert.doesNotMatch(await response.text(), /recordId|biasPercentPoints/u);
  const before = calls;
  assert.equal((await fetch(`${origin}/api/v1/sites/ballydidean/forecast-observations`)).status, 503);
  assert.equal(calls, before);
});
