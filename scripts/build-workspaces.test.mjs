import assert from "node:assert/strict";
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import {
  buildWorkspaces,
  copyWorkspaceRuntimeAssets,
  discoverWorkspaces,
  orderWorkspaces,
} from "./build-workspaces.mjs";

const workspaces = [
  { name: "@weather/api", path: "apps/api", dependencies: ["@weather/database", "@weather/domain"] },
  { name: "@weather/database", path: "packages/database", dependencies: ["@weather/domain"] },
  { name: "@weather/domain", path: "packages/domain", dependencies: [] },
  { name: "@weather/web", path: "apps/web", dependencies: [] },
];

// preserve a single dependency-ordered compilation
test("workspace build visits every package once after its dependencies", async () => {
  const built = [];

  await buildWorkspaces(workspaces, async (workspace) => {
    built.push(workspace.name);
  });

  assert.deepEqual(built, [
    "@weather/domain",
    "@weather/database",
    "@weather/api",
    "@weather/web",
  ]);
  assert.equal(new Set(built).size, workspaces.length);
});

// stop compilation at the first failure
test("workspace build does not continue after a compiler failure", async () => {
  const built = [];

  await assert.rejects(
    buildWorkspaces(workspaces, async (workspace) => {
      built.push(workspace.name);

      // simulate one failing dependency
      if (workspace.name === "@weather/database") {
        throw new Error("typescript failed");
      }
    }),
    /typescript failed/u,
  );
  assert.deepEqual(built, ["@weather/domain", "@weather/database"]);
});

// retain the reviewed public module in clean root builds
test("workspace build copies the web runtime contract after compilation", async () => {
  const root = await mkdtemp(join(tmpdir(), "weather-workspace-assets-"));
  try {
    await mkdir(join(root, "apps/web/src"), { recursive: true });
    await mkdir(join(root, "apps/web/dist"), { recursive: true });
    await writeFile(
      join(root, "apps/web/src/adjustment-maintenance-contract.mjs"),
      "export const contract = true;\n",
    );
    copyWorkspaceRuntimeAssets(workspaces.at(-1), root);
    assert.equal(
      await readFile(join(root, "apps/web/dist/adjustment-maintenance-contract.mjs"), "utf8"),
      "export const contract = true;\n",
    );
  } finally {
    await rm(root, { force: true, recursive: true });
  }
});

// reject malformed workspace graphs
test("workspace build rejects cycles, duplicates, and unknown local dependencies", () => {
  assert.throws(
    () => orderWorkspaces([
      { name: "@weather/a", path: "packages/a", dependencies: ["@weather/b"] },
      { name: "@weather/b", path: "packages/b", dependencies: ["@weather/a"] },
    ]),
    /cycle/u,
  );
  assert.throws(
    () => orderWorkspaces([workspaces[0], workspaces[0]]),
    /duplicate/u,
  );
  assert.throws(
    () => orderWorkspaces([{ name: "@weather/a", path: "packages/a", dependencies: ["@weather/missing"] }]),
    /unknown workspace/u,
  );
});

// verify real package metadata still defines the intended graph
test("workspace discovery includes all eight production packages", async () => {
  const discovered = await discoverWorkspaces(new URL("..", import.meta.url));
  const ordered = orderWorkspaces(discovered);

  assert.equal(ordered.length, 8);
  assert.deepEqual(new Set(ordered.map((workspace) => workspace.name)), new Set([
    "@weather/api",
    "@weather/contracts",
    "@weather/web",
    "@weather/worker",
    "@weather/database",
    "@weather/domain",
    "@weather/forecast-adjustment",
    "@weather/providers",
  ]));
});

// keep generated Rust contracts ahead of whole-repository compilation
test("root build checks Rust contract drift before compiling workspaces", async () => {
  const packageJson = JSON.parse(await readFile(new URL("../package.json", import.meta.url), "utf8"));

  assert.match(
    packageJson.scripts.build,
    /^node scripts\/generate-rust-contracts\.mjs --check && node scripts\/build-workspaces\.mjs$/u,
  );
  assert.match(
    packageJson.scripts.lint,
    / && node scripts\/generate-rust-contracts\.mjs --check$/u,
  );
  assert.match(
    packageJson.scripts.typecheck,
    /^node scripts\/generate-rust-contracts\.mjs --check && npm run typecheck --workspaces$/u,
  );
  assert.equal(
    packageJson.scripts["check:fast"],
    "npm run lint && npm run build && npm run test:fast:compiled",
  );
});

// require the same gate when compiling a contract workspace directly
test("standalone contract consumers also check generated Rust contract drift", async () => {
  // inspect each workspace which owns or imports the canonical contracts
  for (const path of ["apps/api", "packages/contracts"]) {
    const manifest = JSON.parse(await readFile(new URL(`../${path}/package.json`, import.meta.url), "utf8"));
    assert.match(manifest.scripts.build, /^node \.\.\/\.\.\/scripts\/generate-rust-contracts\.mjs --check && /u);
    assert.match(manifest.scripts.typecheck, /^node \.\.\/\.\.\/scripts\/generate-rust-contracts\.mjs --check && /u);
  }
});

// keep contract generation inputs confined to the Docker build stage
test("Docker build checks generated contracts without retaining them at runtime", async () => {
  const dockerfile = await readFile(new URL("../Dockerfile", import.meta.url), "utf8");
  const dependencyStage = dockerfile.slice(
    dockerfile.indexOf("FROM ${NODE_IMAGE} AS dependencies"),
    dockerfile.indexOf("FROM dependencies AS build"),
  );
  const buildStage = dockerfile.slice(
    dockerfile.indexOf("FROM dependencies AS build"),
    dockerfile.indexOf("FROM scratch AS image-check-files"),
  );
  const productionDependencyStage = dockerfile.slice(
    dockerfile.indexOf("FROM dependencies AS production-dependencies"),
    dockerfile.indexOf("FROM ${NODE_IMAGE} AS runtime"),
  );
  const runtimeStages = dockerfile.slice(dockerfile.indexOf("FROM ${NODE_IMAGE} AS runtime"));

  assert.match(
    dependencyStage,
    /COPY packages\/contracts\/package\.json packages\/contracts\/package\.json[\s\S]*RUN npm ci --ignore-scripts/u,
  );
  assert.match(buildStage, /COPY config\/rust-contract-roots\.json config\//u);
  assert.match(buildStage, /COPY scripts\/generate-rust-contracts\.mjs scripts\//u);
  assert.match(buildStage, /COPY scripts\/lib\/forecast-observation-rust\.mjs scripts\/lib\//u);
  assert.match(
    buildStage,
    /COPY rust\/crates\/weather-contracts\/generated\.contracts\.json rust\/crates\/weather-contracts\/retained-rain-hurdle-wind\.json rust\/crates\/weather-contracts\//u,
  );
  assert.match(
    buildStage,
    /COPY rust\/crates\/weather-contracts\/src\/generated\.rs rust\/crates\/weather-contracts\/src\//u,
  );
  assert.ok(buildStage.indexOf("scripts/generate-rust-contracts.mjs") < buildStage.indexOf("RUN npm run build"));
  assert.ok(buildStage.indexOf("generated.contracts.json") < buildStage.indexOf("RUN npm run build"));
  assert.match(productionDependencyStage, /unlink node_modules\/@weather\/contracts/u);
  assert.doesNotMatch(runtimeStages, /rust\/crates\/weather-contracts/u);
  assert.doesNotMatch(runtimeStages, /packages\/contracts/u);
  assert.doesNotMatch(runtimeStages, /generate-rust-contracts\.mjs|forecast-observation-rust\.mjs|rust-contract-roots\.json/u);
});
