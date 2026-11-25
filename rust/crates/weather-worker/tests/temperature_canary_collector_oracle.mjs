import { collectEcmwfTemperatureCanaryRuns } from "../../../../apps/worker/dist/index.js";

const requests = [];
const persisted = [];
const repository = {
  // return a cold inventory so the collector plans the two newest cycles
  async listEcmwfTemperatureCanaryRunInitializations() {
    return [];
  },
  // return a supported causal state for only the newest serving run
  async listCausalEcmwfTemperatureCanaryPriorHours() {
    return [];
  },
  // preserve cold-start behavior for the normal deterministic fixture
  async listCausalForecastObservationHourlyStations() {
    return [];
  },
  // retain the exact normalized persistence disposition
  async persistEcmwfTemperatureCanaryRun(_pool, input) {
    persisted.push(input.stateStatus);
    return {};
  },
};

const result = await collectEcmwfTemperatureCanaryRuns({}, {
  // provide one already-normalized exact eighteen-hour source response
  async fetchEcmwfSingleRun(request, controls) {
    requests.push([request, controls]);
    return {
      adapterVersion: "open-meteo-ecmwf-single-run/v1",
      attempts: 1,
      hours: Array.from({ length: 18 }, (_, index) => ({
        modelLeadHours: index + 1,
        rawRelativeHumidityPercent: 75,
        rawTemperatureC: 10,
        rawWindSpeedMps: 3,
        validAt: new Date(Date.parse(request.runInitializedAt) + (index + 1) * 3_600_000)
          .toISOString(),
      })),
      modelCycle: "50r1",
      providerResponseSha256: "a".repeat(64),
      receivedAt: "2026-09-08T00:35:00.000Z",
      runInitializedAt: request.runInitializedAt,
      upstreamModel: "ecmwf_ifs",
    };
  },
  fetchOptions: { maxBodyBytes: 900_000, timeoutMs: 20_000 },
  now: () => new Date("2026-09-08T00:35:00.000Z"),
  repository,
  site: {
    site: {
      key: "ballydidean",
      latitude: 56,
      longitude: -3,
    },
  },
});

const cycles = result.requestedRuns.map((value) => new Date(value).getUTCHours()).join(",");
const controls = requests[0][1];
process.stdout.write(
  `${cycles}|${result.persistedRuns}|${result.failedRuns}|${controls.maxAttempts},${controls.timeoutMs},${controls.maxBodyBytes}|${persisted.join(",")}`,
);
