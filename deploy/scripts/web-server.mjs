import { constants as fsConstants } from "node:fs";
import { createHash, randomBytes } from "node:crypto";
import { lstat, open, readFile, realpath } from "node:fs/promises";
import { createServer } from "node:http";
import { dirname, join, resolve } from "node:path";

import {
  projectWidgetForecast,
  WIDGET_FORECAST_MAX_BYTES,
} from "../../apps/web/dist/widget-forecast.js";
import { projectWidgetForecastV2 } from "../../apps/web/dist/widget-forecast-v2.js";
import { projectWidgetForecastV3 } from "../../apps/web/dist/widget-forecast-v3.js";
import { XweatherTileMemoryCache } from "./xweather-tile-cache.mjs";
import { XweatherUsageBudget } from "./xweather-usage-budget.mjs";
import { WeatherAdminStore } from "./weather-admin-store.mjs";
import { HomeNetworkMatcher } from "./home-network.mjs";
import { ForecastObservationCache } from "./forecast-observations.mjs";
import {
  ADJUSTMENT_EVIDENCE_DEFAULT_ROOT,
  AdjustmentEvidenceArchiveTransport,
  AdjustmentEvidenceScheduler,
  AdjustmentEvidenceStore,
  AdjustmentRevisionArchiveStore,
  createAdjustmentRevisionArchiveHandler,
  normalizeAdjustmentEvidenceWindow,
} from "./adjustment-evidence-store.mjs";
import {
  createMaintenanceShadowPredictionMetadata,
  parseAdjustmentRainFixedGaugeTargetProjection,
  parseAdjustmentRevisionProjectionDocument,
  parseRainMaintenanceControlState,
  parseMaintenanceShadowComparator,
  parseMaintenanceShadowSourceProjection,
  parseMaintenanceShadowValues,
  validateMaintenanceShadowComparatorBinding,
} from "../../apps/web/dist/adjustment-maintenance-contract.mjs";
import {
  FORECAST_ADJUSTMENT_SCORECARD_MAX_BYTES,
  FORECAST_ADJUSTMENT_SCORECARD_V2_CONTRACT_VERSION,
  FORECAST_ADJUSTMENT_SCORECARD_V2_MAX_BYTES,
  parseForecastAdjustmentScorecard,
} from "./forecast-adjustment-scorecard-contract.mjs";

const root = resolve(process.cwd());
const publicRoot = join(root, "apps/web/public");
const compiledRoot = join(root, "apps/web/dist");
const adminLoginPath = join(publicRoot, "admin-login.html");
const adminSessionCookieName = "weather_admin_session";
const maximumApiBytes = 1024 * 1024;
const widgetForecastPath = "/api/v1/sites/ballydidean/widget-forecast";
const widgetForecastV2Path = "/api/v2/sites/ballydidean/widget-forecast";
const widgetForecastV3Path = "/api/v3/sites/ballydidean/widget-forecast";
// allow the complete daily trends history
const maximumTrendsApiBytes = 2 * 1024 * 1024;
const maximumMapBytes = 4 * 1024 * 1024;
const adjustmentEvidenceRoot = resolve(
  process.env.WEATHER_ADJUSTMENT_EVIDENCE_ROOT ?? ADJUSTMENT_EVIDENCE_DEFAULT_ROOT,
);
const scheduledCaptureEnabled =
  process.env.WEATHER_ADJUSTMENT_SCHEDULED_CAPTURE_ENABLED === "true";
const scheduledCaptureMigrationReady =
  process.env.WEATHER_ADJUSTMENT_MAINTENANCE_MIGRATION_READY === "true";
const scheduledArchiveTransferEnabled =
  process.env.WEATHER_ADJUSTMENT_ARCHIVE_TRANSFER_ENABLED === "true";
const scheduledCaptureParameter = "__weather_scheduler_nonce";
const scheduledCaptureRequests = new Map();
const apiOrigin = parseApiOrigin(process.env.WEATHER_API_ORIGIN);
const forecastObservationCache = new ForecastObservationCache({ origin: apiOrigin });
const adjustmentMaintenanceApiOrigin = parseOptionalAdjustmentMaintenanceApiOrigin(
  process.env.WEATHER_ADJUSTMENT_MAINTENANCE_API_ORIGIN,
);
const xweatherOrigin = parseXweatherOrigin(
  process.env.WEATHER_XWEATHER_MAP_ORIGIN ?? "https://maps.api.xweather.com",
);
const xweatherCredentials = await loadXweatherCredentials();
const port = parsePort(process.env.PORT ?? "3000");
const adjustmentMaintenanceInternalPort = parseOptionalPort(
  process.env.WEATHER_ADJUSTMENT_MAINTENANCE_INTERNAL_PORT,
);
const release = parseAssetRelease(process.env.WEATHER_RELEASE ?? "development");
// bind rendered metadata to the source revision baked into this exact web image
const webRevision = process.env.WEATHER_WEB_REVISION ?? "development";
// reject invalid identities before rendering any application document
if (webRevision !== "development" && !/^[a-f0-9]{40}$/u.test(webRevision)) {
  throw new Error("invalid web source revision");
}
const productionAnalytics = process.env.NODE_ENV === "production" && release !== "development";
const homeNetworkMatcher = new HomeNetworkMatcher();
const forecastMapPreloadSite = await loadForecastMapPreloadSite(
  process.env.WEATHER_SITE_CONFIG_PATH ?? join(root, "config/sites/ballydidean.json"),
);
const adminStore = new WeatherAdminStore({
  authPath: process.env.WEATHER_ADMIN_AUTH_PATH ?? "/var/lib/weather/xweather/admin-auth.json",
  bootstrapTokenPath: process.env.WEATHER_ADMIN_BOOTSTRAP_TOKEN_PATH ??
    process.env.WEATHER_XWEATHER_CLIENT_SECRET_FILE ??
    "/run/secrets/weather_xweather_client_secret",
  center: forecastMapPreloadSite ?? { latitude: 47.95043, longitude: -122.42797 },
  layoutPath: process.env.WEATHER_PROPERTY_SENSOR_LAYOUT_PATH ?? "/var/lib/weather/xweather/property-sensor-layout.json",
});
const adjustmentEvidenceStore = await new AdjustmentEvidenceStore({
  root: adjustmentEvidenceRoot,
}).initialize();
const adjustmentRevisionArchiveHandler = createAdjustmentRevisionArchiveHandler({
  store: new AdjustmentRevisionArchiveStore({
    parseProjection: parseAdjustmentRevisionArchiveProjection,
    parseRainControlState: parseRainMaintenanceControlState,
    root: adjustmentEvidenceRoot,
    validateShadowStage: validateAdjustmentShadowRevisionStage,
    validateShadowCapsule: validateAdjustmentShadowRevisionCapsule,
  }),
});
// keep archive transfer inert until the controller explicitly enables it
const adjustmentEvidenceArchiveTransport = scheduledArchiveTransferEnabled
  ? await new AdjustmentEvidenceArchiveTransport({
      root: adjustmentEvidenceRoot,
    }).initialize().catch(
      // preserve ordinary serving when archive initialization refuses
      () => null,
    )
  : null;
const adjustmentEvidenceScheduler = await new AdjustmentEvidenceScheduler({
  enabled: scheduledCaptureEnabled,
  migrationReady: scheduledCaptureMigrationReady,
  readScheduleSlots: readAdjustmentMaintenanceScheduleSlots,
  root: adjustmentEvidenceRoot,
  trigger: triggerScheduledAdjustmentCapture,
}).initialize();
const assetPrefix = `/assets/${release}/`;
const assets = new Map([
  ["/", { cache: "no-cache", path: join(publicRoot, "index.html"), template: true, type: "text/html; charset=utf-8" }],
  ["/index.html", { cache: "no-cache", path: join(publicRoot, "index.html"), template: true, type: "text/html; charset=utf-8" }],
  ["/logs", { cache: "no-cache", path: join(publicRoot, "index.html"), template: true, type: "text/html; charset=utf-8" }],
  ["/logs/", { cache: "no-cache", path: join(publicRoot, "index.html"), template: true, type: "text/html; charset=utf-8" }],
  ["/map", { cache: "no-cache", path: join(publicRoot, "index.html"), template: true, type: "text/html; charset=utf-8" }],
  ["/map/", { cache: "no-cache", path: join(publicRoot, "index.html"), template: true, type: "text/html; charset=utf-8" }],
  ["/admin", { cache: "no-store", path: join(publicRoot, "index.html"), template: true, type: "text/html; charset=utf-8" }],
  ["/admin/", { cache: "no-store", path: join(publicRoot, "index.html"), template: true, type: "text/html; charset=utf-8" }],
  ["/forecast", { cache: "no-cache", path: join(publicRoot, "index.html"), template: true, type: "text/html; charset=utf-8" }],
  ["/forecast/", { cache: "no-cache", path: join(publicRoot, "index.html"), template: true, type: "text/html; charset=utf-8" }],
  ["/trends", { cache: "no-cache", path: join(publicRoot, "index.html"), template: true, type: "text/html; charset=utf-8" }],
  ["/trends/", { cache: "no-cache", path: join(publicRoot, "index.html"), template: true, type: "text/html; charset=utf-8" }],
  ["/settings", { cache: "no-cache", path: join(publicRoot, "index.html"), template: true, type: "text/html; charset=utf-8" }],
  ["/settings/", { cache: "no-cache", path: join(publicRoot, "index.html"), template: true, type: "text/html; charset=utf-8" }],
  // keep the public policy readable without the application or analytics
  ["/privacy", { analytics: false, cache: "no-cache", path: join(publicRoot, "privacy.html"), template: true, type: "text/html; charset=utf-8" }],
  ["/privacy/", { analytics: false, cache: "no-cache", path: join(publicRoot, "privacy.html"), template: true, type: "text/html; charset=utf-8" }],
  ["/manifest.webmanifest", { cache: "no-cache", path: join(publicRoot, "manifest.webmanifest"), type: "application/manifest+json; charset=utf-8" }],
  ["/service-worker.js", { cache: "no-store", path: join(publicRoot, "service-worker.js"), template: true, type: "text/javascript; charset=utf-8" }],
  ["/brand/ballydidean-wide.svg", { cache: "public, max-age=86400", path: join(publicRoot, "brand/ballydidean-wide.svg"), type: "image/svg+xml" }],
  ["/brand/favicon.svg", { cache: "public, max-age=86400", path: join(publicRoot, "brand/favicon.svg"), type: "image/svg+xml" }],
  ["/brand/ballydidean-weather-icon-32.png", { cache: "no-cache", path: join(publicRoot, "brand/ballydidean-weather-icon-32.png"), type: "image/png" }],
  ["/brand/ballydidean-weather-icon-180.png", { cache: "no-cache", path: join(publicRoot, "brand/ballydidean-weather-icon-180.png"), type: "image/png" }],
  ["/brand/ballydidean-weather-icon-192.png", { cache: "no-cache", path: join(publicRoot, "brand/ballydidean-weather-icon-192.png"), type: "image/png" }],
  ["/brand/ballydidean-weather-icon-512.png", { cache: "no-cache", path: join(publicRoot, "brand/ballydidean-weather-icon-512.png"), type: "image/png" }],
  ["/brand/ballydidean-weather-icon-maskable-512.png", { cache: "no-cache", path: join(publicRoot, "brand/ballydidean-weather-icon-maskable-512.png"), type: "image/png" }],
  // expose only approved current-condition artwork
  ["/weather-icons/01-sunny.svg", { cache: "no-cache", path: join(publicRoot, "weather-icons/01-sunny.svg"), type: "image/svg+xml" }],
  ["/weather-icons/02-sunny-wind.svg", { cache: "no-cache", path: join(publicRoot, "weather-icons/02-sunny-wind.svg"), type: "image/svg+xml" }],
  ["/weather-icons/03-partly-cloudy.svg", { cache: "no-cache", path: join(publicRoot, "weather-icons/03-partly-cloudy.svg"), type: "image/svg+xml" }],
  ["/weather-icons/04-partly-cloudy-wind.svg", { cache: "no-cache", path: join(publicRoot, "weather-icons/04-partly-cloudy-wind.svg"), type: "image/svg+xml" }],
  ["/weather-icons/05-cloudy.svg", { cache: "no-cache", path: join(publicRoot, "weather-icons/05-cloudy.svg"), type: "image/svg+xml" }],
  ["/weather-icons/06-cloudy-wind.svg", { cache: "no-cache", path: join(publicRoot, "weather-icons/06-cloudy-wind.svg"), type: "image/svg+xml" }],
  ["/weather-icons/07-light-rain.svg", { cache: "no-cache", path: join(publicRoot, "weather-icons/07-light-rain.svg"), type: "image/svg+xml" }],
  ["/weather-icons/08-light-rain-wind.svg", { cache: "no-cache", path: join(publicRoot, "weather-icons/08-light-rain-wind.svg"), type: "image/svg+xml" }],
  ["/weather-icons/09-heavy-rain.svg", { cache: "no-cache", path: join(publicRoot, "weather-icons/09-heavy-rain.svg"), type: "image/svg+xml" }],
  ["/weather-icons/10-heavy-rain-wind.svg", { cache: "no-cache", path: join(publicRoot, "weather-icons/10-heavy-rain-wind.svg"), type: "image/svg+xml" }],
  ["/weather-icons/12-unavailable.svg", { cache: "no-cache", path: join(publicRoot, "weather-icons/12-unavailable.svg"), type: "image/svg+xml" }],
  ["/weather-icons/13-clear-night.svg", { cache: "no-cache", path: join(publicRoot, "weather-icons/13-clear-night.svg"), type: "image/svg+xml" }],
  ["/weather-icons/14-clear-night-wind.svg", { cache: "no-cache", path: join(publicRoot, "weather-icons/14-clear-night-wind.svg"), type: "image/svg+xml" }],
  ["/weather-icons/15-partly-cloudy-night.svg", { cache: "no-cache", path: join(publicRoot, "weather-icons/15-partly-cloudy-night.svg"), type: "image/svg+xml" }],
  ["/weather-icons/16-partly-cloudy-night-wind.svg", { cache: "no-cache", path: join(publicRoot, "weather-icons/16-partly-cloudy-night-wind.svg"), type: "image/svg+xml" }],
  ["/fonts/google-sans-flex-latin.woff2", { cache: "public, max-age=31536000, immutable", path: join(publicRoot, "fonts/google-sans-flex-latin.woff2"), type: "font/woff2" }],
  ["/fonts/LICENSE-google-sans-flex.txt", { cache: "public, max-age=86400", path: join(publicRoot, "fonts/LICENSE-google-sans-flex.txt"), type: "text/plain; charset=utf-8" }],
  ["/fonts/material-symbols-rounded-v4.woff2", { cache: "public, max-age=31536000, immutable", path: join(publicRoot, "fonts/material-symbols-rounded-v4.woff2"), type: "font/woff2" }],
  ["/fonts/LICENSE-material-symbols.txt", { cache: "public, max-age=86400", path: join(publicRoot, "fonts/LICENSE-material-symbols.txt"), type: "text/plain; charset=utf-8" }],
]);
const versionedAssets = new Map([
  ["styles.css", { cache: "public, max-age=31536000, immutable", path: join(publicRoot, "styles.css"), type: "text/css; charset=utf-8" }],
  ["client.js", { cache: "public, max-age=31536000, immutable", path: join(compiledRoot, "client.js"), type: "text/javascript; charset=utf-8" }],
  ["hosted-updates.js", { cache: "public, max-age=31536000, immutable", path: join(compiledRoot, "hosted-updates.js"), type: "text/javascript; charset=utf-8" }],
  ["app-install.js", { cache: "public, max-age=31536000, immutable", path: join(compiledRoot, "app-install.js"), type: "text/javascript; charset=utf-8" }],
  ["app-version.js", { cache: "public, max-age=31536000, immutable", path: join(compiledRoot, "app-version.js"), type: "text/javascript; charset=utf-8" }],
  ["index.js", { cache: "public, max-age=31536000, immutable", path: join(compiledRoot, "index.js"), type: "text/javascript; charset=utf-8" }],
  ["solar-cloud.js", { cache: "public, max-age=31536000, immutable", path: join(compiledRoot, "solar-cloud.js"), type: "text/javascript; charset=utf-8" }],
  ["solar-cloud-evidence.js", { cache: "public, max-age=31536000, immutable", path: join(compiledRoot, "solar-cloud-evidence.js"), type: "text/javascript; charset=utf-8" }],
  ["local-forecast-bias.js", { cache: "public, max-age=31536000, immutable", path: join(compiledRoot, "local-forecast-bias.js"), type: "text/javascript; charset=utf-8" }],
  ["units.js", { cache: "public, max-age=31536000, immutable", path: join(compiledRoot, "units.js"), type: "text/javascript; charset=utf-8" }],
]);

// bind every immutable executable and stylesheet to the served worker
const shellAssetDigests = Object.fromEntries(await Promise.all([...versionedAssets].map(
  // hash the exact image-owned bytes before admitting the web server
  async ([name, asset]) => [
    `${assetPrefix}${name}`,
    `sha256-${createHash("sha256").update(await readFile(asset.path)).digest("base64")}`,
  ],
)));

// expose archive mutation only on the compose-private listener
const adjustmentMaintenanceInternalServer = adjustmentMaintenanceInternalPort === null
  ? null
  : createServer(
      // contain every private transport rejection behind one fixed response
      async (request, response) => {
        await serveAdjustmentRevisionArchive(request, response);
      },
    );

// route isolated edge requests without trusting browser-supplied identities
const server = createServer(async (request, response) => {
  try {
    const requestUrl = new URL(request.url ?? "/", "http://weather.invalid");

    // expose only ephemeral home-network display eligibility
    if (requestUrl.pathname === "/api/v1/viewer-context") {
      // keep viewer context read-only and off every shared cache
      if (request.method !== "GET" && request.method !== "HEAD") {
        sendText(response, 405, "method not allowed\n", { Allow: "GET, HEAD" });
        return;
      }
      const body = JSON.stringify({ data: { homeNetwork: await homeNetworkMatcher.matches(request) } });
      setSecurityHeaders(response);
      response.writeHead(200, {
        "Cache-Control": "private, no-store",
        "Content-Length": String(Buffer.byteLength(body)),
        "Content-Type": "application/json; charset=utf-8",
      });
      response.end(request.method === "HEAD" ? undefined : body);
      return;
    }

    // exchange the HTML login form for an opaque session cookie
    if (requestUrl.pathname === "/admin/login") {
      await loginAdmin(request, response);
      return;
    }

    // revoke only the presented administrator session
    if (requestUrl.pathname === "/admin/logout") {
      logoutAdmin(request, response);
      return;
    }

    // initialize admin access through one secret-bound request only
    if (requestUrl.pathname === "/api/v1/admin/bootstrap") {
      await bootstrapAdmin(request, response);
      return;
    }

    // expose the read-only shared sensor layout
    if (requestUrl.pathname === "/api/v1/sites/ballydidean/property-sensor-layout") {
      await servePropertySensorLayout(request, response);
      return;
    }

    // expose only the current public adjustment switches
    if (requestUrl.pathname === "/api/v1/sites/ballydidean/forecast-adjustment-settings") {
      await serveForecastAdjustmentSettings(request, response, false);
      return;
    }

    // protect adjustment switch reads and writes with the admin session
    if (requestUrl.pathname === "/api/v1/admin/sites/ballydidean/forecast-adjustment-settings") {
      await serveForecastAdjustmentSettings(request, response, true);
      return;
    }

    // expose only the installed aggregate scorecard to administrators
    if (requestUrl.pathname === "/api/v1/admin/sites/ballydidean/forecast-adjustment-scorecard") {
      await serveForecastAdjustmentScorecard(request, response, requestUrl);
      return;
    }

    // update one layout entry behind the administrator session
    if (requestUrl.pathname.startsWith("/api/v1/admin/sites/ballydidean/property-sensor-layout/")) {
      await updatePropertySensorLayout(request, response, requestUrl.pathname);
      return;
    }

    // isolate the closed widget route before the general api proxy
    if (isWidgetForecastPath(requestUrl.pathname)) {
      await serveWidgetForecast(request, response, requestUrl);
      return;
    }

    // share one bounded archive fill without altering recorded or qualified forecast products
    if (requestUrl.pathname === "/api/v1/sites/ballydidean/forecast-observations" || requestUrl.pathname === "/api/v1/sites/ballydidean/solar-cloud-evidence") {
      // reject all browser-controlled archive geometry and mutation methods
      if (request.method !== "GET" && request.method !== "HEAD") {
        sendText(response, 405, "method not allowed\n", { Allow: "GET, HEAD" });
        return;
      }
      // one queryless route keeps cache identity and load bounded
      if (requestUrl.search !== "") {
        sendText(response, 400, "unsupported query\n");
        return;
      }
      try {
        const cached = await forecastObservationCache.get();
        // preserve the existing observation envelope and expose only compact source evidence
        const body = JSON.stringify(requestUrl.pathname.endsWith("/solar-cloud-evidence")
          ? { contractVersion: "solar-cloud-evidence/v1", generatedAt: cached.generatedAt, evidence: cached.solarCloudEvidence }
          : { generatedAt: cached.generatedAt, data: cached.data });
        setSecurityHeaders(response);
        response.writeHead(200, {
          "Cache-Control": "public, max-age=60, s-maxage=60",
          "Content-Length": String(Buffer.byteLength(body)),
          "Content-Type": "application/json; charset=utf-8",
        });
        response.end(request.method === "HEAD" ? undefined : body);
      } catch {
        sendText(response, 503, "recorded history is temporarily unavailable\n", { "Cache-Control": "no-store", "Retry-After": "30" });
      }
      return;
    }

    // proxy only the same-origin API namespace
    if (requestUrl.pathname.startsWith("/api/v1/")) {
      await proxyApi(request, response, requestUrl);
      return;
    }

    // proxy only the bounded weather-map namespace
    if (requestUrl.pathname.startsWith("/maps/xweather/")) {
      await proxyXweatherMap(request, response, requestUrl);
      return;
    }

    // serve only the explicit static allowlist
    if (request.method !== "GET" && request.method !== "HEAD") {
      sendText(response, 405, "method not allowed\n", { Allow: "GET, HEAD" });
      return;
    }

    // unwrap the RemoteAgents preview bridge on the public tunnel
    if (requestUrl.pathname === "/__rac/browser-device") {
      redirectRemoteAgentsBrowser(response, requestUrl);
      return;
    }

    const asset = resolveAsset(requestUrl.pathname);

    // reject traversal and unknown files uniformly
    if (asset === undefined || requestUrl.pathname.includes("\\")) {
      sendText(response, 404, "not found\n");
      return;
    }

    // resolve one authenticated HTML representation
    const isHtmlTemplate = asset.template === true && asset.type === "text/html; charset=utf-8";
    const isAdminRoute = requestUrl.pathname === "/admin" || requestUrl.pathname === "/admin/";
    const isAdmin = isHtmlTemplate && adminStore.authenticateSession(
      readAdminSessionCookie(request.headers.cookie),
    );

    // render a real login page before the protected application shell
    if (isAdminRoute && !isAdmin) {
      await sendAdminLoginPage(
        response,
        request.method === "HEAD",
        requestUrl.searchParams.get("error") === "invalid",
      );
      return;
    }

    // exclude standalone policies even on the production origin
    const analyticsEnabled = productionAnalytics && isHtmlTemplate && !isAdmin && asset.analytics !== false;
    const source = await readFile(asset.path);
    const body = asset.template === true
      ? Buffer.from(renderHtmlTemplate(source.toString("utf8"), requestUrl.pathname, isAdmin, analyticsEnabled))
      : source;
    // permit only HTML documents to render across iframe origins
    if (isHtmlTemplate) {
      setHtmlSecurityHeaders(response, analyticsEnabled);
    } else {
      setSecurityHeaders(response);
    }
    response.writeHead(200, {
      "Cache-Control": isAdmin ? "private, no-store" : asset.cache,
      "Content-Length": String(body.byteLength),
      "Content-Type": asset.type,
      ...(isHtmlTemplate ? { Vary: "Cookie" } : {}),
    });

    // omit response bodies for HEAD
    if (request.method === "HEAD") {
      response.end();
    } else {
      response.end(body);
    }
  } catch {
    sendText(response, 500, "internal server error\n");
  }
});

server.listen(port, "0.0.0.0",
  // start only the admitted clock after the listener is reachable
  () => adjustmentEvidenceScheduler.start());
// listen only inside the container network with no published host port
adjustmentMaintenanceInternalServer?.listen(adjustmentMaintenanceInternalPort, "0.0.0.0");

// dispatch one exact private revision archive request
async function serveAdjustmentRevisionArchive(request, response) {
  try {
    // reject browser methods, query aliases and every non-revision route
    if (request.method !== "POST" || request.url === undefined ||
      ![
        "/internal/adjustment-maintenance/archive/stage",
        "/internal/adjustment-maintenance/archive/publish",
        "/internal/adjustment-maintenance/archive/gap",
        "/internal/adjustment-maintenance/revision/stage",
        "/internal/adjustment-maintenance/revision/publish",
        "/internal/adjustment-maintenance/revision/gap",
        "/internal/adjustment-maintenance/rain-control-state/stage",
        "/internal/adjustment-maintenance/rain-control-state/gap",
        "/internal/adjustment-maintenance/rain-fixed-gauge-target/gap",
        "/internal/adjustment-maintenance/rain-fixed-gauge-target/gap/status",
      ].includes(request.url)) {
      sendJson(response, 404, { error: "not_found" });
      return;
    }
    const contentType = String(request.headers["content-type"] ?? "")
      .split(";", 1)[0]?.trim().toLowerCase();
    // require the api relay's exact machine-readable body class
    if (contentType !== "application/json") {
      sendJson(response, 415, { error: "unsupported_media_type" });
      return;
    }
    const input = await readRequestJson(request, 4_832 * 1024);
    const receipt = await adjustmentRevisionArchiveHandler(request.url, input);
    sendJson(response, 200, receipt);
  } catch (error) {
    // expose only the bounded private spool-capacity condition to the api relay
    if (error?.code === "adjustment_revision_spool_refused") {
      sendJson(response, 429, { error: "adjustment_revision_spool_full" });
      return;
    }
    sendJson(response, 409, { error: "revision_archive_rejected" });
  }
}

// admit the additive large fixed-gauge body without widening legacy grammars
function parseAdjustmentRevisionArchiveProjection(bytes) {
  const value = JSON.parse(Buffer.from(bytes).toString("utf8"));
  return value?.contractVersion === "adjustment-rain-fixed-gauge-target-projection/v1"
    ? parseAdjustmentRainFixedGaugeTargetProjection(bytes)
    : parseAdjustmentRevisionProjectionDocument(bytes);
}

// validate one shadow capsule through the producer's exact source/body contracts
function validateAdjustmentShadowRevisionCapsule(capsule) {
  const source = Buffer.from(capsule.sourceProjectionBase64, "base64");
  const body = Buffer.from(capsule.bodyBase64, "base64");
  validateAdjustmentShadowRevisionStage({
    body,
    ...(capsule.contractVersion === "adjustment-shadow-revision-capsule/v2"
      ? { comparator: Buffer.from(capsule.comparatorBase64, "base64") }
      : {}),
    metadata: capsule.metadata,
    sourceProjection: source,
  });
}

// validate exact staged shadow source and prediction bytes
function validateAdjustmentShadowRevisionStage(input) {
  const source = Buffer.from(input.sourceProjection);
  const body = Buffer.from(input.body);
  parseMaintenanceShadowSourceProjection(source);
  parseMaintenanceShadowValues(body);
  // bind the additive incumbent member to the same source and candidate body
  if (input.comparator !== undefined) {
    const comparator = parseMaintenanceShadowComparator(Buffer.from(input.comparator));
    validateMaintenanceShadowComparatorBinding(comparator, source, body);
  }
  const expected = createMaintenanceShadowPredictionMetadata(body, source);
  const fields = [
    "bodyByteCount", "candidateSha256", "dueKey", "inputSha256", "issuedAt",
    "maxValidAt", "minValidAt", "predictionBodySha256", "predictionSchemaSha256",
    "predictionSha256", "registrationSha256", "rowCount", "sourceReceiptSha256",
    "sourceSha256",
  ];
  // require every compact field to be reproduced from exact decoded bytes
  if (fields.some((field) => input.metadata[field] !== expected[field])) {
    throw new Error("adjustment shadow capsule metadata differs");
  }
}

// identify the exact widget route and its rejected near-matches
function isWidgetForecastPath(pathname) {
  return pathname === widgetForecastPath ||
    pathname.startsWith(`${widgetForecastPath}/`) ||
    /^\/api\/v1\/sites\/[^/]+\/widget-forecast(?:\/|$)/u.test(pathname) ||
    pathname === widgetForecastV2Path ||
    pathname.startsWith(`${widgetForecastV2Path}/`) ||
    /^\/api\/v2\/sites\/[^/]+\/widget-forecast(?:\/|$)/u.test(pathname) ||
    pathname === widgetForecastV3Path ||
    pathname.startsWith(`${widgetForecastV3Path}/`) ||
    /^\/api\/v3\/sites\/[^/]+\/widget-forecast(?:\/|$)/u.test(pathname);
}

// serve one bounded public widget snapshot
async function serveWidgetForecast(request, response, requestUrl) {
  // select only one reviewed versioned projection
  const projector = requestUrl.pathname === widgetForecastPath
    ? projectWidgetForecast
    : requestUrl.pathname === widgetForecastV2Path
      ? projectWidgetForecastV2
      : requestUrl.pathname === widgetForecastV3Path
        ? projectWidgetForecastV3
        : null;

  // reject every site and path outside the fixed public contract
  if (projector === null) {
    sendText(response, 404, "not found\n");
    return;
  }

  // reject caller-controlled projection inputs
  if (requestUrl.search !== "" || (request.url ?? "").includes("?")) {
    sendText(response, 400, "bad request\n");
    return;
  }

  // keep the public projection read-only
  if (request.method !== "GET" && request.method !== "HEAD") {
    sendText(response, 405, "method not allowed\n", { Allow: "GET, HEAD" });
    return;
  }

  try {
    const settings = (await adminStore.readAdjustmentSettingsStatus()).settings;
    // request the anchor-bearing product only for the overnight contract
    const upstreamPath = requestUrl.pathname === widgetForecastV3Path
      ? "/api/v1/sites/ballydidean/forecast?window=overnight"
      : "/api/v1/sites/ballydidean/forecast?days=1";
    const target = new URL(upstreamPath, apiOrigin);
    const upstream = await fetch(target, {
      headers: { Accept: "application/json" },
      method: "GET",
      redirect: "manual",
      signal: AbortSignal.timeout(apiRequestTimeoutMs(target.pathname)),
    });

    // expose only a successfully validated upstream forecast
    if (!upstream.ok) {
      await upstream.body?.cancel();
      throw new Error("widget forecast upstream failed");
    }

    const sourceBody = await readBoundedBody(upstream, maximumApiBytes, "API");
    const filteredBody = filterForecastResponse(sourceBody, settings);
    const filtered = JSON.parse(filteredBody.toString("utf8"));
    const snapshot = projector(filtered, new Date().toISOString());
    const body = Buffer.from(`${JSON.stringify(snapshot)}\n`);

    // enforce the serialized edge response ceiling including its newline
    if (body.byteLength > WIDGET_FORECAST_MAX_BYTES) {
      throw new Error("widget forecast response exceeded the edge limit");
    }

    setSecurityHeaders(response);
    response.writeHead(200, {
      "Cache-Control": "no-store",
      "Content-Length": String(body.byteLength),
      "Content-Type": "application/json; charset=utf-8",
    });
    response.end(request.method === "HEAD" ? undefined : body);
  } catch {
    sendText(response, 502, "upstream unavailable\n");
  }
}

// create one administrator browser session
async function loginAdmin(request, response) {
  // accept only the login form method
  if (request.method !== "POST") {
    sendText(response, 405, "method not allowed\n", { Allow: "POST" });
    return;
  }

  try {
    const form = await readRequestForm(request, 2_048);
    const session = await adminStore.startSession(form.password);

    // return invalid credentials to the framed login page
    if (session === null) {
      sendRedirect(response, 303, "/admin?error=invalid");
      return;
    }

    sendRedirect(response, 303, "/admin", {
      "Set-Cookie": createAdminSessionCookie(
        session.token,
        session.maximumAgeSeconds,
        request,
      ),
    });
  } catch (error) {
    // reject malformed and oversized form submissions uniformly
    if (error instanceof RangeError) {
      sendText(response, 400, "invalid request\n");
    } else {
      sendText(response, 500, "internal server error\n");
    }
  }
}

// clear one administrator browser session
function logoutAdmin(request, response) {
  // accept only an explicit logout form
  if (request.method !== "POST") {
    sendText(response, 405, "method not allowed\n", { Allow: "POST" });
    return;
  }

  adminStore.revokeSession(readAdminSessionCookie(request.headers.cookie));
  sendRedirect(response, 303, "/admin", {
    "Set-Cookie": clearAdminSessionCookie(request),
  });
}

// read one exact opaque administrator cookie
function readAdminSessionCookie(header) {
  // reject missing or oversized cookie collections
  if (typeof header !== "string" || header.length > 8_192) {
    return null;
  }

  const prefix = `${adminSessionCookieName}=`;
  const sessions = header.split(";").flatMap(
    // retain only exact administrator cookie pairs
    (entry) => {
      const cookie = entry.trim();
      return cookie.startsWith(prefix) ? [cookie.slice(prefix.length)] : [];
    },
  );
  return sessions.length === 1 ? sessions[0] : null;
}

// create one iframe-compatible administrator cookie
function createAdminSessionCookie(token, maximumAgeSeconds, request) {
  return [
    `${adminSessionCookieName}=${token}`,
    "Path=/",
    "HttpOnly",
    `Max-Age=${String(maximumAgeSeconds)}`,
    ...adminSessionCookieContext(request),
  ].join("; ");
}

// expire one administrator cookie in the matching context
function clearAdminSessionCookie(request) {
  return [
    `${adminSessionCookieName}=`,
    "Path=/",
    "HttpOnly",
    "Max-Age=0",
    ...adminSessionCookieContext(request),
  ].join("; ");
}

// select cookie attributes for local and HTTPS iframe use
function adminSessionCookieContext(request) {
  return requestUsesHttps(request)
    ? ["SameSite=None", "Secure", "Partitioned"]
    : ["SameSite=Lax"];
}

// recognize HTTPS termination from the trusted edge proxy
function requestUsesHttps(request) {
  const forwarded = Array.isArray(request.headers["x-forwarded-proto"])
    ? request.headers["x-forwarded-proto"][0]
    : request.headers["x-forwarded-proto"];
  return request.socket.encrypted === true || forwarded?.split(",", 1)[0]?.trim() === "https";
}

// initialize the first admin password without persisting plaintext
async function bootstrapAdmin(request, response) {
  // accept only one explicit mutation method
  if (request.method !== "POST") {
    sendText(response, 405, "method not allowed\n", { Allow: "POST" });
    return;
  }

  try {
    const body = await readRequestJson(request, 2_048);
    const result = await adminStore.bootstrap(
      request.headers["x-weather-admin-bootstrap"],
      body.password,
    );

    // reject invalid one-time credentials uniformly
    if (result.status === "unauthorized") {
      sendText(response, 401, "unauthorized\n");
      return;
    }

    // permanently close bootstrap after first configuration
    if (result.status === "already_configured") {
      sendText(response, 409, "admin access is already configured\n");
      return;
    }

    sendJson(response, 201, { configured: true });
  } catch (error) {
    // distinguish malformed client input from server persistence failures
    if (error instanceof RangeError || error instanceof SyntaxError) {
      sendText(response, 400, "invalid request\n");
    } else {
      sendText(response, 500, "internal server error\n");
    }
  }
}

// serve the shared public sensor layout
async function servePropertySensorLayout(request, response) {
  // preserve a read-only public endpoint
  if (request.method !== "GET" && request.method !== "HEAD") {
    sendText(response, 405, "method not allowed\n", { Allow: "GET, HEAD" });
    return;
  }

  const body = { data: await adminStore.readLayout() };
  sendJson(response, 200, body, request.method === "HEAD", {
    "Cache-Control": "no-cache",
  });
}

// serve or replace the persisted global adjustment switches
async function serveForecastAdjustmentSettings(request, response, admin) {
  // require the administrator session before private reads or writes
  if (admin && !adminStore.authenticateSession(readAdminSessionCookie(request.headers.cookie))) {
    sendAdminUnauthorized(response);
    return;
  }

  // provide a small public read without exposing storage diagnostics
  if (request.method === "GET" || request.method === "HEAD") {
    const status = await adminStore.readAdjustmentSettingsStatus();
    sendJson(
      response,
      200,
      admin && status.error !== null
        ? { data: status.settings, error: status.error }
        : { data: status.settings },
      request.method === "HEAD",
    );
    return;
  }

  // reject non-admin methods and cross-site browser mutations
  if (!admin || request.method !== "PUT") {
    sendText(response, 405, "method not allowed\n", { Allow: admin ? "GET, HEAD, PUT" : "GET, HEAD" });
    return;
  }

  // require one genuine same-origin mutation
  if (!isSameOriginMutation(request)) {
    sendText(response, 403, "same-origin request required\n");
    return;
  }

  try {
    const body = await readRequestJson(request, 1_024);
    const settings = await adminStore.writeAdjustmentSettings(body);
    sendJson(response, 200, { data: settings });
  } catch (error) {
    // separate malformed switches from private storage failures
    if (error instanceof RangeError || error instanceof SyntaxError) {
      sendText(response, 400, "invalid request\n");
    } else {
      sendText(response, 500, "internal server error\n");
    }
  }
}

// serve one authenticated aggregate-only adjustment scorecard
async function serveForecastAdjustmentScorecard(request, response, requestUrl) {
  // authenticate before disclosing scorecard availability
  if (!adminStore.authenticateSession(readAdminSessionCookie(request.headers.cookie))) {
    sendAdminUnauthorized(response);
    return;
  }

  // reject caller-controlled scorecard selection
  if (requestUrl.search !== "" || (request.url ?? "").includes("?")) {
    sendText(response, 400, "bad request\n");
    return;
  }

  // keep the private endpoint strictly read-only
  if (request.method !== "GET" && request.method !== "HEAD") {
    sendText(response, 405, "method not allowed\n", { Allow: "GET, HEAD" });
    return;
  }

  try {
    const publication = await readInstalledForecastAdjustmentScorecard(
      adjustmentEvidenceRoot,
      new Date().toISOString(),
    );
    sendJson(response, 200, {
      data: publication.scorecard,
      publicationState: publication.publicationState,
    }, request.method === "HEAD", {
      "Cache-Control": "private, no-store",
    });
  } catch {
    sendText(response, 503, "scorecard unavailable\n", {
      "Cache-Control": "private, no-store",
      "Retry-After": "300",
    });
  }
}

// read one optional fixed private hash pointer without following links
async function readForecastAdjustmentScorecardPointer(path) {
  let pointerBytes;

  try {
    pointerBytes = await readPrivateRegularFile(path, 128);
  } catch (error) {
    // treat only a missing fixed pointer as absent
    if (error?.code === "ENOENT") {
      return null;
    }
    throw error;
  }
  const pointer = JSON.parse(pointerBytes.toString("utf8"));

  // require the exact hash-only pointer contract
  if (pointer === null || Array.isArray(pointer) || typeof pointer !== "object" ||
    Object.keys(pointer).length !== 1 ||
    typeof pointer.sha256 !== "string" || !/^[a-f0-9]{64}$/u.test(pointer.sha256)) {
    throw new Error("scorecard pointer is invalid");
  }
  return pointer.sha256;
}

// read one hash-addressed scorecard through the fixed v1/v2 slots
async function readInstalledForecastAdjustmentScorecard(root, now) {
  const scorecardDirectory = join(root, "scorecards");
  const [rootDetails, directoryDetails, canonicalRoot, canonicalDirectory] = await Promise.all([
    lstat(root),
    lstat(scorecardDirectory),
    realpath(root),
    realpath(scorecardDirectory),
  ]);

  // reject linked roots and scorecard directory escapes
  if (!rootDetails.isDirectory() || rootDetails.isSymbolicLink() ||
    (rootDetails.mode & 0o777) !== 0o700 ||
    !directoryDetails.isDirectory() || directoryDetails.isSymbolicLink() ||
    (directoryDetails.mode & 0o777) !== 0o700 ||
    dirname(canonicalDirectory) !== canonicalRoot) {
    throw new Error("scorecard directory is invalid");
  }

  const [legacySha256, currentV2Sha256, previousV2Sha256, pendingV2Sha256] =
    await Promise.all([
      readForecastAdjustmentScorecardPointer(join(root, "current.json")),
      readForecastAdjustmentScorecardPointer(join(root, "v2-current.json")),
      readForecastAdjustmentScorecardPointer(join(root, "v2-previous.json")),
      readForecastAdjustmentScorecardPointer(join(root, "v2-pending.json")),
    ]);

  // fail closed on forbidden legacy and selected-v2 coexistence
  if (legacySha256 !== null &&
    (currentV2Sha256 !== null || previousV2Sha256 !== null)) {
    throw new Error("scorecard publication slots are invalid");
  }

  // require one readable current or pending publication
  const selection = pendingV2Sha256 !== null
    ? { maximumBytes: FORECAST_ADJUSTMENT_SCORECARD_V2_MAX_BYTES, publicationState: "pending_unapplied", sha256: pendingV2Sha256 }
    : currentV2Sha256 !== null
      ? { maximumBytes: FORECAST_ADJUSTMENT_SCORECARD_V2_MAX_BYTES, publicationState: "current", sha256: currentV2Sha256 }
      : legacySha256 !== null
        ? { maximumBytes: FORECAST_ADJUSTMENT_SCORECARD_MAX_BYTES, publicationState: "legacy_display", sha256: legacySha256 }
        : null;

  if (selection === null) {
    throw new Error("scorecard publication is absent");
  }
  const selectedDirectory = selection.publicationState === "legacy_display"
    ? scorecardDirectory
    : join(root, "scorecards-v2");

  // prove the selected generation's fixed object directory remains private and beneath root
  if (selection.publicationState !== "legacy_display") {
    const [selectedDetails, canonicalSelectedDirectory] = await Promise.all([
      lstat(selectedDirectory),
      realpath(selectedDirectory),
    ]);

    if (!selectedDetails.isDirectory() || selectedDetails.isSymbolicLink() ||
      (selectedDetails.mode & 0o777) !== 0o700 ||
      dirname(canonicalSelectedDirectory) !== canonicalRoot) {
      throw new Error("v2 scorecard directory is invalid");
    }
  }

  const scorecardBytes = await readPrivateRegularFile(
    join(selectedDirectory, `sha256-${selection.sha256}.json`),
    selection.maximumBytes,
  );
  const actualSha256 = createHash("sha256").update(scorecardBytes).digest("hex");

  // bind the pointer to the exact validated publication bytes
  if (actualSha256 !== selection.sha256) {
    throw new Error("scorecard content hash is invalid");
  }
  const scorecard = parseForecastAdjustmentScorecard(scorecardBytes, { now });

  // bind each slot generation to its intended display state
  if ((selection.publicationState === "legacy_display" &&
    scorecard.contractVersion !== "forecast-adjustment-scorecard/v1") ||
    (selection.publicationState !== "legacy_display" &&
      scorecard.contractVersion !== FORECAST_ADJUSTMENT_SCORECARD_V2_CONTRACT_VERSION)) {
    throw new Error("scorecard generation does not match its publication slot");
  }
  return { publicationState: selection.publicationState, scorecard };
}

// read one bounded regular file without following links
async function readPrivateRegularFile(path, maximumBytes) {
  const handle = await open(path, fsConstants.O_RDONLY | fsConstants.O_NOFOLLOW);

  try {
    const details = await handle.stat();

    if (!details.isFile() || (details.mode & 0o777) !== 0o600 ||
      details.size > maximumBytes) {
      throw new Error("private file is invalid");
    }

    return await handle.readFile();
  } finally {
    await handle.close();
  }
}

// bind a state-changing request to the document's own origin
function isSameOriginMutation(request) {
  const origin = request.headers.origin;
  const host = request.headers.host;

  // require one concrete browser origin and host
  if (typeof origin !== "string" || typeof host !== "string") {
    return false;
  }

  try {
    const expected = new URL(`${requestUsesHttps(request) ? "https" : "http"}://${host}`);
    const submitted = new URL(origin);
    return expected.host === host && submitted.origin === expected.origin && origin === submitted.origin;
  } catch {
    return false;
  }
}

// update one server-persisted sensor layout entry
async function updatePropertySensorLayout(request, response, pathname) {
  // require one authenticated admin request
  if (!adminStore.authenticateSession(readAdminSessionCookie(request.headers.cookie))) {
    sendAdminUnauthorized(response);
    return;
  }

  // accept only bounded JSON updates
  if (request.method !== "PUT") {
    sendText(response, 405, "method not allowed\n", { Allow: "PUT" });
    return;
  }

  const prefix = "/api/v1/admin/sites/ballydidean/property-sensor-layout/";
  const encodedKey = pathname.slice(prefix.length);

  try {
    const sensorKey = decodeURIComponent(encodedKey);
    const body = await readRequestJson(request, 4_096);
    const saved = await adminStore.upsertSensor(sensorKey, body);
    sendJson(response, 200, { data: saved });
  } catch (error) {
    // reject malformed keys, bodies, and property positions
    if (error instanceof RangeError || error instanceof SyntaxError || error instanceof URIError) {
      sendText(response, 400, "invalid request\n");
    } else {
      sendText(response, 500, "internal server error\n");
    }
  }
}

// reject one unauthenticated administrator API request
function sendAdminUnauthorized(response) {
  sendText(response, 401, "authentication required\n");
}

// serve the iframe-compatible administrator login page
async function sendAdminLoginPage(response, head, invalidCredentials) {
  const source = await readFile(adminLoginPath, "utf8");
  const body = Buffer.from(renderAdminLoginTemplate(source, invalidCredentials));
  setHtmlSecurityHeaders(response);
  response.writeHead(200, {
    "Cache-Control": "no-store",
    "Content-Length": String(body.byteLength),
    "Content-Type": "text/html; charset=utf-8",
    Vary: "Cookie",
  });
  response.end(head ? undefined : body);
}

// read one bounded JSON request body
async function readRequestJson(request, maximumBytes) {
  const content = await readRequestBody(request, maximumBytes);
  const parsed = JSON.parse(content.toString("utf8"));

  // require one object request body
  if (parsed === null || Array.isArray(parsed) || typeof parsed !== "object") {
    throw new RangeError("request body must be an object");
  }

  return parsed;
}

// read one bounded URL-encoded login form
async function readRequestForm(request, maximumBytes) {
  const contentType = String(request.headers["content-type"] ?? "")
    .split(";", 1)[0]
    ?.trim()
    .toLowerCase();

  // require the browser's native form encoding
  if (contentType !== "application/x-www-form-urlencoded") {
    throw new RangeError("request body must be URL encoded");
  }

  const parameters = new URLSearchParams(
    (await readRequestBody(request, maximumBytes)).toString("utf8"),
  );
  const passwords = parameters.getAll("password");

  // require exactly one bounded password field
  if (passwords.length !== 1 || [...parameters.keys()].some(
    // reject every unexpected form field
    (key) => key !== "password",
  )) {
    throw new RangeError("login form is invalid");
  }

  return { password: passwords[0] };
}

// read one bounded request body
async function readRequestBody(request, maximumBytes) {
  const chunks = [];
  let length = 0;

  // buffer only the configured small request limit
  for await (const chunk of request) {
    const bytes = Buffer.from(chunk);
    length += bytes.byteLength;

    // stop oversized writes before parsing
    if (length > maximumBytes) {
      throw new RangeError("request body is too large");
    }

    chunks.push(bytes);
  }

  return Buffer.concat(chunks, length);
}

// resolve only fixed or active-release assets
function resolveAsset(pathname) {
  // isolate compiled assets by immutable release URL
  if (pathname.startsWith(assetPrefix)) {
    return versionedAssets.get(pathname.slice(assetPrefix.length));
  }

  return assets.get(pathname);
}

// redirect one validated RemoteAgents preview target
function redirectRemoteAgentsBrowser(response, requestUrl) {
  const mode = requestUrl.searchParams.get("mode");
  const location = requestUrl.searchParams.get("location");

  // require the bridge contract and one root-relative target
  if (
    (mode !== "desktop" && mode !== "mobile") ||
    location === null ||
    !location.startsWith("/") ||
    location.startsWith("//")
  ) {
    sendText(response, 400, "invalid browser preview\n");
    return;
  }

  const destination = new URL(location, "http://weather.invalid");

  // reject URL-parser backslash normalization into another origin
  if (destination.origin !== "http://weather.invalid") {
    sendText(response, 400, "invalid browser preview\n");
    return;
  }

  const target = `${destination.pathname}${destination.search}${destination.hash}`;
  setSecurityHeaders(response);
  response.writeHead(302, {
    "Cache-Control": "no-store",
    "Content-Length": "0",
    Location: target,
  });
  response.end();
}

// render one route-aware release template
function renderHtmlTemplate(source, pathname, isAdmin, analyticsEnabled) {
  return source
    .replaceAll("__WEATHER_ASSET_VERSION__", release)
    .replaceAll("__WEATHER_GIT_REVISION__", webRevision)
    .replaceAll("__WEATHER_SHELL_ASSET_DIGESTS__", JSON.stringify(shellAssetDigests))
    .replaceAll("__WEATHER_ADMIN__", String(isAdmin))
    .replaceAll("__WEATHER_ANALYTICS__", String(analyticsEnabled))
    .replaceAll("__WEATHER_ROUTE_PRELOAD__", forecastMapPreloadLink(pathname));
}

// render one bounded login error state
function renderAdminLoginTemplate(source, invalidCredentials) {
  const error = invalidCredentials
    ? '<p class="admin-login-error" role="alert">The password is incorrect.</p>'
    : "";
  return source
    .replaceAll("__WEATHER_ASSET_VERSION__", release)
    .replaceAll("__WEATHER_ADMIN_LOGIN_ERROR__", error);
}

// prioritize the first Today radar frame before JavaScript executes
function forecastMapPreloadLink(pathname) {
  // keep weather-map traffic off unrelated routes
  if (
    (pathname !== "/forecast" && pathname !== "/forecast/") ||
    forecastMapPreloadSite === null
  ) {
    return "";
  }

  const frameMs = Math.floor(Date.now() / (10 * 60 * 1_000)) * 10 * 60 * 1_000;
  const validTime = formatXweatherValidTime(new Date(frameMs));
  const latitude = forecastMapPreloadSite.latitude.toFixed(6);
  const longitude = forecastMapPreloadSite.longitude.toFixed(6);
  const href = `/maps/xweather/history/radar/${validTime}/10/256x168/${latitude},${longitude}.png`;
  return `<link rel="preload" as="image" type="image/png" fetchpriority="high" href="${href}">`;
}

// load only the public map center from the site configuration
async function loadForecastMapPreloadSite(path) {
  try {
    const configuration = JSON.parse(await readFile(path, "utf8"));
    const site = configuration?.site;

    // require one bounded public map center
    if (
      site === null ||
      typeof site !== "object" ||
      !Number.isFinite(site.latitude) ||
      site.latitude < -85 ||
      site.latitude > 85 ||
      !Number.isFinite(site.longitude) ||
      site.longitude < -180 ||
      site.longitude > 180
    ) {
      return null;
    }

    return { latitude: site.latitude, longitude: site.longitude };
  } catch {
    // retain ordinary static serving without optional preload metadata
    return null;
  }
}

// format one provider-compatible UTC timestamp
function formatXweatherValidTime(instant) {
  return [
    String(instant.getUTCFullYear()).padStart(4, "0"),
    String(instant.getUTCMonth() + 1).padStart(2, "0"),
    String(instant.getUTCDate()).padStart(2, "0"),
    String(instant.getUTCHours()).padStart(2, "0"),
    String(instant.getUTCMinutes()).padStart(2, "0"),
    String(instant.getUTCSeconds()).padStart(2, "0"),
  ].join("");
}

// validate one cache-safe release path segment
function parseAssetRelease(value) {
  // allow the direct local development server
  if (value === "development") {
    return value;
  }

  // require the immutable deployment release shape
  if (!/^\d{4}\.\d{2}\.\d{2}-[1-9]\d?$/u.test(value)) {
    throw new Error("WEATHER_RELEASE must use YYYY.MM.DD-N");
  }

  return value;
}

// validate the internal upstream origin
function parseApiOrigin(value) {
  const origin = new URL(value ?? "http://api:3001");

  // reject credentials, non-HTTP protocols, and path prefixes
  if (
    (origin.protocol !== "http:" && origin.protocol !== "https:") ||
    origin.username !== "" ||
    origin.password !== "" ||
    origin.pathname !== "/" ||
    origin.search !== "" ||
    origin.hash !== ""
  ) {
    throw new Error("WEATHER_API_ORIGIN must be a credential-free HTTP origin");
  }

  return origin;
}

// validate one optional compose-private api origin
function parseOptionalAdjustmentMaintenanceApiOrigin(value) {
  // keep inert and local web processes independent of the private listener
  if (value === undefined) {
    return null;
  }
  const origin = new URL(value);
  // admit only the fixed compose service or disposable loopback tests
  if (origin.protocol !== "http:" ||
    !["api", "127.0.0.1", "localhost", "[::1]"].includes(origin.hostname) ||
    origin.username !== "" || origin.password !== "" || origin.pathname !== "/" ||
    origin.search !== "" || origin.hash !== "") {
    throw new Error("WEATHER_ADJUSTMENT_MAINTENANCE_API_ORIGIN must be a compose-private HTTP origin");
  }
  return origin;
}

// read the database-authenticated rolling horizon through the private api listener
async function readAdjustmentMaintenanceScheduleSlots() {
  // fail closed when the private schedule reader is not configured
  if (adjustmentMaintenanceApiOrigin === null) {
    throw new Error("adjustment maintenance api origin is unavailable");
  }
  const target = new URL("/internal/adjustment-maintenance/schedule-slots", adjustmentMaintenanceApiOrigin);
  const response = await fetch(target, {
    headers: { Accept: "application/json" },
    method: "GET",
    redirect: "manual",
    signal: AbortSignal.timeout(5_000),
  });
  // accept only one bounded successful private response
  if (!response.ok) {
    await response.body?.cancel();
    throw new Error("adjustment maintenance schedule read failed");
  }
  const body = await readBoundedBody(response, 8 * 1_024, "adjustment maintenance schedule");
  return JSON.parse(body.toString("utf8"));
}

// validate the Xweather map origin
function parseXweatherOrigin(value) {
  const origin = new URL(value);
  const loopback = ["127.0.0.1", "::1", "localhost"].includes(origin.hostname);

  // require HTTPS outside disposable loopback tests
  if (
    (origin.protocol !== "https:" && !(origin.protocol === "http:" && loopback)) ||
    origin.username !== "" ||
    origin.password !== "" ||
    origin.pathname !== "/" ||
    origin.search !== "" ||
    origin.hash !== ""
  ) {
    throw new Error("WEATHER_XWEATHER_MAP_ORIGIN must be a credential-free HTTPS origin");
  }

  return origin;
}

// validate the listener port
function parsePort(value) {
  const parsed = Number(value);

  // reject invalid listener configuration
  if (!Number.isSafeInteger(parsed) || parsed < 1 || parsed > 65_535) {
    throw new RangeError("PORT must be between 1 and 65535");
  }

  return parsed;
}

// disable the private listener when its compose-only port is absent
function parseOptionalPort(value) {
  // keep local public-server tests free of an additional listener
  if (value === undefined) {
    return null;
  }
  return parsePort(value);
}

// issue one process-tracked loopback request through the public forecast handler
async function triggerScheduledAdjustmentCapture(input) {
  const nonce = randomBytes(32).toString("hex");
  let settled = false;
  let settleCapture;
  const capture = new Promise(
    // retain one resolver unavailable to request headers
    (resolveCapture) => { settleCapture = resolveCapture; },
  );
  const timeout = setTimeout(
    // bound a missing finish or aborted-response result
    () => settle({ status: "capture_timeout" }),
    30_000,
  );

  // settle one scheduler request exactly once
  function settle(result) {
    // ignore duplicate close or timeout notifications
    if (settled) {
      return;
    }
    settled = true;
    settleCapture(result);
  }

  scheduledCaptureRequests.set(nonce, { dueKey: input.dueKey, settle });

  try {
    const target = new URL(input.path, `http://127.0.0.1:${String(port)}`);
    target.searchParams.set(scheduledCaptureParameter, nonce);
    const response = await fetch(target, {
      headers: { Accept: "application/json" },
      method: "GET",
      redirect: "manual",
      signal: AbortSignal.timeout(30_000),
    });
    await response.arrayBuffer();

    // require the normal public route to complete successfully
    if (!response.ok) {
      const error = new Error("scheduled forecast response failed");
      error.code = "forecast_response_failed";
      throw error;
    }
    return await capture;
  } catch (error) {
    settle({ status: "request_failed" });
    throw error;
  } finally {
    clearTimeout(timeout);
    scheduledCaptureRequests.delete(nonce);
  }
}

// recognize only one in-flight loopback nonce owned by this process
function takeScheduledCaptureRequest(request, requestUrl) {
  const nonces = requestUrl.searchParams.getAll(scheduledCaptureParameter);
  const remoteAddress = request.socket.remoteAddress;
  const loopback = remoteAddress === "127.0.0.1" || remoteAddress === "::1" ||
    remoteAddress === "::ffff:127.0.0.1";

  // reject caller headers and every untracked or non-loopback nonce
  if (nonces.length !== 1 || !loopback || !scheduledCaptureRequests.has(nonces[0])) {
    return null;
  }

  const tracked = scheduledCaptureRequests.get(nonces[0]);
  scheduledCaptureRequests.delete(nonces[0]);
  requestUrl.searchParams.delete(scheduledCaptureParameter);
  return tracked;
}

// append only a genuinely committed trusted scheduler response to the online page
async function settleScheduledArchiveCapture(scheduledCapture, preparedEvidence, result) {
  // skip every public response and every deliberately inactive archive controller state
  if (scheduledCapture === null || !scheduledArchiveTransferEnabled) {
    scheduledCapture?.settle(result);
    return;
  }

  // retain a durable scheduler gap when requested archive initialization failed
  if (adjustmentEvidenceArchiveTransport === null) {
    scheduledCapture.settle({ status: "archive_unavailable" });
    return;
  }

  // archive only successful immutable evidence commits after response finish
  if (preparedEvidence !== null &&
    (result?.status === "created" || result?.status === "duplicate")) {
    const archiveResult = await adjustmentEvidenceArchiveTransport.captureCommittedSchedulerEvidence({
      dueKey: scheduledCapture.dueKey,
      prepared: preparedEvidence,
    });

    // retain a scheduler-side gap if even categorical page-gap persistence refused
    if (archiveResult.status === "evidence_gap_persistence_refused") {
      scheduledCapture.settle({ status: "archive_gap_unrecorded" });
      return;
    }
  }
  scheduledCapture.settle(result);
}

// proxy one bounded API request
async function proxyApi(request, response, requestUrl) {
  // keep the edge read-only
  if (request.method !== "GET" && request.method !== "HEAD") {
    sendText(response, 405, "method not allowed\n", { Allow: "GET, HEAD" });
    return;
  }

  const isForecast = requestUrl.pathname === "/api/v1/sites/ballydidean/forecast";
  const scheduledCapture = isForecast
    ? takeScheduledCaptureRequest(request, requestUrl)
    : null;
  const target = new URL(`${requestUrl.pathname}${requestUrl.search}`, apiOrigin);
  const isRainStatus = requestUrl.pathname === "/api/v1/sites/ballydidean/rain-collection";
  const projectsAdjustmentSettings = isForecast || isRainStatus;
  // relax only the exact site trends route
  const maximumResponseBytes = /^\/api\/v1\/sites\/[^/]+\/trends$/u.test(requestUrl.pathname)
    ? maximumTrendsApiBytes
    : maximumApiBytes;

  try {
    const settings = projectsAdjustmentSettings
      ? (await adminStore.readAdjustmentSettingsStatus()).settings
      : null;
    const upstream = await fetch(target, {
      headers: { Accept: request.headers.accept ?? "application/json" },
      // read complete projected JSON even for HEAD so its length stays accurate
      method: request.method === "HEAD" && projectsAdjustmentSettings ? "GET" : request.method,
      redirect: "manual",
      signal: AbortSignal.timeout(apiRequestTimeoutMs(requestUrl.pathname)),
    });
    const sourceBody = request.method === "HEAD" && !projectsAdjustmentSettings
      ? Buffer.alloc(0)
      : await readBoundedBody(upstream, maximumResponseBytes, "API");
    const body = upstream.ok && isForecast
      ? filterForecastResponse(sourceBody, settings)
      : upstream.ok && isRainStatus
        ? filterRainCollectionStatus(sourceBody, settings)
        : sourceBody;
    const contentLength =
      request.method === "HEAD" && !projectsAdjustmentSettings
        ? boundedContentLength(upstream.headers.get("content-length"), maximumResponseBytes)
        : body.byteLength;
    const v2CaptureActive = adjustmentEvidenceScheduler.status().activation === "active";
    const preparedEvidence = upstream.ok && isForecast && request.method === "GET"
      ? v2CaptureActive
        ? adjustmentEvidenceStore.prepareV2(
            body,
            normalizeAdjustmentEvidenceWindow(requestUrl),
            scheduledCapture === null ? "public_get" : "scheduler_request",
          )
        : adjustmentEvidenceStore.prepare(
            body,
            normalizeAdjustmentEvidenceWindow(requestUrl),
          )
      : null;
    setSecurityHeaders(response);
    response.writeHead(upstream.status, {
      "Cache-Control": upstream.ok && !projectsAdjustmentSettings ? apiCacheControl(requestUrl.pathname) : "no-store",
      "Content-Length": String(contentLength),
      "Content-Type": projectsAdjustmentSettings && upstream.ok
        ? "application/json; charset=utf-8"
        : upstream.headers.get("content-type") ?? "application/json; charset=utf-8",
      Vary: "Accept",
    });

    // register the finish handler before sending response bytes
    if (upstream.ok && isForecast && request.method === "GET") {
      adjustmentEvidenceStore.trackResponse(
        response,
        preparedEvidence,
        // keep archive failures out of the successful forecast and scheduler result
        (result) => void settleScheduledArchiveCapture(
          scheduledCapture,
          preparedEvidence,
          result,
        ).catch(
          // settle the genuine evidence result even when archive persistence refuses
          () => scheduledCapture?.settle(result),
        ),
      );
    }

    response.end(request.method === "HEAD" ? undefined : body);
  } catch {
    scheduledCapture?.settle({ status: "request_failed" });
    sendText(response, 502, "upstream unavailable\n");
  }
}

// project one API response through the persisted admin switches
function filterForecastResponse(body, settings) {
  const forecast = JSON.parse(body.toString("utf8"));

  // reject malformed upstream envelopes rather than bypassing a switch
  if (forecast === null || typeof forecast !== "object" || !Array.isArray(forecast.data)) {
    throw new Error("forecast API response is invalid");
  }

  forecast.adjustmentSettings = settings;
  const runtime = forecast.adjustmentRuntime;

  // constrain generic runtime metrics before row decisions are exposed
  if (runtime?.state === "active" && Array.isArray(runtime.enabledMetrics)) {
    runtime.enabledMetrics = runtime.enabledMetrics.filter(
      // retain only categories allowed by the current snapshot
      (metric) => isAdjustmentMetricEnabled(metric, settings),
    );

    // disable a runtime without any permitted metric
    if (runtime.enabledMetrics.length === 0) {
      forecast.adjustmentRuntime = disabledForecastRuntime(runtime);
    }
  }

  // suppress independent runtime identities when their switches are off
  if (!settings.temperature && forecast.temperatureAdjustmentRuntime?.state === "active") {
    forecast.temperatureAdjustmentRuntime = disabledTemperatureRuntime(
      forecast.temperatureAdjustmentRuntime,
    );
  }

  // suppress the rain runtime separately from both existing models
  if (!settings.rain && forecast.rainAdjustmentRuntime?.state === "active") {
    forecast.rainAdjustmentRuntime = disabledRainRuntime(forecast.rainAdjustmentRuntime);
  }

  // apply the same one-request snapshot to every hour
  for (const row of forecast.data) {
    // reject malformed rows instead of leaking unchecked adjustment values
    if (row === null || typeof row !== "object") {
      throw new Error("forecast row is invalid");
    }

    // remove only disallowed generic adjusted metrics
    if (row.adjustment?.state === "active") {
      const permitted = row.adjustment.appliedMetrics?.filter(
        // project the model's selected metrics through admin controls
        (metric) => isAdjustmentMetricEnabled(metric, settings),
      );

      // reject malformed active decisions before responding
      if (!Array.isArray(permitted) || row.adjustment.adjustedMetrics === null ||
        typeof row.adjustment.adjustedMetrics !== "object") {
        throw new Error("forecast adjustment decision is invalid");
      }

      // preserve the active decision only when a metric remains permitted
      if (permitted.length > 0) {
        row.adjustment.appliedMetrics = permitted;
        row.adjustment.adjustedMetrics = Object.fromEntries(
          permitted.map((metric) => [metric, row.adjustment.adjustedMetrics[metric]]),
        );
      } else {
        row.adjustment = disabledForecastDecision();
      }
    }

    // remove sidecar corrections without changing stored raw weather
    if (!settings.temperature && row.temperatureAdjustment?.state === "active") {
      row.temperatureAdjustment = disabledTemperatureDecision(row.temperatureAdjustment);
    }

    // remove rain corrections independently of wind and temperature
    if (!settings.rain && row.rainAdjustment?.state === "active") {
      row.rainAdjustment = disabledRainDecision(row.rainAdjustment);
    }
  }

  return Buffer.from(`${JSON.stringify(forecast)}\n`);
}

// keep aggregate model status aligned with the public rain switch
function filterRainCollectionStatus(body, settings) {
  const status = JSON.parse(body.toString("utf8"));

  // reject malformed aggregate envelopes instead of bypassing a switch
  if (status === null || typeof status !== "object" || status.data === null ||
    typeof status.data !== "object" || typeof status.data.modelEnabled !== "boolean") {
    throw new Error("rain collection status is invalid");
  }

  status.data.modelEnabled = status.data.modelEnabled && settings.rain;
  return Buffer.from(`${JSON.stringify(status)}\n`);
}

// map one metric to its global adjustment category
function isAdjustmentMetricEnabled(metric, settings) {
  // hide every adjusted value when all three switches are off
  if (!settings.temperature && !settings.wind && !settings.rain) {
    return false;
  }

  // keep the optional qualified humidity correction independent otherwise
  if (metric === "relativeHumidityPercent") {
    return true;
  }

  // bind thermal and wind metrics to their respective controls
  if (metric === "temperatureC" || metric === "apparentTemperatureC") {
    return settings.temperature;
  }

  return settings.wind &&
    (metric === "windDirectionDegrees" || metric === "windGustMps" || metric === "windSpeedMps");
}

// redact one disallowed generic runtime identity
function disabledForecastRuntime(runtime) {
  return {
    activationMode: null,
    activeBundle: null,
    authorizationSha256: null,
    candidateArtifactSha256: null,
    enabledMetrics: [],
    evaluationReportSha256: null,
    expiresAt: null,
    loadedAt: runtime.loadedAt,
    qualificationReceiptSha256: null,
    reasonCode: "admin_disabled",
    state: "disabled",
    transferReportSha256: null,
  };
}

// redact one disallowed generic row decision
function disabledForecastDecision() {
  return {
    adjustedMetrics: {},
    appliedMetrics: [],
    contractVersion: "forecast-adjustment-decision/v1",
    reasonCode: "admin_disabled",
    state: "disabled",
  };
}

// redact one disallowed temperature runtime identity
function disabledTemperatureRuntime(runtime) {
  return {
    activeBundle: null,
    authorizationSha256: null,
    expiresAt: null,
    loadedAt: runtime.loadedAt,
    reasonCode: "admin_disabled",
    source: null,
    state: "disabled",
  };
}

// redact one disallowed temperature row correction
function disabledTemperatureDecision(decision) {
  return {
    ...decision,
    branch: null,
    bundleSha256: null,
    correctedTemperatureC: null,
    reasonCode: "admin_disabled",
    recentErrorStateSha256: null,
    sourceForecast: null,
    state: "disabled",
  };
}

// redact one disallowed rain runtime identity
function disabledRainRuntime(runtime) {
  return {
    activeBundle: null,
    loadedAt: runtime.loadedAt,
    reasonCode: "admin_disabled",
    source: null,
    state: "disabled",
  };
}

// redact one disallowed rain row correction
function disabledRainDecision(decision) {
  return {
    ...decision,
    bundleSha256: null,
    correctedPrecipitationMm: null,
    reasonCode: "admin_disabled",
    sourceForecast: null,
    state: "disabled",
  };
}

// select one freshness-aware public API cache policy
function apiCacheControl(pathname) {
  // retain stable site metadata at the edge
  if (pathname === "/api/v1/sites") {
    return "public, max-age=300, s-maxage=3600, stale-while-revalidate=86400";
  }

  // refresh current conditions within one ingestion interval
  if (/^\/api\/v1\/sites\/[^/]+\/current$/u.test(pathname)) {
    return "public, max-age=15, s-maxage=30, stale-while-revalidate=30";
  }

  // retain slower-changing modeled products briefly
  if (/^\/api\/v1\/sites\/[^/]+\/(?:forecast|tides)$/u.test(pathname)) {
    return "public, max-age=60, s-maxage=300, stale-while-revalidate=300";
  }

  // retain one normalized trend snapshot briefly
  if (/^\/api\/v1\/sites\/[^/]+\/trends$/u.test(pathname)) {
    return "public, max-age=300, s-maxage=1800, stale-while-revalidate=86400";
  }

  return "no-store";
}

// allow one bounded cold annual aggregation without relaxing ordinary reads
function apiRequestTimeoutMs(pathname) {
  return /^\/api\/v1\/sites\/[^/]+\/trends$/u.test(pathname) ? 30_000 : 5_000;
}

const XWEATHER_LAYERS = {
  forecast: {
    clouds: "fsatellite",
    precipitation: "fqpf-1h",
    radar: "fradar",
    wind: "fwind-speeds",
  },
  history: {
    clouds: "satellite-geocolor",
    precipitation: "precip-1h",
    radar: "radar",
    wind: "wind-speeds",
  },
};
const XWEATHER_FRAME_PATTERN = /^\/maps\/xweather\/(history|forecast)\/(radar|clouds|precipitation|wind)\/(\d{14})\/(\d{1,2})\/(\d{3,4})x(\d{3,4})\/(-?\d{1,2}(?:\.\d{1,6})?),(-?\d{1,3}(?:\.\d{1,6})?)\.png$/u;
const XWEATHER_FORECAST_FRESHNESS_MS = 60 * 60 * 1_000;
const XWEATHER_MAP_CACHE_BYTES = 256 * 1_024 * 1_024;
const XWEATHER_PROVIDER_CONCURRENCY = 8;
const XWEATHER_PROVIDER_MINIMUM_INTERVAL_MS = 125;
const XWEATHER_PROVIDER_QUEUE_LIMIT = 512;
const XWEATHER_PROVIDER_DAILY_MAP_UNIT_BUDGET = 300;
const XWEATHER_PROVIDER_MONTHLY_MAP_UNIT_BUDGET = 10_000;
const xweatherProviderQueue = [];
let xweatherProviderActive = 0;
let xweatherProviderLastStartedAt = 0;
let xweatherProviderStartTimer = null;
const xweatherUsageBudget = new XweatherUsageBudget({
  dailyLimit: XWEATHER_PROVIDER_DAILY_MAP_UNIT_BUDGET,
  monthlyLimit: XWEATHER_PROVIDER_MONTHLY_MAP_UNIT_BUDGET,
  path: process.env.WEATHER_XWEATHER_USAGE_PATH ?? null,
});
const xweatherTileCache = new XweatherTileMemoryCache({
  forecastFreshnessMs: XWEATHER_FORECAST_FRESHNESS_MS,
  loadTile: queueXweatherTileFetch,
  maximumBytes: XWEATHER_MAP_CACHE_BYTES,
});
// proxy one allowlisted Xweather raster tile
async function proxyXweatherMap(request, response, requestUrl) {
  // keep the map edge read-only
  if (request.method !== "GET" && request.method !== "HEAD") {
    sendText(response, 405, "method not allowed\n", { Allow: "GET, HEAD" });
    return;
  }

  const tile = parseXweatherMapImage(requestUrl.pathname);

  // reject unknown layers and malformed coordinates uniformly
  if (tile === null) {
    sendText(response, 404, "not found\n");
    return;
  }

  // keep the public route disabled without server-side credentials
  if (xweatherCredentials === null) {
    sendText(response, 503, "weather map unavailable\n", { "Retry-After": "300" });
    return;
  }

  try {
    const cached = await xweatherTileCache.get(tile);
    const ageSeconds = Math.max(0, Math.floor((Date.now() - cached.fetchedAt) / 1_000));
    setSecurityHeaders(response);
    response.writeHead(200, {
      "Cache-Control": tile.phase === "history"
        ? "public, max-age=31536000, immutable"
        : "no-store",
      "Content-Length": String(cached.body.byteLength),
      "Content-Type": "image/png",
      "X-Weather-Tile-Age": String(ageSeconds),
      "X-Weather-Tile-Cache": cached.cacheStatus,
    });

    // omit cached tile bodies for HEAD
    if (request.method === "HEAD") {
      response.end();
    } else {
      response.end(cached.body);
    }
  } catch (error) {
    // stop provider spend at the reviewed calendar boundaries
    if (error?.code === "xweather_budget_exhausted") {
      sendText(response, 429, "weather map budget exhausted\n", { "Retry-After": secondsUntilUtcDay() });
    } else {
      sendText(response, 502, "weather tile unavailable\n");
    }
  }
}

// enqueue one provider fetch behind the shared rate boundary
async function queueXweatherTileFetch(tile) {
  // reject excess public demand before memory grows without bound
  if (xweatherProviderQueue.length >= XWEATHER_PROVIDER_QUEUE_LIMIT) {
    throw new Error("Xweather provider queue is full");
  }

  return await new Promise((resolveFetch, rejectFetch) => {
    xweatherProviderQueue.push({ rejectFetch, resolveFetch, tile });
    drainXweatherProviderQueue();
  });
}

// start queued provider requests within concurrency and rate limits
function drainXweatherProviderQueue() {
  // leave one scheduled start in sole control of the queue
  if (
    xweatherProviderStartTimer !== null ||
    xweatherProviderActive >= XWEATHER_PROVIDER_CONCURRENCY ||
    xweatherProviderQueue.length === 0
  ) {
    return;
  }

  const delay = Math.max(
    0,
    xweatherProviderLastStartedAt + XWEATHER_PROVIDER_MINIMUM_INTERVAL_MS - Date.now(),
  );

  // preserve the minimum interval between provider request starts
  if (delay > 0) {
    xweatherProviderStartTimer = setTimeout(
      // resume the provider queue after the rate window
      () => {
        xweatherProviderStartTimer = null;
        drainXweatherProviderQueue();
      },
      delay,
    );
    xweatherProviderStartTimer.unref();
    return;
  }

  const task = xweatherProviderQueue.shift();

  // preserve the checked queue boundary
  if (task === undefined) {
    return;
  }

  xweatherProviderActive += 1;
  xweatherProviderLastStartedAt = Date.now();
  void fetchXweatherTile(task.tile).then(task.resolveFetch, task.rejectFetch).finally(
    // release one provider slot and continue the queue
    () => {
      xweatherProviderActive -= 1;
      drainXweatherProviderQueue();
    },
  );
  drainXweatherProviderQueue();
}

// classify one provider content type without logging parameters
function xweatherDiagnosticContentType(value) {
  const mediaType = value?.split(";", 1)[0]?.trim().toLowerCase();

  // expose only reviewed diagnostic categories
  return ["application/json", "image/png", "text/html", "text/plain"].includes(mediaType ?? "")
    ? mediaType
    : value === null
      ? "missing"
      : "other";
}

// fetch one credentialed tile for the server memory cache
async function fetchXweatherTile(tile) {
  // reject impossible loader use while the integration is disabled
  if (xweatherCredentials === null) {
    throw new Error("Xweather credentials are unavailable");
  }

  reserveXweatherMapUnits(tile);

  const credentials = `${encodeURIComponent(xweatherCredentials.clientId)}_${encodeURIComponent(xweatherCredentials.clientSecret)}`;
  const layer = XWEATHER_LAYERS[tile.phase][tile.layer];
  const geometry = tile.kind === "frame"
    ? `${String(tile.width)}x${String(tile.height)}/${tile.latitude.toFixed(6)},${tile.longitude.toFixed(6)},${String(tile.zoom)}`
    : `${String(tile.zoom)}/${String(tile.column)}/${String(tile.row)}`;
  const target = new URL(
    `${credentials}/${layer}/${geometry}/${tile.validTime}.png`,
    xweatherOrigin,
  );
  let upstream;

  try {
    upstream = await fetch(target, {
      headers: {
        Accept: "image/png",
        "User-Agent": "Ballydidean-Weather/1.0",
      },
      method: "GET",
      redirect: "follow",
      signal: AbortSignal.timeout(8_000),
    });
  } catch (error) {
    // report only one safe transport failure category
    const errorName = error instanceof TypeError ? "TypeError" : "Error";
    process.stderr.write(`Xweather tile fetch failed: error=${errorName}\n`);
    throw error;
  }

  // reject provider errors without exposing credential details
  if (!upstream.ok || !upstream.headers.get("content-type")?.startsWith("image/png")) {
    const mediaType = xweatherDiagnosticContentType(upstream.headers.get("content-type"));
    process.stderr.write(
      `Xweather tile response rejected: status=${String(upstream.status)} content-type=${mediaType}\n`,
    );
    const error = new Error("Xweather tile provider returned an invalid response");
    const retryAfterSeconds = Number(upstream.headers.get("retry-after"));

    // retain only one bounded provider retry delay
    if (Number.isFinite(retryAfterSeconds) && retryAfterSeconds > 0) {
      error.retryAfterMs = Math.min(60_000, Math.ceil(retryAfterSeconds * 1_000));
    }

    throw error;
  }

  return {
    body: await readBoundedBody(upstream, maximumMapBytes, "weather map"),
  };
}

// reserve map units before one billable provider call
function reserveXweatherMapUnits(tile) {
  const units = Math.ceil(tile.width / 256) * Math.ceil(tile.height / 256);
  xweatherUsageBudget.reserve(units);
}

// report the next UTC budget reset in seconds
function secondsUntilUtcDay() {
  const now = new Date();
  const next = Date.UTC(now.getUTCFullYear(), now.getUTCMonth(), now.getUTCDate() + 1);
  return String(Math.max(1, Math.ceil((next - now.getTime()) / 1_000)));
}

// parse one bounded public map-image path
function parseXweatherMapImage(pathname) {
  return parseXweatherFrame(pathname);
}

// parse one bounded public static-frame path
function parseXweatherFrame(pathname) {
  const match = XWEATHER_FRAME_PATTERN.exec(pathname);

  // reject every non-frame route
  if (match === null) {
    return null;
  }

  const [, phase, layer, validTime, zoomText, widthText, heightText, latitudeText, longitudeText] = match;
  const zoom = Number(zoomText);
  const width = Number(widthText);
  const height = Number(heightText);
  const latitude = Number(latitudeText);
  const longitude = Number(longitudeText);
  const instant = parseXweatherValidTime(validTime);
  const now = Date.now();
  const ageMs = instant === null ? Number.POSITIVE_INFINITY : now - instant.getTime();
  const futureMs = instant === null ? Number.POSITIVE_INFINITY : instant.getTime() - now;
  const exactLatitude = forecastMapPreloadSite?.latitude.toFixed(6);
  const exactLongitude = forecastMapPreloadSite?.longitude.toFixed(6);

  // constrain the single reviewed static-map product
  if (
    zoom !== 10 ||
    width !== 256 ||
    height !== 168 ||
    latitude.toFixed(6) !== exactLatitude ||
    longitude.toFixed(6) !== exactLongitude ||
    instant === null ||
    instant.getUTCSeconds() !== 0 ||
    (phase === "history" && instant.getUTCMinutes() % 10 !== 0) ||
    (phase === "history" && (ageMs < -10 * 60 * 1_000 || ageMs > 26 * 60 * 60 * 1_000)) ||
    (phase === "forecast" && instant.getUTCMinutes() !== 0) ||
    (phase === "forecast" && (futureMs < -60 * 60 * 1_000 || futureMs > 26 * 60 * 60 * 1_000))
  ) {
    return null;
  }

  return { height, kind: "frame", latitude, layer, longitude, phase, validTime, width, zoom };
}

// parse one exact UTC Xweather valid time
function parseXweatherValidTime(value) {
  const year = Number(value.slice(0, 4));
  const month = Number(value.slice(4, 6));
  const day = Number(value.slice(6, 8));
  const hour = Number(value.slice(8, 10));
  const minute = Number(value.slice(10, 12));
  const second = Number(value.slice(12, 14));
  const instant = new Date(Date.UTC(year, month - 1, day, hour, minute, second));
  const canonical = [
    String(instant.getUTCFullYear()).padStart(4, "0"),
    String(instant.getUTCMonth() + 1).padStart(2, "0"),
    String(instant.getUTCDate()).padStart(2, "0"),
    String(instant.getUTCHours()).padStart(2, "0"),
    String(instant.getUTCMinutes()).padStart(2, "0"),
    String(instant.getUTCSeconds()).padStart(2, "0"),
  ].join("");

  return canonical === value ? instant : null;
}

// load optional server-side Xweather credentials
async function loadXweatherCredentials() {
  const clientId = await loadOptionalSecret(
    process.env.WEATHER_XWEATHER_CLIENT_ID_FILE ?? "/run/secrets/weather_xweather_client_id",
  );
  const clientSecret = await loadOptionalSecret(
    process.env.WEATHER_XWEATHER_CLIENT_SECRET_FILE ?? "/run/secrets/weather_xweather_client_secret",
  );

  // disable the route only when both files are absent
  if (clientId === null && clientSecret === null) {
    return null;
  }

  // reject partial secret provisioning
  if (clientId === null || clientSecret === null) {
    throw new Error("both Xweather credential files are required");
  }

  return { clientId, clientSecret };
}

// read one private credential file without logging material
async function loadOptionalSecret(path) {
  try {
    const value = (await readFile(path, "utf8")).trim();

    // reject empty, multiline, or control-bearing credentials
    if (value.length < 4 || value.length > 256 || /[\s\u0000-\u001f\u007f]/u.test(value)) {
      throw new Error("Xweather credential file is invalid");
    }

    return value;
  } catch (error) {
    // treat only an absent file as an intentionally disabled integration
    if (error?.code === "ENOENT") {
      return null;
    }

    throw error;
  }
}

// preserve bounded head metadata
function boundedContentLength(value, maximumBytes = maximumApiBytes) {
  const length = Number(value ?? "0");

  // reject invalid or oversized metadata
  if (!Number.isSafeInteger(length) || length < 0 || length > maximumBytes) {
    throw new Error("upstream content length exceeded the edge limit");
  }

  return length;
}

// read an upstream response within the edge limit
async function readBoundedBody(upstream, maximumBytes, description) {
  const reader = upstream.body?.getReader();
  const chunks = [];
  let length = 0;

  // allow legitimate empty responses
  if (reader === undefined) {
    return Buffer.alloc(0);
  }

  // stop before buffering an oversized response
  for (;;) {
    const { done, value } = await reader.read();

    // finish on upstream EOF
    if (done) {
      break;
    }

    length += value.byteLength;

    // reject oversized API bodies
    if (length > maximumBytes) {
      await reader.cancel();
      throw new Error(`${description} response exceeded the edge limit`);
    }

    chunks.push(Buffer.from(value));
  }

  return Buffer.concat(chunks, length);
}

// apply response hardening
function setSecurityHeaders(response, analyticsEnabled = false) {
  const analyticsScripts = analyticsEnabled ? " https://www.googletagmanager.com" : "";
  const analyticsConnections = analyticsEnabled
    ? " https://www.googletagmanager.com https://*.google-analytics.com https://*.google.com"
    : "";
  const analyticsImages = analyticsEnabled ? " https://www.googletagmanager.com https://*.google-analytics.com" : "";
  response.setHeader(
    "Content-Security-Policy",
    `default-src 'self'; base-uri 'none'; connect-src 'self'${analyticsConnections}; font-src 'self'; frame-ancestors *; form-action 'self'; img-src 'self' blob: data: https://tile.openstreetmap.org https://basemap.nationalmap.gov https://imagery.nationalmap.gov${analyticsImages}; object-src 'none'; script-src 'self'${analyticsScripts}; style-src 'self'; worker-src 'self'`,
  );
  response.setHeader("Cross-Origin-Resource-Policy", "same-origin");
  response.setHeader("Referrer-Policy", "strict-origin-when-cross-origin");
  response.setHeader("X-Content-Type-Options", "nosniff");
}

// allow protected HTML documents to load in external iframes
function setHtmlSecurityHeaders(response, analyticsEnabled = false) {
  setSecurityHeaders(response, analyticsEnabled);
  response.setHeader("Cross-Origin-Resource-Policy", "cross-origin");
}

// send one bounded text response
function sendText(response, status, body, headers = {}) {
  const content = Buffer.from(body);
  setSecurityHeaders(response);
  response.writeHead(status, {
    "Cache-Control": "no-store",
    "Content-Length": String(content.byteLength),
    "Content-Type": "text/plain; charset=utf-8",
    ...headers,
  });
  response.end(content);
}

// send one bounded JSON response
function sendJson(response, status, value, head = false, headers = {}) {
  const content = Buffer.from(`${JSON.stringify(value)}\n`);
  setSecurityHeaders(response);
  response.writeHead(status, {
    "Cache-Control": "no-store",
    "Content-Length": String(content.byteLength),
    "Content-Type": "application/json; charset=utf-8",
    ...headers,
  });
  response.end(head ? undefined : content);
}

// send one same-origin browser redirect
function sendRedirect(response, status, location, headers = {}) {
  setSecurityHeaders(response);
  response.writeHead(status, {
    "Cache-Control": "no-store",
    "Content-Length": "0",
    Location: location,
    ...headers,
  });
  response.end();
}

// close without accepting new work
async function shutdown() {
  const servers = [server, adjustmentMaintenanceInternalServer]
    .filter((candidate) => candidate !== null);
  // close both public and private listeners before process exit
  await Promise.all(servers.map((candidate) => new Promise((resolveShutdown, rejectShutdown) => {
    candidate.close((error) => {
      // surface unexpected close failures
      if (error) {
        rejectShutdown(error);
      } else {
        resolveShutdown();
      }
    });
  })));
}

process.once("SIGINT", () => {
  void shutdown();
});
process.once("SIGTERM", () => {
  void shutdown();
});
