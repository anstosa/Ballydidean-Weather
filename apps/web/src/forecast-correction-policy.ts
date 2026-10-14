export type ForecastCorrectionMetric = "uvIndex" | "pressureChange3hHpa" | "cloudCoverPercent";
export type ForecastCorrectionAnchorTier = "farm" | "local_mean" | "ws90";

export interface ForecastCorrectionPolicy {
  readonly strength: 0 | 0.25 | 0.5 | 0.75 | 1;
  readonly horizonHours: 3 | 6 | 12 | 24;
}

export type ForecastCorrectionPolicies = Readonly<Record<ForecastCorrectionMetric,
  Readonly<Partial<Record<ForecastCorrectionAnchorTier, ForecastCorrectionPolicy>>>>>;

const REGIONAL_POLICY: ForecastCorrectionPolicy = Object.freeze({ strength: 0, horizonHours: 3 });

// independent prospective evidence must precede a new nonzero code release
export const DEFAULT_FORECAST_CORRECTION_POLICIES: ForecastCorrectionPolicies = Object.freeze({
  uvIndex: Object.freeze({ farm: REGIONAL_POLICY, local_mean: REGIONAL_POLICY }),
  pressureChange3hHpa: Object.freeze({ farm: REGIONAL_POLICY, local_mean: REGIONAL_POLICY }),
  cloudCoverPercent: Object.freeze({ ws90: REGIONAL_POLICY }),
});

// accept only the frozen finite development grid
export function isForecastCorrectionPolicy(value: unknown): value is ForecastCorrectionPolicy {
  // malformed or unreviewed policies cannot project values
  if (value === null || typeof value !== "object") {
    return false;
  }
  const policy = value as Record<string, unknown>;
  return [0, 0.25, 0.5, 0.75, 1].includes(policy.strength as number) &&
    [3, 6, 12, 24].includes(policy.horizonHours as number);
}

// resolve only an explicitly supported internal anchor cell
export function forecastCorrectionPolicy(
  policies: ForecastCorrectionPolicies,
  metric: ForecastCorrectionMetric,
  tier: ForecastCorrectionAnchorTier,
): ForecastCorrectionPolicy {
  const policy = policies?.[metric]?.[tier];
  return isForecastCorrectionPolicy(policy) ? policy : REGIONAL_POLICY;
}

// taper once from the original observation time without renewing its age
export function forecastCorrectionWeight(target: number, observed: number, policy: unknown): number {
  // unsupported policies and nonforward targets are exact regional fallbacks
  if (!isForecastCorrectionPolicy(policy) || policy.strength === 0 ||
    !Number.isFinite(target) || !Number.isFinite(observed) || target < observed) {
    return 0;
  }
  return policy.strength * Math.max(0, 1 - (target - observed) / (policy.horizonHours * 3_600_000));
}
