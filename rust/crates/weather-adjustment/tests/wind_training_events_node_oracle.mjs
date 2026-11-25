import {
  deduplicateForecastAtomicCandidates,
  scalarNetworkActual,
} from "../../../../packages/forecast-adjustment/dist/algorithm-v1.js";
import { forecastLeadBandFor } from "@weather/domain";

const midnight = "2026-03-08T00:00:00.000Z";
const hour = "2026-03-08T01:00:00.000Z";
const later = "2026-03-08T02:00:00.000Z";
const rows = [
  { kind: "other" },
  { hash: "a", validAt: hour, referenceAt: "2026-03-08T00:01:00.000Z", lead: 1, speed: 5, gust: 10 },
  { hash: "b", validAt: hour, referenceAt: midnight, lead: 1, speed: 6, gust: 11 },
  { hash: "a", validAt: hour, referenceAt: midnight, lead: 1, speed: 7, gust: 12 },
  { hash: "c", validAt: hour, referenceAt: "2026-03-05T23:59:00.000Z", lead: 50, speed: 8, gust: 13 },
  { hash: "d", validAt: later, referenceAt: hour, lead: 1, speed: 9, gust: 14 },
];
const stationValues = [
  { physicalStationKey: "tempest-38270", nearestRank: 4, unnormalizedSpatialWeight: 1, speed: 10, gust: 18 },
  { physicalStationKey: "tempest-126537", nearestRank: 1, unnormalizedSpatialWeight: 2, speed: 4, gust: 7 },
  { physicalStationKey: "tempest-168853", nearestRank: 3, unnormalizedSpatialWeight: 1, speed: 8, gust: 15 },
];

const candidates = rows.flatMap(
  // retain the original source index and canonical retrieval geometry
  (source, index) => source.kind === "other" ? [] : ["windSpeedMps", "windGustMps"].map(
    (metric) => ({
      cohort: "legacy_v4_retrieval_snapshot",
      continuousLeadHours: (Date.parse(source.validAt) - Date.parse(source.referenceAt)) / 3_600_000,
      metric,
      referenceAt: source.referenceAt,
      referenceKind: "retrieval_snapshot",
      stableId: `${source.hash.repeat(64)}:${index}:${metric}`,
      targetLeadHours: source.lead,
      validAt: source.validAt,
    }),
  ),
);

const output = [];
// ground jitter ordering, weighted median and omitted coverage in the original algorithms
for (const selected of deduplicateForecastAtomicCandidates(candidates)) {
  const metricName = selected.metric === "windSpeedMps" ? "speed" : "gust";
  const matched = selected.validAt === hour ? stationValues : stationValues.slice(0, 2);
  const actual = scalarNetworkActual(matched.map(
    (station) => ({ ...station, value: station[metricName] }),
  ));
  const band = forecastLeadBandFor(selected.targetLeadHours);
  // preserve the compiled maintenance-specific suppression after building network targets
  if (actual === null || (selected.metric === "windGustMps" && band === "049-072")) {
    continue;
  }
  const original = rows[Number(selected.stableId.split(":")[1])];
  output.push([
    selected.validAt, selected.metric, band, selected.stableId, actual.value,
    original[metricName], selected.referenceAt,
  ].join("\t"));
}

process.stdout.write(`${output.join("\n")}\n`);
