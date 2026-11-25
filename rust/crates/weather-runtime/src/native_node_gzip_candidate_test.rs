use super::*;
use crate::NativeEvaluationStreamingCodec;
use std::{
    io::Write,
    process::{Command, Stdio},
};

/// Obtain the actual pinned predecessor's level-nine gzip bytes for one input.
fn node_gzip(bytes: &[u8]) -> Vec<u8> {
    let mut child = Command::new("node")
        .args(["-e", "if (process.version !== 'v24.16.0' || process.versions.zlib !== '1.3.1-e00f703') process.exit(2); process.stdout.write(require('node:zlib').gzipSync(require('node:fs').readFileSync(0), {level:9}))"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn().expect("start pinned predecessor compressor");
    child.stdin.take().unwrap().write_all(bytes).unwrap();
    let result = child.wait_with_output().expect("read predecessor gzip");
    assert!(
        result.status.success(),
        "pinned predecessor compressor rejected input"
    );
    result.stdout
}

/// Exercise actual candidate output across the retained data classes and block/window boundaries.
#[test]
fn native_node_gzip_candidate_matches_predecessor_bytes() {
    let codec = NativeEvaluationStreamingCodec;
    let mut rows = String::from("[");
    // generate representative ten-day weather observations in source field order
    for hour in 0..240 {
        use std::fmt::Write as _;
        if hour != 0 {
            rows.push(',');
        }
        write!(
            rows,
            r#"{{"hour":{hour},"rainMm":{},"temperatureC":{},"windSpeedMps":{}}}"#,
            hour % 7,
            11 + hour % 13,
            3 + hour % 5
        )
        .unwrap();
    }
    rows.push(']');
    let base =
        br#"{"date":"2026-10-08T00:35:00.000Z","temperatureC":18.6,"rainMm":0,"source":"ecmwf"}"#;
    let mut vectors = vec![
        Vec::new(),
        b"a".to_vec(),
        b"abc".to_vec(),
        (0..=255).collect::<Vec<u8>>(),
        vec![b'a'; 65536],
        rows.as_bytes().to_vec(),
        base.repeat(50_000),
    ];
    let mut random = 0x7472_6163u32;
    // compare incompressible, repeated and mixed weather JSON around each meaningful boundary
    for size in [
        255, 1023, 16383, 16384, 16385, 32767, 32768, 32769, 65535, 65536, 65537, 262144, 524288,
    ] {
        let mut mixed = Vec::with_capacity(size);
        let mut entropy = Vec::with_capacity(size);
        // preserve a deterministic high-entropy source shared across architectures
        for index in 0..size {
            random = random.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let byte = (random >> 24) as u8;
            entropy.push(byte);
            mixed.push(if index % 512 < 384 {
                base[index % base.len()]
            } else {
                byte
            });
        }
        vectors.extend([
            entropy,
            base.iter().copied().cycle().take(size).collect(),
            mixed,
        ]);
    }
    // prove byte identity, not merely valid decompression
    for (index, bytes) in vectors.iter().enumerate() {
        let actual = native_node_gzip_candidate(bytes).expect("compress candidate vector");
        let expected = node_gzip(bytes);
        assert_eq!(actual, expected, "byte mismatch in vector {index}");
        assert_eq!(
            codec.gunzip_bounded(&actual, bytes.len()).as_deref(),
            Some(bytes.as_slice())
        );
    }
    assert_eq!(vectors.len(), 46);
}

/// Confirm the incremental stream preserves the pinned Node bytes across arbitrary input chunks.
#[test]
fn native_node_gzip_incremental_matches_predecessor() {
    let data = (0..190_000usize)
        .map(|index| ((index * 13 + index / 4093) % 251) as u8)
        .collect::<Vec<_>>();
    let expected = node_gzip(&data);
    // compare both tiny and output-buffer-sized writes without deflate flushes
    for size in [1, 127, 32_768, 90_000] {
        let mut writer = NativeNodeGzipWriter::new(Vec::new()).unwrap();
        for chunk in data.chunks(size) {
            writer.write_all(chunk).unwrap();
        }
        assert_eq!(writer.finish().unwrap(), expected);
    }
    let empty = NativeNodeGzipWriter::new(Vec::new()).unwrap();
    assert_eq!(empty.finish().unwrap(), node_gzip(&[]));
}
