import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFile } from "node:fs/promises";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import vm from "node:vm";

const root = fileURLToPath(new URL("../../../../", import.meta.url));
const source = await readFile(join(root, "deploy/scripts/web-server.mjs"), "utf8");
const now = "2026-09-12T15:15:01.000Z";
const nodeRelease = "2026.11.23-1";
const nodeRevision = "a".repeat(40);
const configuredSite = { latitude: 47.950429954185445, longitude: -122.42797012608193 };

class FixedDate extends Date {
  // admit the same clock in both template implementations
  static now() {
    return Date.parse(now);
  }
}

// isolate the original executable template functions without starting a listener
function slice(startMarker, endMarker) {
  const start = source.indexOf(startMarker);
  const end = source.indexOf(endMarker, start);
  assert.ok(start >= 0 && end > start, startMarker);
  return source.slice(start, end);
}

const context = vm.createContext({
  createHash,
  Date: FixedDate,
  join,
  readFile,
  root,
  nodeRelease,
  nodeRevision,
  configuredSite,
});

const { shellAssetDigests, renderHtmlTemplate, renderAdminLoginTemplate } = await vm.runInContext(
  `(async () => {
    const release = nodeRelease;
    const webRevision = nodeRevision;
    const publicRoot = join(root, "apps/web/public");
    const compiledRoot = join(root, "apps/web/dist");
    const assetPrefix = "/assets/" + release + "/";
    const forecastMapPreloadSite = configuredSite;
    ${slice("const versionedAssets = new Map", "// expose archive mutation")}
    ${slice("function renderHtmlTemplate(", "// load only the public map center")}
    ${slice("function formatXweatherValidTime(", "// validate one cache-safe release path")}
    return { shellAssetDigests, renderHtmlTemplate, renderAdminLoginTemplate };
  })()`,
  context,
);

const publicRoot = join(root, "apps/web/public");
const [index, privacy, worker, login] = await Promise.all(
  // render the actual image-owned files instead of cloned synthetic html
  ["index.html", "privacy.html", "service-worker.js", "admin-login.html"].map(
    // fetch only one existing source template at a time
    (name) => readFile(join(publicRoot, name), "utf8"),
  ),
);
const preloadFixture = "preload: __WEATHER_ROUTE_PRELOAD__";
const fields = {
  digest_map: JSON.stringify(shellAssetDigests),
  order: Object.keys(shellAssetDigests).join(","),
  actual_public: renderHtmlTemplate(index, "/forecast", false, true),
  actual_admin: renderHtmlTemplate(index, "/forecast", true, false),
  actual_privacy: renderHtmlTemplate(privacy, "/privacy", false, false),
  actual_worker: renderHtmlTemplate(worker, "/service-worker.js", false, false),
  actual_login: renderAdminLoginTemplate(login, false),
  preload: renderHtmlTemplate(preloadFixture, "/forecast/", false, true),
  clock: String(Date.parse(now)),
  frame: "20260912151000",
};
// retain the deployed exact midpoint and signed-zero spelling for valid map centers
configuredSite.latitude = 0.0078125;
configuredSite.longitude = -0;
fields.rounded_preload = renderHtmlTemplate(preloadFixture, "/forecast/", false, true);

// retain digest values separately so Rust's fixture codec can check input bytes and output order
for (const [route, digest] of Object.entries(shellAssetDigests)) {
  fields[`digest:${route}`] = digest;
}
// use simple newline fields and hexadecimal framing for exact multi-line byte witnesses
for (const [label, value] of Object.entries(fields)) {
  process.stdout.write(`${label}=${Buffer.from(value, "utf8").toString("hex")}\n`);
}
