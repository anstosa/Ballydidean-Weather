use std::path::Path;
use std::process::Command;

use super::{
    NATIVE_MAINTENANCE_CATALOG_ENTRY_BYTES, NATIVE_MAINTENANCE_CATALOG_HEADER_BYTES,
    NativeMaintenanceCatalogEntry, NativeMaintenanceCatalogShardInput,
    encode_native_maintenance_catalog_entry, encode_native_maintenance_catalog_shard,
    inspect_native_maintenance_catalog_shard,
};

const HASH_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const HASH_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const HASH_C: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const HASH_D: &str = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";

/// Match the actual source artifact for roots, canonical order, and partition header.
#[test]
fn native_catalog_shard_matches_plaintext_archive_source() {
    let entries = [
        NativeMaintenanceCatalogEntry {
            identity_sha256: HASH_A,
            kind: "test-record/v1",
            member_sha256: HASH_B,
        },
        NativeMaintenanceCatalogEntry {
            identity_sha256: HASH_D,
            kind: "adjustment-graph-manifest/v2",
            member_sha256: HASH_C,
        },
    ];
    let native = encode_native_maintenance_catalog_shard(NativeMaintenanceCatalogShardInput {
        entries: &entries,
        generation: "18446744073709551615",
        object_sha256: HASH_C,
        shard_index: 511,
    })
    .unwrap();
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../scripts/research/adjustment_plaintext_archive.mjs");
    let script = r#"
import { pathToFileURL } from "node:url";
const source = await import(pathToFileURL(process.argv[1]));
const bytes = source.encodeCatalogShard({
  entries: [
    { identitySha256: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", kind: "test-record/v1", memberSha256: "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb" },
    { identitySha256: "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd", kind: "adjustment-graph-manifest/v2", memberSha256: "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc" },
  ],
  generation: "18446744073709551615",
  objectSha256: "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
  shardIndex: 511,
});
process.stdout.write(bytes.toString("hex"));
"#;
    let output = Command::new("node")
        .args([
            "--input-type=module",
            "--eval",
            script,
            source.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr),
    );
    let source_hex = String::from_utf8(output.stdout).unwrap();
    assert_eq!(hex(&native), source_hex);

    let inspected = inspect_native_maintenance_catalog_shard(&native).unwrap();
    assert_eq!(inspected.generation, "18446744073709551615");
    assert_eq!(inspected.object_sha256, HASH_C);
    assert_eq!(inspected.shard_index, 511);
    assert_eq!(inspected.entry_count, 2);
    let mut expected = entries
        .iter()
        .copied()
        .map(|entry| encode_native_maintenance_catalog_entry(entry).unwrap())
        .collect::<Vec<_>>();
    expected.sort_unstable();
    assert_eq!(
        inspected.entries[0].identity_sha256,
        hex(&expected[0][32..64])
    );
    assert_eq!(
        inspected.entries[1].identity_sha256,
        hex(&expected[1][32..64])
    );
}

/// Reject noncanonical generations, duplicate records, framing, and entry order.
#[test]
fn native_catalog_wire_refuses_source_invalid_shapes() {
    let entry = NativeMaintenanceCatalogEntry {
        identity_sha256: HASH_A,
        kind: "test-record/v1",
        member_sha256: HASH_B,
    };
    assert!(encode_native_maintenance_catalog_entry(entry).is_some());
    assert!(
        encode_native_maintenance_catalog_entry(NativeMaintenanceCatalogEntry {
            kind: "Test-record/v1",
            ..entry
        })
        .is_none()
    );
    assert!(
        encode_native_maintenance_catalog_shard(NativeMaintenanceCatalogShardInput {
            entries: &[entry, entry],
            generation: "01",
            object_sha256: HASH_C,
            shard_index: 512,
        })
        .is_none()
    );
    assert!(
        encode_native_maintenance_catalog_shard(NativeMaintenanceCatalogShardInput {
            entries: &[entry, entry],
            generation: "1",
            object_sha256: HASH_C,
            shard_index: 0,
        })
        .is_none()
    );
    let second = NativeMaintenanceCatalogEntry {
        identity_sha256: HASH_D,
        kind: "adjustment-graph-manifest/v2",
        member_sha256: HASH_C,
    };
    let mut shard = encode_native_maintenance_catalog_shard(NativeMaintenanceCatalogShardInput {
        entries: &[entry, second],
        generation: "1",
        object_sha256: HASH_C,
        shard_index: 0,
    })
    .unwrap();
    shard[56] = 1;
    assert!(inspect_native_maintenance_catalog_shard(&shard).is_none());
    shard[56] = 0;
    let first = NATIVE_MAINTENANCE_CATALOG_HEADER_BYTES;
    let second = first + NATIVE_MAINTENANCE_CATALOG_ENTRY_BYTES;
    // invert the canonical full-record order without changing the declared count
    for offset in 0..NATIVE_MAINTENANCE_CATALOG_ENTRY_BYTES {
        shard.swap(first + offset, second + offset);
    }
    assert!(inspect_native_maintenance_catalog_shard(&shard).is_none());
    // restore canonical order before independently testing exact-length framing
    for offset in 0..NATIVE_MAINTENANCE_CATALOG_ENTRY_BYTES {
        shard.swap(first + offset, second + offset);
    }
    shard.push(0);
    assert!(inspect_native_maintenance_catalog_shard(&shard).is_none());
}

/// Render fixture bytes through the same lowercase spelling as Node's buffer hex.
fn hex(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len() * 2);
    // append exactly two lowercase hexadecimal digits for each fixture byte
    for byte in bytes {
        use std::fmt::Write as _;
        write!(&mut text, "{byte:02x}").unwrap();
    }
    text
}
