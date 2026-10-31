import {
  buildForecastObservationPoints,
  buildSolarCloudEvidence,
  fromSiteWallClock,
  toSiteWallClock,
} from "../../apps/web/dist/index.js";

// retain one shared compact day rather than repeating archive scans for every viewer
export class ForecastObservationCache {
  #cached = null;
  #pending = null;
  #fetcher;
  #origin;
  #now;
  #maxBytes;
  #timeoutMs;
  #retryMs;
  #retryAfter = 0;

  // bind fixed upstream geometry with injectable limits for deterministic refusal tests
  constructor({ origin, fetcher = fetch, now = () => new Date(), maxBytes = 4 * 1024 * 1024, timeoutMs = 30_000, retryMs = 30_000 }) {
    this.#origin = origin;
    this.#fetcher = fetcher;
    this.#now = now;
    this.#maxBytes = maxBytes;
    this.#timeoutMs = timeoutMs;
    this.#retryMs = retryMs;
  }

  // share only a completed same-local-day projection for five minutes
  async get() {
    const now = this.#now();
    const localDay = toSiteWallClock(now.toISOString(), "America/Los_Angeles").slice(0, 10);
    // never carry yesterday's cache across the farm's local midnight
    if (this.#cached?.day === localDay && this.#cached.expires > now.getTime()) {
      return this.#cached.body;
    }
    // concurrent viewers share a fill without owning its cancellation signal
    if (this.#pending !== null) {
      await this.#pending;
      return this.get();
    }
    // repeated public callers cannot continuously rescan a failing core archive
    if (this.#retryAfter > now.getTime()) {
      throw new Error("Forecast observation retry cooldown is active");
    }
    const pending = this.#fill(now).then(({ body, day }) => {
      this.#cached = { body, day, expires: now.getTime() + 5 * 60_000 };
      return body;
    });
    this.#pending = pending;
    try {
      await pending;
    } catch (error) {
      this.#retryAfter = this.#now().getTime() + this.#retryMs;
      throw error;
    } finally {
      // failed paging never poisons later recovery or publishes an incomplete mean
      this.#pending = null;
    }
    // recheck the farm day after a fill that may have crossed local midnight
    return this.get();
  }

  // read and validate one complete private compact input
  async #fill(requestStartedAt) {
    const abort = new AbortController();
    const signal = abort.signal;
    const timer = setTimeout(() => abort.abort(), this.#timeoutMs);
    const deadline = performance.now() + this.#timeoutMs;
    const url = new URL("/internal/v1/forecast-observation-input", this.#origin);

    try {
      const response = await this.#fetcher(url, {
        headers: { accept: "application/json" }, redirect: "error", signal,
      });
      // a partial or failed private envelope cannot produce public evidence
      if (!response.ok || response.body === null) {
        throw new Error("Forecast observation input is unavailable");
      }
      let body = "";
      let bytes = 0;
      const decoder = new TextDecoder("utf-8", { fatal: true });
      // retain one strict byte and wall-clock bound while streaming
      for await (const chunk of response.body) {
        const nextBytes = bytes + chunk.byteLength;
        // reject an unbounded chunk before decoding or retaining it
        if (nextBytes > Math.min(this.#maxBytes, 4 * 1024 * 1024) || performance.now() >= deadline) {
          throw new Error("Forecast observation input limit exceeded");
        }
        signal.throwIfAborted();
        bytes = nextBytes;
        body += decoder.decode(chunk, { stream: true });
      }
      body += decoder.decode();
      const receivedAt = this.#now();
      const input = parseObservationInput(
        JSON.parse(body),
        requestStartedAt,
        receivedAt,
      );
      const data = buildForecastObservationPoints(
        input.records,
        input.projectionFrom,
        input.through,
      );
      let solarCloudEvidence = null;
      try {
        solarCloudEvidence = buildSolarCloudEvidence(input.records, {
          latitude: 47.950429954185445, longitude: -122.42797012608193,
        }, input.through);
      } catch {
        // optional pairing refusal leaves the recorded projection independently usable
      }
      signal.throwIfAborted();
      // calculation time remains part of the complete-fill deadline
      if (performance.now() >= deadline) {
        throw new Error("Forecast observation deadline exceeded");
      }
      return {
        body: { generatedAt: input.through, data, solarCloudEvidence },
        day: toSiteWallClock(input.projectionFrom, "America/Los_Angeles").slice(0, 10),
      };
    } finally {
      clearTimeout(timer);
      abort.abort();
    }
  }
}

const metricKeys = [
  "apparentTemperatureC", "cloudCoverPercent", "pm25MicrogramsPerCubicMeter",
  "precipitationRateMmPerHour", "pressureHpa", "relativeHumidityPercent",
  "solarRadiationWm2", "temperatureC", "uvIndex", "windDirectionDegrees",
  "windGustMps", "windSpeedMps",
];
const recordKeys = ["id", "metrics", "provenance", "receivedAt", "validAt"];
const provenanceKeys = ["providerKey", "sourceId", "sourceKey", "sourceKind", "stationSlug"];

// narrow one object without accepting arrays
function object(value) {
  return value !== null && typeof value === "object" && !Array.isArray(value) ? value : null;
}

// require one exact key set
function exactKeys(value, keys) {
  return Object.keys(value).toSorted().join("\0") === [...keys].toSorted().join("\0");
}

// require one canonical UTC instant
function instant(value) {
  return typeof value === "string" && Number.isFinite(Date.parse(value)) &&
    new Date(Date.parse(value)).toISOString() === value;
}

// validate the closed compact input without manufacturing public-record fields
function parseObservationInput(value, requestStartedAt, receivedAt) {
  const input = object(value);
  const site = object(input?.site);
  if (input === null || site === null ||
    !exactKeys(input, ["contractVersion", "generatedAt", "projectionFrom", "queryFrom", "records", "site", "through"]) ||
    !exactKeys(site, ["slug", "timezone"]) ||
    input.contractVersion !== "forecast-observation-input/v1" ||
    site.slug !== "ballydidean" || site.timezone !== "America/Los_Angeles" ||
    !instant(input.generatedAt) || input.through !== input.generatedAt ||
    !instant(input.projectionFrom) || !instant(input.queryFrom) ||
    Date.parse(input.generatedAt) < requestStartedAt.getTime() ||
    Date.parse(input.generatedAt) > receivedAt.getTime() ||
    !Array.isArray(input.records) || input.records.length > 20_000) {
    throw new Error("Forecast observation input envelope is invalid");
  }
  const localDay = toSiteWallClock(input.generatedAt, site.timezone).slice(0, 10);
  const expectedProjection = fromSiteWallClock(`${localDay}T00:00`, site.timezone);
  const expectedQuery = new Date(Math.min(
    Date.parse(expectedProjection) - 3 * 3_600_000,
    Date.parse(input.generatedAt) - 24 * 3_600_000 - 20 * 60_000,
  )).toISOString();
  if (input.projectionFrom !== expectedProjection || input.queryFrom !== expectedQuery) {
    throw new Error("Forecast observation input geometry is invalid");
  }

  let previousKind = "physical_sensor";
  let previousValidAt = Number.POSITIVE_INFINITY;
  let previousId = null;
  // validate every minimal record and the frozen physical-then-model ordering
  for (const recordValue of input.records) {
    const record = object(recordValue);
    const metrics = object(record?.metrics);
    const provenance = object(record?.provenance);
    if (record === null || metrics === null || provenance === null ||
      !exactKeys(record, recordKeys) || !exactKeys(metrics, metricKeys) ||
      !exactKeys(provenance, provenanceKeys) || typeof record.id !== "string" ||
      !/^[1-9][0-9]*$/u.test(record.id) ||
      !instant(record.validAt) || !instant(record.receivedAt) ||
      Date.parse(record.validAt) < Date.parse(input.queryFrom) ||
      Date.parse(record.validAt) >= Date.parse(input.through) ||
      !["physical_sensor", "model_current"].includes(provenance.sourceKind) ||
      [provenance.providerKey, provenance.sourceId, provenance.sourceKey, provenance.stationSlug]
        .some((field) => typeof field !== "string" || field.length === 0 || field.length > 512) ||
      metricKeys.some((key) => metrics[key] !== null &&
        (typeof metrics[key] !== "number" || !Number.isFinite(metrics[key])))) {
      throw new Error("Forecast observation input record is invalid");
    }
    const kindOrder = provenance.sourceKind === "physical_sensor" ? 0 : 1;
    const previousOrder = previousKind === "physical_sensor" ? 0 : 1;
    const validAt = Date.parse(record.validAt);
    if (kindOrder < previousOrder || (kindOrder === previousOrder &&
      (validAt > previousValidAt || (validAt === previousValidAt && previousId !== null &&
        BigInt(record.id) > BigInt(previousId))))) {
      throw new Error("Forecast observation input order is invalid");
    }
    previousKind = provenance.sourceKind;
    previousValidAt = validAt;
    previousId = record.id;
  }

  return input;
}
