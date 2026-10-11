import type { WeatherRecord, WeatherSite } from "./index.js";
import { createSolarCloudBias, type SolarCloudBias } from "./solar-cloud.js";

export interface SolarCloudEvidenceSample {
  readonly value: number;
  readonly validAt: string;
  readonly receivedAt: string;
  readonly recordId: string;
  readonly sourceId: string;
  readonly providerKey: string;
  readonly sourceKind: "physical_sensor" | "model_current";
  readonly sourceKey: string;
  readonly stationSlug: string;
}

export interface SolarCloudEvidence {
  readonly evaluatedAt: string;
  readonly ws90: SolarCloudEvidenceSample;
  readonly regional: SolarCloudEvidenceSample;
}

const DAY_MS = 24 * 3_600_000;
const PAIR_GAP_MS = 20 * 60_000;
const MAX_PAIR_COMPARISONS = 64_000;
const SOURCES = {
  ws90: { providerKey: "ecowitt-local", sourceKind: "physical_sensor", sourceKey: "ecowitt-88f15505d89f-local-live-v1", stationSlug: "ballydidean-ecowitt" },
  regional: { providerKey: "open-meteo", sourceKind: "model_current", sourceKey: "open-meteo-current-v1", stationSlug: "open-meteo-virtual" },
} as const;

// admit only explicit canonical timestamps rather than inferred browser freshness
function timestamp(value: unknown): number {
  const time = typeof value === "string" ? Date.parse(value) : NaN;
  return Number.isFinite(time) && new Date(time).toISOString() === value ? time : NaN;
}

// narrow untrusted json without preserving serialized projection metadata
function object(value: unknown): Record<string, unknown> | null {
  return value !== null && typeof value === "object" && !Array.isArray(value) ? value as Record<string, unknown> : null;
}

// copy only the closed physical or regional scalar evidence contract
function sample(value: unknown, kind: keyof typeof SOURCES): SolarCloudEvidenceSample | null {
  const raw = object(value);
  const source = SOURCES[kind];
  // unrelated stations and missing original identities cannot establish a correction
  if (raw === null || raw.providerKey !== source.providerKey || raw.sourceKind !== source.sourceKind ||
    raw.sourceKey !== source.sourceKey || raw.stationSlug !== source.stationSlug ||
    typeof raw.recordId !== "string" || raw.recordId.trim().length === 0 || raw.recordId.length > 512 ||
    typeof raw.sourceId !== "string" || raw.sourceId.trim().length === 0 || raw.sourceId.length > 512 ||
    typeof raw.value !== "number" || !Number.isFinite(raw.value) ||
    !Number.isFinite(timestamp(raw.validAt)) || !Number.isFinite(timestamp(raw.receivedAt))) {
    return null;
  }
  return { ...source, value: raw.value, validAt: raw.validAt as string, receivedAt: raw.receivedAt as string, recordId: raw.recordId, sourceId: raw.sourceId };
}

// rebuild a daytime estimate at its original reception time with an immutable expiry
export function solarCloudBiasFromEvidence(value: unknown, site: Pick<WeatherSite, "latitude" | "longitude">, now = new Date()): SolarCloudBias | null {
  const raw = object(value);
  const ws90 = sample(raw?.ws90, "ws90");
  const regional = sample(raw?.regional, "regional");
  const evaluatedAt = timestamp(raw?.evaluatedAt);
  const nowMs = now.getTime();
  // rehydration cannot renew age or accept future reception and forged evaluation times
  if (ws90 === null || regional === null || !Number.isFinite(nowMs) || !Number.isFinite(evaluatedAt) ||
    evaluatedAt !== Math.max(timestamp(ws90.receivedAt), timestamp(regional.receivedAt)) ||
    evaluatedAt > nowMs || timestamp(ws90.validAt) + DAY_MS <= nowMs) {
    return null;
  }
  return createSolarCloudBias({
    ...site, now: new Date(evaluatedAt).toISOString(),
    ws90: { ...ws90, freshnessStatus: "fresh" },
    regional: { ...regional, freshnessStatus: "fresh" },
  });
}

// validate the compact optional web envelope and discard all unrelated metadata
export function parseSolarCloudEvidenceResponse(value: unknown, site: Pick<WeatherSite, "latitude" | "longitude">, now = new Date()): SolarCloudEvidence | null {
  const raw = object(value);
  const generatedAt = timestamp(raw?.generatedAt);
  // the versioned queryless response must identify its real generation time
  if (raw?.contractVersion !== "solar-cloud-evidence/v1" || !Number.isFinite(generatedAt) || generatedAt > now.getTime()) {
    throw new RangeError("Solar cloud evidence envelope is invalid");
  }
  // a complete archive with no qualifying daylight pair is a valid raw fallback
  if (raw.evidence === null) {
    return null;
  }
  const bias = solarCloudBiasFromEvidence(raw.evidence, site, now);
  const evidence = object(raw.evidence);
  // expired or invalid pairs fail closed without trusting a serialized bias
  if (bias === null || evidence === null || generatedAt < timestamp(evidence.evaluatedAt)) {
    return null;
  }
  return { evaluatedAt: bias.evaluatedAt, ws90: sample(evidence.ws90, "ws90")!, regional: sample(evidence.regional, "regional")! };
}

// extract only original scalar values and closed source identities from normalized history
function recordSample(record: WeatherRecord, kind: keyof typeof SOURCES): SolarCloudEvidenceSample | null {
  return sample({ ...record.provenance, recordId: record.id, validAt: record.validAt, receivedAt: record.receivedAt,
    value: kind === "ws90" ? record.metrics.solarRadiationWm2 : record.metrics.cloudCoverPercent }, kind);
}

// find the newest viable daylight pair with a bounded search over complete history
export function buildSolarCloudEvidence(records: readonly WeatherRecord[], site: Pick<WeatherSite, "latitude" | "longitude">, through: string): SolarCloudEvidence | null {
  const nowMs = timestamp(through);
  // refuse incomplete or unbounded input rather than silently choosing an older correction
  if (!Number.isFinite(nowMs) || records.length > 20_000) {
    throw new RangeError("Solar cloud evidence history limit is invalid");
  }
  // retain only genuinely published samples inside the still-live observation horizon
  const eligible = (kind: keyof typeof SOURCES): SolarCloudEvidenceSample[] => records.flatMap(
    // history freshness labels describe today and must not replace original timestamp checks
    (record) => {
      const value = recordSample(record, kind);
      // source eligibility and original receipt chronology precede every pairing attempt
      if (value === null || timestamp(value.validAt) >= nowMs || timestamp(value.receivedAt) > nowMs ||
        timestamp(value.receivedAt) < timestamp(value.validAt) || timestamp(value.validAt) < nowMs - DAY_MS - PAIR_GAP_MS) {
        return [];
      }
      return [value];
    },
  );
  const ws90 = eligible("ws90").sort(
    // newest observations and their newest original revisions take precedence
    (left, right) => timestamp(right.validAt) - timestamp(left.validAt) || timestamp(right.receivedAt) - timestamp(left.receivedAt) || left.recordId.localeCompare(right.recordId),
  );
  const regional = eligible("regional").sort(
    // ascending model times permit a logarithmic entry into each bounded pairing window
    (left, right) => timestamp(left.validAt) - timestamp(right.validAt) || timestamp(right.receivedAt) - timestamp(left.receivedAt) || left.recordId.localeCompare(right.recordId),
  );
  let comparisons = 0;
  // stop at the newest qualifying ws90 observation including a genuine zero correction
  for (const observed of ws90) {
    let lower = 0;
    let upper = regional.length;
    // binary search avoids rescanning the entire regional archive for every sensor minute
    while (lower < upper) {
      const middle = Math.floor((lower + upper) / 2);
      // skip model samples outside the left edge of the original pairing window
      if (timestamp(regional[middle]!.validAt) < timestamp(observed.validAt) - PAIR_GAP_MS) {
        lower = middle + 1;
      } else {
        upper = middle;
      }
    }
    let best: SolarCloudEvidence | null = null;
    // inspect only nearby model intervals and refuse adversarial revision growth
    for (let index = lower; index < regional.length && timestamp(regional[index]!.validAt) <= timestamp(observed.validAt) + PAIR_GAP_MS; index += 1) {
      comparisons += 1;
      // exhausting the budget cannot publish a falsely complete newest-pair claim
      if (comparisons > MAX_PAIR_COMPARISONS) {
        throw new RangeError("Solar cloud evidence pairing limit exceeded");
      }
      const baseline = regional[index]!;
      const evidence = { evaluatedAt: new Date(Math.max(timestamp(observed.receivedAt), timestamp(baseline.receivedAt))).toISOString(), ws90: observed, regional: baseline };
      // re-evaluate daylight original ages and expiry with the same estimator as the browser
      if (solarCloudBiasFromEvidence(evidence, site, new Date(nowMs)) !== null &&
        (best === null || timestamp(evidence.evaluatedAt) > timestamp(best.evaluatedAt) ||
          (evidence.evaluatedAt === best.evaluatedAt && timestamp(baseline.validAt) > timestamp(best.regional.validAt)))) {
        best = evidence;
      }
    }
    // all revisions at this newest original observation have deterministic precedence
    if (best !== null) {
      return best;
    }
  }
  return null;
}
