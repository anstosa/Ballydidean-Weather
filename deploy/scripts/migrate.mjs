import { pathToFileURL } from "node:url";

// accept only the two separately reviewed atomic migration bridges
export function parseMigrationEntrypointArguments(argumentsList) {
  // preserve the ordinary one-shot without adding runtime authority
  if (argumentsList.length === 0) return {};
  // bind atomicity to one fixed source and target ledger
  if (argumentsList.length === 1 && argumentsList[0] === "--atomic-maintenance-v14") {
    return { atomicMaintenanceV14: true };
  }
  // bind the normal release to the exact schema 21 through 23 bridge
  if (argumentsList.length === 1 && argumentsList[0] === "--atomic-normal-21-23") {
    return { atomicNormal21To23: true };
  }
  throw new Error("migration entrypoint arguments are invalid");
}

// initialize the schema and configured site
export async function migrateAndBootstrap(
  pool,
  migrationDirectory,
  siteConfigurationPath,
  dependencies,
  migrationOptions = {},
  tempestConfigurationPath = null,
  publicStationConfigurationPath = null,
  tideConfigurationPath = null,
  ecowittConfigurationPath = null,
) {
  try {
    // apply checked migrations first
    const migrations = await dependencies.runMigrations(
      pool,
      migrationDirectory,
      migrationOptions,
    );
    // load configuration after the schema is ready
    const siteConfiguration = await dependencies.loadSiteConfiguration(siteConfigurationPath);
    // bootstrap through the owner pool
    const bootstrap = await dependencies.bootstrapSiteConfiguration(pool, siteConfiguration);
    let ecowittBootstrap = null;
    let tempestBootstrap = null;
    let publicStationBootstrap = null;
    let tideBootstrap = null;

    // bootstrap the optional first-party Ecowitt catalog
    if (ecowittConfigurationPath !== null) {
      const ecowittConfiguration = await dependencies.loadEcowittConfiguration(
        ecowittConfigurationPath,
      );
      ecowittBootstrap = await dependencies.bootstrapEcowittConfiguration(
        pool,
        ecowittConfiguration,
      );
    }

    // bootstrap the optional Tempest catalog
    if (tempestConfigurationPath !== null) {
      const tempestConfiguration = await dependencies.loadTempestConfiguration(
        tempestConfigurationPath,
      );
      tempestBootstrap = await dependencies.bootstrapTempestConfiguration(
        pool,
        tempestConfiguration,
      );
    }

    // bootstrap the optional tide catalog
    if (tideConfigurationPath !== null) {
      const tideConfiguration = await dependencies.loadTideConfiguration(
        tideConfigurationPath,
      );
      tideBootstrap = await dependencies.bootstrapTideConfiguration(
        pool,
        tideConfiguration,
      );
    }

    // bootstrap the optional public-station catalog
    if (publicStationConfigurationPath !== null) {
      const publicStationConfiguration =
        await dependencies.loadPublicStationConfiguration(
          publicStationConfigurationPath,
        );
      publicStationBootstrap =
        await dependencies.bootstrapPublicStationConfiguration(
          pool,
          publicStationConfiguration,
        );
    }

    return {
      bootstrap,
      ecowittBootstrap,
      migrations,
      publicStationBootstrap,
      tempestBootstrap,
      tideBootstrap,
    };
  } finally {
    // close pooled sessions
    await pool.end();
  }
}

const isEntrypoint =
  process.argv[1] !== undefined &&
  import.meta.url === pathToFileURL(process.argv[1]).href;

// run only when invoked as the one-shot
if (isEntrypoint) {
  const migrationOptions = parseMigrationEntrypointArguments(process.argv.slice(2));
  const {
    bootstrapEcowittConfiguration,
    bootstrapPublicStationConfiguration,
    bootstrapSiteConfiguration,
    bootstrapTempestConfiguration,
    bootstrapTideConfiguration,
    createDatabasePool,
    loadEcowittConfiguration,
    loadDatabaseConfiguration,
    loadPublicStationConfiguration,
    loadSiteConfiguration,
    loadTempestConfiguration,
    loadTideConfiguration,
    runMigrations,
  } = await import("@weather/database");
  const configuration = await loadDatabaseConfiguration();
  const pool = createDatabasePool(configuration);
  const migrationDirectory =
    process.env.WEATHER_MIGRATION_DIRECTORY ??
    "/opt/weather/packages/database/migrations";
  const siteConfigurationPath =
    process.env.WEATHER_SITE_CONFIG_PATH ??
    "/opt/weather/config/sites/ballydidean.json";
  const ecowittConfigurationPath =
    process.env.WEATHER_ECOWITT_CONFIG_PATH ??
    "/opt/weather/config/ecowitt/gateways.json";
  const tempestConfigurationPath =
    process.env.WEATHER_TEMPEST_CONFIG_PATH ??
    "/opt/weather/config/tempest/stations.json";
  const tideConfigurationPath =
    process.env.WEATHER_TIDE_CONFIG_PATH ??
    "/opt/weather/config/tides/noaa.json";
  const publicStationConfigurationPath =
    process.env.WEATHER_PUBLIC_STATIONS_CONFIG_PATH ??
    "/opt/weather/config/public-stations/stations.json";
  const result = await migrateAndBootstrap(
    pool,
    migrationDirectory,
    siteConfigurationPath,
    {
      bootstrapEcowittConfiguration,
      bootstrapPublicStationConfiguration,
      bootstrapSiteConfiguration,
      bootstrapTempestConfiguration,
      bootstrapTideConfiguration,
      loadEcowittConfiguration,
      loadPublicStationConfiguration,
      loadSiteConfiguration,
      loadTempestConfiguration,
      loadTideConfiguration,
      runMigrations,
    },
    {
      ...migrationOptions,
      lockTimeoutMs: configuration.lockTimeoutMs,
      statementTimeoutMs: configuration.statementTimeoutMs,
    },
    tempestConfigurationPath,
    publicStationConfigurationPath,
    tideConfigurationPath,
    ecowittConfigurationPath,
  );
  console.log(
    JSON.stringify({
      bootstrap: result.bootstrap,
      ecowittBootstrap: result.ecowittBootstrap,
      event: "migrations_complete",
      publicStationBootstrap: result.publicStationBootstrap,
      tempestBootstrap: result.tempestBootstrap,
      tideBootstrap: result.tideBootstrap,
      ...result.migrations,
    }),
  );
}
