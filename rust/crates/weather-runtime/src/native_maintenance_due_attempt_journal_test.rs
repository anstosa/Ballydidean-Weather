use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use weather_contracts::JsonValue;

use super::{
    NativeMaintenanceCompleteDueTerminalRetirement, NativeMaintenanceDueAttemptJournal,
    NativeMaintenanceDueAttemptJournalError, NativeMaintenanceDueTerminalOutcome,
    NativeMaintenanceDueTerminalReason, NativeMaintenanceDueTerminalRetirementKind,
    NativeMaintenanceRecordDevelopmentCandidateInstalled, NativeMaintenanceRecordDueAttemptContext,
    NativeMaintenanceRecordDueTerminalOutcome, NativeMaintenanceRecordDueTerminalRetirement,
};
use crate::{
    NativeMaintenanceCaptureFamily, NativeMaintenanceJournalAddition,
    NativeMaintenanceJournalStore, NativeWirePrimitives,
};

const HASH_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const HASH_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const HASH_C: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const HASH_D: &str = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
const HASH_E: &str = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
const HASH_F: &str = "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";

const NODE_FIXTURE: &str = r#"
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import {
  ADJUSTMENT_MAINTENANCE_STATE_ROOT_KIND,
  MaintenanceJournal,
} from "./scripts/research/adjustment_maintenance_state.mjs";

class SeededJournalStore {
  constructor(bytes) {
    this.bytes = bytes;
  }

  // return the fixed source state identity
  async initialize() {
    return { rootKind: ADJUSTMENT_MAINTENANCE_STATE_ROOT_KIND };
  }

  // execute one serialized in-memory source transaction
  async withLock(callback) {
    return await callback({
      readJournal: async () => Buffer.from(this.bytes),
      append: async (bytes) => {
        this.bytes = Buffer.concat([this.bytes, bytes]);
      },
      writeHeads: async () => {},
    });
  }
}

const root = process.env.WEATHER_DUE_ATTEMPT_FIXTURE_ROOT;
const store = new SeededJournalStore(readFileSync(join(root, "journal.seed")));
const journal = new MaintenanceJournal(store);
await journal.initialize();
const dueKey = "daily/2027-02-03";
const context = {
  dueKey,
  inputHeadSha256: "a".repeat(64),
  lifecycleHeadSha256: "b".repeat(64),
};
await journal.recordDueAttemptContext({
  ...context,
  now: "2027-02-03T01:00:00.000Z",
});
await journal.recordDueAttemptContext({
  ...context,
  now: "2027-02-03T02:00:00.000Z",
});
const outcome = {
  actionEligible: false,
  candidateGraphSha256: null,
  candidateSha256: null,
  fitReceiptSha256: null,
  reason: "daily_candidate_rejected",
  semanticInputSha256: "c".repeat(64),
  servingChanged: false,
  state: "completed",
};
await journal.recordDueTerminalOutcome({
  dueKey,
  now: "2027-02-03T03:00:00.000Z",
  outcome,
});
await journal.recordDueTerminalOutcome({
  dueKey,
  now: "2027-02-03T04:00:00.000Z",
  outcome,
});
const request = { actionSha256: "e".repeat(64), operation: "retire" };
const retirement = await journal.recordDueTerminalRetirement({
  dueKey,
  kind: "qualified_v3",
  now: "2027-02-03T05:00:00.000Z",
  request,
});
await journal.recordDueTerminalRetirement({
  dueKey,
  kind: "qualified_v3",
  now: "2027-02-03T06:00:00.000Z",
  request,
});
await journal.completeDueTerminalRetirement({
  dueKey,
  now: "2027-02-03T07:00:00.000Z",
  requestSha256: retirement.requestSha256,
});
await journal.completeDueTerminalRetirement({
  dueKey,
  now: "2027-02-03T08:00:00.000Z",
  requestSha256: retirement.requestSha256,
});
const installed = {
  actionSha256: "d".repeat(64),
  artifactSha256: "e".repeat(64),
  candidateGraphSha256: "f".repeat(64),
  candidateSha256: "c".repeat(64),
  family: "wind",
  registrationSha256: "b".repeat(64),
  shadowRegistrationSha256: "a".repeat(64),
};
await journal.recordDevelopmentCandidateInstalled({
  ...installed,
  now: "2027-02-03T09:00:00.000Z",
});
await journal.recordDevelopmentCandidateInstalled({
  ...installed,
  now: "2027-02-03T10:00:00.000Z",
});
writeFileSync(join(root, "journal.expected"), store.bytes);
"#;

/// Remove one bounded disposable journal state root.
struct FixtureRoot(PathBuf);

impl FixtureRoot {
    /// Create one unique literal private state directory.
    fn create() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("fixture clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "weather-native-due-attempt-journal-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir(&root).expect("create fixture root");
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700))
            .expect("protect fixture root");
        Self(root)
    }
}

impl Drop for FixtureRoot {
    /// Remove only the process-owned disposable state root.
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
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

/// Generate exact source mutations over the same verified seed bytes.
fn build_node_fixture(root: &Path) {
    let repository = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let output = Command::new("node")
        .args(["--input-type=module", "--eval", NODE_FIXTURE])
        .current_dir(repository)
        .env("WEATHER_DUE_ATTEMPT_FIXTURE_ROOT", root)
        .output()
        .expect("run predecessor due-attempt fixture");
    // surface only the predecessor's bounded fixture diagnostic
    if !output.status.success() {
        panic!(
            "predecessor due-attempt fixture failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

/// Match exact source bytes across append, retry, retirement, and installation paths.
#[test]
fn due_attempt_mutations_match_the_source_and_reuse_exact_retries() {
    let fixture = FixtureRoot::create();
    let mut codec = NativeWirePrimitives;
    let time = NativeWirePrimitives;
    let (mut store, _) =
        NativeMaintenanceJournalStore::initialize_fixture(fixture.0.clone(), &mut codec, &time)
            .expect("initialize fixture journal");
    let due_payload = object(vec![("inputHeadSha256", string(HASH_A))]);
    let registration_payload = object(vec![
        ("candidateSha256", string(HASH_C)),
        ("family", string("wind")),
        ("registrationSha256", string(HASH_B)),
    ]);
    let confirmation_due_key = format!("confirmation/wind/{HASH_C}");
    store
        .append(
            &[
                NativeMaintenanceJournalAddition {
                    due_key: Some("daily/2027-02-03"),
                    kind: "due_registered",
                    payload: &due_payload,
                    record_key: "due/daily/2027-02-03",
                    recorded_at: "2027-02-03T00:00:00.000Z",
                },
                NativeMaintenanceJournalAddition {
                    due_key: Some(&confirmation_due_key),
                    kind: "confirmation_registered",
                    payload: &registration_payload,
                    record_key: "confirmation/wind/bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb/registered",
                    recorded_at: "2027-02-03T00:00:00.000Z",
                },
            ],
            &mut codec,
            &time,
        )
        .expect("seed source registrations");
    fs::copy(
        fixture.0.join("journal.jsonl"),
        fixture.0.join("journal.seed"),
    )
    .expect("copy source seed");
    build_node_fixture(&fixture.0);

    let due_key = "daily/2027-02-03";
    let mut owner = NativeMaintenanceDueAttemptJournal::new(&mut store);
    let context_input = NativeMaintenanceRecordDueAttemptContext {
        due_key,
        input_head_sha256: HASH_A,
        lifecycle_head_sha256: HASH_B,
        now: "2027-02-03T01:00:00.000Z",
    };
    let context = owner
        .record_due_attempt_context(context_input)
        .expect("record due attempt context");
    owner
        .record_due_attempt_context(NativeMaintenanceRecordDueAttemptContext {
            now: "2027-02-03T02:00:00.000Z",
            ..context_input
        })
        .expect("reuse due attempt context");
    assert_eq!(
        owner.read_due_attempt_context(due_key).unwrap(),
        Some(context)
    );

    let outcome = NativeMaintenanceDueTerminalOutcome::completed(
        NativeMaintenanceDueTerminalReason::DailyCandidateRejected,
        HASH_C.to_owned(),
        false,
    )
    .expect("build terminal outcome");
    owner
        .record_due_terminal_outcome(NativeMaintenanceRecordDueTerminalOutcome {
            due_key,
            now: "2027-02-03T03:00:00.000Z",
            outcome: &outcome,
        })
        .expect("record terminal outcome");
    owner
        .record_due_terminal_outcome(NativeMaintenanceRecordDueTerminalOutcome {
            due_key,
            now: "2027-02-03T04:00:00.000Z",
            outcome: &outcome,
        })
        .expect("reuse terminal outcome");
    assert_eq!(
        owner.read_due_terminal_outcome(due_key).unwrap(),
        Some(outcome)
    );

    let request = object(vec![
        ("actionSha256", string(HASH_E)),
        ("operation", string("retire")),
    ]);
    let retirement = owner
        .record_due_terminal_retirement(NativeMaintenanceRecordDueTerminalRetirement {
            due_key,
            kind: NativeMaintenanceDueTerminalRetirementKind::QualifiedV3,
            now: "2027-02-03T05:00:00.000Z",
            request: &request,
        })
        .expect("record retirement");
    owner
        .record_due_terminal_retirement(NativeMaintenanceRecordDueTerminalRetirement {
            due_key,
            kind: NativeMaintenanceDueTerminalRetirementKind::QualifiedV3,
            now: "2027-02-03T06:00:00.000Z",
            request: &request,
        })
        .expect("reuse retirement");
    owner
        .complete_due_terminal_retirement(NativeMaintenanceCompleteDueTerminalRetirement {
            due_key,
            now: "2027-02-03T07:00:00.000Z",
            request_sha256: &retirement.request_sha256,
        })
        .expect("complete retirement");
    owner
        .complete_due_terminal_retirement(NativeMaintenanceCompleteDueTerminalRetirement {
            due_key,
            now: "2027-02-03T08:00:00.000Z",
            request_sha256: &retirement.request_sha256,
        })
        .expect("reuse retirement completion");
    assert!(
        owner
            .read_due_terminal_retirement(due_key)
            .unwrap()
            .unwrap()
            .completed
    );

    owner
        .record_development_candidate_installed(
            NativeMaintenanceRecordDevelopmentCandidateInstalled {
                action_sha256: HASH_D,
                artifact_sha256: HASH_E,
                candidate_graph_sha256: HASH_F,
                candidate_sha256: HASH_C,
                family: NativeMaintenanceCaptureFamily::Wind,
                now: "2027-02-03T09:00:00.000Z",
                registration_sha256: HASH_B,
                shadow_registration_sha256: HASH_A,
            },
        )
        .expect("record development installation");
    owner
        .record_development_candidate_installed(
            NativeMaintenanceRecordDevelopmentCandidateInstalled {
                action_sha256: HASH_D,
                artifact_sha256: HASH_E,
                candidate_graph_sha256: HASH_F,
                candidate_sha256: HASH_C,
                family: NativeMaintenanceCaptureFamily::Wind,
                now: "2027-02-03T10:00:00.000Z",
                registration_sha256: HASH_B,
                shadow_registration_sha256: HASH_A,
            },
        )
        .expect("reuse development installation");

    assert_eq!(
        fs::read(fixture.0.join("journal.jsonl")).expect("read native journal"),
        fs::read(fixture.0.join("journal.expected")).expect("read source journal")
    );

    assert_eq!(
        owner.record_due_attempt_context(NativeMaintenanceRecordDueAttemptContext {
            due_key,
            input_head_sha256: HASH_A,
            lifecycle_head_sha256: HASH_D,
            now: "2027-02-03T11:00:00.000Z",
        }),
        Err(NativeMaintenanceDueAttemptJournalError::JournalRecordCollision)
    );
    assert_eq!(
        owner.record_development_candidate_installed(
            NativeMaintenanceRecordDevelopmentCandidateInstalled {
                action_sha256: HASH_A,
                artifact_sha256: HASH_E,
                candidate_graph_sha256: HASH_F,
                candidate_sha256: HASH_C,
                family: NativeMaintenanceCaptureFamily::Wind,
                now: "2027-02-03T11:00:00.000Z",
                registration_sha256: HASH_B,
                shadow_registration_sha256: HASH_A,
            },
        ),
        Err(NativeMaintenanceDueAttemptJournalError::ConfirmationDevelopmentInstallationCollision)
    );
}
