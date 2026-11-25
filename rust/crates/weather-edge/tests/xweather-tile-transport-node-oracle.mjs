import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import vm from "node:vm";

const source = await readFile(
  new URL("../../../../deploy/scripts/web-server.mjs", import.meta.url),
  "utf8",
);

// extract one complete named production function
function sourceFunction(signature) {
  const start = source.indexOf(signature);
  assert.notEqual(start, -1);
  const opening = source.indexOf("{", start + signature.length);
  let depth = 0;
  let quote = null;
  let escaped = false;

  // balance executable braces while retaining ordinary source strings
  for (let index = opening; index < source.length; index += 1) {
    const character = source[index];

    // ignore braces inside the active source string
    if (quote !== null) {
      // advance escaped and closing string state
      if (escaped) {
        escaped = false;
      } else if (character === "\\") {
        escaped = true;
      } else if (character === quote) {
        quote = null;
      }
      continue;
    }

    // enter one ordinary string
    if (character === '"' || character === "'" || character === "`") {
      quote = character;
      continue;
    }

    // return the first balanced function block
    if (character === "{") {
      depth += 1;
    } else if (character === "}") {
      depth -= 1;
      // stop at the function's closing brace
      if (depth === 0) {
        return source.slice(start, index + 1);
      }
    }
  }
  throw new Error(`unterminated ${signature}`);
}

const layersStart = source.indexOf("const XWEATHER_LAYERS = {");
const layersEnd = source.indexOf("const XWEATHER_FRAME_PATTERN", layersStart);
assert.notEqual(layersStart, -1);
assert.notEqual(layersEnd, -1);
const sourceLayers = source.slice(layersStart, layersEnd);

const context = vm.createContext({ Buffer, URL });
new vm.Script(`
${sourceLayers}
const maximumMapBytes = 4 * 1024 * 1024;
const xweatherCredentials = { clientId: "client id", clientSecret: "secret/slash" };
const xweatherOrigin = new URL("https://maps.api.xweather.com/");
let mode = "success";
let trace = [];
const process = { stderr: { write(value) { trace.push("diagnostic:" + value.trimEnd()); } } };
const AbortSignal = {
  timeout(value) { trace.push("timeout:" + String(value)); return { value }; },
};
function reserveXweatherMapUnits(tile) {
  trace.push("reserve:" + String(Math.ceil(tile.width / 256) * Math.ceil(tile.height / 256)));
}
function upstreamResponse() {
  const status = mode === "invalid" ? 503 : 200;
  const contentType = mode === "invalid" ? " Application/JSON ; charset=utf-8" : "image/png";
  const chunks = mode === "oversize"
    ? [Buffer.alloc(maximumMapBytes, 1), Buffer.from([2])]
    : [Buffer.from([137, 80]), Buffer.from([78, 71])];
  return {
    body: {
      getReader() {
        return {
          async cancel() { trace.push("cancel"); },
          async read() {
            trace.push("read");
            return chunks.length === 0
              ? { done: true, value: undefined }
              : { done: false, value: chunks.shift() };
          },
        };
      },
    },
    headers: {
      get(name) {
        if (name === "content-type") return contentType;
        if (name === "retry-after") return mode === "invalid" ? "60.5" : null;
        return null;
      },
    },
    ok: status >= 200 && status < 300,
    status,
  };
}
async function fetch(target, options) {
  trace.push([
    "fetch", options.method, options.redirect, options.headers.Accept,
    options.headers["User-Agent"], target.href,
  ].join("|"));
  if (mode === "transport") throw new TypeError("opaque fixture transport error");
  return upstreamResponse();
}
${sourceFunction("function xweatherDiagnosticContentType(value)")}
${sourceFunction("async function readBoundedBody(upstream, maximumBytes, description)")}
${sourceFunction("async function fetchXweatherTile(tile)")}
this.run = async (nextMode, tile) => {
  mode = nextMode;
  trace = [];
  try {
    const result = await fetchXweatherTile(tile);
    return { body: result.body, error: null, retryAfterMs: null, trace };
  } catch (error) {
    return {
      body: null,
      error: error.constructor.name + ":" + error.message,
      retryAfterMs: error.retryAfterMs ?? null,
      trace,
    };
  }
};
`).runInContext(context);

const forecastWind = {
  height: 168,
  kind: "frame",
  latitude: 56.123456,
  layer: "wind",
  longitude: -3.654321,
  phase: "forecast",
  validTime: "20261123160000",
  width: 256,
  zoom: 10,
};
const historyRadar = {
  ...forecastWind,
  layer: "radar",
  phase: "history",
  validTime: "20261123155000",
};

const success = await context.run("success", forecastWind);
assert.deepEqual([...success.body], [137, 80, 78, 71]);
assert.deepEqual([...success.trace.slice(0, 3)], [
  "reserve:1",
  "timeout:8000",
  "fetch|GET|follow|image/png|Ballydidean-Weather/1.0|https://maps.api.xweather.com/client%20id_secret%2Fslash/fwind-speeds/256x168/56.123456,-3.654321,10/20261123160000.png",
]);

const history = await context.run("success", historyRadar);
assert.ok(history.trace[2].includes("/radar/256x168/56.123456,-3.654321,10/20261123155000.png"));

const invalid = await context.run("invalid", forecastWind);
assert.equal(invalid.error, "Error:Xweather tile provider returned an invalid response");
assert.equal(invalid.retryAfterMs, 60_000);
assert.ok(invalid.trace.includes(
  "diagnostic:Xweather tile response rejected: status=503 content-type=application/json",
));
assert.ok(!invalid.trace.includes("read"));

const transport = await context.run("transport", forecastWind);
assert.equal(transport.error, "TypeError:opaque fixture transport error");
assert.ok(transport.trace.includes("diagnostic:Xweather tile fetch failed: error=TypeError"));

const oversize = await context.run("oversize", forecastWind);
assert.equal(oversize.error, "Error:weather map response exceeded the edge limit");
assert.ok(oversize.trace.includes("cancel"));

process.stdout.write(
  [
    `success=${success.body.length}|GET|follow|image/png|8000|forecast:fwind-speeds`,
    "history=radar|valid-time:20261123155000",
    `invalid=503|application/json|retry:${String(invalid.retryAfterMs)}|unread`,
    "transport=TypeError|safe-diagnostic",
    "oversize=4194305|cancelled",
  ].join("\n") + "\n",
);
