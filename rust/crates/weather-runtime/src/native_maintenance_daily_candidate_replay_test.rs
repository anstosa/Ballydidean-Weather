use std::io::Write;
use std::process::{Command, Stdio};

use weather_edge::ArchiveWirePrimitives;

use super::*;

const NODE_ORACLE: &str = r#"
import { readFileSync } from "node:fs";
import {
  buildForecastAdjustmentMaintenancePortableCandidate,
  evaluateForecastAdjustmentMaintenanceNativeCandidate,
  evaluateForecastAdjustmentMaintenancePackagedCandidate,
} from "./scripts/research/adjustment_maintenance_runtime_adapter.mjs";
const payload = JSON.parse(readFileSync(0, "utf8"));
const candidateBytes = Buffer.from(payload.candidateBase64, "base64");
const artifactBytes = Buffer.from(payload.artifactBase64, "base64");
const portable = buildForecastAdjustmentMaintenancePortableCandidate({
  candidateBytes,
  family: "temperature",
});
const candidateDecision = JSON.parse(evaluateForecastAdjustmentMaintenanceNativeCandidate({
  candidateBytes,
  family: "temperature",
  input: payload.input,
}).toString("utf8"));
const packagedDecision = JSON.parse(evaluateForecastAdjustmentMaintenancePackagedCandidate({
  artifactBytes,
  family: "temperature",
  input: payload.input,
}).toString("utf8"));
process.stdout.write(JSON.stringify({
  artifactBase64: portable.artifactBytes.toString("base64"),
  candidateDecision,
  packagedDecision,
}));
"#;

const NODE_RAIN_ORACLE: &str = r#"
import { readFileSync } from "node:fs";
import { canonicalJsonBytes } from
  "./scripts/research/adjustment-maintenance-runtime/forecast/candidate.js";
import {
  buildForecastAdjustmentMaintenancePortableCandidate,
  evaluateForecastAdjustmentMaintenanceNativeCandidate,
  evaluateForecastAdjustmentMaintenancePackagedCandidate,
} from "./scripts/research/adjustment_maintenance_runtime_adapter.mjs";
const source = readFileSync(
  "packages/forecast-adjustment/src/rain-hurdle-wind-artifact.ts", "utf8");
const match = source.match(/RAIN_HURDLE_WIND_ARTIFACT_JSON = (.*) as const;\n$/su);
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
const features = Array.from({ length: runtime.featureNames.length }, () => null);
features[5] = 0.2;
const input = {
  contractVersion: "rain-maintenance-feature-parity-input/v1",
  featureNames: runtime.featureNames,
  features,
};
const nativeOutput = evaluateForecastAdjustmentMaintenanceNativeCandidate({
  candidateBytes,
  family: "rain",
  input,
});
const packagedOutput = evaluateForecastAdjustmentMaintenancePackagedCandidate({
  artifactBytes: portable.artifactBytes,
  family: "rain",
  input,
});
const b64 = (value) => Buffer.from(value).toString("base64");
process.stdout.write(JSON.stringify({
  artifact: b64(portable.artifactBytes),
  candidate: b64(candidateBytes),
  input: b64(canonicalJsonBytes(input)),
  nativeOutput: b64(nativeOutput),
  packagedOutput: b64(packagedOutput),
}));
"#;

/// Build one genuine selected permanent fit from the retained runtime bundle.
fn temperature_fit(repo: &std::path::Path) -> Vec<u8> {
    let wire = NativeWirePrimitives;
    let bundle = std::fs::read(
        repo.join("config/forecast-adjustments/ballydidean/temperature-canary-bundles/sha256-4d4e229b42823e53d2db062ec18c625bb2d2378a8a46d641fa95fabb59501b0e.json"),
    )
    .expect("read retained temperature bundle");
    let bundle = wire.decode_json(&bundle).expect("decode retained bundle");
    let model = field(&bundle, "model").expect("retained model").clone();
    wire.encode_sorted_json(&object(vec![
        ("arms", object(vec![])),
        ("confirmationOpened", JsonValue::Boolean(false)),
        ("contractVersion", string("temperature-maintenance-fit/v2")),
        ("developmentIncumbentMae", JsonValue::Number(1.0)),
        ("developmentRawMae", JsonValue::Number(2.0)),
        ("dueMonth", string("2026-10")),
        ("model", model),
        ("selectedArm", string("month_start_expanding")),
        ("servingChanged", JsonValue::Boolean(false)),
        ("state", string("development_candidate")),
    ]))
    .expect("encode exact fit")
}

/// Build the source-shaped evaluator input retained by the v3 daily chunk decoder.
fn temperature_input() -> JsonValue {
    let source_keys = (0..50)
        .map(|index| string(&format!("source-{index}")))
        .collect();
    object(vec![
        ("evaluatedAt", string("2026-10-09T13:35:00.000Z")),
        ("rawBestMatchTemperatureC", JsonValue::Number(17.2)),
        (
            "recentErrorState",
            object(vec![
                ("b24C", JsonValue::Number(0.8)),
                ("b72C", JsonValue::Number(0.3)),
                ("cohort", string("ecmwf_single_run_hindcast")),
                ("localDates", JsonValue::Number(1.0)),
                ("mad72C", JsonValue::Number(0.4)),
                (
                    "maximumSourceRunInitializedAt",
                    string("2026-10-09T06:00:00.000Z"),
                ),
                ("maximumSourceValidAt", string("2026-10-09T12:00:00.000Z")),
                ("n24", JsonValue::Number(12.0)),
                ("n72", JsonValue::Number(50.0)),
                ("sourceKeys", JsonValue::Array(source_keys)),
                ("supported", JsonValue::Boolean(true)),
                ("targetRunInitializedAt", string("2026-10-09T06:00:00.000Z")),
                ("windowEndValidAt", string("2026-10-09T12:00:00.000Z")),
            ]),
        ),
        (
            "sourceForecast",
            object(vec![
                ("adapterVersion", string("open-meteo-ecmwf-single-run/v1")),
                ("dataset", string("single_run")),
                ("firstReceivedAt", string("2026-10-09T13:30:00.000Z")),
                ("modelCycle", string("50r1")),
                ("modelLeadHours", JsonValue::Number(8.0)),
                ("providerKey", string("open-meteo")),
                (
                    "providerResponseSha256",
                    string("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
                ),
                ("rawRelativeHumidityPercent", JsonValue::Number(78.0)),
                ("rawTemperatureC", JsonValue::Number(16.4)),
                ("rawWindSpeedMps", JsonValue::Number(3.2)),
                ("runInitializedAt", string("2026-10-09T06:00:00.000Z")),
                ("upstreamModel", string("ecmwf_ifs")),
                ("validAt", string("2026-10-09T14:00:00.000Z")),
            ]),
        ),
        ("validAt", string("2026-10-09T14:00:00.000Z")),
    ])
}

/// Compare both native replay lanes and portable bytes with the actual predecessor adapter.
#[test]
fn temperature_candidate_and_package_replay_match_node() {
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let wire = NativeWirePrimitives;
    let candidate = temperature_fit(&repo);
    let input = temperature_input();
    let (artifact, _, _) = build_native_temperature_maintenance_package(&candidate)
        .expect("build native portable package");
    let replay = replay_native_temperature_daily_candidate_pair(&input, &candidate, &artifact)
        .expect("replay both native paths");
    let payload = object(vec![
        (
            "artifactBase64",
            string(
                &wire
                    .encode_standard_base64(&artifact)
                    .expect("artifact base64"),
            ),
        ),
        (
            "candidateBase64",
            string(
                &wire
                    .encode_standard_base64(&candidate)
                    .expect("candidate base64"),
            ),
        ),
        ("input", input),
    ]);
    let payload = wire
        .encode_sorted_json(&payload)
        .expect("encode oracle input");
    let mut child = Command::new("node")
        .current_dir(&repo)
        .args(["--input-type=module", "--eval", NODE_ORACLE])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("start Node oracle");
    child
        .stdin
        .take()
        .expect("oracle stdin")
        .write_all(&payload)
        .expect("write oracle input");
    let output = child.wait_with_output().expect("wait for Node oracle");
    assert!(output.status.success(), "Node oracle failed");
    let actual = wire
        .decode_json(&output.stdout)
        .expect("decode Node output");
    let candidate_decision = wire
        .decode_json(&replay.candidate_decision_bytes)
        .expect("decode native candidate decision");
    let packaged_decision = wire
        .decode_json(&replay.packaged_decision_bytes)
        .expect("decode native package decision");
    let expected = object(vec![
        (
            "artifactBase64",
            string(
                &wire
                    .encode_standard_base64(&replay.portable_candidate_bytes)
                    .expect("native artifact base64"),
            ),
        ),
        ("candidateDecision", candidate_decision),
        ("packagedDecision", packaged_decision),
    ]);
    assert_eq!(actual, expected);
}

/// Reuse the genuine source oracle to prove the daily wind binding retains exact bytes.
#[test]
fn wind_candidate_and_package_replay_match_node() {
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let fixture_source = std::fs::read_to_string(
        repo.join("rust/crates/weather-runtime/src/native_maintenance_candidate_parity_test.rs"),
    )
    .expect("read shared wind oracle source");
    let script = fixture_source
        .split_once("const SOURCE_FIXTURE: &str = r#\"")
        .and_then(|(_, tail)| tail.rsplit_once("\"#;").map(|(script, _)| script))
        .expect("extract shared wind oracle");
    let output = Command::new("node")
        .current_dir(&repo)
        .args(["--input-type=module", "--eval", script])
        .output()
        .expect("run shared wind oracle");
    assert!(
        output.status.success(),
        "wind oracle failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let wire = NativeWirePrimitives;
    let fixture = wire
        .decode_json(&output.stdout)
        .expect("decode wind oracle");
    let fixture_bytes = |name: &str| {
        let encoded = text(&fixture, name).expect("wind oracle byte field");
        wire.decode_standard_base64(&encoded)
            .expect("decode wind oracle bytes")
    };
    let candidate = fixture_bytes("candidate");
    let artifact = fixture_bytes("artifact");
    let input = wire
        .decode_json(&fixture_bytes("retainedInput"))
        .expect("decode retained wind input");
    let replay = replay_native_wind_daily_candidate_pair(&input, &candidate, &artifact)
        .expect("replay both wind paths");

    assert_eq!(replay.portable_candidate_bytes, artifact);
    assert_eq!(
        replay.candidate_decision_bytes,
        fixture_bytes("retainedNativeOutput")
    );
    assert_eq!(
        replay.packaged_decision_bytes,
        fixture_bytes("retainedPackagedOutput")
    );
}

/// Compare the projected raw rain artifact and both feature replay lanes with Node.
#[test]
fn rain_candidate_and_package_feature_replay_match_node() {
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let output = Command::new("node")
        .current_dir(&repo)
        .args(["--input-type=module", "--eval", NODE_RAIN_ORACLE])
        .output()
        .expect("run rain oracle");
    assert!(
        output.status.success(),
        "rain oracle failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let wire = NativeWirePrimitives;
    let fixture = wire
        .decode_json(&output.stdout)
        .expect("decode rain oracle");
    let fixture_bytes = |name: &str| {
        let encoded = text(&fixture, name).expect("rain oracle byte field");
        wire.decode_standard_base64(&encoded)
            .expect("decode rain oracle bytes")
    };
    let candidate = fixture_bytes("candidate");
    let artifact = fixture_bytes("artifact");
    let input = wire
        .decode_json(&fixture_bytes("input"))
        .expect("decode rain feature input");
    let replay = replay_native_rain_feature_candidate_pair(&input, &candidate, &artifact)
        .expect("replay both rain feature paths");

    assert_eq!(replay.portable_candidate_bytes, artifact);
    assert_eq!(
        replay.candidate_decision_bytes,
        fixture_bytes("nativeOutput")
    );
    assert_eq!(
        replay.packaged_decision_bytes,
        fixture_bytes("packagedOutput")
    );
}
