//! Resolve worker rain shadow model material from the original installed catalog.

use weather_contracts::AdjustmentShadowFamily;
use weather_database::{AdmittedApiMaintenanceRegistration, Sha256Digest};

use crate::api_maintenance_candidate::{InstalledApiRegistration, bind_installed_registration};
use crate::api_maintenance_candidate_catalog::select_api_shadow_catalog;
use crate::api_maintenance_candidate_members::validate_api_shadow_members;
use crate::worker_rain_candidate_package::{
    check_worker_rain_control_members, check_worker_rain_package,
};
use crate::{
    ApiMaintenanceCandidateCodec, ApiMaintenanceCandidateError, ApiMaintenanceShadowEvidenceValues,
    PreparedWorkerRainMaintenanceCandidate, RainMaintenanceCandidateFiles,
};

/// A shadow artifact and registration admitted from the same installed rain slot.
pub struct LoadedWorkerRainMaintenanceCandidate {
    prepared: PreparedWorkerRainMaintenanceCandidate,
    registration: InstalledApiRegistration,
}

impl LoadedWorkerRainMaintenanceCandidate {
    /// Borrow the admitted portable model without giving it serving authority.
    pub fn prepared(&self) -> &PreparedWorkerRainMaintenanceCandidate {
        &self.prepared
    }

    /// Borrow the unchanged registration fields for the worker's ingest role.
    pub(crate) fn registration(&self) -> AdmittedApiMaintenanceRegistration<'_> {
        self.registration.as_admitted()
    }

    /// Identify the exact immutable catalog whose rain selection was loaded.
    pub fn catalog_sha256(&self) -> &Sha256Digest {
        self.registration.catalog_sha256()
    }
}

/// Load the pending rain shadow slot and validate the attached technical package, if selected.
pub fn load_worker_rain_maintenance_candidate(
    files: &mut impl RainMaintenanceCandidateFiles,
    codec: &mut impl ApiMaintenanceCandidateCodec,
    values: &impl ApiMaintenanceShadowEvidenceValues,
) -> Result<Option<LoadedWorkerRainMaintenanceCandidate>, ApiMaintenanceCandidateError> {
    let family = AdjustmentShadowFamily::Rain;
    let bytes = files.read_catalog()?;
    let catalog = codec
        .parse_canonical_object(&bytes)
        .ok_or(ApiMaintenanceCandidateError::Canonical)?;
    // an omitted shadow slot grants neither scoring nor publication authority
    let Some(selected) = select_api_shadow_catalog(&catalog, family, codec)? else {
        return Ok(None);
    };
    let receipt = &selected.receipt;
    let members = files.read_rain_members(
        &receipt.action_sha256,
        &receipt.candidate_sha256,
        &receipt.bundle_sha256,
    )?;
    validate_api_shadow_members(family, &selected, &members.members, codec)?;
    let prepared = PreparedWorkerRainMaintenanceCandidate::prepare(
        values,
        &members.members.bundle,
        &receipt.bundle_sha256,
    )
    .ok_or(ApiMaintenanceCandidateError::Package)?;
    // v2 requires both linked controls but does not select the independent control slot
    if let Some(controls) = check_worker_rain_package(
        codec,
        &members.package,
        &receipt.candidate_sha256,
        &receipt.bundle_sha256,
        prepared.model_month(),
    )? {
        let controls_bytes = files.read_rain_control_members(
            &controls.control_state_sha256,
            &controls.ordinal_artifact_sha256,
        )?;
        check_worker_rain_control_members(
            values,
            &controls,
            prepared.model_month(),
            &controls_bytes.state,
            &controls_bytes.ordinal,
        )?;
    }
    let registration = bind_installed_registration(family, &bytes, selected, codec)?;
    Ok(Some(LoadedWorkerRainMaintenanceCandidate {
        prepared,
        registration,
    }))
}

#[cfg(test)]
#[path = "worker_rain_candidate_loader_test.rs"]
mod tests;
