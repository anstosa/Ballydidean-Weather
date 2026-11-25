use super::*;
use std::{fs, path::Path, process::Command};

/// Disposable exact fsync owner fixture with injected physical headroom.
fn fixture_capacity() -> io::Result<crate::NativeMaintenanceCapacityEvidence> {
    Ok(crate::NativeMaintenanceCapacityEvidence {
        backing: crate::NativeMaintenanceBackingCapacity {
            allocated_bytes: 0,
            free_bytes: 64 * 1024 * 1024 * 1024,
            free_inodes: 1_000_000,
            root_kind: crate::NativeMaintenanceBackingRootKind::BackingC,
        },
        ext4_free_bytes: 64 * 1024 * 1024 * 1024,
        ext4_free_inodes: 1_000_000,
        ext4_block_size: 4096,
        windows_total_bytes: 512 * 1024 * 1024 * 1024,
    })
}

/// Compare the real native durable pack bytes and checkpoint with the original Node pack.
#[test]
fn native_open_pack_matches_source_checkpoint_and_restart() {
    use std::os::unix::fs::DirBuilderExt;
    let root =
        std::env::temp_dir().join(format!("weather-native-open-pack-{}", std::process::id()));
    fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup {
        /// Remove just the disposable test-owned root.
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    let _cleanup = Cleanup(root.clone());
    let source = root.join("source");
    fs::create_dir(&source).unwrap();
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let oracle = repo.join("rust/crates/weather-runtime/tests/native-cold-custody-node-oracle.mjs");
    let output = Command::new("node")
        .arg(oracle)
        .arg("fixtures")
        .arg(&source)
        .arg("mixed")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let output=Command::new("node").current_dir(&repo).args(["--input-type=module","--eval",r#"
import {readFileSync,writeFileSync} from "node:fs";
import {join} from "node:path";
import {createAdjustmentRevisionCustodyPack} from "./scripts/research/adjustment_revision_custody_pack.mjs";
const root=process.argv[1]; let metadata=Buffer.alloc(0), payload=Buffer.alloc(0);
const store={readMetadata:async()=>metadata,payloadSize:async()=>payload.length,readPayload:async(o,n)=>payload.subarray(o,o+n),appendPayload:async b=>{payload=Buffer.concat([payload,b]);},writeMetadata:async b=>{metadata=Buffer.from(b);},truncatePayload:async n=>{payload=payload.subarray(0,n);},remove:async()=>{metadata=Buffer.alloc(0);payload=Buffer.alloc(0);}};
const pack=createAdjustmentRevisionCustodyPack({store});
const status=await pack.append({start:JSON.parse(readFileSync(join(root,"start.json"))),page:JSON.parse(readFileSync(join(root,"page.json")))});
writeFileSync(join(root,"node.metadata"),metadata); writeFileSync(join(root,"node.payload"),payload);
process.stdout.write(status.checkpointSha256);
"#]).arg(&source).output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let expected = String::from_utf8(output.stdout).unwrap();
    let page_bytes = fs::read(source.join("page.json")).unwrap();
    let start_bytes = fs::read(source.join("start.json")).unwrap();
    let request = request_from_page(&page_bytes).unwrap();
    let page = verify_native_maintenance_cold_remote_page(&page_bytes, &request).unwrap();
    let start = verify_native_revision_cold_start_document(
        &start_bytes,
        &mut NativeWirePrimitives,
        &NativeWirePrimitives,
    )
    .unwrap();
    let native_root = root.join("native");
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&native_root)
        .unwrap();
    let mut pack = NativeMaintenanceOpenPack {
        store:
            crate::native_maintenance_open_pack_store::NativeMaintenanceFixedOpenPackStore::fixture(
                native_root.clone(),
                fixture_capacity,
            ),
    };
    let status = pack.append(&page, Some(&start)).unwrap();
    assert_eq!(status.checkpoint_sha256.as_deref(), Some(expected.as_str()));
    assert_eq!(
        fs::read(native_root.join("revision-custody-open.current")).unwrap(),
        fs::read(source.join("node.metadata")).unwrap()
    );
    assert_eq!(
        fs::read(native_root.join("revision-custody-open.payload")).unwrap(),
        fs::read(source.join("node.payload")).unwrap()
    );
    // reopen the actual durable pack, then converge a byte-identical retry
    let mut pack = NativeMaintenanceOpenPack {
        store:
            crate::native_maintenance_open_pack_store::NativeMaintenanceFixedOpenPackStore::fixture(
                native_root.clone(),
                fixture_capacity,
            ),
    };
    assert_eq!(
        pack.append(&page, Some(&start)).unwrap().checkpoint_sha256,
        status.checkpoint_sha256
    );
    assert_eq!(
        pack.build_sealed_segment().unwrap().checkpoint_sha256,
        expected
    );
    pack.remove_sealed(&expected).unwrap();
    assert!(pack.inspect().unwrap().pages.is_empty());
}
