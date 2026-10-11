import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import vm from "node:vm";

const origin = "https://weather.example";
const analyticsPolicy =
  "default-src 'self'; connect-src 'self' https://www.googletagmanager.com https://*.google-analytics.com https://*.google.com; img-src 'self' data: https://www.googletagmanager.com https://*.google-analytics.com; script-src 'self' https://www.googletagmanager.com; style-src 'self'";
const offlinePolicy =
  "default-src 'self'; connect-src 'self'; img-src 'self' data:; script-src 'self'; style-src 'self'";
const productionShell = '<!doctype html><html data-weather-analytics="true"><head><meta name="weather-release" content="test-release"></head><body>weather</body></html>';
const offlineShell = '<!doctype html><html data-weather-analytics="false"><head><meta name="weather-release" content="test-release"></head><body>weather</body></html>';

// normalize one cache key
function cacheKey(input) {
  const request =
    input instanceof Request || typeof input?.url === "string"
      ? input
      : new Request(new URL(input, origin));
  return request.url;
}

// build one production dashboard response
function productionResponse({ cacheControl = "public, max-age=0", status = 200 } = {}) {
  return new Response(productionShell, {
    headers: {
      "cache-control": cacheControl,
      "content-length": String(Buffer.byteLength(productionShell)),
      "content-security-policy": analyticsPolicy,
      "content-type": "text/html; charset=utf-8",
      "x-weather-test": "preserved",
    },
    status,
  });
}

// build one decoded response with encoded-transfer metadata
function encodedOfflineResponse(policy) {
  return new Response(offlineShell, {
    headers: {
      "cache-control": "public, max-age=0",
      "content-encoding": "gzip",
      "content-length": "42",
      "content-security-policy": policy,
      "content-type": "text/html; charset=utf-8",
      "x-weather-test": "preserved",
    },
    status: 203,
  });
}

// provide browser-style relative request resolution
class WorkerRequest {
  // resolve against the worker origin
  constructor(input, init) {
    const resolved = typeof input === "string" ? new URL(input, origin) : input;
    const { mode = "cors", ...supportedInit } = init ?? {};
    const request = new Request(resolved, supportedInit);
    this.credentials = request.credentials;
    this.integrity = request.integrity;
    this.headers = request.headers;
    this.method = request.method;
    this.mode = mode;
    this.url = request.url;
  }
}

// model one named cache
class MemoryCache {
  entries = new Map();
  operations = [];

  // fetch all entries before committing the precache
  async addAll(requests) {
    this.operations.push({ name: "addAll", requests: [...requests] });
    const additions = [];

    // fetch the complete shell first
    for (const request of requests) {
      additions.push([cacheKey(request), await this.fetcher(request)]);
    }

    // commit the complete shell together
    for (const [key, response] of additions) {
      this.entries.set(key, response.clone());
    }
  }

  // look up one cached response
  async match(request) {
    this.operations.push({ name: "match", request });
    return this.entries.get(cacheKey(request))?.clone();
  }

  // store one cloned response
  async put(request, response) {
    this.operations.push({ name: "put", request, response: response.clone() });
    this.entries.set(cacheKey(request), response.clone());
  }

  // attach the worker fetch implementation
  constructor(fetcher) {
    this.fetcher = fetcher;
  }
}

// load one isolated service worker
async function loadWorker({ fetcher, seededCaches = new Map(), active = null, digests }) {
  const source = await readFile(new URL("../public/service-worker.js", import.meta.url), "utf8");
  const assetDigests = digests ?? Object.fromEntries(
    [...source.matchAll(/`\/assets\/\$\{release\}\/([^`]+)`/gu)].map(
      // model the server's complete immutable asset manifest
      (match) => [`/assets/test-release/${match[1]}`, `sha256-${Buffer.alloc(32).toString("base64")}`],
    ),
  );
  const listeners = new Map();
  const cachesByName = new Map(seededCaches);
  const deletedCaches = [];
  let claimed = false;
  let skipped = false;
  const messages = [];

  // register one worker event
  function addEventListener(name, listener) {
    listeners.set(name, listener);
  }

  // open or create one cache
  async function open(name) {
    let cache = cachesByName.get(name);

    // initialize one absent cache
    if (cache === undefined) {
      cache = new MemoryCache(fetcher);
      cachesByName.set(name, cache);
    }

    return cache;
  }

  const context = vm.createContext({
    Request: WorkerRequest,
    Response,
    Headers,
    Set,
    URL,
    caches: {
      // remove one selected cache
      async delete(name) {
        deletedCaches.push(name);
        return cachesByName.delete(name);
      },
      // list available caches
      async keys() {
        return [...cachesByName.keys()];
      },
      // inspect readiness without creating an empty cache
      async has(name) {
        return cachesByName.has(name);
      },
      open,
    },
    fetch: fetcher,
    self: {
      addEventListener,
      registration: { active },
      clients: {
        // capture progress delivered to controlled and new documents
        async matchAll() {
          return [{ postMessage: (message) => messages.push(message) }];
        },
        // record client activation
        async claim() {
          claimed = true;
        },
      },
      location: { origin },
      // record immediate worker activation
      async skipWaiting() {
        skipped = true;
      },
    },
  });
  vm.runInContext(source.replaceAll("__WEATHER_ASSET_VERSION__", "test-release")
    .replaceAll("__WEATHER_SHELL_ASSET_DIGESTS__", JSON.stringify(assetDigests)), context);

  // dispatch one extendable lifecycle event
  async function dispatchLifecycle(name) {
    let pending;
    listeners.get(name)({
      // capture one lifecycle promise
      waitUntil(promise) {
        pending = promise;
      },
    });
    await pending;
  }

  // dispatch one fetch event
  async function dispatchFetch(request) {
    let responsePromise;
    listeners.get("fetch")({
      request,
      // capture an intercepted response
      respondWith(promise) {
        responsePromise = Promise.resolve(promise);
      },
    });
    return responsePromise;
  }

  return {
    cachesByName,
    deletedCaches,
    dispatchFetch,
    dispatchLifecycle,
    messages,
    // dispatch one page request to the exact worker
    async dispatchMessage(data) {
      let pending;
      const replies = [];
      listeners.get("message")({
        data,
        source: { postMessage: (message) => replies.push(message) },
        waitUntil: (promise) => { pending = promise; },
      });
      await pending;
      return replies;
    },
    // expose lifecycle state
    lifecycle() {
      return { claimed, skipped };
    },
  };
}

// collect every compiled static module reachable from one entry point
async function compiledStaticModulePaths(entry) {
  const distRoot = new URL("../dist/", import.meta.url);
  const pending = [new URL(entry, distRoot)];
  const paths = new Set();

  // traverse the compiled module graph once
  while (pending.length > 0) {
    const moduleUrl = pending.pop();
    const assetPath = `/assets/test-release/${decodeURIComponent(moduleUrl.href.slice(distRoot.href.length))}`;

    // skip modules already inspected
    if (paths.has(assetPath)) {
      continue;
    }

    paths.add(assetPath);
    const source = await readFile(moduleUrl, "utf8");

    // follow static imports and re-exports
    for (const match of source.matchAll(/(?:import|export)\s+(?:[^"']*?\s+from\s+)?["'](\.[^"']+)["']/gu)) {
      const dependency = new URL(match[1], moduleUrl);

      // keep the browser shell inside the compiled web package
      if (!dependency.href.startsWith(distRoot.href)) {
        throw new Error(`Compiled browser dependency escapes dist: ${dependency.href}`);
      }

      pending.push(dependency);
    }
  }

  return paths;
}

// assert one cached dashboard is analytics-safe
async function assertOfflineDashboard(response, { status = 200 } = {}) {
  assert.ok(response);
  assert.equal(response.status, status);
  assert.equal(await response.text(), offlineShell);
  assert.equal(response.headers.get("cache-control"), "public, max-age=0");
  assert.equal(response.headers.get("content-security-policy"), offlinePolicy);
  assert.equal(response.headers.get("content-type"), "text/html; charset=utf-8");
  assert.equal(response.headers.get("content-length"), null);
  assert.equal(response.headers.get("x-weather-test"), "preserved");
}

// verify install-time shell isolation
test("precache sanitizes dashboard shells without changing privacy or assets", async () => {
  const privacyBody = "<!doctype html><title>privacy</title>";
  const assetBody = "asset-body";

  // return route-specific precache content
  async function fetcher(request) {
    const pathname = new URL(request.url).pathname;

    // retain the static privacy document
    if (pathname === "/privacy" || pathname === "/privacy/") {
      return new Response(privacyBody, {
        headers: { "content-security-policy": offlinePolicy, "content-type": "text/html" },
      });
    }

    // return immutable assets unchanged
    if (pathname.includes(".") || pathname.startsWith("/assets/")) {
      return new Response(assetBody, { headers: { "content-type": pathname.endsWith(".js") ? "text/javascript" : "application/octet-stream" } });
    }

    return productionResponse();
  }

  const worker = await loadWorker({ fetcher });
  await worker.dispatchLifecycle("install");
  const cache = worker.cachesByName.get("ballydidean-weather-shell-test-release");
  assert.ok(cache);
  assert.equal(cache.entries.size, worker.messages.at(-1).total + 1);
  await assertOfflineDashboard(await cache.match(`${origin}/forecast`));
  await assertOfflineDashboard(await cache.match(`${origin}/`));
  assert.equal(await (await cache.match(`${origin}/privacy`)).text(), privacyBody);
  assert.equal(await (await cache.match(`${origin}/assets/test-release/client.js`)).text(), assetBody);
  assert.deepEqual(worker.lifecycle(), { claimed: false, skipped: true });
});

// verify every compiled browser dependency is available before the network disappears
test("precache contains the complete compiled static module graph", async () => {
  // return compatible shell and asset responses
  async function fetcher(request) {
    const pathname = new URL(request.url).pathname;

    // keep compiled modules distinguishable from documents
    if (pathname.startsWith("/assets/")) {
      return new Response("export {};", { headers: { "content-type": "text/javascript" } });
    }

    return productionResponse();
  }

  const worker = await loadWorker({ fetcher });
  await worker.dispatchLifecycle("install");
  const cache = worker.cachesByName.get("ballydidean-weather-shell-test-release");
  assert.ok(cache);
  const cachedPaths = new Set(
    [...cache.entries.keys()].map(
      // compare cache entries by same-origin pathname
      (url) => new URL(url).pathname,
    ),
  );
  const missingPaths = [...await compiledStaticModulePaths("client.js")].filter(
    // retain only uncached compiled dependencies
    (path) => !cachedPaths.has(path),
  );
  assert.deepEqual(missingPaths, []);
});

// verify network and offline navigation boundaries
test("navigation returns live analytics but caches only an offline-safe dashboard", async () => {
  let offline = false;

  // switch between online and offline navigation
  async function fetcher(request) {
    // emulate one failed authenticated navigation
    if (offline) {
      throw new Error("offline");
    }

    // supply valid compiled assets during installation
    if (new URL(request.url).pathname.endsWith(".js")) {
      return new Response("export {};", { headers: { "content-type": "text/javascript" } });
    }

    return productionResponse({ status: request.credentials === "include" ? 203 : 200 });
  }

  const worker = await loadWorker({ fetcher });
  await worker.dispatchLifecycle("install");
  const cache = worker.cachesByName.get("ballydidean-weather-shell-test-release");
  const request = new WorkerRequest("/forecast", { credentials: "include", mode: "navigate" });
  const onlineResponse = await worker.dispatchFetch(request);
  assert.equal(onlineResponse.status, 203);
  assert.equal(await onlineResponse.text(), productionShell);
  assert.equal(onlineResponse.headers.get("content-security-policy"), analyticsPolicy);
  assert.equal(onlineResponse.headers.get("content-length"), String(Buffer.byteLength(productionShell)));
  assert.equal(onlineResponse.headers.get("x-weather-test"), "preserved");
  await assertOfflineDashboard(await cache.match(request), { status: 203 });

  offline = true;
  await assertOfflineDashboard(await worker.dispatchFetch(request), { status: 203 });
  const missingRequest = new WorkerRequest("/trends?uncached=1", {
    credentials: "include",
    mode: "navigate",
  });
  await assertOfflineDashboard(await worker.dispatchFetch(missingRequest));
});

// verify no-op sanitation preserves encoded safe responses
test("network cache retains encoded metadata when HTML and policy are already safe", async () => {
  // return one decoded body with original transfer headers
  async function fetcher() {
    return encodedOfflineResponse(offlinePolicy);
  }

  const worker = await loadWorker({ fetcher });
  const cache = new MemoryCache(fetcher);
  worker.cachesByName.set("ballydidean-weather-shell-test-release", cache);
  const request = new WorkerRequest("/settings", { credentials: "include", mode: "navigate" });
  const online = await worker.dispatchFetch(request);
  assert.equal(await online.text(), offlineShell);
  assert.equal(online.headers.get("content-security-policy"), offlinePolicy);
  assert.equal(online.headers.get("content-encoding"), "gzip");
  assert.equal(online.headers.get("content-length"), "42");
  const cached = await cache.match(request);
  assert.equal(cached.status, 203);
  assert.equal(await cached.text(), offlineShell);
  assert.equal(cached.headers.get("content-security-policy"), offlinePolicy);
  assert.equal(cached.headers.get("content-encoding"), "gzip");
  assert.equal(cached.headers.get("content-length"), "42");
  assert.equal(cached.headers.get("x-weather-test"), "preserved");
});

// verify CSP-only sanitation removes stale encoded-transfer metadata
test("network cache strips encoded metadata when only the analytics policy changes", async () => {
  // return one decoded body with an analytics-enabled policy
  async function fetcher() {
    return encodedOfflineResponse(analyticsPolicy);
  }

  const worker = await loadWorker({ fetcher });
  const cache = new MemoryCache(fetcher);
  worker.cachesByName.set("ballydidean-weather-shell-test-release", cache);
  const request = new WorkerRequest("/settings", { credentials: "include", mode: "navigate" });
  const online = await worker.dispatchFetch(request);
  assert.equal(await online.text(), offlineShell);
  assert.equal(online.headers.get("content-security-policy"), analyticsPolicy);
  assert.equal(online.headers.get("content-encoding"), "gzip");
  assert.equal(online.headers.get("content-length"), "42");
  const cached = await cache.match(request);
  assert.equal(cached.status, 203);
  assert.equal(await cached.text(), offlineShell);
  assert.equal(cached.headers.get("content-security-policy"), offlinePolicy);
  assert.equal(cached.headers.get("content-encoding"), null);
  assert.equal(cached.headers.get("content-length"), null);
  assert.equal(cached.headers.get("x-weather-test"), "preserved");
});

// verify excluded and non-cacheable requests
test("cache boundaries preserve admin, api, privacy, and no-store behavior", async () => {
  const privacyBody = "<!doctype html><title>privacy live</title>";

  // return one privacy or no-store response
  async function fetcher(request) {
    const pathname = new URL(request.url).pathname;

    // retain privacy bytes and policy
    if (pathname === "/privacy") {
      return new Response(privacyBody, {
        headers: { "content-security-policy": offlinePolicy, "content-type": "text/html" },
      });
    }

    return productionResponse({ cacheControl: "public, no-store" });
  }

  const worker = await loadWorker({ fetcher });
  const cache = new MemoryCache(fetcher);
  worker.cachesByName.set("ballydidean-weather-shell-test-release", cache);
  const original = new Response("older-safe-shell", { headers: { "content-type": "text/html" } });
  await cache.put(`${origin}/logs`, original);
  const noStore = await worker.dispatchFetch(new WorkerRequest("/logs", { mode: "navigate" }));
  assert.equal(await noStore.text(), productionShell);
  assert.equal(await (await cache.match(`${origin}/logs`)).text(), "older-safe-shell");
  const privacy = await worker.dispatchFetch(new WorkerRequest("/privacy", { mode: "navigate" }));
  assert.equal(await privacy.text(), privacyBody);
  assert.equal(await (await cache.match(`${origin}/privacy`)).text(), privacyBody);
  assert.equal(await worker.dispatchFetch(new WorkerRequest("/admin", { mode: "navigate" })), undefined);
  assert.equal(await worker.dispatchFetch(new WorkerRequest("/api/weather")), undefined);
  assert.equal(await cache.match(`${origin}/admin`), undefined);
  assert.equal(await cache.match(`${origin}/api/weather`), undefined);
});

// verify release cache retirement remains scoped
test("activation deletes only superseded weather shell caches", async () => {
  // return an empty cache response
  async function emptyResponse() {
    return new Response();
  }

  const seededCaches = new Map([
    ["ballydidean-weather-shell-old", new MemoryCache(emptyResponse)],
    ["ballydidean-weather-shell-test-release", new MemoryCache(emptyResponse)],
    ["unrelated-cache", new MemoryCache(emptyResponse)],
  ]);
  const worker = await loadWorker({ fetcher: emptyResponse, seededCaches });
  await worker.dispatchLifecycle("activate");
  assert.deepEqual(worker.deletedCaches, ["ballydidean-weather-shell-old"]);
  assert.deepEqual([...worker.cachesByName.keys()].sort(), [
    "ballydidean-weather-shell-test-release",
    "unrelated-cache",
  ]);
  assert.deepEqual(worker.lifecycle(), { claimed: true, skipped: false });
});

// supply public release-bound shells and ordinary compiled assets
async function completeShellResponse(request) {
  // distinguish immutable modules from public documents
  if (new URL(request.url).pathname.endsWith(".js")) {
    return new Response("export {};", { headers: { "content-type": "text/javascript" } });
  }
  return productionResponse();
}

// retain the active release until a complete update is explicitly applied
test("updates report progress and wait for exact-release activation", async () => {
  const requests = [];
  const oldCache = new MemoryCache(completeShellResponse);
  const worker = await loadWorker({
    active: {},
    seededCaches: new Map([["ballydidean-weather-shell-old", oldCache]]),
    // capture the cookie-free integrity-checked download boundary
    fetcher: async (request) => {
      requests.push(request);
      return completeShellResponse(request);
    },
  });
  await worker.dispatchMessage({ type: "WEATHER_APPLY_UPDATE", release: "test-release" });
  assert.equal(worker.lifecycle().skipped, false);
  await worker.dispatchLifecycle("install");
  const ready = worker.messages.at(-1);
  assert.equal(ready.phase, "ready");
  assert.equal(ready.completed, ready.total);
  assert.ok(worker.messages.some((message) => message.completed > 0 && message.completed < message.total));
  assert.equal(worker.lifecycle().skipped, false);
  assert.equal(worker.cachesByName.get("ballydidean-weather-shell-old"), oldCache);
  assert.ok(requests.every((request) => request.credentials === "omit"));
  assert.ok(requests.filter((request) => request.url.includes("/assets/")).every((request) => request.integrity.startsWith("sha256-")));
  const [status] = await worker.dispatchMessage({ type: "WEATHER_UPDATE_STATUS" });
  assert.equal(status.phase, "ready");
  await worker.dispatchMessage({ type: "WEATHER_APPLY_UPDATE", release: "other-release" });
  assert.equal(worker.lifecycle().skipped, false);
  await worker.dispatchMessage({ type: "WEATHER_APPLY_UPDATE", release: "test-release" });
  assert.equal(worker.lifecycle().skipped, true);
});

// restore a waiting worker after the browser terminates its idle process
test("waiting-worker readiness survives process restart", async () => {
  const first = await loadWorker({ fetcher: completeShellResponse, active: {} });
  await first.dispatchLifecycle("install");
  const restarted = await loadWorker({ fetcher: completeShellResponse, active: {}, seededCaches: first.cachesByName });
  const [status] = await restarted.dispatchMessage({ type: "WEATHER_UPDATE_STATUS" });
  assert.equal(status.phase, "ready");
  assert.equal(status.completed, status.total);
  await restarted.dispatchMessage({ type: "WEATHER_APPLY_UPDATE", release: "test-release" });
  assert.equal(restarted.lifecycle().skipped, true);
});

// reject corrupt incomplete private or mixed-release shells without retiring the active one
test("failed staging preserves the current shell and cannot activate", async () => {
  // exercise independent failure boundaries
  for (const failure of ["network", "http", "module", "private", "release", "integrity", "quota"]) {
    const oldCache = new MemoryCache(completeShellResponse);
    const partial = new MemoryCache(completeShellResponse);
    // emulate a partial cache write failing under storage pressure
    if (failure === "quota") {
      partial.put = async () => { throw new Error("quota"); };
    }
    const seededCaches = new Map([["ballydidean-weather-shell-old", oldCache]]);
    // insert only the targeted staging cache for quota failure
    if (failure === "quota") {
      seededCaches.set("ballydidean-weather-shell-test-release", partial);
    }
    const worker = await loadWorker({
      active: {},
      seededCaches,
      ...(failure === "integrity" ? { digests: {} } : {}),
      // fail one response while retaining ordinary serving
      fetcher: async (request) => {
        const path = new URL(request.url).pathname;
        // simulate a failed immutable module download
        if (path.endsWith("index.js")) {
          // retain each transport and MIME failure independently
          if (failure === "network") { throw new Error("offline"); }
          if (failure === "http") { return new Response("failed", { status: 503 }); }
          if (failure === "module") { return new Response("<html>error</html>", { headers: { "content-type": "text/html" } }); }
        }
        // reject user-specific and cross-release documents
        if (path === "/forecast") {
          if (failure === "private") { return productionResponse({ cacheControl: "private, no-store" }); }
          if (failure === "release") { return new Response(productionShell.replace("test-release", "other-release"), { headers: { "content-type": "text/html" } }); }
        }
        return completeShellResponse(request);
      },
    });
    await assert.rejects(worker.dispatchLifecycle("install"), undefined, failure);
    assert.equal(worker.messages.at(-1).phase, "failed", failure);
    assert.equal(worker.cachesByName.get("ballydidean-weather-shell-old"), oldCache, failure);
    assert.equal(worker.cachesByName.has("ballydidean-weather-shell-test-release"), false, failure);
    await worker.dispatchMessage({ type: "WEATHER_APPLY_UPDATE", release: "test-release" });
    assert.equal(worker.lifecycle().skipped, false, failure);
  }
});
