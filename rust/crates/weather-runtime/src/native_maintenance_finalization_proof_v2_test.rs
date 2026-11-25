use weather_contracts::JsonValue;

use super::{
    NativeMaintenanceFinalizationProofV2Error, parse_native_maintenance_finalization_proof_v2,
    project_native_maintenance_finalized_anchor_v2,
};
use crate::maintenance_canonical_json::{field, text};
use crate::native_maintenance_retirement_files_v2::{
    NativeMaintenanceRetirementEntryV2, NativeMaintenanceRetirementKindV2,
};
use crate::{MaintenanceCanonicalJson, NativeWirePrimitives};

/// Admit a complete source-shaped proof and project its exact finalized anchor.
#[test]
fn parses_and_projects_legacy_finalization_proof() {
    let mut codec = NativeWirePrimitives;
    let anchor = transferred_anchor();
    let anchor_bytes = codec.encode_canonical_json(&anchor).unwrap();
    let anchor_sha256 = codec.sha256(&anchor_bytes).unwrap().as_str().to_owned();
    let proof = proof(&anchor, &anchor_sha256, retirement_entries());
    let proof_bytes = codec.encode_canonical_json(&proof).unwrap();

    let admitted = parse_native_maintenance_finalization_proof_v2(
        &proof_bytes,
        &mut codec,
        &NativeWirePrimitives,
    )
    .unwrap();
    let finalized = project_native_maintenance_finalized_anchor_v2(
        &admitted,
        &anchor_bytes,
        &mut codec,
        &NativeWirePrimitives,
    )
    .unwrap();

    assert_eq!(admitted.bytes, proof_bytes);
    assert_eq!(admitted.entries.len(), 5);
    assert_eq!(
        text(&finalized.value, "ctfState").as_deref(),
        Some("finalized")
    );
    assert_eq!(
        text(&finalized.value, "finalizationProofSha256").as_deref(),
        Some(admitted.digest.as_str())
    );
    assert_eq!(finalized.finalization_proof_sha256, admitted.digest);
    assert_eq!(
        codec.encode_canonical_json(&finalized.value).unwrap(),
        finalized.bytes
    );
    assert_eq!(
        codec.sha256(&finalized.bytes).unwrap().as_str(),
        finalized.digest
    );
}

/// Reject a canonical proof whose retirement population is not source ordered.
#[test]
fn rejects_out_of_order_retirement_population() {
    let mut codec = NativeWirePrimitives;
    let anchor = transferred_anchor();
    let anchor_bytes = codec.encode_canonical_json(&anchor).unwrap();
    let anchor_sha256 = codec.sha256(&anchor_bytes).unwrap().as_str().to_owned();
    let mut entries = retirement_entries();
    entries.reverse();
    let bytes = codec
        .encode_canonical_json(&proof(&anchor, &anchor_sha256, entries))
        .unwrap();

    assert_eq!(
        parse_native_maintenance_finalization_proof_v2(&bytes, &mut codec, &NativeWirePrimitives,)
            .unwrap_err(),
        NativeMaintenanceFinalizationProofV2Error::Proof
    );
}

/// Reject a valid proof document that differs from its transferred anchor authority.
#[test]
fn rejects_crossbinding_mismatch_before_projection() {
    let mut codec = NativeWirePrimitives;
    let anchor = transferred_anchor();
    let anchor_bytes = codec.encode_canonical_json(&anchor).unwrap();
    let anchor_sha256 = codec.sha256(&anchor_bytes).unwrap().as_str().to_owned();
    let mut proof = proof(&anchor, &anchor_sha256, retirement_entries());
    replace(&mut proof, "controlSha256", hash('f'));
    let proof_bytes = codec.encode_canonical_json(&proof).unwrap();
    let admitted = parse_native_maintenance_finalization_proof_v2(
        &proof_bytes,
        &mut codec,
        &NativeWirePrimitives,
    )
    .unwrap();

    assert_eq!(
        project_native_maintenance_finalized_anchor_v2(
            &admitted,
            &anchor_bytes,
            &mut codec,
            &NativeWirePrimitives,
        )
        .unwrap_err(),
        NativeMaintenanceFinalizationProofV2Error::Binding
    );
}

/// Build one complete source-valid transferred v2 anchor.
pub(crate) fn transferred_anchor() -> JsonValue {
    object(vec![
        hash_field("actionSha256", 'a'),
        hash_field("archiveObjectSha256", '1'),
        text_field("catalogGeneration", "1"),
        hash_field("catalogRootSha256", '2'),
        hash_field("catalogWatermarkSha256", '3'),
        text_field("contractVersion", "adjustment-maintenance-anchor/v2"),
        hash_field("controlSha256", '4'),
        text_field("controlVersion", "13"),
        text_field("ctfState", "transferred"),
        text_field("dueKey", "capture/2026-10-08T00:35:00.000Z"),
        number_field("frontierCount", 5.0),
        hash_field("frontierRootSha256", '5'),
        text_field("fullGraphVerifiedAt", "2026-10-08T00:42:00.700Z"),
        hash_field("fullMemberRootSha256", '6'),
        text_field("generation", "7"),
        hash_field("graphManifestSha256", '7'),
        hash_field("lifecycleLedgerRootSha256", '8'),
        hash_field("manifestSha256", '9'),
        (
            "predecessorAnchorSha256".encode_utf16().collect(),
            JsonValue::Null,
        ),
        text_field("publishedAt", "2026-10-08T00:43:00.000Z"),
        hash_field("reportSha256", 'a'),
        hash_field("revisionSnapshotRootSha256", 'b'),
        text_field("sequence", "0"),
        text_field("sourceCommit", &"c".repeat(40)),
        hash_field("workstationJournalHeadSha256", 'd'),
    ])
}

/// Build one complete canonical legacy finalization proof from admitted authority.
pub(crate) fn proof(
    anchor: &JsonValue,
    transferred_sha256: &str,
    retirement_entries: Vec<NativeMaintenanceRetirementEntryV2>,
) -> JsonValue {
    object(vec![
        clone_field(anchor, "actionSha256"),
        text_field("archiveCommitOrdinal", "12"),
        hash_field("burnSha256", 'e'),
        clone_field(anchor, "catalogGeneration"),
        hash_field("catalogRevisionReceiptRootSha256", 'f'),
        clone_field(anchor, "catalogRootSha256"),
        clone_field(anchor, "catalogWatermarkSha256"),
        number_field("confirmationChunkCount", 27.0),
        text_field(
            "contractVersion",
            "adjustment-maintenance-finalization-proof/v1",
        ),
        clone_field(anchor, "controlSha256"),
        clone_field(anchor, "controlVersion"),
        text_field("finalizedAt", "2026-10-08T00:44:00.000Z"),
        clone_field(anchor, "frontierCount"),
        clone_field(anchor, "frontierRootSha256"),
        clone_field(anchor, "fullMemberRootSha256"),
        clone_field(anchor, "graphManifestSha256"),
        hash_field("inputSealSha256", '0'),
        clone_field(anchor, "lifecycleLedgerRootSha256"),
        clone_field(anchor, "manifestSha256"),
        clone_field(anchor, "reportSha256"),
        hash_field("requiredInputRootSha256", '1'),
        (
            "retirementEntries".encode_utf16().collect(),
            JsonValue::Array(
                retirement_entries
                    .into_iter()
                    // retain the admitted source entry representation
                    .map(|entry| entry.to_json())
                    .collect(),
            ),
        ),
        hash_field("retirementSetRootSha256", '2'),
        clone_field(anchor, "revisionSnapshotRootSha256"),
        clone_field(anchor, "sourceCommit"),
        text_field("transferredAnchorSha256", transferred_sha256),
        clone_field(anchor, "workstationJournalHeadSha256"),
    ])
}

/// Build the minimum source population in exact kind and identity order.
pub(crate) fn retirement_entries() -> Vec<NativeMaintenanceRetirementEntryV2> {
    [
        (
            NativeMaintenanceRetirementKindV2::ArchiveAcknowledgement,
            '1',
        ),
        (NativeMaintenanceRetirementKindV2::CycleAcknowledgement, '2'),
        (NativeMaintenanceRetirementKindV2::FinalAcknowledgement, '3'),
        (NativeMaintenanceRetirementKindV2::FinalManifest, '4'),
        (NativeMaintenanceRetirementKindV2::InputSeal, '5'),
    ]
    .into_iter()
    // bind each ordered kind to an exact fixture identity and byte hash
    .map(|(kind, identity)| {
        NativeMaintenanceRetirementEntryV2::new(kind, hash(identity), hash('a')).unwrap()
    })
    .collect()
}

/// Clone one exact anchor field into the proof fixture.
fn clone_field(value: &JsonValue, key: &str) -> (Vec<u16>, JsonValue) {
    (
        key.encode_utf16().collect(),
        field(value, key).unwrap().clone(),
    )
}

/// Replace one exact fixture member without changing its object population.
pub(crate) fn replace(value: &mut JsonValue, key: &str, replacement: String) {
    let JsonValue::Object(fields) = value else {
        panic!("fixture object");
    };
    // update the single exact fixture member selected by name
    for (name, value) in fields {
        if name.iter().copied().eq(key.encode_utf16()) {
            *value = json_text(&replacement);
            return;
        }
    }
    panic!("missing fixture field");
}

/// Build a source JSON object without changing supplied field values.
fn object(fields: Vec<(Vec<u16>, JsonValue)>) -> JsonValue {
    JsonValue::Object(fields)
}

/// Build one lowercase hexadecimal SHA-256 fixture.
fn hash(value: char) -> String {
    value.to_string().repeat(64)
}

/// Build one named hash fixture field.
fn hash_field(name: &str, value: char) -> (Vec<u16>, JsonValue) {
    text_field(name, &hash(value))
}

/// Build one named string fixture field.
fn text_field(name: &str, value: &str) -> (Vec<u16>, JsonValue) {
    (name.encode_utf16().collect(), json_text(value))
}

/// Build one named finite number fixture field.
fn number_field(name: &str, value: f64) -> (Vec<u16>, JsonValue) {
    (name.encode_utf16().collect(), JsonValue::Number(value))
}

/// Build one exact UTF-16 source string.
fn json_text(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}
