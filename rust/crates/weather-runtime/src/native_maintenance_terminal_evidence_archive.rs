//! Bounded canonical terminal graph partition and archive publication over real post-burn values.

use std::collections::BTreeSet;

use weather_contracts::JsonValue;
use weather_edge::{ArchiveWirePrimitives, RevisionArchiveCanonicalCodec, RevisionGapSha256};

use crate::{
    NativeMaintenanceArchiveJournal, NativeMaintenanceArchiveSegment,
    NativeMaintenanceArchiveSegmentMember, NativeMaintenanceArchiveWallClock,
    NativeMaintenanceCaptureFamily, NativeMaintenanceControllerArchiveValidators,
    NativeMaintenanceDailyTerminalValues, NativeMaintenanceInstalledTerminalMaterial,
    NativeMaintenanceJournalStore, NativeMaintenancePlaintextArchive,
    NativeMaintenanceVerifiedFullGraph, NativeWirePrimitives,
    maintenance_canonical_json::{exact_keys, field, text, valid_hash},
    native_maintenance_policy_comparison::{object, string},
    native_maintenance_retirement_files_v2::format_digest,
    publish_native_maintenance_archive_segment,
};

const PART_KIND: &str = "adjustment-maintenance-confirmation-terminal-graph-part/v1";
const MANIFEST_KIND: &str = "adjustment-maintenance-confirmation-terminal-graph-manifest/v1";
const COMPLETE_KIND: &str = "adjustment-maintenance-confirmation-terminal-graph/v1";
const UNSUPPORTED_KIND: &str = "adjustment-maintenance-confirmation-unsupported-terminal-graph/v1";
const PART_MAX_BYTES: usize = 8 * 1024 * 1024;
const PART_MAX_BINDINGS: usize = 4096;
const SEGMENT_MAX_BYTES: usize = 64 * 1024 * 1024;
const SEGMENT_MAX_MEMBERS: usize = 13_000;

/// Exact bounded terminal graph and canonical members.
pub struct NativeMaintenanceTerminalGraphParts {
    pub manifest: JsonValue,
    pub manifest_member: NativeMaintenanceArchiveSegmentMember,
    pub parts: Vec<NativeMaintenanceArchiveSegmentMember>,
}

/// Real archived data members followed by their final custody graph.
pub struct NativeMaintenanceTerminalArchiveEvidence {
    pub data_members: Vec<NativeMaintenanceArchiveSegmentMember>,
    pub final_members: Vec<NativeMaintenanceArchiveSegmentMember>,
    pub terminal_graph_manifest: JsonValue,
}

/// Source effect refusal classes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMaintenanceTerminalEvidenceError {
    Input,
    Graph,
    Member,
    Archive,
}

use NativeMaintenanceTerminalEvidenceError as Error;

/// Split the original sorted bindings at exactly the source's byte and record limits.
pub fn build_native_maintenance_terminal_graph_parts(
    family: NativeMaintenanceCaptureFamily,
    graph: &JsonValue,
) -> Result<NativeMaintenanceTerminalGraphParts, Error> {
    let kind = text(graph, "contractVersion").ok_or(Error::Graph)?;
    let unsupported = kind == UNSUPPORTED_KIND;
    let mut keys = vec![
        "bindings",
        "contractVersion",
        "derivedTargetMemberSha256s",
        "graphManifestSha256s",
        "terminalGraphSha256",
    ];
    // preserve the disjoint unsupported graph shape
    if unsupported {
        keys.push("missingKeySetSha256");
    }
    if (!unsupported && kind != COMPLETE_KIND) || !exact_keys(graph, &keys) {
        return Err(Error::Graph);
    }
    let graph_sha = text(graph, "terminalGraphSha256")
        .filter(|v| valid_hash(v))
        .ok_or(Error::Graph)?;
    let JsonValue::Object(unsigned) = graph else {
        return Err(Error::Graph);
    };
    let unsigned = JsonValue::Object(
        unsigned
            .iter()
            .filter(|(name, _)| {
                !name
                    .iter()
                    .copied()
                    .eq("terminalGraphSha256".encode_utf16())
            })
            .cloned()
            .collect(),
    );
    if hash(&encode(&unsigned)?) != graph_sha {
        return Err(Error::Graph);
    }
    let derived = field(graph, "derivedTargetMemberSha256s").ok_or(Error::Graph)?;
    let references = field(graph, "graphManifestSha256s").ok_or(Error::Graph)?;
    require_sorted_hashes(derived)?;
    require_sorted_hashes(references)?;
    let missing = if unsupported {
        string(
            &text(graph, "missingKeySetSha256")
                .filter(|v| valid_hash(v))
                .ok_or(Error::Graph)?,
        )
    } else {
        JsonValue::Null
    };
    let Some(JsonValue::Array(bindings)) = field(graph, "bindings") else {
        return Err(Error::Graph);
    };
    let mut previous: Option<String> = None;
    let mut groups = Vec::new();
    let mut start = 0;
    let shell_len = encode(&part_document(
        family,
        &graph_sha,
        &[],
        9_500_000,
        9_500_000,
    ))?
    .len();
    let mut bytes = shell_len;
    // compute the exact canonical array length once per row instead of serializing every growing prefix
    for (index, binding) in bindings.iter().enumerate() {
        let key = text(binding, "key").ok_or(Error::Graph)?;
        if previous.as_ref().is_some_and(|previous| previous >= &key) {
            return Err(Error::Graph);
        }
        previous = Some(key);
        let row_bytes = encode(binding)?.len().checked_sub(1).ok_or(Error::Graph)?;
        let candidate = bytes
            .checked_add(row_bytes)
            .and_then(|v| v.checked_add(usize::from(index > start)))
            .ok_or(Error::Graph)?;
        if index > start && (index - start >= PART_MAX_BINDINGS || candidate > PART_MAX_BYTES) {
            groups.push(start..index);
            start = index;
            bytes = shell_len.checked_add(row_bytes).ok_or(Error::Graph)?;
        } else {
            bytes = candidate;
        }
    }
    groups.push(start..bindings.len());
    let mut parts = Vec::with_capacity(groups.len());
    let count = groups.len();
    // retain actual final part coordinates and recheck the source hard bound
    for (index, range) in groups.into_iter().enumerate() {
        let payload = encode(&part_document(
            family,
            &graph_sha,
            &bindings[range],
            count,
            index,
        ))?;
        if payload.len() > PART_MAX_BYTES {
            return Err(Error::Graph);
        }
        parts.push(make_member(PART_KIND, &hash(&payload), payload)?);
    }
    let unsigned = object(vec![
        ("contractVersion", string(MANIFEST_KIND)),
        ("derivedTargetMemberSha256s", derived.clone()),
        ("family", string(family.wire())),
        ("graphManifestSha256s", references.clone()),
        ("missingKeySetSha256", missing),
        ("originalContractVersion", string(&kind)),
        ("originalTerminalGraphSha256", string(&graph_sha)),
        (
            "partSha256s",
            JsonValue::Array(
                parts
                    .iter()
                    .map(|part| string(&format_digest(part.identity_sha256)))
                    .collect(),
            ),
        ),
    ]);
    let manifest_sha = hash(&encode(&unsigned)?);
    let JsonValue::Object(mut fields) = unsigned else {
        return Err(Error::Graph);
    };
    fields.push((
        "terminalGraphManifestSha256".encode_utf16().collect(),
        string(&manifest_sha),
    ));
    let manifest = JsonValue::Object(fields);
    let manifest_member = make_member(MANIFEST_KIND, &manifest_sha, encode(&manifest)?)?;
    Ok(NativeMaintenanceTerminalGraphParts {
        manifest,
        manifest_member,
        parts,
    })
}

/// Build the actual terminal evidence members, keeping raw value and no-action domains disjoint.
pub fn build_native_maintenance_terminal_archive_evidence(
    material: &NativeMaintenanceInstalledTerminalMaterial,
    action: &JsonValue,
    policy_bytes: &[u8],
    candidate_report_bytes: Option<&[u8]>,
) -> Result<NativeMaintenanceTerminalArchiveEvidence, Error> {
    let (family, chunks, derived, references, graph, unsupported) = match &material.assembly {
        NativeMaintenanceDailyTerminalValues::Complete {
            family,
            chunks,
            derived_targets,
            graph_manifest_sha256s,
            terminal_graph,
            ..
        } => (
            *family,
            chunks,
            derived_targets,
            graph_manifest_sha256s,
            terminal_graph,
            false,
        ),
        NativeMaintenanceDailyTerminalValues::Unsupported {
            family,
            chunks,
            derived_targets,
            graph_manifest_sha256s,
            terminal_graph,
            ..
        } => (
            *family,
            chunks,
            derived_targets,
            graph_manifest_sha256s,
            terminal_graph,
            true,
        ),
        NativeMaintenanceDailyTerminalValues::Pending { .. } => return Err(Error::Input),
    };
    let shadow = &material.access.shadow_registration.registration;
    let candidate = text(shadow, "candidateSha256").ok_or(Error::Input)?;
    let registration = text(shadow, "registrationSha256").ok_or(Error::Input)?;
    let confirmation = text(
        &material.access.confirmation_registration,
        "registrationSha256",
    )
    .ok_or(Error::Input)?;
    let full_root = text(&material.full_manifest, "fullMemberRootSha256").ok_or(Error::Input)?;
    let policy = canonical(policy_bytes)?;
    let policy_sha = hash(policy_bytes);
    let action_bytes = encode(action)?;
    // require the original policy authority and complete evaluated report when applicable
    if unsupported != candidate_report_bytes.is_none()
        || text(&policy, "family").as_deref() != Some(family.wire())
    {
        return Err(Error::Input);
    }
    if !unsupported && text(action, "policyReportSha256").as_deref() != Some(&policy_sha) {
        return Err(Error::Input);
    }
    let report = candidate_report_bytes.map(canonical).transpose()?;
    if let Some(report) = &report
        && (text(report, "family").as_deref() != Some(family.wire())
            || text(report, "candidateSha256").as_deref() != Some(&candidate)
            || text(report, "registrationSha256").as_deref() != Some(&registration)
            || text(report, "confirmationRegistrationSha256").as_deref() != Some(&confirmation)
            || text(report, "fullMemberRootSha256").as_deref() != Some(&full_root)
            || text(report, "policyReportSha256").as_deref() != Some(&policy_sha))
    {
        return Err(Error::Input);
    }
    let partition = build_native_maintenance_terminal_graph_parts(family, graph)?;
    let mut data_members = Vec::new();
    // retain the canonical blinded identity and exact physical part bytes
    for chunk in chunks {
        let bytes = NativeWirePrimitives
            .decode_standard_base64(&chunk.payload_base64)
            .ok_or(Error::Member)?;
        let value = canonical(&bytes)?;
        let identity = text(&chunk.metadata, "partSha256")
            .or_else(|| text(&chunk.metadata, "chunkSha256"))
            .ok_or(Error::Member)?;
        if hash(&bytes) != identity {
            return Err(Error::Member);
        }
        data_members.push(make_member(&kind(&value)?, &identity, bytes)?);
    }
    // retain the immutable regional target members only once
    for target in derived {
        let value = canonical(&target.bytes)?;
        if hash(&target.bytes) != target.target_member_sha256 {
            return Err(Error::Member);
        }
        data_members.push(make_member(
            &kind(&value)?,
            &target.target_member_sha256,
            target.bytes.clone(),
        )?);
    }
    data_members.extend(partition.parts);
    let mut final_members = vec![
        partition.manifest_member,
        make_member(
            &kind(&material.full_manifest)?,
            &full_root,
            encode(&material.full_manifest)?,
        )?,
        document_member(&object(vec![
            (
                "contractVersion",
                string("adjustment-confirmation-local-burn-member/v1"),
            ),
            ("localBurn", material.access.local_burn.clone()),
        ]))?,
        document_member(&object(vec![
            (
                "contractVersion",
                string("adjustment-confirmation-native-access-member/v1"),
            ),
            ("nativeAccess", material.access.native_access.clone()),
        ]))?,
    ];
    if let Some(report) = &report {
        final_members.push(document_member(report)?);
    }
    final_members.extend([
        make_member(&kind(&policy)?, &policy_sha, policy_bytes.to_vec())?,
        make_member(&kind(action)?, &hash(&action_bytes), action_bytes)?,
        make_member(
            &kind(&material.access.confirmation_registration)?,
            &confirmation,
            encode(&material.access.confirmation_registration)?,
        )?,
        make_member(
            "adjustment-shadow-registration/v3",
            &registration,
            encode(shadow)?,
        )?,
    ]);
    let mut all_references: BTreeSet<&str> = references.iter().map(String::as_str).collect();
    all_references.insert(&material.candidate.candidate_graph_sha256);
    all_references.insert(&material.access.plan_graph_manifest_sha256);
    // preserve actual predecessor graph references in the source byte order
    for reference in all_references {
        if !valid_hash(reference) {
            return Err(Error::Graph);
        }
        final_members.push(document_member(&object(vec![
            (
                "contractVersion",
                string("adjustment-archive-graph-reference/v1"),
            ),
            ("graphSha256", string(reference)),
        ]))?);
    }
    Ok(NativeMaintenanceTerminalArchiveEvidence {
        data_members,
        final_members,
        terminal_graph_manifest: partition.manifest,
    })
}

/// Publish each exact bounded data batch before the final verified graph.
pub fn publish_native_maintenance_terminal_archive_evidence(
    archive: &mut NativeMaintenancePlaintextArchive<NativeMaintenanceControllerArchiveValidators>,
    journal: &mut NativeMaintenanceJournalStore,
    evidence: NativeMaintenanceTerminalArchiveEvidence,
) -> Result<NativeMaintenanceVerifiedFullGraph, Error> {
    let mut batch = Vec::new();
    let mut bytes = 0usize;
    // move—not copy—each source member into its original bounded batch
    for member in evidence.data_members {
        let length = member.payload.len();
        if !batch.is_empty()
            && (batch.len() >= SEGMENT_MAX_MEMBERS
                || bytes.checked_add(length).ok_or(Error::Archive)? > SEGMENT_MAX_BYTES)
        {
            publish_batch(archive, journal, std::mem::take(&mut batch))?;
            bytes = 0;
        }
        bytes = bytes.checked_add(length).ok_or(Error::Archive)?;
        batch.push(member);
    }
    if !batch.is_empty() {
        publish_batch(archive, journal, batch)?;
    }
    let final_graph = publish_batch(archive, journal, evidence.final_members)?;
    archive
        .verify_full_graph(final_graph, true)
        .map_err(|_| Error::Archive)
}

/// Publish one actual archive segment under its leading immutable checkpoint.
fn publish_batch(
    archive: &mut NativeMaintenancePlaintextArchive<NativeMaintenanceControllerArchiveValidators>,
    journal: &mut NativeMaintenanceJournalStore,
    members: Vec<NativeMaintenanceArchiveSegmentMember>,
) -> Result<[u8; 32], Error> {
    let identity = format_digest(members.first().ok_or(Error::Member)?.identity_sha256);
    let segment = NativeMaintenanceArchiveSegment {
        cross_links: Vec::new(),
        members,
    };
    publish_native_maintenance_archive_segment(
        archive,
        &mut NativeMaintenanceArchiveJournal::new(journal),
        &mut NativeMaintenanceArchiveWallClock,
        &format!("archive/confirmation-terminal-part/{identity}"),
        &segment,
    )
    .map(|publication| publication.manifest_object_sha256)
    .map_err(|_| Error::Archive)
}

/// Build one exact bounded source graph-part value.
fn part_document(
    family: NativeMaintenanceCaptureFamily,
    graph: &str,
    bindings: &[JsonValue],
    count: usize,
    index: usize,
) -> JsonValue {
    object(vec![
        ("bindings", JsonValue::Array(bindings.to_vec())),
        ("contractVersion", string(PART_KIND)),
        ("family", string(family.wire())),
        ("originalTerminalGraphSha256", string(graph)),
        ("partCount", JsonValue::Number(count as f64)),
        ("partIndex", JsonValue::Number(index as f64)),
    ])
}

/// Require the source's sorted unique immutable references.
fn require_sorted_hashes(value: &JsonValue) -> Result<(), Error> {
    let JsonValue::Array(values) = value else {
        return Err(Error::Graph);
    };
    let mut previous = None;
    // reject unordered, repeated or malformed graph references
    for value in values {
        let JsonValue::String(value) = value else {
            return Err(Error::Graph);
        };
        let value = String::from_utf16(value).map_err(|_| Error::Graph)?;
        if !valid_hash(&value) || previous.as_ref().is_some_and(|previous| previous >= &value) {
            return Err(Error::Graph);
        }
        previous = Some(value);
    }
    Ok(())
}

/// Decode and reserialize exact canonical source JSON bytes.
fn canonical(bytes: &[u8]) -> Result<JsonValue, Error> {
    let value = NativeWirePrimitives
        .decode_json(bytes)
        .ok_or(Error::Member)?;
    if encode(&value)? != bytes {
        return Err(Error::Member);
    }
    Ok(value)
}

/// Serialize a source value with canonical member ordering and the final newline.
fn encode(value: &JsonValue) -> Result<Vec<u8>, Error> {
    NativeWirePrimitives
        .encode_sorted_json(value)
        .ok_or(Error::Member)
}

/// Compute the immutable raw byte digest.
fn hash(bytes: &[u8]) -> String {
    format_digest(RevisionArchiveCanonicalCodec::sha256(
        &NativeWirePrimitives,
        bytes,
    ))
}

/// Retain the document's exact registered contract version.
fn kind(value: &JsonValue) -> Result<String, Error> {
    text(value, "contractVersion").ok_or(Error::Member)
}

/// Bind a self-identified or content-addressed member to its exact canonical payload.
fn make_member(
    kind: &str,
    identity: &str,
    payload: Vec<u8>,
) -> Result<NativeMaintenanceArchiveSegmentMember, Error> {
    Ok(NativeMaintenanceArchiveSegmentMember {
        kind: kind.to_owned(),
        identity_sha256: *RevisionGapSha256::from_lower_hex(identity)
            .ok_or(Error::Member)?
            .as_bytes(),
        payload,
    })
}

/// Encode and hash one ordinary canonical document.
fn document_member(value: &JsonValue) -> Result<NativeMaintenanceArchiveSegmentMember, Error> {
    let bytes = encode(value)?;
    make_member(&kind(value)?, &hash(&bytes), bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::Write,
        path::Path,
        process::{Command, Stdio},
    };

    #[test]
    fn terminal_graph_partition_matches_source() {
        // compare both disjoint terminal domains against the retained canonical source builder
        for unsupported in [false, true] {
            let mut graph = vec![
                (
                    "bindings",
                    JsonValue::Array(vec![
                        object(vec![
                            ("key", string("2026-11-01/0")),
                            ("source", string("one")),
                        ]),
                        object(vec![
                            ("key", string("2026-11-01/1")),
                            ("source", string("two")),
                        ]),
                    ]),
                ),
                (
                    "contractVersion",
                    string(if unsupported {
                        UNSUPPORTED_KIND
                    } else {
                        COMPLETE_KIND
                    }),
                ),
                ("derivedTargetMemberSha256s", JsonValue::Array(vec![])),
                (
                    "graphManifestSha256s",
                    JsonValue::Array(vec![string(&"a".repeat(64))]),
                ),
            ];
            if unsupported {
                graph.push(("missingKeySetSha256", string(&"b".repeat(64))));
            }
            let identity = hash(&encode(&object(graph.clone())).unwrap());
            graph.push(("terminalGraphSha256", string(&identity)));
            let graph = object(graph);
            let parts = build_native_maintenance_terminal_graph_parts(
                NativeMaintenanceCaptureFamily::Rain,
                &graph,
            )
            .unwrap();
            let oracle = r#"
import { readFileSync } from "node:fs";
import { buildAdjustmentMaintenanceTerminalGraphParts } from "./scripts/research/adjustment_maintenance_controller.mjs";
const value=JSON.parse(readFileSync(0,"utf8"));
const result=buildAdjustmentMaintenanceTerminalGraphParts({family:"rain",terminalGraph:value});
process.stdout.write(JSON.stringify({ manifest:result.manifestBytes.toString("base64"),parts:result.parts.map(part=>part.bytes.toString("base64")) }));
"#;
            let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
            let mut child = Command::new("node")
                .args(["--input-type=module", "--eval", oracle])
                .current_dir(root)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()
                .unwrap();
            child
                .stdin
                .take()
                .unwrap()
                .write_all(&encode(&graph).unwrap())
                .unwrap();
            let response = child.wait_with_output().unwrap();
            assert!(
                response.status.success(),
                "source oracle: {}",
                String::from_utf8_lossy(&response.stderr)
            );
            let value = NativeWirePrimitives.decode_json(&response.stdout).unwrap();
            let expected = NativeWirePrimitives
                .decode_standard_base64(&text(&value, "manifest").unwrap())
                .unwrap();
            assert_eq!(parts.manifest_member.payload, expected);
            let Some(JsonValue::Array(expected_parts)) = field(&value, "parts") else {
                panic!("source parts")
            };
            assert_eq!(parts.parts.len(), expected_parts.len());
            for (actual, expected) in parts.parts.iter().zip(expected_parts) {
                let JsonValue::String(expected) = expected else {
                    panic!("source part bytes")
                };
                let expected = NativeWirePrimitives
                    .decode_standard_base64(&String::from_utf16(expected).unwrap())
                    .unwrap();
                assert_eq!(actual.payload, expected);
            }
        }
    }
}
