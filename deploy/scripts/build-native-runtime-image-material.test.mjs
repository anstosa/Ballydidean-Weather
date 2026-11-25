import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import {
  chmodSync,
  mkdirSync,
  mkdtempSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { spawnSync } from "node:child_process";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import test from "node:test";
import { buildNativeRuntimeImageMaterialInput } from "./build-native-runtime-image-material.mjs";
import { buildRustRuntimeImageMaterial } from "./rust-runtime-image-material.mjs";

const SCRIPT = new URL("./build-native-runtime-image-material.mjs", import.meta.url).pathname;

// run one required fixture command without shell interpolation
function run(command, args, cwd) {
  const result = spawnSync(command, args, { cwd, encoding: "utf8" });
  assert.equal(result.status, 0, result.stderr);
  return result.stdout.trim();
}

// write one deterministic source or build member
function write(root, relative, bytes, mode = 0o644) {
  const path = join(root, relative);
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, bytes);
  chmodSync(path, mode);
}

// build one minimal executable AArch64 ELF identity
function aarch64Elf(marker) {
  const bytes = Buffer.alloc(64 + Buffer.byteLength(marker));
  bytes.set([0x7f, 0x45, 0x4c, 0x46, 2, 1, 1], 0);
  bytes.writeUInt16LE(3, 16);
  bytes.writeUInt16LE(183, 18);
  bytes.writeUInt32LE(1, 20);
  bytes.writeBigUInt64LE(0x400000n, 24);
  bytes.write(marker, 64);
  return bytes;
}

// create one clean exact Git checkout and disjoint build-stage export
function fixture() {
  const root = mkdtempSync(join(tmpdir(), "weather-native-material-generator-"));
  const sourceRoot = join(root, "source");
  const buildRoot = join(root, "build");
  mkdirSync(sourceRoot);
  mkdirSync(buildRoot);
  const binarySources = [
    "weather-api.rs",
    "weather-http-health.rs",
    "weather-web.rs",
    "weather-worker-health.rs",
    "weather-worker.rs",
  ];
  // retain every source file named by the fixed native command map
  for (const name of binarySources) {
    write(sourceRoot, `rust/crates/weather-runtime/src/bin/${name}`, `fn main() { /* ${name} */ }\n`);
  }
  write(sourceRoot, "rust/apps/weather-migrate/src/main.rs", "fn main() {}\n");
  write(sourceRoot, "packages/database/migrations/0001.sql", "select 1;\n");
  write(sourceRoot, "config/ecowitt/site.json", "{\"site\":\"fixture\"}\n");
  write(sourceRoot, "config/forecast-adjustments/runtime.json", "{\"model\":true}\n");
  write(sourceRoot, "config/public-stations/stations.json", "[]\n");
  write(sourceRoot, "config/runtime-targets.json", "{\"target\":\"fixture\"}\n");
  write(sourceRoot, "config/sites/ballydidean.json", "{\"key\":\"ballydidean\"}\n");
  write(sourceRoot, "config/tempest/site.json", "{\"site\":\"fixture\"}\n");
  write(sourceRoot, "config/tides/site.json", "{\"site\":\"fixture\"}\n");
  write(sourceRoot, "apps/web/public/index.html", "<main>public</main>\n");
  run("/usr/bin/git", ["init", "-q"], sourceRoot);
  run("/usr/bin/git", ["config", "user.email", "fixture@example.invalid"], sourceRoot);
  run("/usr/bin/git", ["config", "user.name", "fixture"], sourceRoot);
  run("/usr/bin/git", ["add", "."], sourceRoot);
  run("/usr/bin/git", ["commit", "-q", "-m", "fixture"], sourceRoot);
  const sourceCommit = run("/usr/bin/git", ["rev-parse", "HEAD"], sourceRoot);
  const binaries = [
    "weather-api",
    "weather-http-health",
    "weather-migrate",
    "weather-web",
    "weather-worker-health",
    "weather-worker",
  ];
  // export only the native-build products accepted by the Docker scratch stage
  for (const name of binaries) {
    write(buildRoot, `bin/${name}`, aarch64Elf(name), 0o755);
  }
  write(buildRoot, "web-dist/index.html", "<main>built</main>\n");
  write(buildRoot, "web-dist/assets/widget.js", "export const built = true;\n");
  return {
    buildRoot,
    cleanup: () => rmSync(root, { force: true, recursive: true }),
    root,
    sourceCommit,
    sourceRoot,
  };
}

// derive one lowercase digest for focused byte identity assertions
function digest(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}

test("generator derives complete inspector input from build export and exact checkout", () => {
  const staged = fixture();
  try {
    const input = buildNativeRuntimeImageMaterialInput(staged);
    assert.equal(input.inputVersion, "weather-rust-runtime-image-material-input/v1");
    assert.equal(input.source.commit, staged.sourceCommit);
    assert.match(input.source.treeSha256, /^[a-f0-9]{64}$/u);
    assert.deepEqual(input.build, {
      identitySha256: input.build.identitySha256,
      profile: "release",
      target: "aarch64-unknown-linux-gnu",
    });
    assert.match(input.build.identitySha256, /^[a-f0-9]{64}$/u);
    assert.deepEqual(
      input.expected.server.filter((member) => member.role === "binary").map((member) => member.path),
      [
        "/opt/weather/bin/weather-api",
        "/opt/weather/bin/weather-api-health",
        "/opt/weather/bin/weather-migrate",
        "/opt/weather/bin/weather-worker",
        "/opt/weather/bin/weather-worker-health",
      ],
    );
    assert.deepEqual(
      input.expected.web.filter((member) => member.role === "binary").map((member) => member.path),
      ["/opt/weather/bin/weather-web", "/opt/weather/bin/weather-web-health"],
    );
    const site = input.expected.web.find((member) => member.role === "config");
    assert.equal(site.path, "/opt/weather/edge-config/ballydidean.json");
    assert.equal(site.sourcePath, "config/sites/ballydidean.json");
    const staticPaths = input.expected.web
      .filter((member) => member.role === "static")
      .map((member) => member.path);
    assert.deepEqual(staticPaths, [
      "/opt/weather/apps/web/dist/assets/widget.js",
      "/opt/weather/apps/web/dist/index.html",
      "/opt/weather/apps/web/public/index.html",
    ]);
    const api = input.expected.server.find((member) => member.path.endsWith("/weather-api"));
    assert.equal(api.runtimeSha256, digest(aarch64Elf("weather-api")));
    assert.notEqual(api.sourceSha256, api.runtimeSha256);
    const accepted = buildRustRuntimeImageMaterial({
      ...input,
      actual: structuredClone(input.expected),
    });
    assert.equal(accepted.images[0].members.length, input.expected.server.length);
    assert.equal(accepted.images[1].members.length, input.expected.web.length);
  } finally {
    staged.cleanup();
  }
});

test("generator CLI emits the same deterministic inspection input", () => {
  const staged = fixture();
  try {
    const expected = buildNativeRuntimeImageMaterialInput(staged);
    const result = spawnSync(
      process.execPath,
      [SCRIPT, staged.sourceCommit, staged.sourceRoot, staged.buildRoot],
      { encoding: "utf8" },
    );
    assert.equal(result.status, 0, result.stderr);
    assert.deepEqual(JSON.parse(result.stdout), expected);
  } finally {
    staged.cleanup();
  }
});

test("generator rejects source drift and widened build exports", () => {
  const dirty = fixture();
  try {
    write(dirty.sourceRoot, "config/runtime-targets.json", "{\"changed\":true}\n");
    assert.throws(
      () => buildNativeRuntimeImageMaterialInput(dirty),
      /exact clean checkout/u,
    );
  } finally {
    dirty.cleanup();
  }

  const widened = fixture();
  try {
    write(widened.buildRoot, "bin/weather-extra", aarch64Elf("extra"), 0o755);
    assert.throws(
      () => buildNativeRuntimeImageMaterialInput(widened),
      /build export membership differs/u,
    );
  } finally {
    widened.cleanup();
  }
});
