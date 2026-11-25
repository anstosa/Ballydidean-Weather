import assert from "node:assert/strict";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

import {
  loadDatabaseConfiguration,
  toPoolConfiguration,
} from "../../../../packages/database/dist/index.js";

const directory = await mkdtemp(join(tmpdir(), "weather-rust-db-config-"));
const passwordFile = join(directory, "password");

// build one disposable environment object
function environment(overrides = {}) {
  return {
    WEATHER_DATABASE_HOST: "127.0.0.1",
    WEATHER_DATABASE_NAME: "weather",
    WEATHER_DATABASE_PASSWORD_FILE: passwordFile,
    WEATHER_DATABASE_USER: "weather_owner",
    ...overrides,
  };
}

try {
  await writeFile(passwordFile, "secret-value\n", { mode: 0o600 });
  const defaults = await loadDatabaseConfiguration(environment());
  assert.deepEqual(defaults, {
    applicationName: "weather-database",
    database: "weather",
    host: "127.0.0.1",
    lockTimeoutMs: 10_000,
    password: "secret-value",
    port: 5_432,
    ssl: false,
    statementTimeoutMs: 30_000,
    user: "weather_owner",
  });
  assert.deepEqual(toPoolConfiguration(defaults), {
    application_name: "weather-database",
    database: "weather",
    host: "127.0.0.1",
    max: 10,
    password: "secret-value",
    port: 5_432,
    ssl: false,
    statement_timeout: 30_000,
    user: "weather_owner",
  });

  await writeFile(passwordFile, "secret-value \r\n", { mode: 0o600 });
  const explicit = await loadDatabaseConfiguration(environment({
    WEATHER_DATABASE_APPLICATION_NAME: "weather-api",
    WEATHER_DATABASE_LOCK_TIMEOUT_MS: " 4.321e3 ",
    WEATHER_DATABASE_PORT: "65535",
    WEATHER_DATABASE_SSL: "true",
    WEATHER_DATABASE_STATEMENT_TIMEOUT_MS: "300000",
  }));
  assert.equal(explicit.applicationName, "weather-api");
  assert.equal(explicit.lockTimeoutMs, 4_321);
  assert.equal(explicit.password, "secret-value ");
  assert.equal(explicit.port, 65_535);
  assert.equal(explicit.ssl, true);
  assert.equal(explicit.statementTimeoutMs, 300_000);

  const prefixed = await loadDatabaseConfiguration(environment({
    WEATHER_DATABASE_LOCK_TIMEOUT_MS: "\uFEFF0B1111101000\u3000",
    WEATHER_DATABASE_PORT: " 0x1538 ",
    WEATHER_DATABASE_STATEMENT_TIMEOUT_MS: "0o72460",
  }));
  assert.equal(prefixed.lockTimeoutMs, 1_000);
  assert.equal(prefixed.port, 5_432);
  assert.equal(prefixed.statementTimeoutMs, 30_000);

  await assert.rejects(
    loadDatabaseConfiguration(environment({ WEATHER_DATABASE_HOST: " " })),
    /WEATHER_DATABASE_HOST is required/u,
  );
  await assert.rejects(
    loadDatabaseConfiguration(environment({ WEATHER_DATABASE_PORT: "65536" })),
    /WEATHER_DATABASE_PORT must be between 1 and 65535/u,
  );
  const invalidPorts = [
    "+0x1538",
    "0x+1538",
    "-0b1",
    "0o10ms",
    "\u00855432",
  ];

  // reject signed prefixes suffixes and non-node whitespace
  for (const port of invalidPorts) {
    await assert.rejects(
      loadDatabaseConfiguration(environment({ WEATHER_DATABASE_PORT: port })),
      /WEATHER_DATABASE_PORT must be between 1 and 65535/u,
    );
  }
  await assert.rejects(
    loadDatabaseConfiguration(environment({ WEATHER_DATABASE_SSL: "TRUE" })),
    /boolean setting must be true or false/u,
  );
} finally {
  await rm(directory, { force: true, recursive: true });
}

process.stdout.write("database configuration Node oracle passed\n");
