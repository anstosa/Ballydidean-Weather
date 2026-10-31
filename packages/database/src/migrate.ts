import { createHash } from "node:crypto";
import { readdir, readFile } from "node:fs/promises";
import { join } from "node:path";

import type { Pool, PoolClient } from "pg";

import { assertSupportedPostgres, withTransaction } from "./pool.js";

const MIGRATION_LOCK_KEY = 8_032_416_683_782_917;

export interface MigrationResult {
  readonly applied: readonly string[];
  readonly current: readonly string[];
  readonly serverVersionNum: number;
}

export interface MigrationReadiness {
  readonly version: string;
}

export interface MigrationReadinessAuthorization {
  readonly historySha256: string;
  readonly release: string;
}

export interface MigrationReadinessOptions {
  readonly authorization?: MigrationReadinessAuthorization | null;
  readonly release?: string;
}

interface MigrationFile {
  readonly checksum: string;
  readonly name: string;
  readonly sql: string;
}

interface AppliedMigration {
  readonly checksum: string;
  readonly name: string;
}

const MIGRATION_AUTHORIZATION_RELEASE =
  "WEATHER_MIGRATION_AUTHORIZATION_RELEASE";
const MIGRATION_AUTHORIZATION_HISTORY_SHA256 =
  "WEATHER_MIGRATION_AUTHORIZATION_HISTORY_SHA256";
const RELEASE_PATTERN = /^\d{4}\.\d{2}\.\d{2}-[1-9]\d?$/u;
const SHA256_PATTERN = /^[a-f0-9]{64}$/u;
const MAINTENANCE_V14_HISTORY_SHA256 =
  "adb2aade2b019eb0e7a4df5b5bda38017ecbe143613df79cd873f67659068c93";
const NORMAL_FORECAST_HISTORY_SHA256 =
  "1db25d8a515e85b4c71c3a3dbe4a1f61ebdfdf14e095a24b307daa039dfb5201";

// apply checked migrations serially
export async function runMigrations(
  pool: Pool,
  migrationDirectory: string,
  options: Readonly<{
    atomicMaintenanceV14?: boolean;
    atomicNormal21To23?: boolean;
    lockTimeoutMs?: number;
    statementTimeoutMs?: number;
  }> = {},
): Promise<MigrationResult> {
  const client = await pool.connect();
  const lockTimeoutMs = options.lockTimeoutMs ?? 10_000;
  const statementTimeoutMs = options.statementTimeoutMs ?? 30_000;
  let lockHeld = false;

  try {
    const serverVersionNum = await assertSupportedPostgres(client);
    await configureTimeouts(client, lockTimeoutMs, statementTimeoutMs);
    await client.query("SELECT pg_advisory_lock($1::bigint)", [
      MIGRATION_LOCK_KEY,
    ]);
    lockHeld = true;
    await bootstrapMigrationTable(client);
    const migrations = await loadMigrationFiles(migrationDirectory);
    const appliedCount = (
      await validateMigrationHistory(client, migrations)
    ).length;
    const applied: string[] = [];
    const current: string[] = [];

    // keep the two closed atomic lanes mutually exclusive
    if (
      options.atomicMaintenanceV14 === true &&
      options.atomicNormal21To23 === true
    ) {
      throw new Error("atomic migration modes are mutually exclusive");
    }

    // close the inactive bridge to its exact reviewed source and target
    if (options.atomicMaintenanceV14 === true) {
      // reject partial tails and any unreviewed migration bytes
      if (
        migrations.length !== 21 ||
        (appliedCount !== 18 && appliedCount !== 21) ||
        migrationHistorySha256(migrations) !== MAINTENANCE_V14_HISTORY_SHA256
      ) {
        throw new Error("atomic maintenance v14 requires the exact 18-to-21 migration bridge");
      }

      current.push(...migrations.slice(0, appliedCount).map((migration) => migration.name));
      // retain the source ledger if any part of the three-file tail fails
      if (appliedCount === 18) {
        await withTransaction(client, async () => {
          // apply the frozen maintenance tail under one database transaction
          for (const migration of migrations.slice(appliedCount)) {
            await applyMigration(client, migration);
            applied.push(migration.name);
          }
        });
      }
    // close the normal forecast upgrade to its exact source and target
    } else if (options.atomicNormal21To23 === true) {
      // reject schema 22, partial tails and unreviewed migration bytes
      if (
        migrations.length !== 23 ||
        (appliedCount !== 21 && appliedCount !== 23) ||
        migrationHistorySha256(migrations) !== NORMAL_FORECAST_HISTORY_SHA256
      ) {
        throw new Error("atomic normal forecast upgrade requires the exact 21-to-23 migration bridge");
      }

      current.push(...migrations.slice(0, appliedCount).map((migration) => migration.name));
      // retain schema 21 if either additive migration fails
      if (appliedCount === 21) {
        await withTransaction(client, async () => {
          // apply the catalog and read index under one database transaction
          for (const migration of migrations.slice(appliedCount)) {
            await applyMigration(client, migration);
            applied.push(migration.name);
          }
        });
      }
    } else {
      // preserve the ordinary per-file migration transaction contract
      for (const [index, migration] of migrations.entries()) {
        // retain the verified prefix
        if (index < appliedCount) {
          current.push(migration.name);
          continue;
        }

        await withTransaction(client, async () => {
          await applyMigration(client, migration);
        });
        applied.push(migration.name);
      }
    }

    return { applied, current, serverVersionNum };
  } finally {
    // release the session lock
    if (lockHeld) {
      await client.query("SELECT pg_advisory_unlock($1::bigint)", [
        MIGRATION_LOCK_KEY,
      ]);
    }

    client.release();
  }
}

// record exact file bytes within the caller's transaction
async function applyMigration(client: PoolClient, migration: MigrationFile): Promise<void> {
  await client.query(migration.sql);
  await client.query(
    "INSERT INTO schema_migrations (name, checksum) VALUES ($1, $2)",
    [migration.name, migration.checksum],
  );
}

// verify the complete ledger without mutation
export async function verifyMigrationReadiness(
  pool: Pool,
  migrationDirectory: string,
  options: MigrationReadinessOptions = {},
): Promise<MigrationReadiness> {
  const migrations = await loadMigrationFiles(migrationDirectory);
  const authorization = options.authorization ?? null;
  const applied = await validateMigrationHistory(
    pool,
    migrations,
    authorization !== null,
  );
  const appliedCount = Math.min(applied.length, migrations.length);

  // bind compatibility to one running release
  if (
    authorization !== null &&
    (options.release === undefined || authorization.release !== options.release)
  ) {
    throw new Error("migration authorization release mismatch");
  }

  // authorize only the exact complete ledger
  if (
    applied.length > migrations.length &&
    (authorization === null ||
      authorization.historySha256 !== migrationHistorySha256(applied))
  ) {
    throw new Error(
      authorization === null
        ? "migration history diverges from ordered artifacts"
        : "migration authorization history mismatch",
    );
  }

  // require every shipped artifact
  if (appliedCount !== migrations.length) {
    throw new Error("database has pending migration artifacts");
  }

  const version = migrations.at(-1)?.name;

  // preserve the non-empty artifact invariant
  if (version === undefined) {
    throw new Error("migration directory contains no ordered SQL files");
  }

  return { version };
}

// parse bounded immutable runtime authorization
export function readMigrationReadinessAuthorization(
  environment: Readonly<Record<string, string | undefined>> = process.env,
): MigrationReadinessAuthorization | null {
  const release = environment[MIGRATION_AUTHORIZATION_RELEASE];
  const historySha256 = environment[MIGRATION_AUTHORIZATION_HISTORY_SHA256];

  // treat an explicitly empty compose contract as absent
  if (
    (release === undefined || release.length === 0) &&
    (historySha256 === undefined || historySha256.length === 0)
  ) {
    return null;
  }

  // reject partial authorization material
  if (
    release === undefined ||
    release.length === 0 ||
    historySha256 === undefined ||
    historySha256.length === 0
  ) {
    throw new Error("migration authorization must be complete");
  }

  // restrict authorization to immutable deployment releases
  if (!RELEASE_PATTERN.test(release)) {
    throw new Error("migration authorization release is invalid");
  }

  // require one canonical ledger digest
  if (!SHA256_PATTERN.test(historySha256)) {
    throw new Error("migration authorization history SHA-256 is invalid");
  }

  return { historySha256, release };
}

// configure bounded lock and statement waits
async function configureTimeouts(
  client: PoolClient,
  lockTimeoutMs: number,
  statementTimeoutMs: number,
): Promise<void> {
  // reject unsafe timeouts
  if (
    !Number.isSafeInteger(lockTimeoutMs) ||
    lockTimeoutMs < 100 ||
    lockTimeoutMs > 60_000 ||
    !Number.isSafeInteger(statementTimeoutMs) ||
    statementTimeoutMs < 100 ||
    statementTimeoutMs > 300_000
  ) {
    throw new RangeError("migration timeouts must be positive bounded integers");
  }

  await client.query("SELECT set_config('lock_timeout', $1, false)", [
    `${lockTimeoutMs}ms`,
  ]);
  await client.query("SELECT set_config('statement_timeout', $1, false)", [
    `${statementTimeoutMs}ms`,
  ]);
}

// create the checksum ledger before the first file
async function bootstrapMigrationTable(client: PoolClient): Promise<void> {
  await withTransaction(client, async () => {
    await client.query(`
      CREATE TABLE IF NOT EXISTS schema_migrations (
        name text PRIMARY KEY,
        checksum char(64) NOT NULL CHECK (checksum ~ '^[a-f0-9]{64}$'),
        applied_at timestamptz NOT NULL DEFAULT clock_timestamp()
      )
    `);
  });
}

// load ordered migration files
async function loadMigrationFiles(
  migrationDirectory: string,
): Promise<readonly MigrationFile[]> {
  const names = (await readdir(migrationDirectory))
    .filter((name) => /^\d{4}_[a-z0-9_]+\.sql$/u.test(name))
    .sort((left, right) => left.localeCompare(right));
  const migrations: MigrationFile[] = [];

  // fail closed on an empty migration directory
  if (names.length === 0) {
    throw new Error("migration directory contains no ordered SQL files");
  }

  // checksum exact file bytes
  for (const name of names) {
    const sql = await readFile(join(migrationDirectory, name), "utf8");
    migrations.push({
      checksum: createHash("sha256").update(sql).digest("hex"),
      name,
      sql,
    });
  }

  return migrations;
}

// validate the complete applied prefix
async function validateMigrationHistory(
  client: Pick<Pool | PoolClient, "query">,
  migrations: readonly MigrationFile[],
  allowTrailingApplied = false,
): Promise<readonly AppliedMigration[]> {
  const result = await client.query<AppliedMigration>(
    "SELECT name, checksum FROM schema_migrations ORDER BY name",
  );

  // require applied history to remain an exact prefix
  for (const [index, applied] of result.rows.entries()) {
    const artifact = migrations[index];

    // allow only authorized compatibility history
    if (artifact === undefined && allowTrailingApplied) {
      continue;
    }

    // reject deleted or retroactively inserted files
    if (artifact === undefined || artifact.name !== applied.name) {
      throw new Error("migration history diverges from ordered artifacts");
    }

    // fail closed on changed history
    if (applied.checksum !== artifact.checksum) {
      throw new Error(`migration checksum mismatch: ${artifact.name}`);
    }
  }

  return result.rows;
}

// hash one canonical ordered applied ledger
function migrationHistorySha256(
  migrations: readonly AppliedMigration[],
): string {
  const hash = createHash("sha256");

  // retain exact row boundaries
  for (const migration of migrations) {
    hash.update(`${migration.name}:${migration.checksum}\n`);
  }

  return hash.digest("hex");
}
