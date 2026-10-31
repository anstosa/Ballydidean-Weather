import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { execFileSync, spawn, spawnSync } from "node:child_process";
import { once } from "node:events";
import { readdirSync, readFileSync } from "node:fs";
import { chmod, mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { basename, join, relative, resolve, sep } from "node:path";
import test from "node:test";

const repoRoot = resolve(import.meta.dirname, "../..");
const deployRoot = join(repoRoot, "deploy");
const scriptsRoot = join(deployRoot, "scripts");

// render a deterministic Compose model
function renderCompose(overrides = []) {
  const files = [join(deployRoot, "compose.yaml"), ...overrides.map((name) => join(deployRoot, name))];
  const argumentsList = [
    "compose",
    "--project-name",
    "weather",
    "--env-file",
    join(deployRoot, ".env.example"),
  ];

  // add ordered Compose files
  for (const file of files) {
    argumentsList.push("--file", file);
  }

  argumentsList.push("config", "--format", "json");
  return JSON.parse(
    execFileSync("docker", argumentsList, { cwd: repoRoot, encoding: "utf8" }),
  );
}

// collect files recursively
function collectFiles(root) {
  return readdirSync(root, { withFileTypes: true }).flatMap((entry) => {
    const path = join(root, entry.name);
    return entry.isDirectory() ? collectFiles(path) : [path];
  });
}

// read one repository artifact
function read(path) {
  return readFileSync(join(repoRoot, path), "utf8");
}

// extract one executable workflow run block
function workflowRunScript(workflow, stepName) {
  const lines = workflow.split("\n");
  const stepIndex = lines.findIndex((line) => line.trim() === `- name: ${stepName}`);

  // require the named step
  if (stepIndex < 0) {
    throw new Error(`workflow step not found: ${stepName}`);
  }

  const runIndex = lines.findIndex(
    (line, index) => index > stepIndex && line.trim() === "run: |",
  );

  // require one shell block
  if (runIndex < 0) {
    throw new Error(`workflow run block not found: ${stepName}`);
  }

  const indentation = lines[runIndex].indexOf("run:");
  const script = [];

  // collect the indented shell body
  for (let index = runIndex + 1; index < lines.length; index += 1) {
    const line = lines[index];
    const leading = line.search(/\S/u);

    // stop at the next workflow key
    if (leading >= 0 && leading <= indentation) {
      break;
    }

    script.push(line.slice(indentation + 2));
  }

  return script.join("\n");
}

// list secret sources
function secretSources(service) {
  return (service.secrets ?? []).map((secret) => secret.source).sort();
}

// reserve one loopback port
async function reservePort() {
  const listener = createServer();
  listener.listen(0, "127.0.0.1");
  await once(listener, "listening");
  const address = listener.address();

  // require a TCP listener address
  if (address === null || typeof address === "string") {
    throw new Error("failed to reserve a TCP port");
  }

  listener.close();
  await once(listener, "close");
  return address.port;
}

// wait for a bounded local startup
async function waitForServer(url) {
  // retry only the disposable local server
  for (let attempt = 0; attempt < 50; attempt += 1) {
    try {
      const response = await fetch(url);

      // accept any completed HTTP response
      if (response.status > 0) {
        return;
      }
    } catch {
      await new Promise((resolveWait) => setTimeout(resolveWait, 50));
    }
  }

  throw new Error(`server did not start: ${url}`);
}

// verify the production topology
test("production Compose has the exact six-network isolation matrix", () => {
  const compose = renderCompose(["compose.verify.yaml"]);
  const services = Object.keys(compose.services).sort();
  assert.deepEqual(services, ["api", "cloudflared", "migration", "postgres", "web", "worker"]);
  assert.deepEqual(Object.keys(compose.networks).sort(), [
    "data",
    "edge",
    "map_egress",
    "provider_egress",
    "tunnel_egress",
    "web_api",
  ]);
  assert.equal(compose.networks.edge.internal, true);
  assert.equal(compose.networks.web_api.internal, true);
  assert.equal(compose.networks.data.internal, true);
  assert.notEqual(compose.networks.map_egress.internal, true);
  assert.notEqual(compose.networks.provider_egress.internal, true);
  assert.notEqual(compose.networks.tunnel_egress.internal, true);

  const memberships = Object.fromEntries(
    Object.entries(compose.services).map(([name, service]) => [
      name,
      Object.keys(service.networks).sort(),
    ]),
  );
  assert.deepEqual(memberships, {
    api: ["data", "web_api"],
    cloudflared: ["edge", "tunnel_egress"],
    migration: ["data"],
    postgres: ["data"],
    web: ["edge", "map_egress", "web_api"],
    worker: ["data", "provider_egress"],
  });

  // reject undeclared exposure and dual egress
  for (const [name, service] of Object.entries(compose.services)) {
    assert.equal(service.platform, "linux/arm64", `${name} must target ARM64`);
    assert.equal(service.ports, undefined, `${name} must publish no production ports`);
    const networks = Object.keys(service.networks);
    const egressNetworks = networks.filter(
      // count only outbound network boundaries
      (network) => ["map_egress", "provider_egress", "tunnel_egress"].includes(network),
    );
    assert.equal(
      egressNetworks.length <= 1,
      true,
      `${name} must join at most one egress network`,
    );
  }
});

// verify hardening and capacity
test("services are non-root, read-only, bounded, ordered, and healthy", () => {
  const compose = renderCompose(["compose.verify.yaml"]);
  const hardened = ["api", "cloudflared", "migration", "postgres", "web", "worker"];

  // check every container boundary
  for (const name of hardened) {
    const service = compose.services[name];
    assert.ok(service.user && !/^0(?::0)?$/u.test(service.user), `${name} must be non-root`);
    assert.equal(service.read_only, true, `${name} must use a read-only root filesystem`);
    assert.ok(service.cap_drop.includes("ALL"), `${name} must drop all capabilities`);
    assert.ok(
      service.security_opt.includes("no-new-privileges:true"),
      `${name} must disable privilege escalation`,
    );
    assert.ok(Number(service.pids_limit) > 0, `${name} must cap processes`);
    assert.ok(service.deploy.resources.limits.cpus, `${name} must cap CPU`);
    assert.ok(service.deploy.resources.limits.memory, `${name} must cap memory`);
    assert.ok(Number(service.mem_limit) > 0, `${name} must enforce memory outside Swarm`);
    assert.ok(service.logging.options["max-size"], `${name} must rotate logs`);
    assert.ok(service.stop_grace_period, `${name} must bound shutdown`);
  }

  assert.equal(compose.services.api.user, "10002:10002");
  assert.equal(compose.services.worker.user, "10002:10002");
  assert.equal(compose.services.web.user, "10002:10002");
  assert.equal(compose.services.postgres.user, "999:999");
  assert.equal(compose.services.cloudflared.user, "65532:65532");
  assert.equal(Number(compose.services.postgres.deploy.resources.limits.memory), 536870912);
  assert.equal(Number(compose.services.api.deploy.resources.limits.memory), 268435456);
  assert.equal(Number(compose.services.worker.deploy.resources.limits.memory), 268435456);
  assert.equal(Number(compose.services.web.deploy.resources.limits.memory), 402653184);
  assert.equal(Number(compose.services.cloudflared.deploy.resources.limits.memory), 134217728);
  assert.equal(Number(compose.services.postgres.mem_limit), 536870912);
  assert.equal(Number(compose.services.migration.mem_limit), 268435456);
  assert.equal(Number(compose.services.api.mem_limit), 268435456);
  assert.equal(Number(compose.services.worker.mem_limit), 268435456);
  assert.equal(Number(compose.services.web.mem_limit), 402653184);
  assert.equal(Number(compose.services.cloudflared.mem_limit), 134217728);
  assert.equal(compose.services.migration.depends_on.postgres.condition, "service_healthy");
  assert.equal(compose.services.api.depends_on.migration.condition, "service_completed_successfully");
  assert.equal(compose.services.worker.depends_on.migration.condition, "service_completed_successfully");
  assert.equal(compose.services.web.depends_on.api.condition, "service_healthy");
  assert.equal(compose.services.cloudflared.depends_on.web.condition, "service_healthy");

  // require health probes on steady services
  for (const name of ["api", "cloudflared", "postgres", "web", "worker"]) {
    assert.ok(compose.services[name].healthcheck.test, `${name} must define a health probe`);
  }
});

// verify role-specific secret access
test("database and connector secrets are scoped to least-privilege consumers", () => {
  const compose = renderCompose(["compose.verify.yaml"]);
  assert.deepEqual(secretSources(compose.services.api), ["weather_api_password"]);
  assert.deepEqual(secretSources(compose.services.worker), [
    "weather_tempest_api_key",
    "weather_worker_ingest_password",
  ]);
  assert.deepEqual(secretSources(compose.services.migration), ["weather_migration_owner_password"]);
  assert.deepEqual(secretSources(compose.services.cloudflared), ["cloudflare_tunnel_token"]);
  assert.deepEqual(secretSources(compose.services.web), [
    "weather_xweather_client_id",
    "weather_xweather_client_secret",
  ]);
  assert.deepEqual(secretSources(compose.services.postgres), [
    "weather_postgres_admin_password",
    "weather_postgres_api_password",
    "weather_postgres_ingest_password",
    "weather_postgres_owner_password",
    "weather_postgres_training_export_password",
  ]);
  assert.equal(
    compose.services.postgres.environment.POSTGRES_PASSWORD_FILE,
    "/run/secrets/weather_admin_password",
  );

  // isolate the administrator credential
  for (const [name, service] of Object.entries(compose.services)) {
    if (name !== "postgres") {
      assert.equal(secretSources(service).includes("weather_postgres_admin_password"), false);
      assert.equal(
        secretSources(service).includes("weather_postgres_training_export_password"),
        false,
      );
    }
  }

  // exclude operator-only training paths
  for (const serviceName of ["api", "web", "worker", "migration"]) {
    assert.doesNotMatch(
      JSON.stringify(compose.services[serviceName]),
      /weather_training_export|\.weather-data|\.weather-models|model-evidence/u,
    );
  }

  // require distinct host-owned secret sources
  const credentialSecretFiles = Object.entries(compose.secrets)
    .filter(([name]) => name !== "cloudflare_tunnel_token")
    .map(([, secret]) => secret.file);
  assert.equal(credentialSecretFiles.length, 11);
  assert.equal(new Set(credentialSecretFiles).size, credentialSecretFiles.length);

  // reject ignored compose ownership metadata
  for (const service of Object.values(compose.services)) {
    for (const secret of service.secrets ?? []) {
      assert.equal(secret.uid, undefined, `${secret.source} must use host ownership`);
      assert.equal(secret.gid, undefined, `${secret.source} must use host ownership`);
      assert.equal(secret.mode, undefined, `${secret.source} must use host mode`);
    }
  }

  const environments = JSON.stringify(
    Object.fromEntries(
      Object.entries(compose.services).map(([name, service]) => [name, service.environment ?? {}]),
    ),
  );
  assert.doesNotMatch(environments, /PASSWORD=(?!_FILE)|TUNNEL_TOKEN=/u);
  assert.match(environments, /WEATHER_DATABASE_PASSWORD_FILE/u);
});

test("PostgreSQL bootstrap rejects equal administrator and owner credentials", async () => {
  const directory = await mkdtemp(join(tmpdir(), "weather-postgres-separation-"));
  const bin = join(directory, "bin");
  const admin = join(directory, "admin");
  const owner = join(directory, "owner");
  const api = join(directory, "api");
  const ingest = join(directory, "ingest");
  const trainingExport = join(directory, "training-export");

  try {
    await mkdir(bin);
    await Promise.all([
      writeFile(admin, "shared-password\n"),
      writeFile(owner, "shared-password\n"),
      writeFile(api, "api-password\n"),
      writeFile(ingest, "ingest-password\n"),
      writeFile(trainingExport, "training-export-password\n"),
      writeFile(join(bin, "psql"), "#!/usr/bin/env bash\ncat >/dev/null\n"),
    ]);
    await chmod(join(bin, "psql"), 0o700);
    const result = spawnSync(join(deployRoot, "postgres/010-create-runtime-roles.sh"), [], {
      cwd: repoRoot,
      encoding: "utf8",
      env: {
        ...process.env,
        PATH: `${bin}:${process.env.PATH}`,
        WEATHER_ADMIN_PASSWORD_FILE: admin,
        WEATHER_API_PASSWORD_FILE: api,
        WEATHER_INGEST_PASSWORD_FILE: ingest,
        WEATHER_OWNER_PASSWORD_FILE: owner,
        WEATHER_TRAINING_EXPORT_PASSWORD_FILE: trainingExport,
      },
    });
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /administrator and owner passwords must differ/u);
  } finally {
    await rm(directory, { force: true, recursive: true });
  }
});

// verify retained administrator reconciliation
test("PostgreSQL reconciles retained administrator credentials before network startup", () => {
  const compose = renderCompose(["compose.verify.yaml"]);
  const postgres = compose.services.postgres;
  const bootstrap = read("deploy/postgres/010-create-runtime-roles.sh");
  const entrypoint = read("deploy/postgres/postgres-admin-entrypoint.sh");
  const runtimeAcl = read("deploy/postgres/runtime-acl-v2.sql");
  const entrypointMount = postgres.volumes.find(
    (volume) => volume.target === "/usr/local/bin/weather-postgres-entrypoint",
  );
  assert.deepEqual(postgres.entrypoint, ["/usr/local/bin/weather-postgres-entrypoint"]);
  assert.deepEqual(postgres.command, ["postgres"]);
  assert.ok(entrypointMount);
  assert.equal(entrypointMount.read_only, true);
  assert.deepEqual(postgres.healthcheck.test, [
    "CMD",
    "/usr/local/bin/weather-postgres-entrypoint",
    "health",
  ]);
  assert.match(entrypoint, /PG_VERSION/u);
  assert.match(entrypoint, /listen_addresses=''/u);
  assert.match(entrypoint, /pg_ctl[\s\S]*start/u);
  assert.match(entrypoint, /ALTER ROLE postgres WITH LOGIN PASSWORD/u);
  assert.match(entrypoint, /pg_db_role_setting/u);
  assert.match(entrypoint, /IN DATABASE %I RESET ALL/u);
  assert.match(bootstrap, /pg_db_role_setting/u);
  assert.match(bootstrap, /IN DATABASE %I RESET ALL/u);
  assert.match(runtimeAcl, /pg_db_role_setting/u);
  assert.match(runtimeAcl, /IN DATABASE %I RESET ALL/u);
  assert.doesNotMatch(runtimeAcl, /REVOKE ALL ON SCHEMA %I FROM PUBLIC/u);
  assert.match(entrypoint, /exec \/usr\/local\/bin\/docker-entrypoint\.sh/u);
  assert.match(entrypoint, /PGPASSWORD[\s\S]*--host postgres/u);
  assert.doesNotMatch(entrypoint, /printf[^\n]*(?:admin_password|owner_password)/u);
  assert.match(read("deploy/scripts/verify-static.sh"), /deploy\/postgres\/\*\.sh/u);
});

// verify pinned dependencies and durable storage
test("all four production images are immutable digest references", () => {
  const compose = renderCompose(["compose.verify.yaml"]);
  const images = new Set(Object.values(compose.services).map((service) => service.image));
  assert.equal(images.size, 4);

  // require complete registry references
  for (const image of images) {
    assert.match(image, /^[^@\s]+@sha256:[a-f0-9]{64}$/u);
  }

  assert.match(compose.services.postgres.image, /^postgres@sha256:/u);
  assert.match(compose.services.cloudflared.image, /^cloudflare\/cloudflared@sha256:/u);
  assert.equal(compose.services.api.image, compose.services.migration.image);
  assert.equal(compose.services.api.image, compose.services.worker.image);
  assert.ok(compose.services.cloudflared.command.includes("--token-file"));
  assert.ok(compose.services.cloudflared.command.includes("--no-autoupdate"));
  const postgresData = compose.services.postgres.volumes.find(
    (volume) => volume.target === "/var/lib/postgresql/data",
  );
  assert.equal(postgresData.type, "volume");
  assert.match(postgresData.source, /weather_verify_postgres/u);
  assert.match(read("deploy/compose.yaml"), /\/var\/lib\/weather\/postgres/u);
});

// verify deterministic arm64 resolution
test("image resolver selects one exact linux arm64 manifest", () => {
  const armDigest = `sha256:${"a".repeat(64)}`;
  const update = read("deploy/scripts/update.sh");
  const index = JSON.stringify({
    manifests: [
      { digest: `sha256:${"b".repeat(64)}`, platform: { architecture: "amd64", os: "linux" } },
      { digest: armDigest, platform: { architecture: "arm64", os: "linux", variant: "v8" } },
      { digest: `sha256:${"c".repeat(64)}`, platform: { architecture: "unknown", os: "unknown" } },
    ],
  });
  const result = spawnSync(
    process.execPath,
    [join(scriptsRoot, "resolve-image.mjs"), "registry.example/weather/server:release"],
    { encoding: "utf8", input: index },
  );
  assert.equal(result.status, 0, result.stderr);
  assert.equal(result.stdout, `registry.example/weather/server@${armDigest}\n`);

  const descriptor = JSON.stringify({
    Descriptor: {
      digest: armDigest,
      platform: { architecture: "arm64", os: "linux", variant: "v8" },
    },
  });
  const pinned = spawnSync(
    process.execPath,
    [join(scriptsRoot, "resolve-image.mjs"), `registry.example/weather/server@${armDigest}`],
    { encoding: "utf8", input: descriptor },
  );
  assert.equal(pinned.status, 0, pinned.stderr);
  assert.equal(pinned.stdout, `registry.example/weather/server@${armDigest}\n`);
  assert.match(update, /if \[\[ "\$image" == \*@sha256:\* \]\]; then[\s\S]*docker manifest inspect --verbose/u);

  const ambiguous = JSON.stringify({
    manifests: [
      { digest: armDigest, platform: { architecture: "arm64", os: "linux" } },
      { digest: `sha256:${"d".repeat(64)}`, platform: { architecture: "arm64", os: "linux" } },
    ],
  });
  const rejected = spawnSync(
    process.execPath,
    [join(scriptsRoot, "resolve-image.mjs"), "registry.example/weather/server:release"],
    { encoding: "utf8", input: ambiguous },
  );
  assert.notEqual(rejected.status, 0);
});

// verify cross-lane runtime commands
test("container commands match the API, worker, and web runtime contracts", () => {
  const compose = renderCompose(["compose.verify.yaml"]);
  assert.deepEqual(compose.services.migration.command, ["node", "deploy/scripts/migrate.mjs"]);
  assert.deepEqual(compose.services.api.command, ["node", "apps/api/dist/main.js"]);
  assert.deepEqual(compose.services.worker.command, ["node", "apps/worker/dist/worker.js"]);
  assert.deepEqual(compose.services.web.command, ["node", "deploy/scripts/web-server.mjs"]);
  assert.equal(compose.services.migration.environment.WEATHER_DATABASE_USER, "weather_owner");
  assert.equal(compose.services.api.environment.WEATHER_MIGRATION_AUTHORIZATION_RELEASE, "");
  assert.equal(
    compose.services.api.environment.WEATHER_MIGRATION_AUTHORIZATION_HISTORY_SHA256,
    "",
  );
  assert.equal(compose.services.worker.environment.WEATHER_MIGRATION_AUTHORIZATION_RELEASE, "");
  assert.equal(
    compose.services.worker.environment.WEATHER_MIGRATION_AUTHORIZATION_HISTORY_SHA256,
    "",
  );

  // keep authorization outside unrelated services
  for (const service of ["migration", "postgres", "web", "cloudflared"]) {
    assert.equal(
      compose.services[service].environment?.WEATHER_MIGRATION_AUTHORIZATION_RELEASE,
      undefined,
    );
    assert.equal(
      compose.services[service].environment?.WEATHER_MIGRATION_AUTHORIZATION_HISTORY_SHA256,
      undefined,
    );
  }
  assert.equal(
    compose.services.migration.environment.WEATHER_SITE_CONFIG_PATH,
    "/opt/weather/config/sites/ballydidean.json",
  );
  assert.equal(compose.services.api.environment.WEATHER_API_PORT, "3001");
  assert.equal(
    compose.services.api.environment.WEATHER_FORECAST_ADJUSTMENT_WIND_CANARY_KILL_SWITCH,
    "0",
  );
  // keep the new experimental temperature path independently fail-closed
  for (const service of ["api", "worker"]) {
    assert.equal(
      compose.services[service].environment.WEATHER_FORECAST_ADJUSTMENT_TEMPERATURE_CANARY_KILL_SWITCH,
      "1",
    );
  }
  assert.equal(compose.services.worker.environment.WEATHER_DATABASE_USER, "weather_ingest");
  assert.equal(compose.services.web.environment.WEATHER_RELEASE, "2026.08.22-1");
  assert.match(JSON.stringify(compose.services.api.healthcheck.test), /127\.0\.0\.1:3001\/api\/v1\/health/u);
  assert.deepEqual(compose.services.worker.healthcheck.test, [
    "CMD",
    "node",
    "apps/worker/dist/health.js",
  ]);
  assert.equal(
    compose.services.worker.environment.WEATHER_SITE_CONFIG_PATH,
    "/opt/weather/config/sites/ballydidean.json",
  );
  assert.equal(
    compose.services.web.environment.WEATHER_ADJUSTMENT_MAINTENANCE_API_ORIGIN,
    "http://api:3003",
  );
  assert.equal(compose.services.migration.volumes, undefined);
  const dockerfile = read("Dockerfile");
  const webServer = read("deploy/scripts/web-server.mjs");
  assert.match(
    dockerfile,
    /COPY --chown=10002:10002 config\/forecast-adjustments config\/forecast-adjustments/u,
  );
  assert.match(dockerfile, /apps\/web\/public apps\/web\/public/u);
  assert.match(dockerfile, /deploy\/scripts\/web-server\.mjs/u);
  assert.match(dockerfile, /deploy\/scripts\/xweather-tile-cache\.mjs/u);
  assert.match(dockerfile, /deploy\/scripts\/xweather-usage-budget\.mjs/u);
  assert.match(webServer, /XWEATHER_FORECAST_FRESHNESS_MS = 60 \* 60 \* 1_000/u);
  assert.match(webServer, /XWEATHER_PROVIDER_DAILY_MAP_UNIT_BUDGET = 300/u);
  assert.match(webServer, /XWEATHER_PROVIDER_MONTHLY_MAP_UNIT_BUDGET = 10_000/u);
  assert.doesNotMatch(webServer, /XWEATHER_MANUAL_REFRESH_COOLDOWN|startForecastRefresh|forecastFootprints|X-Weather-Map-Refresh/u);
});

// verify the reviewed adjustment tree is server-only image material
test("server image bakes only reviewed forecast adjustment config", () => {
  const dockerfile = read("Dockerfile");
  assert.match(dockerfile, /ARG NODE_IMAGE=node:24\.16\.0-bookworm-slim/u);
  assert.equal(read(".nvmrc"), "24.16.0\n");
  const serverStage = dockerfile
    .split("FROM runtime AS server\n")[1]
    .split("\nFROM runtime AS web\n")[0];
  const webStage = dockerfile.split("\nFROM runtime AS web\n")[1];
  const registry = read("config/forecast-adjustments/ballydidean.json");
  assert.equal(
    registry,
    '{"activeBundle":null,"contractVersion":"forecast-adjustment-registry/v1"}\n',
  );
  const windCanaryRegistry = JSON.parse(
    read("config/forecast-adjustments/ballydidean-wind-canary.json"),
  );
  assert.equal(windCanaryRegistry.contractVersion, "forecast-adjustment-wind-canary-registry/v1");
  assert.equal(windCanaryRegistry.activeBundle.bundleSha256, "51f8efd63bef678a7f02d11bdab91405ec48f19808f64d3fe8354036c9b302a2");
  const temperatureRegistry = JSON.parse(
    read("config/forecast-adjustments/ballydidean-temperature-canary.json"),
  );
  assert.equal(temperatureRegistry.contractVersion, "forecast-adjustment-temperature-canary-registry/v1");
  assert.equal(temperatureRegistry.activeBundle.bundleSha256, "4d4e229b42823e53d2db062ec18c625bb2d2378a8a46d641fa95fabb59501b0e");
  const temperatureBundle = JSON.parse(read(join("config/forecast-adjustments/ballydidean", temperatureRegistry.activeBundle.path)));
  assert.equal(temperatureBundle.model.scope, "assumed_delay6_next12");
  assert.equal(temperatureBundle.servedForecastIdentity.upstreamModel, "ecmwf_ifs");
  assert.doesNotMatch(JSON.stringify(temperatureBundle), /trainingKeys|sourceKeys|\/dev\/shm|\/home\//u);
  // preserve the retired artifact independently of registry selection
  const windCanaryBundle = JSON.parse(
    read(join(
      "config/forecast-adjustments/ballydidean/wind-canary-bundles",
      "sha256-8ada04b924326665b7c49be37876727e9fdc853e9b0eb3decc7fe68c62acc96b.json",
    )),
  );
  assert.equal(
    windCanaryBundle.artifactKind,
    "wind_transfer_canary_runtime_bundle",
  );
  assert.deepEqual(
    [...new Set(windCanaryBundle.candidate.enabledMetricBands.map(
      // retain the retired artifact's metric allowlist
      (pair) => pair.metric,
    ))].sort(),
    ["windGustMps", "windSpeedMps"],
  );
  assert.match(
    serverStage,
    /packages\/forecast-adjustment\/dist packages\/forecast-adjustment\/dist/u,
  );
  assert.match(
    serverStage,
    /packages\/forecast-adjustment\/package\.json packages\/forecast-adjustment\/package\.json/u,
  );
  assert.match(
    serverStage,
    /config\/forecast-adjustments config\/forecast-adjustments/u,
  );
  assert.doesNotMatch(
    serverStage,
    /\.weather-data|\.weather-models|model-evidence|training_export_password/u,
  );
  assert.doesNotMatch(webStage, /config\/forecast-adjustments|sha256-[a-f0-9]{64}\.json/u);
  assert.match(
    webStage,
    /RUN unlink node_modules\/@weather\/forecast-adjustment/u,
  );

  const compose = renderCompose(["compose.verify.yaml"]);

  // deny operator evidence and keys as API or web mounts
  for (const serviceName of ["api", "web"]) {
    assert.doesNotMatch(
      JSON.stringify(compose.services[serviceName].volumes ?? []),
      /\.weather-data|\.weather-models|model-evidence|training-export|training_export|decrypt|encryption-key/iu,
    );
  }
});

// exercise the static edge and same-origin proxy
test("web edge serves allowlisted assets and bounded read-only upstream proxies", async () => {
  const fixtureRoot = await mkdtemp(join(tmpdir(), "weather-web-edge-"));
  const apiPort = await reservePort();
  const xweatherPort = await reservePort();
  const webPort = await reservePort();
  await mkdir(join(fixtureRoot, "apps/web/public/brand"), { recursive: true });
  await mkdir(join(fixtureRoot, "apps/web/public/fonts"), { recursive: true });
  await mkdir(join(fixtureRoot, "apps/web/public"), { recursive: true });
  await mkdir(join(fixtureRoot, "apps/web/dist"), { recursive: true });
  await mkdir(join(fixtureRoot, "config/sites"), { recursive: true });
  await writeFile(
    join(fixtureRoot, "apps/web/public/index.html"),
    '<!doctype html><html data-weather-admin="__WEATHER_ADMIN__" data-weather-analytics="__WEATHER_ANALYTICS__"><title>Weather</title><meta name="weather-revision" content="__WEATHER_GIT_REVISION__"><link rel="manifest" href="/manifest.webmanifest">__WEATHER_ROUTE_PRELOAD__<link rel="stylesheet" href="/assets/__WEATHER_ASSET_VERSION__/styles.css"><script type="module" src="/assets/__WEATHER_ASSET_VERSION__/client.js"></script></html>\n',
  );
  await writeFile(
    join(fixtureRoot, "apps/web/public/admin-login.html"),
    '<!doctype html><title>Admin sign in</title>__WEATHER_ADMIN_LOGIN_ERROR__<form action="/admin/login" method="post"><input name="password" type="password"><button>Sign in</button></form><link rel="stylesheet" href="/assets/__WEATHER_ASSET_VERSION__/styles.css">\n',
  );
  await writeFile(
    join(fixtureRoot, "apps/web/public/privacy.html"),
    await readFile(join(repoRoot, "apps/web/public/privacy.html")),
  );
  await writeFile(
    join(fixtureRoot, "apps/web/public/manifest.webmanifest"),
    JSON.stringify({ display: "standalone", icons: [], name: "Weather", start_url: "/" }),
  );
  await writeFile(
    join(fixtureRoot, "apps/web/public/service-worker.js"),
    'const release = "__WEATHER_ASSET_VERSION__"; const digests = __WEATHER_SHELL_ASSET_DIGESTS__;\n',
  );
  await writeFile(
    join(fixtureRoot, "config/sites/ballydidean.json"),
    JSON.stringify({ site: { latitude: 47.950429954185445, longitude: -122.42797012608193 } }),
  );
  await writeFile(join(fixtureRoot, "apps/web/public/styles.css"), "body { color: #123; }\n");
  await writeFile(join(fixtureRoot, "apps/web/public/brand/ballydidean-wide.svg"), "<svg xmlns=\"http://www.w3.org/2000/svg\"></svg>\n");
  await writeFile(join(fixtureRoot, "apps/web/public/brand/favicon.svg"), "<svg xmlns=\"http://www.w3.org/2000/svg\"></svg>\n");
  await writeFile(join(fixtureRoot, "apps/web/public/brand/ballydidean-weather-icon-32.png"), "favicon\n");
  await writeFile(join(fixtureRoot, "apps/web/public/brand/ballydidean-weather-icon-180.png"), "apple-touch\n");
  await writeFile(join(fixtureRoot, "apps/web/public/brand/ballydidean-weather-icon-192.png"), "icon-192\n");
  await writeFile(join(fixtureRoot, "apps/web/public/brand/ballydidean-weather-icon-512.png"), "icon-512\n");
  await writeFile(join(fixtureRoot, "apps/web/public/brand/ballydidean-weather-icon-maskable-512.png"), "icon-maskable\n");
  await writeFile(join(fixtureRoot, "apps/web/public/fonts/google-sans-flex-latin.woff2"), "google-sans-flex-font\n");
  await writeFile(join(fixtureRoot, "apps/web/public/fonts/LICENSE-google-sans-flex.txt"), "OFL-1.1\n");
  await writeFile(join(fixtureRoot, "apps/web/public/fonts/material-symbols-rounded-v4.woff2"), "material-symbols-rounded-font\n");
  await writeFile(join(fixtureRoot, "apps/web/public/fonts/LICENSE-material-symbols.txt"), "Apache-2.0\n");
  await writeFile(join(fixtureRoot, "apps/web/dist/client.js"), "export { ready } from './index.js';\n");
  await writeFile(join(fixtureRoot, "apps/web/dist/hosted-updates.js"), "export const ready = true;\n");
  await writeFile(join(fixtureRoot, "apps/web/dist/app-install.js"), "export const install = true;\n");
  await writeFile(join(fixtureRoot, "apps/web/dist/app-version.js"), "export const version = true;\n");
  await writeFile(join(fixtureRoot, "apps/web/dist/index.js"), "export { ready } from './units.js';\n");
  await writeFile(join(fixtureRoot, "apps/web/dist/units.js"), "export const ready = true;\n");
  await writeFile(join(fixtureRoot, "apps/web/dist/solar-cloud.js"), "export const sunlight = true;\n");
  await writeFile(join(fixtureRoot, "apps/web/dist/solar-cloud-evidence.js"), "export const evidence = true;\n");
  // expose the new policy module in the isolated web-edge fixture
  await writeFile(join(fixtureRoot, "apps/web/dist/forecast-correction-policy.js"), "export const policy = 'regional';\n");
  await writeFile(join(fixtureRoot, "apps/web/dist/local-forecast-bias.js"), "export const localBias = true;\n");
  const xweatherClientId = join(fixtureRoot, "xweather-client-id");
  const xweatherClientSecret = join(fixtureRoot, "xweather-client-secret");
  const xweatherUsagePath = join(fixtureRoot, "xweather-usage.json");
  const adminAuthPath = join(fixtureRoot, "admin-auth.json");
  const adminBootstrapTokenPath = join(fixtureRoot, "admin-bootstrap-token");
  const propertySensorLayoutPath = join(fixtureRoot, "property-sensor-layout.json");
  await writeFile(xweatherClientId, "test-client-id\n");
  await writeFile(xweatherClientSecret, "test-client-secret\n");
  await writeFile(adminBootstrapTokenPath, "test-admin-bootstrap-token-with-32-bytes\n");

  const largeApiBody = Buffer.alloc((1024 * 1024) + 1, "t");
  const oversizedTrendsApiBody = Buffer.alloc((2 * 1024 * 1024) + 1, "x");
  const forecastFixture = {
    adjustmentRuntime: {
      activationMode: "wind_canary",
      activeBundle: "wind-bundle",
      authorizationSha256: "wind-authorization",
      candidateArtifactSha256: "wind-candidate",
      enabledMetrics: ["windSpeedMps"],
      evaluationReportSha256: null,
      expiresAt: "2026-09-22T00:00:00.000Z",
      loadedAt: "2026-09-13T00:00:00.000Z",
      qualificationReceiptSha256: null,
      reasonCode: null,
      state: "active",
      transferReportSha256: "wind-transfer",
    },
    temperatureAdjustmentRuntime: {
      activeBundle: "temperature-bundle",
      authorizationSha256: "temperature-authorization",
      expiresAt: "2026-09-22T00:00:00.000Z",
      loadedAt: "2026-09-13T00:00:00.000Z",
      reasonCode: null,
      source: { hourCount: 1 },
      state: "active",
    },
    rainAdjustmentRuntime: {
      activeBundle: "rain-bundle",
      loadedAt: "2026-09-13T00:00:00.000Z",
      reasonCode: null,
      source: { hourCount: 1 },
      state: "active",
    },
    data: [{
      metrics: { precipitationMm: 1, temperatureC: 12, windSpeedMps: 3 },
      provenance: { sourceKey: "fixture-forecast" },
      adjustment: {
        adjustedMetrics: { windSpeedMps: 4 },
        appliedMetrics: ["windSpeedMps"],
        contractVersion: "forecast-adjustment-decision/v1",
        reasonCode: null,
        state: "active",
      },
      temperatureAdjustment: {
        branch: "direct",
        bundleSha256: "temperature-bundle",
        contractVersion: "forecast-temperature-canary-decision/v1",
        correctedTemperatureC: 13,
        rawBestMatchTemperatureC: 12,
        reasonCode: null,
        recentErrorStateSha256: "temperature-error-state",
        sourceForecast: { rawTemperatureC: 11 },
        state: "active",
      },
      rainAdjustment: {
        bundleSha256: "rain-bundle",
        contractVersion: "forecast-rain-adjustment-decision/v1",
        correctedPrecipitationMm: 1.5,
        rawBestMatchPrecipitationMm: 1,
        reasonCode: null,
        sourceForecast: { rawPrecipitationMm: 1.2 },
        state: "active",
      },
    }],
  };
  // map route fixtures to bounded response bodies
  const apiFixtureBodies = new Map([
    ["/api/v1/sites/ballydidean/forecast", Buffer.from(JSON.stringify(forecastFixture))],
    ["/api/v1/sites/ballydidean/rain-collection", Buffer.from(JSON.stringify({
      data: { modelEnabled: true, qualificationEnabled: false, receipts: 1 },
    }))],
    ["/api/v1/sites/ballydidean/current?large=1", largeApiBody],
    ["/api/v1/sites/ballydidean/trends", largeApiBody],
    ["/api/v1/sites/oversized/trends", oversizedTrendsApiBody],
  ]);
  // provide route-specific bounded API fixtures
  const api = createServer((request, response) => {
    // select one bounded response body
    const body = apiFixtureBodies.get(request.url ?? "") ??
      Buffer.from(JSON.stringify({ method: request.method, path: request.url }));
    response.setHeader("Content-Type", "application/json");
    response.setHeader("Content-Length", String(Buffer.byteLength(body)));
    response.end(body);
  });
  api.listen(apiPort, "127.0.0.1");
  await once(api, "listening");

  const xweatherRequests = [];
  let xweatherActive = 0;
  let xweatherMaximumActive = 0;
  const xweatherStartedAt = [];
  const providerBodySecret =
    "provider-body-secret https://provider.example/private/fake-client-id_fake-client-secret";
  const mapPng = Buffer.from(
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=",
    "base64",
  );
  const xweather = createServer(async (request, response) => {
    const requestPath = request.url ?? "/";
    xweatherRequests.push(`${request.method ?? "GET"} ${requestPath}`);
    xweatherStartedAt.push(Date.now());
    xweatherActive += 1;
    xweatherMaximumActive = Math.max(xweatherMaximumActive, xweatherActive);

    await new Promise(
      // keep concurrent provider requests observable
      (resolveWait) => setTimeout(resolveWait, 20),
    );

    // emulate one non-ok provider response
    if (requestPath.includes("/satellite-geocolor/")) {
      response.statusCode = 503;
      response.setHeader("Content-Type", "application/json; diagnostic-secret=header-secret");
      response.end(providerBodySecret);
      xweatherActive -= 1;
      return;
    }

    // emulate one unexpected provider media type
    if (requestPath.includes("/precip-1h/")) {
      response.setHeader("Content-Type", "text/plain; diagnostic-secret=wrong-type-secret");
      response.end(providerBodySecret);
      xweatherActive -= 1;
      return;
    }

    // emulate one provider transport failure
    if (requestPath.includes("/wind-speeds/")) {
      request.socket.destroy();
      xweatherActive -= 1;
      return;
    }

    response.setHeader("Content-Type", "image/png");
    response.setHeader("Content-Length", String(mapPng.byteLength));
    response.end(request.method === "HEAD" ? undefined : mapPng);
    xweatherActive -= 1;
  });
  xweather.listen(xweatherPort, "127.0.0.1");
  await once(xweather, "listening");

  const edge = spawn(process.execPath, [join(scriptsRoot, "web-server.mjs")], {
    cwd: fixtureRoot,
    env: {
      ...process.env,
      NODE_ENV: "production",
      PORT: String(webPort),
      WEATHER_API_ORIGIN: `http://127.0.0.1:${apiPort}`,
      WEATHER_ADMIN_AUTH_PATH: adminAuthPath,
      WEATHER_ADMIN_BOOTSTRAP_TOKEN_PATH: adminBootstrapTokenPath,
      WEATHER_PROPERTY_SENSOR_LAYOUT_PATH: propertySensorLayoutPath,
      WEATHER_RELEASE: "2026.08.25-7",
      WEATHER_WEB_REVISION: "0123456789abcdef0123456789abcdef01234567",
      WEATHER_XWEATHER_CLIENT_ID_FILE: xweatherClientId,
      WEATHER_XWEATHER_CLIENT_SECRET_FILE: xweatherClientSecret,
      WEATHER_XWEATHER_MAP_ORIGIN: `http://127.0.0.1:${xweatherPort}`,
      WEATHER_XWEATHER_USAGE_PATH: xweatherUsagePath,
    },
    stdio: ["ignore", "ignore", "pipe"],
  });
  const edgeDiagnostics = [];
  edge.stderr.on(
    "data",
    // collect only disposable edge diagnostics
    (chunk) => edgeDiagnostics.push(String(chunk)),
  );

  try {
    await waitForServer(`http://127.0.0.1:${webPort}/`);
    const home = await fetch(`http://127.0.0.1:${webPort}/`);
    const forecast = await fetch(`http://127.0.0.1:${webPort}/forecast`);
    const logs = await fetch(`http://127.0.0.1:${webPort}/logs`);
    const map = await fetch(`http://127.0.0.1:${webPort}/map`);
    const trends = await fetch(`http://127.0.0.1:${webPort}/trends`);
    const settings = await fetch(`http://127.0.0.1:${webPort}/settings`);
    // require a public policy independent of login and application scripts
    for (const pathname of ["/privacy", "/privacy/"]) {
      const privacy = await fetch(`http://127.0.0.1:${webPort}${pathname}`);
      const policy = await privacy.text();
      assert.equal(privacy.status, 200);
      assert.equal(privacy.headers.get("content-type"), "text/html; charset=utf-8");
      assert.equal(privacy.headers.get("cache-control"), "no-cache");
      assert.equal(privacy.headers.get("set-cookie"), null);
      assert.doesNotMatch(privacy.headers.get("content-security-policy"), /google-analytics|googletagmanager/u);
      assert.match(policy, /<h1>Privacy policy<\/h1>/u);
      assert.match(policy, /href="mailto:sanctuary@ballydidean\.farm"/u);
      assert.match(policy, /Google Analytics 4/u);
      assert.match(policy, /href="\/assets\/2026\.08\.25-7\/styles\.css"/u);
      assert.doesNotMatch(policy, /<script\b|__WEATHER_|id="weather-app"/u);
      const head = await fetch(`http://127.0.0.1:${webPort}${pathname}`, { method: "HEAD" });
      assert.equal(head.status, 200);
      assert.equal(head.headers.get("content-length"), privacy.headers.get("content-length"));
      assert.equal(await head.text(), "");
    }
    const remoteAgentsPreview = await fetch(
      `http://127.0.0.1:${webPort}/__rac/browser-device?mode=desktop&location=%2Ftrends%3Fpreview%3D1`,
      { redirect: "manual" },
    );
    const invalidRemoteAgentsPreview = await fetch(
      `http://127.0.0.1:${webPort}/__rac/browser-device?mode=desktop&location=https%3A%2F%2Fevil.example`,
      { redirect: "manual" },
    );
    const adminBeforeBootstrap = await fetch(`http://127.0.0.1:${webPort}/admin`);
    const publicLayoutBefore = await fetch(`http://127.0.0.1:${webPort}/api/v1/sites/ballydidean/property-sensor-layout`);
    const publicSwitchesBeforeBootstrap = await fetch(`http://127.0.0.1:${webPort}/api/v1/sites/ballydidean/forecast-adjustment-settings`);
    // keep direct-origin clients unprivileged even when they spoof forwarding headers
    const viewerContext = await fetch(`http://127.0.0.1:${webPort}/api/v1/viewer-context`, {
      headers: { "cf-connecting-ip": "203.0.113.20", "x-forwarded-for": "192.168.11.20" },
    });
    assert.equal(viewerContext.status, 200);
    assert.equal(viewerContext.headers.get("cache-control"), "private, no-store");
    assert.deepEqual(await viewerContext.json(), { data: { homeNetwork: false } });
    const viewerHead = await fetch(`http://127.0.0.1:${webPort}/api/v1/viewer-context`, { method: "HEAD" });
    assert.equal(viewerHead.status, 200);
    assert.equal(await viewerHead.text(), "");
    const viewerMutation = await fetch(`http://127.0.0.1:${webPort}/api/v1/viewer-context`, { method: "POST" });
    assert.equal(viewerMutation.status, 405);
    const rejectedBootstrap = await fetch(`http://127.0.0.1:${webPort}/api/v1/admin/bootstrap`, {
      body: JSON.stringify({ password: "test-admin-password" }),
      headers: {
        "content-type": "application/json",
        "x-weather-admin-bootstrap": "wrong-bootstrap-token-that-is-long-enough",
      },
      method: "POST",
    });
    const acceptedBootstrap = await fetch(`http://127.0.0.1:${webPort}/api/v1/admin/bootstrap`, {
      body: JSON.stringify({ password: "test-admin-password" }),
      headers: {
        "content-type": "application/json",
        "x-weather-admin-bootstrap": "test-admin-bootstrap-token-with-32-bytes",
      },
      method: "POST",
    });
    const legacyBasicAuthorization = `Basic ${Buffer.from("admin:test-admin-password").toString("base64")}`;
    const basicAdmin = await fetch(`http://127.0.0.1:${webPort}/admin`, {
      headers: { authorization: legacyBasicAuthorization },
    });
    const basicMutation = await fetch(`http://127.0.0.1:${webPort}/api/v1/admin/sites/ballydidean/property-sensor-layout/soil-1`, {
      body: JSON.stringify({
        displayName: "Basic auth must fail",
        icon: "temperature",
        latitude: 47.9505,
        longitude: -122.4281,
      }),
      headers: {
        authorization: legacyBasicAuthorization,
        "content-type": "application/json",
      },
      method: "PUT",
    });
    const invalidLogin = await fetch(`http://127.0.0.1:${webPort}/admin/login`, {
      body: new URLSearchParams({ password: "wrong-password" }),
      headers: { "content-type": "application/x-www-form-urlencoded" },
      method: "POST",
      redirect: "manual",
    });
    const invalidLoginPage = await fetch(
      `http://127.0.0.1:${webPort}${invalidLogin.headers.get("location")}`,
    );
    const duplicatePasswordLogin = await fetch(`http://127.0.0.1:${webPort}/admin/login`, {
      body: "password=test-admin-password&password=test-admin-password",
      headers: { "content-type": "application/x-www-form-urlencoded" },
      method: "POST",
      redirect: "manual",
    });
    const extraFieldLogin = await fetch(`http://127.0.0.1:${webPort}/admin/login`, {
      body: new URLSearchParams({ password: "test-admin-password", remember: "yes" }),
      headers: { "content-type": "application/x-www-form-urlencoded" },
      method: "POST",
      redirect: "manual",
    });
    const legacyUsernameLogin = await fetch(`http://127.0.0.1:${webPort}/admin/login`, {
      body: new URLSearchParams({ password: "test-admin-password", username: "admin" }),
      headers: { "content-type": "application/x-www-form-urlencoded" },
      method: "POST",
      redirect: "manual",
    });
    const login = await fetch(`http://127.0.0.1:${webPort}/admin/login`, {
      body: new URLSearchParams({ password: "test-admin-password" }),
      headers: {
        "content-type": "application/x-www-form-urlencoded",
        "x-forwarded-proto": "https",
      },
      method: "POST",
      redirect: "manual",
    });
    const sessionCookieHeader = login.headers.get("set-cookie") ?? "";
    const sessionCookie = sessionCookieHeader.split(";", 1)[0];
    const admin = await fetch(`http://127.0.0.1:${webPort}/admin`, {
      headers: { cookie: sessionCookie },
    });
    const authenticatedHome = await fetch(`http://127.0.0.1:${webPort}/`, {
      headers: { cookie: sessionCookie },
    });
    const savedLayout = await fetch(`http://127.0.0.1:${webPort}/api/v1/admin/sites/ballydidean/property-sensor-layout/soil-1`, {
      body: JSON.stringify({
        displayName: "Orchard soil",
        icon: "temperature",
        latitude: 47.9505,
        longitude: -122.4281,
      }),
      headers: {
        cookie: sessionCookie,
        "content-type": "application/json",
      },
      method: "PUT",
    });
    const forecastUrl = `http://127.0.0.1:${webPort}/api/v1/sites/ballydidean/forecast`;
    const rainStatusUrl = `http://127.0.0.1:${webPort}/api/v1/sites/ballydidean/rain-collection`;
    const publicSwitchUrl = `http://127.0.0.1:${webPort}/api/v1/sites/ballydidean/forecast-adjustment-settings`;
    const adminSwitchUrl = `http://127.0.0.1:${webPort}/api/v1/admin/sites/ballydidean/forecast-adjustment-settings`;
    const sameOrigin = `http://127.0.0.1:${webPort}`;
    const initialSwitches = await fetch(publicSwitchUrl);
    const forecastMutation = await fetch(forecastUrl, { method: "POST" });
    const rainStatusMutation = await fetch(rainStatusUrl, { method: "PUT" });
    const unauthenticatedSwitches = await fetch(adminSwitchUrl);
    const unauthenticatedWrite = await fetch(adminSwitchUrl, {
      body: JSON.stringify({ version: 1, temperature: false, wind: false, rain: false }),
      headers: { "content-type": "application/json", origin: sameOrigin },
      method: "PUT",
    });
    assert.deepEqual(await initialSwitches.json(), {
      data: { version: 1, temperature: true, wind: true, rain: true },
    });
    assert.equal(forecastMutation.status, 405);
    assert.equal(rainStatusMutation.status, 405);
    assert.equal(unauthenticatedSwitches.status, 401);
    assert.equal(unauthenticatedWrite.status, 401);

    // reject authenticated cross-site writes before touching persistence
    for (const origin of [undefined, "https://foreign.example"]) {
      const rejected = await fetch(adminSwitchUrl, {
        body: JSON.stringify({ version: 1, temperature: false, wind: false, rain: false }),
        headers: {
          cookie: sessionCookie,
          "content-type": "application/json",
          ...(origin === undefined ? {} : { origin }),
        },
        method: "PUT",
      });
      assert.equal(rejected.status, 403);
    }

    const invalidWrite = await fetch(adminSwitchUrl, {
      body: JSON.stringify({ version: 1, temperature: true, wind: false, rain: "false" }),
      headers: { cookie: sessionCookie, "content-type": "application/json", origin: sameOrigin },
      method: "PUT",
    });
    assert.equal(invalidWrite.status, 400);
    const adjustmentSettingsPath = join(fixtureRoot, "forecast-adjustment-settings.json");

    // verify every independent combination across persisted and projected state
    for (let mask = 0; mask < 8; mask += 1) {
      const switches = {
        version: 1,
        temperature: Boolean(mask & 1),
        wind: Boolean(mask & 2),
        rain: Boolean(mask & 4),
      };
      const updated = await fetch(adminSwitchUrl, {
        body: JSON.stringify(switches),
        headers: { cookie: sessionCookie, "content-type": "application/json", origin: sameOrigin },
        method: "PUT",
      });
      const publicRead = await fetch(publicSwitchUrl);
      const adminRead = await fetch(adminSwitchUrl, { headers: { cookie: sessionCookie } });
      const projectedResponse = await fetch(forecastUrl);
      const projected = await projectedResponse.json();
      const rainStatus = await (await fetch(rainStatusUrl)).json();
      const row = projected.data[0];

      assert.equal(updated.status, 200);
      assert.deepEqual(await updated.json(), { data: switches });
      assert.deepEqual(await publicRead.json(), { data: switches });
      assert.deepEqual(await adminRead.json(), { data: switches });
      assert.deepEqual(JSON.parse(await readFile(adjustmentSettingsPath, "utf8")), switches);
      assert.deepEqual(projected.adjustmentSettings, switches);
      assert.equal(projectedResponse.headers.get("cache-control"), "no-store");
      assert.deepEqual(row.metrics, forecastFixture.data[0].metrics);
      assert.deepEqual(row.provenance, forecastFixture.data[0].provenance);
      assert.equal(row.adjustment.state, switches.wind ? "active" : "disabled");
      assert.equal(projected.adjustmentRuntime.state, switches.wind ? "active" : "disabled");
      assert.equal(row.temperatureAdjustment.state, switches.temperature ? "active" : "disabled");
      assert.equal(projected.temperatureAdjustmentRuntime.state, switches.temperature ? "active" : "disabled");
      assert.equal(row.rainAdjustment.state, switches.rain ? "active" : "disabled");
      assert.equal(projected.rainAdjustmentRuntime.state, switches.rain ? "active" : "disabled");
      assert.equal(row.adjustment.adjustedMetrics.windSpeedMps, switches.wind ? 4 : undefined);
      assert.equal(row.temperatureAdjustment.correctedTemperatureC, switches.temperature ? 13 : null);
      assert.equal(row.rainAdjustment.correctedPrecipitationMm, switches.rain ? 1.5 : null);
      assert.equal(rainStatus.data.modelEnabled, switches.rain);
      assert.equal(rainStatus.data.qualificationEnabled, false);
    }

    const projectedHead = await fetch(forecastUrl, { method: "HEAD" });
    const projectedGet = await fetch(forecastUrl);
    assert.equal(projectedHead.status, 200);
    assert.equal(Number(projectedHead.headers.get("content-length")),
      (await projectedGet.arrayBuffer()).byteLength);

    // preserve a visible admin diagnostic while corrupted state fails raw
    await writeFile(adjustmentSettingsPath, "{invalid json\n");
    const corruptPublic = await fetch(publicSwitchUrl);
    const corruptAdmin = await fetch(adminSwitchUrl, { headers: { cookie: sessionCookie } });
    const corruptForecast = await (await fetch(forecastUrl)).json();
    const corruptRainStatus = await (await fetch(rainStatusUrl)).json();
    assert.deepEqual(await corruptPublic.json(), {
      data: { version: 1, temperature: false, wind: false, rain: false },
    });
    assert.deepEqual(await corruptAdmin.json(), {
      data: { version: 1, temperature: false, wind: false, rain: false },
      error: "adjustment_settings_unavailable",
    });
    assert.equal(corruptForecast.data[0].adjustment.state, "disabled");
    assert.equal(corruptForecast.data[0].temperatureAdjustment.state, "disabled");
    assert.equal(corruptForecast.data[0].rainAdjustment.state, "disabled");
    assert.equal(corruptRainStatus.data.modelEnabled, false);

    // permit the administrator to repair a malformed settings file
    const repairedSwitches = await fetch(adminSwitchUrl, {
      body: JSON.stringify({ version: 1, temperature: true, wind: true, rain: true }),
      headers: { cookie: sessionCookie, "content-type": "application/json", origin: sameOrigin },
      method: "PUT",
    });
    assert.equal(repairedSwitches.status, 200);
    await writeFile(`${adjustmentSettingsPath}.initialized`, "damaged marker\n");
    const damagedMarker = await (await fetch(adminSwitchUrl, { headers: { cookie: sessionCookie } })).json();
    assert.equal(damagedMarker.error, "adjustment_settings_unavailable");
    assert.deepEqual(damagedMarker.data, {
      version: 1, temperature: false, wind: false, rain: false,
    });
    const repairedMarker = await fetch(adminSwitchUrl, {
      body: JSON.stringify({ version: 1, temperature: false, wind: false, rain: true }),
      headers: { cookie: sessionCookie, "content-type": "application/json", origin: sameOrigin },
      method: "PUT",
    });
    assert.equal(repairedMarker.status, 200);
    assert.deepEqual((await (await fetch(publicSwitchUrl)).json()).data, {
      version: 1, temperature: false, wind: false, rain: true,
    });
    const logout = await fetch(`http://127.0.0.1:${webPort}/admin/logout`, {
      headers: {
        cookie: sessionCookie,
        "x-forwarded-proto": "https",
      },
      method: "POST",
      redirect: "manual",
    });
    const adminAfterLogout = await fetch(`http://127.0.0.1:${webPort}/admin`, {
      headers: { cookie: sessionCookie },
    });
    const rejectedSavedLayout = await fetch(`http://127.0.0.1:${webPort}/api/v1/admin/sites/ballydidean/property-sensor-layout/soil-1`, {
      body: JSON.stringify({
        displayName: "Rejected update",
        icon: "temperature",
        latitude: 47.9505,
        longitude: -122.4281,
      }),
      headers: {
        cookie: sessionCookie,
        "content-type": "application/json",
      },
      method: "PUT",
    });
    const publicLayoutAfter = await fetch(`http://127.0.0.1:${webPort}/api/v1/sites/ballydidean/property-sensor-layout`);
    const repeatedBootstrap = await fetch(`http://127.0.0.1:${webPort}/api/v1/admin/bootstrap`, {
      body: JSON.stringify({ password: "replacement-password" }),
      headers: {
        "content-type": "application/json",
        "x-weather-admin-bootstrap": "test-admin-bootstrap-token-with-32-bytes",
      },
      method: "POST",
    });
    const brand = await fetch(`http://127.0.0.1:${webPort}/brand/ballydidean-wide.svg`);
    const favicon = await fetch(`http://127.0.0.1:${webPort}/brand/favicon.svg`);
    const pwaFavicon = await fetch(`http://127.0.0.1:${webPort}/brand/ballydidean-weather-icon-32.png`);
    const appIcon = await fetch(`http://127.0.0.1:${webPort}/brand/ballydidean-weather-icon-512.png`);
    const manifest = await fetch(`http://127.0.0.1:${webPort}/manifest.webmanifest`);
    const serviceWorker = await fetch(`http://127.0.0.1:${webPort}/service-worker.js`);
    const font = await fetch(`http://127.0.0.1:${webPort}/fonts/google-sans-flex-latin.woff2`);
    const fontLicense = await fetch(`http://127.0.0.1:${webPort}/fonts/LICENSE-google-sans-flex.txt`);
    const iconFont = await fetch(`http://127.0.0.1:${webPort}/fonts/material-symbols-rounded-v4.woff2`);
    const iconFontLicense = await fetch(`http://127.0.0.1:${webPort}/fonts/LICENSE-material-symbols.txt`);
    const client = await fetch(`http://127.0.0.1:${webPort}/assets/2026.08.25-7/client.js`);
    const hostedUpdates = await fetch(`http://127.0.0.1:${webPort}/assets/2026.08.25-7/hosted-updates.js`);
    const appInstall = await fetch(`http://127.0.0.1:${webPort}/assets/2026.08.25-7/app-install.js`);
    const appVersion = await fetch(`http://127.0.0.1:${webPort}/assets/2026.08.25-7/app-version.js`);
    const library = await fetch(`http://127.0.0.1:${webPort}/assets/2026.08.25-7/index.js`);
    const units = await fetch(`http://127.0.0.1:${webPort}/assets/2026.08.25-7/units.js`);
    const solarCloud = await fetch(`http://127.0.0.1:${webPort}/assets/2026.08.25-7/solar-cloud.js`);
    // every transitive browser import belongs to the immutable shell inventory
    const forecastPolicy = await fetch(`http://127.0.0.1:${webPort}/assets/2026.08.25-7/forecast-correction-policy.js`);
    const localBias = await fetch(`http://127.0.0.1:${webPort}/assets/2026.08.25-7/local-forecast-bias.js`);
    const stylesheet = await fetch(`http://127.0.0.1:${webPort}/assets/2026.08.25-7/styles.css`);
    const staleAsset = await fetch(`http://127.0.0.1:${webPort}/assets/2026.08.25-6/index.js`);
    const unversionedAsset = await fetch(`http://127.0.0.1:${webPort}/index.js`);
    const proxied = await fetch(`http://127.0.0.1:${webPort}/api/v1/sites/ballydidean/current?check=1`);
    const annualTrends = await fetch(`http://127.0.0.1:${webPort}/api/v1/sites/ballydidean/trends`);
    const annualTrendsHead = await fetch(`http://127.0.0.1:${webPort}/api/v1/sites/ballydidean/trends`, { method: "HEAD" });
    const oversizedOrdinary = await fetch(`http://127.0.0.1:${webPort}/api/v1/sites/ballydidean/current?large=1`);
    const oversizedOrdinaryHead = await fetch(`http://127.0.0.1:${webPort}/api/v1/sites/ballydidean/current?large=1`, { method: "HEAD" });
    const oversizedTrends = await fetch(`http://127.0.0.1:${webPort}/api/v1/sites/oversized/trends`);
    const oversizedTrendsHead = await fetch(`http://127.0.0.1:${webPort}/api/v1/sites/oversized/trends`, { method: "HEAD" });
    const health = await fetch(`http://127.0.0.1:${webPort}/api/v1/health`);
    const head = await fetch(`http://127.0.0.1:${webPort}/api/v1/sites`, { method: "HEAD" });
    const mutation = await fetch(`http://127.0.0.1:${webPort}/api/v1/sites`, { method: "POST" });
    const legacy = await fetch(`http://127.0.0.1:${webPort}/sites`);
    const future = await fetch(`http://127.0.0.1:${webPort}/api/v2/sites`);
    const encoded = await fetch(`http://127.0.0.1:${webPort}/api/v1%2fsites`);
    const unknown = await fetch(`http://127.0.0.1:${webPort}/secrets/weather_owner_password`);
    const mapValidTime = new Date(Math.floor(Date.now() / (10 * 60 * 1_000)) * 10 * 60 * 1_000).toISOString().replace(/[-:T.Z]/gu, "").slice(0, 14);
    const mapForecastTime = new Date(Math.ceil(Date.now() / (60 * 60 * 1_000)) * 60 * 60 * 1_000).toISOString().replace(/[-:T.Z]/gu, "").slice(0, 14);
    const frameGeometry = "10/256x168/47.950430,-122.427970.png";
    const weatherTile = await fetch(`http://127.0.0.1:${webPort}/maps/xweather/history/radar/${mapValidTime}/${frameGeometry}`);
    const weatherTileAgain = await fetch(`http://127.0.0.1:${webPort}/maps/xweather/history/radar/${mapValidTime}/${frameGeometry}`);
    const weatherTileHead = await fetch(`http://127.0.0.1:${webPort}/maps/xweather/forecast/wind/${mapForecastTime}/${frameGeometry}`, { method: "HEAD" });
    const forecastRadarTile = await fetch(`http://127.0.0.1:${webPort}/maps/xweather/forecast/radar/${mapForecastTime}/${frameGeometry}`);
    const providerStatusFailure = await fetch(`http://127.0.0.1:${webPort}/maps/xweather/history/clouds/${mapValidTime}/${frameGeometry}`);
    const providerContentTypeFailure = await fetch(`http://127.0.0.1:${webPort}/maps/xweather/history/precipitation/${mapValidTime}/${frameGeometry}`);
    const providerFetchFailure = await fetch(`http://127.0.0.1:${webPort}/maps/xweather/history/wind/${mapValidTime}/${frameGeometry}`);
    const refreshMutation = await fetch(`http://127.0.0.1:${webPort}/maps/xweather/forecast/refresh`, { method: "POST" });
    const refreshRead = await fetch(`http://127.0.0.1:${webPort}/maps/xweather/forecast/refresh`);

    const forecastTileAgain = await fetch(`http://127.0.0.1:${webPort}/maps/xweather/forecast/wind/${mapForecastTime}/${frameGeometry}`);
    const invalidWeatherLayer = await fetch(`http://127.0.0.1:${webPort}/maps/xweather/history/temperature/${mapValidTime}/${frameGeometry}`);
    const staleWeatherTile = await fetch(`http://127.0.0.1:${webPort}/maps/xweather/history/radar/20190101000000/${frameGeometry}`);
    const weatherMutation = await fetch(`http://127.0.0.1:${webPort}/maps/xweather/history/radar/${mapValidTime}/${frameGeometry}`, { method: "POST" });
    assert.equal(home.status, 200);
    assert.equal(forecast.status, 200);
    assert.equal(logs.status, 200);
    assert.equal(map.status, 200);
    assert.equal(trends.status, 200);
    assert.equal(settings.status, 200);
    assert.equal(remoteAgentsPreview.status, 302);
    assert.equal(remoteAgentsPreview.headers.get("location"), "/trends?preview=1");
    assert.equal(remoteAgentsPreview.headers.get("cache-control"), "no-store");
    assert.equal(invalidRemoteAgentsPreview.status, 400);
    assert.equal(adminBeforeBootstrap.status, 200);
    assert.equal(adminBeforeBootstrap.headers.get("www-authenticate"), null);
    assert.match(adminBeforeBootstrap.headers.get("content-security-policy"), /frame-ancestors \*/u);
    assert.match(adminBeforeBootstrap.headers.get("content-security-policy"), /form-action 'self'/u);
    assert.equal(adminBeforeBootstrap.headers.get("cross-origin-resource-policy"), "cross-origin");
    assert.equal(adminBeforeBootstrap.headers.get("x-frame-options"), null);
    assert.deepEqual(await publicLayoutBefore.json(), { data: [] });
    assert.deepEqual(await publicSwitchesBeforeBootstrap.json(), {
      data: { version: 1, temperature: false, wind: false, rain: false },
    });
    assert.equal(rejectedBootstrap.status, 401);
    assert.equal(acceptedBootstrap.status, 201);
    assert.equal(basicAdmin.status, 200);
    assert.equal(basicMutation.status, 401);
    assert.equal(invalidLogin.status, 303);
    assert.equal(invalidLogin.headers.get("location"), "/admin?error=invalid");
    assert.equal(duplicatePasswordLogin.status, 400);
    assert.equal(duplicatePasswordLogin.headers.get("set-cookie"), null);
    assert.equal(extraFieldLogin.status, 400);
    assert.equal(extraFieldLogin.headers.get("set-cookie"), null);
    assert.equal(legacyUsernameLogin.status, 400);
    assert.equal(legacyUsernameLogin.headers.get("set-cookie"), null);
    assert.equal(login.status, 303);
    assert.equal(login.headers.get("location"), "/admin");
    assert.match(sessionCookieHeader, /^weather_admin_session=[A-Za-z0-9_-]{43};/u);
    assert.match(sessionCookieHeader, /HttpOnly/u);
    assert.match(sessionCookieHeader, /Max-Age=43200/u);
    assert.match(sessionCookieHeader, /SameSite=None/u);
    assert.match(sessionCookieHeader, /Secure/u);
    assert.match(sessionCookieHeader, /Partitioned/u);
    assert.equal(admin.status, 200);
    assert.equal(admin.headers.get("cache-control"), "private, no-store");
    assert.equal(admin.headers.get("cross-origin-resource-policy"), "cross-origin");
    assert.equal(admin.headers.get("vary"), "Cookie");
    assert.equal(authenticatedHome.headers.get("cache-control"), "private, no-store");
    assert.equal(savedLayout.status, 200);
    assert.equal(logout.status, 303);
    assert.equal(logout.headers.get("location"), "/admin");
    assert.match(logout.headers.get("set-cookie"), /Max-Age=0/u);
    assert.equal(adminAfterLogout.status, 200);
    assert.equal(rejectedSavedLayout.status, 401);
    assert.equal(rejectedSavedLayout.headers.get("www-authenticate"), null);
    assert.deepEqual(await publicLayoutAfter.json(), {
      data: [
        {
          displayName: "Orchard soil",
          icon: "temperature",
          latitude: 47.9505,
          longitude: -122.4281,
          sensorKey: "soil-1",
          updatedAt: (await savedLayout.clone().json()).data.updatedAt,
        },
      ],
    });
    assert.equal(repeatedBootstrap.status, 409);
    const homeBody = await home.text();
    const authenticatedHomeBody = await authenticatedHome.text();
    const adminBody = await admin.text();
    const adminLoginBody = await adminBeforeBootstrap.text();
    const basicAdminBody = await basicAdmin.text();
    const invalidLoginBody = await invalidLoginPage.text();
    const loggedOutAdminBody = await adminAfterLogout.text();
    const forecastBody = await forecast.text();
    const logsBody = await logs.text();
    const mapBody = await map.text();
    const trendsBody = await trends.text();
    const settingsBody = await settings.text();
    // permit analytics only in unauthenticated production page shells
    for (const body of [homeBody, forecastBody, logsBody, mapBody, trendsBody, settingsBody]) {
      assert.match(body, /data-weather-analytics="true"/u);
      assert.doesNotMatch(body, /__WEATHER_ANALYTICS__/u);
    }
    // retain the authenticated and login privacy boundaries
    for (const body of [authenticatedHomeBody, adminBody]) {
      assert.match(body, /data-weather-analytics="false"/u);
    }
    assert.doesNotMatch(adminLoginBody, /data-weather-analytics="true"|googletagmanager/u);
    assert.match(homeBody, /<title>Weather<\/title>/u);
    assert.match(homeBody, /data-weather-admin="false"/u);
    assert.match(authenticatedHomeBody, /data-weather-admin="true"/u);
    assert.match(adminBody, /data-weather-admin="true"/u);
    assert.match(adminLoginBody, /<title>Admin sign in<\/title>/u);
    assert.match(adminLoginBody, /action="\/admin\/login" method="post"/u);
    assert.match(adminLoginBody, /name="password"/u);
    assert.doesNotMatch(adminLoginBody, /name="username"/u);
    assert.doesNotMatch(adminLoginBody, /incorrect/u);
    assert.match(basicAdminBody, /<title>Admin sign in<\/title>/u);
    assert.doesNotMatch(basicAdminBody, /data-weather-admin="true"/u);
    assert.match(invalidLoginBody, /password is incorrect/u);
    assert.match(loggedOutAdminBody, /<title>Admin sign in<\/title>/u);
    assert.match(homeBody, /rel="manifest" href="\/manifest\.webmanifest"/u);
    assert.match(homeBody, /\/assets\/2026\.08\.25-7\/styles\.css/u);
    assert.match(homeBody, /\/assets\/2026\.08\.25-7\/client\.js/u);
    assert.match(logsBody, /\/assets\/2026\.08\.25-7\/styles\.css/u);
    assert.match(logsBody, /\/assets\/2026\.08\.25-7\/client\.js/u);
    assert.match(mapBody, /\/assets\/2026\.08\.25-7\/styles\.css/u);
    assert.match(mapBody, /\/assets\/2026\.08\.25-7\/client\.js/u);
    assert.match(forecastBody, /\/assets\/2026\.08\.25-7\/styles\.css/u);
    assert.match(forecastBody, /\/assets\/2026\.08\.25-7\/client\.js/u);
    assert.match(forecastBody, /rel="preload" as="image"[\s\S]*\/maps\/xweather\/history\/radar\/\d{14}\/10\/256x168\/47\.950430,-122\.427970\.png/u);
    assert.doesNotMatch(homeBody, /\/maps\/xweather\/history\/radar\//u);
    assert.match(trendsBody, /\/assets\/2026\.08\.25-7\/styles\.css/u);
    assert.match(trendsBody, /\/assets\/2026\.08\.25-7\/client\.js/u);
    assert.match(settingsBody, /\/assets\/2026\.08\.25-7\/styles\.css/u);
    assert.match(settingsBody, /\/assets\/2026\.08\.25-7\/client\.js/u);
    assert.match(settingsBody, /name="weather-revision" content="0123456789abcdef0123456789abcdef01234567"/u);
    assert.equal(brand.headers.get("content-type"), "image/svg+xml");
    assert.equal(favicon.headers.get("content-type"), "image/svg+xml");
    assert.equal(pwaFavicon.headers.get("content-type"), "image/png");
    assert.equal(appIcon.headers.get("content-type"), "image/png");
    assert.equal(manifest.headers.get("content-type"), "application/manifest+json; charset=utf-8");
    assert.equal(manifest.headers.get("cache-control"), "no-cache");
    assert.equal(serviceWorker.headers.get("content-type"), "text/javascript; charset=utf-8");
    assert.equal(serviceWorker.headers.get("cache-control"), "no-store");
    const workerBody = await serviceWorker.text();
    assert.match(workerBody, /2026\.08\.25-7/u);
    assert.ok(!workerBody.includes("__WEATHER_SHELL_ASSET_DIGESTS__"));
    const digestMap = JSON.parse(workerBody.match(/const digests = (.+);/u)[1]);
    assert.equal(hostedUpdates.status, 200);
    assert.equal(hostedUpdates.headers.get("cache-control"), "public, max-age=31536000, immutable");
    assert.equal(digestMap["/assets/2026.08.25-7/hosted-updates.js"],
      `sha256-${createHash("sha256").update(await hostedUpdates.text()).digest("base64")}`);
    assert.equal(appInstall.status, 200);
    assert.equal(appInstall.headers.get("cache-control"), "public, max-age=31536000, immutable");
    assert.equal(digestMap["/assets/2026.08.25-7/app-install.js"],
      `sha256-${createHash("sha256").update(await appInstall.text()).digest("base64")}`);
    assert.equal(appVersion.status, 200);
    assert.equal(digestMap["/assets/2026.08.25-7/app-version.js"],
      `sha256-${createHash("sha256").update(await appVersion.text()).digest("base64")}`);
    assert.equal(font.headers.get("content-type"), "font/woff2");
    assert.match(font.headers.get("cache-control"), /immutable/u);
    assert.equal(fontLicense.headers.get("content-type"), "text/plain; charset=utf-8");
    assert.equal(iconFont.headers.get("content-type"), "font/woff2");
    assert.match(iconFont.headers.get("cache-control"), /immutable/u);
    assert.equal(iconFontLicense.headers.get("content-type"), "text/plain; charset=utf-8");
    assert.equal(client.headers.get("content-type"), "text/javascript; charset=utf-8");
    assert.equal(library.headers.get("content-type"), "text/javascript; charset=utf-8");
    assert.equal(units.headers.get("content-type"), "text/javascript; charset=utf-8");
    assert.equal(solarCloud.status, 200);
    assert.equal(solarCloud.headers.get("content-type"), "text/javascript; charset=utf-8");
    assert.match(solarCloud.headers.get("cache-control"), /immutable/u);
    assert.equal(await solarCloud.text(), "export const sunlight = true;\n");
    assert.equal(localBias.status, 200);
    assert.equal(forecastPolicy.status, 200);
    assert.equal(await forecastPolicy.text(), "export const policy = 'regional';\n");
    assert.equal(localBias.headers.get("content-type"), "text/javascript; charset=utf-8");
    assert.match(localBias.headers.get("cache-control"), /immutable/u);
    assert.equal(await localBias.text(), "export const localBias = true;\n");
    assert.match(stylesheet.headers.get("cache-control"), /immutable/u);
    assert.equal(staleAsset.status, 404);
    assert.equal(annualTrends.status, 200);
    assert.equal((await annualTrends.arrayBuffer()).byteLength, largeApiBody.byteLength);
    assert.equal(annualTrends.headers.get("cache-control"), "public, max-age=300, s-maxage=1800, stale-while-revalidate=86400");
    assert.equal(annualTrendsHead.status, 200);
    assert.equal(Number(annualTrendsHead.headers.get("content-length")), largeApiBody.byteLength);
    assert.equal(oversizedOrdinary.status, 502);
    assert.equal(oversizedOrdinaryHead.status, 502);
    assert.equal(oversizedTrends.status, 502);
    assert.equal(oversizedTrendsHead.status, 502);
    assert.equal(unversionedAsset.status, 404);
    assert.deepEqual(await proxied.json(), {
      method: "GET",
      path: "/api/v1/sites/ballydidean/current?check=1",
    });
    assert.match(proxied.headers.get("cache-control"), /max-age=15/u);
    assert.equal(proxied.headers.get("vary"), "Accept");
    assert.deepEqual(await health.json(), { method: "GET", path: "/api/v1/health" });
    assert.equal(head.status, 200);
    assert.ok(Number(head.headers.get("content-length")) > 0);
    assert.equal(mutation.status, 405);
    assert.equal(legacy.status, 404);
    assert.equal(future.status, 404);
    assert.equal(encoded.status, 404);
    assert.equal(unknown.status, 404);
    assert.equal(weatherTile.status, 200);
    assert.equal(weatherTile.headers.get("content-type"), "image/png");
    assert.equal(weatherTile.headers.get("x-weather-tile-cache"), "miss");
    assert.match(weatherTile.headers.get("cache-control"), /immutable/u);
    assert.deepEqual(Buffer.from(await weatherTile.arrayBuffer()), mapPng);
    assert.equal(weatherTileAgain.status, 200);
    assert.equal(weatherTileAgain.headers.get("x-weather-tile-cache"), "hit");
    assert.deepEqual(Buffer.from(await weatherTileAgain.arrayBuffer()), mapPng);
    assert.equal(weatherTileHead.status, 200);
    assert.equal(Number(weatherTileHead.headers.get("content-length")), mapPng.byteLength);
    assert.equal(weatherTileHead.headers.get("x-weather-tile-cache"), "miss");
    assert.equal(weatherTileHead.headers.get("cache-control"), "no-store");
    assert.equal(forecastRadarTile.status, 200);
    assert.equal(forecastRadarTile.headers.get("cache-control"), "no-store");
    assert.equal(providerStatusFailure.status, 502);
    assert.equal(providerContentTypeFailure.status, 502);
    assert.equal(providerFetchFailure.status, 502);
    assert.equal(await providerStatusFailure.text(), "weather tile unavailable\n");
    assert.equal(await providerContentTypeFailure.text(), "weather tile unavailable\n");
    assert.equal(await providerFetchFailure.text(), "weather tile unavailable\n");
    assert.equal(refreshMutation.status, 405);
    assert.equal(refreshRead.status, 404);
    assert.equal(forecastTileAgain.status, 200);
    assert.equal(forecastTileAgain.headers.get("x-weather-tile-cache"), "hit");
    assert.equal(invalidWeatherLayer.status, 404);
    assert.equal(staleWeatherTile.status, 404);
    assert.equal(weatherMutation.status, 405);
    assert.equal(
      xweatherRequests.filter((entry) => entry.endsWith(`/radar/256x168/47.950430,-122.427970,10/${mapValidTime}.png`)).length,
      1,
    );
    assert.equal(
      xweatherRequests.filter((entry) => entry.endsWith(`/fradar/256x168/47.950430,-122.427970,10/${mapForecastTime}.png`)).length,
      1,
    );
    assert.equal(
      xweatherRequests.filter((entry) => entry.endsWith(`/fwind-speeds/256x168/47.950430,-122.427970,10/${mapForecastTime}.png`)).length,
      1,
    );
    const usage = JSON.parse(await readFile(xweatherUsagePath, "utf8"));
    assert.equal(usage.dayUnits, 6);
    assert.equal(usage.monthUnits, 6);
    assert.ok(xweatherMaximumActive <= 8);
    const diagnostics = edgeDiagnostics.join("");
    assert.match(diagnostics, /Xweather tile response rejected: status=503 content-type=application\/json/u);
    assert.match(diagnostics, /Xweather tile response rejected: status=200 content-type=text\/plain/u);
    assert.match(diagnostics, /Xweather tile fetch failed: error=TypeError/u);
    assert.doesNotMatch(
      diagnostics,
      /test-client-id|test-client-secret|provider-body-secret|provider\.example|fake-client-id|fake-client-secret|header-secret|wrong-type-secret|satellite-geocolor|precip-1h|wind-speeds/u,
    );

    // keep every provider start below the shared request-rate ceiling
    for (let index = 1; index < xweatherStartedAt.length; index += 1) {
      assert.ok((xweatherStartedAt[index] ?? 0) - (xweatherStartedAt[index - 1] ?? 0) >= 100);
    }
    assert.match(home.headers.get("content-security-policy"), /default-src 'self'/u);
    assert.match(home.headers.get("content-security-policy"), /script-src 'self' https:\/\/www\.googletagmanager\.com;/u);
    assert.match(home.headers.get("content-security-policy"), /connect-src 'self' https:\/\/www\.googletagmanager\.com https:\/\/\*\.google-analytics\.com https:\/\/\*\.google\.com;/u);
    assert.match(home.headers.get("content-security-policy"), /img-src [^;]* https:\/\/www\.googletagmanager\.com https:\/\/\*\.google-analytics\.com;/u);
    assert.doesNotMatch(home.headers.get("content-security-policy"), /unsafe-inline|unsafe-eval/u);
    // keep third-party script permissions off sensitive documents and non-html responses
    for (const response of [adminBeforeBootstrap, admin, authenticatedHome, client, serviceWorker, manifest, viewerContext, proxied, unknown]) {
      assert.doesNotMatch(response.headers.get("content-security-policy"), /google/u);
      assert.match(response.headers.get("content-security-policy"), /script-src 'self';/u);
    }
    // allow tunnel preview embedding
    assert.match(home.headers.get("content-security-policy"), /frame-ancestors \*/u);
    assert.match(
      home.headers.get("content-security-policy"),
      /img-src 'self' blob: data: https:\/\/tile\.openstreetmap\.org https:\/\/basemap\.nationalmap\.gov https:\/\/imagery\.nationalmap\.gov/u,
    );
    assert.equal(
      home.headers.get("referrer-policy"),
      "strict-origin-when-cross-origin",
    );
    assert.equal(home.headers.get("x-frame-options"), null);
  } finally {
    // stop only disposable test processes
    edge.kill("SIGTERM");
    await Promise.race([
      once(edge, "exit"),
      new Promise((resolveWait) => setTimeout(resolveWait, 2_000)),
    ]);
    api.close();
    await once(api, "close");
    xweather.close();
    await once(xweather, "close");
    await rm(fixtureRoot, { force: true, recursive: true });
  }
});

// require both an immutable release and the production runtime before enabling analytics
test("web edge excludes analytics from development and nonproduction runtimes", async () => {
  const temporaryRoot = await mkdtemp(join(tmpdir(), "weather-analytics-edge-"));
  try {
    // exercise each false side of the production gate independently
    for (const [runtime, release] of [["production", "development"], ["development", "2026.09.21-3"], ["test", "2026.09.21-3"]]) {
      const port = await reservePort();
      const edge = spawn(process.execPath, [join(scriptsRoot, "web-server.mjs")], {
        cwd: repoRoot,
        env: {
          ...process.env,
          NODE_ENV: runtime,
          PORT: String(port),
          WEATHER_RELEASE: release,
          WEATHER_API_ORIGIN: "http://127.0.0.1:9",
          WEATHER_ADMIN_AUTH_PATH: join(temporaryRoot, "auth.json"),
          WEATHER_ADMIN_BOOTSTRAP_TOKEN_PATH: join(temporaryRoot, "absent-bootstrap"),
          WEATHER_PROPERTY_SENSOR_LAYOUT_PATH: join(temporaryRoot, "layout.json"),
          WEATHER_XWEATHER_CLIENT_ID_FILE: join(temporaryRoot, "absent-client-id"),
          WEATHER_XWEATHER_CLIENT_SECRET_FILE: join(temporaryRoot, "absent-client-secret"),
          WEATHER_XWEATHER_USAGE_PATH: join(temporaryRoot, "usage.json"),
        },
        stdio: ["ignore", "ignore", "pipe"],
      });
      const diagnostics = [];
      // retain disposable startup errors for failed assertions
      edge.stderr.on("data", (chunk) => diagnostics.push(String(chunk)));
      try {
        await waitForServer(`http://127.0.0.1:${port}/`);
        const response = await fetch(`http://127.0.0.1:${port}/`);
        assert.equal(response.status, 200, diagnostics.join(""));
        assert.match(await response.text(), /data-weather-analytics="false"/u);
        assert.doesNotMatch(response.headers.get("content-security-policy"), /google/u);
        assert.match(response.headers.get("content-security-policy"), /connect-src 'self';/u);
        assert.match(response.headers.get("content-security-policy"), /script-src 'self';/u);
      } finally {
        // stop only this disposable server before the next environment
        const stopped = once(edge, "exit");
        edge.kill("SIGTERM");
        await stopped;
      }
    }
  } finally {
    await rm(temporaryRoot, { force: true, recursive: true });
  }
});

// verify the local-only override
test("local Compose binds loopback, preserves isolation, and replaces the connector", () => {
  const compose = renderCompose(["compose.local.yaml"]);

  // preserve application egress boundaries
  assert.equal(compose.networks.edge.internal, true);
  assert.equal(compose.networks.web_api.internal, true);
  assert.equal(compose.networks.data.internal, true);
  assert.notEqual(compose.networks.map_egress.internal, true);
  assert.notEqual(compose.networks.local_postgres_ingress.internal, true);
  assert.notEqual(compose.networks.local_web_ingress.internal, true);
  assert.deepEqual(Object.keys(compose.services.api.networks).sort(), ["data", "web_api"]);
  assert.deepEqual(Object.keys(compose.services.web.networks).sort(), [
    "edge",
    "local_web_ingress",
    "map_egress",
    "web_api",
  ]);
  assert.ok(compose.services.migration.build);
  assert.equal(compose.services.api.build, undefined);
  assert.equal(compose.services.worker.build, undefined);
  const published = [
    ...(compose.services.web.ports ?? []),
    ...(compose.services.postgres.ports ?? []),
  ];

  // require loopback for each local port
  for (const port of published) {
    assert.equal(port.host_ip, "127.0.0.1");
  }

  assert.equal(compose.services.cloudflared.image, "node:24-bookworm-slim");
  assert.deepEqual(secretSources(compose.services.cloudflared), []);
  assert.match(JSON.stringify(compose.services.cloudflared.command), /local tunnel disabled/u);
  const postgresData = compose.services.postgres.volumes.find(
    (volume) => volume.target === "/var/lib/postgresql/data",
  );
  assert.equal(postgresData.type, "volume");
  assert.match(postgresData.source, /weather_local_postgres/u);
});

// verify backup and restore boundaries
test("backup is encrypted and restore is disposable verification only", () => {
  const backup = read("deploy/scripts/backup.sh");
  const backupStream = read("deploy/scripts/backup-stream.sh");
  const common = read("deploy/scripts/common.sh");
  const pullBackup = read("deploy/scripts/pull-backup.sh");
  const restore = read("deploy/scripts/restore.sh");
  assert.match(backup, /pg_dump[\s\S]*--format=custom/u);
  assert.match(backup, /\|[\s\n]*age --recipient/u);
  assert.match(backup, /\.partial/u);
  assert.match(backup, /sha256sum/u);
  assert.match(backup, /chmod 600/u);
  assert.match(backup, /trap cleanup EXIT/u);
  assert.match(backup, /env_value[^\n]*WEATHER_DATABASE_NAME/u);
  assert.doesNotMatch(backup, /--dbname weather\b/u);
  assert.match(backup, /output_dir=\/var\/lib\/weather\/backups/u);
  assert.match(backup, /publication_complete/u);
  assert.match(backup, /sync -f/u);
  assert.doesNotMatch(backup, /pg_dump[^\n]*--file|>[^\n]*\.dump(?:["']|\s)/u);
  assert.match(backupStream, /pg_dump[\s\S]*--format=custom[\s\S]*\|[\s\n]*age --recipient/u);
  assert.match(backupStream, /Progress and errors use standard error/u);
  assert.doesNotMatch(backupStream, /\.dump|output_dir|mktemp/u);
  assert.match(pullBackup, /\$deploy_dir\/backups\/weather-nightly\.dump\.age/u);
  assert.match(pullBackup, /ssh-run\.sh" backup-stream/u);
  assert.match(pullBackup, /pg_restore --list[\s\S]*cat >\/dev\/null/u);
  assert.match(pullBackup, /age --decrypt[\s\S]*\|[\s\n]*verify_archive_stream/u);
  assert.match(pullBackup, /weather\.env/u);
  assert.match(pullBackup, /sha256sum/u);
  assert.match(pullBackup, /mv "\$partial" "\$output"/u);
  assert.match(pullBackup, /trap cleanup EXIT/u);
  assert.match(restore, /only verify mode is supported/u);
  assert.match(restore, /live database replacement and cutover are not supported/u);
  assert.match(restore, /_verify_/u);
  assert.match(restore, /createdb[\s\S]*--owner weather_owner/u);
  assert.match(restore, /age --decrypt[\s\S]*\|[\s\n]*[\s\S]*pg_restore/u);
  assert.match(restore, /server_version[\s\S]*150000/u);
  assert.match(restore, /schema_migrations/u);
  assert.match(restore, /rolsuper OR rolcreatedb OR rolcreaterole OR rolreplication/u);
  assert.match(restore, /dropdb[\s\S]*--if-exists/u);
  assert.match(restore, /verify_runtime_database_acl/u);
  const runtimeAclV1 = read("deploy/postgres/runtime-acl-v1.sql");
  const runtimeAclV2 = read("deploy/postgres/runtime-acl-v2.sql");
  assert.match(runtimeAclV1, /REVOKE ALL ON ALL TABLES[^;]*weather_api, weather_ingest/u);
  assert.match(runtimeAclV1, /GRANT SELECT ON schema_migrations TO weather_ingest/u);
  assert.match(runtimeAclV1, /black_globe_temperature_c[\s\S]*wet_bulb_globe_temperature_c/u);
  assert.match(runtimeAclV1, /wet_bulb_globe_temperature_c[\s\S]*water_level_m/u);
  assert.match(runtimeAclV1, /ALTER DEFAULT PRIVILEGES FOR ROLE weather_owner/u);
  assert.doesNotMatch(runtimeAclV1, /forecast_anchor_records/u);
  assert.match(runtimeAclV2, /source_kind,[\s\S]*capabilities,[\s\S]*cadence_seconds/u);
  assert.match(runtimeAclV2, /GRANT INSERT ON forecast_anchor_records TO weather_ingest/u);
  assert.match(runtimeAclV2, /GRANT SELECT \([\s\S]*content_hash,[\s\S]*revision_count[\s\S]*\) ON forecast_anchor_records TO weather_ingest/u);
  assert.doesNotMatch(runtimeAclV2, /forecast_anchor_records TO weather_api/u);
  assert.match(
    runtimeAclV2,
    /GRANT SELECT ON forecast_runtime_provenance_v1 TO weather_api/u,
  );
  assert.doesNotMatch(
    runtimeAclV2,
    /source_config_fingerprint[\s\S]*ON sources TO weather_api/u,
  );
  assert.match(common, /runtime-acl-v2\.sql/u);
  assert.match(
    common,
    /has_table_privilege\('weather_ingest', 'schema_migrations', 'SELECT'\)/u,
  );
  assert.match(
    common,
    /NOT has_schema_privilege\('weather_ingest', 'public', 'CREATE'\)/u,
  );
  assert.match(
    common,
    /NOT has_table_privilege\('weather_ingest', 'schema_migrations', 'INSERT,UPDATE,DELETE,TRUNCATE,REFERENCES,TRIGGER'\)/u,
  );
  assert.doesNotMatch(restore, /ALTER DATABASE[\s\S]*(?:RENAME|OWNER)|mv[\s\S]*postgres/u);
});

// verify the closed obsolete Weather image selector
test("image cleanup selects only unprotected Weather-owned identities", async () => {
  const directory = await mkdtemp(join(tmpdir(), "weather-image-selector-"));
  const update = join(repoRoot, "deploy/scripts/update.sh");
  const id =
    // create one full local image identity
    (character) => `sha256:${character.repeat(64)}`;
  const inventory = join(directory, "images.tsv");
  const repositories = join(directory, "repositories");
  const references = join(directory, "references");
  const protectedIds = join(directory, "protected-ids");

  try {
    await writeFile(repositories, [
      "ghcr.io/anstosa/weather-server",
      "ghcr.io/anstosa/weather-web",
      "",
    ].join("\n"));
    await writeFile(references, [
      `ghcr.io/anstosa/weather-server@${id("c")}`,
      `ghcr.io/anstosa/weather-web@${id("d")}`,
      "",
    ].join("\n"));
    await writeFile(protectedIds, `${id("e")}\n${id("f")}\n`);
    await writeFile(inventory, [
      `${id("a")}\tghcr.io/anstosa/weather-server\told\t${id("1")}`,
      `${id("b")}\tghcr.io/anstosa/weather-web\told\t${id("2")}`,
      `${id("c")}\tghcr.io/anstosa/weather-server\tcurrent\t${id("c")}`,
      `${id("d")}\tghcr.io/anstosa/weather-web\tcandidate\t${id("d")}`,
      `${id("e")}\tghcr.io/anstosa/weather-server\tprevious\t${id("e")}`,
      `${id("f")}\tghcr.io/anstosa/weather-web\tcontainer\t${id("f")}`,
      `${id("7")}\texample.invalid/other\tlatest\t${id("7")}`,
      `${id("8")}\t<none>\t<none>\t<none>`,
      `${id("9")}\tghcr.io/anstosa/weather-server\tmixed\t${id("9")}`,
      `${id("9")}\texample.invalid/shared\tmixed\t${id("9")}`,
      "",
    ].join("\n"));
    const result = spawnSync("bash", [
      "-c",
      'source "$1"; select_obsolete_weather_image_ids "$2" "$3" "$4" "$5"',
      "weather-image-selector",
      update,
      inventory,
      repositories,
      references,
      protectedIds,
    ], { encoding: "utf8" });
    assert.equal(result.status, 0, `${result.stderr}\n${result.stdout}`);
    assert.deepEqual(result.stdout.trim().split("\n"), [id("a"), id("b")]);

    // reject caller-supplied repositories outside the fixed Weather pair
    await writeFile(repositories, [
      "example.invalid/foreign-server",
      "ghcr.io/anstosa/weather-web",
      "",
    ].join("\n"));
    const foreignAuthority = spawnSync("bash", [
      "-c",
      'source "$1"; select_obsolete_weather_image_ids "$2" "$3" "$4" "$5"',
      "weather-image-selector",
      update,
      inventory,
      repositories,
      references,
      protectedIds,
    ], { encoding: "utf8" });
    assert.notEqual(foreignAuthority.status, 0);
    assert.match(foreignAuthority.stderr, /repository contract is invalid/u);

    const foreignEnvironment = join(directory, "foreign.env");
    await writeFile(foreignEnvironment, [
      `WEATHER_SERVER_IMAGE=example.invalid/foreign-server@${id("1")}`,
      `WEATHER_WEB_IMAGE=ghcr.io/anstosa/weather-web@${id("2")}`,
      `POSTGRES_IMAGE=postgres@${id("3")}`,
      `CLOUDFLARED_IMAGE=cloudflare/cloudflared@${id("4")}`,
      "",
    ].join("\n"));
    const foreignEnvironmentResult = spawnSync("bash", [
      "-c",
      'source "$1"; append_image_cleanup_environment "$2" false "$3" "$4" "$5"',
      "weather-image-environment",
      update,
      foreignEnvironment,
      join(directory, "environment-repositories"),
      join(directory, "environment-references"),
      join(directory, "environment-identities"),
    ], { encoding: "utf8" });
    assert.notEqual(foreignEnvironmentResult.status, 0);
    assert.match(foreignEnvironmentResult.stderr, /outside cleanup authority/u);
  } finally {
    await rm(directory, { force: true, recursive: true });
  }
});

// verify race refusal, idempotence and no-force failure behavior
test("image cleanup rechecks frozen state and never forces removal", async () => {
  const directory = await mkdtemp(join(tmpdir(), "weather-image-cleanup-"));
  const update = join(repoRoot, "deploy/scripts/update.sh");
  const firstImageId = `sha256:${"a".repeat(64)}`;
  const secondImageId = `sha256:${"c".repeat(64)}`;
  const firstDigest = `sha256:${"b".repeat(64)}`;
  const secondDigest = `sha256:${"d".repeat(64)}`;
  const inventory = join(directory, "images.tsv");
  const repositories = join(directory, "repositories");
  const references = join(directory, "references");
  const log = join(directory, "docker.log");
  const raced = join(directory, "raced");
  const harness = `
source "$1"
# replay one frozen fake image inventory
snapshot_docker_image_inventory() { cp "$INVENTORY" "$1"; }
# expose no fake container references
snapshot_docker_container_image_ids() { : >"$1"; }
# change only the second state freeze in race mode
write_weather_image_cleanup_contract() {
  if [[ "$MODE" == race && -e "$RACED" ]]; then
    printf 'current=changed\\nprevious=\\n' >"$3"
    printf '%s\\n' "ghcr.io/anstosa/weather-server@$SECOND_DIGEST" >"$5"
  else
    printf 'current=stable\\nprevious=\\n' >"$3"
    : >"$5"
  fi
  cp "$REPOSITORIES" "$4"
  : >"$6"
}
# emulate only the closed no-force image removal call
docker() {
  printf '%s\\n' "$*" >>"$DOCKER_LOG"
  if [[ "$1 $2" != 'image rm' ]]; then
    return 64
  fi
  if [[ "$MODE" == failure ]]; then
    return 1
  fi
  awk -F '\\t' -v removed="$3" '$1 != removed' "$INVENTORY" >"$INVENTORY.next"
  mv "$INVENTORY.next" "$INVENTORY"
  if [[ "$MODE" == race && "$3" == "$FIRST_IMAGE_ID" ]]; then
    : >"$RACED"
  fi
}
cleanup_obsolete_weather_images unused-source unused-target
if [[ "$MODE" == idempotent ]]; then
  cleanup_obsolete_weather_images unused-source unused-target
fi
`;

  try {
    await writeFile(repositories, [
      "ghcr.io/anstosa/weather-server",
      "ghcr.io/anstosa/weather-web",
      "",
    ].join("\n"));
    await writeFile(references, "");

    // exercise successful cleanup twice against one mutable fake cache
    await writeFile(
      inventory,
      [
        `${firstImageId}\tghcr.io/anstosa/weather-server\told-a\t${firstDigest}`,
        `${secondImageId}\tghcr.io/anstosa/weather-server\told-c\t${secondDigest}`,
        "",
      ].join("\n"),
    );
    await writeFile(log, "");
    await rm(raced, { force: true });
    const idempotent = spawnSync("bash", [
      "-c", harness, "weather-image-cleanup", update,
    ], {
      encoding: "utf8",
      env: {
        ...process.env,
        DOCKER_LOG: log,
        FIRST_IMAGE_ID: firstImageId,
        INVENTORY: inventory,
        MODE: "idempotent",
        RACED: raced,
        REFERENCES: references,
        REPOSITORIES: repositories,
        SECOND_DIGEST: secondDigest,
      },
    });
    assert.equal(idempotent.status, 0, idempotent.stderr);
    assert.equal(
      await readFile(log, "utf8"),
      `image rm ${firstImageId}\nimage rm ${secondImageId}\n`,
    );

    // protect the second candidate when release state races after the first deletion
    await writeFile(
      inventory,
      [
        `${firstImageId}\tghcr.io/anstosa/weather-server\told-a\t${firstDigest}`,
        `${secondImageId}\tghcr.io/anstosa/weather-server\told-c\t${secondDigest}`,
        "",
      ].join("\n"),
    );
    await writeFile(log, "");
    await rm(raced, { force: true });
    const racedResult = spawnSync("bash", [
      "-c", harness, "weather-image-cleanup", update,
    ], {
      encoding: "utf8",
      env: {
        ...process.env,
        DOCKER_LOG: log,
        FIRST_IMAGE_ID: firstImageId,
        INVENTORY: inventory,
        MODE: "race",
        RACED: raced,
        REFERENCES: references,
        REPOSITORIES: repositories,
        SECOND_DIGEST: secondDigest,
      },
    });
    assert.notEqual(racedResult.status, 0);
    assert.match(racedResult.stderr, /candidate changed during per-image selection/u);
    assert.equal(await readFile(log, "utf8"), `image rm ${firstImageId}\n`);

    // surface Docker refusal without adding force or prune authority
    await writeFile(
      inventory,
      `${firstImageId}\tghcr.io/anstosa/weather-server\told-a\t${firstDigest}\n`,
    );
    await writeFile(log, "");
    await rm(raced, { force: true });
    const failed = spawnSync("bash", [
      "-c", harness, "weather-image-cleanup", update,
    ], {
      encoding: "utf8",
      env: {
        ...process.env,
        DOCKER_LOG: log,
        FIRST_IMAGE_ID: firstImageId,
        INVENTORY: inventory,
        MODE: "failure",
        RACED: raced,
        REFERENCES: references,
        REPOSITORIES: repositories,
        SECOND_DIGEST: secondDigest,
      },
    });
    assert.notEqual(failed.status, 0);
    assert.match(failed.stderr, /obsolete Weather image removal failed/u);
    assert.equal(await readFile(log, "utf8"), `image rm ${firstImageId}\n`);
    assert.doesNotMatch(await readFile(log, "utf8"), /force|prune/u);
  } finally {
    await rm(directory, { force: true, recursive: true });
  }
});

// verify v13 retains every non-storage resource gate while replacing obsolete storage math
test("v13 resource gate accepts only the exact reviewed legacy storage failures", async () => {
  const directory = await mkdtemp(join(tmpdir(), "weather-v13-resource-"));
  const evidencePath = join(directory, "preflight-v13-resource.json");
  const update = join(repoRoot, "deploy/scripts/update.sh");
  const fixture = () => ({
    architecture: { docker: "aarch64", expected: "aarch64", host: "aarch64" },
    capturedAt: new Date().toISOString(),
    cpu: { actual: 4, minimum: 4 },
    docker: {
      freeBytes: 2_486_505_472,
      inodeFreePercent: 95,
      minimumFreeBytes: 4 * 1_024 * 1_024 * 1_024,
      root: "/var/lib/docker",
    },
    load15: { actual: 0.5, maximum: 2 },
    memoryAvailableBytes: {
      minimumObserved: 3 * 1_024 * 1_024 * 1_024,
      minimumRequired: 1_792 * 1_024 * 1_024,
    },
    pass: false,
    sampleSeconds: 900,
    swapBytesPerMinute: { actual: 0, maximum: 1_024 * 1_024 },
    varLib: {
      databaseBytes: 1_000_000_000,
      freeBytes: 2_486_505_472,
      inodeFreePercent: 95,
      minimumFreeBytes: 10 * 1_024 * 1_024 * 1_024,
    },
  });
  const runGate = () => spawnSync("bash", [
    "-c",
    'source "$1"; v13_resource_evidence=$2; require_v13_resource_gate',
    "weather-v13-resource",
    update,
    evidencePath,
  ], { encoding: "utf8" });
  const writeEvidence = async (evidence) => {
    await writeFile(evidencePath, `${JSON.stringify(evidence)}\n`, { mode: 0o600 });
    await chmod(evidencePath, 0o600);
  };

  try {
    await writeEvidence(fixture());
    assert.equal(runGate().status, 0);

    const fullPass = fixture();
    fullPass.pass = true;
    fullPass.varLib.freeBytes = fullPass.varLib.minimumFreeBytes;
    fullPass.docker.freeBytes = fullPass.docker.minimumFreeBytes;
    await writeEvidence(fullPass);
    assert.equal(runGate().status, 0);

    const loadFailure = fixture();
    loadFailure.load15.actual = 3;
    await writeEvidence(loadFailure);
    assert.notEqual(runGate().status, 0);

    const unaccountedPass = fixture();
    unaccountedPass.pass = true;
    await writeEvidence(unaccountedPass);
    assert.notEqual(runGate().status, 0);

    const missingFailure = fixture();
    missingFailure.docker.freeBytes = missingFailure.docker.minimumFreeBytes;
    await writeEvidence(missingFailure);
    assert.notEqual(runGate().status, 0);

    const stale = fixture();
    stale.capturedAt = "2026-10-08T00:00:00.000Z";
    await writeEvidence(stale);
    assert.notEqual(runGate().status, 0);

    const drift = fixture();
    drift.unreviewed = true;
    await writeEvidence(drift);
    assert.notEqual(runGate().status, 0);
  } finally {
    await rm(directory, { force: true, recursive: true });
  }
});

test("normal v15 resource gate accepts only a full pass or varLib-only legacy failure", async () => {
  const directory = await mkdtemp(join(tmpdir(), "weather-v15-resource-"));
  const evidencePath = join(directory, "preflight-v15-resource.json");
  const update = join(repoRoot, "deploy/scripts/update.sh");
  const fixture = () => ({
    architecture: { docker: "aarch64", expected: "aarch64", host: "aarch64" },
    capturedAt: new Date().toISOString(),
    cpu: { actual: 4, minimum: 4 },
    docker: { freeBytes: 5 * 1_024 ** 3, inodeFreePercent: 95,
      minimumFreeBytes: 4 * 1_024 ** 3, root: "/var/lib/docker" },
    load15: { actual: 0.5, maximum: 2 },
    memoryAvailableBytes: { minimumObserved: 3 * 1_024 ** 3,
      minimumRequired: 1_792 * 1_024 ** 2 },
    pass: false,
    sampleSeconds: 900,
    swapBytesPerMinute: { actual: 0, maximum: 1_024 ** 2 },
    varLib: { databaseBytes: 1_000_000_000, freeBytes: 5 * 1_024 ** 3,
      inodeFreePercent: 95, minimumFreeBytes: 10 * 1_024 ** 3 },
  });
  const runGate = () => spawnSync("bash", ["-c",
    'source "$1"; v13_resource_evidence=$2; require_normal_release_resource_gate',
    "weather-v15-resource", update, evidencePath], { encoding: "utf8" });
  const writeEvidence = async (evidence) => {
    await writeFile(evidencePath, `${JSON.stringify(evidence)}\n`, { mode: 0o600 });
    await chmod(evidencePath, 0o600);
  };

  try {
    await writeEvidence(fixture());
    assert.equal(runGate().status, 0);

    const fullPass = fixture();
    fullPass.pass = true;
    fullPass.varLib.freeBytes = fullPass.varLib.minimumFreeBytes;
    await writeEvidence(fullPass);
    assert.equal(runGate().status, 0);

    const bothStorage = fixture();
    bothStorage.docker.freeBytes = 1;
    await writeEvidence(bothStorage);
    assert.notEqual(runGate().status, 0);

    const dockerOnly = fixture();
    dockerOnly.varLib.freeBytes = dockerOnly.varLib.minimumFreeBytes;
    dockerOnly.docker.freeBytes = 1;
    await writeEvidence(dockerOnly);
    assert.notEqual(runGate().status, 0);

    const loadFailure = fixture();
    loadFailure.load15.actual = 3;
    await writeEvidence(loadFailure);
    assert.notEqual(runGate().status, 0);
  } finally {
    await rm(directory, { force: true, recursive: true });
  }
});

test("normal stage qualification rejects over-envelope and low-water samples", async () => {
  const directory = await mkdtemp(join(tmpdir(), "weather-normal-stage-envelope-"));
  const evaluatorPath = join(repoRoot, "deploy/scripts/adjustment-evaluation-package.mjs");
  const update = join(repoRoot, "deploy/scripts/update.sh");
  const { evaluateNormalStageCapacity } = await import(evaluatorPath);
  const now = new Date();
  const digest = (character) => `sha256:${character.repeat(64)}`;
  const base = {
    activeDatabaseBytes: 1_000_000_000,
    captureReserveBytes: 4_112_384,
    capturedAt: now.toISOString(),
    fixtureInodes: 4_096,
    freeBytes: 40_000_000_000,
    freeInodes: 100_000,
    imagePullPeakBytes: 100_000_000,
    imageReferences: [
      { reference: `ghcr.io/anstosa/weather-server@${digest("a")}`, role: "source", runtime: "server" },
      { reference: `ghcr.io/anstosa/weather-web@${digest("b")}`, role: "source", runtime: "web" },
      { reference: `ghcr.io/anstosa/weather-server@${digest("c")}`, role: "target", runtime: "server" },
      { reference: `ghcr.io/anstosa/weather-web@${digest("d")}`, role: "target", runtime: "web" },
    ],
    inodeFloor: 32_768,
    largestSourceIndexBytes: 100_000_000,
    maxWalBytes: 1_000_000,
    newImageInodes: 1_000,
    operationId: "normal-2026-10-09-3-to-2026-10-29-1",
    persistentImageGrowthBytes: 50_000_000,
    postgresComposeProject: "weather",
    postgresContainerId: "e".repeat(64),
    postgresContainerImageId: digest("f"),
    postgresContainerImageReference: `postgres@${digest("1")}`,
    postgresDataDirectory: "/var/lib/postgresql/data",
    postgresDataFilesystemDevice: 1,
    protectedFloorBytes: 2_030_043_136,
    retirementCreditBytes: 0,
    scratchBytes: 268_435_456,
    sourceDatabaseFileCount: 10_000,
    sourceDatabaseOid: 16_384,
    sourceEnvSha256: "2".repeat(64),
    sourceLedgerSha256: "6de5c8c7efaa448aeb12bf1a9debe6fe7d4d4d1003ee0e21ab619ffa624c3424",
    sourceRelease: "2026.10.09-3",
    sourceSchemaVersion: 21,
    systemIdentifier: "123456789",
    targetEnvSha256: "3".repeat(64),
    targetLedgerSha256: "422dac5e0547295b7fd973f8e1b234a013dd629b04001215a044bee928b77b6e",
    targetRelease: "2026.10.29-1",
    targetSchemaVersion: 22,
    weatherPostgresDir: "/var/lib/weather/postgres",
    weatherPostgresDirFilesystemDevice: 1,
  };
  const pre = evaluateNormalStageCapacity({ ...base, phase: "pre_pull" }, now);
  const post = evaluateNormalStageCapacity({ ...base, phase: "post_pull" }, now);
  const canonical = (value) => Array.isArray(value)
    ? `[${value.map(canonical).join(",")}]`
    : value !== null && typeof value === "object"
      ? `{${Object.keys(value).sort().map((key) => `${JSON.stringify(key)}:${canonical(value[key])}`).join(",")}}`
      : JSON.stringify(value);
  const prePath = join(directory, "pre.json");
  const postPath = join(directory, "post.json");
  const samplesPath = join(directory, "samples.json");
  const outputPath = join(directory, "qualification.json");
  await writeFile(prePath, canonical(pre), { mode: 0o600 });
  await writeFile(postPath, canonical(post), { mode: 0o600 });
  const runSeal = () => spawnSync("bash", ["-c",
    'source "$1"; write_normal_stage_qualification_receipt source.env target.env "$2" "$3" "$4" "$5"',
    "weather-normal-stage-envelope", update, prePath, postPath, samplesPath, outputPath],
  { encoding: "utf8" });

  try {
    // reject a measured peak larger than the post-pull headroom
    for (const [initialFreeBytes, minimumFreeBytes] of [
      [41_000_000_000, 2_034_155_520],
      [40_000_000_000, 2_034_155_519],
    ]) {
      const body = {
        contractVersion: "normal-stage-capacity-samples/v1",
        initialFreeBytes,
        initialFreeInodes: 100_000,
        minimumFreeBytes,
        minimumFreeInodes: 50_000,
        migrationDurationMilliseconds: 1_000,
        migrationMinimumFreeBytes: minimumFreeBytes,
        migrationMinimumFreeInodes: 50_000,
        migrationPeakGrowthBytes: initialFreeBytes - minimumFreeBytes,
        migrationPeakGrowthInodes: 50_000,
        migrationPhaseEndIndex: 1,
        migrationPhaseEndTimestamp: now.getTime() + 1,
        migrationPhaseStartIndex: 0,
        migrationPhaseStartTimestamp: now.getTime(),
        peakGrowthBytes: initialFreeBytes - minimumFreeBytes,
        peakGrowthInodes: 50_000,
        samples: [
          { capturedAtMilliseconds: now.getTime(), freeBytes: initialFreeBytes, freeInodes: 100_000, sequence: 0 },
          { capturedAtMilliseconds: now.getTime() + 1, freeBytes: minimumFreeBytes, freeInodes: 50_000, sequence: 1 },
        ],
      };
      const samples = { ...body,
        receiptSha256: createHash("sha256").update(canonical(body)).digest("hex") };
      await writeFile(samplesPath, canonical(samples), { mode: 0o600 });
      const result = runSeal();
      assert.notEqual(result.status, 0);
      assert.match(result.stderr, /exceeds its post-pull capacity envelope/u);
    }
    // reject a hash-consistent sample artifact with a missing sequence ordinal
    const gapBody = {
      contractVersion: "normal-stage-capacity-samples/v1",
      initialFreeBytes: 10_000_000_000,
      initialFreeInodes: 100_000,
      minimumFreeBytes: 9_999_000_000,
      minimumFreeInodes: 99_000,
      migrationDurationMilliseconds: 1_000,
      migrationMinimumFreeBytes: 9_999_000_000,
      migrationMinimumFreeInodes: 99_000,
      migrationPeakGrowthBytes: 1_000_000,
      migrationPeakGrowthInodes: 1_000,
      migrationPhaseEndIndex: 1,
      migrationPhaseEndTimestamp: now.getTime() + 1,
      migrationPhaseStartIndex: 0,
      migrationPhaseStartTimestamp: now.getTime(),
      peakGrowthBytes: 1_000_000,
      peakGrowthInodes: 1_000,
      samples: [
        { capturedAtMilliseconds: now.getTime(), freeBytes: 10_000_000_000, freeInodes: 100_000, sequence: 0 },
        { capturedAtMilliseconds: now.getTime() + 1, freeBytes: 9_999_000_000, freeInodes: 99_000, sequence: 2 },
      ],
    };
    await writeFile(samplesPath, canonical({
      ...gapBody,
      receiptSha256: createHash("sha256").update(canonical(gapBody)).digest("hex"),
    }), { mode: 0o600 });
    const gap = runSeal();
    assert.notEqual(gap.status, 0);
    assert.match(gap.stderr, /exceeds its post-pull capacity envelope/u);
    // reserve only the SQL/ACL phase peak, not the already allocated 14 GB clone
    const cloneBody = {
      contractVersion: "normal-stage-capacity-samples/v1",
      initialFreeBytes: 39_000_000_000,
      initialFreeInodes: 100_000,
      minimumFreeBytes: 24_998_000_000,
      minimumFreeInodes: 79_998,
      migrationDurationMilliseconds: 1_000,
      migrationMinimumFreeBytes: 24_998_000_000,
      migrationMinimumFreeInodes: 79_998,
      migrationPeakGrowthBytes: 2_000_000,
      migrationPeakGrowthInodes: 2,
      migrationPhaseEndIndex: 2,
      migrationPhaseEndTimestamp: now.getTime() + 2,
      migrationPhaseStartIndex: 1,
      migrationPhaseStartTimestamp: now.getTime() + 1,
      peakGrowthBytes: 14_002_000_000,
      peakGrowthInodes: 20_002,
      samples: [
        { capturedAtMilliseconds: now.getTime(), freeBytes: 39_000_000_000, freeInodes: 100_000, sequence: 0 },
        { capturedAtMilliseconds: now.getTime() + 1, freeBytes: 25_000_000_000, freeInodes: 80_000, sequence: 1 },
        { capturedAtMilliseconds: now.getTime() + 2, freeBytes: 24_998_000_000, freeInodes: 79_998, sequence: 2 },
        { capturedAtMilliseconds: now.getTime() + 3, freeBytes: 38_000_000_000, freeInodes: 99_000, sequence: 3 },
      ],
    };
    await writeFile(samplesPath, canonical({
      ...cloneBody,
      receiptSha256: createHash("sha256").update(canonical(cloneBody)).digest("hex"),
    }), { mode: 0o600 });
    const clone = runSeal();
    assert.equal(clone.status, 0, clone.stderr);
    const qualification = JSON.parse(await readFile(outputPath, "utf8"));
    // include the one-gigabyte post-receipt gap in the whole-stage envelope
    assert.equal(qualification.wholeStagePeakBytes, 15_002_000_000);
    assert.equal(qualification.postPullFreeBytes, 40_000_000_000);
    assert.equal(qualification.measuredMigrationPeakBytes, 2_000_000);
    assert.equal(qualification.measuredMigrationPeakInodes, 2);
  } finally {
    await rm(directory, { force: true, recursive: true });
  }
});

// verify the fresh local minimum without substituting for the full oracle
test("image pulls require the protected floor plus next-capture bytes and inodes", () => {
  const update = join(repoRoot, "deploy/scripts/update.sh");
  const runGate =
    // execute one injected filesystem sample through the sourced gate
    (sample) => spawnSync("bash", [
      "-c",
      'source "$1"; require_command() { :; }; stat() { printf "%s\\n" "$SAMPLE"; }; require_image_pull_capacity_floor',
      "weather-image-pull-capacity",
      update,
    ], { encoding: "utf8", env: { ...process.env, SAMPLE: sample } });
  const exact = runGate("496620 4096 32768");
  assert.equal(exact.status, 0, exact.stderr);
  const oneBlockUnder = runGate("496619 4096 32768");
  assert.notEqual(oneBlockUnder.status, 0);
  assert.match(oneBlockUnder.stderr, /protected capture floor/u);
  const oneInodeUnder = runGate("496620 4096 32767");
  assert.notEqual(oneInodeUnder.status, 0);
  assert.match(oneInodeUnder.stderr, /protected capture floor/u);
});

// verify the unsafe direct-deploy entry point is closed
test("yolo deployment is closed before any release mutation", () => {
  const update = read("deploy/scripts/update.sh");
  const yoloCase = update.split("  yolo)\n")[1].split("  stage)\n")[0];
  assert.match(yoloCase, /die "yolo is closed; use the staged normal release path"/u);
  assert.doesNotMatch(yoloCase, /yolo_release|prepare_yolo_release|compose|record_release_success/u);
});

// verify the fixed v13 bridge cannot bypass literal capacity or exact source restoration
test("legacy v13 and normal v15 paths use their distinct capacity contracts", () => {
  const update = read("deploy/scripts/update.sh");
  const capacityFunction = update
    .split("require_literal_adjustment_release_capacity() (")[1]
    .split("\n)\n\n# cap one frozen Docker inventory")[0];
  const prepareYolo = update
    .split("prepare_yolo_release() {")[1]
    .split("\n}\n\n# apply one source-preserving direct release")[0];
  const yoloRelease = update
    .split("yolo_release() (")[1]
    .split("\n)\n\n# activate one forward release")[0];
  const startRelease = update
    .split("start_release() (")[1]
    .split("\n)\n\n# roll back images")[0];
  const stageCase = update.split("  stage)\n")[1].split("  activate)\n")[0];
  assert.match(capacityFunction, /release-capacity/u);
  assert.match(capacityFunction, /SOURCE_SERVER|source_server/u);
  assert.match(capacityFunction, /source_web/u);
  assert.match(capacityFunction, /target_server/u);
  assert.match(capacityFunction, /target_web/u);
  assert.match(capacityFunction, /retirementCreditBytes !== 0/u);
  assert.doesNotMatch(capacityFunction, /image rm|prune|--force/u);
  assert.match(capacityFunction, /POSTGRES_IMAGE CLOUDFLARED_IMAGE/u);
  assert.equal((prepareYolo.match(/require_literal_adjustment_release_capacity/gu) ?? []).length, 4);
  assert.equal(
    yoloRelease.indexOf("verify_fixed_v13_source_compatibility") <
      yoloRelease.indexOf("publish_migration_authorization"),
    true,
  );
  assert.equal(
    yoloRelease.lastIndexOf("require_literal_adjustment_release_capacity") <
      yoloRelease.indexOf('record_release_success "$release" "$current"'),
    true,
  );
  assert.match(yoloRelease, /restore_images "\$current_env" "\$current" "\$release"/u);
  assert.match(yoloRelease, /capacity diagnostic/u);
  assert.doesNotMatch(stageCase, /require_v13_resource_gate|require_literal_adjustment_release_capacity/u);
  assert.match(stageCase, /require_normal_stage_capacity[\s\S]*pre_pull/u);
  assert.match(stageCase, /require_normal_stage_capacity[\s\S]*post_pull/u);
  assert.match(stageCase, /normal-21-22/u);
  assert.doesNotMatch(startRelease, /require_v13_resource_gate|require_literal_adjustment_release_capacity/u);
  assert.equal(
    startRelease.indexOf("require_normal_activation_capacity") <
      startRelease.indexOf("Creating pre-migration encrypted backup"),
    true,
  );
  assert.match(startRelease, /--max-output-bytes/u);
});

test("historical schema-18 to schema-21 activation requires the measured ARM64 rehearsal", () => {
  const update = read("deploy/scripts/update.sh");
  const rehearsal = update
    .split("historical_v14_18_21_rehearsal() (")[1]
    .split("\n)\n\n# require one measured full-size ARM64 proof")[0];
  const activation = update
    .split("inert_v14_release() (")[1]
    .split("\n)\n\n# activate one forward release")[0];
  const receiptValidator = update
    .split("require_historical_v14_rehearsal_receipt() {")[1]
    .split("\n}\n\n# apply one source-preserving direct release")[0];
  assert.match(rehearsal, /pg_dump[\s\S]*--format=custom[\s\S]*pg_restore/u);
  assert.match(rehearsal, /candidate_oid/u);
  assert.match(rehearsal, /historical rehearsal clone absence check failed/u);
  assert.match(rehearsal, /historical rehearsal migration name already exists/u);
  assert.match(rehearsal,
    /Config\.Labels\.com\.docker\.compose\.project[\s\S]*Config\.Labels\.com\.docker\.compose\.service[\s\S]*Config\.Image/u);
  assert.match(rehearsal, /postgres\|1000000000\|536870912/u);
  assert.match(rehearsal, /--atomic-maintenance-v14/u);
  assert.match(rehearsal, /WEATHER_DATABASE_STATEMENT_TIMEOUT_MS=300000/u);
  assert.match(rehearsal, /duration_milliseconds <= 150000/u);
  assert.match(rehearsal, /appDataEqual: true/u);
  assert.match(rehearsal, /cleanupConfirmed: true/u);
  assert.match(rehearsal, /minimumFreeBytesObserved/u);
  assert.match(rehearsal, /peakWalBytes/u);
  assert.match(rehearsal, /peakTempBytes/u);
  const receiptGate = activation.indexOf("require_historical_v14_rehearsal_receipt");
  const mutation = activation.indexOf("active_mutation_started=true");
  assert.equal(receiptGate >= 0 && receiptGate < mutation, true);
  assert.match(activation, /require_normal_release_resource_gate[\s\S]*require_literal_full_v14_release_capacity/u);
  assert.match(activation,
    /capture_supported_writer_container[\s\S]*active_mutation_started=true[\s\S]*stop_supported_writer_container[\s\S]*require_supported_writers_quiesced[\s\S]*require_historical_v14_rehearsal_receipt[\s\S]*start_postgres[\s\S]*require_supported_writers_quiesced[\s\S]*--atomic-maintenance-v14/u);
  assert.match(update, /duration \* liveRows > 150000n \* candidateRows/u);
  assert.match(update, /duration \* liveHeapBytes > 150000n \* candidateHeapBytes/u);
  assert.doesNotMatch(`${rehearsal}\n${receiptValidator}`,
    /64 \* 1024 \* 1024|67_108_864/u);
});

test("supported backfills take the shared lifecycle lock before reading active state", () => {
  const common = read("deploy/scripts/common.sh");
  assert.match(common,
    /acquire_weather_lifecycle_lock\(\)[\s\S]*flock --exclusive --nonblock[\s\S]*\/proc\/self\/fd/u);
  for (const name of [
    "public-stations-backfill.sh",
    "tempest-backfill.sh",
    "tide-backfill.sh",
  ]) {
    const script = read(`deploy/scripts/${name}`);
    assert.equal(script.indexOf("acquire_weather_lifecycle_lock") <
      script.indexOf("read_release_state"), true, name);
    assert.equal(script.indexOf("acquire_weather_lifecycle_lock") <
      script.indexOf("compose run"), true, name);
  }
});

// verify a post-health capacity failure restores exact source images before returning
test("v13 yolo compensates post-health oracle failure before preserving old markers", async () => {
  const directory = await mkdtemp(join(tmpdir(), "weather-v13-yolo-compensation-"));
  const releases = join(directory, "releases");
  const source = join(releases, "2026.10.07-3.env");
  const target = join(releases, "2026.10.08-1.env");
  const transcript = join(directory, "transcript");
  const sourceServer =
    "ghcr.io/anstosa/weather-server@sha256:d0688756c33875940f67fbb782d9e67a2405a811d2aca40bd155f6ceffa71a74";
  const sourceWeb =
    "ghcr.io/anstosa/weather-web@sha256:739d063cd911bcd7c6637082e356889ef60ac7a76caabc737070703fdf83746f";
  const infrastructure = [
    `POSTGRES_IMAGE=postgres@sha256:${"c".repeat(64)}`,
    `CLOUDFLARED_IMAGE=cloudflare/cloudflared@sha256:${"d".repeat(64)}`,
    "WEATHER_DATABASE_NAME=weather",
    "WEATHER_POSTGRES_DIR=/var/lib/weather/postgres",
    "WEATHER_FORECAST_ADJUSTMENT_WIND_CANARY_KILL_SWITCH=0",
    "WEATHER_FORECAST_ADJUSTMENT_TEMPERATURE_CANARY_KILL_SWITCH=1",
  ];

  try {
    await mkdir(releases);
    await writeFile(source, [
      "WEATHER_RELEASE=2026.10.07-3",
      `WEATHER_SERVER_IMAGE=${sourceServer}`,
      `WEATHER_WEB_IMAGE=${sourceWeb}`,
      ...infrastructure,
      `WEATHER_CONTROL_PLANE_SHA256=${"e".repeat(64)}`,
      "WEATHER_CONTROL_PLANE_VERSION=12",
      "",
    ].join("\n"), { mode: 0o600 });
    await writeFile(target, [
      "WEATHER_RELEASE=2026.10.08-1",
      `WEATHER_SERVER_IMAGE=ghcr.io/anstosa/weather-server@sha256:${"a".repeat(64)}`,
      `WEATHER_WEB_IMAGE=ghcr.io/anstosa/weather-web@sha256:${"b".repeat(64)}`,
      ...infrastructure,
      `WEATHER_CONTROL_PLANE_SHA256=${"f".repeat(64)}`,
      "WEATHER_CONTROL_PLANE_VERSION=13",
      "",
    ].join("\n"), { mode: 0o600 });
    const harness = `
source "$1"
releases_dir=$2
state_dir=$3
source_env=$4
target_env=$5
transcript=$6
history=${"9".repeat(64)}
literal_calls=0
read_optional_release_state() { printf '%s\\n' 2026.10.07-3; }
release_env() { printf '%s/%s.env\\n' "$releases_dir" "$1"; }
validate_release_env() { :; }
require_control_plane_compatibility() { :; }
prepare_yolo_release() { printf '%s\\n' "$target_env"; }
require_deployment_secrets() { :; }
verify_fixed_v13_source_compatibility() {
  write_migration_authorization "$3" 2026.10.07-3 2026.10.08-1 "$history"
}
require_v13_resource_gate() { :; }
require_literal_adjustment_release_capacity() {
  literal_calls=$((literal_calls + 1))
  printf 'literal:%s\\n' "$literal_calls" >>"$transcript"
  [[ "$literal_calls" != 2 ]]
}
start_postgres() { printf 'postgres\\n' >>"$transcript"; }
compose() { printf 'compose:%s\\n' "$*" >>"$transcript"; }
migration_history_sha256() { printf '%s\\n' "$history"; }
apply_runtime_database_acl() { printf 'acl:apply\\n' >>"$transcript"; }
verify_runtime_database_acl() { printf 'acl:verify\\n' >>"$transcript"; }
start_exact_release() { printf 'target:start\\n' >>"$transcript"; }
restore_images() { printf 'source:restore:%s:%s\\n' "$2" "$3" >>"$transcript"; }
write_private_state() { printf 'state:%s:%s\\n' "$(basename "$1")" "$2" >>"$transcript"; }
write_active_symlink() { printf 'active:%s\\n' "$1" >>"$transcript"; }
record_release_success() { printf 'record\\n' >>"$transcript"; }
set +e
yolo_release 2026.10.08-1 "$source_env"
status=$?
set -e
printf 'status:%s\\n' "$status" >>"$transcript"
[[ "$status" == 1 ]]
`;
    const result = spawnSync("bash", [
      "-c",
      harness,
      "weather-v13-yolo-compensation",
      join(repoRoot, "deploy/scripts/update.sh"),
      releases,
      join(directory, "state"),
      source,
      target,
      transcript,
    ], { encoding: "utf8" });
    assert.equal(result.status, 0, `${result.stderr}\n${result.stdout}`);
    const operations = await readFile(transcript, "utf8");
    assert.match(operations, /target:start\nliteral:2\nsource:restore:2026\.10\.07-3:2026\.10\.08-1/u);
    assert.match(operations, /source:restore[^]*active:2026\.10\.07-3[^]*literal:3/u);
    assert.match(operations, /state:current-release:2026\.10\.07-3/u);
    assert.doesNotMatch(operations, /record/u);
    assert.match(operations, /status:1/u);
  } finally {
    await rm(directory, { force: true, recursive: true });
  }
});

// verify conditional compensation calls cannot mask an early required-step failure
test("source restoration returns immediately when PostgreSQL restart fails", async () => {
  const update = join(repoRoot, "deploy/scripts/update.sh");
  const harness = `
source "$1"
transcript=$2
prepare_xweather_usage_directory() { printf 'prepare\\n' >>"$transcript"; }
start_postgres() { printf 'postgres-failed\\n' >>"$transcript"; return 1; }
apply_runtime_database_acl() { printf 'unexpected-acl\\n' >>"$transcript"; }
verify_runtime_database_acl() { printf 'unexpected-verify\\n' >>"$transcript"; }
compose() { printf 'unexpected-compose\\n' >>"$transcript"; }
status=0
restore_images source.env || status=$?
printf 'status:%s\\n' "$status" >>"$transcript"
[[ "$status" == 1 ]]
`;
  const directory = await mkdtemp(join(tmpdir(), "weather-restore-failure-"));
  const transcript = join(directory, "transcript");

  try {
    const result = spawnSync("bash", [
      "-c", harness, "weather-restore-failure", update, transcript,
    ], { encoding: "utf8" });
    assert.equal(result.status, 0, result.stderr);
    assert.equal(
      await readFile(transcript, "utf8"),
      "prepare\npostgres-failed\nstatus:1\n",
    );
  } finally {
    await rm(directory, { force: true, recursive: true });
  }
});

// verify a blocked literal receipt stops before any pull authority
test("literal release capacity refusal is a hard shell gate", () => {
  const update = join(repoRoot, "deploy/scripts/update.sh");
  const harness = `
source "$1"
require_file() { :; }
env_value() {
  case "$2" in
    WEATHER_RELEASE) printf '%s\n' '2026.10.07-3' ;;
    WEATHER_CONTROL_PLANE_VERSION) printf '%s\n' '13' ;;
    WEATHER_SERVER_IMAGE) printf '%s\n' 'ghcr.io/anstosa/weather-server@sha256:${"a".repeat(64)}' ;;
    WEATHER_WEB_IMAGE) printf '%s\n' 'ghcr.io/anstosa/weather-web@sha256:${"b".repeat(64)}' ;;
  esac
}
node() {
  printf '%s\n' '{"contractVersion":"adjustment-release-capacity/v3","state":"capacity_blocked","retirementCreditBytes":0}'
  return 3
}
require_literal_adjustment_release_capacity source.env target.env
printf '%s\n' reached-pull
`;
  const result = spawnSync("bash", ["-c", harness, "weather-literal-capacity", update], {
    encoding: "utf8",
  });
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /literal adjustment release image capacity is blocked/u);
  assert.doesNotMatch(result.stdout, /reached-pull/u);
});

// verify trap-only cleanup portability
test("trap-only cleanup functions suppress supported ShellCheck diagnostics", () => {
  const update = read("deploy/scripts/update.sh");
  assert.match(
    update,
    /  # shellcheck disable=SC2317,SC2329\n  cleanup_compatibility\(\) \{/u,
  );
  assert.match(
    update,
    /  # shellcheck disable=SC2317,SC2329\n  cleanup_normal_activation\(\) \{/u,
  );
  assert.match(
    update,
    /  # shellcheck disable=SC2317,SC2329\n  restore_failed_yolo\(\) \{/u,
  );
  assert.match(
    update,
    /  # shellcheck disable=SC2317,SC2329\n  cleanup_historical_rehearsal\(\) \{/u,
  );
});

// verify release-state safety
test("release operations stage, compatibility-check, activate, rollback, and recover Weather only", () => {
  const update = read("deploy/scripts/update.sh");
  const stageCase = update.split("  stage)\n")[1].split("  activate)\n")[0];
  const rollbackCase = update.split("  rollback)\n")[1].split("  recover)\n")[0];
  const rollbackFunction = update
    .split("rollback_release() {")[1]
    .split("\n}\n\n# dispatch one operator action")[0];
  const startRelease = update
    .split("start_release() (")[1]
    .split("\n)\n\n# roll back images")[0];
  assert.match(stageCase, /compose config --quiet/u);
  assert.match(stageCase, /resolve_arm64_image/u);
  assert.match(stageCase, /compose pull/u);
  assert.match(stageCase, /cleanup_obsolete_weather_images/u);
  assert.match(stageCase, /require_image_pull_capacity_floor/u);
  assert.match(stageCase, /mktemp/u);
  assert.match(stageCase, /trap[\s\S]*EXIT/u);
  assert.match(stageCase, /mv[\s\S]*\$target/u);
  assert.doesNotMatch(stageCase, /compose up|compose down/u);
  const stageControlGate = stageCase.indexOf("normal_release_source_env");
  assert.notEqual(stageControlGate, -1);

  // require compatibility before stage mutation
  for (const mutation of [
    "require_normal_release_resource_gate",
    "mkdir -p \"$releases_dir\"",
    "resolve_arm64_image",
    "mktemp",
    "trap 'rm -f",
    "compose pull",
  ]) {
    assert.equal(stageControlGate < stageCase.indexOf(mutation), true, mutation);
  }
  const cleanupIndex = stageCase.indexOf("cleanup_obsolete_weather_images");
  const basicCapacityIndex = stageCase.indexOf("require_image_pull_capacity_floor");
  const fullCapacityIndex = stageCase.indexOf("require_normal_stage_capacity");
  const pullIndex = stageCase.indexOf("compose pull");
  assert.equal(fullCapacityIndex < cleanupIndex, true);
  assert.equal(cleanupIndex < basicCapacityIndex, true);
  assert.equal(basicCapacityIndex < pullIndex, true);
  assert.doesNotMatch(
    update.split("# locate one validated release environment")[0],
    /mkdir -p/u,
  );
  const activationControlGate = startRelease.indexOf("normal_release_source_env");
  const activationSchemaRead = startRelease.indexOf(
    'read_release_state "$state_dir/schema-release"',
  );
  const activationSchemaGate = startRelease.indexOf(
    '[[ "$current" == "$schema_release" && "$current" != "$target" ]]',
  );
  const activationCleanupTrap = startRelease.indexOf("trap cleanup_normal_activation EXIT");
  assert.notEqual(activationSchemaRead, -1);
  assert.equal(activationSchemaRead < activationSchemaGate, true);
  // reject stale targets before every mutation boundary
  for (const mutation of [
    "read_normal_stage_qualification",
    "require_deployment_secrets",
    "Creating pre-migration encrypted backup",
    "compose run --detach",
  ]) {
    assert.equal(activationSchemaGate < startRelease.indexOf(mutation), true, mutation);
  }
  assert.equal(
    activationSchemaGate < startRelease.lastIndexOf('start_postgres "$current_env"'),
    true,
  );
  assert.equal(
    activationSchemaGate < startRelease.lastIndexOf('write_private_state "$state_dir/schema-release"'),
    true,
  );
  assert.equal(activationSchemaGate < activationCleanupTrap, true);
  assert.equal(activationControlGate < activationCleanupTrap, true);
  assert.equal(activationCleanupTrap < startRelease.lastIndexOf('start_postgres "$current_env"'), true);
  assert.match(update, /imagetools inspect/u);
  assert.match(update, /weather_compat_/u);
  assert.match(update, /compatibility-provider\.mjs/u);
  assert.match(update, /WEATHER_OPEN_METEO_/u);
  assert.match(update, /WEATHER_DATABASE_NAME="\$candidate"/u);
  assert.match(update, /\/api\/v1\/health/u);
  assert.match(update, /\/api\/v1\/sites/u);
  assert.match(update, /\/current/u);
  assert.match(update, /\/history/u);
  assert.match(update, /apps\/api\/dist\/main\.js/u);
  assert.match(update, /apps\/worker\/dist\/worker\.js --once/u);
  assert.match(update, /state='succeeded'/u);
  assert.match(update, /0009_forecast_anchor_records\.sql/u);
  assert.match(update, /0014_rain_collection\.sql/u);
  assert.match(update, /0015_rain_station_access\.sql/u);
  assert.match(update, /0016_rain_adjustment\.sql/u);
  // retain the latest export schema in compatibility proof
  assert.match(update, /0017_adjustment_evaluation_export\.sql/u);
  assert.match(update, /DROP VIEW IF EXISTS rain_collection_status_v1/u);
  assert.match(update, /DROP TABLE IF EXISTS rain_adjustment_runs/u);
  assert.match(update, /DROP TABLE IF EXISTS rain_capture_receipts/u);
  assert.match(update, /DROP TABLE IF EXISTS forecast_anchor_records/u);
  assert.match(update, /open-meteo-forecast-v4/u);
  assert.match(update, /\/forecast/u);
  assert.match(update, /forecastBody\.data\.length===0/u);
  assert.match(update, /non_compatibility_source_ids/u);
  assert.match(update, /provider_key = 'open-meteo'/u);
  assert.match(update, /provider_key <> 'open-meteo'/u);
  assert.match(update, /UPDATE sources SET active = false/u);
  assert.match(update, /UPDATE sources SET active = true/u);
  assert.doesNotMatch(update, /state='success'/u);
  assert.match(update, /last_committed_at/u);
  assert.match(update, /verify_runtime_database_acl/u);
  assert.match(update, /migration_history_sha256/u);
  assert.match(update, /write_migration_authorization/u);
  assert.match(update, /schema-release/u);
  assert.doesNotMatch(update, /pg_dump[^\n]*--no-acl/u);
  assert.match(update, /Creating pre-migration encrypted backup/u);
  assert.match(update, /compose up -d --remove-orphans --wait/u);
  assert.match(update, /record success only after every health gate/u);
  assert.doesNotMatch(startRelease, /compose down --remove-orphans/u);
  assert.match(rollbackCase, /rollback_release/u);
  assert.match(rollbackFunction, /restore_images/u);
  assert.match(update, /--no-deps --force-recreate --wait postgres/u);
  assert.match(update, /--no-deps --wait[\s\\]+api worker web cloudflared/u);
  assert.doesNotMatch(rollbackFunction, /backup\.sh|compose run[^\n]*migration|start_release/u);
  assert.doesNotMatch(
    update,
    /docker (?:system|volume|network) prune|compose down[^\n]*(?:--volumes|\s-v(?:\s|$))/u,
  );
  assert.doesNotMatch(update, /docker image prune|docker image rm[^\n]*--force/u);
  assert.match(read("deploy/compose.local.yaml"), /WEATHER_LOCAL_CLOUDFLARED_IMAGE/u);
  const composeIntegration = read("deploy/test/compose.integration.test.mjs");
  // preserve prior image file modes under a restrictive umask
  assert.match(composeIntegration, /"git",[\s\n]*\["-c", "tar\.umask=0022", "archive"/u);
  assert.match(composeIntegration, /"--extract", "--same-permissions"/u);
  assert.match(composeIntegration, /baselineServerRelease = "2026\.09\.01-9"/u);
  assert.match(composeIntegration, /weather\.test\.baseline/u);
  assert.match(composeIntegration, /0009_forecast_anchor_records\.sql/u);
  assert.match(composeIntegration, /0014_rain_collection\.sql/u);
  assert.match(composeIntegration, /0015_rain_station_access\.sql/u);
  assert.match(composeIntegration, /0016_rain_adjustment\.sql/u);
  // bind the new migration at both image boundaries
  assert.match(composeIntegration, /0017_adjustment_evaluation_export\.sql/u);
  assert.match(composeIntegration, /9999_candidate_contract\.sql/u);
  assert.match(
    read("docs/operations/raspberry-pi.md"),
    /allowlists only the verified installed version 10[\s\S]*rejects any other[\s\S]*mismatch before changing/u,
  );
});

// verify capacity thresholds
test("capacity preflight records every numeric coexistence gate", () => {
  const preflight = read("deploy/scripts/preflight-capacity.sh");
  assert.match(preflight, /sample_seconds=900/u);
  assert.match(preflight, /aarch64/u);
  assert.match(preflight, /cpus >= 4/u);
  assert.match(preflight, /cpus \* 0\.50/u);
  assert.match(preflight, /1792 \* 1024 \* 1024/u);
  assert.match(preflight, /1024 \* 1024/u);
  assert.match(preflight, /3 \* database_bytes \+ 5 \* 1024 \* 1024 \* 1024/u);
  assert.match(preflight, /10 \* 1024 \* 1024 \* 1024/u);
  assert.match(preflight, /4 \* 1024 \* 1024 \* 1024/u);
  assert.match(preflight, /inode_free_percent >= 10/u);
  assert.match(preflight, /df --output=ipcent/u);
  assert.doesNotMatch(preflight, /df -Pi --output=ipcent/u);
  assert.match(preflight, /JSON\.stringify/u);
  assert.match(preflight, /while \(\([\s\S]*done\n\nload15=\$\(awk/u);
});

// verify strict release-state parsing
test("release state fails closed on missing, multiline, and unsafe values", async () => {
  const directory = await mkdtemp(join(tmpdir(), "weather-release-state-"));
  const common = join(scriptsRoot, "common.sh");
  const cases = ["", "2026.08.22-1\nextra\n", "../../escape\n", "latest\n"];

  try {
    // reject every malformed state case
    for (const [index, value] of cases.entries()) {
      const path = join(directory, `invalid-${index}`);
      await writeFile(path, value, { mode: 0o600 });
      const result = spawnSync(
        "bash",
        ["-c", 'source "$1"; read_release_state "$2"', "weather-state-test", common, path],
        { encoding: "utf8" },
      );
      assert.notEqual(result.status, 0, `${JSON.stringify(value)} must fail closed`);
    }

    const valid = join(directory, "valid");
    await writeFile(valid, "2026.08.22-1\n", { mode: 0o600 });
    const result = spawnSync(
      "bash",
      ["-c", 'source "$1"; read_release_state "$2"', "weather-state-test", common, valid],
      { encoding: "utf8" },
    );
    assert.equal(result.status, 0, result.stderr);
    assert.equal(result.stdout, "2026.08.22-1\n");
  } finally {
    await rm(directory, { force: true, recursive: true });
  }
});

// verify deterministic compatibility responses
test("compatibility provider serves deterministic weather and air-quality payloads", async () => {
  const providerPort = await reservePort();
  const provider = spawn(process.execPath, [join(scriptsRoot, "compatibility-provider.mjs")], {
    cwd: repoRoot,
    env: { ...process.env, HOST: "127.0.0.1", PORT: String(providerPort) },
    stdio: "ignore",
  });

  try {
    await waitForServer(`http://127.0.0.1:${providerPort}/health`);
    const current = await fetch(`http://127.0.0.1:${providerPort}/v1/forecast`);
    const forecast = await fetch(
      `http://127.0.0.1:${providerPort}/v1/forecast?hourly=temperature_2m`,
    );
    const airQuality = await fetch(
      `http://127.0.0.1:${providerPort}/v1/air-quality?hourly=pm2_5`,
    );
    const archive = await fetch(`http://127.0.0.1:${providerPort}/v1/archive`);
    const unknown = await fetch(`http://127.0.0.1:${providerPort}/unknown`);
    assert.equal(current.status, 200);
    assert.equal(forecast.status, 200);
    assert.equal(airQuality.status, 200);
    assert.equal(archive.status, 200);
    assert.equal(unknown.status, 404);
    assert.equal((await current.json()).current.time, "2026-08-22T06:30");
    assert.deepEqual(
      (await forecast.json()).hourly.uv_index,
      [0.2, 0.5, 0.9],
    );
    assert.deepEqual((await airQuality.json()).hourly.pm2_5, [5, 8, 12]);
    assert.deepEqual((await archive.json()).hourly.time, ["2026-08-22T05:00", "2026-08-22T06:00"]);
  } finally {
    // stop only the disposable provider
    provider.kill("SIGTERM");
    await Promise.race([
      once(provider, "exit"),
      new Promise((resolveWait) => setTimeout(resolveWait, 2_000)),
    ]);
  }
});

// verify isolated operator controls
test("SSH, sudo, and systemd controls use Weather-only absolute identities", () => {
  const ssh = [
    read("deploy/scripts/ssh-dispatch.sh"),
    read("deploy/scripts/ssh-run.sh"),
    read("deploy/scripts/remote-ops.sh"),
    read("deploy/config/ssh_config.example"),
    read("docs/operations/raspberry-pi.md"),
  ].join("\n");
  assert.match(ssh, /SSH_ORIGINAL_COMMAND/u);
  assert.match(ssh, /weather-ssh/u);
  assert.match(ssh, /PasswordAuthentication no/u);
  assert.match(ssh, /PermitRootLogin no/u);
  assert.doesNotMatch(ssh, /\beval\b|sh\s+-c\s+["']?\$SSH_ORIGINAL_COMMAND/u);
  assert.match(read("deploy/sudoers/weather-ops"), /weather-ssh[\s\S]*weather-remote-ops/u);
  const unit = read("deploy/systemd/weather-compose.service");
  assert.match(unit, /WorkingDirectory=\/opt\/weather\/current\/deploy/u);
  assert.match(unit, /--project-name weather/u);
  assert.doesNotMatch(unit, /\.\.\/|actionable/u);
  const backupService = read("deploy/systemd/weather-backup-local.service");
  const backupTimer = read("deploy/systemd/weather-backup-local.timer");
  assert.match(backupService, /ExecStart=%h\/weather\/deploy\/scripts\/pull-backup\.sh/u);
  assert.match(backupTimer, /OnCalendar=\*-\*-\* 02:30:00/u);
  assert.match(backupTimer, /Persistent=true/u);
  assert.match(ssh, /backup-stream/u);
  assert.match(ssh, /yolo/u);
});

// verify no neighboring deployment identity leaked
test("deployment artifacts contain no neighboring identity or production secret", () => {
  const backupsRoot = join(deployRoot, "backups");
  const files = [
    join(repoRoot, "Dockerfile"),
    ...collectFiles(deployRoot),
    ...collectFiles(join(repoRoot, "docs/operations")),
    ...collectFiles(join(repoRoot, ".github/workflows")),
  ];
  const combined = files
    // exclude encrypted runtime payloads
    .filter((path) => !path.startsWith(`${backupsRoot}${sep}`))
    .filter((path) => !path.endsWith("deployment-contract.test.mjs"))
    .map((path) => `# ${relative(repoRoot, path)}\n${readFileSync(path, "utf8")}`)
    .join("\n");
  assert.doesNotMatch(combined, /\/opt\/actionable|\/var\/lib\/actionable|actionable-compose|10001:10001/iu);
  assert.doesNotMatch(combined, /BEGIN (?:RSA|OPENSSH|EC) PRIVATE KEY|age-secret-key-/iu);
  assert.doesNotMatch(combined, /docker\s+(?:system|volume|network)\s+prune/iu);
  assert.doesNotMatch(combined, /cloudflared\s+tunnel\s+(?:create|delete|route)/iu);
});

// retain a complete local command without repeating static checks in ci
test("deployment workflows run static checks once and scope expensive integration", () => {
  const manifest = JSON.parse(read("package.json"));
  assert.equal(
    manifest.scripts["test:deploy"],
    "bash deploy/scripts/verify-static.sh && WEATHER_RUN_DEPLOY_INTEGRATION=1 node --test --test-concurrency=1 deploy/test/*.integration.test.mjs",
  );

  const check = read(".github/workflows/check.yml");
  const publish = read(".github/workflows/publish-images.yml");
  assert.equal((check.match(/run: deploy\/scripts\/verify-static\.sh/gu) ?? []).length, 1);
  assert.doesNotMatch(check, /run: npm run test:deploy/u);
  assert.match(check, /if: needs\.change-selection\.outputs\.deploy_integration == 'true'/u);
  assert.match(check, /run: WEATHER_RUN_DEPLOY_INTEGRATION=1 node --test --test-concurrency=1 deploy\/test\/\*\.integration\.test\.mjs/u);
  assert.doesNotMatch(check, /^\s+WEATHER_RUN_DEPLOY_INTEGRATION:/mu);
  assert.doesNotMatch(publish, /verify-static\.sh|npm run (?:check|test:deploy|test:integration|test:e2e)/u);
});

// verify release workflow immutability
test("release workflow publishes only immutable ARM64 server and web images", () => {
  const workflow = read(".github/workflows/publish-images.yml");
  assert.match(
    workflow,
    /tags:\n\s+- "20\[0-9\]\[0-9\]\.\[0-9\]\[0-9\]\.\[0-9\]\[0-9\]-\[0-9\]\+"/u,
  );
  assert.doesNotMatch(workflow, /tags:\n\s+- "[^"\n]*\?/u);
  assert.match(workflow, /for target in server web/u);
  assert.match(workflow, /--platform linux\/arm64/u);
  assert.match(workflow, /--provenance=true --push/u);
  assert.match(workflow, /--load/u);
  assert.match(workflow, /imagetools inspect/u);
  assert.match(workflow, /postgres:17\.10-bookworm/u);
  assert.match(workflow, /upload-artifact/u);
  const dependencyJob = workflow.split("  dependency-manifests:\n")[1].split("\n  publish:\n")[0];
  assert.match(dependencyJob, /runs-on: ubuntu-24\.04-arm/u);
  assert.match(workflow.split("\n  publish:\n")[1], /runs-on: ubuntu-24\.04-arm/u);
  assert.doesNotMatch(workflow, /setup-qemu/u);
  assert.match(workflow, /run: node scripts\/await-check\.mjs/u);
  assert.match(workflow, /CI_CHECK_MODE: release/u);
  assert.match(workflow, /CI_SHA: \$\{\{ steps\.release\.outputs\.sha \}\}/u);
  assert.match(workflow, /git rev-parse "refs\/tags\/\$\{release\}\^\{commit\}"/u);
  assert.match(workflow, /--target image-check-files/u);
  assert.match(workflow, /WEATHER_TEST_BUILD_PACKAGE_ROOT:/u);
  const inspection = workflow.indexOf("run: node --test deploy/test/forecast-adjustment-image.integration.test.mjs");
  const publication = workflow.indexOf("--provenance=true --push");
  assert.ok(inspection > 0 && publication > inspection);
  // retain separately named immutable digest evidence for host deployment
  for (const artifact of ["server-image-digest", "web-image-digest", "dependency-image-digests"]) {
    assert.ok(workflow.includes(`name: ${artifact}`));
  }
  assert.doesNotMatch(workflow, /:latest|ssh-run|update\.sh activate/u);
});

// retain build output comparison without another compilation or local state
test("image builds share one compilation and export exact comparison bytes", () => {
  const dockerfile = read("Dockerfile");
  assert.equal((dockerfile.match(/RUN npm run build/gu) ?? []).length, 1);
  assert.match(dockerfile, /COPY scripts\/build-workspaces\.mjs scripts\/build-workspaces\.mjs/u);
  assert.match(dockerfile, /FROM scratch AS image-check-files[\s\S]*COPY --from=build \/opt\/weather\/packages\/forecast-adjustment\/dist \/forecast-adjustment\/dist/u);
  assert.match(dockerfile, /FROM runtime AS web[\s\S]*ARG WEATHER_WEB_REVISION=development\nENV WEATHER_WEB_REVISION=\$\{WEATHER_WEB_REVISION\}/u);
  const publisher = read(".github/workflows/publish-images.yml");
  assert.equal((publisher.match(/--build-arg "WEATHER_WEB_REVISION=\$\{GITHUB_SHA\}"/gu) ?? []).length, 2);
  const ignored = read(".dockerignore").split("\n");
  assert.ok(ignored.includes("**/dist"));
  assert.ok(ignored.includes("node_modules"));
  assert.ok(ignored.includes(".omx"));
  // keep private research and deployment state out of the build context
  for (const path of [".weather-data", ".weather-models", "deploy/backups", "deploy/secrets", "deploy/releases", "deploy/state"]) {
    assert.ok(ignored.includes(path));
  }
});

// verify canonical PostgreSQL version output
test("dependency manifest workflow shell enforces ARM64 PostgreSQL 17+ verification", async () => {
  const directory = await mkdtemp(join(tmpdir(), "weather-workflow-execution-"));
  const bin = join(directory, "bin");
  const workflow = read(".github/workflows/publish-images.yml");
  const script = workflowRunScript(workflow, "Resolve pinned dependency ARM64 digests").replaceAll(
    "deploy/scripts/resolve-image.mjs",
    join(deployRoot, "scripts/resolve-image.mjs"),
  );

  try {
    await mkdir(bin);
    await writeFile(
      join(bin, "docker"),
      `#!/usr/bin/env bash
set -euo pipefail
# emulate manifest inspection
if [[ "$1 $2 $3" == "buildx imagetools inspect" ]]; then
  printf '{"schemaVersion":2,"manifests":[{"digest":"sha256:%s","platform":{"architecture":"arm64","os":"linux"}}]}\n' "$(printf 'c%.0s' {1..64})"
  exit 0
fi
# emulate ARM64 execution
if [[ "$1" == run ]]; then
  printf '%s\n' "$POSTGRES_VERSION_OUTPUT"
  exit 0
fi
exit 64
`,
    );
    await chmod(join(bin, "docker"), 0o700);
    // execute the exact workflow shell
    const runWithVersion = (versionOutput) =>
      spawnSync("bash", ["-c", script], {
        cwd: directory,
        encoding: "utf8",
        env: {
          ...process.env,
          PATH: `${bin}:${process.env.PATH}`,
          POSTGRES_VERSION_OUTPUT: versionOutput,
        },
      });
    const accepted17 = runWithVersion("postgres (PostgreSQL) 17.10 (Debian 17.10-1.pgdg13+1)");
    assert.equal(accepted17.status, 0, accepted17.stderr);
    assert.match(readFileSync(join(directory, "dependency-images.env"), "utf8"), /POSTGRES_IMAGE=.*@sha256:/u);
    assert.match(readFileSync(join(directory, "dependency-images.env"), "utf8"), /CLOUDFLARED_IMAGE=.*@sha256:/u);
    const accepted20 = runWithVersion("postgres (PostgreSQL) 20.1 (Debian 20.1-1.pgdg13+1)");
    assert.equal(accepted20.status, 0, accepted20.stderr);
    const rejected16 = runWithVersion("postgres (PostgreSQL) 16.9 (Debian 16.9-1.pgdg13+1)");
    assert.equal(rejected16.status, 1, rejected16.stderr);
    const rejected100 = runWithVersion("postgres (PostgreSQL) 100.1 (Debian 100.1-1.pgdg13+1)");
    assert.equal(rejected100.status, 1, rejected100.stderr);
  } finally {
    await rm(directory, { force: true, recursive: true });
  }
});

// reject any published runtime that differs from its inspected candidate
test("published digest verification requires the inspected image configuration", async () => {
  const directory = await mkdtemp(join(tmpdir(), "weather-image-digest-verification-"));
  const bin = join(directory, "bin");
  const script = workflowRunScript(read(".github/workflows/publish-images.yml"), "Resolve published ARM64 digests")
    .replaceAll("deploy/scripts/resolve-image.mjs", join(scriptsRoot, "resolve-image.mjs"));
  const digest = `sha256:${"d".repeat(64)}`;
  try {
    await mkdir(bin);
    await writeFile(join(bin, "docker"), `#!/usr/bin/env bash
set -euo pipefail
# emulate the published index and platform manifest
if [[ "$1 $2 $3" == "buildx imagetools inspect" ]]; then
  # return the platform configuration for immutable references
  if [[ "$4" == *@sha256:* ]]; then
    printf '{"config":{"digest":"%s"}}\\n' "$REMOTE_CONFIG"
  else
    printf '{"manifests":[{"digest":"sha256:%s","platform":{"architecture":"arm64","os":"linux"}}]}\\n' "$(printf 'c%.0s' {1..64})"
  fi
  exit 0
fi
# report the previously inspected local configuration
if [[ "$1 $2" == "image inspect" ]]; then
  printf '%s\\n' "$LOCAL_CONFIG"
  exit 0
fi
exit 64
`);
    await chmod(join(bin, "docker"), 0o700);
    // execute the exact publishing shell without contacting a registry
    const verify = (remoteConfig) => spawnSync("bash", ["-c", script], {
      cwd: directory,
      encoding: "utf8",
      env: { ...process.env, PATH: `${bin}:${process.env.PATH}`, RELEASE: "2026.09.14-6", LOCAL_CONFIG: digest, REMOTE_CONFIG: remoteConfig },
    });
    const mismatch = verify(`sha256:${"e".repeat(64)}`);
    assert.equal(mismatch.status, 1);
    assert.match(mismatch.stderr, /differs from inspected candidate/u);
    assert.equal(readdirSync(directory).includes("server-image-digest.env"), false);
    const success = verify(digest);
    assert.equal(success.status, 0, success.stderr);
    assert.match(readFileSync(join(directory, "server-image-digest.env"), "utf8"), /^WEATHER_SERVER_IMAGE=ghcr\.io\/anstosa\/weather-server@sha256:[a-f0-9]{64}\n$/u);
    assert.match(readFileSync(join(directory, "web-image-digest.env"), "utf8"), /^WEATHER_WEB_IMAGE=ghcr\.io\/anstosa\/weather-web@sha256:[a-f0-9]{64}\n$/u);
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});

test("production release reaches the API and deployment status exposes operations evidence", () => {
  const compose = renderCompose(["compose.verify.yaml"]);
  const status = read("deploy/scripts/status.sh");
  assert.equal(compose.services.api.environment.WEATHER_RELEASE, "2026.08.22-1");
  assert.equal(compose.services.api.environment.WEATHER_VERSION, undefined);
  assert.match(status, /env_value[^\n]*WEATHER_DATABASE_NAME/u);
  assert.doesNotMatch(status, /--dbname weather\b/u);
  assert.match(status, /connector/u);
  assert.match(status, /latest_run/u);
  assert.match(status, /stale_run/u);
  assert.match(status, /chunk_outcome/u);
  assert.match(status, /failure_evidence/u);
});

// keep mandatory smoke gates while scheduling full dependent coverage
test("check workflow builds once and retains conservative conditional coverage", () => {
  const workflow = read(".github/workflows/check.yml");
  assert.match(workflow, /actions\/checkout@v4[\s\S]*fetch-depth: 0/u);
  assert.match(workflow, /schedule:\n\s+- cron:/u);
  assert.match(workflow, /workflow_dispatch:/u);
  assert.match(workflow, /branches:\n\s+- '\*\*'/u);
  assert.match(workflow, /CI_CHECK_MODE: baseline/u);
  assert.match(workflow, /CI_BASE_VERIFIED:/u);
  assert.match(workflow, /run: node scripts\/ci-changes\.mjs/u);
  assert.equal((workflow.match(/run: npm run build/gu) ?? []).length, 1);
  assert.match(workflow, /run: npm run lint/u);
  assert.match(workflow, /npm run test:compiled/u);
  assert.match(workflow, /npm run test:fast:compiled/u);
  assert.match(workflow, /npm run test:integration:compiled/u);
  assert.match(workflow, /npm run test:e2e:compiled/u);
  assert.match(workflow, /npm run test:e2e:smoke:compiled/u);
  assert.doesNotMatch(workflow, /run: npm run check|docker build --target/u);
});

// keep helper coverage visible
test("all deployment shell entrypoints have stable names", () => {
  const names = collectFiles(scriptsRoot)
    .filter((path) => path.endsWith(".sh"))
    .map((path) => basename(path))
    .sort();
  assert.deepEqual(names, [
    "adjustment-evaluation-export.sh",
    "backup-stream.sh",
    "backup.sh",
    "common.sh",
    "forecast-training-export.sh",
    "install-adjustment-scorecard.sh",
    "preflight-capacity.sh",
    "public-stations-backfill.sh",
    "publish-adjustment-scorecard.sh",
    "pull-adjustment-evaluation-export.sh",
    "pull-backup.sh",
    "pull-forecast-training-export.sh",
    "remote-ops.sh",
    "restore.sh",
    "ssh-dispatch.sh",
    "ssh-run.sh",
    "status.sh",
    "tempest-backfill.sh",
    "tide-backfill.sh",
    "update.sh",
    "verify-static.sh",
  ]);
});
