import { createHash } from "node:crypto";
import { constants, readFileSync } from "node:fs";
import { lstat, mkdir, open, readdir, statfs, writeFile } from "node:fs/promises";
import { isAbsolute, resolve, sep } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

import {
  buildBackendHostObservation,
  buildRetainedBackendHostObservationV1,
  canonicalHostJson,
  HOST_EVIDENCE_CONTRACT_VERSION,
  HOST_EVIDENCE_REQUEST_VERSION,
  HOST_OBSERVATION_VERSION,
  HOST_PRODUCER,
} from "./backend-host-observation.mjs";
import {
  validatePublication,
  validateStageReceipt,
} from "./schema-preserving-release.mjs";

export const LIVE_STATUS_CONTRACT_VERSION = "weather-backend-live-status/v2";
export const LIVE_STATUS_MAX_AGE_MS = 600_000;

const INVENTORY_VERSION = "backend-runtime-inventory/v1";
const RUNTIME_MAP_VERSION = "weather-backend-runtime-live-map/v1";
const KNOWN_HOSTS_SHA256 = "be8b84c8c3599bd2f0e4eaaa93f494f75e574f28d1dc3febea36b4b28986496d";
const BLUEBERRY_FINGERPRINT = "SHA256:1Tsf/CWA+NMQDo4Gw1uCoAXInZgPVEFoPrjFP+lVc98";
const PRODUCTION_CUSTODY_ROOT = "/home/ubuntu/.weather/backend-performance-evidence/v2";
const PRODUCTION_MAIN_UID = 1000n;
const CAPTURE_POLICY = freezeCanonical({
  contractVersion: "weather-backend-main-capture-policy/v1",
  knownHosts: {
    byteLength: 142,
    path: `${PRODUCTION_CUSTODY_ROOT}/trust/blueberry-known-hosts`,
    sha256: KNOWN_HOSTS_SHA256,
  },
  ssh: {
    fingerprint: BLUEBERRY_FINGERPRINT,
    host: "192.168.11.94",
    port: 22,
    user: "admin",
  },
});
const CAPTURE_POLICY_BYTES = canonicalReceiptBytes(CAPTURE_POLICY);
const POLICY_SHA256 = sha256Bytes(CAPTURE_POLICY_BYTES);
const MAX_ARTIFACT_BYTES = 64 * 1024 * 1024;
const EMPTY_SHA256 = createHash("sha256").update(Buffer.alloc(0)).digest("hex");
const SOURCE_PROCESS_ENVIRONMENT = freezeCanonical({
  HOME: "/home/ubuntu",
  LANG: "C",
  PATH: "/home/linuxbrew/.linuxbrew/bin:/home/ubuntu/n/bin:/usr/local/bin:/usr/bin:/bin",
});
const digestPattern = /^[a-f0-9]{64}$/u;
const historicalValidationToken = Symbol("historical-custody-validation");
const moduleDirectory = fileURLToPath(new URL(".", import.meta.url));
const repositoryRoot = resolve(moduleDirectory, "..");
const phaseGeneration = new Map([
  ["baseline", "node-v1"],
  ["pre-cutover", "mixed"],
  ["post-cutover", "rust-v1"],
  ["rollback-drill", "node-v1-rollback"],
]);
const nextPhase = new Map([
  ["baseline", "pre-cutover"],
  ["pre-cutover", "post-cutover"],
  ["post-cutover", "rollback-drill"],
  ["rollback-drill", "post-cutover"],
]);
const activationSources = new Set([
  "container-inspect",
  "systemd-show",
  "process-identity",
  "never-active-alternative",
]);
const toolingPaths = {
  evaluator: "scripts/database-page-performance.mjs",
  observer: "scripts/backend-host-observation.mjs",
  receiptWriter: "scripts/backend-performance-receipts.mjs",
  resourceCollector: "scripts/backend-performance-resources.mjs",
};
const measurementSourcePaths = [
  "config/backend-runtime-inventory.json",
  "config/backend-runtime-live-map.json",
  "package.json",
  "scripts/await-check.mjs",
  "scripts/backend-host-observation.mjs",
  "scripts/backend-performance-receipts.mjs",
  "scripts/backend-performance-resources.mjs",
  "scripts/database-page-performance.mjs",
  "scripts/prepare-schema-preserving-publication.mjs",
  "scripts/schema-preserving-release.mjs",
].sort();

// recognize one plain record
function isRecord(value) {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

// construct one stable coded error
function codedError(code, message, cause) {
  const error = new Error(message, cause === undefined ? undefined : { cause });
  error.code = code;
  return error;
}

// hash one exact byte sequence
function sha256Bytes(value) {
  return createHash("sha256").update(value).digest("hex");
}

// compare exact record keys
function hasExactKeys(value, expected) {
  return isRecord(value) && Object.keys(value).length === expected.size && Object.keys(value).every(
    // accept only reviewed fields
    (key) => expected.has(key),
  );
}

// recognize one canonical UTC instant
function isTimestamp(value) {
  // reject invalid and normalized aliases
  if (typeof value !== "string") {
    return false;
  }
  const milliseconds = Date.parse(value);
  return Number.isFinite(milliseconds) && new Date(milliseconds).toISOString() === value;
}

// convert values to unambiguous JSON data
function canonicalize(value, ancestors = new Set(), path = "$") {
  // retain scalar JSON values
  if (value === null || typeof value === "string" || typeof value === "boolean") {
    return value;
  }
  // retain portable integral numbers
  if (typeof value === "number") {
    // reject signed zero and lossy encodings
    if (!Number.isSafeInteger(value) || Object.is(value, -0)) {
      throw codedError("receipt.schema", `canonical JSON number is ambiguous at ${path}`);
    }
    return value;
  }
  // reject silently discarded JSON values
  if (value === undefined || typeof value === "bigint" || typeof value === "function" || typeof value === "symbol") {
    throw codedError("receipt.schema", `canonical JSON contains an unsupported value at ${path}`);
  }
  // reject recursive input
  if (ancestors.has(value)) {
    throw codedError("receipt.schema", `canonical JSON contains a cycle at ${path}`);
  }
  ancestors.add(value);
  let result;

  // preserve dense arrays only
  if (Array.isArray(value)) {
    result = [];

    // canonicalize every explicit slot
    for (let index = 0; index < value.length; index += 1) {
      // reject sparse arrays
      if (!Object.hasOwn(value, index)) {
        throw codedError("receipt.schema", `canonical JSON contains an array hole at ${path}[${String(index)}]`);
      }
      result.push(canonicalize(value[index], ancestors, `${path}[${String(index)}]`));
    }
  } else {
    const prototype = Object.getPrototypeOf(value);

    // reject exotic objects
    if (prototype !== Object.prototype && prototype !== null) {
      throw codedError("receipt.schema", `canonical JSON contains an exotic object at ${path}`);
    }
    const ownKeys = Reflect.ownKeys(value);

    // require ordinary enumerable data fields
    for (const key of ownKeys) {
      const descriptor = Object.getOwnPropertyDescriptor(value, key);

      // reject magic symbols accessors and hidden values
      if (typeof key !== "string" || key === "__proto__" || descriptor === undefined ||
        descriptor.enumerable !== true || !("value" in descriptor) || "get" in descriptor || "set" in descriptor) {
        throw codedError("receipt.schema", `canonical JSON contains an ambiguous property at ${path}`);
      }
    }
    result = {};

    // canonicalize sorted record fields
    for (const key of ownKeys.sort()) {
      result[key] = canonicalize(value[key], ancestors, `${path}.${key}`);
    }
  }
  ancestors.delete(value);
  return result;
}

// encode one canonical receipt document
export function canonicalReceiptBytes(value) {
  return Buffer.from(`${JSON.stringify(canonicalize(value))}\n`, "utf8");
}

// parse one exact canonical receipt document
function parseCanonicalReceiptBytes(value, code = "receipt.schema") {
  const bytes = Buffer.from(value ?? []);
  let parsed;

  // bound each canonical document
  if (bytes.length === 0 || bytes.length > MAX_ARTIFACT_BYTES) {
    throw codedError(code, "canonical document size is invalid");
  }
  try {
    parsed = JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(bytes));
  } catch (error) {
    throw codedError(code, "canonical document is not valid UTF-8 JSON", error);
  }
  // reject alternate encodings
  if (!bytes.equals(canonicalReceiptBytes(parsed))) {
    throw codedError(code, "document bytes are not canonical receipt JSON");
  }
  return parsed;
}

// skip JSON whitespace in one validated source document
function skipSourceJsonWhitespace(text, offset) {
  let cursor = offset;

  // advance over JSON whitespace only
  while (cursor < text.length && /[\u0009\u000a\u000d\u0020]/u.test(text[cursor])) {
    cursor += 1;
  }
  return cursor;
}

// locate one validated JSON string boundary
function sourceJsonStringEnd(text, offset) {
  let cursor = offset + 1;

  // scan escaped code units without interpreting source data
  while (cursor < text.length) {
    // close one unescaped string
    if (text[cursor] === '"') {
      return cursor + 1;
    }
    // skip one escaped scalar
    if (text[cursor] === "\\") {
      cursor += text[cursor + 1] === "u" ? 6 : 2;
    } else {
      cursor += 1;
    }
  }
  throw codedError("receipt.schema", "source JSON string is unterminated");
}

// scan one parsed JSON value for duplicate object keys
function scanSourceJsonValue(text, offset) {
  let cursor = skipSourceJsonWhitespace(text, offset);

  // scan one object recursively
  if (text[cursor] === "{") {
    cursor = skipSourceJsonWhitespace(text, cursor + 1);
    const keys = new Set();

    // accept one empty object
    if (text[cursor] === "}") {
      return cursor + 1;
    }

    // scan every object member
    while (cursor < text.length) {
      const end = sourceJsonStringEnd(text, cursor);
      const key = JSON.parse(text.slice(cursor, end));

      // reject duplicate decoded keys
      if (keys.has(key)) {
        throw codedError("receipt.schema", "source JSON contains a duplicate key");
      }
      keys.add(key);
      cursor = skipSourceJsonWhitespace(text, end);

      // require one object separator
      if (text[cursor] !== ":") {
        throw codedError("receipt.schema", "source JSON object separator is invalid");
      }
      cursor = scanSourceJsonValue(text, cursor + 1);
      cursor = skipSourceJsonWhitespace(text, cursor);

      // close or advance one object member
      if (text[cursor] === "}") {
        return cursor + 1;
      }
      cursor = skipSourceJsonWhitespace(text, cursor + 1);
    }
  }

  // scan one array recursively
  if (text[cursor] === "[") {
    cursor = skipSourceJsonWhitespace(text, cursor + 1);

    // accept one empty array
    if (text[cursor] === "]") {
      return cursor + 1;
    }

    // scan every array member
    while (cursor < text.length) {
      cursor = scanSourceJsonValue(text, cursor);
      cursor = skipSourceJsonWhitespace(text, cursor);

      // close or advance one array member
      if (text[cursor] === "]") {
        return cursor + 1;
      }
      cursor = skipSourceJsonWhitespace(text, cursor + 1);
    }
  }

  // skip one string primitive
  if (text[cursor] === '"') {
    return sourceJsonStringEnd(text, cursor);
  }

  // skip one scalar primitive
  while (cursor < text.length && !/[\u0009\u000a\u000d\u0020,}\]]/u.test(text[cursor])) {
    cursor += 1;
  }
  return cursor;
}

// parse one bounded source JSON byte sequence without canonical-order assumptions
function parseStrictSourceJson(value, label) {
  const bytes = Buffer.from(value ?? []);
  let text;
  let parsed;

  // bound one source document before decoding
  if (bytes.length === 0 || bytes.length > MAX_ARTIFACT_BYTES) {
    throw codedError("receipt.schema", `${label} size is invalid`);
  }
  try {
    text = new TextDecoder("utf-8", { fatal: true }).decode(bytes);
    parsed = JSON.parse(text);
  } catch (error) {
    throw codedError("receipt.schema", `${label} is not valid UTF-8 JSON`, error);
  }
  const completed = skipSourceJsonWhitespace(text, scanSourceJsonValue(text, 0));

  // require the duplicate scanner to consume the parsed document
  if (completed !== text.length) {
    throw codedError("receipt.schema", `${label} has trailing or ambiguous JSON bytes`);
  }
  canonicalize(parsed);
  return parsed;
}

// hash one canonical receipt document
export function receiptSha256(valueOrBytes) {
  // hash exact verified canonical bytes
  if (Buffer.isBuffer(valueOrBytes) || valueOrBytes instanceof Uint8Array || typeof valueOrBytes === "string") {
    const bytes = Buffer.from(valueOrBytes);
    parseCanonicalReceiptBytes(bytes);
    return sha256Bytes(bytes);
  }
  return sha256Bytes(canonicalReceiptBytes(valueOrBytes));
}

// detach and recursively freeze JSON data
function freezeCanonical(value) {
  const detached = canonicalize(value);

  // freeze children before parents
  function freezeNested(item) {
    // recurse through JSON containers
    if (item !== null && typeof item === "object") {
      for (const child of Object.values(item)) {
        freezeNested(child);
      }
      Object.freeze(item);
    }
  }
  freezeNested(detached);
  return detached;
}

// encode one fixed remote helper command
function encodedPythonCommand(source, operationId) {
  const encoded = Buffer.from(source, "utf8").toString("base64");
  return `/usr/bin/python3 -I -B -S -c 'import base64;exec(base64.b64decode("${encoded}"))' ${operationId}`;
}

// weather-retained-v1-stage-helper-source:start
const STAGE_HELPER_SOURCE = String.raw`import base64,hashlib,json,os,shutil,stat,sys
ROOT="/tmp/weather-backend-host-observation-v2-admin-stage"
NAMES=("backend-runtime-inventory.json","backend-runtime-live-map.json","observer.mjs","request.json")
# fail one invalid package
def die(): raise SystemExit(1)
# stage one bounded exact package
created=False
try:
 data=sys.stdin.buffer.read(16777217)
# reject one oversized package
 if len(data)>16777216: die()
 package=json.loads(data)
# require the fixed package version
 if package.get("contractVersion")!="weather-backend-main-stage-package/v1": die()
 files=package.get("files")
# require every fixed member exactly once
 if not isinstance(files,list) or tuple(sorted(x.get("name") for x in files))!=NAMES: die()
 os.mkdir(ROOT,0o700)
 created=True
 rootfd=os.open(ROOT,os.O_RDONLY|os.O_DIRECTORY|os.O_NOFOLLOW)
# write each verified member privately
 for item in files:
# require the closed member shape
  if set(item)!={"byteLength","dataBase64","name","sha256"}: die()
  raw=base64.b64decode(item["dataBase64"],validate=True)
# bind declared identity to bytes
  if len(raw)!=item["byteLength"] or hashlib.sha256(raw).hexdigest()!=item["sha256"]: die()
  fd=os.open(item["name"],os.O_WRONLY|os.O_CREAT|os.O_EXCL|os.O_NOFOLLOW,0o600,dir_fd=rootfd)
# close each member descriptor
  with os.fdopen(fd,"wb",buffering=0) as out: out.write(raw); os.fsync(out.fileno())
 os.close(rootfd)
except BaseException:
# clean only the stage created by this process
 if created: shutil.rmtree(ROOT)
 die()
`;
// weather-retained-v1-stage-helper-source:end

// weather-retained-v1-cleanup-helper-source:start
const CLEANUP_HELPER_SOURCE = String.raw`import hashlib,os,stat,sys
OP=sys.argv[1] if len(sys.argv)==6 else ""
ROOT="/tmp/weather-backend-host-observation-v2-admin-stage"
NAMES=("observer.mjs","backend-runtime-inventory.json","backend-runtime-live-map.json","request.json")
# require the fixed cleanup operation and trusted digests
if OP!="blueberry-compose-v1": raise SystemExit(1)
EXPECTED=dict(zip(NAMES,sys.argv[2:]))
# reject missing or malformed trusted identities
if any(len(value)!=64 or any(ch not in "0123456789abcdef" for ch in value) for value in EXPECTED.values()): raise SystemExit(1)
# close any inherited input before cleanup
try: sys.stdin.close()
except Exception: raise SystemExit(1)
rootfd=None
try:
 rootfd=os.open(ROOT,os.O_RDONLY|os.O_DIRECTORY|os.O_NOFOLLOW)
 root=os.fstat(rootfd)
 # bind the stage to the unprivileged executing account
 if not stat.S_ISDIR(root.st_mode) or root.st_uid!=os.geteuid() or root.st_gid!=os.getegid() or stat.S_IMODE(root.st_mode)!=0o700: raise RuntimeError()
 # require only the four staged members
 if tuple(sorted(os.listdir(rootfd)))!=tuple(sorted(NAMES)): raise RuntimeError()
 # validate every staged member before deleting any member
 for name in NAMES:
  fd=os.open(name,os.O_RDONLY|os.O_NOFOLLOW|os.O_NONBLOCK,dir_fd=rootfd)
  try:
   before=os.fstat(fd)
   # require one private exact member
   if not stat.S_ISREG(before.st_mode) or before.st_uid!=os.geteuid() or before.st_gid!=os.getegid() or stat.S_IMODE(before.st_mode)!=0o600 or before.st_nlink!=1 or before.st_size<1 or before.st_size>67108864: raise RuntimeError()
   chunks=[];remaining=67108865
   # hash one bounded staged member
   while remaining>0:
    chunk=os.read(fd,min(65536,remaining))
    # stop at the exact end
    if not chunk: break
    chunks.append(chunk);remaining-=len(chunk)
   after=os.fstat(fd);raw=b"".join(chunks)
   # reject changed or substituted bytes
   if len(raw)!=before.st_size or (before.st_dev,before.st_ino,before.st_size,before.st_mtime_ns,before.st_ctime_ns)!=(after.st_dev,after.st_ino,after.st_size,after.st_mtime_ns,after.st_ctime_ns) or hashlib.sha256(raw).hexdigest()!=EXPECTED[name]: raise RuntimeError()
  finally:
   os.close(fd)
 # close directory membership before unlinking
 if tuple(sorted(os.listdir(rootfd)))!=tuple(sorted(NAMES)) or os.fstat(rootfd).st_ino!=root.st_ino: raise RuntimeError()
 # unlink only the validated fixed members
 for name in NAMES: os.unlink(name,dir_fd=rootfd)
 os.close(rootfd);rootfd=None
 os.rmdir(ROOT)
 # require the fixed stage to be absent
 try: os.stat(ROOT,follow_symlinks=False);raise RuntimeError()
 except FileNotFoundError: pass
except BaseException:
 # close any retained directory descriptor
 if rootfd is not None:
  try: os.close(rootfd)
  except OSError: pass
 raise SystemExit(1)
`;
// weather-retained-v1-cleanup-helper-source:end

// weather-retained-v1-root-helper-source:start
const ROOT_HELPER_SOURCE = String.raw`import hashlib,json,os,shutil,stat,subprocess,sys
OP=sys.argv[1] if len(sys.argv)==6 else ""
# require one finite operation
if OP not in ("blueberry-compose-v1","workstation-runner-v1"): raise SystemExit(1)
EXPECTED=dict(zip(("observer.mjs","backend-runtime-inventory.json","backend-runtime-live-map.json","request.json"),sys.argv[2:]))
# require four exact trusted digests
if any(len(value)!=64 or any(ch not in "0123456789abcdef" for ch in value) for value in EXPECTED.values()): raise SystemExit(1)
# close password input before opening request bytes
try: sys.stdin.close()
except Exception: raise SystemExit(1)
REMOTE=OP=="blueberry-compose-v1"
SOURCE="/tmp/weather-backend-host-observation-v2-admin-stage" if REMOTE else "/home/ubuntu/weather"
ROOT="/run/weather-backend-host-observation-v2-blueberry" if REMOTE else "/run/weather-backend-host-observation-v2-workstation"
NODE="/usr/bin/node" if REMOTE else "/home/ubuntu/n/bin/node"
REQUEST="/home/ubuntu/.weather/backend-performance-evidence/v2/incoming/workstation-runner/request.json"
MAIN_UID=1000
MAIN_GID=1000
# stage and run one fixed observer
created=False
source_rootfd=None
source_dirfds=[]
try:
 os.mkdir(ROOT,0o700)
 created=True
 names=("backend-runtime-inventory.json","backend-runtime-live-map.json","observer.mjs","request.json")
 source_rootfd=os.open(SOURCE,os.O_RDONLY|os.O_DIRECTORY|os.O_NOFOLLOW)
 source_dirfds.append(source_rootfd)
 source_meta=os.fstat(source_rootfd)
# require one exact unprivileged stage directory
 if not stat.S_ISDIR(source_meta.st_mode): raise RuntimeError()
 if REMOTE and (stat.S_IMODE(source_meta.st_mode)!=0o700 or source_meta.st_uid==0 or source_meta.st_gid==0 or tuple(sorted(os.listdir(source_rootfd)))!=names): raise RuntimeError()
 if not REMOTE and (source_meta.st_uid!=MAIN_UID or source_meta.st_gid!=MAIN_GID): raise RuntimeError()
 local_parents={}
 if not REMOTE:
  config_fd=os.open("config",os.O_RDONLY|os.O_DIRECTORY|os.O_NOFOLLOW,dir_fd=source_rootfd);source_dirfds.append(config_fd)
  scripts_fd=os.open("scripts",os.O_RDONLY|os.O_DIRECTORY|os.O_NOFOLLOW,dir_fd=source_rootfd);source_dirfds.append(scripts_fd)
  request_parent=os.path.dirname(REQUEST)
  request_fd=os.open(request_parent,os.O_RDONLY|os.O_DIRECTORY|os.O_NOFOLLOW);source_dirfds.append(request_fd)
  for fd in (config_fd,scripts_fd,request_fd):
   meta=os.fstat(fd)
   if not stat.S_ISDIR(meta.st_mode) or meta.st_uid!=MAIN_UID or meta.st_gid!=MAIN_GID: raise RuntimeError()
  local_parents={"backend-runtime-inventory.json":(config_fd,"backend-runtime-inventory.json",0o644),"backend-runtime-live-map.json":(config_fd,"backend-runtime-live-map.json",0o644),"observer.mjs":(scripts_fd,"backend-host-observation.mjs",0o644),"request.json":(request_fd,"request.json",0o600)}
 source_identities={}
# copy each trusted fixed source
 for name in names:
  dst=os.path.join(ROOT,name)
  parent_fd,leaf,expected_mode=(source_rootfd,name,0o600) if REMOTE else local_parents[name]
  source_fd=os.open(leaf,os.O_RDONLY|os.O_NOFOLLOW|os.O_NONBLOCK,dir_fd=parent_fd)
  before=os.fstat(source_fd)
# require one stable trusted regular source
  expected_uid=source_meta.st_uid if REMOTE else MAIN_UID
  expected_gid=source_meta.st_gid if REMOTE else MAIN_GID
  if not stat.S_ISREG(before.st_mode) or before.st_nlink!=1 or before.st_size<1 or before.st_size>67108864 or before.st_uid!=expected_uid or before.st_gid!=expected_gid or stat.S_IMODE(before.st_mode)!=expected_mode: raise RuntimeError()
  with os.fdopen(source_fd,"rb",buffering=0) as inp,open(dst,"xb",buffering=0) as out:
   raw=inp.read(67108865); after=os.fstat(inp.fileno())
# bind source bytes before privileged execution
   identity=(before.st_dev,before.st_ino,before.st_size,before.st_mtime_ns,before.st_ctime_ns,before.st_mode,before.st_uid,before.st_gid,before.st_nlink)
   if len(raw)!=before.st_size or identity!=(after.st_dev,after.st_ino,after.st_size,after.st_mtime_ns,after.st_ctime_ns,after.st_mode,after.st_uid,after.st_gid,after.st_nlink) or hashlib.sha256(raw).hexdigest()!=EXPECTED[name]: raise RuntimeError()
   source_identities[name]=(identity,parent_fd,leaf)
   out.write(raw); out.flush(); os.fsync(out.fileno())
   os.fchmod(out.fileno(),0o600)
# recheck every source pathname after all reads
 if REMOTE:
  path_meta=os.stat(SOURCE,follow_symlinks=False)
  if (path_meta.st_dev,path_meta.st_ino)!=(source_meta.st_dev,source_meta.st_ino) or tuple(sorted(os.listdir(source_rootfd)))!=names: raise RuntimeError()
 else:
  path_meta=os.stat(SOURCE,follow_symlinks=False)
  if (path_meta.st_dev,path_meta.st_ino)!=(source_meta.st_dev,source_meta.st_ino): raise RuntimeError()
  for path,fd in ((os.path.join(SOURCE,"config"),config_fd),(os.path.join(SOURCE,"scripts"),scripts_fd),(os.path.dirname(REQUEST),request_fd)):
   value=os.stat(path,follow_symlinks=False);pinned=os.fstat(fd)
   if (value.st_dev,value.st_ino)!=(pinned.st_dev,pinned.st_ino): raise RuntimeError()
 for name in names:
   identity,parent_fd,leaf=source_identities[name]
   replay=os.open(leaf,os.O_RDONLY|os.O_NOFOLLOW|os.O_NONBLOCK,dir_fd=parent_fd)
   try:
    value=os.fstat(replay)
    if (value.st_dev,value.st_ino,value.st_size,value.st_mtime_ns,value.st_ctime_ns,value.st_mode,value.st_uid,value.st_gid,value.st_nlink)!=identity: raise RuntimeError()
   finally: os.close(replay)
 for fd in reversed(source_dirfds): os.close(fd)
 source_dirfds=[];source_rootfd=None
 with open(os.path.join(ROOT,"request.json"),"rb",buffering=0) as request:
  result=subprocess.run([NODE,os.path.join(ROOT,"observer.mjs"),"collect","--request","/dev/stdin"],stdin=request,stdout=subprocess.PIPE,stderr=subprocess.PIPE,env={"LANG":"C"},cwd="/",timeout=30,check=False)
# require bounded successful collection
 if result.returncode!=0 or len(result.stdout)>68157440 or len(result.stderr)>65536: raise RuntimeError()
 sys.stdout.buffer.write(result.stdout); sys.stderr.buffer.write(result.stderr)
finally:
# close any retained source descriptors
  for fd in reversed(source_dirfds):
   try: os.close(fd)
   except OSError: pass
# clean the exclusive root stage
  if created:
   try: shutil.rmtree(ROOT)
   except Exception: raise SystemExit(1)
`;
// weather-retained-v1-root-helper-source:end

// weather-retained-v1-deployed-source-helper-source:start
const DEPLOYED_SOURCE_HELPER_SOURCE = String.raw`import base64,hashlib,json,os,re,stat,sys
OPERATION="blueberry-deployed-source-v1"
ROOT="/opt/weather/current"
MAX_JSON=1048576
MAX_ENV=65536
MAX_BUNDLE=8388608
RELEASE=re.compile(r"^[0-9]{4}\.[0-9]{2}\.[0-9]{2}-[1-9][0-9]?$" )
DIGEST=re.compile(r"^[a-f0-9]{64}$")
IMAGE=re.compile(r"^[^@\s]+@sha256:[a-f0-9]{64}$")
DB=re.compile(r"^[a-z][a-z0-9_]{0,62}$")
POSTGRES=re.compile(r"^/var/lib/weather(?:/[A-Za-z0-9._-]+)+$")
CURRENT_KEYS=("WEATHER_RELEASE","WEATHER_SERVER_IMAGE","WEATHER_WEB_IMAGE","POSTGRES_IMAGE","CLOUDFLARED_IMAGE","WEATHER_DATABASE_NAME","WEATHER_POSTGRES_DIR","WEATHER_FORECAST_ADJUSTMENT_WIND_CANARY_KILL_SWITCH","WEATHER_FORECAST_ADJUSTMENT_TEMPERATURE_CANARY_KILL_SWITCH","WEATHER_CONTROL_PLANE_SHA256","WEATHER_CONTROL_PLANE_VERSION")
LEGACY_KEYS=tuple(key for key in CURRENT_KEYS if key!="WEATHER_FORECAST_ADJUSTMENT_TEMPERATURE_CANARY_KILL_SWITCH")
LEGACY_SHA="c4d74581b84505e065fdec63447dfdded1d14221e459777a88e37729275f33b5"
DIRS={}

# fail one fixed source-copy operation
def die():
 raise SystemExit(1)

# encode one portable descriptor identity
def identity(value,kind):
 return {"ctimeNs":str(value.st_ctime_ns),"dev":str(value.st_dev),"gid":value.st_gid,"ino":str(value.st_ino),"mode":format(stat.S_IMODE(value.st_mode),"04o"),"mtimeNs":str(value.st_mtime_ns),"nlink":str(value.st_nlink),"size":str(value.st_size),"type":kind,"uid":value.st_uid}

# encode one retained raw field
def raw_field(value):
 return {"byteLength":len(value),"dataBase64":base64.b64encode(value).decode("ascii"),"encoding":"base64","sha256":hashlib.sha256(value).hexdigest()}

# open one fixed directory path without following links
def open_dir(parent,name):
 fd=os.open(name,os.O_RDONLY|os.O_DIRECTORY|os.O_NOFOLLOW,dir_fd=parent)
 try:
  details=os.fstat(fd)
  # require a directory handle
  if not stat.S_ISDIR(details.st_mode): die()
  return fd,details
 except BaseException:
  os.close(fd);raise

# open the pinned source root component by component
def open_root():
 root=os.open("/",os.O_RDONLY|os.O_DIRECTORY)
 opened=[root]
 current=root
 try:
  # walk only the fixed source root
  for component in ("opt","weather","current"):
   current,details=open_dir(current,component);opened.append(current)
  # require the installed root policy
  if details.st_uid!=0 or details.st_gid!=0 or stat.S_IMODE(details.st_mode)!=0o755:
   die()
  return opened,current,details
 except BaseException:
  # close every acquired root descriptor
  for fd in reversed(opened):
   try: os.close(fd)
   except OSError: pass
  raise

# open one descriptor-confined parent directory
def open_parent(root,components):
 opened=[];current=root
 try:
  # walk one fixed relative parent
  for index,component in enumerate(components):
   current,details=open_dir(current,component);opened.append(current)
   key="/".join(components[:index+1]);value=identity(details,"directory")
   # reject a mixed directory snapshot
   if key in DIRS and DIRS[key]!=value: die()
   DIRS[key]=value
  return opened,current
 except BaseException:
  # close every acquired parent descriptor
  for fd in reversed(opened):
   try: os.close(fd)
   except OSError: pass
  raise

# read one bounded stable regular file
def read_regular(root,path,maximum):
 components=path.split("/");opened,parent=open_parent(root,components[:-1]);handle=None
 try:
  handle=os.open(components[-1],os.O_RDONLY|os.O_NOFOLLOW,dir_fd=parent)
  before=os.fstat(handle)
  # require the exact private file policy
  if not stat.S_ISREG(before.st_mode) or before.st_uid!=0 or before.st_gid!=0 or stat.S_IMODE(before.st_mode)!=0o600 or before.st_nlink!=1 or before.st_size<1 or before.st_size>maximum:
   die()
  chunks=[];remaining=maximum+1
  # read within the fixed byte bound
  while remaining>0:
   chunk=os.read(handle,min(65536,remaining))
   # stop at exact end of file
   if not chunk: break
   chunks.append(chunk);remaining-=len(chunk)
  value=b"".join(chunks);after=os.fstat(handle)
  # reject mutation during the read
  if len(value)!=before.st_size or identity(before,"regular-file")!=identity(after,"regular-file"):
   die()
  os.close(handle);handle=None
  final=None
  try:
   final=os.open(components[-1],os.O_RDONLY|os.O_NOFOLLOW,dir_fd=parent)
   final_details=os.fstat(final)
  finally:
   # close the final replay descriptor
   if final is not None: os.close(final)
  # reject path replacement after the read
  if identity(before,"regular-file")!=identity(final_details,"regular-file"):
   die()
  return {"identity":identity(before,"regular-file"),"path":f"{ROOT}/{path}","raw":raw_field(value)},value
 finally:
  # close an interrupted readable descriptor
  if handle is not None:
   try: os.close(handle)
   except OSError: pass
  # close every acquired parent descriptor
  for fd in reversed(opened):
   try: os.close(fd)
   except OSError: pass

# re-open one captured file during final closing validation
def recheck_regular(root,proof):
 path=proof["path"][len(ROOT)+1:];components=path.split("/");opened,parent=open_parent(root,components[:-1])
 handle=None
 try:
  handle=os.open(components[-1],os.O_RDONLY|os.O_NOFOLLOW,dir_fd=parent)
  details=os.fstat(handle)
  # reject final file identity drift
  if identity(details,"regular-file")!=proof["identity"]: die()
 finally:
  # close every final replay descriptor
  if handle is not None:
   try: os.close(handle)
   except OSError: pass
  # close every acquired parent descriptor
  for fd in reversed(opened):
   try: os.close(fd)
   except OSError: pass

# re-open every traversed directory during final closing validation
def recheck_dirs(root):
 # re-open every traversed directory
 for path,expected in sorted(DIRS.items(),key=lambda item:(item[0].count("/"),item[0])):
  opened,current=open_parent(root,path.split("/"))
  try:
   # reject directory identity drift
   if identity(os.fstat(current),"directory")!=expected: die()
  finally:
   # close every directory replay descriptor
   for fd in reversed(opened):
    try: os.close(fd)
    except OSError: pass

# validate one exact nonsecret release environment
def validate_env(value,release):
 try: text=value.decode("utf-8")
 except UnicodeDecodeError: die()
 # require one deterministic text payload
 if not text.endswith("\n") or "\r" in text or "\x00" in text: die()
 lines=text[:-1].split("\n")
 pairs=[];has_blank=False
 # parse only fixed assignments and legacy blanks
 for line in lines:
  # retain one legacy blank separator
  if not line:
   has_blank=True;continue
  # reject malformed assignments
  if "=" not in line: die()
  key,item=line.split("=",1);pairs.append((key,item))
 keys=tuple(key for key,_ in pairs)
 # require one exact field sequence
 if keys not in (CURRENT_KEYS,LEGACY_KEYS): die()
 # reject blanks in the current schema
 if has_blank and keys!=LEGACY_KEYS: die()
 values=dict(pairs)
 # bind the selected release
 if values["WEATHER_RELEASE"]!=release or not RELEASE.fullmatch(release): die()
 # validate every immutable image reference
 for key in ("WEATHER_SERVER_IMAGE","WEATHER_WEB_IMAGE","POSTGRES_IMAGE","CLOUDFLARED_IMAGE"):
  # require one digest-pinned image
  if not IMAGE.fullmatch(values[key]): die()
 # validate database identity and storage root
 if not DB.fullmatch(values["WEATHER_DATABASE_NAME"]) or not POSTGRES.fullmatch(values["WEATHER_POSTGRES_DIR"]): die()
 # validate every retained canary bit
 for key in ("WEATHER_FORECAST_ADJUSTMENT_WIND_CANARY_KILL_SWITCH","WEATHER_FORECAST_ADJUSTMENT_TEMPERATURE_CANARY_KILL_SWITCH"):
  # require one binary switch
  if key in values and values[key] not in ("0","1"): die()
 # validate the control-plane identity
 if not DIGEST.fullmatch(values["WEATHER_CONTROL_PLANE_SHA256"]) or not values["WEATHER_CONTROL_PLANE_VERSION"].isdigit() or int(values["WEATHER_CONTROL_PLANE_VERSION"])<1: die()
 # pin the sole legacy control plane
 if keys==LEGACY_KEYS and (values["WEATHER_CONTROL_PLANE_VERSION"]!="6" or values["WEATHER_CONTROL_PLANE_SHA256"]!=LEGACY_SHA): die()

# read and close the exact active alias
def read_alias(root,path,release,target):
 components=path.split("/");opened,parent=open_parent(root,components[:-1])
 try:
  before=os.stat(components[-1],dir_fd=parent,follow_symlinks=False)
  # require one root-owned active alias
  if not stat.S_ISLNK(before.st_mode) or before.st_uid!=0 or before.st_gid!=0: die()
  link=os.readlink(components[-1],dir_fd=parent)
  expected=f"../releases/{release}.env"
  # bind the alias to the selected release
  if link!=expected: die()
  after=os.stat(components[-1],dir_fd=parent,follow_symlinks=False)
  # reject alias mutation during the read
  if identity(before,"symbolic-link")!=identity(after,"symbolic-link"): die()
  return {"identity":identity(before,"symbolic-link"),"linkTarget":link,"path":f"{ROOT}/{path}","resolvedPath":target["path"],"targetIdentity":target["identity"],"targetSha256":target["raw"]["sha256"]}
 finally:
  # close every acquired alias parent
  for fd in reversed(opened):
   try: os.close(fd)
   except OSError: pass

# collect only the fixed deployed-source set
def main():
 # require the sole reviewed operation
 if len(sys.argv)!=2 or sys.argv[1]!=OPERATION: die()
 try: os.close(0)
 except OSError: pass
 opened,root,root_details=open_root()
 try:
  marker,marker_bytes=read_regular(root,"deploy/state/current-release",128)
  try: release=marker_bytes.decode("ascii").removesuffix("\n")
  except UnicodeDecodeError: die()
  # bind one canonical marker payload
  if marker_bytes!=f"{release}\n".encode() or not RELEASE.fullmatch(release): die()
  state=f"deploy/state/schema-preserving-release-v1/{release}"
  publication,_=read_regular(root,f"{state}/publication.json",MAX_JSON)
  stage,_=read_regular(root,f"{state}/stage.json",MAX_JSON)
  ready,_=read_regular(root,f"{state}/ready.json",MAX_JSON)
  protected,_=read_regular(root,f"{state}/protected.json",MAX_JSON)
  environment,environment_bytes=read_regular(root,f"deploy/releases/{release}.env",MAX_ENV)
  validate_env(environment_bytes,release)
  active=read_alias(root,"deploy/state/active.env",release,environment)
  # re-open every captured regular file at close
  for proof in (marker,publication,stage,ready,protected,environment):
   recheck_regular(root,proof)
  # re-open the active alias at close
  if read_alias(root,"deploy/state/active.env",release,environment)!=active: die()
  recheck_dirs(root)
  final=os.fstat(root)
  # reject source-root identity drift
  if identity(root_details,"directory")!=identity(final,"directory"): die()
  bundle={"activeAlias":active,"contractVersion":"weather-backend-deployed-source-capture/v1","currentRelease":marker,"operationId":OPERATION,"protected":protected,"publication":publication,"ready":ready,"releaseEnvironment":environment,"sourceRoot":{"identity":identity(root_details,"directory"),"path":ROOT},"stage":stage}
  output=(json.dumps(bundle,sort_keys=True,separators=(",",":"))+"\n").encode()
  # bound the complete canonical bundle
  if len(output)>MAX_BUNDLE: die()
  sys.stdout.buffer.write(output)
 finally:
  # close every pinned source-root descriptor
  for fd in reversed(opened):
   try: os.close(fd)
   except OSError: pass

# run only the fixed operation
try: main()
except BaseException: die()
`;
// weather-retained-v1-deployed-source-helper-source:end

const SSH_COMMON = [
  "-F", "/dev/null", "-T",
  "-o", "BatchMode=yes", "-o", "IdentitiesOnly=yes",
  "-o", "StrictHostKeyChecking=yes",
  "-o", `UserKnownHostsFile=${CAPTURE_POLICY.knownHosts.path}`,
  "-o", "GlobalKnownHostsFile=/dev/null", "-o", "UpdateHostKeys=no",
  "-o", "RequestTTY=no", "-o", "ForwardAgent=no", "-o", "ForwardX11=no",
  "-o", "ClearAllForwardings=yes", "-o", "PermitLocalCommand=no",
  "-o", "ConnectTimeout=10", "-i", "/home/ubuntu/.ssh/id_ed25519",
  "-p", "22", "admin@192.168.11.94",
];

const capturePlanTemplates = {
  "blueberry-compose-v1": {
    profile: "blueberry-compose",
    steps: [
      {
        argv: [...SSH_COMMON, encodedPythonCommand(STAGE_HELPER_SOURCE, "blueberry-compose-v1")],
        cwd: "/",
        environment: { LANG: "C" },
        id: "blueberry.stage-package",
        program: "/usr/bin/ssh",
        stderrMaxBytes: 65_536,
        stdinKind: "stage-package",
        stdoutMaxBytes: 0,
        timeoutMs: 45_000,
      },
      {
        argv: [...SSH_COMMON, `/usr/bin/sudo -S -p '' -- ${encodedPythonCommand(ROOT_HELPER_SOURCE, "blueberry-compose-v1")}`],
        cwd: "/",
        environment: { LANG: "C" },
        id: "blueberry.collect",
        program: "/usr/bin/ssh",
        stderrMaxBytes: 65_536,
        stdinKind: "protected-sudo-password",
        stdoutMaxBytes: 68_157_440,
        timeoutMs: 45_000,
      },
      {
        argv: [...SSH_COMMON, encodedPythonCommand(CLEANUP_HELPER_SOURCE, "blueberry-compose-v1")],
        cwd: "/",
        environment: { LANG: "C" },
        id: "blueberry.cleanup",
        program: "/usr/bin/ssh",
        stderrMaxBytes: 65_536,
        stdinKind: "none",
        stdoutMaxBytes: 0,
        timeoutMs: 45_000,
      },
    ],
  },
  "workstation-runner-v1": {
    profile: "workstation-runner",
    steps: [
      {
        argv: ["-S", "-p", "", "--", "/usr/bin/python3", "-I", "-B", "-S", "-c", ROOT_HELPER_SOURCE, "workstation-runner-v1"],
        cwd: "/",
        environment: { LANG: "C" },
        id: "workstation.collect",
        program: "/usr/bin/sudo",
        stderrMaxBytes: 65_536,
        stdinKind: "protected-sudo-password",
        stdoutMaxBytes: 68_157_440,
        timeoutMs: 45_000,
      },
    ],
  },
};

export const MAIN_CAPTURE_PLANS = freezeCanonical(capturePlanTemplates);

export const DEPLOYED_SOURCE_CAPTURE_PLAN = freezeCanonical({
  operationId: "blueberry-deployed-source-v1",
  steps: [
    {
      argv: [
        ...SSH_COMMON,
        `/usr/bin/sudo -S -p '' -- ${encodedPythonCommand(DEPLOYED_SOURCE_HELPER_SOURCE, "blueberry-deployed-source-v1")}`,
      ],
      cwd: "/",
      environment: { LANG: "C" },
      id: "blueberry.deployed-source.collect",
      program: "/usr/bin/ssh",
      stderrMaxBytes: 65_536,
      stdinKind: "protected-sudo-password",
      stdoutMaxBytes: 8_388_608,
      timeoutMs: 15_000,
    },
  ],
});

const RETAINED_V1_SSH_COMMON = Object.freeze([
  "-F", "/dev/null", "-T",
  "-o", "BatchMode=yes", "-o", "IdentitiesOnly=yes",
  "-o", "StrictHostKeyChecking=yes",
  "-o", "UserKnownHostsFile=/home/ubuntu/.weather/backend-performance-evidence/v2/trust/blueberry-known-hosts",
  "-o", "GlobalKnownHostsFile=/dev/null", "-o", "UpdateHostKeys=no",
  "-o", "RequestTTY=no", "-o", "ForwardAgent=no", "-o", "ForwardX11=no",
  "-o", "ClearAllForwardings=yes", "-o", "PermitLocalCommand=no",
  "-o", "ConnectTimeout=10", "-i", "/home/ubuntu/.ssh/id_ed25519",
  "-p", "22", "admin@192.168.11.94",
]);
const MAIN_CAPTURE_PLAN_V1_DESCRIPTORS = freezeCanonical({
  "blueberry-compose-v1": {
    profile: "blueberry-compose",
    steps: [
      {
        cwd: "/", environment: { LANG: "C" }, helper: "stage", id: "blueberry.stage-package",
        program: "/usr/bin/ssh", stderrMaxBytes: 65_536, stdinKind: "stage-package",
        stdoutMaxBytes: 0, timeoutMs: 45_000,
      },
      {
        cwd: "/", environment: { LANG: "C" }, helper: "root", id: "blueberry.collect",
        program: "/usr/bin/ssh", stderrMaxBytes: 65_536, stdinKind: "protected-sudo-password",
        stdoutMaxBytes: 68_157_440, timeoutMs: 45_000,
      },
      {
        cwd: "/", environment: { LANG: "C" }, helper: "cleanup", id: "blueberry.cleanup",
        program: "/usr/bin/ssh", stderrMaxBytes: 65_536, stdinKind: "none",
        stdoutMaxBytes: 0, timeoutMs: 45_000,
      },
    ],
  },
  "workstation-runner-v1": {
    profile: "workstation-runner",
    steps: [{
      cwd: "/", environment: { LANG: "C" }, helper: "root", id: "workstation.collect",
      program: "/usr/bin/sudo", stderrMaxBytes: 65_536, stdinKind: "protected-sudo-password",
      stdoutMaxBytes: 68_157_440, timeoutMs: 45_000,
    }],
  },
});
const DEPLOYED_SOURCE_CAPTURE_V1_DESCRIPTOR = freezeCanonical({
  operationId: "blueberry-deployed-source-v1",
  step: {
    cwd: "/", environment: { LANG: "C" }, helper: "deployed-source", id: "blueberry.deployed-source.collect",
    program: "/usr/bin/ssh", stderrMaxBytes: 65_536, stdinKind: "protected-sudo-password",
    stdoutMaxBytes: 8_388_608, timeoutMs: 15_000,
  },
});

// extract one exact retained helper body without executing archived source
function retainedWriterHelperSource(writerBytes, name) {
  const source = new TextDecoder("utf-8", { fatal: true }).decode(writerBytes);
  const start = `// weather-retained-v1-${name}-helper-source:start\nconst ${name === "stage" ? "STAGE" : name === "cleanup" ? "CLEANUP" : name === "root" ? "ROOT" : "DEPLOYED_SOURCE"}_HELPER_SOURCE = String.raw\``;
  const end = `\n\`;\n// weather-retained-v1-${name}-helper-source:end`;
  const first = source.indexOf(start);

  // require one unique retained helper boundary
  if (first < 0 || source.lastIndexOf(start) !== first) {
    throw codedError("receipt.authority", `retained ${name} helper boundary is invalid`);
  }
  const contentStart = first + start.length;
  const contentEnd = source.indexOf(end, contentStart);

  // reject missing or ambiguous closing boundaries
  if (contentEnd < 0 || source.indexOf(end, contentEnd + end.length) >= 0) {
    throw codedError("receipt.authority", `retained ${name} helper boundary is ambiguous`);
  }
  return `${source.slice(contentStart, contentEnd)}\n`;
}

// encode the immutable v1 helper command independently from active plan factories
function encodedRetainedV1PythonCommand(source, operationId) {
  const encoded = Buffer.from(source, "utf8").toString("base64");
  return `/usr/bin/python3 -I -B -S -c 'import base64;exec(base64.b64decode("${encoded}"))' ${operationId}`;
}

// materialize one immutable v1 descriptor with helper bytes from its retained writer
function retainedMainCaptureV1Template(operationId, writerBytes) {
  const descriptor = MAIN_CAPTURE_PLAN_V1_DESCRIPTORS[operationId];

  // reject operations outside the immutable v1 descriptor set
  if (descriptor === undefined) {
    throw codedError("receipt.authority", "retained capture operation is invalid");
  }
  const helpers = {
    cleanup: retainedWriterHelperSource(writerBytes, "cleanup"),
    root: retainedWriterHelperSource(writerBytes, "root"),
    stage: retainedWriterHelperSource(writerBytes, "stage"),
  };
  const steps = descriptor.steps.map(
    // replace only the helper-bearing fixed vector component
    (step) => {
      const { helper, ...fixed } = step;

      // build each immutable remote or local v1 vector
      if (step.id === "blueberry.stage-package") {
        return { ...fixed, argv: [...RETAINED_V1_SSH_COMMON, encodedRetainedV1PythonCommand(helpers[helper], operationId)] };
      }
      if (step.id === "blueberry.collect") {
        return {
          ...fixed,
          argv: [...RETAINED_V1_SSH_COMMON, `/usr/bin/sudo -S -p '' -- ${encodedRetainedV1PythonCommand(helpers[helper], operationId)}`],
        };
      }
      if (step.id === "blueberry.cleanup") {
        return { ...fixed, argv: [...RETAINED_V1_SSH_COMMON, encodedRetainedV1PythonCommand(helpers[helper], operationId)] };
      }
      if (step.id === "workstation.collect") {
        return {
          ...fixed,
          argv: ["-S", "-p", "", "--", "/usr/bin/python3", "-I", "-B", "-S", "-c", helpers[helper], operationId],
        };
      }
      throw codedError("receipt.authority", "retained v1 descriptor contains an unknown step");
    },
  );
  return freezeCanonical({ profile: descriptor.profile, steps });
}

// materialize the immutable deployed-source v1 plan from its retained writer helper
function retainedDeployedSourceV1Plan(writerBytes) {
  const helperSource = retainedWriterHelperSource(writerBytes, "deployed-source");
  const descriptor = DEPLOYED_SOURCE_CAPTURE_V1_DESCRIPTOR;
  const { helper: _helper, ...fixed } = descriptor.step;
  const steps = [{
    ...fixed,
    argv: [
      ...RETAINED_V1_SSH_COMMON,
      `/usr/bin/sudo -S -p '' -- ${encodedRetainedV1PythonCommand(helperSource, descriptor.operationId)}`,
    ],
  }];
  return freezeCanonical({
    contractVersion: "weather-backend-deployed-source-plan/v1",
    operationId: descriptor.operationId,
    steps,
  });
}

// materialize exact lifecycle helpers for controlled local tests
export function materializeSyntheticBlueberryLifecycleHelpers({ rootDir }) {
  // confine the noncanonical helper roots to one normalized temporary path
  if (typeof rootDir !== "string" || resolve(rootDir) !== rootDir || !rootDir.startsWith("/tmp/") ||
    !/^\/tmp\/[A-Za-z0-9._/-]+$/u.test(rootDir) || rootDir.split("/").includes("..")) {
    throw codedError("receipt.authority", "synthetic lifecycle root is invalid");
  }
  const stageRoot = `${rootDir}/admin-stage`;
  const runRoot = `${rootDir}/run-stage`;
  return freezeCanonical({
    canonicalEligible: false,
    cleanupSource: CLEANUP_HELPER_SOURCE.replaceAll(
      "/tmp/weather-backend-host-observation-v2-admin-stage",
      stageRoot,
    ),
    rootSource: ROOT_HELPER_SOURCE
      .replaceAll("/tmp/weather-backend-host-observation-v2-admin-stage", stageRoot)
      .replaceAll("/run/weather-backend-host-observation-v2-blueberry", runRoot)
      .replaceAll('NODE="/usr/bin/node"', `NODE=${JSON.stringify(process.execPath)}`),
    runRoot,
    stageRoot,
    stageSource: STAGE_HELPER_SOURCE.replaceAll(
      "/tmp/weather-backend-host-observation-v2-admin-stage",
      stageRoot,
    ),
  });
}

// materialize the exact workstation helper over caller-owned temporary paths
export function materializeSyntheticWorkstationRootHelper(input) {
  const keys = new Set(["repositoryRoot", "requestPath", "runRoot"]);

  // confine every synthetic local path to one normalized temporary tree
  if (!hasExactKeys(input, keys) || [input.repositoryRoot, input.requestPath, input.runRoot].some(
    // reject traversal aliases and production paths
    (path) => typeof path !== "string" || resolve(path) !== path || !path.startsWith("/tmp/") ||
      !/^\/tmp\/[A-Za-z0-9._/-]+$/u.test(path) || path.split("/").includes(".."),
  )) {
    throw codedError("receipt.authority", "synthetic workstation helper paths are invalid");
  }
  const uid = typeof process.getuid === "function" ? process.getuid() : 1000;
  const gid = typeof process.getgid === "function" ? process.getgid() : 1000;
  return freezeCanonical({
    canonicalEligible: false,
    rootSource: ROOT_HELPER_SOURCE
      .replace('SOURCE="/tmp/weather-backend-host-observation-v2-admin-stage" if REMOTE else "/home/ubuntu/weather"',
        `SOURCE="/tmp/weather-backend-host-observation-v2-admin-stage" if REMOTE else ${JSON.stringify(input.repositoryRoot)}`)
      .replace('ROOT="/run/weather-backend-host-observation-v2-blueberry" if REMOTE else "/run/weather-backend-host-observation-v2-workstation"',
        `ROOT="/run/weather-backend-host-observation-v2-blueberry" if REMOTE else ${JSON.stringify(input.runRoot)}`)
      .replace('NODE="/usr/bin/node" if REMOTE else "/home/ubuntu/n/bin/node"',
        `NODE="/usr/bin/node" if REMOTE else ${JSON.stringify(process.execPath)}`)
      .replace('REQUEST="/home/ubuntu/.weather/backend-performance-evidence/v2/incoming/workstation-runner/request.json"',
        `REQUEST=${JSON.stringify(input.requestPath)}`)
      .replace("MAIN_UID=1000\nMAIN_GID=1000", `MAIN_UID=${String(uid)}\nMAIN_GID=${String(gid)}`),
  });
}

// add one stable plan diagnostic
function planDiagnostic(diagnostics, code, path, message) {
  diagnostics.push({ code, message, path });
}

// validate the fixed authenticated-host trust bytes
function validateCaptureTrust(trustPolicyBytes, knownHostsBytes, retained) {
  // require bounded exact byte inputs
  if (!Buffer.isBuffer(trustPolicyBytes) || !Buffer.isBuffer(knownHostsBytes) ||
    trustPolicyBytes.length === 0 || trustPolicyBytes.length > 65_536 ||
    knownHostsBytes.length === 0 || knownHostsBytes.length > 65_536) {
    throw codedError("receipt.authority", "capture trust bytes are invalid");
  }
  const policy = parseCanonicalReceiptBytes(trustPolicyBytes, "receipt.authority");

  // bind the complete finite policy and known-host bytes
  if (canonicalHostJson(policy) !== canonicalHostJson(CAPTURE_POLICY) ||
    sha256Bytes(trustPolicyBytes) !== POLICY_SHA256 ||
    knownHostsBytes.length !== CAPTURE_POLICY.knownHosts.byteLength ||
    sha256Bytes(knownHostsBytes) !== KNOWN_HOSTS_SHA256) {
    throw codedError("receipt.authority", "capture trust policy or known-hosts bytes differ");
  }
  const line = knownHostsBytes.toString("utf8");
  const fields = line.endsWith("\n") ? line.slice(0, -1).split(" ") : [];

  // require the sole reviewed ED25519 anchor and fingerprint
  if (fields.length !== 3 || fields[1] !== "ssh-ed25519" || !/^\|1\|[^|]+\|[^|]+$/u.test(fields[0])) {
    throw codedError("receipt.authority", "known-hosts anchor is invalid");
  }
  const keyBytes = decodeCanonicalBase64(fields[2], "receipt.authority", "known-hosts key");
  const fingerprint = `SHA256:${createHash("sha256").update(keyBytes).digest("base64").replace(/=+$/u, "")}`;

  // bind the exact reviewed host-key fingerprint
  if (fingerprint !== BLUEBERRY_FINGERPRINT) {
    throw codedError("receipt.authority", "known-hosts fingerprint differs");
  }
  // retain both exact trust inputs when assembling a receipt
  if (retained instanceof Map) {
    retainArtifact(retained, trustPolicyBytes);
    retainArtifact(retained, knownHostsBytes);
  }
  return { knownHostsBytes, policy, policySha256: POLICY_SHA256, trustPolicyBytes };
}

// materialize one exact capture plan from source bytes
function materializeCapturePlan(operationId, packageBytesByName, trustedBytesByName) {
  const template = MAIN_CAPTURE_PLANS[operationId];
  const expectedNames = [
    "backend-runtime-inventory.json",
    "backend-runtime-live-map.json",
    "observer.mjs",
    "output.json",
    "request.json",
  ];

  const trustedNames = expectedNames.filter(
    // exclude collected output from pre-execution trust
    (name) => name !== "output.json",
  );

  // reject unknown operations and open packages
  if (template === undefined || !(packageBytesByName instanceof Map) ||
    canonicalHostJson([...packageBytesByName.keys()].sort()) !== canonicalHostJson(expectedNames) ||
    !(trustedBytesByName instanceof Map) ||
    canonicalHostJson([...trustedBytesByName.keys()].sort()) !== canonicalHostJson(trustedNames)) {
    throw codedError("receipt.authority", "capture package names or operation are invalid");
  }
  const inputs = expectedNames.map(
    // bind each package member to exact bytes
    (name) => {
      const stagedBytes = packageBytesByName.get(name);
      const bytes = name === "output.json" ? stagedBytes : trustedBytesByName.get(name);

      // require one bounded byte member
      if (!Buffer.isBuffer(bytes) || bytes.length === 0 || bytes.length > MAX_ARTIFACT_BYTES ||
        !Buffer.isBuffer(stagedBytes) || !stagedBytes.equals(bytes)) {
        throw codedError("receipt.authority", `capture package member ${name} is invalid`);
      }
      return { byteLength: bytes.length, name, sha256: sha256Bytes(bytes) };
    },
  );
  const inputDigests = new Map(inputs.map(
    // index exact package member digests
    (input) => [input.name, input.sha256],
  ));
  const trustedDigests = [
    inputDigests.get("observer.mjs"),
    inputDigests.get("backend-runtime-inventory.json"),
    inputDigests.get("backend-runtime-live-map.json"),
    inputDigests.get("request.json"),
  ];
  const steps = template.steps.map(
    // materialize only the two fixed collect vector forms
    (step) => {
      // leave the Blueberry stage vector unchanged
      if (step.id === "blueberry.stage-package") {
        return step;
      }
      // append validated hashes inside the one fixed SSH remote command
      if (step.id === "blueberry.collect") {
        return {
          ...step,
          argv: [...step.argv.slice(0, -1), `${step.argv.at(-1)} ${trustedDigests.join(" ")}`],
        };
      }
      // append the same trusted hashes to the fixed cleanup command
      if (step.id === "blueberry.cleanup") {
        return {
          ...step,
          argv: [...step.argv.slice(0, -1), `${step.argv.at(-1)} ${trustedDigests.join(" ")}`],
        };
      }
      // append validated hashes as fixed sudo-helper positional arguments
      if (step.id === "workstation.collect") {
        return { ...step, argv: [...step.argv, ...trustedDigests] };
      }
      throw codedError("receipt.authority", "capture plan contains an unknown step");
    },
  );
  return freezeCanonical({
    contractVersion: "weather-backend-main-capture-plan/v1",
    inputs,
    operationId,
    profile: template.profile,
    steps,
  });
}

// validate one capture against exact retained plan bytes
function validateMainCaptureWithPlan({
  operationId,
  packageBytesByName,
  captureBytes,
  knownHostsBytes,
  planBytes,
  template = MAIN_CAPTURE_PLANS[operationId],
  trustPolicyBytes,
}) {
  const diagnostics = [];
  let plan = null;
  let capture = null;

  // parse the exact retained plan and trust inputs
  try {
    validateCaptureTrust(trustPolicyBytes, knownHostsBytes);
    plan = parseCanonicalReceiptBytes(planBytes, "receipt.authority");
    capture = parseCanonicalReceiptBytes(captureBytes, "receipt.authority");
  } catch (error) {
    planDiagnostic(diagnostics, error.code ?? "receipt.authority", "$", error.message);
  }
  const expectedNames = [
    "backend-runtime-inventory.json",
    "backend-runtime-live-map.json",
    "observer.mjs",
    "output.json",
    "request.json",
  ];
  const expectedInputs = expectedNames.map(
    // bind every retained plan input to exact package bytes
    (name) => ({ byteLength: packageBytesByName.get(name)?.length, name, sha256: sha256Bytes(packageBytesByName.get(name) ?? Buffer.alloc(0)) }),
  );
  const planKeys = new Set(["contractVersion", "inputs", "operationId", "profile", "steps"]);

  // require the closed retained plan root and exact input identities
  if (plan !== null && (!hasExactKeys(plan, planKeys) || plan.contractVersion !== "weather-backend-main-capture-plan/v1" ||
    plan.operationId !== operationId || plan.profile !== template?.profile ||
    canonicalHostJson(plan.inputs) !== canonicalHostJson(expectedInputs) || !Array.isArray(plan.steps))) {
    planDiagnostic(diagnostics, "receipt.authority", "$.plan", "retained capture plan is invalid");
  }
  const captureKeys = new Set(["contractVersion", "operationId", "outputSha256", "planSha256", "policySha256", "requestSha256", "steps"]);

  // validate the closed capture root
  if (plan !== null && (!hasExactKeys(capture, captureKeys) ||
    capture.contractVersion !== "weather-backend-main-capture/v1" ||
    capture.operationId !== operationId || capture.policySha256 !== POLICY_SHA256 ||
    capture.planSha256 !== receiptSha256(plan) ||
    capture.requestSha256 !== sha256Bytes(packageBytesByName.get("request.json")) ||
    capture.outputSha256 !== sha256Bytes(packageBytesByName.get("output.json")) ||
    !Array.isArray(capture.steps) || capture.steps.length !== plan.steps.length)) {
    planDiagnostic(diagnostics, "receipt.authority", "$.capture", "capture root or plan binding is invalid");
  }
  let previousCompleted = null;

  // validate every exact executed vector and result
  for (let index = 0; plan !== null && index < plan.steps.length && Array.isArray(capture?.steps); index += 1) {
    const expected = plan.steps[index];
    const actual = capture.steps[index];
    const stepKeys = new Set(["argv", "completedAt", "cwd", "environment", "exitCode", "id", "program", "signal", "startedAt", "stderr", "stdinKind", "stdout"]);
    const stderrKeys = new Set(["byteLength", "dataBase64", "sha256"]);
    const stdoutKeys = new Set(["byteLength", "sha256"]);
    const collect = expected.id.endsWith(".collect");

    // require exact vectors and closed outcomes
    if (!hasExactKeys(actual, stepKeys) ||
      canonicalHostJson({ argv: actual?.argv, cwd: actual?.cwd, environment: actual?.environment, id: actual?.id, program: actual?.program, stdinKind: actual?.stdinKind }) !==
        canonicalHostJson({ argv: expected.argv, cwd: expected.cwd, environment: expected.environment, id: expected.id, program: expected.program, stdinKind: expected.stdinKind }) ||
      !isTimestamp(actual?.startedAt) || !isTimestamp(actual?.completedAt) ||
      Date.parse(actual?.startedAt) > Date.parse(actual?.completedAt) ||
      (previousCompleted !== null && Date.parse(actual.startedAt) <= previousCompleted) ||
      actual?.exitCode !== 0 || actual?.signal !== null ||
      !hasExactKeys(actual?.stderr, stderrKeys) || actual.stderr.byteLength !== 0 ||
      actual.stderr.sha256 !== EMPTY_SHA256 || actual.stderr.dataBase64 !== "" ||
      !hasExactKeys(actual?.stdout, stdoutKeys) ||
      (collect
        ? actual.stdout.byteLength !== packageBytesByName.get("output.json")?.length || actual.stdout.sha256 !== capture.outputSha256
        : actual.stdout.byteLength !== 0 || actual.stdout.sha256 !== EMPTY_SHA256)) {
      planDiagnostic(diagnostics, "receipt.authority", `$.steps[${String(index)}]`, "capture step differs from the fixed plan or outcome");
    }
    previousCompleted = Date.parse(actual?.completedAt);
  }
  diagnostics.sort(
    // keep plan diagnostics deterministic
    (left, right) => canonicalHostJson(left).localeCompare(canonicalHostJson(right)),
  );
  return {
    canonicalEligible: false,
    diagnostics,
    ok: diagnostics.length === 0,
    planSha256: plan === null ? null : receiptSha256(plan),
  };
}

// bind one retained plan helper body to its retained writer source
function validateRetainedPlanSource(planBytes, operationId, writerBytes) {
  const plan = parseCanonicalReceiptBytes(planBytes, "receipt.authority");
  const template = retainedMainCaptureV1Template(operationId, writerBytes);
  const inputDigests = new Map(plan.inputs?.map(
    // index every retained plan input
    (input) => [input.name, input.sha256],
  ));
  const trusted = [
    inputDigests.get("observer.mjs"),
    inputDigests.get("backend-runtime-inventory.json"),
    inputDigests.get("backend-runtime-live-map.json"),
    inputDigests.get("request.json"),
  ];

  // require one complete source-owned template shape
  if (!Buffer.isBuffer(writerBytes) || template === undefined || !Array.isArray(plan.steps) ||
    plan.steps.length !== template.steps.length || trusted.some((value) => !digestPattern.test(value ?? ""))) {
    throw codedError("receipt.authority", "retained capture plan source binding is invalid");
  }
  // validate each immutable v1 vector with only its four trusted input digests materialized
  for (let index = 0; index < template.steps.length; index += 1) {
    const expected = template.steps[index];
    const actual = plan.steps[index];
    const expectedVector = structuredClone(expected);

    // materialize only the two hash-bearing fixed vectors
    if (["blueberry.collect", "blueberry.cleanup"].includes(expected.id)) {
      expectedVector.argv = [...expected.argv.slice(0, -1), `${expected.argv.at(-1)} ${trusted.join(" ")}`];
    } else if (expected.id === "workstation.collect") {
      expectedVector.argv = [...expected.argv, ...trusted];
    }
    // reject every helper or non-helper vector substitution
    if (canonicalHostJson(actual) !== canonicalHostJson(expectedVector)) {
      throw codedError("receipt.authority", `retained plan step ${actual.id ?? String(index)} vector differs`);
    }
  }
}

// validate one nonexecuting fixed Main capture package fixture
export function validateSyntheticMainCapturePackage({
  operationId,
  packageBytesByName,
  trustedBytesByName,
  captureBytes,
  knownHostsBytes,
  trustPolicyBytes,
}) {
  let planBytes = Buffer.alloc(0);

  try {
    planBytes = canonicalReceiptBytes(materializeCapturePlan(operationId, packageBytesByName, trustedBytesByName));
  } catch (error) {
    return {
      canonicalEligible: false,
      diagnostics: [{ code: error.code ?? "receipt.authority", message: error.message, path: "$" }],
      ok: false,
      planSha256: null,
    };
  }
  return validateMainCaptureWithPlan({
    captureBytes,
    knownHostsBytes,
    operationId,
    packageBytesByName,
    planBytes,
    trustPolicyBytes,
  });
}

// decode one canonical base64 field
function decodeCanonicalBase64(value, code, label) {
  // reject noncanonical or unbounded text
  if (typeof value !== "string" || value.length > MAX_ARTIFACT_BYTES * 2 || !/^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$/u.test(value)) {
    throw codedError(code, `${label} is not canonical base64`);
  }
  const bytes = Buffer.from(value, "base64");

  // reject alternate encodings
  if (bytes.toString("base64") !== value) {
    throw codedError(code, `${label} is not canonical base64`);
  }
  return bytes;
}

// validate one fixed native process result
function validateFixedProcessResult(result, expected, code = "receipt.authority") {
  const keys = new Set([
    "argv", "completedAt", "cwd", "environment", "exitCode", "program", "signal", "startedAt",
    "stderrBase64", "stdoutBase64",
  ]);

  // require the exact process vector and successful interval
  if (!hasExactKeys(result, keys) || result.program !== expected.program || result.cwd !== expected.cwd ||
    canonicalHostJson(result.argv) !== canonicalHostJson(expected.argv) ||
    canonicalHostJson(result.environment) !== canonicalHostJson(expected.environment) ||
    result.exitCode !== 0 || result.signal !== null ||
    !isTimestamp(result.startedAt) || !isTimestamp(result.completedAt) ||
    Date.parse(result.startedAt) > Date.parse(result.completedAt)) {
    throw codedError(code, "fixed native process result is invalid");
  }
  const stdout = decodeCanonicalBase64(result.stdoutBase64, code, "stdout");
  const stderr = decodeCanonicalBase64(result.stderrBase64, code, "stderr");

  // require one silent control process
  if (stdout.length !== 0 || stderr.length !== 0) {
    throw codedError(code, "fixed native process emitted unexpected bytes");
  }
  return { completedAt: result.completedAt, startedAt: result.startedAt };
}

// validate the exact committed measurement source proof
function validateMeasurementSourceProof(receiptBytes, captureBytes, retained, options = { bindExecutingBytes: true }) {
  const receipt = parseCanonicalReceiptBytes(receiptBytes, "receipt.schema");
  const capture = parseCanonicalReceiptBytes(captureBytes, "receipt.schema");
  const receiptKeys = new Set(["captureSha256", "commit", "contractVersion", "files", "repository"]);
  const captureKeys = new Set(["commit", "completedAt", "contractVersion", "repository", "startedAt", "steps"]);

  // bind the fixed measurement receipt and native capture
  if (!hasExactKeys(receipt, receiptKeys) || receipt.contractVersion !== "weather-backend-measurement-source-receipt/v2" ||
    receipt.repository !== "anstosa/Ballydidean-Weather" || !/^[a-f0-9]{40}$/u.test(receipt.commit ?? "") ||
    receipt.captureSha256 !== sha256Bytes(captureBytes) || !hasExactKeys(capture, captureKeys) ||
    capture.contractVersion !== "weather-backend-measurement-git-capture/v1" ||
    capture.repository !== receipt.repository || capture.commit !== receipt.commit ||
    !isTimestamp(capture.startedAt) || !isTimestamp(capture.completedAt) ||
    Date.parse(capture.startedAt) > Date.parse(capture.completedAt) || !Array.isArray(receipt.files) ||
    !Array.isArray(capture.steps)) {
    throw codedError("receipt.schema", "measurement source receipt or capture is invalid");
  }
  // require only the closed validation mode
  if (!hasExactKeys(options, new Set(["bindExecutingBytes"])) || typeof options.bindExecutingBytes !== "boolean") {
    throw codedError("receipt.schema", "measurement source validation mode is invalid");
  }
  const expectedFiles = measurementSourcePaths.map(
    // derive every fixed committed source path
    (path, index) => ({ index, path, value: receipt.files[index] }),
  );

  // bind the retained receipt to the fixed source path set
  if (receipt.files.length !== expectedFiles.length || capture.steps.length !== expectedFiles.length + 1 ||
    expectedFiles.some(
      // require one exact retained source identity
      ({ path, value }) => !hasExactKeys(value, new Set(["path", "sha256"])) || value.path !== path ||
        !digestPattern.test(value.sha256 ?? ""),
    )) {
    throw codedError("receipt.digest", "measurement source files are invalid");
  }
  const expectedSteps = [
    {
      argv: ["rev-parse", `${receipt.commit}^{commit}`],
      id: "measurement.git.commit",
      path: null,
    },
    ...measurementSourcePaths.map(
      // bind one exact git-show operation
      (path) => ({
        argv: ["show", `${receipt.commit}:${path}`],
        id: `measurement.git.show:${path}`,
        path,
      }),
    ),
  ];
  let priorCompleted = null;
  const sourceBytesByPath = new Map();

  // validate every full raw git result
  for (let index = 0; index < expectedSteps.length; index += 1) {
    const actual = capture.steps[index];
    const expected = expectedSteps[index];
    const keys = new Set([
      "argv", "completedAt", "cwd", "environment", "exitCode", "id", "program", "signal",
      "startedAt", "stderrBase64", "stdoutBase64",
    ]);

    // require the source-owned vector and interval
    if (!hasExactKeys(actual, keys) || actual.id !== expected.id || actual.program !== "/usr/bin/git" ||
      actual.cwd !== repositoryRoot || canonicalHostJson(actual.argv) !== canonicalHostJson(expected.argv) ||
      canonicalHostJson(actual.environment) !== canonicalHostJson(SOURCE_PROCESS_ENVIRONMENT) ||
      actual.exitCode !== 0 || actual.signal !== null || !isTimestamp(actual.startedAt) || !isTimestamp(actual.completedAt) ||
      Date.parse(actual.startedAt) > Date.parse(actual.completedAt) ||
      Date.parse(actual.startedAt) < Date.parse(capture.startedAt) || Date.parse(actual.completedAt) > Date.parse(capture.completedAt) ||
      (priorCompleted !== null && Date.parse(actual.startedAt) <= priorCompleted)) {
      throw codedError("receipt.authority", `measurement source step ${String(index)} is invalid`);
    }
    const stdout = decodeCanonicalBase64(actual.stdoutBase64, "receipt.authority", `${actual.id} stdout`);
    const stderr = decodeCanonicalBase64(actual.stderrBase64, "receipt.authority", `${actual.id} stderr`);

    const retainedBytes = expected.path === null
      ? Buffer.from(`${receipt.commit}\n`, "utf8")
      : stdout;

    // bind every retained git result to the receipt identity
    if (stderr.length !== 0 || (expected.path === null && !stdout.equals(retainedBytes)) ||
      (expected.path !== null && sha256Bytes(stdout) !== receipt.files[index - 1].sha256)) {
      throw codedError("receipt.digest", `measurement source step ${actual.id} bytes differ`);
    }
    // bind incoming source proof to the currently executing support bytes
    if (options.bindExecutingBytes && expected.path !== null &&
      !stdout.equals(readFileSync(resolve(repositoryRoot, expected.path)))) {
      throw codedError("receipt.digest", `measurement source step ${actual.id} differs from executing bytes`);
    }
    // retain every committed source blob for historical semantic validation
    if (expected.path !== null) {
      sourceBytesByPath.set(expected.path, stdout);
    }
    priorCompleted = Date.parse(actual.completedAt);
  }
  retainArtifact(retained, captureBytes);
  return { capture, receipt, sourceBytesByPath };
}

// validate one portable deployed-source identity
function validateDeployedIdentity(identity, expectedType, expectedMode, label) {
  const keys = new Set([
    "ctimeNs", "dev", "gid", "ino", "mode", "mtimeNs", "nlink", "size", "type", "uid",
  ]);
  const decimal = /^(?:0|[1-9][0-9]*)$/u;

  // require one closed root-owned descriptor identity
  if (!hasExactKeys(identity, keys) || identity.type !== expectedType || identity.mode !== expectedMode ||
    identity.uid !== 0 || identity.gid !== 0 ||
    ![identity.ctimeNs, identity.dev, identity.ino, identity.mtimeNs, identity.nlink, identity.size].every(
      // require portable unsigned scalar strings
      (value) => typeof value === "string" && decimal.test(value),
    )) {
    throw codedError("receipt.binding", `${label} identity is invalid`);
  }
}

// decode one exact deployed-source regular proof
function validateRegularProof(proof, expectedPath, maximum, label) {
  const keys = new Set(["identity", "path", "raw"]);
  const rawKeys = new Set(["byteLength", "dataBase64", "encoding", "sha256"]);

  // require the exact descriptor path and raw envelope
  if (!hasExactKeys(proof, keys) || proof.path !== expectedPath || !hasExactKeys(proof.raw, rawKeys) ||
    proof.raw.encoding !== "base64" || !Number.isSafeInteger(proof.raw.byteLength) || proof.raw.byteLength <= 0 ||
    proof.raw.byteLength > maximum || !digestPattern.test(proof.raw.sha256 ?? "")) {
    throw codedError("receipt.schema", `${label} proof is invalid`);
  }
  validateDeployedIdentity(proof.identity, "regular-file", "0600", label);

  // require one single-link stable source file
  if (proof.identity.nlink !== "1" || proof.identity.size !== String(proof.raw.byteLength)) {
    throw codedError("receipt.binding", `${label} metadata differs from raw bytes`);
  }
  const bytes = decodeCanonicalBase64(proof.raw.dataBase64, "receipt.schema", `${label} raw`);

  // bind the retained bytes to metadata and digest
  if (bytes.length !== proof.raw.byteLength || sha256Bytes(bytes) !== proof.raw.sha256) {
    throw codedError("receipt.digest", `${label} raw bytes differ`);
  }
  return bytes;
}

// parse one exact nonsecret release environment
function validateReleaseEnvironment(bytes, release) {
  let text;

  try {
    text = new TextDecoder("utf-8", { fatal: true }).decode(bytes);
  } catch (error) {
    throw codedError("receipt.schema", "release environment is not UTF-8", error);
  }
  // require one final newline and plain assignments only
  if (!text.endsWith("\n") || text.includes("\r") || text.includes("\0")) {
    throw codedError("receipt.schema", "release environment encoding is invalid");
  }
  const lines = text.slice(0, -1).split("\n");
  const hasBlank = lines.includes("");
  const entries = lines.filter(
    // retain blank separators only for the pinned legacy format
    (line) => line !== "",
  ).map(
    // parse one deterministic assignment
    (line) => {
      const index = line.indexOf("=");

      // reject blanks malformed keys and duplicate separators
      if (index <= 0) {
        throw codedError("receipt.schema", "release environment assignment is invalid");
      }
      return [line.slice(0, index), line.slice(index + 1)];
    },
  );
  const currentKeys = [
    "WEATHER_RELEASE", "WEATHER_SERVER_IMAGE", "WEATHER_WEB_IMAGE", "POSTGRES_IMAGE", "CLOUDFLARED_IMAGE",
    "WEATHER_DATABASE_NAME", "WEATHER_POSTGRES_DIR", "WEATHER_FORECAST_ADJUSTMENT_WIND_CANARY_KILL_SWITCH",
    "WEATHER_FORECAST_ADJUSTMENT_TEMPERATURE_CANARY_KILL_SWITCH", "WEATHER_CONTROL_PLANE_SHA256",
    "WEATHER_CONTROL_PLANE_VERSION",
  ];
  const legacyKeys = currentKeys.filter(
    // retain the fixed legacy omission only
    (key) => key !== "WEATHER_FORECAST_ADJUSTMENT_TEMPERATURE_CANARY_KILL_SWITCH",
  );
  const keys = entries.map(
    // retain exact source order
    ([key]) => key,
  );

  // reject unknown duplicated or credential-bearing fields
  if ((canonicalHostJson(keys) !== canonicalHostJson(currentKeys) && canonicalHostJson(keys) !== canonicalHostJson(legacyKeys)) ||
    (hasBlank && canonicalHostJson(keys) !== canonicalHostJson(legacyKeys))) {
    throw codedError("receipt.schema", "release environment keys are invalid");
  }
  const values = Object.fromEntries(entries);

  // bind only the existing nonsecret release contract
  if (values.WEATHER_RELEASE !== release || !/^\d{4}\.\d{2}\.\d{2}-[1-9]\d?$/u.test(release) ||
    !["WEATHER_SERVER_IMAGE", "WEATHER_WEB_IMAGE", "POSTGRES_IMAGE", "CLOUDFLARED_IMAGE"].every(
      // require digest-pinned images
      (key) => /^[^@\s]+@sha256:[a-f0-9]{64}$/u.test(values[key] ?? ""),
    ) || !/^[a-z][a-z0-9_]{0,62}$/u.test(values.WEATHER_DATABASE_NAME ?? "") ||
    !/^\/var\/lib\/weather(?:\/[A-Za-z0-9._-]+)+$/u.test(values.WEATHER_POSTGRES_DIR ?? "") ||
    ![values.WEATHER_FORECAST_ADJUSTMENT_WIND_CANARY_KILL_SWITCH,
      values.WEATHER_FORECAST_ADJUSTMENT_TEMPERATURE_CANARY_KILL_SWITCH ?? "0"].every(
      // require one fixed canary bit
      (value) => ["0", "1"].includes(value),
    ) || !digestPattern.test(values.WEATHER_CONTROL_PLANE_SHA256 ?? "") ||
    !/^[1-9]\d*$/u.test(values.WEATHER_CONTROL_PLANE_VERSION ?? "")) {
    throw codedError("receipt.schema", "release environment values are invalid");
  }
  // accept only the pinned legacy contract
  if (canonicalHostJson(keys) === canonicalHostJson(legacyKeys) &&
    (values.WEATHER_CONTROL_PLANE_VERSION !== "6" ||
      values.WEATHER_CONTROL_PLANE_SHA256 !== "c4d74581b84505e065fdec63447dfdded1d14221e459777a88e37729275f33b5")) {
    throw codedError("receipt.binding", "legacy release environment control identity is invalid");
  }
  return values;
}

// compare only stable recertified publication identities
function stablePublicationIdentity(publication) {
  return {
    contractVersion: publication.contractVersion,
    operatorSha256: publication.operatorSha256,
    serverImage: publication.serverImage,
    serverImageId: publication.serverImageId,
    sourceTree: publication.sourceTree,
    targetCommit: publication.targetCommit,
    targetRelease: publication.targetRelease,
    targetTree: publication.targetTree,
    validatorSha256: publication.validatorSha256,
    webImage: publication.webImage,
    webImageId: publication.webImageId,
  };
}

// validate the existing protected-state snapshot schema
function validateProtectedSnapshot(bytes) {
  const value = parseStrictSourceJson(bytes, "protected snapshot");
  const paths = [
    "/opt/weather/current/deploy/state/schema-release",
    "/opt/weather/current/deploy/state/adjustment-candidate-catalog.json",
    "/opt/weather/current/deploy/state/adjustment-capture-epoch.json",
    "/opt/weather/current/deploy/state/adjustment-release-state/current.json",
    "/var/lib/weather/xweather/forecast-adjustment-settings.json",
  ];

  // require one exact bounded protected-state record
  if (!hasExactKeys(value, new Set(["files", "infrastructure"])) ||
    typeof value.infrastructure !== "string" || value.infrastructure.length === 0 ||
    value.infrastructure.length > 65_536 || value.infrastructure.includes("\0") ||
    !Array.isArray(value.files) || value.files.length !== paths.length) {
    throw codedError("receipt.schema", "protected snapshot is invalid");
  }

  // validate every fixed protected metadata row
  for (let index = 0; index < paths.length; index += 1) {
    const item = value.files[index];

    // bind exact path metadata without assigning a source commit
    if (!hasExactKeys(item, new Set(["inode", "mode", "owner", "path", "sha256"])) ||
      item.path !== paths[index] || !/^\d+:\d+$/u.test(item.inode ?? "") ||
      !Number.isSafeInteger(item.mode) || item.mode <= 0 || !/^\d+:\d+$/u.test(item.owner ?? "") ||
      !digestPattern.test(item.sha256 ?? "")) {
      throw codedError("receipt.schema", `protected snapshot file ${String(index)} is invalid`);
    }
  }
  return value;
}

// validate one fixed deployed-source native capture
function validateDeployedSourceCapture(capture, bundleBytes, planBytes, writerBytes) {
  const plan = parseCanonicalReceiptBytes(planBytes, "receipt.authority");
  const expectedPlan = retainedDeployedSourceV1Plan(writerBytes);
  const rootKeys = new Set(["bundleSha256", "contractVersion", "operationId", "planSha256", "policySha256", "steps"]);
  const stepKeys = new Set([
    "argv", "completedAt", "cwd", "environment", "exitCode", "id", "program", "signal",
    "startedAt", "stderr", "stdinKind", "stdout",
  ]);
  const step = capture?.steps?.[0];
  const expected = expectedPlan.steps[0];

  // require the exact source-owned retained plan and native outcome
  if (canonicalHostJson(plan) !== canonicalHostJson(expectedPlan) ||
    !hasExactKeys(capture, rootKeys) || capture.contractVersion !== "weather-backend-deployed-source-main-capture/v1" ||
    capture.operationId !== expectedPlan.operationId || capture.planSha256 !== receiptSha256(plan) ||
    capture.policySha256 !== POLICY_SHA256 || capture.bundleSha256 !== sha256Bytes(bundleBytes) ||
    !Array.isArray(capture.steps) || capture.steps.length !== 1 || !hasExactKeys(step, stepKeys) ||
    canonicalHostJson({ argv: step?.argv, cwd: step?.cwd, environment: step?.environment, id: step?.id, program: step?.program, stdinKind: step?.stdinKind }) !==
      canonicalHostJson({ argv: expected.argv, cwd: expected.cwd, environment: expected.environment, id: expected.id, program: expected.program, stdinKind: expected.stdinKind }) ||
    !isTimestamp(step?.startedAt) || !isTimestamp(step?.completedAt) || Date.parse(step.startedAt) > Date.parse(step.completedAt) ||
    step.exitCode !== 0 || step.signal !== null || !hasExactKeys(step.stderr, new Set(["byteLength", "dataBase64", "sha256"])) ||
    step.stderr.byteLength !== 0 || step.stderr.dataBase64 !== "" || step.stderr.sha256 !== EMPTY_SHA256 ||
    !hasExactKeys(step.stdout, new Set(["byteLength", "sha256"])) || step.stdout.byteLength !== bundleBytes.length ||
    step.stdout.sha256 !== sha256Bytes(bundleBytes)) {
    throw codedError("receipt.authority", "deployed source capture differs from the fixed plan or outcome");
  }
  return { completedAt: step.completedAt, startedAt: step.startedAt };
}

// validate the full deployed source proof and retained host bytes
function validateDeployedSourceProof(documents, retained, writerBytes) {
  const proof = parseCanonicalReceiptBytes(documents.proofBytes, "receipt.schema");
  const hostCapture = parseCanonicalReceiptBytes(documents.hostCaptureBytes, "receipt.schema");
  const hostBundle = parseCanonicalReceiptBytes(documents.hostBundleBytes, "receipt.schema");
  const recertified = parseStrictSourceJson(documents.recertifiedPublicationBytes, "recertified publication");
  const preparer = parseCanonicalReceiptBytes(documents.preparerCaptureBytes, "receipt.schema");
  const proofKeys = new Set([
    "contractVersion", "hostBundleSha256", "hostCaptureSha256", "hostPlanSha256", "preparerCaptureSha256",
    "recertifiedPublicationSha256",
  ]);

  // bind every retained deployed-source document
  if (!hasExactKeys(proof, proofKeys) || proof.contractVersion !== "weather-backend-deployed-source-proof/v1" ||
    proof.hostBundleSha256 !== sha256Bytes(documents.hostBundleBytes) ||
    proof.hostCaptureSha256 !== sha256Bytes(documents.hostCaptureBytes) ||
    proof.hostPlanSha256 !== sha256Bytes(documents.hostPlanBytes) ||
    proof.preparerCaptureSha256 !== sha256Bytes(documents.preparerCaptureBytes) ||
    proof.recertifiedPublicationSha256 !== sha256Bytes(documents.recertifiedPublicationBytes)) {
    throw codedError("receipt.digest", "deployed source proof digests differ");
  }
  const captureInterval = validateDeployedSourceCapture(
    hostCapture,
    documents.hostBundleBytes,
    documents.hostPlanBytes,
    writerBytes,
  );
  const bundleKeys = new Set([
    "activeAlias", "contractVersion", "currentRelease", "operationId", "protected", "publication",
    "ready", "releaseEnvironment", "sourceRoot", "stage",
  ]);

  // require the fixed host bundle root and pinned source directory
  if (!hasExactKeys(hostBundle, bundleKeys) || hostBundle.contractVersion !== "weather-backend-deployed-source-capture/v1" ||
    hostBundle.operationId !== DEPLOYED_SOURCE_CAPTURE_V1_DESCRIPTOR.operationId ||
    !hasExactKeys(hostBundle.sourceRoot, new Set(["identity", "path"])) ||
    hostBundle.sourceRoot.path !== "/opt/weather/current") {
    throw codedError("receipt.schema", "deployed source bundle is invalid");
  }
  validateDeployedIdentity(hostBundle.sourceRoot.identity, "directory", "0755", "source root");
  const markerBytes = validateRegularProof(
    hostBundle.currentRelease,
    "/opt/weather/current/deploy/state/current-release",
    128,
    "current release",
  );
  const release = markerBytes.toString("ascii").replace(/\n$/u, "");

  // require the exact private marker grammar
  if (!/^\d{4}\.\d{2}\.\d{2}-[1-9]\d?$/u.test(release) || !markerBytes.equals(Buffer.from(`${release}\n`, "utf8"))) {
    throw codedError("receipt.binding", "current release marker is invalid");
  }
  const stateRoot = `/opt/weather/current/deploy/state/schema-preserving-release-v1/${release}`;
  const publicationBytes = validateRegularProof(hostBundle.publication, `${stateRoot}/publication.json`, 1_048_576, "publication");
  const stageBytes = validateRegularProof(hostBundle.stage, `${stateRoot}/stage.json`, 1_048_576, "stage");
  const readyBytes = validateRegularProof(hostBundle.ready, `${stateRoot}/ready.json`, 1_048_576, "ready");
  const protectedBytes = validateRegularProof(hostBundle.protected, `${stateRoot}/protected.json`, 1_048_576, "protected");
  const environmentBytes = validateRegularProof(
    hostBundle.releaseEnvironment,
    `/opt/weather/current/deploy/releases/${release}.env`,
    65_536,
    "release environment",
  );
  const historical = parseStrictSourceJson(publicationBytes, "historical publication");
  const stage = parseStrictSourceJson(stageBytes, "stage receipt");
  const environment = validateReleaseEnvironment(environmentBytes, release);
  validateProtectedSnapshot(protectedBytes);

  try {
    validatePublication(historical, release);
    validatePublication(recertified, release);
    validateStageReceipt(stage, release);
  } catch (error) {
    throw codedError("receipt.binding", "publication or stage evidence is invalid", error);
  }
  // bind stable recertification identities without workflow-run byte equality
  if (canonicalHostJson(stablePublicationIdentity(historical)) !== canonicalHostJson(stablePublicationIdentity(recertified)) ||
    !readyBytes.equals(stageBytes) || stage.publicationSha256 !== sha256Bytes(publicationBytes) ||
    stage.protectedSha256 !== sha256Bytes(protectedBytes) || stage.targetEnvironmentSha256 !== sha256Bytes(environmentBytes) ||
    environment.WEATHER_SERVER_IMAGE !== historical.serverImage || environment.WEATHER_WEB_IMAGE !== historical.webImage) {
    throw codedError("receipt.binding", "deployed publication stage or environment identities differ");
  }
  const alias = hostBundle.activeAlias;

  // bind the active alias to the one captured release environment
  if (!hasExactKeys(alias, new Set(["identity", "linkTarget", "path", "resolvedPath", "targetIdentity", "targetSha256"])) ||
    alias.path !== "/opt/weather/current/deploy/state/active.env" || alias.linkTarget !== `../releases/${release}.env` ||
    alias.resolvedPath !== hostBundle.releaseEnvironment.path || alias.targetSha256 !== hostBundle.releaseEnvironment.raw.sha256 ||
    canonicalHostJson(alias.targetIdentity) !== canonicalHostJson(hostBundle.releaseEnvironment.identity)) {
    throw codedError("receipt.binding", "active release alias differs");
  }
  validateDeployedIdentity(alias.identity, "symbolic-link", "0777", "active alias");
  // require one exact active alias inode
  if (alias.identity.nlink !== "1" || alias.identity.size !== String(Buffer.byteLength(alias.linkTarget, "utf8"))) {
    throw codedError("receipt.binding", "active release alias link count is invalid");
  }
  const preparerKeys = new Set(["completedAt", "contractVersion", "result", "startedAt"]);
  const expectedResult = {
    argv: [
      "/home/ubuntu/weather/scripts/prepare-schema-preserving-publication.mjs",
      release,
      `${PRODUCTION_CUSTODY_ROOT}/deployed-source/recertified-publication.json`,
    ],
    cwd: `${PRODUCTION_CUSTODY_ROOT}/deployed-source/tag-tree`,
    environment: SOURCE_PROCESS_ENVIRONMENT,
    program: "/home/ubuntu/n/bin/node",
  };

  // require the exact isolated recertification process
  if (!hasExactKeys(preparer, preparerKeys) || preparer.contractVersion !== "weather-backend-publication-preparer-capture/v1" ||
    preparer.startedAt !== preparer.result?.startedAt || preparer.completedAt !== preparer.result?.completedAt) {
    throw codedError("receipt.authority", "publication preparer capture is invalid");
  }
  const preparerInterval = validateFixedProcessResult(preparer.result, expectedResult);

  // retain every immutable source proof sidecar
  for (const bytes of Object.values(documents)) {
    retainArtifact(retained, bytes);
  }
  return {
    captureInterval,
    environment,
    historicalPublication: historical,
    hostBundle,
    preparerInterval,
    proof,
    recertifiedPublication: recertified,
    release,
    stage,
  };
}

const observationKeys = new Set([
  "artifacts", "authority", "cgroups", "collection", "contractVersion", "docker",
  "evidenceSha256", "hostRole", "identity", "phase", "processes", "producer",
  "profile", "release", "runner", "systemd",
]);
const artifactObservationKeys = new Set([
  "artifactId", "bindingIds", "containerId", "declaredPath", "gid", "imageId",
  "kind", "mode", "present", "probeIds", "resolvedPath", "scope", "sha256",
  "symlinkTarget", "type", "uid",
]);

// register one retained content-addressed artifact
function retainArtifact(artifactBytesBySha256, value) {
  const bytes = Buffer.from(value ?? []);

  // bound retained documents before copying
  if (bytes.length === 0 || bytes.length > MAX_ARTIFACT_BYTES) {
    throw codedError("receipt.schema", "retained artifact size is invalid");
  }
  const sha256 = sha256Bytes(bytes);
  const existing = artifactBytesBySha256.get(sha256);

  // reject an impossible in-memory digest collision
  if (existing !== undefined && !existing.equals(bytes)) {
    throw codedError("receipt.digest", `retained artifact ${sha256} collides`);
  }
  artifactBytesBySha256.set(sha256, Buffer.from(bytes));
  return { byteLength: bytes.length, relativePath: `artifacts/${sha256}.bin`, sha256 };
}

// retain exact local tool source bytes
function retainTooling(artifactBytesBySha256) {
  const tooling = {};

  // retain each identity class independently
  for (const [name, sourcePath] of Object.entries(toolingPaths)) {
    tooling[name] = {
      artifact: retainArtifact(artifactBytesBySha256, readFileSync(resolve(repositoryRoot, sourcePath))),
      sourcePath,
    };
  }
  return tooling;
}

// parse one checked source JSON document
function parseCheckedJson(bytes, version, label) {
  let value;

  try {
    value = JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(Buffer.from(bytes)));
  } catch (error) {
    throw codedError("receipt.schema", `${label} is not valid UTF-8 JSON`, error);
  }
  const actual = label === "inventory" ? value?.schemaVersion : value?.contractVersion;

  // require one exact checked schema version
  if (!isRecord(value) || actual !== version) {
    throw codedError("receipt.schema", `${label} schema is invalid`);
  }
  return value;
}

// parse one canonical host JSON document
function parseCanonicalHostBytes(bytes, label) {
  const encoded = Buffer.from(bytes ?? []);
  let value;

  // bound one transported document
  if (encoded.length === 0 || encoded.length > MAX_ARTIFACT_BYTES) {
    throw codedError("receipt.schema", `${label} size is invalid`);
  }
  try {
    value = JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(encoded));
  } catch (error) {
    throw codedError("receipt.schema", `${label} is not valid UTF-8 JSON`, error);
  }
  const compact = Buffer.from(canonicalHostJson(value), "utf8");
  const line = Buffer.concat([compact, Buffer.from("\n")]);

  // accept only exact staged or CLI framing
  if (!encoded.equals(compact) && !encoded.equals(line)) {
    throw codedError("receipt.schema", `${label} bytes are not canonical host JSON`);
  }
  return value;
}

// validate one cleared structural observation fixture
function validateFixtureObservation(observation, inventoryBytes, runtimeMapBytes, phase) {
  // require the exact observation root and interval
  if (!hasExactKeys(observation, observationKeys) || observation.contractVersion !== HOST_OBSERVATION_VERSION ||
    observation.producer !== HOST_PRODUCER || observation.phase !== phase ||
    !["blueberry-compose", "workstation-runner"].includes(observation.profile) ||
    observation.hostRole !== (observation.profile === "blueberry-compose" ? "blueberry" : "workstation") ||
    !Array.isArray(observation.artifacts) || !Array.isArray(observation.cgroups) ||
    !Array.isArray(observation.processes) || !Array.isArray(observation.systemd) ||
    !hasExactKeys(observation.collection, new Set(["complete", "completedAt", "startedAt"])) ||
    observation.collection.complete !== true || !isTimestamp(observation.collection.startedAt) ||
    !isTimestamp(observation.collection.completedAt) ||
    Date.parse(observation.collection.startedAt) > Date.parse(observation.collection.completedAt)) {
    throw codedError("receipt.schema", "cleared observation fixture is invalid");
  }
  const authorityKeys = new Set([
    "canonicalEligible", "inventorySha256", "kind", "liveEligible", "probeIds",
    "producerSha256", "runtimeMapSha256",
  ]);

  // require explicitly nonpromotable fixture authority
  if (!hasExactKeys(observation.authority, authorityKeys) ||
    observation.authority.canonicalEligible !== false || observation.authority.liveEligible !== false ||
    observation.authority.kind !== "injected-test" ||
    observation.authority.inventorySha256 !== sha256Bytes(inventoryBytes) ||
    observation.authority.runtimeMapSha256 !== sha256Bytes(runtimeMapBytes) ||
    !digestPattern.test(observation.authority.producerSha256 ?? "") ||
    !digestPattern.test(observation.evidenceSha256 ?? "") || !Array.isArray(observation.authority.probeIds)) {
    throw codedError("receipt.authority", "cleared observation fixture authority is invalid");
  }
  // require all artifact fields consumed by Gate3
  for (const artifact of observation.artifacts) {
    // reject open or unbound artifact records
    if (!hasExactKeys(artifact, artifactObservationKeys) || typeof artifact.artifactId !== "string" ||
      typeof artifact.kind !== "string" || typeof artifact.present !== "boolean" ||
      !Array.isArray(artifact.bindingIds) || artifact.bindingIds.length === 0 ||
      !artifact.bindingIds.every((value) => typeof value === "string") ||
      !Array.isArray(artifact.probeIds) || artifact.probeIds.length === 0) {
      throw codedError("receipt.schema", "cleared observation artifact is invalid");
    }
  }
  // require profile-exclusive runtime shapes
  if ((observation.profile === "blueberry-compose" && (!isRecord(observation.docker) || observation.runner !== null)) ||
    (observation.profile === "workstation-runner" && (observation.docker !== null || !isRecord(observation.runner)))) {
    throw codedError("receipt.schema", "cleared observation profile records are invalid");
  }
  return observation;
}

// derive one closed synthetic transport record
function deriveFixtureTransport(capture, observation, observationBytes) {
  const transport = capture.transport;
  const common = ["completedAt", "exitCode", "observationSha256", "producerSha256", "startedAt"];
  const expected = new Set([...common, observation.profile === "blueberry-compose" ? "ssh" : "process"]);

  // require the profile-owned envelope and successful result
  if (!hasExactKeys(transport, expected) || transport.exitCode !== 0) {
    throw codedError("receipt.authority", "synthetic transport envelope or result is invalid");
  }
  // bind transport digests to exact observer bytes
  if (transport.observationSha256 !== sha256Bytes(observationBytes) ||
    transport.producerSha256 !== observation.authority.producerSha256) {
    throw codedError("receipt.digest", "synthetic transport digest is invalid");
  }
  // require exact interval containment
  if (!isTimestamp(transport.startedAt) || !isTimestamp(transport.completedAt) ||
    Date.parse(transport.startedAt) > Date.parse(observation.collection.startedAt) ||
    Date.parse(observation.collection.completedAt) > Date.parse(transport.completedAt)) {
    throw codedError("receipt.chronology", "synthetic transport identity or interval is invalid");
  }
  // require fixed Blueberry SSH trust facts
  if (observation.profile === "blueberry-compose") {
    if (!hasExactKeys(transport.ssh, new Set(["hostKeyFingerprint", "knownHostsSha256", "policySha256"])) ||
      transport.ssh.policySha256 !== POLICY_SHA256 || transport.ssh.knownHostsSha256 !== KNOWN_HOSTS_SHA256 ||
      transport.ssh.hostKeyFingerprint !== BLUEBERRY_FINGERPRINT) {
      throw codedError("receipt.authority", "synthetic Blueberry trust envelope is invalid");
    }
    return freezeCanonical({ ...transport, kind: "main-authenticated-ssh" });
  }
  const processKeys = new Set([
    "egid", "euid", "executablePath", "executableSha256", "gid", "pid",
    "startTimeTicks", "uid",
  ]);

  // require fixed local protected-process identity
  if (!hasExactKeys(transport.process, processKeys) || transport.process.uid !== 1000 ||
    transport.process.gid !== 1000 || transport.process.euid !== 0 || transport.process.egid !== 0 ||
    typeof transport.process.executablePath !== "string" ||
    !digestPattern.test(transport.process.executableSha256 ?? "") ||
    !Number.isSafeInteger(transport.process.pid) || transport.process.pid <= 0 ||
    !/^[1-9][0-9]*$/u.test(transport.process.startTimeTicks ?? "")) {
    throw codedError("receipt.authority", "synthetic local process identity is invalid");
  }
  return freezeCanonical({ ...transport, kind: "main-authenticated-local" });
}

// derive one physical-shaped raw Gate2 capture
function deriveRawCapture(capture, inventory, runtimeMap, inventoryBytes, runtimeMapBytes, phase, retained) {
  const requestBytes = Buffer.from(capture.requestBytes ?? []);
  const outputBytes = Buffer.from(capture.outputBytes ?? []);
  const request = parseCanonicalHostBytes(requestBytes, "capture.requestBytes");
  const evidence = parseCanonicalHostBytes(outputBytes, "capture.outputBytes");

  // bind raw request source bytes exactly
  if (request.contractVersion !== HOST_EVIDENCE_REQUEST_VERSION || evidence.contractVersion !== HOST_EVIDENCE_CONTRACT_VERSION ||
    request.inventory?.json !== inventoryBytes.toString("utf8") || request.runtimeMap?.json !== runtimeMapBytes.toString("utf8") ||
    request.inventory?.sha256 !== sha256Bytes(inventoryBytes) || request.runtimeMap?.sha256 !== sha256Bytes(runtimeMapBytes) ||
    evidence.profile !== request.profile) {
    throw codedError("receipt.digest", "raw Gate2 capture source binding is invalid");
  }
  const built = buildBackendHostObservation({ evidence, inventory, phase, runtimeMap });

  // refuse invalid raw observations
  if (!built.ok || built.observation === null) {
    const error = codedError("receipt.invalid", "raw Gate2 observation is invalid");
    error.diagnostics = built.diagnostics;
    throw error;
  }
  const observationBytes = canonicalReceiptBytes(built.observation);
  const syntheticTransport = {
    completedAt: capture.transport?.completedAt,
    exitCode: capture.transport?.exitCode,
    observationSha256: sha256Bytes(observationBytes),
    producerSha256: capture.transport?.producerSha256,
    startedAt: capture.transport?.startedAt,
    ...(built.observation.profile === "blueberry-compose" ? { ssh: capture.transport?.ssh } : { process: capture.transport?.process }),
  };
  return {
    hostRole: built.observation.hostRole,
    observation: freezeCanonical(built.observation),
    observationSha256: receiptSha256(observationBytes),
    profile: built.observation.profile,
    source: {
      kind: "raw-gate2",
      output: retainArtifact(retained, outputBytes),
      request: retainArtifact(retained, requestBytes),
    },
    transport: deriveFixtureTransport({ transport: syntheticTransport }, built.observation, observationBytes),
  };
}

// derive one physical Main-custodied Gate2 capture
function deriveMainCustodyCapture(capture, inventory, runtimeMap, inventoryBytes, runtimeMapBytes, phase, retained) {
  const requestBytes = Buffer.from(capture.requestBytes ?? []);
  const outputBytes = Buffer.from(capture.outputBytes ?? []);
  const nativeCaptureBytes = Buffer.from(capture.nativeCaptureBytes ?? []);
  const materializedPlanBytes = Buffer.from(capture.materializedPlanBytes ?? []);
  const request = parseCanonicalHostBytes(requestBytes, "capture.requestBytes");
  const evidence = parseCanonicalHostBytes(outputBytes, "capture.outputBytes");

  // bind the physical request to exact checked and producer bytes
  if (request.contractVersion !== HOST_EVIDENCE_REQUEST_VERSION || evidence.contractVersion !== HOST_EVIDENCE_CONTRACT_VERSION ||
    request.inventory?.json !== inventoryBytes.toString("utf8") || request.runtimeMap?.json !== runtimeMapBytes.toString("utf8") ||
    request.inventory?.sha256 !== sha256Bytes(inventoryBytes) || request.runtimeMap?.sha256 !== sha256Bytes(runtimeMapBytes) ||
    evidence.profile !== request.profile || request.producerSha256 !== sha256Bytes(readFileSync(resolve(repositoryRoot, toolingPaths.observer)))) {
    throw codedError("receipt.digest", "Main custody Gate2 source binding is invalid");
  }
  const built = buildBackendHostObservation({ evidence, inventory, phase, runtimeMap });

  // require one complete physical live-eligible observation
  if (!built.ok || built.observation === null || built.observation.authority?.liveEligible !== true ||
    built.observation.authority?.kind !== "physical-host") {
    const error = codedError("receipt.invalid", "Main custody Gate2 observation is invalid");
    error.diagnostics = built.diagnostics;
    throw error;
  }
  const nativeCapture = parseCanonicalReceiptBytes(nativeCaptureBytes, "receipt.authority");
  const materializedPlan = parseCanonicalReceiptBytes(materializedPlanBytes, "receipt.authority");
  const operationId = built.observation.profile === "blueberry-compose" ? "blueberry-compose-v1" : "workstation-runner-v1";
  const collectStep = nativeCapture.steps?.find(
    // select the one fixed collection result
    (step) => step.id?.endsWith(".collect"),
  );

  // require the native result to contain the built host interval
  if (nativeCapture.planSha256 !== receiptSha256(materializedPlan) ||
    !isTimestamp(collectStep?.startedAt) || !isTimestamp(collectStep?.completedAt) ||
    Date.parse(collectStep.startedAt) > Date.parse(built.observation.collection.startedAt) ||
    Date.parse(built.observation.collection.completedAt) > Date.parse(collectStep.completedAt)) {
    throw codedError("receipt.chronology", "Main custody capture interval is invalid");
  }
  const observationBytes = canonicalReceiptBytes(built.observation);
  const requestReference = retainArtifact(retained, requestBytes);
  const outputReference = retainArtifact(retained, outputBytes);
  const nativeReference = retainArtifact(retained, nativeCaptureBytes);
  return {
    hostRole: built.observation.hostRole,
    observation: freezeCanonical(built.observation),
    observationSha256: receiptSha256(observationBytes),
    profile: built.observation.profile,
    source: {
      kind: "raw-main-custody",
      nativeCapture: nativeReference,
      output: outputReference,
      plan: retainArtifact(retained, materializedPlanBytes),
      request: requestReference,
    },
    transport: freezeCanonical({
      captureSha256: nativeReference.sha256,
      completedAt: collectStep.completedAt,
      exitCode: 0,
      kind: "main-fixed-native-capture",
      observationSha256: sha256Bytes(observationBytes),
      operationId,
      planSha256: nativeCapture.planSha256,
      producerSha256: request.producerSha256,
      startedAt: collectStep.startedAt,
    }),
  };
}

// derive one synthetic capture union member
function deriveSyntheticCapture(capture, inventory, runtimeMap, inventoryBytes, runtimeMapBytes, phase, retained) {
  // route physical Main custody through the cleared raw builder
  if (capture?.kind === "raw-main-custody" && hasExactKeys(capture, new Set([
    "kind", "materializedPlanBytes", "nativeCaptureBytes", "outputBytes", "requestBytes",
  ]))) {
    return deriveMainCustodyCapture(capture, inventory, runtimeMap, inventoryBytes, runtimeMapBytes, phase, retained);
  }
  // route raw Gate2 bytes through the cleared builder
  if (capture?.kind === "raw-gate2" && hasExactKeys(capture, new Set(["kind", "outputBytes", "requestBytes", "transport"]))) {
    return deriveRawCapture(capture, inventory, runtimeMap, inventoryBytes, runtimeMapBytes, phase, retained);
  }
  // accept only the closed fixture union
  if (capture?.kind !== "cleared-observation-fixture" ||
    !hasExactKeys(capture, new Set(["kind", "observationBytes", "transport"]))) {
    throw codedError("receipt.schema", "synthetic capture shape is invalid");
  }
  const observationBytes = Buffer.from(capture.observationBytes ?? []);
  const observation = parseCanonicalReceiptBytes(observationBytes);
  validateFixtureObservation(observation, inventoryBytes, runtimeMapBytes, phase);
  return {
    hostRole: observation.hostRole,
    observation: freezeCanonical(observation),
    observationSha256: receiptSha256(observationBytes),
    profile: observation.profile,
    source: { kind: "cleared-observation-fixture", observation: retainArtifact(retained, observationBytes) },
    transport: deriveFixtureTransport(capture, observation, observationBytes),
  };
}

// select one source-owned activation policy from the authoritative receipt phase
function contextActivationSource(context, generation) {
  const selectedByGeneration = context.activationSourceByGeneration;

  // preserve predecessor maps that predate generation-selected native contexts
  if (selectedByGeneration === undefined) {
    return context.activationSource;
  }
  const generationKeys = new Set(phaseGeneration.values());

  // require the scanner's closed selector and inactive compatibility fallback
  if (!isRecord(selectedByGeneration) ||
    !hasExactKeys(selectedByGeneration, generationKeys) ||
    context.activationSource !== "never-active-alternative") {
    throw codedError("receipt.binding", `context ${context.id} generation activation is invalid`);
  }
  const selected = selectedByGeneration[generation];

  // refuse unknown policies and any pre-cutover native activation claim
  if (!activationSources.has(selected) ||
    (generation !== "rust-v1" && selected !== "never-active-alternative")) {
    throw codedError("receipt.binding", `context ${context.id} generation activation is unsupported`);
  }
  return selected;
}

// require a selected container to execute one physically observed native application binary
function validateSelectedContainerExecutable(context, container, contextArtifactDefinitions) {
  const executablePaths = contextArtifactDefinitions.filter(
    // retain only installed native application bytes from the selected image
    (artifact) => artifact.kind === "container-file" &&
      artifact.path?.startsWith("/opt/weather/bin/weather-") === true,
  ).map(
    // compare exact installed paths without basename substitution
    (artifact) => artifact.path,
  );
  const argv = container.actualArgv ?? container.configuredArgv;

  // reject static rows that do not bind the running service to native bytes
  if (executablePaths.length === 0 || !Array.isArray(argv) || !executablePaths.includes(argv[0])) {
    throw codedError("receipt.binding", `container context ${context.id} does not execute its native artifact`);
  }
}

// require an active selected unit to execute one physically observed native runner binary
function validateSelectedSystemdExecutable(context, unit, contextArtifactDefinitions) {
  const executablePaths = contextArtifactDefinitions.filter(
    // retain exact installed runner executables only
    (artifact) => artifact.kind === "runner-file" &&
      artifact.declaredPath?.includes("/bin/weather-") === true,
  ).map(
    // compare the source-owned account-relative installation path
    (artifact) => artifact.declaredPath,
  );

  // reject active units that are not bound to one selected native executable
  if (executablePaths.length === 0 || typeof unit.execStart !== "string" ||
    !executablePaths.some(
      // admit the exact account-home suffix after the host observer resolves ${HOME}
      (path) => unit.execStart.includes(path) ||
        (path.startsWith("${HOME}/") && unit.execStart.includes(path.slice("${HOME}".length))),
    )) {
    throw codedError("receipt.binding", `systemd context ${context.id} does not execute its native artifact`);
  }
}

// derive one context installation state
function deriveContextState(
  context,
  observation,
  contextArtifacts,
  contextArtifactDefinitions,
  fixtureOnly,
  activationSource,
) {
  // derive the conditional runner package as one closure
  if (context.rootKind === "runner-package") {
    const state = observation.runner?.state;

    // accept the exact state vocabulary only
    if (!["installed-active", "installed-enabled-inactive", "installed-disabled-inactive", "installed-failed", "not-installed"].includes(state)) {
      throw codedError("receipt.binding", `runner context ${context.id} state is invalid`);
    }
    return state;
  }
  // derive one exact systemd unit state
  if (activationSource === "systemd-show") {
    const unit = observation.systemd.find(
      // select the source-owned unit
      (candidate) => candidate?.unit === context.unit,
    );

    // require the matching typed unit
    const sourceOnlyControl = Array.isArray(context.accountingScopes) && context.accountingScopes.length > 0 &&
      context.accountingScopes.every(
        // exclude application activation claims
        (scope) => scope === "release-control:static-source",
      );

    // derive only a complete absent fixture or nonapplication static-control closure
    if (!isRecord(unit) && (fixtureOnly === true || sourceOnlyControl) &&
      contextArtifacts.length === context.artifactIds.length && contextArtifacts.every(
        // require every allowed installed artifact to be absent
        (artifact) => artifact.present === false,
      )) {
      return "not-installed";
    }
    if (!isRecord(unit)) {
      throw codedError("receipt.binding", `systemd context ${context.id} is missing`);
    }
    if (unit.loadState === "not-found") {
      return "not-installed";
    }
    if (unit.activeState === "failed") {
      return "installed-failed";
    }
    if (unit.activeState === "active") {
      // bind generation-selected activation to the native executable rather than static rows alone
      if (context.activationSourceByGeneration !== undefined) {
        validateSelectedSystemdExecutable(context, unit, contextArtifactDefinitions);
      }
      return "installed-active";
    }
    if (["enabled", "enabled-runtime"].includes(unit.enabledState)) {
      return "installed-enabled-inactive";
    }
    return "installed-disabled-inactive";
  }
  // derive dormant alternatives from their complete installed closure
  if (["never-active-alternative", "process-identity"].includes(activationSource)) {
    const presentCount = contextArtifacts.filter(
      // count exact installed members only
      (artifact) => artifact.present === true,
    ).length;

    // reject partial alternative installations
    if (presentCount !== 0 && presentCount !== context.artifactIds.length) {
      throw codedError("receipt.binding", `context ${context.id} has a partial installed closure`);
    }
    return presentCount === 0 ? "not-installed" : "installed-disabled-inactive";
  }
  // require every expected installed artifact
  if (contextArtifacts.length !== context.artifactIds.length || contextArtifacts.some(
    // reject one absent artifact
    (artifact) => artifact.present !== true,
  )) {
    throw codedError("receipt.binding", `context ${context.id} has missing installed artifacts`);
  }
  // derive persistent and one-shot container state
  if (activationSource === "container-inspect") {
    const container = observation.docker?.containers?.find(
      // select the exact service
      (candidate) => candidate?.service === context.service,
    );

    // preserve an absent image-backed migration as inactive
    if (context.service === "migration" && container === undefined) {
      return "installed-disabled-inactive";
    }
    if (!isRecord(container)) {
      throw codedError("receipt.binding", `container context ${context.id} is missing`);
    }
    // bind generation-selected activation to the native executable rather than static rows alone
    if (context.activationSourceByGeneration !== undefined) {
      validateSelectedContainerExecutable(context, container, contextArtifactDefinitions);
    }
    if (context.service === "migration") {
      // accept only one successful completed one-shot
      if (container.oneOff === true && container.running === false && container.status === "exited" && container.exitCode === 0) {
        return "installed-disabled-inactive";
      }
      throw codedError("receipt.binding", "migration container is failed or transient");
    }
    // require persistent services to be running
    if (container.oneOff !== false || container.running !== true || container.status !== "running") {
      throw codedError("receipt.binding", `container context ${context.id} is not running`);
    }
    return "installed-active";
  }
  throw codedError("receipt.binding", `context ${context.id} activation source is unsupported`);
}

// derive every exact runtime-map binding
function deriveBindings(runtimeMap, captures, phase) {
  const generation = phaseGeneration.get(phase);

  // refuse binding derivation outside the fixed receipt lifecycle
  if (generation === undefined) {
    throw codedError("receipt.phase", `unsupported phase ${String(phase)}`);
  }
  const contexts = new Map(runtimeMap.contexts.map(
    // index source-owned contexts
    (context) => [context.id, context],
  ));
  const capturesByProfile = new Map(captures.map(
    // index fixed profile captures
    (capture) => [capture.profile, capture],
  ));
  const bindings = [];
  const artifactDefinitions = new Map(runtimeMap.artifacts.map(
    // index source-owned artifact definitions
    (artifact) => [artifact.id, artifact],
  ));

  // derive one state per exact entry-root-instance tuple
  for (const binding of runtimeMap.bindings) {
    const context = contexts.get(binding.contextId);
    const capture = capturesByProfile.get(context?.profile);

    // reject missing or substituted contexts
    if (!isRecord(context) || capture === undefined || context.rootEntryId !== binding.rootEntryId) {
      throw codedError("receipt.binding", `binding ${binding.id} has no exact context`);
    }
    const contextBindingIds = new Set(runtimeMap.bindings.filter(
      // retain this exact context only
      (candidate) => candidate.contextId === context.id,
    ).map(
      // retain stable identities
      (candidate) => candidate.id,
    ));
    const contextArtifacts = capture.observation.artifacts.filter(
      // retain artifact backlinks to this context
      (artifact) => artifact.bindingIds.some((id) => contextBindingIds.has(id)),
    );
    const artifactsById = new Map(contextArtifacts.map(
      // index exact artifact observations
      (artifact) => [artifact.artifactId, artifact],
    ));

    // require every binding artifact backlink
    for (const artifactId of binding.artifactIds) {
      const artifact = artifactsById.get(artifactId);
      const definition = artifactDefinitions.get(artifactId);

      // reject omissions and cross-context substitutions
      if (artifact === undefined || definition === undefined ||
        !artifact.bindingIds.includes(binding.id) || artifact.kind !== definition.kind) {
        throw codedError("receipt.binding", `binding ${binding.id} lacks artifact ${artifactId}`);
      }
      // bind image artifacts to the exact inspected image role
      if (definition.imageTarget !== undefined && definition.imageTarget !== null && artifact.imageId !== null) {
        const image = capture.observation.docker?.images?.find(
          // select the exact image role
          (candidate) => candidate?.role === definition.imageTarget,
        );

        // reject cross-image artifact replay
        if (!isRecord(image) || artifact.imageId !== image.imageId) {
          throw codedError("receipt.binding", `artifact ${artifactId} image identity differs`);
        }
      }
    }
    const state = deriveContextState(
      context,
      capture.observation,
      context.artifactIds.map(
        // preserve declared artifact identity order
        (id) => artifactsById.get(id),
      ).filter(Boolean),
      context.artifactIds.map(
        // preserve declared physical artifact definitions in context order
        (id) => artifactDefinitions.get(id),
      ).filter(Boolean),
      capture.source.kind === "cleared-observation-fixture",
      contextActivationSource(context, generation),
    );
    bindings.push({
      artifactIds: [...binding.artifactIds],
      contextId: binding.contextId,
      entryId: binding.entryId,
      hostRole: context.hostRole,
      id: binding.id,
      profile: context.profile,
      rootEntryId: binding.rootEntryId,
      state,
    });
  }
  return bindings.sort(
    // retain stable binding bytes
    (left, right) => left.id.localeCompare(right.id),
  );
}

// derive sequence generation and predecessor from one phase
function deriveChainIdentity(phase, previousReceipt) {
  const generation = phaseGeneration.get(phase);

  // require one known phase
  if (generation === undefined) {
    throw codedError("receipt.phase", `unsupported phase ${String(phase)}`);
  }
  // require baseline as the sole origin
  if (previousReceipt === null) {
    if (phase !== "baseline") {
      throw codedError("receipt.phase", "receipt chain must begin at baseline");
    }
    return { generation, previousReceiptSha256: null, sequence: 0 };
  }
  // require the exact recovery transition graph
  if (nextPhase.get(previousReceipt.phase) !== phase) {
    throw codedError("receipt.phase", `${phase} cannot follow ${String(previousReceipt.phase)}`);
  }
  return {
    generation,
    previousReceiptSha256: receiptSha256(previousReceipt),
    sequence: previousReceipt.sequence + 1,
  };
}

// assemble one structural noncanonical live receipt
export function assembleSyntheticLiveStatusReceipt(input) {
  const inputKeys = new Set([
    "captures", "inventoryBytes", "measurementSourceReceiptBytes", "phase",
    "previousReceiptBytes", "runtimeMapBytes", "writerNow",
  ]);

  // reject open input and caller freshness overrides
  if (!hasExactKeys(input, inputKeys) || !Array.isArray(input.captures) || input.captures.length !== 2 ||
    !isTimestamp(input.writerNow)) {
    throw codedError("receipt.schema", "synthetic assembly input is invalid");
  }
  const inventoryBytes = Buffer.from(input.inventoryBytes ?? []);
  const runtimeMapBytes = Buffer.from(input.runtimeMapBytes ?? []);
  const measurementBytes = Buffer.from(input.measurementSourceReceiptBytes ?? []);
  const inventory = parseCheckedJson(inventoryBytes, INVENTORY_VERSION, "inventory");
  const runtimeMap = parseCheckedJson(runtimeMapBytes, RUNTIME_MAP_VERSION, "runtimeMap");

  // require a complete checked source closure
  if (!Array.isArray(inventory.entries) || !Array.isArray(runtimeMap.artifacts) ||
    !Array.isArray(runtimeMap.bindings) || !Array.isArray(runtimeMap.contexts) ||
    runtimeMap.inventorySchemaVersion !== inventory.schemaVersion) {
    throw codedError("receipt.schema", "checked inventory or runtime map is incomplete");
  }
  parseCanonicalReceiptBytes(measurementBytes);
  const previousReceipt = input.previousReceiptBytes === null
    ? null
    : parseCanonicalReceiptBytes(input.previousReceiptBytes, "chain.previous");
  const chain = deriveChainIdentity(input.phase, previousReceipt);
  const artifactBytesBySha256 = new Map();
  const artifacts = {
    inventory: retainArtifact(artifactBytesBySha256, inventoryBytes),
    measurementSourceReceipt: retainArtifact(artifactBytesBySha256, measurementBytes),
    runtimeMap: retainArtifact(artifactBytesBySha256, runtimeMapBytes),
  };
  const captures = input.captures.map(
    // derive each capture independently
    (capture) => deriveSyntheticCapture(capture, inventory, runtimeMap, inventoryBytes, runtimeMapBytes, input.phase, artifactBytesBySha256),
  ).sort(
    // retain the fixed profile order
    (left, right) => left.profile.localeCompare(right.profile),
  );
  const profiles = captures.map(
    // retain only profile identities
    (capture) => capture.profile,
  );

  // require each fixed profile exactly once
  if (canonicalHostJson(profiles) !== canonicalHostJson(["blueberry-compose", "workstation-runner"])) {
    throw codedError("receipt.profile", "both fixed profiles are required exactly once");
  }
  const intervals = captures.map(
    // retain actual transport intervals
    (capture) => ({ completedAt: capture.transport.completedAt, startedAt: capture.transport.startedAt }),
  ).sort(
    // order by actual start
    (left, right) => Date.parse(left.startedAt) - Date.parse(right.startedAt),
  );

  // reject overlapping host transports
  for (let index = 1; index < intervals.length; index += 1) {
    // require the prior capture to finish first
    if (Date.parse(intervals[index].startedAt) <= Date.parse(intervals[index - 1].completedAt)) {
      throw codedError("receipt.chronology", "capture intervals overlap");
    }
  }
  const startedAt = intervals[0].startedAt;
  const completedAt = intervals.at(-1).completedAt;
  const writerNowMs = Date.parse(input.writerNow);

  // enforce fixed freshness for every incoming capture
  for (const capture of captures) {
    const completedMs = Date.parse(capture.transport.completedAt);

    // reject stale and future heads
    if (completedMs > writerNowMs || writerNowMs - completedMs > LIVE_STATUS_MAX_AGE_MS) {
      throw codedError("receipt.freshness", `capture ${capture.profile} is stale or future`);
    }
  }
  // separate the new interval from its predecessor
  if (previousReceipt !== null && Date.parse(startedAt) <= Date.parse(previousReceipt.completedAt)) {
    throw codedError("receipt.chronology", "receipt overlaps or backdates its predecessor");
  }
  const receipt = freezeCanonical({
    artifacts,
    authority: {
      blockers: [
        "gate6-authenticated-capture",
        "gate6-committed-measurement-tree",
        "gate6-deployed-source-provenance",
      ],
      canonicalEligible: false,
      custody: "same-main-uid-private-files",
      origin: "synthetic-fixture",
    },
    bindings: deriveBindings(runtimeMap, captures, input.phase),
    captures,
    completedAt,
    contractVersion: LIVE_STATUS_CONTRACT_VERSION,
    generation: chain.generation,
    phase: input.phase,
    previousReceiptSha256: chain.previousReceiptSha256,
    sequence: chain.sequence,
    startedAt,
    tooling: retainTooling(artifactBytesBySha256),
    writtenAt: input.writerNow,
  });
  const validation = validateLiveStatusReceipt(receipt, {
    artifactBytesBySha256,
    previousReceipt,
    writerNow: input.writerNow,
  });

  // refuse internal assembly drift
  if (!validation.ok) {
    const error = codedError("receipt.invalid", "assembled synthetic receipt is invalid");
    error.diagnostics = validation.diagnostics;
    throw error;
  }
  return { artifactBytesBySha256, receipt };
}

// bind deployed source classes to one cleared Blueberry observation
function validateDeployedObservationJoins(source, captures) {
  const blueberry = captures.find(
    // select the only deployed application host
    (capture) => capture.profile === "blueberry-compose",
  )?.observation;

  // require one exact deployed application observation
  if (!isRecord(blueberry) || blueberry.release?.currentRelease !== source.release ||
    blueberry.release?.previousRelease !== source.stage.sourceRelease) {
    throw codedError("receipt.binding", "deployed release observation is missing");
  }
  const images = new Map((blueberry.docker?.images ?? []).map(
    // index exact image roles
    (image) => [image.role, image],
  ));
  const server = images.get("server");
  const web = images.get("web");

  // bind distinct core and web image authority
  if (server?.reference !== source.historicalPublication.serverImage || server?.imageId !== source.historicalPublication.serverImageId ||
    web?.reference !== source.historicalPublication.webImage || web?.imageId !== source.historicalPublication.webImageId) {
    throw codedError("receipt.binding", "deployed image identities differ from the host observation");
  }
  const bundleBytes = Buffer.from(
    `${source.historicalPublication.validatorSha256}  schema-preserving-release.mjs\n` +
    `${source.historicalPublication.operatorSha256}  schema-preserving-release.sh\n`,
    "utf8",
  );

  // bind only the installed operator bundle to the deployed tag class
  if (blueberry.release?.operator?.bundleSha256 !== sha256Bytes(bundleBytes)) {
    throw codedError("receipt.binding", "installed operator bundle differs from deployed source");
  }
}

// build one source-backed custody authority record
function custodyAuthority(canonicalEligible, measurementBytes, deployedProofBytes) {
  return {
    blockers: canonicalEligible ? [] : ["synthetic-custody"],
    canonicalEligible,
    custody: "same-main-uid-private-files",
    deployedSourceProofSha256: sha256Bytes(deployedProofBytes),
    measurementSourceReceiptSha256: sha256Bytes(measurementBytes),
    origin: canonicalEligible ? "main-fixed-custody" : "synthetic-main-custody",
  };
}

// validate one source-backed receipt using the structural v2 validator
function validateCustodyReceipt(receipt, retained, previousReceipt, writerNow, canonicalEligible, sourceDocuments, historical = false) {
  const diagnostics = [];
  const expectedAuthority = custodyAuthority(
    canonicalEligible,
    sourceDocuments.measurementReceiptBytes,
    sourceDocuments.deployed.proofBytes,
  );

  // bind the private authority record and deployed proof reference
  if (canonicalHostJson(receipt.authority) !== canonicalHostJson(expectedAuthority)) {
    addDiagnostic(diagnostics, "receipt.authority", "$.authority", "custody receipt authority is invalid");
  }
  const deployedReference = receipt.artifacts?.deployedSourceProof;
  const deployedBytes = validateArtifactReference(
    deployedReference,
    retained,
    "$.artifacts.deployedSourceProof",
    diagnostics,
  );

  // bind the top-level deployed proof class
  if (deployedBytes !== null && !deployedBytes.equals(sourceDocuments.deployed.proofBytes)) {
    addDiagnostic(diagnostics, "receipt.digest", "$.artifacts.deployedSourceProof", "deployed source proof differs");
  }
  const capturePolicyBytes = validateArtifactReference(
    receipt.artifacts?.capturePolicy,
    retained,
    "$.artifacts.capturePolicy",
    diagnostics,
  );
  const knownHostsBytes = validateArtifactReference(
    receipt.artifacts?.knownHosts,
    retained,
    "$.artifacts.knownHosts",
    diagnostics,
  );

  // bind both retained trust artifacts to the validated custody inputs
  if (capturePolicyBytes !== null && !capturePolicyBytes.equals(sourceDocuments.trustPolicyBytes)) {
    addDiagnostic(diagnostics, "receipt.digest", "$.artifacts.capturePolicy", "capture policy differs");
  }
  if (knownHostsBytes !== null && !knownHostsBytes.equals(sourceDocuments.knownHostsBytes)) {
    addDiagnostic(diagnostics, "receipt.digest", "$.artifacts.knownHosts", "known-hosts bytes differ");
  }
  const structural = structuredClone(receipt);
  structural.authority = {
    blockers: [
      "gate6-authenticated-capture",
      "gate6-committed-measurement-tree",
      "gate6-deployed-source-provenance",
    ],
    canonicalEligible: false,
    custody: "same-main-uid-private-files",
    origin: "synthetic-fixture",
  };
  delete structural.artifacts.deployedSourceProof;
  delete structural.artifacts.capturePolicy;
  delete structural.artifacts.knownHosts;
  const structuralValidation = validateLiveStatusReceipt(structural, {
    artifactBytesBySha256: retained,
    captureTrust: {
      knownHostsBytes: sourceDocuments.knownHostsBytes,
      trustPolicyBytes: sourceDocuments.trustPolicyBytes,
    },
    historicalValidationToken: historical ? historicalValidationToken : undefined,
    previousReceipt,
    writerNow,
  });
  diagnostics.push(...structuralValidation.diagnostics);
  diagnostics.sort(
    // retain deterministic custody diagnostics
    (left, right) => canonicalHostJson(left).localeCompare(canonicalHostJson(right)),
  );
  return {
    canonicalEligible: false,
    diagnostics,
    ok: diagnostics.length === 0,
    sha256: diagnostics.length === 0 ? receiptSha256(receipt) : null,
  };
}

// validate every fixed custody source document before sampling writer time
function validateMainCustodySources(sourceDocuments) {
  const retained = new Map();
  const trust = validateCaptureTrust(sourceDocuments.trustPolicyBytes, sourceDocuments.knownHostsBytes, retained);
  const measurement = validateMeasurementSourceProof(
    sourceDocuments.measurementReceiptBytes,
    sourceDocuments.measurementCaptureBytes,
    retained,
  );
  const deployed = validateDeployedSourceProof(
    sourceDocuments.deployed,
    retained,
    measurement.sourceBytesByPath.get(toolingPaths.receiptWriter),
  );
  return { deployed, measurement, retained, sourceDocuments, trust };
}

// assemble one internally validated source-backed live receipt
function assembleMainCustodyReceipt(input) {
  const validatedSources = input.validatedSources ?? validateMainCustodySources(input.sourceDocuments);
  const { deployed, measurement, retained } = validatedSources;
  const structural = assembleSyntheticLiveStatusReceipt({
    captures: input.captures,
    inventoryBytes: input.inventoryBytes,
    measurementSourceReceiptBytes: input.sourceDocuments.measurementReceiptBytes,
    phase: input.phase,
    previousReceiptBytes: input.previousReceiptBytes,
    runtimeMapBytes: input.runtimeMapBytes,
    writerNow: input.writerNow,
  });

  // merge every structural retained sidecar
  for (const [digest, bytes] of structural.artifactBytesBySha256) {
    retained.set(digest, bytes);
  }
  validateDeployedObservationJoins(deployed, structural.receipt.captures);

  // require all source proof collection to precede the live observation
  for (const interval of [measurement.capture, deployed.captureInterval, deployed.preparerInterval]) {
    const completedAt = interval.completedAt;

    // reject source proof collected after the live interval or writer clock
    if (!isTimestamp(completedAt) || Date.parse(completedAt) > Date.parse(structural.receipt.startedAt) ||
      Date.parse(completedAt) > Date.parse(input.writerNow)) {
      throw codedError("receipt.chronology", "source proof interval follows the live observation");
    }
  }
  const receipt = structuredClone(structural.receipt);
  receipt.artifacts.capturePolicy = retainArtifact(retained, validatedSources.sourceDocuments.trustPolicyBytes);
  receipt.artifacts.deployedSourceProof = retainArtifact(retained, input.sourceDocuments.deployed.proofBytes);
  receipt.artifacts.knownHosts = retainArtifact(retained, validatedSources.sourceDocuments.knownHostsBytes);
  receipt.authority = custodyAuthority(
    input.canonicalEligible,
    validatedSources.sourceDocuments.measurementReceiptBytes,
    validatedSources.sourceDocuments.deployed.proofBytes,
  );
  const frozen = freezeCanonical(receipt);
  const previousReceipt = input.previousReceiptBytes === null
    ? null
    : parseCanonicalReceiptBytes(input.previousReceiptBytes, "chain.previous");
  const validation = validateCustodyReceipt(
    frozen,
    retained,
    previousReceipt,
    input.writerNow,
    input.canonicalEligible,
    validatedSources.sourceDocuments,
  );

  // refuse any internal source-backed assembly drift
  if (!validation.ok) {
    const error = codedError("receipt.invalid", "assembled custody receipt is invalid");
    error.diagnostics = validation.diagnostics;
    throw error;
  }
  return { artifactBytesBySha256: retained, receipt: frozen };
}

const receiptKeys = new Set([
  "artifacts", "authority", "bindings", "captures", "completedAt", "contractVersion",
  "generation", "phase", "previousReceiptSha256", "sequence", "startedAt", "tooling", "writtenAt",
]);

// add one deterministic validation diagnostic
function addDiagnostic(diagnostics, code, path, message) {
  diagnostics.push({ code, message, path });
}

// validate one retained artifact reference
function validateArtifactReference(reference, retained, path, diagnostics) {
  const keys = new Set(["byteLength", "relativePath", "sha256"]);

  // require one exact content-addressed reference
  if (!hasExactKeys(reference, keys) || !Number.isSafeInteger(reference.byteLength) || reference.byteLength <= 0 ||
    !digestPattern.test(reference.sha256 ?? "") || reference.relativePath !== `artifacts/${reference.sha256}.bin`) {
    addDiagnostic(diagnostics, "receipt.schema", path, "artifact reference is invalid");
    return null;
  }
  const bytes = retained?.get?.(reference.sha256);

  // bind exact retained bytes
  if (!Buffer.isBuffer(bytes) || bytes.length !== reference.byteLength || sha256Bytes(bytes) !== reference.sha256) {
    addDiagnostic(diagnostics, "receipt.digest", path, "artifact bytes differ from their reference");
    return null;
  }
  return bytes;
}

// recover the synthetic transport input from a stored derived record
function storedTransportInput(transport, profile) {
  const { kind, ...rest } = transport ?? {};

  // reject a mismatched derived transport kind
  if (kind !== (profile === "blueberry-compose" ? "main-authenticated-ssh" : "main-authenticated-local")) {
    throw codedError("receipt.authority", "stored transport kind differs from its profile");
  }
  return rest;
}

// validate one stored physical Main capture and native transport
function validateStoredMainCapture(
  capture,
  inventoryBytes,
  runtimeMapBytes,
  inventory,
  runtimeMap,
  phase,
  retained,
  captureTrust,
  observerBytes,
  writerBytes,
) {
  const source = capture.source;
  const requestBytes = validateArtifactReference(source.request, retained, "$.source.request", []);
  const outputBytes = validateArtifactReference(source.output, retained, "$.source.output", []);
  const nativeCaptureBytes = validateArtifactReference(source.nativeCapture, retained, "$.source.nativeCapture", []);
  const planBytes = validateArtifactReference(source.plan, retained, "$.source.plan", []);

  // require every physical raw sidecar
  if (requestBytes === null || outputBytes === null || nativeCaptureBytes === null || planBytes === null ||
    !Buffer.isBuffer(observerBytes)) {
    throw codedError("receipt.digest", "physical Main capture sidecars are missing");
  }
  const operationId = capture.profile === "blueberry-compose" ? "blueberry-compose-v1" : "workstation-runner-v1";
  const packageBytesByName = new Map([
    ["backend-runtime-inventory.json", inventoryBytes],
    ["backend-runtime-live-map.json", runtimeMapBytes],
    ["observer.mjs", observerBytes],
    ["output.json", outputBytes],
    ["request.json", requestBytes],
  ]);
  const retainedTemplate = retainedMainCaptureV1Template(operationId, writerBytes);
  const planValidation = validateMainCaptureWithPlan({
    captureBytes: nativeCaptureBytes,
    knownHostsBytes: captureTrust?.knownHostsBytes,
    operationId,
    packageBytesByName,
    planBytes,
    template: retainedTemplate,
    trustPolicyBytes: captureTrust?.trustPolicyBytes,
  });

  // require the exact source-owned native plan
  if (!planValidation.ok) {
    throw codedError("receipt.authority", "stored Main capture plan is invalid");
  }
  validateRetainedPlanSource(planBytes, operationId, writerBytes);
  const request = parseCanonicalHostBytes(requestBytes, "capture.requestBytes");
  const built = buildRetainedBackendHostObservationV1({
    evidenceBytes: outputBytes,
    inventoryBytes,
    observerSourceBytes: observerBytes,
    phase,
    requestBytes,
    runtimeMapBytes,
  });

  // rederive every incoming and historical observation from retained v1 semantics
  if (!built.ok || built.observation === null ||
    canonicalHostJson(built.observation) !== canonicalHostJson(capture.observation)) {
    throw codedError("receipt.binding", "stored Main observation differs from raw evidence");
  }
  const nativeCapture = parseCanonicalReceiptBytes(nativeCaptureBytes, "receipt.authority");
  const collectStep = nativeCapture.steps.find(
    // select the fixed collection result
    (step) => step.id.endsWith(".collect"),
  );
  const expectedTransport = {
    captureSha256: source.nativeCapture.sha256,
    completedAt: collectStep.completedAt,
    exitCode: 0,
    kind: "main-fixed-native-capture",
    observationSha256: sha256Bytes(canonicalReceiptBytes(capture.observation)),
    operationId,
    planSha256: nativeCapture.planSha256,
    producerSha256: request.producerSha256,
    startedAt: collectStep.startedAt,
  };

  // bind stored transport only to the native outcome
  if (canonicalHostJson(capture.transport) !== canonicalHostJson(expectedTransport)) {
    throw codedError("receipt.authority", "stored Main transport differs from native capture");
  }
}

// validate one structural v2 live receipt
export function validateLiveStatusReceipt(receipt, options = {}) {
  const diagnostics = [];
  const retained = options.artifactBytesBySha256;
  const historical = options.historicalValidationToken === historicalValidationToken;

  // require the exact v2 receipt root
  if (!hasExactKeys(receipt, receiptKeys) || receipt.contractVersion !== LIVE_STATUS_CONTRACT_VERSION) {
    addDiagnostic(diagnostics, "receipt.schema", "$", "live receipt root is invalid");
  }
  const expectedAuthority = {
    blockers: [
      "gate6-authenticated-capture",
      "gate6-committed-measurement-tree",
      "gate6-deployed-source-provenance",
    ],
    canonicalEligible: false,
    custody: "same-main-uid-private-files",
    origin: "synthetic-fixture",
  };

  // require nonpromotable synthetic authority
  if (canonicalHostJson(receipt?.authority ?? null) !== canonicalHostJson(expectedAuthority)) {
    addDiagnostic(diagnostics, "receipt.authority", "$.authority", "synthetic receipt authority is invalid");
  }
  const topArtifacts = receipt?.artifacts;

  // require the exact source artifact set
  if (!hasExactKeys(topArtifacts, new Set(["inventory", "measurementSourceReceipt", "runtimeMap"]))) {
    addDiagnostic(diagnostics, "receipt.schema", "$.artifacts", "receipt source artifacts are invalid");
  }
  const inventoryBytes = validateArtifactReference(topArtifacts?.inventory, retained, "$.artifacts.inventory", diagnostics);
  const runtimeMapBytes = validateArtifactReference(topArtifacts?.runtimeMap, retained, "$.artifacts.runtimeMap", diagnostics);
  const measurementBytes = validateArtifactReference(topArtifacts?.measurementSourceReceipt, retained, "$.artifacts.measurementSourceReceipt", diagnostics);
  let inventory = null;
  let runtimeMap = null;

  // parse only resolved source bytes
  try {
    if (inventoryBytes !== null) {
      inventory = parseCheckedJson(inventoryBytes, INVENTORY_VERSION, "inventory");
    }
    if (runtimeMapBytes !== null) {
      runtimeMap = parseCheckedJson(runtimeMapBytes, RUNTIME_MAP_VERSION, "runtimeMap");
    }
    if (measurementBytes !== null) {
      parseCanonicalReceiptBytes(measurementBytes);
    }
  } catch (error) {
    addDiagnostic(diagnostics, error.code ?? "receipt.schema", "$.artifacts", error.message);
  }
  const captures = Array.isArray(receipt?.captures) ? receipt.captures : [];
  const retainedObserverBytes = validateArtifactReference(
    receipt?.tooling?.observer?.artifact,
    retained,
    "$.tooling.observer.artifact",
    [],
  );
  const retainedWriterBytes = validateArtifactReference(
    receipt?.tooling?.receiptWriter?.artifact,
    retained,
    "$.tooling.receiptWriter.artifact",
    [],
  );
  const captureProfiles = captures.map(
    // retain fixed profile identities
    (capture) => capture?.profile,
  );

  // require exactly both fixed profiles in fixed order
  if (canonicalHostJson(captureProfiles) !== canonicalHostJson(["blueberry-compose", "workstation-runner"])) {
    addDiagnostic(diagnostics, "receipt.profile", "$.captures", "receipt profiles are invalid");
  }
  // validate every retained capture
  for (let index = 0; index < captures.length; index += 1) {
    const capture = captures[index];
    const path = `$.captures[${String(index)}]`;
    const captureKeys = new Set(["hostRole", "observation", "observationSha256", "profile", "source", "transport"]);

    // require a closed capture envelope
    if (!hasExactKeys(capture, captureKeys) || receiptSha256(capture?.observation ?? null) !== capture?.observationSha256) {
      addDiagnostic(diagnostics, "receipt.schema", path, "capture envelope is invalid");
      continue;
    }
    // bind redundant capture identity to the derived observation
    if (capture.profile !== capture.observation?.profile || capture.hostRole !== capture.observation?.hostRole) {
      addDiagnostic(diagnostics, "receipt.profile", path, "capture identity differs from its observation");
    }
    // validate fixture and raw capture source references
    if (capture.source?.kind === "cleared-observation-fixture" &&
      hasExactKeys(capture.source, new Set(["kind", "observation"]))) {
      const bytes = validateArtifactReference(capture.source.observation, retained, `${path}.source.observation`, diagnostics);

      // bind retained observation bytes to semantics
      if (bytes !== null && receiptSha256(bytes) !== capture.observationSha256) {
        addDiagnostic(diagnostics, "receipt.digest", `${path}.source.observation`, "observation bytes differ");
      }
    } else if (capture.source?.kind === "raw-gate2" &&
      hasExactKeys(capture.source, new Set(["kind", "output", "request"]))) {
      validateArtifactReference(capture.source.output, retained, `${path}.source.output`, diagnostics);
      validateArtifactReference(capture.source.request, retained, `${path}.source.request`, diagnostics);
    } else if (capture.source?.kind === "raw-main-custody" &&
      hasExactKeys(capture.source, new Set(["kind", "nativeCapture", "output", "plan", "request"]))) {
      validateArtifactReference(capture.source.output, retained, `${path}.source.output`, diagnostics);
      validateArtifactReference(capture.source.request, retained, `${path}.source.request`, diagnostics);
      validateArtifactReference(capture.source.nativeCapture, retained, `${path}.source.nativeCapture`, diagnostics);
      validateArtifactReference(capture.source.plan, retained, `${path}.source.plan`, diagnostics);
    } else {
      addDiagnostic(diagnostics, "receipt.schema", `${path}.source`, "capture source is invalid");
    }
    try {
      if (capture.source?.kind === "raw-main-custody" && inventoryBytes !== null && runtimeMapBytes !== null &&
        inventory !== null && runtimeMap !== null) {
        validateStoredMainCapture(
          capture,
          inventoryBytes,
          runtimeMapBytes,
          inventory,
          runtimeMap,
          receipt.phase,
          retained,
          options.captureTrust,
          retainedObserverBytes,
          retainedWriterBytes,
        );
      } else if (inventoryBytes !== null && runtimeMapBytes !== null) {
        validateFixtureObservation(capture.observation, inventoryBytes, runtimeMapBytes, receipt.phase);
        deriveFixtureTransport(
          { transport: storedTransportInput(capture.transport, capture.profile) },
          capture.observation,
          canonicalReceiptBytes(capture.observation),
        );
      }
    } catch (error) {
      addDiagnostic(diagnostics, error.code ?? "receipt.schema", path, error.message);
    }
  }
  // recompute exact binding coverage and states
  if (runtimeMap !== null && captures.length === 2) {
    try {
      const expectedBindings = deriveBindings(runtimeMap, captures, receipt.phase);

      // reject caller-authored state or coverage
      if (canonicalHostJson(receipt.bindings) !== canonicalHostJson(expectedBindings)) {
        addDiagnostic(diagnostics, "receipt.binding", "$.bindings", "receipt bindings differ from checked observations");
      }
    } catch (error) {
      addDiagnostic(diagnostics, error.code ?? "receipt.binding", "$.bindings", error.message);
    }
  }
  const toolingKeys = new Set(Object.keys(toolingPaths));

  // require every distinct local tooling identity
  if (!hasExactKeys(receipt?.tooling, toolingKeys)) {
    addDiagnostic(diagnostics, "receipt.schema", "$.tooling", "tooling identities are invalid");
  } else {
    // compare each retained tool to exact executing bytes
    for (const [name, sourcePath] of Object.entries(toolingPaths)) {
      const item = receipt.tooling[name];

      // bind source path and content address
      if (!hasExactKeys(item, new Set(["artifact", "sourcePath"])) || item.sourcePath !== sourcePath) {
        addDiagnostic(diagnostics, "receipt.schema", `$.tooling.${name}`, "tool identity is invalid");
        continue;
      }
      const bytes = validateArtifactReference(item.artifact, retained, `$.tooling.${name}.artifact`, diagnostics);

      // prevent source-class substitutions
      if (!historical && bytes !== null && !bytes.equals(readFileSync(resolve(repositoryRoot, sourcePath)))) {
        addDiagnostic(diagnostics, "receipt.digest", `$.tooling.${name}`, "tool bytes differ from the executing tree");
      }
    }
  }
  const previousReceipt = options.previousReceipt ?? null;
  let chain = null;

  // derive exact phase and predecessor identity
  try {
    chain = deriveChainIdentity(receipt?.phase, previousReceipt);
  } catch (error) {
    addDiagnostic(diagnostics, error.code ?? "receipt.phase", "$.phase", error.message);
  }
  if (chain !== null && (receipt.sequence !== chain.sequence || receipt.generation !== chain.generation ||
    receipt.previousReceiptSha256 !== chain.previousReceiptSha256)) {
    addDiagnostic(diagnostics, "chain.previous", "$", "receipt chain identity differs");
  }
  // require one valid receipt interval
  if (!isTimestamp(receipt?.startedAt) || !isTimestamp(receipt?.completedAt) || !isTimestamp(receipt?.writtenAt) ||
    Date.parse(receipt?.startedAt) > Date.parse(receipt?.completedAt) ||
    Date.parse(receipt?.completedAt) > Date.parse(receipt?.writtenAt)) {
    addDiagnostic(diagnostics, "receipt.chronology", "$", "receipt interval is invalid");
  }
  const intervals = captures.map(
    // retain transport intervals
    (capture) => capture?.transport,
  ).sort(
    // order by actual start
    (left, right) => Date.parse(left?.startedAt) - Date.parse(right?.startedAt),
  );

  // require the receipt interval to equal the transport envelope
  if (intervals.length > 0 && (receipt.startedAt !== intervals[0]?.startedAt || receipt.completedAt !== intervals.at(-1)?.completedAt)) {
    addDiagnostic(diagnostics, "receipt.chronology", "$", "receipt interval differs from transports");
  }
  // reject overlapping transport intervals in reusable validation
  for (let index = 1; index < intervals.length; index += 1) {
    // require strict separation between fixed profile captures
    if (Date.parse(intervals[index]?.startedAt) <= Date.parse(intervals[index - 1]?.completedAt)) {
      addDiagnostic(diagnostics, "receipt.chronology", "$.captures", "capture intervals overlap");
    }
  }
  // require strict predecessor separation
  if (previousReceipt !== null && Date.parse(receipt?.startedAt) <= Date.parse(previousReceipt?.completedAt)) {
    addDiagnostic(diagnostics, "receipt.chronology", "$.startedAt", "receipt overlaps or backdates its predecessor");
  }
  // enforce freshness only for the incoming head
  if (options.writerNow !== undefined && options.enforceIncomingFreshness !== false) {
    if (!isTimestamp(options.writerNow) || receipt?.writtenAt !== options.writerNow) {
      addDiagnostic(diagnostics, "receipt.chronology", "$.writtenAt", "writer-owned time differs");
    } else {
      const nowMs = Date.parse(options.writerNow);

      // validate each incoming capture age independently
      for (const capture of captures) {
        const completedMs = Date.parse(capture?.transport?.completedAt);

        // reject stale and future captures
        if (!Number.isFinite(completedMs) || completedMs > nowMs || nowMs - completedMs > LIVE_STATUS_MAX_AGE_MS) {
          addDiagnostic(diagnostics, "receipt.freshness", `$.captures.${String(capture?.profile)}`, "incoming capture is stale or future");
        }
      }
    }
  }
  diagnostics.sort(
    // retain stable diagnostic bytes
    (left, right) => canonicalHostJson(left).localeCompare(canonicalHostJson(right)),
  );
  return {
    canonicalEligible: false,
    diagnostics,
    ok: diagnostics.length === 0,
    sha256: hasExactKeys(receipt, receiptKeys) ? receiptSha256(receipt) : null,
  };
}

// classify one private-path error
function privatePathError(error) {
  const message = error instanceof Error ? error.message : String(error);

  // retain symlink identity
  if (error?.code === "ELOOP" || /symbolic link/u.test(message)) {
    return codedError("path.symlink", message, error);
  }
  // retain owner identity
  if (/owned by/u.test(message)) {
    return codedError("path.owner", message, error);
  }
  // retain mode identity
  if (/mode/u.test(message)) {
    return codedError("path.mode", message, error);
  }
  return codedError("path.type", message, error);
}

// validate one normalized contained path
function validateRelativePath(relativePath, label) {
  // reject traversal and platform aliases
  if (typeof relativePath !== "string" || relativePath.length === 0 || isAbsolute(relativePath) ||
    relativePath.includes("\\") || relativePath.split("/").some(
      // reject empty and dot components
      (segment) => segment === "" || segment === "." || segment === "..",
    )) {
    throw codedError("path.traversal", `${label} must be a normalized contained relative path`);
  }
  return relativePath;
}

// require exact private filesystem metadata
async function assertPrivatePath(path, kind) {
  let metadata;

  try {
    metadata = await lstat(path, { bigint: true });
  } catch (error) {
    throw privatePathError(error);
  }
  // reject symbolic links before following
  if (metadata.isSymbolicLink()) {
    throw codedError("path.symlink", `${path} must not be a symbolic link`);
  }
  // require the requested object kind
  if ((kind === "directory" && !metadata.isDirectory()) || (kind === "file" && !metadata.isFile())) {
    throw codedError("path.type", `${path} must be a ${kind}`);
  }
  // bind custody to the invoking account
  if (typeof process.getuid === "function" && metadata.uid !== BigInt(process.getuid())) {
    throw codedError("path.owner", `${path} must be owned by the invoking user`);
  }
  const expectedMode = kind === "directory" ? 0o700n : 0o600n;

  // require exact private permissions
  if ((metadata.mode & 0o777n) !== expectedMode) {
    throw codedError("path.mode", `${path} must have mode ${expectedMode.toString(8)}`);
  }
  // require a single file link
  if (kind === "file" && metadata.nlink !== 1n) {
    throw codedError("path.type", `${path} must have exactly one link`);
  }
  return metadata;
}

// compare stable private file identity
function sameIdentity(left, right) {
  return left.dev === right.dev && left.ino === right.ino && left.size === right.size &&
    left.mtimeNs === right.mtimeNs && left.ctimeNs === right.ctimeNs &&
    left.mode === right.mode && left.uid === right.uid && left.gid === right.gid && left.nlink === right.nlink;
}

// compare one pinned directory identity without mutable entry timestamps
function sameDirectoryIdentity(left, right) {
  return left.dev === right.dev && left.ino === right.ino && left.mode === right.mode &&
    left.uid === right.uid && left.gid === right.gid && left.nlink === right.nlink;
}

// compare one directory object while its owned child count is changing
function sameDirectoryObject(left, right) {
  return left.dev === right.dev && left.ino === right.ino && left.mode === right.mode &&
    left.uid === right.uid && left.gid === right.gid;
}

// map one pinned directory handle into procfs
function pinnedDirectoryPath(value, name = null) {
  const base = `/proc/self/fd/${String(value.handle.fd)}`;
  return name === null ? base : `${base}/${name}`;
}

// open one exact private directory for descriptor-relative use
async function pinPrivateDirectory(path, expectedUid = null) {
  const filesystem = await statfs("/proc/self/fd", { bigint: true });

  // require the Linux procfs descriptor bridge
  if (filesystem.type !== 0x9fa0n) {
    throw codedError("path.type", "procfs descriptor bridge is unavailable");
  }
  let handle;

  try {
    handle = await open(path, constants.O_RDONLY | constants.O_DIRECTORY | constants.O_NOFOLLOW | constants.O_NONBLOCK);
    const identity = await handle.stat({ bigint: true });
    const uid = expectedUid ?? (typeof process.getuid === "function" ? BigInt(process.getuid()) : identity.uid);

    // validate the actually opened private directory
    if (!identity.isDirectory() || identity.uid !== uid || (identity.mode & 0o777n) !== 0o700n) {
      throw codedError("path.type", `${path} is not an exact private directory`);
    }
    return { handle, identity, path };
  } catch (error) {
    // close one rejected descriptor
    if (handle !== undefined) {
      await handle.close();
    }
    throw error;
  }
}

// open one private child through its pinned parent
async function pinPrivateChildDirectory(parent, name, path) {
  let handle;

  try {
    handle = await open(
      pinnedDirectoryPath(parent, name),
      constants.O_RDONLY | constants.O_DIRECTORY | constants.O_NOFOLLOW | constants.O_NONBLOCK,
    );
    const identity = await handle.stat({ bigint: true });
    const uid = parent.identity.uid;

    // validate the actually opened private child
    if (!identity.isDirectory() || identity.uid !== uid || (identity.mode & 0o777n) !== 0o700n) {
      throw codedError("path.type", `${path} is not an exact private directory`);
    }
    return { handle, identity, path };
  } catch (error) {
    // close one rejected child descriptor
    if (handle !== undefined) {
      await handle.close();
    }
    throw error;
  }
}

// re-open one pinned child and compare its current path entry
async function revalidatePinnedDirectory(parent, child, name) {
  let reopened;

  try {
    reopened = await open(
      pinnedDirectoryPath(parent, name),
      constants.O_RDONLY | constants.O_DIRECTORY | constants.O_NOFOLLOW | constants.O_NONBLOCK,
    );
    const current = await reopened.stat({ bigint: true });

    // reject replacement of the pinned child path
    if (!sameDirectoryIdentity(current, child.identity)) {
      throw codedError("path.type", `${child.path} changed after admission`);
    }
  } catch (error) {
    // preserve symlink-specific closing failures
    if (error?.code === "ELOOP") {
      throw codedError("path.symlink", `${child.path} became a symbolic link`, error);
    }
    // classify a replaced parent or non-directory child
    if (error?.code === "ENOTDIR") {
      throw codedError("path.type", `${child.path} is no longer a directory`, error);
    }
    throw error;
  } finally {
    // close the replay descriptor
    if (reopened !== undefined) {
      await reopened.close();
    }
  }
}

// re-open one pinned absolute root and compare its current path entry
async function revalidatePinnedRoot(root) {
  const reopened = await pinPrivateDirectory(root.path);

  try {
    // reject replacement of the admitted root path
    if (!sameDirectoryIdentity(reopened.identity, root.identity)) {
      throw codedError("path.type", `${root.path} changed after admission`);
    }
  } finally {
    await reopened.handle.close();
  }
}

// create one retained custody-input closing context
function createCustodyClosing(root) {
  return { directories: [], leaves: [], root, rootMembers: null };
}

// admit only the writer-owned live-status child creation into the pinned root
async function refreshCustodyRootAfterWriterLayout(closing, expectedMembers) {
  const current = await closing.root.handle.stat({ bigint: true });
  const members = (await readdir(pinnedDirectoryPath(closing.root))).sort();

  // preserve root identity and the exact post-writer custody membership
  if (!sameDirectoryObject(current, closing.root.identity) ||
    canonicalHostJson(members) !== canonicalHostJson([...expectedMembers].sort())) {
    throw codedError("chain.member", "custody root changed while creating live-status");
  }
  closing.root.identity = current;
  closing.rootMembers = members;
}

// retain one read-only custody directory and its exact membership
async function retainCustodyDirectory(closing, parent, directory, name) {
  const proof = {
    directory,
    identity: directory.identity,
    members: null,
    name,
    parent,
  };
  closing.directories.push(proof);
  proof.members = (await readdir(pinnedDirectoryPath(directory))).sort();
}

// revalidate every authority input immediately before publication
async function revalidateCustodyClosing(closing) {
  await revalidatePinnedRoot(closing.root);
  const rootMembers = (await readdir(pinnedDirectoryPath(closing.root))).sort();

  // reject any root member added or removed after writer layout admission
  if (!Array.isArray(closing.rootMembers) ||
    canonicalHostJson(rootMembers) !== canonicalHostJson(closing.rootMembers)) {
    throw codedError("chain.member", "custody root membership changed after admission");
  }

  // close every leaf identity against its current pinned path entry
  for (const proof of closing.leaves) {
    let handle;

    try {
      handle = await open(
        pinnedDirectoryPath(proof.directory, proof.name),
        constants.O_RDONLY | constants.O_NOFOLLOW | constants.O_NONBLOCK | constants.O_NOCTTY,
      );
      // reject any in-place or replacement drift after admission
      if (!sameIdentity(await handle.stat({ bigint: true }), proof.identity)) {
        throw codedError("path.type", `${proof.label} changed after custody admission`);
      }
    } finally {
      // close every final leaf replay
      if (handle !== undefined) {
        await handle.close();
      }
    }
  }
  // close every directory identity and membership against its pinned parent
  for (const proof of closing.directories) {
    await revalidatePinnedDirectory(proof.parent, proof.directory, proof.name);
    const current = await proof.directory.handle.stat({ bigint: true });
    const members = (await readdir(pinnedDirectoryPath(proof.directory))).sort();

    // reject directory content or metadata drift after admission
    if (!sameIdentity(current, proof.identity) || canonicalHostJson(members) !== canonicalHostJson(proof.members)) {
      throw codedError("chain.member", `${proof.directory.path} changed after custody admission`);
    }
  }
}

// close every retained custody input in reverse order
async function closeCustodyClosing(closing) {
  // release child directories before the pinned root
  for (const proof of [...closing.directories].reverse()) {
    await proof.directory.handle.close();
  }
  await closing.root.handle.close();
}

// read one stable private leaf through its pinned parent
async function readStablePrivateFileAt(directory, name, closing = null, label = name) {
  let handle;

  try {
    handle = await open(
      pinnedDirectoryPath(directory, name),
      constants.O_RDONLY | constants.O_NOFOLLOW | constants.O_NONBLOCK | constants.O_NOCTTY,
    );
    const before = await handle.stat({ bigint: true });
    const uid = directory.identity.uid;

    // validate the opened private regular file before allocation
    if (!before.isFile() || before.nlink !== 1n || before.size <= 0n || before.size > BigInt(MAX_ARTIFACT_BYTES)) {
      throw codedError("path.type", `${name} is not an exact private file`);
    }
    // require the invoking account owner
    if (before.uid !== uid) {
      throw codedError("path.owner", `${name} is not owned by the invoking user`);
    }
    // require the exact private file mode
    if ((before.mode & 0o777n) !== 0o600n) {
      throw codedError("path.mode", `${name} must have mode 600`);
    }
    // retain the first opened identity through final publication
    if (closing !== null) {
      closing.leaves.push({ directory, identity: before, label, name });
    }
    const chunks = [];
    let remaining = MAX_ARTIFACT_BYTES + 1;

    // read at most the fixed cap plus one byte
    while (remaining > 0) {
      const chunk = Buffer.allocUnsafe(Math.min(65_536, remaining));
      const { bytesRead } = await handle.read(chunk, 0, chunk.length, null);

      // stop at the exact file end
      if (bytesRead === 0) {
        break;
      }
      chunks.push(chunk.subarray(0, bytesRead));
      remaining -= bytesRead;
    }
    const bytes = Buffer.concat(chunks);
    const after = await handle.stat({ bigint: true });

    // reject mutation during the bounded read
    if (bytes.length > MAX_ARTIFACT_BYTES || !sameIdentity(before, after) || after.size !== BigInt(bytes.length)) {
      throw codedError("path.type", `${name} changed while being read`);
    }
    await handle.close();
    handle = undefined;
    const replay = await open(
      pinnedDirectoryPath(directory, name),
      constants.O_RDONLY | constants.O_NOFOLLOW | constants.O_NONBLOCK | constants.O_NOCTTY,
    );

    try {
      // reject path replacement after the read
      if (!sameIdentity(before, await replay.stat({ bigint: true }))) {
        throw codedError("path.type", `${name} changed after being read`);
      }
    } finally {
      await replay.close();
    }
    return bytes;
  } catch (error) {
    // preserve stable classified failures
    if (typeof error?.code === "string" && error.code.includes(".")) {
      throw error;
    }
    throw privatePathError(error);
  } finally {
    // close interrupted file reads
    if (handle !== undefined) {
      await handle.close();
    }
  }
}

// read one bounded stable private file
async function readStablePrivateFile(path) {
  await assertPrivatePath(path, "file");
  let handle;

  try {
    handle = await open(path, constants.O_RDONLY | constants.O_NOFOLLOW);
    const before = await handle.stat({ bigint: true });

    // bound before allocation
    if (before.size <= 0n || before.size > BigInt(MAX_ARTIFACT_BYTES)) {
      throw codedError("path.type", `${path} size is invalid`);
    }
    const bytes = await handle.readFile();
    const after = await handle.stat({ bigint: true });

    // reject mutation during the read
    if (!sameIdentity(before, after) || after.size !== BigInt(bytes.length) || after.nlink !== 1n) {
      throw codedError("path.type", `${path} changed while being read`);
    }
    return bytes;
  } catch (error) {
    // preserve classified failures
    if (typeof error?.code === "string" && error.code.includes(".")) {
      throw error;
    }
    throw privatePathError(error);
  } finally {
    // close every opened descriptor
    if (handle !== undefined) {
      await handle.close();
    }
  }
}

// validate one exact private root
async function validatePrivateRoot(rootDir) {
  // reject relative and nonnormalized roots
  if (typeof rootDir !== "string" || !isAbsolute(rootDir) || resolve(rootDir) !== rootDir) {
    throw codedError("path.traversal", "rootDir must be a normalized absolute path");
  }
  await assertPrivatePath(rootDir, "directory");
  return rootDir;
}

// create and verify one contained private directory
async function preparePrivateDirectory(rootDir, relativePath) {
  const normalized = validateRelativePath(relativePath, "relativeDirectory");
  let current = rootDir;

  // create every directory component independently
  for (const segment of normalized.split("/")) {
    current = resolve(current, segment);

    // preserve containment before mutation
    if (!current.startsWith(`${rootDir}${sep}`)) {
      throw codedError("path.traversal", "private directory escapes rootDir");
    }
    try {
      await mkdir(current, { mode: 0o700 });
    } catch (error) {
      // accept one existing directory only
      if (error?.code !== "EEXIST") {
        throw error;
      }
    }
    await assertPrivatePath(current, "directory");
  }
  return current;
}

// write one reusable content-addressed artifact
async function writeArtifactCreateOnce(directory, digest, bytes) {
  const path = resolve(directory, `${digest}.bin`);

  try {
    await writeFile(path, bytes, { flag: "wx", mode: 0o600 });
  } catch (error) {
    // verify one concurrent or prior exact artifact
    if (error?.code === "EEXIST") {
      const existing = await readStablePrivateFile(path);

      // reject content substitution
      if (!existing.equals(bytes)) {
        throw codedError("receipt.digest", `existing artifact ${digest} differs`);
      }
      return;
    }
    throw error;
  }
  await assertPrivatePath(path, "file");
}

// write one canonical receipt once
async function writeReceiptCreateOnce(path, receipt) {
  try {
    await writeFile(path, canonicalReceiptBytes(receipt), { flag: "wx", mode: 0o600 });
  } catch (error) {
    // preserve create-once race identity
    if (error?.code === "EEXIST") {
      throw codedError("write.exists", `${path} already exists`, error);
    }
    throw error;
  }
  await assertPrivatePath(path, "file");
}

// create one private file through a pinned parent
async function writePinnedFileCreateOnce(directory, name, bytes, allowExactExisting) {
  let handle;

  try {
    handle = await open(
      pinnedDirectoryPath(directory, name),
      constants.O_WRONLY | constants.O_CREAT | constants.O_EXCL | constants.O_NOFOLLOW |
        constants.O_NONBLOCK | constants.O_NOCTTY,
      0o600,
    );
    await handle.writeFile(bytes);
    await handle.sync();
    const identity = await handle.stat({ bigint: true });
    const uid = typeof process.getuid === "function" ? BigInt(process.getuid()) : identity.uid;

    // validate the newly opened private leaf
    if (!identity.isFile() || identity.uid !== uid || (identity.mode & 0o777n) !== 0o600n ||
      identity.nlink !== 1n || identity.size !== BigInt(bytes.length)) {
      throw codedError("path.type", `${name} created with invalid identity`);
    }
  } catch (error) {
    // accept one prior exact content-addressed artifact only
    if (error?.code === "EEXIST" && allowExactExisting) {
      const existing = await readStablePrivateFileAt(directory, name);

      if (!existing.equals(bytes)) {
        throw codedError("receipt.digest", `existing artifact ${name} differs`);
      }
      return;
    }
    // classify one duplicate receipt publication
    if (error?.code === "EEXIST") {
      throw codedError("write.exists", `${name} already exists`, error);
    }
    throw error;
  } finally {
    // close every create descriptor
    if (handle !== undefined) {
      await handle.close();
    }
  }
  const replay = await readStablePrivateFileAt(directory, name);

  // reject closing byte drift
  if (!replay.equals(bytes)) {
    throw codedError("receipt.digest", `${name} differs after publication`);
  }
}

// collect every artifact digest referenced by one receipt
function referencedDigests(receipt) {
  const digests = new Set();

  // inspect canonical JSON recursively
  function visit(value) {
    // inspect arrays
    if (Array.isArray(value)) {
      for (const item of value) {
        visit(item);
      }
      return;
    }
    // ignore scalars
    if (!isRecord(value)) {
      return;
    }
    // retain exact artifact references
    if (hasExactKeys(value, new Set(["byteLength", "relativePath", "sha256"])) &&
      value.relativePath === `artifacts/${value.sha256}.bin`) {
      digests.add(value.sha256);
      return;
    }
    for (const nested of Object.values(value)) {
      visit(nested);
    }
  }
  visit(receipt);
  return [...digests].sort();
}

// load exact sidecars referenced by one receipt
async function loadReceiptArtifacts(directory, receipt, pinnedDirectory = null) {
  const retained = new Map();
  const readArtifact = async (name) => pinnedDirectory === null
    ? readStablePrivateFile(resolve(directory, name))
    : readStablePrivateFileAt(pinnedDirectory, name);

  // read every referenced content address
  for (const digest of referencedDigests(receipt)) {
    const bytes = await readArtifact(`${digest}.bin`);

    // bind path name to bytes
    if (sha256Bytes(bytes) !== digest) {
      throw codedError("receipt.digest", `artifact ${digest} differs from its filename`);
    }
    retained.set(digest, bytes);
  }

  // load source-proof members addressed by the retained proof documents
  if (receipt?.artifacts?.measurementSourceReceipt !== undefined) {
    const measurementBytes = retained.get(receipt.artifacts.measurementSourceReceipt.sha256);

    // resolve the exact measurement capture sidecar
    if (Buffer.isBuffer(measurementBytes)) {
      const measurement = parseCanonicalReceiptBytes(measurementBytes, "receipt.schema");
      const digest = measurement.captureSha256;

      if (digestPattern.test(digest ?? "") && !retained.has(digest)) {
        retained.set(digest, await readArtifact(`${digest}.bin`));
      }
    }
  }
  // resolve the exact deployed-source proof members
  if (receipt?.artifacts?.deployedSourceProof !== undefined) {
    const proofBytes = retained.get(receipt.artifacts.deployedSourceProof.sha256);

    if (Buffer.isBuffer(proofBytes)) {
      const proof = parseCanonicalReceiptBytes(proofBytes, "receipt.schema");

      // load every fixed digest member once
      for (const key of [
        "hostBundleSha256", "hostCaptureSha256", "hostPlanSha256", "preparerCaptureSha256",
        "recertifiedPublicationSha256",
      ]) {
        const digest = proof[key];

        // reject malformed or missing proof members
        if (!digestPattern.test(digest ?? "")) {
          throw codedError("receipt.digest", `deployed source proof ${key} is invalid`);
        }
        if (!retained.has(digest)) {
          retained.set(digest, await readArtifact(`${digest}.bin`));
        }
      }
    }
  }
  return retained;
}

// recover exact source-proof documents from retained content addresses
function sourceDocumentsFromRetained(receipt, retained) {
  const measurementReceiptBytes = retained.get(receipt.artifacts?.measurementSourceReceipt?.sha256);
  const deployedProofBytes = retained.get(receipt.artifacts?.deployedSourceProof?.sha256);
  const trustPolicyBytes = retained.get(receipt.artifacts?.capturePolicy?.sha256);
  const knownHostsBytes = retained.get(receipt.artifacts?.knownHosts?.sha256);

  // require both top-level proof classes
  if (!Buffer.isBuffer(measurementReceiptBytes) || !Buffer.isBuffer(deployedProofBytes) ||
    !Buffer.isBuffer(trustPolicyBytes) || !Buffer.isBuffer(knownHostsBytes)) {
    throw codedError("receipt.digest", "custody receipt source proof is incomplete");
  }
  const measurementReceipt = parseCanonicalReceiptBytes(measurementReceiptBytes, "receipt.schema");
  const deployedProof = parseCanonicalReceiptBytes(deployedProofBytes, "receipt.schema");
  const get = (digest, label) => {
    const bytes = retained.get(digest);

    // require one exact retained proof member
    if (!Buffer.isBuffer(bytes)) {
      throw codedError("receipt.digest", `${label} sidecar is missing`);
    }
    return bytes;
  };
  return {
    deployed: {
      hostBundleBytes: get(deployedProof.hostBundleSha256, "host bundle"),
      hostCaptureBytes: get(deployedProof.hostCaptureSha256, "host capture"),
      hostPlanBytes: get(deployedProof.hostPlanSha256, "host plan"),
      preparerCaptureBytes: get(deployedProof.preparerCaptureSha256, "preparer capture"),
      proofBytes: deployedProofBytes,
      recertifiedPublicationBytes: get(deployedProof.recertifiedPublicationSha256, "recertified publication"),
    },
    knownHostsBytes,
    measurementCaptureBytes: get(measurementReceipt.captureSha256, "measurement capture"),
    measurementReceiptBytes,
    trustPolicyBytes,
  };
}

// validate one historical source-backed custody receipt
function validateHistoricalCustodyReceipt(receipt, retained, previousReceipt) {
  const sourceDocuments = sourceDocumentsFromRetained(receipt, retained);
  validateCaptureTrust(sourceDocuments.trustPolicyBytes, sourceDocuments.knownHostsBytes, retained);
  const measurement = validateMeasurementSourceProof(
    sourceDocuments.measurementReceiptBytes,
    sourceDocuments.measurementCaptureBytes,
    retained,
    { bindExecutingBytes: false },
  );
  const deployed = validateDeployedSourceProof(
    sourceDocuments.deployed,
    retained,
    measurement.sourceBytesByPath.get(toolingPaths.receiptWriter),
  );
  validateDeployedObservationJoins(deployed, receipt.captures ?? []);

  // bind retained historical tooling to its own committed source proof
  for (const [name, sourcePath] of Object.entries(toolingPaths)) {
    const committed = measurement.receipt.files.find(
      // select the exact retained source path
      (item) => item.path === sourcePath,
    );

    // reject historical tool sidecars from another committed snapshot
    if (committed?.sha256 !== receipt.tooling?.[name]?.artifact?.sha256) {
      throw codedError("receipt.digest", `historical tooling ${name} differs from its measurement proof`);
    }
  }

  // require source proofs to precede the historical live interval
  for (const interval of [measurement.capture, deployed.captureInterval, deployed.preparerInterval]) {
    if (Date.parse(interval.completedAt) > Date.parse(receipt.startedAt)) {
      throw codedError("receipt.chronology", "historical source proof follows the live receipt");
    }
  }
  return validateCustodyReceipt(
    receipt,
    retained,
    previousReceipt,
    undefined,
    receipt.authority?.canonicalEligible === true,
    sourceDocuments,
    true,
  );
}

// validate the complete historical synthetic chain
async function readSyntheticChain(
  receiptDirectory,
  artifactDirectory,
  options = { requireCanonical: false },
  pinned = null,
) {
  // require the closed chain admission mode
  if (!hasExactKeys(options, new Set(["requireCanonical"])) || typeof options.requireCanonical !== "boolean") {
    throw codedError("receipt.schema", "chain admission mode is invalid");
  }
  const members = await readdir(
    pinned === null ? receiptDirectory : pinnedDirectoryPath(pinned.receipts),
    { withFileTypes: true },
  );
  const names = [];

  // reject unrelated chain members
  for (const member of members) {
    // accept canonical receipt filenames only
    if (!member.isFile() || !/^\d{6}-(?:baseline|pre-cutover|post-cutover|rollback-drill)\.json$/u.test(member.name)) {
      throw codedError("chain.member", `unexpected chain member ${member.name}`);
    }
    names.push(member.name);
  }
  names.sort();
  let previousReceipt = null;
  let previousBytes = null;
  const expectedArtifactNames = new Set();

  // validate each predecessor without historical freshness expiry
  for (let sequence = 0; sequence < names.length; sequence += 1) {
    const name = names[sequence];

    // reject holes and reordering
    if (!name.startsWith(`${String(sequence).padStart(6, "0")}-`)) {
      throw codedError("chain.previous", "receipt chain is not contiguous");
    }
    const bytes = pinned === null
      ? await readStablePrivateFile(resolve(receiptDirectory, name))
      : await readStablePrivateFileAt(pinned.receipts, name);
    const receipt = parseCanonicalReceiptBytes(bytes, "chain.previous");

    // bind filename to exact receipt identity
    if (name !== `${String(sequence).padStart(6, "0")}-${receipt.phase}.json` || receipt.sequence !== sequence) {
      throw codedError("chain.previous", `chain member ${name} identity differs`);
    }
    const retained = await loadReceiptArtifacts(artifactDirectory, receipt, pinned?.artifacts ?? null);

    // retain every direct and indirect artifact member
    for (const digest of retained.keys()) {
      expectedArtifactNames.add(`${digest}.bin`);
    }

    // prevent canonical history from following fixture authority
    if (options.requireCanonical &&
      (receipt.authority?.origin !== "main-fixed-custody" || receipt.authority?.canonicalEligible !== true)) {
      throw codedError("receipt.authority", `chain member ${name} is not canonical Main custody`);
    }
    const validation = ["main-fixed-custody", "synthetic-main-custody"].includes(receipt.authority?.origin)
      ? validateHistoricalCustodyReceipt(receipt, retained, previousReceipt)
      : validateLiveStatusReceipt(receipt, { artifactBytesBySha256: retained, previousReceipt });

    // reject forged historical members
    if (!validation.ok) {
      const error = codedError("chain.previous", `chain member ${name} is invalid`);
      error.diagnostics = validation.diagnostics;
      throw error;
    }
    previousReceipt = receipt;
    previousBytes = bytes;
  }
  const artifactNames = (await readdir(
    pinned === null ? artifactDirectory : pinnedDirectoryPath(pinned.artifacts),
  )).sort();

  // reject missing or unreferenced private artifact members
  if (canonicalHostJson(artifactNames) !== canonicalHostJson([...expectedArtifactNames].sort())) {
    throw codedError("chain.member", "receipt artifact membership differs from the chain");
  }
  return { previousBytes, previousReceipt };
}

// append one noncanonical structural receipt
export async function appendSyntheticLiveStatusReceipt(input) {
  const keys = new Set([
    "captures", "clock", "inventoryBytes", "measurementSourceReceiptBytes", "phase",
    "relativeDirectory", "rootDir", "runtimeMapBytes",
  ]);

  // reject open options production custody and caller freshness overrides
  if (!hasExactKeys(input, keys) || !isRecord(input.clock) || Object.keys(input.clock).length !== 1 ||
    typeof input.clock.now !== "function" || input.rootDir === PRODUCTION_CUSTODY_ROOT) {
    throw codedError("receipt.authority", "synthetic append input is invalid");
  }
  await validatePrivateRoot(input.rootDir);
  const relativeDirectory = validateRelativePath(input.relativeDirectory, "relativeDirectory");

  // keep the synthetic writer on one fixed child boundary
  if (relativeDirectory.includes("/")) {
    throw codedError("path.traversal", "relativeDirectory must contain one segment");
  }
  const outputDirectory = await preparePrivateDirectory(input.rootDir, relativeDirectory);
  const artifactDirectory = await preparePrivateDirectory(outputDirectory, "artifacts");
  const receiptDirectory = await preparePrivateDirectory(outputDirectory, "receipts");
  const outputMembers = await readdir(outputDirectory, { withFileTypes: true });

  // require the fixed two-directory layout
  if (outputMembers.length !== 2 || !outputMembers.every(
    // accept only fixed private directories
    (member) => member.isDirectory() && ["artifacts", "receipts"].includes(member.name),
  )) {
    throw codedError("chain.member", "live-status directory has unexpected members");
  }
  const storage = await pinLiveStatusStorage(input.rootDir, outputDirectory, artifactDirectory, receiptDirectory);
  let assembled;

  try {
    const chain = await readSyntheticChain(receiptDirectory, artifactDirectory, { requireCanonical: false }, storage);
    const writerNow = input.clock.now();
    assembled = assembleSyntheticLiveStatusReceipt({
      captures: input.captures,
      inventoryBytes: input.inventoryBytes,
      measurementSourceReceiptBytes: input.measurementSourceReceiptBytes,
      phase: input.phase,
      previousReceiptBytes: chain.previousBytes,
      runtimeMapBytes: input.runtimeMapBytes,
      writerNow,
    });
  } catch (error) {
    await closeLiveStatusStorage(storage);
    throw error;
  }
  return persistAssembledReceipt(assembled, storage, false);
}

// load the fixed source-proof document set from one private custody root
async function loadCustodySourceDocuments(rootDir, admittedRoot = null, closing = null) {
  const acquired = [];
  const root = admittedRoot ?? await pinPrivateDirectory(rootDir);

  // retain an internally acquired root for cleanup
  if (admittedRoot === null) {
    acquired.push(root);
  }
  try {
    const rootMembers = (await readdir(pinnedDirectoryPath(root))).sort();
    const deployed = await pinPrivateChildDirectory(root, "deployed-source", resolve(rootDir, "deployed-source"));

    // transfer each admitted handle to exactly one cleanup owner
    if (closing === null) acquired.push(deployed);
    else await retainCustodyDirectory(closing, root, deployed, "deployed-source");
    const trust = await pinPrivateChildDirectory(root, "trust", resolve(rootDir, "trust"));

    // retain trust before any later acquisition can fail
    if (closing === null) acquired.push(trust);
    else await retainCustodyDirectory(closing, root, trust, "trust");
    const tagTree = await pinPrivateChildDirectory(deployed, "tag-tree", resolve(rootDir, "deployed-source/tag-tree"));

    // retain the tag tree before reading its contents
    if (closing === null) acquired.push(tagTree);
    else await retainCustodyDirectory(closing, deployed, tagTree, "tag-tree");
    const deployedMembers = (await readdir(pinnedDirectoryPath(deployed))).sort();
    const trustMembers = (await readdir(pinnedDirectoryPath(trust))).sort();

    const documents = {
      deployed: {
        hostBundleBytes: await readStablePrivateFileAt(deployed, "host-bundle.json", closing, "deployed host bundle"),
        hostCaptureBytes: await readStablePrivateFileAt(deployed, "host-capture.json", closing, "deployed host capture"),
        hostPlanBytes: await readStablePrivateFileAt(deployed, "host-plan.json", closing, "deployed host plan"),
        preparerCaptureBytes: await readStablePrivateFileAt(deployed, "preparer-capture.json", closing, "publication preparer capture"),
        proofBytes: await readStablePrivateFileAt(deployed, "proof.json", closing, "deployed source proof"),
        recertifiedPublicationBytes: await readStablePrivateFileAt(deployed, "recertified-publication.json", closing, "recertified publication"),
      },
      knownHostsBytes: await readStablePrivateFileAt(trust, "blueberry-known-hosts", closing, "known-hosts"),
      measurementCaptureBytes: await readStablePrivateFileAt(root, "measurement-source-git-capture.json", closing, "measurement git capture"),
      measurementReceiptBytes: await readStablePrivateFileAt(root, "measurement-source-receipt.json", closing, "measurement source receipt"),
      trustPolicyBytes: await readStablePrivateFileAt(trust, "policy.json", closing, "capture trust policy"),
    };

    // require the complete fixed source and trust memberships after leaf validation
    if (canonicalHostJson(deployedMembers) !== canonicalHostJson([
      "host-bundle.json", "host-capture.json", "host-plan.json", "preparer-capture.json", "proof.json",
      "recertified-publication.json", "tag-tree",
    ]) || canonicalHostJson(trustMembers) !== canonicalHostJson(["blueberry-known-hosts", "policy.json"])) {
      throw codedError("chain.member", "custody source membership differs");
    }
    await revalidatePinnedDirectory(root, deployed, "deployed-source");
    await revalidatePinnedDirectory(root, trust, "trust");
    await revalidatePinnedDirectory(deployed, tagTree, "tag-tree");

    // reject directory membership drift during the source read
    if (canonicalHostJson((await readdir(pinnedDirectoryPath(root))).sort()) !== canonicalHostJson(rootMembers) ||
      canonicalHostJson((await readdir(pinnedDirectoryPath(deployed))).sort()) !== canonicalHostJson(deployedMembers) ||
      canonicalHostJson((await readdir(pinnedDirectoryPath(trust))).sort()) !== canonicalHostJson(trustMembers)) {
      throw codedError("chain.member", "custody source membership changed during read");
    }
    await revalidatePinnedRoot(root);
    return documents;
  } finally {
    // close only handles acquired inside this loader
    for (const value of acquired.reverse()) {
      await value.handle.close();
    }
  }
}

// load both exact noncanonical cleared-observation fixtures
async function loadSyntheticCustodyCaptures(rootDir, admittedRoot = null, closing = null) {
  const captures = [];
  const acquired = [];
  const root = admittedRoot ?? await pinPrivateDirectory(rootDir);

  // retain an internally acquired root for cleanup
  if (admittedRoot === null) {
    acquired.push(root);
  }
  const synthetic = await pinPrivateChildDirectory(root, "synthetic-captures", resolve(rootDir, "synthetic-captures"));

  // transfer the synthetic root before reading its membership
  if (closing === null) acquired.push(synthetic);
  else await retainCustodyDirectory(closing, root, synthetic, "synthetic-captures");

  try {
    const syntheticMembers = (await readdir(pinnedDirectoryPath(synthetic))).sort();

    // require both exact synthetic profiles
    if (canonicalHostJson(syntheticMembers) !== canonicalHostJson(["blueberry-compose", "workstation-runner"])) {
      throw codedError("chain.member", "synthetic capture membership differs");
    }
    // load one fixed profile fixture at a time
    for (const profile of ["blueberry-compose", "workstation-runner"]) {
      const directory = resolve(rootDir, "synthetic-captures", profile);
      const profileDirectory = await pinPrivateChildDirectory(synthetic, profile, directory);

      // transfer each profile handle before reading any member
      if (closing === null) acquired.push(profileDirectory);
      else await retainCustodyDirectory(closing, synthetic, profileDirectory, profile);
      const names = (await readdir(pinnedDirectoryPath(profileDirectory))).sort();

      // load the compact cleared-observation fixture form
      if (canonicalHostJson(names) === canonicalHostJson(["observation.json", "transport.json"])) {
        const observationBytes = await readStablePrivateFileAt(profileDirectory, "observation.json", closing, `${profile} observation`);
        const transportBytes = await readStablePrivateFileAt(profileDirectory, "transport.json", closing, `${profile} transport`);
        captures.push({
          kind: "cleared-observation-fixture",
          observationBytes,
          transport: parseCanonicalReceiptBytes(transportBytes, "receipt.schema"),
        });
      } else if (canonicalHostJson(names) === canonicalHostJson([
        "materialized-plan.json", "native-capture.json", "output.json", "request.json",
      ])) {
        // load the exact physical Main fixture form
        captures.push({
          kind: "raw-main-custody",
          materializedPlanBytes: await readStablePrivateFileAt(profileDirectory, "materialized-plan.json", closing, `${profile} materialized plan`),
          nativeCaptureBytes: await readStablePrivateFileAt(profileDirectory, "native-capture.json", closing, `${profile} native capture`),
          outputBytes: await readStablePrivateFileAt(profileDirectory, "output.json", closing, `${profile} output`),
          requestBytes: await readStablePrivateFileAt(profileDirectory, "request.json", closing, `${profile} request`),
        });
      } else {
        throw codedError("receipt.schema", `synthetic capture directory ${profile} is invalid`);
      }
      await revalidatePinnedDirectory(synthetic, profileDirectory, profile);

      // reject profile membership drift during the read
      if (canonicalHostJson((await readdir(pinnedDirectoryPath(profileDirectory))).sort()) !== canonicalHostJson(names)) {
        throw codedError("chain.member", `synthetic capture directory ${profile} changed`);
      }
    }
    await revalidatePinnedDirectory(root, synthetic, "synthetic-captures");

    // reject profile-set drift during the read
    if (canonicalHostJson((await readdir(pinnedDirectoryPath(synthetic))).sort()) !== canonicalHostJson(syntheticMembers)) {
      throw codedError("chain.member", "synthetic capture membership changed");
    }
    await revalidatePinnedRoot(root);
    return captures;
  } finally {
    // close every directory acquired inside this loader
    for (const value of acquired.reverse()) {
      await value.handle.close();
    }
  }
}

// pin one admitted live-status directory tree
async function pinLiveStatusStorage(rootDir, outputDirectory, artifactDirectory, receiptDirectory) {
  const acquired = [];
  let root;
  let output;
  let artifacts;
  let receipts;

  try {
    root = await pinPrivateDirectory(rootDir); acquired.push(root);
    output = await pinPrivateDirectory(outputDirectory); acquired.push(output);
    artifacts = await pinPrivateDirectory(artifactDirectory); acquired.push(artifacts);
    receipts = await pinPrivateDirectory(receiptDirectory); acquired.push(receipts);
    await revalidatePinnedDirectory(root, output, outputDirectory.slice(rootDir.length + 1));
    await revalidatePinnedDirectory(output, artifacts, "artifacts");
    await revalidatePinnedDirectory(output, receipts, "receipts");
    const outputNames = (await readdir(pinnedDirectoryPath(output))).sort();

    // require the fixed directory membership through the pinned handle
    if (canonicalHostJson(outputNames) !== canonicalHostJson(["artifacts", "receipts"])) {
      throw codedError("chain.member", "live-status directory has unexpected members");
    }
    return { artifactDirectory, artifacts, output, receiptDirectory, receipts, root };
  } catch (error) {
    // close every partially admitted directory handle
    for (const value of acquired.reverse()) {
      await value.handle.close();
    }
    throw error;
  }
}

// close one pinned live-status tree in reverse order
async function closeLiveStatusStorage(storage) {
  // release children before their retained parents
  for (const value of [storage.receipts, storage.artifacts, storage.output, storage.root]) {
    await value.handle.close();
  }
}

// revalidate one pinned tree and its exact current membership
async function revalidateLiveStatusStorage(storage, artifactNames, receiptNames) {
  await revalidatePinnedRoot(storage.root);
  await revalidatePinnedDirectory(
    storage.root,
    storage.output,
    storage.output.path.slice(storage.root.path.length + 1),
  );
  await revalidatePinnedDirectory(storage.output, storage.artifacts, "artifacts");
  await revalidatePinnedDirectory(storage.output, storage.receipts, "receipts");
  const outputNames = (await readdir(pinnedDirectoryPath(storage.output))).sort();
  const currentArtifacts = (await readdir(pinnedDirectoryPath(storage.artifacts))).sort();
  const currentReceipts = (await readdir(pinnedDirectoryPath(storage.receipts))).sort();

  // bind every directory member before final publication
  if (canonicalHostJson(outputNames) !== canonicalHostJson(["artifacts", "receipts"]) ||
    canonicalHostJson(currentArtifacts) !== canonicalHostJson([...artifactNames].sort()) ||
    canonicalHostJson(currentReceipts) !== canonicalHostJson([...receiptNames].sort())) {
    throw codedError("chain.member", "live-status closing membership differs");
  }
}

// prepare one exact private live-status chain layout
async function prepareLiveStatusChain(rootDir, requireCanonical = false) {
  const outputDirectory = await preparePrivateDirectory(rootDir, "live-status");
  const artifactDirectory = await preparePrivateDirectory(outputDirectory, "artifacts");
  const receiptDirectory = await preparePrivateDirectory(outputDirectory, "receipts");
  const outputMembers = await readdir(outputDirectory, { withFileTypes: true });

  // reject any unreviewed chain member directory
  if (outputMembers.length !== 2 || !outputMembers.every(
    // accept only the fixed chain directories
    (member) => member.isDirectory() && ["artifacts", "receipts"].includes(member.name),
  )) {
    throw codedError("chain.member", "live-status directory has unexpected members");
  }
  const storage = await pinLiveStatusStorage(rootDir, outputDirectory, artifactDirectory, receiptDirectory);

  try {
    const chain = await readSyntheticChain(receiptDirectory, artifactDirectory, { requireCanonical }, storage);
    return { ...storage, chain };
  } catch (error) {
    await closeLiveStatusStorage(storage);
    throw error;
  }
}

// validate canonical chain admission without granting authority
export async function validateSyntheticCanonicalChainAdmission(input) {
  // reject open options and production-root selection
  if (!hasExactKeys(input, new Set(["rootDir"])) || input.rootDir === PRODUCTION_CUSTODY_ROOT) {
    throw codedError("receipt.authority", "synthetic canonical admission input is invalid");
  }
  const rootDir = await validatePrivateRoot(input.rootDir);
  const outputDirectory = resolve(rootDir, "live-status");
  const artifactDirectory = resolve(outputDirectory, "artifacts");
  const receiptDirectory = resolve(outputDirectory, "receipts");
  await assertPrivatePath(outputDirectory, "directory");
  await assertPrivatePath(artifactDirectory, "directory");
  await assertPrivatePath(receiptDirectory, "directory");
  const storage = await pinLiveStatusStorage(rootDir, outputDirectory, artifactDirectory, receiptDirectory);

  try {
    await readSyntheticChain(receiptDirectory, artifactDirectory, { requireCanonical: true }, storage);
    return { canonicalEligible: false, ok: true };
  } finally {
    await closeLiveStatusStorage(storage);
  }
}

// validate retained noncanonical history without assembling one incoming member
export async function validateSyntheticHistoricalCustodyChain(input) {
  // reject open options and production-root selection
  if (!hasExactKeys(input, new Set(["rootDir"])) || input.rootDir === PRODUCTION_CUSTODY_ROOT) {
    throw codedError("receipt.authority", "synthetic historical chain input is invalid");
  }
  const rootDir = await validatePrivateRoot(input.rootDir);
  const outputDirectory = resolve(rootDir, "live-status");
  const artifactDirectory = resolve(outputDirectory, "artifacts");
  const receiptDirectory = resolve(outputDirectory, "receipts");
  await assertPrivatePath(outputDirectory, "directory");
  await assertPrivatePath(artifactDirectory, "directory");
  await assertPrivatePath(receiptDirectory, "directory");
  const storage = await pinLiveStatusStorage(rootDir, outputDirectory, artifactDirectory, receiptDirectory);

  try {
    await readSyntheticChain(receiptDirectory, artifactDirectory, { requireCanonical: false }, storage);
    return { canonicalEligible: false, ok: true };
  } finally {
    await closeLiveStatusStorage(storage);
  }
}

// validate the fixed production custody principal without granting filesystem authority
export function validateSyntheticProductionCustodyIdentity(input) {
  // reject open metadata and every principal except fixed Main uid 1000
  if (!hasExactKeys(input, new Set(["uid"])) || !Number.isSafeInteger(input.uid) ||
    BigInt(input.uid) !== PRODUCTION_MAIN_UID) {
    throw codedError("path.owner", "production custody must be owned by Main uid 1000");
  }
  return { canonicalEligible: false, ok: true };
}

// persist one assembled receipt through the shared create-once writer
async function persistAssembledReceipt(assembled, storage, canonicalEligible) {
  const artifactNames = new Set(await readdir(pinnedDirectoryPath(storage.artifacts)));
  const receiptNames = new Set(await readdir(pinnedDirectoryPath(storage.receipts)));

  try {
    await revalidateLiveStatusStorage(storage, artifactNames, receiptNames);

    // persist every sidecar before its referencing receipt
    for (const [digest, bytes] of assembled.artifactBytesBySha256) {
      const name = `${digest}.bin`;
      await writePinnedFileCreateOnce(storage.artifacts, name, bytes, true);
      artifactNames.add(name);
    }
    await revalidateLiveStatusStorage(storage, artifactNames, receiptNames);
    // close every authority input immediately before final receipt publication
    if (storage.custodyClosing !== undefined) {
      await revalidateCustodyClosing(storage.custodyClosing);
    }
    const filename = `${String(assembled.receipt.sequence).padStart(6, "0")}-${assembled.receipt.phase}.json`;
    const path = resolve(storage.receiptDirectory, filename);
    await writePinnedFileCreateOnce(storage.receipts, filename, canonicalReceiptBytes(assembled.receipt), false);
    receiptNames.add(filename);
    await revalidateLiveStatusStorage(storage, artifactNames, receiptNames);
    return {
      canonicalEligible,
      path,
      receipt: assembled.receipt,
      sha256: receiptSha256(assembled.receipt),
    };
  } finally {
    await closeLiveStatusStorage(storage);
    // close every retained authority input after publication or failure
    if (storage.custodyClosing !== undefined) {
      await closeCustodyClosing(storage.custodyClosing);
    }
  }
}

// append one fully validated noncanonical Main custody fixture
export async function appendSyntheticMainCustodyReceipt(input) {
  const keys = new Set(["clock", "rootDir"]);

  // reject caller authority paths phases captures and promotion flags
  if (!hasExactKeys(input, keys) || !isRecord(input.clock) || Object.keys(input.clock).length !== 1 ||
    typeof input.clock.now !== "function" || input.rootDir === PRODUCTION_CUSTODY_ROOT) {
    throw codedError("receipt.authority", "synthetic Main custody append input is invalid");
  }
  const rootDir = input.rootDir;

  // require one normalized absolute synthetic custody root
  if (typeof rootDir !== "string" || !isAbsolute(rootDir) || resolve(rootDir) !== rootDir) {
    throw codedError("path.traversal", "rootDir must be a normalized absolute path");
  }
  const custodyRoot = await pinPrivateDirectory(rootDir);
  const custodyClosing = createCustodyClosing(custodyRoot);
  let sourceDocuments;
  let validatedSources;
  let captures;

  try {
    const rootMembers = (await readdir(pinnedDirectoryPath(custodyRoot))).sort();
    const allowedRootMembers = [
      "deployed-source", "measurement-source-git-capture.json", "measurement-source-receipt.json",
      "synthetic-captures", "trust",
    ];

    // allow only the writer-created live-status directory in later phases
    if (rootMembers.includes("live-status")) {
      allowedRootMembers.push("live-status");
      allowedRootMembers.sort();
    }
    if (canonicalHostJson(rootMembers) !== canonicalHostJson(allowedRootMembers)) {
      throw codedError("chain.member", "synthetic Main custody root membership differs");
    }
    sourceDocuments = await loadCustodySourceDocuments(rootDir, custodyRoot, custodyClosing);
    validatedSources = validateMainCustodySources(sourceDocuments);
    captures = await loadSyntheticCustodyCaptures(rootDir, custodyRoot, custodyClosing);
    await revalidatePinnedRoot(custodyRoot);
  } catch (error) {
    await closeCustodyClosing(custodyClosing);
    throw error;
  }
  let storage;

  try {
    storage = await prepareLiveStatusChain(rootDir);
    await refreshCustodyRootAfterWriterLayout(custodyClosing, [
      "deployed-source", "live-status", "measurement-source-git-capture.json",
      "measurement-source-receipt.json", "synthetic-captures", "trust",
    ]);
    storage.custodyClosing = custodyClosing;
  } catch (error) {
    // close a writer layout rejected before ownership transfer
    if (storage !== undefined) {
      await closeLiveStatusStorage(storage);
    }
    await closeCustodyClosing(custodyClosing);
    throw error;
  }
  let assembled;

  try {
    const phase = storage.chain.previousReceipt === null
      ? "baseline"
      : nextPhase.get(storage.chain.previousReceipt.phase);

    // reject one chain phase outside the fixed recovery graph
    if (phase === undefined) {
      throw codedError("receipt.phase", "receipt chain has no valid next phase");
    }
    const writerNow = input.clock.now();
    const inventoryBytes = readFileSync(resolve(repositoryRoot, "config/backend-runtime-inventory.json"));
    const runtimeMapBytes = readFileSync(resolve(repositoryRoot, "config/backend-runtime-live-map.json"));
    assembled = assembleMainCustodyReceipt({
      canonicalEligible: false,
      captures,
      inventoryBytes,
      phase,
      previousReceiptBytes: storage.chain.previousBytes,
      runtimeMapBytes,
      sourceDocuments,
      validatedSources,
      writerNow,
    });
  } catch (error) {
    await closeLiveStatusStorage(storage);
    await closeCustodyClosing(custodyClosing);
    throw error;
  }
  return persistAssembledReceipt(assembled, storage, false);
}

// validate fixed absolute custody ancestors without changing them
async function validateCustodyAncestors() {
  const ancestors = ["/", "/home", "/home/ubuntu", "/home/ubuntu/.weather"];

  // require every pinned ancestor to be a safe real directory
  for (const path of ancestors) {
    let metadata;

    try {
      metadata = await lstat(path, { bigint: true });
    } catch (cause) {
      const error = codedError("receipt.authority", "fixed Main custody root is unavailable", cause);
      error.path = PRODUCTION_CUSTODY_ROOT;
      throw error;
    }

    // reject links non-directories foreign owners and writable ancestors
    if (metadata.isSymbolicLink() || !metadata.isDirectory() ||
      ![0n, 1000n].includes(metadata.uid) || (metadata.mode & 0o022n) !== 0n) {
      throw codedError("receipt.authority", `unsafe fixed custody ancestor ${path}`);
    }
  }
}

// load one exact fixed Main capture package without executing it
async function loadFixedMainCapturePackage(operationId, captureTrust, custodyRoot, incomingDirectory, closing) {
  const profile = MAIN_CAPTURE_PLANS[operationId]?.profile;

  // reject any operation outside the two reviewed plans
  if (profile === undefined) {
    throw codedError("receipt.authority", "fixed Main capture operation is invalid");
  }
  const packagePath = resolve(PRODUCTION_CUSTODY_ROOT, "incoming", profile);
  const packageDirectory = await pinPrivateChildDirectory(incomingDirectory, profile, packagePath);
  await retainCustodyDirectory(closing, incomingDirectory, packageDirectory, profile);
  const fixedNames = [
    "backend-runtime-inventory.json", "backend-runtime-live-map.json", "capture.json",
    "observer.mjs", "output.json", "request.json",
  ];
  const packageBytesByName = new Map();

  try {
    const members = (await readdir(pinnedDirectoryPath(packageDirectory))).sort();

    // require the exact fixed package membership
    if (canonicalHostJson(members) !== canonicalHostJson(fixedNames)) {
      throw codedError("chain.member", `fixed Main package ${profile} membership differs`);
    }
    // read each exact package member stably
    for (const name of fixedNames.filter(
      // keep capture metadata separate from staged inputs
      (value) => value !== "capture.json",
    )) {
      packageBytesByName.set(name, await readStablePrivateFileAt(packageDirectory, name, closing, `${profile} ${name}`));
    }
    const captureBytes = await readStablePrivateFileAt(packageDirectory, "capture.json", closing, `${profile} capture`);
    const trustedBytesByName = new Map([
      ["backend-runtime-inventory.json", readFileSync(resolve(repositoryRoot, "config/backend-runtime-inventory.json"))],
      ["backend-runtime-live-map.json", readFileSync(resolve(repositoryRoot, "config/backend-runtime-live-map.json"))],
      ["observer.mjs", readFileSync(resolve(repositoryRoot, "scripts/backend-host-observation.mjs"))],
      ["request.json", packageBytesByName.get("request.json")],
    ]);
    const validation = validateSyntheticMainCapturePackage({
      captureBytes,
      knownHostsBytes: captureTrust.knownHostsBytes,
      operationId,
      packageBytesByName,
      trustPolicyBytes: captureTrust.trustPolicyBytes,
      trustedBytesByName,
    });

    // reject any plan vector or outcome drift
    if (!validation.ok) {
      const error = codedError("receipt.authority", `fixed Main capture ${operationId} is invalid`);
      error.diagnostics = validation.diagnostics;
      throw error;
    }
    await revalidatePinnedDirectory(incomingDirectory, packageDirectory, profile);

    // reject package membership drift during the read
    if (canonicalHostJson((await readdir(pinnedDirectoryPath(packageDirectory))).sort()) !== canonicalHostJson(members)) {
      throw codedError("chain.member", `fixed Main package ${profile} changed during read`);
    }
    await revalidatePinnedDirectory(custodyRoot, incomingDirectory, "incoming");
    const planBytes = canonicalReceiptBytes(materializeCapturePlan(operationId, packageBytesByName, trustedBytesByName));
    return { captureBytes, operationId, packageBytesByName, planBytes };
  } catch (error) {
    // close a package directory that cannot enter the retained closing set
    throw error;
  }
}

// load the fixed Main custody boundary before canonical derivation
async function loadProductionCustody() {
  await validateCustodyAncestors();
  let custodyRoot;
  let incoming;
  let closing;

  try {
    custodyRoot = await pinPrivateDirectory(PRODUCTION_CUSTODY_ROOT, PRODUCTION_MAIN_UID);
    closing = createCustodyClosing(custodyRoot);
    const rootMembers = (await readdir(pinnedDirectoryPath(custodyRoot))).sort();
    const allowedRootMembers = [
      "deployed-source", "incoming", "measurement-source-git-capture.json",
      "measurement-source-receipt.json", "trust",
    ];

    // allow only the writer-created live-status directory in later phases
    if (rootMembers.includes("live-status")) {
      allowedRootMembers.push("live-status");
      allowedRootMembers.sort();
    }
    if (canonicalHostJson(rootMembers) !== canonicalHostJson(allowedRootMembers)) {
      throw codedError("chain.member", "fixed Main custody root membership differs");
    }
    incoming = await pinPrivateChildDirectory(
      custodyRoot,
      "incoming",
      resolve(PRODUCTION_CUSTODY_ROOT, "incoming"),
    );
    await retainCustodyDirectory(closing, custodyRoot, incoming, "incoming");
    const incomingMembers = (await readdir(pinnedDirectoryPath(incoming))).sort();

    // require both fixed capture package directories
    if (canonicalHostJson(incomingMembers) !== canonicalHostJson(["blueberry-compose", "workstation-runner"])) {
      throw codedError("chain.member", "fixed Main incoming membership differs");
    }
    const sourceDocuments = await loadCustodySourceDocuments(PRODUCTION_CUSTODY_ROOT, custodyRoot, closing);
    const captureTrust = validateCaptureTrust(sourceDocuments.trustPolicyBytes, sourceDocuments.knownHostsBytes);
    const packages = [
      await loadFixedMainCapturePackage("blueberry-compose-v1", captureTrust, custodyRoot, incoming, closing),
      await loadFixedMainCapturePackage("workstation-runner-v1", captureTrust, custodyRoot, incoming, closing),
    ];
    await revalidatePinnedDirectory(custodyRoot, incoming, "incoming");

    // reject root or incoming membership drift during authority reads
    if (canonicalHostJson((await readdir(pinnedDirectoryPath(custodyRoot))).sort()) !== canonicalHostJson(rootMembers) ||
      canonicalHostJson((await readdir(pinnedDirectoryPath(incoming))).sort()) !== canonicalHostJson(incomingMembers)) {
      throw codedError("chain.member", "fixed Main custody membership changed during read");
    }
    await revalidatePinnedRoot(custodyRoot);
    return { closing, packages, sourceDocuments };
  } catch (cause) {
    // attach the fixed production boundary to unavailable-root failures
    if (custodyRoot === undefined) {
      const error = codedError("receipt.authority", "fixed Main custody root is unavailable", cause);
      error.path = PRODUCTION_CUSTODY_ROOT;
      throw error;
    }
    if (closing !== undefined) {
      await closeCustodyClosing(closing);
      closing = undefined;
      custodyRoot = undefined;
      incoming = undefined;
    }
    throw cause;
  } finally {
    // close nonretained production descriptors only
    if (closing === undefined && incoming !== undefined) {
      await incoming.handle.close();
    }
    if (closing === undefined && custodyRoot !== undefined) {
      await custodyRoot.handle.close();
    }
  }
}

// convert one validated fixed package to a physical raw capture input
function mainPackageCapture(value) {
  return {
    kind: "raw-main-custody",
    materializedPlanBytes: value.planBytes,
    nativeCaptureBytes: value.captureBytes,
    outputBytes: value.packageBytesByName.get("output.json"),
    requestBytes: value.packageBytesByName.get("request.json"),
  };
}

// append one canonical receipt from the fixed Main custody root
export async function appendLiveStatusReceipt(options) {
  // reject caller-selected paths captures clocks and adapters
  if (arguments.length !== 0 || options !== undefined) {
    throw codedError("receipt.authority", "canonical append accepts no caller-selected input");
  }
  const custody = await loadProductionCustody();
  let validatedSources;
  let first;
  let second;
  let inventoryBytes;
  let runtimeMapBytes;
  let storage;

  try {
    validatedSources = validateMainCustodySources(custody.sourceDocuments);
    [first, second] = custody.packages;
    inventoryBytes = first.packageBytesByName.get("backend-runtime-inventory.json");
    runtimeMapBytes = first.packageBytesByName.get("backend-runtime-live-map.json");

    // require both profile packages to bind the same checked source bytes
    if (!inventoryBytes.equals(second.packageBytesByName.get("backend-runtime-inventory.json")) ||
      !runtimeMapBytes.equals(second.packageBytesByName.get("backend-runtime-live-map.json"))) {
      throw codedError("receipt.digest", "fixed Main capture packages use different checked source bytes");
    }
    storage = await prepareLiveStatusChain(PRODUCTION_CUSTODY_ROOT, true);
    await refreshCustodyRootAfterWriterLayout(custody.closing, [
      "deployed-source", "incoming", "live-status", "measurement-source-git-capture.json",
      "measurement-source-receipt.json", "trust",
    ]);
    storage.custodyClosing = custody.closing;
  } catch (error) {
    // close a production writer layout rejected before ownership transfer
    if (storage !== undefined) {
      await closeLiveStatusStorage(storage);
    }
    await closeCustodyClosing(custody.closing);
    throw error;
  }
  let assembled;

  try {
    const phase = storage.chain.previousReceipt === null
      ? "baseline"
      : nextPhase.get(storage.chain.previousReceipt.phase);

    // reject one chain phase outside the fixed recovery graph
    if (phase === undefined) {
      throw codedError("receipt.phase", "receipt chain has no valid next phase");
    }
    const writerNow = new Date().toISOString();
    assembled = assembleMainCustodyReceipt({
      canonicalEligible: true,
      captures: custody.packages.map(
        // derive exact raw packages without caller input
        (value) => mainPackageCapture(value),
      ),
      inventoryBytes,
      phase,
      previousReceiptBytes: storage.chain.previousBytes,
      runtimeMapBytes,
      sourceDocuments: custody.sourceDocuments,
      validatedSources,
      writerNow,
    });
  } catch (error) {
    await closeLiveStatusStorage(storage);
    await closeCustodyClosing(custody.closing);
    throw error;
  }
  return persistAssembledReceipt(assembled, storage, true);
}

// reject direct CLI authority
async function runDirectCli() {
  throw codedError("receipt.authority", "Main custody orchestration is required");
}

// run only on direct unsupported invocation
if (process.argv[1] !== undefined && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  runDirectCli().catch(
    // emit one controlled error without capture bytes
    (error) => {
      process.stderr.write(`${error?.code ?? "receipt.error"}: ${error instanceof Error ? error.message : String(error)}\n`);
      process.exitCode = 2;
    },
  );
}
