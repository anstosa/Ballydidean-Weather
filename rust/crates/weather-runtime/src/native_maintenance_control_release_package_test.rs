use std::{path::PathBuf, process::Command};

use weather_contracts::JsonValue;
use weather_edge::ArchiveWirePrimitives;

use super::{
    NativeMaintenanceControlReferenceCompensationInput,
    NativeMaintenanceControlReferenceTargetInput,
    build_native_maintenance_control_reference_compensation,
    build_native_maintenance_control_reference_target,
};
use crate::{NativeWirePrimitives, maintenance_canonical_json::field};

const SOURCE_ORACLE: &str = r#"
import {readFile} from 'node:fs/promises';
import {
  buildAdjustmentRainControlReferenceAction,
  packageAdjustmentRainControlReference,
} from './scripts/research/adjustment_model_release.mjs';
import {
  adjustmentSha256,
  canonicalJsonBytes,
} from './scripts/research/adjustment_plaintext_archive.mjs';
import {
  createRainMaintenanceControlState,
  encodeRainMaintenanceControlState,
  RAIN_MAINTENANCE_CONTROL_RECIPE_SHA256,
} from './scripts/research/adjustment-maintenance-runtime/forecast/rain-maintenance-controls.js';
import {
  encodeMaintenanceBinary64,
} from './scripts/research/adjustment-maintenance-runtime/forecast/maintenance-shadow-values.js';

const source = await readFile(
  './packages/forecast-adjustment/src/rain-hurdle-wind-artifact.ts', 'utf8');
const artifact = JSON.parse(JSON.parse(
  source.match(/RAIN_HURDLE_WIND_ARTIFACT_JSON = (.*) as const;\n$/su)[1]));
delete artifact.nativeModelSha256;
delete artifact.provenanceSha256;
const ordinalArtifactBytes = canonicalJsonBytes(artifact);
const monthStart = Date.parse(`${artifact.modelMonth}-01T00:00:00.000Z`);
const calibrationEnd = monthStart - 7 * 86_400_000;
const calibrationStart = calibrationEnd - 90 * 86_400_000;
const generatedAt = new Date(calibrationEnd + 3_600_000).toISOString();
const state = createRainMaintenanceControlState({
  calibrationEndAt: new Date(calibrationEnd).toISOString(),
  calibrationStartAt: new Date(calibrationStart).toISOString(),
  contractVersion: 'rain-maintenance-control-state/v1',
  epochWitnessSha256: 'a'.repeat(64),
  generatedAt,
  legacyCalibrationStartAt: new Date(calibrationEnd - 45 * 86_400_000).toISOString(),
  legacyRawScale: 1,
  modelMonth: artifact.modelMonth,
  ordinalArtifactSha256: adjustmentSha256(ordinalArtifactBytes),
  recentFallbackReason: 'recent_calibration',
  recentRawScale: 1,
  recentSupported: true,
  recipeSha256: RAIN_MAINTENANCE_CONTROL_RECIPE_SHA256,
  sameWindowRawScale: 1,
  scheduleContractSha256: 'b'.repeat(64),
  sourceMemberRootSha256: 'c'.repeat(64),
  sourceReceiptRootSha256: 'd'.repeat(64),
  support: {
    calibrationDates: 60,
    calibrationHours: 500,
    calibrationRows: 500,
    calibrationWetDates: 5,
    calibrationWetHours: 20,
    effectiveDates64: encodeMaintenanceBinary64(30),
    effectiveWetDates64: encodeMaintenanceBinary64(3),
    legacyCalibrationRows: 200,
    legacyTrainingRows: 1_000,
    legacyTrainingWetRows: 100,
    trainingDates: 180,
    trainingHours: 1_000,
    trainingRows: 1_000,
    trainingWetDates: 20,
    trainingWetHours: 100,
  },
  trainingMaximumValidAt: new Date(calibrationStart - 8 * 86_400_000).toISOString(),
});
const controlStateBytes = encodeRainMaintenanceControlState(state);
const action = {
  actionKind: 'control_reference',
  contractVersion: 'forecast-adjustment-rain-control-reference-action/v1',
  controlStateSha256: adjustmentSha256(controlStateBytes),
  createdAt: generatedAt,
  dueMonth: artifact.modelMonth,
  expectedCatalogReceiptSha256: null,
  expectedSettingsSha256: 'e'.repeat(64),
  expectedSourceCommit: '1'.repeat(40),
  expectedSourceRelease: '2027.01.09-1',
  family: 'rain',
  fencingToken: '1',
  graphManifestSha256: 'f'.repeat(64),
  ordinalArtifactSha256: adjustmentSha256(ordinalArtifactBytes),
  predecessorActionSha256: null,
  reason: 'premonth_reference',
  sourceMemberRootSha256: state.sourceMemberRootSha256,
  sourceReceiptRootSha256: state.sourceReceiptRootSha256,
  validThrough: new Date(monthStart).toISOString(),
};
const target = packageAdjustmentRainControlReference({
  action,
  baselineSelectorBytes: null,
  controlStateBytes,
  ordinalArtifactBytes,
});
const selector = target.files.find(({path}) =>
  path === 'config/forecast-adjustments/ballydidean-rain-control-reference.json').bytes;
const compensationAction = {
  ...action,
  actionKind: 'compensate_control_reference',
  expectedSourceCommit: '2'.repeat(40),
  expectedSourceRelease: '2027.01.10-1',
  predecessorActionSha256: buildAdjustmentRainControlReferenceAction(action).actionSha256,
  reason: 'failed_control_reference',
};
const restore = packageAdjustmentRainControlReference({
  action: compensationAction,
  baselineSelectorBytes: selector,
  controlStateBytes: null,
  ordinalArtifactBytes: null,
});
const remove = packageAdjustmentRainControlReference({
  action: compensationAction,
  baselineSelectorBytes: null,
  controlStateBytes: null,
  ordinalArtifactBytes: null,
});
const packageJson = (value) => ({
  actionSha256: value.actionSha256,
  files: value.files.map(({bytes, path}) => ({bytes: bytes.toString('base64'), path})),
  packageRootSha256: value.packageRootSha256,
  removedPaths: value.removedPaths,
  totalBytes: value.totalBytes,
});
process.stdout.write(JSON.stringify({
  action,
  controlStateBytes: controlStateBytes.toString('base64'),
  ordinalArtifactBytes: ordinalArtifactBytes.toString('base64'),
  remove: packageJson(remove),
  restore: packageJson(restore),
  selector: selector.toString('base64'),
  target: packageJson(target),
}));
"#;

/// Run the predecessor fixture from the repository root and decode its closed response.
fn source_fixture() -> JsonValue {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .unwrap();
    let output = Command::new("node")
        .args(["--input-type=module", "--eval", SOURCE_ORACLE])
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    NativeWirePrimitives.decode_json(&output.stdout).unwrap()
}

/// Decode one required UTF-16 string member from the source response.
fn text(value: &JsonValue, name: &str) -> String {
    let JsonValue::String(value) = field(value, name).unwrap() else {
        panic!("{name} was not text");
    };
    String::from_utf16(value).unwrap()
}

/// Decode one required base64 fixture member through the production primitive.
fn bytes(value: &JsonValue, name: &str) -> Vec<u8> {
    NativeWirePrimitives
        .decode_standard_base64(&text(value, name))
        .unwrap()
}

/// Decode one required JSON array member.
fn array<'a>(value: &'a JsonValue, name: &str) -> &'a [JsonValue] {
    let JsonValue::Array(values) = field(value, name).unwrap() else {
        panic!("{name} was not an array");
    };
    values
}

/// Compare a native package to the source package's paths, bytes, identities, and removals.
fn assert_package(actual: &super::NativeMaintenanceControlReleasePackage, expected: &JsonValue) {
    assert_eq!(actual.action_sha256, text(expected, "actionSha256"));
    assert_eq!(
        actual.package_root_sha256,
        text(expected, "packageRootSha256")
    );
    let JsonValue::Number(total) = field(expected, "totalBytes").unwrap() else {
        panic!("totalBytes was not numeric");
    };
    assert_eq!(actual.package_total_bytes, *total as usize);
    assert_eq!(
        actual.removed_paths,
        array(expected, "removedPaths")
            .iter()
            .map(|value| {
                let JsonValue::String(value) = value else {
                    panic!("removed path was not text");
                };
                String::from_utf16(value).unwrap()
            })
            .collect::<Vec<_>>()
    );
    let expected_files = array(expected, "files");
    assert_eq!(actual.package_files.len(), expected_files.len());
    // compare every sorted source file without losing exact byte framing
    for (actual, expected) in actual.package_files.iter().zip(expected_files) {
        assert_eq!(actual.path, text(expected, "path"));
        assert_eq!(actual.bytes, bytes(expected, "bytes"));
    }
}

#[test]
fn target_and_both_compensation_selector_states_match_the_source() {
    let fixture = source_fixture();
    let action = field(&fixture, "action").unwrap();
    let control_state_bytes = bytes(&fixture, "controlStateBytes");
    let ordinal_artifact_bytes = bytes(&fixture, "ordinalArtifactBytes");
    let target = build_native_maintenance_control_reference_target(
        NativeMaintenanceControlReferenceTargetInput {
            control_state_bytes: &control_state_bytes,
            due_key: &format!("control-reference/rain/{}", text(action, "dueMonth")),
            expected_catalog_receipt_sha256: None,
            expected_settings_sha256: &text(action, "expectedSettingsSha256"),
            expected_source_commit: &text(action, "expectedSourceCommit"),
            expected_source_release: &text(action, "expectedSourceRelease"),
            fencing_token: &text(action, "fencingToken"),
            graph_manifest_sha256: &text(action, "graphManifestSha256"),
            now: &text(action, "createdAt"),
            ordinal_artifact_bytes: &ordinal_artifact_bytes,
            source_member_root_sha256: &text(action, "sourceMemberRootSha256"),
            source_receipt_root_sha256: &text(action, "sourceReceiptRootSha256"),
        },
    )
    .unwrap();
    assert_package(&target, field(&fixture, "target").unwrap());

    let selector = bytes(&fixture, "selector");
    let restore = build_native_maintenance_control_reference_compensation(
        NativeMaintenanceControlReferenceCompensationInput {
            baseline_selector_bytes: Some(&selector),
            compensation_created_at: &text(action, "createdAt"),
            target: &target,
            target_commit_sha: &"2".repeat(40),
            target_release_tag: "2027.01.10-1",
        },
    )
    .unwrap();
    assert_package(&restore, field(&fixture, "restore").unwrap());

    let remove = build_native_maintenance_control_reference_compensation(
        NativeMaintenanceControlReferenceCompensationInput {
            baseline_selector_bytes: None,
            compensation_created_at: &text(action, "createdAt"),
            target: &target,
            target_commit_sha: &"2".repeat(40),
            target_release_tag: "2027.01.10-1",
        },
    )
    .unwrap();
    assert_package(&remove, field(&fixture, "remove").unwrap());
}
