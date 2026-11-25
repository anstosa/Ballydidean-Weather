import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import {
  chmodSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  realpathSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

const CHECK_WORKFLOW = new URL("../.github/workflows/check.yml", import.meta.url);
const STEP_NAME = "Prove the nested Rust toolchain pin supersedes the runner default";

// extract only the checked-in shell body before the following workflow step
function nestedRustToolchainStepBody() {
  const workflow = readFileSync(CHECK_WORKFLOW, "utf8");
  const header = `      - name: ${STEP_NAME}`;
  const stepStart = workflow.indexOf(header);
  assert.notEqual(stepStart, -1, `missing workflow step: ${STEP_NAME}`);
  const nextStep = workflow.indexOf("\n      - name:", stepStart + header.length);
  assert.notEqual(nextStep, -1, `missing step after: ${STEP_NAME}`);
  const step = workflow.slice(stepStart, nextStep);
  assert.match(step, /\n        shell: bash\n/u);
  const runMarker = "        run: |\n";
  const bodyStart = step.indexOf(runMarker);
  assert.notEqual(bodyStart, -1, `missing shell body: ${STEP_NAME}`);
  return step.slice(bodyStart + runMarker.length).replace(/^ {10}/gmu, "");
}

// run the exact workflow body with two observable fake Rust commands
function runNestedRustToolchainStep(nestedVersion, nestedReason = "file") {
  const root = realpathSync(mkdtempSync(join(tmpdir(), "weather-rust-toolchain-pin-")));
  const bin = join(root, "bin");
  const rust = join(root, "rust");
  const trace = join(root, "trace.txt");
  mkdirSync(bin, { mode: 0o700 });
  mkdirSync(rust, { mode: 0o700 });
  writeFileSync(
    join(rust, "rust-toolchain.toml"),
    '[toolchain]\nchannel = "1.97.1"\nprofile = "minimal"\n',
  );
  writeFileSync(trace, "");
  writeFileSync(
    join(bin, "rustup"),
    `#!/usr/bin/env bash
set -euo pipefail
printf 'rustup:%s:%s\\n' "\${PWD}" "\$*" >> "\${TRACE}"
# model the root default and the exact nested selection reason
if [[ "$*" == 'show active-toolchain' ]]; then
  if [[ "\${PWD}" == */rust ]]; then
    if [[ "\${FIXTURE_NESTED_REASON}" == file ]]; then
      printf "1.97.1-x86_64-unknown-linux-gnu (overridden by '%s/rust-toolchain.toml')\\n" "\${PWD}"
    else
      printf "1.97.1-x86_64-unknown-linux-gnu (directory override for '%s')\\n" "\${PWD}"
    fi
  else
    printf '1.85.1-x86_64-unknown-linux-gnu (default)\\n'
  fi
fi
`,
  );
  writeFileSync(
    join(bin, "rustc"),
    `#!/usr/bin/env bash
set -euo pipefail
printf 'rustc:%s:%s\\n' "\${PWD}" "\$*" >> "\${TRACE}"
version=1.85.1
# model the nested rust-toolchain.toml override
[[ "\${PWD}" == */rust ]] && version="\${FIXTURE_NESTED_VERSION}"
printf 'rustc %s (fixture)\\nrelease: %s\\n' "\${version}" "\${version}"
`,
  );
  chmodSync(join(bin, "rustup"), 0o700);
  chmodSync(join(bin, "rustc"), 0o700);
  const environment = { ...process.env };
  delete environment.RUSTUP_TOOLCHAIN;
  environment.FIXTURE_NESTED_VERSION = nestedVersion;
  environment.FIXTURE_NESTED_REASON = nestedReason;
  environment.PATH = `${bin}:${environment.PATH}`;
  environment.TRACE = trace;
  try {
    const result = spawnSync("bash", ["-c", nestedRustToolchainStepBody()], {
      cwd: root,
      encoding: "utf8",
      env: environment,
    });
    return {
      ...result,
      root,
      trace: readFileSync(trace, "utf8").trim().split("\n"),
    };
  } finally {
    rmSync(root, { force: true, recursive: true });
  }
}

// prove the root control and nested pin through the exact command sequence
test("workflow checks the fixed control before the nested Rust pin", () => {
  const result = runNestedRustToolchainStep("1.97.1");
  assert.equal(result.status, 0, result.stderr);
  assert.deepEqual(result.trace, [
    `rustup:${result.root}:toolchain install 1.85.1 --profile minimal`,
    `rustup:${result.root}:default 1.85.1`,
    `rustup:${result.root}:show active-toolchain`,
    `rustc:${result.root}:-Vv`,
    `rustup:${result.root}:toolchain install 1.97.1 --profile minimal --component clippy --component rustfmt`,
    `rustup:${result.root}/rust:show active-toolchain`,
    `rustc:${result.root}/rust:-Vv`,
  ]);
  assert.match(
    result.stdout,
    /repository root:[\s\S]+release: 1\.85\.1\nnested Rust workspace:[\s\S]+release: 1\.97\.1/u,
  );
});

// reject a nested directory that still resolves the root control compiler
test("workflow fails when the nested compiler remains on the control", () => {
  const result = runNestedRustToolchainStep("1.85.1");
  assert.notEqual(result.status, 0);
  assert.equal(
    result.trace.at(-1),
    `rustc:${result.root}/rust:-Vv`,
  );
});

// reject an equivalent version selected by a directory override instead of the pin file
test("workflow requires the nested compiler to come from the pin file", () => {
  const result = runNestedRustToolchainStep("1.97.1", "directory");
  assert.notEqual(result.status, 0);
  assert.match(result.stdout, /directory override for.+\n[\s\S]+release: 1\.97\.1/u);
});
