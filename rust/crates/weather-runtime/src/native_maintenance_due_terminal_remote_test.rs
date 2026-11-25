use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use weather_contracts::JsonValue;

use super::{
    NativeMaintenanceDueTerminalRemoteError, NativeMaintenanceDueTerminalRemoteOperation,
    NativeMaintenanceDueTerminalRemotePort, reconcile_native_maintenance_due_terminal_retirement,
};
use crate::maintenance_canonical_json::{field, text};
use crate::{
    MaintenanceCanonicalJson, NativeMaintenanceDueAttemptJournal,
    NativeMaintenanceDueTerminalRetirementKind, NativeMaintenanceJournalAddition,
    NativeMaintenanceJournalStore, NativeMaintenanceRecordDueTerminalRetirement,
    NativeWirePrimitives,
};

const HASH_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

const NODE_FIXTURE: &str = r#"
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import {
  canonicalBytes,
  finalizationProof,
  sha256,
  unsupportedProof,
} from "./deploy/test/fixtures/adjustment-owner-requests.mjs";
import {
  validateAdjustmentShadowTerminalRetirementRequestV3,
  validateAdjustmentShadowUnsupportedTerminalRetirementRequestV1,
} from "./deploy/scripts/adjustment-evaluation-package.mjs";

const root = process.env.WEATHER_DUE_TERMINAL_REMOTE_FIXTURE_ROOT;

// construct one source-valid terminal row for either retirement proof class
function terminalRecord(input) {
  return {
    accessSha256: input.accessSha256,
    actionCompletedAt: "2028-02-09T09:10:00.000Z",
    actionDisposition: input.actionDisposition,
    actionSha256: input.actionSha256,
    candidateSha256: "d".repeat(64),
    contractVersion: "adjustment-shadow-terminal-record/v3",
    family: "temperature",
    finalizedMetadataRootSha256: "4".repeat(64),
    finalizedPredictionCount: 2,
    maintenanceAnchorSha256: input.maintenanceAnchorSha256,
    metadataGeneration: 1,
    recordedAt: "2028-02-09T09:11:00.000Z",
    reconciliationSha256: "6".repeat(64),
    registrationSha256: input.registrationSha256,
    reservedKeySha256: "8".repeat(64),
    sourceSha256: "9".repeat(64),
    terminalMemberSha256: input.terminalMemberSha256,
    terminalResultSha256: "b".repeat(64),
  };
}

// bind one tombstone to the exact retained terminal row
function tombstone(record) {
  return {
    contractVersion: "adjustment-shadow-terminal-tombstone/v3",
    reconciliationSha256: record.reconciliationSha256,
    registrationSha256: record.registrationSha256,
    terminalResultSha256: record.terminalResultSha256,
  };
}

const qualifiedRecord = terminalRecord({
  accessSha256: "1".repeat(64),
  actionDisposition: "promoted_verified",
  actionSha256: "2".repeat(64),
  maintenanceAnchorSha256: "5".repeat(64),
  registrationSha256: "7".repeat(64),
  terminalMemberSha256: "a".repeat(64),
});
const qualified = {
  contractVersion: "adjustment-shadow-terminal-retirement-request/v3",
  finalizationProof: finalizationProof({
    actionSha256: qualifiedRecord.actionSha256,
    confirmationAccessSha256: qualifiedRecord.accessSha256,
    fullMemberRootSha256: qualifiedRecord.terminalMemberSha256,
    transferredAnchorSha256: qualifiedRecord.maintenanceAnchorSha256,
  }),
  registrationSha256: qualifiedRecord.registrationSha256,
  terminalRecord: qualifiedRecord,
  terminalTombstone: tombstone(qualifiedRecord),
};
validateAdjustmentShadowTerminalRetirementRequestV3(qualified);
writeFileSync(join(root, "qualified.json"), canonicalBytes(qualified));

const proof = unsupportedProof();
const unsupportedRecord = terminalRecord({
  accessSha256: proof.confirmationAccessSha256,
  actionDisposition: "support_failed_no_action",
  actionSha256: proof.actionSha256,
  maintenanceAnchorSha256: sha256(proof),
  registrationSha256: proof.registrationSha256,
  terminalMemberSha256: proof.fullMemberRootSha256,
});
const unsupported = {
  contractVersion: "adjustment-shadow-unsupported-terminal-retirement-request/v1",
  registrationSha256: unsupportedRecord.registrationSha256,
  terminalRecord: unsupportedRecord,
  terminalTombstone: tombstone(unsupportedRecord),
  unsupportedProof: proof,
};
validateAdjustmentShadowUnsupportedTerminalRetirementRequestV1(unsupported);
writeFileSync(join(root, "unsupported.json"), canonicalBytes(unsupported));
"#;

/// Remove one bounded disposable journal and request fixture root.
struct FixtureRoot(PathBuf);

impl FixtureRoot {
    /// Create one unique literal private fixture directory.
    fn create() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("fixture clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "weather-native-due-terminal-remote-{}-{nonce}",
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

/// Generate two exact source-admitted retirement request documents.
fn build_node_fixture(root: &Path) {
    let repository = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let output = Command::new("node")
        .args(["--input-type=module", "--eval", NODE_FIXTURE])
        .current_dir(repository)
        .env("WEATHER_DUE_TERMINAL_REMOTE_FIXTURE_ROOT", root)
        .output()
        .expect("run predecessor terminal retirement fixture");
    // surface only the predecessor's bounded fixture diagnostic
    if !output.status.success() {
        panic!(
            "predecessor terminal retirement fixture failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

/// One captured exact fixed-operation exchange.
#[derive(Debug)]
struct CapturedExchange {
    operation: NativeMaintenanceDueTerminalRemoteOperation,
    request_sha256: String,
    request: Vec<u8>,
}

/// Ordinary in-memory transport returning a genuine canonical bound acknowledgement.
#[derive(Default)]
struct FixtureRemote {
    exchanges: Vec<CapturedExchange>,
    reject_next_response: bool,
}

impl NativeMaintenanceDueTerminalRemotePort for FixtureRemote {
    /// Capture one exact request and construct the source response projection.
    fn retire(
        &mut self,
        operation: NativeMaintenanceDueTerminalRemoteOperation,
        request_sha256: &str,
        canonical_request: &[u8],
    ) -> io::Result<Vec<u8>> {
        let mut wire = NativeWirePrimitives;
        let request = wire
            .parse_canonical_object(canonical_request)
            .ok_or_else(invalid_response)?;
        let actual_sha256 = MaintenanceCanonicalJson::sha256(&mut wire, canonical_request)
            .ok_or_else(invalid_response)?;
        // bind the fixture to the same canonical identity operand as production
        if actual_sha256.as_str() != request_sha256 {
            return Err(invalid_response());
        }
        let terminal = field(&request, "terminalRecord").ok_or_else(invalid_response)?;
        let registration = text(&request, "registrationSha256").ok_or_else(invalid_response)?;
        let reconciliation = text(terminal, "reconciliationSha256").ok_or_else(invalid_response)?;
        self.exchanges.push(CapturedExchange {
            operation,
            request_sha256: request_sha256.to_owned(),
            request: canonical_request.to_vec(),
        });
        let response = object(vec![
            (
                "contractVersion",
                string("adjustment-shadow-terminal-retirement-result/v3"),
            ),
            (
                "reconciliationSha256",
                string(if self.reject_next_response {
                    HASH_A
                } else {
                    &reconciliation
                }),
            ),
            ("registrationSha256", string(&registration)),
            ("state", string("retired")),
        ]);
        self.reject_next_response = false;
        wire.encode_canonical_json(&response)
            .ok_or_else(invalid_response)
    }
}

/// Build one stable private fixture transport error.
fn invalid_response() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "fixture response invalid")
}

/// Parse one source-generated canonical object.
fn read_request(path: &Path) -> JsonValue {
    let mut wire = NativeWirePrimitives;
    wire.parse_canonical_object(&fs::read(path).expect("read request fixture"))
        .expect("parse canonical request fixture")
}

/// Build one exact ordinary object in source order.
fn object(fields: Vec<(&str, JsonValue)>) -> JsonValue {
    JsonValue::Object(
        fields
            .into_iter()
            .map(|(name, value)| (name.encode_utf16().collect(), value))
            .collect(),
    )
}

/// Build one source JSON string.
fn string(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}

/// Reconcile both source contract classes and never complete before a valid response.
#[test]
fn terminal_remote_reconciliation_matches_source_requests_and_acknowledgements() {
    let fixture = FixtureRoot::create();
    build_node_fixture(&fixture.0);
    let qualified = read_request(&fixture.0.join("qualified.json"));
    let unsupported = read_request(&fixture.0.join("unsupported.json"));
    let mut codec = NativeWirePrimitives;
    let time = NativeWirePrimitives;
    let (mut store, _) =
        NativeMaintenanceJournalStore::initialize_fixture(fixture.0.clone(), &mut codec, &time)
            .expect("initialize fixture journal");
    let due_payload = object(vec![("inputHeadSha256", string(HASH_A))]);
    let due_keys = ["daily/2028-02-09", "daily/2028-02-10", "daily/2028-02-11"];
    store
        .append(
            &[
                NativeMaintenanceJournalAddition {
                    due_key: Some(due_keys[0]),
                    kind: "due_registered",
                    payload: &due_payload,
                    record_key: "due/daily/2028-02-09",
                    recorded_at: "2028-02-09T00:00:00.000Z",
                },
                NativeMaintenanceJournalAddition {
                    due_key: Some(due_keys[1]),
                    kind: "due_registered",
                    payload: &due_payload,
                    record_key: "due/daily/2028-02-10",
                    recorded_at: "2028-02-09T00:00:00.000Z",
                },
                NativeMaintenanceJournalAddition {
                    due_key: Some(due_keys[2]),
                    kind: "due_registered",
                    payload: &due_payload,
                    record_key: "due/daily/2028-02-11",
                    recorded_at: "2028-02-09T00:00:00.000Z",
                },
            ],
            &mut codec,
            &time,
        )
        .expect("seed due registrations");
    let mut journal = NativeMaintenanceDueAttemptJournal::new(&mut store);
    journal
        .record_due_terminal_retirement(NativeMaintenanceRecordDueTerminalRetirement {
            due_key: due_keys[0],
            kind: NativeMaintenanceDueTerminalRetirementKind::QualifiedV3,
            now: "2028-02-09T01:00:00.000Z",
            request: &qualified,
        })
        .expect("retain qualified retirement");
    journal
        .record_due_terminal_retirement(NativeMaintenanceRecordDueTerminalRetirement {
            due_key: due_keys[1],
            kind: NativeMaintenanceDueTerminalRetirementKind::UnsupportedV1,
            now: "2028-02-09T01:00:00.000Z",
            request: &unsupported,
        })
        .expect("retain unsupported retirement");
    journal
        .record_due_terminal_retirement(NativeMaintenanceRecordDueTerminalRetirement {
            due_key: due_keys[2],
            kind: NativeMaintenanceDueTerminalRetirementKind::QualifiedV3,
            now: "2028-02-09T01:00:00.000Z",
            request: &qualified,
        })
        .expect("retain interrupted retirement");

    let mut remote = FixtureRemote::default();
    let qualified_result = reconcile_native_maintenance_due_terminal_retirement(
        &mut journal,
        &mut remote,
        due_keys[0],
        "2028-02-09T02:00:00.000Z",
    )
    .expect("reconcile qualified retirement");
    assert!(qualified_result.completed);
    let unsupported_result = reconcile_native_maintenance_due_terminal_retirement(
        &mut journal,
        &mut remote,
        due_keys[1],
        "2028-02-09T02:00:00.000Z",
    )
    .expect("reconcile unsupported retirement");
    assert!(unsupported_result.completed);
    assert_eq!(remote.exchanges.len(), 2);
    assert_eq!(
        remote.exchanges[0].operation,
        NativeMaintenanceDueTerminalRemoteOperation::QualifiedV3
    );
    assert_eq!(
        remote.exchanges[1].operation,
        NativeMaintenanceDueTerminalRemoteOperation::UnsupportedV1
    );
    assert_eq!(
        remote.exchanges[0].request,
        fs::read(fixture.0.join("qualified.json")).unwrap()
    );
    assert_eq!(
        remote.exchanges[1].request,
        fs::read(fixture.0.join("unsupported.json")).unwrap()
    );
    assert!(
        remote
            .exchanges
            .iter()
            .all(|exchange| exchange.request_sha256.len() == 64)
    );

    reconcile_native_maintenance_due_terminal_retirement(
        &mut journal,
        &mut remote,
        due_keys[0],
        "not-consulted-on-completed-retry",
    )
    .expect("reuse completed retirement");
    assert_eq!(remote.exchanges.len(), 2);

    remote.reject_next_response = true;
    assert_eq!(
        reconcile_native_maintenance_due_terminal_retirement(
            &mut journal,
            &mut remote,
            due_keys[2],
            "2028-02-09T02:00:00.000Z",
        ),
        Err(NativeMaintenanceDueTerminalRemoteError::Response)
    );
    assert!(
        !journal
            .read_due_terminal_retirement(due_keys[2])
            .unwrap()
            .unwrap()
            .completed
    );
}
