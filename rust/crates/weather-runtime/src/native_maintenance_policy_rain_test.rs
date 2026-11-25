use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use weather_contracts::JsonValue;

use super::{
    STABLE_GATE_IDS, evaluate_native_rain_maintenance_policy,
    evaluate_native_rain_maintenance_promotion,
};
use crate::maintenance_canonical_json::field;
use crate::native_maintenance_policy_comparison::canonical_without;
use crate::{MaintenanceCanonicalJson, NativeMaintenancePolicyKind, NativeWirePrimitives};

const NODE_FIXTURE: &str = r#"
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { canonicalJsonBytes } from "./scripts/research/adjustment-maintenance-runtime/forecast/candidate.js";
import { localCalendarFeaturesFor } from "./scripts/research/adjustment-maintenance-runtime/forecast/calendar.js";
import {
  createRainMaintenanceEvaluationRow,
  evaluateRainMaintenancePromotion,
  evaluateRainMaintenanceRegression,
} from "./scripts/research/adjustment-maintenance-runtime/forecast/maintenance-policy.js";

const hash = "a".repeat(64);
const probability = { atLeast0_1: 0.9, atLeast1_0: 0.8, atLeast2_5: 0.1 };
const seasonStarts = [
  "2026-11-02T08:00:00.000Z",
  "2026-12-02T08:00:00.000Z",
  "2027-03-02T08:00:00.000Z",
  "2027-06-02T07:00:00.000Z",
];
const rows = seasonStarts.flatMap((seasonStart, seasonIndex) =>
  Array.from({ length: 7 }, (_unused, dayIndex) => {
    const run = Date.parse(seasonStart) + dayIndex * 86_400_000;
    return Array.from({ length: 23 }, (_hour, horizonIndex) => {
      const horizonHours = horizonIndex + 1;
      const validAt = new Date(run + horizonHours * 3_600_000).toISOString();
      const localDate = localCalendarFeaturesFor(validAt).localDate;
      return createRainMaintenanceEvaluationRow({
    actualBestMatchPrediction: 1.1,
    applied: true,
    candidatePrediction: 1,
    candidateProbability: probability,
    evidenceClass: "prospective_receipt",
    farmTarget: 20,
    firstEdgeCommittedAt: new Date(Date.parse(validAt) - 2 * 3_600_000).toISOString(),
    horizonHours,
    incumbentPrediction: 1.5,
    incumbentProbability: probability,
    key: `rain:s${seasonIndex}:d${dayIndex}:h${horizonHours}:${validAt}`,
    localDate,
    nativeSourcePrediction: 2,
    nativeSourceProbability: probability,
    nearestThree: null,
    persistencePrediction: 3,
    provenanceComplete: true,
    providerFamily: null,
    rawTargetHourTemperatureC: 3,
    recentVolumeScalePrediction: 3,
    runKey: `run:${seasonIndex}:${dayIndex}`,
    sameWindowVolumeScalePrediction: 3,
    sourceRowSha256: hash,
    sourceReceiptAt: new Date(Date.parse(validAt) - 3 * 3_600_000).toISOString(),
    stationKey: null,
    target: 1,
    targetRowSha256: hash,
    unchangedOrdinalPrediction: 1.2,
    validAt,
    volumeScalePrediction: 3,
      });
    });
  }).flat());
const epoch = {
  epochEndExclusive: "2027-09-30T07:00:00.000Z",
  epochStartInclusive: "2026-10-31T07:00:00.000Z",
  evaluatedAt: "2027-10-07T07:00:00.000Z",
  registeredAt: "2026-10-30T07:00:00.000Z",
};
const fixture = {
  epoch,
  promotion: evaluateRainMaintenancePromotion(rows),
  regression: evaluateRainMaintenanceRegression(rows, epoch),
  rows,
};
writeFileSync(join(process.env.WEATHER_RAIN_POLICY_FIXTURE_ROOT, "fixture.json"),
  canonicalJsonBytes(fixture));
"#;

/// Remove one bounded disposable source fixture root.
struct FixtureRoot(PathBuf);

impl FixtureRoot {
    /// Create one unique literal private fixture directory.
    fn create() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("fixture clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "weather-native-rain-policy-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir(&root).expect("create fixture root");
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700))
            .expect("protect fixture root");
        Self(root)
    }
}

impl Drop for FixtureRoot {
    /// Remove only the process-owned disposable fixture root.
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Generate source rows plus exact promotion and regression reports.
fn build_node_fixture(root: &Path) {
    let repository = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let output = Command::new("node")
        .args(["--input-type=module", "--eval", NODE_FIXTURE])
        .current_dir(repository)
        .env("WEATHER_RAIN_POLICY_FIXTURE_ROOT", root)
        .output()
        .expect("run predecessor rain policy fixture");
    // surface only the predecessor's bounded fixture diagnostic
    if !output.status.success() {
        panic!(
            "predecessor rain policy fixture failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

/// Replace one object member without changing its closed field set.
fn replace(value: &mut JsonValue, key: &str, replacement: JsonValue) {
    let JsonValue::Object(fields) = value else {
        panic!("fixture value is not an object");
    };
    let field = fields
        .iter_mut()
        .find(|(name, _)| name.iter().copied().eq(key.encode_utf16()))
        .expect("fixture field");
    field.1 = replacement;
}

/// Match every source gate and reject a correctly rehashed physically cold row.
#[test]
fn rain_policy_matches_source_promotion_and_regression_reports() {
    let fixture_root = FixtureRoot::create();
    build_node_fixture(&fixture_root.0);
    let mut wire = NativeWirePrimitives;
    let fixture = wire
        .parse_canonical_object(
            &fs::read(fixture_root.0.join("fixture.json")).expect("read rain fixture"),
        )
        .expect("parse rain fixture");
    let JsonValue::Array(rows) = field(&fixture, "rows").unwrap() else {
        panic!("rain rows are not an array");
    };
    let epoch = field(&fixture, "epoch").unwrap();
    let promotion =
        evaluate_native_rain_maintenance_policy(NativeMaintenancePolicyKind::Promotion, rows, None)
            .expect("evaluate native rain promotion");
    assert_eq!(
        wire.encode_canonical_json(&promotion).unwrap(),
        wire.encode_canonical_json(field(&fixture, "promotion").unwrap())
            .unwrap()
    );
    let regression = evaluate_native_rain_maintenance_policy(
        NativeMaintenancePolicyKind::Regression,
        rows,
        Some(epoch),
    )
    .expect("evaluate native rain regression");
    assert_eq!(
        wire.encode_canonical_json(&regression).unwrap(),
        wire.encode_canonical_json(field(&fixture, "regression").unwrap())
            .unwrap()
    );
    let JsonValue::Array(gates) = field(&promotion, "gates").unwrap() else {
        panic!("rain gates are not an array");
    };
    assert_eq!(
        gates
            .iter()
            .filter(|gate| {
                field(gate, "id").is_some_and(|value| {
                    matches!(value, JsonValue::String(units) if STABLE_GATE_IDS.iter().any(|id| units.iter().copied().eq(id.encode_utf16())))
                })
            })
            .count(),
        49
    );

    let mut cold = rows[0].clone();
    replace(
        &mut cold,
        "rawTargetHourTemperatureC",
        JsonValue::Number(2.0),
    );
    replace(&mut cold, "rowSha256", JsonValue::String(Vec::new()));
    let digest = canonical_without(&cold, "rowSha256").expect("rehash cold row");
    replace(
        &mut cold,
        "rowSha256",
        JsonValue::String(digest.encode_utf16().collect()),
    );
    let mut cold_rows = rows.clone();
    cold_rows[0] = cold;
    assert_eq!(evaluate_native_rain_maintenance_promotion(&cold_rows), None);
}
