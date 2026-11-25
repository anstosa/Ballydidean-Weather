//! Complete native graph membership verification over the retained immutable plaintext objects.

use crate::{
    NativeMaintenanceCasInventory, NativeMaintenanceCasStreamValidator,
    NativeMaintenanceGraphEntry, NativeMaintenanceGraphManifest,
    inspect_native_maintenance_cas_stream,
    native_maintenance_graph_manifest::NATIVE_GRAPH_MANIFEST_MAXIMUM_BYTES,
    native_maintenance_retirement_files_v2::format_digest, read_native_maintenance_graph_manifest,
};
use std::{
    collections::{HashMap, HashSet},
    io::{Read, Seek, SeekFrom},
};

const MAX_GRAPH_CHAIN: usize = 16_744;
const GRAPH_KIND: &str = "adjustment-graph-manifest/v2";

/// Explicit content-addressed readonly store port for full graph custody.
pub trait NativeMaintenanceFullGraphObjects {
    type Reader: Read + Seek;

    /// Open exactly the immutable object represented by this hash, with its measured size.
    fn open_object(&mut self, digest: [u8; 32]) -> Option<(Self::Reader, u64)>;
}

/// Verified head graph and the actual bounded predecessor population.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeMaintenanceVerifiedFullGraph {
    pub manifest: NativeMaintenanceGraphManifest,
    pub manifest_object_sha256: [u8; 32],
    pub object_count: usize,
    pub member_count: usize,
    pub predecessor_count: usize,
}

/// Verify every exact member in the identified graph and optionally its entire prior chain.
pub fn verify_native_maintenance_full_graph(
    store: &mut impl NativeMaintenanceFullGraphObjects,
    head_digest: [u8; 32],
    validators: &mut impl NativeMaintenanceCasStreamValidator,
    verify_predecessors: bool,
) -> Option<NativeMaintenanceVerifiedFullGraph> {
    let mut visited = HashSet::new();
    let mut current = Some(head_digest);
    let mut head = None;
    // walk actual immutable predecessor objects rather than inventing a retained frontier
    while let Some(digest) = current {
        // retain the source cycle and maximum archive object count
        if visited.len() >= MAX_GRAPH_CHAIN || !visited.insert(digest) {
            return None;
        }
        let manifest = read_graph_object(store, digest, validators)?;
        let mut objects: HashMap<[u8; 32], Vec<&NativeMaintenanceGraphEntry>> = HashMap::new();
        // group exact portable entries by their admitted content-addressed object
        for entry in &manifest.entries {
            objects
                .entry(parse_digest(&entry.object_sha256)?)
                .or_default()
                .push(entry);
        }
        // compare the entire actual member inventory for each declared object
        for (object_digest, entries) in &objects {
            let inventory = read_inventory(store, *object_digest, validators)?;
            if !matches_entries(&inventory, entries) {
                return None;
            }
        }
        let predecessor = if verify_predecessors {
            match &manifest.predecessor_graph_sha256 {
                Some(value) => Some(parse_digest(value)?),
                None => None,
            }
        } else {
            None
        };
        let object_count = objects.len();
        drop(objects);
        // retain only the head graph while verifying every actual predecessor
        if head.is_none() {
            let member_count = manifest.entries.len();
            head = Some(NativeMaintenanceVerifiedFullGraph {
                manifest,
                manifest_object_sha256: digest,
                object_count,
                member_count,
                predecessor_count: 0,
            });
        }
        current = predecessor;
    }
    let mut result = head?;
    result.predecessor_count = visited.len().checked_sub(1)?;
    Some(result)
}

/// Stream and bind one exact declared object to its expected content address.
fn read_inventory(
    store: &mut impl NativeMaintenanceFullGraphObjects,
    digest: [u8; 32],
    validators: &mut impl NativeMaintenanceCasStreamValidator,
) -> Option<NativeMaintenanceCasInventory> {
    let (reader, size) = store.open_object(digest)?;
    let inventory = inspect_native_maintenance_cas_stream(reader, size, validators)?;
    (inventory.object_sha256 == digest).then_some(inventory)
}

/// Read the exact validated single graph member while retaining its file handle.
fn read_graph_object(
    store: &mut impl NativeMaintenanceFullGraphObjects,
    digest: [u8; 32],
    validators: &mut impl NativeMaintenanceCasStreamValidator,
) -> Option<NativeMaintenanceGraphManifest> {
    let (mut reader, size) = store.open_object(digest)?;
    let inventory = inspect_native_maintenance_cas_stream(&mut reader, size, validators)?;
    let [entry] = inventory.members.as_slice() else {
        return None;
    };
    // reject hidden companion members, wrong kind or different immutable addresses
    if inventory.object_sha256 != digest
        || entry.kind != GRAPH_KIND
        || entry.member_length > NATIVE_GRAPH_MANIFEST_MAXIMUM_BYTES as u64
    {
        return None;
    }
    let mut bytes = vec![0; usize::try_from(entry.member_length).ok()?];
    reader.seek(SeekFrom::Start(entry.member_offset)).ok()?;
    reader.read_exact(&mut bytes).ok()?;
    // rebind the reread to the previously verified graph member hash
    let actual =
        weather_edge::RevisionArchiveCanonicalCodec::sha256(&crate::NativeWirePrimitives, &bytes);
    if actual != entry.member_sha256 {
        return None;
    }
    read_native_maintenance_graph_manifest(&bytes)
}

/// Require no missing, extra, reordered or substituted source member metadata.
fn matches_entries(
    inventory: &NativeMaintenanceCasInventory,
    expected: &[&NativeMaintenanceGraphEntry],
) -> bool {
    if inventory.members.len() != expected.len() {
        return false;
    }
    let object = format_digest(inventory.object_sha256);
    // original graph order sorts kind then globally unique identity inside each object
    inventory
        .members
        .iter()
        .zip(expected)
        .all(|(member, entry)| {
            member.kind == entry.kind
                && format_digest(member.identity_sha256) == entry.identity_sha256
                && object == entry.object_sha256
                && member.member_offset == entry.member_offset
                && member.member_length == entry.member_length
                && format_digest(member.member_sha256) == entry.member_sha256
        })
}

/// Parse a source content address without allocating a hex-decoding crate.
fn parse_digest(value: &str) -> Option<[u8; 32]> {
    Some(*weather_edge::RevisionGapSha256::from_lower_hex(value)?.as_bytes())
}

#[cfg(test)]
#[path = "native_maintenance_full_graph_test.rs"]
mod tests;
