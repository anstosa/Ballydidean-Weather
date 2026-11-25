import { buildCausalTemperatureRecentErrorState } from "../../../../apps/worker/dist/index.js";
import { localCalendarFeaturesFor } from "../../../../packages/forecast-adjustment/dist/index.js";

const fixture = process.argv[2];
const targetRunInitializedAt = "2026-09-08T00:00:00.000Z";
const targetMilliseconds = Date.parse(targetRunInitializedAt);
const windowEndMilliseconds = targetMilliseconds - 7 * 3_600_000;
const windowEndValidAt = new Date(windowEndMilliseconds).toISOString();
const slots = Array.from({ length: 72 }, (_, age) => {
  const validAt = new Date(windowEndMilliseconds - age * 3_600_000).toISOString();
  return {
    age,
    localDate: localCalendarFeaturesFor(validAt).localDate,
    validAt,
  };
});

// Select one causal forecast corpus without changing the production reducer.
function forecastIsPresent(age) {
  if (fixture === "supported") {
    return age < 40 && age !== 2;
  }
  if (fixture === "insufficient") {
    return age < 8;
  }
  if (fixture === "clipped") {
    return age < 24;
  }
  if (fixture === "no-evidence") {
    return true;
  }
  throw new Error(`unknown fixture ${fixture}`);
}

// Resolve one deterministic raw forecast and source identity.
function forecast(slot) {
  const latestSourceMilliseconds = Date.parse(slot.validAt) - 7 * 3_600_000;
  const runInitializedAt = new Date(
    Math.floor(latestSourceMilliseconds / (6 * 3_600_000)) * 6 * 3_600_000,
  ).toISOString();
  return {
    key: `${runInitializedAt}/${slot.validAt}/k-${String(slot.age).padStart(2, "0")}`,
    rawTemperatureC: fixture === "clipped" ? 0 : 10 + (slot.age % 5) * 0.25,
    runInitializedAt,
    validAt: slot.validAt,
  };
}

// Build one station matrix that covers network, missing, and nearest-gate paths.
function stationRows(slot) {
  if (fixture === "no-evidence") {
    return [
      ["ambient-maxweather", 9],
      ["tempest-126537", 10],
      ["tempest-168853", 11],
    ];
  }
  if (fixture === "supported" && slot.age % 11 === 0) {
    return [
      ["ballydidean-ecowitt", 12],
      ["tempest-64255", 13],
    ];
  }
  const center = fixture === "clipped" ? 20 : 12 + ((slot.age % 7) - 3) * 0.4;
  return [
    ["ambient-merlin", center - 2],
    ["ballydidean-ecowitt", center],
    ["tempest-38270", slot.age % 7 === 0 ? null : center + 1],
    ["tempest-64255", center + 2],
  ];
}

const priorHours = slots.filter((slot) => forecastIsPresent(slot.age)).map(forecast);
const stationHours = slots.flatMap((slot) =>
  stationRows(slot).map(([physicalStationKey, temperatureC]) => ({
    metrics: { temperatureC },
    physicalStationKey,
    validAt: slot.validAt,
  })),
);
const repository = {
  // Return the admitted private forecasts without database I/O.
  async listCausalEcmwfTemperatureCanaryPriorHours() {
    return priorHours;
  },
  // Return the admitted station matrix without database I/O.
  async listCausalForecastObservationHourlyStations() {
    return stationHours;
  },
};
const state = await buildCausalTemperatureRecentErrorState({}, {
  repository,
  siteSlug: "ballydidean",
  targetRunInitializedAt,
});

// Encode one nullable binary64 without decimal-rounding ambiguity.
function nullableBits(value) {
  if (value === null) {
    return "null";
  }
  const bytes = new ArrayBuffer(8);
  const view = new DataView(bytes);
  view.setFloat64(0, value, false);
  return view.getBigUint64(0, false).toString(16).padStart(16, "0");
}

console.log([
  nullableBits(state.b24C),
  nullableBits(state.b72C),
  state.localDates,
  nullableBits(state.mad72C),
  state.maximumSourceRunInitializedAt ?? "null",
  state.maximumSourceValidAt ?? "null",
  state.n24,
  state.n72,
  state.sourceKeys.join(","),
  state.supported ? "1" : "0",
  state.targetRunInitializedAt,
  state.windowEndValidAt,
].join("\t"));
console.log(slots.map((slot) =>
  `${slot.validAt}|${slot.localDate}|${forecast(slot).runInitializedAt}`
).join(","));
console.log(`${targetMilliseconds}\t${windowEndMilliseconds}`);
