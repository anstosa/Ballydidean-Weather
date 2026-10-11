import { createHash } from "node:crypto";
import { once } from "node:events";
import { readFileSync } from "node:fs";
import { createServer } from "node:http";
import { join, resolve } from "node:path";

const root = resolve(import.meta.dirname, "../../..");
const publicRoot = join(root, "apps/web/public");
const compiledRoot = join(root, "apps/web/dist");
export const androidWebViewAgent = "Mozilla/5.0 (Linux; Android 16; Pixel 4 Build/AP; wv) AppleWebKit/537.36 (KHTML, like Gecko) Version/4.0 Chrome/140.0.0.0 Mobile Safari/537.36";

// run real release transitions against the compiled updater and worker
export async function startHostedUpdateFixture({ legacy = false } = {}) {
  let release = "2026.10.11-1";
  let corrupt = false;
  let downloadGate = null;
  let releaseDownload = null;
  const workerSource = readFileSync(join(publicRoot, "service-worker.js"), "utf8");
  const paths = [...workerSource.matchAll(/`\/assets\/\$\{release\}\/([^`]+)`/gu)].map(
    // derive the exact production module allowlist
    (match) => match[1],
  );
  const requests = [];

  // serve the real updater with a tiny observable hosted application
  function assetBody(name, version) {
    // isolate update behavior from unrelated weather API fixtures
    if (name === "client.js") {
      return Buffer.from(`import { registerHostedUpdates } from './hosted-updates.js';
        document.getElementById('unit').textContent = localStorage.getItem('unit') ?? 'Fahrenheit';
        document.getElementById('celsius').addEventListener('click', () => {
          localStorage.setItem('unit', 'Celsius'); document.getElementById('unit').textContent = 'Celsius';
        });
        ${legacy && version === "2026.10.11-1"
          ? "navigator.serviceWorker.register('/service-worker.js?release=2026.10.11-1', { scope: '/', updateViaCache: 'none' }).then((registration) => registration.update()).catch(() => {});"
          : "registerHostedUpdates().catch(() => {});"}
        document.body.dataset.booted = '${version}';`);
    }
    return readFileSync(join(name === "styles.css" ? publicRoot : compiledRoot, name));
  }

  // bind the exact expected executable bytes to each fixture release
  function digests(version) {
    return Object.fromEntries(paths.map(
      // publish the production subresource-integrity format
      (name) => [`/assets/${version}/${name}`, `sha256-${createHash("sha256").update(assetBody(name, version)).digest("base64")}`],
    ));
  }

  const server = createServer(async (request, response) => {
    const path = new URL(request.url, "http://fixture.invalid").pathname;
    const version = release;
    requests.push({ path, version, cookie: request.headers.cookie ?? null });
    response.setHeader("cache-control", "no-store");
    // render the byte-distinct stable worker with image-owned integrity metadata
    if (path === "/service-worker.js") {
      response.setHeader("content-type", "text/javascript");
      response.end(workerSource.replaceAll("__WEATHER_ASSET_VERSION__", version)
        .replaceAll("__WEATHER_SHELL_ASSET_DIGESTS__", JSON.stringify(digests(version))));
      return;
    }
    // serve only the active immutable asset release
    if (path.startsWith(`/assets/${version}/`)) {
      const name = path.slice(`/assets/${version}/`.length);
      // reject names outside the reviewed shell
      if (!paths.includes(name)) {
        response.writeHead(404).end();
        return;
      }
      // pause one large asset to make real download progress observable
      if (name === "index.js" && downloadGate !== null) {
        await downloadGate;
      }
      response.setHeader("content-type", name.endsWith(".js") ? "text/javascript" : "text/css");
      response.setHeader("cache-control", "public, max-age=31536000, immutable");
      response.end(corrupt && name === "index.js" ? "export const corrupted = true;" : assetBody(name, version));
      return;
    }
    // retain the production same-origin artwork and font paths
    if (path.startsWith("/brand/") || path.startsWith("/weather-icons/") || path.startsWith("/fonts/") || path === "/manifest.webmanifest") {
      try {
        response.setHeader("content-type", path.endsWith(".svg") ? "image/svg+xml" : "application/octet-stream");
        response.end(readFileSync(join(publicRoot, path.slice(1))));
      } catch {
        response.writeHead(404).end();
      }
      return;
    }
    // leave live api data off the shell cache
    if (path.startsWith("/api/")) {
      response.writeHead(503).end();
      return;
    }
    response.setHeader("content-type", "text/html");
    response.setHeader("cache-control", "no-cache");
    response.end(`<!doctype html><html><head><meta name="viewport" content="width=device-width, initial-scale=1"><meta name="weather-release" content="${version}"><link rel="stylesheet" href="/assets/${version}/styles.css"></head><body>
      <main class="shell"><h1>Release ${version}</h1><p id="unit"></p><button id="celsius">Use Celsius</button><a href="/forecast">Forecast</a></main>
      <script type="module" src="/assets/${version}/client.js"></script></body></html>`);
  });
  server.listen(0, "127.0.0.1");
  await once(server, "listening");
  return {
    origin: `http://127.0.0.1:${server.address().port}`,
    requests,
    // select one next immutable release without reloading the current page
    update(next, { corruptAsset = false, pause = false } = {}) {
      release = next;
      corrupt = corruptAsset;
      // stage one controlled transport delay
      if (pause) {
        downloadGate = new Promise((resolveDownload) => { releaseDownload = resolveDownload; });
      }
    },
    // release the held asset without changing its expected bytes
    finishDownload() {
      releaseDownload?.();
      downloadGate = null;
      releaseDownload = null;
    },
    // stop only this fixture's disposable listener
    async close() {
      releaseDownload?.();
      server.close();
      await once(server, "close");
    },
  };
}
