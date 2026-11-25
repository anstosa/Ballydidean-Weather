import {
  FORECAST_OBSERVATION_STATIONS,
} from "../../../../packages/domain/dist/index.js";

// encode one JavaScript binary64 value without decimal formatting drift
function bits(value) {
  const bytes = Buffer.allocUnsafe(8);
  bytes.writeDoubleBE(value);
  return bytes.toString("hex");
}

const rows = FORECAST_OBSERVATION_STATIONS.map((station, index) => [
  "STATION",
  String(index),
  station.key,
  station.providerFamily,
  String(station.nearestRank),
  bits(station.latitude),
  bits(station.longitude),
  bits(station.distanceMeters),
  bits(station.unnormalizedSpatialWeight),
  station.eligibleMetrics.join(","),
  station.acceptedSourceKeys.join(","),
]);

process.stdout.write(`${rows.map((row) => row.join("\t")).join("\n")}\n`);
