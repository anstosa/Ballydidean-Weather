import assert from "node:assert/strict";
import test from "node:test";

import { migrateAndBootstrap, parseMigrationEntrypointArguments } from "../scripts/migrate.mjs";

// the optional bridges cannot accept arbitrary flags or schema operands
test("migration entrypoint accepts only the two closed atomic flags", () => {
  assert.deepEqual(parseMigrationEntrypointArguments([]), {});
  assert.deepEqual(parseMigrationEntrypointArguments(["--atomic-maintenance-v14"]), {
    atomicMaintenanceV14: true,
  });
  assert.deepEqual(parseMigrationEntrypointArguments(["--atomic-normal-21-23"]), {
    atomicNormal21To23: true,
  });
  // refuse duplicates and unknown arguments before database initialization
  for (const argumentsList of [["--atomic"], ["--atomic-maintenance-v14", "21"],
    ["--atomic-maintenance-v14", "--atomic-maintenance-v14"],
    ["--atomic-normal-21-23", "--atomic-normal-21-23"],
    ["--atomic-maintenance-v14", "--atomic-normal-21-23"]]) {
    assert.throws(() => parseMigrationEntrypointArguments(argumentsList), /arguments are invalid/u);
  }
});

// create ordered entrypoint doubles
function createDependencies(events, records) {
  return {
    // retain stable bootstrap identities
    async bootstrapSiteConfiguration(pool, configuration) {
      events.push(["bootstrap", pool.name, configuration.site.key]);
      const existing = records.get(configuration.site.key);

      // reuse the configured site identity
      if (existing !== undefined) {
        return existing;
      }

      const created = {
        providerId: "provider-id",
        siteId: "site-id",
        sourceIds: ["current-source-id", "archive-source-id"],
        stationId: "station-id",
      };
      records.set(configuration.site.key, created);
      return created;
    },
    // load the requested configuration
    async loadSiteConfiguration(path) {
      events.push(["load-site", path]);
      return { site: { key: "ballydidean" } };
    },
    // apply migrations before configuration loading
    async runMigrations(pool, directory) {
      events.push(["migrate", pool.name, directory]);
      return { applied: ["0001_initial_weather.sql"], current: [], serverVersionNum: 170_010 };
    },
  };
}

// create one disposable pool double
function createPool(name, events) {
  return {
    name,
    // record pool cleanup
    async end() {
      events.push(["end", name]);
    },
  };
}

// verify migration-first bootstrap ordering
test("migration entrypoint bootstraps the configured site after migrations", async () => {
  const events = [];
  const dependencies = createDependencies(events, new Map());
  const result = await migrateAndBootstrap(
    createPool("owner", events),
    "/migrations",
    "/config/ballydidean.json",
    dependencies,
  );

  assert.deepEqual(events, [
    ["migrate", "owner", "/migrations"],
    ["load-site", "/config/ballydidean.json"],
    ["bootstrap", "owner", "ballydidean"],
    ["end", "owner"],
  ]);
  assert.equal(result.bootstrap.siteId, "site-id");
  assert.deepEqual(result.migrations.applied, ["0001_initial_weather.sql"]);
});

// verify optional connector bootstrap ordering
test("migration entrypoint bootstraps Tempest after the owning site", async () => {
  const events = [];
  const dependencies = {
    async bootstrapSiteConfiguration() {
      events.push("bootstrap-site");
      return { siteId: "site-id" };
    },
    async bootstrapTempestConfiguration() {
      events.push("bootstrap-tempest");
      return { providerId: "tempest-provider-id" };
    },
    async loadSiteConfiguration() {
      events.push("load-site");
      return { site: { key: "ballydidean" } };
    },
    async loadTempestConfiguration() {
      events.push("load-tempest");
      return { siteKey: "ballydidean" };
    },
    async runMigrations() {
      events.push("migrate");
      return { applied: [], current: [], serverVersionNum: 170_010 };
    },
  };
  const result = await migrateAndBootstrap(
    createPool("owner", events),
    "/migrations",
    "/config/ballydidean.json",
    dependencies,
    {},
    "/config/tempest.json",
  );

  assert.deepEqual(events, [
    "migrate",
    "load-site",
    "bootstrap-site",
    "load-tempest",
    "bootstrap-tempest",
    ["end", "owner"],
  ]);
  assert.equal(result.tempestBootstrap.providerId, "tempest-provider-id");
});

// verify the first-party gateway catalog is included in production bootstrap
test("migration entrypoint bootstraps Ecowitt after the owning site", async () => {
  const events = [];
  const dependencies = {
    async bootstrapEcowittConfiguration() {
      events.push("bootstrap-ecowitt");
      return { providerId: "ecowitt-provider-id" };
    },
    async bootstrapSiteConfiguration() {
      events.push("bootstrap-site");
      return { siteId: "site-id" };
    },
    async loadEcowittConfiguration() {
      events.push("load-ecowitt");
      return { siteKey: "ballydidean" };
    },
    async loadSiteConfiguration() {
      events.push("load-site");
      return { site: { key: "ballydidean" } };
    },
    async runMigrations() {
      events.push("migrate");
      return { applied: [], current: [], serverVersionNum: 170_010 };
    },
  };
  const result = await migrateAndBootstrap(
    createPool("owner", events),
    "/migrations",
    "/config/ballydidean.json",
    dependencies,
    {},
    null,
    null,
    null,
    "/config/ecowitt.json",
  );

  assert.deepEqual(events, [
    "migrate",
    "load-site",
    "bootstrap-site",
    "load-ecowitt",
    "bootstrap-ecowitt",
    ["end", "owner"],
  ]);
  assert.equal(result.ecowittBootstrap.providerId, "ecowitt-provider-id");
});

test("migration entrypoint passes configured lock and statement timeouts", async () => {
  const events = [];
  const dependencies = {
    // return one stable bootstrap
    async bootstrapSiteConfiguration() {
      return { siteId: "site-id" };
    },
    // return one stable configuration
    async loadSiteConfiguration() {
      return { site: { key: "ballydidean" } };
    },
    // capture configured migration waits
    async runMigrations(_pool, _directory, options) {
      events.push(options);
      return { applied: [], current: ["0001_initial_weather.sql"], serverVersionNum: 170_010 };
    },
  };
  const options = { lockTimeoutMs: 12_345, statementTimeoutMs: 54_321 };
  await migrateAndBootstrap(
    createPool("owner", events),
    "/migrations",
    "/config/ballydidean.json",
    dependencies,
    options,
  );

  assert.deepEqual(events[0], options);
});

// verify the composed entrypoint can be rerun
test("migration entrypoint preserves bootstrap identities across reruns", async () => {
  const events = [];
  const records = new Map();
  const dependencies = createDependencies(events, records);
  const first = await migrateAndBootstrap(
    createPool("first-owner", events),
    "/migrations",
    "/config/ballydidean.json",
    dependencies,
  );
  const second = await migrateAndBootstrap(
    createPool("second-owner", events),
    "/migrations",
    "/config/ballydidean.json",
    dependencies,
  );

  assert.deepEqual(second.bootstrap, first.bootstrap);
  assert.equal(records.size, 1);
  assert.equal(events.filter(([event]) => event === "bootstrap").length, 2);
});

// verify failures still close the owner pool
test("migration entrypoint does not bootstrap after a migration failure", async () => {
  const events = [];
  const pool = createPool("owner", events);
  const dependencies = {
    // reject unexpected bootstrap calls
    async bootstrapSiteConfiguration() {
      assert.fail("bootstrap must not run");
    },
    // reject unexpected configuration loads
    async loadSiteConfiguration() {
      assert.fail("site configuration must not load");
    },
    // fail the migration step
    async runMigrations() {
      events.push(["migrate", "owner", "/migrations"]);
      throw new Error("migration failed");
    },
  };

  await assert.rejects(
    () => migrateAndBootstrap(pool, "/migrations", "/config/ballydidean.json", dependencies),
    /migration failed/u,
  );
  assert.deepEqual(events, [
    ["migrate", "owner", "/migrations"],
    ["end", "owner"],
  ]);
});
