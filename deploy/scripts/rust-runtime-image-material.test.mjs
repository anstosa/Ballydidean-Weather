import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import {
  chmodSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  unlinkSync,
  writeFileSync,
} from "node:fs";
import { spawnSync } from "node:child_process";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import test from "node:test";
import {
  RUNTIME_IMAGE_MATERIAL_CLASS,
  RUNTIME_IMAGE_MATERIAL_VERSION,
  buildRustRuntimeImageMaterial,
  inspectRustRuntimeImageMaterial,
} from "./rust-runtime-image-material.mjs";

const FIXTURE_URL = new URL("./rust-runtime-image-material.fixture.json", import.meta.url);
const SCRIPT_PATH = new URL("./rust-runtime-image-material.mjs", import.meta.url).pathname;

// load one fresh material fixture
function fixture() {
  return JSON.parse(readFileSync(FIXTURE_URL, "utf8"));
}

// hash one canonical member sequence
function materialSha256(members) {
  const preimage = `${members.map((member) => JSON.stringify(member)).join("\n")}\n`;
  return createHash("sha256").update(preimage).digest("hex");
}

// build one minimal standard elf64 executable fixture
function aarch64ElfFixture(marker, type) {
  const payload = Buffer.from(`fixture:${marker}\n`, "utf8");
  const body = Buffer.alloc(64 + 56 + payload.length);
  body.set([0x7f, 0x45, 0x4c, 0x46, 2, 1, 1], 0);
  body.writeUInt16LE(type, 16);
  body.writeUInt16LE(183, 18);
  body.writeUInt32LE(1, 20);
  body.writeBigUInt64LE(0x400000n, 24);
  body.writeBigUInt64LE(64n, 32);
  body.writeUInt16LE(64, 52);
  body.writeUInt16LE(56, 54);
  body.writeUInt16LE(1, 56);
  body.writeUInt32LE(1, 64);
  body.writeUInt32LE(5, 68);
  body.writeBigUInt64LE(0n, 72);
  body.writeBigUInt64LE(0x400000n, 80);
  body.writeBigUInt64LE(0x400000n, 88);
  body.writeBigUInt64LE(BigInt(body.length), 96);
  body.writeBigUInt64LE(BigInt(body.length), 104);
  body.writeBigUInt64LE(0x1000n, 112);
  payload.copy(body, 120);
  return body;
}

// replace one staged member while preserving expected digest parity
function replaceExtractedMember(staged, runtimePath, body, mode = 0o755) {
  const record = staged.bytes.get(runtimePath);
  writeFileSync(record.path, body);
  chmodSync(record.path, mode);
  record.member.runtimeSha256 = createHash("sha256").update(body).digest("hex");
}

// stage the complete accepted server and web sets under disposable extracted roots
function extractedFixture() {
  const directory = mkdtempSync(join(tmpdir(), "weather-rust-image-material-"));
  const roots = {
    server: join(directory, "server"),
    web: join(directory, "web"),
  };
  const input = fixture();
  const bytes = new Map();
  // create every declared runtime member with a distinct deterministic body
  for (const image of ["server", "web"]) {
    // bind the expected digest to the bytes placed under the extracted root
    for (const member of input.expected[image]) {
      // stage native binary roles as executable aarch64 elf64 material
      const body = member.role === "binary"
        ? aarch64ElfFixture(`${image}:${member.path}`, image === "web" ? 3 : 2)
        : Buffer.from(`${image}:${member.path}\n`, "utf8");
      const relative = member.path.slice("/opt/weather/".length);
      const path = join(roots[image], relative);
      mkdirSync(dirname(path), { recursive: true });
      writeFileSync(path, body);
      // remove umask variability from staged member modes
      chmodSync(path, member.role === "binary" ? 0o755 : 0o644);
      member.runtimeSha256 = createHash("sha256").update(body).digest("hex");
      bytes.set(member.path, { member, path });
    }
  }
  // prove inspection never accepts the fixture's caller-supplied actual inventory
  input.actual = {
    server: "ignored caller inventory",
    web: null,
  };
  // remove every disposable runtime member after the owning test completes
  const cleanup = () => rmSync(directory, { force: true, recursive: true });
  return {
    bytes,
    cleanup,
    directory,
    input,
    roots,
  };
}

// build deterministic accepted server and web material
test("material preflight binds native ARM64 commands and full expected membership", () => {
  const input = fixture();
  const manifest = buildRustRuntimeImageMaterial(input);
  assert.equal(manifest.contractVersion, RUNTIME_IMAGE_MATERIAL_VERSION);
  assert.equal(manifest.artifactClass, RUNTIME_IMAGE_MATERIAL_CLASS);
  assert.equal(manifest.attestsBuiltImage, false);
  assert.deepEqual(manifest.platform, { architecture: "arm64", os: "linux" });
  assert.deepEqual(manifest.source, input.source);
  assert.deepEqual(manifest.build, input.build);
  assert.deepEqual(manifest.images.map((image) => image.name), ["server", "web"]);

  const [server, web] = manifest.images;
  assert.deepEqual(server.commands, [
    { argv: ["/opt/weather/bin/weather-api"], name: "api" },
    { argv: ["/opt/weather/bin/weather-api-health", "api"], name: "api-health" },
    { argv: ["/opt/weather/bin/weather-migrate"], name: "migrate" },
    { argv: ["/opt/weather/bin/weather-worker"], name: "worker" },
    { argv: ["/opt/weather/bin/weather-worker-health"], name: "worker-health" },
  ]);
  assert.deepEqual(web.commands, [
    { argv: ["/opt/weather/bin/weather-web"], name: "web" },
    { argv: ["/opt/weather/bin/weather-web-health", "web"], name: "web-health" },
  ]);
  assert.deepEqual(server.members, input.expected.server);
  assert.deepEqual(web.members, input.expected.web);
  assert.equal(server.materialManifestSha256, materialSha256(server.members));
  assert.equal(web.materialManifestSha256, materialSha256(web.members));
  assert.deepEqual(manifest.boundary, {
    serverRoles: ["binary", "config", "migration", "model"],
    sharedApplicationPaths: [],
    webRoles: ["binary", "config", "static"],
  });
});

// keep output stable across supplied member order
test("material preflight canonicalizes actual and expected ordering", () => {
  const ordered = fixture();
  const reordered = fixture();
  reordered.actual.server.reverse();
  reordered.actual.web.reverse();
  reordered.expected.server.reverse();
  reordered.expected.web.reverse();
  assert.deepEqual(
    buildRustRuntimeImageMaterial(reordered),
    buildRustRuntimeImageMaterial(ordered),
  );
});

// reject missing extra or changed supplied material
test("material preflight compares complete actual and expected members", () => {
  const missing = fixture();
  missing.actual.server.pop();
  assert.throws(
    () => buildRustRuntimeImageMaterial(missing),
    /server actual members do not exactly match expected members/u,
  );

  const extra = fixture();
  const extraMember = structuredClone(extra.actual.web.at(-1));
  extraMember.path = "/opt/weather/static/extra.html";
  extra.actual.web.push(extraMember);
  assert.throws(
    () => buildRustRuntimeImageMaterial(extra),
    /web actual members do not exactly match expected members/u,
  );

  const changed = fixture();
  changed.actual.server[0].runtimeSha256 = "f".repeat(64);
  assert.throws(
    () => buildRustRuntimeImageMaterial(changed),
    /server actual members do not exactly match expected members/u,
  );
});

// preserve the accepted role split
test("material preflight enforces server and web role boundaries", () => {
  const wrongRole = fixture();
  wrongRole.actual.server.find((member) => member.role === "config").role = "static";
  assert.throws(
    () => buildRustRuntimeImageMaterial(wrongRole),
    /role is outside the server boundary/u,
  );

  const missingRole = fixture();
  missingRole.actual.server.pop();
  missingRole.expected.server.pop();
  assert.throws(
    () => buildRustRuntimeImageMaterial(missingRole),
    /server is missing required model material/u,
  );
});

// preserve the exact disjoint native binary set
test("material preflight enforces exact native binaries and disjoint runtime paths", () => {
  const extraBinary = fixture();
  const member = structuredClone(extraBinary.actual.server[0]);
  member.path = "/opt/weather/bin/weather-extra";
  member.sourcePath = "rust/apps/weather-extra";
  extraBinary.actual.server.push(member);
  extraBinary.expected.server.push(structuredClone(member));
  assert.throws(
    () => buildRustRuntimeImageMaterial(extraBinary),
    /server binaries do not match the accepted native command set/u,
  );

  const overlap = fixture();
  overlap.actual.web.find((member) => member.role === "static").path =
    overlap.actual.server.find((member) => member.role === "config").path;
  overlap.expected.web.find((member) => member.role === "static").path =
    overlap.expected.server.find((member) => member.role === "config").path;
  assert.throws(
    () => buildRustRuntimeImageMaterial(overlap),
    /appears in both image boundaries/u,
  );
});

// bind exact source and native build identities
test("material preflight rejects unpinned or non-ARM64 build identity", () => {
  const badCommit = fixture();
  badCommit.source.commit = "main";
  assert.throws(
    () => buildRustRuntimeImageMaterial(badCommit),
    /exact lowercase commit SHA/u,
  );

  const badTarget = fixture();
  badTarget.build.target = "x86_64-unknown-linux-gnu";
  assert.throws(
    () => buildRustRuntimeImageMaterial(badTarget),
    /release aarch64-unknown-linux-gnu target/u,
  );
});

// bind the declared runtime members to hashes observed from extracted files
test("extracted material inspection ignores caller actual rows and hashes staged bytes", () => {
  const staged = extractedFixture();
  try {
    const manifest = inspectRustRuntimeImageMaterial(staged.input, staged.roots);
    assert.equal(manifest.artifactClass, "offline-extracted-image-material-inspection");
    assert.equal(manifest.attestsBuiltImage, false);
    assert.equal(manifest.inspection.provesElfArchitecture, true);
    assert.equal(manifest.inspection.provesPlatform, false);
    assert.equal(manifest.inspection.provesBuildIdentity, false);
    assert.deepEqual(manifest.images.map((image) => image.members), [
      staged.input.expected.server,
      staged.input.expected.web,
    ]);
    assert.deepEqual(manifest.images.map((image) => image.materialManifestSha256), [
      materialSha256(staged.input.expected.server),
      materialSha256(staged.input.expected.web),
    ]);
  } finally {
    staged.cleanup();
  }
});

// reject a binary whose digest matches but elf machine label is x86-64
test("extracted material inspection rejects x86-64 ELF identity", () => {
  const staged = extractedFixture();
  try {
    const runtimePath = "/opt/weather/bin/weather-api";
    const body = readFileSync(staged.bytes.get(runtimePath).path);
    body.writeUInt16LE(62, 18);
    replaceExtractedMember(staged, runtimePath, body);
    assert.throws(
      () => inspectRustRuntimeImageMaterial(staged.input, staged.roots),
      /weather-api must have a little-endian AArch64 ELF64 header/u,
    );
  } finally {
    staged.cleanup();
  }
});

// reject relocatable elf material even when its digest matches
test("extracted material inspection rejects ELF relocatable type", () => {
  const staged = extractedFixture();
  try {
    const runtimePath = "/opt/weather/bin/weather-migrate";
    const body = readFileSync(staged.bytes.get(runtimePath).path);
    body.writeUInt16LE(1, 16);
    replaceExtractedMember(staged, runtimePath, body);
    assert.throws(
      () => inspectRustRuntimeImageMaterial(staged.input, staged.roots),
      /weather-migrate must have an AArch64 ELF executable type and entry point/u,
    );
  } finally {
    staged.cleanup();
  }
});

// reject matching elf bytes without any executable mode bit
test("extracted material inspection rejects non-executable ELF mode", () => {
  const staged = extractedFixture();
  try {
    const runtimePath = "/opt/weather/bin/weather-worker";
    const body = readFileSync(staged.bytes.get(runtimePath).path);
    replaceExtractedMember(staged, runtimePath, body, 0o644);
    assert.throws(
      () => inspectRustRuntimeImageMaterial(staged.input, staged.roots),
      /weather-worker must be executable/u,
    );
  } finally {
    staged.cleanup();
  }
});

// reject a declared program table truncated from matching elf bytes
test("extracted material inspection rejects truncated ELF program table", () => {
  const staged = extractedFixture();
  try {
    const runtimePath = "/opt/weather/bin/weather-web";
    const body = readFileSync(staged.bytes.get(runtimePath).path).subarray(0, 64);
    replaceExtractedMember(staged, runtimePath, body);
    assert.throws(
      () => inspectRustRuntimeImageMaterial(staged.input, staged.roots),
      /weather-web must have an in-file ELF64 program header table/u,
    );
  } finally {
    staged.cleanup();
  }
});

// reject an expected member that is absent from the extracted server root
test("extracted material inspection rejects missing files", () => {
  const staged = extractedFixture();
  try {
    unlinkSync(staged.bytes.get("/opt/weather/bin/weather-api").path);
    assert.throws(
      () => inspectRustRuntimeImageMaterial(staged.input, staged.roots),
      /server.*(?:missing|exactly match)|missing.*server/iu,
    );
  } finally {
    staged.cleanup();
  }
});

// reject undeclared runtime bytes rather than silently narrowing the observed set
test("extracted material inspection rejects extra files", () => {
  const staged = extractedFixture();
  try {
    const extra = join(staged.roots.web, "static/undeclared.js");
    mkdirSync(dirname(extra), { recursive: true });
    writeFileSync(extra, "undeclared\n");
    assert.throws(
      () => inspectRustRuntimeImageMaterial(staged.input, staged.roots),
      /web.*(?:extra|unexpected|exactly match)|(?:extra|unexpected).*web/iu,
    );
  } finally {
    staged.cleanup();
  }
});

// reject a regular file whose observed bytes no longer match the expected digest
test("extracted material inspection rejects changed file bytes", () => {
  const staged = extractedFixture();
  try {
    writeFileSync(
      staged.bytes.get("/opt/weather/static/index.html").path,
      "changed runtime bytes\n",
    );
    assert.throws(
      () => inspectRustRuntimeImageMaterial(staged.input, staged.roots),
      /(?:web.*(?:digest|hash|match))|(?:(?:digest|hash|match).*web)/iu,
    );
  } finally {
    staged.cleanup();
  }
});

// reject aliases before one extracted tree can stand in for both image boundaries
test("extracted material inspection requires distinct roots", () => {
  const staged = extractedFixture();
  try {
    assert.throws(
      () => inspectRustRuntimeImageMaterial(staged.input, {
        server: staged.roots.server,
        web: staged.roots.server,
      }),
      /roots?.*(?:distinct|alias|same)|(?:distinct|alias|same).*roots?/iu,
    );
  } finally {
    staged.cleanup();
  }
});

// exercise the real inspect CLI over disposable extracted application roots
test("extracted material inspection CLI succeeds and reports byte drift", () => {
  const staged = extractedFixture();
  try {
    const inputPath = join(staged.directory, "inspection-input.json");
    const cliInput = structuredClone(staged.input);
    delete cliInput.actual;
    writeFileSync(inputPath, `${JSON.stringify(cliInput)}\n`);
    const accepted = spawnSync(
      process.execPath,
      [SCRIPT_PATH, "--inspect", inputPath, staged.roots.server, staged.roots.web],
      { encoding: "utf8" },
    );
    assert.equal(accepted.status, 0, accepted.stderr);
    const manifest = JSON.parse(accepted.stdout);
    assert.equal(manifest.artifactClass, "offline-extracted-image-material-inspection");
    assert.equal(manifest.attestsBuiltImage, false);

    writeFileSync(
      staged.bytes.get("/opt/weather/bin/weather-worker").path,
      "changed worker\n",
    );
    const rejected = spawnSync(
      process.execPath,
      [SCRIPT_PATH, "--inspect", inputPath, staged.roots.server, staged.roots.web],
      { encoding: "utf8" },
    );
    assert.equal(rejected.status, 1, rejected.stdout);
    assert.match(rejected.stderr, /rust-runtime-image-material:.*(?:digest|hash|match)/iu);
  } finally {
    staged.cleanup();
  }
});

// run the checked fixture through the offline CLI
test("material fixture is runnable without image tooling", () => {
  const result = spawnSync(process.execPath, [SCRIPT_PATH, FIXTURE_URL.pathname], {
    encoding: "utf8",
  });
  assert.equal(result.status, 0, result.stderr);
  const manifest = JSON.parse(result.stdout);
  assert.equal(manifest.artifactClass, RUNTIME_IMAGE_MATERIAL_CLASS);
  assert.equal(manifest.attestsBuiltImage, false);
});
