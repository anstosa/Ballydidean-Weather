//! Verified native current-graph restore into explicit fixed caller-owned output names.

use crate::{
    NativeMaintenanceCasStreamValidator, NativeMaintenanceFullGraphObjects,
    verify_native_maintenance_full_graph,
};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    io::{Read, Seek, SeekFrom},
};
const DEFAULT_MAXIMUM_BYTES: u64 = 4 * 1024 * 1024 * 1024;

/// Explicit admitted restore request for one already verified portable identity.
pub struct NativeMaintenanceGraphRestoreTarget<'a> {
    pub file_name: &'a str,
    pub identity_sha256: [u8; 32],
}

/// Explicit file-owner boundary for bounded verified plaintext restoration.
pub trait NativeMaintenanceGraphRestoreSink {
    /// Write one exact named regular file exclusively and consume the full admitted stream.
    fn write_exclusive(
        &mut self,
        name: &str,
        reader: &mut impl Read,
        expected_length: u64,
    ) -> Option<()>;
}

/// Exact source restored-file and logical-byte totals.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NativeMaintenanceGraphRestoreResult {
    pub restored_bytes: u64,
    pub restored_files: usize,
}

/// Restore only caller-selected identities from a fully verified current immutable graph.
pub fn restore_native_maintenance_full_graph(
    store: &mut impl NativeMaintenanceFullGraphObjects,
    head: [u8; 32],
    validators: &mut impl NativeMaintenanceCasStreamValidator,
    sink: &mut impl NativeMaintenanceGraphRestoreSink,
    targets: &[NativeMaintenanceGraphRestoreTarget<'_>],
    maximum_bytes: Option<u64>,
) -> Option<NativeMaintenanceGraphRestoreResult> {
    let verified = verify_native_maintenance_full_graph(store, head, validators, false)?;
    let mut entries = HashMap::with_capacity(verified.manifest.entries.len());
    // address only actual member identities from the verified current graph
    for entry in &verified.manifest.entries {
        let digest =
            *weather_edge::RevisionGapSha256::from_lower_hex(&entry.identity_sha256)?.as_bytes();
        entries.insert(digest, entry);
    }
    let mut names = HashSet::with_capacity(targets.len());
    let mut bytes = 0u64;
    // pre-admit every exact target name, graph identity and cumulative byte allowance
    for target in targets {
        if !valid_restore_name(target.file_name) || !names.insert(target.file_name) {
            return None;
        }
        let entry = entries.get(&target.identity_sha256)?;
        bytes = bytes.checked_add(entry.member_length)?;
        if bytes > maximum_bytes.unwrap_or(DEFAULT_MAXIMUM_BYTES) {
            return None;
        }
    }
    // stream each admitted range directly from its original readonly object
    for target in targets {
        let entry = entries.get(&target.identity_sha256)?;
        let digest =
            *weather_edge::RevisionGapSha256::from_lower_hex(&entry.object_sha256)?.as_bytes();
        let (mut reader, size) = store.open_object(digest)?;
        if entry.member_offset.checked_add(entry.member_length)? > size {
            return None;
        }
        reader.seek(SeekFrom::Start(entry.member_offset)).ok()?;
        let mut payload = VerifiedPayloadReader {
            inner: reader.take(entry.member_length),
            hasher: Sha256::new(),
            consumed: 0,
        };
        sink.write_exclusive(target.file_name, &mut payload, entry.member_length)?;
        let expected =
            *weather_edge::RevisionGapSha256::from_lower_hex(&entry.member_sha256)?.as_bytes();
        // require exact consumed bytes and original member identity through the output boundary
        let hash: [u8; 32] = payload.hasher.finalize().into();
        if payload.consumed != entry.member_length || hash != expected {
            return None;
        }
    }
    Some(NativeMaintenanceGraphRestoreResult {
        restored_bytes: bytes,
        restored_files: names.len(),
    })
}

struct VerifiedPayloadReader<R> {
    inner: R,
    hasher: Sha256,
    consumed: u64,
}
impl<R: Read> Read for VerifiedPayloadReader<R> {
    /// Retain only the exact streamed member digest and consumed byte count.
    fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
        let count = self.inner.read(bytes)?;
        self.hasher.update(&bytes[..count]);
        self.consumed = self
            .consumed
            .checked_add(count as u64)
            .ok_or_else(|| std::io::Error::other("restore length overflow"))?;
        Ok(count)
    }
}

/// Retain the predecessor's bounded literal basename grammar.
fn valid_restore_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    (1..=128).contains(&bytes.len())
        && bytes[0].is_ascii_alphanumeric()
        && bytes
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    /// Preserve the source's admitted fixed archive restore names.
    fn exact_restore_basename_grammar() {
        assert!(valid_restore_name("candidate-01.json"));
        assert!(valid_restore_name("artifact.parquet"));
        assert!(!valid_restore_name("subdir/artifact"));
        assert!(!valid_restore_name(""));
    }
}
