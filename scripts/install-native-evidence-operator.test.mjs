import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import {
  chmodSync,
  lstatSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join, relative, resolve } from "node:path";
import test from "node:test";

const repoRoot = resolve(import.meta.dirname, "..");
const installerSource = join(repoRoot, "scripts/install-native-evidence-operator.sh");
const commit = "a".repeat(40);

// hash one exact fixture byte sequence
function sha256(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}

// write one executable fixture helper
function writeExecutable(path, lines) {
  writeFileSync(path, `${lines.join("\n")}\n`, { mode: 0o755 });
  chmodSync(path, 0o755);
}

// build a minimal ELF64 little-endian AArch64 header for the real ABI probe
function aarch64ElfHeader() {
  const bytes = Buffer.alloc(64);
  Buffer.from([0x7f, 0x45, 0x4c, 0x46, 0x02, 0x01, 0x01]).copy(bytes, 0);
  Buffer.from([0xb7, 0x00]).copy(bytes, 18);
  return bytes;
}

// create the exact six-member package consumed by the real installer
function packageFixture(directory) {
  mkdirSync(directory, { mode: 0o700 });
  const legacyDispatch = Buffer.from("# retained native dispatch source\n", "utf8");
  const gateway = Buffer.from([
    "#!/usr/bin/env bash",
    "set -euo pipefail",
    `[[ \"$#\" == 2 && \"$1\" == --verify-native-package && \"$2\" == ${commit} ]] || exit 64`,
    "[[ ${NATIVE_INSTALL_VERIFY:-failure} == success ]]",
    "",
  ].join("\n"), "utf8");
  const sourceCommit = Buffer.from(`${commit}\n`, "utf8");
  const binary = aarch64ElfHeader();
  const members = [
    ["native-dashboard-release-control.sh", Buffer.from("# fixture native dashboard controller\n"), 0o644],
    ["native-evidence-remote-dispatch.sh", legacyDispatch, 0o644],
    ["native-generation-remote-ops.sh", gateway, 0o755],
    ["native-release-control-remote-dispatch.sh", Buffer.from("# fixture native release controller\n"), 0o644],
    ["source-commit", sourceCommit, 0o644],
    ["weather-evidence", binary, 0o755],
  ];
  // publish every exact package member before deriving its manifest
  for (const [name, bytes, mode] of members) {
    writeFileSync(join(directory, name), bytes, { mode });
    chmodSync(join(directory, name), mode);
  }
  const manifest = Buffer.from(
    members.map(([name, bytes]) => `${sha256(bytes)}  ${name}\n`).join(""),
    "utf8",
  );
  writeFileSync(join(directory, "manifest.sha256"), manifest, { mode: 0o644 });
  chmodSync(join(directory, "manifest.sha256"), 0o644);
  chmodSync(directory, 0o700);
  return { bundle: sha256(manifest), gateway, members };
}

// snapshot retained bytes and modes without treating timestamps as identity
function treeSnapshot(root) {
  const records = [];
  // walk only the disposable fixture tree
  function visit(path) {
    const details = lstatSync(path);
    const name = relative(root, path) || ".";
    records.push({
      bytesSha256: details.isFile() ? sha256(readFileSync(path)) : null,
      mode: details.mode & 0o777,
      path: name,
      type: details.isDirectory() ? "directory" : "file",
    });
    // visit directory members in canonical order
    if (details.isDirectory()) {
      for (const member of readdirSync(path).sort()) {
        visit(join(path, member));
      }
    }
  }
  visit(root);
  return records;
}

// create one fully disposable rewritten installer environment
function installerFixture({ seedLegacyWithoutReceipt = false } = {}) {
  const root = mkdtempSync(join(tmpdir(), "weather-native-evidence-install-"));
  const operatorRoot = join(root, "opt/weather/operators/native-evidence");
  const sbin = join(root, "usr/local/sbin");
  const tools = join(root, "tools");
  const source = join(root, "package");
  mkdirSync(sbin, { mode: 0o755, recursive: true });
  mkdirSync(tools, { mode: 0o755 });
  const packageMaterial = packageFixture(source);
  const legacyGateway = Buffer.from([
    "#!/usr/bin/env bash",
    "printf 'legacy gateway\\n'",
    "",
  ].join("\n"), "utf8");
  const gateway = join(sbin, "weather-remote-ops");
  writeFileSync(gateway, legacyGateway, { mode: 0o755 });
  chmodSync(gateway, 0o755);

  const installer = join(root, "install-native-evidence-operator.sh");
  const rewritten = readFileSync(installerSource, "utf8")
    .replace('[[ "$EUID" == 0 ]]', `[[ "$EUID" == ${process.getuid()} ]]`)
    .replaceAll("/opt/weather/operators/native-evidence", operatorRoot)
    .replaceAll("/usr/local/sbin", sbin);
  writeFileSync(installer, rewritten, { mode: 0o755 });
  chmodSync(installer, 0o755);

  // project root ownership while preserving actual fixture modes and link counts
  writeExecutable(join(tools, "stat"), [
    "#!/usr/bin/env bash",
    "set -euo pipefail",
    "if [[ ${1-} == -c && ${2-} == %u:%g:%a ]]; then",
    "  printf '0:0:%s\\n' \"$(/usr/bin/stat -c %a -- \"${@: -1}\")\"",
    "  exit",
    "fi",
    "if [[ ${1-} == -c && ${2-} == %u:%g:%a:%h ]]; then",
    "  printf '0:0:%s:%s\\n' \"$(/usr/bin/stat -c %a -- \"${@: -1}\")\" \"$(/usr/bin/stat -c %h -- \"${@: -1}\")\"",
    "  exit",
    "fi",
    "exec /usr/bin/stat \"$@\"",
  ]);
  // discard only fixture-inapplicable root owner operands
  writeExecutable(join(tools, "install"), [
    "#!/usr/bin/env bash",
    "set -euo pipefail",
    "arguments=()",
    "while (($#)); do",
    "  case $1 in",
    "    -o|-g) shift 2 ;;",
    "    *) arguments+=(\"$1\"); shift ;;",
    "  esac",
    "done",
    "exec /usr/bin/install \"${arguments[@]}\"",
  ]);
  // project the production host architecture without bypassing the ELF probe
  writeExecutable(join(tools, "uname"), [
    "#!/usr/bin/env bash",
    "if [[ ${1-} == -m ]]; then printf 'aarch64\\n'; else exec /usr/bin/uname \"$@\"; fi",
  ]);

  // seed the exact interrupted predecessor publication when requested
  if (seedLegacyWithoutReceipt) {
    mkdirSync(operatorRoot, { mode: 0o755, recursive: true });
    chmodSync(operatorRoot, 0o755);
    writeFileSync(join(operatorRoot, "legacy-remote-ops.sh"), legacyGateway, { mode: 0o755 });
    chmodSync(join(operatorRoot, "legacy-remote-ops.sh"), 0o755);
  }
  return {
    bundle: packageMaterial.bundle,
    gateway,
    gatewayBytes: packageMaterial.gateway,
    installer,
    legacyGateway,
    legacySha256: sha256(legacyGateway),
    operatorRoot,
    root,
    source,
    tools,
  };
}

// execute the rewritten real installer with one selected gateway probe outcome
function runInstaller(context, verification) {
  return spawnSync("bash", [
    context.installer,
    context.source,
    context.bundle,
    context.legacySha256,
    context.legacySha256,
  ], {
    encoding: "utf8",
    env: {
      ...process.env,
      NATIVE_INSTALL_VERIFY: verification,
      PATH: `${context.tools}:${process.env.PATH}`,
    },
  });
}

// assert the retained package and predecessor gateway identities
function assertInstalled(context) {
  const target = join(context.operatorRoot, "packages", context.bundle);
  assert.deepEqual(readFileSync(context.gateway), context.gatewayBytes);
  assert.deepEqual(
    readFileSync(join(context.operatorRoot, "legacy-remote-ops.sh")),
    context.legacyGateway,
  );
  assert.equal(
    readFileSync(join(context.operatorRoot, "legacy-remote-ops.sha256"), "utf8"),
    `${context.legacySha256}\n`,
  );
  assert.deepEqual(
    readFileSync(join(context.operatorRoot, "gateways", context.legacySha256)),
    context.legacyGateway,
  );
  assert.equal(
    readFileSync(join(context.operatorRoot, "by-commit", commit), "utf8"),
    `${context.bundle}\n`,
  );
  assert.equal(lstatSync(target).mode & 0o777, 0o755);
  // compare every retained package member byte for byte
  for (const member of readdirSync(context.source).sort()) {
    assert.deepEqual(readFileSync(join(target, member)), readFileSync(join(context.source, member)));
  }
}

test("installer publishes one immutable package and retries without identity drift", () => {
  const context = installerFixture();
  try {
    const initial = runInstaller(context, "success");
    assert.equal(initial.status, 0, initial.stderr);
    assert.match(initial.stdout, new RegExp(`Installed native evidence package ${context.bundle}`, "u"));
    assertInstalled(context);

    const firstRetry = runInstaller(context, "success");
    assert.equal(firstRetry.status, 0, firstRetry.stderr);
    const retriedState = treeSnapshot(context.root);
    const secondRetry = runInstaller(context, "success");
    assert.equal(secondRetry.status, 0, secondRetry.stderr);
    assert.deepEqual(treeSnapshot(context.root), retriedState);
    assertInstalled(context);
  } finally {
    rmSync(context.root, { force: true, recursive: true });
  }
});

test("installer completes the exact interrupted legacy receipt retry", () => {
  const context = installerFixture({ seedLegacyWithoutReceipt: true });
  try {
    const result = runInstaller(context, "success");
    assert.equal(result.status, 0, `${result.stderr}\n${JSON.stringify(treeSnapshot(context.root))}`);
    assertInstalled(context);
  } finally {
    rmSync(context.root, { force: true, recursive: true });
  }
});

test("installer restores exact gateway bytes after probe failure and then recovers", () => {
  const context = installerFixture();
  try {
    const failed = runInstaller(context, "failure");
    assert.notEqual(failed.status, 0);
    assert.match(failed.stderr, /native evidence package verification failed/u);
    assert.deepEqual(readFileSync(context.gateway), context.legacyGateway);
    assert.equal(
      readFileSync(join(context.operatorRoot, "by-commit", commit), "utf8"),
      `${context.bundle}\n`,
    );

    const recovered = runInstaller(context, "success");
    assert.equal(recovered.status, 0, recovered.stderr);
    assertInstalled(context);
  } finally {
    rmSync(context.root, { force: true, recursive: true });
  }
});
