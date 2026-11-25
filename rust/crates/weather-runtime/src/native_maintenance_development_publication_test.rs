use std::path::Path;
use std::process::Command;
use std::sync::OnceLock;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use weather_contracts::JsonValue;
use weather_edge::ArchiveWirePrimitives;

use crate::native_maintenance_development_publication::{
    NativeMaintenanceDevelopmentPublicationInput, build_native_maintenance_development_publication,
};
use crate::{
    NativeFamilyReleaseCurrent, NativeMaintenanceCandidateParityReceipt,
    NativeMaintenanceCaptureFamily, NativeMaintenanceConfirmationPreregistration,
    NativeMaintenanceGraphEntry, NativeMaintenanceGraphManifest,
    NativeMaintenancePortableCandidate, NativeMaintenanceVerifiedFullGraph, NativeWirePrimitives,
    maintenance_canonical_json::{field, text},
    verify_native_maintenance_attempt_report,
};

/// Match the genuine rain inactive package, action, and custody graph to the source publisher.
#[test]
fn native_development_publication_matches_source_rain_material() {
    let fixture = source_fixture();
    let candidate_bytes = fixture_bytes(fixture, "candidateBytes");
    let artifact_bytes = fixture_bytes(fixture, "artifactBytes");
    let parity_bytes = fixture_bytes(fixture, "parityBytes");
    let parity_fixture = NativeWirePrimitives.decode_json(&parity_bytes).unwrap();
    let registration = fixture_value(fixture, "registration").clone();
    let report_bytes = fixture_bytes(fixture, "reportBytes");
    let report = verify_native_maintenance_attempt_report(&report_bytes).unwrap();
    let graph_sha256 = fixture_text(fixture, "candidateGraphSha256");
    let graph = verified_graph(
        &graph_sha256,
        fixture_array(fixture, "candidateGraphMembers")
            .iter()
            .map(json_string)
            .collect(),
    );
    let portable = NativeMaintenancePortableCandidate {
        artifact_bytes,
        artifact_sha256: fixture_text(fixture, "artifactSha256"),
        candidate_sha256: fixture_text(fixture, "candidateSha256"),
        source_identity_sha256: fixture_text(fixture, "sourceIdentitySha256"),
    };
    let parity = NativeMaintenanceCandidateParityReceipt {
        parity_sha256: fixture_text(fixture, "paritySha256"),
        bytes: parity_bytes,
        fixture: parity_fixture,
    };
    let current = NativeFamilyReleaseCurrent {
        active_installed_receipt_sha256: None,
        catalog_sha256: "f".repeat(64),
        commit: fixture_text(fixture, "sourceCommit"),
        control_installed_receipt_sha256: None,
        contract_version: "adjustment-family-release-current-status/v1".to_owned(),
        family: "rain".to_owned(),
        release: fixture_text(fixture, "sourceRelease"),
        settings_sha256: fixture_text(fixture, "settingsSha256"),
        shadow_installed_receipt_sha256: Some(fixture_text(fixture, "installedSha256")),
        source_server_image_digest: "e".repeat(64),
    };
    let confirmation = NativeMaintenanceConfirmationPreregistration {
        registration_sha256: fixture_text(fixture, "confirmationRegistrationSha256"),
        already_registered: false,
    };
    let created_at = fixture_text(fixture, "createdAt");
    let expected_plan_sha256 = fixture_text(fixture, "expectedPlanSha256");
    let fencing_token = fixture_text(fixture, "fencingToken");
    let lifecycle_head_sha256 = fixture_text(fixture, "lifecycleHeadSha256");
    let report_created_at = fixture_text(fixture, "reportCreatedAt");
    let material = build_native_maintenance_development_publication(
        NativeMaintenanceDevelopmentPublicationInput {
            candidate_bytes: &candidate_bytes,
            candidate_graph: &graph,
            confirmation_preregistration: &confirmation,
            created_at: &created_at,
            current: &current,
            expected_plan_sha256: &expected_plan_sha256,
            family: NativeMaintenanceCaptureFamily::Rain,
            fencing_token: &fencing_token,
            lifecycle_head_sha256: &lifecycle_head_sha256,
            parity: &parity,
            portable: &portable,
            registration: &registration,
            report: &report,
            report_created_at: &report_created_at,
        },
    )
    .unwrap();
    assert_eq!(material.action_bytes, fixture_bytes(fixture, "actionBytes"));
    assert_eq!(
        material.action_sha256,
        fixture_text(fixture, "actionSha256")
    );
    assert_eq!(material.release_date, fixture_text(fixture, "releaseDate"));
    assert_eq!(
        material.package_root_sha256,
        fixture_text(fixture, "packageRootSha256")
    );
    assert_eq!(
        material.package_total_bytes,
        fixture_number(fixture, "packageTotalBytes") as usize
    );
    assert_files(
        &material.family_files,
        fixture_array(fixture, "familyFiles"),
    );
    assert_files(
        &material.package_files,
        fixture_array(fixture, "packageFiles"),
    );
    assert_eq!(
        material.shadow_projection_bytes,
        fixture_bytes(fixture, "shadowProjectionBytes")
    );
    assert_eq!(
        material.custody_binding_bytes,
        fixture_bytes(fixture, "custodyBindingBytes")
    );
    assert_eq!(
        material.custody_binding_sha256,
        fixture_text(fixture, "custodyBindingSha256")
    );
    let expected_members = fixture_array(fixture, "custodyMembers");
    assert_eq!(
        material.custody_segment.members.len(),
        expected_members.len()
    );
    // retain exact member order, semantic identities, kinds, and bodies
    for (actual, expected) in material
        .custody_segment
        .members
        .iter()
        .zip(expected_members)
    {
        assert_eq!(actual.kind, json_text(expected, "kind"));
        assert_eq!(
            hex(actual.identity_sha256),
            json_text(expected, "identitySha256")
        );
        assert_eq!(actual.payload, json_bytes(expected, "payload"));
    }
    let expected_links = fixture_array(fixture, "custodyCrossLinks");
    assert_eq!(
        material.custody_segment.cross_links.len(),
        expected_links.len()
    );
    // retain exact source relationship order without graph normalization
    for (actual, expected) in material
        .custody_segment
        .cross_links
        .iter()
        .zip(expected_links)
    {
        assert_eq!(
            actual.from_identity_sha256,
            json_text(expected, "fromIdentitySha256")
        );
        assert_eq!(actual.relation, json_text(expected, "relation"));
        assert_eq!(
            actual.to_identity_sha256,
            json_text(expected, "toIdentitySha256")
        );
    }
}

/// Prove graph admission refuses a hash-only candidate graph missing one required member.
#[test]
fn native_development_publication_requires_every_source_graph_member() {
    let fixture = source_fixture();
    let candidate_bytes = fixture_bytes(fixture, "candidateBytes");
    let artifact_bytes = fixture_bytes(fixture, "artifactBytes");
    let parity_bytes = fixture_bytes(fixture, "parityBytes");
    let parity = NativeMaintenanceCandidateParityReceipt {
        parity_sha256: fixture_text(fixture, "paritySha256"),
        fixture: NativeWirePrimitives.decode_json(&parity_bytes).unwrap(),
        bytes: parity_bytes,
    };
    let portable = NativeMaintenancePortableCandidate {
        artifact_bytes,
        artifact_sha256: fixture_text(fixture, "artifactSha256"),
        candidate_sha256: fixture_text(fixture, "candidateSha256"),
        source_identity_sha256: fixture_text(fixture, "sourceIdentitySha256"),
    };
    let members = fixture_array(fixture, "candidateGraphMembers");
    let graph = verified_graph(
        &fixture_text(fixture, "candidateGraphSha256"),
        members[..members.len() - 1]
            .iter()
            .map(json_string)
            .collect(),
    );
    let report =
        verify_native_maintenance_attempt_report(&fixture_bytes(fixture, "reportBytes")).unwrap();
    let current = NativeFamilyReleaseCurrent {
        active_installed_receipt_sha256: None,
        catalog_sha256: "f".repeat(64),
        commit: fixture_text(fixture, "sourceCommit"),
        control_installed_receipt_sha256: None,
        contract_version: "adjustment-family-release-current-status/v1".to_owned(),
        family: "rain".to_owned(),
        release: fixture_text(fixture, "sourceRelease"),
        settings_sha256: fixture_text(fixture, "settingsSha256"),
        shadow_installed_receipt_sha256: Some(fixture_text(fixture, "installedSha256")),
        source_server_image_digest: "e".repeat(64),
    };
    let confirmation = NativeMaintenanceConfirmationPreregistration {
        registration_sha256: fixture_text(fixture, "confirmationRegistrationSha256"),
        already_registered: false,
    };
    let created_at = fixture_text(fixture, "createdAt");
    let expected_plan_sha256 = fixture_text(fixture, "expectedPlanSha256");
    let fencing_token = fixture_text(fixture, "fencingToken");
    let lifecycle_head_sha256 = fixture_text(fixture, "lifecycleHeadSha256");
    let report_created_at = fixture_text(fixture, "reportCreatedAt");
    let result = build_native_maintenance_development_publication(
        NativeMaintenanceDevelopmentPublicationInput {
            candidate_bytes: &candidate_bytes,
            candidate_graph: &graph,
            confirmation_preregistration: &confirmation,
            created_at: &created_at,
            current: &current,
            expected_plan_sha256: &expected_plan_sha256,
            family: NativeMaintenanceCaptureFamily::Rain,
            fencing_token: &fencing_token,
            lifecycle_head_sha256: &lifecycle_head_sha256,
            parity: &parity,
            portable: &portable,
            registration: fixture_value(fixture, "registration"),
            report: &report,
            report_created_at: &report_created_at,
        },
    );
    assert!(matches!(
        result,
        Err(crate::native_maintenance_development_publication::NativeMaintenanceDevelopmentPublicationError::Graph)
    ));
}

/// Construct one typed verified head graph from source-provided immutable identities.
fn verified_graph(digest: &str, identities: Vec<String>) -> NativeMaintenanceVerifiedFullGraph {
    let entries = identities
        .into_iter()
        .map(|identity_sha256| NativeMaintenanceGraphEntry {
            kind: "fixture-member/v1".to_owned(),
            identity_sha256,
            object_sha256: "d".repeat(64),
            member_offset: 0,
            member_length: 1,
            member_sha256: "e".repeat(64),
        })
        .collect::<Vec<_>>();
    let member_count = entries.len();
    NativeMaintenanceVerifiedFullGraph {
        manifest: NativeMaintenanceGraphManifest {
            predecessor_graph_sha256: None,
            entries,
            cross_links: Vec::new(),
            graph_root_sha256: "c".repeat(64),
        },
        manifest_object_sha256: decode_digest(digest),
        object_count: 1,
        member_count,
        predecessor_count: 0,
    }
}

/// Compare one ordered native file matrix to base64 source material.
fn assert_files(
    actual: &[crate::native_maintenance_development_publication::NativeMaintenanceDevelopmentPublicationFile],
    expected: &[JsonValue],
) {
    assert_eq!(actual.len(), expected.len());
    // preserve source order as well as exact repository path and bytes
    for (actual, expected) in actual.iter().zip(expected) {
        assert_eq!(actual.path, json_text(expected, "path"));
        assert_eq!(actual.bytes, json_bytes(expected, "bytes"));
    }
}

/// Cache the bounded source oracle for both ordinary tests.
fn source_fixture() -> &'static JsonValue {
    static FIXTURE: OnceLock<JsonValue> = OnceLock::new();
    FIXTURE.get_or_init(|| {
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let output = Command::new("node")
            .args(["--input-type=module", "--eval", SOURCE_FIXTURE])
            .current_dir(repo)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        NativeWirePrimitives.decode_json(&output.stdout).unwrap()
    })
}

/// Borrow one fixture object field.
fn fixture_value<'a>(fixture: &'a JsonValue, name: &str) -> &'a JsonValue {
    field(fixture, name).unwrap()
}

/// Clone one fixture string field.
fn fixture_text(fixture: &JsonValue, name: &str) -> String {
    text(fixture, name).unwrap()
}

/// Decode one fixture standard-base64 field.
fn fixture_bytes(fixture: &JsonValue, name: &str) -> Vec<u8> {
    STANDARD.decode(fixture_text(fixture, name)).unwrap()
}

/// Borrow one fixture array field.
fn fixture_array<'a>(fixture: &'a JsonValue, name: &str) -> &'a [JsonValue] {
    let JsonValue::Array(values) = fixture_value(fixture, name) else {
        panic!("fixture field is not an array");
    };
    values
}

/// Read one finite fixture number.
fn fixture_number(fixture: &JsonValue, name: &str) -> f64 {
    let JsonValue::Number(value) = fixture_value(fixture, name) else {
        panic!("fixture field is not numeric");
    };
    *value
}

/// Decode one JSON string scalar.
fn json_string(value: &JsonValue) -> String {
    let JsonValue::String(units) = value else {
        panic!("fixture value is not text");
    };
    String::from_utf16(units).unwrap()
}

/// Decode one nested object string field.
fn json_text(value: &JsonValue, name: &str) -> String {
    text(value, name).unwrap()
}

/// Decode one nested object standard-base64 field.
fn json_bytes(value: &JsonValue, name: &str) -> Vec<u8> {
    STANDARD.decode(json_text(value, name)).unwrap()
}

/// Decode one lowercase hexadecimal SHA-256 fixture address.
fn decode_digest(value: &str) -> [u8; 32] {
    let mut bytes = [0u8; 32];
    // decode the already source-validated fixed-length lowercase identity
    for (index, pair) in value.as_bytes().chunks_exact(2).enumerate() {
        bytes[index] = nibble(pair[0]) * 16 + nibble(pair[1]);
    }
    bytes
}

/// Decode one lowercase hexadecimal nibble.
fn nibble(value: u8) -> u8 {
    // accept lowercase fixture digits only
    match value {
        b'0'..=b'9' => value - b'0',
        b'a'..=b'f' => value - b'a' + 10,
        _ => panic!("fixture digest is invalid"),
    }
}

/// Render one typed digest for comparison with source text.
fn hex(value: [u8; 32]) -> String {
    value.iter().map(|byte| format!("{byte:02x}")).collect()
}

const SOURCE_FIXTURE: &str = r#"
import { readFile } from "node:fs/promises";
import {
  adjustmentSha256,
  canonicalJsonBytes,
} from "./scripts/research/adjustment_plaintext_archive.mjs";
import {
  buildAdjustmentDevelopmentCustodyGraphSegment,
} from "./scripts/research/adjustment_maintenance_controller.mjs";
import {
  buildAdjustmentModelAction,
  packageAdjustmentModelFamily,
} from "./scripts/research/adjustment_model_release.mjs";
import { buildPortableRainModelPackage } from "./scripts/research/adjustment_rain_model_package.mjs";
import { ADJUSTMENT_ROLLING_SCHEDULE_SHA256 } from "./scripts/research/adjustment_rolling_schedule.mjs";

const hash = (character) => character.repeat(64);
const b64 = (bytes) => Buffer.from(bytes).toString("base64");
const runtimeSource = await readFile("packages/forecast-adjustment/src/rain-hurdle-wind-artifact.ts", "utf8");
const runtimeMatch = runtimeSource.match(/RAIN_HURDLE_WIND_ARTIFACT_JSON = (.*) as const;\n$/su);
// require the checked-in generated artifact
if (runtimeMatch === null) throw new Error("retained runtime unavailable");
const runtime = JSON.parse(JSON.parse(runtimeMatch[1]));
const categories = Object.fromEntries(runtime.categoryScales.map(
  (scale, index) => [String(index + 1), { scale }],
));
const candidateBytes = canonicalJsonBytes({
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
    projectionId: "retained-native-parity",
  },
  confirmationOpened: false,
  contractVersion: "rain-maintenance-fit/v2",
  reason: "development_gate_passer",
  state: "development_candidate",
});
const portable = buildPortableRainModelPackage(candidateBytes);
const candidateGraphSha256 = hash("1");
const lifecycleHeadSha256 = hash("2");
const settingsSha256 = hash("3");
const installedSha256 = hash("4");
const policySha256 = hash("5");
const cohortSha256 = hash("6");
const expectedPlanSha256 = hash("7");
const reservedKeySha256 = hash("8");
const sourceSha256 = hash("9");
const epochWitnessSha256 = hash("a");
const confirmationRegistrationSha256 = hash("b");
const inputHeadSha256 = hash("c");
const fitReceiptSha256 = hash("d");
const semanticInputSha256 = hash("e");
const sourceIdentitySha256 = hash("f");
const intervalStartAt = "2026-11-01T07:00:00.000Z";
const intervalEndAt = "2027-10-01T07:00:00.000Z";
const targetCutoffAt = "2026-10-01T00:00:00.000Z";
const terminalAt = "2027-10-08T07:00:00.000Z";
const registrationMaterial = [
  "adjustment-shadow-registration/v3",
  "ballydidean",
  "rain",
  portable.candidateSha256,
  portable.artifactSha256,
  policySha256,
  cohortSha256,
  reservedKeySha256,
  sourceSha256,
  epochWitnessSha256,
  ADJUSTMENT_ROLLING_SCHEDULE_SHA256,
  "none",
  intervalStartAt,
  intervalEndAt,
  targetCutoffAt,
  terminalAt,
].join("\n") + "\n";
const registration = {
  artifactSha256: portable.artifactSha256,
  candidateSha256: portable.candidateSha256,
  cohortSha256,
  epochWitnessSha256,
  family: "rain",
  intervalEndAt,
  intervalStartAt,
  policySha256,
  predecessorRegistrationSha256: null,
  registrationSha256: adjustmentSha256(Buffer.from(registrationMaterial)),
  reservedKeySha256,
  scheduleContractSha256: ADJUSTMENT_ROLLING_SCHEDULE_SHA256,
  siteKey: "ballydidean",
  sourceSha256,
  targetCutoffAt,
  terminalAt,
};
const parity = {
  candidateSha256: portable.candidateSha256,
  contractVersion: "forecast-adjustment-model-parity/v1",
  family: "rain",
  retainedInputSha256: hash("1"),
  retainedNativeOutputSha256: hash("2"),
  retainedPackagedOutputSha256: hash("2"),
  syntheticInputSha256: hash("3"),
  syntheticNativeOutputSha256: hash("4"),
  syntheticPackagedOutputSha256: hash("4"),
};
const parityBytes = canonicalJsonBytes(parity);
const originalCutoffAt = "2026-10-01T00:00:00.000Z";
const dueKey = "monthly/rain/2026-10";
const attemptIdentitySha256 = adjustmentSha256(canonicalJsonBytes({
  contractVersion: "forecast-adjustment-maintenance-attempt-identity/v1",
  dueKey,
  inputHeadSha256,
  originalCutoffAt,
}));
const report = {
  actionEligible: false,
  attemptIdentitySha256,
  candidateGraphSha256,
  candidateSha256: portable.candidateSha256,
  contractVersion: "forecast-adjustment-maintenance-attempt/v2",
  dueKey,
  family: "rain",
  fitReceiptSha256,
  inputHeadSha256,
  lifecycleHeadSha256,
  mode: "monthly",
  originalCutoffAt,
  reason: "development_candidate_archived",
  reservedConfirmationExposed: false,
  semanticInputSha256,
  servingChanged: false,
  state: "completed",
};
const reportBytes = canonicalJsonBytes(report);
const reportCreatedAt = "2026-10-10T23:30:00.000Z";
const createdAt = "2026-10-11T00:30:00.000Z";
const sourceCommit = "0123456789abcdef0123456789abcdef01234567";
const sourceRelease = "2026.10.09-1";
const fencingToken = "7";
const action = {
  actionKind: "shadow",
  candidateGraphSha256,
  candidateSha256: portable.candidateSha256,
  contractVersion: "forecast-adjustment-model-action/v1",
  createdAt,
  expectedInstalledReceiptSha256: installedSha256,
  expectedSettingsSha256: settingsSha256,
  expectedSourceCommit: sourceCommit,
  expectedSourceRelease: sourceRelease,
  family: "rain",
  fencingToken,
  fullMemberRootSha256: null,
  lifecycleHeadSha256,
  policyDecision: "pending",
  policyReportSha256: adjustmentSha256(reportBytes),
  predecessorActionSha256: null,
  reason: "development_candidate",
  reportCreatedAt,
  siteKey: "ballydidean",
  validThrough: "2026-10-17T23:30:00.000Z",
};
const builtAction = buildAdjustmentModelAction(action);
const packaged = packageAdjustmentModelFamily({
  action,
  familyFiles: portable.shadowFamilyFiles,
  parity: parityBytes,
  shadowRegistration: registration,
});
const custody = buildAdjustmentDevelopmentCustodyGraphSegment({
  action,
  artifactBytes: portable.shadowFamilyFiles[0].bytes,
  candidateBytes,
  candidateGraphSha256,
  registration,
  reportBytes,
});
const shadowProjection = packaged.files.find((file) => file.path.includes("/shadow-catalog/"));
const candidateGraphMembers = [
  portable.candidateSha256,
  portable.artifactSha256,
  registration.registrationSha256,
  policySha256,
  cohortSha256,
  expectedPlanSha256,
  sourceSha256,
];
process.stdout.write(JSON.stringify({
  actionBytes: b64(builtAction.bytes),
  actionSha256: builtAction.actionSha256,
  artifactBytes: b64(portable.shadowFamilyFiles[0].bytes),
  artifactSha256: portable.artifactSha256,
  candidateBytes: b64(candidateBytes),
  candidateGraphMembers,
  candidateGraphSha256,
  candidateSha256: portable.candidateSha256,
  confirmationRegistrationSha256,
  createdAt,
  custodyBindingBytes: b64(canonicalJsonBytes(custody.binding)),
  custodyBindingSha256: custody.bindingSha256,
  custodyCrossLinks: custody.segment.crossLinks,
  custodyMembers: custody.segment.members.map((member) => ({
    identitySha256: member.identitySha256,
    kind: member.kind,
    payload: b64(member.payload),
  })),
  expectedPlanSha256,
  familyFiles: portable.shadowFamilyFiles.map((file) => ({ path: file.path, bytes: b64(file.bytes) })),
  fencingToken,
  installedSha256,
  lifecycleHeadSha256,
  packageFiles: packaged.files.map((file) => ({ path: file.path, bytes: b64(file.bytes) })),
  packageRootSha256: packaged.packageRootSha256,
  packageTotalBytes: packaged.totalBytes,
  parityBytes: b64(parityBytes),
  paritySha256: adjustmentSha256(parityBytes),
  registration,
  releaseDate: "2026.10.10",
  reportBytes: b64(reportBytes),
  reportCreatedAt,
  settingsSha256,
  shadowProjectionBytes: b64(shadowProjection.bytes),
  sourceCommit,
  sourceIdentitySha256,
  sourceRelease,
}));
"#;
