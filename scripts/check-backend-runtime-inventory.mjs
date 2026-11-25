import { createHash } from "node:crypto";
import { lstat, readFile, readdir, realpath } from "node:fs/promises";
import { basename, dirname, extname, isAbsolute, join, posix, resolve, sep } from "node:path";
import { pathToFileURL } from "node:url";
import ts from "typescript";

export const GENERATIONS = Object.freeze(["node-v1", "rust-v1", "mixed"]);
export const PHASES = Object.freeze([
  "baseline",
  "pre-cutover",
  "post-cutover",
  "rollback-drill",
]);
export const RUNTIME_LIVE_MAP_SCHEMA_VERSION = "weather-backend-runtime-live-map/v1";

const SCHEMA_VERSION = "backend-runtime-inventory/v1";
const SCANNER_VERSION = 1;
const DEFAULT_MANIFEST_PATH = "config/backend-runtime-inventory.json";
const DEFAULT_RUNTIME_MAP_PATH = "config/backend-runtime-live-map.json";
const SUPPORTED_DOCKERFILE_DIGEST = "29a03e726fa26ef3f6bd7ff26b1de6b2a3657c679b8d76fe40aac8c8740d75cd";
const SUPPORTED_COMPOSE_DIGEST = "0fa4b2b990345a110ab324e13bf2e3528c19d530bf9a4d2b8576d3b42eebeade";
const SUPPORTED_RUNNER_INSTALLER_DIGEST = "f5edfc6fc8a91acbde590e7a207f067d9c72c3157c606d3a48a3fc441368e557";
const POLICY_GENERATIONS = Object.freeze([
  "node-v1",
  "node-v1-rollback",
  "rust-v1",
  "mixed",
]);
const ENTRY_KINDS = new Set([
  "compose-command",
  "compose-probe",
  "package-script",
  "docker-entrypoint",
  "docker-command",
  "shell-invocation",
  "systemd-service",
  "systemd-timer",
  "installer-allowlist",
  "forced-remote",
  "private-command",
  "import",
  "runtime-generation-command",
  "runtime-generation-probe",
  "native-binary",
]);
const CLASSIFICATIONS = new Set([
  "application",
  "release-control",
  "oracle",
  "build",
  "offline-research",
]);
const RUNTIMES = new Set(["node", "python", "rust", "shell", "systemd", "docker", "other"]);
const FORCED_REMOTE_NAMES = new Set(["ssh-run.sh", "ssh-dispatch.sh", "remote-ops.sh"]);
const PRIVATE_COMMAND_PATTERN =
  /adjustment|evidence|archive|model|forecast-training|scorecard|research/u;
const SHELL_EXECUTION_PATTERN =
  /(?:^|[\s;&|])(?:exec\s+)?(?:env\s+\S+\s+)*(?:node|\/(?:[A-Za-z0-9_.${}-]+\/)*node|python3?|compose|\/opt\/weather\/bin\/weather-[a-z0-9-]+)(?:\s|$)|\$\{[A-Z_]*NODE[A-Z_]*:-node\}|\$\{?[A-Z_]*(?:NODE|PYTHON)[A-Z_]*\}?|compose\s+exec/u;
const MODULE_TOKEN_PATTERN = /(?:^|[\s"'[,])([@A-Za-z0-9_./-]+\.(?:mjs|js|ts))(?:[\s"'\],]|$)/gu;
const NODE_INTERPRETER_PATTERN = /(?:^|[\s[(,;=&|])(?:node|\/(?:[A-Za-z0-9_.${}-]+\/)*node|\$\{[A-Z_]*NODE[A-Z_]*:-node\}|\$\{?[A-Z_]*NODE[A-Z_]*\}?)(?=[\s\]),;&|]|$)/u;
const PYTHON_INTERPRETER_PATTERN = /(?:^|[\s[(,;=&|])(?:(?:\/(?:[A-Za-z0-9_.${}-]+\/)*)?python(?:3(?:\.\d+)?)?|\$\{?[A-Z_]*PYTHON[A-Z_]*\}?)(?=[\s\]),;&|]|$)/u;
const RUST_APPLICATION_PATTERN = /(?:\/opt\/weather\/bin|\/home\/ubuntu\/\.weather\/adjustment-maintenance\/runner\/current\/bin)\/weather-[a-z0-9-]+/u;
const NATIVE_EVIDENCE_PACKAGE_PREFIX =
  "/opt/weather/operators/native-evidence/packages/${NATIVE_EVIDENCE_BUNDLE_SHA256}/";
const NATIVE_EVIDENCE_REMOTE_PATHS = new Set([
  "deploy/scripts/native-dashboard-release-control.sh",
  "deploy/scripts/native-evidence-remote-dispatch.sh",
  "deploy/scripts/native-generation-remote-ops.sh",
  "deploy/scripts/native-release-control-remote-dispatch.sh",
]);
const NATIVE_EVIDENCE_CONTROL_PATHS = new Set([
  "deploy/scripts/package-native-evidence-operator.sh",
  "scripts/install-native-evidence-operator.sh",
]);
const LEGACY_NODE_APPLICATION_PATHS = new Set([
  "deploy/scripts/adjustment-evaluation-export.sh",
  "deploy/scripts/adjustment-evaluation-package.mjs",
  "deploy/scripts/forecast-training-export.sh",
  "deploy/scripts/forecast-training-package.mjs",
  "deploy/scripts/install-adjustment-scorecard.sh",
  "deploy/scripts/public-stations-backfill.sh",
  "deploy/scripts/publish-adjustment-scorecard.sh",
  "deploy/scripts/tempest-backfill.sh",
  "deploy/scripts/tide-backfill.sh",
]);
const LIVE_MAP_PROFILES = new Set(["blueberry-compose", "workstation-runner"]);
const LIVE_MAP_HOST_ROLES = new Set(["blueberry", "workstation"]);
const LIVE_MAP_ROOT_KINDS = new Set([
  "compose-service",
  "docker-image-default",
  "package-script",
  "host-systemd",
  "host-file",
  "runner-package",
]);
const LIVE_MAP_ACTIVATION_SOURCES = new Set([
  "container-inspect",
  "systemd-show",
  "process-identity",
  "never-active-alternative",
]);
const LIVE_MAP_ARTIFACT_KINDS = new Set([
  "host-file",
  "container-file",
  "package-metadata",
  "image-config",
  "systemd-unit",
  "runner-file",
]);
const RUNNER_HOME_PREFIX = "${HOME}/.weather/adjustment-maintenance/runner/current/";
const RUNNER_LITERAL_HOME_PREFIX = "/home/ubuntu/.weather/adjustment-maintenance/runner/current/";

// derive executable runtimes from static command evidence
function invokedRuntimesFromText(text) {
  const runtimes = [];
  const normalized = text.replace(/["']/gu, "");

  // retain direct node interpreter evidence
  if (NODE_INTERPRETER_PATTERN.test(normalized)) {
    runtimes.push("node");
  }
  // retain direct Python interpreter evidence independently of Node
  if (PYTHON_INTERPRETER_PATTERN.test(normalized)) {
    runtimes.push("python");
  }

  // retain declared native application evidence
  if (RUST_APPLICATION_PATTERN.test(normalized)) {
    runtimes.push("rust");
  }
  return runtimes;
}

// classify one package script from source-owned context
function classifyPackageScript(scriptName, command, invokedRuntimes) {
  // reserve tests and verification as oracle-only
  if (/^(?:test(?::|$)|verify:|remote:tunnel:check$)/u.test(scriptName)) {
    return "oracle";
  }

  // reserve build graph and inventory checks as build-only
  if (
    /^(?:build(?::|$)|check(?::|$)|clean$|lint$|typecheck$|runtime-inventory:check$)/u
      .test(scriptName)
  ) {
    return "build";
  }

  // retain known offline model evaluation commands
  if (/^weather:forecast-adjustment:performance$/u.test(scriptName)) {
    return "offline-research";
  }

  // retain evaluation and evidence one-shots as control surfaces
  if (/^weather:forecast-adjustment:(?:bundle|evaluate|evaluate-retained|evidence)$/u.test(scriptName)) {
    return "release-control";
  }

  // keep remote, archive and evidence control operations outside applications
  if (/^(?:remote:|.*(?:archive|evidence|publish|pull-evaluation))/u.test(scriptName)) {
    return "release-control";
  }

  // direct application interpreters and stable service names are applications
  if (
    invokedRuntimes.length > 0 ||
    /^(?:start$|health$|weather:(?:backfill|public-stations:backfill|tides:backfill|tempest:backfill)$)/u
      .test(scriptName) ||
    RUST_APPLICATION_PATTERN.test(command)
  ) {
    return "application";
  }
  return "release-control";
}

// classify one shell execution root from source-owned context
function classifyShellRoot(kind, sourcePath, invokedRuntimes) {
  // preserve legacy business execution as application work until its native owner is selected
  if (LEGACY_NODE_APPLICATION_PATHS.has(sourcePath)) {
    return "application";
  }

  // forced and private tools remain control surfaces
  if (kind === "forced-remote" || kind === "private-command") {
    return "release-control";
  }

  // verification scripts remain build-only
  if (/verify|test/u.test(sourcePath)) {
    return "build";
  }

  // known lifecycle tooling remains release control
  if (/schema|release|maintenance|compatibility|rollout|status|backup|restore|preflight|install|pull|publish/u.test(sourcePath)) {
    return "release-control";
  }

  // a direct application interpreter is an application root
  if (invokedRuntimes.includes("node") || invokedRuntimes.includes("python") || invokedRuntimes.includes("rust")) {
    return "application";
  }
  return "release-control";
}

// choose the strongest execution classification
function strongestClassification(left, right) {
  const priority = new Map([
    ["build", 0],
    ["oracle", 1],
    ["offline-research", 2],
    ["release-control", 3],
    ["application", 4],
  ]);
  return priority.get(left) >= priority.get(right) ? left : right;
}

// hash exact source bytes
function sha256(value) {
  return createHash("sha256").update(value).digest("hex");
}

// canonicalize json-compatible values
function canonicalize(value) {
  // preserve scalar values
  if (value === null || typeof value !== "object") {
    return value;
  }

  // preserve array order
  if (Array.isArray(value)) {
    return value.map(canonicalize);
  }
  const result = {};

  // sort object keys
  for (const key of Object.keys(value).sort()) {
    result[key] = canonicalize(value[key]);
  }
  return result;
}

// serialize deterministic evidence
function canonicalJson(value) {
  return JSON.stringify(canonicalize(value));
}

// retain scanner-owned fields
function structuralEntry(entry) {
  return {
    id: entry.id,
    kind: entry.kind,
    sourcePath: entry.sourcePath,
    installedPaths: entry.installedPaths,
    sourceDigest: entry.sourceDigest,
    parents: entry.parents,
    classification: entry.classification,
    runtime: entry.runtime,
    invokedRuntimes: entry.invokedRuntimes,
    command: entry.command,
    probe: entry.probe,
  };
}

// bind a row to structural evidence
export function createEntryDigest(entry) {
  return sha256(canonicalJson(structuralEntry(entry)));
}

// create one stable diagnostic
function diagnostic(code, message, details = {}) {
  return { code, ...details, message };
}

// sort diagnostics deterministically
function sortDiagnostics(diagnostics) {
  return diagnostics.sort((left, right) => canonicalJson(left).localeCompare(canonicalJson(right)));
}

// normalize a repository-relative path
function resolveSourcePath(rootDir, sourcePath) {
  // require portable relative paths
  if (
    typeof sourcePath !== "string" ||
    sourcePath.length === 0 ||
    isAbsolute(sourcePath) ||
    sourcePath.includes("\\")
  ) {
    throw new Error("path is not repository-relative");
  }
  const normalized = posix.normalize(sourcePath);

  // reject traversal and normalization drift
  if (normalized !== sourcePath || normalized === ".." || normalized.startsWith("../")) {
    throw new Error("path traverses outside the repository");
  }
  return resolve(rootDir, ...sourcePath.split("/"));
}

// verify a regular file without following links
async function requireContainedFile(rootDir, rootRealPath, sourcePath) {
  const absolutePath = resolveSourcePath(rootDir, sourcePath);
  let currentPath = rootDir;

  // reject links in every component
  for (const segment of sourcePath.split("/")) {
    currentPath = join(currentPath, segment);
    const status = await lstat(currentPath);

    // links can escape the selected root
    if (status.isSymbolicLink()) {
      throw new Error(`symbolic link is unsupported: ${sourcePath}`);
    }
  }
  const resolvedPath = await realpath(absolutePath);
  const status = await lstat(resolvedPath);

  // require a contained regular file
  if (!resolvedPath.startsWith(`${rootRealPath}${sep}`) || !status.isFile()) {
    throw new Error(`source is not a contained regular file: ${sourcePath}`);
  }
  return absolutePath;
}

// read and hash one source
async function readSource(context, sourcePath, optional = false) {
  // reuse exact source bytes
  if (context.sources.has(sourcePath)) {
    return context.sources.get(sourcePath);
  }
  let absolutePath;

  // retain stable path diagnostics
  try {
    absolutePath = await requireContainedFile(context.rootDir, context.rootRealPath, sourcePath);
  } catch (error) {
    // allow absent optional discovery roots
    if (optional && error.code === "ENOENT") {
      return null;
    }
    const message = String(error.message);
    const code = message.includes("symbolic link")
      ? "path.symlink-escape"
      : message.includes("travers") || message.includes("relative")
        ? "path.traversal"
        : "discovery.unsupported";
    context.diagnostics.push(diagnostic(code, message, { path: sourcePath }));
    return null;
  }
  const bytes = await readFile(absolutePath);
  const source = {
    absolutePath,
    bytes,
    digest: sha256(bytes),
    text: bytes.toString("utf8"),
  };
  context.sources.set(sourcePath, source);
  return source;
}

// create one discovered row
function discoveredEntry(fields) {
  const entry = {
    id: fields.id,
    kind: fields.kind,
    sourcePath: fields.sourcePath,
    installedPaths: [...fields.installedPaths].sort(),
    sourceDigest: fields.sourceDigest,
    parents: [...fields.parents].sort((left, right) =>
      canonicalJson(left).localeCompare(canonicalJson(right))),
    classification: fields.classification ?? "release-control",
    runtime: fields.runtime,
    invokedRuntimes: [...new Set(fields.invokedRuntimes ?? (
      ["node", "python", "rust"].includes(fields.runtime) ? [fields.runtime] : []
    ))].sort(),
    command: fields.command ?? null,
    probe: fields.probe ?? null,
  };
  return { ...entry, entryDigest: createEntryDigest(entry) };
}

// add one identity or merge import parents
function addEntry(context, entry) {
  const existing = context.entries.get(entry.id);

  // retain a new identity
  if (existing === undefined) {
    context.entries.set(entry.id, entry);
    return;
  }

  // imports can have several exact parents
  if (existing.kind === "import" && entry.kind === "import") {
    const comparableExisting = {
      ...existing,
      parents: [],
      installedPaths: [],
      classification: undefined,
      entryDigest: undefined,
    };
    const comparableEntry = {
      ...entry,
      parents: [],
      installedPaths: [],
      classification: undefined,
      entryDigest: undefined,
    };

    // merge only identical module observations
    if (canonicalJson(comparableExisting) === canonicalJson(comparableEntry)) {
      const parents = new Map();

      // retain existing parents
      for (const parent of existing.parents) {
        parents.set(canonicalJson(parent), parent);
      }

      // retain new parents
      for (const parent of entry.parents) {
        parents.set(canonicalJson(parent), parent);
      }
      const installedPaths = new Set([...existing.installedPaths, ...entry.installedPaths]);
      context.entries.set(entry.id, discoveredEntry({
        ...existing,
        parents: [...parents.values()],
        installedPaths: [...installedPaths],
        classification: strongestClassification(existing.classification, entry.classification),
      }));
      return;
    }
  }
  context.diagnostics.push(diagnostic(
    "discovery.unsupported",
    `duplicate discovered id ${entry.id}`,
    { entryId: entry.id, path: entry.sourcePath },
  ));
}

// list one optional bounded directory
async function listDirectory(context, repositoryPath) {
  let absolutePath;

  // reject unsafe scanner roots
  try {
    absolutePath = resolveSourcePath(context.rootDir, repositoryPath);
  } catch (error) {
    context.diagnostics.push(diagnostic("path.traversal", String(error.message), {
      path: repositoryPath,
    }));
    return [];
  }

  // inspect the root without following links
  try {
    const status = await lstat(absolutePath);

    // reject linked roots
    if (status.isSymbolicLink()) {
      context.diagnostics.push(diagnostic("path.symlink-escape", "scanner root is a link", {
        path: repositoryPath,
      }));
      return [];
    }

    // reject non-directory roots
    if (!status.isDirectory()) {
      context.diagnostics.push(diagnostic("discovery.unsupported", "scanner root is not a directory", {
        path: repositoryPath,
      }));
      return [];
    }
  } catch (error) {
    // omit absent fixture classes
    if (error.code === "ENOENT") {
      return [];
    }
    context.diagnostics.push(diagnostic("discovery.unsupported", String(error.message), {
      path: repositoryPath,
    }));
    return [];
  }
  const members = await readdir(absolutePath, { withFileTypes: true });
  // sort directory evidence
  return members.sort((left, right) => left.name.localeCompare(right.name));
}

// walk bounded roots without following links
async function walkFiles(context, repositoryPath, predicate) {
  const files = [];
  const members = await listDirectory(context, repositoryPath);

  // inspect each direct child
  for (const member of members) {
    const memberPath = `${repositoryPath}/${member.name}`;

    // reject linked members
    if (member.isSymbolicLink()) {
      context.diagnostics.push(diagnostic("path.symlink-escape", "scanner member is a link", {
        path: memberPath,
      }));
      continue;
    }

    // recurse through ordinary directories
    if (member.isDirectory()) {
      files.push(...await walkFiles(context, memberPath, predicate));
      continue;
    }

    // retain matching files
    if (member.isFile() && predicate(memberPath)) {
      files.push(memberPath);
    }
  }
  return files;
}

// discover root and direct workspace manifests
async function packageManifestPaths(context) {
  const paths = [];
  const rootSource = await readSource(context, "package.json", true);

  // omit package discovery when metadata is absent
  if (rootSource === null) {
    return paths;
  }
  paths.push("package.json");
  let rootPackage;

  // parse root workspace metadata
  try {
    rootPackage = JSON.parse(rootSource.text);
  } catch (error) {
    context.diagnostics.push(diagnostic("discovery.unsupported", `invalid package.json: ${error.message}`, {
      path: "package.json",
    }));
    return paths;
  }
  const workspaces = Array.isArray(rootPackage.workspaces) ? rootPackage.workspaces : [];

  // support only direct-child workspaces
  for (const workspace of workspaces) {
    // reject general glob syntax
    if (typeof workspace !== "string" || !workspace.endsWith("/*")) {
      context.diagnostics.push(diagnostic("discovery.unsupported", "unsupported workspace pattern", {
        path: "package.json",
      }));
      continue;
    }
    const parentPath = workspace.slice(0, -2);
    const members = await listDirectory(context, parentPath);

    // inspect direct workspace children
    for (const member of members) {
      // omit non-directories and links
      if (!member.isDirectory() || member.isSymbolicLink()) {
        continue;
      }
      const packagePath = `${parentPath}/${member.name}/package.json`;
      const source = await readSource(context, packagePath, true);

      // retain present package manifests
      if (source !== null) {
        paths.push(packagePath);
      }
    }
  }
  return paths.sort();
}

// inventory every package script
async function discoverPackageScripts(context) {
  const packagePaths = await packageManifestPaths(context);

  // scan every direct package
  for (const packagePath of packagePaths) {
    const source = await readSource(context, packagePath);

    // skip diagnosed sources
    if (source === null) {
      continue;
    }
    let packageJson;

    // parse the checked package
    try {
      packageJson = JSON.parse(source.text);
    } catch (error) {
      context.diagnostics.push(diagnostic("discovery.unsupported", `invalid package json: ${error.message}`, {
        path: packagePath,
      }));
      continue;
    }
    const scripts = packageJson.scripts ?? {};

    // require a literal script object
    if (scripts === null || typeof scripts !== "object" || Array.isArray(scripts)) {
      context.diagnostics.push(diagnostic("discovery.unsupported", "package scripts must be an object", {
        path: packagePath,
      }));
      continue;
    }

    // inventory every named script
    for (const scriptName of Object.keys(scripts).sort()) {
      const command = scripts[scriptName];

      // reject nonliteral executable metadata
      if (typeof command !== "string" || command.length === 0) {
        context.diagnostics.push(diagnostic("discovery.unsupported", "package script must be a string", {
          entryId: `package-script:${packagePath}#${scriptName}`,
          path: packagePath,
        }));
        continue;
      }
      const runtime = /(?:^|\s)node(?:\s|$)/u.test(command)
        ? "node"
        : PYTHON_INTERPRETER_PATTERN.test(command.replace(/["']/gu, "")) ? "python"
        : /\/opt\/weather\/bin\/weather-[a-z0-9-]+/u.test(command)
          ? "rust"
          : "shell";
      const invokedRuntimes = invokedRuntimesFromText(command);
      addEntry(context, discoveredEntry({
        id: `package-script:${packagePath}#${scriptName}`,
        kind: "package-script",
        sourcePath: packagePath,
        installedPaths: [],
        sourceDigest: source.digest,
        parents: [],
        classification: classifyPackageScript(scriptName, command, invokedRuntimes),
        runtime,
        invokedRuntimes,
        command,
        probe: null,
      }));
    }
  }
}

// extract one bounded yaml scalar or block
function extractYamlField(lines, key) {
  const matches = [];

  // inspect exact field names
  for (let index = 0; index < lines.length; index += 1) {
    const match = lines[index].match(new RegExp(`^(\\s*)${key}:\\s*(.*)$`, "u"));

    // skip unrelated yaml
    if (match === null) {
      continue;
    }
    const indentation = match[1].length;
    const inlineValue = match[2].trim();

    // retain inline values
    if (inlineValue.length > 0) {
      matches.push(inlineValue);
      continue;
    }
    const block = [];

    // retain the more-indented block
    for (let cursor = index + 1; cursor < lines.length; cursor += 1) {
      const line = lines[cursor];
      const trimmed = line.trim();
      const childIndentation = line.length - line.trimStart().length;

      // stop at the next peer field
      if (trimmed.length > 0 && childIndentation <= indentation) {
        break;
      }

      // omit formatting comments
      if (trimmed.length > 0 && !trimmed.startsWith("#")) {
        block.push(trimmed);
      }
    }
    matches.push(block.join(" "));
  }
  return matches;
}

// inventory compose commands and probes
async function discoverCompose(context) {
  const members = await listDirectory(context, "deploy");
  const composePaths = [];

  // select direct compose manifests
  for (const member of members) {
    // retain bounded compose names
    if (member.isFile() && /^compose(?:\.[a-z0-9-]+)?\.yaml$/u.test(member.name)) {
      composePaths.push(`deploy/${member.name}`);
    }
  }

  // scan each compose manifest
  for (const composePath of composePaths.sort()) {
    const source = await readSource(context, composePath);

    // skip diagnosed sources
    if (source === null) {
      continue;
    }
    const lines = source.text.split(/\r?\n/u);
    const servicesIndex = lines.indexOf("services:");

    // reject executable yaml anchors instead of evaluating yaml merges
    for (let index = 0; index < lines.length; index += 1) {
      const anchor = lines[index].match(/^(\s*)[^#]*&[A-Za-z0-9_-]+\s*$/u);

      // skip ordinary yaml lines
      if (anchor === null) {
        continue;
      }
      const indentation = anchor[1].length;
      const block = [];

      // retain the anchor body
      for (let cursor = index + 1; cursor < lines.length; cursor += 1) {
        const line = lines[cursor];
        const trimmed = line.trim();
        const childIndentation = line.length - line.trimStart().length;

        // stop at the next peer mapping
        if (trimmed.length > 0 && childIndentation <= indentation) {
          break;
        }
        block.push(line);
      }

      // fail closed on inherited executable fields
      if (extractYamlField(block, "command").length > 0 ||
        extractYamlField(block, "entrypoint").length > 0 ||
        extractYamlField(block, "test").length > 0) {
        context.diagnostics.push(diagnostic(
          "discovery.unsupported",
          "compose executable yaml anchors are unsupported",
          { path: composePath },
        ));
      }
    }

    // require an explicit services map
    if (servicesIndex < 0) {
      context.diagnostics.push(diagnostic("discovery.unsupported", "compose services map is missing", {
        path: composePath,
      }));
      continue;
    }
    const serviceStarts = [];

    // locate direct service keys
    for (let index = servicesIndex + 1; index < lines.length; index += 1) {
      const line = lines[index];

      // stop at the next top-level key
      if (/^[A-Za-z0-9_-]+:\s*$/u.test(line)) {
        break;
      }
      const match = line.match(/^  (?:(?:"([A-Za-z0-9_-]+)")|(?:'([A-Za-z0-9_-]+)')|([A-Za-z0-9_-]+)):\s*$/u);

      // retain a literal service
      if (match !== null) {
        serviceStarts.push({ name: match[1] ?? match[2] ?? match[3], index });
        continue;
      }

      // reject inline or aliased service maps
      if (/^  (?:"[A-Za-z0-9_-]+"|'[A-Za-z0-9_-]+'|[A-Za-z0-9_-]+):\s*\S/u.test(line)) {
        context.diagnostics.push(diagnostic(
          "discovery.unsupported",
          "inline or aliased compose service maps are unsupported",
          { path: composePath },
        ));
      }
    }

    // inspect each service block
    for (let position = 0; position < serviceStarts.length; position += 1) {
      const service = serviceStarts[position];
      const end = serviceStarts[position + 1]?.index ?? lines.length;
      const block = lines.slice(service.index + 1, end);
      const commands = extractYamlField(block, "command");
      const entrypoints = extractYamlField(block, "entrypoint");
      const probes = extractYamlField(block, "test");

      // reject repeated executable fields
      if (commands.length > 1 || entrypoints.length > 1 || probes.length > 1) {
        context.diagnostics.push(diagnostic("discovery.unsupported", "compose executable field repeats", {
          path: composePath,
        }));
        continue;
      }
      const evidence = canonicalJson({
        entrypoint: entrypoints[0] ?? null,
        command: commands[0] ?? null,
      });

      // inventory a command or entrypoint
      if (commands.length === 1 || entrypoints.length === 1) {
        // reject executable aliases instead of interpreting yaml
        if ([...commands, ...entrypoints].some((value) => value.startsWith("*"))) {
          context.diagnostics.push(diagnostic(
            "discovery.unsupported",
            "compose executable aliases are unsupported",
            { path: composePath },
          ));
          continue;
        }
        const runtime = /(?:^|[\s[,])node(?:[\s,\]]|$)/u.test(evidence)
          ? "node"
          : PYTHON_INTERPRETER_PATTERN.test(evidence.replace(/["']/gu, "")) ? "python"
          : /\/opt\/weather\/bin\/weather-[a-z0-9-]+/u.test(evidence)
            ? "rust"
            : "docker";
        const invokedRuntimes = invokedRuntimesFromText(evidence);
        addEntry(context, discoveredEntry({
          id: `compose-command:${composePath}#${service.name}`,
          kind: "compose-command",
          sourcePath: composePath,
          installedPaths: [`/opt/weather/current/${composePath}`],
          sourceDigest: source.digest,
          parents: [],
          classification: composePath === "deploy/compose.yaml" && invokedRuntimes.length > 0
            ? "application"
            : composePath === "deploy/compose.yaml"
              ? "release-control"
              : "build",
          runtime,
          invokedRuntimes,
          command: evidence,
          probe: null,
        }));
      }

      // inventory an exact health probe
      if (probes.length === 1) {
        const runtime = /(?:^|[\s[,])node(?:[\s,\]]|$)/u.test(probes[0])
          ? "node"
          : PYTHON_INTERPRETER_PATTERN.test(probes[0].replace(/["']/gu, "")) ? "python"
          : /\/opt\/weather\/bin\/weather-[a-z0-9-]+/u.test(probes[0])
            ? "rust"
            : "docker";
        const invokedRuntimes = invokedRuntimesFromText(probes[0]);
        addEntry(context, discoveredEntry({
          id: `compose-probe:${composePath}#${service.name}`,
          kind: "compose-probe",
          sourcePath: composePath,
          installedPaths: [`/opt/weather/current/${composePath}`],
          sourceDigest: source.digest,
          parents: [{
            type: "invocation",
            id: `compose-command:${composePath}#${service.name}`,
            path: composePath,
          }],
          classification: composePath === "deploy/compose.yaml" && invokedRuntimes.length > 0
            ? "application"
            : composePath === "deploy/compose.yaml"
              ? "release-control"
              : "build",
          runtime,
          invokedRuntimes,
          command: null,
          probe: probes[0],
        }));
      }
    }
  }
}

// inventory docker stage commands
async function discoverDockerfile(context) {
  const source = await readSource(context, "Dockerfile", true);

  // omit absent fixture packaging
  if (source === null) {
    return;
  }
  let stage = "unnamed";
  const counts = new Map();
  const stageRuntimes = new Map();
  const logicalLines = [];
  let continued = "";

  // join explicit docker continuations
  for (const physicalLine of source.text.split(/\r?\n/u)) {
    const trimmed = physicalLine.trimEnd();
    continued = continued.length === 0 ? trimmed : `${continued} ${trimmed.trimStart()}`;

    // retain a continued instruction
    if (trimmed.endsWith("\\")) {
      continued = continued.slice(0, -1).trimEnd();
      continue;
    }
    logicalLines.push(continued);
    continued = "";
  }

  // retain one final continued instruction
  if (continued.length > 0) {
    logicalLines.push(continued);
  }

  // inspect deployment instructions
  for (const rawLine of logicalLines) {
    const line = rawLine.trim();
    const fromMatch = line.match(/^FROM\s+\S+(?:\s+AS\s+([A-Za-z0-9_.-]+))?$/iu);

    // update the stage context
    if (fromMatch !== null) {
      stage = fromMatch[1] ?? "unnamed";
      continue;
    }
    const instruction = line.match(/^(ENTRYPOINT|CMD)\s+(.+)$/iu);

    // skip build-only instructions
    if (instruction === null) {
      continue;
    }
    const directive = instruction[1].toUpperCase();
    const kind = directive === "ENTRYPOINT" ? "docker-entrypoint" : "docker-command";
    const key = `${stage}:${kind}`;
    const count = (counts.get(key) ?? 0) + 1;
    counts.set(key, count);
    const suffix = count === 1 ? "" : `:${count}`;
    let runtime = /(?:^|[\s[,"])node(?:[\s,\]"]|$)/u.test(instruction[2])
      ? "node"
      : PYTHON_INTERPRETER_PATTERN.test(instruction[2].replace(/["']/gu, "")) ? "python"
      : /\/opt\/weather\/bin\/weather-[a-z0-9-]+/u.test(instruction[2])
        ? "rust"
        : "docker";

    // inherit an explicit stage entrypoint runtime for command arguments
    if (runtime === "docker" && stageRuntimes.has(stage)) {
      runtime = stageRuntimes.get(stage);
    }

    // retain an explicit stage interpreter
    if (directive === "ENTRYPOINT" && ["node", "python", "rust"].includes(runtime)) {
      stageRuntimes.set(stage, runtime);
    }
    const invokedRuntimes = ["node", "python", "rust"].includes(runtime) ? [runtime] : [];
    addEntry(context, discoveredEntry({
      id: `${kind}:Dockerfile#${stage}${suffix}`,
      kind,
      sourcePath: "Dockerfile",
      installedPaths: [`image://${stage}`],
      sourceDigest: source.digest,
      parents: [],
      classification: invokedRuntimes.length > 0 ? "application" : "release-control",
      runtime,
      invokedRuntimes,
      command: instruction[2],
      probe: null,
    }));
  }
}

// decode only the JSON-shaped literals admitted for the native Compose override
function decodeRuntimeGenerationLiteral(node) {
  // retain exact string values
  if (ts.isStringLiteralLike(node)) {
    return node.text;
  }

  // retain exact ordered arrays
  if (ts.isArrayLiteralExpression(node)) {
    const values = [];

    // decode every literal array member
    for (const element of node.elements) {
      const value = decodeRuntimeGenerationLiteral(element);

      // reject unsupported expressions instead of evaluating source code
      if (value === undefined) {
        return undefined;
      }
      values.push(value);
    }
    return values;
  }

  // retain exact object literals without spreads or computed fields
  if (ts.isObjectLiteralExpression(node)) {
    const value = {};

    // decode every ordinary property assignment
    for (const property of node.properties) {
      if (
        !ts.isPropertyAssignment(property) ||
        (!ts.isIdentifier(property.name) && !ts.isStringLiteralLike(property.name))
      ) {
        return undefined;
      }
      const member = decodeRuntimeGenerationLiteral(property.initializer);

      // reject one unsupported nested expression
      if (member === undefined) {
        return undefined;
      }
      value[property.name.text] = member;
    }
    return value;
  }
  return undefined;
}

// extract the one closed Rust Compose overlay without executing its Node owner
function runtimeGenerationComposeServices(context, sourcePath, text) {
  const sourceFile = ts.createSourceFile(
    sourcePath,
    text,
    ts.ScriptTarget.Latest,
    true,
    ts.ScriptKind.JS,
  );

  // fail closed on unsupported module syntax
  if (sourceFile.parseDiagnostics.length > 0) {
    context.diagnostics.push(diagnostic(
      "discovery.unsupported",
      "runtime generation module syntax is unsupported",
      { path: sourcePath },
    ));
    return null;
  }
  const owner = sourceFile.statements.find(
    // select the exact exported function declaration
    (statement) => ts.isFunctionDeclaration(statement) &&
      statement.name?.text === "runtimeGenerationComposeOverride",
  );
  let services = null;

  // inspect only direct return statements in the closed function body
  for (const statement of owner?.body?.statements ?? []) {
    if (!ts.isReturnStatement(statement) || statement.expression === undefined) {
      continue;
    }
    const decoded = decodeRuntimeGenerationLiteral(statement.expression);

    // retain the one object return that owns the services map
    if (
      decoded !== undefined &&
      decoded !== null &&
      typeof decoded === "object" &&
      !Array.isArray(decoded) &&
      decoded.services !== undefined
    ) {
      services = decoded.services;
    }
  }
  const expectedServices = ["api", "migration", "web", "worker"];

  // require the exact finite native service set
  if (
    services === null ||
    typeof services !== "object" ||
    Array.isArray(services) ||
    canonicalJson(Object.keys(services).sort()) !== canonicalJson(expectedServices)
  ) {
    context.diagnostics.push(diagnostic(
      "discovery.unsupported",
      "runtime generation Compose override is not a closed service map",
      { path: sourcePath },
    ));
    return null;
  }
  return services;
}

// inventory the native command overlay selected by schema-preserving release metadata
async function discoverRuntimeGenerationComposeOverride(context) {
  const sourcePath = "scripts/schema-preserving-release.mjs";
  const source = await readSource(context, sourcePath, true);

  // omit an absent optional release owner in small fixtures
  if (source === null) {
    return;
  }
  const services = runtimeGenerationComposeServices(context, sourcePath, source.text);

  // stop after one bounded extraction diagnostic
  if (services === null) {
    return;
  }

  // inventory every exact generated command and optional probe
  for (const serviceName of Object.keys(services).sort()) {
    const service = services[serviceName];
    const command = service?.command;

    // require one direct native binary command per selected service
    if (
      !Array.isArray(command) ||
      command.length !== 1 ||
      typeof command[0] !== "string" ||
      !RUST_APPLICATION_PATTERN.test(command[0])
    ) {
      context.diagnostics.push(diagnostic(
        "discovery.unsupported",
        "runtime generation service command is not one native binary",
        { path: sourcePath, service: serviceName },
      ));
      continue;
    }
    const commandEvidence = canonicalJson(command);
    addEntry(context, discoveredEntry({
      id: `runtime-generation-command:${sourcePath}#${serviceName}`,
      kind: "runtime-generation-command",
      sourcePath,
      installedPaths: [],
      sourceDigest: source.digest,
      parents: [{
        type: "invocation",
        id: "shell-invocation:scripts/schema-preserving-release.sh",
        path: "scripts/schema-preserving-release.sh",
      }],
      classification: "application",
      runtime: "rust",
      invokedRuntimes: ["rust"],
      command: commandEvidence,
      probe: null,
    }));
    const probe = service?.healthcheck?.test;

    // omit migration, whose one-shot exit is its readiness boundary
    if (probe === undefined) {
      continue;
    }

    // require one fixed native health command without a shell interpreter
    if (
      !Array.isArray(probe) ||
      probe.length < 2 ||
      probe[0] !== "CMD" ||
      typeof probe[1] !== "string" ||
      !RUST_APPLICATION_PATTERN.test(probe[1]) ||
      probe.slice(2).some((argument) => typeof argument !== "string")
    ) {
      context.diagnostics.push(diagnostic(
        "discovery.unsupported",
        "runtime generation service probe is not one native binary",
        { path: sourcePath, service: serviceName },
      ));
      continue;
    }
    addEntry(context, discoveredEntry({
      id: `runtime-generation-probe:${sourcePath}#${serviceName}`,
      kind: "runtime-generation-probe",
      sourcePath,
      installedPaths: [],
      sourceDigest: source.digest,
      parents: [{
        type: "invocation",
        id: `runtime-generation-command:${sourcePath}#${serviceName}`,
        path: sourcePath,
      }],
      classification: "application",
      runtime: "rust",
      invokedRuntimes: ["rust"],
      command: null,
      probe: canonicalJson(probe),
    }));
  }
}

// inventory fixed native binaries from their checked source and concrete installed entrypoint
async function discoverNativeBinaryEntrypoints(context) {
  const dockerSource = context.sources.get("Dockerfile");
  const installerSource = context.sources.get("scripts/install-adjustment-maintenance-runner.sh");
  const hostPackageSource = await readSource(
    context,
    "deploy/scripts/package-native-evidence-operator.sh",
    true,
  );
  const hostInstallerSource = await readSource(
    context,
    "scripts/install-native-evidence-operator.sh",
    true,
  );
  const imageEnabled = dockerSource?.text.includes("FROM native-runtime AS native-server") === true;
  const runnerEnabled = installerSource?.text.includes("ADJUSTMENT_RUNNER_NATIVE_EXECUTABLES") === true;
  const hostEvidenceEnabled = hostPackageSource?.text.includes("weather-evidence") === true &&
    hostInstallerSource?.text.includes("/opt/weather/operators/native-evidence") === true;
  const specifications = [
    {
      authority: "image",
      id: "server-api",
      installedPath: "/opt/weather/bin/weather-api",
      parents: ["runtime-generation-command:scripts/schema-preserving-release.mjs#api"],
      sourcePath: "rust/crates/weather-runtime/src/bin/weather-api.rs",
    },
    {
      authority: "image",
      id: "server-api-health",
      installedPath: "/opt/weather/bin/weather-api-health",
      parents: ["runtime-generation-probe:scripts/schema-preserving-release.mjs#api"],
      sourcePath: "rust/crates/weather-runtime/src/bin/weather-http-health.rs",
    },
    {
      authority: "image",
      id: "server-migrate",
      installedPath: "/opt/weather/bin/weather-migrate",
      parents: ["runtime-generation-command:scripts/schema-preserving-release.mjs#migration"],
      sourcePath: "rust/apps/weather-migrate/src/main.rs",
    },
    {
      authority: "image",
      id: "server-worker",
      installedPath: "/opt/weather/bin/weather-worker",
      parents: ["runtime-generation-command:scripts/schema-preserving-release.mjs#worker"],
      sourcePath: "rust/crates/weather-runtime/src/bin/weather-worker.rs",
    },
    {
      authority: "image",
      command: ["/opt/weather/bin/weather-worker", "backfill"],
      id: "server-worker-open-meteo-backfill",
      installedPath: "/opt/weather/bin/weather-worker",
      parents: ["native-binary:server-worker"],
      sourcePath: "rust/crates/weather-runtime/src/bin/weather-worker.rs",
      token: 'argument == "backfill"',
    },
    {
      authority: "image",
      command: ["/opt/weather/bin/weather-worker", "public-stations-backfill"],
      id: "server-worker-public-stations-backfill",
      installedPath: "/opt/weather/bin/weather-worker",
      parents: [
        "runtime-generation-command:scripts/schema-preserving-release.mjs#worker",
        "forced-remote:deploy/scripts/native-generation-remote-ops.sh",
      ],
      sourcePath: "rust/crates/weather-runtime/src/bin/weather-worker.rs",
      token: 'Some("public-stations-backfill")',
    },
    {
      authority: "image",
      command: ["/opt/weather/bin/weather-worker", "tide-backfill"],
      id: "server-worker-tide-backfill",
      installedPath: "/opt/weather/bin/weather-worker",
      parents: [
        "runtime-generation-command:scripts/schema-preserving-release.mjs#worker",
        "forced-remote:deploy/scripts/native-generation-remote-ops.sh",
      ],
      sourcePath: "rust/crates/weather-runtime/src/bin/weather-worker.rs",
      token: 'Some("tide-backfill")',
    },
    {
      authority: "image",
      command: ["/opt/weather/bin/weather-worker", "tempest-backfill"],
      id: "server-worker-tempest-backfill",
      installedPath: "/opt/weather/bin/weather-worker",
      parents: [
        "runtime-generation-command:scripts/schema-preserving-release.mjs#worker",
        "forced-remote:deploy/scripts/native-generation-remote-ops.sh",
      ],
      sourcePath: "rust/crates/weather-runtime/src/bin/weather-worker.rs",
      token: 'Some("tempest-backfill")',
    },
    {
      authority: "image",
      id: "server-worker-health",
      installedPath: "/opt/weather/bin/weather-worker-health",
      parents: ["runtime-generation-probe:scripts/schema-preserving-release.mjs#worker"],
      sourcePath: "rust/crates/weather-runtime/src/bin/weather-worker-health.rs",
    },
    {
      authority: "image",
      id: "web",
      installedPath: "/opt/weather/bin/weather-web",
      parents: ["runtime-generation-command:scripts/schema-preserving-release.mjs#web"],
      sourcePath: "rust/crates/weather-runtime/src/bin/weather-web.rs",
    },
    {
      authority: "image",
      id: "web-health",
      installedPath: "/opt/weather/bin/weather-web-health",
      parents: ["runtime-generation-probe:scripts/schema-preserving-release.mjs#web"],
      sourcePath: "rust/crates/weather-runtime/src/bin/weather-http-health.rs",
    },
    {
      authority: "runner",
      id: "runner-capture",
      installedPath: `${RUNNER_HOME_PREFIX}bin/weather-maintenance-capture`,
      parents: [
        "systemd-service:scripts/systemd/native/weather-adjustment-archive.service",
        "systemd-service:scripts/systemd/native/weather-adjustment-daily.service",
        "systemd-service:scripts/systemd/native/weather-adjustment-monthly.service",
      ],
      sourcePath: "rust/crates/weather-runtime/src/bin/weather-maintenance-capture.rs",
    },
    {
      authority: "runner",
      id: "runner-fit",
      installedPath: `${RUNNER_HOME_PREFIX}bin/weather-maintenance-fit`,
      parents: ["native-binary:runner-capture"],
      sourcePath: "rust/crates/weather-runtime/src/bin/weather-maintenance-fit.rs",
    },
    {
      authority: "runner",
      id: "runner-wind-fit",
      installedPath: `${RUNNER_HOME_PREFIX}bin/weather-maintenance-wind-fit`,
      parents: ["native-binary:runner-capture"],
      sourcePath: "rust/crates/weather-runtime/src/bin/weather-maintenance-wind-fit.rs",
    },
    {
      authority: "host-evidence",
      classification: "release-control",
      id: "host-evidence",
      installedPath: `${NATIVE_EVIDENCE_PACKAGE_PREFIX}weather-evidence`,
      parents: [
        "forced-remote:deploy/scripts/native-evidence-remote-dispatch.sh",
        "forced-remote:deploy/scripts/native-generation-remote-ops.sh",
      ],
      sourcePath: "rust/crates/weather-runtime/src/bin/weather-evidence.rs",
    },
  ];

  // inspect every finite binary only when its owning installation surface is present
  for (const specification of specifications) {
    if (
      (specification.authority === "image" && !imageEnabled) ||
      (specification.authority === "runner" && !runnerEnabled) ||
      (specification.authority === "host-evidence" && !hostEvidenceEnabled)
    ) {
      continue;
    }
    const source = await readSource(context, specification.sourcePath);

    // skip only after recording the missing fixed source diagnostic
    if (source === null) {
      continue;
    }
    // refuse a declared worker one-shot absent from the actual installed parser
    if (specification.token !== undefined && !source.text.includes(specification.token)) {
      context.diagnostics.push(diagnostic(
        "discovery.unsupported",
        "native worker one-shot parser differs",
        { entry: specification.id, path: specification.sourcePath },
      ));
      continue;
    }
    addEntry(context, discoveredEntry({
      id: `native-binary:${specification.id}`,
      kind: "native-binary",
      sourcePath: specification.sourcePath,
      installedPaths: specification.command === undefined ? [specification.installedPath] : [],
      sourceDigest: source.digest,
      parents: specification.parents.map((id) => ({
        type: "invocation",
        id,
        path: specification.authority === "image"
          ? "scripts/schema-preserving-release.mjs"
          : "scripts/install-adjustment-maintenance-runner.sh",
      })),
      classification: specification.classification ?? "application",
      runtime: "rust",
      invokedRuntimes: ["rust"],
      command: canonicalJson(specification.command ?? [specification.installedPath]),
      probe: null,
    }));
  }
}

// join explicit shell continuations
function shellLogicalLines(text) {
  const logicalLines = [];
  let current = "";

  // preserve shell execution order
  for (const physicalLine of text.split(/\r?\n/u)) {
    const trimmedEnd = physicalLine.trimEnd();
    current = current.length === 0 ? trimmedEnd : `${current} ${trimmedEnd.trimStart()}`;

    // continue a backslash join
    if (trimmedEnd.endsWith("\\")) {
      current = current.slice(0, -1).trimEnd();
      continue;
    }
    logicalLines.push(current.trim());
    current = "";
  }

  // retain an unterminated final line
  if (current.length > 0) {
    logicalLines.push(current.trim());
  }
  return logicalLines;
}

// select direct shell execution sites
function shellExecutionObservations(lines) {
  const observations = [];

  // inspect non-comment logical lines
  for (const line of lines) {
    // skip formatting and comments
    if (line.length === 0 || line.startsWith("#")) {
      continue;
    }

    // retain explicit runtime calls
    if (SHELL_EXECUTION_PATTERN.test(line)) {
      observations.push(line.replace(/\s+/gu, " "));
    }
  }
  return observations;
}

// select forced-command cases and dispatches
function forcedRemoteObservations(context, sourcePath, lines) {
  const observations = [];
  let caseDepth = 0;

  // inspect literal cases and dispatches
  for (const line of lines) {
    // enter only literal shell case bodies before interpreting arm labels
    if (/^case(?:\s|$).*\sin$/u.test(line)) {
      caseDepth += 1;
      continue;
    }
    // leave the innermost case body without parsing esac as an arm
    if (/^esac(?:\s|$)/u.test(line)) {
      caseDepth = Math.max(0, caseDepth - 1);
      continue;
    }
    const caseMatch = caseDepth > 0
      ? line.match(/^\s*([^#][^()]*)\)\s*(?:.*)?$/u)
      : null;

    // retain a literal case label
    if (caseMatch !== null) {
      const label = caseMatch[1].trim();

      // reject computed operation labels
      if (/[$`()]/u.test(label)) {
        context.diagnostics.push(diagnostic(
          "discovery.unsupported",
          "computed forced-command case label",
          { path: sourcePath },
        ));
      } else {
        observations.push(`case:${label}`);
      }
    }

    // retain exact dispatch sites
    if (/(?:^|\s)(?:exec|ssh)(?:\s|$)|weather-remote-ops/u.test(line)) {
      observations.push(`dispatch:${line.replace(/\s+/gu, " ")}`);
    }
  }
  return observations;
}

// retain exact native package construction and installation control operations
function nativeEvidenceControlObservations(lines) {
  return lines.filter((line) =>
    /(?:^|\s)(?:flock|install|mv|readelf|sha256sum|sync)(?:\s|$)|weather-evidence|weather-remote-ops/u
      .test(line));
}

// inventory shell and forced-command surfaces
async function discoverShell(context) {
  const shellPaths = [
    ...await walkFiles(context, "deploy/scripts", (path) => path.endsWith(".sh")),
    ...await walkFiles(context, "deploy/postgres", (path) => path.endsWith(".sh")),
    ...await walkFiles(context, "scripts", (path) => path.endsWith(".sh")),
  ].sort();

  // scan each bounded shell source
  for (const sourcePath of shellPaths) {
    const source = await readSource(context, sourcePath);

    // skip diagnosed sources
    if (source === null) {
      continue;
    }
    const lines = shellLogicalLines(source.text);
    const forced = FORCED_REMOTE_NAMES.has(basename(sourcePath)) ||
      NATIVE_EVIDENCE_REMOTE_PATHS.has(sourcePath);
    const observations = forced
      ? forcedRemoteObservations(context, sourcePath, lines)
      : NATIVE_EVIDENCE_CONTROL_PATHS.has(sourcePath)
        ? nativeEvidenceControlObservations(lines)
      : shellExecutionObservations(lines);

    // omit files without runtime observations
    if (observations.length === 0) {
      continue;
    }
    const privateCommand = !forced && (
      PRIVATE_COMMAND_PATTERN.test(sourcePath) ||
      observations.some((observation) => PRIVATE_COMMAND_PATTERN.test(observation))
    );
    const kind = forced
      ? "forced-remote"
      : privateCommand
        ? "private-command"
        : "shell-invocation";
    const invokedRuntimes = invokedRuntimesFromText(observations.join("\n"));

    // bind variable-dispatched native package members to their concrete Rust executable
    if (NATIVE_EVIDENCE_REMOTE_PATHS.has(sourcePath)) {
      invokedRuntimes.push("rust");
    }
    const installedPaths = sourcePath === "deploy/scripts/native-generation-remote-ops.sh"
      ? [
        "/usr/local/sbin/weather-remote-ops",
        `${NATIVE_EVIDENCE_PACKAGE_PREFIX}native-generation-remote-ops.sh`,
      ]
      : NATIVE_EVIDENCE_REMOTE_PATHS.has(sourcePath)
        ? [`${NATIVE_EVIDENCE_PACKAGE_PREFIX}${basename(sourcePath)}`]
        : sourcePath.startsWith("deploy/")
          ? [`/opt/weather/current/${sourcePath}`]
          : [];
    addEntry(context, discoveredEntry({
      id: `${kind}:${sourcePath}`,
      kind,
      sourcePath,
      installedPaths,
      sourceDigest: source.digest,
      parents: [],
      classification: classifyShellRoot(kind, sourcePath, invokedRuntimes),
      runtime: "shell",
      invokedRuntimes,
      command: `observations:${observations.length}:sha256:${sha256(canonicalJson(observations))}`,
      probe: null,
    }));
  }
}

// parse literal installer file arrays
function installerFileArrays(context, sourcePath, lines) {
  const arrays = [];
  const knownArrays = new Map();

  // locate supported readonly file arrays
  for (let index = 0; index < lines.length; index += 1) {
    const start = lines[index].match(
      /^(?:readonly\s+-a|declare\s+-(?:ar|ra))\s+([A-Z0-9_]+_FILES)=\((.*)$/u,
    );

    // fail closed on unsupported file-array declarations
    if (start === null) {
      // reject candidate file arrays outside the bounded grammar
      if (/(?:readonly|declare).*\b[A-Z0-9_]+_FILES=\(/u.test(lines[index])) {
        context.diagnostics.push(diagnostic(
          "discovery.unsupported",
          "unsupported installer file-array declaration",
          { path: sourcePath },
        ));
      }
      continue;
    }
    const members = [];
    let remainder = start[2].trim();
    let closed = remainder.endsWith(")");

    // remove a same-line closing parenthesis
    if (closed) {
      remainder = remainder.slice(0, -1).trim();
    }
    const fragments = remainder.length === 0 ? [] : [remainder];

    // consume following member lines when needed
    while (!closed && index + 1 < lines.length) {
      index += 1;
      let line = lines[index].trim();

      // close the bounded array
      if (line === ")") {
        closed = true;
        break;
      }

      // accept a final member plus closing parenthesis
      if (line.endsWith(")")) {
        closed = true;
        line = line.slice(0, -1).trim();
      }

      // retain a nonempty member fragment
      if (line.length > 0) {
        fragments.push(line);
      }
    }

    // parse each literal fragment without shell evaluation
    for (const fragment of fragments) {
      const expansion = fragment.match(/^"\$\{([A-Z0-9_]+_FILES)\[@\]\}"$/u);

      // expand only an earlier literal file array
      if (expansion !== null && knownArrays.has(expansion[1])) {
        members.push(...knownArrays.get(expansion[1]));
        continue;
      }
      const literalMembers = [];
      const literalPattern = /"([^"$`]+)"/gu;
      let literalMatch;
      let consumed = "";

      // retain every same-line literal member
      while ((literalMatch = literalPattern.exec(fragment)) !== null) {
        literalMembers.push(literalMatch[1]);
        consumed += literalMatch[0];
      }
      const compactFragment = fragment.replace(/\s+/gu, "");
      const compactConsumed = consumed.replace(/\s+/gu, "");

      // reject substitutions and general shell arrays
      if (literalMembers.length === 0 || compactFragment !== compactConsumed) {
        context.diagnostics.push(diagnostic(
          "discovery.unsupported",
          `unsupported ${start[1]} member`,
          { path: sourcePath },
        ));
        continue;
      }
      members.push(...literalMembers);
    }

    // reject unterminated arrays
    if (!closed) {
      context.diagnostics.push(diagnostic(
        "discovery.unsupported",
        `unterminated ${start[1]} array`,
        { path: sourcePath },
      ));
    }
    knownArrays.set(start[1], members);
    arrays.push({ name: start[1], members });
  }
  return arrays;
}

// inventory installer allowlist members
async function discoverInstallerAllowlists(context) {
  const installerPaths = await walkFiles(
    context,
    "scripts",
    (path) => /^scripts\/install-[^/]+\.sh$/u.test(path),
  );

  // inspect each top-level installer
  for (const installerPath of installerPaths.sort()) {
    const installerSource = await readSource(context, installerPath);

    // skip diagnosed installers
    if (installerSource === null) {
      continue;
    }
    const arrays = installerFileArrays(
      context,
      installerPath,
      installerSource.text.split(/\r?\n/u),
    );

    // inspect each literal allowlist
    for (const array of arrays) {
      // inventory every exact member
      for (const memberPath of array.members) {
        const memberSource = await readSource(context, memberPath);

        // skip diagnosed members
        if (memberSource === null) {
          continue;
        }
        const runtime = [".js", ".mjs", ".ts"].includes(extname(memberPath))
          ? "node"
          : extname(memberPath) === ".py" ? "python" : "other";
        addEntry(context, discoveredEntry({
          id: `installer-allowlist:${installerPath}#${array.name}:${memberPath}`,
          kind: "installer-allowlist",
          sourcePath: memberPath,
          installedPaths: [
            `\${HOME}/.weather/adjustment-maintenance/runner/current/${memberPath}`,
          ],
          sourceDigest: memberSource.digest,
          parents: [{
            type: "invocation",
            id: `private-command:${installerPath}`,
            path: installerPath,
          }],
          classification: "application",
          runtime,
          invokedRuntimes: [runtime],
          command: memberPath,
          probe: null,
        }));
      }
    }
  }
}

// inventory systemd execution and timers
async function discoverSystemd(context) {
  const unitPaths = [
    ...await walkFiles(
      context,
      "deploy/systemd",
      (path) => path.endsWith(".service") || path.endsWith(".timer"),
    ),
    ...await walkFiles(
      context,
      "scripts/systemd/system",
      (path) => path.endsWith(".service") || path.endsWith(".timer"),
    ),
    ...await walkFiles(
      context,
      "scripts/systemd/native",
      (path) => path.endsWith(".service") || path.endsWith(".timer"),
    ),
  ].sort();

  // inspect every unit
  for (const unitPath of unitPaths) {
    const source = await readSource(context, unitPath);

    // skip diagnosed units
    if (source === null) {
      continue;
    }
    const service = unitPath.endsWith(".service");
    const observations = [];

    // retain runtime-relevant ini fields
    for (const line of source.text.split(/\r?\n/u)) {
      // retain bounded unit keys
      if (/^(?:ExecStart|ExecStartPre|ExecStop|OnCalendar|OnUnitActiveSec|OnBootSec|Unit)=/u.test(line)) {
        observations.push(line);
      }
    }

    // reject inert unit declarations
    if (observations.length === 0) {
      context.diagnostics.push(diagnostic(
        "discovery.unsupported",
        "unit has no executable or timer declaration",
        { path: unitPath },
      ));
      continue;
    }
    const kind = service ? "systemd-service" : "systemd-timer";
    const invokedRuntimes = invokedRuntimesFromText(observations.join("\n"));
    addEntry(context, discoveredEntry({
      id: `${kind}:${unitPath}`,
      kind,
      sourcePath: unitPath,
      installedPaths: [`/etc/systemd/system/${basename(unitPath)}`],
      sourceDigest: source.digest,
      parents: [],
      classification: unitPath.includes("weather-adjustment-")
        ? "application"
        : "release-control",
      runtime: "systemd",
      invokedRuntimes,
      command: service ? canonicalJson(observations) : null,
      probe: service ? null : canonicalJson(observations),
    }));
  }
}

// extract module-shaped command tokens
function commandModuleTokens(command) {
  const tokens = [];
  MODULE_TOKEN_PATTERN.lastIndex = 0;
  let match;

  // retain every literal module token
  while ((match = MODULE_TOKEN_PATTERN.exec(command)) !== null) {
    tokens.push(match[1]);
  }
  return tokens;
}

// extract modules that follow a supported node interpreter
function nodeExecutableModuleTokens(command) {
  const tokens = [];
  const normalized = command
    .replace(/["']/gu, "")
    .replace(/\$\{?deploy_dir\}?\/scripts\//gu, "deploy/scripts/");
  const pattern = /(?:^|[\s;&|[(,])(?:exec\s+)?(?:node|\/(?:[A-Za-z0-9_.${}-]+\/)*node|\$\{[A-Z_]*NODE[A-Z_]*:-node\}|\$\{?[A-Z_]*NODE[A-Z_]*\}?)(?:\s+|,\s*)(?:(?:--[A-Za-z0-9_.-]+(?:=[^\s,]+)?)(?:\s+|,\s*))*([A-Za-z0-9_./-]+\.(?:mjs|js|ts))/gu;
  let match;

  // retain every literal interpreter target
  while ((match = pattern.exec(normalized)) !== null) {
    tokens.push(match[1]);
  }
  return tokens;
}

// detect computed node executable targets
function hasUnsupportedNodeModuleTarget(command) {
  const normalized = command.replace(/["']/gu, "");
  return /(?:^|[\s;&|[(,])(?:exec\s+)?(?:node|\/(?:[A-Za-z0-9_.${}-]+\/)*node|\$\{[A-Z_]*NODE[A-Z_]*:-node\}|\$\{?[A-Z_]*NODE[A-Z_]*\}?)(?:\s+|,\s*)(?:(?:--[A-Za-z0-9_.-]+(?:=[^\s,]+)?)(?:\s+|,\s*))*\$\{?[A-Z_]/u
    .test(normalized);
}

// map emitted or installed paths to canonical sources
async function mapCommandModule(context, token, options = {}) {
  const originalToken = token;
  let repositoryPath = token;
  const basePath = options.basePath ?? "";
  const installRoot = options.installRoot ?? "/opt/weather";
  let installedPath;

  // recover immutable runner repository suffixes
  if (repositoryPath.includes("/current/")) {
    repositoryPath = repositoryPath.slice(repositoryPath.indexOf("/current/") + 9);
    installedPath = originalToken;
  } else if (repositoryPath.startsWith("/opt/weather/")) {
    repositoryPath = repositoryPath.slice("/opt/weather/".length);
    installedPath = originalToken;
  }

  // reject unrelated absolute image paths
  if (isAbsolute(repositoryPath)) {
    return null;
  }
  repositoryPath = repositoryPath.replace(/^\.\//u, "");
  repositoryPath = basePath.length === 0 || /^(?:apps|packages|scripts|deploy|config)\//u.test(repositoryPath)
    ? repositoryPath
    : posix.normalize(posix.join(basePath, repositoryPath));
  installedPath ??= `${installRoot}/${repositoryPath}`;
  const candidates = [repositoryPath];

  // prefer canonical typescript sources
  if (/^(?:apps|packages)\//u.test(repositoryPath) && repositoryPath.includes("/dist/")) {
    const sourcePath = repositoryPath.replace("/dist/", "/src/");
    candidates.unshift(sourcePath.endsWith(".js")
      ? sourcePath.replace(/\.js$/u, ".ts")
      : sourcePath);
  }

  // select the first present regular source
  for (const candidate of candidates) {
    let status;

    // inspect a bounded candidate
    try {
      status = await lstat(resolveSourcePath(context.rootDir, candidate));
    } catch (error) {
      // continue through absent candidates
      if (error.code === "ENOENT") {
        continue;
      }
      context.diagnostics.push(diagnostic("discovery.unsupported", String(error.message), {
        path: candidate,
      }));
      continue;
    }

    // reject linked application roots
    if (status.isSymbolicLink()) {
      context.diagnostics.push(diagnostic(
        "path.symlink-escape",
        "application module is a symbolic link",
        { path: candidate },
      ));
      return null;
    }

    // retain regular sources
    if (status.isFile()) {
      return { sourcePath: candidate, installedPath };
    }
  }
  return null;
}

// build workspace package source mappings
async function workspaceModuleMap(context) {
  const mappings = new Map();
  const packagePaths = await packageManifestPaths(context);

  // inspect each workspace package
  for (const packagePath of packagePaths) {
    // skip the root package
    if (packagePath === "package.json") {
      continue;
    }
    const source = await readSource(context, packagePath);

    // skip diagnosed sources
    if (source === null) {
      continue;
    }
    let packageJson;

    // parse package identity
    try {
      packageJson = JSON.parse(source.text);
    } catch {
      continue;
    }

    // retain named workspace packages
    if (typeof packageJson.name === "string") {
      mappings.set(packageJson.name, {
        sourcePath: `${dirname(packagePath)}/src/index.ts`,
        installedPath: `/opt/weather/${dirname(packagePath)}/dist/index.js`,
      });
    }
  }
  return mappings;
}

// decide whether one import declaration emits runtime loading
function isRuntimeImportDeclaration(node) {
  const clause = node.importClause;

  // side-effect imports always load the module
  if (clause === undefined) {
    return true;
  }

  // whole-clause type imports are erased
  if (clause.isTypeOnly) {
    return false;
  }

  // a default binding is a runtime import
  if (clause.name !== undefined) {
    return true;
  }

  // namespace imports are runtime imports
  if (clause.namedBindings !== undefined && ts.isNamespaceImport(clause.namedBindings)) {
    return true;
  }

  // mixed named imports load only with one value binding
  if (clause.namedBindings !== undefined && ts.isNamedImports(clause.namedBindings)) {
    return clause.namedBindings.elements.some((element) => !element.isTypeOnly);
  }
  return false;
}

// decide whether one re-export emits runtime loading
function isRuntimeExportDeclaration(node) {
  // type-only re-exports are erased
  if (node.isTypeOnly) {
    return false;
  }

  // exports without a module specifier do not add an edge
  if (node.moduleSpecifier === undefined) {
    return false;
  }

  // retain star and side-effect-capable re-exports
  if (node.exportClause === undefined || !ts.isNamedExports(node.exportClause)) {
    return true;
  }

  // an empty or value export retains runtime resolution
  return node.exportClause.elements.length === 0 ||
    node.exportClause.elements.some((element) => !element.isTypeOnly);
}

// collect one named import binding
function namedImportBinding(sourceFile, moduleName, importedName) {
  const bindings = new Set();

  // inspect top-level imports only
  for (const statement of sourceFile.statements) {
    // skip unrelated or type-only imports
    if (
      !ts.isImportDeclaration(statement) ||
      statement.importClause?.isTypeOnly ||
      !ts.isStringLiteralLike(statement.moduleSpecifier) ||
      statement.moduleSpecifier.text !== moduleName
    ) {
      continue;
    }
    const namedBindings = statement.importClause?.namedBindings;

    // retain matching named value imports
    if (namedBindings !== undefined && ts.isNamedImports(namedBindings)) {
      // inspect each named binding
      for (const element of namedBindings.elements) {
        const imported = element.propertyName?.text ?? element.name.text;

        // retain only the reviewed binding
        if (!element.isTypeOnly && imported === importedName) {
          bindings.add(element.name.text);
        }
      }
    }
  }
  return bindings;
}

// recognize reviewed constructed imports by exact source and ast shape
function reviewedConstructedImport(
  sourcePath,
  argument,
  joinBindings,
  urlBindings,
  initializers,
) {
  let resolvedArgument = argument;

  // resolve one same-source const initializer
  if (ts.isIdentifier(resolvedArgument) && initializers.has(resolvedArgument.text)) {
    resolvedArgument = initializers.get(resolvedArgument.text);
  }

  // require pathToFileURL(join(...)).href exactly
  if (
    !ts.isPropertyAccessExpression(resolvedArgument) ||
    resolvedArgument.name.text !== "href" ||
    !ts.isCallExpression(resolvedArgument.expression) ||
    !ts.isIdentifier(resolvedArgument.expression.expression) ||
    !urlBindings.has(resolvedArgument.expression.expression.text) ||
    resolvedArgument.expression.arguments.length !== 1
  ) {
    return null;
  }
  const joinCall = resolvedArgument.expression.arguments[0];

  // require the imported join helper
  if (
    !ts.isCallExpression(joinCall) ||
    !ts.isIdentifier(joinCall.expression) ||
    !joinBindings.has(joinCall.expression.text) ||
    joinCall.arguments.length < 2
  ) {
    return null;
  }
  const suffix = joinCall.arguments.slice(-2);

  // retain the evidence-store reviewed sibling import
  if (
    sourcePath === "deploy/scripts/adjustment-evidence-store.mjs" &&
    ts.isStringLiteralLike(suffix[0]) && suffix[0].text === "scripts" &&
    ts.isStringLiteralLike(suffix[1]) && suffix[1].text === "adjustment-evaluation-package.mjs"
  ) {
    return "./adjustment-evaluation-package.mjs";
  }

  // bind performance imports to the repository root plus one exact path
  if (
    sourcePath === "apps/worker/src/forecast-adjustment-performance-cli.ts" &&
    joinCall.arguments.length === 2 &&
    ts.isIdentifier(joinCall.arguments[0]) &&
    joinCall.arguments[0].text === "REPOSITORY_ROOT" &&
    ts.isStringLiteralLike(joinCall.arguments[1])
  ) {
    const reviewedPaths = new Map([
      [
        "deploy/scripts/adjustment-evaluation-package.mjs",
        "../../../deploy/scripts/adjustment-evaluation-package.mjs",
      ],
      [
        "deploy/scripts/forecast-adjustment-scorecard-contract.mjs",
        "../../../deploy/scripts/forecast-adjustment-scorecard-contract.mjs",
      ],
    ]);
    return reviewedPaths.get(joinCall.arguments[1].text) ?? null;
  }
  return null;
}

// recognize direct require-style loading
function isRequireCall(node) {
  // recognize global require calls
  if (ts.isIdentifier(node.expression) && node.expression.text === "require") {
    return true;
  }

  // recognize module.require calls
  return ts.isPropertyAccessExpression(node.expression) &&
    node.expression.name.text === "require";
}

// extract runtime module edges with the existing typescript parser
function moduleSpecifiers(context, sourcePath, text) {
  const scriptKind = sourcePath.endsWith(".ts") ? ts.ScriptKind.TS : ts.ScriptKind.JS;
  const sourceFile = ts.createSourceFile(
    sourcePath,
    text,
    ts.ScriptTarget.Latest,
    true,
    scriptKind,
  );
  const specifiers = new Set();

  // reject syntax the parser cannot represent safely
  if (sourceFile.parseDiagnostics.length > 0) {
    context.diagnostics.push(diagnostic(
      "discovery.unsupported",
      `module syntax is unsupported by TypeScript: ${sourceFile.parseDiagnostics[0].code}`,
      { path: sourcePath },
    ));
    return [];
  }
  const joinBindings = namedImportBinding(sourceFile, "node:path", "join");
  const urlBindings = namedImportBinding(sourceFile, "node:url", "pathToFileURL");
  const createRequireBindings = namedImportBinding(sourceFile, "node:module", "createRequire");
  const initializers = new Map();

  // collect same-source variable initializers for reviewed imports
  function collectInitializers(node) {
    // retain identifier declarations with one initializer
    if (
      ts.isVariableDeclaration(node) &&
      ts.isIdentifier(node.name) &&
      node.initializer !== undefined
    ) {
      initializers.set(node.name.text, node.initializer);
    }
    ts.forEachChild(node, collectInitializers);
  }
  collectInitializers(sourceFile);

  // reject loader aliases that escape direct require inspection
  if (createRequireBindings.size > 0) {
    context.diagnostics.push(diagnostic(
      "discovery.unsupported",
      "createRequire module loading is unsupported",
      { path: sourcePath },
    ));
  }

  // visit every executable module-loading node
  function visit(node) {
    // retain value and side-effect imports
    if (
      ts.isImportDeclaration(node) &&
      isRuntimeImportDeclaration(node) &&
      ts.isStringLiteralLike(node.moduleSpecifier)
    ) {
      specifiers.add(node.moduleSpecifier.text);
    }

    // retain value re-exports
    if (
      ts.isExportDeclaration(node) &&
      isRuntimeExportDeclaration(node) &&
      node.moduleSpecifier !== undefined &&
      ts.isStringLiteralLike(node.moduleSpecifier)
    ) {
      specifiers.add(node.moduleSpecifier.text);
    }

    // retain import-equals require literals
    if (
      ts.isImportEqualsDeclaration(node) &&
      !node.isTypeOnly &&
      ts.isExternalModuleReference(node.moduleReference) &&
      node.moduleReference.expression !== undefined
    ) {
      const expression = node.moduleReference.expression;

      // require one literal import-equals target
      if (ts.isStringLiteralLike(expression)) {
        specifiers.add(expression.text);
      } else {
        context.diagnostics.push(diagnostic(
          "discovery.unsupported",
          "nonliteral import-equals target is unsupported",
          { path: sourcePath },
        ));
      }
    }

    // inspect runtime call expressions
    if (ts.isCallExpression(node)) {
      // retain literal dynamic imports and reject computed targets
      if (node.expression.kind === ts.SyntaxKind.ImportKeyword) {
        const argument = node.arguments[0];
        const reviewedSpecifier = node.arguments.length === 1
          ? reviewedConstructedImport(
              sourcePath,
              argument,
              joinBindings,
              urlBindings,
              initializers,
            )
          : null;

        // retain one literal dynamic import
        if (node.arguments.length === 1 && ts.isStringLiteralLike(argument)) {
          specifiers.add(argument.text);
        } else if (reviewedSpecifier !== null) {
          specifiers.add(reviewedSpecifier);
        } else {
          context.diagnostics.push(diagnostic(
            "discovery.dynamic-import",
            "nonliteral dynamic import is unsupported",
            { path: sourcePath },
          ));
        }
      }

      // retain literal require calls and reject computed targets
      if (isRequireCall(node)) {
        const argument = node.arguments[0];

        // retain one literal require target
        if (node.arguments.length === 1 && ts.isStringLiteralLike(argument)) {
          specifiers.add(argument.text);
        } else {
          context.diagnostics.push(diagnostic(
            "discovery.unsupported",
            "nonliteral require target is unsupported",
            { path: sourcePath },
          ));
        }
      }
    }
    ts.forEachChild(node, visit);
  }
  visit(sourceFile);
  return [...specifiers].sort();
}

// resolve one local or workspace import
async function resolveModuleSpecifier(
  context,
  workspaceModules,
  parentPath,
  parentInstalledPath,
  specifier,
) {
  // omit node builtins and external packages
  if (
    specifier.startsWith("node:") ||
    (!specifier.startsWith(".") && !specifier.startsWith("@weather/"))
  ) {
    return null;
  }

  // resolve workspace packages explicitly
  if (specifier.startsWith("@weather/")) {
    const packageName = specifier.split("/").slice(0, 2).join("/");
    const packageRoot = workspaceModules.get(packageName);

    // reject unresolved local packages
    if (packageRoot === undefined) {
      context.diagnostics.push(diagnostic(
        "discovery.unsupported",
        `unresolved workspace import ${specifier}`,
        { path: parentPath },
      ));
      return null;
    }
    return packageRoot;
  }
  const basePath = posix.normalize(posix.join(dirname(parentPath), specifier));
  const installedPath = posix.normalize(posix.join(dirname(parentInstalledPath), specifier));

  // reject import traversal
  if (basePath === ".." || basePath.startsWith("../")) {
    context.diagnostics.push(diagnostic(
      "path.traversal",
      `import escapes repository: ${specifier}`,
      { path: parentPath },
    ));
    return null;
  }
  const candidates = [basePath];

  // prefer canonical workspace source over generated output
  if (/^(?:apps|packages)\//u.test(basePath) && basePath.includes("/dist/")) {
    const sourcePath = basePath.replace("/dist/", "/src/");
    candidates.unshift(sourcePath.endsWith(".js")
      ? sourcePath.replace(/\.js$/u, ".ts")
      : sourcePath);
  }

  // map emitted javascript specifiers to typescript
  if (basePath.endsWith(".js")) {
    candidates.unshift(basePath.replace(/\.js$/u, ".ts"));
  }

  // support extensionless local modules
  if (extname(basePath).length === 0) {
    candidates.push(`${basePath}.ts`, `${basePath}.mjs`, `${basePath}.js`);
  }

  // select the first present source
  for (const candidate of candidates) {
    let status;

    // inspect a bounded candidate
    try {
      status = await lstat(resolveSourcePath(context.rootDir, candidate));
    } catch (error) {
      // continue through absent candidates
      if (error.code === "ENOENT") {
        continue;
      }
      context.diagnostics.push(diagnostic("discovery.unsupported", String(error.message), {
        path: candidate,
      }));
      continue;
    }

    // reject linked transitive modules
    if (status.isSymbolicLink()) {
      context.diagnostics.push(diagnostic(
        "path.symlink-escape",
        "imported module is a symbolic link",
        { path: candidate },
      ));
      return null;
    }

    // retain regular modules
    if (status.isFile()) {
      return { sourcePath: candidate, installedPath };
    }
  }
  context.diagnostics.push(diagnostic(
    "discovery.unsupported",
    `unresolved local import ${specifier}`,
    { path: parentPath },
  ));
  return null;
}

// derive every supported executable module root
async function executionModuleRoots(context) {
  const roots = new Map();

  // inspect each executable row outside build-only contexts
  for (const entry of context.entries.values()) {
    // exclude type packaging and test/build commands
    if (["build", "oracle"].includes(entry.classification)) {
      continue;
    }
    let evidence = `${entry.command ?? ""} ${entry.probe ?? ""}`;
    let tokens = [];
    let basePath = "";
    let installRoot = "/opt/weather";

    // seed exact installer snapshot members
    if (entry.kind === "installer-allowlist") {
      // retain only executable module members
      if ([".js", ".mjs", ".ts"].includes(extname(entry.sourcePath))) {
        const item = {
          sourcePath: entry.sourcePath,
          installedPath: entry.installedPaths[0],
          classification: entry.classification,
          parent: { type: "invocation", id: entry.id, path: entry.sourcePath },
        };
        roots.set(canonicalJson(item), item);
      }
      continue;
    }

    // read shell source instead of its observation digest
    if (["shell-invocation", "private-command", "forced-remote"].includes(entry.kind)) {
      const source = await readSource(context, entry.sourcePath);

      // skip diagnosed shell sources
      if (source === null) {
        continue;
      }
      evidence = shellLogicalLines(source.text).join("\n");
      tokens = nodeExecutableModuleTokens(evidence);
      installRoot = "/opt/weather/current";
    } else if (entry.kind === "package-script") {
      tokens = nodeExecutableModuleTokens(evidence);
      basePath = dirname(entry.sourcePath) === "." ? "" : dirname(entry.sourcePath);
      installRoot = "/opt/weather/current";
    } else if ([
      "compose-command",
      "compose-probe",
      "docker-command",
      "docker-entrypoint",
      "systemd-service",
    ].includes(entry.kind)) {
      tokens = commandModuleTokens(evidence);
    } else {
      continue;
    }

    // reject computed executable mappings in scanned roots
    if (hasUnsupportedNodeModuleTarget(evidence)) {
      context.diagnostics.push(diagnostic(
        "discovery.unsupported",
        "computed node executable mapping is unsupported",
        { entryId: entry.id, path: entry.sourcePath },
      ));
    }

    // resolve each literal module token
    for (const token of tokens) {
      const mapping = await mapCommandModule(context, token, { basePath, installRoot });

      // omit inline probes and external images
      if (mapping === null) {
        // fail closed on missing application executable targets
        if (entry.classification === "application") {
          context.diagnostics.push(diagnostic(
            "discovery.unsupported",
            `application executable module is unresolved: ${token}`,
            { entryId: entry.id, path: entry.sourcePath },
          ));
        }
        continue;
      }
      const item = {
        ...mapping,
        classification: LEGACY_NODE_APPLICATION_PATHS.has(mapping.sourcePath)
          ? "application"
          : entry.classification,
        parent: { type: "invocation", id: entry.id, path: entry.sourcePath },
      };
      roots.set(canonicalJson(item), item);
    }
  }
  return [...roots.values()].sort((left, right) => canonicalJson(left).localeCompare(canonicalJson(right)));
}

// inventory transitive executable imports
async function discoverImports(context) {
  const workspaceModules = await workspaceModuleMap(context);
  const queue = await executionModuleRoots(context);
  const scannedPriority = new Map();

  // consume every module parent edge
  for (let index = 0; index < queue.length; index += 1) {
    const item = queue[index];
    const source = await readSource(context, item.sourcePath);

    // skip diagnosed module sources
    if (source === null) {
      continue;
    }
    const entryId = `import:${item.sourcePath}`;
    addEntry(context, discoveredEntry({
      id: entryId,
      kind: "import",
      sourcePath: item.sourcePath,
      installedPaths: item.sourcePath.startsWith(
        "scripts/research/adjustment-maintenance-runtime/",
      )
        ? [
            item.installedPath,
            `\${HOME}/.weather/adjustment-maintenance/runner/current/${item.sourcePath}`,
          ]
        : [item.installedPath],
      sourceDigest: source.digest,
      parents: [item.parent],
      classification: item.classification,
      runtime: "node",
      invokedRuntimes: ["node"],
      command: null,
      probe: null,
    }));

    const priority = ["build", "oracle", "offline-research", "release-control", "application"]
      .indexOf(item.classification);

    // rescan only when a stronger executing parent appears
    if ((scannedPriority.get(item.sourcePath) ?? -1) >= priority) {
      continue;
    }
    scannedPriority.set(item.sourcePath, priority);

    // resolve every supported import
    for (const specifier of moduleSpecifiers(context, item.sourcePath, source.text)) {
      const child = await resolveModuleSpecifier(
        context,
        workspaceModules,
        item.sourcePath,
        item.installedPath,
        specifier,
      );

      // skip externals and diagnosed failures
      if (child === null) {
        continue;
      }
      queue.push({
        ...child,
        classification: item.classification,
        parent: { type: "import", id: entryId, path: item.sourcePath },
      });
    }
  }
}

// discover the bounded static inventory
export async function discoverRuntimeInventory({ rootDir }) {
  const absoluteRoot = resolve(rootDir);
  const context = {
    rootDir: absoluteRoot,
    rootRealPath: await realpath(absoluteRoot),
    diagnostics: [],
    entries: new Map(),
    sources: new Map(),
  };
  await discoverPackageScripts(context);
  await discoverCompose(context);
  await discoverDockerfile(context);
  await discoverRuntimeGenerationComposeOverride(context);
  await discoverShell(context);
  await discoverInstallerAllowlists(context);
  await discoverSystemd(context);
  await discoverNativeBinaryEntrypoints(context);
  await discoverImports(context);
  const entries = [...context.entries.values()];

  // sort stable identities
  entries.sort((left, right) => left.id.localeCompare(right.id));
  return { entries, diagnostics: sortDiagnostics(context.diagnostics) };
}

// recognize plain json objects
function isPlainObject(value) {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

// build one deterministic source-owned context identity
function runtimeContextId(profile, rootEntryId, instanceKind, instanceValue) {
  return `runtime-context:${profile}:${rootEntryId}#${instanceKind}=${instanceValue}`;
}

// build one deterministic per-instance artifact binding identity
function runtimeBindingId(contextId, entryId) {
  return `runtime-binding:${contextId}:${entryId}`;
}

// retain one fixed runner account policy
function runnerAccountPolicy() {
  return {
    accountName: "ubuntu",
    homeTemplate: "${HOME}",
    resolution: "nss-passwd-exact",
  };
}

// normalize the one fixed runner account installation root
function runnerInstalledPath(installedPath) {
  // retain the source-owned home template
  if (installedPath.startsWith(RUNNER_HOME_PREFIX)) {
    return installedPath;
  }
  // collapse the installer-required ubuntu home alias
  if (installedPath.startsWith(RUNNER_LITERAL_HOME_PREFIX)) {
    return `${RUNNER_HOME_PREFIX}${installedPath.slice(RUNNER_LITERAL_HOME_PREFIX.length)}`;
  }
  return null;
}

// list checked Docker runtime image targets
function availableImageTargets(entries) {
  const targets = new Set();

  // inspect exact Docker image roots
  for (const entry of entries) {
    // retain only supported production image stages
    if (
      (entry.kind === "docker-command" || entry.kind === "docker-entrypoint") &&
      ["server", "web"].includes(entry.id.split("#").at(-1))
    ) {
      targets.add(entry.id.split("#").at(-1));
    }
  }
  return targets;
}

// map one copied package manifest to exact runtime images
function packageImageTargets(sourcePath, entries) {
  const available = availableImageTargets(entries);

  // keep isolated package fixtures deterministic
  if (available.size === 0) {
    return sourcePath === "package.json" ? ["server"] : [];
  }
  // bind the web package only to the web image
  if (sourcePath === "apps/web/package.json") {
    return available.has("web") ? ["web"] : [];
  }
  const serverPackages = new Set([
    "apps/api/package.json",
    "apps/worker/package.json",
    "packages/database/package.json",
    "packages/domain/package.json",
    "packages/forecast-adjustment/package.json",
    "packages/providers/package.json",
  ]);

  // bind the root package copied into both runtime images
  if (sourcePath === "package.json") {
    return [...available].sort();
  }
  // bind only package manifests copied into the server image
  if (serverPackages.has(sourcePath) && available.has("server")) {
    return ["server"];
  }
  return [];
}

// map one production Compose service to its image domain
function composeImageTarget(service) {
  // bind server-image application services
  if (["api", "migration", "worker"].includes(service)) {
    return "server";
  }
  // bind exact external and web image domains
  if (["web", "postgres", "cloudflared"].includes(service)) {
    return service;
  }
  return null;
}

// choose one literal runtime root kind
function runtimeRootKind(entry) {
  // map selected native service commands to their real Compose instances
  if (entry.kind === "runtime-generation-command") {
    return "compose-service";
  }
  // retain Compose service roots
  if (entry.kind === "compose-command") {
    return "compose-service";
  }
  // retain Docker default roots
  if (entry.kind === "docker-command" || entry.kind === "docker-entrypoint") {
    return "docker-image-default";
  }
  // retain package declaration roots
  if (entry.kind === "package-script") {
    return "package-script";
  }
  // retain installed unit and timer roots
  if (entry.kind === "systemd-service" || entry.kind === "systemd-timer") {
    return "host-systemd";
  }
  // retain the fixed runner installer and isolated runner fixtures
  if (
    /install-adjustment-maintenance-runner\.sh/u.test(entry.id) ||
    (entry.kind === "installer-allowlist" && entry.installedPaths?.some(
      // recognize unresolved runner-home installations only
      (path) => path.startsWith("${HOME}/"),
    ))
  ) {
    return "runner-package";
  }
  return "host-file";
}

// retain the baseline activation while declaring the selected physical owner per generation
function generationAwareContextDescriptors(entry, contexts) {
  // preserve ordinary predecessor contexts byte-for-byte
  if (!isGenerationSelectedAlternative(entry)) {
    return contexts;
  }
  return contexts.map((context) => {
    const selectedActivationSource = context.activationSource;
    const activationSourceByGeneration = {};

    // project every reviewed generation instead of inferring current installation state
    for (const generation of POLICY_GENERATIONS) {
      const reachable = entry.generations?.[generation]?.reachable ??
        ["rust-v1", "mixed"].includes(generation);
      activationSourceByGeneration[generation] = generation === "rust-v1" && reachable
        ? selectedActivationSource
        : "never-active-alternative";
    }
    return {
      ...context,
      activationSource: "never-active-alternative",
      activationSourceByGeneration,
    };
  });
}

// choose one literal host profile for a source root
function rootProfile(entry) {
  // keep runner application roots on the secondary host
  if (runtimeRootKind(entry) === "runner-package" || /weather-adjustment-/u.test(entry.id)) {
    return "workstation-runner";
  }
  // keep local backup units on the workstation
  if (/weather-backup-local/u.test(entry.id)) {
    return "workstation-runner";
  }
  // keep local source-only control scripts on the workstation
  if (/ssh-run\.sh/u.test(entry.id) || entry.sourcePath.startsWith("scripts/")) {
    return "workstation-runner";
  }
  return "blueberry-compose";
}

// derive exact context descriptors for one checked root
function rootContextDescriptors(entry, entries) {
  const rootKind = runtimeRootKind(entry);
  const contexts = [];

  // bind selected native commands to the same concrete production services they replace
  if (entry.kind === "runtime-generation-command") {
    const service = entry.id.split("#").at(-1);
    contexts.push({
      accountPolicy: null,
      activationSource: "container-inspect",
      hostRole: "blueberry",
      imageTarget: composeImageTarget(service),
      instanceKind: "service",
      instanceValue: service,
      profile: "blueberry-compose",
      rootKind,
      service,
      unit: null,
    });
    return generationAwareContextDescriptors(entry, contexts);
  }

  // derive one installed-alternative context per copied package manifest
  if (rootKind === "package-script") {
    for (const imageTarget of packageImageTargets(entry.sourcePath, entries)) {
      contexts.push({
        accountPolicy: null,
        activationSource: "never-active-alternative",
        hostRole: "blueberry",
        imageTarget,
        instanceKind: "image",
        instanceValue: imageTarget,
        profile: "blueberry-compose",
        rootKind,
        service: null,
        unit: null,
      });
    }
    return generationAwareContextDescriptors(entry, contexts);
  }
  // retain the local Compose override as an inactive source alternative
  if (rootKind === "compose-service" && entry.sourcePath !== "deploy/compose.yaml") {
    const service = entry.id.split("#").at(-1);
    contexts.push({
      accountPolicy: null,
      activationSource: "never-active-alternative",
      hostRole: "workstation",
      imageTarget: composeImageTarget(service),
      instanceKind: "service",
      instanceValue: service,
      profile: "workstation-runner",
      rootKind,
      service,
      unit: null,
    });
    return generationAwareContextDescriptors(entry, contexts);
  }
  // derive exact production Compose service identity
  if (rootKind === "compose-service") {
    const service = entry.id.split("#").at(-1);
    contexts.push({
      accountPolicy: null,
      activationSource: "container-inspect",
      hostRole: "blueberry",
      imageTarget: composeImageTarget(service),
      instanceKind: "service",
      instanceValue: service,
      profile: "blueberry-compose",
      rootKind,
      service,
      unit: null,
    });
    return generationAwareContextDescriptors(entry, contexts);
  }
  // derive supported Docker defaults as installed alternatives
  if (rootKind === "docker-image-default") {
    const imageTarget = entry.id.split("#").at(-1);

    // omit nonproduction fixture stages from live expectations
    if (!["server", "web"].includes(imageTarget)) {
      return generationAwareContextDescriptors(entry, contexts);
    }
    contexts.push({
      accountPolicy: null,
      activationSource: "never-active-alternative",
      hostRole: "blueberry",
      imageTarget,
      instanceKind: "image",
      instanceValue: imageTarget,
      profile: "blueberry-compose",
      rootKind,
      service: null,
      unit: null,
    });
    return generationAwareContextDescriptors(entry, contexts);
  }
  const profile = rootProfile(entry);
  const hostRole = profile === "blueberry-compose" ? "blueberry" : "workstation";

  // derive exact systemd unit identity
  if (rootKind === "host-systemd") {
    const unit = basename(entry.sourcePath);
    const accountPolicy = /weather-adjustment-/u.test(unit) ? runnerAccountPolicy() : null;
    contexts.push({
      accountPolicy,
      activationSource: "systemd-show",
      hostRole,
      imageTarget: null,
      instanceKind: accountPolicy === null ? "unit" : "account",
      instanceValue: accountPolicy === null ? unit : accountPolicy.accountName,
      profile,
      rootKind,
      service: null,
      unit,
    });
    return generationAwareContextDescriptors(entry, contexts);
  }
  // derive the conditional runner-package identity
  if (rootKind === "runner-package") {
    const accountPolicy = runnerAccountPolicy();
    contexts.push({
      accountPolicy,
      activationSource: "systemd-show",
      hostRole: "workstation",
      imageTarget: null,
      instanceKind: "account",
      instanceValue: accountPolicy.accountName,
      profile: "workstation-runner",
      rootKind,
      service: null,
      unit: null,
    });
    return generationAwareContextDescriptors(entry, contexts);
  }
  // omit source-only roots without declared installation authority
  if ((entry.installedPaths?.length ?? 0) === 0) {
    return generationAwareContextDescriptors(entry, contexts);
  }
  contexts.push({
    accountPolicy: null,
    activationSource: "process-identity",
    hostRole,
    imageTarget: null,
    instanceKind: "host",
    instanceValue: hostRole,
    profile,
    rootKind,
    service: null,
    unit: null,
  });
  return generationAwareContextDescriptors(entry, contexts);
}

// parse one checked Docker literal array
function dockerLiteralCommand(entry) {
  // accept only the scanner's canonical literal array
  try {
    const parsed = JSON.parse(entry.command ?? "null");

    // retain string arrays only
    if (Array.isArray(parsed) && parsed.every((value) => typeof value === "string")) {
      return parsed;
    }
  } catch {
    return null;
  }
  // retain fixed fixture defaults when structural command evidence is absent
  if (entry.id.endsWith("#server")) {
    return ["node", "apps/api/dist/main.js"];
  }
  // retain the reviewed web default
  if (entry.id.endsWith("#web")) {
    return ["node", "deploy/scripts/web-server.mjs"];
  }
  return null;
}

// choose one expected installed-file mode
function expectedArtifactMode(sourcePath, kind) {
  // preserve immutable owner-private runner files
  if (kind === "runner-file") {
    return "0400";
  }
  // preserve executable host controls
  if (
    kind === "host-file" &&
    (sourcePath.endsWith(".rs") || (
      sourcePath.endsWith(".sh") &&
      !sourcePath.endsWith("native-evidence-remote-dispatch.sh") &&
      !sourcePath.endsWith("native-release-control-remote-dispatch.sh") &&
      !sourcePath.endsWith("native-dashboard-release-control.sh")
    ))
  ) {
    return "0755";
  }
  return "0644";
}

// list exact image copies for one installed container path
function containerImageTargets(entry, path, entries) {
  const available = availableImageTargets(entries);
  const targets = [];
  const webSupport = new Set([
    "adjustment-evidence-store.mjs",
    "forecast-adjustment-scorecard-contract.mjs",
    "forecast-observations.mjs",
    "home-network.mjs",
    "weather-admin-store.mjs",
    "web-server.mjs",
    "xweather-tile-cache.mjs",
    "xweather-usage-budget.mjs",
  ]);

  // bind each installed native executable to its owning runtime image
  if (path.startsWith("/opt/weather/bin/weather-")) {
    const imageTarget = /^\/opt\/weather\/bin\/weather-web(?:$|-health$)/u.test(path)
      ? "web"
      : "server";

    // emit only image targets actually declared by the checked Dockerfile
    if (available.has(imageTarget)) {
      targets.push(imageTarget);
    }
    return targets;
  }

  // bind exact compiled server trees copied by Dockerfile
  if (
    path.startsWith("/opt/weather/apps/api/") ||
    path.startsWith("/opt/weather/apps/worker/") ||
    path.startsWith("/opt/weather/packages/database/") ||
    path.startsWith("/opt/weather/packages/domain/") ||
    path.startsWith("/opt/weather/packages/forecast-adjustment/") ||
    path.startsWith("/opt/weather/packages/providers/")
  ) {
    if (available.has("server")) {
      targets.push("server");
    }
    return targets;
  }
  // bind the exact compiled web tree
  if (path.startsWith("/opt/weather/apps/web/")) {
    if (available.has("web")) {
      targets.push("web");
    }
    return targets;
  }
  const deployScript = path.match(/^\/opt\/weather\/deploy\/scripts\/([^/]+\.mjs)$/u)?.[1];

  // bind the server wildcard and selected web support copies
  if (deployScript !== undefined) {
    if (available.has("server")) {
      targets.push("server");
    }
    // retain the explicit web copy list
    if (available.has("web") && webSupport.has(deployScript)) {
      targets.push("web");
    }
  }
  // retain small fixed fixtures with one declared server image
  if (targets.length === 0 && available.has("server") && (
    path.startsWith("/opt/weather/apps/api/") || path.startsWith("/opt/weather/apps/worker/")
  )) {
    targets.push("server");
  }
  return targets;
}

// map host-release emitted files into exact Docker copy trees
function emittedContainerPath(installedPath) {
  const match = installedPath.match(
    /^\/opt\/weather\/current\/(apps\/(?:api|worker|web)\/dist\/.+|packages\/(?:database|domain|forecast-adjustment|providers)\/dist\/.+)$/u,
  );

  // retain only paths covered by reviewed whole-dist Docker copies
  if (match === null) {
    return null;
  }
  return `/opt/weather/${match[1]}`;
}

// create one deterministic artifact ID
function runtimeArtifactId(kind, entryId, index, imageTarget = null) {
  // retain package image identity in its auditable ID
  if (kind === "package-metadata") {
    return `artifact:${kind}:${entryId}#${imageTarget}`;
  }
  return `artifact:${kind}:${entryId}#${String(index)}`;
}

// create exact source-owned artifacts for one entry
function sourceArtifactsForEntry(entry, entries) {
  const artifacts = [];
  let index = 0;

  // preserve copied package.json definitions without executable promotion
  if (entry.kind === "package-script" && entry.parents.length === 0) {
    for (const imageTarget of packageImageTargets(entry.sourcePath, entries)) {
      const scriptName = entry.id.split("#").at(-1);
      artifacts.push({
        authority: { kind: "dockerfile-copy", sourcePath: "Dockerfile" },
        executableClosure: false,
        id: runtimeArtifactId("package-metadata", entry.id, index, imageTarget),
        imageTarget,
        kind: "package-metadata",
        path: entry.sourcePath === "package.json"
          ? "/opt/weather/package.json"
          : `/opt/weather/${entry.sourcePath}`,
        scriptDefinitionOnly: true,
        scriptNames: [scriptName],
        sourceEntryId: entry.id,
      });
      index += 1;
    }
  }
  // preserve exact stage-local Docker defaults
  if (entry.kind === "docker-command" || entry.kind === "docker-entrypoint") {
    const imageTarget = entry.id.split("#").at(-1);

    // retain supported production image defaults only
    if (["server", "web"].includes(imageTarget)) {
      const command = dockerLiteralCommand(entry);
      artifacts.push({
        authority: { kind: "dockerfile-stage", sourcePath: "Dockerfile" },
        declaredCmd: entry.kind === "docker-command" ? command : null,
        declaredEntrypoint: entry.kind === "docker-entrypoint" ? command : null,
        id: runtimeArtifactId("image-config", entry.id, index),
        imageTarget,
        kind: "image-config",
        sourceEntryId: entry.id,
      });
      index += 1;
    }
  }

  // map every declared installed path through a fixed artifact domain
  for (const installedPath of entry.installedPaths ?? []) {
    // image pseudo-paths are represented by the Docker default above
    if (installedPath.startsWith("image://")) {
      continue;
    }
    const declaredRunnerPath = runnerInstalledPath(installedPath);

    // preserve unresolved runner-home paths
    if (declaredRunnerPath !== null) {
      artifacts.push({
        accountPolicy: runnerAccountPolicy(),
        authority: {
          kind: "runner-installer",
          sourcePath: "scripts/install-adjustment-maintenance-runner.sh",
        },
        declaredPath: declaredRunnerPath,
        // the native runner installs executable bytes separately from immutable source files
        expectedMode: entry.kind === "native-binary" ? "0500" : expectedArtifactMode(entry.sourcePath, "runner-file"),
        expectedType: "regular-file",
        id: runtimeArtifactId("runner-file", entry.id, index),
        kind: "runner-file",
        ownerPolicy: "nss-account",
        resolvedRootPolicy: "nss-account-home",
        sourceEntryId: entry.id,
      });
      index += 1;
      continue;
    }
    // preserve exact installed unit fragments
    if (installedPath.startsWith("/etc/systemd/system/")) {
      artifacts.push({
        authority: {
          kind: "systemd-installer",
          sourcePath: /weather-adjustment-/u.test(installedPath)
            ? "scripts/install-adjustment-maintenance-runner.sh"
            : "scripts/install-schema-preserving-release.sh",
        },
        expectedMode: "0644",
        id: runtimeArtifactId("systemd-unit", entry.id, index),
        kind: "systemd-unit",
        ownerPolicy: "root",
        path: installedPath,
        sourceEntryId: entry.id,
        unit: basename(installedPath),
      });
      index += 1;
      continue;
    }
    const emittedPath = emittedContainerPath(installedPath);
    const containerPath = emittedPath ?? installedPath;
    const imageTargets = containerPath.startsWith("/opt/weather/") &&
      !containerPath.startsWith("/opt/weather/current/")
      ? containerImageTargets(entry, containerPath, entries)
      : [];

    // preserve one artifact for every exact copied image domain
    for (const imageTarget of imageTargets) {
      artifacts.push({
        authority: { kind: "dockerfile-copy", sourcePath: "Dockerfile" },
        contentRole: "runtime-bytes",
        expectedMode: expectedArtifactMode(entry.sourcePath, "container-file"),
        expectedType: "regular-file",
        id: runtimeArtifactId("container-file", entry.id, index),
        imageTarget,
        kind: "container-file",
        ownerPolicy: "runtime-user",
        path: containerPath,
        sourceEntryId: entry.id,
      });
      index += 1;
    }
    // keep copied container paths out of the host-file domain
    if (imageTargets.length > 0 && emittedPath === null) {
      continue;
    }
    artifacts.push({
      authority: { kind: "inventory-installed-path", sourcePath: entry.sourcePath },
      contentRole: "operator-control",
      expectedMode: expectedArtifactMode(entry.sourcePath, "host-file"),
      expectedType: "regular-file",
      id: runtimeArtifactId("host-file", entry.id, index),
      kind: "host-file",
      ownerPolicy: "root",
      path: installedPath,
      sourceEntryId: entry.id,
    });
    index += 1;
  }
  const uniqueArtifacts = new Map();

  // collapse duplicate installed paths merged from several parents
  for (const artifact of artifacts) {
    const semanticIdentity = artifactSemanticIdentity(artifact);

    // retain the first deterministic artifact identity
    if (!uniqueArtifacts.has(semanticIdentity)) {
      uniqueArtifacts.set(semanticIdentity, artifact);
    }
  }
  return [...uniqueArtifacts.values()];
}

// close one source root over all typed parent edges
function runtimeClosureEntries(entries, rootEntryId) {
  const closure = new Set([rootEntryId]);
  let changed = true;

  // continue until every descendant is retained
  while (changed) {
    changed = false;

    // inspect every potential child row
    for (const entry of entries) {
      // skip established descendants
      if (closure.has(entry.id)) {
        continue;
      }
      // retain children of any established parent
      if (entry.parents.some((parent) => closure.has(parent.id))) {
        closure.add(entry.id);
        changed = true;
      }
    }
  }
  return entries.filter(
    // preserve checked inventory order before stable output sorting
    (entry) => closure.has(entry.id),
  );
}

// decide whether one artifact belongs to one context instance
function artifactMatchesContext(artifact, context, entry, rootEntryId) {
  // bind package definitions only to their exact package root image
  if (artifact.kind === "package-metadata") {
    return entry.id === rootEntryId && context.rootKind === "package-script" &&
      artifact.imageTarget === context.imageTarget;
  }
  // bind Docker defaults only to their exact Docker root image
  if (artifact.kind === "image-config") {
    return entry.id === rootEntryId && context.rootKind === "docker-image-default" &&
      artifact.imageTarget === context.imageTarget;
  }
  // bind container bytes only to matching image-backed contexts
  if (artifact.kind === "container-file") {
    return artifact.imageTarget === context.imageTarget;
  }
  // bind runner snapshots only to account-resolved contexts
  if (artifact.kind === "runner-file") {
    return context.accountPolicy !== null;
  }
  // bind unit fragments only to their own systemd root
  if (artifact.kind === "systemd-unit") {
    return entry.id === rootEntryId && context.rootKind === "host-systemd";
  }
  // bind host control files to host contexts and exact Compose declarations
  if (artifact.kind === "host-file") {
    return context.imageTarget === null || (
      context.rootKind === "compose-service" &&
      (entry.id === rootEntryId || entry.kind === "compose-probe")
    );
  }
  return false;
}

// add Docker COPY closure entries to an image-default context
function contextEntries(entries, root, context, artifactsByEntry) {
  const selected = new Map(runtimeClosureEntries(entries, root.id).map(
    // retain exact entry identities
    (entry) => [entry.id, entry],
  ));

  // expand checked production Docker roots to every copied artifact
  if (context.rootKind === "docker-image-default" && typeof root.sourceDigest === "string") {
    for (const entry of entries) {
      const copied = (artifactsByEntry.get(entry.id) ?? []).some(
        // select exact copied bytes in this image
        (artifact) => artifact.kind === "container-file" && artifact.imageTarget === context.imageTarget,
      );

      // retain every copied image artifact source
      if (copied) {
        selected.set(entry.id, entry);
      }
    }
  }
  return [...selected.values()].sort(
    // retain deterministic entry order
    (left, right) => left.id.localeCompare(right.id),
  );
}

// identify source-checked alternatives that cannot be simultaneously installed or active
function isGenerationSelectedAlternative(entry) {
  return entry.kind === "runtime-generation-command" ||
    entry.kind === "runtime-generation-probe" ||
    entry.kind === "native-binary" ||
    entry.sourcePath.startsWith("scripts/systemd/native/") ||
    entry.id.includes("#ADJUSTMENT_RUNNER_NATIVE_") ||
    ["native-server", "native-web"].includes(entry.id.split("#").at(-1));
}

// derive the literal current-repository runtime context map
export function deriveRuntimeLiveMap({ entries }) {
  const allArtifactsByEntry = new Map();

  // derive source-owned artifact candidates once
  for (const entry of entries) {
    allArtifactsByEntry.set(entry.id, sourceArtifactsForEntry(entry, entries));
  }
  const artifactsById = new Map();
  const bindings = [];
  const contexts = [];
  const roots = entries.filter(
    // retain ordinary roots plus selected native service commands with explicit physical identity
    (entry) => (Array.isArray(entry.parents) && entry.parents.length === 0) ||
      entry.kind === "runtime-generation-command",
  );

  // derive every exact root/profile/instance mapping
  for (const root of roots) {
    for (const descriptor of rootContextDescriptors(root, entries)) {
      const contextId = runtimeContextId(
        descriptor.profile,
        root.id,
        descriptor.instanceKind,
        descriptor.instanceValue,
      );
      const contextArtifactIds = new Set();
      const context = {
        accountPolicy: descriptor.accountPolicy,
        accountingScopes: [root.accountingScope],
        activationSource: descriptor.activationSource,
        artifactIds: [],
        hostRole: descriptor.hostRole,
        id: contextId,
        imageTarget: descriptor.imageTarget,
        profile: descriptor.profile,
        rootEntryId: root.id,
        rootKind: descriptor.rootKind,
        service: descriptor.service,
        unit: descriptor.unit,
      };

      // retain the closed generation selector only for mutually exclusive native owners
      if (descriptor.activationSourceByGeneration !== undefined) {
        context.activationSourceByGeneration = descriptor.activationSourceByGeneration;
      }

      // bind every transitive or copied artifact to this exact root instance
      for (const entry of contextEntries(entries, root, context, allArtifactsByEntry)) {
        const bindingArtifactIds = [];

        // retain every artifact installed in this exact context
        for (const artifact of allArtifactsByEntry.get(entry.id) ?? []) {
          // skip artifacts outside this instance domain
          if (!artifactMatchesContext(artifact, context, entry, root.id)) {
            continue;
          }
          artifactsById.set(artifact.id, artifact);
          contextArtifactIds.add(artifact.id);
          bindingArtifactIds.push(artifact.id);
        }
        bindingArtifactIds.sort();
        bindings.push({
          artifactIds: bindingArtifactIds,
          contextId,
          entryId: entry.id,
          id: runtimeBindingId(contextId, entry.id),
          rootEntryId: root.id,
        });
      }
      context.artifactIds = [...contextArtifactIds].sort();
      contexts.push(context);
    }
  }
  const artifacts = [...artifactsById.values()].sort(
    // keep artifact identities deterministic
    (left, right) => left.id.localeCompare(right.id),
  );
  bindings.sort(
    // keep binding identities deterministic
    (left, right) => left.id.localeCompare(right.id),
  );
  contexts.sort(
    // keep context identities deterministic
    (left, right) => left.id.localeCompare(right.id),
  );
  return {
    artifacts,
    bindings,
    contexts,
    contractVersion: RUNTIME_LIVE_MAP_SCHEMA_VERSION,
    inventorySchemaVersion: SCHEMA_VERSION,
  };
}

// reject fields outside one closed runtime-map record
function validateLiveMapKeys(value, allowed, path, diagnostics) {
  // let the owning schema reject non-records
  if (!isPlainObject(value)) {
    return;
  }
  // inspect every declared field
  for (const key of Object.keys(value)) {
    // reject hidden runtime assertions
    if (!allowed.has(key)) {
      diagnostics.push(diagnostic("live-map.schema", `unsupported field ${key}`, { path: `${path}.${key}` }));
    }
  }
}

// identify duplicate semantic artifacts independently of IDs
function artifactSemanticIdentity(artifact) {
  const copy = { ...artifact };
  delete copy.id;
  return canonicalJson(copy);
}

// validate one closed artifact record shape
function validateRuntimeMapArtifactShape(artifact, path, diagnostics) {
  const common = new Set(["authority", "id", "kind", "sourceEntryId"]);
  const variants = {
    "container-file": ["contentRole", "expectedMode", "expectedType", "imageTarget", "ownerPolicy", "path"],
    "host-file": ["contentRole", "expectedMode", "expectedType", "ownerPolicy", "path"],
    "image-config": ["declaredCmd", "declaredEntrypoint", "imageTarget"],
    "package-metadata": ["executableClosure", "imageTarget", "path", "scriptDefinitionOnly", "scriptNames"],
    "runner-file": ["accountPolicy", "declaredPath", "expectedMode", "expectedType", "ownerPolicy", "resolvedRootPolicy"],
    "systemd-unit": ["expectedMode", "ownerPolicy", "path", "unit"],
  };
  const fields = variants[artifact?.kind];

  // require one supported artifact record
  if (!isPlainObject(artifact) || typeof artifact.id !== "string" || !LIVE_MAP_ARTIFACT_KINDS.has(artifact.kind) || fields === undefined) {
    diagnostics.push(diagnostic("live-map.artifact", "artifact record is invalid", { path }));
    return;
  }
  validateLiveMapKeys(artifact, new Set([...common, ...fields]), path, diagnostics);
  validateLiveMapKeys(artifact.authority, new Set(["kind", "sourcePath"]), `${path}.authority`, diagnostics);
}

// compare one exact source-derived record collection
function compareRuntimeRecords(actual, expected, kind, diagnostics) {
  const actualById = new Map();
  const expectedById = new Map(expected.map(
    // index expected identities
    (record) => [record.id, record],
  ));

  // reject duplicate identities
  for (let index = 0; index < actual.length; index += 1) {
    const record = actual[index];
    const path = `$.${kind}[${String(index)}]`;

    // reject malformed or duplicate IDs
    if (!isPlainObject(record) || typeof record.id !== "string") {
      diagnostics.push(diagnostic("live-map.schema", `${kind} record identity is invalid`, { path }));
      continue;
    }
    // reject duplicate exact identities
    if (actualById.has(record.id)) {
      diagnostics.push(diagnostic("live-map.duplicate", `duplicate ${kind} ${record.id}`, { path }));
    }
    actualById.set(record.id, record);
  }

  // require every exact derived record
  for (const [id, expectedRecord] of expectedById) {
    const actualRecord = actualById.get(id);

    // reject omitted derived records
    if (actualRecord === undefined) {
      diagnostics.push(diagnostic("live-map.coverage", `missing ${kind} ${id}`));
      continue;
    }
    // reject changed source-to-instance mappings
    if (canonicalJson(actualRecord) !== canonicalJson(expectedRecord)) {
      const code = kind === "artifacts" && actualRecord.imageTarget !== expectedRecord.imageTarget
        ? "live-map.mapping"
        : kind === "artifacts"
          ? "live-map.artifact"
          : "live-map.mapping";
      diagnostics.push(diagnostic(code, `${kind} ${id} differs from source-owned semantics`));
    }
  }

  // reject every extra record
  for (const id of actualById.keys()) {
    // retain expected identities only
    if (!expectedById.has(id)) {
      diagnostics.push(diagnostic("live-map.coverage", `extra ${kind} ${id}`));
    }
  }
}

// require the finite reviewed deployment recipes for the production inventory
function validateSupportedRuntimeSources(entries, diagnostics) {
  const productionInventory = entries.some(
    // recognize the checked repository without weakening small fixtures
    (entry) => entry.id === "package-script:package.json#backend-performance:evidence",
  );

  // keep isolated fixtures independent of repository source hashes
  if (!productionInventory) {
    return;
  }
  const productionServices = entries.filter(
    // select production Compose command roots
    (entry) => entry.kind === "compose-command" && entry.sourcePath === "deploy/compose.yaml",
  ).map(
    // retain exact service names
    (entry) => entry.id.split("#").at(-1),
  ).sort();
  const dockerTargets = entries.filter(
    // select checked Docker default commands
    (entry) => entry.kind === "docker-command" && entry.sourcePath === "Dockerfile",
  ).map(
    // retain exact image stages
    (entry) => entry.id.split("#").at(-1),
  ).sort();
  const composeDigests = new Set(entries.filter(
    // retain every production Compose observation
    (entry) => entry.sourcePath === "deploy/compose.yaml",
  ).map(
    // retain source-owned byte identities
    (entry) => entry.sourceDigest,
  ));
  const dockerDigests = new Set(entries.filter(
    // retain every Dockerfile observation
    (entry) => entry.sourcePath === "Dockerfile",
  ).map(
    // retain source-owned byte identities
    (entry) => entry.sourceDigest,
  ));
  const runnerInstaller = entries.find(
    // select the fixed runner installer root
    (entry) => entry.id === "private-command:scripts/install-adjustment-maintenance-runner.sh",
  );

  // reject deployment-recipe drift until the finite map is reviewed again
  if (
    canonicalJson(productionServices) !== canonicalJson(["api", "cloudflared", "migration", "postgres", "web", "worker"]) ||
    canonicalJson(dockerTargets) !== canonicalJson([
      "native-server",
      "native-web",
      "server",
      "web",
    ]) ||
    canonicalJson([...composeDigests]) !== canonicalJson([SUPPORTED_COMPOSE_DIGEST]) ||
    canonicalJson([...dockerDigests]) !== canonicalJson([SUPPORTED_DOCKERFILE_DIGEST]) ||
    runnerInstaller?.sourceDigest !== SUPPORTED_RUNNER_INSTALLER_DIGEST
  ) {
    diagnostics.push(diagnostic(
      "live-map.mapping",
      "deployment recipe bytes or finite root set are unsupported",
    ));
  }
}

// validate one fixed source-owned runtime context map
export function validateRuntimeLiveMap({ inventory, runtimeMap }) {
  const diagnostics = [];
  const rootKeys = new Set(["artifacts", "bindings", "contexts", "contractVersion", "inventorySchemaVersion"]);
  validateLiveMapKeys(runtimeMap, rootKeys, "$", diagnostics);

  // require one exact versioned map and checked inventory
  if (
    !isPlainObject(runtimeMap) ||
    runtimeMap.contractVersion !== RUNTIME_LIVE_MAP_SCHEMA_VERSION ||
    runtimeMap.inventorySchemaVersion !== inventory?.schemaVersion ||
    !Array.isArray(runtimeMap.artifacts) ||
    !Array.isArray(runtimeMap.bindings) ||
    !Array.isArray(runtimeMap.contexts) ||
    !Array.isArray(inventory?.entries)
  ) {
    diagnostics.push(diagnostic("live-map.schema", "runtime live map root is invalid"));
    return { diagnostics: sortDiagnostics(diagnostics), ok: false };
  }
  validateSupportedRuntimeSources(inventory.entries, diagnostics);

  // validate every artifact shape and semantic uniqueness
  const semanticArtifacts = new Set();
  for (let index = 0; index < runtimeMap.artifacts.length; index += 1) {
    const artifact = runtimeMap.artifacts[index];
    const path = `$.artifacts[${String(index)}]`;
    validateRuntimeMapArtifactShape(artifact, path, diagnostics);

    // reject duplicate semantic artifacts under different IDs
    if (isPlainObject(artifact)) {
      const semanticIdentity = artifactSemanticIdentity(artifact);

      // reject repeated semantics independently of identity
      if (semanticArtifacts.has(semanticIdentity)) {
        diagnostics.push(diagnostic("live-map.duplicate", "duplicate semantic artifact", { path }));
      }
      semanticArtifacts.add(semanticIdentity);
    }
  }
  const expected = deriveRuntimeLiveMap({ entries: inventory.entries });
  compareRuntimeRecords(runtimeMap.artifacts, expected.artifacts, "artifacts", diagnostics);
  compareRuntimeRecords(runtimeMap.contexts, expected.contexts, "contexts", diagnostics);
  compareRuntimeRecords(runtimeMap.bindings, expected.bindings, "bindings", diagnostics);
  const bindingTuples = new Set();
  const boundArtifacts = new Set();

  // validate one unique entry root and instance tuple per binding
  for (let index = 0; index < runtimeMap.bindings.length; index += 1) {
    const binding = runtimeMap.bindings[index];
    const path = `$.bindings[${String(index)}]`;
    validateLiveMapKeys(
      binding,
      new Set(["artifactIds", "contextId", "entryId", "id", "rootEntryId"]),
      path,
      diagnostics,
    );
    const tuple = canonicalJson([
      binding?.entryId,
      binding?.rootEntryId,
      binding?.contextId,
    ]);

    // reject duplicate tuple identities independently of record IDs
    if (bindingTuples.has(tuple)) {
      diagnostics.push(diagnostic("live-map.duplicate", "duplicate entry root context binding", { path }));
    }
    bindingTuples.add(tuple);

    // reject unordered duplicate or malformed artifact references
    if (
      !Array.isArray(binding?.artifactIds) ||
      binding.artifactIds.some((artifactId) => typeof artifactId !== "string") ||
      canonicalJson(binding.artifactIds) !== canonicalJson([...new Set(binding.artifactIds)].sort())
    ) {
      diagnostics.push(diagnostic("live-map.schema", "binding artifact references are invalid", { path }));
      continue;
    }

    // retain every exact bound artifact identity
    for (const artifactId of binding.artifactIds) {
      boundArtifacts.add(artifactId);
    }
  }

  // reject every orphan artifact
  for (const artifact of runtimeMap.artifacts) {
    // require one exact instance binding
    if (!boundArtifacts.has(artifact?.id)) {
      diagnostics.push(diagnostic("live-map.coverage", `artifact ${String(artifact?.id)} has no runtime binding`));
    }
  }
  const sortedDiagnostics = sortDiagnostics(diagnostics);
  return { diagnostics: sortedDiagnostics, ok: sortedDiagnostics.length === 0 };
}

// validate the checked manifest schema
function validateManifestSchema(manifest) {
  const diagnostics = [];

  // require a versioned root object
  if (!isPlainObject(manifest)) {
    return [diagnostic("manifest.schema", "manifest must be an object")];
  }

  // bind exact schema and scanner versions
  if (manifest.schemaVersion !== SCHEMA_VERSION || manifest.scannerVersion !== SCANNER_VERSION) {
    diagnostics.push(diagnostic("manifest.schema", "manifest version is unsupported"));
  }

  // require bounded-parser disclosures
  if (
    !Array.isArray(manifest.limitations) ||
    manifest.limitations.length === 0 ||
    manifest.limitations.some((value) => typeof value !== "string" || value.length === 0)
  ) {
    diagnostics.push(diagnostic("manifest.schema", "limitations must be nonempty strings"));
  }

  // require explicit generation policies
  if (!isPlainObject(manifest.generationPolicies)) {
    diagnostics.push(diagnostic("manifest.schema", "generationPolicies must be an object"));
  }

  // require checked entries
  if (!Array.isArray(manifest.entries)) {
    diagnostics.push(diagnostic("manifest.schema", "entries must be an array"));
    return diagnostics;
  }
  const ids = new Set();

  // validate every checked row
  for (const entry of manifest.entries) {
    // require one stable identity
    if (!isPlainObject(entry) || typeof entry.id !== "string" || entry.id.length === 0) {
      diagnostics.push(diagnostic("manifest.schema", "entry id is missing"));
      continue;
    }

    // reject duplicate identities
    if (ids.has(entry.id)) {
      diagnostics.push(diagnostic("manifest.duplicate-id", `duplicate entry id ${entry.id}`, {
        entryId: entry.id,
      }));
      continue;
    }
    ids.add(entry.id);

    // require bounded enum values
    if (
      !ENTRY_KINDS.has(entry.kind) ||
      !CLASSIFICATIONS.has(entry.classification) ||
      !RUNTIMES.has(entry.runtime)
    ) {
      diagnostics.push(diagnostic("manifest.schema", "entry enum is invalid", {
        entryId: entry.id,
      }));
    }

    // require exact source and parent identity fields
    if (
      typeof entry.sourcePath !== "string" ||
      !/^[a-f0-9]{64}$/u.test(entry.sourceDigest ?? "") ||
      !/^[a-f0-9]{64}$/u.test(entry.entryDigest ?? "") ||
      !Array.isArray(entry.installedPaths) ||
      !Array.isArray(entry.parents) ||
      !Array.isArray(entry.invokedRuntimes) ||
      entry.invokedRuntimes.some((runtime) => !["node", "python", "rust", "other"].includes(runtime))
    ) {
      diagnostics.push(diagnostic("manifest.schema", "entry source identity is invalid", {
        entryId: entry.id,
      }));
    }

    // reject unsafe checked source paths
    try {
      resolveSourcePath("/inventory-root", entry.sourcePath);
    } catch (error) {
      diagnostics.push(diagnostic("path.traversal", String(error.message), {
        entryId: entry.id,
        path: entry.sourcePath,
      }));
    }

    // require ownership, parity and accounting declarations
    if (
      typeof entry.owner !== "string" || entry.owner.length === 0 ||
      typeof entry.parityTest !== "string" || entry.parityTest.length === 0 ||
      typeof entry.accountingScope !== "string" || entry.accountingScope.length === 0
    ) {
      diagnostics.push(diagnostic("manifest.schema", "entry policy ownership is incomplete", {
        entryId: entry.id,
      }));
    }

    // validate typed parent edges
    for (const parent of entry.parents ?? []) {
      // reject incomplete parent identity
      if (
        !isPlainObject(parent) ||
        !["invocation", "import"].includes(parent.type) ||
        typeof parent.id !== "string" ||
        typeof parent.path !== "string"
      ) {
        diagnostics.push(diagnostic("manifest.schema", "entry parent is invalid", {
          entryId: entry.id,
        }));
      }
    }

    // require all declared generations
    if (!isPlainObject(entry.generations)) {
      diagnostics.push(diagnostic("manifest.schema", "entry generations are missing", {
        entryId: entry.id,
      }));
      continue;
    }

    // validate every generation declaration
    for (const generation of POLICY_GENERATIONS) {
      const declaration = entry.generations[generation];

      // require explicit reachability and replacement context
      if (
        !isPlainObject(declaration) ||
        typeof declaration.reachable !== "boolean" ||
        !(typeof declaration.command === "string" || declaration.command === null) ||
        !(typeof declaration.probe === "string" || declaration.probe === null) ||
        !(typeof declaration.replacement === "string" || declaration.replacement === null)
      ) {
        diagnostics.push(diagnostic("manifest.schema", `invalid ${generation} declaration`, {
          entryId: entry.id,
        }));
      }
    }
  }
  return diagnostics;
}

// close application reachability over parents
function applicationReachableEntryIds(entries) {
  const knownIds = new Set();
  const reachable = new Set();

  // index rows and seed applications
  for (const entry of entries) {
    knownIds.add(entry.id);

    // seed application rows
    if (entry.classification === "application") {
      reachable.add(entry.id);
    }
  }
  let changed = true;

  // close over typed parent edges
  while (changed) {
    changed = false;

    // inspect each unreached child
    for (const entry of entries) {
      // skip established reachability
      if (reachable.has(entry.id)) {
        continue;
      }

      // inherit any application parent
      if (entry.parents.some((parent) => knownIds.has(parent.id) && reachable.has(parent.id))) {
        reachable.add(entry.id);
        changed = true;
      }
    }
  }
  return reachable;
}

// validate classification and generation policy
function validatePolicy(manifest, requestedGeneration, phase) {
  const diagnostics = [];
  const effectiveGeneration = requestedGeneration === "node-v1" && phase === "rollback-drill"
    ? "node-v1-rollback"
    : requestedGeneration;
  const entries = manifest.entries;
  const policy = manifest.generationPolicies?.[effectiveGeneration];

  // require one finite exact policy
  if (
    !isPlainObject(policy) ||
    !Array.isArray(policy.reachableEntryIds) ||
    policy.reachableEntryIds.some((id) => typeof id !== "string")
  ) {
    diagnostics.push(diagnostic(
      "generation.policy",
      `missing ${effectiveGeneration} reachability policy`,
    ));
    return diagnostics;
  }
  const rowReachable = entries
    .filter((entry) => entry.generations[effectiveGeneration]?.reachable === true)
    .map((entry) => entry.id)
    .sort();
  const policyReachable = [...policy.reachableEntryIds].sort();

  // bind policy and rows exactly
  if (canonicalJson(rowReachable) !== canonicalJson(policyReachable)) {
    diagnostics.push(diagnostic(
      "generation.policy",
      `${effectiveGeneration} reachability differs from rows`,
    ));
  }
  const entryById = new Map();

  // index typed generation edges
  for (const entry of entries) {
    entryById.set(entry.id, entry);
  }
  const effectiveReachable = new Set(policyReachable);
  let closureChanged = true;

  // close application execution and import reachability
  while (closureChanged) {
    closureChanged = false;

    // inspect each application child
    for (const entry of entries) {
      // skip non-application and established rows
      if (entry.classification !== "application" || effectiveReachable.has(entry.id)) {
        continue;
      }
      const reachableApplicationParent = entry.parents.some((parent) => {
        const parentEntry = entryById.get(parent.id);
        return parentEntry?.classification === "application" && effectiveReachable.has(parent.id);
      });

      // require every reachable application edge to remain reachable
      if (reachableApplicationParent) {
        effectiveReachable.add(entry.id);
        closureChanged = true;
        diagnostics.push(diagnostic(
          "generation.policy",
          `${effectiveGeneration} omits a child of a reachable application parent`,
          { entryId: entry.id },
        ));
      }
    }
  }
  const applicationReachable = applicationReachableEntryIds(entries);

  // enforce classification boundaries
  for (const entry of entries) {
    // reject offline or control claims below applications
    if (applicationReachable.has(entry.id) && entry.classification !== "application") {
      diagnostics.push(diagnostic(
        "entry.classification",
        `application-reachable row is classified ${entry.classification}`,
        { entryId: entry.id, path: entry.sourcePath },
      ));
    }

    // reserve oracle for non-installed build scope
    if (
      entry.classification === "oracle" &&
      (entry.installedPaths.length > 0 || entry.accountingScope !== "build-only")
    ) {
      diagnostics.push(diagnostic(
        "entry.classification",
        "oracle row has production reachability",
        { entryId: entry.id, path: entry.sourcePath },
      ));
    }
  }

  // require the exact predecessor interpreter application closure
  if (effectiveGeneration === "node-v1" || effectiveGeneration === "node-v1-rollback") {
    // inspect every node application row
    for (const entry of entries) {
      // retain both Node and Python application owners in the predecessor
      if (
        entry.classification === "application" &&
        (["node", "python"].includes(entry.runtime) || entry.invokedRuntimes.some(runtime => ["node", "python"].includes(runtime))) &&
        entry.generations[effectiveGeneration].reachable !== true
      ) {
        diagnostics.push(diagnostic(
          "generation.node-application",
          "predecessor interpreter application row is not reachable",
          { entryId: entry.id },
        ));
      }
    }
  }

  // require a real rust closure with no node applications
  if (effectiveGeneration === "rust-v1") {
    let rustApplications = 0;

    // inspect candidate application rows
    for (const entry of entries) {
      // count discovered rust applications
      if (
        entry.classification === "application" &&
        effectiveReachable.has(entry.id) &&
        (entry.runtime === "rust" || entry.invokedRuntimes.includes("rust"))
      ) {
        rustApplications += 1;
      }

      // reject retained node applications
      if (
        entry.classification === "application" &&
        effectiveReachable.has(entry.id) &&
        (entry.runtime === "node" || entry.invokedRuntimes.includes("node"))
      ) {
        diagnostics.push(diagnostic(
          "generation.rust-node-application",
          "rust-v1 reaches a node application row",
          { entryId: entry.id },
        ));
      }
      // reject a retained Python application just as any other interpreter owner
      if (entry.classification === "application" && effectiveReachable.has(entry.id) &&
          (entry.runtime === "python" || entry.invokedRuntimes.includes("python"))) {
        diagnostics.push(diagnostic("generation.rust-python-application", "rust-v1 reaches a python application row", { entryId: entry.id }));
      }
    }

    // future command names are not discovered binaries
    if (rustApplications === 0) {
      diagnostics.push(diagnostic(
        "generation.rust-empty",
        "rust-v1 has no discovered rust application row",
      ));
    }
  }

  // require the finite mixed union
  if (effectiveGeneration === "mixed") {
    const allowedUnion = Array.isArray(policy.allowedUnionEntryIds)
      ? [...policy.allowedUnionEntryIds].sort()
      : null;

    // reject a missing or open union
    if (allowedUnion === null || canonicalJson(allowedUnion) !== canonicalJson(policyReachable)) {
      diagnostics.push(diagnostic(
        "generation.mixed-union",
        "mixed allowed union is not exact",
      ));
    }
  }
  return diagnostics;
}

// compare checked and discovered closure
function compareEntries(manifestEntries, discoveredEntries) {
  const diagnostics = [];
  const expected = new Map();
  const discovered = new Map();

  // index checked rows
  for (const entry of manifestEntries) {
    expected.set(entry.id, entry);
  }

  // index discovered rows
  for (const entry of discoveredEntries) {
    discovered.set(entry.id, entry);
  }

  // require every checked row
  for (const [entryId, expectedEntry] of expected) {
    const discoveredEntry = discovered.get(entryId);

    // reject removed rows
    if (discoveredEntry === undefined) {
      diagnostics.push(diagnostic("entry.missing", "checked entry is not discovered", {
        entryId,
        path: expectedEntry.sourcePath,
      }));
      continue;
    }

    // reject manifest-authored classification spoofing explicitly
    if (expectedEntry.classification !== discoveredEntry.classification) {
      diagnostics.push(diagnostic(
        "entry.classification",
        "checked classification differs from source-owned discovery",
        { entryId, path: expectedEntry.sourcePath },
      ));
    }
    const expectedParents = new Set(expectedEntry.parents.map((parent) => canonicalJson(parent)));

    // report newly discovered import or invocation edges explicitly
    if (discoveredEntry.parents.some((parent) => !expectedParents.has(canonicalJson(parent)))) {
      diagnostics.push(diagnostic(
        "entry.extra",
        "discovered entry has an unchecked parent edge",
        { entryId, path: expectedEntry.sourcePath },
      ));
    }
    const expectedStructural = {
      ...structuralEntry(expectedEntry),
      entryDigest: expectedEntry.entryDigest,
    };
    const discoveredStructural = {
      ...structuralEntry(discoveredEntry),
      entryDigest: discoveredEntry.entryDigest,
    };

    // reject digest, parent or command drift
    if (canonicalJson(expectedStructural) !== canonicalJson(discoveredStructural)) {
      diagnostics.push(diagnostic("entry.changed", "checked entry differs from discovery", {
        entryId,
        path: expectedEntry.sourcePath,
      }));
    }
  }

  // reject every unknown discovered row
  for (const [entryId, discoveredEntry] of discovered) {
    // retain only extra rows
    if (!expected.has(entryId)) {
      diagnostics.push(diagnostic("entry.extra", "discovered entry is not checked", {
        entryId,
        path: discoveredEntry.sourcePath,
      }));
    }
  }
  return diagnostics;
}

// parse bounded command arguments
export function parseRuntimeInventoryArguments(argumentsList) {
  const options = { generation: null, phase: null };

  // consume flag-value pairs
  for (let index = 0; index < argumentsList.length; index += 2) {
    const flag = argumentsList[index];
    const value = argumentsList[index + 1];

    // reject unknown or valueless flags
    if (value === undefined || !["--generation", "--phase"].includes(flag)) {
      throw new Error(
        "usage: check-backend-runtime-inventory.mjs --generation node-v1|rust-v1|mixed --phase baseline|pre-cutover|post-cutover|rollback-drill",
      );
    }
    const key = flag === "--generation" ? "generation" : "phase";

    // reject repeated flags
    if (options[key] !== null) {
      throw new Error(`${flag} may be provided only once`);
    }
    options[key] = value;
  }

  // require supported values
  if (!GENERATIONS.includes(options.generation) || !PHASES.includes(options.phase)) {
    throw new Error(
      "usage: check-backend-runtime-inventory.mjs --generation node-v1|rust-v1|mixed --phase baseline|pre-cutover|post-cutover|rollback-drill",
    );
  }
  return options;
}

// validate one checked inventory without global mutation
export async function validateRuntimeInventory({
  rootDir,
  manifestPath = DEFAULT_MANIFEST_PATH,
  runtimeMapPath = DEFAULT_RUNTIME_MAP_PATH,
  generation,
  phase,
}) {
  const diagnostics = [];

  // reject unsafe api options
  if (!GENERATIONS.includes(generation) || !PHASES.includes(phase)) {
    return {
      ok: false,
      generation,
      phase,
      manifestDigest: null,
      runtimeMapDigest: null,
      runtimeMapArtifactCount: 0,
      runtimeMapBindingCount: 0,
      runtimeMapContextCount: 0,
      diagnostics: [diagnostic("argument.invalid", "generation or phase is invalid")],
      discovered: [],
    };
  }
  const absoluteRoot = resolve(rootDir);
  let manifestBytes;

  // read the external manifest without scanning it
  try {
    const rootRealPath = await realpath(absoluteRoot);
    const absoluteManifestPath = await requireContainedFile(
      absoluteRoot,
      rootRealPath,
      manifestPath,
    );
    manifestBytes = await readFile(absoluteManifestPath);
  } catch (error) {
    const message = String(error.message);
    const code = message.includes("symbolic link")
      ? "path.symlink-escape"
      : message.includes("travers") || message.includes("relative")
        ? "path.traversal"
        : "manifest.read";
    return {
      ok: false,
      generation,
      phase,
      manifestDigest: null,
      runtimeMapDigest: null,
      runtimeMapArtifactCount: 0,
      runtimeMapBindingCount: 0,
      runtimeMapContextCount: 0,
      diagnostics: [diagnostic(code, message, { path: manifestPath })],
      discovered: [],
    };
  }
  let manifest;

  // distinguish json syntax from schema drift
  try {
    manifest = JSON.parse(manifestBytes.toString("utf8"));
  } catch (error) {
    return {
      ok: false,
      generation,
      phase,
      manifestDigest: sha256(manifestBytes),
      runtimeMapDigest: null,
      runtimeMapArtifactCount: 0,
      runtimeMapBindingCount: 0,
      runtimeMapContextCount: 0,
      diagnostics: [diagnostic("manifest.syntax", String(error.message), {
        path: manifestPath,
      })],
      discovered: [],
    };
  }
  let runtimeMapBytes;

  // read the checked runtime map through the same containment boundary
  try {
    const rootRealPath = await realpath(absoluteRoot);
    const absoluteRuntimeMapPath = await requireContainedFile(
      absoluteRoot,
      rootRealPath,
      runtimeMapPath,
    );
    runtimeMapBytes = await readFile(absoluteRuntimeMapPath);
  } catch (error) {
    const message = String(error.message);
    const code = message.includes("symbolic link")
      ? "path.symlink-escape"
      : message.includes("travers") || message.includes("relative")
        ? "path.traversal"
        : "live-map.read";
    return {
      ok: false,
      generation,
      phase,
      manifestDigest: sha256(manifestBytes),
      runtimeMapDigest: null,
      runtimeMapArtifactCount: 0,
      runtimeMapBindingCount: 0,
      runtimeMapContextCount: 0,
      diagnostics: [diagnostic(code, message, { path: runtimeMapPath })],
      discovered: [],
    };
  }
  let runtimeMap;

  // distinguish checked-map syntax from semantic drift
  try {
    runtimeMap = JSON.parse(runtimeMapBytes.toString("utf8"));
  } catch (error) {
    return {
      ok: false,
      generation,
      phase,
      manifestDigest: sha256(manifestBytes),
      runtimeMapDigest: sha256(runtimeMapBytes),
      runtimeMapArtifactCount: 0,
      runtimeMapBindingCount: 0,
      runtimeMapContextCount: 0,
      diagnostics: [diagnostic("live-map.syntax", String(error.message), {
        path: runtimeMapPath,
      })],
      discovered: [],
    };
  }
  diagnostics.push(...validateManifestSchema(manifest));
  const runtimeMapValidation = validateRuntimeLiveMap({ inventory: manifest, runtimeMap });
  diagnostics.push(...runtimeMapValidation.diagnostics);
  const discovery = await discoverRuntimeInventory({ rootDir: absoluteRoot });
  diagnostics.push(...discovery.diagnostics);

  // compare only a usable checked schema
  if (diagnostics.every((item) => !item.code.startsWith("manifest.") && item.code !== "path.traversal")) {
    diagnostics.push(...compareEntries(manifest.entries, discovery.entries));
    diagnostics.push(...validatePolicy(manifest, generation, phase));
  }
  const sortedDiagnostics = sortDiagnostics(diagnostics);
  return {
    ok: sortedDiagnostics.length === 0,
    generation,
    phase,
    manifestDigest: sha256(manifestBytes),
    runtimeMapDigest: sha256(runtimeMapBytes),
    runtimeMapArtifactCount: Array.isArray(runtimeMap.artifacts) ? runtimeMap.artifacts.length : 0,
    runtimeMapBindingCount: Array.isArray(runtimeMap.bindings) ? runtimeMap.bindings.length : 0,
    runtimeMapContextCount: Array.isArray(runtimeMap.contexts) ? runtimeMap.contexts.length : 0,
    diagnostics: sortedDiagnostics,
    discovered: discovery.entries,
  };
}

// run the cli through the exported validator
async function main() {
  let options;

  // render argument errors without a stack
  try {
    options = parseRuntimeInventoryArguments(process.argv.slice(2));
  } catch (error) {
    process.stderr.write(`${error.message}\n`);
    process.exitCode = 2;
    return;
  }
  const result = await validateRuntimeInventory({
    rootDir: resolve(import.meta.dirname, ".."),
    generation: options.generation,
    phase: options.phase,
  });
  const summary = {
    schemaVersion: "backend-runtime-inventory-check/v1",
    generation: result.generation,
    phase: result.phase,
    manifestDigest: result.manifestDigest,
    runtimeMapDigest: result.runtimeMapDigest,
    runtimeMapArtifactCount: result.runtimeMapArtifactCount,
    runtimeMapBindingCount: result.runtimeMapBindingCount,
    runtimeMapContextCount: result.runtimeMapContextCount,
    entryCount: result.discovered.length,
    ok: result.ok,
    diagnostics: result.diagnostics,
  };
  process.stdout.write(`${JSON.stringify(summary, null, 2)}\n`);

  // fail on any structural or policy diagnostic
  if (!result.ok) {
    process.exitCode = 1;
  }
}

// avoid cli side effects when imported
if (process.argv[1] !== undefined && import.meta.url === pathToFileURL(process.argv[1]).href) {
  await main();
}
