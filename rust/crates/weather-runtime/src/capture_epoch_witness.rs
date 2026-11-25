//! verify one canonical future-only adjustment capture epoch witness

use weather_api::{
    AdmittedAdjustmentRevisionCaptureWitness, CanonicalInstant, CurrentTimeResolver,
};
use weather_contracts::{
    AdjustmentRevisionCaptureEpochWitness,
    AdjustmentRevisionCaptureEpochWitnessActivationKindLiteral,
    AdjustmentRevisionCaptureEpochWitnessArchiveCommitOrdinalLiteral,
    AdjustmentRevisionCaptureEpochWitnessContractVersionLiteral,
    AdjustmentRevisionCaptureEpochWitnessControlPlaneVersionEnum, JsDateOrString, JsonValue,
};

use crate::maintenance_canonical_json::{exact_keys, text, valid_hash};
use crate::{CAPTURE_EPOCH_WITNESS_MAX_BYTES, MaintenanceCanonicalJson};

pub(crate) const GENESIS_FRONTIER_SHA256: &str =
    "5d932b9623819be9432877a513194158708719497d755155f5ed9a4b501b48af";
const CONTROL_PLANE_V17_SHA256: &str =
    "6ea5cf5b770904bdfa557b29af2dd8899701d8f1b4a15b632f55ab5121032cbb";
const FINAL_V14_MIGRATION_HISTORY: &str =
    "6de5c8c7efaa448aeb12bf1a9debe6fe7d4d4d1003ee0e21ab619ffa624c3424";
const JS_TIMECLIP_MAX_MS: i64 = 8_640_000_000_000_000;
const WITNESS_KEYS: &[&str] = &[
    "activationKind",
    "archiveCommitOrdinal",
    "catalogFrontierSha256",
    "contractVersion",
    "controlPlaneSha256",
    "controlPlaneVersion",
    "databaseMigrationHistorySha256",
    "epochAt",
    "servingSnapshotSha256",
    "sourceCommit",
    "sourceRelease",
    "sourceServerImageDigest",
    "sourceWebImageDigest",
    "witnessSha256",
];
const CAPTURE_EPOCH_IDENTITIES: &[CaptureEpochIdentity] = &[
    CaptureEpochIdentity {
        control_plane_version: "14",
        database_migration_history_sha256: "c683c4f937c7f02b00f6ab49f75268eead81d2a221a8a9a23e38f9e4802a11b0",
        control_plane_sha256: None,
    },
    CaptureEpochIdentity {
        control_plane_version: "14",
        database_migration_history_sha256: FINAL_V14_MIGRATION_HISTORY,
        control_plane_sha256: None,
    },
    CaptureEpochIdentity {
        control_plane_version: "17",
        database_migration_history_sha256: "107ce430f1ac9478d54d94646da7fa4a54cbe7112db13efd7244d8894d0defdf",
        control_plane_sha256: Some(CONTROL_PLANE_V17_SHA256),
    },
    CaptureEpochIdentity {
        control_plane_version: "17",
        database_migration_history_sha256: "3aec6846d009a2bb2cee02279dceb9bd2d3572dc1b3a83eed2ce9a1724311345",
        control_plane_sha256: Some(CONTROL_PLANE_V17_SHA256),
    },
];

struct CaptureEpochIdentity {
    control_plane_version: &'static str,
    database_migration_history_sha256: &'static str,
    control_plane_sha256: Option<&'static str>,
}

/// one bounded witness admission failure without file or identity detail
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureEpochWitnessError {
    File,
    Canonical,
    Identity,
    Clock,
}

#[derive(Clone, Copy)]
enum WitnessSource {
    MountedApplication,
    Operator,
}

/// verify canonical witness bytes without conferring installed-file authority
pub fn verify_capture_epoch_witness(
    bytes: &[u8],
    codec: &mut impl MaintenanceCanonicalJson,
    time: &dyn CurrentTimeResolver,
) -> Result<AdmittedAdjustmentRevisionCaptureWitness, CaptureEpochWitnessError> {
    verify_witness(bytes, codec, time, WitnessSource::MountedApplication)
}

/// verify the operator witness using the original host ledger and release formats
pub(crate) fn verify_operator_capture_epoch_witness(
    bytes: &[u8],
    codec: &mut impl MaintenanceCanonicalJson,
    time: &dyn CurrentTimeResolver,
) -> Result<AdmittedAdjustmentRevisionCaptureWitness, CaptureEpochWitnessError> {
    verify_witness(bytes, codec, time, WitnessSource::Operator)
}

/// share the wire, hash and canonical date checks across the original witnesses
fn verify_witness(
    bytes: &[u8],
    codec: &mut impl MaintenanceCanonicalJson,
    time: &dyn CurrentTimeResolver,
    source: WitnessSource,
) -> Result<AdmittedAdjustmentRevisionCaptureWitness, CaptureEpochWitnessError> {
    // bound parsing before admitting any caller-owned material
    if !(2..=CAPTURE_EPOCH_WITNESS_MAX_BYTES).contains(&bytes.len()) {
        return Err(CaptureEpochWitnessError::Canonical);
    }
    let value = codec
        .parse_canonical_object(bytes)
        .ok_or(CaptureEpochWitnessError::Canonical)?;
    // require the closed object and its original sorted lf-terminated bytes
    if !exact_keys(&value, WITNESS_KEYS)
        || codec
            .encode_canonical_json(&value)
            .ok_or(CaptureEpochWitnessError::Canonical)?
            != bytes
    {
        return Err(CaptureEpochWitnessError::Canonical);
    }

    let activation_kind = required_text(&value, "activationKind")?;
    let archive_commit_ordinal = required_text(&value, "archiveCommitOrdinal")?;
    let catalog_frontier_sha256 = required_text(&value, "catalogFrontierSha256")?;
    let contract_version = required_text(&value, "contractVersion")?;
    let control_plane_sha256 = required_text(&value, "controlPlaneSha256")?;
    let control_plane_version = required_text(&value, "controlPlaneVersion")?;
    let database_migration_history_sha256 =
        required_text(&value, "databaseMigrationHistorySha256")?;
    let epoch_at = required_text(&value, "epochAt")?;
    let serving_snapshot_sha256 = required_text(&value, "servingSnapshotSha256")?;
    let source_commit = required_text(&value, "sourceCommit")?;
    let source_release = required_text(&value, "sourceRelease")?;
    let source_server_image_digest = required_text(&value, "sourceServerImageDigest")?;
    let source_web_image_digest = required_text(&value, "sourceWebImageDigest")?;
    let witness_sha256 = required_text(&value, "witnessSha256")?;

    // admit only one reviewed control-plane and migration-history tuple
    let identity_matches = CAPTURE_EPOCH_IDENTITIES.iter().any(|identity| {
        identity.control_plane_version == control_plane_version.as_str()
            && identity.database_migration_history_sha256
                == database_migration_history_sha256.as_str()
            && (!matches!(source, WitnessSource::Operator)
                || identity.control_plane_version != "14"
                || identity.database_migration_history_sha256 == FINAL_V14_MIGRATION_HISTORY)
            && identity
                .control_plane_sha256
                .is_none_or(|expected| expected == control_plane_sha256.as_str())
    });
    if activation_kind != AdjustmentRevisionCaptureEpochWitnessActivationKindLiteral::WIRE_VALUE
        || archive_commit_ordinal
            != AdjustmentRevisionCaptureEpochWitnessArchiveCommitOrdinalLiteral::WIRE_VALUE
        || catalog_frontier_sha256 != GENESIS_FRONTIER_SHA256
        || contract_version
            != AdjustmentRevisionCaptureEpochWitnessContractVersionLiteral::WIRE_VALUE
        || !identity_matches
    {
        return Err(CaptureEpochWitnessError::Identity);
    }

    // retain the wire grammar independently of the date resolver's accepted aliases
    let epoch = match source {
        WitnessSource::MountedApplication if utc_millisecond_shape(&epoch_at) => time
            .resolve(&JsDateOrString::String(epoch_at.encode_utf16().collect()))
            .ok(),
        WitnessSource::MountedApplication => None,
        WitnessSource::Operator => resolve_operator_capture_clock(&epoch_at, time),
    }
    .ok_or(CaptureEpochWitnessError::Clock)?;
    // close every source and digest spelling before recomputing the witness address
    if !valid_commit(&source_commit)
        || !valid_release(&source_release)
        || (matches!(source, WitnessSource::Operator) && source_release.len() > 13)
        || !valid_image_digest(&source_server_image_digest)
        || !valid_image_digest(&source_web_image_digest)
        || !valid_hash(&control_plane_sha256)
        || !valid_hash(&serving_snapshot_sha256)
        || !valid_hash(&witness_sha256)
    {
        return Err(CaptureEpochWitnessError::Identity);
    }

    let unsigned = unsigned_witness(&value)?;
    let unsigned_bytes = codec
        .encode_canonical_json(&unsigned)
        .ok_or(CaptureEpochWitnessError::Canonical)?;
    let computed = codec
        .sha256(&unsigned_bytes)
        .ok_or(CaptureEpochWitnessError::Canonical)?;
    if computed.as_str() != witness_sha256 {
        return Err(CaptureEpochWitnessError::Identity);
    }

    let control_plane_version = match control_plane_version.as_str() {
        "14" => AdjustmentRevisionCaptureEpochWitnessControlPlaneVersionEnum::Value14,
        "17" => AdjustmentRevisionCaptureEpochWitnessControlPlaneVersionEnum::Value17,
        _ => return Err(CaptureEpochWitnessError::Identity),
    };
    let witness = AdjustmentRevisionCaptureEpochWitness {
        activationKind: AdjustmentRevisionCaptureEpochWitnessActivationKindLiteral,
        archiveCommitOrdinal: AdjustmentRevisionCaptureEpochWitnessArchiveCommitOrdinalLiteral,
        catalogFrontierSha256: catalog_frontier_sha256,
        contractVersion: AdjustmentRevisionCaptureEpochWitnessContractVersionLiteral,
        controlPlaneSha256: control_plane_sha256,
        controlPlaneVersion: control_plane_version,
        databaseMigrationHistorySha256: database_migration_history_sha256,
        epochAt: epoch_at,
        servingSnapshotSha256: serving_snapshot_sha256,
        sourceCommit: source_commit,
        sourceRelease: source_release,
        sourceServerImageDigest: source_server_image_digest,
        sourceWebImageDigest: source_web_image_digest,
        witnessSha256: witness_sha256,
    };
    AdmittedAdjustmentRevisionCaptureWitness::from_verified(witness, epoch)
        .map_err(|_| CaptureEpochWitnessError::Clock)
}

/// decode one required witness string while preserving identity error ownership
fn required_text(value: &JsonValue, key: &str) -> Result<String, CaptureEpochWitnessError> {
    text(value, key).ok_or(CaptureEpochWitnessError::Identity)
}

/// Match the original four-digit UTC millisecond spelling before checking its calendar value.
fn utc_millisecond_shape(value: &str) -> bool {
    matches_digit_shape(value, b"dddd-dd-ddTdd:dd:dd.dddZ")
}

/// Match a fixed ASCII wire grammar before parsing any date components.
fn matches_digit_shape(value: &str, shape: &[u8]) -> bool {
    value.len() == shape.len()
        && shape
            .iter()
            .zip(value.bytes())
            .all(|(&expected, observed)| {
                // require decimal digits at each variable position
                if expected == b'd' {
                    observed.is_ascii_digit()
                } else {
                    observed == expected
                }
            })
}

/// Admit the original canonical ISO spelling and its valid Gregorian UTC date.
pub(crate) fn resolve_operator_capture_clock(
    value: &str,
    time: &dyn CurrentTimeResolver,
) -> Option<CanonicalInstant> {
    let year_width = match value.as_bytes() {
        [b'+' | b'-', ..] => 7,
        _ => 4,
    };
    let (year_text, date) = value.split_at_checked(year_width)?;
    let year_digits = match year_width {
        4 => year_text,
        _ => &year_text[1..],
    };
    // admit only canonical ordinary or signed six-digit years
    if !year_digits.bytes().all(|value| value.is_ascii_digit())
        || !matches_digit_shape(date, b"-dd-ddTdd:dd:dd.dddZ")
    {
        return None;
    }
    let year: i32 = year_text.parse().ok()?;
    // exclude redundant positive signs, negative zero and dates outside ECMAScript TimeClip
    if (year_width == 7 && (0..10_000).contains(&year))
        || !(-271_821..=275_760).contains(&year)
        || (year == -271_821 && date < "-04-20T00:00:00.000Z")
        || (year == 275_760 && date > "-09-13T00:00:00.000Z")
    {
        return None;
    }
    let month: u8 = date[1..3].parse().ok()?;
    let day: u8 = date[4..6].parse().ok()?;
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let maximum_day = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return None,
    };
    // reject normalized day overflow and 24:00 aliases before resolution
    if !(1..=maximum_day).contains(&day)
        || &date[7..9] > "23"
        || &date[10..12] > "59"
        || &date[13..15] > "59"
    {
        return None;
    }
    let instant = time
        .resolve(&JsDateOrString::String(value.encode_utf16().collect()))
        .ok()?;
    // retain only the exact clock and a finite JavaScript date result
    (instant.utc == value
        && (-JS_TIMECLIP_MAX_MS..=JS_TIMECLIP_MAX_MS).contains(&instant.epoch_milliseconds))
    .then_some(instant)
}

/// remove only the self-address before canonical witness hashing
fn unsigned_witness(value: &JsonValue) -> Result<JsonValue, CaptureEpochWitnessError> {
    let JsonValue::Object(fields) = value else {
        return Err(CaptureEpochWitnessError::Canonical);
    };
    // retain every original member except the witness digest itself
    Ok(JsonValue::Object(
        fields
            .iter()
            .filter(|(name, _)| !name.iter().copied().eq("witnessSha256".encode_utf16()))
            .cloned()
            .collect(),
    ))
}

/// recognize one lowercase hexadecimal source commit
fn valid_commit(value: &str) -> bool {
    value.len() == 40 && value.bytes().all(lower_hex)
}

/// recognize one immutable release with a one-to-three digit revision
fn valid_release(value: &str) -> bool {
    let bytes = value.as_bytes();
    // require the exact yyyy.mm.dd-revision punctuation and widths
    bytes.len() >= 12
        && bytes.len() <= 14
        && bytes[..4].iter().all(u8::is_ascii_digit)
        && bytes[4] == b'.'
        && bytes[5..7].iter().all(u8::is_ascii_digit)
        && bytes[7] == b'.'
        && bytes[8..10].iter().all(u8::is_ascii_digit)
        && bytes[10] == b'-'
        && matches!(bytes[11], b'1'..=b'9')
        && bytes[12..].iter().all(u8::is_ascii_digit)
}

/// recognize one lowercase sha256 container image digest
fn valid_image_digest(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(valid_hash)
}

/// recognize one lowercase hexadecimal byte
const fn lower_hex(byte: u8) -> bool {
    byte.is_ascii_digit() || matches!(byte, b'a'..=b'f')
}
