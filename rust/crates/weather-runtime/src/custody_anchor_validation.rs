//! Validate the two custody-only action anchors without granting model authority.

use weather_api::CurrentTimeResolver;
use weather_contracts::JsonValue;

use crate::custody_anchor_current::CustodyAnchorKind;
use crate::maintenance_anchor_status::{
    MaintenanceAnchorStatusJson, clock_field, hash_field, nullable_hash_field, regex_text,
    uint64_field, valid_control_version, valid_source_commit,
};
use crate::maintenance_canonical_json::{exact_keys, field, text};

const DEVELOPMENT_CONTRACT: &str = "adjustment-development-custody-anchor/v1";
const RAIN_CONTROL_CONTRACT: &str = "adjustment-rain-control-custody-anchor/v1";

const DEVELOPMENT_KEYS: &[&str] = &[
    "actionKind",
    "actionSha256",
    "archiveCommitOrdinal",
    "artifactSha256",
    "candidateGraphSha256",
    "candidateSha256",
    "captureEpochWitnessSha256",
    "contractVersion",
    "controlSha256",
    "controlVersion",
    "custodyAcknowledgedAt",
    "custodyAcknowledgementSha256",
    "custodyCheckpointSha256",
    "developmentGraphSha256",
    "dueKey",
    "family",
    "frontierSha256",
    "fullGraphVerifiedAt",
    "inputHeadSha256",
    "lifecycleLedgerRootSha256",
    "memberRootSha256",
    "pageSha256",
    "policyReportSha256",
    "predecessorAnchorSha256",
    "registrationSha256",
    "sequence",
    "sourceCommit",
    "sourceSha256",
    "startMemberSha256",
    "startSha256",
];
const DEVELOPMENT_HASH_KEYS: &[&str] = &[
    "actionSha256",
    "artifactSha256",
    "candidateGraphSha256",
    "candidateSha256",
    "captureEpochWitnessSha256",
    "controlSha256",
    "custodyAcknowledgementSha256",
    "custodyCheckpointSha256",
    "developmentGraphSha256",
    "frontierSha256",
    "inputHeadSha256",
    "lifecycleLedgerRootSha256",
    "memberRootSha256",
    "pageSha256",
    "policyReportSha256",
    "registrationSha256",
    "sourceSha256",
    "startSha256",
];
const RAIN_CONTROL_KEYS: &[&str] = &[
    "actionKind",
    "actionSha256",
    "archiveCommitOrdinal",
    "captureEpochWitnessSha256",
    "contractVersion",
    "controlSha256",
    "controlStateSha256",
    "controlVersion",
    "custodyAcknowledgedAt",
    "custodyAcknowledgementSha256",
    "custodyCheckpointSha256",
    "dueMonth",
    "fencingToken",
    "frontierSha256",
    "fullGraphVerifiedAt",
    "graphManifestSha256",
    "memberRootSha256",
    "ordinalArtifactSha256",
    "pageSha256",
    "predecessorAnchorSha256",
    "sequence",
    "sourceCommit",
    "sourceMemberRootSha256",
    "sourceReceiptRootSha256",
    "startMemberSha256",
    "startSha256",
    "workstationJournalHeadSha256",
];
const RAIN_CONTROL_HASH_KEYS: &[&str] = &[
    "actionSha256",
    "captureEpochWitnessSha256",
    "controlSha256",
    "controlStateSha256",
    "custodyAcknowledgementSha256",
    "custodyCheckpointSha256",
    "frontierSha256",
    "graphManifestSha256",
    "memberRootSha256",
    "ordinalArtifactSha256",
    "pageSha256",
    "sourceMemberRootSha256",
    "sourceReceiptRootSha256",
    "startSha256",
    "workstationJournalHeadSha256",
];
const NULLABLE_HASH_KEYS: &[&str] = &["predecessorAnchorSha256", "startMemberSha256"];

/// validate one exact development or rain-control custody anchor.
pub(crate) fn validate_custody_anchor(
    kind: CustodyAnchorKind,
    value: &JsonValue,
    codec: &mut impl MaintenanceAnchorStatusJson,
    time: &dyn CurrentTimeResolver,
) -> Option<()> {
    // retain the distinct closed schemas and action-specific identities
    match kind {
        CustodyAnchorKind::Development => validate_development(value, codec, time),
        CustodyAnchorKind::RainControl => validate_rain_control(value, codec, time),
    }
}

// validate one custody-only development action anchor
fn validate_development(
    value: &JsonValue,
    codec: &mut impl MaintenanceAnchorStatusJson,
    time: &dyn CurrentTimeResolver,
) -> Option<()> {
    exact_keys(value, DEVELOPMENT_KEYS).then_some(())?;
    let due_key = regex_text(value, "dueKey", codec)?;
    let due_family = monthly_due_family(&due_key)?;
    let family = text(value, "family")?;
    let control_version = regex_text(value, "controlVersion", codec)?;
    let source_commit = regex_text(value, "sourceCommit", codec)?;
    // keep development custody separate from terminal model authority
    (text(value, "contractVersion")?.as_str() == DEVELOPMENT_CONTRACT
        && text(value, "actionKind")?.as_str() == "shadow"
        && due_family == family
        && valid_control_version(&control_version)
        && valid_source_commit(&source_commit))
    .then_some(())?;
    uint64_field(value, "archiveCommitOrdinal")?;
    validate_shared_tail(value, DEVELOPMENT_HASH_KEYS, time)
}

// validate one custody-only rain control-reference action anchor
fn validate_rain_control(
    value: &JsonValue,
    codec: &mut impl MaintenanceAnchorStatusJson,
    time: &dyn CurrentTimeResolver,
) -> Option<()> {
    exact_keys(value, RAIN_CONTROL_KEYS).then_some(())?;
    let due_month = regex_text(value, "dueMonth", codec)?;
    let control_version = regex_text(value, "controlVersion", codec)?;
    let source_commit = regex_text(value, "sourceCommit", codec)?;
    // keep control-reference custody separate from terminal model authority
    (text(value, "contractVersion")?.as_str() == RAIN_CONTROL_CONTRACT
        && text(value, "actionKind")?.as_str() == "control_reference"
        && valid_rain_due_month(&due_month)
        && valid_control_version(&control_version)
        && valid_source_commit(&source_commit))
    .then_some(())?;
    uint64_field(value, "archiveCommitOrdinal")?;
    uint64_field(value, "fencingToken")?;
    validate_shared_tail(value, RAIN_CONTROL_HASH_KEYS, time)
}

// validate the clocks, identities and first-sequence predecessor relation shared by both anchors
fn validate_shared_tail(
    value: &JsonValue,
    hash_keys: &[&str],
    time: &dyn CurrentTimeResolver,
) -> Option<()> {
    let sequence = uint64_field(value, "sequence")?;
    let custody_at = clock_field(value, "custodyAcknowledgedAt", time)?;
    let verified_at = clock_field(value, "fullGraphVerifiedAt", time)?;
    // bind every action, graph and packed-custody identity
    hash_keys
        .iter()
        .all(|key| hash_field(value, key))
        .then_some(())?;
    // permit nullable identities only at their genuine first boundaries
    NULLABLE_HASH_KEYS
        .iter()
        .all(|key| nullable_hash_field(value, key))
        .then_some(())?;
    let predecessor_absent = matches!(
        field(value, "predecessorAnchorSha256"),
        Some(JsonValue::Null)
    );
    ((sequence == 0) == predecessor_absent && verified_at >= custody_at).then_some(())
}

// extract the family from the original monthly due-key regex without calendar normalization
fn monthly_due_family(value: &str) -> Option<&str> {
    let suffix = value.strip_prefix("monthly/")?;
    let (family, month) = suffix.split_once('/')?;
    // retain the exact regex shape rather than validating a real calendar month
    (matches!(family, "temperature" | "wind" | "rain")
        && month.len() == 7
        && month
            .bytes()
            .enumerate()
            .all(|(index, byte)| index == 4 && byte == b'-' || index != 4 && byte.is_ascii_digit()))
    .then_some(family)
}

// match the original 20xx and 01-through-12 rain month grammar
pub(crate) fn valid_rain_due_month(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 7
        && bytes[0..2] == *b"20"
        && bytes[2..4].iter().all(u8::is_ascii_digit)
        && bytes[4] == b'-'
        && matches!(
            &bytes[5..7],
            b"01"
                | b"02"
                | b"03"
                | b"04"
                | b"05"
                | b"06"
                | b"07"
                | b"08"
                | b"09"
                | b"10"
                | b"11"
                | b"12"
        )
}
