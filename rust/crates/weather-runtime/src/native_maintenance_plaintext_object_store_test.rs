use super::*;
use std::fs::{self, DirBuilder};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};

use crate::native_maintenance_backing_capacity::{
    NativeMaintenanceBackingCapacity, NativeMaintenanceBackingRootKind,
};
use crate::native_maintenance_cas_pack::{
    NativeMaintenanceCasMember, NativeMaintenanceCasMemberValidator,
    NativeMaintenanceCasStreamSession, NativeMaintenanceCasStreamValidator,
    encode_native_maintenance_cas_pack, inspect_native_maintenance_cas_pack,
};
use crate::native_maintenance_full_graph::verify_native_maintenance_full_graph;
use crate::native_maintenance_graph_manifest::{
    NativeMaintenanceGraphEntry, build_native_maintenance_graph_manifest,
};
use crate::native_maintenance_retirement_files_v2::format_digest;

const FREE_INODE_FLOOR: u64 = 32_768;
const FREE_SPACE_FLOOR: u64 = 16 * 1_024 * 1_024 * 1_024;

/// One disposable exact home and fixed archive layout.
struct FixtureHome {
    archive_root: PathBuf,
    home: PathBuf,
}

impl FixtureHome {
    /// Create one process-unique owner-private fixed-layout archive fixture.
    fn create() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("fixture clock")
            .as_nanos();
        let home = std::env::temp_dir().join(format!(
            "weather-native-plaintext-object-store-{}-{nonce}",
            std::process::id()
        ));
        DirBuilder::new()
            .mode(0o700)
            .create(&home)
            .expect("create fixture home");
        let mut current = home.clone();
        // create the exact source archive chain with private directory modes
        for segment in [
            ".weather",
            "adjustment-maintenance",
            "v2",
            "archive-primary",
            "objects",
        ] {
            current.push(segment);
            DirBuilder::new()
                .mode(0o700)
                .create(&current)
                .expect("create fixture archive directory");
        }
        Self {
            archive_root: current.parent().expect("archive parent").to_path_buf(),
            home,
        }
    }

    /// Return the literal fixed object directory.
    fn object_root(&self) -> PathBuf {
        self.archive_root.join("objects")
    }
}

impl Drop for FixtureHome {
    /// Remove only the process-owned disposable fixture root.
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.home);
    }
}

/// One explicit dual-layer capacity sample above every source floor.
fn capacity() -> NativeMaintenanceCapacityEvidence {
    NativeMaintenanceCapacityEvidence {
        backing: NativeMaintenanceBackingCapacity {
            allocated_bytes: 0,
            free_bytes: FREE_SPACE_FLOOR + 8 * 1_024 * 1_024,
            free_inodes: FREE_INODE_FLOOR + 128,
            root_kind: NativeMaintenanceBackingRootKind::BackingC,
        },
        ext4_free_bytes: FREE_SPACE_FLOOR + 8 * 1_024 * 1_024,
        ext4_free_inodes: FREE_INODE_FLOOR + 128,
        ext4_block_size: 4096,
        windows_total_bytes: 128 * 1_024 * 1_024 * 1_024,
    }
}

/// Admit only the one exact ordinary CAS fixture member.
struct FixtureValidator;

impl NativeMaintenanceCasMemberValidator for FixtureValidator {
    /// Validate the exact fixture kind and payload.
    fn validate(&mut self, kind: &str, payload: &[u8]) -> bool {
        kind == "fixture/plaintext-object" && payload == b"weather-object"
    }
}

/// Retain one small fixture member for streamed validation.
struct FixtureSession {
    bytes: Vec<u8>,
}

impl NativeMaintenanceCasStreamValidator for FixtureValidator {
    type Session = FixtureSession;

    /// Open only the exact finite ordinary fixture member.
    fn begin(&mut self, kind: &str, length: u64) -> Option<Self::Session> {
        (kind == "fixture/plaintext-object" && length == 14).then(|| FixtureSession {
            bytes: Vec::with_capacity(14),
        })
    }
}

impl NativeMaintenanceCasStreamSession for FixtureSession {
    /// Retain no more than the advertised fixture member length.
    fn write(&mut self, bytes: &[u8]) -> bool {
        self.bytes.extend_from_slice(bytes);
        self.bytes.len() <= 14
    }

    /// Validate the complete fixture payload after streaming.
    fn finish(self) -> bool {
        self.bytes == b"weather-object"
    }
}

/// Encode one real source-shaped binary CAS object for filesystem publication.
fn cas_bytes() -> Vec<u8> {
    encode_native_maintenance_cas_pack(
        vec![NativeMaintenanceCasMember {
            kind: "fixture/plaintext-object",
            identity_sha256: Sha256::digest(b"weather-object").into(),
            payload: b"weather-object",
        }],
        &mut FixtureValidator,
    )
    .expect("encode fixture CAS")
}

/// Build one source-valid graph object addressing the ordinary fixture CAS member.
fn graph_cas_bytes(payload_object: &[u8]) -> Vec<u8> {
    let inventory = inspect_native_maintenance_cas_pack(payload_object, &mut FixtureValidator)
        .expect("inspect fixture payload CAS");
    let member = inventory.members.first().expect("fixture member");
    let manifest = build_native_maintenance_graph_manifest(
        None,
        vec![NativeMaintenanceGraphEntry {
            identity_sha256: format_digest(member.identity_sha256),
            kind: member.kind.clone(),
            member_length: member.member_length,
            member_offset: member.member_offset,
            member_sha256: format_digest(member.member_sha256),
            object_sha256: format_digest(inventory.object_sha256),
        }],
        vec![],
    )
    .expect("build fixture graph manifest");
    let payload = manifest.canonical_bytes().expect("encode fixture manifest");
    encode_native_maintenance_cas_pack(
        vec![NativeMaintenanceCasMember {
            kind: "adjustment-graph-manifest/v2",
            identity_sha256: Sha256::digest(&payload).into(),
            payload: &payload,
        }],
        &mut FixtureValidator,
    )
    .expect("encode fixture graph CAS")
}

/// Encode one digest with the source lowercase hexadecimal spelling.
fn fixture_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    // encode every fixture digest byte without formatting behavior
    for byte in bytes {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}

/// Commit, reopen, stream-validate, and idempotently reuse one immutable object.
#[test]
fn plaintext_object_store_publishes_and_reuses_verified_cas_bytes() {
    let fixture = FixtureHome::create();
    let measured = capacity();
    let mut store =
        NativeMaintenancePlaintextObjectStore::initialize_fixture(fixture.archive_root.clone())
            .expect("initialize fixture store");
    assert_eq!(store.root_kind(), NATIVE_MAINTENANCE_ARCHIVE_ROOT_KIND);
    let bytes = cas_bytes();
    let envelope = store.measure(&measured).expect("measure empty archive");
    let committed = store
        .commit_object(
            &bytes,
            NativeMaintenancePlaintextObjectAdmission {
                envelope: &envelope,
                free_inode_floor: FREE_INODE_FLOOR,
            },
        )
        .expect("commit fixture object");
    assert!(committed.created);
    assert_eq!(committed.byte_length, bytes.len() as u64);
    let expected_hash = fixture_hex(&Sha256::digest(&bytes));
    assert_eq!(committed.object_sha256, expected_hash);
    let object_path = fixture
        .object_root()
        .join(format!("sha256-{expected_hash}.obj"));
    assert_eq!(
        fs::read(&object_path).expect("read committed object"),
        bytes
    );
    let details = fs::symlink_metadata(&object_path).expect("stat committed object");
    assert_eq!(details.permissions().mode() & 0o777, 0o600);
    assert_eq!(details.nlink(), 1);
    let reader = store
        .open_object(&expected_hash)
        .expect("open fixture object")
        .expect("fixture object exists");
    assert_eq!(reader.size(), bytes.len() as u64);
    assert_eq!(
        reader.read_range(0, 8).expect("read CAS magic"),
        b"WXACAS01"
    );
    let inventory = store
        .inspect_object(&expected_hash, &mut FixtureValidator)
        .expect("inspect fixture CAS")
        .expect("fixture CAS exists");
    assert_eq!(inventory.byte_length, bytes.len() as u64);
    assert_eq!(inventory.members.len(), 1);

    let graph_bytes = graph_cas_bytes(&bytes);
    let graph_envelope = store.measure(&measured).expect("measure payload archive");
    let graph_commit = store
        .commit_object(
            &graph_bytes,
            NativeMaintenancePlaintextObjectAdmission {
                envelope: &graph_envelope,
                free_inode_floor: FREE_INODE_FLOOR,
            },
        )
        .expect("commit fixture graph object");
    let graph_digest: [u8; 32] = Sha256::digest(&graph_bytes).into();
    assert_eq!(graph_commit.object_sha256, format_digest(graph_digest));
    let verified =
        verify_native_maintenance_full_graph(&mut store, graph_digest, &mut FixtureValidator, true)
            .expect("verify fixture full graph");
    store
        .rotate_verified_head(&verified)
        .expect("publish genesis graph head");
    store
        .rotate_verified_head(&verified)
        .expect("rotate graph head to previous");
    let pointer = format!("{}\n", format_digest(graph_digest));
    assert_eq!(
        fs::read(fixture.archive_root.join("head.current")).expect("read current head"),
        pointer.as_bytes()
    );
    assert_eq!(
        fs::read(fixture.archive_root.join("head.previous")).expect("read previous head"),
        pointer.as_bytes()
    );
    let published = store.measure(&measured).expect("measure published archive");
    assert_eq!(published.object_count, 2);
    assert_eq!(published.incoming_count, 0);
    assert_eq!(published.file_count, 4);
    assert_eq!(published.task_inodes, 4);
    let allocated = [
        object_path,
        fixture
            .object_root()
            .join(format!("sha256-{}.obj", format_digest(graph_digest))),
        fixture.archive_root.join("head.current"),
        fixture.archive_root.join("head.previous"),
    ]
    .into_iter()
    .map(|path| {
        fs::symlink_metadata(path)
            .expect("stat archive file")
            .blocks()
            * 512
    })
    .sum::<u64>();
    assert_eq!(published.allocated_bytes, allocated);

    let refused_envelope = NativeMaintenancePlaintextPublicationEnvelope {
        free_bytes: 0,
        ..published
    };
    let reused = store
        .commit_object(
            &bytes,
            NativeMaintenancePlaintextObjectAdmission {
                envelope: &refused_envelope,
                free_inode_floor: FREE_INODE_FLOOR,
            },
        )
        .expect("reuse existing object before admission");
    assert!(!reused.created);
    assert_eq!(reused.object_sha256, expected_hash);
    let entries = fs::read_dir(fixture.object_root())
        .expect("list fixture objects")
        .collect::<Result<Vec<_>, _>>()
        .expect("read fixture object entries");
    assert_eq!(entries.len(), 2);
}

/// Refuse insufficient explicit capacity before creating any incoming file.
#[test]
fn plaintext_object_store_refuses_unadmitted_capacity_without_mutation() {
    let fixture = FixtureHome::create();
    let measured = capacity();
    let store =
        NativeMaintenancePlaintextObjectStore::initialize_fixture(fixture.archive_root.clone())
            .expect("initialize fixture store");
    let refused = NativeMaintenancePlaintextPublicationEnvelope {
        free_bytes: FREE_SPACE_FLOOR,
        ..store.measure(&measured).expect("measure empty archive")
    };
    let error = store
        .commit_object(
            &cas_bytes(),
            NativeMaintenancePlaintextObjectAdmission {
                envelope: &refused,
                free_inode_floor: FREE_INODE_FLOOR,
            },
        )
        .expect_err("refuse capacity below rounded delta");
    assert_eq!(error, NativeMaintenancePlaintextObjectStoreError::Capacity);
    assert_eq!(
        fs::read_dir(fixture.object_root())
            .expect("list fixture objects")
            .count(),
        0
    );
}
