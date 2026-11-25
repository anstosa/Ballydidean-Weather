use std::fs::{self, File};
use std::os::fd::AsRawFd;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use weather_contracts::JsonValue;
use weather_edge::RevisionArchiveCanonicalClock;

use crate::{
    MaintenanceCanonicalJson, NativeFamilyReleaseCompensationObservation,
    NativeFamilyReleaseCurrent, NativeFamilyReleaseCurrentObservation,
    NativeFamilyReleaseDurableProof, NativeFamilyReleaseHostController, NativeFamilyReleaseInput,
    NativeFamilyReleaseInspectedCurrent, NativeFamilyReleaseLiveObservation,
    NativeFamilyReleaseMutationRecord, NativeFamilyReleaseProductionPort,
    NativeFamilyReleaseProofOwner, NativeFamilyReleaseTransactionError,
    NativeFamilyReleaseTransactionResult, NativeFamilyReleaseTransactionStore,
    NativeFamilyReleaseTransactionStorePaths, NativeFamilyReleaseVerifiedProof,
    NativeWirePrimitives, execute_native_family_release_transaction,
};

const SOURCE_COMMIT: &str = "1111111111111111111111111111111111111111";
const TARGET_COMMIT: &str = "2222222222222222222222222222222222222222";
const COMPENSATION_COMMIT: &str = "3333333333333333333333333333333333333333";
const SOURCE_RELEASE: &str = "2026.10.09-1";
const TARGET_RELEASE: &str = "2026.10.10-1";
const COMPENSATION_RELEASE: &str = "2026.10.11-1";
const HASH_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const HASH_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const HASH_C: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const HASH_D: &str = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
const HASH_E: &str = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
const NOW: &str = "2026-10-07T12:00:00.000Z";
const SETTINGS: &[u8] = b"{\"version\":1,\"temperature\":true,\"wind\":true,\"rain\":true}\n";
static FIXTURE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy)]
struct FixtureClock;

impl RevisionArchiveCanonicalClock for FixtureClock {
    /// Return one exact canonical transaction fixture clock.
    fn canonical_now(&mut self) -> Option<String> {
        Some(NOW.to_owned())
    }
}

#[derive(Clone)]
struct FixtureHost {
    current: NativeFamilyReleaseInspectedCurrent,
    live: NativeFamilyReleaseLiveObservation,
    applied: usize,
}

impl NativeFamilyReleaseHostController for FixtureHost {
    /// Return the independently verified current fixture for the selected family.
    fn inspect_current(
        &mut self,
        family: &str,
    ) -> Result<NativeFamilyReleaseInspectedCurrent, NativeFamilyReleaseTransactionError> {
        // reject a substituted family before exposing the fixture observation
        if family != "temperature" {
            return Err(NativeFamilyReleaseTransactionError::Input);
        }
        Ok(self.current.clone())
    }

    /// Retain one actual target-application call for ordering evidence.
    fn apply_target(
        &mut self,
        _record: &NativeFamilyReleaseMutationRecord,
        _proof: &NativeFamilyReleaseDurableProof,
    ) -> Result<(), NativeFamilyReleaseTransactionError> {
        self.applied += 1;
        Ok(())
    }

    /// Return the exact independently observed target fixture.
    fn verify_live(
        &mut self,
        _record: &NativeFamilyReleaseMutationRecord,
        _proof: &NativeFamilyReleaseDurableProof,
    ) -> Result<NativeFamilyReleaseLiveObservation, NativeFamilyReleaseTransactionError> {
        Ok(self.live.clone())
    }

    /// Return a complete prebuilt compensation fixture if the adapter requests recovery.
    fn compensate(
        &mut self,
        record: &NativeFamilyReleaseMutationRecord,
        proof: &NativeFamilyReleaseDurableProof,
    ) -> Result<NativeFamilyReleaseCompensationObservation, NativeFamilyReleaseTransactionError>
    {
        Ok(NativeFamilyReleaseCompensationObservation {
            action_sha256: proof.compensation_action_sha256.clone(),
            live: NativeFamilyReleaseLiveObservation {
                commit: proof.compensation_commit.clone(),
                family: record.input.family.clone(),
                family_identity_sha256: proof.compensation_family_identity_sha256.clone(),
                image_digests_sha256: proof.compensation_image_digests_sha256.clone(),
                release: record.input.compensating_release.clone(),
                settings_bytes: SETTINGS.to_vec(),
            },
        })
    }
}

#[derive(Clone)]
struct FixtureProof(NativeFamilyReleaseVerifiedProof);

impl NativeFamilyReleaseProofOwner for FixtureProof {
    /// Return the already source-verified fixture proof.
    fn verify(
        &mut self,
        _input: &NativeFamilyReleaseInput,
        _current: &NativeFamilyReleaseCurrentObservation,
        _current_authority: &NativeFamilyReleaseCurrent,
        _prior: Option<&NativeFamilyReleaseMutationRecord>,
        _now: &str,
    ) -> Result<NativeFamilyReleaseVerifiedProof, NativeFamilyReleaseTransactionError> {
        Ok(self.0.clone())
    }
}

/// Persist the complete successful transaction once and replay it without another host mutation.
#[test]
fn native_family_release_production_port_persists_and_replays_transaction() {
    let root = fixture_root();
    fs::create_dir(&root).unwrap();
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
    let lock = File::open(&root).unwrap();
    let state_root = root.join("adjustment-release-state");
    // fixture ownership follows the actual process identity rather than weakening file admission
    let uid = unsafe { libc::geteuid() };
    // fixture ownership follows the actual process identity rather than weakening file admission
    let gid = unsafe { libc::getegid() };
    let store = NativeFamilyReleaseTransactionStore::for_paths(
        NativeFamilyReleaseTransactionStorePaths {
            state_root: state_root.clone(),
            release_lock_root: root.clone(),
        },
        lock.as_raw_fd(),
        uid,
        gid,
    );
    let (input, host, proof) = fixture();
    let action_sha256 = input.action_sha256.clone();
    let mut port = NativeFamilyReleaseProductionPort::new(
        store,
        host,
        FixtureProof(proof),
        FixtureClock,
        "temperature",
    );

    let acknowledged = execute_native_family_release_transaction(input.clone(), &mut port).unwrap();
    assert!(matches!(
        acknowledged,
        NativeFamilyReleaseTransactionResult::Mutation(_)
    ));
    assert_eq!(port.host.applied, 1);
    let transaction_path = state_root
        .join("transactions")
        .join(format!("sha256-{action_sha256}.json"));
    let transaction = fs::read(&transaction_path).unwrap();
    assert!(String::from_utf8_lossy(&transaction).contains("\"state\":\"acknowledged\""));
    assert_eq!(fs::read(state_root.join("maximum-fence")).unwrap(), b"2\n");
    assert!(
        String::from_utf8_lossy(&fs::read(state_root.join("current.json")).unwrap())
            .contains("\"state\":\"acknowledged\"")
    );

    let replayed = execute_native_family_release_transaction(input, &mut port).unwrap();
    assert_eq!(replayed, acknowledged);
    assert_eq!(port.host.applied, 1);
    assert_eq!(fs::read(transaction_path).unwrap(), transaction);
    fs::remove_dir_all(root).unwrap();
}

/// Build one complete target proof and its matching source/live observations.
fn fixture() -> (
    NativeFamilyReleaseInput,
    FixtureHost,
    NativeFamilyReleaseVerifiedProof,
) {
    let settings_sha256 = digest(SETTINGS);
    let target_action = object(vec![
        ("actionKind", string("shadow")),
        ("candidateGraphSha256", string(HASH_B)),
        ("candidateSha256", string(HASH_C)),
        (
            "contractVersion",
            string("forecast-adjustment-model-action/v1"),
        ),
        ("expectedInstalledReceiptSha256", JsonValue::Null),
        ("expectedSettingsSha256", string(&settings_sha256)),
        ("expectedSourceCommit", string(SOURCE_COMMIT)),
        ("expectedSourceRelease", string(SOURCE_RELEASE)),
        ("family", string("temperature")),
        ("fencingToken", string("2")),
        ("fullMemberRootSha256", JsonValue::Null),
        ("lifecycleHeadSha256", string(HASH_D)),
        ("policyDecision", string("pending")),
        ("policyReportSha256", string(HASH_A)),
        ("reportCreatedAt", string(NOW)),
        ("validThrough", string("2026-10-14T12:00:00.000Z")),
    ]);
    let action_sha256 = digest_json(&target_action);
    let compensation_action = object(vec![
        ("actionKind", string("compensate_shadow")),
        ("candidateGraphSha256", string(HASH_B)),
        ("candidateSha256", string(HASH_C)),
        (
            "contractVersion",
            string("forecast-adjustment-model-action/v1"),
        ),
        ("expectedInstalledReceiptSha256", JsonValue::Null),
        ("expectedSettingsSha256", string(&settings_sha256)),
        ("expectedSourceCommit", string(TARGET_COMMIT)),
        ("expectedSourceRelease", string(TARGET_RELEASE)),
        ("family", string("temperature")),
        ("fencingToken", string("2")),
        ("fullMemberRootSha256", JsonValue::Null),
        ("lifecycleHeadSha256", string(HASH_D)),
        ("policyDecision", string("pending")),
        ("policyReportSha256", string(HASH_A)),
        ("predecessorActionSha256", string(&action_sha256)),
        ("reportCreatedAt", string(NOW)),
        ("validThrough", string("2026-10-14T12:00:00.000Z")),
    ]);
    let compensation_action_sha256 = digest_json(&compensation_action);
    let authority = object(vec![
        ("actionKind", string("shadow")),
        ("actionSha256", string(&action_sha256)),
        ("candidateGraphSha256", string(HASH_B)),
        ("candidateSha256", string(HASH_C)),
        (
            "contractVersion",
            string("adjustment-development-custody-anchor/v1"),
        ),
        ("family", string("temperature")),
        ("lifecycleLedgerRootSha256", string(HASH_D)),
        ("policyReportSha256", string(HASH_A)),
        ("sourceCommit", string(SOURCE_COMMIT)),
    ]);
    let input = NativeFamilyReleaseInput {
        action_sha256,
        compensating_release: COMPENSATION_RELEASE.to_owned(),
        expected_current_release: SOURCE_RELEASE.to_owned(),
        expected_settings_sha256: settings_sha256.clone(),
        expected_source_release: SOURCE_RELEASE.to_owned(),
        family: "temperature".to_owned(),
        fencing_token: "2".to_owned(),
        report_sha256: HASH_A.to_owned(),
        target_release: TARGET_RELEASE.to_owned(),
    };
    let proof = NativeFamilyReleaseVerifiedProof {
        action: target_action,
        authority,
        compensation_action,
        durable: NativeFamilyReleaseDurableProof {
            baseline_catalog_bytes: b"{}\n".to_vec(),
            compensation_action_sha256,
            compensation_commit: COMPENSATION_COMMIT.to_owned(),
            compensation_family_identity_sha256: HASH_B.to_owned(),
            compensation_image_digests_sha256: HASH_C.to_owned(),
            source_settings_bytes: SETTINGS.to_vec(),
            target_catalog_bytes: b"{}\n".to_vec(),
            target_commit: TARGET_COMMIT.to_owned(),
            target_family_identity_sha256: HASH_D.to_owned(),
            target_image_digests_sha256: HASH_E.to_owned(),
        },
    };
    let observation = NativeFamilyReleaseCurrentObservation {
        commit: SOURCE_COMMIT.to_owned(),
        release: SOURCE_RELEASE.to_owned(),
        settings_bytes: SETTINGS.to_vec(),
        maximum_fence: String::new(),
        active_installed_receipt_sha256: None,
        shadow_installed_receipt_sha256: None,
        control_installed_receipt_sha256: None,
    };
    let current = NativeFamilyReleaseCurrent {
        active_installed_receipt_sha256: None,
        catalog_sha256: HASH_B.to_owned(),
        commit: SOURCE_COMMIT.to_owned(),
        control_installed_receipt_sha256: None,
        contract_version: "adjustment-family-release-current-status/v1".to_owned(),
        family: "temperature".to_owned(),
        release: SOURCE_RELEASE.to_owned(),
        settings_sha256,
        shadow_installed_receipt_sha256: None,
        source_server_image_digest: HASH_C.to_owned(),
    };
    let live = NativeFamilyReleaseLiveObservation {
        commit: TARGET_COMMIT.to_owned(),
        family: "temperature".to_owned(),
        family_identity_sha256: HASH_D.to_owned(),
        image_digests_sha256: HASH_E.to_owned(),
        release: TARGET_RELEASE.to_owned(),
        settings_bytes: SETTINGS.to_vec(),
    };
    (
        input,
        FixtureHost {
            current: NativeFamilyReleaseInspectedCurrent {
                observation,
                authority: current,
            },
            live,
            applied: 0,
        },
        proof,
    )
}

/// Select only a crash-left applying transaction for installed boot recovery.
#[test]
fn native_family_release_store_selects_single_crash_left_recovery() {
    let root = fixture_root();
    fs::create_dir(&root).unwrap();
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
    let lock = File::open(&root).unwrap();
    let state_root = root.join("adjustment-release-state");
    // fixture ownership follows the actual process identity rather than weakening admission
    let uid = unsafe { libc::geteuid() };
    // fixture ownership follows the actual process identity rather than weakening admission
    let gid = unsafe { libc::getegid() };
    let store = NativeFamilyReleaseTransactionStore::for_paths(
        NativeFamilyReleaseTransactionStorePaths {
            state_root: state_root.clone(),
            release_lock_root: root.clone(),
        },
        lock.as_raw_fd(),
        uid,
        gid,
    );
    let (input, host, proof) = fixture();
    let action_sha256 = input.action_sha256.clone();
    let mut port = NativeFamilyReleaseProductionPort::new(
        store,
        host,
        FixtureProof(proof),
        FixtureClock,
        "temperature",
    );
    execute_native_family_release_transaction(input, &mut port).unwrap();
    let path = state_root
        .join("transactions")
        .join(format!("sha256-{action_sha256}.json"));
    let acknowledged = fs::read_to_string(&path).unwrap();
    let applying = acknowledged.replace("\"state\":\"acknowledged\"", "\"state\":\"applying\"");
    fs::write(&path, applying).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();

    assert_eq!(
        port.store.read_unfinished_recovery_action().unwrap(),
        Some(action_sha256)
    );
    fs::remove_dir_all(root).unwrap();
}

/// Allocate one unique absolute disposable store root.
fn fixture_root() -> PathBuf {
    let sequence = FIXTURE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "native-family-release-port-{}-{sequence}",
        std::process::id()
    ))
}

/// Construct one JSON object without relying on producer-private helpers.
fn object(fields: Vec<(&str, JsonValue)>) -> JsonValue {
    JsonValue::Object(
        fields
            .into_iter()
            .map(|(key, value)| (key.encode_utf16().collect(), value))
            .collect(),
    )
}

/// Construct one exact UTF-16 JSON string.
fn string(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}

/// Hash exact fixture bytes with the approved native primitive.
fn digest(bytes: &[u8]) -> String {
    MaintenanceCanonicalJson::sha256(&mut NativeWirePrimitives, bytes)
        .unwrap()
        .as_str()
        .to_owned()
}

/// Hash one canonical fixture action.
fn digest_json(value: &JsonValue) -> String {
    let bytes =
        MaintenanceCanonicalJson::encode_canonical_json(&mut NativeWirePrimitives, value).unwrap();
    digest(&bytes)
}

/// Retain a blocked refresh and the pre-pull sample before selecting a ready capacity refresh.
#[test]
fn native_family_release_capacity_refresh_preserves_receipt_history() {
    let root = fixture_root();
    fs::create_dir(&root).unwrap();
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
    let lock = File::open(&root).unwrap();
    let state_root = root.join("adjustment-release-state");
    fs::create_dir(&state_root).unwrap();
    fs::set_permissions(&state_root, fs::Permissions::from_mode(0o700)).unwrap();
    // follow the actual fixture ownership at the same private store boundary
    let uid = unsafe { libc::geteuid() };
    let gid = unsafe { libc::getegid() };
    let store = NativeFamilyReleaseTransactionStore::for_paths(
        NativeFamilyReleaseTransactionStorePaths {
            state_root: state_root.clone(),
            release_lock_root: root.clone(),
        },
        lock.as_raw_fd(),
        uid,
        gid,
    );
    let original = b"{\"state\":\"capacity_ready\"}\n";
    let refused = b"{\"state\":\"capacity_blocked\"}\n";
    let renewed = b"{\"measuredAt\":\"2026-10-07T12:00:00.000Z\",\"state\":\"capacity_ready\"}\n";
    let path = state_root.join(format!("capacity-sha256-{HASH_A}.json"));
    fs::write(&path, original).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();

    store
        .refresh_capacity_receipt(HASH_A, original, refused, false)
        .unwrap();
    assert_eq!(fs::read(&path).unwrap(), original);
    store
        .refresh_capacity_receipt(HASH_A, original, renewed, true)
        .unwrap();
    assert_eq!(fs::read(&path).unwrap(), renewed);
    // verify both measured receipts remain available under their exact raw content identity
    for bytes in [original.as_slice(), refused.as_slice()] {
        let digest = MaintenanceCanonicalJson::sha256(&mut NativeWirePrimitives, bytes).unwrap();
        let history = state_root.join(format!(
            "capacity-history-sha256-{HASH_A}-{}.json",
            digest.as_str()
        ));
        assert_eq!(fs::read(history).unwrap(), bytes);
    }
    fs::remove_dir_all(root).unwrap();
}
