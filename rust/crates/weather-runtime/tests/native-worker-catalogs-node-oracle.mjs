import { resolve } from "node:path";
import { pathToFileURL } from "node:url";

// use the actual compiled predecessor's five catalog material owners
const root = resolve(import.meta.dirname, "../../../..");
const load = async (module, method, path) => {
  const owner = await import(pathToFileURL(resolve(root, "packages/database/dist", module)));
  return await owner[method](resolve(root, path));
};
const [site, ecowitt, tempest, publicStations, tides] = await Promise.all([
  load("config.js", "loadSiteConfiguration", "config/sites/ballydidean.json"),
  load("ecowitt-config.js", "loadEcowittConfiguration", "config/ecowitt/gateways.json"),
  load("tempest-config.js", "loadTempestConfiguration", "config/tempest/stations.json"),
  load("public-stations-config.js", "loadPublicStationConfiguration", "config/public-stations/stations.json"),
  load("tides-config.js", "loadTideConfiguration", "config/tides/noaa.json"),
]);
// retain normalized identities and all catalog fingerprints without exposing connector settings
const value = [
  [site.version, site.provider.key, site.site.key, site.station.key, site.sources.map(source => [source.key, source.fingerprint, source.active])],
  [ecowitt.version, ecowitt.siteKey, ecowitt.provider.key, ecowitt.stations.map(station => [station.key, station.sourceKey, station.fingerprint, station.active])],
  [tempest.version, tempest.siteKey, tempest.provider.key, tempest.stations.map(station => [station.key, station.sourceKey, station.fingerprint, station.active])],
  [publicStations.version, publicStations.siteKey, publicStations.providers.map(provider => provider.key), publicStations.stations.map(station => [station.key, station.active, station.sources.map(source => [source.key, source.providerKey, source.fingerprint, source.active])])],
  [tides.version, tides.siteKey, tides.provider.key, tides.stations.map(station => [station.key, station.active, station.source.key, station.source.fingerprint, station.source.active])],
];
process.stdout.write(`${JSON.stringify(value)}\n`);
