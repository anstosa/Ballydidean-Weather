//! Exact canonical path-free graph manifests used by the plaintext maintenance archive.

use crate::{
    MaintenanceCanonicalJson, NativeWirePrimitives,
    maintenance_canonical_json::{exact_keys, field, text, valid_hash},
};
use std::collections::{HashMap, HashSet};
use weather_contracts::JsonValue;

const VERSION: &str = "adjustment-graph-manifest/v2";
const MAX_ENTRIES: usize = 9_500_000;
const MAXIMUM_SAFE_INTEGER: u64 = 9_007_199_254_740_991;
pub(crate) const NATIVE_GRAPH_MANIFEST_MAXIMUM_BYTES: usize = 32 * 1024 * 1024;
const ENTRY_FIELDS: &[&str] = &[
    "kind",
    "identitySha256",
    "objectSha256",
    "memberOffset",
    "memberLength",
    "memberSha256",
];
const LINK_FIELDS: &[&str] = &["fromIdentitySha256", "relation", "toIdentitySha256"];
const MANIFEST_FIELDS: &[&str] = &[
    "contractVersion",
    "predecessorGraphSha256",
    "entries",
    "crossLinks",
    "graphRootSha256",
];

/// One explicitly located immutable CAS member.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeMaintenanceGraphEntry {
    pub kind: String,
    pub identity_sha256: String,
    pub object_sha256: String,
    pub member_offset: u64,
    pub member_length: u64,
    pub member_sha256: String,
}

/// One path-free semantic relation within the exact graph identity set.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeMaintenanceGraphCrossLink {
    pub from_identity_sha256: String,
    pub relation: String,
    pub to_identity_sha256: String,
}

/// One native graph projection with the exact predecessor hash.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeMaintenanceGraphManifest {
    pub predecessor_graph_sha256: Option<String>,
    pub entries: Vec<NativeMaintenanceGraphEntry>,
    pub cross_links: Vec<NativeMaintenanceGraphCrossLink>,
    pub graph_root_sha256: String,
}

impl NativeMaintenanceGraphManifest {
    /// Produce the exact sorted LF-framed retained graph JSON.
    pub fn canonical_bytes(&self) -> Option<Vec<u8>> {
        NativeWirePrimitives.encode_canonical_json(&object(vec![
            ("contractVersion", string(VERSION)),
            (
                "predecessorGraphSha256",
                nullable_hash(&self.predecessor_graph_sha256),
            ),
            (
                "entries",
                JsonValue::Array(self.entries.iter().map(entry_json).collect()),
            ),
            (
                "crossLinks",
                JsonValue::Array(self.cross_links.iter().map(link_json).collect()),
            ),
            ("graphRootSha256", string(&self.graph_root_sha256)),
        ]))
    }
}

/// Build the actual graph with deterministic ordering, closed ranges and complete links.
pub fn build_native_maintenance_graph_manifest(
    predecessor_graph_sha256: Option<String>,
    mut entries: Vec<NativeMaintenanceGraphEntry>,
    mut cross_links: Vec<NativeMaintenanceGraphCrossLink>,
) -> Option<NativeMaintenanceGraphManifest> {
    // retain the source graph population, optional strong predecessor and exact safe offsets
    if entries.is_empty()
        || entries.len() > MAX_ENTRIES
        || cross_links.len() > MAX_ENTRIES
        || predecessor_graph_sha256
            .as_deref()
            .is_some_and(|hash| !valid_hash(hash))
    {
        return None;
    }
    let mut identities = HashSet::with_capacity(entries.len());
    let mut ranges: HashMap<&str, Vec<(u64, u64)>> = HashMap::new();
    // validate portable identities and preserve every originally admitted safe member range
    for entry in &entries {
        if !valid_native_cas_kind(&entry.kind)
            || !valid_hash(&entry.identity_sha256)
            || !valid_hash(&entry.object_sha256)
            || !valid_hash(&entry.member_sha256)
            || entry.member_offset > MAXIMUM_SAFE_INTEGER
            || entry.member_length > MAXIMUM_SAFE_INTEGER
            || !identities.insert(entry.identity_sha256.as_str())
        {
            return None;
        }
        ranges.entry(&entry.object_sha256).or_default().push((
            entry.member_offset,
            entry.member_offset.checked_add(entry.member_length)?,
        ));
    }
    // sort offsets once per object instead of performing a quadratic pairwise scan
    for group in ranges.values_mut() {
        group.sort_unstable();
        let mut max_end = 0;
        // preserve the source's strict overlap rule including interior zero-length ranges
        for &(start, end) in group.iter() {
            if start < max_end {
                return None;
            }
            max_end = max_end.max(end);
        }
    }
    // require both actual graph endpoints and the source relation grammar
    for link in &cross_links {
        let relation = link.relation.as_bytes();
        if !identities.contains(link.from_identity_sha256.as_str())
            || !identities.contains(link.to_identity_sha256.as_str())
            || !(1..=64).contains(&relation.len())
            || !relation[0].is_ascii_lowercase()
            || !relation
                .iter()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_')
        {
            return None;
        }
    }
    drop(ranges);
    drop(identities);
    entries.sort_unstable_by(|a, b| {
        (
            &a.kind,
            &a.identity_sha256,
            &a.object_sha256,
            a.member_offset,
        )
            .cmp(&(
                &b.kind,
                &b.identity_sha256,
                &b.object_sha256,
                b.member_offset,
            ))
    });
    // canonical link key order is fromIdentitySha256, relation, toIdentitySha256
    cross_links.sort_unstable_by(|a, b| {
        (&a.from_identity_sha256, &a.relation, &a.to_identity_sha256).cmp(&(
            &b.from_identity_sha256,
            &b.relation,
            &b.to_identity_sha256,
        ))
    });
    let unsigned = object(vec![
        (
            "crossLinks",
            JsonValue::Array(cross_links.iter().map(link_json).collect()),
        ),
        (
            "entries",
            JsonValue::Array(entries.iter().map(entry_json).collect()),
        ),
        (
            "predecessorGraphSha256",
            nullable_hash(&predecessor_graph_sha256),
        ),
    ]);
    let mut codec = NativeWirePrimitives;
    let preimage = codec.encode_canonical_json(&unsigned)?;
    let graph_root_sha256 = codec.sha256(&preimage)?.as_str().to_owned();
    Some(NativeMaintenanceGraphManifest {
        predecessor_graph_sha256,
        entries,
        cross_links,
        graph_root_sha256,
    })
}

/// Validate and reconstruct a complete retained graph through the real canonical owner.
pub fn read_native_maintenance_graph_manifest(
    bytes: &[u8],
) -> Option<NativeMaintenanceGraphManifest> {
    // retain the archive built-in graph schema limit before decoding
    if bytes.len() > NATIVE_GRAPH_MANIFEST_MAXIMUM_BYTES {
        return None;
    }
    let mut codec = NativeWirePrimitives;
    let value = codec.parse_canonical_object(bytes)?;
    if !exact_keys(&value, MANIFEST_FIELDS)
        || text(&value, "contractVersion").as_deref() != Some(VERSION)
    {
        return None;
    }
    let predecessor = match field(&value, "predecessorGraphSha256")? {
        JsonValue::Null => None,
        _ => Some(text(&value, "predecessorGraphSha256")?),
    };
    let entries = array(&value, "entries")?
        .iter()
        .map(parse_entry)
        .collect::<Option<Vec<_>>>()?;
    let links = array(&value, "crossLinks")?
        .iter()
        .map(parse_link)
        .collect::<Option<Vec<_>>>()?;
    let rebuilt = build_native_maintenance_graph_manifest(predecessor, entries, links)?;
    // require exact canonical graph bytes including array order and root digest
    (rebuilt.canonical_bytes()?.as_slice() == bytes).then_some(rebuilt)
}

/// Admit exactly one source CAS kind spelling.
pub(crate) fn valid_native_cas_kind(kind: &str) -> bool {
    if kind.is_empty() || kind.len() > 255 {
        return false;
    }
    let (first, tail) = kind
        .split_once('/')
        .map_or((kind, None), |(a, b)| (a, Some(b)));
    let bytes = first.as_bytes();
    !bytes.is_empty()
        && bytes[0].is_ascii_lowercase()
        && bytes
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-')
        && tail.is_none_or(|value| {
            let bytes = value.as_bytes();
            !bytes.is_empty()
                && (bytes[0].is_ascii_lowercase() || bytes[0].is_ascii_digit())
                && bytes.iter().all(|b| {
                    b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'-' | b'.')
                })
        })
}

/// Decode a closed source graph entry.
fn parse_entry(value: &JsonValue) -> Option<NativeMaintenanceGraphEntry> {
    if !exact_keys(value, ENTRY_FIELDS) {
        return None;
    }
    Some(NativeMaintenanceGraphEntry {
        kind: text(value, "kind")?,
        identity_sha256: text(value, "identitySha256")?,
        object_sha256: text(value, "objectSha256")?,
        member_offset: safe_integer(value, "memberOffset")?,
        member_length: safe_integer(value, "memberLength")?,
        member_sha256: text(value, "memberSha256")?,
    })
}

/// Decode one exact path-free cross link.
fn parse_link(value: &JsonValue) -> Option<NativeMaintenanceGraphCrossLink> {
    if !exact_keys(value, LINK_FIELDS) {
        return None;
    }
    Some(NativeMaintenanceGraphCrossLink {
        from_identity_sha256: text(value, "fromIdentitySha256")?,
        relation: text(value, "relation")?,
        to_identity_sha256: text(value, "toIdentitySha256")?,
    })
}

/// Admit exact source nonnegative safe JSON integers.
fn safe_integer(value: &JsonValue, key: &str) -> Option<u64> {
    let JsonValue::Number(number) = field(value, key)? else {
        return None;
    };
    (*number >= 0.0 && number.fract() == 0.0 && *number <= MAXIMUM_SAFE_INTEGER as f64)
        .then_some(*number as u64)
}

/// Borrow one actual source array without normalizing its order.
fn array<'a>(value: &'a JsonValue, key: &str) -> Option<&'a [JsonValue]> {
    let JsonValue::Array(items) = field(value, key)? else {
        return None;
    };
    Some(items)
}

/// Convert one validated graph member to exact canonical wire fields.
fn entry_json(entry: &NativeMaintenanceGraphEntry) -> JsonValue {
    object(vec![
        ("kind", string(&entry.kind)),
        ("identitySha256", string(&entry.identity_sha256)),
        ("objectSha256", string(&entry.object_sha256)),
        (
            "memberOffset",
            JsonValue::Number(entry.member_offset as f64),
        ),
        (
            "memberLength",
            JsonValue::Number(entry.member_length as f64),
        ),
        ("memberSha256", string(&entry.member_sha256)),
    ])
}

/// Convert one validated semantic link to source wire names.
fn link_json(link: &NativeMaintenanceGraphCrossLink) -> JsonValue {
    object(vec![
        ("fromIdentitySha256", string(&link.from_identity_sha256)),
        ("relation", string(&link.relation)),
        ("toIdentitySha256", string(&link.to_identity_sha256)),
    ])
}

/// Preserve a null genesis predecessor.
fn nullable_hash(value: &Option<String>) -> JsonValue {
    value.as_deref().map_or(JsonValue::Null, string)
}

/// Construct the original JSON member names as UTF-16.
fn object(fields: Vec<(&str, JsonValue)>) -> JsonValue {
    JsonValue::Object(
        fields
            .into_iter()
            .map(|(key, value)| (key.encode_utf16().collect(), value))
            .collect(),
    )
}

/// Encode one source JSON string.
fn string(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}

#[cfg(test)]
#[path = "native_maintenance_graph_manifest_test.rs"]
mod tests;
