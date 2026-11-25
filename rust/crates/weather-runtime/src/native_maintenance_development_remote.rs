//! Closed native transport material for development custody installation and family application.

use std::fmt::{self, Display, Formatter};
use std::io;

use weather_contracts::JsonValue;
use weather_edge::RevisionArchiveCanonicalCodec;

use crate::custody_anchor_current::CustodyAnchorKind;
use crate::custody_anchor_validation::validate_custody_anchor;
use crate::maintenance_canonical_json::{exact_keys, field, text, valid_hash};
use crate::native_maintenance_retirement_files_v2::format_digest;
use crate::{
    MaintenanceCanonicalJson, NativeMaintenanceDatabaseLedgerReader, NativeWirePrimitives,
};

const ANCHOR_INSTALL_COMMAND: &str = "adjustment-development-custody-anchor-install-v1";
const ANCHOR_INSTALLATION_VERSION: &str = "adjustment-development-custody-anchor-installation/v1";
const FAMILY_APPLY_COMMAND: &str = "adjustment-family-release";
const FAMILY_STATUS_VERSION: &str = "adjustment-family-release-status/v1";
const ANCHOR_INSTALLATION_KEYS: &[&str] = &["anchorSha256", "contractVersion", "state"];
const FAMILY_STATUS_KEYS: &[&str] = &[
    "actionSha256",
    "compensatingRelease",
    "compensationState",
    "contractVersion",
    "family",
    "fencingToken",
    "outcome",
    "state",
    "targetRelease",
];

/// Canonical custody-anchor bytes and the exact content-addressed remote operand.
#[derive(Clone, Debug, PartialEq)]
pub struct NativeMaintenanceDevelopmentAnchorInstallRequest {
    pub anchor: JsonValue,
    pub canonical_bytes: Vec<u8>,
    pub anchor_sha256: String,
}

/// Verified acknowledgement for the exact custody-anchor bytes submitted by the caller.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeMaintenanceDevelopmentAnchorInstallation {
    pub anchor_sha256: String,
    pub canonical_bytes: Vec<u8>,
}

/// Typed caller material for the source's fixed nine-operand family operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NativeMaintenanceFamilyReleaseApplyInput<'a> {
    pub action_sha256: &'a str,
    pub compensating_release: &'a str,
    pub expected_current_release: &'a str,
    pub expected_settings_sha256: &'a str,
    pub expected_source_release: &'a str,
    pub family: &'a str,
    pub fencing_token: &'a str,
    pub report_sha256: &'a str,
    pub target_release: &'a str,
}

/// Closed validated operands in the exact source transport order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeMaintenanceFamilyReleaseApplyRequest {
    action_sha256: String,
    arguments: [String; 9],
}

impl NativeMaintenanceFamilyReleaseApplyRequest {
    /// Borrow all nine operands in the predecessor's forced-command order.
    pub fn arguments(&self) -> [&str; 9] {
        self.arguments.each_ref().map(String::as_str)
    }

    /// Borrow the immutable action identity used to bind the returned status.
    pub fn action_sha256(&self) -> &str {
        &self.action_sha256
    }
}

/// Terminal source states admitted directly from one family apply command.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMaintenanceFamilyReleaseApplyState {
    Acknowledged,
    CompensationRequired,
    OperatorOffUnapplied,
}

/// Categorical compensation state emitted by the retained family transaction store.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMaintenanceFamilyReleaseCompensationState {
    Absent,
    Failed,
    Verified,
}

/// Optional outcome admitted by the exact terminal-state combination rules.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMaintenanceFamilyReleaseOutcome {
    Active,
    DeployedOperatorOff,
    OperatorOffUnapplied,
}

/// Canonical and fully validated terminal response from the family operation.
#[derive(Clone, Debug, PartialEq)]
pub struct NativeMaintenanceFamilyReleaseApplyStatus {
    pub value: JsonValue,
    pub canonical_bytes: Vec<u8>,
    pub action_sha256: String,
    pub compensating_release: String,
    pub compensation_state: NativeMaintenanceFamilyReleaseCompensationState,
    pub family: Option<String>,
    pub fencing_token: String,
    pub outcome: Option<NativeMaintenanceFamilyReleaseOutcome>,
    pub state: NativeMaintenanceFamilyReleaseApplyState,
    pub target_release: String,
}

/// Closed request, transport, or acknowledgement refusal category.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMaintenanceDevelopmentRemoteError {
    AnchorRequest,
    FamilyRequest,
    Transport,
    AnchorResponse,
    FamilyResponse,
}

impl Display for NativeMaintenanceDevelopmentRemoteError {
    /// Render one path-free error without exposing owner transport diagnostics.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        // preserve the predecessor's distinct invalid-request and invalid-response boundaries
        formatter.write_str(match self {
            Self::AnchorRequest => "development custody anchor request is invalid",
            Self::FamilyRequest => "family release request is invalid",
            Self::Transport => "maintenance remote transport failed",
            Self::AnchorResponse => "development custody anchor installation response is invalid",
            Self::FamilyResponse => "family_release_response_invalid",
        })
    }
}

impl std::error::Error for NativeMaintenanceDevelopmentRemoteError {}

/// Only the two fixed owner operations required before development family publication.
pub trait NativeMaintenanceDevelopmentRemotePort {
    /// Submit exact canonical custody-anchor bytes under their content identity.
    fn install_development_custody_anchor(
        &mut self,
        anchor_sha256: &str,
        canonical_bytes: &[u8],
    ) -> io::Result<Vec<u8>>;

    /// Submit one already closed family operation without caller-selected command material.
    fn apply_family_release(
        &mut self,
        request: &NativeMaintenanceFamilyReleaseApplyRequest,
    ) -> io::Result<Vec<u8>>;
}

impl NativeMaintenanceDevelopmentRemotePort for NativeMaintenanceDatabaseLedgerReader {
    /// Reuse the fixed SSH owner with the source install verb, hash operand, and stdin bytes.
    fn install_development_custody_anchor(
        &mut self,
        anchor_sha256: &str,
        canonical_bytes: &[u8],
    ) -> io::Result<Vec<u8>> {
        self.request_document(
            ANCHOR_INSTALL_COMMAND,
            &[anchor_sha256],
            Some(canonical_bytes),
        )
    }

    /// Reuse the fixed SSH owner with the exact source family verb and operand order.
    fn apply_family_release(
        &mut self,
        request: &NativeMaintenanceFamilyReleaseApplyRequest,
    ) -> io::Result<Vec<u8>> {
        self.request_document(FAMILY_APPLY_COMMAND, &request.arguments(), None)
    }
}

/// Validate and encode one exact custody-only development anchor before transport.
pub fn build_native_maintenance_development_anchor_install_request(
    anchor: &JsonValue,
) -> Result<NativeMaintenanceDevelopmentAnchorInstallRequest, NativeMaintenanceDevelopmentRemoteError>
{
    use NativeMaintenanceDevelopmentRemoteError::AnchorRequest;

    let mut codec = NativeWirePrimitives;
    let time = NativeWirePrimitives;
    validate_custody_anchor(CustodyAnchorKind::Development, anchor, &mut codec, &time)
        .ok_or(AnchorRequest)?;
    let canonical_bytes =
        MaintenanceCanonicalJson::encode_canonical_json(&mut codec, anchor).ok_or(AnchorRequest)?;
    let anchor_sha256 = format_digest(RevisionArchiveCanonicalCodec::sha256(
        &NativeWirePrimitives,
        &canonical_bytes,
    ));
    Ok(NativeMaintenanceDevelopmentAnchorInstallRequest {
        anchor: anchor.clone(),
        canonical_bytes,
        anchor_sha256,
    })
}

/// Install one validated anchor and bind the canonical response to the submitted bytes.
pub fn install_native_maintenance_development_custody_anchor(
    remote: &mut impl NativeMaintenanceDevelopmentRemotePort,
    request: &NativeMaintenanceDevelopmentAnchorInstallRequest,
) -> Result<NativeMaintenanceDevelopmentAnchorInstallation, NativeMaintenanceDevelopmentRemoteError>
{
    use NativeMaintenanceDevelopmentRemoteError::{AnchorRequest, Transport};

    let rebuilt = build_native_maintenance_development_anchor_install_request(&request.anchor)?;
    // prevent a caller from mutating any public request member after construction
    if rebuilt != *request {
        return Err(AnchorRequest);
    }
    let bytes = remote
        .install_development_custody_anchor(&request.anchor_sha256, &request.canonical_bytes)
        .map_err(|_| Transport)?;
    verify_native_maintenance_development_anchor_installation(request, &bytes)
}

/// Verify the exact canonical installed acknowledgement without performing transport.
pub fn verify_native_maintenance_development_anchor_installation(
    request: &NativeMaintenanceDevelopmentAnchorInstallRequest,
    bytes: &[u8],
) -> Result<NativeMaintenanceDevelopmentAnchorInstallation, NativeMaintenanceDevelopmentRemoteError>
{
    use NativeMaintenanceDevelopmentRemoteError::AnchorResponse;

    let response =
        MaintenanceCanonicalJson::parse_canonical_object(&mut NativeWirePrimitives, bytes)
            .ok_or(AnchorResponse)?;
    let anchor_sha256 = text(&response, "anchorSha256").ok_or(AnchorResponse)?;
    // bind the fixed response schema, contract, state, and submitted content identity
    if !exact_keys(&response, ANCHOR_INSTALLATION_KEYS)
        || !valid_hash(&anchor_sha256)
        || anchor_sha256 != request.anchor_sha256
        || text(&response, "contractVersion").as_deref() != Some(ANCHOR_INSTALLATION_VERSION)
        || text(&response, "state").as_deref() != Some("installed")
    {
        return Err(AnchorResponse);
    }
    Ok(NativeMaintenanceDevelopmentAnchorInstallation {
        anchor_sha256,
        canonical_bytes: bytes.to_vec(),
    })
}

/// Validate the exact source request and freeze its nine transport operands.
pub fn build_native_maintenance_family_release_apply_request(
    input: NativeMaintenanceFamilyReleaseApplyInput<'_>,
) -> Result<NativeMaintenanceFamilyReleaseApplyRequest, NativeMaintenanceDevelopmentRemoteError> {
    use NativeMaintenanceDevelopmentRemoteError::FamilyRequest;

    // close hashes, literals, release grammar, fence range, and compare-and-swap identities
    if ![
        input.action_sha256,
        input.expected_settings_sha256,
        input.report_sha256,
    ]
    .into_iter()
    .all(valid_hash)
        || ![
            input.compensating_release,
            input.expected_current_release,
            input.expected_source_release,
            input.target_release,
        ]
        .into_iter()
        .all(valid_request_release)
        || !valid_family(input.family)
        || !valid_positive_u64(input.fencing_token)
        || input.expected_current_release != input.expected_source_release
        || input.target_release == input.compensating_release
        || input.target_release == input.expected_current_release
        || input.compensating_release == input.expected_current_release
    {
        return Err(FamilyRequest);
    }
    let arguments = [
        input.target_release.to_owned(),
        input.compensating_release.to_owned(),
        input.expected_current_release.to_owned(),
        input.expected_source_release.to_owned(),
        input.expected_settings_sha256.to_owned(),
        input.family.to_owned(),
        input.action_sha256.to_owned(),
        input.report_sha256.to_owned(),
        input.fencing_token.to_owned(),
    ];
    Ok(NativeMaintenanceFamilyReleaseApplyRequest {
        action_sha256: input.action_sha256.to_owned(),
        arguments,
    })
}

/// Apply one validated family request and admit only a canonical terminal response.
pub fn apply_native_maintenance_family_release(
    remote: &mut impl NativeMaintenanceDevelopmentRemotePort,
    request: &NativeMaintenanceFamilyReleaseApplyRequest,
) -> Result<NativeMaintenanceFamilyReleaseApplyStatus, NativeMaintenanceDevelopmentRemoteError> {
    use NativeMaintenanceDevelopmentRemoteError::Transport;

    let bytes = remote
        .apply_family_release(request)
        .map_err(|_| Transport)?;
    verify_native_maintenance_family_release_apply_status(request, &bytes)
}

/// Verify the source's exact canonical terminal status and requested action binding.
pub fn verify_native_maintenance_family_release_apply_status(
    request: &NativeMaintenanceFamilyReleaseApplyRequest,
    bytes: &[u8],
) -> Result<NativeMaintenanceFamilyReleaseApplyStatus, NativeMaintenanceDevelopmentRemoteError> {
    use NativeMaintenanceDevelopmentRemoteError::FamilyResponse;

    let value = MaintenanceCanonicalJson::parse_canonical_object(&mut NativeWirePrimitives, bytes)
        .ok_or(FamilyResponse)?;
    // reject absence and every extension before reading categorical members
    if !exact_keys(&value, FAMILY_STATUS_KEYS) {
        return Err(FamilyResponse);
    }
    let action_sha256 = text(&value, "actionSha256").ok_or(FamilyResponse)?;
    let compensating_release = text(&value, "compensatingRelease").ok_or(FamilyResponse)?;
    let compensation_state = parse_compensation_state(&value).ok_or(FamilyResponse)?;
    let family = parse_family_or_null(&value).ok_or(FamilyResponse)?;
    let fencing_token = text(&value, "fencingToken").ok_or(FamilyResponse)?;
    let state = parse_terminal_state(&value).ok_or(FamilyResponse)?;
    let target_release = text(&value, "targetRelease").ok_or(FamilyResponse)?;
    let outcome = parse_outcome(&value).ok_or(FamilyResponse)?;
    // retain the original response validator and its action-only caller cross-binding
    if text(&value, "contractVersion").as_deref() != Some(FAMILY_STATUS_VERSION)
        || !valid_hash(&action_sha256)
        || action_sha256 != request.action_sha256
        || !valid_status_fence(&fencing_token)
        || !valid_status_release(&target_release)
        || !valid_status_release(&compensating_release)
        || !valid_terminal_combination(state, compensation_state, outcome)
    {
        return Err(FamilyResponse);
    }
    Ok(NativeMaintenanceFamilyReleaseApplyStatus {
        value,
        canonical_bytes: bytes.to_vec(),
        action_sha256,
        compensating_release,
        compensation_state,
        family,
        fencing_token,
        outcome,
        state,
        target_release,
    })
}

// recognize only the three immutable family literals
fn valid_family(value: &str) -> bool {
    matches!(value, "temperature" | "wind" | "rain")
}

// recognize the request's fixed one-or-two-digit positive release revision
fn valid_request_release(value: &str) -> bool {
    valid_release_prefix(value)
        .is_some_and(|revision| (1..=2).contains(&revision.len()) && valid_revision(revision))
}

// recognize the status response's unbounded positive release revision
fn valid_status_release(value: &str) -> bool {
    valid_release_prefix(value).is_some_and(valid_revision)
}

// preserve the source regex's date-shaped prefix without calendar normalization
fn valid_release_prefix(value: &str) -> Option<&str> {
    let bytes = value.as_bytes();
    // require every fixed separator and digit slot before slicing the revision
    if bytes.len() < 12
        || bytes.get(4) != Some(&b'.')
        || bytes.get(7) != Some(&b'.')
        || bytes.get(10) != Some(&b'-')
        || !bytes[..4].iter().all(u8::is_ascii_digit)
        || !bytes[5..7].iter().all(u8::is_ascii_digit)
        || !bytes[8..10].iter().all(u8::is_ascii_digit)
    {
        return None;
    }
    value.get(11..)
}

// recognize a positive decimal revision without a leading zero
fn valid_revision(value: &str) -> bool {
    matches!(value.as_bytes().first(), Some(b'1'..=b'9'))
        && value.bytes().all(|byte| byte.is_ascii_digit())
}

// recognize one canonical positive unsigned 64-bit decimal
fn valid_positive_u64(value: &str) -> bool {
    valid_status_fence(value) && value.parse::<u64>().is_ok()
}

// recognize the response validator's one-to-twenty-digit positive decimal
fn valid_status_fence(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 20
        && matches!(value.as_bytes().first(), Some(b'1'..=b'9'))
        && value.bytes().all(|byte| byte.is_ascii_digit())
}

// decode the family-or-daily-null vocabulary retained by the status validator
fn parse_family_or_null(value: &JsonValue) -> Option<Option<String>> {
    // distinguish explicit null from absent, aliased, or mistyped families
    match field(value, "family")? {
        JsonValue::Null => Some(None),
        JsonValue::String(units) => {
            let family = String::from_utf16(units).ok()?;
            valid_family(&family).then_some(Some(family))
        }
        _ => None,
    }
}

// decode only one exact terminal source state
fn parse_terminal_state(value: &JsonValue) -> Option<NativeMaintenanceFamilyReleaseApplyState> {
    // close every in-flight and unknown state before the caller treats apply as complete
    match text(value, "state")?.as_str() {
        "acknowledged" => Some(NativeMaintenanceFamilyReleaseApplyState::Acknowledged),
        "compensation_required" => {
            Some(NativeMaintenanceFamilyReleaseApplyState::CompensationRequired)
        }
        "operator_off_unapplied" => {
            Some(NativeMaintenanceFamilyReleaseApplyState::OperatorOffUnapplied)
        }
        _ => None,
    }
}

// decode only one categorical compensation state
fn parse_compensation_state(
    value: &JsonValue,
) -> Option<NativeMaintenanceFamilyReleaseCompensationState> {
    // retain every state allowed by the family status schema
    match text(value, "compensationState")?.as_str() {
        "absent" => Some(NativeMaintenanceFamilyReleaseCompensationState::Absent),
        "failed" => Some(NativeMaintenanceFamilyReleaseCompensationState::Failed),
        "verified" => Some(NativeMaintenanceFamilyReleaseCompensationState::Verified),
        _ => None,
    }
}

// decode the exact nullable outcome vocabulary
fn parse_outcome(value: &JsonValue) -> Option<Option<NativeMaintenanceFamilyReleaseOutcome>> {
    // distinguish the required explicit null from absent or mistyped members
    match field(value, "outcome")? {
        JsonValue::Null => Some(None),
        JsonValue::String(units) if units.iter().copied().eq("active".encode_utf16()) => {
            Some(Some(NativeMaintenanceFamilyReleaseOutcome::Active))
        }
        JsonValue::String(units)
            if units
                .iter()
                .copied()
                .eq("deployed_operator_off".encode_utf16()) =>
        {
            Some(Some(
                NativeMaintenanceFamilyReleaseOutcome::DeployedOperatorOff,
            ))
        }
        JsonValue::String(units)
            if units
                .iter()
                .copied()
                .eq("operator_off_unapplied".encode_utf16()) =>
        {
            Some(Some(
                NativeMaintenanceFamilyReleaseOutcome::OperatorOffUnapplied,
            ))
        }
        _ => None,
    }
}

// admit only the predecessor's terminal state, compensation, and outcome combinations
fn valid_terminal_combination(
    state: NativeMaintenanceFamilyReleaseApplyState,
    compensation: NativeMaintenanceFamilyReleaseCompensationState,
    outcome: Option<NativeMaintenanceFamilyReleaseOutcome>,
) -> bool {
    // retain the exact categorical tuple rather than independently validating each member
    match state {
        NativeMaintenanceFamilyReleaseApplyState::Acknowledged => {
            compensation == NativeMaintenanceFamilyReleaseCompensationState::Absent
                && matches!(
                    outcome,
                    Some(
                        NativeMaintenanceFamilyReleaseOutcome::Active
                            | NativeMaintenanceFamilyReleaseOutcome::DeployedOperatorOff
                    )
                )
        }
        NativeMaintenanceFamilyReleaseApplyState::CompensationRequired => outcome.is_none(),
        NativeMaintenanceFamilyReleaseApplyState::OperatorOffUnapplied => {
            compensation == NativeMaintenanceFamilyReleaseCompensationState::Absent
                && outcome == Some(NativeMaintenanceFamilyReleaseOutcome::OperatorOffUnapplied)
        }
    }
}

#[cfg(test)]
#[path = "native_maintenance_development_remote_test.rs"]
mod tests;
