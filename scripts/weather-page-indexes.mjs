#!/usr/bin/env node

import { spawn } from "node:child_process";
import { createHash, randomUUID } from "node:crypto";
import {
  constants as fsConstants,
  createReadStream,
} from "node:fs";
import {
  chmod,
  link,
  lstat,
  mkdir,
  open,
  readFile,
  readlink,
  readdir,
  realpath,
  rename,
  stat,
  statfs,
  unlink,
} from "node:fs/promises";
import { arch, cpus, loadavg } from "node:os";
import { basename, dirname, isAbsolute, join, relative, resolve, sep } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const SCRIPT_PATH = fileURLToPath(import.meta.url);
const SCRIPT_ROOT = dirname(SCRIPT_PATH);
const HASH_PATTERN = /^[a-f0-9]{64}$/u;
const UUID_PATTERN = /^[a-f0-9]{8}-[a-f0-9]{4}-4[a-f0-9]{3}-[89ab][a-f0-9]{3}-[a-f0-9]{12}$/u;
const GIT_HASH_PATTERN = /^[a-f0-9]{40}$/u;
const RELEASE_PATTERN = /^\d{4}\.\d{2}\.\d{2}-[1-9]\d?$/u;
const SAFE_IDENTIFIER_PATTERN = /^[a-z_][a-z0-9_]{0,62}$/u;
const ADMISSION_IDENTITY_KEYS = [
  "aclDigest",
  "catalogDigest",
  "controlDigest",
  "databaseDigest",
  "forecastSettingsDigest",
  "imageDigest",
  "ledgerDigest",
  "manifestDigest",
  "packageDigest",
  "settingsDigest",
  "sourceDigest",
];
const PACKAGE_FILES = [
  "release-manifest.json",
  "weather-page-indexes.json",
  "weather-page-indexes.mjs",
];
const SETTING_NAMES = [
  "block_size",
  "data_directory",
  "data_checksums",
  "default_tablespace",
  "lc_collate",
  "lc_ctype",
  "max_connections",
  "server_version_num",
  "shared_buffers",
  "temp_tablespaces",
  "wal_level",
];
const REPLACEMENT_PEAK_FIELDS = [
  "indexBytesHighWater",
  "inodesHighWater",
  "tempBytesHighWater",
  "walGeneratedBytes",
  "walRetainedBytesHighWater",
];

// report one bounded contract failure
export class ContractError extends Error {
  // initialize one contract failure
  constructor(message, details = undefined) {
    super(message);
    this.name = "ContractError";
    this.details = details;
  }
}

// require one true contract condition
export function requireCondition(condition, message, details = undefined) {
  // reject false conditions
  if (!condition) {
    throw new ContractError(message, details);
  }
}

// convert an integer-like value
export function toBigInt(value, label) {
  // accept safe numeric integers
  if (typeof value === "number") {
    requireCondition(Number.isSafeInteger(value) && value >= 0, `${label} must be a nonnegative safe integer`);
    return BigInt(value);
  }

  requireCondition(typeof value === "string" && /^(0|[1-9]\d*)$/u.test(value), `${label} must be a nonnegative integer string`);
  return BigInt(value);
}

// divide with an integer ceiling
export function ceilDivide(numerator, denominator) {
  requireCondition(denominator > 0n, "division denominator must be positive");
  return (numerator + denominator - 1n) / denominator;
}

// sort object keys recursively
export function stableValue(value) {
  // preserve arrays in declared order
  if (Array.isArray(value)) {
    const result = [];

    // normalize each array entry
    for (const entry of value) {
      result.push(stableValue(entry));
    }

    return result;
  }

  // sort plain object keys
  if (value !== null && typeof value === "object") {
    const result = {};
    const keys = Object.keys(value).sort();

    // normalize each object entry
    for (const key of keys) {
      result[key] = stableValue(value[key]);
    }

    return result;
  }

  return value;
}

// serialize one canonical value
export function stableJson(value) {
  return JSON.stringify(stableValue(value));
}

// hash bytes or text
export function sha256(value) {
  return createHash("sha256").update(value).digest("hex");
}

// hash one regular file
export async function hashFile(path) {
  const handle = await open(path, fsConstants.O_RDONLY | fsConstants.O_NOFOLLOW);
  const digest = createHash("sha256");

  try {
    const stream = createReadStream(path, { fd: handle.fd, autoClose: false });

    // absorb file chunks
    for await (const chunk of stream) {
      digest.update(chunk);
    }

    return digest.digest("hex");
  } finally {
    await handle.close();
  }
}

// read one strict json document
export async function readJson(path) {
  const bytes = await readFile(path);
  let value;

  try {
    value = JSON.parse(bytes.toString("utf8"));
  } catch (error) {
    throw new ContractError(`invalid JSON document: ${path}`, { cause: error.message });
  }

  requireCondition(value !== null && typeof value === "object" && !Array.isArray(value), `JSON document must be an object: ${path}`);
  return { bytes, value };
}

// quote one sql literal
export function quoteLiteral(value) {
  requireCondition(typeof value === "string", "SQL literal must be a string");
  return `'${value.replaceAll("'", "''")}'`;
}

// quote one sql identifier
export function quoteIdentifier(value) {
  requireCondition(typeof value === "string" && value.length > 0, "SQL identifier must be nonempty");
  return `"${value.replaceAll('"', '""')}"`;
}

// normalize canonical index sql
export function normalizeIndexDefinition(value) {
  requireCondition(typeof value === "string", "index definition must be text");
  return value.replace(/\s+/gu, " ").trim().replace(/;$/u, "");
}

// validate the closed index manifest
export function validateIndexManifest(manifest) {
  requireCondition(manifest?.schema === "weather-page-indexes/v2", "unsupported index manifest schema");
  requireCondition(manifest.operation?.advisoryLockKey === "8032416683782917", "unexpected advisory lock key");
  requireCondition(manifest.replacement?.schema === "weather-page-index-replacement/v1", "replacement manifest schema changed");
  const replacement = manifest.replacement;
  const declared = [replacement.target, replacement.predecessor, replacement.covering, ...replacement.protected];
  requireCondition(Array.isArray(replacement.protected) && replacement.protected.length === 3, "replacement protected index set changed");
  requireCondition(stableJson(declared.map((index) => [index?.id, index?.name])) === stableJson([
    ["weather-records-source-product-run", "weather_records_source_product_run_idx"],
    ["weather-records-source-valid", "weather_records_source_valid_idx"],
    ["weather-records-current", "weather_records_current_idx"],
    ["weather-records-primary", "weather_records_pkey"],
    ["weather-records-identity", "weather_records_identity_key"],
    ["weather-records-kind-valid", "weather_records_kind_valid_idx"],
  ]), "replacement index ordering changed");
  const expected = [
    { keys: ["source_id:bigint:int8_ops:null:ASC:LAST", "product_run_at:timestamp with time zone:timestamptz_ops:null:ASC:LAST"], include: [], dependencies: ["source_id", "product_run_at", "product_run_at"], unique: false, primary: false, nullsNotDistinct: false, predicate: "product_run_at IS NOT NULL", constraint: null, canonical: "CREATE INDEX weather_records_source_product_run_idx ON public.weather_records USING btree (source_id, product_run_at) WHERE (product_run_at IS NOT NULL)", create: "CREATE INDEX CONCURRENTLY weather_records_source_product_run_idx ON public.weather_records USING btree (source_id, product_run_at) WHERE product_run_at IS NOT NULL", drop: "DROP INDEX CONCURRENTLY public.weather_records_source_product_run_idx" },
    { keys: ["source_id:bigint:int8_ops:null:ASC:LAST", "valid_at:timestamp with time zone:timestamptz_ops:null:DESC:FIRST", "id:bigint:int8_ops:null:DESC:FIRST"], include: [], dependencies: ["source_id", "valid_at", "id"], unique: false, primary: false, nullsNotDistinct: false, predicate: null, constraint: null, canonical: "CREATE INDEX weather_records_source_valid_idx ON public.weather_records USING btree (source_id, valid_at DESC, id DESC)", create: "CREATE INDEX CONCURRENTLY weather_records_source_valid_idx ON public.weather_records USING btree (source_id, valid_at DESC, id DESC)", drop: "DROP INDEX CONCURRENTLY public.weather_records_source_valid_idx" },
    { keys: ["source_id:bigint:int8_ops:null:ASC:LAST", "valid_at:timestamp with time zone:timestamptz_ops:null:DESC:FIRST", "id:bigint:int8_ops:null:DESC:FIRST"], include: ["last_received_at:timestamp with time zone", "source_kind:character varying(32)"], dependencies: ["source_id", "valid_at", "id", "last_received_at", "source_kind"], unique: false, primary: false, nullsNotDistinct: false, predicate: null, constraint: null, canonical: "CREATE INDEX weather_records_current_idx ON public.weather_records USING btree (source_id, valid_at DESC, id DESC) INCLUDE (last_received_at, source_kind)" },
    { keys: ["id:bigint:int8_ops:null:ASC:LAST"], include: [], dependencies: [], unique: true, primary: true, nullsNotDistinct: false, predicate: null, constraint: { name: "weather_records_pkey", type: "p", validated: true }, canonical: "CREATE UNIQUE INDEX weather_records_pkey ON public.weather_records USING btree (id)" },
    { keys: ["source_id:bigint:int8_ops:null:ASC:LAST", "source_kind:character varying(32):text_ops:pg_catalog.default:ASC:LAST", "valid_at:timestamp with time zone:timestamptz_ops:null:ASC:LAST", "product_run_at:timestamp with time zone:timestamptz_ops:null:ASC:LAST"], include: [], dependencies: [], unique: true, primary: false, nullsNotDistinct: true, predicate: null, constraint: { name: "weather_records_identity_key", type: "u", validated: true }, canonical: "CREATE UNIQUE INDEX weather_records_identity_key ON public.weather_records USING btree (source_id, source_kind, valid_at, product_run_at) NULLS NOT DISTINCT" },
    { keys: ["source_kind:character varying(32):text_ops:pg_catalog.default:ASC:LAST", "valid_at:timestamp with time zone:timestamptz_ops:null:DESC:FIRST", "id:bigint:int8_ops:null:DESC:FIRST"], include: [], dependencies: ["source_kind", "valid_at", "id"], unique: false, primary: false, nullsNotDistinct: false, predicate: null, constraint: null, canonical: "CREATE INDEX weather_records_kind_valid_idx ON public.weather_records USING btree (source_kind, valid_at DESC, id DESC)" },
  ];

  // validate every exact semantic index declaration
  for (let position = 0; position < declared.length; position += 1) {
    const index = declared[position];
    const contract = expected[position];
    requireCondition(index.schema === "public" && index.table === "weather_records" && index.method === "btree", `replacement index ${position + 1} table or method changed`);
    requireCondition(index.unique === contract.unique && index.primary === contract.primary && index.nullsNotDistinct === contract.nullsNotDistinct
      && index.exclusion === false && index.immediate === true && index.clustered === false && index.checkXmin === false && index.replicaIdentity === false, `replacement index ${position + 1} flags changed`);
    requireCondition(stableJson(index.keys.map((key) => `${key.name}:${key.type}:${key.opclass}:${key.collation === null ? "null" : `${key.collation?.schema}.${key.collation?.name}`}:${key.direction}:${key.nulls}`)) === stableJson(contract.keys)
      && index.keys.every((key) => key.opclassSchema === "pg_catalog"), `replacement index ${position + 1} keys changed`);
    requireCondition(stableJson(index.include.map((column) => `${column.name}:${column.type}`)) === stableJson(contract.include), `replacement index ${position + 1} includes changed`);
    requireCondition(stableJson(index.dependencyColumns) === stableJson(contract.dependencies), `replacement index ${position + 1} dependencies changed`);
    requireCondition(index.expressions === null && index.predicate === contract.predicate
      && index.tablespace === null && index.effectiveTablespace === "pg_default" && stableJson(index.reloptions) === "[]", `replacement index ${position + 1} expression or storage changed`);
    requireCondition(stableJson(index.constraint) === stableJson(contract.constraint)
      && normalizeIndexDefinition(index.canonicalDefinition) === contract.canonical, `replacement index ${position + 1} definition changed`);
    requireCondition(contract.create === undefined || normalizeIndexDefinition(index.createSql) === contract.create, `replacement index ${position + 1} create SQL changed`);
    requireCondition(contract.drop === undefined || normalizeIndexDefinition(index.dropSql) === contract.drop, `replacement index ${position + 1} drop SQL changed`);
  }

  validateRestoreDescriptor(manifest.restoreContext);
  return manifest;
}

// validate frozen restore identity
export function validateRestoreDescriptor(descriptor) {
  requireCondition(descriptor?.schema === "weather-page-index-restore-context/v1", "unsupported restore context schema");
  requireCondition(descriptor.repository === "anstosa/weather", "unexpected restore repository");
  requireCondition(descriptor.tag === "2026.10.09-1", "unexpected restore tag");
  requireCondition(descriptor.commit === "56b327d9c750946f6f6963b6fe1fa5c9bba791ca", "unexpected restore commit");
  requireCondition(descriptor.tree === "ecd9599ebf4907c67b912c742eb8fc3d8e6ee8df", "unexpected restore tree");
  requireCondition(descriptor.fileCount === 813 && descriptor.decodedBytes === 17_238_740, "unexpected restore tree dimensions");
  requireCondition(Array.isArray(descriptor.migrations) && descriptor.migrations.length === 18, "restore context must bind eighteen migrations");

  // enforce exact migration ordering
  for (let position = 0; position < descriptor.migrations.length; position += 1) {
    const expectedPrefix = String(position + 1).padStart(4, "0");
    requireCondition(basename(descriptor.migrations[position]).startsWith(`${expectedPrefix}_`), `restore migration ${expectedPrefix} is missing or reordered`);
  }

  return descriptor;
}

// construct the complete catalog query
export function catalogFingerprintSql(indexSpec, database = undefined) {
  const qualifiedTable = `${quoteIdentifier(indexSpec.schema)}.${quoteIdentifier(indexSpec.table)}`;
  const indexName = quoteLiteral(indexSpec.name);
  const databaseFilter = database === undefined ? "current_database()" : quoteLiteral(database);

  return `
WITH target AS (
  SELECT
    idx.indexrelid,
    idx.indrelid,
    idx.indnatts,
    idx.indnkeyatts,
    idx.indisunique,
    idx.indisprimary,
    idx.indisexclusion,
    idx.indimmediate,
    idx.indisclustered,
    idx.indisvalid,
    idx.indcheckxmin,
    idx.indisready,
    idx.indislive,
    idx.indisreplident,
    idx.indnullsnotdistinct,
    idx.indkey,
    idx.indcollation,
    idx.indclass,
    idx.indoption,
    idx.indexprs,
    idx.indpred
  FROM pg_catalog.pg_index idx
  JOIN pg_catalog.pg_class index_relation ON index_relation.oid = idx.indexrelid
  JOIN pg_catalog.pg_namespace index_namespace ON index_namespace.oid = index_relation.relnamespace
  WHERE index_namespace.nspname = ${quoteLiteral(indexSpec.schema)}
    AND index_relation.relname = ${indexName}
), positions AS (
  SELECT target.*, position
  FROM target
  CROSS JOIN LATERAL generate_series(1, target.indnatts) AS position
)
SELECT COALESCE((
  SELECT json_build_object(
    'database', ${databaseFilter},
    'databaseOid', (SELECT oid::text FROM pg_catalog.pg_database WHERE datname = ${databaseFilter}),
    'schema', table_namespace.nspname,
    'table', table_relation.relname,
    'tableOid', table_relation.oid::text,
    'tableOwner', pg_catalog.pg_get_userbyid(table_relation.relowner),
    'indexSchema', index_namespace.nspname,
    'index', index_relation.relname,
    'indexOid', index_relation.oid::text,
    'indexOwner', pg_catalog.pg_get_userbyid(index_relation.relowner),
    'accessMethod', access_method.amname,
    'keyCount', target.indnkeyatts,
    'attributeCount', target.indnatts,
    'indkey', ARRAY(SELECT key_value::integer FROM unnest(target.indkey) WITH ORDINALITY AS key_entry(key_value, ordinal) ORDER BY ordinal),
    'columns', COALESCE((SELECT json_agg(json_build_object(
      'position', positions.position,
      'attributeNumber', positions.indkey[positions.position - 1],
      'name', attribute.attname,
      'type', CASE WHEN attribute.attnum = 0 THEN NULL ELSE pg_catalog.format_type(attribute.atttypid, attribute.atttypmod) END
    ) ORDER BY positions.position)
      FROM positions
      LEFT JOIN pg_catalog.pg_attribute attribute
        ON attribute.attrelid = positions.indrelid
       AND attribute.attnum = positions.indkey[positions.position - 1]
    ), '[]'::json),
    'opclasses', COALESCE((SELECT json_agg(json_build_object(
      'position', positions.position,
      'oid', operator_class.oid::text,
      'schema', operator_namespace.nspname,
      'name', operator_class.opcname,
      'default', operator_class.opcdefault
    ) ORDER BY positions.position)
      FROM positions
      JOIN pg_catalog.pg_opclass operator_class ON operator_class.oid = positions.indclass[positions.position - 1]
      JOIN pg_catalog.pg_namespace operator_namespace ON operator_namespace.oid = operator_class.opcnamespace
    ), '[]'::json),
    'collations', COALESCE((SELECT json_agg(CASE
      WHEN positions.indcollation[positions.position - 1] = 0 THEN NULL
      ELSE json_build_object(
        'position', positions.position,
        'oid', collation_relation.oid::text,
        'schema', collation_namespace.nspname,
        'name', collation_relation.collname
      ) END ORDER BY positions.position)
      FROM positions
      LEFT JOIN pg_catalog.pg_collation collation_relation ON collation_relation.oid = positions.indcollation[positions.position - 1]
      LEFT JOIN pg_catalog.pg_namespace collation_namespace ON collation_namespace.oid = collation_relation.collnamespace
    ), '[]'::json),
    'indoption', ARRAY(SELECT option_value::integer FROM unnest(target.indoption) WITH ORDINALITY AS option_entry(option_value, ordinal) ORDER BY ordinal),
    'ordering', COALESCE((SELECT json_agg(json_build_object(
      'position', positions.position,
      'direction', CASE WHEN (positions.indoption[positions.position - 1] & 1) = 1 THEN 'DESC' ELSE 'ASC' END,
      'nulls', CASE WHEN (positions.indoption[positions.position - 1] & 2) = 2 THEN 'FIRST' ELSE 'LAST' END
    ) ORDER BY positions.position) FROM positions), '[]'::json),
    'expressions', pg_catalog.pg_get_expr(target.indexprs, target.indrelid, true),
    'predicate', pg_catalog.pg_get_expr(target.indpred, target.indrelid, true),
    'unique', target.indisunique,
    'primary', target.indisprimary,
    'exclusion', target.indisexclusion,
    'immediate', target.indimmediate,
    'clustered', target.indisclustered,
    'checkXmin', target.indcheckxmin,
    'replicaIdentity', target.indisreplident,
    'nullsNotDistinct', target.indnullsnotdistinct,
    'valid', target.indisvalid,
    'ready', target.indisready,
    'live', target.indislive,
    'canonicalDefinition', pg_catalog.pg_get_indexdef(target.indexrelid, 0, false),
    'storedTablespace', CASE WHEN index_relation.reltablespace = 0 THEN NULL ELSE stored_tablespace.spcname END,
    'effectiveTablespace', CASE
      WHEN index_relation.reltablespace = 0 THEN database_tablespace.spcname
      ELSE stored_tablespace.spcname
    END,
    'reloptions', COALESCE(index_relation.reloptions, ARRAY[]::text[]),
    'constraintBacking', COALESCE((SELECT json_agg(json_build_object(
      'oid', constraint_relation.oid::text,
      'schema', constraint_namespace.nspname,
      'name', constraint_relation.conname,
      'type', constraint_relation.contype,
      'validated', constraint_relation.convalidated
    ) ORDER BY constraint_namespace.nspname, constraint_relation.conname)
      FROM pg_catalog.pg_constraint constraint_relation
      JOIN pg_catalog.pg_namespace constraint_namespace ON constraint_namespace.oid = constraint_relation.connamespace
      WHERE constraint_relation.conindid = target.indexrelid
    ), '[]'::json),
    'dependencies', COALESCE((SELECT json_agg(json_build_object(
      'class', dependency.classid::regclass::text,
      'objectId', dependency.objid::text,
      'objectSubId', dependency.objsubid,
      'referencedClass', dependency.refclassid::regclass::text,
      'referencedObjectId', dependency.refobjid::text,
      'referencedObjectSubId', dependency.refobjsubid,
      'type', dependency.deptype,
      'object', pg_catalog.pg_describe_object(dependency.classid, dependency.objid, dependency.objsubid),
      'referencedObject', pg_catalog.pg_describe_object(dependency.refclassid, dependency.refobjid, dependency.refobjsubid)
    ) ORDER BY dependency.classid, dependency.objid, dependency.objsubid, dependency.refclassid, dependency.refobjid, dependency.refobjsubid, dependency.deptype)
      FROM pg_catalog.pg_depend dependency
      WHERE dependency.classid = 'pg_catalog.pg_class'::regclass
        AND dependency.objid = target.indexrelid
    ), '[]'::json)
  )
  FROM target
  JOIN pg_catalog.pg_class table_relation ON table_relation.oid = target.indrelid
  JOIN pg_catalog.pg_namespace table_namespace ON table_namespace.oid = table_relation.relnamespace
  JOIN pg_catalog.pg_class index_relation ON index_relation.oid = target.indexrelid
  JOIN pg_catalog.pg_namespace index_namespace ON index_namespace.oid = index_relation.relnamespace
  JOIN pg_catalog.pg_am access_method ON access_method.oid = index_relation.relam
  JOIN pg_catalog.pg_database database_relation ON database_relation.datname = ${databaseFilter}
  JOIN pg_catalog.pg_tablespace database_tablespace ON database_tablespace.oid = database_relation.dattablespace
  LEFT JOIN pg_catalog.pg_tablespace stored_tablespace ON stored_tablespace.oid = index_relation.reltablespace
  WHERE target.indrelid = ${quoteLiteral(qualifiedTable)}::regclass
), 'null'::json)::text;
`.trim();
}

// normalize a catalog fingerprint
export function normalizeCatalogFingerprint(value) {
  // preserve missing candidates
  if (value === null || value === undefined) {
    return null;
  }

  const normalized = structuredClone(value);
  normalized.canonicalDefinition = normalizeIndexDefinition(normalized.canonicalDefinition);

  // sort declared storage options
  if (Array.isArray(normalized.reloptions)) {
    normalized.reloptions.sort();
  }

  // sort dependencies by canonical bytes
  if (Array.isArray(normalized.dependencies)) {
    // order complete dependency tuples
    normalized.dependencies.sort((left, right) => stableJson(left).localeCompare(stableJson(right)));
  }

  // sort constraints by canonical bytes
  if (Array.isArray(normalized.constraintBacking)) {
    // order complete constraint tuples
    normalized.constraintBacking.sort((left, right) => stableJson(left).localeCompare(stableJson(right)));
  }

  return normalized;
}

// hash a complete catalog tuple
export function catalogFingerprintHash(value) {
  return sha256(stableJson(normalizeCatalogFingerprint(value)));
}

// capture the complete table index inventory in name order
export async function captureIndexInventory(config, indexSpec, database = config.postgres.database) {
  const names = await queryPsqlJson(config, `SELECT COALESCE(json_agg(index_relation.relname ORDER BY index_relation.relname), '[]'::json)
  FROM pg_catalog.pg_index index_entry
  JOIN pg_catalog.pg_class index_relation ON index_relation.oid = index_entry.indexrelid
  WHERE index_entry.indrelid = ${quoteLiteral(`${indexSpec.schema}.${indexSpec.table}`)}::regclass;`, database);
  const inventory = [];

  // fingerprint every actual index without an open allowlist
  for (const name of names) {
    requireCondition(SAFE_IDENTIFIER_PATTERN.test(name), "table index inventory contains an unsafe name");
    const value = await queryPsqlJson(config, catalogFingerprintSql({ ...indexSpec, name }, database), database);
    requireCondition(value !== null, `table index disappeared during inventory capture: ${name}`);
    inventory.push(normalizeCatalogFingerprint(value));
  }

  return inventory;
}

// validate the declared and extra replacement inventory
export function validateReplacementInventory(inventory, manifest, expectedOwner, state = "source", sourceInventory = undefined) {
  requireCondition(Array.isArray(inventory), "replacement index inventory is missing");
  const replacement = manifest.replacement;
  const byName = new Map();

  // index every unique catalog tuple
  for (const catalog of inventory) {
    requireCondition(typeof catalog?.index === "string" && !byName.has(catalog.index), "replacement index inventory is duplicated");
    byName.set(catalog.index, catalog);
  }

  const target = byName.get(replacement.target.name) ?? null;
  const predecessor = byName.get(replacement.predecessor.name) ?? null;
  const covering = validateCatalogFingerprint(byName.get(replacement.covering.name), replacement.covering, expectedOwner);
  const protectedIndexes = replacement.protected.map((index) => validateCatalogFingerprint(byName.get(index.name), index, expectedOwner));

  // validate the selected catalog state
  if (state === "source") {
    requireCondition(target === null, "preexisting target forbids replacement");
    validateCatalogFingerprint(predecessor, replacement.predecessor, expectedOwner);
  } else if (state === "intermediate") {
    validateCatalogFingerprint(target, replacement.target, expectedOwner);
    validateCatalogFingerprint(predecessor, replacement.predecessor, expectedOwner);
  } else if (state === "final") {
    validateCatalogFingerprint(target, replacement.target, expectedOwner);
    requireCondition(predecessor === null, "replacement predecessor still exists");
  } else if (state === "restored") {
    requireCondition(target === null, "reverted target still exists");
    validateCatalogFingerprint(predecessor, replacement.predecessor, expectedOwner);
  } else {
    throw new ContractError(`unknown replacement inventory state: ${state}`);
  }

  const managedNames = new Set([
    replacement.target.name,
    replacement.predecessor.name,
    replacement.covering.name,
    ...replacement.protected.map((index) => index.name),
  ]);
  const extras = inventory.filter((catalog) => !managedNames.has(catalog.index));

  // bind all unchanged covering, protected, and extra OIDs to the source snapshot
  if (sourceInventory !== undefined) {
    const sourceByName = new Map(sourceInventory.map((catalog) => [catalog.index, catalog]));
    const unchangedNames = [replacement.covering.name, ...replacement.protected.map((index) => index.name), ...extras.map((catalog) => catalog.index)].sort();
    const sourceExtraNames = sourceInventory.filter((catalog) => !managedNames.has(catalog.index)).map((catalog) => catalog.index).sort();
    requireCondition(stableJson(extras.map((catalog) => catalog.index).sort()) === stableJson(sourceExtraNames), "replacement extra index inventory changed");

    // compare every preserved full tuple including its original OID
    for (const name of unchangedNames) {
      requireCondition(stableJson(byName.get(name)) === stableJson(sourceByName.get(name)), `replacement preserved index changed: ${name}`);
    }
  }

  return { target, predecessor, covering, protected: protectedIndexes, extras };
}

// compare a fingerprint to the static manifest
export function validateCatalogFingerprint(value, indexSpec, expectedTableOwner = undefined, options = {}) {
  const actual = normalizeCatalogFingerprint(value);
  const expectedAttributes = [...indexSpec.keys, ...indexSpec.include];
  requireCondition(actual !== null, "index is absent");
  requireCondition(actual.schema === indexSpec.schema && actual.indexSchema === indexSpec.schema, "index namespace mismatch");
  requireCondition(actual.table === indexSpec.table && actual.index === indexSpec.name, "index relation mismatch");
  requireCondition(expectedTableOwner === undefined || actual.tableOwner === expectedTableOwner, "table owner mismatch");
  requireCondition(actual.indexOwner === actual.tableOwner, "index owner must equal table owner");
  requireCondition(actual.accessMethod === indexSpec.method, "index access method mismatch");
  requireCondition(actual.keyCount === indexSpec.keys.length && actual.attributeCount === expectedAttributes.length, "index key or include count mismatch");
  requireCondition(Array.isArray(actual.columns) && actual.columns.length === expectedAttributes.length, "index column count mismatch");
  requireCondition(Array.isArray(actual.opclasses) && actual.opclasses.length === indexSpec.keys.length, "index opclass count mismatch");
  requireCondition(Array.isArray(actual.collations) && actual.collations.length === expectedAttributes.length, "index collation count mismatch");
  requireCondition(Array.isArray(actual.ordering) && actual.ordering.length === expectedAttributes.length, "index ordering count mismatch");

  // compare each ordered key property
  for (let position = 0; position < indexSpec.keys.length; position += 1) {
    const expected = indexSpec.keys[position];
    const column = actual.columns[position];
    const opclass = actual.opclasses[position];
    const collation = actual.collations[position];
    const ordering = actual.ordering[position];
    requireCondition(column.position === position + 1 && column.name === expected.name && column.type === expected.type, `index column ${position + 1} mismatch`);
    requireCondition(opclass.position === position + 1 && opclass.schema === expected.opclassSchema && opclass.name === expected.opclass && opclass.default === true, `index opclass ${position + 1} mismatch`);
    requireCondition(expected.collation === null ? collation === null : collation?.schema === expected.collation.schema && collation?.name === expected.collation.name, `index collation ${position + 1} mismatch`);
    requireCondition(ordering.position === position + 1 && ordering.direction === expected.direction && ordering.nulls === expected.nulls, `index ordering ${position + 1} mismatch`);
  }

  // compare every ordered include property
  for (let position = 0; position < indexSpec.include.length; position += 1) {
    const expected = indexSpec.include[position];
    const column = actual.columns[indexSpec.keys.length + position];
    requireCondition(column.position === indexSpec.keys.length + position + 1 && column.name === expected.name && column.type === expected.type, `index include ${position + 1} mismatch`);
  }

  requireCondition(actual.expressions === indexSpec.expressions && actual.predicate === indexSpec.predicate, "index expression or predicate mismatch");
  requireCondition(actual.unique === indexSpec.unique && actual.primary === indexSpec.primary, "index uniqueness flags mismatch");
  requireCondition(actual.exclusion === indexSpec.exclusion && actual.nullsNotDistinct === indexSpec.nullsNotDistinct, "index exclusion flags mismatch");
  requireCondition(actual.immediate === indexSpec.immediate && actual.clustered === indexSpec.clustered && actual.checkXmin === indexSpec.checkXmin, "index lifecycle flags mismatch");
  requireCondition(actual.replicaIdentity === indexSpec.replicaIdentity, "index replica identity mismatch");
  requireCondition(actual.storedTablespace === indexSpec.tablespace, "index stored tablespace mismatch");
  requireCondition(actual.effectiveTablespace === indexSpec.effectiveTablespace, "index effective tablespace mismatch");
  requireCondition(stableJson(actual.reloptions) === stableJson(indexSpec.reloptions), "index reloptions mismatch");
  requireCondition(normalizeIndexDefinition(actual.canonicalDefinition) === normalizeIndexDefinition(indexSpec.canonicalDefinition), "canonical index definition mismatch");
  requireCondition(typeof actual.valid === "boolean" && typeof actual.ready === "boolean" && typeof actual.live === "boolean", "index validity flags are missing");

  // require a fully usable index by default
  if (options.requireValid !== false) {
    requireCondition(actual.valid === true && actual.ready === true && actual.live === true, "index validity flags are incomplete");
  }
  requireCondition(Array.isArray(actual.constraintBacking) && Array.isArray(actual.dependencies), "index constraint or dependencies are missing");

  // validate one exact constraint-backed index
  if (indexSpec.constraint !== null) {
    requireCondition(actual.constraintBacking.length === 1, "index constraint backing changed");
    const constraint = actual.constraintBacking[0];
    requireCondition(constraint.schema === indexSpec.schema && constraint.name === indexSpec.constraint.name
      && constraint.type === indexSpec.constraint.type && constraint.validated === indexSpec.constraint.validated, "index constraint backing changed");
    requireCondition(actual.dependencies.length === 1 && actual.dependencies[0].class === "pg_class"
      && actual.dependencies[0].objectId === actual.indexOid && actual.dependencies[0].objectSubId === 0
      && actual.dependencies[0].referencedClass === "pg_constraint"
      && actual.dependencies[0].referencedObjectId === constraint.oid
      && actual.dependencies[0].referencedObjectSubId === 0 && actual.dependencies[0].type === "i", "constraint index dependency changed");
  } else {
    requireCondition(actual.constraintBacking.length === 0, "plain index backs a constraint");
    requireCondition(actual.dependencies.length === indexSpec.dependencyColumns.length, "index dependencies are missing or foreign");
    const attributeNumbers = new Map(actual.columns.map((column) => [column.name, column.attributeNumber]));

    // compare every dependency column with duplicate predicate references retained
    for (const name of new Set(indexSpec.dependencyColumns)) {
      const attributeNumber = attributeNumbers.get(name);
      const expectedCount = indexSpec.dependencyColumns.filter((column) => column === name).length;
      const actualCount = actual.dependencies.filter((dependency) => dependency.class === "pg_class"
        && dependency.objectId === actual.indexOid && dependency.objectSubId === 0
        && dependency.referencedClass === "pg_class" && dependency.referencedObjectId === actual.tableOid
        && dependency.referencedObjectSubId === attributeNumber && dependency.type === "a").length;
      requireCondition(attributeNumber !== undefined && actualCount === expectedCount, `index dependency changed: ${name}`);
    }
  }

  return actual;
}

// calculate deterministic admission thresholds
export function calculateAdmission(document, constants) {
  requireCondition(document?.schema === "weather-page-index-admission/v1", "unsupported admission schema");
  requireCondition(stableJson(Object.keys(document.identities ?? {}).sort()) === stableJson(ADMISSION_IDENTITY_KEYS), "admission identity set is incomplete or open");

  // require every closed identity digest
  for (const name of ADMISSION_IDENTITY_KEYS) {
    requireCondition(HASH_PATTERN.test(document.identities[name]), `admission identity is invalid: ${name}`);
  }

  requireCondition(Array.isArray(document.filesystems) && document.filesystems.length > 0, "admission filesystems are missing");
  const liveRows = toBigInt(document.scale?.liveRows, "live row count");
  const rehearsalRows = toBigInt(document.scale?.rehearsalRows, "rehearsal row count");
  const liveHeapBytes = toBigInt(document.scale?.liveHeapBytes, "live heap bytes");
  const rehearsalHeapBytes = toBigInt(document.scale?.rehearsalHeapBytes, "rehearsal heap bytes");
  requireCondition(HASH_PATTERN.test(document.scale?.columnDistributionDigest), "column distribution digest is missing");
  requireCondition(HASH_PATTERN.test(document.scale?.buildConfigDigest), "build configuration digest is missing");
  requireCondition(rehearsalRows > 0n && rehearsalHeapBytes > 0n, "rehearsal scale denominators must be positive");
  // select the worst scale ratio
  const scale = [
    1n,
    ceilDivide(liveRows, rehearsalRows),
    ceilDivide(liveHeapBytes, rehearsalHeapBytes),
  ].reduce((maximum, candidate) => candidate > maximum ? candidate : maximum, 1n);
  const duration = toBigInt(document.buildDurationSeconds ?? 5400, "build duration");
  requireCondition(duration === 5400n, "replacement admission must reserve two 45-minute windows");
  const minimumByteFloor = toBigInt(constants.minimumByteFloor, "minimum byte floor");
  const dockerByteFloor = toBigInt(constants.dockerByteFloor, "docker byte floor");
  const fixedByteReservation = toBigInt(constants.fixedByteReservation, "fixed byte reservation");
  const fixedBuildByteReservation = toBigInt(constants.fixedBuildByteReservation, "fixed build byte reservation");
  const minimumInodeFloor = toBigInt(constants.minimumInodeFloor, "minimum inode floor");
  const fixedBuildInodeReservation = toBigInt(constants.fixedBuildInodeReservation, "fixed build inode reservation");
  const filesystems = [];
  const identities = new Set();

  // calculate each unique filesystem once
  for (const filesystem of document.filesystems) {
    requireCondition(typeof filesystem.id === "string" && filesystem.id.length > 0, "filesystem identity is missing");
    requireCondition(!identities.has(filesystem.id), `duplicate filesystem identity: ${filesystem.id}`);
    identities.add(filesystem.id);
    requireCondition(Array.isArray(filesystem.paths) && filesystem.paths.length > 0, `filesystem paths are missing: ${filesystem.id}`);
    const totalInodes = toBigInt(filesystem.totalInodes, `${filesystem.id} total inodes`);
    const indexBytes = toBigInt(filesystem.rehearsal?.indexBytes, `${filesystem.id} index peak`);
    const tempBytes = toBigInt(filesystem.rehearsal?.tempBytes, `${filesystem.id} temp peak`);
    const walBytes = toBigInt(filesystem.rehearsal?.walBytes, `${filesystem.id} WAL peak`);
    const peakInodes = toBigInt(filesystem.rehearsal?.allocatedInodes, `${filesystem.id} inode peak`);
    requireCondition(indexBytes > 0n && walBytes > 0n && peakInodes > 0n, `filesystem rehearsal peaks must be measured and nonzero: ${filesystem.id}`);
    const growthBytes = toBigInt(filesystem.growth?.bytesPerSecond, `${filesystem.id} byte growth`);
    const growthInodes = toBigInt(filesystem.growth?.inodesPerSecond, `${filesystem.id} inode growth`);
    const dockerFloor = filesystem.dockerShares === true ? dockerByteFloor : 0n;
    const protectedBytes = (minimumByteFloor > dockerFloor ? minimumByteFloor : dockerFloor) + fixedByteReservation;
    const fractionalInodes = ceilDivide(totalInodes, 10n);
    const protectedInodes = fractionalInodes > minimumInodeFloor ? fractionalInodes : minimumInodeFloor;
    const reserveBytes = scale * (indexBytes + tempBytes + walBytes)
      + growthBytes * duration
      + fixedBuildByteReservation;
    const reserveInodes = scale * peakInodes
      + growthInodes * duration
      + fixedBuildInodeReservation;
    filesystems.push({
      id: filesystem.id,
      protectedBytes: protectedBytes.toString(),
      protectedInodes: protectedInodes.toString(),
      reserveBytes: reserveBytes.toString(),
      reserveInodes: reserveInodes.toString(),
      requiredFreeBytes: (protectedBytes + reserveBytes).toString(),
      requiredFreeInodes: (protectedInodes + reserveInodes).toString(),
    });
  }

  const observedLoop = Number(document.host?.baselineP95LoopSeconds);
  requireCondition(Number.isFinite(observedLoop) && observedLoop >= 0, "baseline loop interval is invalid");
  const heartbeatAllowanceSeconds = Math.max(
    Number(constants.minimumHeartbeatAllowanceSeconds),
    2 * observedLoop,
  );

  return {
    scale: scale.toString(),
    buildDurationSeconds: Number(duration),
    heartbeatAllowanceSeconds,
    filesystems,
  };
}

// validate admission freshness and capacity
export function evaluateAdmission(document, constants, current, now = new Date(), options = {}) {
  const calculated = calculateAdmission(document, constants);
  const reasons = [];
  const rehearsalAge = (now.getTime() - Date.parse(document.timestamps?.rehearsalMeasuredAt)) / 1000;
  const ingestionAge = (now.getTime() - Date.parse(document.timestamps?.ingestionMeasuredAt)) / 1000;
  const liveAge = (now.getTime() - Date.parse(document.timestamps?.liveSampledAt)) / 1000;

  // reject malformed timestamps
  if (![rehearsalAge, ingestionAge, liveAge].every(Number.isFinite)) {
    reasons.push("admission timestamps are invalid");
  } else {
    // reject future evidence
    if (rehearsalAge < 0 || ingestionAge < 0 || liveAge < 0) {
      reasons.push("admission evidence is from the future");
    }

    // enforce rehearsal freshness
    if (rehearsalAge > Number(constants.rehearsalMaximumAgeSeconds)) {
      reasons.push("rehearsal evidence expired");
    }

    // enforce ingestion freshness
    if (ingestionAge > Number(constants.ingestionMaximumAgeSeconds)) {
      reasons.push("ingestion baseline expired");
    }

    // enforce live freshness
    if (liveAge > Number(constants.liveSampleMaximumAgeSeconds)) {
      reasons.push("live filesystem sample expired");
    }
  }

  // compare frozen threshold calculations
  if (stableJson(calculated) !== stableJson(document.computed)) {
    reasons.push("frozen admission calculations do not match formulas");
  }

  const expectedIdentities = document.identities ?? {};
  const currentIdentities = current?.identities ?? {};

  // compare all bound identities
  for (const [name, expected] of Object.entries(expectedIdentities)) {
    // reject missing or changed identity
    if (currentIdentities[name] !== expected) {
      reasons.push(`identity changed: ${name}`);
    }
  }

  const admittedRows = toBigInt(document.scale.liveRows, "admitted live rows");
  const admittedHeap = toBigInt(document.scale.liveHeapBytes, "admitted live heap");
  const currentRows = toBigInt(current?.scale?.liveRows, "current live rows");
  const currentHeap = toBigInt(current?.scale?.liveHeapBytes, "current live heap");

  // reject row growth above one percent
  if (currentRows * 100n > admittedRows * 101n) {
    reasons.push("live row count grew more than one percent");
  }

  // reject heap growth above one percent
  if (currentHeap * 100n > admittedHeap * 101n) {
    reasons.push("live heap grew more than one percent");
  }

  const currentFilesystems = new Map();

  // index current filesystem samples
  for (const filesystem of current?.filesystems ?? []) {
    currentFilesystems.set(filesystem.id, filesystem);
  }

  // compare every filesystem threshold
  for (const threshold of calculated.filesystems) {
    const filesystem = currentFilesystems.get(threshold.id);

    // reject unresolved mappings
    if (filesystem === undefined) {
      reasons.push(`filesystem mapping is missing: ${threshold.id}`);
      continue;
    }

    // reject insufficient bytes
    if (toBigInt(filesystem.freeBytes, `${threshold.id} free bytes`) < toBigInt(threshold.requiredFreeBytes, `${threshold.id} required bytes`)) {
      reasons.push(`filesystem byte admission failed: ${threshold.id}`);
    }

    // reject insufficient inodes
    if (toBigInt(filesystem.freeInodes, `${threshold.id} free inodes`) < toBigInt(threshold.requiredFreeInodes, `${threshold.id} required inodes`)) {
      reasons.push(`filesystem inode admission failed: ${threshold.id}`);
    }
  }

  const host = current?.host ?? {};

  // enforce the architecture
  const expectedArchitecture = options.expectedArchitecture ?? "arm64";

  // enforce the operation architecture
  if (host.architecture !== expectedArchitecture) {
    reasons.push(`host architecture must be ${expectedArchitecture}`);
  }

  // enforce cpu capacity
  if (!Number.isInteger(host.cpuCount) || host.cpuCount < Number(constants.minimumCpuCount)) {
    reasons.push("host CPU count is below the minimum");
  }

  // enforce load capacity
  if (!Number.isFinite(host.load15) || host.load15 > Number(constants.maximumLoadPerCpu) * host.cpuCount) {
    reasons.push("host load is above the maximum");
  }

  // enforce available memory
  if (toBigInt(host.availableMemoryBytes, "available memory") < toBigInt(constants.minimumAvailableMemoryBytes, "minimum memory")) {
    reasons.push("available memory is below the minimum");
  }

  // enforce swap traffic
  if (toBigInt(host.swapBytesPerMinute, "swap bytes per minute") > toBigInt(constants.maximumSwapBytesPerMinute, "maximum swap")) {
    reasons.push("swap traffic is above the maximum");
  }

  // enforce fixture write cost
  if (!Number.isFinite(host.fixtureWriteP95Ratio) || host.fixtureWriteP95Ratio > Number(constants.maximumFixtureWriteP95Ratio)) {
    reasons.push("fixture write p95 exceeds the maximum ratio");
  }

  return {
    admitted: reasons.length === 0,
    calculated,
    reasons,
  };
}

// validate one frozen admission hash
export function validateAdmissionDigest(document) {
  requireCondition(typeof document.digest === "string" && HASH_PATTERN.test(document.digest), "admission digest is missing");
  const copy = structuredClone(document);
  delete copy.digest;
  requireCondition(sha256(stableJson(copy)) === document.digest, "admission digest mismatch");
  return document;
}

// produce one admission digest
export function sealAdmission(document) {
  const copy = structuredClone(document);
  delete copy.digest;
  return { ...copy, digest: sha256(stableJson(copy)) };
}

// bind one admission to its fresh full-window recheck
export function refreshAdmissionLiveSample(admission, current) {
  requireCondition(Number.isFinite(Date.parse(current?.capturedAt)), "fresh admission sample timestamp is invalid");
  const refreshed = structuredClone(admission);
  refreshed.timestamps.liveSampledAt = current.capturedAt;
  return sealAdmission(refreshed);
}

// validate one relative package path
export function validateRelativePath(path, label = "path") {
  requireCondition(typeof path === "string" && path.length > 0, `${label} is missing`);
  requireCondition(!isAbsolute(path) && !path.includes("\\") && !path.includes("\0"), `${label} is not a canonical relative path`);
  const parts = path.split("/");
  // reject every unsafe path segment
  requireCondition(parts.every((part) => part.length > 0 && part !== "." && part !== ".."), `${label} contains an unsafe segment`);
  return path;
}

// validate a release package manifest
export function validateReleaseManifest(release, indexManifest) {
  requireCondition(release?.schema === "weather-page-index-release/v1", "unsupported release manifest schema");
  requireCondition(RELEASE_PATTERN.test(release.tag), "release tag is not immutable");
  requireCondition(GIT_HASH_PATTERN.test(release.commit), "release commit must be a Git SHA-1");
  requireCondition(GIT_HASH_PATTERN.test(release.tree), "release tree must be a Git SHA-1");
  requireCondition(typeof release.repository === "string" && release.repository.length > 0, "release repository is missing");
  requireCondition(Array.isArray(release.files) && release.files.length === 2, "release manifest must bind two payload files");
  const expectedPaths = ["weather-page-indexes.json", "weather-page-indexes.mjs"];
  const seenPaths = [];

  // validate each payload entry
  for (const entry of release.files) {
    validateRelativePath(entry.path, "release file path");
    requireCondition(entry.path === basename(entry.path), "release payload must be root-level");
    requireCondition(HASH_PATTERN.test(entry.digest), `release file digest is invalid: ${entry.path}`);
    requireCondition(Number.isSafeInteger(entry.size) && entry.size >= 0 && entry.size <= 256 * 1024, `release file size is invalid: ${entry.path}`);
    requireCondition(entry.mode === "100644" || entry.mode === "100755", `release source mode is invalid: ${entry.path}`);
    seenPaths.push(entry.path);
  }

  seenPaths.sort();
  requireCondition(stableJson(seenPaths) === stableJson(expectedPaths), "release payload paths changed");
  validateReleaseRestoreContext(release.restoreContext, indexManifest.restoreContext);
  return release;
}

// validate a complete packaged restore tree
export function validateReleaseRestoreContext(context, expected) {
  requireCondition(context?.schema === expected.schema, "release restore context schema mismatch");
  requireCondition(context.tag === expected.tag && context.commit === expected.commit && context.tree === expected.tree, "release restore identity mismatch");
  requireCondition(context.fileCount === expected.fileCount && context.decodedBytes === expected.decodedBytes, "release restore dimensions mismatch");
  requireCondition(Array.isArray(context.files) && context.files.length === expected.fileCount, "release restore file list is incomplete");
  requireCondition(HASH_PATTERN.test(context.filesDigest), "release restore file-list digest is invalid");
  requireCondition(context.archive?.name === `restore-context-${expected.commit.slice(0, 12)}.tar.gz`, "restore archive name mismatch");
  requireCondition(HASH_PATTERN.test(context.archive?.digest), "restore archive digest is invalid");
  requireCondition(Number.isSafeInteger(context.archive?.size) && context.archive.size > 0, "restore archive size is invalid");
  requireCondition(context.archive.size <= expected.maximumDecodedBytes, "restore archive exceeds its bound");
  const files = [];
  const paths = new Set();
  let decodedBytes = 0;
  let previousPath = "";

  // validate every bound Git object
  for (const entry of context.files) {
    validateRelativePath(entry.path, "restore file path");
    requireCondition(entry.path > previousPath, "restore file list must be sorted and unique");
    requireCondition(entry.mode === "100644" || entry.mode === "100755", `restore file mode is invalid: ${entry.path}`);
    requireCondition(GIT_HASH_PATTERN.test(entry.blob), `restore Git blob is invalid: ${entry.path}`);
    requireCondition(HASH_PATTERN.test(entry.digest), `restore file digest is invalid: ${entry.path}`);
    requireCondition(Number.isSafeInteger(entry.size) && entry.size >= 0, `restore file size is invalid: ${entry.path}`);
    requireCondition(!paths.has(entry.path), `duplicate restore file: ${entry.path}`);
    paths.add(entry.path);
    files.push(entry);
    decodedBytes += entry.size;
    previousPath = entry.path;
  }

  requireCondition(decodedBytes === expected.decodedBytes, "restore decoded byte total mismatch");
  requireCondition(sha256(stableJson(files)) === context.filesDigest, "restore file-list digest mismatch");

  // require each critical dependency
  for (const criticalPath of expected.criticalPaths) {
    requireCondition(paths.has(criticalPath), `restore critical file is missing: ${criticalPath}`);
  }

  // require exact migration membership
  for (const migration of expected.migrations) {
    requireCondition(paths.has(migration), `restore migration is missing: ${migration}`);
  }

  const migrationPaths = [];

  // collect all packaged migrations
  for (const path of paths) {
    // select migration sql files
    if (path.startsWith("packages/database/migrations/") && path.endsWith(".sql")) {
      migrationPaths.push(path);
    }
  }

  migrationPaths.sort();
  requireCondition(stableJson(migrationPaths) === stableJson(expected.migrations), "restore migration set changed");
  requireCondition(context.critical !== null && typeof context.critical === "object", "restore critical descriptor is missing");
  const criticalNames = ["restore", "common", "compose", "acl"];

  // verify each repeated critical dependency digest
  for (let position = 0; position < criticalNames.length; position += 1) {
    const name = criticalNames[position];
    const entry = context.critical[name];
    const expectedPath = expected.criticalPaths[position];
    requireCondition(entry?.path === expectedPath, `restore critical ${name} path mismatch`);
    const sourceEntry = files.find((candidate) => candidate.path === expectedPath);
    requireCondition(sourceEntry !== undefined && entry.digest === sourceEntry.digest, `restore critical ${name} digest mismatch`);
  }

  requireCondition(Array.isArray(context.critical.migrations) && context.critical.migrations.length === expected.migrations.length, "restore critical migration descriptor is incomplete");

  // verify each repeated migration digest
  for (let position = 0; position < expected.migrations.length; position += 1) {
    const entry = context.critical.migrations[position];
    const expectedPath = expected.migrations[position];
    const sourceEntry = files.find((candidate) => candidate.path === expectedPath);
    requireCondition(entry?.path === expectedPath && sourceEntry !== undefined && entry.digest === sourceEntry.digest, `restore critical migration ${position + 1} mismatch`);
  }

  return context;
}

// validate the installed package bytes
export async function validateInstalledPackage(packageRoot = SCRIPT_ROOT) {
  const canonicalRoot = await realpath(packageRoot);
  requireCondition(canonicalRoot === resolve(packageRoot), "package root must be canonical");
  const rootInfo = await lstat(canonicalRoot);
  requireCondition(rootInfo.isDirectory() && !rootInfo.isSymbolicLink(), "package root must be a real directory");
  requireCondition(rootInfo.uid === 0 && (rootInfo.mode & 0o777) === 0o700, "package root must be root-owned mode 0700");
  const entries = await readdir(canonicalRoot, { withFileTypes: true });
  const names = [];

  // validate each installed member
  for (const entry of entries) {
    requireCondition(entry.isFile() && !entry.isSymbolicLink(), `package member must be a regular file: ${entry.name}`);
    names.push(entry.name);
  }

  names.sort();
  requireCondition(stableJson(names) === stableJson(PACKAGE_FILES), "installed package members changed");
  const { bytes: indexBytes, value: indexManifest } = await readJson(join(canonicalRoot, "weather-page-indexes.json"));
  const { value: releaseManifest } = await readJson(join(canonicalRoot, "release-manifest.json"));
  validateIndexManifest(indexManifest);
  validateReleaseManifest(releaseManifest, indexManifest);

  let installedBytes = 0;

  // verify all installed modes and payload hashes
  for (const name of PACKAGE_FILES) {
    const path = join(canonicalRoot, name);
    const info = await lstat(path);
    requireCondition(info.isFile() && !info.isSymbolicLink() && info.nlink === 1, `installed member is unsafe: ${name}`);
    requireCondition(info.uid === 0 && (info.mode & 0o777) === 0o600, `installed member must be root-owned mode 0600: ${name}`);
    requireCondition(info.size <= 256 * 1024, `installed member exceeds 256 KiB: ${name}`);
    installedBytes += info.size;
  }

  requireCondition(installedBytes <= 768 * 1024, "installed package exceeds 768 KiB");

  // verify the two self-independent payloads
  for (const entry of releaseManifest.files) {
    const path = join(canonicalRoot, entry.path);
    const info = await stat(path);
    requireCondition(info.size === entry.size, `installed file size mismatch: ${entry.path}`);
    requireCondition(await hashFile(path) === entry.digest, `installed file digest mismatch: ${entry.path}`);
  }

  return {
    indexBytes,
    indexManifest,
    releaseManifest,
    packageDigest: sha256(await readFile(join(canonicalRoot, "release-manifest.json"))),
  };
}

// validate a materialized restore context
export async function validateRestoreContextRoot(root, releaseContext, allowedRuntimeFiles = []) {
  const canonicalRoot = await realpath(root);
  requireCondition(canonicalRoot === resolve(root), "restore context root must be canonical");
  const rootInfo = await lstat(canonicalRoot);
  requireCondition(rootInfo.isDirectory() && !rootInfo.isSymbolicLink(), "restore context root must be a real directory");
  requireCondition(rootInfo.uid === 0 && (rootInfo.mode & 0o077) === 0, "restore context root must be private and root-owned");
  const expectedPaths = new Set();

  // validate declared synthetic secret exceptions
  for (const runtimeFile of allowedRuntimeFiles) {
    validateRelativePath(runtimeFile, "synthetic secret path");
    requireCondition(runtimeFile.startsWith("deploy/secrets/") && !runtimeFile.slice("deploy/secrets/".length).includes("/"), `synthetic secret path is outside deploy/secrets: ${runtimeFile}`);
  }

  // verify every bound restore byte
  for (const entry of releaseContext.files) {
    const path = join(canonicalRoot, entry.path);
    const canonicalPath = resolve(path);
    requireCondition(canonicalPath.startsWith(`${canonicalRoot}${sep}`), `restore file escaped its root: ${entry.path}`);
    const info = await lstat(path);
    requireCondition(info.isFile() && !info.isSymbolicLink() && info.nlink === 1, `restore file is unsafe: ${entry.path}`);
    requireCondition(info.size === entry.size, `restore file size mismatch: ${entry.path}`);
    requireCondition(await hashFile(path) === entry.digest, `restore file digest mismatch: ${entry.path}`);
    expectedPaths.add(entry.path);
  }

  const actualPaths = [];

  // walk one restore directory
  async function walk(directory, prefix = "") {
    const entries = await readdir(directory, { withFileTypes: true });

    // visit each declared tree entry
    for (const entry of entries) {
      const relativePath = prefix === "" ? entry.name : `${prefix}/${entry.name}`;
      const path = join(directory, entry.name);

      // descend into real directories
      if (entry.isDirectory() && !entry.isSymbolicLink()) {
        await walk(path, relativePath);
      } else {
        requireCondition(entry.isFile() && !entry.isSymbolicLink(), `restore context contains an unsafe entry: ${relativePath}`);
        const info = await lstat(path);

        // require private root-owned synthetic secrets
        if (allowedRuntimeFiles.includes(relativePath)) {
          requireCondition(info.uid === 0 && (info.mode & 0o077) === 0 && info.nlink === 1, `synthetic secret is unsafe: ${relativePath}`);
        }

        actualPaths.push(relativePath);
      }
    }
  }

  await walk(canonicalRoot);
  actualPaths.sort();
  const declaredPaths = [...expectedPaths, ...allowedRuntimeFiles].sort();
  requireCondition(stableJson(actualPaths) === stableJson(declaredPaths), "restore context contains missing or extra files");
  return canonicalRoot;
}

// validate disposable postgres mount isolation
export function validateDisposableMount(inspect, restoreConfig, descriptor) {
  requireCondition(inspect !== null && typeof inspect === "object", "postgres container inspection is missing");
  requireCondition(typeof restoreConfig.composeProject === "string" && restoreConfig.composeProject.startsWith("pageidx_restore_"), "restore project name is not isolated");
  requireCondition(!descriptor.forbiddenProjectNames.includes(restoreConfig.composeProject), "restore project is live");
  requireCondition(!descriptor.forbiddenDatabaseNames.includes(restoreConfig.fixtureDatabase), "restore database is live");
  requireCondition(typeof restoreConfig.weatherPostgresDir === "string" && isAbsolute(restoreConfig.weatherPostgresDir), "WEATHER_POSTGRES_DIR must be canonical and absolute");
  const mounts = Array.isArray(inspect.Mounts) ? inspect.Mounts : [];
  let dataMount;

  // locate the actual postgres data mount
  for (const mount of mounts) {
    // bind the canonical data destination
    if (mount.Destination === "/var/lib/postgresql/data") {
      requireCondition(dataMount === undefined, "postgres data mount is ambiguous");
      dataMount = mount;
    }
  }

  requireCondition(dataMount !== undefined && typeof dataMount.Source === "string", "postgres data mount is missing");
  const mountSource = resolve(dataMount.Source);
  const configuredSource = resolve(restoreConfig.weatherPostgresDir);
  requireCondition(mountSource === configuredSource, "postgres data mount does not match WEATHER_POSTGRES_DIR");

  // reject every production mount source
  for (const forbidden of descriptor.forbiddenDataMountSources) {
    requireCondition(mountSource !== resolve(forbidden), "postgres data mount is production");
  }

  requireCondition(inspect.Config?.Labels?.["com.docker.compose.project"] === restoreConfig.composeProject, "postgres compose project mismatch");
  requireCondition(inspect.Config?.Labels?.["com.docker.compose.service"] === "postgres", "postgres compose service mismatch");
  return {
    containerId: inspect.Id,
    image: inspect.Image,
    mountSource,
    mountType: dataMount.Type,
    startedAt: inspect.State?.StartedAt,
  };
}

// validate one separately measured restore coexistence gate
export function validateRestoreCoexistence(report, actual, now = new Date()) {
  requireCondition(report?.schema === "weather-page-restore-source-size/v1", "restore source-size report schema mismatch");
  requireCondition(report.archive?.digest === actual.archive.digest
    && report.archive?.size === actual.archive.size, "restore source-size report archive identity mismatch");
  requireCondition(report.database?.name === actual.database.name
    && report.database?.oid === actual.database.oid
    && report.database?.owner === actual.database.owner
    && report.database?.systemIdentifier === actual.database.systemIdentifier
    && report.database?.sizeBytes === actual.database.sizeBytes, "restore source-size report database identity mismatch");
  const ageSeconds = (now.getTime() - Date.parse(report.capturedAt)) / 1000;
  requireCondition(Number.isFinite(ageSeconds) && ageSeconds >= 0 && ageSeconds <= 86_400, "restore source-size report is stale or future-dated");
  const databaseBytes = toBigInt(report.database.sizeBytes, "restore source database bytes");
  requireCondition(databaseBytes > 0n, "restore source database size is missing");
  requireCondition(BigInt(report.archive.size) <= databaseBytes, "restore archive is larger than its measured source database");
  const requiredFreeBytes = 3n * databaseBytes + 5n * 1024n ** 3n;
  const freeBytes = toBigInt(actual.filesystem.freeBytes, "restore filesystem free bytes");
  requireCondition(freeBytes >= requiredFreeBytes, "restore coexistence capacity is insufficient", {
    databaseBytes: databaseBytes.toString(),
    freeBytes: freeBytes.toString(),
    requiredFreeBytes: requiredFreeBytes.toString(),
  });
  return {
    schema: "weather-page-restore-coexistence/v1",
    capturedAt: new Date().toISOString(),
    reportDigest: actual.reportDigest,
    archive: actual.archive,
    database: actual.database,
    filesystem: actual.filesystem,
    requiredFreeBytes: requiredFreeBytes.toString(),
  };
}

// capture and validate the actual restore coexistence capacity
export async function captureRestoreCoexistence(config, restoreInputs, dataRoot) {
  const restoreConfig = config.restore;
  requireCondition(typeof restoreConfig.sourceSizeReport === "string" && HASH_PATTERN.test(restoreConfig.sourceSizeDigest), "restore source-size evidence is missing");
  const file = await validatePrivateRestoreInput(restoreConfig.sourceSizeReport, "restore source-size report", 64 * 1024);
  requireCondition(file.digest === restoreConfig.sourceSizeDigest, "restore source-size report digest mismatch");
  const { value: report } = await readJson(file.path);
  const database = await queryPsqlJson(
    { ...config, postgres: { ...config.postgres, container: restoreConfig.postgresContainer, database: restoreConfig.fixtureDatabase } },
    `SELECT json_build_object(
      'name', current_database(),
      'oid', database_relation.oid::text,
      'owner', pg_get_userbyid(database_relation.datdba),
      'systemIdentifier', (SELECT system_identifier::text FROM pg_control_system()),
      'sizeBytes', pg_database_size(current_database())::text
    ) FROM pg_database database_relation WHERE database_relation.datname = current_database();`,
    restoreConfig.fixtureDatabase,
  );
  const filesystem = await statfs(dataRoot, { bigint: true });
  const dataInfo = await stat(dataRoot);
  return validateRestoreCoexistence(report, {
    reportDigest: file.digest,
    archive: { digest: restoreInputs.archive.digest, size: restoreInputs.archive.size },
    database,
    filesystem: {
      dev: dataInfo.dev.toString(),
      path: dataRoot,
      freeBytes: (filesystem.bavail * filesystem.bsize).toString(),
    },
  });
}

// validate one private restore input file
export async function validatePrivateRestoreInput(path, label, maximumBytes = undefined) {
  const canonicalPath = await realpath(path);
  requireCondition(canonicalPath === resolve(path), `${label} path must be canonical`);
  const info = await lstat(canonicalPath);
  requireCondition(info.isFile() && !info.isSymbolicLink() && info.nlink === 1, `${label} must be a regular single-link file`);
  requireCondition(info.uid === 0 && (info.mode & 0o777) === 0o600, `${label} must be root-owned mode 0600`);
  requireCondition(info.size > 0 && (maximumBytes === undefined || info.size <= maximumBytes), `${label} size is invalid`);
  return { path: canonicalPath, size: info.size, digest: await hashFile(canonicalPath), dev: info.dev.toString(), ino: info.ino.toString() };
}

// run one bounded child command
export async function runCommand(command, args, options = {}) {
  // supervise one child process
  return await new Promise((resolvePromise, rejectPromise) => {
    const child = spawn(command, args, {
      cwd: options.cwd,
      env: options.env,
      stdio: [options.input === undefined ? "ignore" : "pipe", "pipe", "pipe"],
    });
    const stdout = [];
    const stderr = [];
    let timedOut = false;
    // terminate one expired command
    const timer = options.timeoutMilliseconds === undefined ? undefined : setTimeout(() => {
      timedOut = true;
      child.kill("SIGTERM");
    }, options.timeoutMilliseconds);

    // collect standard output
    child.stdout.on("data", (chunk) => {
      stdout.push(chunk);
    });

    // collect standard error
    child.stderr.on("data", (chunk) => {
      stderr.push(chunk);
    });

    // report spawn failures
    child.on("error", (error) => {
      // clear the timeout
      if (timer !== undefined) {
        clearTimeout(timer);
      }

      rejectPromise(error);
    });

    // resolve command completion
    child.on("close", (code, signal) => {
      // clear the timeout
      if (timer !== undefined) {
        clearTimeout(timer);
      }

      const result = {
        code,
        signal,
        timedOut,
        stdout: Buffer.concat(stdout).toString("utf8"),
        stderr: Buffer.concat(stderr).toString("utf8"),
      };

      // reject failed commands by default
      if (code !== 0 && options.allowFailure !== true) {
        rejectPromise(new ContractError(`command failed: ${command}`, result));
      } else {
        resolvePromise(result);
      }
    });

    // provide bounded standard input
    if (options.input !== undefined) {
      child.stdin.end(options.input);
    }
  });
}

// create docker psql arguments
export function dockerPsqlArgs(config, database = config.postgres.database, extra = []) {
  requireCondition(typeof config.postgres?.container === "string" && config.postgres.container.length > 0, "postgres container is missing");
  requireCondition(SAFE_IDENTIFIER_PATTERN.test(database), "postgres database name is unsafe");
  requireCondition(SAFE_IDENTIFIER_PATTERN.test(config.postgres.user), "postgres user name is unsafe");
  return [
    "exec",
    "-i",
    "-e",
    `PGAPPNAME=${config.postgres.applicationName ?? "weather-page-index-v1"}`,
    config.postgres.container,
    "psql",
    "-X",
    "--no-psqlrc",
    "--set=ON_ERROR_STOP=1",
    "--tuples-only",
    "--no-align",
    "--username",
    config.postgres.user,
    "--dbname",
    database,
    ...extra,
  ];
}

// execute one psql query
export async function queryPsql(config, sql, database = config.postgres.database) {
  const result = await runCommand("docker", dockerPsqlArgs(config, database, ["--command", sql]), {
    timeoutMilliseconds: config.postgres.queryTimeoutMilliseconds ?? 30_000,
  });
  return result.stdout.trim();
}

// execute and decode one json query
export async function queryPsqlJson(config, sql, database = config.postgres.database) {
  const output = await queryPsql(config, sql, database);
  requireCondition(output.length > 0, "psql JSON query returned no output");

  try {
    return JSON.parse(output);
  } catch (error) {
    throw new ContractError("psql JSON query returned invalid JSON", { output, cause: error.message });
  }
}

// inspect one docker container
export async function inspectContainer(name) {
  const result = await runCommand("docker", ["inspect", name], { timeoutMilliseconds: 15_000 });
  let decoded;

  try {
    decoded = JSON.parse(result.stdout);
  } catch (error) {
    throw new ContractError(`docker inspection was not JSON: ${name}`, { cause: error.message });
  }

  requireCondition(Array.isArray(decoded) && decoded.length === 1, `docker inspection was ambiguous: ${name}`);
  return decoded[0];
}

// inspect one closed container set in one call
export async function inspectContainers(names) {
  requireCondition(Array.isArray(names) && names.length > 0 && names.every((name) => typeof name === "string" && name.length > 0), "container inspection set is invalid");
  requireCondition(new Set(names).size === names.length, "container inspection set contains duplicates");
  const result = await runCommand("docker", ["inspect", ...names], { timeoutMilliseconds: 15_000 });
  let decoded;

  try {
    decoded = JSON.parse(result.stdout);
  } catch (error) {
    throw new ContractError("docker inspect returned invalid JSON", { cause: error.message });
  }

  requireCondition(Array.isArray(decoded) && decoded.length === names.length, "docker inspect result set changed");
  const byName = new Map();

  // bind each requested name to one result
  for (const container of decoded) {
    const canonicalName = typeof container?.Name === "string" ? container.Name.replace(/^\//u, "") : undefined;
    requireCondition(canonicalName !== undefined && names.includes(canonicalName), "docker inspect returned an unexpected container");
    requireCondition(!byName.has(canonicalName), `docker inspect duplicated container: ${canonicalName}`);
    byName.set(canonicalName, container);
  }

  return byName;
}

// hash a declared file set
export async function hashFileSet(root, files) {
  requireCondition(Array.isArray(files) && files.length > 0, "snapshot file set is missing");
  const entries = [];

  // hash each declared file
  for (const file of [...files].sort()) {
    validateRelativePath(file, "snapshot file path");
    const path = join(root, file);
    const info = await lstat(path);
    requireCondition(info.isFile() && !info.isSymbolicLink(), `snapshot file is unsafe: ${file}`);
    entries.push({ path: file, size: info.size, digest: await hashFile(path) });
  }

  return { entries, digest: sha256(stableJson(entries)) };
}

// read one exact release marker
export async function readReleaseMarker(path) {
  const info = await lstat(path);
  requireCondition(info.isFile() && !info.isSymbolicLink(), `release marker is unsafe: ${path}`);
  const value = (await readFile(path, "utf8")).trim();
  requireCondition(RELEASE_PATTERN.test(value), `release marker is invalid: ${path}`);
  return value;
}

// parse one non-evaluated environment file
export function parseEnvironmentFile(contents) {
  const values = {};
  const lines = contents.split(/\r?\n/u);

  // parse each nonempty assignment
  for (const line of lines) {
    // ignore blank lines only
    if (line.length === 0) {
      continue;
    }

    const match = line.match(/^([A-Z][A-Z0-9_]*)=(.*)$/u);
    requireCondition(match !== null && match[2].length > 0, "release environment contains an invalid assignment");
    requireCondition(values[match[1]] === undefined, `release environment repeats ${match[1]}`);
    values[match[1]] = match[2];
  }

  return values;
}

// collect one complete regular file tree
async function collectRegularTree(root, directory, entries) {
  const children = await readdir(directory, { withFileTypes: true });

  // walk every sorted directory member
  for (const child of children.sort((left, right) => left.name < right.name ? -1 : left.name > right.name ? 1 : 0)) {
    const path = join(directory, child.name);

    // descend only real directories
    if (child.isDirectory() && !child.isSymbolicLink()) {
      await collectRegularTree(root, path, entries);
    } else {
      requireCondition(child.isFile() && !child.isSymbolicLink(), `control plane entry is unsafe: ${path}`);
      const info = await lstat(path);
      requireCondition(info.nlink === 1, `control plane entry has multiple links: ${path}`);
      entries.push({ path: relative(root, path).split(sep).join("/"), absolutePath: path, size: info.size });
    }
  }
}

// hash the complete installed control plane
export async function hashControlPlane(installRoot) {
  const canonicalRoot = await realpath(installRoot);
  requireCondition(canonicalRoot === resolve(installRoot), "installed source root must be canonical");
  const deployRoot = join(canonicalRoot, "deploy");
  const entries = [];

  // enumerate every fixed control subtree
  for (const relativeDirectory of ["scripts", "postgres", "systemd", "sudoers"]) {
    const directory = join(deployRoot, relativeDirectory);
    const info = await lstat(directory);
    requireCondition(info.isDirectory() && !info.isSymbolicLink(), `control directory is unsafe: ${relativeDirectory}`);
    await collectRegularTree(canonicalRoot, directory, entries);
  }

  entries.sort((left, right) => left.path < right.path ? -1 : left.path > right.path ? 1 : 0);

  const composePath = join(deployRoot, "compose.yaml");
  const composeInfo = await lstat(composePath);
  requireCondition(composeInfo.isFile() && !composeInfo.isSymbolicLink() && composeInfo.nlink === 1, "control compose file is unsafe");
  entries.push({ path: "deploy/compose.yaml", absolutePath: composePath, size: composeInfo.size });
  const digest = createHash("sha256");
  const outputEntries = [];

  // reproduce the installed control digest stream
  for (const entry of entries) {
    const fileDigest = await hashFile(entry.absolutePath);
    digest.update(entry.path);
    digest.update("\0");
    digest.update(fileDigest);
    digest.update("\n");
    outputEntries.push({ path: entry.path, size: entry.size, digest: fileDigest });
  }

  requireCondition(outputEntries.length > 5, "control plane inventory is unexpectedly small");
  return { entries: outputEntries, digest: digest.digest("hex") };
}

// capture the real committed release source
export async function captureDeploymentSource(installRoot) {
  const root = await realpath(installRoot);
  requireCondition(root === resolve(installRoot), "installed source root must be canonical");
  const deployRoot = join(root, "deploy");
  const stateRoot = join(deployRoot, "state");
  const releasesRoot = join(deployRoot, "releases");

  // reject aliased deployment roots
  for (const directory of [deployRoot, stateRoot, releasesRoot]) {
    const info = await lstat(directory);
    requireCondition(info.isDirectory() && !info.isSymbolicLink() && await realpath(directory) === resolve(directory), `deployment root is unsafe: ${directory}`);
  }

  const core = await readReleaseMarker(join(stateRoot, "current-release"));
  const schema = await readReleaseMarker(join(stateRoot, "schema-release"));
  let web = core;
  const currentWebPath = join(stateRoot, "current-web-release");

  try {
    web = await readReleaseMarker(currentWebPath);
  } catch (error) {
    // allow true absence without hiding unsafe entries
    if (error?.code !== "ENOENT") {
      throw error;
    }
  }

  const activeLink = await readlink(join(stateRoot, "active.env"));
  requireCondition(activeLink === `../releases/${core}.env`, "active core environment link changed");
  const activePath = join(releasesRoot, `${core}.env`);
  const activeInfo = await lstat(activePath);
  requireCondition(activeInfo.isFile() && !activeInfo.isSymbolicLink(), "active core environment is unsafe");
  const activeBytes = await readFile(activePath);
  const active = parseEnvironmentFile(activeBytes.toString("utf8"));
  requireCondition(active.WEATHER_RELEASE === core, "active core environment release changed");
  requireCondition(/^\d+$/u.test(active.WEATHER_CONTROL_PLANE_VERSION), "recorded control version is invalid");
  requireCondition(HASH_PATTERN.test(active.WEATHER_CONTROL_PLANE_SHA256), "recorded control digest is invalid");
  let webEnvironmentDigest = sha256(activeBytes);

  // bind the independent dashboard environment
  if (web !== core) {
    const activeWebLink = await readlink(join(stateRoot, "active-web.env"));
    requireCondition(activeWebLink === `../releases/${web}.web.env`, "active dashboard environment link changed");
    const webPath = join(releasesRoot, `${web}.web.env`);
    const webInfo = await lstat(webPath);
    requireCondition(webInfo.isFile() && !webInfo.isSymbolicLink(), "active dashboard environment is unsafe");
    const webBytes = await readFile(webPath);
    const webEnvironment = parseEnvironmentFile(webBytes.toString("utf8"));
    requireCondition(typeof webEnvironment.WEATHER_WEB_IMAGE === "string", "dashboard environment image is missing");
    webEnvironmentDigest = sha256(webBytes);
  } else {
    // reject an orphaned dashboard link
    try {
      await lstat(join(stateRoot, "active-web.env"));
      throw new ContractError("dashboard environment link exists without an override");
    } catch (error) {
      // allow true absence only
      if (error?.code !== "ENOENT") {
        throw error;
      }
    }
  }

  const stateEntries = await readdir(stateRoot);

  // refuse every incomplete release transaction marker
  for (const name of stateEntries) {
    requireCondition(name !== "dashboard-web-transaction.env"
      && name !== ".v14-compatibility-bridge-v1.pending"
      && !name.endsWith(".pending"), `incomplete release transaction exists: ${name}`);
  }

  const value = {
    core,
    schema,
    web,
    activeEnvironmentDigest: sha256(activeBytes),
    webEnvironmentDigest,
    recordedControlDigest: active.WEATHER_CONTROL_PLANE_SHA256,
    recordedControlVersion: active.WEATHER_CONTROL_PLANE_VERSION,
    configuredImages: {
      server: active.WEATHER_SERVER_IMAGE,
      web: active.WEATHER_WEB_IMAGE,
      postgres: active.POSTGRES_IMAGE,
      cloudflared: active.CLOUDFLARED_IMAGE,
    },
    database: active.WEATHER_DATABASE_NAME,
  };
  return { ...value, digest: sha256(stableJson(value)) };
}

// select every canary setting recursively
export function collectCanarySettings(value, prefix = "") {
  const result = {};

  // traverse arrays by position
  if (Array.isArray(value)) {
    for (let position = 0; position < value.length; position += 1) {
      Object.assign(result, collectCanarySettings(value[position], `${prefix}[${position}]`));
    }

    return result;
  }

  // traverse plain objects by key
  if (value !== null && typeof value === "object") {
    for (const [key, child] of Object.entries(value)) {
      const path = prefix === "" ? key : `${prefix}.${key}`;

      // retain canary and kill-switch leaves
      if (/canary|kill.?switch/iu.test(key) && (child === null || typeof child !== "object")) {
        result[path] = child;
      }

      Object.assign(result, collectCanarySettings(child, path));
    }
  }

  return result;
}

// capture the administrator forecast settings file
export async function captureForecastSettings(path) {
  const canonicalPath = await realpath(path);
  requireCondition(canonicalPath === resolve(path), "forecast settings path must be canonical");
  const info = await lstat(canonicalPath);
  requireCondition(info.isFile() && !info.isSymbolicLink() && info.nlink === 1, "forecast settings file is unsafe");
  requireCondition((info.mode & 0o777) === 0o600 && (info.uid === 0 || info.uid === 10002), "forecast settings ownership or mode is unsafe");
  requireCondition(info.size > 0 && info.size <= 4096, "forecast settings size is unsafe");
  const bytes = await readFile(canonicalPath);
  let document;

  try {
    document = JSON.parse(bytes.toString("utf8"));
  } catch (error) {
    throw new ContractError("forecast settings file is invalid JSON", { cause: error.message });
  }

  const markerPath = `${canonicalPath}.initialized`;
  const canonicalMarkerPath = await realpath(markerPath);
  requireCondition(canonicalMarkerPath === resolve(markerPath), "forecast settings marker path must be canonical");
  const markerInfo = await lstat(canonicalMarkerPath);
  requireCondition(markerInfo.isFile() && !markerInfo.isSymbolicLink() && markerInfo.nlink === 1, "forecast settings marker is unsafe");
  requireCondition((markerInfo.mode & 0o777) === 0o600 && (markerInfo.uid === 0 || markerInfo.uid === 10002), "forecast settings marker ownership or mode is unsafe");
  const markerBytes = await readFile(canonicalMarkerPath);
  requireCondition(markerBytes.toString("utf8") === "forecast-adjustment-settings/v1\n", "forecast settings marker content changed");
  const result = {
    path: canonicalPath,
    dev: info.dev.toString(),
    ino: info.ino.toString(),
    size: info.size,
    digest: sha256(bytes),
    marker: {
      path: canonicalMarkerPath,
      dev: markerInfo.dev.toString(),
      ino: markerInfo.ino.toString(),
      size: markerInfo.size,
      digest: sha256(markerBytes),
    },
    canaries: collectCanarySettings(document),
  };
  return { ...result, combinedDigest: sha256(stableJson({ settings: result.digest, marker: result.marker.digest })) };
}

// build the complete stable acl snapshot query
function aclSnapshotSql() {
  return `SELECT json_build_object(
    'roles', COALESCE((SELECT json_agg(json_build_object(
      'name', rolname,
      'superuser', rolsuper,
      'inherit', rolinherit,
      'createRole', rolcreaterole,
      'createDb', rolcreatedb,
      'canLogin', rolcanlogin,
      'replication', rolreplication,
      'bypassRls', rolbypassrls
    ) ORDER BY rolname) FROM pg_roles WHERE rolname LIKE 'weather_%'), '[]'::json),
    'databaseAcl', (SELECT datacl::text FROM pg_database WHERE datname = current_database()),
    'schemas', COALESCE((SELECT json_agg(json_build_object(
      'schema', namespace.nspname,
      'owner', pg_get_userbyid(namespace.nspowner),
      'acl', namespace.nspacl::text
    ) ORDER BY namespace.nspname) FROM pg_namespace namespace WHERE namespace.nspname = 'public'), '[]'::json),
    'relations', COALESCE((SELECT json_agg(json_build_object(
      'schema', namespace.nspname,
      'name', relation.relname,
      'kind', relation.relkind,
      'owner', pg_get_userbyid(relation.relowner),
      'acl', relation.relacl::text
    ) ORDER BY namespace.nspname, relation.relname)
    FROM pg_class relation JOIN pg_namespace namespace ON namespace.oid = relation.relnamespace
    WHERE namespace.nspname = 'public' AND relation.relkind NOT IN ('i', 'I')), '[]'::json),
    'columns', COALESCE((SELECT json_agg(json_build_object(
      'schema', namespace.nspname,
      'relation', relation.relname,
      'column', attribute.attname,
      'acl', attribute.attacl::text
    ) ORDER BY namespace.nspname, relation.relname, attribute.attnum)
    FROM pg_attribute attribute
    JOIN pg_class relation ON relation.oid = attribute.attrelid
    JOIN pg_namespace namespace ON namespace.oid = relation.relnamespace
    WHERE namespace.nspname = 'public' AND relation.relkind NOT IN ('i', 'I') AND attribute.attnum > 0 AND NOT attribute.attisdropped AND attribute.attacl IS NOT NULL), '[]'::json),
    'defaultAcls', COALESCE((SELECT json_agg(json_build_object(
      'owner', pg_get_userbyid(default_acl.defaclrole),
      'namespace', COALESCE(namespace.nspname, ''),
      'objectType', default_acl.defaclobjtype,
      'acl', default_acl.defaclacl::text
    ) ORDER BY default_acl.defaclrole, default_acl.defaclnamespace, default_acl.defaclobjtype)
    FROM pg_default_acl default_acl LEFT JOIN pg_namespace namespace ON namespace.oid = default_acl.defaclnamespace), '[]'::json),
    'roleSettings', COALESCE((SELECT json_agg(json_build_object(
      'role', pg_get_userbyid(role_setting.setrole),
      'database', CASE WHEN role_setting.setdatabase = 0 THEN '' ELSE (SELECT datname FROM pg_database WHERE oid = role_setting.setdatabase) END,
      'settings', role_setting.setconfig
    ) ORDER BY role_setting.setrole, role_setting.setdatabase)
    FROM pg_db_role_setting role_setting
    WHERE role_setting.setrole IN (SELECT oid FROM pg_roles WHERE rolname LIKE 'weather_%')), '[]'::json)
  );`;
}

// capture release and database identities
export async function captureRuntimeSnapshot(config, indexSpec, migrationPaths, options = {}) {
  requireCondition(Array.isArray(migrationPaths) && migrationPaths.length === 18, "snapshot migration contract is incomplete");
  const restoreContext = options.restoreContext;
  const source = restoreContext === undefined
    ? await captureDeploymentSource(config.paths.installRoot)
    : {
        repository: restoreContext.repository,
        tag: restoreContext.tag,
        commit: restoreContext.commit,
        tree: restoreContext.tree,
        filesDigest: restoreContext.filesDigest,
        digest: sha256(stableJson({
          repository: restoreContext.repository,
          tag: restoreContext.tag,
          commit: restoreContext.commit,
          tree: restoreContext.tree,
          filesDigest: restoreContext.filesDigest,
        })),
      };
  const control = restoreContext === undefined
    ? await hashControlPlane(config.paths.installRoot)
    : { digest: sha256(stableJson(restoreContext.critical)) };
  const forecastSettings = restoreContext === undefined
    ? await captureForecastSettings(config.paths.forecastSettings)
    : { combinedDigest: sha256(stableJson({ schema: "weather-page-index-restore-settings/v1", filesDigest: restoreContext.filesDigest })) };

  const containers = [];
  const containerNames = new Set(config.containers ?? []);
  containerNames.add(config.postgres.container);
  requireCondition(config.restoreOnly === true || containerNames.size >= 5, "live snapshot must bind all five runtime containers");
  const composeProject = config.postgres.composeProject;
  requireCondition(typeof composeProject === "string" && composeProject.length > 0, "postgres compose project is missing");
  const migrationContainers = await runCommand("docker", [
    "ps",
    "--quiet",
    "--filter",
    `label=com.docker.compose.project=${composeProject}`,
    "--filter",
    "label=com.docker.compose.service=migration",
  ], { timeoutMilliseconds: 15_000 });
  requireCondition(migrationContainers.stdout.trim() === "", "a migration transaction is in flight");

  // capture each live image and start identity
  for (const name of [...containerNames].sort()) {
    const inspect = await inspectContainer(name);
    containers.push({
      name,
      id: inspect.Id,
      image: inspect.Image,
      imageReference: inspect.Config?.Image,
      startedAt: inspect.State?.StartedAt,
      restartCount: inspect.RestartCount,
      oomKilled: inspect.State?.OOMKilled,
      status: inspect.State?.Status,
      project: inspect.Config?.Labels?.["com.docker.compose.project"],
      service: inspect.Config?.Labels?.["com.docker.compose.service"],
    });
  }

  // require the complete live service set
  if (config.restoreOnly !== true) {
    const services = containers.map((container) => container.service).sort();
    requireCondition(stableJson(services) === stableJson(["api", "cloudflared", "postgres", "web", "worker"]), "live container service inventory changed");
    requireCondition(containers.every((container) => container.project === composeProject), "live container compose project changed");
    requireCondition(containers.find((container) => container.service === "api")?.name === config.monitor.apiContainer, "monitor API container is not the live API service");
  }

  const databaseSql = `SELECT json_build_object(
    'database', current_database(),
    'databaseOid', database_relation.oid::text,
    'databaseOwner', pg_get_userbyid(database_relation.datdba),
    'systemIdentifier', (SELECT system_identifier::text FROM pg_control_system()),
    'timelineId', (SELECT timeline_id FROM pg_control_checkpoint()),
    'tableOid', table_relation.oid::text,
    'tableOwner', pg_get_userbyid(table_relation.relowner),
    'liveRows', ${restoreContext === undefined ? "GREATEST(table_relation.reltuples, 0)::bigint::text" : `(SELECT count(*)::text FROM ${quoteIdentifier(indexSpec.schema)}.${quoteIdentifier(indexSpec.table)})`},
    'liveHeapBytes', pg_relation_size(table_relation.oid)::text
  )
  FROM pg_database database_relation
  JOIN pg_class table_relation ON table_relation.oid = ${quoteLiteral(`${indexSpec.schema}.${indexSpec.table}`)}::regclass
  WHERE database_relation.datname = current_database();`;
  const database = await queryPsqlJson(config, databaseSql);
  const ledger = await queryPsqlJson(config, `SELECT COALESCE(json_agg(json_build_object('name', name, 'checksum', checksum) ORDER BY name), '[]'::json) FROM schema_migrations;`);
  const ledgerNames = ledger.map((entry) => entry.name);
  requireCondition(stableJson(ledgerNames) === stableJson(migrationPaths.map((path) => basename(path))), "database ledger is not the exact ordered 0018 contract");
  const settings = await queryPsqlJson(config, `SELECT COALESCE(json_object_agg(name, setting ORDER BY name), '{}'::json) FROM pg_settings WHERE name = ANY(ARRAY[${SETTING_NAMES.map(quoteLiteral).join(",")}]);`);
  const acl = await queryPsqlJson(config, aclSnapshotSql());
  const catalog = await queryPsqlJson(config, catalogFingerprintSql(indexSpec));
  const indexInventory = await captureIndexInventory(config, indexSpec);
  const snapshot = {
    capturedAt: new Date().toISOString(),
    source,
    control,
    releases: restoreContext === undefined
      ? {
          core: source.core,
          schema: source.schema,
          web: source.web,
          recordedControlDigest: source.recordedControlDigest,
          recordedControlVersion: source.recordedControlVersion,
          activeEnvironmentDigest: source.activeEnvironmentDigest,
          webEnvironmentDigest: source.webEnvironmentDigest,
        }
      : {
          schema: "weather-page-index-restore-source/v1",
          tag: restoreContext.tag,
          commit: restoreContext.commit,
          tree: restoreContext.tree,
          filesDigest: restoreContext.filesDigest,
        },
    forecastSettings,
    containers,
    database,
    ledger,
    settings,
    acl,
    catalog: normalizeCatalogFingerprint(catalog),
    indexInventory,
  };

  return {
    ...snapshot,
    identities: {
      sourceDigest: source.digest,
      controlDigest: control.digest,
      imageDigest: sha256(stableJson(containers)),
      ledgerDigest: sha256(stableJson(ledger)),
      settingsDigest: sha256(stableJson(settings)),
      forecastSettingsDigest: forecastSettings.combinedDigest,
      aclDigest: sha256(stableJson(acl)),
      databaseDigest: sha256(stableJson({
        database: database.database,
        databaseOid: database.databaseOid,
        databaseOwner: database.databaseOwner,
        systemIdentifier: database.systemIdentifier,
        timelineId: database.timelineId,
        tableOid: database.tableOid,
        tableOwner: database.tableOwner,
      })),
      catalogDigest: sha256(stableJson(indexInventory)),
    },
  };
}

// compare runtime identities to explicit expectations
export function assertExpectedSnapshot(snapshot, expected) {
  const required = [
    "sourceDigest",
    "controlDigest",
    "imageDigest",
    "ledgerDigest",
    "settingsDigest",
    "forecastSettingsDigest",
    "aclDigest",
    "databaseDigest",
  ];

  // require each frozen source identity
  for (const name of required) {
    requireCondition(typeof expected?.[name] === "string" && HASH_PATTERN.test(expected[name]), `expected ${name} is missing`);
    requireCondition(snapshot.identities[name] === expected[name], `runtime ${name} changed`);
  }

  requireCondition(expected?.releases !== null && typeof expected.releases === "object", "expected release markers are missing");
  requireCondition(stableJson(snapshot.releases) === stableJson(expected.releases), "runtime release markers changed");

  return snapshot;
}

// read available host memory
async function readAvailableMemory() {
  const contents = await readFile("/proc/meminfo", "utf8");
  const match = contents.match(/^MemAvailable:\s+(\d+)\s+kB$/mu);
  requireCondition(match !== null, "MemAvailable is unavailable");
  return BigInt(match[1]) * 1024n;
}

// read swap traffic counters
async function readSwapPages() {
  const contents = await readFile("/proc/vmstat", "utf8");
  const inputMatch = contents.match(/^pswpin\s+(\d+)$/mu);
  const outputMatch = contents.match(/^pswpout\s+(\d+)$/mu);
  requireCondition(inputMatch !== null && outputMatch !== null, "swap counters are unavailable");
  return BigInt(inputMatch[1]) + BigInt(outputMatch[1]);
}

// capture one actual filesystem sample
export async function sampleFilesystem(filesystem) {
  requireCondition(Array.isArray(filesystem.paths) && filesystem.paths.length > 0, `filesystem paths missing: ${filesystem.id}`);
  let device;
  let sample;

  // resolve every path to one device
  for (const path of filesystem.paths) {
    const pathInfo = await stat(path);
    const pathSample = await statfs(path, { bigint: true });

    // bind the first device
    if (device === undefined) {
      device = pathInfo.dev.toString();
      sample = pathSample;
    } else {
      requireCondition(device === pathInfo.dev.toString(), `filesystem paths do not share a device: ${filesystem.id}`);
    }
  }

  requireCondition(filesystem.id === device, `filesystem device identity changed: ${filesystem.id}`);
  const totalBytes = sample.blocks * sample.bsize;
  const allocatedBytes = (sample.blocks - sample.bfree) * sample.bsize;
  const allocatedInodes = sample.files - sample.ffree;
  return {
    id: filesystem.id,
    allocatedBytes: allocatedBytes.toString(),
    allocatedInodes: allocatedInodes.toString(),
    freeBytes: (sample.bavail * sample.bsize).toString(),
    freeInodes: sample.ffree.toString(),
    totalBytes: totalBytes.toString(),
    totalInodes: sample.files.toString(),
  };
}

// derive every live operation filesystem from runtime sources
export async function deriveFilesystemInventory(config) {
  const postgresInspect = await inspectContainer(config.postgres.container);
  const mounts = Array.isArray(postgresInspect.Mounts) ? postgresInspect.Mounts : [];
  const dockerInfo = await runCommand("docker", ["info", "--format", "{{json .DockerRootDir}}"], { timeoutMilliseconds: 15_000 });
  const dockerRootValue = JSON.parse(dockerInfo.stdout);
  requireCondition(typeof dockerRootValue === "string" && isAbsolute(dockerRootValue), "Docker root directory is unresolved");
  const databasePaths = await queryPsqlJson(config, `SELECT json_build_object(
    'dataDirectory', current_setting('data_directory'),
    'defaultTablespace', current_setting('default_tablespace'),
    'tempTablespaces', current_setting('temp_tablespaces'),
    'tablespaces', COALESCE((SELECT json_agg(json_build_object('name', spcname, 'location', pg_tablespace_location(oid)) ORDER BY spcname) FROM pg_tablespace), '[]'::json)
  );`);

  // translate one container path through its exact mount
  function hostPath(containerPath) {
    let selected;

    // choose the longest containing mount destination
    for (const mount of mounts) {
      const destination = mount.Destination?.replace(/\/$/u, "");

      // retain containing mounts only
      if (typeof destination === "string" && (containerPath === destination || containerPath.startsWith(`${destination}/`))
        && (selected === undefined || destination.length > selected.Destination.length)) {
        selected = { ...mount, Destination: destination };
      }
    }

    requireCondition(selected !== undefined && typeof selected.Source === "string", `container path has no host mount: ${containerPath}`);
    return join(selected.Source, containerPath.slice(selected.Destination.length).replace(/^\//u, ""));
  }

  const dataRoot = await realpath(hostPath(databasePaths.dataDirectory));
  const paths = [
    { path: dataRoot, role: "postgres-data" },
    { path: await realpath(join(dataRoot, "pg_wal")), role: "postgres-wal" },
    { path: await realpath(JSON.parse(dockerInfo.stdout)), role: "docker-root" },
  ];

  // bind every explicit tablespace location
  for (const tablespace of databasePaths.tablespaces) {
    // default tablespaces live under the data root
    if (tablespace.location === "") {
      paths.push({ path: dataRoot, role: `tablespace:${tablespace.name}` });
    } else {
      paths.push({ path: await realpath(hostPath(tablespace.location)), role: `tablespace:${tablespace.name}` });
    }
  }

  const tempTablespaces = databasePaths.tempTablespaces.split(",").map((value) => value.trim()).filter(Boolean);

  // bind default temp allocation when no override exists
  if (tempTablespaces.length === 0) {
    paths.push({ path: dataRoot, role: "postgres-temp" });
  } else {
    // bind every configured temp tablespace
    for (const name of tempTablespaces) {
      const tablespace = databasePaths.tablespaces.find((candidate) => candidate.name === name);
      requireCondition(tablespace !== undefined, `temp tablespace is unresolved: ${name}`);
      paths.push({ path: tablespace.location === "" ? dataRoot : await realpath(hostPath(tablespace.location)), role: `postgres-temp:${name}` });
    }
  }

  const grouped = new Map();

  // group actual paths by device identity
  for (const entry of paths) {
    const info = await stat(entry.path);
    const id = info.dev.toString();
    let group = grouped.get(id);

    // initialize one device group
    if (group === undefined) {
      const filesystem = await statfs(entry.path, { bigint: true });
      group = { id, paths: new Set(), roles: new Set(), dockerShares: false, totalInodes: filesystem.files.toString() };
      grouped.set(id, group);
    }

    group.paths.add(entry.path);
    group.roles.add(entry.role);

    // mark Docker colocation explicitly
    if (entry.role === "docker-root") {
      group.dockerShares = true;
    }
  }

  const result = [];

  // serialize deterministic device groups
  for (const group of grouped.values()) {
    result.push({
      id: group.id,
      paths: [...group.paths].sort(),
      roles: [...group.roles].sort(),
      dockerShares: group.dockerShares,
      totalInodes: group.totalInodes,
    });
  }

  result.sort((left, right) => left.id.localeCompare(right.id));
  return result;
}

// capture one admission recheck sample
export async function captureAdmissionSample(config, admission, snapshot, swapBaseline = undefined) {
  const filesystems = [];

  // sample every admitted filesystem
  for (const filesystem of admission.filesystems) {
    filesystems.push(await sampleFilesystem(filesystem));
  }

  const swapPages = await readSwapPages();
  let swapBytesPerMinute = 0n;

  // calculate bounded swap traffic
  if (swapBaseline !== undefined) {
    const elapsedMilliseconds = Math.max(1, Date.now() - swapBaseline.capturedAt);
    const deltaPages = swapPages >= swapBaseline.pages ? swapPages - swapBaseline.pages : 0n;
    swapBytesPerMinute = deltaPages * 4096n * 60_000n / BigInt(elapsedMilliseconds);
  }

  return {
    capturedAt: new Date().toISOString(),
    identities: {
      aclDigest: snapshot.identities.aclDigest,
      catalogDigest: snapshot.identities.catalogDigest,
      controlDigest: snapshot.identities.controlDigest,
      databaseDigest: snapshot.identities.databaseDigest,
      forecastSettingsDigest: snapshot.identities.forecastSettingsDigest,
      imageDigest: snapshot.identities.imageDigest,
      ledgerDigest: snapshot.identities.ledgerDigest,
      packageDigest: admission.identities.packageDigest,
      manifestDigest: admission.identities.manifestDigest,
      settingsDigest: snapshot.identities.settingsDigest,
      sourceDigest: snapshot.identities.sourceDigest,
    },
    scale: {
      liveRows: snapshot.database.liveRows,
      liveHeapBytes: snapshot.database.liveHeapBytes,
    },
    filesystems,
    containers: snapshot.containers,
    host: {
      architecture: arch(),
      cpuCount: cpus().length,
      load15: loadavg()[2],
      availableMemoryBytes: (await readAvailableMemory()).toString(),
      swapBytesPerMinute: swapBytesPerMinute.toString(),
      fixtureWriteP95Ratio: admission.host.fixtureWriteP95Ratio,
    },
    swapBaseline: { pages: swapPages, capturedAt: Date.now() },
  };
}

// acquire the shared state-directory inode flock
export async function acquireStateDirectoryLock(stateRoot, options = {}) {
  const canonicalPath = await realpath(stateRoot);
  requireCondition(canonicalPath === resolve(stateRoot), "state root must be canonical");
  const handle = await open(canonicalPath, fsConstants.O_RDONLY | fsConstants.O_DIRECTORY | fsConstants.O_NOFOLLOW);
  const before = await handle.stat();
  requireCondition(before.isDirectory(), "state lock target must be a directory");
  const child = spawn("sh", ["-c", "flock --exclusive --nonblock 3 || exit 73; printf 'LOCKED\\n'; cat >/dev/null"], {
    cwd: canonicalPath,
    stdio: ["pipe", "pipe", "pipe", handle.fd],
  });
  const lockCompletion = new Promise((resolvePromise) => {
    // retain lock process completion
    child.once("close", (code, signal) => {
      resolvePromise({ code, signal });
    });
  });
  let released = false;

  // release every held child and file resource once
  async function releaseHeldLock() {
    // close the lock-holder input once
    if (!released) {
      released = true;
      child.stdin.end();
      await lockCompletion;
      await handle.close();
    }
  }

  // await exact lock acknowledgement
  let marker;

  try {
    marker = await new Promise((resolvePromise, rejectPromise) => {
      let output = "";

      // read the lock marker
      child.stdout.on("data", (chunk) => {
        output += chunk.toString("utf8");

        // accept the exact marker
        if (output.includes("LOCKED\n")) {
          resolvePromise(output);
        }
      });

      // reject lock process errors
      child.on("error", rejectPromise);

      // reject early process completion
      child.on("close", (code) => {
        // reject missing lock acquisition
        if (!output.includes("LOCKED\n")) {
          rejectPromise(new ContractError("shared state directory lock is busy", { code }));
        }
      });
    });
  } catch (error) {
    await releaseHeldLock();
    throw error;
  }

  try {
    requireCondition(marker.includes("LOCKED\n"), "shared state directory lock did not acknowledge");

    // permit deterministic post-lock fault injection in tests
    if (typeof options.afterAcknowledged === "function") {
      await options.afterAcknowledged();
    }

    const after = await lstat(canonicalPath);
    requireCondition(after.dev === before.dev && after.ino === before.ino, "state root inode changed after locking");
  } catch (error) {
    await releaseHeldLock();
    throw error;
  }

  return {
    identity: { dev: before.dev.toString(), ino: before.ino.toString(), path: canonicalPath },
    // release the held inode lock
    async release() {
      await releaseHeldLock();
    },
  };
}

// atomically fsync one private json artifact
export async function writeDurableJson(path, value, mode = 0o600) {
  const directory = dirname(path);
  await mkdir(directory, { recursive: true, mode: 0o700 });
  const temporary = join(directory, `.${basename(path)}.${process.pid}.${randomUUID()}.tmp`);
  const handle = await open(temporary, fsConstants.O_CREAT | fsConstants.O_EXCL | fsConstants.O_WRONLY | fsConstants.O_NOFOLLOW, mode);

  try {
    await handle.writeFile(`${JSON.stringify(value, null, 2)}\n`);
    await handle.sync();
  } finally {
    await handle.close();
  }

  await chmod(temporary, mode);
  await rename(temporary, path);
  const directoryHandle = await open(directory, fsConstants.O_RDONLY | fsConstants.O_DIRECTORY);

  try {
    await directoryHandle.sync();
  } finally {
    await directoryHandle.close();
  }
}

// atomically create one private artifact without replacement
export async function writeNewDurableFile(path, bytes, mode = 0o600) {
  const directory = dirname(path);
  const temporary = join(directory, `.${basename(path)}.${process.pid}.${randomUUID()}.tmp`);
  const handle = await open(temporary, fsConstants.O_CREAT | fsConstants.O_EXCL | fsConstants.O_WRONLY | fsConstants.O_NOFOLLOW, mode);
  let published = false;

  try {
    await handle.writeFile(bytes);
    await handle.sync();
    await handle.close();
    await chmod(temporary, mode);
    await link(temporary, path);
    published = true;
  } finally {
    // close an interrupted write
    if (handle.fd !== -1) {
      await handle.close().catch(() => undefined);
    }

    await unlink(temporary).catch(() => undefined);
  }

  requireCondition(published, `artifact was not published: ${path}`);
  const info = await lstat(path);
  requireCondition(info.isFile() && !info.isSymbolicLink() && info.nlink === 1 && (info.mode & 0o777) === mode, `published artifact is unsafe: ${path}`);
  const directoryHandle = await open(directory, fsConstants.O_RDONLY | fsConstants.O_DIRECTORY);

  try {
    await directoryHandle.sync();
  } finally {
    await directoryHandle.close();
  }
}

// append and fsync one audit record
export async function appendAudit(path, value) {
  const directory = dirname(path);
  await mkdir(directory, { recursive: true, mode: 0o700 });
  const handle = await open(path, fsConstants.O_CREAT | fsConstants.O_APPEND | fsConstants.O_WRONLY | fsConstants.O_NOFOLLOW, 0o600);

  try {
    await handle.writeFile(`${stableJson(value)}\n`);
    await handle.sync();
  } finally {
    await handle.close();
  }
}

// pause for one bounded interval
export async function delay(milliseconds) {
  // resolve one timer
  await new Promise((resolvePromise) => {
    setTimeout(resolvePromise, milliseconds);
  });
}

// match durable ownership to a live catalog tuple
export function validateOwnedIndex(ownership, catalog, operationIdentity) {
  requireCondition(ownership?.schema === "weather-page-index-ownership/v2", "index ownership record is missing");
  requireCondition(ownership.captured === true, "index ownership was never durably captured");
  requireCondition(typeof ownership.operationId === "string" && ownership.operationId === operationIdentity.operationId, "ownership operation changed");
  requireCondition(ownership.clusterDigest === operationIdentity.clusterDigest, "ownership cluster changed");
  requireCondition(ownership.databaseOid === operationIdentity.databaseOid, "ownership database changed");
  requireCondition(ownership.tableOid === operationIdentity.tableOid, "ownership table changed");
  requireCondition(ownership.indexOid === catalog?.indexOid, "owned index OID changed");
  requireCondition(ownership.fingerprintHash === catalogFingerprintHash(catalog), "owned index fingerprint changed");
  requireCondition(Array.isArray(catalog.constraintBacking) && catalog.constraintBacking.length === 0, "owned index gained a constraint");
  return true;
}

// parse one legacy utc14 timestamp
export function parseUtc14(value) {
  requireCondition(/^\d{14}$/u.test(value), "restore candidate timestamp is malformed");
  const year = Number(value.slice(0, 4));
  const month = Number(value.slice(4, 6));
  const day = Number(value.slice(6, 8));
  const hour = Number(value.slice(8, 10));
  const minute = Number(value.slice(10, 12));
  const second = Number(value.slice(12, 14));
  const timestamp = Date.UTC(year, month - 1, day, hour, minute, second);
  const normalized = new Date(timestamp).toISOString().replace(/[-:TZ.]/gu, "").slice(0, 14);
  requireCondition(normalized === value, "restore candidate timestamp is not a calendar time");
  return timestamp;
}

// select a uniquely owned restore candidate
export function bindRestoreCandidate(before, after, expectation) {
  requireCondition(Array.isArray(before) && Array.isArray(after), "restore catalog snapshots are missing");
  const beforeOids = new Set();

  // record every preexisting database oid
  for (const database of before) {
    beforeOids.add(String(database.oid));
  }

  const expectedPrefix = `${expectation.fixtureDatabase.slice(0, 32)}_verify_`;
  const expectedPattern = new RegExp(`^${expectedPrefix}(\\d{14})_${expectation.childPid}$`, "u");
  const candidates = [];

  // find newly created exact-pattern candidates
  for (const database of after) {
    const oid = String(database.oid);
    const nameMatch = database.name.match(expectedPattern);

    // select only absent-before candidates
    if (!beforeOids.has(oid)
      && nameMatch !== null
      && database.owner === expectation.owner) {
      const createdAt = Date.parse(database.createdAt);
      const namedAt = parseUtc14(nameMatch[1]);

      // bind only candidates within the supervisor interval
      if (Number.isFinite(createdAt)
        && createdAt >= expectation.startedAtEpochMilliseconds - expectation.clockSkewMilliseconds
        && createdAt <= expectation.observedAtEpochMilliseconds + expectation.clockSkewMilliseconds
        && namedAt >= expectation.startedAtEpochMilliseconds - expectation.clockSkewMilliseconds
        && namedAt <= expectation.observedAtEpochMilliseconds + expectation.clockSkewMilliseconds) {
        candidates.push(database);
      }
    }
  }

  requireCondition(candidates.length === 1, "restore candidate ownership is ambiguous", { candidates });
  return {
    name: candidates[0].name,
    oid: String(candidates[0].oid),
    owner: candidates[0].owner,
    createdAt: candidates[0].createdAt,
  };
}

// parse a successful legacy restore candidate
export function parseLegacyRestoreCandidate(stdout) {
  const retained = [...stdout.matchAll(/^Retained diagnostic database: ([a-z][a-z0-9_]{0,62})$/gmu)];
  const verified = [...stdout.matchAll(/^Verified disposable restore database ([a-z][a-z0-9_]{0,62}):/gmu)];
  requireCondition(retained.length === 1 && verified.length === 1, "legacy restore success output is incomplete");
  requireCondition(retained[0][1] === verified[0][1], "legacy restore output names disagree");
  return retained[0][1];
}

// query create-index ownership progress
export async function queryCreateProgress(config, session, indexSpec, database = config.postgres.database) {
  const sql = `SELECT COALESCE((SELECT json_build_object(
    'pid', progress.pid,
    'backendStart', activity.backend_start,
    'databaseOid', progress.datid::text,
    'tableOid', progress.relid::text,
    'indexOid', progress.index_relid::text,
    'command', progress.command,
    'phase', progress.phase,
    'lockersTotal', progress.lockers_total,
    'lockersDone', progress.lockers_done,
    'blocksTotal', progress.blocks_total,
    'blocksDone', progress.blocks_done,
    'tuplesTotal', progress.tuples_total,
    'tuplesDone', progress.tuples_done
  )
  FROM pg_stat_progress_create_index progress
  JOIN pg_stat_activity activity ON activity.pid = progress.pid
  WHERE progress.pid = ${Number(session.pid)}
    AND activity.backend_start = ${quoteLiteral(session.backendStart)}::timestamptz
    AND progress.datid = ${quoteLiteral(session.databaseOid)}::oid
    AND progress.relid = ${quoteLiteral(session.tableOid)}::oid
    AND progress.command = 'CREATE INDEX CONCURRENTLY'
    AND progress.index_relid <> 0
    AND EXISTS (
      SELECT 1 FROM pg_class index_relation
      JOIN pg_namespace index_namespace ON index_namespace.oid = index_relation.relnamespace
      WHERE index_relation.oid = progress.index_relid
        AND index_namespace.nspname = ${quoteLiteral(indexSpec.schema)}
        AND index_relation.relname = ${quoteLiteral(indexSpec.name)}
    )
  LIMIT 1), 'null'::json)::text;`;
  return await queryPsqlJson(config, sql, database);
}

// cancel exactly one bound ddl backend
export async function cancelOwnedBackend(config, session, database = config.postgres.database) {
  const sql = `SELECT COALESCE((SELECT pg_cancel_backend(pid)
    FROM pg_stat_activity
    WHERE pid = ${Number(session.pid)}
      AND backend_start = ${quoteLiteral(session.backendStart)}::timestamptz
      AND datid = ${quoteLiteral(session.databaseOid)}::oid
      AND application_name = ${quoteLiteral(session.applicationName)}), false);`;
  return (await queryPsql(config, sql, database)) === "t";
}

// terminate exactly one bound ddl backend
export async function terminateOwnedBackend(config, session, database = config.postgres.database) {
  const sql = `SELECT COALESCE((SELECT pg_terminate_backend(pid)
    FROM pg_stat_activity
    WHERE pid = ${Number(session.pid)}
      AND backend_start = ${quoteLiteral(session.backendStart)}::timestamptz
      AND datid = ${quoteLiteral(session.databaseOid)}::oid
      AND application_name = ${quoteLiteral(session.applicationName)}), true);`;
  return (await queryPsql(config, sql, database)) === "t";
}

// inspect exact backend and progress liveness
export async function inspectOwnedBackend(config, session, database = config.postgres.database) {
  const sql = `SELECT json_build_object(
    'active', EXISTS (
      SELECT 1 FROM pg_stat_activity
      WHERE pid = ${Number(session.pid)}
        AND backend_start = ${quoteLiteral(session.backendStart)}::timestamptz
        AND datid = ${quoteLiteral(session.databaseOid)}::oid
        AND application_name = ${quoteLiteral(session.applicationName)}
    ),
    'progress', EXISTS (
      SELECT 1 FROM pg_stat_progress_create_index progress
      JOIN pg_stat_activity activity ON activity.pid = progress.pid
      WHERE progress.pid = ${Number(session.pid)}
        AND activity.backend_start = ${quoteLiteral(session.backendStart)}::timestamptz
        AND progress.datid = ${quoteLiteral(session.databaseOid)}::oid
    )
  )::text;`;
  return await queryPsqlJson(config, sql, database);
}

// prove one owned backend and build are terminal
export async function ensureOwnedBackendTerminal(config, session, database = config.postgres.database, controls = {}) {
  const inspect = controls.inspect ?? inspectOwnedBackend;
  const cancel = controls.cancel ?? cancelOwnedBackend;
  const terminate = controls.terminate ?? terminateOwnedBackend;
  const pause = controls.delay ?? delay;
  let terminationRequested = false;

  // retain monitoring until positive terminal proof
  while (true) {
    let status;

    try {
      status = await inspect(config, session, database);
    } catch {
      await pause(2000);
      continue;
    }

    // accept only absent activity and progress
    if (status.active === false && status.progress === false) {
      return status;
    }

    try {
      // escalate one still-live exact backend
      if (terminationRequested) {
        await terminate(config, session, database);
      } else {
        await cancel(config, session, database);
        terminationRequested = true;
      }
    } catch {
      terminationRequested = true;
    }

    await pause(2000);
  }
}

// reject competing schema or index work
export async function assertNoCompetingDdl(config, ownPid, database = config.postgres.database) {
  const sql = `SELECT count(*)::text FROM pg_stat_activity
    WHERE datname = current_database()
      AND pid <> ${Number(ownPid)}
      AND pid <> pg_backend_pid()
      AND state <> 'idle'
      AND (
        query ~* '(^|[^a-z])(create|drop|alter|reindex)[[:space:]]+(index|table|schema)'
        OR application_name IN ('weather-migrate', 'weather-page-index-v1')
      );`;
  requireCondition(await queryPsql(config, sql, database) === "0", "competing schema or index activity is present");
}

// open one same-session advisory-lock ddl protocol
export async function openDdlSession(config, manifest, database = config.postgres.database) {
  const applicationName = config.postgres.applicationName ?? manifest.operation.applicationName;
  const args = dockerPsqlArgs({
    ...config,
    postgres: { ...config.postgres, applicationName },
  }, database);
  const child = spawn("docker", args, { stdio: ["pipe", "pipe", "pipe"] });
  const lines = [];
  const waiters = [];
  let buffer = "";
  let stderr = "";
  let closed;
  let closeRequested = false;

  // deliver newly completed protocol lines
  function deliver(line) {
    lines.push(line);

    // notify every pending waiter
    for (const waiter of [...waiters]) {
      // resolve matching lines
      if (waiter.predicate(line)) {
        clearTimeout(waiter.timer);
        waiters.splice(waiters.indexOf(waiter), 1);
        waiter.resolve(line);
      }
    }
  }

  // parse standard output lines
  child.stdout.on("data", (chunk) => {
    buffer += chunk.toString("utf8");
    let newline = buffer.indexOf("\n");

    // emit complete lines
    while (newline >= 0) {
      deliver(buffer.slice(0, newline).trim());
      buffer = buffer.slice(newline + 1);
      newline = buffer.indexOf("\n");
    }
  });

  // retain standard error diagnostics
  child.stderr.on("data", (chunk) => {
    stderr += chunk.toString("utf8");
  });

  // observe protocol process completion
  const completion = new Promise((resolvePromise, rejectPromise) => {
    // reject spawn failures
    child.on("error", rejectPromise);

    // record session completion
    child.on("close", (code, signal) => {
      closed = { code, signal, stderr, lines };

      // reject every waiter stranded by session exit
      for (const waiter of [...waiters]) {
        clearTimeout(waiter.timer);
        waiters.splice(waiters.indexOf(waiter), 1);
        waiter.reject(new ContractError("DDL session closed before protocol completion", closed));
      }

      resolvePromise(closed);
    });
  });

  // wait for one protocol line
  async function waitFor(predicate, timeoutMilliseconds, label) {
    // reuse already buffered lines
    for (const line of lines) {
      // return one matching line
      if (predicate(line)) {
        return line;
      }
    }

    requireCondition(closed === undefined, `DDL session closed before ${label}`);
    // register one bounded waiter
    return await new Promise((resolvePromise, rejectPromise) => {
      const waiter = {
        predicate,
        resolve: resolvePromise,
        reject: rejectPromise,
        timer: undefined,
      };
      // expire one protocol waiter
      waiter.timer = setTimeout(() => {
        const position = waiters.indexOf(waiter);

        // remove the expired waiter
        if (position >= 0) {
          waiters.splice(position, 1);
        }

        rejectPromise(new ContractError(`timed out waiting for ${label}`));
      }, timeoutMilliseconds);
      waiters.push(waiter);
    });
  }

  const setupSql = `
\\set ON_ERROR_STOP on
SET lock_timeout = '${Number(manifest.operation.lockTimeoutMilliseconds)}ms';
SET statement_timeout = '${Number(manifest.operation.buildTimeoutSeconds)}s';
SET maintenance_work_mem = '${manifest.operation.maintenanceWorkMem}';
SET max_parallel_maintenance_workers = ${Number(manifest.operation.maxParallelMaintenanceWorkers)};
SELECT CASE WHEN pg_try_advisory_lock(${manifest.operation.advisoryLockKey}::bigint)
  THEN 'WEATHER_LOCKED|' || pg_backend_pid()::text || '|' || backend_start::text || '|' || datid::text || '|' || ${quoteLiteral(applicationName)}
  ELSE 'WEATHER_BUSY' END
FROM pg_stat_activity WHERE pid = pg_backend_pid();
`.trimStart();
  let parts;
  let tableOid;

  try {
    child.stdin.write(setupSql);
    // await the database lock outcome
    const lockLine = await waitFor((line) => line === "WEATHER_BUSY" || line.startsWith("WEATHER_LOCKED|"), 15_000, "database advisory lock");
    requireCondition(lockLine !== "WEATHER_BUSY", "database advisory lock is busy");
    parts = lockLine.split("|");
    requireCondition(parts.length === 5, "database lock identity is malformed");
    const tableSql = `SELECT ${quoteLiteral(`${manifest.replacement.target.schema}.${manifest.replacement.target.table}`)}::regclass::oid::text;`;
    child.stdin.write(`${tableSql}\n\\echo WEATHER_TABLE_SENT\n`);
    // await the bound table oid
    tableOid = await waitFor((line) => /^\d+$/u.test(line), 10_000, "table oid");
  } catch (error) {
    // close or terminate every partially initialized session
    if (parts?.length === 5) {
      const partial = {
        pid: Number(parts[1]),
        backendStart: parts[2],
        databaseOid: parts[3],
        applicationName: parts[4],
      };
      await terminateOwnedBackend(config, partial, database).catch(() => false);
      await ensureOwnedBackendTerminal(config, partial, database);
    }

    // close an unlocked or already terminated psql client
    if (child.stdin.writable && !child.stdin.destroyed) {
      child.stdin.end("\\q\n");
    }

    const childResult = await Promise.race([
      completion,
      delay(5000).then(() => null),
    ]);

    // terminate a stranded local docker client
    if (childResult === null) {
      child.kill("SIGTERM");
      await completion;
    }

    throw error;
  }

  return {
    child,
    completion,
    lines,
    // expose retained diagnostics
    stderr: () => stderr,
    waitFor,
    pid: Number(parts[1]),
    backendStart: parts[2],
    databaseOid: parts[3],
    applicationName: parts[4],
    tableOid,
    // send trusted manifest sql only
    send(sql) {
      requireCondition(typeof sql === "string" && sql.length > 0, "DDL protocol SQL is empty");
      child.stdin.write(`${sql}\n`);
    },
    // close the advisory-lock session
    close() {
      // avoid writing after a failed psql exit
      if (!closeRequested && child.stdin.writable && !child.stdin.destroyed) {
        closeRequested = true;
        child.stdin.end(`SELECT pg_advisory_unlock(${manifest.operation.advisoryLockKey}::bigint);\n\\q\n`);
      }
    },
  };
}

// close one ddl session with positive backend proof
export async function settleDdlSession(config, session, options = {}) {
  // cancel or terminate a possibly running statement
  if (options.cancel === true) {
    let cancelled = false;

    try {
      cancelled = await cancelOwnedBackend(config, session, options.database);
    } catch {
      cancelled = false;
    }

    // terminate when cancellation was refused
    if (!cancelled) {
      await terminateOwnedBackend(config, session, options.database).catch(() => false);
    }
  } else {
    session.close();
  }

  await ensureOwnedBackendTerminal(config, session, options.database);

  // close a surviving idle client after terminal proof
  session.close();
  const completion = await session.completion;
  return completion;
}

// run one nonoverlapping fixed-cadence sampler
export function startExclusiveIntervalSampler({ intervalMilliseconds, sample, onFailure, schedule = setInterval, unschedule = clearInterval }) {
  requireCondition(Number.isSafeInteger(intervalMilliseconds) && intervalMilliseconds > 0, "sampler interval is invalid");
  requireCondition(typeof sample === "function" && typeof onFailure === "function", "sampler callbacks are missing");
  let active;
  let stopped = false;

  // launch one sample or fail the missed cadence
  async function launch() {
    // refuse overlapping samples
    if (active !== undefined) {
      onFailure(`resource monitor missed its ${intervalMilliseconds}-millisecond cadence`);
      return;
    }

    active = Promise.resolve().then(sample);

    try {
      await active;
    } catch (error) {
      onFailure(`resource monitoring failed: ${error instanceof Error ? error.message : String(error)}`);
    } finally {
      active = undefined;
    }
  }

  void launch();
  const timer = schedule(() => {
    // launch only while active
    if (!stopped) {
      void launch();
    }
  }, intervalMilliseconds);

  return {
    // stop and settle the only possible sample
    async stop() {
      stopped = true;
      unschedule(timer);

      // await the in-flight sample
      if (active !== undefined) {
        await active.catch(() => undefined);
      }
    },
  };
}

// monitor one owned build sample
export async function monitorBuildResources(config, admission, constants, admittedSample, computed) {
  const filesystems = [];
  const reasons = [];
  const admittedById = new Map();
  const thresholdById = new Map();

  // index admission baselines
  for (const filesystem of admittedSample.filesystems) {
    admittedById.set(filesystem.id, filesystem);
  }

  // index admission thresholds
  for (const threshold of computed.filesystems) {
    thresholdById.set(threshold.id, threshold);
  }

  // sample and compare each filesystem
  for (const filesystem of admission.filesystems) {
    const sample = await sampleFilesystem(filesystem);
    filesystems.push(sample);
    const baseline = admittedById.get(sample.id);
    const threshold = thresholdById.get(sample.id);
    requireCondition(baseline !== undefined && threshold !== undefined, `monitor baseline missing: ${sample.id}`);
    const freeBytes = toBigInt(sample.freeBytes, "monitor free bytes");
    const freeInodes = toBigInt(sample.freeInodes, "monitor free inodes");
    const protectedBytes = toBigInt(threshold.protectedBytes, "protected bytes");
    const protectedInodes = toBigInt(threshold.protectedInodes, "protected inodes");
    const allocatedBytes = toBigInt(baseline.freeBytes, "baseline free bytes") > freeBytes
      ? toBigInt(baseline.freeBytes, "baseline free bytes") - freeBytes
      : 0n;
    const allocatedInodes = toBigInt(baseline.freeInodes, "baseline free inodes") > freeInodes
      ? toBigInt(baseline.freeInodes, "baseline free inodes") - freeInodes
      : 0n;

    // enforce the abort byte floor
    if (freeBytes < protectedBytes + toBigInt(constants.abortByteMargin, "abort byte margin")) {
      reasons.push(`protected byte floor breached: ${sample.id}`);
    }

    // enforce the abort inode floor
    if (freeInodes < protectedInodes + toBigInt(constants.abortInodeMargin, "abort inode margin")) {
      reasons.push(`protected inode floor breached: ${sample.id}`);
    }

    // enforce allocated byte reserve
    if (allocatedBytes > toBigInt(threshold.reserveBytes, "reserved bytes")) {
      reasons.push(`allocated byte reserve exceeded: ${sample.id}`);
    }

    // enforce allocated inode reserve
    if (allocatedInodes > toBigInt(threshold.reserveInodes, "reserved inodes")) {
      reasons.push(`allocated inode reserve exceeded: ${sample.id}`);
    }
  }

  // enforce heartbeats only for a live application build
  if (config.restoreCandidate !== true) {
    const heartbeatAgeText = await queryPsql(config, `SELECT COALESCE(EXTRACT(EPOCH FROM clock_timestamp() - max(last_loop_at))::text, 'missing') FROM worker_heartbeats;`);
    const heartbeatAge = Number(heartbeatAgeText);

    // reject missing or stale durable worker heartbeats
    if (!Number.isFinite(heartbeatAge) || heartbeatAge < 0 || heartbeatAge > computed.heartbeatAllowanceSeconds) {
      reasons.push("worker heartbeat exceeded its allowance");
    }
  }

  const containers = [];
  const inspectedContainers = await inspectContainers(admittedSample.containers.map((container) => container.name));

  // detect restarts and oom kills
  for (const expected of admittedSample.containers) {
    const inspected = inspectedContainers.get(expected.name);
    requireCondition(inspected !== undefined, `monitored container disappeared: ${expected.name}`);
    containers.push({ name: expected.name, restartCount: inspected.RestartCount, oomKilled: inspected.State?.OOMKilled });

    // reject container regressions
    if (inspected.RestartCount !== expected.restartCount || inspected.State?.OOMKilled === true) {
      reasons.push(`container restart or OOM detected: ${expected.name}`);
    }
  }

  const endpoints = [];
  const swapPages = await readSwapPages();
  const swapElapsedMilliseconds = Math.max(1, Date.now() - admittedSample.swapBaseline.capturedAt);
  const swapDeltaPages = swapPages >= admittedSample.swapBaseline.pages
    ? swapPages - admittedSample.swapBaseline.pages
    : 0n;
  const swapBytesPerMinute = swapDeltaPages * 4096n * 60_000n / BigInt(swapElapsedMilliseconds);

  // reject excessive swap traffic
  if (swapBytesPerMinute > toBigInt(constants.maximumSwapBytesPerMinute, "maximum swap")) {
    reasons.push("swap traffic exceeded the maximum");
  }

  // probe every bounded live monitoring endpoint
  const endpointPromises = [];

  // launch fixed page probes concurrently
  for (const endpoint of config.restoreCandidate === true ? [] : config.monitor.endpoints) {
    endpointPromises.push(runCommand("docker", [
      "exec",
      config.monitor.apiContainer,
      "node",
      "-e",
      `const controller=new AbortController();const timer=setTimeout(()=>controller.abort(),${Number(endpoint.timeoutMilliseconds)});fetch('http://127.0.0.1:3001${endpoint.path}',{signal:controller.signal}).then(async response=>{const body=await response.arrayBuffer();console.log(JSON.stringify({status:response.status,bytes:body.byteLength}));clearTimeout(timer)}).catch(error=>{console.error(error.message);process.exit(1)})`,
    ], { timeoutMilliseconds: Number(endpoint.timeoutMilliseconds) + 1000, allowFailure: true }).then((result) => ({ endpoint, result })));
  }

  const endpointResults = await Promise.all(endpointPromises);

  // classify every fixed page probe
  for (const { endpoint, result } of endpointResults) {
    let status;
    let bytes;

    try {
      const response = JSON.parse(result.stdout);
      status = response.status;
      bytes = response.bytes;
    } catch {
      status = undefined;
      bytes = undefined;
    }

    endpoints.push({ name: endpoint.name, path: endpoint.path, status, bytes, error: result.stderr.trim() || undefined });

    // reject non-successful or empty API responses
    if (result.code !== 0 || !Number.isInteger(status) || status < 200 || status >= 300 || !Number.isInteger(bytes) || bytes <= 0) {
      reasons.push(`endpoint monitoring failed: ${endpoint.name}`);
    }
  }

  return { capturedAt: new Date().toISOString(), filesystems, containers, endpoints, swapBytesPerMinute: swapBytesPerMinute.toString(), reasons };
}

// classify one exact predecessor compensation state
export function classifyPredecessorCompensation(actual, recorded, indexSpec, expectedOwner) {
  // recreate one absent predecessor
  if (actual === null) {
    return "recreate";
  }

  // reject a replaced same-name object
  if (actual.indexOid !== recorded.indexOid) {
    return "mismatch";
  }

  try {
    validateCatalogFingerprint(actual, indexSpec, expectedOwner, { requireValid: false });
  } catch {
    return "mismatch";
  }

  // retain only the same fully valid original
  if (actual.valid === true && actual.ready === true && actual.live === true) {
    return "retain";
  }

  return "drop-recreate";
}

// run one bounded ddl statement in the retained lock session
async function runSessionDdl(session, sql, marker, timeoutMilliseconds, label) {
  session.send(`${sql};\n\\echo ${marker}`);
  await session.waitFor((line) => line === marker, timeoutMilliseconds, label);
}

// create one index with durable progress ownership in an existing session
async function createOwnedIndexInSession({ config, manifest, admission, admissionSample, operationDirectory, operation, session, indexSpec, ownershipName, controls = {} }) {
  const ownershipPath = join(operationDirectory, ownershipName);
  const marker = `WEATHER_CREATE_FINISHED_${indexSpec.name.toUpperCase()}`;
  session.send(`${indexSpec.createSql};\n\\echo ${marker}`);
  let ownership;
  let createFinished = false;
  const samples = [];
  const startedAt = Date.now();
  let failure;
  let monitorFailure;
  const monitor = startExclusiveIntervalSampler({
    intervalMilliseconds: Number(manifest.operation.resourcePollMilliseconds),
    // capture one complete resource sample
    async sample() {
      const sample = await (controls.monitorResources ?? monitorBuildResources)(config, admission, manifest.admission, admissionSample, admission.computed);
      samples.push(sample);
      await appendAudit(join(operationDirectory, "audit.jsonl"), { event: "resource-sample", index: indexSpec.name, sample });

      // retain the first resource breach
      if (sample.reasons.length > 0 && monitorFailure === undefined) {
        monitorFailure = sample.reasons.join("; ");
      }
    },
    // retain the first sampler failure
    onFailure(message) {
      // retain only the first failure
      if (monitorFailure === undefined) {
        monitorFailure = message;
      }
    },
  });

  try {
    // observe progress until completion or failure
    while (!createFinished && failure === undefined) {
      // stop on an independent monitor failure
      if (monitorFailure !== undefined) {
        failure = monitorFailure;
        break;
      }

      // enforce one component ceiling
      if (Date.now() - startedAt > Number(manifest.operation.buildTimeoutSeconds) * 1000) {
        failure = "build timeout exceeded";
        break;
      }

      // detect the exact completion marker
      if (session.lines.includes(marker)) {
        createFinished = true;
        break;
      }

      let progress;

      try {
        progress = await queryCreateProgress(config, session, indexSpec);
      } catch (error) {
        failure = `progress ownership monitoring failed: ${error.message}`;
        break;
      }

      // durably bind the progress oid
      if (ownership === undefined && progress !== null) {
        const candidate = await queryPsqlJson(config, catalogFingerprintSql(indexSpec));
        requireCondition(candidate !== null && candidate.indexOid === progress.indexOid, "progress index OID does not match the catalog candidate");
        validateCatalogFingerprint(candidate, indexSpec, config.postgres.tableOwner, { requireValid: false });
        ownership = {
          schema: "weather-page-index-ownership/v2",
          operationId: operation.operationId,
          indexName: indexSpec.name,
          captured: true,
          capturedAt: new Date().toISOString(),
          clusterDigest: operation.clusterDigest,
          databaseOid: session.databaseOid,
          tableOid: session.tableOid,
          indexOid: progress.indexOid,
          creatorPid: session.pid,
          creatorBackendStart: session.backendStart,
          progress,
          fingerprint: normalizeCatalogFingerprint(candidate),
          fingerprintHash: catalogFingerprintHash(candidate),
        };
        await writeDurableJson(ownershipPath, ownership);
        await appendAudit(join(operationDirectory, "audit.jsonl"), { event: "ownership-captured", ownership });
      }

      await delay(ownership === undefined ? Number(manifest.operation.progressPollMilliseconds) : 200);
    }
  } finally {
    await monitor.stop();
  }

  // retain a monitor failure that raced completion
  if (failure === undefined && monitorFailure !== undefined) {
    failure = monitorFailure;
  }

  // cancel only the exact still-running backend on failure
  if (failure !== undefined) {
    await cancelOwnedBackend(config, session).catch(() => false);
    await ensureOwnedBackendTerminal(config, session);
    throw new ContractError(failure, { ownership, samples });
  }

  const finalCatalog = await queryPsqlJson(config, catalogFingerprintSql(indexSpec));
  requireCondition(ownership !== undefined, "DDL completed before durable progress index ownership was captured");
  validateCatalogFingerprint(finalCatalog, indexSpec, config.postgres.tableOwner);
  requireCondition(finalCatalog.indexOid === ownership.indexOid, "completed index OID changed after ownership capture");
  ownership = {
    ...ownership,
    completedAt: new Date().toISOString(),
    fingerprint: normalizeCatalogFingerprint(finalCatalog),
    fingerprintHash: catalogFingerprintHash(finalCatalog),
  };
  await writeDurableJson(ownershipPath, ownership);
  return { ownership, ownershipPath, catalog: normalizeCatalogFingerprint(finalCatalog), samples };
}

// validate exact operation ownership for one target
function validateReplacementOwnership(ownership, catalog, operation) {
  requireCondition(ownership?.schema === "weather-page-index-ownership/v2", "replacement target ownership record is missing");
  validateOwnedIndex(ownership, catalog, operation);
  requireCondition(ownership.indexName === catalog.index, "replacement target ownership name changed");
  return true;
}

// acquire one fresh session after positive original-backend terminal proof
async function acquireCompensationSession(config, manifest, session) {
  // terminate every original statement and backend before recovery classification
  if (session !== undefined) {
    await settleDdlSession(config, session, { cancel: true });
  }

  return { session: await openDdlSession(config, manifest), replacement: true };
}

// restore the predecessor before removing an owned target
export async function compensateReplacement({ config, manifest, admission, admissionSample, operationDirectory, operation, session, controls = {} }) {
  const replacement = manifest.replacement;
  const auditPath = join(operationDirectory, "audit.jsonl");
  let ownership;

  try {
    ({ value: ownership } = await readJson(join(operationDirectory, "target-ownership.json")));
  } catch (error) {
    // accept only a target never durably captured
    if (error?.code !== "ENOENT") {
      throw error;
    }
  }
  const held = await acquireCompensationSession(config, manifest, session);
  let settled = false;

  try {
    await assertNoCompetingDdl(config, held.session.pid);
    requireCondition(held.session.databaseOid === operation.databaseOid && held.session.tableOid === operation.tableOid, "compensation database or table identity changed");
    const actual = await queryPsqlJson(config, catalogFingerprintSql(replacement.predecessor));
    const action = classifyPredecessorCompensation(actual, operation.predecessor, replacement.predecessor, config.postgres.tableOwner);
    requireCondition(action !== "mismatch", "predecessor name, OID, or structure changed; target retained", { actual, recorded: operation.predecessor });

    // finish one invalid original before recreation
    if (action === "drop-recreate") {
      await runSessionDdl(held.session, replacement.predecessor.dropSql, "WEATHER_PREDECESSOR_INVALID_DROPPED", 60_000, "invalid predecessor drop");
      requireCondition(await queryPsqlJson(config, catalogFingerprintSql(replacement.predecessor)) === null, "invalid predecessor survived compensation drop");
    }

    // recreate one absent or removed predecessor first
    if (action === "recreate" || action === "drop-recreate") {
      // inject one disposable predecessor recovery fault
      await controls.beforePredecessorRecreate?.({ operation, action });
      await createOwnedIndexInSession({
        config,
        manifest,
        admission,
        admissionSample,
        operationDirectory,
        operation,
        session: held.session,
        indexSpec: replacement.predecessor,
        ownershipName: "predecessor-ownership.json",
        controls,
      });
    }

    const restoredSource = await queryPsqlJson(config, catalogFingerprintSql(replacement.predecessor));
    validateCatalogFingerprint(restoredSource, replacement.predecessor, config.postgres.tableOwner);
    const target = await queryPsqlJson(config, catalogFingerprintSql(replacement.target));

    // remove only the operation-owned target after source recovery
    if (target !== null) {
      requireCondition(ownership !== undefined, "target exists without durable package ownership; retained");
      validateCatalogFingerprint(target, replacement.target, config.postgres.tableOwner, { requireValid: false });
      validateReplacementOwnership(ownership, target, {
        operationId: operation.operationId,
        clusterDigest: operation.clusterDigest,
        databaseOid: held.session.databaseOid,
        tableOid: held.session.tableOid,
      });
      // inject one disposable target cleanup fault
      await controls.beforeTargetCompensationDrop?.({ operation, target });
      await runSessionDdl(held.session, replacement.target.dropSql, "WEATHER_TARGET_COMPENSATED", 60_000, "owned target compensation drop");
      requireCondition(await queryPsqlJson(config, catalogFingerprintSql(replacement.target)) === null, "owned target survived compensation");
    }

    const inventory = await captureIndexInventory(config, replacement.target);
    validateReplacementInventory(inventory, manifest, config.postgres.tableOwner, "restored", operation.sourceInventory);
    // inject one disposable invariant drift fault
    await controls.beforeRecoveryInvariantCheck?.({ operation, inventory });
    const invariants = await captureRecoveryInvariants(config, config.postgres.database, { forecastSettings: operation.sourceForecastSettings });
    const invariantIdentity = validateRecoveryInvariantIdentity(invariants, operation);
    const health = await (controls.monitorResources ?? monitorBuildResources)(config, admission, manifest.admission, admissionSample, admission.computed);
    requireCondition(health.reasons.length === 0, "compensation final health check failed", health);
    const receipt = {
      schema: "weather-page-index-compensation/v1",
      operationId: operation.operationId,
      action,
      completedAt: new Date().toISOString(),
      predecessor: normalizeCatalogFingerprint(restoredSource),
      sourceInventoryDigest: sha256(stableJson(inventory)),
      sourceInvariantDigest: invariants.digest,
      aclDigest: invariantIdentity.aclDigest,
      forecastSettingsDigest: invariantIdentity.forecastSettingsDigest,
      health,
    };
    const completion = await settleDdlSession(config, held.session);
    settled = true;
    requireCondition(completion.code === 0, "compensation DDL session failed", completion);
    await appendAudit(auditPath, { event: "compensated", receipt });
    await writeDurableJson(join(operationDirectory, "compensation.json"), receipt);
    return receipt;
  } catch (error) {
    // close every incomplete compensation backend
    if (!settled) {
      await settleDdlSession(config, held.session, { cancel: true });
    }

    throw error;
  }
}

// execute one create-first managed replacement
export async function runManagedCreate({ config, manifest, admission, admissionSample, operationDirectory, snapshot, snapshotOptions = {}, controls = {} }) {
  const replacement = manifest.replacement;
  const operationId = basename(operationDirectory);
  const auditPath = join(operationDirectory, "audit.jsonl");
  const operationPath = join(operationDirectory, "operation.json");
  const statePath = join(operationDirectory, "replacement-state.json");
  const resultPath = join(operationDirectory, "result.json");
  const sourceState = validateReplacementInventory(snapshot.indexInventory, manifest, config.postgres.tableOwner, "source");
  const sourceInvariants = await captureRecoveryInvariants(config, config.postgres.database, { forecastSettings: snapshot.forecastSettings });
  const session = await openDdlSession(config, manifest);
  let settled = false;
  let dropStarted = false;
  let operation;

  try {
    operation = {
      schema: "weather-page-index-operation/v2",
      operationId,
      status: "replacement-started",
      startedAt: new Date().toISOString(),
      packageDigest: admission.identities.packageDigest,
      manifestDigest: admission.identities.manifestDigest,
      runtimeConfigDigest: admission.runtimeConfigDigest,
      clusterDigest: snapshot.identities.databaseDigest,
      databaseOid: session.databaseOid,
      tableOid: session.tableOid,
      predecessor: sourceState.predecessor,
      sourceInventory: snapshot.indexInventory,
      sourceInventoryDigest: sha256(stableJson(snapshot.indexInventory)),
      sourceIdentities: snapshot.identities,
      sourceInvariantDigest: sourceInvariants.digest,
      sourceAclDigest: sha256(stableJson(sourceInvariants.value.acl)),
      sourceForecastSettings: snapshot.forecastSettings,
      sourceForecastSettingsDigest: sourceInvariants.value.forecastSettings.combinedDigest,
      ddl: { pid: session.pid, backendStart: session.backendStart, applicationName: session.applicationName },
    };
    await writeDurableJson(operationPath, operation);
    await writeDurableJson(statePath, { ...operation, status: "replacement-started" });
    await appendAudit(auditPath, { event: "replacement-started", operation });
    await assertNoCompetingDdl(config, session.pid);
    const created = await createOwnedIndexInSession({
      config,
      manifest,
      admission,
      admissionSample,
      operationDirectory,
      operation,
      session,
      indexSpec: replacement.target,
      ownershipName: "target-ownership.json",
      controls,
    });
    // inject one disposable post-create fault
    await controls.afterTargetCreated?.({ operation, created });
    const beforeDrop = await captureIndexInventory(config, replacement.target);
    const beforeState = validateReplacementInventory(beforeDrop, manifest, config.postgres.tableOwner, "intermediate", snapshot.indexInventory);
    requireCondition(beforeState.predecessor.indexOid === operation.predecessor.indexOid
      && stableJson(beforeState.predecessor) === stableJson(operation.predecessor), "predecessor changed before source drop");
    validateReplacementOwnership(created.ownership, beforeState.target, {
      operationId,
      clusterDigest: operation.clusterDigest,
      databaseOid: session.databaseOid,
      tableOid: session.tableOid,
    });
    await writeDurableJson(statePath, {
      ...operation,
      status: "source-drop-started",
      sourceDropStartedAt: new Date().toISOString(),
      targetOwnership: created.ownership,
    });
    await appendAudit(auditPath, { event: "source-drop-started", predecessor: operation.predecessor, targetOwnership: created.ownership });
    dropStarted = true;
    // inject one disposable pre-drop fault
    await controls.afterSourceDropStarted?.({ operation, created });
    await runSessionDdl(session, replacement.predecessor.dropSql, "WEATHER_SOURCE_DROPPED", controls.sourceDropTimeoutMilliseconds ?? 60_000, "predecessor drop");
    // inject one disposable post-drop fault
    await controls.afterSourceDropped?.({ operation, created });
    const finalInventory = await captureIndexInventory(config, replacement.target);
    const finalState = validateReplacementInventory(finalInventory, manifest, config.postgres.tableOwner, "final", snapshot.indexInventory);
    validateReplacementOwnership(created.ownership, finalState.target, {
      operationId,
      clusterDigest: operation.clusterDigest,
      databaseOid: session.databaseOid,
      tableOid: session.tableOid,
    });
    const health = await (controls.monitorResources ?? monitorBuildResources)(config, admission, manifest.admission, admissionSample, admission.computed);
    requireCondition(health.reasons.length === 0, "replacement final health check failed", health);
    // inject one disposable receipt fault
    await controls.beforeCompletionReceipt?.({ operation, created, health });
    const result = {
      schema: "weather-page-index-result/v2",
      operationId,
      status: "replaced",
      completedAt: new Date().toISOString(),
      predecessor: operation.predecessor,
      target: finalState.target,
      targetOwnershipPath: created.ownershipPath,
      sourceInventoryDigest: operation.sourceInventoryDigest,
      finalInventoryDigest: sha256(stableJson(finalInventory)),
      health,
      samples: created.samples,
    };
    const completion = await settleDdlSession(config, session);
    settled = true;
    requireCondition(completion.code === 0, "replacement DDL lock session failed", completion);
    const after = await captureRuntimeSnapshot(config, replacement.target, manifest.restoreContext.migrations, snapshotOptions);
    assertUnchangedRuntime(snapshot, after);
    validateReplacementInventory(after.indexInventory, manifest, config.postgres.tableOwner, "final", snapshot.indexInventory);
    result.completedAt = new Date().toISOString();
    result.afterIdentities = after.identities;
    result.finalInventoryDigest = sha256(stableJson(after.indexInventory));
    const resultDigest = sha256(stableJson(result));
    const completionReceipt = {
      schema: "weather-page-index-completion/v1",
      operationId,
      completedAt: new Date().toISOString(),
      resultDigest,
      beforeIdentities: snapshot.identities,
      afterIdentities: after.identities,
    };
    await appendAudit(auditPath, { event: "completion-prepared", result, completionReceipt });
    await writeDurableJson(resultPath, result);
    await writeDurableJson(statePath, { ...operation, status: "completed", completedAt: result.completedAt, resultDigest });
    await writeDurableJson(join(operationDirectory, "completion.json"), completionReceipt);
    return result;
  } catch (error) {
    let compensationFailure;

    // compensate every owned target or started source drop before returning failure
    if (operation !== undefined) {
      try {
        await compensateReplacement({ config, manifest, admission, admissionSample, operationDirectory, operation, session, controls });
        settled = true;
      } catch (compensationError) {
        compensationFailure = compensationError instanceof Error ? compensationError.message : String(compensationError);
      }
    }

    // close one session not consumed by compensation
    if (!settled) {
      await settleDdlSession(config, session, { cancel: dropStarted });
    }

    const failure = {
      schema: "weather-page-index-result/v2",
      operationId,
      status: compensationFailure === undefined ? "failed-compensated" : "failed-compensation-incomplete",
      completedAt: new Date().toISOString(),
      failure: error instanceof Error ? error.message : String(error),
      compensationFailure,
    };
    await appendAudit(auditPath, { event: "failed", failure });
    await writeDurableJson(resultPath, failure);
    await writeDurableJson(statePath, { ...operation, status: failure.status, completedAt: failure.completedAt, failure });
    throw new ContractError(failure.failure, failure);
  }
}

// revert one completed replacement with fresh admission
export async function dropOwnedIndex({ config, manifest, operationDirectory, snapshot, admission, controls = {} }) {
  const { value: operation } = await readJson(join(operationDirectory, "operation.json"));
  const statePath = join(operationDirectory, "replacement-state.json");
  const auditPath = join(operationDirectory, "audit.jsonl");
  requireCondition(operation.schema === "weather-page-index-operation/v2" && operation.operationId === basename(operationDirectory), "replacement operation record changed");
  validateReplacementInventory(snapshot.indexInventory, manifest, config.postgres.tableOwner, "final", operation.sourceInventory);
  const revertStartedAt = new Date().toISOString();
  await writeDurableJson(statePath, { ...operation, status: "revert-started", revertStartedAt });
  await appendAudit(auditPath, { event: "revert-started", operationId: operation.operationId, revertStartedAt });

  try {
    const receipt = await compensateReplacement({
      config,
      manifest,
      admission,
      admissionSample: admission.current,
      operationDirectory,
      operation,
      session: undefined,
      controls,
    });
    // inject one disposable pre-receipt revert fault
    await controls.beforeRevertReceipt?.({ operation, receipt });
    const revert = { ...receipt, schema: "weather-page-index-revert/v2" };
    const revertDigest = sha256(stableJson(revert));
    await writeDurableJson(join(operationDirectory, "revert.json"), revert);
    await appendAudit(auditPath, { event: "reverted", revert });
    await writeDurableJson(statePath, {
      ...operation,
      status: "reverted",
      completedAt: receipt.completedAt,
      revertDigest,
    });
    return receipt;
  } catch (error) {
    const failure = {
      schema: "weather-page-index-revert-result/v1",
      operationId: operation.operationId,
      status: "revert-compensation-incomplete",
      completedAt: new Date().toISOString(),
      failure: error instanceof Error ? error.message : String(error),
    };
    await appendAudit(auditPath, { event: "revert-compensation-incomplete", failure });
    await writeDurableJson(join(operationDirectory, "revert-result.json"), failure);
    await writeDurableJson(statePath, { ...operation, status: failure.status, completedAt: failure.completedAt, failure });
    throw new ContractError(failure.failure, failure);
  }
}

// list restore databases with first-observed timestamps
export async function listRestoreDatabases(config, database, observed = new Map(), dataMountSource = undefined) {
  const sql = `SELECT COALESCE(json_agg(json_build_object(
    'name', database_relation.datname,
    'oid', database_relation.oid::text,
    'owner', pg_get_userbyid(database_relation.datdba)
  ) ORDER BY database_relation.datname), '[]'::json)
  FROM pg_database database_relation;`;
  const databases = await queryPsqlJson(config, sql, database);
  const now = new Date().toISOString();

  // attach observer-owned first-seen times
  for (const entry of databases) {
    const oid = String(entry.oid);

    // bind the first observation once
    if (!observed.has(oid)) {
      observed.set(oid, now);
    }

    entry.firstObservedAt = observed.get(oid);

    // capture the actual database directory birth interval
    if (dataMountSource !== undefined) {
      const databaseDirectory = await lstat(join(dataMountSource, "base", oid));
      const createdAtMilliseconds = databaseDirectory.birthtimeMs > 0
        ? databaseDirectory.birthtimeMs
        : databaseDirectory.ctimeMs;
      entry.createdAt = new Date(createdAtMilliseconds).toISOString();
    }
  }

  return databases;
}

// capture restored data and policy invariants
export async function captureCandidateInvariants(config, database) {
  const value = await queryPsqlJson(config, `SELECT json_build_object(
    'ledger', COALESCE((SELECT json_agg(json_build_object('name', name, 'checksum', checksum) ORDER BY name) FROM schema_migrations), '[]'::json),
    'counts', json_build_object(
      'sites', (SELECT count(*)::text FROM sites),
      'sources', (SELECT count(*)::text FROM sources),
      'runs', (SELECT count(*)::text FROM ingestion_runs),
      'records', (SELECT count(*)::text FROM weather_records)
    ),
    'sample', (SELECT md5(COALESCE(string_agg(id::text || ':' || content_hash, ',' ORDER BY id), 'empty')) FROM (SELECT id, content_hash FROM weather_records ORDER BY id LIMIT 10) sample),
    'settings', (SELECT json_object_agg(name, setting ORDER BY name) FROM pg_settings WHERE name = ANY(ARRAY[${SETTING_NAMES.map(quoteLiteral).join(",")}])),
    'roles', (SELECT json_agg(json_build_object('name', rolname, 'superuser', rolsuper, 'createDb', rolcreatedb, 'createRole', rolcreaterole, 'replication', rolreplication) ORDER BY rolname) FROM pg_roles WHERE rolname LIKE 'weather_%'),
    'grants', (SELECT json_agg(json_build_object('grantee', grantee, 'schema', table_schema, 'table', table_name, 'privilege', privilege_type) ORDER BY grantee, table_schema, table_name, privilege_type) FROM information_schema.role_table_grants WHERE grantee LIKE 'weather_%')
  );`, database);
  return { value, digest: sha256(stableJson(value)) };
}

// capture stable database identities that index DDL must not change
export async function captureRecoveryInvariants(config, database = config.postgres.database, options = {}) {
  const stable = await queryPsqlJson(config, `SELECT json_build_object(
    'database', (SELECT json_build_object(
      'name', current_database(),
      'oid', database_relation.oid::text,
      'owner', pg_get_userbyid(database_relation.datdba),
      'systemIdentifier', (SELECT system_identifier::text FROM pg_control_system()),
      'timelineId', (SELECT timeline_id FROM pg_control_checkpoint()),
      'tableOid', table_relation.oid::text,
      'tableOwner', pg_get_userbyid(table_relation.relowner)
    ) FROM pg_database database_relation
      JOIN pg_class table_relation ON table_relation.oid = 'public.weather_records'::regclass
      WHERE database_relation.datname = current_database()),
    'ledger', COALESCE((SELECT json_agg(json_build_object('name', name, 'checksum', checksum) ORDER BY name) FROM schema_migrations), '[]'::json),
    'settings', (SELECT json_object_agg(name, setting ORDER BY name) FROM pg_settings WHERE name = ANY(ARRAY[${SETTING_NAMES.map(quoteLiteral).join(",")}]))
  );`, database);
  const acl = await queryPsqlJson(config, aclSnapshotSql(), database);
  const forecastSettings = config.restoreOnly === true
    ? options.forecastSettings
    : await captureForecastSettings(config.paths.forecastSettings);
  requireCondition(forecastSettings?.combinedDigest !== undefined, "recovery forecast settings identity is missing");
  const value = { ...stable, acl, forecastSettings };
  return { value, digest: sha256(stableJson(value)) };
}

// validate every stable recovery identity before publishing compensation
export function validateRecoveryInvariantIdentity(invariants, operation) {
  const aclDigest = sha256(stableJson(invariants.value.acl));
  const forecastSettingsDigest = invariants.value.forecastSettings?.combinedDigest;
  requireCondition(aclDigest === operation.sourceAclDigest, "compensation changed the full ACL surface");
  requireCondition(forecastSettingsDigest === operation.sourceForecastSettingsDigest, "compensation changed administrator forecast settings");
  requireCondition(invariants.digest === operation.sourceInvariantDigest, "compensation changed data, ledger, ACLs, or settings");
  return { aclDigest, forecastSettingsDigest };
}

// revalidate and remove one bound restore database
export async function cleanupRestoreCandidate(config, baseDatabase, before, bound) {
  const after = await listRestoreDatabases(config, baseDatabase);
  const current = [];

  // select the exact bound name
  for (const database of after) {
    // collect matching names only
    if (database.name === bound.name) {
      current.push(database);
    }
  }

  requireCondition(current.length === 1, "bound restore candidate is missing or duplicated");
  requireCondition(String(current[0].oid) === String(bound.oid) && current[0].owner === bound.owner, "bound restore candidate identity changed");

  // prove the database was absent before
  for (const database of before) {
    requireCondition(String(database.oid) !== String(bound.oid), "bound restore candidate existed before the operation");
  }

  await queryPsql(config, `DROP DATABASE ${quoteIdentifier(bound.name)} WITH (FORCE);`, baseDatabase);
  const final = await listRestoreDatabases(config, baseDatabase);
  // prove the exact oid disappeared
  requireCondition(!final.some((database) => String(database.oid) === String(bound.oid)), "bound restore candidate cleanup failed");
}

// run the start-gated legacy restore wrapper
export async function runRestoreWrapper({ config, manifest, releaseManifest, contextRoot, mode, operationDirectory, admissionPath, packageIdentity }) {
  requireCondition(mode === "verify-restore" || mode === "reconcile-restore", "unsupported restore wrapper mode");
  requireCondition(typeof admissionPath === "string" && packageIdentity !== undefined, "restore wrapper requires disposable admission evidence");
  const restoreConfig = config.restore;
  requireCondition(restoreConfig !== null && typeof restoreConfig === "object", "restore runtime config is missing");
  requireCondition(SAFE_IDENTIFIER_PATTERN.test(restoreConfig.owner), "restore owner is unsafe");
  requireCondition(await realpath(config.paths.installRoot) === await realpath(contextRoot), "restore install root does not match the immutable context");
  await validateRestoreContextRoot(contextRoot, releaseManifest.restoreContext, restoreConfig.syntheticSecretFiles ?? []);
  const restoreInputs = {
    fixtureEnv: await validatePrivateRestoreInput(restoreConfig.fixtureEnv, "restore fixture environment", 64 * 1024),
    identity: await validatePrivateRestoreInput(restoreConfig.identity, "restore age identity", 1024 * 1024),
    archive: await validatePrivateRestoreInput(restoreConfig.archive, "restore archive"),
    archiveChecksum: await validatePrivateRestoreInput(`${restoreConfig.archive}.sha256`, "restore archive checksum", 1024),
  };
  const container = await inspectContainer(restoreConfig.postgresContainer);
  const isolation = validateDisposableMount(container, restoreConfig, manifest.restoreContext);
  const canonicalDataRoot = await realpath(restoreConfig.weatherPostgresDir);
  requireCondition(canonicalDataRoot === resolve(restoreConfig.weatherPostgresDir) && canonicalDataRoot === isolation.mountSource, "restore postgres data root is not canonical");
  const dataRootInfo = await lstat(canonicalDataRoot);
  requireCondition(dataRootInfo.isDirectory() && !dataRootInfo.isSymbolicLink(), "restore postgres data root is unsafe");
  isolation.mountDevice = dataRootInfo.dev.toString();
  isolation.mountInode = dataRootInfo.ino.toString();
  const coexistence = await captureRestoreCoexistence(config, restoreInputs, canonicalDataRoot);
  const baseDatabase = restoreConfig.fixtureDatabase;
  requireCondition(SAFE_IDENTIFIER_PATTERN.test(baseDatabase), "fixture database name is unsafe");
  const observed = new Map();
  const before = await listRestoreDatabases({ ...config, postgres: { ...config.postgres, container: restoreConfig.postgresContainer } }, baseDatabase, observed, isolation.mountSource);
  const prefix = `${baseDatabase.slice(0, 32)}_verify_`;

  // reject preexisting private-prefix candidates
  for (const database of before) {
    requireCondition(!database.name.startsWith(prefix), `preexisting restore candidate blocks rehearsal: ${database.name}`);
  }

  const restorePath = join(contextRoot, "deploy/scripts/restore.sh");
  const childEnvironment = {
    ...process.env,
    WEATHER_COMPOSE_PROJECT_NAME: restoreConfig.composeProject,
    WEATHER_POSTGRES_DIR: restoreConfig.weatherPostgresDir,
    WEATHER_ENV_FILE: restoreConfig.fixtureEnv,
  };
  const child = spawn("sh", [
    "-c",
    "IFS= read -r gate; [ \"$gate\" = GO ]; exec \"$@\"",
    "weather-page-index-restore-supervisor",
    restorePath,
    "verify",
    restoreConfig.archive,
    "--identity",
    restoreConfig.identity,
    "--retain",
    "--env-file",
    restoreConfig.fixtureEnv,
  ], {
    cwd: contextRoot,
    env: childEnvironment,
    stdio: ["pipe", "pipe", "pipe"],
  });
  const startedAtEpochMilliseconds = Date.now();
  const bootId = (await readFile("/proc/sys/kernel/random/boot_id", "utf8")).trim();
  const processStat = await readFile(`/proc/${child.pid}/stat`, "utf8");
  const operation = {
    schema: "weather-page-index-restore-operation/v1",
    operationId: basename(operationDirectory),
    mode,
    status: "start-gated",
    startedAt: new Date(startedAtEpochMilliseconds).toISOString(),
    monotonicNanoseconds: process.hrtime.bigint().toString(),
    childPid: child.pid,
    processStartTicks: processStat.split(" ")[21],
    bootId,
    candidatePattern: `${prefix}<UTC14>_${child.pid}`,
    before,
    isolation,
    coexistence,
    restoreInputs,
    restoreContextDigest: releaseManifest.restoreContext.filesDigest,
  };
  await writeDurableJson(join(operationDirectory, "restore-operation.json"), operation);
  await appendAudit(join(operationDirectory, "audit.jsonl"), { event: "restore-start-gated", operation });
  const stdout = [];
  const stderr = [];
  let childResult;

  // collect legacy standard output
  child.stdout.on("data", (chunk) => {
    stdout.push(chunk);
  });

  // collect legacy standard error
  child.stderr.on("data", (chunk) => {
    stderr.push(chunk);
  });

  // observe the gated child lifecycle
  const childCompletion = new Promise((resolvePromise, rejectPromise) => {
    // reject child spawn failures
    child.on("error", rejectPromise);

    // record child completion
    child.on("close", (code, signal) => {
      childResult = { code, signal };
      resolvePromise(childResult);
    });
  });
  child.stdin.end("GO\n");
  let bound;
  let ambiguous;
  let after = before;

  // observe candidates during and after child execution
  while (childResult === undefined) {
    after = await listRestoreDatabases({ ...config, postgres: { ...config.postgres, container: restoreConfig.postgresContainer } }, baseDatabase, observed, isolation.mountSource);
    const newCandidates = [];

    // collect private-prefix absent-before candidates
    for (const database of after) {
      // detect preexisting database oids
      const wasBefore = before.some((prior) => String(prior.oid) === String(database.oid));

      // retain only owned-name candidates
      if (!wasBefore && database.name.startsWith(prefix) && database.name.endsWith(`_${child.pid}`) && database.owner === restoreConfig.owner) {
        newCandidates.push(database);
      }
    }

    // durably bind exactly one candidate
    if (newCandidates.length === 1 && bound === undefined) {
      bound = {
        name: newCandidates[0].name,
        oid: String(newCandidates[0].oid),
        owner: newCandidates[0].owner,
        firstObservedAt: newCandidates[0].firstObservedAt,
      };
      await writeDurableJson(join(operationDirectory, "restore-ownership.json"), {
        schema: "weather-page-index-restore-ownership/v1",
        operationId: basename(operationDirectory),
        capturedAt: new Date().toISOString(),
        ...bound,
        childPid: child.pid,
        bootId,
        processStartTicks: operation.processStartTicks,
        cluster: isolation,
      });
    // reject ambiguous or replaced candidates
    } else if (newCandidates.length > 1 || (bound !== undefined && newCandidates.some((candidate) => String(candidate.oid) !== bound.oid))) {
      ambiguous = "multiple or replaced restore candidates observed";
    }

    await delay(100);
  }

  await childCompletion;
  after = await listRestoreDatabases({ ...config, postgres: { ...config.postgres, container: restoreConfig.postgresContainer } }, baseDatabase, observed, isolation.mountSource);
  const finalExpectation = {
    fixtureDatabase: baseDatabase,
    childPid: child.pid,
    owner: restoreConfig.owner,
    startedAtEpochMilliseconds,
    observedAtEpochMilliseconds: Date.now(),
    clockSkewMilliseconds: 2000,
  };
  let finalBinding;

  try {
    const withTimes = [];

    // normalize actual creation timestamps for pure binding
    for (const database of after) {
      withTimes.push(database);
    }

    finalBinding = bindRestoreCandidate(before, withTimes, finalExpectation);
  } catch (error) {
    // permit a true pre-create failure only
    const newPrefixCandidates = after.filter((database) => !before.some((prior) => String(prior.oid) === String(database.oid)) && database.name.startsWith(prefix));

    // retain ambiguity when an object exists
    if (newPrefixCandidates.length !== 0) {
      ambiguous = error.message;
    }
  }

  // durably bind a candidate first observed after a fast child exit
  if (bound === undefined && finalBinding !== undefined && ambiguous === undefined) {
    bound = {
      name: finalBinding.name,
      oid: finalBinding.oid,
      owner: finalBinding.owner,
      firstObservedAt: finalBinding.createdAt,
    };
    await writeDurableJson(join(operationDirectory, "restore-ownership.json"), {
      schema: "weather-page-index-restore-ownership/v1",
      operationId: basename(operationDirectory),
      capturedAt: new Date().toISOString(),
      ...bound,
      childPid: child.pid,
      bootId,
      processStartTicks: operation.processStartTicks,
      cluster: isolation,
    });
    await appendAudit(join(operationDirectory, "audit.jsonl"), { event: "restore-bound-after-exit", bound });
  }

  // require durable and final binding agreement
  if (bound !== undefined && finalBinding !== undefined) {
    // retain changed bindings without cleanup authority
    if (bound.name !== finalBinding.name || bound.oid !== finalBinding.oid || bound.owner !== finalBinding.owner) {
      ambiguous = "restore binding changed after child exit";
    }
  }

  const childStdout = Buffer.concat(stdout).toString("utf8");
  const childStderr = Buffer.concat(stderr).toString("utf8");
  let wrapperFailure = ambiguous;
  let wrapperFailureDetails;
  let catalog;
  let invariantsBefore;
  let invariantsAfter;
  let candidateAdmission;
  let candidateSnapshot;

  try {
    // validate successful child output and candidate
    if (childResult.code === 0 && wrapperFailure === undefined) {
      requireCondition(bound !== undefined && finalBinding !== undefined, "legacy restore succeeded without a bound candidate");
      requireCondition(parseLegacyRestoreCandidate(childStdout) === bound.name, "legacy success output did not match the bound candidate");
      const candidateConfig = {
        ...config,
        restoreCandidate: true,
        containers: [restoreConfig.postgresContainer],
        monitor: { endpoints: [] },
        postgres: {
          ...config.postgres,
          container: restoreConfig.postgresContainer,
          composeProject: restoreConfig.composeProject,
          database: bound.name,
        },
      };
      candidateSnapshot = await captureRuntimeSnapshot(
        candidateConfig,
        manifest.replacement.target,
        manifest.restoreContext.migrations,
        { restoreContext: releaseManifest.restoreContext },
      );
      requireCondition(candidateSnapshot.database.databaseOid === bound.oid
        && candidateSnapshot.database.databaseOwner === bound.owner
        && candidateSnapshot.database.tableOwner === restoreConfig.owner, "restored candidate database identity differs from its binding");
      candidateAdmission = await loadRestoreAdmission(admissionPath, manifest, packageIdentity, candidateSnapshot, candidateConfig);
      invariantsBefore = await captureCandidateInvariants(candidateConfig, bound.name);
      catalog = await queryPsqlJson(candidateConfig, catalogFingerprintSql(manifest.replacement.target, bound.name), bound.name);

      // reconcile only the exact pre-replacement catalog
      if (mode === "reconcile-restore") {
        validateReplacementInventory(candidateSnapshot.indexInventory, manifest, restoreConfig.owner, "source");
        await runManagedCreate({
          config: candidateConfig,
          manifest,
          admission: candidateAdmission,
          admissionSample: candidateAdmission.current,
          operationDirectory: join(operationDirectory, "reconcile-index"),
          snapshot: candidateSnapshot,
          snapshotOptions: { restoreContext: releaseManifest.restoreContext },
        });
        catalog = await queryPsqlJson(candidateConfig, catalogFingerprintSql(manifest.replacement.target, bound.name), bound.name);
      } else {
        validateReplacementInventory(candidateSnapshot.indexInventory, manifest, restoreConfig.owner, "final");
      }

      validateCatalogFingerprint(catalog, manifest.replacement.target, restoreConfig.owner);
      invariantsAfter = await captureCandidateInvariants(candidateConfig, bound.name);
      requireCondition(invariantsAfter.digest === invariantsBefore.digest, "restore reconciliation changed data, ledger, ACLs, or settings");
      const finalSnapshot = await captureRuntimeSnapshot(
        candidateConfig,
        manifest.replacement.target,
        manifest.restoreContext.migrations,
        { restoreContext: releaseManifest.restoreContext },
      );
      assertUnchangedRuntime(candidateSnapshot, finalSnapshot);
      validateReplacementInventory(finalSnapshot.indexInventory, manifest, restoreConfig.owner, "final", candidateSnapshot.indexInventory);
    } else if (childResult.code !== 0 && wrapperFailure === undefined) {
      wrapperFailure = `legacy restore failed with exit ${childResult.code}`;
    }
  } catch (error) {
    wrapperFailure = error instanceof Error ? error.message : String(error);
    wrapperFailureDetails = error instanceof ContractError ? error.details : undefined;
  }

  // clean only a durably bound candidate
  if (bound !== undefined && ambiguous === undefined) {
    try {
      const cleanupInspect = await inspectContainer(restoreConfig.postgresContainer);
      const cleanupIsolation = validateDisposableMount(cleanupInspect, restoreConfig, manifest.restoreContext);
      requireCondition(cleanupIsolation.containerId === isolation.containerId
        && cleanupIsolation.image === isolation.image
        && cleanupIsolation.mountSource === isolation.mountSource
        && cleanupIsolation.startedAt === isolation.startedAt, "restore cluster identity changed before cleanup");
      await cleanupRestoreCandidate({ ...config, postgres: { ...config.postgres, container: restoreConfig.postgresContainer } }, baseDatabase, before, bound);
    } catch (error) {
      const cleanupFailure = `restore candidate cleanup failed: ${error instanceof Error ? error.message : String(error)}`;
      wrapperFailure = wrapperFailure === undefined ? cleanupFailure : `${wrapperFailure}; ${cleanupFailure}`;
    }
  }

  const result = {
    schema: "weather-page-index-restore-result/v1",
    operationId: basename(operationDirectory),
    mode,
    status: wrapperFailure === undefined ? "passed" : "failed",
    completedAt: new Date().toISOString(),
    child: { ...childResult, stdout: childStdout, stderr: childStderr },
    binding: bound,
    catalog: normalizeCatalogFingerprint(catalog),
    catalogHash: catalog === undefined ? null : catalogFingerprintHash(catalog),
    invariantsBefore: invariantsBefore?.digest,
    invariantsAfter: invariantsAfter?.digest,
    candidateAdmissionDigest: candidateAdmission?.digest,
    candidateIdentities: candidateSnapshot?.identities,
    failure: wrapperFailure,
    failureDetails: wrapperFailureDetails,
  };
  await appendAudit(join(operationDirectory, "audit.jsonl"), { event: "restore-result", result });
  await writeDurableJson(join(operationDirectory, "restore-result.json"), result);
  requireCondition(wrapperFailure === undefined, wrapperFailure ?? "restore wrapper failed", result);
  return result;
}

// parse the closed command line
export function parseArguments(argv) {
  const modes = new Set(["plan", "verify", "sample-ingestion", "build-admission", "apply", "revert", "verify-restore", "reconcile-restore"]);
  requireCondition(argv.length >= 1 && modes.has(argv[0]), "expected plan, verify, sample-ingestion, build-admission, apply, revert, verify-restore, or reconcile-restore");
  const options = { mode: argv[0] };
  const allowedByMode = {
    plan: ["--config"],
    verify: ["--config"],
    "sample-ingestion": ["--config", "--duration-seconds", "--output", "--rehearsal-report", "--rehearsal-digest"],
    "build-admission": ["--config", "--output", "--rehearsal-report", "--rehearsal-digest", "--ingestion-report", "--ingestion-digest"],
    apply: ["--config", "--admission"],
    revert: ["--config", "--operation", "--admission"],
    "verify-restore": ["--config", "--admission", "--restore-context"],
    "reconcile-restore": ["--config", "--admission", "--restore-context"],
  };
  const allowed = new Set(allowedByMode[options.mode]);

  // parse exact option pairs
  for (let position = 1; position < argv.length; position += 2) {
    const name = argv[position];
    const value = argv[position + 1];
    requireCondition(allowed.has(name), `unknown option: ${name}`);
    requireCondition(typeof value === "string" && value.length > 0 && !value.startsWith("--"), `${name} requires a value`);
    // convert one kebab option name
    const key = name.slice(2).replace(/-([a-z])/gu, (_match, letter) => letter.toUpperCase());
    requireCondition(options[key] === undefined, `duplicate option: ${name}`);
    options[key] = value;
  }

  requireCondition(typeof options.config === "string", "--config is required");

  // require admission for mutating database modes
  if (options.mode === "apply" || options.mode === "verify-restore" || options.mode === "reconcile-restore") {
    requireCondition(typeof options.admission === "string", "--admission is required");
  }

  // require prior ownership for revert
  if (options.mode === "revert") {
    requireCondition(typeof options.operation === "string", "--operation is required");
    requireCondition(typeof options.admission === "string", "--admission is required for revert");
  }

  // require immutable restore context for wrappers
  if (options.mode === "verify-restore" || options.mode === "reconcile-restore") {
    requireCondition(typeof options.restoreContext === "string", "--restore-context is required");
  }

  // require the complete read-only ingestion producer inputs
  if (options.mode === "sample-ingestion") {
    requireCondition(typeof options.output === "string", "--output is required");
    requireCondition(typeof options.rehearsalReport === "string", "--rehearsal-report is required");
    requireCondition(typeof options.rehearsalDigest === "string" && HASH_PATTERN.test(options.rehearsalDigest), "--rehearsal-digest is required");
    requireCondition(typeof options.durationSeconds === "string" && /^(?:[6-9]\d{2}|[1-3]\d{3}|4[0-4]\d{2}|4500)$/u.test(options.durationSeconds), "--duration-seconds must be from 600 through 4500");
  }

  // require the complete admission-construction inputs
  if (options.mode === "build-admission") {
    requireCondition(typeof options.output === "string", "--output is required");
    requireCondition(typeof options.rehearsalReport === "string", "--rehearsal-report is required");
    requireCondition(typeof options.rehearsalDigest === "string" && HASH_PATTERN.test(options.rehearsalDigest), "--rehearsal-digest is required");
    requireCondition(typeof options.ingestionReport === "string", "--ingestion-report is required");
    requireCondition(typeof options.ingestionDigest === "string" && HASH_PATTERN.test(options.ingestionDigest), "--ingestion-digest is required");
  }

  return options;
}

// validate private runtime configuration
export async function readRuntimeConfig(path) {
  const canonicalPath = await realpath(path);
  requireCondition(canonicalPath === resolve(path), "runtime config path must be canonical");
  const info = await lstat(canonicalPath);
  requireCondition(info.isFile() && !info.isSymbolicLink(), "runtime config must be a regular file");
  requireCondition((info.mode & 0o077) === 0, "runtime config must be private");
  const { value } = await readJson(canonicalPath);
  requireCondition(value.schema === "weather-page-index-runtime/v1", "unsupported runtime config schema");
  requireCondition(typeof value.restoreOnly === "boolean", "runtime restoreOnly flag is missing");
  requireCondition(typeof value.paths?.stateRoot === "string" && isAbsolute(value.paths.stateRoot), "runtime state root is missing");
  requireCondition(typeof value.paths?.installRoot === "string" && isAbsolute(value.paths.installRoot), "runtime install root is missing");
  requireCondition(typeof value.paths?.forecastSettings === "string" && isAbsolute(value.paths.forecastSettings), "runtime forecast settings path is missing");
  requireCondition(SAFE_IDENTIFIER_PATTERN.test(value.postgres?.database), "runtime database is unsafe");
  requireCondition(SAFE_IDENTIFIER_PATTERN.test(value.postgres?.user), "runtime postgres user is unsafe");
  requireCondition(typeof value.paths?.evidenceRoot === "string" && isAbsolute(value.paths.evidenceRoot), "runtime evidence root is missing");
  const swapSampleMilliseconds = value.admission?.swapSampleMilliseconds;

  // validate only bounded swap observation windows
  if (swapSampleMilliseconds !== undefined) {
    requireCondition(Number.isSafeInteger(swapSampleMilliseconds)
      && swapSampleMilliseconds >= 5000 && swapSampleMilliseconds <= 300_000, "runtime swap sample duration must be a bounded integer");
  }

  // require complete live monitoring inputs
  if (value.restoreOnly !== true) {
    requireCondition(swapSampleMilliseconds === undefined || swapSampleMilliseconds >= 60_000, "live swap sampling must observe at least sixty seconds");
    requireCondition(Array.isArray(value.containers) && value.containers.length === 5, "live container inventory must contain five names");
    requireCondition(typeof value.monitor?.apiContainer === "string" && value.containers.includes(value.monitor.apiContainer), "live API monitor container is missing");
    requireCondition(Array.isArray(value.monitor?.endpoints), "live endpoint monitor inventory is missing");
    requireCondition(stableJson(value.monitor.endpoints) === stableJson([
      { name: "now", path: "/api/v1/sites/ballydidean/current", timeoutMilliseconds: 2000 },
      { name: "forecast", path: "/api/v1/sites/ballydidean/forecast?days=10", timeoutMilliseconds: 2000 },
    ]), "live endpoint monitor must contain exact Now and Forecast probes");
  } else {
    // label shortened isolated samples as diagnostic-only
    requireCondition(swapSampleMilliseconds === undefined || swapSampleMilliseconds >= 60_000
      || value.admission?.diagnosticShortSwapSample === true, "short isolated swap sampling must be labeled diagnostic");
    requireCondition(value.restore?.fixtureOnlyAdmission === undefined || value.restore.fixtureOnlyAdmission === true, "restore fixture admission marker is invalid");
  }

  return value;
}

// compare pre and post immutable runtime identities
export function assertUnchangedRuntime(before, after) {
  const names = ["sourceDigest", "controlDigest", "imageDigest", "ledgerDigest", "settingsDigest", "forecastSettingsDigest", "aclDigest", "databaseDigest"];

  // compare every protected identity
  for (const name of names) {
    requireCondition(before.identities[name] === after.identities[name], `protected runtime identity changed: ${name}`);
  }
}

// load one private hash-bound admission report
export async function loadAdmissionReport(reference, expectedSchema) {
  requireCondition(reference !== null && typeof reference === "object", `${expectedSchema} reference is missing`);
  requireCondition(stableJson(Object.keys(reference).sort()) === stableJson(["capturedAt", "digest", "path", "schema"]), `${expectedSchema} reference fields changed`);
  requireCondition(reference.schema === expectedSchema && HASH_PATTERN.test(reference.digest), `${expectedSchema} reference identity is invalid`);
  requireCondition(Number.isFinite(Date.parse(reference.capturedAt)), `${expectedSchema} capturedAt is invalid`);
  const file = await validatePrivateRestoreInput(reference.path, `${expectedSchema} report`, 16 * 1024 * 1024);
  requireCondition(file.digest === reference.digest, `${expectedSchema} report digest mismatch`);
  const { value } = await readJson(file.path);
  requireCondition(value.contractVersion === expectedSchema, `${expectedSchema} report schema mismatch`);
  return value;
}

// validate one measured rehearsal report
export function validateRehearsalReport(report, manifest, options = {}) {
  requireCondition(report?.contractVersion === "weather-page-index-rehearsal/v1", "rehearsal report schema mismatch");
  const fixtureOnly = options.allowFixtureOnly === true && report.fixtureOnly === true;
  requireCondition(report.fixtureOnly !== true || fixtureOnly, "fixture rehearsal is restricted to isolated restore validation");
  requireCondition(!fixtureOnly || report.qualification === false, "fixture rehearsal must be non-qualifying");
  const measured = report.admission;
  requireCondition(measured?.contractVersion === "weather-page-index-rehearsal-admission/v1", "rehearsal admission schema mismatch");
  requireCondition(measured.sourceIdentity?.tag === manifest.restoreContext.tag
    && measured.sourceIdentity?.commit === manifest.restoreContext.commit
    && measured.sourceIdentity?.tree === manifest.restoreContext.tree
    && HASH_PATTERN.test(measured.sourceIdentity?.migrationHistorySha256), "rehearsal source identity mismatch");
  requireCondition(measured.postgres?.major === 17, "rehearsal must use PostgreSQL 17");
  requireCondition(measured.index?.name === manifest.replacement.target.name
    && normalizeIndexDefinition(measured.index?.sql) === normalizeIndexDefinition(manifest.replacement.target.createSql)
    && measured.index?.maintenanceWorkMemBytes === 64 * 1024 * 1024
    && measured.index?.maxParallelMaintenanceWorkers === 0
    && measured.index?.lockTimeoutMs === 5000
    && measured.index?.statementTimeoutMs === 2_700_000, "rehearsal index build settings mismatch");
  const replacement = measured.replacement;
  requireCondition(replacement?.target === sha256(stableJson(manifest.replacement.target))
    && replacement.predecessor === sha256(stableJson(manifest.replacement.predecessor))
    && replacement.covering === sha256(stableJson(manifest.replacement.covering))
    && replacement.protected === sha256(stableJson(manifest.replacement.protected)), "rehearsal replacement manifest identity mismatch");
  requireCondition(HASH_PATTERN.test(replacement.originalCatalogSha256)
    && HASH_PATTERN.test(replacement.finalCatalogSha256)
    && replacement.originalCatalogSha256 !== replacement.finalCatalogSha256
    && replacement.restoredCatalogSha256 === replacement.originalCatalogSha256, "rehearsal replacement catalog transitions are incomplete");
  requireCondition(Number.isSafeInteger(measured.distribution?.total) && measured.distribution.total > 0, "rehearsal distribution is missing");
  requireCondition(Number.isSafeInteger(measured.base?.rows) && measured.base.rows > 0
    && Number.isSafeInteger(measured.base?.heapBytes) && measured.base.heapBytes > 0
    && Array.isArray(measured.base?.rowShapes) && measured.base.rowShapes.length > 0, "rehearsal base scale is missing");
  const valid = measured.validBuild;
  requireCondition(Number.isFinite(valid?.elapsedMs) && valid.elapsedMs > 0
    && Number.isSafeInteger(valid?.indexBytesHighWater) && valid.indexBytesHighWater > 0
    && Number.isSafeInteger(valid?.tempBytesHighWater) && valid.tempBytesHighWater >= 0
    && Number.isSafeInteger(valid?.walGeneratedBytes) && valid.walGeneratedBytes > 0
    && Number.isSafeInteger(valid?.walRetainedBytesHighWater) && valid.walRetainedBytesHighWater >= 0
    && Number.isSafeInteger(valid?.inodesHighWater) && valid.inodesHighWater > 0, "valid rehearsal build peaks are incomplete");
  const resources = measured.replacementResources;
  requireCondition(stableJson(Object.keys(resources ?? {}).sort()) === stableJson(["predecessorCancelled", "predecessorValid", "targetCancelled", "targetValid"]), "replacement component resource envelopes are incomplete");

  // validate all four component envelopes
  for (const [name, peaks] of Object.entries(resources)) {
    requireCondition(stableJson(Object.keys(peaks).sort()) === stableJson(REPLACEMENT_PEAK_FIELDS)
      && REPLACEMENT_PEAK_FIELDS.every((field) => Number.isSafeInteger(peaks[field]) && peaks[field] >= 0)
      && peaks.walGeneratedBytes > 0 && peaks.inodesHighWater > 0
      && (name.endsWith("Cancelled") || peaks.indexBytesHighWater > 0), `replacement component peaks are invalid: ${name}`);
  }

  const combined = {};

  // derive the whole replacement component-wise envelope
  for (const field of REPLACEMENT_PEAK_FIELDS) {
    combined[field] = Math.max(resources.targetValid[field], resources.targetCancelled[field])
      + Math.max(resources.predecessorValid[field], resources.predecessorCancelled[field]);
  }

  requireCondition(REPLACEMENT_PEAK_FIELDS.every((field) => valid[field] === combined[field]), "combined replacement build peaks differ from component envelopes");
  const failed = measured.failedBuild;
  const failedPeaks = failed?.resourcePeaks;
  requireCondition(failed?.kind === "owned-backend-cancellation" && failed.errorCode === "57014" && failed.objectAbsent === true
    && failedPeaks !== null && typeof failedPeaks === "object"
    && stableJson(Object.keys(failedPeaks).sort()) === stableJson(["indexBytesHighWater", "inodesHighWater", "tempBytesHighWater", "walGeneratedBytes", "walRetainedBytesHighWater"])
    && Number.isSafeInteger(failedPeaks.indexBytesHighWater) && failedPeaks.indexBytesHighWater >= 0
    && Number.isSafeInteger(failedPeaks.tempBytesHighWater) && failedPeaks.tempBytesHighWater >= 0
    && Number.isSafeInteger(failedPeaks.walGeneratedBytes) && failedPeaks.walGeneratedBytes > 0
    && Number.isSafeInteger(failedPeaks.walRetainedBytesHighWater) && failedPeaks.walRetainedBytesHighWater >= 0
    && Number.isSafeInteger(failedPeaks.inodesHighWater) && failedPeaks.inodesHighWater > 0, "failed or cancelled build resource peaks are missing");
  requireCondition(report.writes?.pass === true
    && (fixtureOnly ? report.writes.fixtureContract === true && report.writes.measured === false : report.writes.fixtureContract !== true)
    && Number.isFinite(report.writes?.p95Ratio)
    && report.writes.p95Ratio <= 1.1, "measured fixture write p95 gate failed");
  const forecast = measured.forecastReplay;
  requireCondition((fixtureOnly ? forecast?.fixtureContract === true && forecast.measured === false : forecast?.measured === true)
    && forecast.pass === true && forecast.outcomesEqual === true
    && Number.isFinite(forecast.p95Ratio) && forecast.p95Ratio > 0 && forecast.p95Ratio <= 1.1, "forecast replay gate failed");

  // validate both forecast replay phases
  for (const phase of [forecast.baseline, forecast.candidate]) {
    requireCondition(phase?.batchCount === 200 && Array.isArray(phase.durationsMs) && phase.durationsMs.length === phase.batchCount
      && phase.durationsMs.every((duration) => Number.isFinite(duration) && duration > 0)
      && phase.p95Ms === nearestRankPercentile(phase.durationsMs, 0.95)
      && Array.isArray(phase.outcomes) && phase.outcomes.length === phase.batchCount
      && HASH_PATTERN.test(phase.outcomeSha256) && sha256(JSON.stringify(phase.outcomes)) === phase.outcomeSha256, "forecast replay measurements are incomplete");
  }

  requireCondition(Math.abs(forecast.candidate.p95Ms / forecast.baseline.p95Ms - forecast.p95Ratio) <= Number.EPSILON * Math.max(1, Math.abs(forecast.p95Ratio)), "forecast replay p95 ratio differs");
  requireCondition(report.pass === true, "rehearsal report did not pass");
  return report;
}

// bind one genuine mixed-ingestion rehearsal envelope
export function deriveBurstReplayEvidence(rehearsal, rehearsalDigest, options = {}) {
  requireCondition(HASH_PATTERN.test(rehearsalDigest), "rehearsal digest is invalid");
  const burst = rehearsal?.admission?.burstReplay;
  const fixtureOnly = options.allowFixtureOnly === true && rehearsal.fixtureOnly === true;
  requireCondition((fixtureOnly ? burst?.fixtureContract === true && burst.measured === false : burst?.measured === true)
    && burst?.pass === true && burst?.outcomesEqual === true, "rehearsal mixed-ingestion replay did not pass");
  requireCondition(Number.isFinite(burst.p95Ratio) && burst.p95Ratio > 0 && burst.p95Ratio <= 1.1, "rehearsal mixed-ingestion p95 ratio is invalid");
  const phases = [burst.baseline, burst.candidate];

  // validate both measured replay phases
  for (const phase of phases) {
    requireCondition(Number.isSafeInteger(phase?.batchCount) && phase.batchCount > 0, "rehearsal mixed-ingestion batch count is invalid");
    requireCondition(Number.isSafeInteger(phase.batchMix?.forecast) && phase.batchMix.forecast > 0
      && Number.isSafeInteger(phase.batchMix?.tideObservation) && phase.batchMix.tideObservation > 0
      && Number.isSafeInteger(phase.batchMix?.tidePrediction) && phase.batchMix.tidePrediction > 0
      && Number.isSafeInteger(phase.batchMix?.physical) && phase.batchMix.physical > 0, "rehearsal mixed-ingestion batch composition is incomplete");
    requireCondition(Array.isArray(phase.durationsMs) && phase.durationsMs.length === phase.batchCount
      && phase.durationsMs.every((duration) => Number.isFinite(duration) && duration > 0), "rehearsal mixed-ingestion durations are incomplete");
    requireCondition(Array.isArray(phase.outcomes) && phase.outcomes.length === phase.batchCount
      && HASH_PATTERN.test(phase.outcomeSha256), "rehearsal mixed-ingestion outcomes are incomplete");

    // bind each replay outcome to its declared batch mix
    for (const outcome of phase.outcomes) {
      requireCondition(Number(outcome.forecast) === phase.batchMix.forecast
        && Number(outcome.tide_observation) === phase.batchMix.tideObservation
        && Number(outcome.tide_prediction) === phase.batchMix.tidePrediction
        && Number(outcome.physical) === phase.batchMix.physical, "rehearsal mixed-ingestion outcome differs from its batch mix");
    }

    requireCondition(sha256(JSON.stringify(phase.outcomes)) === phase.outcomeSha256, "rehearsal mixed-ingestion outcome digest mismatch");
    requireCondition(Number.isFinite(phase.p95Ms) && phase.p95Ms > 0
      && phase.p95Ms === nearestRankPercentile(phase.durationsMs, 0.95)
      && Number.isSafeInteger(phase.generatedWalBytes) && phase.generatedWalBytes > 0
      && Number.isSafeInteger(phase.relationBytesDelta) && phase.relationBytesDelta >= 0, "rehearsal mixed-ingestion resource envelope is incomplete");
  }

  requireCondition(stableJson(burst.baseline.batchMix) === stableJson(burst.candidate.batchMix), "rehearsal mixed-ingestion batch mixes differ");
  const measuredRatio = burst.candidate.p95Ms / burst.baseline.p95Ms;
  requireCondition(Math.abs(measuredRatio - burst.p95Ratio) <= Number.EPSILON * Math.max(1, Math.abs(measuredRatio)), "rehearsal mixed-ingestion p95 ratio differs");
  return {
    schema: "weather-page-ingestion-burst-evidence/v1",
    coverageOrigin: fixtureOnly ? "fixture-contract" : "disposable-replay",
    measured: !fixtureOnly,
    ...(fixtureOnly ? { fixtureContract: true } : {}),
    rehearsalDigest,
    burstDigest: sha256(stableJson(burst)),
    batchCount: burst.candidate.batchCount,
    batchMix: burst.candidate.batchMix,
    baselineP95Milliseconds: burst.baseline.p95Ms,
    candidateP95Milliseconds: burst.candidate.p95Ms,
    p95Ratio: burst.p95Ratio,
    baselineWalBytes: String(burst.baseline.generatedWalBytes),
    candidateWalBytes: String(burst.candidate.generatedWalBytes),
    baselineRelationBytes: String(burst.baseline.relationBytesDelta),
    candidateRelationBytes: String(burst.candidate.relationBytesDelta),
  };
}

// calculate one nearest-rank percentile
export function nearestRankPercentile(values, percentile) {
  requireCondition(Array.isArray(values) && values.length > 0, "percentile values are missing");
  requireCondition(Number.isFinite(percentile) && percentile > 0 && percentile <= 1, "percentile is invalid");
  const sorted = [...values].sort((left, right) => left - right);
  requireCondition(sorted.every((value) => Number.isFinite(value) && value >= 0), "percentile values are invalid");
  return sorted[Math.max(0, Math.ceil(sorted.length * percentile) - 1)];
}

// collect nonoverlapping two-second ingestion samples
export async function collectIngestionSamples({ durationMilliseconds, capture, intervalMilliseconds = 2000, monotonicNow = () => performance.now(), sleep = delay }) {
  requireCondition(Number.isSafeInteger(durationMilliseconds) && durationMilliseconds >= 600_000 && durationMilliseconds <= 4_500_000, "ingestion sample duration must be from ten through seventy-five minutes");
  requireCondition(intervalMilliseconds === 2000, "ingestion sample cadence must be two seconds");
  requireCondition(typeof capture === "function" && typeof monotonicNow === "function" && typeof sleep === "function", "ingestion sampler callbacks are missing");
  const samples = [];
  const monotonicStartedAt = monotonicNow();
  let nextDueAt = monotonicStartedAt;

  // sample until the measured wall-clock window is complete
  while (samples.length === 0 || Date.parse(samples.at(-1).capturedAt) - Date.parse(samples[0].capturedAt) < durationMilliseconds) {
    // wait for the next fixed cadence after the first sample
    if (samples.length > 0) {
      const beforeSleep = monotonicNow();
      requireCondition(beforeSleep <= nextDueAt, "ingestion sampling missed its two-second cadence");
      await sleep(Math.max(0, nextDueAt - beforeSleep));
      requireCondition(monotonicNow() < nextDueAt + intervalMilliseconds, "ingestion sampling missed its two-second cadence");
    }

    const sample = await capture();
    requireCondition(Number.isFinite(Date.parse(sample?.capturedAt)), "ingestion sample timestamp is invalid");
    samples.push(sample);
    nextDueAt = monotonicStartedAt + samples.length * intervalMilliseconds;
    requireCondition(monotonicNow() <= nextDueAt, "ingestion sampling exceeded its two-second cadence");
    requireCondition(samples.length <= Math.ceil(durationMilliseconds / intervalMilliseconds) + 3, "ingestion wall clock did not advance within its bounded duration");
  }

  return samples;
}

// extract measured worker-loop intervals
export function workerLoopIntervals(samples, startedAt, finishedAt) {
  const startMilliseconds = Date.parse(startedAt);
  const finishMilliseconds = Date.parse(finishedAt);
  requireCondition(Number.isFinite(startMilliseconds) && Number.isFinite(finishMilliseconds) && finishMilliseconds > startMilliseconds, "worker interval bounds are invalid");
  const byWorker = new Map();

  // collect distinct in-window heartbeat advances
  for (const sample of samples) {
    requireCondition(Array.isArray(sample.database?.heartbeats), "worker heartbeat sample is missing");

    // collect every worker heartbeat
    for (const heartbeat of sample.database.heartbeats) {
      const milliseconds = Date.parse(heartbeat.lastLoopAt);
      requireCondition(typeof heartbeat.workerInstance === "string" && heartbeat.workerInstance.length > 0 && Number.isFinite(milliseconds), "worker heartbeat identity is invalid");

      // retain only measured-window loop timestamps
      if (milliseconds >= startMilliseconds && milliseconds <= finishMilliseconds) {
        const values = byWorker.get(heartbeat.workerInstance) ?? new Set();
        values.add(milliseconds);
        byWorker.set(heartbeat.workerInstance, values);
      }
    }
  }

  const intervals = [];

  // calculate each worker's actual loop gaps
  for (const [workerInstance, values] of byWorker.entries()) {
    const timestamps = [...values].sort((left, right) => left - right);

    // compare each consecutive loop
    for (let position = 1; position < timestamps.length; position += 1) {
      intervals.push({ workerInstance, seconds: (timestamps[position] - timestamps[position - 1]) / 1000 });
    }
  }

  requireCondition(intervals.length > 0 && intervals.every((entry) => entry.seconds > 0), "worker loop intervals were not observed");
  return intervals;
}

// capture one read-only production-ingestion sample
export async function captureIngestionSample(config, filesystems) {
  const filesystemSamples = [];

  // sample every derived host filesystem
  for (const filesystem of filesystems) {
    filesystemSamples.push(await sampleFilesystem(filesystem));
  }

  const database = await queryPsqlJson(config, `SELECT json_build_object(
    'capturedAt', clock_timestamp(),
    'walLsn', pg_current_wal_lsn()::text,
    'walBytes', pg_catalog.pg_wal_lsn_diff(pg_current_wal_lsn(), '0/0')::text,
    'heartbeats', COALESCE((SELECT json_agg(json_build_object(
      'workerInstance', worker_instance,
      'lastLoopAt', last_loop_at,
      'lastSuccessAt', last_success_at,
      'currentActivity', current_activity,
      'workerVersion', worker_version,
      'updatedAt', updated_at
    ) ORDER BY worker_instance) FROM worker_heartbeats), '[]'::json)
  );`);
  requireCondition(Number.isFinite(Date.parse(database?.capturedAt)) && Array.isArray(database?.heartbeats), "database ingestion sample is incomplete");
  toBigInt(database.walBytes, "database WAL position");
  return { capturedAt: database.capturedAt, database, filesystems: filesystemSamples };
}

// read actual scheduled runs observed during one baseline window
export async function queryObservedIngestionRuns(config, startedAt, finishedAt) {
  requireCondition(Number.isFinite(Date.parse(startedAt)) && Date.parse(finishedAt) >= Date.parse(startedAt), "ingestion run query bounds are invalid");
  return queryPsqlJson(config, `SELECT COALESCE(json_agg(json_build_object(
    'id', run.id::text,
    'sourceKey', source.source_key,
    'mode', run.mode,
    'state', run.state,
    'adapterVersion', run.adapter_version,
    'startedAt', run.started_at,
    'completedAt', run.completed_at,
    'attempts', run.attempts,
    'recordCount', run.record_count,
    'requestedStart', run.requested_start,
    'requestedEndExclusive', run.requested_end_exclusive,
    'errorCode', run.error_code
  ) ORDER BY run.started_at, run.id), '[]'::json)
  FROM ingestion_runs run
  JOIN sources source ON source.id = run.source_id
  WHERE run.mode = 'scheduled'
    AND run.started_at >= ${quoteLiteral(startedAt)}::timestamptz
    AND run.started_at <= ${quoteLiteral(finishedAt)}::timestamptz
    AND source.source_key IN ('open-meteo-forecast-v4', 'noaa-glendale-tide-predictions-v1');`);
}

// summarize one measured ingestion window
export function summarizeIngestionSamples({ samples, identities, filesystems, observedRuns, burstEvidence }) {
  requireCondition(Array.isArray(samples) && samples.length >= 10, "ingestion samples are incomplete");
  requireCondition(Array.isArray(observedRuns), "observed ingestion runs are missing");
  requireCondition(burstEvidence?.schema === "weather-page-ingestion-burst-evidence/v1" && burstEvidence.measured === true, "mixed-ingestion burst evidence is missing");
  const startedAt = samples[0].capturedAt;
  const finishedAt = samples.at(-1).capturedAt;
  const durationMilliseconds = Date.parse(finishedAt) - Date.parse(startedAt);
  requireCondition(durationMilliseconds >= 600_000, "ingestion baseline is shorter than ten minutes");
  const sampleTimestamps = samples.map((sample) => sample.capturedAt);

  // require a complete nonoverlapping cadence
  for (let position = 1; position < sampleTimestamps.length; position += 1) {
    const gap = Date.parse(sampleTimestamps[position]) - Date.parse(sampleTimestamps[position - 1]);
    requireCondition(gap > 0 && gap < 4000, "ingestion sample cadence has a missing interval");
  }

  const loopIntervals = workerLoopIntervals(samples, startedAt, finishedAt);
  const loopP95Seconds = nearestRankPercentile(loopIntervals.map((entry) => entry.seconds), 0.95);
  const summarizedFilesystems = [];

  // summarize each derived filesystem independently
  for (const filesystem of filesystems) {
    const measurements = samples.map((sample) => {
      const measured = sample.filesystems.find((candidate) => candidate.id === filesystem.id);
      requireCondition(measured !== undefined, `ingestion sample omitted filesystem: ${filesystem.id}`);
      return { capturedAt: sample.capturedAt, ...measured };
    });
    const before = measurements[0];
    const after = measurements.at(-1);
    const beforeBytes = toBigInt(before.allocatedBytes, "ingestion before bytes");
    const afterBytes = toBigInt(after.allocatedBytes, "ingestion after bytes");
    const beforeInodes = toBigInt(before.allocatedInodes, "ingestion before inodes");
    const afterInodes = toBigInt(after.allocatedInodes, "ingestion after inodes");
    const netBytes = afterBytes > beforeBytes ? afterBytes - beforeBytes : 0n;
    const netInodes = afterInodes > beforeInodes ? afterInodes - beforeInodes : 0n;
    summarizedFilesystems.push({
      id: filesystem.id,
      before,
      after,
      netAllocatedBytes: netBytes.toString(),
      netAllocatedInodes: netInodes.toString(),
      bytesPerSecond: ceilDivide(netBytes * 1000n, BigInt(durationMilliseconds)).toString(),
      inodesPerSecond: ceilDivide(netInodes * 1000n, BigInt(durationMilliseconds)).toString(),
      samples: measurements,
    });
  }

  const walSamples = samples.map((sample) => ({
    capturedAt: sample.capturedAt,
    walLsn: sample.database.walLsn,
    walBytes: sample.database.walBytes,
  }));
  const beforeWal = toBigInt(walSamples[0].walBytes, "ingestion before WAL");
  const afterWal = toBigInt(walSamples.at(-1).walBytes, "ingestion after WAL");
  const forecastLiveRunIds = [];
  const tideLiveRunIds = [];

  // classify actual live runs without claiming fixture coverage origin
  for (const run of observedRuns) {
    requireCondition(typeof run?.id === "string" && /^(0|[1-9]\d*)$/u.test(run.id)
      && ["open-meteo-forecast-v4", "noaa-glendale-tide-predictions-v1"].includes(run.sourceKey)
      && run.mode === "scheduled" && Number.isFinite(Date.parse(run.startedAt)), "observed ingestion run is invalid");

    // retain successful live run identities separately
    if (run.state === "succeeded" && Number.isSafeInteger(run.recordCount) && run.recordCount > 0 && Number.isFinite(Date.parse(run.completedAt))) {
      // classify the exact source
      if (run.sourceKey === "open-meteo-forecast-v4") {
        forecastLiveRunIds.push(run.id);
      } else {
        tideLiveRunIds.push(run.id);
      }
    }
  }

  return {
    contractVersion: "weather-page-ingestion-admission/v1",
    identities,
    startedAt,
    finishedAt,
    durationSeconds: durationMilliseconds / 1000,
    cadenceMilliseconds: 2000,
    sampleCount: samples.length,
    events: {
      eventCoverageOrigin: "disposable-replay",
      forecast: burstEvidence.batchMix.forecast > 0,
      tide: burstEvidence.batchMix.tideObservation > 0 && burstEvidence.batchMix.tidePrediction > 0,
      burstEnvelopeMeasured: burstEvidence.measured,
      liveRunIds: { forecast: forecastLiveRunIds, tide: tideLiveRunIds },
      observedLiveRuns: observedRuns,
    },
    burstEvidence,
    loopP95Seconds,
    heartbeat: {
      intervals: loopIntervals,
      samples: samples.map((sample) => ({ capturedAt: sample.capturedAt, heartbeats: sample.database.heartbeats })),
    },
    wal: {
      before: walSamples[0],
      after: walSamples.at(-1),
      generatedBytes: (afterWal > beforeWal ? afterWal - beforeWal : 0n).toString(),
      samples: walSamples,
    },
    filesystems: summarizedFilesystems,
  };
}

// validate one normal-ingestion producer report
export function validateIngestionReport(report, identities, filesystems, rehearsal, rehearsalDigest, options = {}) {
  requireCondition(report?.contractVersion === "weather-page-ingestion-admission/v1", "ingestion report schema mismatch");
  const fixtureOnly = options.allowFixtureOnly === true && rehearsal.fixtureOnly === true && report.fixtureOnly === true;
  requireCondition(stableJson(report.identities) === stableJson(identities), "ingestion report runtime identity mismatch");
  const expectedBurstEvidence = deriveBurstReplayEvidence(rehearsal, rehearsalDigest, { allowFixtureOnly: fixtureOnly });

  // validate one non-qualifying current fixture snapshot without invented history
  if (fixtureOnly) {
    requireCondition(report.qualification === false && report.contractOnly === true
      && report.startedAt === report.finishedAt && report.durationSeconds === 0
      && report.cadenceMilliseconds === null && report.sampleCount === 1
      && Number.isFinite(Date.parse(report.finishedAt)), "fixture ingestion contract is not explicitly non-qualifying");
    requireCondition(stableJson(report.burstEvidence) === stableJson(expectedBurstEvidence), "fixture burst contract differs");
    requireCondition(report.events?.eventCoverageOrigin === "fixture-contract"
      && report.events.forecast === false && report.events.tide === false
      && report.events.burstEnvelopeMeasured === false
      && stableJson(report.events.liveRunIds) === stableJson({ forecast: [], tide: [] })
      && stableJson(report.events.observedLiveRuns) === "[]", "fixture ingestion event contract claims unobserved activity");
    requireCondition(Number.isFinite(report.loopP95Seconds) && report.loopP95Seconds > 0
      && report.heartbeat?.capturedAt === report.finishedAt && Array.isArray(report.heartbeat.heartbeats), "fixture heartbeat snapshot is incomplete");
    requireCondition(report.wal?.capturedAt === report.finishedAt
      && /^(0|[1-9]\d*)$/u.test(report.wal.walBytes), "fixture WAL snapshot is incomplete");
    requireCondition(Array.isArray(report.filesystems) && report.filesystems.length === filesystems.length, "fixture filesystem snapshots are incomplete");
    const expectedIds = filesystems.map((filesystem) => filesystem.id).sort();
    const actualIds = report.filesystems.map((filesystem) => filesystem.id).sort();
    requireCondition(stableJson(actualIds) === stableJson(expectedIds), "fixture filesystem identity set changed");

    // require one actual current sample per fixture device
    for (const filesystem of report.filesystems) {
      requireCondition(filesystem.capturedAt === report.finishedAt
        && filesystem.id.length > 0
        && toBigInt(filesystem.allocatedBytes, "fixture allocated bytes") >= 0n
        && toBigInt(filesystem.allocatedInodes, "fixture allocated inodes") >= 0n
        && toBigInt(filesystem.freeBytes, "fixture free bytes") > 0n
        && toBigInt(filesystem.freeInodes, "fixture free inodes") > 0n
        && filesystem.bytesPerSecond === "0" && filesystem.inodesPerSecond === "0", `fixture filesystem snapshot is invalid: ${filesystem.id}`);
    }
    return report;
  }

  requireCondition(report.fixtureOnly !== true, "ingestion fixture-only marker is invalid");
  const durationMilliseconds = Date.parse(report.finishedAt) - Date.parse(report.startedAt);
  requireCondition(Number.isFinite(durationMilliseconds) && durationMilliseconds >= 600_000 && report.durationSeconds === durationMilliseconds / 1000, "ingestion baseline is shorter than ten minutes or its duration differs");
  requireCondition(report.cadenceMilliseconds === 2000 && Number.isSafeInteger(report.sampleCount) && report.sampleCount >= 301, "ingestion sample cadence is incomplete");
  requireCondition(stableJson(report.burstEvidence) === stableJson(expectedBurstEvidence), "ingestion burst evidence differs from the hash-bound rehearsal");
  requireCondition(report.events?.eventCoverageOrigin === "disposable-replay"
    && report.events?.forecast === (expectedBurstEvidence.batchMix.forecast > 0)
    && report.events?.tide === (expectedBurstEvidence.batchMix.tideObservation > 0 && expectedBurstEvidence.batchMix.tidePrediction > 0)
    && report.events?.burstEnvelopeMeasured === !fixtureOnly, "ingestion event coverage is incomplete");
  requireCondition(Array.isArray(report.heartbeat?.samples) && report.heartbeat.samples.length === report.sampleCount, "ingestion heartbeat samples are incomplete");
  requireCondition(report.heartbeat.samples[0].capturedAt === report.startedAt
    && report.heartbeat.samples.at(-1).capturedAt === report.finishedAt, "ingestion heartbeat bounds differ");

  // require every fixed sampling gap
  for (let position = 1; position < report.heartbeat.samples.length; position += 1) {
    const gap = Date.parse(report.heartbeat.samples[position].capturedAt) - Date.parse(report.heartbeat.samples[position - 1].capturedAt);
    requireCondition(gap > 0 && gap < 4000, "ingestion report contains a missed sampling cadence");
  }

  const measuredIntervals = workerLoopIntervals(
    report.heartbeat.samples.map((sample) => ({ database: { heartbeats: sample.heartbeats } })),
    report.startedAt,
    report.finishedAt,
  );
  requireCondition(stableJson(report.heartbeat.intervals) === stableJson(measuredIntervals), "ingestion heartbeat intervals differ");
  const measuredLoopP95 = nearestRankPercentile(measuredIntervals.map((entry) => entry.seconds), 0.95);
  requireCondition(report.loopP95Seconds === measuredLoopP95 && report.loopP95Seconds > 0, "ingestion loop p95 is missing or differs");
  requireCondition(Array.isArray(report.wal?.samples) && report.wal.samples.length === report.sampleCount
    && stableJson(report.wal.before) === stableJson(report.wal.samples[0])
    && stableJson(report.wal.after) === stableJson(report.wal.samples.at(-1)), "ingestion WAL samples are incomplete");
  const beforeWal = toBigInt(report.wal.before.walBytes, "ingestion before WAL");
  const afterWal = toBigInt(report.wal.after.walBytes, "ingestion after WAL");
  requireCondition(toBigInt(report.wal.generatedBytes, "ingestion generated WAL") === (afterWal > beforeWal ? afterWal - beforeWal : 0n), "ingestion generated WAL differs");
  requireCondition(report.events.liveRunIds !== null && typeof report.events.liveRunIds === "object"
    && Array.isArray(report.events.liveRunIds.forecast) && Array.isArray(report.events.liveRunIds.tide)
    && Array.isArray(report.events.observedLiveRuns), "observed live ingestion runs are missing");
  const classifiedForecast = [];
  const classifiedTide = [];

  // validate and classify separately observed live runs
  for (const run of report.events.observedLiveRuns) {
    requireCondition(typeof run?.id === "string" && /^(0|[1-9]\d*)$/u.test(run.id)
      && ["open-meteo-forecast-v4", "noaa-glendale-tide-predictions-v1"].includes(run.sourceKey)
      && run.mode === "scheduled" && Number.isFinite(Date.parse(run.startedAt))
      && Date.parse(run.startedAt) >= Date.parse(report.startedAt) && Date.parse(run.startedAt) <= Date.parse(report.finishedAt), "observed live ingestion run is invalid");

    // classify only successful nonempty runs
    if (run.state === "succeeded" && Number.isSafeInteger(run.recordCount) && run.recordCount > 0 && Number.isFinite(Date.parse(run.completedAt))) {
      // separate the exact source identities
      if (run.sourceKey === "open-meteo-forecast-v4") {
        classifiedForecast.push(run.id);
      } else {
        classifiedTide.push(run.id);
      }
    }
  }

  requireCondition(stableJson(report.events.liveRunIds.forecast) === stableJson(classifiedForecast)
    && stableJson(report.events.liveRunIds.tide) === stableJson(classifiedTide), "observed live ingestion run classification differs");
  requireCondition(Array.isArray(report.filesystems) && report.filesystems.length === filesystems.length, "ingestion filesystem measurements are incomplete");
  const expectedIds = filesystems.map((filesystem) => filesystem.id).sort();
  const actualIds = report.filesystems.map((filesystem) => filesystem.id).sort();
  requireCondition(stableJson(actualIds) === stableJson(expectedIds), "ingestion filesystem identity set changed");

  // require nonnegative measured rates and samples
  for (const filesystem of report.filesystems) {
    const beforeBytes = toBigInt(filesystem.before?.allocatedBytes, "ingestion before bytes");
    const afterBytes = toBigInt(filesystem.after?.allocatedBytes, "ingestion after bytes");
    const beforeInodes = toBigInt(filesystem.before?.allocatedInodes, "ingestion before inodes");
    const afterInodes = toBigInt(filesystem.after?.allocatedInodes, "ingestion after inodes");
    const netBytes = afterBytes > beforeBytes ? afterBytes - beforeBytes : 0n;
    const netInodes = afterInodes > beforeInodes ? afterInodes - beforeInodes : 0n;
    requireCondition(toBigInt(filesystem.netAllocatedBytes, "ingestion net bytes") === netBytes
      && toBigInt(filesystem.netAllocatedInodes, "ingestion net inodes") === netInodes
      && toBigInt(filesystem.bytesPerSecond, "ingestion byte rate") === ceilDivide(netBytes * 1000n, BigInt(durationMilliseconds))
      && toBigInt(filesystem.inodesPerSecond, "ingestion inode rate") === ceilDivide(netInodes * 1000n, BigInt(durationMilliseconds))
      && Array.isArray(filesystem.samples) && filesystem.samples.length === report.sampleCount
      && stableJson(filesystem.before) === stableJson(filesystem.samples[0])
      && stableJson(filesystem.after) === stableJson(filesystem.samples.at(-1)), `ingestion filesystem measurement is invalid: ${filesystem.id}`);

    // require each exact sample timestamp and device
    for (let position = 0; position < filesystem.samples.length; position += 1) {
      const sample = filesystem.samples[position];
      requireCondition(sample.id === filesystem.id
        && sample.capturedAt === report.heartbeat.samples[position].capturedAt
        && sample.capturedAt === report.wal.samples[position].capturedAt
        && toBigInt(sample.allocatedBytes, "ingestion sampled bytes") >= 0n
        && toBigInt(sample.allocatedInodes, "ingestion sampled inodes") >= 0n, `ingestion filesystem sample differs: ${filesystem.id}`);
    }
  }

  return report;
}

// construct the exact closed admission identity set
export function buildAdmissionIdentities(snapshot, packageIdentity) {
  return {
    aclDigest: snapshot.identities.aclDigest,
    catalogDigest: snapshot.identities.catalogDigest,
    controlDigest: snapshot.identities.controlDigest,
    databaseDigest: snapshot.identities.databaseDigest,
    forecastSettingsDigest: snapshot.identities.forecastSettingsDigest,
    imageDigest: snapshot.identities.imageDigest,
    ledgerDigest: snapshot.identities.ledgerDigest,
    manifestDigest: sha256(packageIdentity.indexBytes),
    packageDigest: packageIdentity.packageDigest,
    settingsDigest: snapshot.identities.settingsDigest,
    sourceDigest: snapshot.identities.sourceDigest,
  };
}

// load and validate one admission document
export async function loadAdmission(path, manifest, packageIdentity, snapshot, config) {
  const { value: admission } = await readJson(path);
  validateAdmissionDigest(admission);
  const expectedIdentities = buildAdmissionIdentities(snapshot, packageIdentity);
  requireCondition(stableJson(admission.identities) === stableJson(expectedIdentities), "admission identity set or value mismatch");
  requireCondition(admission.runtimeConfigDigest === sha256(stableJson(config)), "admission runtime config digest mismatch");
  requireCondition(stableJson(Object.keys(admission.evidence ?? {}).sort()) === stableJson(["ingestion", "rehearsal"]), "admission evidence references are incomplete");
  const rehearsal = validateRehearsalReport(await loadAdmissionReport(admission.evidence.rehearsal, "weather-page-index-rehearsal/v1"), manifest);
  const derivedFilesystems = await deriveFilesystemInventory(config);
  const ingestion = validateIngestionReport(
    await loadAdmissionReport(admission.evidence.ingestion, "weather-page-ingestion-admission/v1"),
    expectedIdentities,
    derivedFilesystems,
    rehearsal,
    admission.evidence.rehearsal.digest,
  );
  requireCondition(admission.evidence.rehearsal.capturedAt === rehearsal.admission.validBuild.finishedAt, "rehearsal reference timestamp is not report-derived");
  requireCondition(admission.evidence.ingestion.capturedAt === ingestion.finishedAt, "ingestion reference timestamp is not report-derived");
  const valid = rehearsal.admission.validBuild;
  const failed = rehearsal.admission.failedBuild.resourcePeaks;
  const indexBytes = Math.max(valid.indexBytesHighWater, failed.indexBytesHighWater);
  const tempBytes = Math.max(valid.tempBytesHighWater, failed.tempBytesHighWater);
  const walBytes = Math.max(valid.walGeneratedBytes + valid.walRetainedBytesHighWater, failed.walGeneratedBytes + failed.walRetainedBytesHighWater);
  const allocatedInodes = Math.max(valid.inodesHighWater, failed.inodesHighWater);
  requireCondition([indexBytes, walBytes, allocatedInodes].every((value) => Number.isSafeInteger(value) && value > 0)
    && Number.isSafeInteger(tempBytes) && tempBytes >= 0, "rehearsal valid/failed peaks are invalid");
  const ingestionById = new Map();

  // bind each ingestion device measurement
  for (const filesystem of ingestion.filesystems) {
    ingestionById.set(filesystem.id, filesystem);
  }

  const expectedFilesystems = [];

  // derive the closed admission filesystem set
  for (const filesystem of derivedFilesystems) {
    const growth = ingestionById.get(filesystem.id);
    requireCondition(growth !== undefined, `ingestion growth is missing for filesystem ${filesystem.id}`);
    expectedFilesystems.push({
      ...filesystem,
      rehearsal: {
        indexBytes: String(indexBytes),
        tempBytes: String(tempBytes),
        walBytes: String(walBytes),
        allocatedInodes: String(allocatedInodes),
      },
      growth: {
        bytesPerSecond: String(growth.bytesPerSecond),
        inodesPerSecond: String(growth.inodesPerSecond),
      },
    });
  }

  requireCondition(stableJson(admission.filesystems) === stableJson(expectedFilesystems), "admission filesystem inventory or measured peaks differ");
  requireCondition(admission.scale.rehearsalRows === String(rehearsal.admission.base.rows)
    && admission.scale.rehearsalHeapBytes === String(rehearsal.admission.base.heapBytes)
    && admission.scale.liveRows === String(snapshot.database.liveRows)
    && admission.scale.liveHeapBytes === String(snapshot.database.liveHeapBytes)
    && admission.scale.columnDistributionDigest === sha256(stableJson(rehearsal.admission.distribution))
    && admission.scale.buildConfigDigest === sha256(stableJson(rehearsal.admission.index)), "admission scale or rehearsal configuration differs");
  requireCondition(admission.host.fixtureWriteP95Ratio === rehearsal.writes.p95Ratio
    && admission.host.baselineP95LoopSeconds === ingestion.loopP95Seconds, "admission measured host gates differ");
  requireCondition(admission.timestamps.rehearsalMeasuredAt === admission.evidence.rehearsal.capturedAt
    && admission.timestamps.ingestionMeasuredAt === admission.evidence.ingestion.capturedAt, "admission evidence timestamps differ");
  const initialSwap = { pages: await readSwapPages(), capturedAt: Date.now() };
  await delay(config.admission?.swapSampleMilliseconds ?? 60_000);
  const current = await captureAdmissionSample(config, admission, snapshot, initialSwap);
  const refreshed = refreshAdmissionLiveSample(admission, current);
  const verdict = evaluateAdmission(refreshed, manifest.admission, current);
  requireCondition(verdict.admitted, "admission refused", verdict);
  return { ...refreshed, current };
}

// rebind measured evidence to one disposable restored candidate
export async function loadRestoreAdmission(path, manifest, packageIdentity, snapshot, config) {
  const { value: template } = await readJson(path);
  validateAdmissionDigest(template);
  const allowFixtureOnly = config.restore?.fixtureOnlyAdmission === true;
  requireCondition(stableJson(Object.keys(template.evidence ?? {}).sort()) === stableJson(["ingestion", "rehearsal"]), "restore admission evidence references are incomplete");
  const rehearsal = validateRehearsalReport(
    await loadAdmissionReport(template.evidence.rehearsal, "weather-page-index-rehearsal/v1"),
    manifest,
    { allowFixtureOnly },
  );
  const ingestionReport = await loadAdmissionReport(template.evidence.ingestion, "weather-page-ingestion-admission/v1");
  requireCondition(stableJson(Object.keys(ingestionReport.identities ?? {}).sort()) === stableJson(ADMISSION_IDENTITY_KEYS), "restore ingestion identity set is incomplete");
  const ingestion = validateIngestionReport(
    ingestionReport,
    ingestionReport.identities,
    ingestionReport.filesystems.map((filesystem) => ({ id: filesystem.id })),
    rehearsal,
    template.evidence.rehearsal.digest,
    { allowFixtureOnly },
  );
  requireCondition(template.evidence.rehearsal.capturedAt === rehearsal.admission.validBuild.finishedAt
    && template.evidence.ingestion.capturedAt === ingestion.finishedAt, "restore admission evidence timestamps differ");
  const valid = rehearsal.admission.validBuild;
  const failed = rehearsal.admission.failedBuild.resourcePeaks;
  const indexBytes = Math.max(valid.indexBytesHighWater, failed.indexBytesHighWater);
  const tempBytes = Math.max(valid.tempBytesHighWater, failed.tempBytesHighWater);
  const walBytes = Math.max(valid.walGeneratedBytes + valid.walRetainedBytesHighWater, failed.walGeneratedBytes + failed.walRetainedBytesHighWater);
  const allocatedInodes = Math.max(valid.inodesHighWater, failed.inodesHighWater);
  requireCondition([indexBytes, walBytes, allocatedInodes].every((value) => Number.isSafeInteger(value) && value > 0)
    && Number.isSafeInteger(tempBytes) && tempBytes >= 0, "restore rehearsal peaks are invalid");
  let maximumBytesPerSecond = 0n;
  let maximumInodesPerSecond = 0n;

  // conservatively bind the largest observed device growth rate
  for (const filesystem of ingestion.filesystems) {
    const bytesPerSecond = toBigInt(filesystem.bytesPerSecond, "restore ingestion byte rate");
    const inodesPerSecond = toBigInt(filesystem.inodesPerSecond, "restore ingestion inode rate");

    // retain the largest byte rate
    if (bytesPerSecond > maximumBytesPerSecond) {
      maximumBytesPerSecond = bytesPerSecond;
    }

    // retain the largest inode rate
    if (inodesPerSecond > maximumInodesPerSecond) {
      maximumInodesPerSecond = inodesPerSecond;
    }
  }

  const filesystems = await deriveFilesystemInventory(config);
  const identities = buildAdmissionIdentities(snapshot, packageIdentity);
  const document = {
    schema: "weather-page-index-admission/v1",
    identities,
    runtimeConfigDigest: sha256(stableJson(config)),
    evidence: template.evidence,
    timestamps: {
      rehearsalMeasuredAt: template.evidence.rehearsal.capturedAt,
      ingestionMeasuredAt: template.evidence.ingestion.capturedAt,
      liveSampledAt: new Date().toISOString(),
    },
    scale: {
      rehearsalRows: String(rehearsal.admission.base.rows),
      rehearsalHeapBytes: String(rehearsal.admission.base.heapBytes),
      liveRows: String(snapshot.database.liveRows),
      liveHeapBytes: String(snapshot.database.liveHeapBytes),
      columnDistributionDigest: sha256(stableJson(rehearsal.admission.distribution)),
      buildConfigDigest: sha256(stableJson(rehearsal.admission.index)),
    },
    buildDurationSeconds: 2 * Number(manifest.operation.buildTimeoutSeconds),
    host: {
      baselineP95LoopSeconds: ingestion.loopP95Seconds,
      fixtureWriteP95Ratio: rehearsal.writes.p95Ratio,
    },
    filesystems: filesystems.map((filesystem) => ({
      ...filesystem,
      rehearsal: {
        indexBytes: String(indexBytes),
        tempBytes: String(tempBytes),
        walBytes: String(walBytes),
        allocatedInodes: String(allocatedInodes),
      },
      growth: {
        bytesPerSecond: maximumBytesPerSecond.toString(),
        inodesPerSecond: maximumInodesPerSecond.toString(),
      },
    })),
  };
  document.computed = calculateAdmission(document, manifest.admission);
  const admission = sealAdmission(document);
  const initialSwap = { pages: await readSwapPages(), capturedAt: Date.now() };
  await delay(config.admission?.swapSampleMilliseconds ?? 60_000);
  const current = await captureAdmissionSample(config, admission, snapshot, initialSwap);
  admission.timestamps.liveSampledAt = current.capturedAt;
  const resealed = sealAdmission(admission);
  const verdict = evaluateAdmission(resealed, manifest.admission, current, new Date(), { expectedArchitecture: arch() });
  requireCondition(verdict.admitted, "restore candidate admission refused", verdict);
  return { ...resealed, current };
}

// validate one new private evidence output path
export async function validateNewEvidenceOutput(config, path) {
  requireCondition(typeof path === "string" && isAbsolute(path) && path === resolve(path), "ingestion output path must be absolute and canonical");
  const evidenceRoot = await realpath(config.paths.evidenceRoot);
  const outputDirectory = await realpath(dirname(path));
  requireCondition(outputDirectory === evidenceRoot || outputDirectory.startsWith(`${evidenceRoot}${sep}`), "ingestion output escapes the evidence root");
  const rootInfo = await lstat(evidenceRoot);
  const directoryInfo = await lstat(outputDirectory);
  requireCondition(rootInfo.uid === 0 && rootInfo.isDirectory() && !rootInfo.isSymbolicLink() && (rootInfo.mode & 0o077) === 0, "evidence root must be private and root-owned");
  requireCondition(directoryInfo.uid === 0 && directoryInfo.isDirectory() && !directoryInfo.isSymbolicLink() && (directoryInfo.mode & 0o077) === 0, "ingestion output directory must be private and root-owned");

  // require both artifacts to be new
  for (const candidate of [path, `${path}.sha256`]) {
    try {
      await lstat(candidate);
      throw new ContractError(`ingestion evidence already exists: ${candidate}`);
    } catch (error) {
      // accept only a missing target
      if (error instanceof ContractError || error?.code !== "ENOENT") {
        throw error;
      }
    }
  }

  return path;
}

// produce one hash-bound read-only production-ingestion baseline
export async function sampleProductionIngestion({ config, manifest, packageIdentity, durationSeconds, outputPath, rehearsalPath, rehearsalDigest }) {
  requireCondition(Number.isSafeInteger(durationSeconds) && durationSeconds >= 600 && durationSeconds <= 4500, "ingestion duration must be from 600 through 4500 seconds");
  requireCondition(HASH_PATTERN.test(rehearsalDigest), "rehearsal digest is invalid");
  await validateNewEvidenceOutput(config, outputPath);
  const rehearsalFile = await validatePrivateRestoreInput(rehearsalPath, "ingestion rehearsal report", 16 * 1024 * 1024);
  requireCondition(rehearsalFile.digest === rehearsalDigest, "ingestion rehearsal digest mismatch");
  const { value: rehearsal } = await readJson(rehearsalFile.path);
  validateRehearsalReport(rehearsal, manifest);
  const burstEvidence = deriveBurstReplayEvidence(rehearsal, rehearsalDigest);
  const indexSpec = manifest.replacement.target;
  const before = await captureRuntimeSnapshot(config, indexSpec, manifest.restoreContext.migrations);
  assertExpectedSnapshot(before, config.expected);
  const filesystems = await deriveFilesystemInventory(config);
  const identities = buildAdmissionIdentities(before, packageIdentity);
  const samples = await collectIngestionSamples({
    durationMilliseconds: durationSeconds * 1000,
    // capture one actual filesystem and database sample
    capture() {
      return captureIngestionSample(config, filesystems);
    },
  });
  const startedAt = samples[0].capturedAt;
  const finishedAt = samples.at(-1).capturedAt;
  const observedRuns = await queryObservedIngestionRuns(config, startedAt, finishedAt);
  const after = await captureRuntimeSnapshot(config, indexSpec, manifest.restoreContext.migrations);
  assertExpectedSnapshot(after, config.expected);
  requireCondition(stableJson(before.identities) === stableJson(after.identities), "runtime identity changed during ingestion sampling");
  requireCondition(stableJson(before.releases) === stableJson(after.releases), "runtime release markers changed during ingestion sampling");
  const packageAfter = await validateInstalledPackage();
  requireCondition(packageAfter.packageDigest === packageIdentity.packageDigest
    && sha256(packageAfter.indexBytes) === sha256(packageIdentity.indexBytes), "installed package changed during ingestion sampling");
  const report = summarizeIngestionSamples({ samples, identities, filesystems, observedRuns, burstEvidence });
  validateIngestionReport(report, identities, filesystems, rehearsal, rehearsalDigest);
  const bytes = Buffer.from(`${JSON.stringify(report, null, 2)}\n`, "utf8");
  const digest = sha256(bytes);
  await writeNewDurableFile(outputPath, bytes);
  await writeNewDurableFile(`${outputPath}.sha256`, Buffer.from(`${digest}  ${basename(outputPath)}\n`, "utf8"));
  return {
    report,
    reference: {
      schema: report.contractVersion,
      path: outputPath,
      digest,
      capturedAt: report.finishedAt,
    },
    checksumPath: `${outputPath}.sha256`,
  };
}

// build one closed live admission from measured evidence
export async function buildLiveAdmission({ config, manifest, packageIdentity, outputPath, rehearsalPath, rehearsalDigest, ingestionPath, ingestionDigest }) {
  requireCondition(HASH_PATTERN.test(rehearsalDigest) && HASH_PATTERN.test(ingestionDigest), "admission evidence digest is invalid");
  await validateNewEvidenceOutput(config, outputPath);
  const rehearsalFile = await validatePrivateRestoreInput(rehearsalPath, "admission rehearsal report", 16 * 1024 * 1024);
  const ingestionFile = await validatePrivateRestoreInput(ingestionPath, "admission ingestion report", 16 * 1024 * 1024);
  requireCondition(rehearsalFile.digest === rehearsalDigest && ingestionFile.digest === ingestionDigest, "admission evidence digest mismatch");
  const { value: rehearsalValue } = await readJson(rehearsalFile.path);
  const { value: ingestionValue } = await readJson(ingestionFile.path);
  const rehearsal = validateRehearsalReport(rehearsalValue, manifest);
  const snapshot = await captureRuntimeSnapshot(config, manifest.replacement.target, manifest.restoreContext.migrations);
  assertExpectedSnapshot(snapshot, config.expected);
  const filesystems = await deriveFilesystemInventory(config);
  const identities = buildAdmissionIdentities(snapshot, packageIdentity);
  const ingestion = validateIngestionReport(ingestionValue, identities, filesystems, rehearsal, rehearsalDigest);
  const evidence = {
    rehearsal: {
      schema: "weather-page-index-rehearsal/v1",
      path: rehearsalFile.path,
      digest: rehearsalDigest,
      capturedAt: rehearsal.admission.validBuild.finishedAt,
    },
    ingestion: {
      schema: "weather-page-ingestion-admission/v1",
      path: ingestionFile.path,
      digest: ingestionDigest,
      capturedAt: ingestion.finishedAt,
    },
  };
  const valid = rehearsal.admission.validBuild;
  const failed = rehearsal.admission.failedBuild.resourcePeaks;
  const indexBytes = Math.max(valid.indexBytesHighWater, failed.indexBytesHighWater);
  const tempBytes = Math.max(valid.tempBytesHighWater, failed.tempBytesHighWater);
  const walBytes = Math.max(valid.walGeneratedBytes + valid.walRetainedBytesHighWater, failed.walGeneratedBytes + failed.walRetainedBytesHighWater);
  const allocatedInodes = Math.max(valid.inodesHighWater, failed.inodesHighWater);
  const ingestionById = new Map(ingestion.filesystems.map((filesystem) => [filesystem.id, filesystem]));
  const document = {
    schema: "weather-page-index-admission/v1",
    identities,
    runtimeConfigDigest: sha256(stableJson(config)),
    evidence,
    timestamps: {
      rehearsalMeasuredAt: evidence.rehearsal.capturedAt,
      ingestionMeasuredAt: evidence.ingestion.capturedAt,
      liveSampledAt: new Date().toISOString(),
    },
    scale: {
      rehearsalRows: String(rehearsal.admission.base.rows),
      rehearsalHeapBytes: String(rehearsal.admission.base.heapBytes),
      liveRows: String(snapshot.database.liveRows),
      liveHeapBytes: String(snapshot.database.liveHeapBytes),
      columnDistributionDigest: sha256(stableJson(rehearsal.admission.distribution)),
      buildConfigDigest: sha256(stableJson(rehearsal.admission.index)),
    },
    buildDurationSeconds: 2 * Number(manifest.operation.buildTimeoutSeconds),
    host: {
      baselineP95LoopSeconds: ingestion.loopP95Seconds,
      fixtureWriteP95Ratio: rehearsal.writes.p95Ratio,
    },
    filesystems: filesystems.map((filesystem) => {
      const growth = ingestionById.get(filesystem.id);
      requireCondition(growth !== undefined, `ingestion growth is missing for filesystem ${filesystem.id}`);
      return {
        ...filesystem,
        rehearsal: {
          indexBytes: String(indexBytes),
          tempBytes: String(tempBytes),
          walBytes: String(walBytes),
          allocatedInodes: String(allocatedInodes),
        },
        growth: {
          bytesPerSecond: String(growth.bytesPerSecond),
          inodesPerSecond: String(growth.inodesPerSecond),
        },
      };
    }),
  };
  document.computed = calculateAdmission(document, manifest.admission);
  const initialSwap = { pages: await readSwapPages(), capturedAt: Date.now() };
  await delay(config.admission?.swapSampleMilliseconds ?? 60_000);
  const draft = sealAdmission(document);
  const current = await captureAdmissionSample(config, draft, snapshot, initialSwap);
  document.timestamps.liveSampledAt = current.capturedAt;
  const admission = sealAdmission(document);
  const verdict = evaluateAdmission(admission, manifest.admission, current);
  requireCondition(verdict.admitted, "admission construction refused", verdict);
  const bytes = Buffer.from(`${JSON.stringify(admission, null, 2)}\n`, "utf8");
  const digest = sha256(bytes);
  await writeNewDurableFile(outputPath, bytes);
  await writeNewDurableFile(`${outputPath}.sha256`, Buffer.from(`${digest}  ${basename(outputPath)}\n`, "utf8"));
  return { admission, path: outputPath, digest, checksumPath: `${outputPath}.sha256` };
}

// validate one closed durable replacement operation state
export function validateReplacementOperationState(operation, state, result, compensation = undefined, revert = undefined, completion = undefined) {
  requireCondition(operation?.schema === "weather-page-index-operation/v2"
    && UUID_PATTERN.test(operation.operationId), "replacement operation identity is missing or malformed");
  requireCondition(state?.schema === operation.schema && state.operationId === operation.operationId
    && state.clusterDigest === operation.clusterDigest
    && state.databaseOid === operation.databaseOid
    && state.tableOid === operation.tableOid
    && state.sourceInventoryDigest === operation.sourceInventoryDigest, "replacement operation state identity changed");
  requireCondition(result?.schema === "weather-page-index-result/v2" && result.operationId === operation.operationId, "replacement operation result is missing or foreign");

  // accept one successfully installed target awaiting explicit revert
  if (state.status === "completed") {
    requireCondition(result.status === "replaced"
      && state.resultDigest === sha256(stableJson(result))
      && compensation === undefined && revert === undefined
      && completion?.schema === "weather-page-index-completion/v1"
      && completion.operationId === operation.operationId
      && completion.resultDigest === state.resultDigest
      && stableJson(completion.beforeIdentities) === stableJson(operation.sourceIdentities)
      && stableJson(completion.afterIdentities) === stableJson(result.afterIdentities), "completed replacement receipt differs");
    return "active";
  }

  // accept one source-first compensated terminal failure
  if (state.status === "failed-compensated") {
    requireCondition(result.status === "failed-compensated"
      && state.failure?.status === result.status
      && compensation?.schema === "weather-page-index-compensation/v1"
      && compensation.operationId === operation.operationId
      && revert === undefined && completion === undefined, "compensated replacement receipt differs");
    return "safe-terminal";
  }

  // accept one explicit completed revert
  if (state.status === "reverted") {
    requireCondition(result.status === "replaced"
      && compensation?.schema === "weather-page-index-compensation/v1"
      && compensation.operationId === operation.operationId
      && revert?.schema === "weather-page-index-revert/v2"
      && revert.operationId === operation.operationId
      && completion?.schema === "weather-page-index-completion/v1"
      && completion.operationId === operation.operationId
      && completion.resultDigest === sha256(stableJson(result))
      && state.revertDigest === sha256(stableJson(revert)), "reverted replacement receipt differs");
    return "safe-terminal";
  }

  throw new ContractError(`replacement operation is incomplete or unsafe: ${String(state.status)}`);
}

// read one optional operation artifact
async function readOptionalOperationJson(path) {
  try {
    const info = await lstat(path);
    requireCondition(info.isFile() && !info.isSymbolicLink() && info.uid === 0
      && info.nlink === 1 && (info.mode & 0o777) === 0o600
      && await realpath(path) === resolve(path), `operation artifact is unsafe: ${path}`);
    return (await readJson(path)).value;
  } catch (error) {
    // accept only an absent optional artifact
    if (error?.code === "ENOENT") {
      return undefined;
    }
    throw error;
  }
}

// refuse every unknown or incomplete prior replacement operation
export async function scanReplacementOperations(evidenceRoot, allowedActiveOperation = undefined) {
  const canonicalRoot = await realpath(evidenceRoot);
  requireCondition(canonicalRoot === resolve(evidenceRoot), "operation evidence root must be canonical");
  const entries = await readdir(canonicalRoot, { withFileTypes: true });
  let allowedFound = false;

  // inspect every UUID operation directory under the held state lock
  for (const entry of entries.sort((left, right) => left.name.localeCompare(right.name))) {
    // ignore non-operation evidence names
    if (!UUID_PATTERN.test(entry.name)) {
      continue;
    }
    requireCondition(entry.isDirectory() && !entry.isSymbolicLink(), `operation entry is unsafe: ${entry.name}`);
    const directory = join(canonicalRoot, entry.name);
    const directoryInfo = await lstat(directory);
    requireCondition(directoryInfo.uid === 0 && (directoryInfo.mode & 0o077) === 0, `operation directory is not private root-owned: ${entry.name}`);
    const operation = await readOptionalOperationJson(join(directory, "operation.json"));

    // distinguish closed restore and benign report directories from replacement work
    if (operation === undefined) {
      const restoreOperation = await readOptionalOperationJson(join(directory, "restore-operation.json"));

      // accept only one closed restore wrapper directory
      if (restoreOperation !== undefined) {
        const restoreResult = await readOptionalOperationJson(join(directory, "restore-result.json"));
        requireCondition(restoreOperation.schema === "weather-page-index-restore-operation/v1"
          && restoreOperation.operationId === entry.name
          && restoreResult?.schema === "weather-page-index-restore-result/v1"
          && restoreResult.operationId === entry.name
          && ["passed", "failed"].includes(restoreResult.status), `restore operation is incomplete or foreign: ${entry.name}`);
        continue;
      }

      const names = (await readdir(directory)).sort();
      requireCondition(names.length > 0, `unknown or crashed operation directory exists: ${entry.name}`);
      const replacementLike = names.some((name) => /operation|replacement|ownership|compensation|revert|audit/iu.test(name));
      requireCondition(!replacementLike, `unknown or crashed replacement operation exists: ${entry.name}`);
      continue;
    }
    requireCondition(operation.operationId === entry.name, `operation directory identity changed: ${entry.name}`);
    const state = await readOptionalOperationJson(join(directory, "replacement-state.json"));
    const result = await readOptionalOperationJson(join(directory, "result.json"));
    requireCondition(state !== undefined && result !== undefined, `replacement operation is missing terminal records: ${entry.name}`);
    const status = validateReplacementOperationState(
      operation,
      state,
      result,
      await readOptionalOperationJson(join(directory, "compensation.json")),
      await readOptionalOperationJson(join(directory, "revert.json")),
      await readOptionalOperationJson(join(directory, "completion.json")),
    );

    // permit only the exact requested completed operation during revert
    if (status === "active") {
      requireCondition(entry.name === allowedActiveOperation, `completed replacement still requires explicit revert: ${entry.name}`);
      allowedFound = true;
    }
  }

  // require the requested revert operation to be the one active replacement
  if (allowedActiveOperation !== undefined) {
    requireCondition(allowedFound, "requested revert operation is not a completed active replacement");
  }
  return true;
}

// create one private operation directory
async function createOperationDirectory(config) {
  const root = await realpath(config.paths.evidenceRoot);
  requireCondition(root === resolve(config.paths.evidenceRoot), "evidence root must be canonical");
  const info = await lstat(root);
  requireCondition(info.isDirectory() && !info.isSymbolicLink() && (info.mode & 0o077) === 0, "evidence root must be private");
  const operationId = randomUUID();
  const directory = join(root, operationId);
  await mkdir(directory, { mode: 0o700 });
  return directory;
}

// require root for privileged live operations
function requireRoot() {
  requireCondition(typeof process.getuid === "function" && process.getuid() === 0, "privileged lifecycle commands require root");
}

// bind live and restore configuration modes
export function validateRuntimeMode(config, mode) {
  const restoreMode = mode === "verify-restore" || mode === "reconcile-restore";

  // reserve isolated roots for restore wrappers only
  if (config.restoreOnly === true) {
    requireCondition(restoreMode, "restore-only runtime config cannot run a live lifecycle mode");
  } else {
    requireCondition(!restoreMode, "live runtime config cannot run an isolated restore wrapper");
    requireCondition(config.paths.stateRoot === "/opt/weather/current/deploy/state", "live state root changed");
    requireCondition(config.paths.installRoot === "/opt/weather/current", "live install root changed");
    requireCondition(config.paths.forecastSettings === "/var/lib/weather/xweather/forecast-adjustment-settings.json", "live forecast settings path changed");
  }

  return true;
}

// execute one lifecycle command
export async function main(argv = process.argv.slice(2)) {
  // print help without package validation
  if (argv.length === 1 && (argv[0] === "--help" || argv[0] === "-h")) {
    process.stdout.write("Usage: weather-page-indexes.mjs plan|verify|sample-ingestion|build-admission|apply|revert|verify-restore|reconcile-restore --config FILE [--admission FILE] [--operation DIR] [--restore-context DIR] [--duration-seconds 600..4500 --output FILE --rehearsal-report FILE --rehearsal-digest SHA256] [--ingestion-report FILE --ingestion-digest SHA256]\n");
    return;
  }

  const options = parseArguments(argv);
  const packageIdentity = await validateInstalledPackage();
  const manifest = packageIdentity.indexManifest;
  const config = await readRuntimeConfig(options.config);
  const indexSpec = manifest.replacement.target;
  validateRuntimeMode(config, options.mode);

  // run read-only planning
  if (options.mode === "plan") {
    const snapshot = await captureRuntimeSnapshot(config, indexSpec, manifest.restoreContext.migrations);
    assertExpectedSnapshot(snapshot, config.expected);
    validateReplacementInventory(snapshot.indexInventory, manifest, config.postgres.tableOwner, "source");
    process.stdout.write(`${JSON.stringify({ mode: "plan", snapshot }, null, 2)}\n`);
    return;
  }

  // run read-only catalog verification
  if (options.mode === "verify") {
    const snapshot = await captureRuntimeSnapshot(config, indexSpec, manifest.restoreContext.migrations);
    assertExpectedSnapshot(snapshot, config.expected);
    const replacement = validateReplacementInventory(snapshot.indexInventory, manifest, config.postgres.tableOwner, "final");
    process.stdout.write(`${JSON.stringify({ mode: "verify", replacement, catalogHash: catalogFingerprintHash(replacement.target) }, null, 2)}\n`);
    return;
  }

  // produce one read-only normal-ingestion evidence report
  if (options.mode === "sample-ingestion") {
    requireRoot();
    const result = await sampleProductionIngestion({
      config,
      manifest,
      packageIdentity,
      durationSeconds: Number(options.durationSeconds),
      outputPath: resolve(options.output),
      rehearsalPath: options.rehearsalReport,
      rehearsalDigest: options.rehearsalDigest,
    });
    process.stdout.write(`${JSON.stringify({ mode: "sample-ingestion", reference: result.reference, checksumPath: result.checksumPath }, null, 2)}\n`);
    return;
  }

  // build one closed live admission from measured reports
  if (options.mode === "build-admission") {
    requireRoot();
    const result = await buildLiveAdmission({
      config,
      manifest,
      packageIdentity,
      outputPath: resolve(options.output),
      rehearsalPath: options.rehearsalReport,
      rehearsalDigest: options.rehearsalDigest,
      ingestionPath: options.ingestionReport,
      ingestionDigest: options.ingestionDigest,
    });
    process.stdout.write(`${JSON.stringify({ mode: "build-admission", path: result.path, digest: result.digest, checksumPath: result.checksumPath }, null, 2)}\n`);
    return;
  }

  requireRoot();
  const lock = await acquireStateDirectoryLock(config.paths.stateRoot);

  try {
    // apply one admitted index
    if (options.mode === "apply") {
      await scanReplacementOperations(config.paths.evidenceRoot);
      const before = await captureRuntimeSnapshot(config, indexSpec, manifest.restoreContext.migrations);
      assertExpectedSnapshot(before, config.expected);
      const admission = await loadAdmission(options.admission, manifest, packageIdentity, before, config);
      const operationDirectory = await createOperationDirectory(config);
      const result = await runManagedCreate({ config, manifest, admission, admissionSample: admission.current, operationDirectory, snapshot: before });
      process.stdout.write(`${JSON.stringify({ mode: "apply", operationDirectory, result }, null, 2)}\n`);
      return;
    }

    // revert one exact operation-owned index
    if (options.mode === "revert") {
      const operationDirectory = await realpath(options.operation);
      const evidenceRoot = await realpath(config.paths.evidenceRoot);
      requireCondition(operationDirectory.startsWith(`${evidenceRoot}${sep}`), "operation directory escapes evidence root");
      await scanReplacementOperations(evidenceRoot, basename(operationDirectory));
      const snapshot = await captureRuntimeSnapshot(config, indexSpec, manifest.restoreContext.migrations);
      assertExpectedSnapshot(snapshot, config.expected);
      const admission = await loadAdmission(options.admission, manifest, packageIdentity, snapshot, config);
      const receipt = await dropOwnedIndex({ config, manifest, operationDirectory, snapshot, admission });
      process.stdout.write(`${JSON.stringify({ mode: "revert", receipt }, null, 2)}\n`);
      return;
    }

    // run one isolated restore wrapper
    if (options.mode === "verify-restore" || options.mode === "reconcile-restore") {
      const operationDirectory = await createOperationDirectory(config);
      const result = await runRestoreWrapper({
        config,
        manifest,
        releaseManifest: packageIdentity.releaseManifest,
        contextRoot: options.restoreContext,
        mode: options.mode,
        operationDirectory,
        admissionPath: options.admission,
        packageIdentity,
      });
      process.stdout.write(`${JSON.stringify({ mode: options.mode, operationDirectory, result }, null, 2)}\n`);
      return;
    }

    throw new ContractError(`unhandled mode: ${options.mode}`);
  } finally {
    await lock.release();
  }
}

// run only as the direct cli entrypoint
if (import.meta.url === pathToFileURL(process.argv[1] ?? "").href) {
  // report one cli failure as json
  main().catch((error) => {
    const output = {
      error: error instanceof Error ? error.message : String(error),
      details: error instanceof ContractError ? error.details : undefined,
    };
    process.stderr.write(`${JSON.stringify(output, null, 2)}\n`);
    process.exitCode = 1;
  });
}
