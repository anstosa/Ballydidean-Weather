//! Native exact plaintext CAS pack encoding and streamed member inventory.

use crate::native_maintenance_graph_manifest::{
    NATIVE_GRAPH_MANIFEST_MAXIMUM_BYTES, read_native_maintenance_graph_manifest,
    valid_native_cas_kind,
};
use sha2::{Digest, Sha256};
use std::collections::HashSet;

const MAGIC: &[u8; 8] = b"WXACAS01";
const MAX_MEMBERS: usize = 65_536;
const MAXIMUM_SAFE_INTEGER: u64 = 9_007_199_254_740_991;
const STREAM_BUFFER: usize = 256 * 1024;

/// One archive member input whose domain payload must be explicitly validated.
pub struct NativeMaintenanceCasMember<'a> {
    pub kind: &'a str,
    pub identity_sha256: [u8; 32],
    pub payload: &'a [u8],
}

/// Exact source CAS member inventory of one validated immutable object.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeMaintenanceCasMemberInventory {
    pub kind: String,
    pub identity_sha256: [u8; 32],
    pub member_length: u64,
    pub member_offset: u64,
    pub member_sha256: [u8; 32],
}

/// The original readonly CAS object and its exact portable member addresses.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeMaintenanceCasInventory {
    pub byte_length: u64,
    pub object_sha256: [u8; 32],
    pub members: Vec<NativeMaintenanceCasMemberInventory>,
}

/// Required registered source domain validator for non-graph member types.
pub trait NativeMaintenanceCasMemberValidator {
    /// Validate the exact member against its closed registered payload schema.
    fn validate(&mut self, kind: &str, payload: &[u8]) -> bool;
}

/// Actual independently bounded stream validator for the original CAS reader.
pub trait NativeMaintenanceCasStreamValidator {
    type Session: NativeMaintenanceCasStreamSession;

    /// Open the registered member kind with its advertised finite schema ceiling.
    fn begin(&mut self, kind: &str, length: u64) -> Option<Self::Session>;
}

/// One source domain validation session that never needs the whole object retained.
pub trait NativeMaintenanceCasStreamSession {
    /// Consume one bounded raw member chunk.
    fn write(&mut self, bytes: &[u8]) -> bool;
    /// Finish only after the entire member was validated.
    fn finish(self) -> bool;
}

/// Encode exact source binary framing after validating every member and global identity.
pub fn encode_native_maintenance_cas_pack(
    mut members: Vec<NativeMaintenanceCasMember<'_>>,
    validators: &mut impl NativeMaintenanceCasMemberValidator,
) -> Option<Vec<u8>> {
    // preserve the source nonempty and finite member count
    if members.is_empty() || members.len() > MAX_MEMBERS {
        return None;
    }
    members.sort_unstable_by(|a, b| (a.kind, a.identity_sha256).cmp(&(b.kind, b.identity_sha256)));
    let mut identities = HashSet::with_capacity(members.len());
    let mut size = 16usize;
    // admit every original typed member before producing any candidate bytes
    for member in &members {
        if !valid_native_cas_kind(member.kind)
            || !identities.insert(member.identity_sha256)
            || u64::try_from(member.payload.len()).ok()? > MAXIMUM_SAFE_INTEGER
            || !validate_member(validators, member.kind, member.payload)
        {
            return None;
        }
        size = size
            .checked_add(42)?
            .checked_add(member.kind.len())?
            .checked_add(member.payload.len())?;
    }
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(size).ok()?;
    bytes.extend_from_slice(MAGIC);
    bytes.extend_from_slice(&u32::try_from(members.len()).ok()?.to_be_bytes());
    bytes.extend_from_slice(&[0; 4]);
    // emit each source member length, kind, binary identity and payload in canonical order
    for member in members {
        bytes.extend_from_slice(&u16::try_from(member.kind.len()).ok()?.to_be_bytes());
        bytes.extend_from_slice(member.kind.as_bytes());
        bytes.extend_from_slice(&member.identity_sha256);
        bytes.extend_from_slice(&u64::try_from(member.payload.len()).ok()?.to_be_bytes());
        bytes.extend_from_slice(member.payload);
    }
    Some(bytes)
}

/// Inspect one source-admitted buffer without copying its individual payloads.
pub fn inspect_native_maintenance_cas_pack(
    bytes: &[u8],
    validators: &mut impl NativeMaintenanceCasMemberValidator,
) -> Option<NativeMaintenanceCasInventory> {
    let mut reader = std::io::Cursor::new(bytes);
    let count = read_header(&mut reader)?;
    let mut members = Vec::with_capacity(count);
    let mut identities = HashSet::with_capacity(count);
    let mut previous: Option<(String, [u8; 32])> = None;
    // validate every source member header and direct borrowed buffer range
    for _ in 0..count {
        let (kind, identity, length) = read_member_header(&mut reader)?;
        admit_member_order(&mut previous, &mut identities, &kind, identity)?;
        let offset = reader.position();
        let end = offset.checked_add(length)?;
        let payload = bytes.get(usize::try_from(offset).ok()?..usize::try_from(end).ok()?)?;
        // refuse every undisclosed or invalid member schema
        if !validate_member(validators, &kind, payload) {
            return None;
        }
        reader.set_position(end);
        members.push(NativeMaintenanceCasMemberInventory {
            kind,
            identity_sha256: identity,
            member_length: length,
            member_offset: offset,
            member_sha256: Sha256::digest(payload).into(),
        });
    }
    // reject every byte not declared by the original member count
    if reader.position() != u64::try_from(bytes.len()).ok()? {
        return None;
    }
    Some(NativeMaintenanceCasInventory {
        byte_length: reader.position(),
        object_sha256: Sha256::digest(bytes).into(),
        members,
    })
}

/// Inspect a potentially large retained object through bounded source member chunks.
pub fn inspect_native_maintenance_cas_stream(
    source: impl std::io::Read,
    size: u64,
    validators: &mut impl NativeMaintenanceCasStreamValidator,
) -> Option<NativeMaintenanceCasInventory> {
    let mut reader = HashedReader {
        inner: source,
        hash: Sha256::new(),
        offset: 0,
    };
    let count = read_header(&mut reader)?;
    let mut members = Vec::with_capacity(count);
    let mut identities = HashSet::with_capacity(count);
    let mut previous: Option<(String, [u8; 32])> = None;
    let mut buffer = vec![0u8; STREAM_BUFFER];
    // visit every source member without collecting the entire object
    for _ in 0..count {
        let (kind, identity, length) = read_member_header(&mut reader)?;
        admit_member_order(&mut previous, &mut identities, &kind, identity)?;
        let offset = reader.offset;
        // refuse short objects before driving an explicit domain validator
        if offset.checked_add(length)? > size {
            return None;
        }
        let mut validator = if kind == "adjustment-graph-manifest/v2" {
            // retain the source's special bounded graph validator independently of caller validators
            if length > NATIVE_GRAPH_MANIFEST_MAXIMUM_BYTES as u64 {
                return None;
            }
            StreamValidation::Graph(Vec::with_capacity(usize::try_from(length).ok()?))
        } else {
            StreamValidation::Domain(validators.begin(&kind, length)?)
        };
        let mut hash = Sha256::new();
        let mut remaining = length;
        // stream bounded chunks and the exact payload hash together
        while remaining > 0 {
            let count = usize::try_from(remaining.min(STREAM_BUFFER as u64)).ok()?;
            std::io::Read::read_exact(&mut reader, &mut buffer[..count]).ok()?;
            hash.update(&buffer[..count]);
            match &mut validator {
                StreamValidation::Graph(bytes) => bytes.extend_from_slice(&buffer[..count]),
                StreamValidation::Domain(session) => {
                    if !session.write(&buffer[..count]) {
                        return None;
                    }
                }
            }
            remaining -= count as u64;
        }
        let admitted = match validator {
            StreamValidation::Graph(bytes) => {
                read_native_maintenance_graph_manifest(&bytes).is_some()
            }
            StreamValidation::Domain(session) => session.finish(),
        };
        if !admitted {
            return None;
        }
        members.push(NativeMaintenanceCasMemberInventory {
            kind,
            identity_sha256: identity,
            member_length: length,
            member_offset: offset,
            member_sha256: hash.finalize().into(),
        });
    }
    // require the source object size and actual EOF rather than silently dropping trailing bytes
    if reader.offset != size || std::io::Read::read(&mut reader, &mut [0u8; 1]).ok()? != 0 {
        return None;
    }
    Some(NativeMaintenanceCasInventory {
        byte_length: reader.offset,
        object_sha256: reader.hash.finalize().into(),
        members,
    })
}

enum StreamValidation<Session> {
    Graph(Vec<u8>),
    Domain(Session),
}

struct HashedReader<R> {
    inner: R,
    hash: Sha256,
    offset: u64,
}
impl<R: std::io::Read> std::io::Read for HashedReader<R> {
    /// Hash exactly the retained bytes consumed by the bounded parser.
    fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
        let count = self.inner.read(bytes)?;
        self.hash.update(&bytes[..count]);
        self.offset = self
            .offset
            .checked_add(count as u64)
            .ok_or_else(|| std::io::Error::other("CAS size overflow"))?;
        Ok(count)
    }
}

/// Validate the built-in source graph or the exact explicitly registered member.
fn validate_member(
    validators: &mut impl NativeMaintenanceCasMemberValidator,
    kind: &str,
    payload: &[u8],
) -> bool {
    if kind == "adjustment-graph-manifest/v2" {
        read_native_maintenance_graph_manifest(payload).is_some()
    } else {
        validators.validate(kind, payload)
    }
}

/// Decode exact magic, finite member count and reserved header zeros.
fn read_header(reader: &mut impl std::io::Read) -> Option<usize> {
    let mut header = [0u8; 16];
    reader.read_exact(&mut header).ok()?;
    let count = u32::from_be_bytes(header[8..12].try_into().ok()?) as usize;
    (header[..8] == *MAGIC && header[12..] == [0; 4] && (1..=MAX_MEMBERS).contains(&count))
        .then_some(count)
}

/// Read one complete canonical kind, identity and safe source member length.
fn read_member_header(reader: &mut impl std::io::Read) -> Option<(String, [u8; 32], u64)> {
    let mut length = [0u8; 2];
    reader.read_exact(&mut length).ok()?;
    let length = u16::from_be_bytes(length) as usize;
    if !(1..=255).contains(&length) {
        return None;
    }
    let mut kind = [0u8; 255];
    reader.read_exact(&mut kind[..length]).ok()?;
    let kind = std::str::from_utf8(&kind[..length]).ok()?;
    if !valid_native_cas_kind(kind) {
        return None;
    }
    let mut identity = [0u8; 32];
    reader.read_exact(&mut identity).ok()?;
    let mut member = [0u8; 8];
    reader.read_exact(&mut member).ok()?;
    let member = u64::from_be_bytes(member);
    (member <= MAXIMUM_SAFE_INTEGER).then(|| (kind.to_owned(), identity, member))
}

/// Admit only the original strict member order and globally unique identities.
fn admit_member_order(
    previous: &mut Option<(String, [u8; 32])>,
    identities: &mut HashSet<[u8; 32]>,
    kind: &str,
    identity: [u8; 32],
) -> Option<()> {
    if previous
        .as_ref()
        .is_some_and(|(prior, id)| (prior.as_str(), *id) >= (kind, identity))
        || !identities.insert(identity)
    {
        return None;
    }
    *previous = Some((kind.to_owned(), identity));
    Some(())
}

#[cfg(test)]
#[path = "native_maintenance_cas_pack_test.rs"]
mod tests;
