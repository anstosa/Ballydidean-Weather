import { spawnSync } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const generatedPackageName = "weather-contracts";
const repositoryRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const rustWorkspace = resolve(repositoryRoot, "rust");
const workspaceManifest = resolve(rustWorkspace, "Cargo.toml");

// identify only the exact deterministic generated DTO package
function isGeneratedContractsPackage(candidate) {
  const expectedManifest = resolve(
    repositoryRoot,
    "rust/crates/weather-contracts/Cargo.toml",
  );
  const expectedSource = resolve(
    repositoryRoot,
    "rust/crates/weather-contracts/src/generated.rs",
  );
  const targets = Array.isArray(candidate?.targets) ? candidate.targets : [];

  // require the exact package, manifest, and single generated library target
  return candidate?.name === generatedPackageName &&
    resolve(candidate.manifest_path ?? "") === expectedManifest &&
    targets.length === 1 &&
    Array.isArray(targets[0].kind) &&
    targets[0].kind.includes("lib") &&
    resolve(targets[0].src_path ?? "") === expectedSource;
}

// select every hand-authored package from one complete no-dependency metadata document
export function selectHandAuthoredWorkspacePackages(metadata) {
  if (!Array.isArray(metadata?.packages) ||
    !Array.isArray(metadata.workspace_members)) {
    throw new Error("Cargo metadata does not describe a workspace");
  }
  const workspaceMembers = new Set(metadata.workspace_members);
  const packages = [];
  let generatedPackageCount = 0;

  // bind package names only to exact workspace member identifiers
  for (const candidate of metadata.packages) {
    if (!workspaceMembers.has(candidate.id)) {
      continue;
    }

    // exclude only the proven deterministic generated DTO package
    if (candidate.name === generatedPackageName) {
      if (!isGeneratedContractsPackage(candidate)) {
        throw new Error("weather-contracts is not the expected generated-only crate");
      }
      generatedPackageCount += 1;
      continue;
    }
    if (typeof candidate.name !== "string" || candidate.name.length === 0) {
      throw new Error("Cargo metadata contains an unnamed workspace package");
    }
    packages.push(candidate.name);
  }

  // fail closed if metadata did not prove exactly one generated package
  if (generatedPackageCount !== 1) {
    throw new Error("Cargo workspace must contain exactly one generated weather-contracts crate");
  }
  if (packages.length === 0) {
    throw new Error("Cargo workspace contains no hand-authored Rust packages");
  }
  packages.sort();

  // reject duplicate package names before constructing Cargo selectors
  for (let index = 1; index < packages.length; index += 1) {
    if (packages[index] === packages[index - 1]) {
      throw new Error(`duplicate workspace package name: ${packages[index]}`);
    }
  }
  return packages;
}

// construct the three exact quality commands for selected package names
export function rustSourceQualityCommands(packageNames, manifestPath = workspaceManifest) {
  if (!Array.isArray(packageNames) || packageNames.length === 0) {
    throw new Error("Rust source-quality checks require at least one package");
  }
  const selectors = [];

  // add one explicit package selector without workspace-wide generated linting
  for (const packageName of packageNames) {
    selectors.push("--package", packageName);
  }
  return [
    ["fmt", "--manifest-path", manifestPath, ...selectors, "--", "--check"],
    [
      "build",
      "--manifest-path",
      manifestPath,
      "--all-targets",
      "--locked",
      "--offline",
      ...selectors,
    ],
    [
      "clippy",
      "--manifest-path",
      manifestPath,
      "--all-targets",
      "--locked",
      "--offline",
      "--no-deps",
      ...selectors,
      "--",
      "-D",
      "warnings",
    ],
  ];
}

// execute one Cargo command with inherited diagnostics
function runCargo(commandArguments, environment, capture = false) {
  process.stdout.write(`+ cargo ${commandArguments.join(" ")}\n`);
  const result = spawnSync("cargo", commandArguments, {
    // select the nested rustup pin before cargo reads the workspace manifest
    cwd: rustWorkspace,
    encoding: capture ? "utf8" : undefined,
    env: environment,
    stdio: capture ? ["ignore", "pipe", "inherit"] : "inherit",
  });

  // preserve process startup failures as ordinary check failures
  if (result.error) {
    throw result.error;
  }
  if (result.status !== 0) {
    throw new Error(`cargo ${commandArguments[0]} failed with status ${String(result.status)}`);
  }
  return capture ? result.stdout : undefined;
}

// run source quality inside the caller's bounded target or one removed temporary target
export function runRustSourceQuality(environment = process.env) {
  const inheritedTarget = environment.CARGO_TARGET_DIR;
  const temporaryTarget = inheritedTarget
    ? null
    : mkdtempSync(resolve(tmpdir(), "weather-rust-source-quality-"));
  const executionEnvironment = {
    ...environment,
    CARGO_TARGET_DIR: inheritedTarget ?? temporaryTarget,
  };

  try {
    const metadataText = runCargo([
      "metadata",
      "--manifest-path",
      workspaceManifest,
      "--format-version",
      "1",
      "--locked",
      "--offline",
      "--no-deps",
    ], executionEnvironment, true);
    const metadata = JSON.parse(metadataText);
    const packages = selectHandAuthoredWorkspacePackages(metadata);
    process.stdout.write(
      `hand-authored Rust packages: ${packages.join(", ")}\n` +
      `generated-only Rust package: ${generatedPackageName}\n`,
    );

    // execute formatting, build, then lint over the same explicit package set
    for (const commandArguments of rustSourceQualityCommands(packages)) {
      runCargo(commandArguments, executionEnvironment);
    }
  } finally {
    // remove only a target directory created by this invocation
    if (temporaryTarget !== null) {
      rmSync(temporaryTarget, { force: true, recursive: true });
    }
  }
}

// run only when invoked as the source-quality executable
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  runRustSourceQuality();
}
