import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import vm from "node:vm";

const repositoryRoot = new URL("../../../../", import.meta.url);
const source = await readFile(
  new URL("deploy/scripts/weather-admin-store.mjs", repositoryRoot),
  "utf8",
);

// extract one complete source class method without rewriting its behavior
function sourceMethod(signature) {
  const start = source.indexOf(signature);
  assert.notEqual(start, -1);
  const opening = source.indexOf("{", start);
  let depth = 0;
  let quote = null;
  let escaped = false;

  // balance executable blocks while ignoring string contents
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
    if (character === "\"" || character === "'" || character === "`") {
      quote = character;
      continue;
    }

    // return the first balanced complete method
    if (character === "{") {
      depth += 1;
    } else if (character === "}") {
      depth -= 1;
      if (depth === 0) {
        return source.slice(start, index + 1);
      }
    }
  }

  throw new Error(`unterminated source method: ${signature}`);
}

const methods = [
  "async bootstrap(providedToken, password)",
  "async startSession(password)",
  "authenticateSession(token)",
  "revokeSession(token)",
  "async #authenticateCredentials(password)",
  "\n  #pruneSessions(now)",
].map(sourceMethod).join("\n");

const trace = [];
let authState = null;
let now = 1_778_065_200_000;
let tokenOrdinal = 0;

const context = vm.createContext({
  Buffer,
  Error,
  Map,
  RangeError,
  trace,
  // persist one already admitted typed auth state
  async atomicWriteJson(_path, state) {
    trace.push("write-auth");
    authState = structuredClone(state);
  },
  // admit the already verified credential derivation result
  async derivePasswordHash() {
    trace.push("derive");
    return {
      byteLength: 64,
      toString() {
        return "admitted-verifier";
      },
    };
  },
  // preserve the source wall-clock read position
  fixedNow() {
    trace.push("clock");
    return now;
  },
  // expose only the fixture auth state through the persistent port
  async readOptionalJson() {
    trace.push("read-auth");
    return authState === null ? null : structuredClone(authState);
  },
  // expose one already admitted bootstrap token setting
  async readOptionalText() {
    trace.push("read-bootstrap");
    return "admitted-bootstrap-token-with-32-bytes";
  },
  // retain a source-shaped parsed auth state without implementing a codec
  parseAuthState(state) {
    return {
      hash: { byteLength: 64 },
      salt: state.salt,
    };
  },
  // inject one salt or admitted opaque session token
  randomBytes(size) {
    if (size === 16) {
      trace.push("salt");
      return { toString: () => "admitted-salt" };
    }
    trace.push("token");
    tokenOrdinal += 1;
    const token = String(tokenOrdinal).padStart(43, "A");
    return { toString: () => token };
  },
  // retain the already admitted secret form
  requireSecret(value) {
    return value;
  },
  // inject the verified bootstrap-token comparison result
  secureTextEqual() {
    trace.push("bootstrap-credential");
    return true;
  },
  // inject one already admitted token digest without hashing here
  sessionTokenDigest(token) {
    trace.push("digest");
    return `admitted-digest-${token}`;
  },
  structuredClone,
  // inject the verified credential comparison result
  timingSafeEqual() {
    trace.push("credential");
    return true;
  },
});

new vm.Script(`
const AUTH_VERSION = 1;
const ADMIN_SESSION_TTL_SECONDS = 12 * 60 * 60;
const MAXIMUM_ADMIN_SESSIONS = 64;
const SESSION_TOKEN_PATTERN = /^[A-Za-z0-9_-]{43}$/u;
class OracleWeatherAdminStore {
  #authPath = "auth";
  #bootstrapTokenPath = "bootstrap";
  #now = fixedNow;
  #sessions = new Map();
  ${methods}
}
this.OracleWeatherAdminStore = OracleWeatherAdminStore;
`).runInContext(context);

const store = new context.OracleWeatherAdminStore();
const configured = await store.bootstrap(
  "admitted-bootstrap-token-with-32-bytes",
  "admitted-password",
);
const bootstrapOrder = trace.join(",");
trace.length = 0;

const first = await store.startSession("admitted-password");
const loginOrder = trace.join(",");
trace.length = 0;
const firstRead = store.authenticateSession(first.token);
const readOrder = trace.join(",");
trace.length = 0;

// fill the bounded registry once beyond its retained capacity
let latest = first;
for (let index = 0; index < 64; index += 1) {
  latest = await store.startSession("admitted-password");
}
trace.length = 0;
const firstAfterCap = store.authenticateSession(first.token);
const latestAfterCap = store.authenticateSession(latest.token);

now += 12 * 60 * 60 * 1_000;
trace.length = 0;
const expiredLatest = store.authenticateSession(latest.token);

const revocable = await store.startSession("admitted-password");
trace.length = 0;
store.revokeSession(revocable.token);
const logoutOrder = trace.join(",");
trace.length = 0;
const afterLogout = store.authenticateSession(revocable.token);

assert.equal(configured.status, "configured");
assert.equal(first.maximumAgeSeconds, 43_200);
assert.equal(firstRead, true);
assert.equal(firstAfterCap, false);
assert.equal(latestAfterCap, true);
assert.equal(expiredLatest, false);
assert.equal(afterLogout, false);

process.stdout.write(`bootstrap=${configured.status}:${bootstrapOrder}\n`);
process.stdout.write(`login=${first.maximumAgeSeconds}:${loginOrder}\n`);
process.stdout.write(`read=${String(firstRead)}:${readOrder}\n`);
process.stdout.write(`cap=${String(firstAfterCap)}:${String(latestAfterCap)}\n`);
process.stdout.write(`expired=${String(expiredLatest)}\n`);
process.stdout.write(`logout=${String(afterLogout)}:${logoutOrder}\n`);
