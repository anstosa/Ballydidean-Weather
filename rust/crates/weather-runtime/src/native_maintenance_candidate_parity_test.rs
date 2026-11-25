use std::path::Path;
use std::process::Command;
use std::sync::OnceLock;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use weather_contracts::JsonValue;
use weather_edge::ArchiveWirePrimitives;

use crate::native_maintenance_candidate_parity::{
    build_native_maintenance_candidate_parity, build_native_maintenance_fit_parity_inputs,
};
use crate::{
    NativeMaintenanceCandidateParityRequest, NativeMaintenanceCaptureFamily, NativeWirePrimitives,
};

/// Match candidate packaging, fit parity inputs, model outputs, and receipt bytes to source Node.
#[test]
fn native_wind_candidate_parity_matches_source() {
    let fixture = source_fixture();
    let candidate = decode_fixture_bytes(fixture, "candidate");
    let fit_input = decode_fixture_json(fixture, "fitInput");
    let inputs = build_native_maintenance_fit_parity_inputs(
        NativeMaintenanceCaptureFamily::Wind,
        &fit_input,
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        inputs.retained_input_bytes,
        decode_fixture_bytes(fixture, "retainedInput")
    );
    assert_eq!(
        inputs.synthetic_input_bytes,
        decode_fixture_bytes(fixture, "syntheticInput")
    );
    assert_eq!(
        inputs.retained_input_sha256,
        fixture_text(fixture, "retainedInputSha256")
    );
    assert_eq!(
        inputs.synthetic_input_sha256,
        fixture_text(fixture, "syntheticInputSha256")
    );
    let material =
        build_native_maintenance_candidate_parity(NativeMaintenanceCandidateParityRequest {
            candidate_bytes: &candidate,
            family: NativeMaintenanceCaptureFamily::Wind,
            retained_input: Some(&inputs.retained_input),
            synthetic_input: Some(&inputs.synthetic_input),
        })
        .unwrap()
        .unwrap();
    assert_eq!(
        material.portable.artifact_bytes,
        decode_fixture_bytes(fixture, "artifact")
    );
    assert_eq!(
        material.portable.artifact_sha256,
        fixture_text(fixture, "artifactSha256")
    );
    assert_eq!(
        material.portable.candidate_sha256,
        fixture_text(fixture, "candidateSha256")
    );
    assert_eq!(
        material.portable.source_identity_sha256,
        fixture_text(fixture, "sourceIdentitySha256")
    );
    assert_eq!(
        material.evidence.retained_native_output,
        decode_fixture_bytes(fixture, "retainedNativeOutput")
    );
    assert_eq!(
        material.evidence.retained_packaged_output,
        decode_fixture_bytes(fixture, "retainedPackagedOutput")
    );
    assert_eq!(
        material.evidence.synthetic_native_output,
        decode_fixture_bytes(fixture, "syntheticNativeOutput")
    );
    assert_eq!(
        material.evidence.synthetic_packaged_output,
        decode_fixture_bytes(fixture, "syntheticPackagedOutput")
    );
    assert_eq!(
        material.receipt.bytes,
        decode_fixture_bytes(fixture, "receipt")
    );
    assert_eq!(
        material.receipt.parity_sha256,
        fixture_text(fixture, "paritySha256")
    );
    assert_eq!(material.due_parity().value, material.receipt.fixture);
}

/// Match temperature fit selection, package identities, decisions, and receipt to source Node.
#[test]
fn native_temperature_candidate_parity_matches_source() {
    let fixture = source_fixture();
    let candidate = decode_fixture_bytes(fixture, "temperatureCandidate");
    let fit_input = decode_fixture_json(fixture, "temperatureFitInput");
    let inputs = build_native_maintenance_fit_parity_inputs(
        NativeMaintenanceCaptureFamily::Temperature,
        &fit_input,
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        inputs.retained_input_bytes,
        decode_fixture_bytes(fixture, "temperatureRetainedInput")
    );
    assert_eq!(
        inputs.synthetic_input_bytes,
        decode_fixture_bytes(fixture, "temperatureSyntheticInput")
    );
    let material =
        build_native_maintenance_candidate_parity(NativeMaintenanceCandidateParityRequest {
            candidate_bytes: &candidate,
            family: NativeMaintenanceCaptureFamily::Temperature,
            retained_input: Some(&inputs.retained_input),
            synthetic_input: Some(&inputs.synthetic_input),
        })
        .unwrap()
        .unwrap();
    assert_eq!(
        material.portable.artifact_bytes,
        decode_fixture_bytes(fixture, "temperatureArtifact")
    );
    assert_eq!(
        material.portable.artifact_sha256,
        fixture_text(fixture, "temperatureArtifactSha256")
    );
    assert_eq!(
        material.portable.source_identity_sha256,
        fixture_text(fixture, "temperatureSourceIdentitySha256")
    );
    assert_eq!(
        material.evidence.retained_native_output,
        decode_fixture_bytes(fixture, "temperatureRetainedNativeOutput")
    );
    assert_eq!(
        material.evidence.synthetic_packaged_output,
        decode_fixture_bytes(fixture, "temperatureSyntheticPackagedOutput")
    );
    assert_eq!(
        material.receipt.bytes,
        decode_fixture_bytes(fixture, "temperatureReceipt")
    );
    assert_eq!(
        material.receipt.parity_sha256,
        fixture_text(fixture, "temperatureParitySha256")
    );
}

/// Match rain v3 fit selection and bounded feature perturbation to source Node.
#[test]
fn native_rain_fit_parity_inputs_match_source() {
    let fixture = source_fixture();
    let fit_input = decode_fixture_json(fixture, "rainFitInput");
    let inputs = build_native_maintenance_fit_parity_inputs(
        NativeMaintenanceCaptureFamily::Rain,
        &fit_input,
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        inputs.retained_input_bytes,
        decode_fixture_bytes(fixture, "rainRetainedInput")
    );
    assert_eq!(
        inputs.synthetic_input_bytes,
        decode_fixture_bytes(fixture, "rainSyntheticInput")
    );
    assert_eq!(
        inputs.retained_input_sha256,
        fixture_text(fixture, "rainRetainedInputSha256")
    );
    assert_eq!(
        inputs.synthetic_input_sha256,
        fixture_text(fixture, "rainSyntheticInputSha256")
    );
}

/// Match rain raw-fit packaging, feature decisions, identities, and receipt to source Node.
#[test]
fn native_rain_candidate_parity_matches_source() {
    let fixture = source_fixture();
    let candidate = decode_fixture_bytes(fixture, "rainCandidate");
    let fit_input = decode_fixture_json(fixture, "rainFitInput");
    let inputs = build_native_maintenance_fit_parity_inputs(
        NativeMaintenanceCaptureFamily::Rain,
        &fit_input,
    )
    .unwrap()
    .unwrap();
    let material =
        build_native_maintenance_candidate_parity(NativeMaintenanceCandidateParityRequest {
            candidate_bytes: &candidate,
            family: NativeMaintenanceCaptureFamily::Rain,
            retained_input: Some(&inputs.retained_input),
            synthetic_input: Some(&inputs.synthetic_input),
        })
        .unwrap()
        .unwrap();
    assert_eq!(
        material.portable.artifact_bytes,
        decode_fixture_bytes(fixture, "rainArtifact")
    );
    assert_eq!(
        material.portable.artifact_sha256,
        fixture_text(fixture, "rainArtifactSha256")
    );
    assert_eq!(
        material.portable.source_identity_sha256,
        fixture_text(fixture, "rainSourceIdentitySha256")
    );
    assert_eq!(
        material.evidence.retained_native_output,
        decode_fixture_bytes(fixture, "rainRetainedNativeOutput")
    );
    assert_eq!(
        material.evidence.synthetic_packaged_output,
        decode_fixture_bytes(fixture, "rainSyntheticPackagedOutput")
    );
    assert_eq!(
        material.receipt.bytes,
        decode_fixture_bytes(fixture, "rainReceipt")
    );
    assert_eq!(
        material.receipt.parity_sha256,
        fixture_text(fixture, "rainParitySha256")
    );
}

/// Generate one genuine selected wind fit and the predecessor's complete parity result.
fn source_fixture() -> &'static JsonValue {
    static FIXTURE: OnceLock<JsonValue> = OnceLock::new();
    FIXTURE.get_or_init(|| {
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let output = Command::new("node")
            .current_dir(repo)
            .args(["--input-type=module", "--eval", SOURCE_FIXTURE])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "source fixture failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        NativeWirePrimitives.decode_json(&output.stdout).unwrap()
    })
}

/// Decode one standard-base64 byte field from the source fixture.
fn decode_fixture_bytes(fixture: &JsonValue, name: &str) -> Vec<u8> {
    STANDARD.decode(fixture_text(fixture, name)).unwrap()
}

/// Decode one canonical JSON byte field from the source fixture.
fn decode_fixture_json(fixture: &JsonValue, name: &str) -> JsonValue {
    NativeWirePrimitives
        .decode_json(&decode_fixture_bytes(fixture, name))
        .unwrap()
}

/// Read one unique UTF-16 string field from the source fixture.
fn fixture_text(fixture: &JsonValue, name: &str) -> String {
    let JsonValue::Object(fields) = fixture else {
        panic!("fixture is not an object")
    };
    let JsonValue::String(value) = fields
        .iter()
        .find_map(|(key, value)| key.iter().copied().eq(name.encode_utf16()).then_some(value))
        .unwrap()
    else {
        panic!("fixture field is not text")
    };
    String::from_utf16(value).unwrap()
}

const SOURCE_FIXTURE: &str = r#"
import { readFile } from "node:fs/promises";
import {
  FORECAST_ADJUSTMENT_CANONICAL_FORECAST_IDENTITY_V1,
  FORECAST_ADJUSTMENT_CANONICAL_TRAINING_PROVENANCE_V1,
  canonicalJsonBytes,
  canonicalSha256,
  createForecastAdjustmentCandidate,
} from "./scripts/research/adjustment-maintenance-runtime/forecast/candidate.js";
import {
  createDevelopmentReport,
  evaluateDevelopmentLosoFold,
} from "./scripts/research/adjustment-maintenance-runtime/forecast/evaluate.js";
import { runtimeCalendarFingerprint } from
  "./scripts/research/adjustment-maintenance-runtime/forecast/calendar.js";
import {
  buildForecastAdjustmentMaintenanceFitParityInputs,
} from "./scripts/research/adjustment_maintenance_runtime_adapter.mjs";
import {
  buildAdjustmentMaintenanceCandidateParity,
  buildAdjustmentMaintenanceParityReceipt,
} from "./scripts/research/adjustment_maintenance_controller.mjs";
import {
  RAIN_HURDLE_WIND_ARTIFACT_JSON,
} from "./packages/forecast-adjustment/dist/rain-hurdle-wind-artifact.js";

const leadBands = ["001-024", "025-048", "049-072", "073-096", "097-120", "121-144", "145-168"];
const pairs = [
  ...leadBands.map((leadBand) => ({ leadBand, metric: "windSpeedMps" })),
  ...leadBands.filter((leadBand) => leadBand !== "049-072")
    .map((leadBand) => ({ leadBand, metric: "windGustMps" })),
];
const stationScores = [
  ["ambient-maxweather", "ambient"],
  ["ballydidean-ecowitt", "ecowitt"],
  ["netatmo-nearby", "netatmo"],
  ["tempest-126537", "tempest"],
  ["tempest-168853", "tempest"],
].map(([physicalStationKey, providerFamily]) => ({
  adjustedLoss: 9,
  eventCount: 100,
  physicalStationKey,
  pointSkill: 0.1,
  providerFamily,
  rawLoss: 10,
  remainingNetworkScoreEvents: 100,
  scoreMatches: 100,
  trainingMatches: 500,
}));
const developmentReport = createDevelopmentReport({
  enabledMetricBands: pairs,
  folds: pairs.flatMap((metricBand) => [1, 2, 3, 4, 5].map((fold) =>
    evaluateDevelopmentLosoFold({
      auxiliaryModelSha256s: ["1", "2", "3", "4", "5"].map((value) => value.repeat(64)),
      bootstrapLowerBound: 0.01,
      fold,
      materialHarmSliceKeys: [],
      metricBand,
      stationScores,
    }))),
});
const candidate = createForecastAdjustmentCandidate({
  coefficients: pairs.map((pair) => ({
    coefficient: 0.25,
    daypart: null,
    effectiveEventCount: 200,
    leadBand: pair.leadBand,
    level: 1,
    metric: pair.metric,
    month: null,
    season: null,
  })),
  developmentReportSha256: developmentReport.developmentReportSha256,
  enabledMetricBands: pairs,
  evaluationEpochId: "maintenance-2026-10",
  exportManifestSha256: canonicalSha256({ contractVersion: "fixture/v1" }),
  finalTrainingCutoff: "2026-09-01T06:59:59.999Z",
  forecastIdentity: FORECAST_ADJUSTMENT_CANONICAL_FORECAST_IDENTITY_V1,
  runtimeFingerprint: runtimeCalendarFingerprint(),
  trainingEnvelopes: pairs.map((pair) => ({
    leadBand: pair.leadBand,
    maximum: 40,
    metric: pair.metric,
    minimum: 0,
  })),
  trainingProvenance: FORECAST_ADJUSTMENT_CANONICAL_TRAINING_PROVENANCE_V1,
});
const candidateBytes = Buffer.from(canonicalJsonBytes({
  candidate,
  confirmationOpened: false,
  contractVersion: "wind-maintenance-fit/v2",
  developmentReport,
  dueMonth: "2026-10",
  state: "development_candidate",
}));
const metrics = Object.fromEntries([
  "apparentTemperatureC", "blackGlobeTemperatureC", "cloudCoverPercent",
  "pm25MicrogramsPerCubicMeter", "precipitationMm", "precipitationRateMmPerHour",
  "pressureHpa", "relativeHumidityPercent", "soilElectricalConductivityMicrosiemensPerCm",
  "soilMoisturePercent", "solarRadiationWm2", "temperatureC", "uvIndex", "waterLevelM",
  "wetBulbGlobeTemperatureC", "windDirectionDegrees", "windGustMps", "windSpeedMps",
].map((metric) => [metric, null]));
metrics.windGustMps = 8;
metrics.windSpeedMps = 5;
const retained = {
  evaluatedAt: "2026-10-08T00:00:00.000Z",
  metrics,
  rawForecastProvenance: {
    ...FORECAST_ADJUSTMENT_CANONICAL_FORECAST_IDENTITY_V1,
    referenceAt: "2026-10-08T00:00:00.000Z",
    targetLeadHours: 1,
    validAt: "2026-10-08T01:00:00.000Z",
  },
};
const fitInput = {
  contractVersion: "wind-maintenance-fit-input/v2",
  rows: [{
    adapterContracts: [retained.rawForecastProvenance.adapterVersion],
    contentHashes: ["f".repeat(64)],
    contractEpoch: retained.rawForecastProvenance.contractEpoch,
    dataset: retained.rawForecastProvenance.dataset,
    metrics: { windGustMps: 8, windSpeedMps: 5 },
    receivedAt: retained.evaluatedAt,
    recordKind: "legacy_v4_retrieval_snapshot",
    referenceAt: retained.rawForecastProvenance.referenceAt,
    sourceConfigFingerprints: [retained.rawForecastProvenance.sourceConfigFingerprint],
    sourceKeys: [retained.rawForecastProvenance.sourceKey],
    targetLeadHours: retained.rawForecastProvenance.targetLeadHours,
    upstreamModel: retained.rawForecastProvenance.upstreamModel,
    validAt: retained.rawForecastProvenance.validAt,
  }],
};
const inputs = buildForecastAdjustmentMaintenanceFitParityInputs({ family: "wind", fitInput });
const result = buildAdjustmentMaintenanceCandidateParity({
  candidateBytes,
  family: "wind",
  retainedInput: inputs.retainedInput,
  syntheticInput: inputs.syntheticInput,
});
const receipt = buildAdjustmentMaintenanceParityReceipt({
  candidateSha256: result.portable.candidateSha256,
  family: "wind",
  parity: result.parity,
});
const temperatureInstalled = JSON.parse(await readFile(
  "./config/forecast-adjustments/ballydidean/temperature-canary-bundles/" +
    "sha256-4d4e229b42823e53d2db062ec18c625bb2d2378a8a46d641fa95fabb59501b0e.json",
  "utf8",
));
const temperatureCandidate = Buffer.from(canonicalJsonBytes({
  arms: {},
  confirmationOpened: false,
  contractVersion: "temperature-maintenance-fit/v2",
  developmentIncumbentMae: 1,
  developmentRawMae: 2,
  dueMonth: "2026-10",
  model: temperatureInstalled.model,
  selectedArm: "month_start_expanding",
  servingChanged: false,
  state: "development_candidate",
}));
const temperatureRetained = {
  evaluatedAt: "2026-10-08T06:10:00.000Z",
  rawBestMatchTemperatureC: 17.2,
  recentErrorState: {
    b24C: 0.8,
    b72C: 0.3,
    cohort: "ecmwf_single_run_hindcast",
    localDates: 3,
    mad72C: 0.4,
    maximumSourceRunInitializedAt: "2026-10-07T00:00:00.000Z",
    maximumSourceValidAt: "2026-10-07T17:00:00.000Z",
    n24: 12,
    n72: 50,
    sourceKeys: Array.from({ length: 50 }, (_unused, index) => `source-${index}`),
    supported: true,
    targetRunInitializedAt: "2026-10-08T00:00:00.000Z",
    windowEndValidAt: "2026-10-07T17:00:00.000Z",
  },
  sourceForecast: {
    adapterVersion: "open-meteo-ecmwf-single-run/v1",
    dataset: "single_run",
    firstReceivedAt: "2026-10-08T06:05:00.000Z",
    modelCycle: "50r1",
    modelLeadHours: 7,
    providerKey: "open-meteo",
    providerResponseSha256: "e".repeat(64),
    rawRelativeHumidityPercent: 78,
    rawTemperatureC: 16.4,
    rawWindSpeedMps: 3.2,
    runInitializedAt: "2026-10-08T00:00:00.000Z",
    upstreamModel: "ecmwf_ifs",
    validAt: "2026-10-08T07:00:00.000Z",
  },
  validAt: "2026-10-08T07:00:00.000Z",
};
const temperatureFitInput = {
  contractVersion: "temperature-maintenance-fit-input/v2",
  developmentRows: [],
  trainingRows: [{
    adapterVersion: temperatureRetained.sourceForecast.adapterVersion,
    firstReceivedAt: temperatureRetained.sourceForecast.firstReceivedAt,
    key: "temperature-row",
    modelCycle: temperatureRetained.sourceForecast.modelCycle,
    modelLeadHours: temperatureRetained.sourceForecast.modelLeadHours,
    providerResponseSha256: temperatureRetained.sourceForecast.providerResponseSha256,
    rawBestMatchTemperatureC: temperatureRetained.rawBestMatchTemperatureC,
    rawRelativeHumidityPercent: temperatureRetained.sourceForecast.rawRelativeHumidityPercent,
    rawTemperatureC: temperatureRetained.sourceForecast.rawTemperatureC,
    rawWindSpeedMps: temperatureRetained.sourceForecast.rawWindSpeedMps,
    runInitializedAt: temperatureRetained.sourceForecast.runInitializedAt,
    sourceReceiptAt: temperatureRetained.evaluatedAt,
    state: temperatureRetained.recentErrorState,
    validAt: temperatureRetained.validAt,
  }],
};
const temperatureInputs = buildForecastAdjustmentMaintenanceFitParityInputs({
  family: "temperature",
  fitInput: temperatureFitInput,
});
const temperatureResult = buildAdjustmentMaintenanceCandidateParity({
  candidateBytes: temperatureCandidate,
  family: "temperature",
  retainedInput: temperatureInputs.retainedInput,
  syntheticInput: temperatureInputs.syntheticInput,
});
const temperatureReceipt = buildAdjustmentMaintenanceParityReceipt({
  candidateSha256: temperatureResult.portable.candidateSha256,
  family: "temperature",
  parity: temperatureResult.parity,
});
const rainArtifact = JSON.parse(RAIN_HURDLE_WIND_ARTIFACT_JSON);
const rainFitInput = {
  contractVersion: "rain-maintenance-fit-input/v3",
  developmentRows: [],
  featureNames: rainArtifact.featureNames,
  trainingRows: [{
    features: Array.from({ length: 107 }, (_unused, index) => index === 5 ? 2 : 0),
    key: "rain-row",
    validAt: "2026-10-08T01:00:00.000Z",
  }],
};
const rainInputs = buildForecastAdjustmentMaintenanceFitParityInputs({
  family: "rain",
  fitInput: rainFitInput,
});
const rainCategories = Object.fromEntries(rainArtifact.categoryScales.map(
  (scale, index) => [String(index + 1), { scale }],
));
const rainCandidate = Buffer.from(canonicalJsonBytes({
  artifact: {
    calibration: {
      categories: rainCategories,
      contractVersion: "rain-hurdle-calibration/v1",
      rules: rainArtifact.rules,
    },
    contractVersion: "rain-maintenance-artifact/v2",
    featureNames: rainArtifact.featureNames,
    heads: rainArtifact.heads,
    modelMonth: rainArtifact.modelMonth,
    projectionId: "R1_winter_scale_0_90",
  },
  confirmationOpened: false,
  contractVersion: "rain-maintenance-fit/v3",
  reason: "development_gate_passer",
  state: "development_candidate",
}));
const rainResult = buildAdjustmentMaintenanceCandidateParity({
  candidateBytes: rainCandidate,
  family: "rain",
  retainedInput: rainInputs.retainedInput,
  syntheticInput: rainInputs.syntheticInput,
});
const rainReceipt = buildAdjustmentMaintenanceParityReceipt({
  candidateSha256: rainResult.portable.candidateSha256,
  family: "rain",
  parity: rainResult.parity,
});
const b64 = (value) => Buffer.from(value).toString("base64");
process.stdout.write(JSON.stringify({
  artifact: b64(result.portable.artifactBytes),
  artifactSha256: result.portable.artifactSha256,
  candidate: b64(candidateBytes),
  candidateSha256: result.portable.candidateSha256,
  fitInput: b64(Buffer.from(canonicalJsonBytes(fitInput))),
  paritySha256: receipt.paritySha256,
  receipt: b64(receipt.bytes),
  retainedInput: b64(inputs.retainedInputBytes),
  retainedInputSha256: inputs.retainedInputSha256,
  retainedNativeOutput: b64(result.parity.retainedNativeOutput),
  retainedPackagedOutput: b64(result.parity.retainedPackagedOutput),
  rainArtifact: b64(rainResult.portable.artifactBytes),
  rainArtifactSha256: rainResult.portable.artifactSha256,
  rainCandidate: b64(rainCandidate),
  rainFitInput: b64(Buffer.from(canonicalJsonBytes(rainFitInput))),
  rainParitySha256: rainReceipt.paritySha256,
  rainReceipt: b64(rainReceipt.bytes),
  rainRetainedInput: b64(rainInputs.retainedInputBytes),
  rainRetainedInputSha256: rainInputs.retainedInputSha256,
  rainRetainedNativeOutput: b64(rainResult.parity.retainedNativeOutput),
  rainSourceIdentitySha256: rainResult.portable.sourceIdentitySha256,
  rainSyntheticInput: b64(rainInputs.syntheticInputBytes),
  rainSyntheticInputSha256: rainInputs.syntheticInputSha256,
  rainSyntheticPackagedOutput: b64(rainResult.parity.syntheticPackagedOutput),
  sourceIdentitySha256: result.portable.sourceIdentitySha256,
  syntheticInput: b64(inputs.syntheticInputBytes),
  syntheticInputSha256: inputs.syntheticInputSha256,
  syntheticNativeOutput: b64(result.parity.syntheticNativeOutput),
  syntheticPackagedOutput: b64(result.parity.syntheticPackagedOutput),
  temperatureArtifact: b64(temperatureResult.portable.artifactBytes),
  temperatureArtifactSha256: temperatureResult.portable.artifactSha256,
  temperatureCandidate: b64(temperatureCandidate),
  temperatureFitInput: b64(Buffer.from(canonicalJsonBytes(temperatureFitInput))),
  temperatureParitySha256: temperatureReceipt.paritySha256,
  temperatureReceipt: b64(temperatureReceipt.bytes),
  temperatureRetainedInput: b64(temperatureInputs.retainedInputBytes),
  temperatureRetainedNativeOutput: b64(temperatureResult.parity.retainedNativeOutput),
  temperatureSourceIdentitySha256: temperatureResult.portable.sourceIdentitySha256,
  temperatureSyntheticInput: b64(temperatureInputs.syntheticInputBytes),
  temperatureSyntheticPackagedOutput: b64(temperatureResult.parity.syntheticPackagedOutput),
}));
"#;
