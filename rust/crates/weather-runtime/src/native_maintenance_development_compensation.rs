//! Pure source-exact package material for a paired development-shadow compensation release.

use std::fmt::{self, Display, Formatter};

use weather_contracts::JsonValue;
use weather_edge::{ArchiveWirePrimitives, RevisionArchiveCanonicalCodec};

use crate::maintenance_canonical_json::{exact_keys, field, text, valid_hash};
use crate::native_maintenance_policy_comparison::{object, string};
use crate::native_maintenance_retirement_files_v2::format_digest;
use crate::{
    NativeMaintenanceCaptureFamily, NativeMaintenanceDevelopmentPublicationFile,
    NativeWirePrimitives,
};

const ACTION_VERSION: &str = "forecast-adjustment-model-action/v1";
const PACKAGE_MAXIMUM_BYTES: usize = 8 * 1_024 * 1_024;
const DAY_MILLISECONDS: i64 = 86_400_000;
const ACTION_KEYS: &[&str] = &[
    "actionKind",
    "candidateGraphSha256",
    "candidateSha256",
    "contractVersion",
    "createdAt",
    "expectedSettingsSha256",
    "expectedInstalledReceiptSha256",
    "expectedSourceCommit",
    "expectedSourceRelease",
    "family",
    "fencingToken",
    "fullMemberRootSha256",
    "lifecycleHeadSha256",
    "policyDecision",
    "policyReportSha256",
    "predecessorActionSha256",
    "reason",
    "reportCreatedAt",
    "siteKey",
    "validThrough",
];

/// Already published target coordinates required to build its paired compensation.
#[derive(Clone, Copy, Debug)]
pub struct NativeMaintenanceDevelopmentCompensationTarget<'a> {
    pub action: &'a JsonValue,
    pub action_bytes: &'a [u8],
    pub action_sha256: &'a str,
    pub commit_sha: &'a str,
    pub release_tag: &'a str,
}

/// Exact post-target clock and immutable publication coordinates.
#[derive(Clone, Copy, Debug)]
pub struct NativeMaintenanceDevelopmentCompensationInput<'a> {
    pub compensation_created_at: &'a str,
    pub target: NativeMaintenanceDevelopmentCompensationTarget<'a>,
}

/// Target and predecessor identities retained across the paired compensation boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeMaintenanceDevelopmentCompensationTargetIdentity {
    pub action_sha256: String,
    pub commit_sha: String,
    pub expected_installed_receipt_sha256: Option<String>,
    pub original_expected_settings_sha256: String,
    pub original_expected_source_commit: String,
    pub original_expected_source_release: String,
    pub release_tag: String,
    pub target_identity_sha256: String,
}

/// Complete action and package material before any Git or remote operation.
#[derive(Clone, Debug, PartialEq)]
pub struct NativeMaintenanceDevelopmentCompensation {
    pub action: JsonValue,
    pub action_bytes: Vec<u8>,
    pub action_path: String,
    pub action_sha256: String,
    pub family: NativeMaintenanceCaptureFamily,
    pub fencing_token: String,
    pub package_files: Vec<NativeMaintenanceDevelopmentPublicationFile>,
    pub package_root_sha256: String,
    pub package_total_bytes: usize,
    pub removed_paths: Vec<String>,
    pub source_commit: String,
    pub target: NativeMaintenanceDevelopmentCompensationTargetIdentity,
}

/// Closed target, action, clock, or package refusal category.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMaintenanceDevelopmentCompensationError {
    Target,
    Clock,
    Action,
    Package,
    Codec,
}

impl Display for NativeMaintenanceDevelopmentCompensationError {
    /// Render one stable path-free compensation material failure.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        // keep source target, action, and package failures distinguishable
        formatter.write_str(match self {
            Self::Target => "development compensation target is invalid",
            Self::Clock => "development compensation clock is invalid",
            Self::Action => "development compensation action is invalid",
            Self::Package => "development compensation package is invalid",
            Self::Codec => "development compensation serialization failed",
        })
    }
}

impl std::error::Error for NativeMaintenanceDevelopmentCompensationError {}

/// Build the exact compensate-shadow action and its one-file public package.
pub fn build_native_maintenance_development_compensation(
    input: NativeMaintenanceDevelopmentCompensationInput<'_>,
) -> Result<NativeMaintenanceDevelopmentCompensation, NativeMaintenanceDevelopmentCompensationError>
{
    use NativeMaintenanceDevelopmentCompensationError as Error;

    let wire = NativeWirePrimitives;
    let target = admit_target(&wire, input.target)?;
    let created_at = wire
        .canonical_utc_milliseconds(input.compensation_created_at)
        .ok_or(Error::Clock)?;
    // compensation remains recoverable after validity expiry but never predates its report
    if created_at < target.report_created_at {
        return Err(Error::Clock);
    }
    let action = object(vec![
        ("actionKind", string("compensate_shadow")),
        (
            "candidateGraphSha256",
            string(&target.candidate_graph_sha256),
        ),
        ("candidateSha256", string(&target.candidate_sha256)),
        ("contractVersion", string(ACTION_VERSION)),
        ("createdAt", string(input.compensation_created_at)),
        (
            "expectedInstalledReceiptSha256",
            target
                .expected_installed_receipt_sha256
                .as_deref()
                .map_or(JsonValue::Null, string),
        ),
        (
            "expectedSettingsSha256",
            string(&target.expected_settings_sha256),
        ),
        ("expectedSourceCommit", string(input.target.commit_sha)),
        ("expectedSourceRelease", string(input.target.release_tag)),
        ("family", string(target.family.wire())),
        ("fencingToken", string(&target.fencing_token)),
        ("fullMemberRootSha256", JsonValue::Null),
        ("lifecycleHeadSha256", string(&target.lifecycle_head_sha256)),
        ("policyDecision", string("pending")),
        ("policyReportSha256", string(&target.policy_report_sha256)),
        (
            "predecessorActionSha256",
            string(input.target.action_sha256),
        ),
        ("reason", string("development_shadow_failure")),
        ("reportCreatedAt", string(&target.report_created_at_text)),
        ("siteKey", string("ballydidean")),
        ("validThrough", string(&target.valid_through)),
    ]);
    validate_compensation_action(&wire, &action, input.target.action_sha256)?;
    let action_bytes = wire.encode_sorted_json(&action).ok_or(Error::Codec)?;
    let action_sha256 = format_digest(wire.sha256(&action_bytes));
    let action_path =
        format!("config/forecast-adjustments/ballydidean/actions/sha256-{action_sha256}.json");
    let package_files = vec![NativeMaintenanceDevelopmentPublicationFile {
        path: action_path.clone(),
        bytes: action_bytes.clone(),
    }];
    let package_total_bytes = action_bytes.len();
    // preserve the source package bounds before constructing its operations identity
    if package_total_bytes == 0 || package_total_bytes > PACKAGE_MAXIMUM_BYTES {
        return Err(Error::Package);
    }
    let operations = JsonValue::Array(vec![object(vec![
        ("bytes", JsonValue::Number(package_total_bytes as f64)),
        ("path", string(&action_path)),
        ("sha256", string(&action_sha256)),
    ])]);
    let operations_bytes = wire.encode_sorted_json(&operations).ok_or(Error::Codec)?;
    let package_root_sha256 = format_digest(wire.sha256(&operations_bytes));
    Ok(NativeMaintenanceDevelopmentCompensation {
        action,
        action_bytes,
        action_path,
        action_sha256,
        family: target.family,
        fencing_token: target.fencing_token,
        package_files,
        package_root_sha256,
        package_total_bytes,
        removed_paths: Vec::new(),
        source_commit: input.target.commit_sha.to_owned(),
        target: NativeMaintenanceDevelopmentCompensationTargetIdentity {
            action_sha256: input.target.action_sha256.to_owned(),
            commit_sha: input.target.commit_sha.to_owned(),
            expected_installed_receipt_sha256: target.expected_installed_receipt_sha256,
            original_expected_settings_sha256: target.expected_settings_sha256,
            original_expected_source_commit: target.expected_source_commit,
            original_expected_source_release: target.expected_source_release,
            release_tag: input.target.release_tag.to_owned(),
            target_identity_sha256: target.candidate_sha256,
        },
    })
}

struct AdmittedTarget {
    candidate_graph_sha256: String,
    candidate_sha256: String,
    expected_installed_receipt_sha256: Option<String>,
    expected_settings_sha256: String,
    expected_source_commit: String,
    expected_source_release: String,
    family: NativeMaintenanceCaptureFamily,
    fencing_token: String,
    lifecycle_head_sha256: String,
    policy_report_sha256: String,
    report_created_at: i64,
    report_created_at_text: String,
    valid_through: String,
}

// admit one exact ordinary shadow action and its published target coordinates
fn admit_target(
    wire: &NativeWirePrimitives,
    target: NativeMaintenanceDevelopmentCompensationTarget<'_>,
) -> Result<AdmittedTarget, NativeMaintenanceDevelopmentCompensationError> {
    use NativeMaintenanceDevelopmentCompensationError::Target;

    // require exact canonical target bytes and their public action identity
    if !valid_hash(target.action_sha256)
        || wire.encode_sorted_json(target.action).as_deref() != Some(target.action_bytes)
        || format_digest(wire.sha256(target.action_bytes)) != target.action_sha256
        || !exact_keys(target.action, ACTION_KEYS)
        || text(target.action, "contractVersion").as_deref() != Some(ACTION_VERSION)
        || text(target.action, "actionKind").as_deref() != Some("shadow")
        || text(target.action, "siteKey").as_deref() != Some("ballydidean")
        || text(target.action, "policyDecision").as_deref() != Some("pending")
        || text(target.action, "reason").as_deref() != Some("development_candidate")
        || !valid_commit(target.commit_sha)
        || !valid_release(target.release_tag)
    {
        return Err(Target);
    }
    let candidate_graph_sha256 = required_hash(target.action, "candidateGraphSha256")?;
    let candidate_sha256 = required_hash(target.action, "candidateSha256")?;
    let expected_installed_receipt_sha256 =
        nullable_hash(target.action, "expectedInstalledReceiptSha256")?;
    let expected_settings_sha256 = required_hash(target.action, "expectedSettingsSha256")?;
    let expected_source_commit = text(target.action, "expectedSourceCommit").ok_or(Target)?;
    let expected_source_release = text(target.action, "expectedSourceRelease").ok_or(Target)?;
    let family = family(target.action)?;
    let fencing_token = text(target.action, "fencingToken").ok_or(Target)?;
    let lifecycle_head_sha256 = required_hash(target.action, "lifecycleHeadSha256")?;
    let policy_report_sha256 = required_hash(target.action, "policyReportSha256")?;
    let created_at_text = text(target.action, "createdAt").ok_or(Target)?;
    let report_created_at_text = text(target.action, "reportCreatedAt").ok_or(Target)?;
    let valid_through = text(target.action, "validThrough").ok_or(Target)?;
    let created_at = wire
        .canonical_utc_milliseconds(&created_at_text)
        .ok_or(Target)?;
    let report_created_at = wire
        .canonical_utc_milliseconds(&report_created_at_text)
        .ok_or(Target)?;
    let through = wire
        .canonical_utc_milliseconds(&valid_through)
        .ok_or(Target)?;
    // bind the ordinary target chronology, nullable fields, fence, and original source
    if report_created_at > created_at
        || created_at > through
        || through.checked_sub(report_created_at) != Some(7 * DAY_MILLISECONDS)
        || !valid_commit(&expected_source_commit)
        || !valid_release(&expected_source_release)
        || !valid_fencing_token(&fencing_token)
        || !matches!(
            field(target.action, "fullMemberRootSha256"),
            Some(JsonValue::Null)
        )
        || !matches!(
            field(target.action, "predecessorActionSha256"),
            Some(JsonValue::Null)
        )
    {
        return Err(Target);
    }
    Ok(AdmittedTarget {
        candidate_graph_sha256,
        candidate_sha256,
        expected_installed_receipt_sha256,
        expected_settings_sha256,
        expected_source_commit,
        expected_source_release,
        family,
        fencing_token,
        lifecycle_head_sha256,
        policy_report_sha256,
        report_created_at,
        report_created_at_text,
        valid_through,
    })
}

// validate the constructed compensation against the source action matrix
fn validate_compensation_action(
    wire: &NativeWirePrimitives,
    action: &JsonValue,
    predecessor_action_sha256: &str,
) -> Result<(), NativeMaintenanceDevelopmentCompensationError> {
    use NativeMaintenanceDevelopmentCompensationError::Action;
    let candidate = required_hash(action, "candidateSha256")?;
    let graph = required_hash(action, "candidateGraphSha256")?;
    let report = wire
        .canonical_utc_milliseconds(&text(action, "reportCreatedAt").ok_or(Action)?)
        .ok_or(Action)?;
    let created = wire
        .canonical_utc_milliseconds(&text(action, "createdAt").ok_or(Action)?)
        .ok_or(Action)?;
    let through = wire
        .canonical_utc_milliseconds(&text(action, "validThrough").ok_or(Action)?)
        .ok_or(Action)?;
    // require the complete development-only compensation tuple and unchanged candidate pair
    if candidate.is_empty()
        || graph.is_empty()
        || text(action, "actionKind").as_deref() != Some("compensate_shadow")
        || text(action, "policyDecision").as_deref() != Some("pending")
        || text(action, "reason").as_deref() != Some("development_shadow_failure")
        || text(action, "predecessorActionSha256").as_deref() != Some(predecessor_action_sha256)
        || !matches!(field(action, "fullMemberRootSha256"), Some(JsonValue::Null))
        || report > created
        || through.checked_sub(report) != Some(7 * DAY_MILLISECONDS)
    {
        return Err(Action);
    }
    Ok(())
}

// decode one required lowercase hash member
fn required_hash(
    value: &JsonValue,
    key: &str,
) -> Result<String, NativeMaintenanceDevelopmentCompensationError> {
    let hash = text(value, key).ok_or(NativeMaintenanceDevelopmentCompensationError::Target)?;
    valid_hash(&hash)
        .then_some(hash)
        .ok_or(NativeMaintenanceDevelopmentCompensationError::Target)
}

// decode one explicit null or lowercase hash member
fn nullable_hash(
    value: &JsonValue,
    key: &str,
) -> Result<Option<String>, NativeMaintenanceDevelopmentCompensationError> {
    use NativeMaintenanceDevelopmentCompensationError::Target;
    // preserve explicit absence separately from a missing or mistyped member
    match field(value, key) {
        Some(JsonValue::Null) => Ok(None),
        Some(JsonValue::String(units)) => {
            let hash = String::from_utf16(units).map_err(|_| Target)?;
            valid_hash(&hash).then_some(Some(hash)).ok_or(Target)
        }
        _ => Err(Target),
    }
}

// decode one exact adjustment family
fn family(
    value: &JsonValue,
) -> Result<NativeMaintenanceCaptureFamily, NativeMaintenanceDevelopmentCompensationError> {
    // retain the source's closed family set without aliases
    match text(value, "family").as_deref() {
        Some("temperature") => Ok(NativeMaintenanceCaptureFamily::Temperature),
        Some("wind") => Ok(NativeMaintenanceCaptureFamily::Wind),
        Some("rain") => Ok(NativeMaintenanceCaptureFamily::Rain),
        _ => Err(NativeMaintenanceDevelopmentCompensationError::Target),
    }
}

// recognize one exact lowercase forty-character Git commit
fn valid_commit(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

// recognize one immutable calendar release tag
fn valid_release(value: &str) -> bool {
    let bytes = value.as_bytes();
    let revision = bytes.get(11..).unwrap_or_default();
    bytes.len() >= 12
        && bytes.get(4) == Some(&b'.')
        && bytes.get(7) == Some(&b'.')
        && bytes.get(10) == Some(&b'-')
        && bytes[..4].iter().all(u8::is_ascii_digit)
        && bytes[5..7].iter().all(u8::is_ascii_digit)
        && bytes[8..10].iter().all(u8::is_ascii_digit)
        && matches!(revision.first(), Some(b'1'..=b'9'))
        && revision.iter().all(u8::is_ascii_digit)
}

// recognize one canonical positive unsigned 64-bit fence
fn valid_fencing_token(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 20
        && matches!(value.as_bytes().first(), Some(b'1'..=b'9'))
        && value.bytes().all(|byte| byte.is_ascii_digit())
        && value.parse::<u64>().is_ok()
}

#[cfg(test)]
#[path = "native_maintenance_development_compensation_test.rs"]
mod tests;
