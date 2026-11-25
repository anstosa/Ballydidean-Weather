//! Strict compiled-loader verification for all original installed family slots.

use weather_contracts::AdjustmentShadowFamily;

use crate::api_maintenance_candidate::load_api_candidate;
use crate::api_maintenance_candidate_package::admit_qualified_api_canary_package;
use crate::worker_rain_candidate_package::check_worker_rain_package;
use crate::worker_rain_serving_catalog::{
    select_qualified_serving_catalog, select_worker_rain_serving_catalog,
    validate_qualified_serving_members, validate_worker_rain_serving_members,
};
use crate::{
    ApiMaintenanceCandidateCodec, ApiMaintenanceCandidateError, ApiMaintenanceShadowEvidenceValues,
    ApiQualifiedCanaryFiles, NativeApiMaintenanceCandidateFiles,
    PreparedWorkerRainMaintenanceCandidate, WorkerRainControlReferenceFiles,
    WorkerRainServingFiles, load_worker_rain_control_reference,
    load_worker_rain_maintenance_candidate,
};

use ApiMaintenanceCandidateError::{Canonical, Package};

/// Verify every original installed candidate role with strict selected-package semantics.
pub fn verify_installed_family_catalog(
    files: &mut (
             impl ApiQualifiedCanaryFiles + WorkerRainServingFiles + WorkerRainControlReferenceFiles
         ),
    codec: &mut impl ApiMaintenanceCandidateCodec,
    values: &impl ApiMaintenanceShadowEvidenceValues,
) -> Result<(), ApiMaintenanceCandidateError> {
    // preserve the original temperature then wind shadow/active loader order
    for family in [
        AdjustmentShadowFamily::Temperature,
        AdjustmentShadowFamily::Wind,
    ] {
        let _ = load_api_candidate(family, files, codec)?;
        verify_installed_active_canary(family, files, codec)?;
    }
    // retain the independent rain shadow, active and separate control loader checks
    let _ = load_worker_rain_maintenance_candidate(files, codec, values)?;
    verify_installed_active_rain(files, codec, values)?;
    let _ = load_worker_rain_control_reference(files, codec, values)?;
    Ok(())
}

/// Bind the fixed immutable application root and original read-only installed catalog mount.
pub fn verify_native_installed_family_catalog(
    codec: &mut impl ApiMaintenanceCandidateCodec,
    values: &impl ApiMaintenanceShadowEvidenceValues,
) -> Result<(), ApiMaintenanceCandidateError> {
    let mut files = NativeApiMaintenanceCandidateFiles::new("/opt/weather")?;
    verify_installed_family_catalog(&mut files, codec, values)
}

// admit one installed active canary package without converting an error to startup raw mode
pub(crate) fn verify_installed_active_canary(
    family: AdjustmentShadowFamily,
    files: &mut impl ApiQualifiedCanaryFiles,
    codec: &mut impl ApiMaintenanceCandidateCodec,
) -> Result<(), ApiMaintenanceCandidateError> {
    let bytes = files.read_catalog()?;
    let catalog = codec.parse_canonical_object(&bytes).ok_or(Canonical)?;
    // genuine absence is the original null selected by the compiled verifier
    let Some(selected) = select_qualified_serving_catalog(&catalog, family, codec)? else {
        return Ok(());
    };
    let members = files.read_qualified_canary_members(
        family,
        &selected.action_sha256,
        &selected.bundle_sha256,
    )?;
    validate_qualified_serving_members(
        codec,
        &selected,
        family,
        &members.registry,
        &members.action,
    )?;
    admit_qualified_api_canary_package(
        family,
        &selected.bundle_sha256,
        &selected.candidate_sha256,
        &members.bundle,
        codec,
    )?;
    Ok(())
}

// admit the selected qualified rain artifact and technical receipt without startup fallback
fn verify_installed_active_rain(
    files: &mut impl WorkerRainServingFiles,
    codec: &mut impl ApiMaintenanceCandidateCodec,
    values: &impl ApiMaintenanceShadowEvidenceValues,
) -> Result<(), ApiMaintenanceCandidateError> {
    let bytes = files.read_catalog()?;
    let catalog = codec.parse_canonical_object(&bytes).ok_or(Canonical)?;
    // raw serving is not itself an installed active maintenance selection
    let Some(selected) = select_worker_rain_serving_catalog(&catalog, codec)? else {
        return Ok(());
    };
    let members = files.read_worker_rain_serving_members(
        &selected.action_sha256,
        &selected.candidate_sha256,
        &selected.bundle_sha256,
    )?;
    validate_worker_rain_serving_members(codec, &selected, &members.registry, &members.action)?;
    let packaged = PreparedWorkerRainMaintenanceCandidate::prepare(
        values,
        &members.bundle,
        &selected.bundle_sha256,
    )
    .ok_or(Package)?;
    // the original active verifier validates the package's linked identities but does not load control members
    let _ = check_worker_rain_package(
        codec,
        &members.package,
        &selected.candidate_sha256,
        &selected.bundle_sha256,
        packaged.model_month(),
    )?;
    Ok(())
}
