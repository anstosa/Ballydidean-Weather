use weather_contracts::JsonValue;

use super::{
    NativeMaintenanceColdRemoteDocumentError,
    verify_native_revision_cold_custody_acknowledgement_v2_document,
    verify_native_revision_cold_start_document,
};
use crate::native_maintenance_revision_cold_drain::{
    NativeRevisionColdCustodyRequest, NativeRevisionColdPageRequest,
};
use crate::{MaintenanceCanonicalJson, NativeWirePrimitives};

const CUTOFF: &str = "2026-11-01T07:00:00.000Z";

/// Admit one exact empty snapshot start and retain its canonical source bytes.
#[test]
fn verifies_canonical_cold_start_and_nested_snapshot_identity() {
    let bytes = cold_start_bytes();
    let verified = verify_native_revision_cold_start_document(
        &bytes,
        &mut NativeWirePrimitives,
        &NativeWirePrimitives,
    )
    .expect("verify cold start");

    assert_eq!(verified.cutoff_at, CUTOFF);
    assert_eq!(verified.watermark_archive_commit_ordinal, "0");
    assert_eq!(verified.canonical_bytes, bytes);
    assert_eq!(verified.start_sha256.len(), 64);
}

/// Refuse a validly shaped start whose watermark differs from its snapshot.
#[test]
fn refuses_cold_start_watermark_drift() {
    let bytes = cold_start_bytes();
    let mut value = NativeWirePrimitives
        .parse_canonical_object(&bytes)
        .expect("parse start fixture");
    replace(&mut value, "watermarkArchiveCommitOrdinal", string("1"));
    let changed = canonical(&value);

    assert_eq!(
        verify_native_revision_cold_start_document(
            &changed,
            &mut NativeWirePrimitives,
            &NativeWirePrimitives,
        ),
        Err(NativeMaintenanceColdRemoteDocumentError::Binding),
    );
}

/// Admit one self-addressed v2 acknowledgement bound to every request coordinate.
#[test]
fn verifies_canonical_v2_acknowledgement_and_request_binding() {
    let request = custody_request();
    let bytes = acknowledgement_bytes(&request);
    let verified = verify_native_revision_cold_custody_acknowledgement_v2_document(
        &bytes,
        &request,
        &mut NativeWirePrimitives,
        &NativeWirePrimitives,
    )
    .expect("verify custody acknowledgement");

    assert_eq!(verified.next_archive_commit_ordinal, "1");
    assert_eq!(verified.next_frontier_sha256, hash('3'));
    assert_eq!(verified.request, request);
    assert_eq!(verified.canonical_bytes, bytes);
}

/// Refuse an acknowledgement when its typed request would conceal another page identity.
#[test]
fn refuses_v2_acknowledgement_request_drift() {
    let request = custody_request();
    let bytes = acknowledgement_bytes(&request);
    let mut changed = request.clone();
    changed.page_sha256 = hash('9');

    assert_eq!(
        verify_native_revision_cold_custody_acknowledgement_v2_document(
            &bytes,
            &changed,
            &mut NativeWirePrimitives,
            &NativeWirePrimitives,
        ),
        Err(NativeMaintenanceColdRemoteDocumentError::Binding),
    );
}

/// Build one source-valid empty serving snapshot and its addressed start envelope.
fn cold_start_bytes() -> Vec<u8> {
    let frontier = hash('f');
    let snapshot_identity = digest(
        format!("adjustment-revision-serving-snapshot/v1\n{CUTOFF}\n0\n{frontier}\n").as_bytes(),
    );
    let snapshot = object(vec![
        member("archiveCommitOrdinal", string("0")),
        member(
            "contractVersion",
            string("adjustment-revision-serving-snapshot/v1"),
        ),
        member("cutoffAt", string(CUTOFF)),
        member("entries", JsonValue::Array(Vec::new())),
        member("entryCount", JsonValue::Number(0.0)),
        member("frontierSha256", string(&frontier)),
        member("snapshotSha256", string(&snapshot_identity)),
    ]);
    let unsigned = object(vec![
        member(
            "contractVersion",
            string("adjustment-revision-cold-transfer-start/v1"),
        ),
        member("servingSnapshot", snapshot.clone()),
        member("watermarkArchiveCommitOrdinal", string("0")),
        member("watermarkFrontierSha256", string(&frontier)),
    ]);
    let start_sha256 = digest(&canonical(&unsigned));
    object_bytes_with_address(unsigned, "startSha256", &start_sha256)
}

/// Build one complete archived-page request with exact source coordinates.
fn custody_request() -> NativeRevisionColdCustodyRequest {
    NativeRevisionColdCustodyRequest {
        page: NativeRevisionColdPageRequest {
            after_archive_commit_ordinal: "0".to_owned(),
            after_frontier_sha256: hash('a'),
            previous_page_sha256: hash('b'),
            start_sha256: hash('c'),
            watermark_archive_commit_ordinal: "2".to_owned(),
            watermark_frontier_sha256: hash('d'),
        },
        custody_checkpoint_sha256: hash('e'),
        member_root_sha256: hash('f'),
        page_sha256: hash('1'),
        start_member_sha256: hash('2'),
    }
}

/// Build one source-valid packed-custody acknowledgement for the supplied request.
fn acknowledgement_bytes(request: &NativeRevisionColdCustodyRequest) -> Vec<u8> {
    let unsigned = object(vec![
        member("acknowledgedAt", string("2026-11-01T08:00:00.000Z")),
        member(
            "afterArchiveCommitOrdinal",
            string(&request.page.after_archive_commit_ordinal),
        ),
        member(
            "afterFrontierSha256",
            string(&request.page.after_frontier_sha256),
        ),
        member("authority", string("cold_custody_only")),
        member(
            "contractVersion",
            string("adjustment-revision-custody-acknowledgement/v2"),
        ),
        member(
            "custodyCheckpointSha256",
            string(&request.custody_checkpoint_sha256),
        ),
        member("memberRootSha256", string(&request.member_root_sha256)),
        member("nextArchiveCommitOrdinal", string("1")),
        member("nextFrontierSha256", string(&hash('3'))),
        member("pageSha256", string(&request.page_sha256)),
        member("previousAcknowledgementSha256", JsonValue::Null),
        member(
            "previousPageSha256",
            string(&request.page.previous_page_sha256),
        ),
        member(
            "retirementEntries",
            JsonValue::Array(vec![
                retirement("revision_commit_receipt", '4'),
                retirement("revision_frontier_successor", '5'),
                retirement("revision_projection", '6'),
                retirement("revision_stage_receipt", '7'),
            ]),
        ),
        member("startMemberSha256", string(&request.start_member_sha256)),
        member("startSha256", string(&request.page.start_sha256)),
        member(
            "watermarkArchiveCommitOrdinal",
            string(&request.page.watermark_archive_commit_ordinal),
        ),
        member(
            "watermarkFrontierSha256",
            string(&request.page.watermark_frontier_sha256),
        ),
    ]);
    let acknowledgement_sha256 = digest(&canonical(&unsigned));
    object_bytes_with_address(unsigned, "acknowledgementSha256", &acknowledgement_sha256)
}

/// Build one fixed retirement address with distinct file and semantic identities.
fn retirement(kind: &str, character: char) -> JsonValue {
    object(vec![
        member("fileSha256", string(&hash(character))),
        member("identitySha256", string(&hash(character))),
        member("kind", string(kind)),
    ])
}

/// Add one self-address member to an unsigned fixture and encode it canonically.
fn object_bytes_with_address(unsigned: JsonValue, name: &str, address: &str) -> Vec<u8> {
    let JsonValue::Object(mut fields) = unsigned else {
        panic!("fixture object");
    };
    fields.push(member(name, string(address)));
    canonical(&JsonValue::Object(fields))
}

/// Replace one known fixture object member without changing its key population.
fn replace(value: &mut JsonValue, name: &str, replacement: JsonValue) {
    let JsonValue::Object(fields) = value else {
        panic!("fixture object");
    };
    let (_, field) = fields
        .iter_mut()
        .find(|(key, _)| key.iter().copied().eq(name.encode_utf16()))
        .expect("fixture field");
    *field = replacement;
}

/// Encode one fixture through the production canonical framing owner.
fn canonical(value: &JsonValue) -> Vec<u8> {
    NativeWirePrimitives
        .encode_canonical_json(value)
        .expect("encode canonical fixture")
}

/// Hash one fixture using the same production digest owner.
fn digest(bytes: &[u8]) -> String {
    NativeWirePrimitives
        .sha256(bytes)
        .expect("hash fixture")
        .as_str()
        .to_owned()
}

/// Construct one UTF-16 JSON object.
fn object(fields: Vec<(Vec<u16>, JsonValue)>) -> JsonValue {
    JsonValue::Object(fields)
}

/// Construct one UTF-16 JSON object member.
fn member(name: &str, value: JsonValue) -> (Vec<u16>, JsonValue) {
    (name.encode_utf16().collect(), value)
}

/// Construct one UTF-16 JSON string.
fn string(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}

/// Produce one lowercase digest-shaped fixture identity.
fn hash(character: char) -> String {
    character.to_string().repeat(64)
}
