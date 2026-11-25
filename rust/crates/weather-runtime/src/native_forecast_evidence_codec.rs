//! Concrete compression and streaming digest owners for retained forecast evidence.

use std::io::{self, Read};

use flate2::read::MultiGzDecoder;
use sha2::{Digest, Sha256};
use weather_database::Sha256Digest;

use crate::{AdjustmentEvaluationMemberDigest, AdjustmentEvaluationStreamingValues};

/// Native bounded decoding and incremental digest for retained gzip members.
// compression is not selected until it reproduces Node's exact retained bytes
#[derive(Clone, Copy, Default)]
pub struct NativeEvaluationStreamingCodec;

impl NativeEvaluationStreamingCodec {
    /// Read no more than one byte past the contract's output limit.
    pub fn gunzip_bounded(&self, bytes: &[u8], maximum_bytes: usize) -> Option<Vec<u8>> {
        let limit = u64::try_from(maximum_bytes).ok()?.checked_add(1)?;
        let mut decoded = Vec::new();
        MultiGzDecoder::new(bytes)
            .take(limit)
            .read_to_end(&mut decoded)
            .ok()?;
        (decoded.len() <= maximum_bytes).then_some(decoded)
    }
}

/// An incremental SHA-256 state owned by one opened evaluation member.
pub struct NativeEvaluationMemberDigest(Sha256);

impl AdjustmentEvaluationMemberDigest for NativeEvaluationMemberDigest {
    /// Absorb only the next supplied chunk.
    fn update(&mut self, bytes: &[u8]) {
        self.0.update(bytes);
    }

    /// Complete the exact lowercase digest without retaining the source bytes.
    fn finish(self) -> Option<Sha256Digest> {
        use std::fmt::Write as _;
        let mut encoded = String::with_capacity(64);
        // render the fixed digest without another dependency
        for byte in self.0.finalize() {
            write!(&mut encoded, "{byte:02x}").ok()?;
        }
        Some(Sha256Digest::admitted(encoded))
    }
}

impl AdjustmentEvaluationStreamingValues for NativeEvaluationStreamingCodec {
    type Digest = NativeEvaluationMemberDigest;
    type Inflated<R: Read> = MultiGzDecoder<R>;

    /// Allocate one fixed-size digest state per open member.
    fn begin_sha256(&mut self) -> Self::Digest {
        NativeEvaluationMemberDigest(Sha256::new())
    }

    /// Decode concatenated members while pulling from the caller's streaming reader.
    fn gunzip_stream<R: Read>(&mut self, compressed: R) -> io::Result<Self::Inflated<R>> {
        Ok(MultiGzDecoder::new(compressed))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NativeWirePrimitives;
    use flate2::{Compression, GzBuilder};
    use std::{
        io::Write,
        process::{Command, Stdio},
    };
    use weather_edge::RevisionArchiveCanonicalCodec;

    /// Produce a valid native decoding fixture without claiming retained-byte identity.
    fn decoding_fixture_gzip(bytes: &[u8]) -> Vec<u8> {
        let mut encoder = GzBuilder::new()
            .mtime(0)
            .operating_system(3)
            .write(Vec::new(), Compression::best());
        encoder.write_all(bytes).unwrap();
        encoder.finish().unwrap()
    }

    /// Produce the predecessor's actual compressed bytes for one representative document.
    fn node_gzip(plaintext: &[u8]) -> Vec<u8> {
        let mut child = Command::new("node")
            .args([
                "-e",
                "process.stdout.write(require('node:zlib').gzipSync(require('node:fs').readFileSync(0), {level:9}))",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(plaintext).unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success());
        output.stdout
    }

    /// Compare the real streaming decoder and digest owner against predecessor gzip bytes.
    #[test]
    fn native_forecast_evidence_matches_node_decoding_and_streaming() {
        let codec = NativeEvaluationStreamingCodec;
        let plaintext = br#"{"forecast":[1,2,3],"source":"ecmwf"}"#;
        let compressed = node_gzip(plaintext);
        assert_eq!(
            codec
                .gunzip_bounded(&compressed, plaintext.len())
                .as_deref(),
            Some(plaintext.as_slice())
        );
        assert_eq!(codec.gunzip_bounded(&compressed, plaintext.len() - 1), None);

        let mut codec = codec;
        let mut stream = codec.gunzip_stream(compressed.as_slice()).unwrap();
        let mut digest = codec.begin_sha256();
        let mut buffer = [0u8; 7];
        // hash the same plaintext in small chunks without a retained aggregate
        loop {
            let count = stream.read(&mut buffer).unwrap();
            if count == 0 {
                break;
            }
            digest.update(&buffer[..count]);
        }
        let expected = RevisionArchiveCanonicalCodec::sha256(&NativeWirePrimitives, plaintext);
        let expected = expected
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        assert_eq!(digest.finish(), Some(Sha256Digest::admitted(expected)));
    }

    /// Exercise a multi-kilobyte document with the typical forecast row repetition.
    #[cfg(feature = "node-zlib-candidate")]
    #[test]
    fn native_forecast_rows_match_node_gzip_bytes() {
        let codec = NativeEvaluationStreamingCodec;
        let mut rows = String::from("[");
        // generate a finite ten-day weather payload using common field names
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
        let plaintext = rows.as_bytes();
        let compressed = crate::native_node_gzip_candidate(plaintext).unwrap();
        assert_eq!(compressed, node_gzip(plaintext));
        assert_eq!(
            codec
                .gunzip_bounded(&compressed, plaintext.len())
                .as_deref(),
            Some(plaintext)
        );
    }

    /// Exercise broad member decoding without selecting the compression candidate.
    #[test]
    fn native_forecast_gzip_decodes_node_and_native_members() {
        let codec = NativeEvaluationStreamingCodec;
        let maximum = 512 * 1024;
        let mut state = 0x7472_6163_u32;
        let mut mixed = Vec::with_capacity(maximum);
        let field_bytes = br#"date,temperatureC,rainMm,"source":"ecmwf""#;
        // vary the row fields without introducing external fixture allocations
        for index in 0..maximum {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let byte = if index % 512 < 384 {
                field_bytes[index % field_bytes.len()]
            } else {
                (state >> 24) as u8
            };
            mixed.push(byte);
        }
        // native decoding supports either valid member, without claiming compression identity
        for plaintext in [&mixed[..32 * 1024], &mixed[..]] {
            let expected = node_gzip(plaintext);
            let compressed = decoding_fixture_gzip(plaintext);
            assert_eq!(
                codec.gunzip_bounded(&expected, plaintext.len()).as_deref(),
                Some(plaintext)
            );
            assert_eq!(
                codec
                    .gunzip_bounded(&compressed, plaintext.len())
                    .as_deref(),
                Some(plaintext)
            );
        }
    }
}
