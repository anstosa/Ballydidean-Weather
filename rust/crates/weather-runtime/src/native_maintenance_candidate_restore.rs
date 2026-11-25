//! Restore the root-acknowledged installed candidate from the actual retained archive chain.

use std::{
    collections::{BTreeMap, BTreeSet},
    io::Read,
};
use weather_edge::{RevisionArchiveCanonicalCodec, RevisionGapSha256};

use crate::{
    MaintenanceCanonicalJson, NativeMaintenanceCaptureFamily,
    NativeMaintenanceControllerArchiveValidators, NativeMaintenanceGraphRestoreSink,
    NativeMaintenanceGraphRestoreTarget, NativeMaintenanceJournalStore,
    NativeMaintenancePlaintextObjectStore, NativeWirePrimitives,
    build_native_rain_maintenance_artifact, build_native_temperature_maintenance_package,
    maintenance_canonical_json::{text, valid_hash},
    native_maintenance_candidate_parity::build_wind_portable_candidate,
    native_maintenance_confirmation_access_journal::kind,
    native_maintenance_confirmation_journal::is_terminal,
    native_maintenance_retirement_files_v2::format_digest,
    restore_native_maintenance_full_graph, verify_native_maintenance_full_graph,
};

const MAXIMUM_MEMBER_BYTES: usize = 8 * 1024 * 1024;
const MAXIMUM_GRAPH_CHAIN: usize = 16_744;

/// Original raw fit, independently rebuilt package and exact archived parity receipt.
pub struct NativeMaintenanceRestoredCandidate {
    pub candidate_bytes: Vec<u8>,
    pub artifact_bytes: Vec<u8>,
    pub parity_receipt_bytes: Vec<u8>,
    pub parity_sha256: String,
    pub candidate_graph_sha256: String,
}

/// Resolve the active candidate only through the acknowledged local installation and actual archive head.
pub fn restore_native_maintenance_active_candidate(
    family: NativeMaintenanceCaptureFamily,
    candidate_sha256: &str,
    artifact_sha256: &str,
    journal: &mut NativeMaintenanceJournalStore,
    archive: &mut NativeMaintenancePlaintextObjectStore,
) -> Option<NativeMaintenanceRestoredCandidate> {
    if !valid_hash(candidate_sha256) || !valid_hash(artifact_sha256) {
        return None;
    }
    let records = journal
        .read_verified(&mut NativeWirePrimitives, &NativeWirePrimitives)
        .ok()?;
    let prefix = format!("confirmation/{}/", family.wire());
    let family_records = records
        .iter()
        .filter(|record| {
            text(&record.payload, "family").as_deref() == Some(family.wire())
                || record
                    .due_key
                    .as_ref()
                    .is_some_and(|due| due.starts_with(&prefix))
        })
        .collect::<Vec<_>>();
    let registration = family_records
        .iter()
        .copied()
        .filter(|record| kind(record, "confirmation_registered"))
        .find(|record| {
            text(&record.payload, "registrationSha256")
                .is_some_and(|registration| !is_terminal(&family_records, &registration))
        })?;
    // preserve the exact active family slot and its acknowledged immutable installation event
    if text(&registration.payload, "candidateSha256")?.as_str() != candidate_sha256 {
        return None;
    }
    let installed = records.iter().rev().find(|record| {
        kind(record, "confirmation_development_installed")
            && text(&record.payload, "candidateSha256").as_deref() == Some(candidate_sha256)
    })?;
    if text(&installed.payload, "artifactSha256")?.as_str() != artifact_sha256 {
        return None;
    }
    let candidate_graph_sha256 = text(&installed.payload, "candidateGraphSha256")?;
    let candidate_kind = format!(
        "forecast-adjustment-{}-development-candidate/v1",
        family.wire()
    );
    let mut visited = BTreeSet::new();
    let mut head = archive.read_head().ok()??;
    let mut validators = NativeMaintenanceControllerArchiveValidators::new();
    // walk the real authenticated predecessor chain and stop on the first matching candidate identity
    let graph = loop {
        if visited.len() >= MAXIMUM_GRAPH_CHAIN || !visited.insert(head) {
            return None;
        }
        let graph = verify_native_maintenance_full_graph(archive, head, &mut validators, false)?;
        if let Some(entry) = graph
            .manifest
            .entries
            .iter()
            .find(|entry| entry.identity_sha256 == candidate_sha256)
        {
            if entry.kind != candidate_kind || format_digest(head) != candidate_graph_sha256 {
                return None;
            }
            break graph;
        }
        head = parse_hash(graph.manifest.predecessor_graph_sha256.as_deref()?)?;
    };
    let mut parity = graph
        .manifest
        .entries
        .iter()
        .filter(|entry| entry.kind == "forecast-adjustment-model-parity/v1");
    let parity_sha256 = parity.next()?.identity_sha256.clone();
    if parity.next().is_some() {
        return None;
    }
    let mut sink = CandidateSink(BTreeMap::new());
    restore_native_maintenance_full_graph(
        archive,
        head,
        &mut validators,
        &mut sink,
        &[
            NativeMaintenanceGraphRestoreTarget {
                file_name: "candidate.json",
                identity_sha256: parse_hash(candidate_sha256)?,
            },
            NativeMaintenanceGraphRestoreTarget {
                file_name: "artifact.json",
                identity_sha256: parse_hash(artifact_sha256)?,
            },
            NativeMaintenanceGraphRestoreTarget {
                file_name: "parity.json",
                identity_sha256: parse_hash(&parity_sha256)?,
            },
        ],
        Some((16 * 1024 * 1024 + 16 * 1024) as u64),
    )?;
    let candidate_bytes = sink.0.remove("candidate.json")?;
    let artifact_bytes = sink.0.remove("artifact.json")?;
    let parity_receipt_bytes = sink.0.remove("parity.json")?;
    let wire = NativeWirePrimitives;
    // retain identity of the raw fit and parity member rather than comparing normalized content
    if format_digest(RevisionArchiveCanonicalCodec::sha256(
        &wire,
        &candidate_bytes,
    )) != candidate_sha256
        || format_digest(RevisionArchiveCanonicalCodec::sha256(
            &wire,
            &parity_receipt_bytes,
        )) != parity_sha256
    {
        return None;
    }
    let parity = NativeWirePrimitives.parse_canonical_object(&parity_receipt_bytes)?;
    if text(&parity, "contractVersion")? != "forecast-adjustment-model-parity/v1"
        || text(&parity, "candidateSha256")? != candidate_sha256
        || text(&parity, "family")? != family.wire()
    {
        return None;
    }
    let (rebuilt, identity) = match family {
        NativeMaintenanceCaptureFamily::Temperature => {
            let (bytes, _, identity) =
                build_native_temperature_maintenance_package(&candidate_bytes).ok()?;
            (bytes, identity)
        }
        NativeMaintenanceCaptureFamily::Wind => {
            let portable = build_wind_portable_candidate(&candidate_bytes).ok()?;
            (portable.artifact_bytes, portable.artifact_sha256)
        }
        NativeMaintenanceCaptureFamily::Rain => {
            let portable = build_native_rain_maintenance_artifact(&candidate_bytes).ok()?;
            (portable.artifact_bytes, portable.artifact_sha256)
        }
    };
    // independently rebuild and compare the actual self-addressed portable artifact
    if identity != artifact_sha256 || rebuilt != artifact_bytes {
        return None;
    }
    Some(NativeMaintenanceRestoredCandidate {
        candidate_bytes,
        artifact_bytes,
        parity_receipt_bytes,
        parity_sha256,
        candidate_graph_sha256,
    })
}

struct CandidateSink(BTreeMap<String, Vec<u8>>);
impl NativeMaintenanceGraphRestoreSink for CandidateSink {
    /// Retain only three bounded actual immutable graph outputs.
    fn write_exclusive(
        &mut self,
        name: &str,
        reader: &mut impl Read,
        expected_length: u64,
    ) -> Option<()> {
        let length = usize::try_from(expected_length).ok()?;
        if length > MAXIMUM_MEMBER_BYTES
            || !matches!(name, "candidate.json" | "artifact.json" | "parity.json")
            || self.0.contains_key(name)
        {
            return None;
        }
        let mut bytes = Vec::with_capacity(length);
        reader.read_to_end(&mut bytes).ok()?;
        if bytes.len() != length {
            return None;
        }
        self.0.insert(name.to_owned(), bytes);
        Some(())
    }
}

/// Admit one exact lower-hex graph or member identity.
fn parse_hash(value: &str) -> Option<[u8; 32]> {
    Some(*RevisionGapSha256::from_lower_hex(value)?.as_bytes())
}
