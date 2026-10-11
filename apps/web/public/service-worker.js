const release = "__WEATHER_ASSET_VERSION__";
const cachePrefix = "ballydidean-weather-shell-";
const shellCache = `${cachePrefix}${release}`;
const readyPath = `/__weather-shell-ready/${release}`;
const shellAssetDigests = __WEATHER_SHELL_ASSET_DIGESTS__;
const updateState = { type: "WEATHER_UPDATE_STATE", release, phase: "downloading", completed: 0, total: 0 };
const shellPaths = [
  "/",
  "/logs",
  "/map",
  "/forecast",
  "/trends",
  "/settings",
  "/privacy",
  "/privacy/",
  "/manifest.webmanifest",
  "/brand/ballydidean-weather-icon-192.png",
  "/brand/ballydidean-weather-icon-512.png",
  "/brand/ballydidean-weather-icon-maskable-512.png",
  "/brand/ballydidean-weather-icon-180.png",
  "/brand/ballydidean-weather-icon-32.png",
  "/weather-icons/01-sunny.svg",
  "/weather-icons/02-sunny-wind.svg",
  "/weather-icons/03-partly-cloudy.svg",
  "/weather-icons/04-partly-cloudy-wind.svg",
  "/weather-icons/05-cloudy.svg",
  "/weather-icons/06-cloudy-wind.svg",
  "/weather-icons/07-light-rain.svg",
  "/weather-icons/08-light-rain-wind.svg",
  "/weather-icons/09-heavy-rain.svg",
  "/weather-icons/10-heavy-rain-wind.svg",
  "/weather-icons/12-unavailable.svg",
  "/weather-icons/13-clear-night.svg",
  "/weather-icons/14-clear-night-wind.svg",
  "/weather-icons/15-partly-cloudy-night.svg",
  "/weather-icons/16-partly-cloudy-night-wind.svg",
  "/fonts/google-sans-flex-latin.woff2",
  "/fonts/material-symbols-rounded-v4.woff2",
  `/assets/${release}/styles.css`,
  `/assets/${release}/client.js`,
  `/assets/${release}/hosted-updates.js`,
  `/assets/${release}/index.js`,
  `/assets/${release}/units.js`,
  `/assets/${release}/solar-cloud.js`,
  `/assets/${release}/solar-cloud-evidence.js`,
  `/assets/${release}/local-forecast-bias.js`
];
const shellPathSet = new Set(shellPaths);
updateState.total = shellPaths.length;
const dashboardPaths = new Set(["/", "/logs", "/map", "/forecast", "/trends", "/settings"]);
const analyticsSources = new Set([
  "https://www.googletagmanager.com",
  "https://*.google-analytics.com",
  "https://*.google.com",
]);

// remove analytics sources from one policy
function sanitizeAnalyticsPolicy(policy) {
  return policy
    .split(";")
    .map(
      // remove analytics sources from one directive
      (directive) => directive
        .trim()
        .split(/\s+/u)
        .filter(
          // retain non-analytics sources
          (source) => !analyticsSources.has(source),
        )
        .join(" "),
    )
    .filter(
      // discard empty directives
      (directive) => directive.length > 0,
    )
    .join("; ");
}

// build one analytics-free offline document
async function sanitizeOfflineNavigation(response) {
  const body = await response.clone().text();
  const safeBody = body.replaceAll('data-weather-analytics="true"', 'data-weather-analytics="false"');
  const policy = response.headers.get("content-security-policy");
  const safePolicy = policy === null ? null : sanitizeAnalyticsPolicy(policy);

  // retain already-safe static responses exactly
  if (safeBody === body && safePolicy === policy) {
    return response;
  }

  const headers = new Headers(response.headers);

  // remove analytics network permissions
  if (safePolicy !== null) {
    headers.set("content-security-policy", safePolicy);
  }

  // remove body metadata invalidated by rewriting
  if (safeBody !== body || headers.has("content-encoding")) {
    headers.delete("content-encoding");
    headers.delete("content-length");
  }

  return new Response(safeBody, {
    headers,
    status: response.status,
    statusText: response.statusText,
  });
}

// reject user-specific navigation responses
function canCacheNavigation(response) {
  const directives = (response.headers.get("cache-control") ?? "")
    .split(",")
    .map(
      // normalize one cache directive name
      (directive) => directive.trim().split("=", 1)[0]?.toLowerCase(),
    );
  const contentType = response.headers.get("content-type")?.toLowerCase();
  return (
    response.ok &&
    contentType?.startsWith("text/html") === true &&
    !directives.includes("private") &&
    !directives.includes("no-store")
  );
}

// report bounded download metadata without application data
async function reportUpdate() {
  try {
    const clients = await self.clients.matchAll({ type: "window", includeUncontrolled: true });
    // notify every document observing this registration
    for (const client of clients) {
      client.postMessage({ ...updateState });
    }
  } catch {
    // disappearing clients must not invalidate a verified installation
  }
}

// answer waiting-worker status and exact-release activation requests
// recover readiness after an idle waiting worker has been restarted
async function restoreUpdateState() {
  // avoid interfering with active installation or known failures
  if (updateState.phase !== "downloading" || updateState.completed !== 0 || !await caches.has(shellCache)) {
    return;
  }
  const cache = await caches.open(shellCache);
  const marker = await cache.match(readyPath);
  // trust only the final marker written after the complete install
  if (marker !== undefined && await marker.text() === release) {
    updateState.completed = updateState.total;
    updateState.phase = "ready";
  }
}

self.addEventListener("message", (event) => {
  event.waitUntil((async () => {
    await restoreUpdateState();
    // return verified readiness even when the page missed installation
    if (event.data?.type === "WEATHER_UPDATE_STATUS") {
      event.source?.postMessage({ ...updateState });
    }
    // never activate incomplete or unrelated releases
    if (event.data?.type === "WEATHER_APPLY_UPDATE" && event.data.release === release && updateState.phase === "ready") {
      await self.skipWaiting();
    }
  })());
});

// download and verify the complete shell before declaring readiness
self.addEventListener("install", (event) => {
  event.waitUntil((async () => {
    let cache;
    try {
      await reportUpdate();
      const responses = [];
      // validate all bytes before writing the staged shell
      for (const path of shellPaths) {
        const digest = shellAssetDigests[path];
        // fail closed when an executable lacks its image-owned checksum
        if (path.startsWith(`/assets/${release}/`) && !/^sha256-[A-Za-z0-9+/]{43}=$/u.test(digest ?? "")) {
          throw new Error("Missing shell integrity");
        }
        const request = new Request(path, {
          credentials: "omit",
          cache: "reload",
          ...(digest === undefined ? {} : { integrity: digest }),
        });
        let response = await fetch(request);
        // reject transport failures and disguised module error pages
        if (!response.ok || (path.endsWith(".js") && !response.headers.get("content-type")?.includes("javascript"))) {
          throw new Error("Invalid shell asset");
        }
        // consume each transfer before using another connection
        const bytes = await response.arrayBuffer();
        const headers = new Headers(response.headers);
        headers.delete("content-encoding");
        headers.delete("content-length");
        response = new Response(bytes, { headers, status: response.status, statusText: response.statusText });
        // refuse private shells and a deployment changing during installation
        if (dashboardPaths.has(path)) {
          const body = await response.clone().text();
          const embeddedRelease = body.match(/<meta\s+name="weather-release"\s+content="([^"]+)"/u)?.[1];
          // keep staged routes tied to the same public release
          if (!canCacheNavigation(response) || embeddedRelease !== release) {
            throw new Error("Invalid shell document");
          }
          response = await sanitizeOfflineNavigation(response);
        }
        responses.push([request, response]);
        updateState.completed += 1;
        await reportUpdate();
      }
      cache = await caches.open(shellCache);
      // commit only verified responses before reporting readiness
      for (const [request, response] of responses) {
        await cache.put(request, response);
      }
      await cache.put(readyPath, new Response(release));
      updateState.phase = "ready";
      await reportUpdate();
      // first installs have no active release to interrupt
      if (self.registration.active === null) {
        await self.skipWaiting();
      }
    } catch (error) {
      // discard only this failed staging cache while preserving the active shell
      if (cache !== undefined) {
        await caches.delete(shellCache);
      }
      updateState.phase = "failed";
      await reportUpdate();
      throw error;
    }
  })());
});

// retire only superseded weather shells
self.addEventListener("activate", (event) => {
  event.waitUntil((async () => {
    const cacheNames = await caches.keys();

    // remove prior release caches
    for (const cacheName of cacheNames) {
      // preserve unrelated and current caches
      if (cacheName.startsWith(cachePrefix) && cacheName !== shellCache) {
        await caches.delete(cacheName);
      }
    }

    await self.clients.claim();
  })());
});

// serve the offline shell without caching weather data
self.addEventListener("fetch", (event) => {
  const request = event.request;

  // ignore mutations
  if (request.method !== "GET") {
    return;
  }

  const url = new URL(request.url);

  // ignore other origins
  if (url.origin !== self.location.origin) {
    return;
  }

  // leave live weather and map data on the network path
  if (
    url.pathname === "/admin" ||
    url.pathname === "/admin/" ||
    url.pathname.startsWith("/api/") ||
    url.pathname.startsWith("/maps/")
  ) {
    return;
  }

  // refresh navigations before using the offline shell
  if (request.mode === "navigate") {
    event.respondWith(networkFirstNavigation(request));
    return;
  }

  // intercept only known shell assets
  if (shellPathSet.has(url.pathname)) {
    event.respondWith(cacheFirstShellAsset(request));
  }
});

// update one route while retaining an offline fallback
async function networkFirstNavigation(request) {
  const cache = await caches.open(shellCache);

  try {
    const response = await fetch(request);

    // retain only public route shells
    if (canCacheNavigation(response)) {
      await cache.put(request, await sanitizeOfflineNavigation(response.clone()));
    }

    return response;
  } catch {
    const exact = await cache.match(request);

    // prefer the requested offline route
    if (exact !== undefined) {
      return exact;
    }

    return await cache.match("/") ?? Response.error();
  }
}

// use one immutable cached shell asset
async function cacheFirstShellAsset(request) {
  const cache = await caches.open(shellCache);
  const cached = await cache.match(request);

  // reuse the release shell
  if (cached !== undefined) {
    return cached;
  }

  return fetch(request);
}
