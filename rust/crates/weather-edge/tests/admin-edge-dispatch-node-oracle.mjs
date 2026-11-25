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

const functions = [
  "async function loginAdmin(request, response)",
  "function logoutAdmin(request, response)",
  "async function bootstrapAdmin(request, response)",
  "function readAdminSessionCookie(header)",
  "function createAdminSessionCookie(token, maximumAgeSeconds, request)",
  "function clearAdminSessionCookie(request)",
  "function adminSessionCookieContext(request)",
  "function requestUsesHttps(request)",
  "function sendText(response, status, body, headers = {})",
  "function sendJson(response, status, value, head = false, headers = {})",
  "function sendRedirect(response, status, location, headers = {})",
].map(sourceFunction).join("\n");

const trace = [];
const context = vm.createContext({
  Buffer,
  RangeError,
  SyntaxError,
  adminStore: {
    // retain one source-shaped normal bootstrap result
    async bootstrap(providedToken, password) {
      assert.equal(providedToken, "admitted-bootstrap");
      assert.equal(password, "admitted-password");
      trace.push("bootstrap");
      return { status: "configured" };
    },
    // retain one source-shaped normal login result
    async startSession(password) {
      assert.equal(password, "admitted-password");
      trace.push("login");
      return {
        maximumAgeSeconds: 43_200,
        token: "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
      };
    },
    // retain one admitted opaque session revocation
    revokeSession(token) {
      assert.equal(token, "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA");
      trace.push("logout");
    },
  },
  // provide one already decoded bounded form
  async readRequestForm() {
    return { password: "admitted-password" };
  },
  // provide one already decoded bounded bootstrap body
  async readRequestJson() {
    return { password: "admitted-password" };
  },
  // preserve header selection while omitting protocol security header material
  setSecurityHeaders() {},
  trace,
});

new vm.Script(`
const adminSessionCookieName = "weather_admin_session";
${functions}
this.loginAdmin = loginAdmin;
this.logoutAdmin = logoutAdmin;
this.bootstrapAdmin = bootstrapAdmin;
`).runInContext(context);

// create one transport recorder for the extracted source handlers
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

const bootstrap = responseRecorder();
await context.bootstrapAdmin({
  headers: { "x-weather-admin-bootstrap": "admitted-bootstrap" },
  method: "POST",
}, bootstrap);
assert.equal(trace.shift(), "bootstrap");
assert.equal(JSON.parse(bootstrap.body.toString("utf8")).configured, true);

const secureRequest = {
  headers: {},
  method: "POST",
  socket: { encrypted: true },
};
const login = responseRecorder();
await context.loginAdmin(secureRequest, login);
assert.equal(trace.shift(), "login");
assert.match(login.headers["Set-Cookie"], /Max-Age=43200/u);
assert.match(login.headers["Set-Cookie"], /SameSite=None; Secure; Partitioned/u);

const logout = responseRecorder();
context.logoutAdmin({
  ...secureRequest,
  headers: { cookie: "weather_admin_session=AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA" },
}, logout);
assert.equal(trace.shift(), "logout");
assert.match(logout.headers["Set-Cookie"], /Max-Age=0/u);
assert.match(logout.headers["Set-Cookie"], /SameSite=None; Secure; Partitioned/u);
assert.deepEqual(trace, []);

console.log(`bootstrap=${bootstrap.status}|${bootstrap.headers["Cache-Control"]}|configured`);
console.log(`login=${login.status}|${login.headers.Location}|${login.headers["Cache-Control"]}|secure-partitioned|43200`);
console.log(`logout=${logout.status}|${logout.headers.Location}|${logout.headers["Cache-Control"]}|secure-partitioned`);
