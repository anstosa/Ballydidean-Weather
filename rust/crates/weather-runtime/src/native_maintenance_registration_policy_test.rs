use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use weather_contracts::AdjustmentShadowFamily;

use crate::MaintenanceCanonicalJson;
use crate::NativeWirePrimitives;
use crate::native_maintenance_registration_policy::{
    NativeAdjustmentRegistrationCohortInput, NativeAdjustmentRollingShadowRegistrationInput,
    NativeAdjustmentTerminalNoActionDisposition, NativeAdjustmentTerminalNoActionReceiptInput,
    build_native_adjustment_registration_cohort_descriptor,
    build_native_adjustment_registration_policy_descriptor,
    build_native_adjustment_rolling_shadow_registration,
    build_native_adjustment_terminal_no_action_receipt,
};
use crate::native_maintenance_rolling_schedule::{
    NativeAdjustmentRollingWindowInput, build_adjustment_rolling_window,
};

const HASH_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const HASH_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const HASH_C: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const HASH_D: &str = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
const HASH_F: &str = "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";

const NODE_FIXTURE: &str = r#"
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import {
  buildAdjustmentRegistrationCohortDescriptor,
  buildAdjustmentRegistrationPolicyDescriptor,
  buildAdjustmentRollingShadowRegistration,
  buildAdjustmentTerminalNoActionReceipt,
} from "./scripts/research/adjustment_maintenance_controller.mjs";
import { canonicalJsonBytes } from "./scripts/research/adjustment_plaintext_archive.mjs";

const A = "a".repeat(64);
const B = "b".repeat(64);
const C = "c".repeat(64);
const D = "d".repeat(64);
const F = "f".repeat(64);
const policy = buildAdjustmentRegistrationPolicyDescriptor({ family: "rain" });
const cohort = buildAdjustmentRegistrationCohortDescriptor({
  cutoffAt: "2027-08-01T00:00:00.000Z",
  dueMonth: "2027-08",
  epochWitnessSha256: D,
  family: "rain",
  historicalMemberRootSha256: B,
  inputManifestSha256: C,
});
const rolling = buildAdjustmentRollingShadowRegistration({
  artifactSha256: A,
  candidateSha256: B,
  cohortSha256: cohort.cohortSha256,
  epochAt: "2026-10-09T04:10:45.000Z",
  epochWitnessSha256: D,
  family: "rain",
  fitMonth: "2027-08",
  policySha256: policy.policySha256,
  predecessorRegistrationSha256: null,
  predecessorTerminalAt: null,
  requestedAt: "2027-08-02T12:00:00.000Z",
  reservedKeySha256: F,
  sourceSha256: A,
});
const terminal = buildAdjustmentTerminalNoActionReceipt({
  completedAt: "2028-08-07T07:01:00.000Z",
  disposition: "rejected",
  finalizedAt: "2028-08-07T07:00:00.000Z",
  fullMemberRootSha256: A,
  policyReportSha256: B,
  registrationSha256: rolling.registration.registrationSha256,
});

writeFileSync(join(process.env.WEATHER_POLICY_FIXTURE_ROOT, "policy.json"), policy.bytes);
writeFileSync(join(process.env.WEATHER_POLICY_FIXTURE_ROOT, "cohort.json"), cohort.bytes);
writeFileSync(join(process.env.WEATHER_POLICY_FIXTURE_ROOT, "plan.json"),
  canonicalJsonBytes(rolling.plan));
writeFileSync(join(process.env.WEATHER_POLICY_FIXTURE_ROOT, "registration.json"),
  canonicalJsonBytes(rolling.registration));
writeFileSync(join(process.env.WEATHER_POLICY_FIXTURE_ROOT, "identity.json"),
  terminal.identityBytes);
writeFileSync(join(process.env.WEATHER_POLICY_FIXTURE_ROOT, "receipt.json"),
  terminal.receiptBytes);
"#;

/// Remove one bounded disposable differential-fixture root.
struct FixtureRoot(PathBuf);

impl FixtureRoot {
    /// Create one unique private process-local fixture directory.
    fn create() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("fixture clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "weather-native-registration-policy-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir(&root).expect("create fixture root");
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700))
            .expect("protect fixture root");
        Self(root)
    }

    /// Read one exact source-produced artifact from the disposable root.
    fn read(&self, name: &str) -> Vec<u8> {
        fs::read(self.0.join(name)).expect("read source artifact")
    }
}

impl Drop for FixtureRoot {
    /// Remove only the process-owned disposable fixture root.
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Run the actual predecessor builders into one disposable fixture root.
fn build_node_fixture(root: &Path) {
    let repository = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let output = Command::new("node")
        .args(["--input-type=module", "--eval", NODE_FIXTURE])
        .current_dir(repository)
        .env("WEATHER_POLICY_FIXTURE_ROOT", root)
        .output()
        .expect("run predecessor policy builders");
    // surface only the predecessor's bounded fixture diagnostic
    if !output.status.success() {
        panic!(
            "predecessor policy fixture failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

/// Match all four native artifacts against actual source builder bytes.
#[test]
fn native_registration_policy_artifacts_match_source_builders() {
    let fixture = FixtureRoot::create();
    build_node_fixture(&fixture.0);
    let mut codec = NativeWirePrimitives::new();
    let time = NativeWirePrimitives::new();

    let policy = build_native_adjustment_registration_policy_descriptor(
        AdjustmentShadowFamily::Rain,
        &mut codec,
    )
    .expect("build policy descriptor");
    assert_eq!(policy.bytes, fixture.read("policy.json"));

    let cohort = build_native_adjustment_registration_cohort_descriptor(
        NativeAdjustmentRegistrationCohortInput {
            cutoff_at: "2027-08-01T00:00:00.000Z",
            due_month: "2027-08",
            epoch_witness_sha256: HASH_D,
            family: AdjustmentShadowFamily::Rain,
            historical_member_root_sha256: HASH_B,
            input_manifest_sha256: HASH_C,
        },
        &mut codec,
        &time,
    )
    .expect("build cohort descriptor");
    assert_eq!(cohort.bytes, fixture.read("cohort.json"));

    let window = build_adjustment_rolling_window(NativeAdjustmentRollingWindowInput {
        epoch_at: "2026-10-09T04:10:45.000Z",
        epoch_witness_sha256: HASH_D,
        family: "rain",
        fit_month: "2027-08",
        predecessor_terminal_at: None,
        requested_at: "2027-08-02T12:00:00.000Z",
    })
    .expect("build native rolling window");
    let rolling = build_native_adjustment_rolling_shadow_registration(
        NativeAdjustmentRollingShadowRegistrationInput {
            artifact_sha256: HASH_A,
            candidate_sha256: HASH_B,
            cohort_sha256: &cohort.cohort_sha256,
            family: AdjustmentShadowFamily::Rain,
            policy_sha256: &policy.policy_sha256,
            predecessor_registration_sha256: None,
            reserved_key_sha256: HASH_F,
            source_sha256: HASH_A,
        },
        &window,
        &mut codec,
        &time,
    )
    .expect("build rolling registration");
    assert_eq!(
        codec
            .encode_canonical_json(&rolling.plan)
            .expect("encode rolling plan"),
        fixture.read("plan.json")
    );
    assert_eq!(
        codec
            .encode_canonical_json(&rolling.registration)
            .expect("encode rolling registration"),
        fixture.read("registration.json")
    );

    let terminal = build_native_adjustment_terminal_no_action_receipt(
        NativeAdjustmentTerminalNoActionReceiptInput {
            completed_at: "2028-08-07T07:01:00.000Z",
            disposition: NativeAdjustmentTerminalNoActionDisposition::Rejected,
            finalized_at: "2028-08-07T07:00:00.000Z",
            full_member_root_sha256: HASH_A,
            policy_report_sha256: HASH_B,
            registration_sha256: &rolling.registration_sha256,
        },
        &mut codec,
        &time,
    )
    .expect("build no-action receipt");
    assert_eq!(terminal.identity_bytes, fixture.read("identity.json"));
    assert_eq!(terminal.receipt_bytes, fixture.read("receipt.json"));
}
