use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::Command;

use weather_edge::{
    RevisionOnlinePayload, RevisionOnlinePayloadAddress, RevisionOnlinePayloadAdmission,
    RevisionOnlinePayloadKind, RevisionOnlineSpoolError, RevisionOnlineSpoolStorage,
    with_revision_online_payload,
};

use RevisionOnlinePayloadKind::{
    Projection, RainControlState as State, ShadowCapsule as Capsule, ShadowStage as Stage,
};
const PROJECTION_BYTES: usize = 3_500 * 1024;
const MATCHING_CAPSULE_BYTES: usize = 1_332 * 1024;
type FileSpec = (RevisionOnlinePayloadKind, &'static str, usize);

#[derive(Default)]
struct FixtureSpool {
    files: BTreeMap<RevisionOnlinePayloadAddress, usize>,
    held: bool,
    trace: Vec<String>,
}

impl RevisionOnlineSpoolStorage for FixtureSpool {
    /// Hold the fixture spool lease around checked admission and the persisted operation.
    fn with_revision_spool_lease<T>(
        &mut self,
        operation: impl FnOnce(&mut Self) -> Result<T, RevisionOnlineSpoolError>,
    ) -> Result<T, RevisionOnlineSpoolError> {
        assert!(!self.held);
        self.held = true;
        let result = operation(self);
        self.held = false;
        result
    }

    /// Read content only for the exact incoming target being retried.
    fn read_existing_payload(
        &mut self,
        address: &RevisionOnlinePayloadAddress,
        maximum_bytes: usize,
    ) -> Result<Option<Vec<u8>>, RevisionOnlineSpoolError> {
        assert!(self.held);
        assert_eq!(maximum_bytes, PROJECTION_BYTES);
        self.trace.push("target".to_owned());
        Ok(self.files.get(address).map(
            // retain the normal source body fill byte for the retry case
            |bytes| vec![1; *bytes],
        ))
    }

    /// Preserve exact names in the currently inspected directory.
    fn list_payload_names(
        &mut self,
        kind: RevisionOnlinePayloadKind,
    ) -> Result<Vec<String>, RevisionOnlineSpoolError> {
        assert!(self.held);
        self.trace.push(format!("list:{}", kind.directory()));
        Ok(self
            .files
            .keys()
            .filter_map(
                // retain only members of the exact fixed directory
                |address| {
                    let (member, name) = address.components();
                    (member == kind).then_some(name.to_owned())
                },
            )
            .collect())
    }

    /// Observe each retained size without allocating or returning the stored body.
    fn payload_file_length(
        &mut self,
        address: &RevisionOnlinePayloadAddress,
        maximum_bytes: usize,
    ) -> Result<usize, RevisionOnlineSpoolError> {
        assert!(self.held);
        let (kind, _) = address.components();
        assert_eq!(maximum_bytes, kind.maximum_bytes());
        self.trace.push(format!("size:{}", kind.directory()));
        Ok(self.files[address])
    }
}

struct Case {
    label: &'static str,
    files: &'static [FileSpec],
    target: FileSpec,
    replacement: Option<(RevisionOnlinePayloadKind, &'static str)>,
}

/// Build one admitted immutable locator from fixture data.
fn address(kind: RevisionOnlinePayloadKind, digest: &str) -> RevisionOnlinePayloadAddress {
    RevisionOnlinePayloadAddress::from_sha256(kind, &digest.repeat(64)).expect("fixture digest")
}

/// Interpret a single actual Node oracle result.
fn field(line: &str) -> Option<(&str, &str)> {
    line.split_once('=')
}

#[test]
// compare exact Node admission decisions and retention ordering under ordinary bounded loads
fn matches_node_shared_slot_byte_retry_and_replacement_rules() {
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/revision-online-spool-node-oracle.mjs");
    let oracle = Command::new("node")
        .arg(script)
        .output()
        .expect("Node spool oracle");
    assert!(
        oracle.status.success(),
        "{}",
        String::from_utf8_lossy(&oracle.stderr)
    );
    let stdout = String::from_utf8(oracle.stdout).expect("spool oracle UTF-8");
    let observed: BTreeMap<_, _> = stdout.lines().filter_map(field).collect();
    let cases = [
        Case {
            label: "first",
            files: &[],
            target: (State, "a", 4),
            replacement: None,
        },
        Case {
            label: "retry",
            files: &[
                (Projection, "b", PROJECTION_BYTES),
                (Capsule, "c", MATCHING_CAPSULE_BYTES),
            ],
            target: (Capsule, "c", MATCHING_CAPSULE_BYTES),
            replacement: None,
        },
        Case {
            label: "third",
            files: &[
                (Projection, "b", PROJECTION_BYTES),
                (Capsule, "c", MATCHING_CAPSULE_BYTES),
            ],
            target: (State, "a", 4),
            replacement: None,
        },
        Case {
            label: "exact-ceiling",
            files: &[(Projection, "b", PROJECTION_BYTES)],
            target: (Capsule, "c", MATCHING_CAPSULE_BYTES),
            replacement: None,
        },
        Case {
            label: "over-ceiling",
            files: &[(Projection, "b", PROJECTION_BYTES)],
            target: (Capsule, "c", MATCHING_CAPSULE_BYTES + 1),
            replacement: None,
        },
        Case {
            label: "stage-replacement",
            files: &[(Projection, "b", PROJECTION_BYTES), (Stage, "c", 512)],
            target: (Capsule, "d", MATCHING_CAPSULE_BYTES),
            replacement: Some((Stage, "c")),
        },
        Case {
            label: "other-stage-still-charged",
            files: &[(Projection, "b", PROJECTION_BYTES), (Stage, "c", 512)],
            target: (Capsule, "d", MATCHING_CAPSULE_BYTES),
            replacement: Some((Stage, "e")),
        },
    ];
    // retain separate fixture lease, files and incoming bytes for each ordinary workflow
    for case in cases {
        let mut storage = FixtureSpool {
            files: case
                .files
                .iter()
                .map(
                    // keep existing payload sizes without materializing their bodies
                    |&(kind, digest, size)| (address(kind, digest), size),
                )
                .collect(),
            ..FixtureSpool::default()
        };
        let (kind, digest, size) = case.target;
        let target = address(kind, digest);
        let bytes = vec![1; size];
        let replacement = case.replacement.map(
            // prepare the optional exact superseded stage locator
            |(kind, digest)| address(kind, digest),
        );
        let payload = RevisionOnlinePayload::new(&target, &bytes).expect("admitted bytes");
        let payload = match &replacement {
            Some(stage) => payload
                .replacing_shadow_stage(stage)
                .expect("shadow replacement"),
            None => payload,
        };
        let result = with_revision_online_payload(
            &mut storage,
            &payload,
            // require successful callback execution under the same held spool lease
            |storage, disposition| {
                assert!(storage.held);
                Ok(disposition)
            },
        );
        let outcome = match result {
            Ok(disposition) => {
                let expected = if case.label == "retry" {
                    RevisionOnlinePayloadAdmission::IdenticalRetry
                } else {
                    RevisionOnlinePayloadAdmission::Fresh
                };
                assert_eq!(disposition, expected);
                "admitted"
            }
            Err(RevisionOnlineSpoolError::Capacity) => "capacity",
            Err(error) => panic!("unexpected spool error: {error:?}"),
        };
        assert!(!storage.held);
        assert_eq!(
            format!("{outcome}|{}", storage.trace.join(",")),
            observed[case.label],
            "case {}",
            case.label,
        );
    }
}
