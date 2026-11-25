import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import vm from "node:vm";

const source = await readFile(
  new URL("../../../../deploy/scripts/web-server.mjs", import.meta.url),
  "utf8",
);

// extract one complete production function without rewriting its behavior
function sourceFunction(signature) {
  const start = source.indexOf(signature);
  assert.notEqual(start, -1, `missing source function: ${signature}`);
  const opening = source.indexOf("{", start + signature.length);
  let depth = 0;
  let escaped = false;
  let quote = null;

  // balance executable blocks while ignoring source string contents
  for (let index = opening; index < source.length; index += 1) {
    const character = source[index];

    // advance one active source string
    if (quote !== null) {
      if (escaped) {
        escaped = false;
      } else if (character === "\\") {
        escaped = true;
      } else if (character === quote) {
        quote = null;
      }
      continue;
    }

    // enter one ordinary source string
    if (character === '"' || character === "'" || character === "`") {
      quote = character;
      continue;
    }

    // return the first balanced complete function
    if (character === "{") {
      depth += 1;
    } else if (character === "}") {
      depth -= 1;
      if (depth === 0) {
        return source.slice(start, index + 1);
      }
    }
  }

  throw new Error(`unterminated source function: ${signature}`);
}

const redirectSource = sourceFunction(
  "function redirectRemoteAgentsBrowser(response, requestUrl)",
);
const context = vm.createContext({
  URL,
  // valid fixtures never select the text error adapter
  sendText() {
    throw new Error("unexpected preview rejection");
  },
  // retain response policy without implementing HTTP security headers
  setSecurityHeaders() {},
});

new vm.Script(`${redirectSource}\nthis.redirectRemoteAgentsBrowser = redirectRemoteAgentsBrowser;`)
  .runInContext(context);

// execute one admitted source URL through the production redirect function
function redirect(url) {
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
  context.redirectRemoteAgentsBrowser(
    response,
    new URL(url, "http://weather.invalid"),
  );
  assert.equal(response.body, undefined);
  return response;
}

// render the transport-independent response fields retained by Rust
function signature(response) {
  return [
    response.status,
    response.headers["Cache-Control"],
    response.headers["Content-Length"],
    response.headers.Location,
  ].join("|");
}

const desktop = redirect(
  "/__rac/browser-device?mode=desktop&location=%2Fforecast%3Fdays%3D2%23hour-1",
);
const mobile = redirect(
  "/__rac/browser-device?mode=mobile&location=%2Fmap%2F%3Flayer%3Drain%23now",
);

console.log(`desktop=${signature(desktop)}`);
console.log(`mobile=${signature(mobile)}`);
