//! Source-exact blinded confirmation-plan archive publication and retained graph references.

use std::collections::{BTreeMap, BTreeSet};

use weather_contracts::JsonValue;
use weather_edge::{ArchiveWirePrimitives, RevisionArchiveCanonicalCodec, RevisionGapSha256};

use crate::{
    MaintenanceCanonicalJson, NativeMaintenanceArchiveJournal, NativeMaintenanceArchivePublication,
    NativeMaintenanceArchiveSegment, NativeMaintenanceArchiveSegmentArchive,
    NativeMaintenanceArchiveSegmentClock, NativeMaintenanceArchiveSegmentMember,
    NativeMaintenanceGraphCrossLink, NativeMaintenanceJournalStore, NativeWirePrimitives,
    maintenance_canonical_json::{exact_keys, field, text, valid_hash},
    native_maintenance_policy_comparison::{object, string},
    native_maintenance_retirement_files_v2::format_digest,
    publish_native_maintenance_archive_segment,
};

pub const NATIVE_CONFIRMATION_PLAN_GRAPH_VERSION: &str =
    "adjustment-maintenance-confirmation-plan-graph/v1";
pub const NATIVE_UNSUPPORTED_CONFIRMATION_PLAN_GRAPH_VERSION: &str =
    "adjustment-maintenance-confirmation-plan-graph/v2";
pub const NATIVE_CONFIRMATION_PLAN_GRAPH_MAXIMUM_BYTES: usize = 8 * 1024 * 1024;
const REFERENCE_VERSION: &str = "adjustment-archive-graph-reference/v1";

const ENTRY_KEYS: &[&str] = &[
    "comparatorMemberSha256",
    "dueKey",
    "graphManifestSha256",
    "payloadIdentitySha256",
    "predictionBodySha256",
    "receiptSha256",
    "sourceProjectionSha256",
    "sourceReceiptSha256",
];
const V1_KEYS: &[&str] = &[
    "contractVersion",
    "eligiblePredictionSetSha256",
    "entries",
    "family",
    "planGraphSha256",
    "registrationSha256",
];
const V2_KEYS: &[&str] = &[
    "contractVersion",
    "eligiblePredictionSetSha256",
    "entries",
    "family",
    "missingDueKeySetSha256",
    "missingDueKeys",
    "planGraphSha256",
    "registrationSha256",
];

/// Exact value-blind plan and the selected capsule identities grouped by immutable cold graph.
pub struct NativeMaintenanceConfirmationPlanGraph {
    pub plan_graph_sha256: String,
    pub contract_version: String,
    pub retained_by_graph: BTreeMap<String, BTreeSet<String>>,
}

/// Validate the complete or explicitly unsupported source grammar and every retained root.
pub fn validate_native_maintenance_confirmation_plan_graph(
    value: &JsonValue,
) -> Option<NativeMaintenanceConfirmationPlanGraph> {
    let contract = text(value, "contractVersion")?;
    let unsupported = match contract.as_str() {
        NATIVE_CONFIRMATION_PLAN_GRAPH_VERSION => false,
        NATIVE_UNSUPPORTED_CONFIRMATION_PLAN_GRAPH_VERSION => true,
        _ => return None,
    };
    if !exact_keys(value, if unsupported { V2_KEYS } else { V1_KEYS })
        || !matches!(
            text(value, "family")?.as_str(),
            "temperature" | "wind" | "rain"
        )
    {
        return None;
    }
    let plan_graph_sha256 = text(value, "planGraphSha256")?;
    let eligible = text(value, "eligiblePredictionSetSha256")?;
    if !valid_hash(&plan_graph_sha256)
        || !valid_hash(&eligible)
        || !valid_hash(&text(value, "registrationSha256")?)
    {
        return None;
    }
    let entries_value = field(value, "entries")?;
    let JsonValue::Array(entries) = entries_value else {
        return None;
    };
    if !unsupported && entries.is_empty() {
        return None;
    }
    let mut retained_by_graph = BTreeMap::<String, BTreeSet<String>>::new();
    let mut present = BTreeSet::new();
    let mut previous = None::<String>;
    // require exact source population order and bind every authenticated capsule member
    for entry in entries {
        if !exact_keys(entry, ENTRY_KEYS) {
            return None;
        }
        for key in ENTRY_KEYS.iter().copied().filter(|key| *key != "dueKey") {
            if !valid_hash(&text(entry, key)?) {
                return None;
            }
        }
        let due = text(entry, "dueKey")?;
        if !valid_capture_due(&due) || previous.as_ref().is_some_and(|prior| prior >= &due) {
            return None;
        }
        previous = Some(due.clone());
        present.insert(due);
        retained_by_graph
            .entry(text(entry, "graphManifestSha256")?)
            .or_default()
            .insert(text(entry, "payloadIdentitySha256")?);
    }
    if digest_value(entries_value)? != eligible {
        return None;
    }
    // preserve the disjoint missing due identities and their exact hierarchical root
    if unsupported {
        let missing_value = field(value, "missingDueKeys")?;
        let JsonValue::Array(missing) = missing_value else {
            return None;
        };
        let mut previous = None::<String>;
        for due in missing {
            let JsonValue::String(due) = due else {
                return None;
            };
            let due = String::from_utf16(due).ok()?;
            if !valid_capture_due(&due)
                || present.contains(&due)
                || previous.as_ref().is_some_and(|prior| prior >= &due)
            {
                return None;
            }
            previous = Some(due);
        }
        if digest_value(missing_value)? != text(value, "missingDueKeySetSha256")? {
            return None;
        }
    }
    let JsonValue::Object(fields) = value else {
        return None;
    };
    let unsigned = JsonValue::Object(
        fields
            .iter()
            .filter(|(key, _)| !key.iter().copied().eq("planGraphSha256".encode_utf16()))
            .cloned()
            .collect(),
    );
    if digest_value(&unsigned)? != plan_graph_sha256 {
        return None;
    }
    Some(NativeMaintenanceConfirmationPlanGraph {
        plan_graph_sha256,
        contract_version: contract,
        retained_by_graph,
    })
}

/// Admit canonical archived source bytes before accepting a plan graph member.
pub fn validate_native_maintenance_confirmation_plan_graph_bytes(
    kind: &str,
    bytes: &[u8],
) -> Option<()> {
    if bytes.is_empty() || bytes.len() > NATIVE_CONFIRMATION_PLAN_GRAPH_MAXIMUM_BYTES {
        return None;
    }
    let value = NativeWirePrimitives.parse_canonical_object(bytes)?;
    let graph = validate_native_maintenance_confirmation_plan_graph(&value)?;
    (graph.contract_version == kind).then_some(())
}

/// Build the exact compact source plan segment with one graph-reference member per unique cold graph.
pub fn build_native_maintenance_confirmation_plan_segment(
    plan_graph: &JsonValue,
) -> Option<NativeMaintenanceArchiveSegment> {
    let admitted = validate_native_maintenance_confirmation_plan_graph(plan_graph)?;
    let wire = NativeWirePrimitives;
    let plan_bytes = wire.encode_sorted_json(plan_graph)?;
    if plan_bytes.len() > NATIVE_CONFIRMATION_PLAN_GRAPH_MAXIMUM_BYTES {
        return None;
    }
    let mut members = vec![NativeMaintenanceArchiveSegmentMember {
        kind: admitted.contract_version,
        identity_sha256: *RevisionGapSha256::from_lower_hex(&admitted.plan_graph_sha256)?
            .as_bytes(),
        payload: plan_bytes,
    }];
    let mut cross_links = Vec::with_capacity(admitted.retained_by_graph.len());
    // retain reference identities in source hash order without copying any selected value bytes
    for graph_sha256 in admitted.retained_by_graph.keys() {
        let reference = object(vec![
            ("contractVersion", string(REFERENCE_VERSION)),
            ("graphSha256", string(graph_sha256)),
        ]);
        let bytes = wire.encode_sorted_json(&reference)?;
        let identity = RevisionArchiveCanonicalCodec::sha256(&wire, &bytes);
        cross_links.push(NativeMaintenanceGraphCrossLink {
            from_identity_sha256: admitted.plan_graph_sha256.clone(),
            relation: "binds_confirmation_source_graph".to_owned(),
            to_identity_sha256: format_digest(identity),
        });
        members.push(NativeMaintenanceArchiveSegmentMember {
            kind: REFERENCE_VERSION.to_owned(),
            identity_sha256: identity,
            payload: bytes,
        });
    }
    Some(NativeMaintenanceArchiveSegment {
        members,
        cross_links,
    })
}

/// Verify every named cold graph and publish the original blinded plan under its archive due.
pub fn archive_native_maintenance_confirmation_plan<Archive, Clock>(
    plan_graph: &JsonValue,
    archive: &mut Archive,
    journal: &mut NativeMaintenanceJournalStore,
    clock: &mut Clock,
) -> Option<NativeMaintenanceArchivePublication>
where
    Archive: NativeMaintenanceArchiveSegmentArchive,
    Clock: NativeMaintenanceArchiveSegmentClock,
{
    let admitted = validate_native_maintenance_confirmation_plan_graph(plan_graph)?;
    // verify each unique graph once, then prove every retained capsule under that graph
    for (graph, selected) in &admitted.retained_by_graph {
        let digest = *RevisionGapSha256::from_lower_hex(graph)?.as_bytes();
        let verified = archive.verify_full_graph(digest).ok()?;
        let retained = verified
            .manifest
            .entries
            .iter()
            .filter(|entry| entry.kind == "adjustment-shadow-revision-capsule/v2")
            .map(|entry| entry.identity_sha256.as_str())
            .collect::<BTreeSet<_>>();
        if selected
            .iter()
            .any(|identity| !retained.contains(identity.as_str()))
        {
            return None;
        }
    }
    let segment = build_native_maintenance_confirmation_plan_segment(plan_graph)?;
    let due_key = format!("archive/confirmation-plan/{}", admitted.plan_graph_sha256);
    let publication = publish_native_maintenance_archive_segment(
        archive,
        &mut NativeMaintenanceArchiveJournal::new(journal),
        clock,
        &due_key,
        &segment,
    )
    .ok()?;
    archive
        .verify_full_graph(publication.manifest_object_sha256)
        .ok()?;
    Some(publication)
}

/// Match the exact predecessor capture due spelling, without broad date normalization.
fn valid_capture_due(value: &str) -> bool {
    let Some(instant) = value.strip_prefix("capture/") else {
        return false;
    };
    let bytes = instant.as_bytes();
    bytes.len() == 24
        && &bytes[..2] == b"20"
        && bytes[2..4].iter().all(u8::is_ascii_digit)
        && bytes[4] == b'-'
        && bytes[5..7].iter().all(u8::is_ascii_digit)
        && bytes[7] == b'-'
        && bytes[8..10].iter().all(u8::is_ascii_digit)
        && bytes[10] == b'T'
        && matches!(&bytes[11..13], b"00" | b"06" | b"12" | b"18")
        && &bytes[13..] == b":35:00.000Z"
}

/// Hash exact predecessor LF-framed canonical JSON.
fn digest_value(value: &JsonValue) -> Option<String> {
    let wire = NativeWirePrimitives;
    Some(format_digest(RevisionArchiveCanonicalCodec::sha256(
        &wire,
        &wire.encode_sorted_json(value)?,
    )))
}
