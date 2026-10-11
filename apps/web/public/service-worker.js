const release = "__WEATHER_ASSET_VERSION__";
const cachePrefix = "ballydidean-weather-shell-";
const shellCache = `${cachePrefix}${release}`;
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
  `/assets/${release}/index.js`,
  `/assets/${release}/units.js`,
  `/assets/${release}/solar-cloud.js`,
  `/assets/${release}/solar-cloud-evidence.js`,
  `/assets/${release}/local-forecast-bias.js`
];
const shellPathSet = new Set(shellPaths);
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

// cache the versioned application shell
self.addEventListener("install", (event) => {
  event.waitUntil((async () => {
    const cache = await caches.open(shellCache);
    const shellRequests = shellPaths.map(
      // keep administrator cookies out of the shared shell
      (path) => new Request(path, { credentials: "omit" }),
    );
    await cache.addAll(shellRequests);

    // sanitize each precached dashboard document
    for (const request of shellRequests) {
      const pathname = new URL(request.url).pathname;

      // preserve static documents and assets
      if (!dashboardPaths.has(pathname)) {
        continue;
      }

      const response = await cache.match(request);

      // fail installation before activating an unsafe shell
      if (response === undefined) {
        throw new Error(`Missing precached dashboard response for ${pathname}`);
      }

      await cache.put(request, await sanitizeOfflineNavigation(response));
    }

    await self.skipWaiting();
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
