use weather_contracts::JsonValue;

use super::{
    CHECKPOINT_KIND, NativeMaintenanceColdArchiveValidators, delegated_gap_or_genesis_kind,
};
use crate::capture_epoch_witness::GENESIS_FRONTIER_SHA256;
use crate::native_maintenance_cas_pack::{
    NativeMaintenanceCasMemberValidator, NativeMaintenanceCasStreamSession,
    NativeMaintenanceCasStreamValidator,
};
use crate::native_maintenance_revision_cold_drain::ADJUSTMENT_REVISION_COLD_CYCLE_MAXIMUM_BYTES;
use crate::{MaintenanceCanonicalJson, NativeWirePrimitives};

const CUTOFF: &str = "2026-11-01T07:00:00.000Z";

/// Validate one empty later-page checkpoint through buffer and chunked stream paths.
#[test]
fn validates_exact_cold_checkpoint_buffer_and_stream() {
    let bytes = checkpoint_bytes();
    let mut validators = NativeMaintenanceColdArchiveValidators::new();

    assert!(validators.validate(CHECKPOINT_KIND, &bytes));
    let mut session = validators
        .begin(CHECKPOINT_KIND, bytes.len() as u64)
        .expect("open checkpoint stream");
    let split = bytes.len() / 2;
    assert!(session.write(&bytes[..split]));
    assert!(session.write(&bytes[split..]));
    assert!(session.finish());
}

/// Delegate the exact genesis snapshot kind through the shared closed registry.
#[test]
fn delegates_only_registered_gap_or_genesis_kinds() {
    let bytes = genesis_snapshot_bytes();
    let mut validators = NativeMaintenanceColdArchiveValidators::new();

    assert!(delegated_gap_or_genesis_kind(
        "adjustment-revision-serving-snapshot/v1"
    ));
    assert!(validators.validate("adjustment-revision-serving-snapshot/v1", &bytes));
    assert!(!validators.validate("application/json", &bytes));
    assert!(
        validators
            .begin("application/json", bytes.len() as u64)
            .is_none()
    );
}

/// Refuse advertised cold members beyond their selected stream ceiling.
#[test]
fn refuses_oversized_or_unknown_streams_before_allocation() {
    let mut validators = NativeMaintenanceColdArchiveValidators::new();

    assert!(
        validators
            .begin(
                CHECKPOINT_KIND,
                ADJUSTMENT_REVISION_COLD_CYCLE_MAXIMUM_BYTES as u64 + 1,
            )
            .is_none()
    );
    assert!(validators.begin("unknown/cold-member", 1).is_none());
}

/// Refuse a canonical checkpoint whose compact member root is caller-selected.
#[test]
fn refuses_checkpoint_member_root_drift() {
    let bytes = checkpoint_bytes();
    let mut value = NativeWirePrimitives
        .parse_canonical_object(&bytes)
        .expect("parse checkpoint fixture");
    replace(&mut value, "memberRootSha256", string(&hash('9')));

    assert!(
        !NativeMaintenanceColdArchiveValidators::new()
            .validate(CHECKPOINT_KIND, &canonical(&value),)
    );
}

/// Build one source-valid empty later-page checkpoint.
fn checkpoint_bytes() -> Vec<u8> {
    let root = object(vec![
        member("entries", JsonValue::Array(Vec::new())),
        member("startMemberSha256", JsonValue::Null),
    ]);
    let member_root_sha256 = digest(&canonical(&root));
    canonical(&object(vec![
        member("afterArchiveCommitOrdinal", string("0")),
        member("afterFrontierSha256", string(&hash('1'))),
        member("contractVersion", string(CHECKPOINT_KIND)),
        member("entries", JsonValue::Array(Vec::new())),
        member("eof", JsonValue::Boolean(true)),
        member("memberRootSha256", string(&member_root_sha256)),
        member("nextArchiveCommitOrdinal", string("0")),
        member("nextFrontierSha256", string(&hash('1'))),
        member("pageSha256", string(&hash('2'))),
        member("previousPageSha256", string(&hash('3'))),
        member("startMemberSha256", JsonValue::Null),
        member("startSha256", string(&hash('4'))),
        member("watermarkArchiveCommitOrdinal", string("0")),
        member("watermarkFrontierSha256", string(&hash('1'))),
    ]))
}

/// Build one exact zero-frontier serving snapshot for delegated ancestry validation.
fn genesis_snapshot_bytes() -> Vec<u8> {
    let snapshot_sha256 = digest(
        format!(
            "adjustment-revision-serving-snapshot/v1\n{CUTOFF}\n0\n{GENESIS_FRONTIER_SHA256}\n"
        )
        .as_bytes(),
    );
    canonical(&object(vec![
        member("archiveCommitOrdinal", string("0")),
        member(
            "contractVersion",
            string("adjustment-revision-serving-snapshot/v1"),
        ),
        member("cutoffAt", string(CUTOFF)),
        member("entries", JsonValue::Array(Vec::new())),
        member("entryCount", JsonValue::Number(0.0)),
        member("frontierSha256", string(GENESIS_FRONTIER_SHA256)),
        member("snapshotSha256", string(&snapshot_sha256)),
    ]))
}

/// Replace one known fixture field without changing its key population.
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

/// Hash one exact fixture byte sequence through the production digest owner.
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
