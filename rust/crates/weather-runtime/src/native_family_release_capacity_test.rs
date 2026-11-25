use std::io::{BufReader, Cursor, Read, Write};
use std::process::{Command, Stdio};

use flate2::{Compression, write::GzEncoder};

use super::{measure_tar_stream, read_curl_response_headers, sha256_digest};
use crate::native_family_release_capacity::{
    NativeFamilyReleaseCapacityError, NativeFamilyReleaseCapacityImage,
    NativeFamilyReleaseCapacityLayer, NativeFamilyReleaseCapacityMeasurement,
    NativeFamilyReleaseLocalLayer, admit_ready_native_family_release_capacity,
    evaluate_native_dashboard_release_capacity, evaluate_native_family_release_capacity,
};

const HASH: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const MEASURED: &str = "2026-10-08T01:02:03.004Z";

/// prove the native ready receipt equals the original pure capacity formula.
#[test]
fn ready_receipt_matches_source_formula() {
    let receipt = evaluate_native_family_release_capacity(measurement(10_000_000_000, 100_000, 0))
        .expect("native capacity receipt");
    assert_eq!(receipt.exit_code(), 0);
    let native = receipt.canonical_bytes().expect("canonical native receipt");
    let source = source_receipt(10_000_000_000, 100_000, 0);
    assert_eq!(native, source);
    admit_ready_native_family_release_capacity(
        &receipt,
        HASH,
        "wind",
        "2026.10.09-1",
        "2026-10-08T01:17:03.004Z",
    )
    .expect("fresh ready receipt");
}

/// prove the dashboard policy projects the original formula with the actual native core.
#[test]
fn dashboard_receipt_matches_source_formula_and_fixed_core_roles() {
    let input = dashboard_measurement(10_000_000_000, 100_000);
    let receipt = evaluate_native_dashboard_release_capacity(HASH, "2026.11.25-1", input.clone())
        .expect("native dashboard capacity receipt");
    assert_eq!(receipt.exit_code(), 0);
    assert_eq!(
        receipt
            .canonical_bytes()
            .expect("canonical dashboard receipt"),
        source_dashboard_receipt(10_000_000_000, 100_000),
    );

    let mut substituted = input;
    substituted.images[4].digest = format!("sha256:{}", "9".repeat(64));
    assert_eq!(
        evaluate_native_dashboard_release_capacity(HASH, "2026.11.25-1", substituted),
        Err(NativeFamilyReleaseCapacityError::Arguments),
    );
}

/// retain a complete denied receipt and the predecessor's status three.
#[test]
fn denied_receipt_preserves_ordered_reasons() {
    let receipt = evaluate_native_family_release_capacity(measurement(0, 0, 8 * 1_024 * 1_024 + 1))
        .expect("blocked capacity receipt");
    assert_eq!(receipt.exit_code(), 3);
    assert_eq!(
        receipt.reasons,
        [
            "protected_floor_or_transaction_peak",
            "inode_floor",
            "runtime_package_ceiling",
        ]
    );
    assert_eq!(
        admit_ready_native_family_release_capacity(
            &receipt,
            HASH,
            "wind",
            "2026.10.09-1",
            MEASURED,
        ),
        Err(NativeFamilyReleaseCapacityError::Stale)
    );
}

/// reject a ready receipt after the exact fifteen-minute authority window.
#[test]
fn ready_receipt_expires_after_fifteen_minutes() {
    let receipt =
        evaluate_native_family_release_capacity(measurement(10_000_000_000, 100_000, 0)).unwrap();
    assert_eq!(
        admit_ready_native_family_release_capacity(
            &receipt,
            HASH,
            "wind",
            "2026.10.09-1",
            "2026-10-08T01:17:03.005Z",
        ),
        Err(NativeFamilyReleaseCapacityError::Stale)
    );
}

/// prove streamed tar allocation and identities equal the original layer owner.
#[test]
fn streamed_tar_measurement_matches_source_owner() {
    let tar = tar_fixture();
    let native = measure_tar_stream(&mut Cursor::new(&tar)).expect("native tar measurement");
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(&tar).unwrap();
    let compressed = encoder.finish().unwrap();
    let blob = sha256_digest(&compressed);
    let diff = sha256_digest(&tar);
    let source = source_layer_measurement(&compressed, &blob, &diff);
    assert_eq!(native.allocated_bytes, 12_288);
    assert_eq!(native.entry_inodes, 2);
    assert_eq!(native.digest, diff);
    assert_eq!(source, format!("12_288 2 {blob} {diff}").replace('_', ""));
}

/// prove direct registry headers leave every layer body byte on the stream.
#[test]
fn direct_registry_headers_preserve_stream_body() {
    let response = b"HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\n\r\nbody";
    let mut reader = BufReader::new(Cursor::new(response));
    let headers = read_curl_response_headers(&mut reader).expect("direct response headers");
    let mut body = Vec::new();
    reader.read_to_end(&mut body).unwrap();
    assert_eq!(headers.status, 200);
    assert_eq!(headers.location, None);
    assert_eq!(body, b"body");
}

/// build one complete portable ustar layer with one nested regular file.
fn tar_fixture() -> Vec<u8> {
    let mut header = [0_u8; 512];
    header[..16].copy_from_slice(b"nested/value.txt");
    header[100..108].copy_from_slice(b"0000644\0");
    header[108..116].copy_from_slice(b"0000000\0");
    header[116..124].copy_from_slice(b"0000000\0");
    header[124..136].copy_from_slice(b"00000000005\0");
    header[136..148].copy_from_slice(b"00000000000\0");
    header[148..156].fill(b' ');
    header[156] = b'0';
    header[257..263].copy_from_slice(b"ustar\0");
    header[263..265].copy_from_slice(b"00");
    let checksum = header.iter().map(|byte| u64::from(*byte)).sum::<u64>();
    header[148..156].copy_from_slice(format!("{checksum:06o}\0 ").as_bytes());
    let mut tar = header.to_vec();
    tar.extend_from_slice(b"hello");
    tar.resize(1_024, 0);
    tar.resize(2_048, 0);
    tar
}

/// run the original streaming owner over the identical gzip and diff identity.
fn source_layer_measurement(compressed: &[u8], blob: &str, diff: &str) -> String {
    let script = r#"
import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';
import { measureReleaseLayer } from './deploy/scripts/adjustment-evaluation-package.mjs';
const bytes=readFileSync(0);
const blob=process.argv[1];
const diff=process.argv[2];
const measured=await measureReleaseLayer(new Response(bytes),{digest:blob,size:bytes.length},diff);
process.stdout.write(`${measured.allocatedBytes} ${measured.entryInodes} ${measured.blobDigest} ${diff}`);
"#;
    let mut child = Command::new("node")
        .args(["--input-type=module", "--eval", script, blob, diff])
        .current_dir(env!("CARGO_MANIFEST_DIR").trim_end_matches("/rust/crates/weather-runtime"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start source layer owner");
    child.stdin.as_mut().unwrap().write_all(compressed).unwrap();
    drop(child.stdin.take());
    let output = child.wait_with_output().expect("read source layer owner");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

/// build one six-role normalized measurement with two locally retained chains.
fn measurement(
    free_bytes: u64,
    free_inodes: u64,
    runtime_package_bytes: u64,
) -> NativeFamilyReleaseCapacityMeasurement {
    let roles = [
        "source",
        "source",
        "target",
        "target",
        "compensating",
        "compensating",
    ];
    let runtimes = ["server", "web", "server", "web", "server", "web"];
    let digests = ['1', '2', '3', '4', '5', '6'];
    let images = roles
        .into_iter()
        .zip(runtimes)
        .zip(digests)
        .map(|((role, runtime), digit)| {
            let digest = format!("sha256:{}", digit.to_string().repeat(64));
            NativeFamilyReleaseCapacityImage {
                digest: digest.clone(),
                layers: vec![NativeFamilyReleaseCapacityLayer {
                    blob_digest: digest.clone(),
                    chain_id: digest.clone(),
                    compressed_bytes: 1_000,
                    diff_id: digest,
                    entry_inodes: 2,
                    unpacked_bytes: 4_096,
                }],
                metadata_bytes: 8_192,
                role,
                runtime,
            }
        })
        .collect::<Vec<_>>();
    NativeFamilyReleaseCapacityMeasurement {
        action_sha256: HASH.to_owned(),
        family: "wind".to_owned(),
        free_bytes,
        free_inodes,
        inventory: images[..2]
            .iter()
            .map(|image| NativeFamilyReleaseLocalLayer {
                allocated_bytes: 4_096,
                chain_id: image.layers[0].chain_id.clone(),
            })
            .collect(),
        images,
        measured_at: MEASURED.to_owned(),
        retained_control_bytes: 4_096,
        runtime_package_bytes,
        source_release: "2026.10.09-1".to_owned(),
    }
}

/// build one web-only dashboard measurement over a literal retained core.
fn dashboard_measurement(
    free_bytes: u64,
    free_inodes: u64,
) -> NativeFamilyReleaseCapacityMeasurement {
    let mut input = measurement(free_bytes, free_inodes, 0);
    input.action_sha256 = HASH.to_owned();
    input.family.clear();
    input.source_release = "2026.11.25-1".to_owned();
    let source_server = input.images[0].clone();
    let source_web = input.images[1].clone();
    input.images[2] = NativeFamilyReleaseCapacityImage {
        role: "target",
        ..source_server.clone()
    };
    input.images[4] = NativeFamilyReleaseCapacityImage {
        role: "compensating",
        ..source_server
    };
    input.images[5] = NativeFamilyReleaseCapacityImage {
        role: "compensating",
        ..source_web
    };
    input
}

/// run the original exported formula over the same genuine typed measurement.
fn source_receipt(free_bytes: u64, free_inodes: u64, runtime_package_bytes: u64) -> Vec<u8> {
    let script = r#"
import { evaluateAdjustmentReleaseCapacity } from './deploy/scripts/adjustment-evaluation-package.mjs';
import { readFileSync } from 'node:fs';
const [freeBytes, freeInodes, runtimePackageBytes] = JSON.parse(readFileSync(0, 'utf8'));
const roles=['source','source','target','target','compensating','compensating'];
const runtimes=['server','web','server','web','server','web'];
const digits=['1','2','3','4','5','6'];
const images=roles.map((role,index)=>{const digest=`sha256:${digits[index].repeat(64)}`;return {digest,layers:[{blobDigest:digest,chainId:digest,compressedBytes:1000,diffId:digest,entryInodes:2,unpackedBytes:4096}],metadataBytes:8192,role,runtime:runtimes[index]};});
const base=evaluateAdjustmentReleaseCapacity({engineMetadataBytes:67108864,freeBytes,freeInodes,futureStateBytes:1048576,images,inventory:images.slice(0,2).map(image=>({allocatedBytes:4096,chainId:image.layers[0].chainId})),maximumOwnedBytes:4294967296,measuredAt:'2026-10-08T01:02:03.004Z',nextStateInodes:1004,pullScratchBytes:67108864,retainedControlBytes:4096,runtimePackageBytes});
const value={...base,actionSha256:'a'.repeat(64),family:'wind',compensationScope:'family-only-new-release-compensation',contractVersion:'adjustment-family-release-capacity/v1',sourceRelease:'2026.10.09-1'};
const canonical=value=>Array.isArray(value)?value.map(canonical):value&&typeof value==='object'?Object.fromEntries(Object.keys(value).sort().map(key=>[key,canonical(value[key])])):value;
process.stdout.write(`${JSON.stringify(canonical(value))}\n`);
"#;
    let mut child = Command::new("node")
        .args(["--input-type=module", "--eval", script])
        .current_dir(env!("CARGO_MANIFEST_DIR").trim_end_matches("/rust/crates/weather-runtime"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("start source evaluator");
    write!(
        child.stdin.as_mut().unwrap(),
        "[{free_bytes},{free_inodes},{runtime_package_bytes}]"
    )
    .unwrap();
    drop(child.stdin.take());
    let output = child.wait_with_output().expect("read source evaluator");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}

/// run the predecessor dashboard formula with the same actual-core normalized measurement.
fn source_dashboard_receipt(free_bytes: u64, free_inodes: u64) -> Vec<u8> {
    let script = r#"
import { evaluateAdjustmentReleaseCapacity } from './deploy/scripts/adjustment-evaluation-package.mjs';
import { readFileSync } from 'node:fs';
const [freeBytes, freeInodes] = JSON.parse(readFileSync(0, 'utf8'));
const roles=['source','source','target','target','compensating','compensating'];
const runtimes=['server','web','server','web','server','web'];
const digits=['1','2','1','4','1','2'];
const images=roles.map((role,index)=>{const digest=`sha256:${digits[index].repeat(64)}`;return {digest,layers:[{blobDigest:digest,chainId:digest,compressedBytes:1000,diffId:digest,entryInodes:2,unpackedBytes:4096}],metadataBytes:8192,role,runtime:runtimes[index]};});
const base=evaluateAdjustmentReleaseCapacity({engineMetadataBytes:67108864,freeBytes,freeInodes,futureStateBytes:1048576,images,inventory:images.slice(0,2).map(image=>({allocatedBytes:4096,chainId:image.layers[0].chainId})),maximumOwnedBytes:4294967296,measuredAt:'2026-10-08T01:02:03.004Z',nextStateInodes:1004,pullScratchBytes:67108864,retainedControlBytes:4096,runtimePackageBytes:0});
const value={...base,compensationScope:'fixed-core-web-only-source-restore',contractVersion:'dashboard-web-release-capacity/v1',publicationSha256:'a'.repeat(64),sourceRelease:'2026.11.25-1'};
const canonical=value=>Array.isArray(value)?value.map(canonical):value&&typeof value==='object'?Object.fromEntries(Object.keys(value).sort().map(key=>[key,canonical(value[key])])):value;
process.stdout.write(`${JSON.stringify(canonical(value))}\n`);
"#;
    let mut child = Command::new("node")
        .args(["--input-type=module", "--eval", script])
        .current_dir(env!("CARGO_MANIFEST_DIR").trim_end_matches("/rust/crates/weather-runtime"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("start source dashboard evaluator");
    write!(
        child.stdin.as_mut().unwrap(),
        "[{free_bytes},{free_inodes}]"
    )
    .unwrap();
    drop(child.stdin.take());
    let output = child
        .wait_with_output()
        .expect("read source dashboard evaluator");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}
