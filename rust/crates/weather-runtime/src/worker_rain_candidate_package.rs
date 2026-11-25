//! Admit the rain worker's technical package and optional monthly control members.

use weather_domain::{AdmittedRainMaintenanceControlState, admit_rain_maintenance_control_state};

use crate::PreparedWorkerRainMaintenanceCandidate;
use crate::api_maintenance_admission::{
    ApiMaintenanceShadowEvidenceValues, ApiRainControlPrimitives,
};
use crate::api_maintenance_candidate::{
    ApiMaintenanceCandidateCodec, ApiMaintenanceCandidateError, exact_keys, text, valid_hash,
};

const RAIN_PACKAGE_V1: &str = "forecast-adjustment-rain-model-package/v1";
const RAIN_PACKAGE_V2: &str = "forecast-adjustment-rain-model-package/v2";
const RAIN_PACKAGE_V1_KEYS: &[&str] = &[
    "artifactPath",
    "artifactSha256",
    "candidateSha256",
    "compiledSourcePath",
    "compiledSourceSha256",
    "contractVersion",
    "modelMonth",
    "registrySha256",
];
const RAIN_PACKAGE_V2_KEYS: &[&str] = &[
    "artifactPath",
    "artifactSha256",
    "candidateSha256",
    "compiledSourcePath",
    "compiledSourceSha256",
    "contractVersion",
    "modelMonth",
    "registrySha256",
    "controlStatePath",
    "controlStateSha256",
    "ordinalArtifactPath",
    "ordinalArtifactSha256",
];

type CandidateError = ApiMaintenanceCandidateError;

/// Exact optional fixed-control members selected by a v2 technical package.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RainControlMemberSelection {
    pub control_state_sha256: String,
    pub ordinal_artifact_sha256: String,
}

/// Admit one canonical v1 or v2 rain technical package selected by the worker catalog.
pub(crate) fn check_worker_rain_package(
    codec: &mut impl ApiMaintenanceCandidateCodec,
    bytes: &[u8],
    candidate_sha256: &str,
    bundle_sha256: &str,
    model_month: &str,
) -> Result<Option<RainControlMemberSelection>, CandidateError> {
    let package = codec
        .parse_canonical_object(bytes)
        .ok_or(CandidateError::Canonical)?;
    let version = text(&package, "contractVersion").ok_or(CandidateError::Package)?;
    let expected_keys = match version.as_str() {
        RAIN_PACKAGE_V1 => RAIN_PACKAGE_V1_KEYS,
        RAIN_PACKAGE_V2 => RAIN_PACKAGE_V2_KEYS,
        _ => return Err(CandidateError::Package),
    };
    text(&package, "compiledSourceSha256")
        .filter(|identity| valid_hash(identity))
        .ok_or(CandidateError::Package)?;
    text(&package, "registrySha256")
        .filter(|identity| valid_hash(identity))
        .ok_or(CandidateError::Package)?;
    let artifact_path = format!("rain-runtime-artifacts/sha256-{bundle_sha256}.json");
    // bind the candidate receipt to its exact portable artifact, source path, and due month
    if !exact_keys(&package, expected_keys)
        || !valid_hash(candidate_sha256)
        || !valid_hash(bundle_sha256)
        || text(&package, "candidateSha256").as_deref() != Some(candidate_sha256)
        || text(&package, "artifactSha256").as_deref() != Some(bundle_sha256)
        || text(&package, "artifactPath").as_deref() != Some(artifact_path.as_str())
        || text(&package, "compiledSourcePath").as_deref()
            != Some("packages/forecast-adjustment/src/rain-hurdle-wind-artifact.ts")
        || text(&package, "modelMonth").as_deref() != Some(model_month)
    {
        return Err(CandidateError::Package);
    }
    // legacy packages intentionally carry no fixed-control scoring authority
    if version == RAIN_PACKAGE_V1 {
        return Ok(None);
    }
    let control_state_sha256 = text(&package, "controlStateSha256")
        .filter(|identity| valid_hash(identity))
        .ok_or(CandidateError::Package)?;
    let ordinal_artifact_sha256 = text(&package, "ordinalArtifactSha256")
        .filter(|identity| valid_hash(identity))
        .ok_or(CandidateError::Package)?;
    let control_path =
        format!("rain-maintenance-control-states/sha256-{control_state_sha256}.json");
    let ordinal_path = format!("rain-runtime-artifacts/sha256-{ordinal_artifact_sha256}.json");
    // v2 grants control scoring only to its exact two content-addressed members
    if text(&package, "controlStatePath").as_deref() != Some(control_path.as_str())
        || text(&package, "ordinalArtifactPath").as_deref() != Some(ordinal_path.as_str())
    {
        return Err(CandidateError::Package);
    }
    Ok(Some(RainControlMemberSelection {
        control_state_sha256,
        ordinal_artifact_sha256,
    }))
}

/// Validate the exact state and portable ordinal artifact selected by a v2 package.
pub(crate) fn check_worker_rain_control_members(
    values: &impl ApiMaintenanceShadowEvidenceValues,
    selected: &RainControlMemberSelection,
    model_month: &str,
    state_bytes: &[u8],
    ordinal_bytes: &[u8],
) -> Result<
    (
        AdmittedRainMaintenanceControlState,
        PreparedWorkerRainMaintenanceCandidate,
    ),
    CandidateError,
> {
    let state_sha256 = values.sha256(state_bytes).ok_or(CandidateError::Package)?;
    // refuse the selected state before decoding content under a substituted address
    if state_sha256.as_str() != selected.control_state_sha256 {
        return Err(CandidateError::Package);
    }
    let state =
        admit_rain_maintenance_control_state(&ApiRainControlPrimitives(values), state_bytes)
            .ok_or(CandidateError::Package)?;
    let ordinal = PreparedWorkerRainMaintenanceCandidate::prepare(
        values,
        ordinal_bytes,
        &selected.ordinal_artifact_sha256,
    )
    .ok_or(CandidateError::Package)?;
    // cross-bind the state, technical receipt, ordinal artifact, and selected due month
    if state.ordinal_artifact_sha256 != selected.ordinal_artifact_sha256
        || state.model_month != model_month
        || ordinal.model_month() != model_month
    {
        return Err(CandidateError::Package);
    }
    Ok((state, ordinal))
}
