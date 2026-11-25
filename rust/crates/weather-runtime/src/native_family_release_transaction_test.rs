use weather_contracts::JsonValue;

use crate::maintenance_canonical_json::MaintenanceCanonicalJson;
use crate::{
    NativeFamilyReleaseCurrentObservation, NativeFamilyReleaseDurableProof,
    NativeFamilyReleaseInput, NativeFamilyReleaseLiveObservation,
    NativeFamilyReleaseMutationRecord, NativeFamilyReleaseMutationState,
    NativeFamilyReleaseOperatorOffRecord, NativeFamilyReleaseOutcome,
    NativeFamilyReleasePriorRecord, NativeFamilyReleaseTransactionError,
    NativeFamilyReleaseTransactionPort, NativeFamilyReleaseTransactionResult,
    NativeFamilyReleaseVerifiedProof, NativeWirePrimitives,
    execute_native_family_release_transaction,
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
const SETTINGS_ON: &[u8] = b"{\"version\":1,\"temperature\":true,\"wind\":true,\"rain\":true}\n";
const SETTINGS_OFF: &[u8] = b"{\"version\":1,\"temperature\":false,\"wind\":true,\"rain\":true}\n";

#[derive(Clone)]
struct FixturePort {
    current: NativeFamilyReleaseCurrentObservation,
    events: Vec<&'static str>,
    fail_apply: bool,
    live: NativeFamilyReleaseLiveObservation,
    prior: Option<NativeFamilyReleasePriorRecord>,
    proof: NativeFamilyReleaseVerifiedProof,
    failed_record: Option<NativeFamilyReleaseMutationRecord>,
}

impl NativeFamilyReleaseTransactionPort for FixturePort {
    /// Retain one explicit lock event around every fixture transaction.
    fn with_release_lock<T>(
        &mut self,
        operation: impl FnOnce(&mut Self) -> Result<T, NativeFamilyReleaseTransactionError>,
    ) -> Result<T, NativeFamilyReleaseTransactionError> {
        self.events.push("lock");
        operation(self)
    }

    /// Return the fixed canonical fixture clock.
    fn clock(&mut self) -> Result<String, NativeFamilyReleaseTransactionError> {
        self.events.push("clock");
        Ok(NOW.to_owned())
    }

    /// Return the selected durable replay fixture.
    fn read_prepared(
        &mut self,
        _action_sha256: &str,
    ) -> Result<Option<NativeFamilyReleasePriorRecord>, NativeFamilyReleaseTransactionError> {
        self.events.push("read");
        Ok(self.prior.clone())
    }

    /// Return the selected source compare-and-swap fixture.
    fn inspect_current(
        &mut self,
    ) -> Result<NativeFamilyReleaseCurrentObservation, NativeFamilyReleaseTransactionError> {
        self.events.push("inspect");
        Ok(self.current.clone())
    }

    /// Return material already admitted by the separate authority fixture.
    fn verify_proof(
        &mut self,
        _input: &NativeFamilyReleaseInput,
        _current: &NativeFamilyReleaseCurrentObservation,
        _prior: Option<&NativeFamilyReleaseMutationRecord>,
        _now: &str,
    ) -> Result<NativeFamilyReleaseVerifiedProof, NativeFamilyReleaseTransactionError> {
        self.events.push("proof");
        Ok(self.proof.clone())
    }

    /// Persist or replay the fixture operator-off record.
    fn acknowledge_operator_off(
        &mut self,
        record: &NativeFamilyReleaseOperatorOffRecord,
    ) -> Result<NativeFamilyReleaseOperatorOffRecord, NativeFamilyReleaseTransactionError> {
        self.events.push("operator_off");
        Ok(record.clone())
    }

    /// Retain the prepared fixture transition.
    fn prepare(
        &mut self,
        _record: &NativeFamilyReleaseMutationRecord,
        _proof: &NativeFamilyReleaseVerifiedProof,
    ) -> Result<(), NativeFamilyReleaseTransactionError> {
        self.events.push("prepare");
        Ok(())
    }

    /// Retain the applying fixture transition.
    fn mark_applying(
        &mut self,
        _record: &NativeFamilyReleaseMutationRecord,
    ) -> Result<(), NativeFamilyReleaseTransactionError> {
        self.events.push("applying");
        Ok(())
    }

    /// Apply the fixture target or inject its post-applying failure.
    fn apply_images(
        &mut self,
        _record: &NativeFamilyReleaseMutationRecord,
        _proof: &NativeFamilyReleaseVerifiedProof,
    ) -> Result<(), NativeFamilyReleaseTransactionError> {
        self.events.push("apply");
        // expose one target failure only after applying has been persisted
        if self.fail_apply {
            return Err(NativeFamilyReleaseTransactionError::Apply);
        }
        Ok(())
    }

    /// Return the independently verified fixture live state.
    fn verify_live(
        &mut self,
        _record: &NativeFamilyReleaseMutationRecord,
        _proof: &NativeFamilyReleaseVerifiedProof,
    ) -> Result<NativeFamilyReleaseLiveObservation, NativeFamilyReleaseTransactionError> {
        self.events.push("live");
        Ok(self.live.clone())
    }

    /// Retain the exact compensation-required fixture record.
    fn record_failure(
        &mut self,
        record: &NativeFamilyReleaseMutationRecord,
    ) -> Result<(), NativeFamilyReleaseTransactionError> {
        self.events.push("failure");
        self.failed_record = Some(record.clone());
        Ok(())
    }

    /// Return the durably acknowledged fixture record.
    fn acknowledge(
        &mut self,
        record: &NativeFamilyReleaseMutationRecord,
        _live: &NativeFamilyReleaseLiveObservation,
    ) -> Result<NativeFamilyReleaseMutationRecord, NativeFamilyReleaseTransactionError> {
        self.events.push("ack");
        Ok(record.clone())
    }
}

/// Exercise success, response-lost operator-off, unsafe replay, and post-applying failure paths.
#[test]
fn native_family_release_transaction_preserves_source_transitions() {
    let (input, mut port) = fixture(SETTINGS_ON);
    let result = execute_native_family_release_transaction(input.clone(), &mut port).unwrap();
    let NativeFamilyReleaseTransactionResult::Mutation(record) = result else {
        panic!("target transaction returned operator-off record");
    };
    assert_eq!(record.state, NativeFamilyReleaseMutationState::Acknowledged);
    assert_eq!(record.outcome, Some(NativeFamilyReleaseOutcome::Active));
    assert_eq!(
        port.events,
        [
            "lock", "read", "inspect", "clock", "proof", "prepare", "applying", "apply", "live",
            "clock", "ack"
        ]
    );

    let mut acknowledged_replay = port.clone();
    acknowledged_replay.events.clear();
    acknowledged_replay.prior = Some(NativeFamilyReleasePriorRecord::Mutation(record.clone()));
    let replayed =
        execute_native_family_release_transaction(input.clone(), &mut acknowledged_replay).unwrap();
    assert_eq!(
        replayed,
        NativeFamilyReleaseTransactionResult::Mutation(record.clone())
    );
    assert_eq!(acknowledged_replay.events, ["lock", "read"]);

    let mut replay = port.clone();
    replay.events.clear();
    replay.prior = Some(NativeFamilyReleasePriorRecord::Mutation(
        NativeFamilyReleaseMutationRecord {
            state: NativeFamilyReleaseMutationState::Applying,
            ..record.clone()
        },
    ));
    let result = execute_native_family_release_transaction(input.clone(), &mut replay).unwrap();
    let NativeFamilyReleaseTransactionResult::Mutation(record) = result else {
        panic!("applying replay returned operator-off record");
    };
    assert_eq!(
        record.state,
        NativeFamilyReleaseMutationState::CompensationRequired
    );
    assert_eq!(replay.events, ["lock", "read"]);

    let (operator_off_input, mut operator_off) = fixture(SETTINGS_OFF);
    operator_off.events.clear();
    let result =
        execute_native_family_release_transaction(operator_off_input.clone(), &mut operator_off)
            .unwrap();
    let NativeFamilyReleaseTransactionResult::OperatorOff(operator_record) = result else {
        panic!("operator-off transaction returned mutation record");
    };
    assert_eq!(
        operator_off.events,
        ["lock", "read", "inspect", "clock", "operator_off"]
    );
    operator_off.events.clear();
    operator_off.prior = Some(NativeFamilyReleasePriorRecord::OperatorOff(
        operator_record.clone(),
    ));
    assert_eq!(
        execute_native_family_release_transaction(operator_off_input, &mut operator_off).unwrap(),
        NativeFamilyReleaseTransactionResult::OperatorOff(operator_record)
    );
    assert_eq!(operator_off.events, ["lock", "read", "operator_off"]);

    let (_, mut failing) = fixture(SETTINGS_ON);
    failing.events.clear();
    failing.fail_apply = true;
    assert_eq!(
        execute_native_family_release_transaction(input, &mut failing),
        Err(NativeFamilyReleaseTransactionError::Apply)
    );
    assert_eq!(
        failing.events,
        [
            "lock", "read", "inspect", "clock", "proof", "prepare", "applying", "apply", "clock",
            "failure"
        ]
    );
    assert_eq!(
        failing.failed_record.unwrap().state,
        NativeFamilyReleaseMutationState::CompensationRequired
    );
}

/// Build one fully cross-bound target and compensation proof fixture.
fn fixture(settings: &[u8]) -> (NativeFamilyReleaseInput, FixturePort) {
    let settings_sha256 = digest(settings);
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
            source_settings_bytes: settings.to_vec(),
            target_catalog_bytes: b"{}\n".to_vec(),
            target_commit: TARGET_COMMIT.to_owned(),
            target_family_identity_sha256: HASH_D.to_owned(),
            target_image_digests_sha256: HASH_E.to_owned(),
        },
    };
    let input = NativeFamilyReleaseInput {
        action_sha256,
        compensating_release: COMPENSATION_RELEASE.to_owned(),
        expected_current_release: SOURCE_RELEASE.to_owned(),
        expected_settings_sha256: settings_sha256,
        expected_source_release: SOURCE_RELEASE.to_owned(),
        family: "temperature".to_owned(),
        fencing_token: "2".to_owned(),
        report_sha256: HASH_A.to_owned(),
        target_release: TARGET_RELEASE.to_owned(),
    };
    let current = NativeFamilyReleaseCurrentObservation {
        commit: SOURCE_COMMIT.to_owned(),
        release: SOURCE_RELEASE.to_owned(),
        settings_bytes: settings.to_vec(),
        maximum_fence: "1".to_owned(),
        active_installed_receipt_sha256: None,
        shadow_installed_receipt_sha256: None,
        control_installed_receipt_sha256: None,
    };
    let live = NativeFamilyReleaseLiveObservation {
        commit: TARGET_COMMIT.to_owned(),
        family: "temperature".to_owned(),
        family_identity_sha256: HASH_D.to_owned(),
        image_digests_sha256: HASH_E.to_owned(),
        release: TARGET_RELEASE.to_owned(),
        settings_bytes: settings.to_vec(),
    };
    (
        input,
        FixturePort {
            current,
            events: Vec::new(),
            fail_apply: false,
            live,
            prior: None,
            proof,
            failed_record: None,
        },
    )
}

/// Construct one JSON object without depending on private producer helpers.
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

/// Hash fixture bytes with the native approved primitive.
fn digest(bytes: &[u8]) -> String {
    MaintenanceCanonicalJson::sha256(&mut NativeWirePrimitives, bytes)
        .unwrap()
        .as_str()
        .to_owned()
}

/// Hash one canonical fixture object exactly like the transaction owner.
fn digest_json(value: &JsonValue) -> String {
    let bytes =
        MaintenanceCanonicalJson::encode_canonical_json(&mut NativeWirePrimitives, value).unwrap();
    digest(&bytes)
}
