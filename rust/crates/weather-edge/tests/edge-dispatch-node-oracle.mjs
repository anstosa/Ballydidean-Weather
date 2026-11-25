import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import vm from "node:vm";

const source = await readFile(
  new URL("../../../../deploy/scripts/web-server.mjs", import.meta.url),
  "utf8",
);
const start = source.indexOf("const server = createServer(async (request, response) => {");
const end = source.indexOf("\n\nserver.listen(", start);

assert.notEqual(start, -1, "production public server callback is present");
assert.notEqual(end, -1, "production public server callback is bounded");

const priorityMarkers = [
  'requestUrl.pathname === "/api/v1/sites/ballydidean/property-sensor-layout"',
  'requestUrl.pathname === "/api/v1/sites/ballydidean/forecast-adjustment-settings"',
  'requestUrl.pathname === "/api/v1/admin/sites/ballydidean/forecast-adjustment-settings"',
  "isWidgetForecastPath(requestUrl.pathname)",
  'requestUrl.pathname === "/api/v1/sites/ballydidean/forecast-observations"',
  'requestUrl.pathname.startsWith("/api/v1/")',
  'requestUrl.pathname.startsWith("/maps/xweather/")',
  "const asset = resolveAsset(requestUrl.pathname)",
];
let previousIndex = -1;

// prove the source callback keeps every specialized route before its broader fallback
for (const marker of priorityMarkers) {
  const markerIndex = source.indexOf(marker, start);
  assert.ok(markerIndex > previousIndex && markerIndex < end, `route priority marker: ${marker}`);
  previousIndex = markerIndex;
}

const calls = [];
const context = vm.createContext({
  Buffer,
  JSON,
  URL,
  adminStore: {
    // admit the ordinary authenticated administrator representation
    authenticateSession: () => {
      calls.push("admin-auth");
      return true;
    },
  },
  bootstrapAdmin: async () => calls.push("bootstrap"),
  createServer: (handler) => ({ handler }),
  forecastObservationCache: {
    // mark the real recorded route branch before returning its source-shaped object
    get: async () => {
      calls.push("recorded");
      return {
        data: [],
        generatedAt: "2026-11-23T12:00:00.000Z",
        solarCloudEvidence: {},
      };
    },
  },
  homeNetworkMatcher: {
    // mark the actual inline viewer branch before returning admitted eligibility
    matches: async () => {
      calls.push("viewer");
      return true;
    },
  },
  isWidgetForecastPath: (pathname) => pathname === "/api/v3/sites/ballydidean/widget-forecast",
  loginAdmin: async () => calls.push("login"),
  logoutAdmin: () => calls.push("logout"),
  productionAnalytics: false,
  proxyApi: async () => calls.push("api"),
  proxyXweatherMap: async () => calls.push("map"),
  readAdminSessionCookie: () => null,
  // retain the static branch without touching the host filesystem
  readFile: async (path) => {
    calls.push(path === "admin-index" ? "admin-read" : "static");
    return Buffer.from(path === "admin-index" ? "admin template" : "static fixture");
  },
  redirectRemoteAgentsBrowser: () => calls.push("preview"),
  renderHtmlTemplate: (body, pathname, isAdmin, analyticsEnabled) => {
    calls.push(`admin-render:${String(isAdmin)}:${String(analyticsEnabled)}`);
    assert.equal(pathname, "/admin");
    return body;
  },
  resolveAsset: (pathname) => pathname === "/admin"
    ? {
        analytics: true,
        cache: "no-cache",
        path: "admin-index",
        template: true,
        type: "text/html; charset=utf-8",
      }
    : {
        analytics: true,
        cache: "public, max-age=86400",
        path: "fixture",
        template: false,
        type: "image/svg+xml",
      },
  sendAdminLoginPage: async () => calls.push("admin-page"),
  sendText: (_response, status) => calls.push(`text:${status}`),
  serveForecastAdjustmentScorecard: async () => calls.push("scorecard"),
  serveForecastAdjustmentSettings: async (_request, _response, admin) => {
    calls.push(`settings:${admin ? "admin" : "public"}`);
  },
  servePropertySensorLayout: async () => calls.push("layout"),
  serveWidgetForecast: async () => calls.push("widget"),
  setHtmlSecurityHeaders: () => {},
  setSecurityHeaders: () => {},
  updatePropertySensorLayout: async () => calls.push("layout-update"),
});

vm.runInContext(`${source.slice(start, end)}\nthis.__handler = server.handler;`, context);

// run one ordinary request through the actual extracted production callback
async function selected(pathname, method = "GET", expectedCallCount = 1) {
  calls.length = 0;
  const response = {
    body: undefined,
    headers: undefined,
    status: undefined,
    end(body) {
      this.body = body;
    },
    writeHead(status, headers) {
      this.status = status;
      this.headers = headers;
    },
  };
  await context.__handler(
    { headers: {}, method, url: pathname },
    response,
  );
  assert.equal(calls.length, expectedCallCount, `route owner trace for ${pathname}`);
  return { call: calls[0], response, trace: [...calls] };
}

// return only the selected route owner for ordinary priority assertions
async function selectedCall(pathname, method = "GET") {
  return (await selected(pathname, method)).call;
}

console.log(
  `public-settings=${await selectedCall("/api/v1/sites/ballydidean/forecast-adjustment-settings")}`,
);
console.log(
  `admin-settings=${await selectedCall("/api/v1/admin/sites/ballydidean/forecast-adjustment-settings", "HEAD")}`,
);
console.log(`widget=${await selectedCall("/api/v3/sites/ballydidean/widget-forecast", "HEAD")}`);
console.log(
  `recorded=${await selectedCall("/api/v1/sites/ballydidean/forecast-observations")}`,
);
console.log(`api=${await selectedCall("/api/v1/sites/ballydidean/current")}`);
console.log(`map=${await selectedCall("/maps/xweather/history/clouds/frame.png")}`);
console.log(`static=${await selectedCall("/brand/favicon.svg", "HEAD")}`);

const viewerGet = await selected("/api/v1/viewer-context");
assert.equal(JSON.parse(viewerGet.response.body).data.homeNetwork, true);
console.log(
  `viewer-get=${viewerGet.call}|${viewerGet.response.status}|${viewerGet.response.headers["Cache-Control"]}|body`,
);
const viewerHead = await selected("/api/v1/viewer-context", "HEAD");
assert.equal(viewerHead.response.body, undefined);
console.log(
  `viewer-head=${viewerHead.call}|${viewerHead.response.status}|${viewerHead.response.headers["Cache-Control"]}|none`,
);
console.log(
  `property-update=${await selectedCall("/api/v1/admin/sites/ballydidean/property-sensor-layout/soil-1", "PUT")}`,
);
console.log(
  `preview=${await selectedCall("/__rac/browser-device?mode=desktop&location=%2Fforecast")}`,
);
const adminPageGet = await selected("/admin", "GET", 3);
console.log(
  `admin-page-get=${adminPageGet.trace.join(",")}|${adminPageGet.response.status}|${adminPageGet.response.headers["Cache-Control"]}|${adminPageGet.response.headers.Vary}|body`,
);
const adminPageHead = await selected("/admin", "HEAD", 3);
assert.equal(adminPageHead.response.body, undefined);
console.log(
  `admin-page-head=${adminPageHead.trace.join(",")}|${adminPageHead.response.status}|${adminPageHead.response.headers["Cache-Control"]}|${adminPageHead.response.headers.Vary}|none`,
);
