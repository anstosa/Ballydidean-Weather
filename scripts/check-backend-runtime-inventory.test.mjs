import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import {
  lstat,
  mkdir,
  mkdtemp,
  readFile,
  readdir,
  readlink,
  rm,
  symlink,
  unlink,
  writeFile,
} from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

import {
  GENERATIONS,
  PHASES,
  discoverRuntimeInventory,
  parseRuntimeInventoryArguments,
  validateRuntimeInventory,
} from "./check-backend-runtime-inventory.mjs";

const MANIFEST_PATH = "config/backend-runtime-inventory.json";
const RUNTIME_MAP_PATH = "config/backend-runtime-live-map.json";
const REPOSITORY_ROOT = fileURLToPath(new URL("..", import.meta.url));
const RUNTIME_LIVE_MAP_VERSION = "weather-backend-runtime-live-map/v1";
const EXPECTED_KINDS = [
  "compose-command",
  "compose-probe",
  "docker-command",
  "docker-entrypoint",
  "forced-remote",
  "import",
  "installer-allowlist",
  "package-script",
  "private-command",
  "shell-invocation",
  "systemd-service",
  "systemd-timer",
];

// load the independently tested runtime-map API
async function runtimeLiveMapApi() {
  const module = await import("./check-backend-runtime-inventory.mjs");
  assert.equal(typeof module.validateRuntimeLiveMap, "function");
  assert.equal(typeof module.deriveRuntimeLiveMap, "function");
  assert.equal(module.RUNTIME_LIVE_MAP_SCHEMA_VERSION, RUNTIME_LIVE_MAP_VERSION);
  return module;
}

// build one deterministic source-owned context identity
function runtimeContextId(profile, rootEntryId, instanceKind, instanceValue) {
  return `runtime-context:${profile}:${rootEntryId}#${instanceKind}=${instanceValue}`;
}

// build one deterministic transitive artifact binding identity
function runtimeBinding(contextId, entryId, artifactIds, rootEntryId = entryId) {
  const exactArtifactIds = Array.isArray(artifactIds) ? [...artifactIds] : [artifactIds];
  return {
    artifactIds: exactArtifactIds.sort(),
    contextId,
    entryId,
    id: `runtime-binding:${contextId}:${entryId}`,
    rootEntryId,
  };
}

// hash one exact byte sequence
function sha256(value) {
  return createHash("sha256").update(value).digest("hex");
}

// write one repository-relative fixture tree
async function writeTree(rootDir, files) {
  // create every declared fixture source
  for (const [relativePath, contents] of Object.entries(files)) {
    const absolutePath = join(rootDir, relativePath);
    await mkdir(dirname(absolutePath), { recursive: true });
    await writeFile(absolutePath, contents);
  }
}

// write one deterministic JSON document
async function writeJson(rootDir, relativePath, value) {
  const absolutePath = join(rootDir, relativePath);
  await mkdir(dirname(absolutePath), { recursive: true });
  await writeFile(absolutePath, `${JSON.stringify(value, null, 2)}\n`);
}

// write one exact fixture runtime map
async function writeFixtureRuntimeMap(rootDir, inventory) {
  const { deriveRuntimeLiveMap } = await runtimeLiveMapApi();
  const runtimeMap = deriveRuntimeLiveMap({ entries: inventory.entries });
  await writeJson(
    rootDir,
    RUNTIME_MAP_PATH,
    runtimeMap,
  );
  return runtimeMap;
}

// read one checked repository json document
async function readRepositoryJson(relativePath) {
  return JSON.parse(await readFile(join(REPOSITORY_ROOT, relativePath), "utf8"));
}

// require the finalized discovery envelope
async function discoverFixture(rootDir) {
  const result = await discoverRuntimeInventory({ rootDir });
  assert.deepEqual(result.diagnostics, []);
  return result.entries;
}

// retain one explicit generation mapping
function generationDeclaration(reachable, observation, replacement) {
  return {
    reachable,
    command: observation.command ?? null,
    probe: observation.probe ?? null,
    replacement,
  };
}

// classify one representative discovered row honestly
function classificationFor(observation) {
  const applicationKinds = new Set([
    "compose-command",
    "compose-probe",
    "docker-command",
    "docker-entrypoint",
    "shell-invocation",
    "systemd-service",
    "systemd-timer",
  ]);

  // keep app entrypoints and probes in the application boundary
  if (applicationKinds.has(observation.kind)) {
    return "application";
  }

  // keep transitive application imports in the application boundary
  if (observation.kind === "import") {
    return "application";
  }
  return "release-control";
}

// retain exact sorted identifiers
function sortedIds(entries, generation) {
  const ids = [];

  // collect only declared reachable entries
  for (const entry of entries) {
    // preserve the manifest generation truth
    if (entry.generations[generation].reachable) {
      ids.push(entry.id);
    }
  }
  return ids.sort();
}

// select one reviewed annotation record
function reviewedAnnotations(entry) {
  return {
    owner: entry.owner,
    parityTest: entry.parityTest,
    accountingScope: entry.accountingScope,
    generations: entry.generations,
  };
}

// hydrate reviewed metadata around structural discovery
function buildManifest(discovered) {
  const entries = [];

  // bind every discovered row to explicit ownership and generations
  for (const observation of discovered) {
    const rustReachable = observation.runtime === "rust";
    const nodeReachable = !rustReachable;
    entries.push({
      id: observation.id,
      kind: observation.kind,
      sourcePath: observation.sourcePath,
      installedPaths: observation.installedPaths,
      sourceDigest: observation.sourceDigest,
      entryDigest: observation.entryDigest,
      parents: observation.parents,
      command: observation.command,
      probe: observation.probe,
      classification: observation.classification ?? classificationFor(observation),
      owner: "weather-backend",
      parityTest: "test/runtime-parity.test.mjs",
      accountingScope: "fixture-cgroup",
      runtime: observation.runtime,
      invokedRuntimes: observation.invokedRuntimes,
      generations: {
        "node-v1": generationDeclaration(
          nodeReachable,
          observation,
          nodeReachable ? "rust-v1 fixture replacement" : null,
        ),
        "node-v1-rollback": generationDeclaration(
          nodeReachable,
          observation,
          nodeReachable ? "rust-v1 fixture replacement" : null,
        ),
        "rust-v1": generationDeclaration(rustReachable, observation, null),
        mixed: generationDeclaration(true, observation, null),
      },
    });
  }

  const mixedIds = sortedIds(entries, "mixed");
  return {
    schemaVersion: "backend-runtime-inventory/v1",
    scannerVersion: 1,
    limitations: [
      "bounded fixture parsers reject computed execution and import mappings",
      "static source presence does not establish installed enabled or active state",
    ],
    generationPolicies: {
      "node-v1": {
        reachableEntryIds: sortedIds(entries, "node-v1"),
      },
      "node-v1-rollback": {
        reachableEntryIds: sortedIds(entries, "node-v1-rollback"),
      },
      "rust-v1": {
        reachableEntryIds: sortedIds(entries, "rust-v1"),
      },
      mixed: {
        reachableEntryIds: [...mixedIds],
        allowedUnionEntryIds: [...mixedIds],
      },
    },
    entries,
  };
}

// create one representative static repository fixture
async function createFixture() {
  const rootDir = await mkdtemp(join(tmpdir(), "weather-runtime-inventory-"));
  const files = {
    "package.json": `${JSON.stringify({
      name: "runtime-inventory-fixture",
      private: true,
      scripts: {
        archive: "node scripts/research/archive.mjs",
      },
    }, null, 2)}\n`,
    "deploy/compose.yaml": `services:\n  api:\n    command: [node, apps/api/main.mjs]\n    healthcheck:\n      test: [CMD, node, apps/api/health.mjs]\n  rust-api:\n    command: [/opt/weather/bin/weather-api]\n`,
    Dockerfile: `FROM scratch AS server\nENTRYPOINT ["node"]\nCMD ["apps/api/main.mjs"]\n\nFROM scratch AS rust-server\nENTRYPOINT ["/opt/weather/bin/weather-api"]\nCMD ["serve"]\n`,
    "apps/api/main.mjs": `import { applicationHelper } from "../../scripts/research/application-helper.mjs";\nexport const main = applicationHelper;\n`,
    "apps/api/health.mjs": `export const healthy = true;\n`,
    "apps/worker/worker.mjs": `export const worker = true;\n`,
    "test/runtime-parity.test.mjs": `export const parity = true;\n`,
    "scripts/research/application-helper.mjs": `import { leaf } from "./leaf.mjs";\nexport const applicationHelper = leaf;\n`,
    "scripts/research/leaf.mjs": `export const leaf = "fixture";\n`,
    "scripts/research/archive.mjs": `export const archive = true;\n`,
    "scripts/research/evidence.mjs": `export const evidence = true;\n`,
    "scripts/research/controller.mjs": `export const controller = true;\n`,
    "deploy/scripts/run-worker.sh": `#!/usr/bin/env bash\nexec node apps/worker/worker.mjs\n`,
    "deploy/scripts/adjustment-evidence.sh": `#!/usr/bin/env bash\nexec node scripts/research/evidence.mjs\n`,
    "deploy/scripts/remote-ops.sh": `#!/usr/bin/env bash\ncase "\${SSH_ORIGINAL_COMMAND:-}" in\n  status) exec node deploy/scripts/status.mjs ;;\n  backup) exec /opt/weather/bin/weather-backup ;;\nesac\n`,
    "deploy/scripts/status.mjs": `export const status = true;\n`,
    "scripts/install-adjustment-maintenance-runner.sh": `#!/usr/bin/env bash\nreadonly -a RUNNER_FILES=(\n  "scripts/research/archive.mjs"\n  "scripts/research/controller.mjs"\n)\n`,
    "scripts/systemd/system/weather-adjustment.service": `[Unit]\nDescription=fixture adjustment runner\n\n[Service]\nType=simple\nExecStart=/usr/bin/node /opt/weather/current/scripts/research/controller.mjs\n`,
    "scripts/systemd/system/weather-adjustment.timer": `[Unit]\nDescription=fixture adjustment timer\n\n[Timer]\nOnCalendar=hourly\nUnit=weather-adjustment.service\n`,
  };
  await writeTree(rootDir, files);
  const discovered = await discoverFixture(rootDir);
  const manifest = buildManifest(discovered);
  await writeJson(rootDir, MANIFEST_PATH, manifest);
  const runtimeMap = await writeFixtureRuntimeMap(rootDir, manifest);
  return { rootDir, manifest, discovered, runtimeMap };
}

// create executable roots without compose docker or systemd support
async function createExecutionRootFixture() {
  const rootDir = await mkdtemp(join(tmpdir(), "weather-runtime-roots-"));
  await writeTree(rootDir, {
    "package.json": `${JSON.stringify({
      name: "runtime-root-fixture",
      private: true,
      scripts: {
        job: "node scripts/job.mjs",
        build: "node scripts/build-tool.mjs",
      },
    }, null, 2)}\n`,
    "scripts/job.mjs": `import "./package-child.mjs";\nexport const job = true;\n`,
    "scripts/package-child.mjs": `export const child = "package";\n`,
    "scripts/build-tool.mjs": `export const build = true;\n`,
    "scripts/shell-job.mjs": `import "./shell-child.mjs";\nexport const job = true;\n`,
    "scripts/shell-child.mjs": `export const child = "shell";\n`,
    "scripts/private-job.mjs": `import "./private-child.mjs";\nexport const job = true;\n`,
    "scripts/private-child.mjs": `export const child = "private";\n`,
    "scripts/private-env-job.mjs": `import "./private-env-child.mjs";\nexport const job = true;\n`,
    "scripts/private-env-child.mjs": `export const child = "private-env";\n`,
    "scripts/forced-job.mjs": `import "./forced-child.mjs";\nexport const job = true;\n`,
    "scripts/forced-child.mjs": `export const child = "forced";\n`,
    "scripts/installer-job.mjs": `import "./installer-child.mjs";\nexport const job = true;\n`,
    "scripts/installer-child.mjs": `export const child = "installer";\n`,
    "deploy/scripts/run.sh": `#!/usr/bin/env bash\nexec /usr/bin/node scripts/shell-job.mjs\n`,
    "deploy/scripts/forecast-adjustment-evidence.sh": `#!/usr/bin/env bash\nexec node scripts/private-job.mjs\n`,
    "deploy/scripts/adjustment-model.sh": `#!/usr/bin/env bash\nexec \${WEATHER_NODE} scripts/private-env-job.mjs\n`,
    "deploy/scripts/remote-ops.sh": `#!/usr/bin/env bash\ncase "\${SSH_ORIGINAL_COMMAND:-}" in\n  backfill) exec /usr/bin/node scripts/forced-job.mjs ;;\nesac\n`,
    "scripts/install-adjustment-maintenance-runner.sh": `#!/usr/bin/env bash\nreadonly -a RUNNER_FILES=(\n  "scripts/installer-job.mjs"\n)\n`,
  });
  const discovered = await discoverFixture(rootDir);
  const manifest = buildManifest(discovered);
  await writeJson(rootDir, MANIFEST_PATH, manifest);
  const runtimeMap = await writeFixtureRuntimeMap(rootDir, manifest);
  return { rootDir, manifest, discovered, runtimeMap };
}

// create one fixed package-metadata runtime-map fixture
async function createPackageMapFixture() {
  const rootDir = await mkdtemp(join(tmpdir(), "weather-runtime-package-map-"));
  await writeTree(rootDir, {
    "package.json": `${JSON.stringify({
      name: "runtime-package-map-fixture",
      private: true,
      scripts: {
        build: "node scripts/build.mjs",
        serve: "node scripts/serve.mjs",
        test: "node test/oracle.mjs",
      },
    }, null, 2)}\n`,
    "scripts/build.mjs": `export const build = true;\n`,
    "scripts/serve.mjs": `export const serve = true;\n`,
    "test/oracle.mjs": `export const oracle = true;\n`,
  });
  const discovered = await discoverFixture(rootDir);
  const inventory = buildManifest(discovered);
  const scriptNames = ["build", "serve", "test"];
  const artifacts = [];
  const bindings = [];
  const contexts = [];

  // hand-author every package root and installed metadata artifact
  for (const scriptName of scriptNames) {
    const rootEntryId = `package-script:package.json#${scriptName}`;
    const artifactId = `artifact:package-metadata:${rootEntryId}#server`;
    artifacts.push({
      authority: {
        kind: "dockerfile-copy",
        sourcePath: "Dockerfile",
      },
      executableClosure: false,
      id: artifactId,
      imageTarget: "server",
      kind: "package-metadata",
      path: "/opt/weather/package.json",
      scriptDefinitionOnly: true,
      scriptNames: [scriptName],
      sourceEntryId: rootEntryId,
    });
    const context = {
      accountPolicy: null,
      accountingScopes: ["fixture-cgroup"],
      activationSource: "never-active-alternative",
      artifactIds: [artifactId],
      hostRole: "blueberry",
      id: runtimeContextId(
        "blueberry-compose",
        rootEntryId,
        "image",
        "server",
      ),
      imageTarget: "server",
      profile: "blueberry-compose",
      rootEntryId,
      rootKind: "package-script",
      service: null,
      unit: null,
    };
    contexts.push(context);
    bindings.push(runtimeBinding(context.id, rootEntryId, artifactId));

    // retain the serve root's source-only executable closure
    if (scriptName === "serve") {
      bindings.push(runtimeBinding(
        context.id,
        "import:scripts/serve.mjs",
        [],
        rootEntryId,
      ));
    }
  }
  return {
    rootDir,
    discovered,
    inventory,
    runtimeMap: {
      artifacts,
      bindings,
      contexts,
      contractVersion: RUNTIME_LIVE_MAP_VERSION,
      inventorySchemaVersion: inventory.schemaVersion,
    },
  };
}

// isolate one package-map fixture lifecycle
async function withPackageMapFixture(run) {
  const fixture = await createPackageMapFixture();
  try {
    return await run(fixture);
  } finally {
    await rm(fixture.rootDir, { force: true, recursive: true });
  }
}

// build one fixed cross-context runtime-map fixture
function contextMapFixture() {
  const apiId = "compose-command:deploy/compose.yaml#api";
  const workerId = "compose-command:deploy/compose.yaml#worker";
  const imageId = "docker-command:Dockerfile#server";
  const unitId = "systemd-service:scripts/systemd/system/weather-compose.service";
  const runnerId = "installer-allowlist:scripts/install-adjustment-maintenance-runner.sh#RUNNER_FILES:scripts/runner.mjs";
  const hostFileId = "forced-remote:deploy/scripts/remote-ops.sh";
  const runnerAccount = {
    accountName: "ubuntu",
    homeTemplate: "${HOME}",
    resolution: "nss-passwd-exact",
  };
  const entries = [
    {
      id: apiId,
      kind: "compose-command",
      sourcePath: "deploy/compose.yaml",
      installedPaths: ["/opt/weather/apps/api/dist/main.js"],
      parents: [],
      accountingScope: "api-cgroup",
    },
    {
      id: workerId,
      kind: "compose-command",
      sourcePath: "deploy/compose.yaml",
      installedPaths: ["/opt/weather/apps/worker/dist/main.js"],
      parents: [],
      accountingScope: "worker-cgroup",
    },
    {
      id: imageId,
      kind: "docker-command",
      sourcePath: "Dockerfile",
      installedPaths: ["image://server"],
      parents: [],
      accountingScope: "installed-alternative",
    },
    {
      id: unitId,
      kind: "systemd-service",
      sourcePath: "scripts/systemd/system/weather-compose.service",
      installedPaths: ["/etc/systemd/system/weather-compose.service"],
      parents: [],
      accountingScope: "operator",
    },
    {
      id: runnerId,
      kind: "installer-allowlist",
      sourcePath: "scripts/install-adjustment-maintenance-runner.sh",
      installedPaths: [
        "${HOME}/.weather/adjustment-maintenance/runner/current/scripts/runner.mjs",
      ],
      parents: [],
      accountingScope: "runner-cgroup",
    },
    {
      id: hostFileId,
      kind: "forced-remote",
      sourcePath: "deploy/scripts/remote-ops.sh",
      installedPaths: ["/usr/local/bin/weather-remote-ops"],
      parents: [],
      accountingScope: "operator",
    },
  ];
  const artifacts = [
    {
      authority: { kind: "dockerfile-copy", sourcePath: "Dockerfile" },
      contentRole: "runtime-bytes",
      expectedMode: "0644",
      expectedType: "regular-file",
      id: `artifact:container-file:${apiId}#0`,
      imageTarget: "server",
      kind: "container-file",
      ownerPolicy: "runtime-user",
      path: "/opt/weather/apps/api/dist/main.js",
      sourceEntryId: apiId,
    },
    {
      authority: { kind: "dockerfile-copy", sourcePath: "Dockerfile" },
      contentRole: "runtime-bytes",
      expectedMode: "0644",
      expectedType: "regular-file",
      id: `artifact:container-file:${workerId}#0`,
      imageTarget: "server",
      kind: "container-file",
      ownerPolicy: "runtime-user",
      path: "/opt/weather/apps/worker/dist/main.js",
      sourceEntryId: workerId,
    },
    {
      authority: { kind: "dockerfile-stage", sourcePath: "Dockerfile" },
      declaredCmd: ["node", "apps/api/dist/main.js"],
      declaredEntrypoint: null,
      id: `artifact:image-config:${imageId}#0`,
      imageTarget: "server",
      kind: "image-config",
      sourceEntryId: imageId,
    },
    {
      authority: {
        kind: "systemd-installer",
        sourcePath: "scripts/install-schema-preserving-release.sh",
      },
      expectedMode: "0644",
      id: `artifact:systemd-unit:${unitId}#0`,
      kind: "systemd-unit",
      ownerPolicy: "root",
      path: "/etc/systemd/system/weather-compose.service",
      sourceEntryId: unitId,
      unit: "weather-compose.service",
    },
    {
      accountPolicy: runnerAccount,
      authority: {
        kind: "runner-installer",
        sourcePath: "scripts/install-adjustment-maintenance-runner.sh",
      },
      declaredPath: "${HOME}/.weather/adjustment-maintenance/runner/current/scripts/runner.mjs",
      expectedMode: "0400",
      expectedType: "regular-file",
      id: `artifact:runner-file:${runnerId}#0`,
      kind: "runner-file",
      ownerPolicy: "nss-account",
      resolvedRootPolicy: "nss-account-home",
      sourceEntryId: runnerId,
    },
    {
      authority: {
        kind: "inventory-installed-path",
        sourcePath: "deploy/scripts/remote-ops.sh",
      },
      contentRole: "operator-control",
      expectedMode: "0755",
      expectedType: "regular-file",
      id: `artifact:host-file:${hostFileId}#0`,
      kind: "host-file",
      ownerPolicy: "root",
      path: "/usr/local/bin/weather-remote-ops",
      sourceEntryId: hostFileId,
    },
  ];
  const contexts = [
    {
      accountPolicy: null,
      accountingScopes: ["api-cgroup"],
      activationSource: "container-inspect",
      artifactIds: [`artifact:container-file:${apiId}#0`],
      hostRole: "blueberry",
      id: runtimeContextId("blueberry-compose", apiId, "service", "api"),
      imageTarget: "server",
      profile: "blueberry-compose",
      rootEntryId: apiId,
      rootKind: "compose-service",
      service: "api",
      unit: null,
    },
    {
      accountPolicy: null,
      accountingScopes: ["worker-cgroup"],
      activationSource: "container-inspect",
      artifactIds: [`artifact:container-file:${workerId}#0`],
      hostRole: "blueberry",
      id: runtimeContextId("blueberry-compose", workerId, "service", "worker"),
      imageTarget: "server",
      profile: "blueberry-compose",
      rootEntryId: workerId,
      rootKind: "compose-service",
      service: "worker",
      unit: null,
    },
    {
      accountPolicy: null,
      accountingScopes: ["installed-alternative"],
      activationSource: "never-active-alternative",
      artifactIds: [`artifact:image-config:${imageId}#0`],
      hostRole: "blueberry",
      id: runtimeContextId("blueberry-compose", imageId, "image", "server"),
      imageTarget: "server",
      profile: "blueberry-compose",
      rootEntryId: imageId,
      rootKind: "docker-image-default",
      service: null,
      unit: null,
    },
    {
      accountPolicy: null,
      accountingScopes: ["operator"],
      activationSource: "systemd-show",
      artifactIds: [`artifact:systemd-unit:${unitId}#0`],
      hostRole: "workstation",
      id: runtimeContextId(
        "workstation-runner",
        unitId,
        "unit",
        "weather-compose.service",
      ),
      imageTarget: null,
      profile: "workstation-runner",
      rootEntryId: unitId,
      rootKind: "host-systemd",
      service: null,
      unit: "weather-compose.service",
    },
    {
      accountPolicy: runnerAccount,
      accountingScopes: ["runner-cgroup"],
      activationSource: "systemd-show",
      artifactIds: [`artifact:runner-file:${runnerId}#0`],
      hostRole: "workstation",
      id: runtimeContextId(
        "workstation-runner",
        runnerId,
        "account",
        "ubuntu",
      ),
      imageTarget: null,
      profile: "workstation-runner",
      rootEntryId: runnerId,
      rootKind: "runner-package",
      service: null,
      unit: null,
    },
    {
      accountPolicy: null,
      accountingScopes: ["operator"],
      activationSource: "process-identity",
      artifactIds: [`artifact:host-file:${hostFileId}#0`],
      hostRole: "blueberry",
      id: runtimeContextId("blueberry-compose", hostFileId, "host", "blueberry"),
      imageTarget: null,
      profile: "blueberry-compose",
      rootEntryId: hostFileId,
      rootKind: "host-file",
      service: null,
      unit: null,
    },
  ];
  const bindings = [];

  // bind every fixed root artifact to its exact runtime instance
  for (const context of contexts) {
    // bind every artifact declared for this context
    for (const artifactId of context.artifactIds) {
      bindings.push(runtimeBinding(
        context.id,
        context.rootEntryId,
        artifactId,
      ));
    }
  }
  return {
    inventory: {
      entries,
      schemaVersion: "backend-runtime-inventory/v1",
    },
    runtimeMap: {
      artifacts,
      bindings,
      contexts,
      contractVersion: RUNTIME_LIVE_MAP_VERSION,
      inventorySchemaVersion: "backend-runtime-inventory/v1",
    },
  };
}

// isolate one executable-root fixture lifecycle
async function withExecutionRootFixture(run) {
  const fixture = await createExecutionRootFixture();
  try {
    return await run(fixture);
  } finally {
    await rm(fixture.rootDir, { force: true, recursive: true });
  }
}

// create one compose-only application graph
async function createClassificationFixture() {
  const rootDir = await mkdtemp(join(tmpdir(), "weather-runtime-classification-"));
  await writeTree(rootDir, {
    "package.json": `${JSON.stringify({
      name: "runtime-classification-fixture",
      private: true,
    }, null, 2)}\n`,
    "deploy/compose.yaml": `services:\n  api:\n    command: [node, apps/api/main.mjs]\n`,
    "apps/api/main.mjs": `import "./child.mjs";\nexport const main = true;\n`,
    "apps/api/child.mjs": `import "./leaf.mjs";\nexport const child = true;\n`,
    "apps/api/leaf.mjs": `export const leaf = true;\n`,
    "test/runtime-parity.test.mjs": `export const parity = true;\n`,
  });
  const discovered = await discoverFixture(rootDir);
  const manifest = buildManifest(discovered);
  await writeJson(rootDir, MANIFEST_PATH, manifest);
  const runtimeMap = await writeFixtureRuntimeMap(rootDir, manifest);
  return { rootDir, manifest, discovered, runtimeMap };
}

// isolate one source-classification fixture lifecycle
async function withClassificationFixture(run) {
  const fixture = await createClassificationFixture();
  try {
    return await run(fixture);
  } finally {
    await rm(fixture.rootDir, { force: true, recursive: true });
  }
}

// isolate one minimal unsupported-root fixture
async function withBareFixture(files, run) {
  const rootDir = await mkdtemp(join(tmpdir(), "weather-runtime-unsupported-"));
  try {
    await writeTree(rootDir, files);
    return await run(rootDir);
  } finally {
    await rm(rootDir, { force: true, recursive: true });
  }
}

// isolate one disposable fixture lifecycle
async function withFixture(run) {
  const fixture = await createFixture();
  try {
    return await run(fixture);
  } finally {
    await rm(fixture.rootDir, { force: true, recursive: true });
  }
}

// find one required manifest entry by kind and optional source path
function requireEntry(manifest, kind, sourcePath = null) {
  // inspect each reviewed row
  for (const entry of manifest.entries) {
    // match the requested structural identity
    if (entry.kind === kind && (sourcePath === null || entry.sourcePath === sourcePath)) {
      return entry;
    }
  }
  assert.fail(`missing ${kind} entry${sourcePath === null ? "" : ` for ${sourcePath}`}`);
}

// find one required row by stable identity
function requireEntryId(entries, entryId) {
  // inspect every structural row
  for (const entry of entries) {
    // retain the exact requested identity
    if (entry.id === entryId) {
      return entry;
    }
  }
  assert.fail(`missing inventory entry ${entryId}`);
}

// find one required runtime-map artifact
function requireMapArtifact(runtimeMap, predicate, label) {
  const artifact = runtimeMap.artifacts.find(
    // select the requested artifact semantics
    (candidate) => predicate(candidate),
  );
  assert.ok(artifact, `missing runtime artifact ${label}`);
  return artifact;
}

// find one required runtime-map context
function requireMapContext(runtimeMap, rootEntryId) {
  const context = runtimeMap.contexts.find(
    // select the exact source-owned root
    (candidate) => candidate.rootEntryId === rootEntryId,
  );
  assert.ok(context, `missing runtime context ${rootEntryId}`);
  return context;
}

// close one root over typed import parents
function transitiveClosureIds(entries, rootId) {
  const closure = new Set([rootId]);
  let changed = true;

  // continue until every descendant is retained
  while (changed) {
    changed = false;

    // inspect every possible child
    for (const entry of entries) {
      // skip established descendants
      if (closure.has(entry.id)) {
        continue;
      }

      // inspect every typed parent edge
      for (const parent of entry.parents) {
        // retain children of the current closure
        if (closure.has(parent.id)) {
          closure.add(entry.id);
          changed = true;
          break;
        }
      }
    }
  }
  return closure;
}

// rebuild exact finite generation policies
function rebuildGenerationPolicies(manifest) {
  const mixedIds = sortedIds(manifest.entries, "mixed");
  manifest.generationPolicies = {
    "node-v1": {
      reachableEntryIds: sortedIds(manifest.entries, "node-v1"),
    },
    "node-v1-rollback": {
      reachableEntryIds: sortedIds(manifest.entries, "node-v1-rollback"),
    },
    "rust-v1": {
      reachableEntryIds: sortedIds(manifest.entries, "rust-v1"),
    },
    mixed: {
      reachableEntryIds: [...mixedIds],
      allowedUnionEntryIds: [...mixedIds],
    },
  };
}

// collect stable diagnostic codes
function diagnosticCodes(result) {
  const codes = [];

  // retain reported order for determinism assertions
  for (const diagnostic of result.diagnostics) {
    codes.push(diagnostic.code);
  }
  return codes;
}

// collect stable discovered identities
function inventoryIds(entries) {
  const ids = new Set();

  // retain every structural row identity
  for (const entry of entries) {
    ids.add(entry.id);
  }
  return ids;
}

// accept exact discovery or an explicit bounded-parser refusal
function assertDiscoveredOrUnsupported(discovery, expectedKind) {
  let discoveredKind = false;

  // inspect every discovered structural row
  for (const entry of discovery.entries) {
    // retain the expected syntax class
    if (entry.kind === expectedKind) {
      discoveredKind = true;
    }
  }
  assert.ok(
    discoveredKind || diagnosticCodes(discovery).includes("discovery.unsupported"),
    `expected ${expectedKind} discovery or explicit unsupported diagnostic`,
  );
}

// require one diagnostic behavior
function assertDiagnostic(result, code) {
  assert.equal(result.ok, false);
  assert.ok(
    diagnosticCodes(result).includes(code),
    `expected ${code}, received ${JSON.stringify(result.diagnostics)}`,
  );
}

// snapshot every fixture byte and link without following links
async function snapshotTree(rootDir) {
  const snapshot = [];

  // walk one relative directory deterministically
  async function visit(relativeDirectory) {
    const absoluteDirectory = join(rootDir, relativeDirectory);
    const names = (await readdir(absoluteDirectory)).sort();

    // inspect each directory member
    for (const name of names) {
      const relativePath = join(relativeDirectory, name);
      const absolutePath = join(rootDir, relativePath);
      const metadata = await lstat(absolutePath);

      // recurse without following directory links
      if (metadata.isDirectory()) {
        snapshot.push([relativePath, "directory"]);
        await visit(relativePath);
        continue;
      }

      // retain links without following their targets
      if (metadata.isSymbolicLink()) {
        snapshot.push([relativePath, "symlink", await readlink(absolutePath)]);
        continue;
      }
      snapshot.push([relativePath, "file", sha256(await readFile(absolutePath))]);
    }
  }

  await visit("");
  return snapshot;
}

// validate one fixture generation
async function validateFixture(fixture, generation = "node-v1", phase = "baseline") {
  return await validateRuntimeInventory({
    rootDir: fixture.rootDir,
    manifestPath: MANIFEST_PATH,
    generation,
    phase,
  });
}

// prove every bounded discovery class and honest baseline policy
test("static discovery covers every declared class and honest Node rollback", async () => {
  // isolate the complete discovery fixture
  await withFixture(async (fixture) => {
    const kinds = [];

    // collect the observed discovery classes
    for (const observation of fixture.discovered) {
      kinds.push(observation.kind);
      assert.match(observation.sourceDigest, /^[a-f0-9]{64}$/u);
      assert.match(observation.entryDigest, /^[a-f0-9]{64}$/u);
      assert.equal("active" in observation, false);
      assert.equal("enabled" in observation, false);
      assert.equal("installed" in observation, false);
    }
    assert.deepEqual([...new Set(kinds)].sort(), EXPECTED_KINDS);
    assert.equal(JSON.stringify(fixture.discovered).includes(fixture.rootDir), false);

    const baseline = await validateFixture(fixture);
    const rollback = await validateFixture(fixture, "node-v1", "rollback-drill");
    const rust = await validateFixture(fixture, "rust-v1", "pre-cutover");
    const mixed = await validateFixture(fixture, "mixed", "pre-cutover");
    assert.equal(baseline.ok, true, JSON.stringify(baseline.diagnostics));
    assert.equal(rollback.ok, true, JSON.stringify(rollback.diagnostics));
    assert.equal(rust.ok, true, JSON.stringify(rust.diagnostics));
    assert.equal(mixed.ok, true, JSON.stringify(mixed.diagnostics));
    assert.match(baseline.runtimeMapDigest, /^[a-f0-9]{64}$/u);
    assert.equal(baseline.runtimeMapArtifactCount, fixture.runtimeMap.artifacts.length);
    assert.equal(baseline.runtimeMapBindingCount, fixture.runtimeMap.bindings.length);
    assert.equal(baseline.runtimeMapContextCount, fixture.runtimeMap.contexts.length);

    const nodeApplicationRows = [];

    // retain honest Node application classifications
    for (const entry of fixture.manifest.entries) {
      assert.ok(Array.isArray(entry.installedPaths));
      assert.deepEqual(
        entry.generations["node-v1-rollback"],
        entry.generations["node-v1"],
      );
      assert.equal("command" in entry.generations["node-v1"], true);
      assert.equal("probe" in entry.generations["node-v1"], true);

      // require the baseline to keep Node application rows visible
      if (entry.runtime === "node" && entry.classification === "application") {
        nodeApplicationRows.push(entry.id);
        assert.equal(entry.generations["node-v1"].reachable, true);
        assert.equal(entry.generations["node-v1-rollback"].reachable, true);
      }
    }
    assert.ok(nodeApplicationRows.length > 0);
  });
});

// keep package metadata separate from executable production reachability
test("runtime map records package metadata without promoting oracle or build code", async () => {
  const { validateRuntimeLiveMap } = await runtimeLiveMapApi();

  // isolate one package metadata map
  await withPackageMapFixture(async (fixture) => {
    const result = validateRuntimeLiveMap({
      inventory: fixture.inventory,
      runtimeMap: fixture.runtimeMap,
    });
    assert.equal(result.ok, true, JSON.stringify(result.diagnostics));
    const testRoot = requireEntryId(
      fixture.inventory.entries,
      "package-script:package.json#test",
    );
    const buildRoot = requireEntryId(
      fixture.inventory.entries,
      "package-script:package.json#build",
    );
    const serveRoot = requireEntryId(
      fixture.inventory.entries,
      "package-script:package.json#serve",
    );
    assert.deepEqual(testRoot.installedPaths, []);
    assert.deepEqual(buildRoot.installedPaths, []);
    assert.deepEqual(serveRoot.installedPaths, []);
    assert.equal(testRoot.classification, "oracle");
    assert.equal(buildRoot.classification, "build");
    assert.equal(serveRoot.classification, "application");
    assert.equal(
      fixture.discovered.some(
        // forbid inert oracle source from entering application closure
        (entry) => entry.id === "import:test/oracle.mjs",
      ),
      false,
    );
    assert.equal(
      fixture.discovered.some(
        // retain the actual application script closure
        (entry) => entry.id === "import:scripts/serve.mjs",
      ),
      true,
    );

    const promotedMetadata = structuredClone(fixture.runtimeMap);
    promotedMetadata.artifacts[0].executableClosure = true;
    assertDiagnostic(
      validateRuntimeLiveMap({
        inventory: fixture.inventory,
        runtimeMap: promotedMetadata,
      }),
      "live-map.artifact",
    );
  });
});

// enforce exact source-root runtime context cardinality and ownership
test("runtime map rejects missing duplicate extra and cross-context mappings", async () => {
  const { validateRuntimeLiveMap } = await runtimeLiveMapApi();
  const fixture = contextMapFixture();
  const valid = validateRuntimeLiveMap({
    inventory: fixture.inventory,
    runtimeMap: fixture.runtimeMap,
  });
  assert.equal(valid.ok, true, JSON.stringify(valid.diagnostics));
  const mutations = [
    // remove one required source context
    ["live-map.coverage", function removeContext(runtimeMap) {
      runtimeMap.contexts.pop();
    }],
    // duplicate one source context identity
    ["live-map.duplicate", function duplicateContext(runtimeMap) {
      runtimeMap.contexts.push(structuredClone(runtimeMap.contexts[0]));
    }],
    // add one context without a source root
    ["live-map.coverage", function addExtraContext(runtimeMap) {
      const extra = structuredClone(runtimeMap.contexts[0]);
      extra.rootEntryId = "compose-command:deploy/compose.yaml#hidden";
      extra.id = runtimeContextId(
        "blueberry-compose",
        extra.rootEntryId,
        "service",
        "hidden",
      );
      extra.service = "hidden";
      runtimeMap.contexts.push(extra);
    }],
    // swap one compose service identity
    ["live-map.mapping", function swapService(runtimeMap) {
      runtimeMap.contexts[0].service = "worker";
    }],
    // swap one container image identity
    ["live-map.mapping", function swapImage(runtimeMap) {
      runtimeMap.contexts[0].imageTarget = "web";
    }],
    // swap one systemd unit identity
    ["live-map.mapping", function swapUnit(runtimeMap) {
      runtimeMap.contexts[3].unit = "weather-worker.service";
    }],
    // swap one runner account identity
    ["live-map.mapping", function swapAccount(runtimeMap) {
      runtimeMap.contexts[4].accountPolicy = {
        accountName: "root",
        homeTemplate: "${HOME}",
        resolution: "nss-passwd-exact",
      };
    }],
    // cross-link one unrelated runtime artifact
    ["live-map.mapping", function crossReferenceArtifact(runtimeMap) {
      runtimeMap.contexts[0].artifactIds = [
        "artifact:container-file:compose-command:deploy/compose.yaml#worker#0",
      ];
    }],
  ];

  // reject each cardinality or source-context substitution
  for (const [code, mutate] of mutations) {
    const runtimeMap = structuredClone(fixture.runtimeMap);
    mutate(runtimeMap);
    assertDiagnostic(
      validateRuntimeLiveMap({ inventory: fixture.inventory, runtimeMap }),
      code,
    );
  }
});

// keep image identities and account-home templates in their typed domains
test("runtime map rejects image pseudo-paths and caller-resolved HOME values", async () => {
  const { validateRuntimeLiveMap } = await runtimeLiveMapApi();
  const fixture = contextMapFixture();
  const imageDefault = requireEntryId(
    fixture.inventory.entries,
    "docker-command:Dockerfile#server",
  );
  assert.deepEqual(imageDefault.installedPaths, ["image://server"]);
  assert.equal(
    fixture.runtimeMap.artifacts.find(
      // select the image configuration artifact
      (artifact) => artifact.kind === "image-config",
    ).kind,
    "image-config",
  );

  const imageAsFile = structuredClone(fixture.runtimeMap);
  imageAsFile.artifacts.find(
    // select the API container file
    (artifact) => artifact.sourceEntryId === "compose-command:deploy/compose.yaml#api",
  ).path = "image://server";
  assertDiagnostic(
    validateRuntimeLiveMap({ inventory: fixture.inventory, runtimeMap: imageAsFile }),
    "live-map.artifact",
  );

  const callerResolvedHome = structuredClone(fixture.runtimeMap);
  callerResolvedHome.artifacts.find(
    // select the runner template artifact
    (artifact) => artifact.kind === "runner-file",
  ).declaredPath = "/home/ubuntu/weather/scripts/runner.mjs";
  assertDiagnostic(
    validateRuntimeLiveMap({
      inventory: fixture.inventory,
      runtimeMap: callerResolvedHome,
    }),
    "live-map.artifact",
  );

  assertDiagnostic(
    validateRuntimeLiveMap({ inventory: fixture.inventory, runtimeMap: undefined }),
    "live-map.schema",
  );
});

// keep fixed and templated runner homes in one workstation installation domain
test("checked runtime map deduplicates literal runner homes outside Blueberry", async () => {
  const { deriveRuntimeLiveMap } = await runtimeLiveMapApi();
  const inventory = await readRepositoryJson(MANIFEST_PATH);
  const runtimeMap = deriveRuntimeLiveMap({ entries: inventory.entries });
  const sourceEntryIds = [
    "import:deploy/scripts/adjustment-evaluation-package.mjs",
    "import:deploy/scripts/adjustment-evidence-store.mjs",
  ];

  // verify both source spellings remain checked inventory inputs
  for (const sourceEntryId of sourceEntryIds) {
    const entry = requireEntryId(inventory.entries, sourceEntryId);
    const sourcePath = sourceEntryId.slice("import:".length);
    const templatedPath = `\${HOME}/.weather/adjustment-maintenance/runner/current/${sourcePath}`;
    const literalPath = `/home/ubuntu/.weather/adjustment-maintenance/runner/current/${sourcePath}`;
    const blueberryPath = `/opt/weather/current/${sourcePath}`;
    assert.ok(entry.installedPaths.includes(templatedPath));
    assert.ok(entry.installedPaths.includes(literalPath));
    assert.ok(entry.installedPaths.includes(blueberryPath));
    const artifacts = runtimeMap.artifacts.filter(
      // select artifacts emitted from the exact shared source
      (artifact) => artifact.sourceEntryId === sourceEntryId,
    );
    const runnerArtifacts = artifacts.filter(
      // select the account-owned runner installation domain
      (artifact) => artifact.kind === "runner-file",
    );
    assert.equal(runnerArtifacts.length, 1);
    assert.equal(runnerArtifacts[0].declaredPath, templatedPath);
    assert.equal(runnerArtifacts[0].expectedMode, "0400");
    assert.equal(runnerArtifacts[0].ownerPolicy, "nss-account");
    assert.equal(
      artifacts.some(
        // reject a second artifact for the fixed-home alias
        (artifact) => artifact.path === literalPath || artifact.declaredPath === literalPath,
      ),
      false,
    );
    const blueberryArtifact = requireMapArtifact(
      runtimeMap,
      // retain the separate root-owned Blueberry release copy
      (artifact) => artifact.sourceEntryId === sourceEntryId &&
        artifact.kind === "host-file" && artifact.path === blueberryPath,
      `${sourcePath} Blueberry release copy`,
    );
    const runnerBindings = runtimeMap.bindings.filter(
      // find every context that binds the deduplicated runner file
      (binding) => binding.artifactIds.includes(runnerArtifacts[0].id),
    );
    assert.ok(runnerBindings.length > 0);

    // require every runner binding to use the exact workstation account context
    for (const binding of runnerBindings) {
      const context = runtimeMap.contexts.find(
        // resolve the binding's exact runtime context
        (candidate) => candidate.id === binding.contextId,
      );
      assert.equal(context?.profile, "workstation-runner");
      assert.equal(context?.hostRole, "workstation");
      assert.equal(context?.accountPolicy?.accountName, "ubuntu");
    }
    assert.equal(
      runtimeMap.contexts.some(
        // retain the root-owned copy in at least one Blueberry context
        (context) => context.profile === "blueberry-compose" &&
          context.artifactIds.includes(blueberryArtifact.id),
      ),
      true,
    );
  }
});

// require the checked map to match its exact source-owned artifact set
test("checked runtime map rejects missing duplicate and semantically changed artifacts", async (testContext) => {
  const { validateRuntimeLiveMap } = await runtimeLiveMapApi();
  const inventory = await readRepositoryJson(MANIFEST_PATH);
  const runtimeMap = await readRepositoryJson(RUNTIME_MAP_PATH);
  const valid = validateRuntimeLiveMap({ inventory, runtimeMap });
  assert.equal(valid.ok, true, JSON.stringify(valid.diagnostics));
  const mutations = [
    {
      code: "live-map.coverage",
      name: "missing installed artifact",
      // remove one currently orphaned API artifact
      mutate(candidate) {
        const artifact = requireMapArtifact(
          candidate,
          // select one canonical API runtime byte
          (item) => item.sourceEntryId === "import:apps/api/src/main.ts" &&
            item.kind === "container-file" && item.imageTarget === "server",
          "API main server byte",
        );
        candidate.artifacts = candidate.artifacts.filter(
          // omit only the selected exact artifact
          (item) => item.id !== artifact.id,
        );
      },
    },
    {
      code: "live-map.duplicate",
      name: "duplicate semantic artifact under a new id",
      // duplicate one artifact without changing its semantics
      mutate(candidate) {
        const artifact = structuredClone(candidate.artifacts[0]);
        artifact.id = `${artifact.id}:duplicate`;
        candidate.artifacts.push(artifact);
      },
    },
    {
      code: "live-map.mapping",
      name: "cross-image API artifact",
      // move one API byte into the unrelated web image
      mutate(candidate) {
        requireMapArtifact(
          candidate,
          // select one canonical API runtime byte
          (item) => item.sourceEntryId === "import:apps/api/src/main.ts" &&
            item.kind === "container-file" && item.imageTarget === "server",
          "API main server byte",
        ).imageTarget = "web";
      },
    },
    {
      code: "live-map.artifact",
      name: "changed Docker default command",
      // replace the exact declared server command
      mutate(candidate) {
        const artifact = requireMapArtifact(
          candidate,
          // select the server image configuration
          (item) => item.kind === "image-config" && item.imageTarget === "server",
          "server image configuration",
        );

        // support the reviewed schema migration during the RED phase
        if ("declaredCmd" in artifact) {
          artifact.declaredCmd = ["python", "evil.py"];
        } else {
          artifact.entrypoint = ["python"];
          artifact.cmd = ["evil.py"];
        }
      },
    },
    {
      code: "live-map.artifact",
      name: "changed package metadata path",
      // substitute one undeclared package copy destination
      mutate(candidate) {
        requireMapArtifact(
          candidate,
          // select one root package metadata artifact
          (item) => item.kind === "package-metadata" && item.sourceEntryId.startsWith("package-script:package.json#"),
          "root package metadata",
        ).path = "/opt/weather/evil/package.json";
      },
    },
    {
      code: "live-map.artifact",
      name: "changed systemd authority",
      // substitute a Dockerfile for the unit installer authority
      mutate(candidate) {
        requireMapArtifact(
          candidate,
          // select one installed systemd unit
          (item) => item.kind === "systemd-unit",
          "systemd unit",
        ).authority = { kind: "dockerfile-copy", sourcePath: "Dockerfile" };
      },
    },
  ];

  // isolate every reviewed fail-open mutation
  for (const mutation of mutations) {
    // report every semantic mutation independently
    await testContext.test(mutation.name, () => {
      const candidate = structuredClone(runtimeMap);
      mutation.mutate(candidate);
      assertDiagnostic(
        validateRuntimeLiveMap({ inventory, runtimeMap: candidate }),
        mutation.code,
      );
    });
  }
});

// bind every transitive installed byte to each actual root instance
test("runtime map covers transitive artifacts per root and instance without orphans", async () => {
  const { validateRuntimeLiveMap } = await runtimeLiveMapApi();
  const inventory = await readRepositoryJson(MANIFEST_PATH);
  const runtimeMap = await readRepositoryJson(RUNTIME_MAP_PATH);
  assert.ok(Array.isArray(runtimeMap.bindings), "missing transitive runtime bindings");
  const referencedArtifacts = new Set(
    runtimeMap.bindings.flatMap(
      // retain every bound artifact identity
      (binding) => binding.artifactIds,
    ),
  );
  assert.deepEqual(
    [...referencedArtifacts].sort(),
    runtimeMap.artifacts.map(
      // retain the complete declared artifact set
      (artifact) => artifact.id,
    ).sort(),
  );
  const sharedEntryId = "import:packages/database/src/index.ts";
  const apiContext = requireMapContext(
    runtimeMap,
    "compose-command:deploy/compose.yaml#api",
  );
  const workerContext = requireMapContext(
    runtimeMap,
    "compose-command:deploy/compose.yaml#worker",
  );

  // require the shared module in both service roots
  for (const context of [apiContext, workerContext]) {
    assert.ok(runtimeMap.bindings.some(
      // match the exact entry root and instance tuple
      (binding) => binding.entryId === sharedEntryId &&
        binding.rootEntryId === context.rootEntryId &&
        binding.contextId === context.id,
    ));
  }
  const missingWorkerBinding = structuredClone(runtimeMap);
  missingWorkerBinding.bindings = missingWorkerBinding.bindings.filter(
    // omit the shared module only from the worker instance
    (binding) => !(binding.entryId === sharedEntryId &&
      binding.rootEntryId === workerContext.rootEntryId &&
      binding.contextId === workerContext.id),
  );
  assertDiagnostic(
    validateRuntimeLiveMap({ inventory, runtimeMap: missingWorkerBinding }),
    "live-map.coverage",
  );
});

// keep one unique binding per entry root and runtime instance tuple
test("runtime map binding identity excludes artifact multiplicity", async () => {
  const { validateRuntimeLiveMap } = await runtimeLiveMapApi();
  const inventory = await readRepositoryJson(MANIFEST_PATH);
  const runtimeMap = await readRepositoryJson(RUNTIME_MAP_PATH);
  const tupleIds = runtimeMap.bindings.map(
    // retain the approved exact binding tuple
    (binding) => JSON.stringify([
      binding.entryId,
      binding.rootEntryId,
      binding.contextId,
    ]),
  );
  assert.equal(
    new Set(tupleIds).size,
    runtimeMap.bindings.length,
    "runtime bindings contain duplicate entry root context tuples",
  );
  const duplicateTuple = structuredClone(runtimeMap);
  const existing = structuredClone(duplicateTuple.bindings[0]);
  const otherArtifact = duplicateTuple.artifacts.find(
    // select an artifact outside the existing binding
    (artifact) => !existing.artifactIds.includes(artifact.id),
  );
  assert.ok(otherArtifact, "missing alternate duplicate-tuple artifact");
  existing.id = `${existing.id}:duplicate`;
  existing.artifactIds = [otherArtifact.id];
  duplicateTuple.bindings.push(existing);
  assertDiagnostic(
    validateRuntimeLiveMap({ inventory, runtimeMap: duplicateTuple }),
    "live-map.duplicate",
  );
});

// bind every declared probe and package one-shot executable to its instance
test("runtime map covers Compose probes and worker performance executables", async () => {
  const inventory = await readRepositoryJson(MANIFEST_PATH);
  const runtimeMap = await readRepositoryJson(RUNTIME_MAP_PATH);
  const artifactsById = new Map(runtimeMap.artifacts.map(
    // index exact checked artifacts
    (artifact) => [artifact.id, artifact],
  ));
  const probeEntries = inventory.entries.filter(
    // select every declared Compose probe
    (entry) => entry.kind === "compose-probe",
  );
  assert.equal(probeEntries.length, 6);

  // require each probe in its exact source root and runtime instance
  for (const probe of probeEntries) {
    assert.equal(probe.parents.length, 1, probe.id);
    const rootEntryId = probe.parents[0].id;
    const context = requireMapContext(runtimeMap, rootEntryId);
    const binding = runtimeMap.bindings.find(
      // match the probe entry root and context tuple
      (candidate) => candidate.entryId === probe.id &&
        candidate.rootEntryId === rootEntryId &&
        candidate.contextId === context.id,
    );
    assert.ok(binding, `missing probe binding ${probe.id}`);
    assert.ok(binding.artifactIds.length > 0, `missing probe artifact ${probe.id}`);

    // bind every probe artifact back to the probe source row
    for (const artifactId of binding.artifactIds) {
      assert.equal(artifactsById.get(artifactId)?.sourceEntryId, probe.id);
    }
  }
  const executableEntryId =
    "import:apps/worker/src/forecast-adjustment-performance-cli.ts";
  const oneShotRoots = inventory.entries.filter(
    // select both package declarations that invoke this one-shot
    (entry) => entry.kind === "package-script" &&
      entry.command?.includes("forecast-adjustment-performance-cli.js"),
  );
  assert.equal(oneShotRoots.length, 2);

  // require the invoked executable in every package one-shot context
  for (const root of oneShotRoots) {
    const context = requireMapContext(runtimeMap, root.id);
    const binding = runtimeMap.bindings.find(
      // match the invoked executable tuple
      (candidate) => candidate.entryId === executableEntryId &&
        candidate.rootEntryId === root.id &&
        candidate.contextId === context.id,
    );
    assert.ok(binding, `missing one-shot executable binding ${root.id}`);
    assert.ok(binding.artifactIds.length > 0, root.id);
    const executableArtifacts = binding.artifactIds.map(
      // resolve every exact executable artifact
      (artifactId) => artifactsById.get(artifactId),
    );
    assert.ok(executableArtifacts.every(
      // require the emitted server-image executable identity
      (artifact) => artifact?.sourceEntryId === executableEntryId &&
        artifact.kind === "container-file" &&
        artifact.imageTarget === "server" &&
        artifact.path === "/opt/weather/apps/worker/dist/forecast-adjustment-performance-cli.js",
    ));
  }
});

// preserve exact deployment recipe semantics in the checked source map
test("checked runtime map matches production Compose Docker and source-only semantics", async () => {
  const runtimeMap = await readRepositoryJson(RUNTIME_MAP_PATH);
  const productionServices = runtimeMap.contexts.filter(
    // select production Compose service roots only
    (context) => context.rootEntryId.startsWith("compose-command:deploy/compose.yaml#") &&
      context.activationSource === "container-inspect",
  );
  assert.deepEqual(
    productionServices.map(
      // retain each exact production service
      (context) => context.service,
    ).sort(),
    ["api", "cloudflared", "migration", "postgres", "web", "worker"],
  );
  const localCloudflared = requireMapContext(
    runtimeMap,
    "compose-command:deploy/compose.local.yaml#cloudflared",
  );
  assert.equal(localCloudflared.activationSource, "never-active-alternative");
  assert.equal(
    runtimeMap.artifacts.some(
      // reject invented source-checkout installation metadata
      (artifact) => artifact.path?.startsWith("/home/ubuntu/weather/scripts/"),
    ),
    false,
  );
  const serverImage = requireMapArtifact(
    runtimeMap,
    // select the server image configuration
    (artifact) => artifact.kind === "image-config" && artifact.imageTarget === "server",
    "server image configuration",
  );
  assert.equal(serverImage.declaredEntrypoint, null);
  assert.deepEqual(serverImage.declaredCmd, ["node", "apps/api/dist/main.js"]);
  const webImage = requireMapArtifact(
    runtimeMap,
    // select the web image configuration
    (artifact) => artifact.kind === "image-config" && artifact.imageTarget === "web",
    "web image configuration",
  );
  assert.equal(webImage.declaredEntrypoint, null);
  assert.deepEqual(webImage.declaredCmd, ["node", "deploy/scripts/web-server.mjs"]);
  const sharedSupportModules = [
    "adjustment-evidence-store.mjs",
    "forecast-adjustment-scorecard-contract.mjs",
    "forecast-observations.mjs",
    "home-network.mjs",
    "weather-admin-store.mjs",
    "web-server.mjs",
    "xweather-tile-cache.mjs",
    "xweather-usage-budget.mjs",
  ];

  // require each explicit web support module in both copied image domains
  for (const fileName of sharedSupportModules) {
    const path = `/opt/weather/deploy/scripts/${fileName}`;
    const imageTargets = runtimeMap.artifacts.filter(
      // select exact container copies of this support module
      (artifact) => artifact.kind === "container-file" && artifact.path === path,
    ).map(
      // retain every image installation domain
      (artifact) => artifact.imageTarget,
    ).sort();
    assert.deepEqual(imageTargets, ["server", "web"], fileName);
  }
});

// make the public inventory check require its default checked runtime map
test("public inventory check fails when the checked runtime map is missing or corrupt", async (testContext) => {
  // isolate one missing checked map
  await testContext.test("missing map", async () => {
    await withFixture(async (fixture) => {
      await unlink(join(fixture.rootDir, RUNTIME_MAP_PATH));
      assertDiagnostic(await validateFixture(fixture), "live-map.read");
    });
  });

  // isolate one corrupt checked map
  await testContext.test("corrupt map", async () => {
    await withFixture(async (fixture) => {
      await writeFile(join(fixture.rootDir, RUNTIME_MAP_PATH), "{not-json\n");
      assertDiagnostic(await validateFixture(fixture), "live-map.syntax");
    });
  });
});

// prove syntax-aware module discovery without regex false positives
test("module discovery accepts legal syntax and ignores type-only or inert text", async () => {
  // isolate one executable module graph
  await withFixture(async (fixture) => {
    await writeTree(fixture.rootDir, {
      "scripts/research/syntax-side-effect.mjs": `export const sideEffect = true;\n`,
      "scripts/research/syntax-commented.mjs": `export const commented = true;\n`,
      "scripts/research/syntax-reexport.mjs": `export const reexported = true;\n`,
      "scripts/research/syntax-dynamic.mjs": `export const dynamicValue = true;\n`,
      "scripts/research/syntax-required.mjs": `export const requiredValue = true;\n`,
      "scripts/research/type-only.ts": `export type OnlyType = string;\n`,
      "scripts/research/export-type-only.ts": `export type OtherType = number;\n`,
      "scripts/research/string-only.mjs": `export const stringOnly = true;\n`,
      "scripts/research/comment-only.mjs": `export const commentOnly = true;\n`,
      "scripts/research/require-string-only.mjs": `export const requireStringOnly = true;\n`,
    });
    await writeFile(
      join(fixture.rootDir, "apps/api/main.mjs"),
      `import"../../scripts/research/syntax-side-effect.mjs";\nimport/* import gap */{ commented }from/* source gap */"../../scripts/research/syntax-commented.mjs";\nexport/* export gap */{ reexported }from/* source gap */"../../scripts/research/syntax-reexport.mjs";\nimport type { OnlyType } from "../../scripts/research/type-only.ts";\nexport type { OtherType } from "../../scripts/research/export-type-only.ts";\nconst dynamicValue = await import(/* literal gap */ "../../scripts/research/syntax-dynamic.mjs");\nconst requiredValue = require(/* literal gap */ "../../scripts/research/syntax-required.mjs");\nconst inertImport = 'import "../../scripts/research/string-only.mjs"';\nconst inertRequire = 'require("../../scripts/research/require-string-only.mjs")';\n/* export { commentOnly } from "../../scripts/research/comment-only.mjs"; */\nexport { commented, dynamicValue, inertImport, inertRequire, requiredValue };\n`,
    );

    const discovered = await discoverFixture(fixture.rootDir);
    const ids = inventoryIds(discovered);
    const expectedRuntimePaths = [
      "scripts/research/syntax-side-effect.mjs",
      "scripts/research/syntax-commented.mjs",
      "scripts/research/syntax-reexport.mjs",
      "scripts/research/syntax-dynamic.mjs",
      "scripts/research/syntax-required.mjs",
    ];

    // require every executable module edge
    for (const sourcePath of expectedRuntimePaths) {
      assert.ok(ids.has(`import:${sourcePath}`), `missing runtime edge ${sourcePath}`);
    }
    const ignoredPaths = [
      "scripts/research/type-only.ts",
      "scripts/research/export-type-only.ts",
      "scripts/research/string-only.mjs",
      "scripts/research/comment-only.mjs",
      "scripts/research/require-string-only.mjs",
    ];

    // exclude types and inert source text from runtime closure
    for (const sourcePath of ignoredPaths) {
      assert.equal(ids.has(`import:${sourcePath}`), false, `unexpected runtime edge ${sourcePath}`);
    }
  });
});

// require offline roots to scan static and unsupported dynamic imports
test("offline performance roots retain static closure and reject computed imports", async () => {
  const files = {
    "package.json": `${JSON.stringify({
      name: "offline-performance-fixture",
      private: true,
      scripts: {
        "weather:forecast-adjustment:performance": "node scripts/offline-performance.mjs",
      },
    })}\n`,
    "scripts/offline-performance.mjs": `import "./offline-child.mjs";\nconst target = "./offline-dynamic.mjs";\nexport const dynamic = await import(target);\n`,
    "scripts/offline-child.mjs": `export const child = true;\n`,
    "scripts/offline-dynamic.mjs": `export const dynamic = true;\n`,
  };

  // isolate one offline executable graph
  await withBareFixture(files, async (rootDir) => {
    const discovery = await discoverRuntimeInventory({ rootDir });
    const ids = inventoryIds(discovery.entries);
    assert.ok(ids.has("import:scripts/offline-performance.mjs"));
    assert.ok(ids.has("import:scripts/offline-child.mjs"));
    assert.equal(ids.has("import:scripts/offline-dynamic.mjs"), false);
    assert.ok(diagnosticCodes(discovery).includes("discovery.dynamic-import"));
  });
});

// prove canonical sources win over absent or stale generated output
test("canonical app and package sources ignore dist while bundled snapshots remain discoverable", async () => {
  // isolate one source and generated-output boundary
  await withFixture(async (fixture) => {
    const rootPackage = JSON.parse(await readFile(join(fixture.rootDir, "package.json"), "utf8"));
    rootPackage.workspaces = ["apps/*", "packages/*"];
    await writeJson(fixture.rootDir, "package.json", rootPackage);
    const composePath = join(fixture.rootDir, "deploy/compose.yaml");
    const compose = await readFile(composePath, "utf8");
    await writeFile(
      composePath,
      `${compose}  web:\n    command: [node, apps/web/dist/index.js]\n`,
    );
    const frozenPath =
      "scripts/research/adjustment-maintenance-runtime/node_modules/@weather/domain/dist/weather-record.js";
    await writeTree(fixture.rootDir, {
      "apps/web/package.json": `${JSON.stringify({ name: "@weather/web", private: true })}\n`,
      "apps/web/src/index.ts": `import { domain } from "@weather/domain";\nimport { frozen } from "../../../${frozenPath}";\nexport const web = [domain, frozen];\n`,
      "packages/domain/package.json": `${JSON.stringify({ name: "@weather/domain", private: true })}\n`,
      "packages/domain/src/index.ts": `export const domain = true;\n`,
      [frozenPath]: `export const frozen = true;\n`,
    });
    const installerPath = join(
      fixture.rootDir,
      "scripts/install-adjustment-maintenance-runner.sh",
    );
    const installer = await readFile(installerPath, "utf8");
    await writeFile(
      installerPath,
      installer.replace(
        ")\n",
        `  "${frozenPath}"\n)\n`,
      ),
    );

    const withoutDist = await discoverFixture(fixture.rootDir);
    const cleanIds = inventoryIds(withoutDist);
    assert.ok(cleanIds.has("import:apps/web/src/index.ts"));
    assert.ok(cleanIds.has("import:packages/domain/src/index.ts"));
    assert.ok(cleanIds.has(`import:${frozenPath}`));
    assert.ok(
      requireEntryId(
        withoutDist,
        "import:apps/web/src/index.ts",
      ).installedPaths.includes("/opt/weather/apps/web/dist/index.js"),
    );
    assert.ok(
      requireEntryId(
        withoutDist,
        "import:packages/domain/src/index.ts",
      ).installedPaths.includes("/opt/weather/packages/domain/dist/index.js"),
    );
    assert.ok(
      requireEntryId(
        withoutDist,
        `import:${frozenPath}`,
      ).installedPaths.includes(
        `\${HOME}/.weather/adjustment-maintenance/runner/current/${frozenPath}`,
      ),
    );

    // exclude generated app and package output identities
    for (const entry of withoutDist) {
      const generatedApp = entry.sourcePath.startsWith("apps/web/dist/");
      const generatedPackage = entry.sourcePath.startsWith("packages/domain/dist/");
      assert.equal(generatedApp || generatedPackage, false);
    }

    await writeTree(fixture.rootDir, {
      "apps/web/dist/index.js": `import "../../../scripts/research/stale-dist.mjs";\n`,
      "packages/domain/dist/index.js": `import "../../../scripts/research/stale-dist.mjs";\n`,
      "scripts/research/stale-dist.mjs": `export const stale = true;\n`,
    });
    const withStaleDist = await discoverFixture(fixture.rootDir);
    assert.deepEqual(withStaleDist, withoutDist);

    await writeTree(fixture.rootDir, {
      "apps/web/src/index.ts": `import { domain } from "@weather/domain";\nimport { changed } from "../../../scripts/research/canonical-changed.mjs";\nexport const web = [changed, domain];\n`,
      "scripts/research/canonical-changed.mjs": `export const changed = true;\n`,
    });
    const changedSource = await discoverFixture(fixture.rootDir);
    const changedIds = inventoryIds(changedSource);
    assert.notDeepEqual(changedSource, withStaleDist);
    assert.ok(changedIds.has("import:scripts/research/canonical-changed.mjs"));
    assert.equal(changedIds.has(`import:${frozenPath}`), true);
    assert.equal(changedIds.has("import:scripts/research/stale-dist.mjs"), false);
  });
});

// prove every executable root class closes over runtime imports
test("package shell private forced and installer roots discover complete module closure", async () => {
  // isolate roots without compose docker or systemd support
  await withExecutionRootFixture(async (fixture) => {
    const ids = inventoryIds(fixture.discovered);
    const expectedRootIds = [
      "package-script:package.json#job",
      "shell-invocation:deploy/scripts/run.sh",
      "private-command:deploy/scripts/forecast-adjustment-evidence.sh",
      "private-command:deploy/scripts/adjustment-model.sh",
      "forced-remote:deploy/scripts/remote-ops.sh",
      "installer-allowlist:scripts/install-adjustment-maintenance-runner.sh#RUNNER_FILES:scripts/installer-job.mjs",
    ];

    // require every executable root observation
    for (const entryId of expectedRootIds) {
      assert.ok(ids.has(entryId), `missing executable root ${entryId}`);
    }
    const expectedModulePaths = [
      "scripts/job.mjs",
      "scripts/package-child.mjs",
      "scripts/shell-job.mjs",
      "scripts/shell-child.mjs",
      "scripts/private-job.mjs",
      "scripts/private-child.mjs",
      "scripts/private-env-job.mjs",
      "scripts/private-env-child.mjs",
      "scripts/forced-job.mjs",
      "scripts/forced-child.mjs",
      "scripts/installer-job.mjs",
      "scripts/installer-child.mjs",
    ];

    // require every root module and transitive child
    for (const sourcePath of expectedModulePaths) {
      assert.ok(ids.has(`import:${sourcePath}`), `missing executable closure ${sourcePath}`);
    }

    assert.equal(
      requireEntryId(
        fixture.discovered,
        "package-script:package.json#job",
      ).classification,
      "application",
    );
    assert.equal(
      requireEntryId(
        fixture.discovered,
        "package-script:package.json#build",
      ).classification,
      "build",
    );
    assert.equal(
      requireEntryId(
        fixture.discovered,
        "shell-invocation:deploy/scripts/run.sh",
      ).classification,
      "application",
    );
    assert.equal(
      requireEntryId(
        fixture.discovered,
        "private-command:deploy/scripts/forecast-adjustment-evidence.sh",
      ).classification,
      "release-control",
    );
    assert.equal(
      requireEntryId(
        fixture.discovered,
        "forced-remote:deploy/scripts/remote-ops.sh",
      ).classification,
      "release-control",
    );
    const installerId =
      "installer-allowlist:scripts/install-adjustment-maintenance-runner.sh#RUNNER_FILES:scripts/installer-job.mjs";
    assert.equal(requireEntryId(fixture.discovered, installerId).classification, "application");
    const installerImport = requireEntryId(
      fixture.discovered,
      "import:scripts/installer-job.mjs",
    );
    let hasInstallerParent = false;
    let hasInstalledSnapshot = false;

    // retain the exact installer invocation edge
    for (const parent of installerImport.parents) {
      // match the installer allowlist root
      if (parent.type === "invocation" && parent.id === installerId) {
        hasInstallerParent = true;
      }
    }

    // retain the declared installed runner snapshot path
    for (const installedPath of installerImport.installedPaths) {
      // match the immutable runner-current mapping
      if (installedPath.includes("adjustment-maintenance/runner/current/scripts/installer-job.mjs")) {
        hasInstalledSnapshot = true;
      }
    }
    assert.equal(hasInstallerParent, true);
    assert.equal(hasInstalledSnapshot, true);
    assert.equal((await validateFixture(fixture)).ok, true);
  });
});

// prove mutations below every executable root invalidate validation
test("package shell private forced and installer child drift fails closed", async () => {
  const childPaths = [
    "scripts/package-child.mjs",
    "scripts/shell-child.mjs",
    "scripts/private-child.mjs",
    "scripts/private-env-child.mjs",
    "scripts/forced-child.mjs",
    "scripts/installer-child.mjs",
  ];

  // isolate every child mutation
  for (const childPath of childPaths) {
    // validate one root closure independently
    await withExecutionRootFixture(async (fixture) => {
      const baseline = await validateFixture(fixture);
      assert.equal(baseline.ok, true, JSON.stringify(baseline.diagnostics));
      const absolutePath = join(fixture.rootDir, childPath);
      const source = await readFile(absolutePath, "utf8");
      await writeFile(absolutePath, `${source}\nexport const drift = true;\n`);
      assertDiagnostic(await validateFixture(fixture), "entry.changed");
    });
  }
});

// reject executable roots whose module mapping is not literal
test("computed package shell private and forced module roots fail closed", async () => {
  const cases = [
    {
      "package.json": `${JSON.stringify({
        name: "computed-package-root",
        private: true,
        scripts: { job: "node \"$WEATHER_JOB_MODULE\"" },
      })}\n`,
    },
    {
      "package.json": `${JSON.stringify({ name: "computed-shell-root", private: true })}\n`,
      "deploy/scripts/run.sh": `#!/usr/bin/env bash\nexec /usr/bin/node "\${WEATHER_JOB_MODULE}"\n`,
    },
    {
      "package.json": `${JSON.stringify({ name: "computed-private-root", private: true })}\n`,
      "deploy/scripts/forecast-adjustment-model.sh": `#!/usr/bin/env bash\nexec node "\${WEATHER_JOB_MODULE}"\n`,
    },
    {
      "package.json": `${JSON.stringify({ name: "computed-forced-root", private: true })}\n`,
      "deploy/scripts/remote-ops.sh": `#!/usr/bin/env bash\ncase "\${SSH_ORIGINAL_COMMAND:-}" in\n  job) exec node "\${WEATHER_JOB_MODULE}" ;;\nesac\n`,
    },
  ];

  // exercise each unsupported root mapping
  for (const files of cases) {
    // isolate one computed mapping
    await withBareFixture(files, async (rootDir) => {
      const discovery = await discoverRuntimeInventory({ rootDir });
      assert.ok(diagnosticCodes(discovery).includes("discovery.unsupported"));
    });
  }
});

// require valid alternate execution syntax to discover or fail explicitly
test("alternate Compose Docker and installer syntax never disappears silently", async () => {
  const cases = [
    {
      kind: "compose-command",
      files: {
        "package.json": `${JSON.stringify({ name: "compose-future", private: true })}\n`,
        "deploy/compose.future.yaml": `services:\n  "hidden-api":\n    command: [node, apps/api/hidden.mjs]\n`,
        "apps/api/hidden.mjs": `export const hidden = true;\n`,
      },
    },
    {
      kind: "compose-command",
      files: {
        "package.json": `${JSON.stringify({ name: "compose-inline", private: true })}\n`,
        "deploy/compose.yaml": `services: { hidden-api: { command: [node, apps/api/hidden.mjs] } }\n`,
        "apps/api/hidden.mjs": `export const hidden = true;\n`,
      },
    },
    {
      kind: "compose-command",
      files: {
        "package.json": `${JSON.stringify({ name: "compose-anchor", private: true })}\n`,
        "deploy/compose.yaml": `x-hidden-command: &hidden-command [node, apps/api/hidden.mjs]\nservices:\n  hidden-api:\n    command: *hidden-command\n`,
        "apps/api/hidden.mjs": `export const hidden = true;\n`,
      },
    },
    {
      kind: "docker-entrypoint",
      files: {
        "package.json": `${JSON.stringify({ name: "docker-lowercase", private: true })}\n`,
        Dockerfile: `from node:24\nentrypoint ["node", \\\n  "apps/api/hidden.mjs"]\n`,
        "apps/api/hidden.mjs": `export const hidden = true;\n`,
      },
    },
    {
      kind: "installer-allowlist",
      files: {
        "package.json": `${JSON.stringify({ name: "installer-declare", private: true })}\n`,
        "scripts/install-hidden-runner.sh": `#!/usr/bin/env bash\ndeclare -ar RUNNER_FILES=("scripts/hidden.mjs")\n`,
        "scripts/hidden.mjs": `export const hidden = true;\n`,
      },
    },
  ];

  // exercise each bounded alternate syntax
  for (const syntaxCase of cases) {
    // isolate one syntax form
    await withBareFixture(syntaxCase.files, async (rootDir) => {
      const discovery = await discoverRuntimeInventory({ rootDir });
      assertDiscoveredOrUnsupported(discovery, syntaxCase.kind);
    });
  }
});

// prevent a manifest from relabeling an entire source-owned application graph
test("source-owned Compose application classification survives whole-closure spoofing", async () => {
  const classifications = ["release-control", "oracle", "build", "offline-research"];

  // exercise every forbidden root relabeling
  for (const classification of classifications) {
    // isolate one complete graph spoof
    await withClassificationFixture(async (fixture) => {
      const baseline = await validateFixture(fixture);
      assert.equal(baseline.ok, true, JSON.stringify(baseline.diagnostics));
      const manifest = structuredClone(fixture.manifest);
      const rootId = "compose-command:deploy/compose.yaml#api";
      const closure = transitiveClosureIds(manifest.entries, rootId);

      // relabel the root and every imported descendant
      for (const entry of manifest.entries) {
        // spoof only the selected source-owned graph
        if (closure.has(entry.id)) {
          entry.classification = classification;
          entry.owner = `${classification}-owner`;
          entry.parityTest = "test/runtime-parity.test.mjs";
          entry.accountingScope = classification === "oracle" ? "build-only" : "fixture-cgroup";
        }
      }
      rebuildGenerationPolicies(manifest);
      await writeJson(fixture.rootDir, MANIFEST_PATH, manifest);
      assertDiagnostic(await validateFixture(fixture), "entry.classification");
    });
  }
});

// reject Rust policy paths that hide Node work behind a shell parent
test("rust-v1 closes reachable application parents and rejects shell-invoked Node", async () => {
  const files = {
    "package.json": `${JSON.stringify({
      name: "rust-policy-fixture",
      private: true,
    })}\n`,
    "deploy/compose.yaml": `services:\n  rust-api:\n    command: [/opt/weather/bin/weather-api]\n`,
    "deploy/scripts/job.sh": `#!/usr/bin/env bash\nexec node apps/job.mjs\n`,
    "apps/job.mjs": `import "./job-child.mjs";\nexport const job = true;\n`,
    "apps/job-child.mjs": `export const child = true;\n`,
  };

  // isolate one mixed Rust and shell application graph
  await withBareFixture(files, async (rootDir) => {
    const discovered = await discoverFixture(rootDir);
    const manifest = buildManifest(discovered);
    const rustRoot = requireEntryId(
      manifest.entries,
      "compose-command:deploy/compose.yaml#rust-api",
    );
    const shellRoot = requireEntryId(
      manifest.entries,
      "shell-invocation:deploy/scripts/job.sh",
    );
    const nodeRoot = requireEntryId(manifest.entries, "import:apps/job.mjs");
    const nodeChild = requireEntryId(manifest.entries, "import:apps/job-child.mjs");
    assert.equal(rustRoot.classification, "application");
    assert.equal(shellRoot.classification, "application");
    assert.equal(nodeRoot.classification, "application");
    assert.equal(nodeChild.classification, "application");

    // clear candidate reachability before selecting the bypass
    for (const entry of manifest.entries) {
      entry.generations["rust-v1"].reachable = false;
    }
    rustRoot.generations["rust-v1"].reachable = true;
    shellRoot.generations["rust-v1"].reachable = true;
    rebuildGenerationPolicies(manifest);
    await writeJson(rootDir, MANIFEST_PATH, manifest);
    await writeFixtureRuntimeMap(rootDir, manifest);
    const result = await validateRuntimeInventory({
      rootDir,
      manifestPath: MANIFEST_PATH,
      generation: "rust-v1",
      phase: "pre-cutover",
    });
    assertDiagnostic(result, "generation.rust-node-application");
    assert.ok(diagnosticCodes(result).includes("generation.policy"));
  });
});

// prove the finite native image overlay and runner roots form an honest mixed transition
test("native image Compose and runner entrypoints close rust-v1 without hiding Node", async () => {
  // render one native runner service with an optional fixed mode
  const nativeUnit = (argumentsList = "") => [
    "[Service]",
    `ExecStart=/home/ubuntu/.weather/adjustment-maintenance/runner/current/bin/weather-maintenance-capture${argumentsList}`,
    "",
  ].join("\n");
  // render one timer bound to its exact service unit
  const nativeTimer = (unit) => `[Timer]\nOnCalendar=daily\nUnit=${unit}\n`;
  const rustMain = "fn main() {}\n";
  const files = {
    "package.json": `${JSON.stringify({ name: "native-runtime-inventory", private: true })}\n`,
    "deploy/compose.yaml": "services:\n  api:\n    command: [node, apps/api/main.mjs]\n",
    "apps/api/main.mjs": "export const main = true;\n",
    Dockerfile: [
      "FROM scratch AS server",
      "CMD [\"node\", \"apps/api/main.mjs\"]",
      "FROM scratch AS web",
      "CMD [\"node\", \"deploy/scripts/web-server.mjs\"]",
      "FROM scratch AS native-runtime",
      "FROM native-runtime AS native-server",
      "CMD [\"/opt/weather/bin/weather-api\"]",
      "FROM native-runtime AS native-web",
      "CMD [\"/opt/weather/bin/weather-web\"]",
      "",
    ].join("\n"),
    "deploy/scripts/web-server.mjs": "export const web = true;\n",
    "deploy/scripts/native-generation-remote-ops.sh": [
      "#!/usr/bin/env bash",
      "native_evidence_binary=/opt/weather/operators/native-evidence/packages/$bundle/weather-evidence",
      "exec \"$native_evidence_binary\" \"$@\"",
      "",
    ].join("\n"),
    "deploy/scripts/native-evidence-remote-dispatch.sh": [
      "#!/usr/bin/env bash",
      "exec \"$native_evidence_binary\" project-maintenance-ledger-v3",
      "",
    ].join("\n"),
    "deploy/scripts/native-dashboard-release-control.sh": [
      "#!/usr/bin/env bash",
      "compose exec web /opt/weather/bin/weather-http-health",
      "",
    ].join("\n"),
    "deploy/scripts/native-release-control-remote-dispatch.sh": [
      "#!/usr/bin/env bash",
      "case \"$action\" in",
      "  activate) sp_activate \"$release\" ;;",
      "  rollback) sp_rollback ;;",
      "esac",
      "",
    ].join("\n"),
    "deploy/scripts/package-native-evidence-operator.sh": [
      "#!/usr/bin/env bash",
      "install -m 0755 \"$binary\" \"$output/weather-evidence\"",
      "",
    ].join("\n"),
    "scripts/install-native-evidence-operator.sh": [
      "#!/usr/bin/env bash",
      "root=/opt/weather/operators/native-evidence",
      "install -m 0755 \"$source/weather-evidence\" \"$root/weather-evidence\"",
      "",
    ].join("\n"),
    "scripts/schema-preserving-release.sh": "#!/usr/bin/env bash\nnode scripts/schema-preserving-release.mjs\n",
    "scripts/schema-preserving-release.mjs": `
export function runtimeGenerationComposeOverride(generation) {
  if (generation === "node-v1") return null;
  return { services: {
    migration: { command: ["/opt/weather/bin/weather-migrate"] },
    api: { command: ["/opt/weather/bin/weather-api"], healthcheck: { test: ["CMD", "/opt/weather/bin/weather-api-health", "api"] } },
    worker: { command: ["/opt/weather/bin/weather-worker"], healthcheck: { test: ["CMD", "/opt/weather/bin/weather-worker-health"] } },
    web: { command: ["/opt/weather/bin/weather-web"], environment: { WEATHER_SITE_CONFIG_PATH: "/opt/weather/edge-config/ballydidean.json" }, healthcheck: { test: ["CMD", "/opt/weather/bin/weather-web-health", "web"] } },
  } };
}
`,
    "scripts/install-adjustment-maintenance-runner.sh": `#!/usr/bin/env bash
readonly -a ADJUSTMENT_RUNNER_NATIVE_EXECUTABLES=(
  "weather-maintenance-capture"
  "weather-maintenance-fit"
  "weather-maintenance-wind-fit"
)
readonly -a ADJUSTMENT_RUNNER_FILES=(
  "scripts/research/rain_refresh.py"
)
`,
    "scripts/research/rain_refresh.py": "print('predecessor fit')\n",
    "scripts/systemd/native/weather-adjustment-archive.service": nativeUnit(),
    "scripts/systemd/native/weather-adjustment-daily.service": nativeUnit(" --daily"),
    "scripts/systemd/native/weather-adjustment-daily.timer": nativeTimer("weather-adjustment-daily.service"),
    "scripts/systemd/native/weather-adjustment-monthly.service": nativeUnit(" --monthly"),
    "scripts/systemd/native/weather-adjustment-monthly.timer": nativeTimer("weather-adjustment-monthly.service"),
    "rust/apps/weather-migrate/src/main.rs": rustMain,
    "rust/crates/weather-runtime/src/bin/weather-api.rs": rustMain,
    "rust/crates/weather-runtime/src/bin/weather-http-health.rs": rustMain,
    "rust/crates/weather-runtime/src/bin/weather-maintenance-capture.rs": rustMain,
    "rust/crates/weather-runtime/src/bin/weather-maintenance-fit.rs": rustMain,
    "rust/crates/weather-runtime/src/bin/weather-maintenance-wind-fit.rs": rustMain,
    "rust/crates/weather-runtime/src/bin/weather-web.rs": rustMain,
    "rust/crates/weather-runtime/src/bin/weather-worker-health.rs": rustMain,
    "rust/crates/weather-runtime/src/bin/weather-evidence.rs": rustMain,
    "rust/crates/weather-runtime/src/bin/weather-worker.rs": [
      "fn main() {",
      "  let _ = argument == \"backfill\";",
      "  let _ = Some(\"public-stations-backfill\");",
      "  let _ = Some(\"tide-backfill\");",
      "  let _ = Some(\"tempest-backfill\");",
      "}",
      "",
    ].join("\n"),
  };

  // isolate the complete finite transition without invoking repository scanning
  await withBareFixture(files, async (rootDir) => {
    const discovered = await discoverFixture(rootDir);
    const ids = inventoryIds(discovered);
    assert.ok(ids.has("docker-command:Dockerfile#native-server"));
    assert.ok(ids.has("runtime-generation-command:scripts/schema-preserving-release.mjs#migration"));
    assert.ok(ids.has("runtime-generation-probe:scripts/schema-preserving-release.mjs#web"));
    assert.ok(ids.has("systemd-service:scripts/systemd/native/weather-adjustment-archive.service"));
    const migration = requireEntryId(discovered, "native-binary:server-migrate");
    assert.equal(migration.sourcePath, "rust/apps/weather-migrate/src/main.rs");
    const gateway = requireEntryId(
      discovered,
      "forced-remote:deploy/scripts/native-generation-remote-ops.sh",
    );
    assert.deepEqual(gateway.invokedRuntimes, ["rust"]);
    assert.deepEqual(gateway.installedPaths, [
      "/opt/weather/operators/native-evidence/packages/${NATIVE_EVIDENCE_BUNDLE_SHA256}/native-generation-remote-ops.sh",
      "/usr/local/sbin/weather-remote-ops",
    ]);
    const dispatcher = requireEntryId(
      discovered,
      "forced-remote:deploy/scripts/native-evidence-remote-dispatch.sh",
    );
    assert.deepEqual(dispatcher.invokedRuntimes, ["rust"]);
    assert.deepEqual(dispatcher.installedPaths, [
      "/opt/weather/operators/native-evidence/packages/${NATIVE_EVIDENCE_BUNDLE_SHA256}/native-evidence-remote-dispatch.sh",
    ]);
    const releaseControl = requireEntryId(
      discovered,
      "forced-remote:deploy/scripts/native-release-control-remote-dispatch.sh",
    );
    assert.deepEqual(releaseControl.invokedRuntimes, ["rust"]);
    assert.deepEqual(releaseControl.installedPaths, [
      "/opt/weather/operators/native-evidence/packages/${NATIVE_EVIDENCE_BUNDLE_SHA256}/native-release-control-remote-dispatch.sh",
    ]);
    const dashboardControl = requireEntryId(
      discovered,
      "forced-remote:deploy/scripts/native-dashboard-release-control.sh",
    );
    assert.ok(dashboardControl.invokedRuntimes.includes("rust"));
    assert.deepEqual(dashboardControl.installedPaths, [
      "/opt/weather/operators/native-evidence/packages/${NATIVE_EVIDENCE_BUNDLE_SHA256}/native-dashboard-release-control.sh",
    ]);
    const hostEvidence = requireEntryId(discovered, "native-binary:host-evidence");
    assert.equal(hostEvidence.classification, "release-control");
    assert.deepEqual(hostEvidence.installedPaths, [
      "/opt/weather/operators/native-evidence/packages/${NATIVE_EVIDENCE_BUNDLE_SHA256}/weather-evidence",
    ]);
    assert.equal(
      requireEntryId(
        discovered,
        "private-command:deploy/scripts/package-native-evidence-operator.sh",
      ).classification,
      "release-control",
    );
    assert.equal(
      requireEntryId(
        discovered,
        "private-command:scripts/install-native-evidence-operator.sh",
      ).classification,
      "release-control",
    );

    // require each actual worker one-shot without inventing a weather-operator binary
    for (const id of [
      "native-binary:server-worker-open-meteo-backfill",
      "native-binary:server-worker-public-stations-backfill",
      "native-binary:server-worker-tempest-backfill",
      "native-binary:server-worker-tide-backfill",
    ]) {
      const oneShot = requireEntryId(discovered, id);
      assert.equal(oneShot.command.includes("/opt/weather/bin/weather-worker"), true);
      assert.equal(oneShot.command.includes("weather-operator"), false);
    }
    const archive = requireEntryId(
      discovered,
      "systemd-service:scripts/systemd/native/weather-adjustment-archive.service",
    );
    assert.deepEqual(archive.invokedRuntimes, ["rust"]);
    const manifest = buildManifest(discovered);
    const nativeFit = requireEntryId(manifest.entries, "native-binary:runner-fit");
    assert.equal(nativeFit.sourcePath, "rust/crates/weather-runtime/src/bin/weather-maintenance-fit.rs");
    const pythonFit = requireEntryId(manifest.entries, "installer-allowlist:scripts/install-adjustment-maintenance-runner.sh#ADJUSTMENT_RUNNER_FILES:scripts/research/rain_refresh.py");
    assert.equal(pythonFit.runtime, "python");
    assert.equal(pythonFit.generations["rust-v1"].reachable, false);
    assert.equal(pythonFit.generations["node-v1"].reachable, true);
    const nodeApi = requireEntryId(manifest.entries, "compose-command:deploy/compose.yaml#api");
    assert.equal(nodeApi.generations["rust-v1"].reachable, false);
    assert.equal(nodeApi.generations.mixed.reachable, true);
    const nativeArchive = requireEntryId(
      manifest.entries,
      "systemd-service:scripts/systemd/native/weather-adjustment-archive.service",
    );
    nativeArchive.generations["node-v1"] = generationDeclaration(false, nativeArchive, null);
    nativeArchive.generations["node-v1-rollback"] = generationDeclaration(
      false,
      nativeArchive,
      null,
    );
    nativeArchive.generations["rust-v1"] = generationDeclaration(true, nativeArchive, null);
    rebuildGenerationPolicies(manifest);
    assert.equal(nativeArchive.generations["rust-v1"].reachable, true);
    await writeJson(rootDir, MANIFEST_PATH, manifest);
    const runtimeMap = await writeFixtureRuntimeMap(rootDir, manifest);
    // bind the installed executable modes independently of immutable runner source modes
    for (const id of ["runner-capture", "runner-fit", "runner-wind-fit"]) {
      const artifact = runtimeMap.artifacts.find(
        // select the actual native runner executable artifact
        (candidate) => candidate.sourceEntryId === `native-binary:${id}`,
      );
      assert.equal(artifact?.kind, "runner-file");
      assert.equal(artifact?.expectedMode, "0500");
    }
    const releaseControlArtifact = runtimeMap.artifacts.find(
      // select the installed owner-private sourced dispatcher bytes
      (artifact) => artifact.sourceEntryId === releaseControl.id,
    );
    assert.equal(releaseControlArtifact?.expectedMode, "0644");
    assert.equal(
      releaseControlArtifact?.path,
      "/opt/weather/operators/native-evidence/packages/${NATIVE_EVIDENCE_BUNDLE_SHA256}/native-release-control-remote-dispatch.sh",
    );
    const releaseControlContext = runtimeMap.contexts.find(
      // select the exact host package runtime context
      (context) => context.rootEntryId === releaseControl.id,
    );
    assert.equal(releaseControlContext?.profile, "blueberry-compose");
    assert.equal(releaseControlContext?.activationSource, "process-identity");
    const nativeApiContext = runtimeMap.contexts.find(
      // select the exact generation-switched production API context
      (context) => context.rootEntryId ===
        "runtime-generation-command:scripts/schema-preserving-release.mjs#api",
    );
    assert.deepEqual(nativeApiContext?.activationSourceByGeneration, {
      mixed: "never-active-alternative",
      "node-v1": "never-active-alternative",
      "node-v1-rollback": "never-active-alternative",
      "rust-v1": "container-inspect",
    });
    assert.equal(nativeApiContext?.activationSource, "never-active-alternative");
    assert.equal(nativeApiContext?.service, "api");
    assert.ok(nativeApiContext?.artifactIds.some(
      // require the actual native application bytes in the selected image context
      (artifactId) => artifactId.includes("native-binary:server-api#"),
    ));
    const nativeArchiveContext = runtimeMap.contexts.find(
      // select the exact generation-switched runner unit context
      (context) => context.rootEntryId ===
        "systemd-service:scripts/systemd/native/weather-adjustment-archive.service",
    );
    assert.equal(
      nativeArchiveContext?.activationSourceByGeneration?.["rust-v1"],
      "systemd-show",
    );
    assert.equal(
      nativeArchiveContext?.activationSourceByGeneration?.["node-v1"],
      "never-active-alternative",
    );
    const workerContext = runtimeMap.contexts.find(
      // select the exact generation-switched production worker context
      (context) => context.rootEntryId ===
        "runtime-generation-command:scripts/schema-preserving-release.mjs#worker",
    );

    // retain logical one-shot bindings without duplicating the shared worker executable bytes
    for (const entryId of [
      "native-binary:server-worker-open-meteo-backfill",
      "native-binary:server-worker-public-stations-backfill",
      "native-binary:server-worker-tempest-backfill",
      "native-binary:server-worker-tide-backfill",
    ]) {
      const binding = runtimeMap.bindings.find(
        // bind every one-shot to the selected worker instance
        (candidate) => candidate.contextId === workerContext?.id && candidate.entryId === entryId,
      );
      assert.deepEqual(binding?.artifactIds, []);
    }
    const rust = await validateRuntimeInventory({
      rootDir,
      manifestPath: MANIFEST_PATH,
      generation: "rust-v1",
      phase: "pre-cutover",
    });
    const mixed = await validateRuntimeInventory({
      rootDir,
      manifestPath: MANIFEST_PATH,
      generation: "mixed",
      phase: "pre-cutover",
    });
    assert.equal(rust.ok, true, JSON.stringify(rust.diagnostics));
    assert.equal(mixed.ok, true, JSON.stringify(mixed.diagnostics));
    // a packaged predecessor Python fitter cannot be made reachable in the Rust candidate
    pythonFit.generations["rust-v1"] = generationDeclaration(true, pythonFit, null);
    rebuildGenerationPolicies(manifest);
    await writeJson(rootDir, MANIFEST_PATH, manifest);
    await writeFixtureRuntimeMap(rootDir, manifest);
    const pythonCandidate = await validateFixture({ rootDir }, "rust-v1", "pre-cutover");
    assertDiagnostic(pythonCandidate, "generation.rust-python-application");
  });
});

// preserve predecessor business execution classification beside the native gateway
test("legacy business wrappers remain applications while native gateway stays control", async () => {
  const files = {
    "package.json": `${JSON.stringify({ name: "legacy-business-fixture", private: true })}\n`,
    "deploy/compose.yaml": "services:\n  api:\n    command: [node, apps/api/main.mjs]\n",
    "apps/api/main.mjs": "export const main = true;\n",
    "deploy/scripts/forecast-training-export.sh": [
      "#!/usr/bin/env bash",
      "exec node deploy/scripts/forecast-training-package.mjs",
      "",
    ].join("\n"),
    "deploy/scripts/forecast-training-package.mjs": "export const training = true;\n",
    "deploy/scripts/adjustment-evaluation-export.sh": [
      "#!/usr/bin/env bash",
      "exec node deploy/scripts/adjustment-evaluation-package.mjs",
      "",
    ].join("\n"),
    "deploy/scripts/adjustment-evaluation-package.mjs": "export const evaluation = true;\n",
    "deploy/scripts/native-generation-remote-ops.sh": [
      "#!/usr/bin/env bash",
      "exec \"$native_evidence_binary\" \"$@\"",
      "",
    ].join("\n"),
  };

  // isolate classification from the checked generated catalog
  await withBareFixture(files, async (rootDir) => {
    const discovered = await discoverFixture(rootDir);

    // retain each predecessor wrapper and its direct module as application work
    for (const id of [
      "private-command:deploy/scripts/adjustment-evaluation-export.sh",
      "private-command:deploy/scripts/forecast-training-export.sh",
      "import:deploy/scripts/adjustment-evaluation-package.mjs",
      "import:deploy/scripts/forecast-training-package.mjs",
    ]) {
      assert.equal(requireEntryId(discovered, id).classification, "application");
    }
    const gateway = requireEntryId(
      discovered,
      "forced-remote:deploy/scripts/native-generation-remote-ops.sh",
    );
    assert.equal(gateway.classification, "release-control");
    assert.deepEqual(gateway.invokedRuntimes, ["rust"]);
  });
});

// prove the checked repository inventory matches every honest supported generation
test("checked repository inventory passes Node rollback Rust and mixed generations", async () => {
  const baseline = await validateRuntimeInventory({
    rootDir: REPOSITORY_ROOT,
    generation: "node-v1",
    phase: "baseline",
  });
  const rollback = await validateRuntimeInventory({
    rootDir: REPOSITORY_ROOT,
    generation: "node-v1",
    phase: "rollback-drill",
  });
  const mixed = await validateRuntimeInventory({
    rootDir: REPOSITORY_ROOT,
    generation: "mixed",
    phase: "pre-cutover",
  });
  const rust = await validateRuntimeInventory({
    rootDir: REPOSITORY_ROOT,
    generation: "rust-v1",
    phase: "pre-cutover",
  });
  assert.equal(baseline.ok, true, JSON.stringify(baseline.diagnostics));
  assert.equal(rollback.ok, true, JSON.stringify(rollback.diagnostics));
  assert.equal(mixed.ok, true, JSON.stringify(mixed.diagnostics));
  assert.equal(rust.ok, true, JSON.stringify(rust.diagnostics));
  const actualIds = inventoryIds(baseline.discovered);
  const expectedWorkerSources = [
    "apps/worker/src/backfill-cli.ts",
    "apps/worker/src/forecast-adjustment-bundle-cli.ts",
    "apps/worker/src/forecast-adjustment-evaluate-cli.ts",
    "apps/worker/src/forecast-adjustment-evidence-cli.ts",
    "apps/worker/src/forecast-adjustment-performance-cli.ts",
  ];

  // retain every application and forecast one-shot canonical source
  for (const sourcePath of expectedWorkerSources) {
    assert.ok(actualIds.has(`import:${sourcePath}`), `missing worker source ${sourcePath}`);
  }
  const nativeIds = [
    "forced-remote:deploy/scripts/native-dashboard-release-control.sh",
    "forced-remote:deploy/scripts/native-evidence-remote-dispatch.sh",
    "forced-remote:deploy/scripts/native-generation-remote-ops.sh",
    "forced-remote:deploy/scripts/native-release-control-remote-dispatch.sh",
    "native-binary:host-evidence",
    "native-binary:server-worker-open-meteo-backfill",
    "native-binary:server-worker-public-stations-backfill",
    "native-binary:server-worker-tempest-backfill",
    "native-binary:server-worker-tide-backfill",
  ];

  // retain the real host package and every native worker one-shot
  for (const id of nativeIds) {
    assert.ok(actualIds.has(id), `missing native runtime entry ${id}`);
  }
  const performanceParent = "import:apps/worker/src/forecast-adjustment-performance-cli.ts";
  const reviewedConstructedImports = [
    "deploy/scripts/adjustment-evaluation-package.mjs",
    "deploy/scripts/forecast-adjustment-scorecard-contract.mjs",
  ];

  // retain only the reviewed performance CLI constructed imports
  for (const sourcePath of reviewedConstructedImports) {
    const entry = requireEntryId(baseline.discovered, `import:${sourcePath}`);
    let hasPerformanceParent = false;

    // inspect exact typed import parents
    for (const parent of entry.parents) {
      // match the canonical performance CLI source
      if (parent.type === "import" && parent.id === performanceParent) {
        hasPerformanceParent = true;
      }
    }
    assert.equal(hasPerformanceParent, true, `missing performance parent for ${sourcePath}`);
  }

  const manifest = JSON.parse(await readFile(join(REPOSITORY_ROOT, MANIFEST_PATH), "utf8"));
  let nodeApplicationCount = 0;

  // verify every current Node application remains explicit and replaceable
  for (const entry of manifest.entries) {
    // inspect only current Node application rows
    if (entry.classification === "application" && entry.runtime === "node") {
      nodeApplicationCount += 1;
      assert.equal(entry.generations["node-v1"].reachable, true);
      assert.equal(entry.generations["node-v1-rollback"].reachable, true);
      assert.equal(entry.generations["rust-v1"].reachable, false);
      assert.equal(typeof entry.generations["node-v1"].replacement, "string");
      assert.ok(entry.generations["node-v1"].replacement.length > 0);
      assert.equal(entry.generations["node-v1"].replacement.includes("weather-operator"), false);
    }
  }
  assert.ok(nodeApplicationCount > 0);

  // require actual native ownership without leaking it into predecessor rollback generations
  for (const id of nativeIds) {
    const entry = requireEntryId(manifest.entries, id);
    assert.equal(entry.generations["node-v1"].reachable, false);
    assert.equal(entry.generations["node-v1-rollback"].reachable, false);
    assert.equal(entry.generations["rust-v1"].reachable, true);
  }
  const legacyApplications = [
    "import:deploy/scripts/forecast-training-package.mjs",
    "private-command:deploy/scripts/adjustment-evaluation-export.sh",
    "private-command:deploy/scripts/forecast-training-export.sh",
    "private-command:deploy/scripts/install-adjustment-scorecard.sh",
    "shell-invocation:deploy/scripts/public-stations-backfill.sh",
    "shell-invocation:deploy/scripts/tempest-backfill.sh",
    "shell-invocation:deploy/scripts/tide-backfill.sh",
  ];

  // keep predecessor business execution explicit and unreachable from the Rust candidate
  for (const id of legacyApplications) {
    const entry = requireEntryId(manifest.entries, id);
    assert.equal(entry.classification, "application");
    assert.equal(entry.generations["node-v1"].reachable, true);
    assert.equal(entry.generations["node-v1-rollback"].reachable, true);
    assert.equal(entry.generations["rust-v1"].reachable, false);
  }
  assert.equal(baseline.discovered.length, manifest.entries.length);
});

// retain reviewed metadata policy while permitting honest finite inventory growth
test("reviewed static inventory preserves metadata policy and derived map", async () => {
  const inventoryText = await readFile(join(REPOSITORY_ROOT, MANIFEST_PATH), "utf8");
  const runtimeMapText = await readFile(join(REPOSITORY_ROOT, RUNTIME_MAP_PATH), "utf8");
  const inventory = JSON.parse(inventoryText);
  const runtimeMap = JSON.parse(runtimeMapText);
  const apiId = "import:apps/api/src/index.ts";
  const receiptsId = "import:scripts/backend-performance-receipts.mjs";
  const hostObservationId = "import:scripts/backend-host-observation.mjs";
  const schemaReleaseId = "import:scripts/schema-preserving-release.mjs";
  const staleDatabasePageId = "import:scripts/database-page-performance.mjs";
  const contractBuildIds = new Set([
    "package-script:packages/contracts/package.json#build",
    "package-script:packages/contracts/package.json#typecheck",
  ]);
  const releaseControlAnnotations = {
    owner: "performance-evidence",
    parityTest: "node --test scripts/backend-performance-receipts.test.mjs scripts/database-page-performance.test.mjs",
    accountingScope: "release-control:performance-evidence",
    generations: {
      "node-v1": { reachable: true, command: null, probe: null, replacement: null },
      "node-v1-rollback": { reachable: true, command: null, probe: null, replacement: null },
      "rust-v1": { reachable: true, command: null, probe: null, replacement: null },
      mixed: { reachable: true, command: null, probe: null, replacement: null },
    },
  };
  const apiAnnotations = {
    owner: "api",
    parityTest: "npm run test:rust:oracle",
    accountingScope: "application:transitive-source-closure",
    generations: {
      "node-v1": { reachable: true, command: null, probe: null, replacement: "/opt/weather/bin/weather-api" },
      "node-v1-rollback": { reachable: true, command: null, probe: null, replacement: "/opt/weather/bin/weather-api" },
      "rust-v1": { reachable: false, command: null, probe: null, replacement: "/opt/weather/bin/weather-api" },
      mixed: { reachable: true, command: null, probe: null, replacement: "/opt/weather/bin/weather-api" },
    },
  };
  assert.ok(inventory.entries.length > 0);
  assert.deepEqual(reviewedAnnotations(requireEntryId(inventory.entries, apiId)), apiAnnotations);
  assert.deepEqual(
    reviewedAnnotations(requireEntryId(inventory.entries, receiptsId)),
    releaseControlAnnotations,
  );
  assert.deepEqual(
    reviewedAnnotations(requireEntryId(inventory.entries, hostObservationId)),
    releaseControlAnnotations,
  );
  assert.deepEqual(
    reviewedAnnotations(requireEntryId(inventory.entries, schemaReleaseId)),
    releaseControlAnnotations,
  );
  assert.equal(inventory.entries.some(
    // reject the retired import identity
    (entry) => entry.id === staleDatabasePageId,
  ), false);

  const buildGeneration = { reachable: true, command: "@entry.command", probe: null, replacement: null };

  // keep both contract workspace commands outside the runtime closure
  for (const contractId of contractBuildIds) {
    const entry = requireEntryId(inventory.entries, contractId);
    assert.equal(entry.classification, "build");
    assert.deepEqual(reviewedAnnotations(entry), {
      owner: "release-operations",
      parityTest: "npm run check:fast",
      accountingScope: "build-only",
      generations: {
        "node-v1": buildGeneration,
        "node-v1-rollback": buildGeneration,
        "rust-v1": buildGeneration,
        mixed: buildGeneration,
      },
    });
  }

  // verify every generation policy against reviewed rows
  for (const generation of ["node-v1", "node-v1-rollback", "rust-v1", "mixed"]) {
    const reachableIds = inventory.generationPolicies[generation].reachableEntryIds;
    assert.ok(reachableIds.length > 0);
    assert.equal(reachableIds.includes(hostObservationId), true);
    assert.equal(reachableIds.includes(schemaReleaseId), true);
    assert.equal(reachableIds.includes(staleDatabasePageId), false);
    assert.deepEqual(reachableIds, sortedIds(inventory.entries, generation));
  }
  assert.deepEqual(
    inventory.generationPolicies.mixed.allowedUnionEntryIds,
    sortedIds(inventory.entries, "mixed"),
  );

  // retain complete reviewed annotations without hard-coding inventory cardinality
  for (const entry of inventory.entries) {
    assert.equal(typeof entry.owner, "string");
    assert.ok(entry.owner.length > 0);
    assert.equal(typeof entry.parityTest, "string");
    assert.ok(entry.parityTest.length > 0);
    assert.equal(typeof entry.accountingScope, "string");
    assert.ok(entry.accountingScope.length > 0);
    assert.equal(JSON.stringify(entry.generations).includes("weather-operator"), false);
  }

  const { deriveRuntimeLiveMap } = await runtimeLiveMapApi();
  const derivedRuntimeMap = deriveRuntimeLiveMap({ entries: inventory.entries });
  assert.deepEqual(runtimeMap, derivedRuntimeMap);
  assert.ok(runtimeMap.artifacts.length > 0);
  assert.ok(runtimeMap.bindings.length > 0);
  assert.ok(runtimeMap.contexts.length > 0);
  assert.ok(Buffer.byteLength(inventoryText) > 0);
  assert.ok(Buffer.byteLength(runtimeMapText) > 0);
});

// reject a Node application introduced into the Rust candidate
test("rust-v1 rejects every reachable Node application", async () => {
  // isolate one policy mutation
  await withFixture(async (fixture) => {
    const manifest = structuredClone(fixture.manifest);
    const entry = requireEntry(manifest, "compose-command");
    entry.generations["rust-v1"].reachable = true;
    manifest.generationPolicies["rust-v1"].reachableEntryIds.push(entry.id);
    manifest.generationPolicies["rust-v1"].reachableEntryIds.sort();
    await writeJson(fixture.rootDir, MANIFEST_PATH, manifest);

    const result = await validateFixture(fixture, "rust-v1", "pre-cutover");
    assertDiagnostic(result, "generation.rust-node-application");
  });
});

// reject any mixed reachability outside the finite declared union
test("mixed accepts only its exact finite transition union", async () => {
  // isolate one union mutation
  await withFixture(async (fixture) => {
    const manifest = structuredClone(fixture.manifest);
    const removed = manifest.generationPolicies.mixed.allowedUnionEntryIds.pop();
    assert.ok(manifest.generationPolicies.mixed.reachableEntryIds.includes(removed));
    await writeJson(fixture.rootDir, MANIFEST_PATH, manifest);

    const result = await validateFixture(fixture, "mixed", "pre-cutover");
    assertDiagnostic(result, "generation.mixed-union");
  });
});

// prevent research paths from hiding application dependencies
test("application imports cannot be relabeled offline oracle or control helpers", async () => {
  // exercise every forbidden hidden-helper classification
  for (const classification of ["offline-research", "oracle", "release-control"]) {
    // isolate each classification failure
    await withFixture(async (fixture) => {
      const manifest = structuredClone(fixture.manifest);
      const helper = requireEntry(
        manifest,
        "import",
        "scripts/research/application-helper.mjs",
      );
      helper.classification = classification;
      await writeJson(fixture.rootDir, MANIFEST_PATH, manifest);

      const result = await validateFixture(fixture);
      assertDiagnostic(result, "entry.classification");
    });
  }
});

// fail every source identity and parent drift mode closed
test("source root entrypoint and import-parent drift invalidate the manifest", async () => {
  const cases = [
    {
      name: "root package digest",
      expected: "entry.changed",
      // mutate the root package source bytes
      mutate: async function mutateRoot(fixture) {
        const packagePath = join(fixture.rootDir, "package.json");
        const original = await readFile(packagePath, "utf8");
        await writeFile(packagePath, original.replace("archive.mjs", "archive.mjs --once"));
      },
    },
    {
      name: "compose entrypoint",
      expected: "entry.changed",
      // mutate one executable command
      mutate: async function mutateEntrypoint(fixture) {
        const composePath = join(fixture.rootDir, "deploy/compose.yaml");
        const original = await readFile(composePath, "utf8");
        await writeFile(composePath, original.replace("apps/api/main.mjs", "apps/api/health.mjs"));
      },
    },
    {
      name: "import parent",
      expected: "entry.changed",
      // move one import edge to another parent
      mutate: async function mutateImportParent(fixture) {
        const mainPath = join(fixture.rootDir, "apps/api/main.mjs");
        const helperPath = join(fixture.rootDir, "scripts/research/application-helper.mjs");
        await writeFile(mainPath, `export const main = true;\n`);
        const helper = await readFile(helperPath, "utf8");
        await writeFile(
          join(fixture.rootDir, "apps/api/health.mjs"),
          `import "../../scripts/research/application-helper.mjs";\n${helper}`,
        );
      },
    },
  ];

  // execute each independent drift case
  for (const driftCase of cases) {
    // isolate one source drift
    await withFixture(async (fixture) => {
      await driftCase.mutate(fixture);
      const result = await validateFixture(fixture);
      assertDiagnostic(result, driftCase.expected);
    });
  }
});

// reject unknown new and removed execution closure
test("unknown new and removed entrypoints and imports fail closed", async () => {
  const cases = [
    {
      expected: "entry.extra",
      // add one undeclared package one-shot
      mutate: async function addPackageScript(fixture) {
        const packagePath = join(fixture.rootDir, "package.json");
        const value = JSON.parse(await readFile(packagePath, "utf8"));
        value.scripts.model = "node scripts/research/controller.mjs";
        await writeJson(fixture.rootDir, "package.json", value);
      },
    },
    {
      expected: "entry.missing",
      // remove one declared systemd service
      mutate: async function removeSystemdService(fixture) {
        await unlink(join(
          fixture.rootDir,
          "scripts/systemd/system/weather-adjustment.service",
        ));
      },
    },
    {
      expected: "entry.changed",
      // expand one forced remote command surface
      mutate: async function expandForcedRemote(fixture) {
        const remotePath = join(fixture.rootDir, "deploy/scripts/remote-ops.sh");
        const original = await readFile(remotePath, "utf8");
        await writeFile(
          remotePath,
          original.replace("esac", "  recover) exec node deploy/scripts/status.mjs ;;\nesac"),
        );
      },
    },
    {
      expected: "entry.extra",
      // expand one installed runner allowlist
      mutate: async function expandInstaller(fixture) {
        const installerPath = join(
          fixture.rootDir,
          "scripts/install-adjustment-maintenance-runner.sh",
        );
        const original = await readFile(installerPath, "utf8");
        await writeFile(
          installerPath,
          original.replace(
            "  \"scripts/research/controller.mjs\"",
            "  \"scripts/research/controller.mjs\"\n  \"scripts/research/evidence.mjs\"",
          ),
        );
      },
    },
    {
      expected: "entry.changed",
      // introduce one new transitive local import
      mutate: async function addImport(fixture) {
        await writeFile(
          join(fixture.rootDir, "scripts/research/leaf.mjs"),
          `import "./evidence.mjs";\nexport const leaf = "fixture";\n`,
        );
      },
    },
  ];

  // exercise every closure expansion and removal
  for (const closureCase of cases) {
    // isolate one closure mutation
    await withFixture(async (fixture) => {
      await closureCase.mutate(fixture);
      const result = await validateFixture(fixture);
      assertDiagnostic(result, closureCase.expected);
    });
  }
});

// reject malformed ambiguous and escaping inputs
test("malformed duplicate traversal symlink and dynamic mappings fail closed", async () => {
  // reject malformed JSON before validation
  await withFixture(async (fixture) => {
    await writeFile(join(fixture.rootDir, MANIFEST_PATH), "{not-json\n");
    assertDiagnostic(await validateFixture(fixture), "manifest.syntax");
  });

  // reject malformed manifest structure
  await withFixture(async (fixture) => {
    const manifest = structuredClone(fixture.manifest);
    manifest.schemaVersion = "backend-runtime-inventory/future";
    await writeJson(fixture.rootDir, MANIFEST_PATH, manifest);
    assertDiagnostic(await validateFixture(fixture), "manifest.schema");
  });

  // reject duplicate stable identities
  await withFixture(async (fixture) => {
    const manifest = structuredClone(fixture.manifest);
    manifest.entries.push(structuredClone(manifest.entries[0]));
    await writeJson(fixture.rootDir, MANIFEST_PATH, manifest);
    assertDiagnostic(await validateFixture(fixture), "manifest.duplicate-id");
  });

  // reject repository traversal in declared source paths
  await withFixture(async (fixture) => {
    const manifest = structuredClone(fixture.manifest);
    manifest.entries[0].sourcePath = "../outside.mjs";
    await writeJson(fixture.rootDir, MANIFEST_PATH, manifest);
    assertDiagnostic(await validateFixture(fixture), "path.traversal");
  });

  // reject a source link that escapes the repository
  await withFixture(async (fixture) => {
    const outsideRoot = await mkdtemp(join(tmpdir(), "weather-runtime-outside-"));
    try {
      const sourcePath = join(fixture.rootDir, "apps/api/health.mjs");
      const outsidePath = join(outsideRoot, "outside.mjs");
      await writeFile(outsidePath, `export const outside = true;\n`);
      await unlink(sourcePath);
      await symlink(outsidePath, sourcePath);
      assertDiagnostic(await validateFixture(fixture), "path.symlink-escape");
    } finally {
      await rm(outsideRoot, { force: true, recursive: true });
    }
  });

  // reject a transitive import link that escapes the repository
  await withFixture(async (fixture) => {
    const outsideRoot = await mkdtemp(join(tmpdir(), "weather-runtime-outside-"));
    try {
      const sourcePath = join(fixture.rootDir, "scripts/research/leaf.mjs");
      const outsidePath = join(outsideRoot, "outside-leaf.mjs");
      await writeFile(outsidePath, `export const outsideLeaf = true;\n`);
      await unlink(sourcePath);
      await symlink(outsidePath, sourcePath);
      assertDiagnostic(await validateFixture(fixture), "path.symlink-escape");
    } finally {
      await rm(outsideRoot, { force: true, recursive: true });
    }
  });

  // reject computed installer allowlist members
  await withFixture(async (fixture) => {
    const installerPath = join(
      fixture.rootDir,
      "scripts/install-adjustment-maintenance-runner.sh",
    );
    await writeFile(
      installerPath,
      `#!/usr/bin/env bash\nreadonly -a RUNNER_FILES=(\n  "\${RUNTIME_ROOT}/archive.mjs"\n)\n`,
    );
    assertDiagnostic(await validateFixture(fixture), "discovery.unsupported");
  });

  // reject app-reachable nonliteral dynamic imports
  await withFixture(async (fixture) => {
    await writeFile(
      join(fixture.rootDir, "apps/api/main.mjs"),
      `const target = "../../scripts/research/application-helper.mjs";\nexport const main = await import(target);\n`,
    );
    assertDiagnostic(await validateFixture(fixture), "discovery.dynamic-import");
  });

  // reject app-reachable nonliteral require calls
  await withFixture(async (fixture) => {
    await writeFile(
      join(fixture.rootDir, "apps/api/main.mjs"),
      `const target = "../../scripts/research/application-helper.mjs";\nexport const main = require(target);\n`,
    );
    assertDiagnostic(await validateFixture(fixture), "discovery.unsupported");
  });
});

// prove successful and failing checks are deterministic and read only
test("validation is deterministic read only and excludes its own manifest hash", async () => {
  // isolate one complete read-only proof
  await withFixture(async (fixture) => {
    const successBefore = await snapshotTree(fixture.rootDir);
    const first = await validateFixture(fixture);
    const second = await validateFixture(fixture);
    const successAfter = await snapshotTree(fixture.rootDir);
    assert.deepEqual(second, first);
    assert.deepEqual(successAfter, successBefore);

    const manifestPath = join(fixture.rootDir, MANIFEST_PATH);
    await writeFile(manifestPath, `${JSON.stringify(fixture.manifest)}\n`);
    const reformatted = await validateFixture(fixture);
    assert.equal(reformatted.ok, true, JSON.stringify(reformatted.diagnostics));
    assert.deepEqual(reformatted.discovered, first.discovered);

    await writeFile(manifestPath, "{broken\n");
    const failureBefore = await snapshotTree(fixture.rootDir);
    const failedFirst = await validateFixture(fixture);
    const failedSecond = await validateFixture(fixture);
    const failureAfter = await snapshotTree(fixture.rootDir);
    assertDiagnostic(failedFirst, "manifest.syntax");
    assert.deepEqual(failedSecond, failedFirst);
    assert.deepEqual(failureAfter, failureBefore);
  });
});

// reject unknown command line context deterministically
test("argument parsing admits only declared generations and phases", async () => {
  assert.deepEqual([...GENERATIONS], ["node-v1", "rust-v1", "mixed"]);
  assert.ok(PHASES.includes("baseline"));
  assert.ok(PHASES.includes("rollback-drill"));
  const parsed = parseRuntimeInventoryArguments([
    "--generation",
    "node-v1",
    "--phase",
    "baseline",
  ]);
  assert.equal(parsed.generation, "node-v1");
  assert.equal(parsed.phase, "baseline");

  // reject an unknown generation through the shared validator
  await withFixture(async (fixture) => {
    const result = await validateFixture(fixture, "future-v9", "baseline");
    assertDiagnostic(result, "argument.invalid");
  });
});
