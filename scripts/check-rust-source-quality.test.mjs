import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import test from "node:test";
import { fileURLToPath } from "node:url";

import {
  rustSourceQualityCommands,
  selectHandAuthoredWorkspacePackages,
} from "./check-rust-source-quality.mjs";

const repositoryRoot = new URL("../", import.meta.url);

// build one Cargo metadata package fixture
function packageFixture(name, id, relativeManifest, relativeSource) {
  return {
    id,
    manifest_path: fileURL(relativeManifest),
    name,
    targets: [{
      kind: ["lib"],
      src_path: fileURL(relativeSource),
    }],
  };
}

// return one decoded absolute fixture path
function fileURL(relativePath) {
  return new URL(relativePath, repositoryRoot).pathname;
}

// prove package derivation excludes only the exact generated DTO target
test("selects every hand-authored Rust workspace package", () => {
  const metadata = {
    packages: [
      packageFixture(
        "weather-contracts",
        "contracts-id",
        "rust/crates/weather-contracts/Cargo.toml",
        "rust/crates/weather-contracts/src/generated.rs",
      ),
      packageFixture(
        "weather-edge",
        "edge-id",
        "rust/crates/weather-edge/Cargo.toml",
        "rust/crates/weather-edge/src/lib.rs",
      ),
      packageFixture(
        "weather-api",
        "api-id",
        "rust/crates/weather-api/Cargo.toml",
        "rust/crates/weather-api/src/lib.rs",
      ),
      packageFixture(
        "outside",
        "outside-id",
        "outside/Cargo.toml",
        "outside/src/lib.rs",
      ),
    ],
    workspace_members: ["contracts-id", "edge-id", "api-id"],
  };
  assert.deepEqual(selectHandAuthoredWorkspacePackages(metadata), [
    "weather-api",
    "weather-edge",
  ]);
});

// prove a same-name hand-authored crate cannot bypass source linting
test("rejects an unproven generated package exclusion", () => {
  const metadata = {
    packages: [packageFixture(
      "weather-contracts",
      "contracts-id",
      "rust/crates/weather-contracts/Cargo.toml",
      "rust/crates/weather-contracts/src/lib.rs",
    )],
    workspace_members: ["contracts-id"],
  };
  assert.throws(
    () => selectHandAuthoredWorkspacePackages(metadata),
    /not the expected generated-only crate/u,
  );
});

// prove any future hand-authored target restores full package quality checks
test("rejects additional targets in the generated package", () => {
  const contracts = packageFixture(
    "weather-contracts",
    "contracts-id",
    "rust/crates/weather-contracts/Cargo.toml",
    "rust/crates/weather-contracts/src/generated.rs",
  );
  contracts.targets.push({
    kind: ["test"],
    src_path: fileURL("rust/crates/weather-contracts/tests/hand-authored.rs"),
  });
  assert.throws(
    () => selectHandAuthoredWorkspacePackages({
      packages: [contracts],
      workspace_members: ["contracts-id"],
    }),
    /not the expected generated-only crate/u,
  );
});

// prove every command retains explicit packages and offline locked compilation
test("constructs formatting build and strict Clippy commands", () => {
  const commands = rustSourceQualityCommands(
    ["weather-api", "weather-edge"],
    "/repo/rust/Cargo.toml",
  );
  assert.deepEqual(commands, [
    [
      "fmt",
      "--manifest-path",
      "/repo/rust/Cargo.toml",
      "--package",
      "weather-api",
      "--package",
      "weather-edge",
      "--",
      "--check",
    ],
    [
      "build",
      "--manifest-path",
      "/repo/rust/Cargo.toml",
      "--all-targets",
      "--locked",
      "--offline",
      "--package",
      "weather-api",
      "--package",
      "weather-edge",
    ],
    [
      "clippy",
      "--manifest-path",
      "/repo/rust/Cargo.toml",
      "--all-targets",
      "--locked",
      "--offline",
      "--no-deps",
      "--package",
      "weather-api",
      "--package",
      "weather-edge",
      "--",
      "-D",
      "warnings",
    ],
  ]);
});

// help and invalid arguments must never start cargo or allocate a build target
test("source-quality command admits only the intended no-argument execution", () => {
  const script = fileURLToPath(new URL("./check-rust-source-quality.mjs", import.meta.url));
  const options = {
    encoding: "utf8",
    env: { ...process.env, PATH: "" },
  };

  // check both conventional help spellings without exposing cargo in the subprocess
  for (const flag of ["--help", "-h"]) {
    const result = spawnSync(process.execPath, [script, flag], options);
    assert.equal(result.status, 0, result.stderr);
    assert.match(result.stdout, /^Usage: node scripts\/check-rust-source-quality\.mjs/mu);
    assert.doesNotMatch(result.stdout, /\+ cargo/u);
  }
  const result = spawnSync(process.execPath, [script, "--unknown"], options);
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /unexpected argument/u);
  assert.doesNotMatch(result.stdout, /\+ cargo/u);
});
