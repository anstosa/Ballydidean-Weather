use std::collections::VecDeque;

use sha2::{Digest, Sha256};

use super::{
    NativeMaintenanceCaptureRemoteAuthority, NativeMaintenanceCaptureRemoteAuthorityError,
    NativeMaintenanceCaptureRemoteCommandPort, NativeMaintenanceCaptureRemoteOperation,
};
use crate::native_maintenance_capture_readiness::{
    NativeMaintenanceCaptureFamily, NativeMaintenanceCaptureSourceAuthority,
};

const HASH_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const HASH_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const HASH_C: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const HASH_D: &str = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
const GENESIS_FRONTIER: &str = "5d932b9623819be9432877a513194158708719497d755155f5ed9a4b501b48af";
const MIGRATION_HISTORY: &str = "6de5c8c7efaa448aeb12bf1a9debe6fe7d4d4d1003ee0e21ab619ffa624c3424";
const EPOCH_AT: &str = "2026-10-10T08:00:00.000Z";

#[derive(Default)]
struct FixtureCommand {
    operations: Vec<NativeMaintenanceCaptureRemoteOperation>,
    responses: VecDeque<Vec<u8>>,
}

impl NativeMaintenanceCaptureRemoteCommandPort for FixtureCommand {
    type Error = &'static str;

    /// Record the typed operation and return its next isolated response.
    fn read_forced(
        &mut self,
        operation: NativeMaintenanceCaptureRemoteOperation,
    ) -> Result<Vec<u8>, Self::Error> {
        self.operations.push(operation);
        self.responses.pop_front().ok_or("missing fixture response")
    }
}

/// Project exact current and lineage documents through their fixed forced verbs.
#[test]
fn native_remote_source_authority_uses_closed_operations() {
    let current = format!(
        concat!(
            "{{\"activeInstalledReceiptSha256\":null,\"catalogSha256\":\"{HASH_A}\",",
            "\"commit\":\"{}\",\"contractVersion\":\"adjustment-family-release-current-status/v1\",",
            "\"controlInstalledReceiptSha256\":null,\"family\":\"temperature\",",
            "\"release\":\"2026.10.10-1\",\"settingsSha256\":\"{HASH_B}\",",
            "\"shadowInstalledReceiptSha256\":\"{HASH_C}\",",
            "\"sourceServerImageDigest\":\"sha256:{HASH_D}\"}}\n"
        ),
        "1".repeat(40),
        HASH_A = HASH_A,
        HASH_B = HASH_B,
        HASH_C = HASH_C,
        HASH_D = HASH_D,
    );
    let lineage = format!(
        concat!(
            "{{\"contractVersion\":\"adjustment-family-release-lineage/v2\",",
            "\"controlSha256\":\"{HASH_A}\",\"controlVersion\":\"14\",",
            "\"currentActionSha256\":null,\"currentCommit\":\"{}\",",
            "\"currentRelease\":\"2026.10.10-1\",\"epochAncestorCommit\":\"{}\",",
            "\"epochWitnessSha256\":\"{HASH_C}\",\"state\":\"verified_epoch_descendant\",",
            "\"verifiedAt\":\"2026-10-11T09:00:00.000Z\"}}\n"
        ),
        "2".repeat(40),
        "1".repeat(40),
        HASH_A = HASH_A,
        HASH_C = HASH_C,
    );
    let mut command = FixtureCommand {
        operations: Vec::new(),
        responses: VecDeque::from([current.into_bytes(), lineage.into_bytes()]),
    };
    let mut authority = NativeMaintenanceCaptureRemoteAuthority::new(&mut command);

    let current = authority
        .read_verified_family_current(NativeMaintenanceCaptureFamily::Temperature)
        .unwrap();
    let lineage = authority.read_verified_family_lineage().unwrap();
    assert_eq!(current.family, "temperature");
    assert_eq!(lineage.current_commit, "2".repeat(40));
    assert_eq!(
        command.operations,
        [
            NativeMaintenanceCaptureRemoteOperation::FamilyCurrent(
                NativeMaintenanceCaptureFamily::Temperature,
            ),
            NativeMaintenanceCaptureRemoteOperation::FamilyLineage,
        ],
    );
    assert_eq!(
        command.operations[0].verb(),
        "adjustment-family-release-current-v1"
    );
    assert_eq!(command.operations[0].arguments(), ["temperature"]);
    assert_eq!(command.operations[1].arguments(), &[] as &[&str]);
}

/// Reuse the actual operator validators for the retained witness and zero snapshot.
#[test]
fn native_remote_epoch_helpers_project_actual_canonical_fixture() {
    let snapshot_sha256 = digest(
        format!("adjustment-revision-serving-snapshot/v1\n{EPOCH_AT}\n0\n{GENESIS_FRONTIER}\n")
            .as_bytes(),
    );
    let snapshot = format!(
        concat!(
            "{{\"archiveCommitOrdinal\":\"0\",",
            "\"contractVersion\":\"adjustment-revision-serving-snapshot/v1\",",
            "\"cutoffAt\":\"{EPOCH_AT}\",\"entries\":[],\"entryCount\":0,",
            "\"frontierSha256\":\"{GENESIS_FRONTIER}\",",
            "\"snapshotSha256\":\"{}\"}}\n"
        ),
        snapshot_sha256,
        EPOCH_AT = EPOCH_AT,
        GENESIS_FRONTIER = GENESIS_FRONTIER,
    );
    let unsigned = format!(
        concat!(
            "{{\"activationKind\":\"inert_v14_pre_activation\",\"archiveCommitOrdinal\":\"0\",",
            "\"catalogFrontierSha256\":\"{GENESIS_FRONTIER}\",",
            "\"contractVersion\":\"adjustment-revision-capture-epoch-witness/v1\",",
            "\"controlPlaneSha256\":\"{HASH_A}\",\"controlPlaneVersion\":\"14\",",
            "\"databaseMigrationHistorySha256\":\"{MIGRATION_HISTORY}\",",
            "\"epochAt\":\"{EPOCH_AT}\",\"servingSnapshotSha256\":\"{}\",",
            "\"sourceCommit\":\"{}\",\"sourceRelease\":\"2026.10.10-1\",",
            "\"sourceServerImageDigest\":\"sha256:{HASH_C}\",",
            "\"sourceWebImageDigest\":\"sha256:{HASH_D}\"}}\n"
        ),
        snapshot_sha256,
        "1".repeat(40),
        GENESIS_FRONTIER = GENESIS_FRONTIER,
        HASH_A = HASH_A,
        MIGRATION_HISTORY = MIGRATION_HISTORY,
        EPOCH_AT = EPOCH_AT,
        HASH_C = HASH_C,
        HASH_D = HASH_D,
    );
    let witness_sha256 = digest(unsigned.as_bytes());
    let witness = format!(
        "{},\"witnessSha256\":\"{}\"}}\n",
        unsigned.strip_suffix("}\n").unwrap(),
        witness_sha256,
    );
    let mut command = FixtureCommand {
        operations: Vec::new(),
        responses: VecDeque::from([witness.into_bytes(), snapshot.into_bytes()]),
    };
    let mut authority = NativeMaintenanceCaptureRemoteAuthority::new(&mut command);

    let witness = authority.read_verified_capture_epoch_witness().unwrap();
    let snapshot = authority.read_verified_capture_epoch_snapshot().unwrap();
    assert_eq!(witness.witness_sha256, witness_sha256);
    assert_eq!(snapshot.snapshot_sha256, snapshot_sha256);
    assert!(snapshot.entries_empty);
    assert_eq!(
        command.operations,
        [
            NativeMaintenanceCaptureRemoteOperation::CaptureEpochWitness,
            NativeMaintenanceCaptureRemoteOperation::CaptureEpochSnapshot,
        ],
    );
}

/// Refuse compact JSON that omits the producer's required newline framing.
#[test]
fn native_remote_authority_rejects_noncanonical_line_framing() {
    let mut command = FixtureCommand {
        operations: Vec::new(),
        responses: VecDeque::from([b"{}".to_vec()]),
    };
    let mut authority = NativeMaintenanceCaptureRemoteAuthority::new(&mut command);
    assert!(matches!(
        authority.read_verified_family_lineage(),
        Err(NativeMaintenanceCaptureRemoteAuthorityError::InvalidResponse),
    ));
}

/// Render one lowercase fixture SHA-256 without a secondary codec dependency.
fn digest(bytes: &[u8]) -> String {
    // spell every digest byte as exactly two lowercase hexadecimal digits
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
