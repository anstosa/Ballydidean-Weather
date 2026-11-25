use std::path::Path;
use std::process::Command;

use weather_edge::ArchiveWirePrimitives;

use super::*;

const NODE_CAUSAL_ORACLE: &str = r#"
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import {
  canonicalJsonBytes,
  encodeMaintenanceBinary64,
  MAINTENANCE_SHADOW_SOURCE_VERSION,
} from "./packages/forecast-adjustment/dist/index.js";
import {
  buildForecastAdjustmentMaintenancePortableCandidate,
  evaluateForecastAdjustmentMaintenanceNativeCandidate,
  evaluateForecastAdjustmentMaintenancePackagedCandidate,
} from "./scripts/research/adjustment_maintenance_runtime_adapter.mjs";
const sourceText = readFileSync(
  "packages/forecast-adjustment/src/rain-hurdle-wind-artifact.ts", "utf8");
const match = sourceText.match(/RAIN_HURDLE_WIND_ARTIFACT_JSON = (.*) as const;\n$/su);
if (match === null) throw new Error("retained rain artifact is absent");
const runtime = JSON.parse(JSON.parse(match[1]));
const categories = Object.fromEntries(runtime.categoryScales.map(
  (scale, index) => [String(index + 1), { scale }],
));
const candidateBytes = Buffer.from(canonicalJsonBytes({
  artifact: {
    calibration: {
      categories,
      contractVersion: "rain-hurdle-calibration/v1",
      rules: runtime.rules,
    },
    contractVersion: "rain-maintenance-artifact/v2",
    featureNames: runtime.featureNames,
    heads: runtime.heads,
    modelMonth: runtime.modelMonth,
    projectionId: "R0_exact_refit",
  },
  confirmationOpened: false,
  contractVersion: "rain-maintenance-fit/v2",
  reason: "development_gate_passer",
  state: "development_candidate",
}));
const portable = buildForecastAdjustmentMaintenancePortableCandidate({
  candidateBytes,
  family: "rain",
});
const hash = "f".repeat(64);
const runInitializedAt = "2026-10-01T00:00:00.000Z";
const completedAt = "2026-10-01T06:01:00.000Z";
const issuedAt = "2026-10-01T08:35:00.000Z";
const contentSha256 = createHash("sha256").update("rain-current").digest("hex");
const hours = Array.from({ length: 48 }, (_unused, index) => ({
  cloudCoverPercent64: encodeMaintenanceBinary64(50),
  leadHours: index + 1,
  precipitationMm64: encodeMaintenanceBinary64(2),
  pressureHpa64: encodeMaintenanceBinary64(1_000),
  relativeHumidityPercent64: encodeMaintenanceBinary64(80),
  temperatureC64: encodeMaintenanceBinary64(12),
  windDirectionDegrees64: encodeMaintenanceBinary64(180),
  windSpeedMps64: encodeMaintenanceBinary64(5),
}));
const source = {
  candidateSha256: hash,
  causalInputs: {
    captureSet: [{
      bodySha256: contentSha256,
      claimId: "rain-current",
      completedAt,
      kind: "forecast",
      runInitializedAt,
      stationId: null,
      windowEndExclusive: null,
      windowStart: null,
    }],
    contractVersion: "adjustment-shadow-rain-causal-inputs/v1",
    currentRun: { completedAt, contentSha256, hours, runInitializedAt },
    priorRuns: [],
    stationHours: [],
  },
  contractVersion: MAINTENANCE_SHADOW_SOURCE_VERSION,
  dueKey: "capture/2026-10-01T06:35:00.000Z",
  family: "rain",
  issuedAt,
  registrationSha256: hash,
  rowCount: 23,
  rows: Array.from({ length: 23 }, (_unused, index) => {
    const modelLeadHours = index + 9;
    return {
      adapterVersion: "open-meteo-rain-capture/v1",
      contentSha256,
      contractEpoch: "rain-prospective-capture/v1",
      dataset: "ecmwf_ifs",
      leadHours: index + 1,
      modelLeadHours,
      precipitationMm64: encodeMaintenanceBinary64(2),
      providerKey: "open-meteo-single-runs",
      receivedAt: completedAt,
      referenceAt: runInitializedAt,
      revisionCount: 0,
      sourceConfigFingerprint: createHash("sha256")
        .update("rain-prospective-open-meteo/v1\necmwf_ifs\n49\n").digest("hex"),
      sourceKey: "rain-prospective-forecast",
      sourceSha256: hash,
      upstreamModel: "ecmwf_ifs",
      validAt: new Date(Date.parse(runInitializedAt) + modelLeadHours * 3_600_000)
        .toISOString(),
    };
  }),
  sourceSha256: hash,
};
const nativeOutput = evaluateForecastAdjustmentMaintenanceNativeCandidate({
  candidateBytes,
  family: "rain",
  input: source,
});
const packagedOutput = evaluateForecastAdjustmentMaintenancePackagedCandidate({
  artifactBytes: portable.artifactBytes,
  family: "rain",
  input: source,
});
const b64 = (value) => Buffer.from(value).toString("base64");
process.stdout.write(JSON.stringify({
  artifact: b64(portable.artifactBytes),
  candidate: b64(candidateBytes),
  nativeOutput: b64(nativeOutput),
  packagedOutput: b64(packagedOutput),
  source: b64(canonicalJsonBytes(source)),
}));
"#;

/// Match complete causal-source feature construction and both model paths to Node.
#[test]
fn causal_rain_replay_matches_node() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let output = Command::new("node")
        .current_dir(repo)
        .args(["--input-type=module", "--eval", NODE_CAUSAL_ORACLE])
        .output()
        .expect("run causal rain oracle");
    assert!(
        output.status.success(),
        "causal rain oracle failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let wire = NativeWirePrimitives;
    let fixture = wire
        .decode_json(&output.stdout)
        .expect("decode causal oracle");
    let bytes = |name: &str| {
        let encoded = text(&fixture, name).expect("causal oracle byte field");
        wire.decode_standard_base64(&encoded)
            .expect("decode causal oracle bytes")
    };
    let candidate = bytes("candidate");
    let artifact = bytes("artifact");
    let source = wire
        .decode_json(&bytes("source"))
        .expect("decode causal source");
    let replay = replay_native_rain_causal_candidate_pair(&source, &candidate, &artifact)
        .expect("replay causal rain pair");

    assert_eq!(replay.portable_candidate_bytes, artifact);
    assert_eq!(replay.candidate_decision_bytes, bytes("nativeOutput"));
    assert_eq!(replay.packaged_decision_bytes, bytes("packagedOutput"));
}
