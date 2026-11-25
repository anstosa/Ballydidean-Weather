// Observe the deployed web entrypoint's configuration and listener ordering without opening sockets.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join, resolve } from "node:path";

const source = readFileSync(new URL("../../../../deploy/scripts/web-server.mjs", import.meta.url), "utf8");
const evidenceSource = readFileSync(new URL("../../../../deploy/scripts/adjustment-evidence-store.mjs", import.meta.url), "utf8");
const root = "/fixture/weather";
const defaultEvidenceRoot = evidenceSource.match(
  /export const ADJUSTMENT_EVIDENCE_DEFAULT_ROOT =\s*"([^"]+)";/u,
)?.[1];
assert.ok(defaultEvidenceRoot);

// extract exact original statements without executing the real listener module
function section(start, end) {
  const left = source.indexOf(start);
  const right = source.indexOf(end, left);
  assert.ok(left >= 0 && right > left, start);
  return source.slice(left, right);
}

const parsers = [
  section("function parseAssetRelease(value)", "// read the database-authenticated"),
  section("function parseXweatherOrigin(value)", "// issue one process-tracked"),
].join("\n");
const bootSource = section("const adjustmentEvidenceRoot =", "const assetPrefix");
const listenSource = section('server.listen(port, "0.0.0.0",', "// dispatch one exact private revision archive request");
const AsyncFunction = Object.getPrototypeOf(async function () {}).constructor;

const boot = new AsyncFunction(
  "process", "resolve", "join", "root", "ADJUSTMENT_EVIDENCE_DEFAULT_ROOT",
  "ForecastObservationCache", "loadXweatherCredentials", "HomeNetworkMatcher",
  "loadForecastMapPreloadSite", "WeatherAdminStore", "AdjustmentEvidenceStore",
  "createAdjustmentRevisionArchiveHandler", "AdjustmentRevisionArchiveStore",
  "parseAdjustmentRevisionArchiveProjection", "parseRainMaintenanceControlState",
  "validateAdjustmentShadowRevisionStage", "validateAdjustmentShadowRevisionCapsule",
  "AdjustmentEvidenceArchiveTransport", "AdjustmentEvidenceScheduler",
  "readAdjustmentMaintenanceScheduleSlots", "triggerScheduledAdjustmentCapture",
  "server", "adjustmentMaintenanceInternalServer", "flushReady",
  `${parsers}
    ${bootSource}
    ${listenSource}
    flushReady();
    return { adjustmentEvidenceRoot, apiOrigin: apiOrigin.origin,
      maintenanceApiOrigin: adjustmentMaintenanceApiOrigin?.origin ?? null,
      xweatherOrigin: xweatherOrigin.origin, port, adjustmentMaintenanceInternalPort,
      release, webRevision, productionAnalytics, scheduledCaptureEnabled,
      scheduledCaptureMigrationReady, scheduledArchiveTransferEnabled,
      archiveReady: adjustmentEvidenceArchiveTransport !== null };
  `,
);

// inject only non-network owners while preserving original top-level source order
async function observe(env, { site = null, refuseArchive = false } = {}) {
  const trace = [];
  let ready;
  let sitePath;
  let adminPaths;
  // retain constructor/init calls as observed by the source driver
  const record = (name) => class {
    constructor(value) { trace.push([name, value?.root ?? null]); }
    async initialize() { trace.push([`${name}:initialize`]); return this; }
  };
  class Admin {
    constructor(value) {
      trace.push(["admin"]);
      adminPaths = value;
    }
  }
  class Archive extends record("archive") {
    async initialize() {
      await super.initialize();
      // model one ordinary archive startup fault without blocking public serving
      if (refuseArchive) throw new Error("disposable archive unavailable");
      return this;
    }
  }
  class Scheduler {
    constructor({ root: path, enabled, migrationReady }) {
      trace.push(["scheduler", path, enabled, migrationReady]);
    }
    async initialize() { trace.push(["scheduler:initialize"]); return this; }
    start() { trace.push(["scheduler:start"]); }
  }
  const server = {
    // observe submission and defer readiness until both listen requests are visible
    listen(port, host, atReady) { trace.push(["public:listen", port, host]); ready = atReady; },
  };
  const privateServer = env.WEATHER_ADJUSTMENT_MAINTENANCE_INTERNAL_PORT === undefined ? null : {
    listen(port, host) { trace.push(["private:listen", port, host]); },
  };
  const noOp = () => undefined;
  const config = await boot(
    { env }, (...parts) => resolve(root, ...parts), join, root, defaultEvidenceRoot,
    record("observations"),
    async () => { trace.push(["provider:credentials"]); return null; },
    record("home"),
    async (path) => { trace.push(["site"]); sitePath = path; return site; },
    Admin, record("evidence"),
    () => { trace.push(["revision:handler"]); return {}; }, record("revision"),
    noOp, noOp, noOp, noOp, Archive, Scheduler, noOp, noOp, server, privateServer,
    () => { trace.push(["public:ready"]); ready(); },
  );
  return { config, sitePath, adminPaths, trace };
}

const production = {
  NODE_ENV: "production", PORT: "4100", WEATHER_RELEASE: "2026.10.08-3",
  WEATHER_WEB_REVISION: "a".repeat(40),
  WEATHER_API_ORIGIN: "http://api:4101",
  WEATHER_ADJUSTMENT_MAINTENANCE_API_ORIGIN: "http://api:9800",
  WEATHER_ADJUSTMENT_MAINTENANCE_INTERNAL_PORT: "9801",
  WEATHER_ADJUSTMENT_EVIDENCE_ROOT: "evidence",
  WEATHER_ADJUSTMENT_SCHEDULED_CAPTURE_ENABLED: "true",
  WEATHER_ADJUSTMENT_MAINTENANCE_MIGRATION_READY: "true",
  WEATHER_ADJUSTMENT_ARCHIVE_TRANSFER_ENABLED: "true",
  WEATHER_SITE_CONFIG_PATH: "/fixture/site.json",
  WEATHER_ADMIN_AUTH_PATH: "/fixture/admin.json",
  WEATHER_ADMIN_BOOTSTRAP_TOKEN_PATH: "/fixture/bootstrap-file",
  WEATHER_PROPERTY_SENSOR_LAYOUT_PATH: "/fixture/layout.json",
  WEATHER_XWEATHER_MAP_ORIGIN: "https://maps.api.xweather.com",
};
const development = await observe({});
const active = await observe(production, { site: { latitude: 48.2, longitude: -122.1 } });
const archiveRefused = await observe(production, { refuseArchive: true });
assert.equal(development.config.port, 3000);
assert.equal(development.config.archiveReady, false);
assert.equal(active.config.archiveReady, true);
assert.equal(archiveRefused.config.archiveReady, false);
assert.deepEqual(active.trace.slice(-4).map(([name]) => name),
  ["public:listen", "private:listen", "public:ready", "scheduler:start"]);

// render a restricted fixture frame so the Rust test needs no test-only JSON dependency
if (process.argv[2] === "--tsv") {
  const visible = new Map([
    ["site", "site"], ["admin", "admin"], ["evidence:initialize", "evidence"],
    ["revision:handler", "revision"], ["archive:initialize", "archive"],
    ["scheduler:initialize", "scheduler"], ["public:listen", "public:listen"],
    ["private:listen", "private:listen"], ["public:ready", "public:ready"],
    ["scheduler:start", "scheduler:start"],
  ]);
  for (const [name, observed] of Object.entries({ development, active, archiveRefused })) {
    const fields = {
      ...observed.config,
      sitePath: observed.sitePath,
      adminAuthPath: observed.adminPaths.authPath,
      adminBootstrapPath: observed.adminPaths.bootstrapTokenPath,
      layoutPath: observed.adminPaths.layoutPath,
      startupOrder: observed.trace.filter(([event]) => visible.has(event)).map(
        ([event, ...values]) => [visible.get(event), ...values].join(":"),
      ).join(","),
    };
    for (const [key, value] of Object.entries(fields)) {
      process.stdout.write(`${name}.${key}\t${String(value)}\n`);
    }
  }
  // use only ordinary supported numeric environment spellings
  for (const [key, value] of Object.entries({
    decimal: "3001", hex: "0x0bb9", octal: "0o5671", binary: "0b101110111001",
    exponent: "3.001e3", whitespace: "\uFEFF3001\u00A0",
  })) {
    const result = await observe({ PORT: value });
    process.stdout.write(`port.${key}\t${result.config.port}\n`);
  }
} else {
  process.stdout.write(JSON.stringify({ development, active, archiveRefused }));
}
