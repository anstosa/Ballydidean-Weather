//! Exact fixed-width plaintext archive catalog entry and shard wire format.

use sha2::{Digest, Sha256};

use crate::maintenance_canonical_json::valid_hash;
use crate::native_maintenance_graph_manifest::valid_native_cas_kind;

pub const NATIVE_MAINTENANCE_CATALOG_ENTRY_BYTES: usize = 96;
pub const NATIVE_MAINTENANCE_CATALOG_HEADER_BYTES: usize = 64;
pub const NATIVE_MAINTENANCE_MAXIMUM_CATALOG_ENTRIES: usize = 9_500_000;
pub const NATIVE_MAINTENANCE_MAXIMUM_CATALOG_SHARDS: u32 = 512;

const CATALOG_MAGIC: &[u8; 8] = b"WXACAT01";

/// One source catalog entry before its kind is replaced by a strong digest.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NativeMaintenanceCatalogEntry<'a> {
    pub identity_sha256: &'a str,
    pub kind: &'a str,
    pub member_sha256: &'a str,
}

/// One complete source shard request retaining canonical decimal generation input.
pub struct NativeMaintenanceCatalogShardInput<'a> {
    pub entries: &'a [NativeMaintenanceCatalogEntry<'a>],
    pub generation: &'a str,
    pub object_sha256: &'a str,
    pub shard_index: u32,
}

/// One decoded fixed-width strong metadata record.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeMaintenanceCatalogWireEntry {
    pub kind_sha256: String,
    pub identity_sha256: String,
    pub member_sha256: String,
}

/// Exact path-free catalog shard projection returned by the source inspector.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeMaintenanceCatalogShard {
    pub entry_count: u32,
    pub entries: Vec<NativeMaintenanceCatalogWireEntry>,
    pub generation: String,
    pub object_sha256: String,
    pub shard_index: u32,
}

/// Encode one source entry as kind-root, identity-root, and member-root bytes.
pub fn encode_native_maintenance_catalog_entry(
    entry: NativeMaintenanceCatalogEntry<'_>,
) -> Option<[u8; NATIVE_MAINTENANCE_CATALOG_ENTRY_BYTES]> {
    // retain the exact source kind and lowercase digest admission before hashing
    if !valid_native_cas_kind(entry.kind)
        || !valid_hash(entry.identity_sha256)
        || !valid_hash(entry.member_sha256)
    {
        return None;
    }
    let mut bytes = [0_u8; NATIVE_MAINTENANCE_CATALOG_ENTRY_BYTES];
    bytes[..32].copy_from_slice(&Sha256::digest(entry.kind.as_bytes()));
    bytes[32..64].copy_from_slice(&parse_digest(entry.identity_sha256)?);
    bytes[64..96].copy_from_slice(&parse_digest(entry.member_sha256)?);
    Some(bytes)
}

/// Encode one bounded shard after sorting its complete 96-byte entry records.
pub fn encode_native_maintenance_catalog_shard(
    input: NativeMaintenanceCatalogShardInput<'_>,
) -> Option<Vec<u8>> {
    let generation = parse_generation(input.generation)?;
    let object_sha256 = parse_digest(input.object_sha256)?;
    // preserve both source shard and aggregate entry ceilings
    if input.shard_index >= NATIVE_MAINTENANCE_MAXIMUM_CATALOG_SHARDS
        || input.entries.len() > NATIVE_MAINTENANCE_MAXIMUM_CATALOG_ENTRIES
    {
        return None;
    }
    let mut entries = Vec::new();
    entries.try_reserve_exact(input.entries.len()).ok()?;
    // encode each admitted record without an infallible aggregate allocation
    for entry in input.entries.iter().copied() {
        entries.push(encode_native_maintenance_catalog_entry(entry)?);
    }
    entries.sort_unstable();
    // reject only duplicate complete metadata records after canonical sorting
    if entries.windows(2).any(|pair| pair[0] == pair[1]) {
        return None;
    }
    let size = NATIVE_MAINTENANCE_CATALOG_HEADER_BYTES.checked_add(
        entries
            .len()
            .checked_mul(NATIVE_MAINTENANCE_CATALOG_ENTRY_BYTES)?,
    )?;
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(size).ok()?;
    bytes.extend_from_slice(CATALOG_MAGIC);
    bytes.extend_from_slice(&generation.to_be_bytes());
    bytes.extend_from_slice(&object_sha256);
    bytes.extend_from_slice(&input.shard_index.to_be_bytes());
    bytes.extend_from_slice(&u32::try_from(entries.len()).ok()?.to_be_bytes());
    bytes.extend_from_slice(&[0_u8; 8]);
    // append the already sorted fixed-width records without additional framing
    for entry in entries {
        bytes.extend_from_slice(&entry);
    }
    Some(bytes)
}

/// Inspect one exact shard while retaining the source's strict entry byte order.
pub fn inspect_native_maintenance_catalog_shard(
    bytes: &[u8],
) -> Option<NativeMaintenanceCatalogShard> {
    // reject a short, substituted, or nonzero-reserved header before count arithmetic
    if bytes.len() < NATIVE_MAINTENANCE_CATALOG_HEADER_BYTES
        || bytes.get(..8)? != CATALOG_MAGIC
        || bytes.get(56..64)? != [0_u8; 8]
    {
        return None;
    }
    let generation = u64::from_be_bytes(bytes.get(8..16)?.try_into().ok()?);
    let object_sha256 = format_digest(bytes.get(16..48)?.try_into().ok()?);
    let shard_index = u32::from_be_bytes(bytes.get(48..52)?.try_into().ok()?);
    let entry_count = u32::from_be_bytes(bytes.get(52..56)?.try_into().ok()?);
    let count = usize::try_from(entry_count).ok()?;
    let expected = NATIVE_MAINTENANCE_CATALOG_HEADER_BYTES
        .checked_add(count.checked_mul(NATIVE_MAINTENANCE_CATALOG_ENTRY_BYTES)?)?;
    // require the exact source count, partition, and no trailing byte
    if shard_index >= NATIVE_MAINTENANCE_MAXIMUM_CATALOG_SHARDS
        || count > NATIVE_MAINTENANCE_MAXIMUM_CATALOG_ENTRIES
        || bytes.len() != expected
    {
        return None;
    }
    let mut entries = Vec::new();
    entries.try_reserve_exact(count).ok()?;
    let mut previous: Option<&[u8]> = None;
    // decode each fixed-width record only after proving strict full-record order
    for index in 0..count {
        let start = NATIVE_MAINTENANCE_CATALOG_HEADER_BYTES
            .checked_add(index.checked_mul(NATIVE_MAINTENANCE_CATALOG_ENTRY_BYTES)?)?;
        let encoded = bytes.get(start..start + NATIVE_MAINTENANCE_CATALOG_ENTRY_BYTES)?;
        // reject duplicate or descending records before projecting their component roots
        if previous.is_some_and(|prior| prior >= encoded) {
            return None;
        }
        entries.push(NativeMaintenanceCatalogWireEntry {
            kind_sha256: format_digest(encoded.get(..32)?.try_into().ok()?),
            identity_sha256: format_digest(encoded.get(32..64)?.try_into().ok()?),
            member_sha256: format_digest(encoded.get(64..96)?.try_into().ok()?),
        });
        previous = Some(encoded);
    }
    Some(NativeMaintenanceCatalogShard {
        entry_count,
        entries,
        generation: generation.to_string(),
        object_sha256,
        shard_index,
    })
}

/// Parse the source's canonical unsigned 64-bit decimal generation string.
fn parse_generation(value: &str) -> Option<u64> {
    let bytes = value.as_bytes();
    // reject empty, leading-zero, nondecimal, and overflowing generation text
    if bytes.is_empty()
        || (bytes.len() > 1 && bytes.first() == Some(&b'0'))
        || !bytes.iter().all(u8::is_ascii_digit)
    {
        return None;
    }
    value.parse().ok()
}

/// Decode one already grammar-checked lowercase SHA-256 string.
fn parse_digest(value: &str) -> Option<[u8; 32]> {
    // retain the shared exact digest admission independently of nibble decoding
    if !valid_hash(value) {
        return None;
    }
    let mut digest = [0_u8; 32];
    // decode each exact hexadecimal pair without accepting prefixes or case changes
    for (index, byte) in digest.iter_mut().enumerate() {
        let offset = index * 2;
        *byte = (hex(value.as_bytes()[offset])? << 4) | hex(value.as_bytes()[offset + 1])?;
    }
    Some(digest)
}

/// Decode one lowercase hexadecimal nibble.
fn hex(byte: u8) -> Option<u8> {
    // admit only the source SHA-256 alphabet
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        _ => None,
    }
}

/// Render one binary digest using the source's lowercase hexadecimal spelling.
fn format_digest(digest: [u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut text = String::with_capacity(64);
    // append exactly two lowercase digits per digest byte
    for byte in digest {
        text.push(HEX[usize::from(byte >> 4)] as char);
        text.push(HEX[usize::from(byte & 0x0f)] as char);
    }
    text
}

#[cfg(test)]
#[path = "native_maintenance_catalog_wire_test.rs"]
mod tests;
