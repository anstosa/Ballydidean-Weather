import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import vm from "node:vm";

const repositoryRoot = new URL("../../../../", import.meta.url);
const source = await readFile(
  new URL("deploy/scripts/web-server.mjs", repositoryRoot),
  "utf8",
);
const template = await readFile(
  new URL("apps/web/public/admin-login.html", repositoryRoot),
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

const functions = [
  "function renderAdminLoginTemplate(source, invalidCredentials)",
  "async function sendAdminLoginPage(response, head, invalidCredentials)",
  "async function bootstrapAdmin(request, response)",
  "function sendText(response, status, body, headers = {})",
].map(sourceFunction).join("\n");
const context = vm.createContext({
  Buffer,
  // return the real source login template through the admitted asset port
  async readFile() {
    return template;
  },
  // retain response metadata without implementing HTTP
  setHtmlSecurityHeaders() {},
  setSecurityHeaders() {},
});

new vm.Script(`
const adminLoginPath = "fixture-admin-login.html";
const release = "2026.11.23-1";
${functions}
this.renderAdminLoginTemplate = renderAdminLoginTemplate;
this.sendAdminLoginPage = sendAdminLoginPage;
this.bootstrapAdmin = bootstrapAdmin;
`).runInContext(context);

const normal = context.renderAdminLoginTemplate(template, false);
assert.match(normal, /\/assets\/2026\.11\.23-1\/styles\.css/u);
assert.doesNotMatch(normal, /__WEATHER_ASSET_VERSION__|__WEATHER_ADMIN_LOGIN_ERROR__/u);
assert.doesNotMatch(normal, /admin-login-error/u);

// capture one ordinary typed response from the extracted bootstrap handler
function responseRecorder() {
  return {
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
}

// execute one read method through the actual bootstrap method gate
async function bootstrapRead(method) {
  const response = responseRecorder();
  await context.bootstrapAdmin({ method }, response);
  return response;
}

const bootstrapGet = await bootstrapRead("GET");
const bootstrapHead = await bootstrapRead("HEAD");
const loginGet = responseRecorder();
await context.sendAdminLoginPage(loginGet, false, false);
const loginHead = responseRecorder();
await context.sendAdminLoginPage(loginHead, true, false);

// render one stable response signature without retaining protocol objects
function bootstrapSignature(response) {
  return [
    response.status,
    response.headers.Allow,
    response.headers["Cache-Control"],
    response.body.toString("utf8").trim(),
  ].join("|");
}

console.log(`login-normal-length=${Buffer.byteLength(normal)}`);
console.log(`login-get=${loginGet.status}|${loginGet.headers["Cache-Control"]}|${loginGet.headers["Content-Type"]}|${loginGet.headers.Vary}|${loginGet.headers["Content-Length"]}|body`);
console.log(`login-head=${loginHead.status}|${loginHead.headers["Cache-Control"]}|${loginHead.headers["Content-Type"]}|${loginHead.headers.Vary}|${loginHead.headers["Content-Length"]}|none`);
console.log(`bootstrap-get=${bootstrapSignature(bootstrapGet)}`);
console.log(`bootstrap-head=${bootstrapSignature(bootstrapHead)}`);
