import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import vm from "node:vm";

const repositoryRoot = resolve(dirname(fileURLToPath(import.meta.url)), "../../../..");
const source = readFileSync(
  resolve(repositoryRoot, "packages/forecast-adjustment/dist/rain-hurdle-wind.js"),
  "utf8",
);
const HOUR_MS = 3_600_000;
const DECISION_DELAY_HOURS = 8;
const STATION_IDS = [
  64255, 225947, 38270, 168853, 126537, 201058,
  203055, 66270, 34768, 88159, 126197, 27140,
];
const STATIONS = [
  [47.95008, -122.43982], [47.94215, -122.42542],
  [47.95293, -122.41414], [47.95498, -122.44074],
  [47.9582, -122.44274], [47.96244, -122.43369],
  [47.96505, -122.4241], [47.93134, -122.42912],
  [47.91752, -122.41112], [47.91563, -122.41845],
  [47.91413, -122.41471], [47.98707, -122.46295],
];

// extract the compiled pure feature owner and its bounded helpers
function sourceSpan(startMarker, endMarker) {
  const start = source.indexOf(startMarker);
  const end = source.indexOf(endMarker, start);
  // fail when the production source span moves
  if (start < 0 || end < 0) {
    throw new Error(`unable to extract ${startMarker}`);
  }
  return source.slice(start, end).replace("export function", "function");
}

// encode one JavaScript binary64 value exactly
function float64Bits(value) {
  const buffer = new ArrayBuffer(8);
  const view = new DataView(buffer);
  view.setFloat64(0, value, false);
  return view.getBigUint64(0, false).toString(16).padStart(16, "0");
}

// encode one Float32Array member exactly
function float32Bits(value) {
  const buffer = new ArrayBuffer(4);
  const view = new DataView(buffer);
  view.setFloat32(0, value, false);
  return view.getUint32(0, false).toString(16).padStart(8, "0");
}

// reproduce the fixed source station weights before the admitted Rust boundary
function stationWeights() {
  const toRadians = Math.PI / 180;
  const siteLatitude = 47.950429954185445 * toRadians;
  const siteLongitude = -122.42797012608193 * toRadians;
  return STATIONS.map(([rawLatitude, rawLongitude]) => {
    const latitude = rawLatitude * toRadians;
    const longitude = rawLongitude * toRadians;
    const a = Math.sin((latitude - siteLatitude) / 2) ** 2 +
      Math.cos(latitude) * Math.cos(siteLatitude) *
      Math.sin((longitude - siteLongitude) / 2) ** 2;
    const distance = 6_371_000 * 2 * Math.asin(Math.sqrt(a));
    return 1 / (1 + (distance / 2000) ** 2);
  });
}

// build one exact 48-hour initialized forecast member
function forecastHours(precipitationOffset = 0) {
  return Array.from({ length: 48 }, (_unused, index) => {
    const leadHours = index + 1;
    return {
      leadHours,
      precipitationMm: leadHours * 0.1 + precipitationOffset,
      temperatureC: 8 + leadHours * 0.2,
      relativeHumidityPercent: 45 + leadHours * 0.5,
      cloudCoverPercent: (leadHours * 7) % 100,
      pressureHpa: 995 + leadHours * 0.6,
      windSpeedMps: 1.5 + leadHours * 0.12,
      windDirectionDegrees: (leadHours * 17) % 360,
    };
  });
}

// build admitted current/prior/station maps for one source fixture
function fixture(name) {
  const initializedMs = Date.parse("2026-09-07T00:00:00.000Z");
  const decisionMs = initializedMs + DECISION_DELAY_HOURS * HOUR_MS;
  const current = { initializedMs, hours: forecastHours() };
  const prior = new Map([
    [initializedMs - 6 * HOUR_MS, { hours: forecastHours(0.3) }],
    [initializedMs - 12 * HOUR_MS, { hours: forecastHours(0.6) }],
  ]);
  const stations = new Map();
  // populate the exact six source lag windows
  for (const lag of [1, 2, 3, 6, 12, 24]) {
    const byStation = new Map();
    // retain fixed catalog ordering in each station window
    for (let index = 0; index < STATION_IDS.length; index += 1) {
      byStation.set(STATION_IDS[index], {
        precipitationMm: lag * 0.03 + index * 0.02,
        temperatureC: 7 + (index % 4),
      });
    }
    stations.set(decisionMs - lag * HOUR_MS, byStation);
  }
  const modelLeadHours = name === "late" ? 23 : 5;
  const lead = DECISION_DELAY_HOURS + modelLeadHours;
  // exercise forecast, vintage, wind, and station missing branches
  if (name === "missing") {
    current.hours[lead - 4].precipitationMm = null;
    current.hours[lead - 7].pressureHpa = null;
    current.hours[lead - 4].windDirectionDegrees = null;
    prior.delete(initializedMs - 12 * HOUR_MS);
    for (const [lagIndex, lag] of [1, 2, 3, 6, 12, 24].entries()) {
      const byStation = stations.get(decisionMs - lag * HOUR_MS);
      // leave alternating catalog members absent at every lag
      for (let index = lagIndex % 2; index < STATION_IDS.length; index += 2) {
        byStation.delete(STATION_IDS[index]);
      }
    }
  } else if (name === "no-near") {
    const temperatureWindow = stations.get(decisionMs - HOUR_MS);
    temperatureWindow.delete(STATION_IDS[0]);
    temperatureWindow.delete(STATION_IDS[1]);
    temperatureWindow.delete(STATION_IDS[2]);
  } else if (name !== "complete" && name !== "late") {
    throw new Error(`unknown fixture ${name}`);
  }
  return { current, initializedMs, modelLeadHours, prior, stations };
}

const selected = fixture(process.argv[2]);
const weights = stationWeights();
const buildSource = sourceSpan(
  "// retain missing gauges as missing rather than dry",
  "// traverse numerical XGBoost trees with their native missing branches",
);
const features = vm.runInNewContext(
  `${buildSource}\nbuildRainHurdleWindFeatures(current, prior, stations, modelLeadHours);`,
  {
    ARTIFACT: { featureNames: Array(107).fill("feature") },
    current: selected.current,
    DECISION_DELAY_HOURS,
    HOUR_MS,
    modelLeadHours: selected.modelLeadHours,
    prior: selected.prior,
    stations: selected.stations,
    STATION_IDS,
    STATION_WEIGHTS: weights,
  },
);
process.stdout.write([
  [...features].map(float32Bits).join(","),
  weights.map(float64Bits).join(","),
  [features[1], features[2], features[3], features[4]].map(float32Bits).join(","),
].join("\t"));
