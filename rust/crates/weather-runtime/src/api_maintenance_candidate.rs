//! Load exactly the inactive wind and temperature model packages selected by the API catalog.

use weather_api::{
    AdmittedTemperatureCanaryRuntime, AdmittedTemperatureMosModelOwner,
    BorrowedTemperatureMosServingModel, ForecastTemperatureMosAdmissionBoundary,
    MaintenanceWindModelCalendar, WeatherApiMaintenanceStartupPreparation,
    WeatherApiStartupSnapshots,
};
use weather_contracts::{
    AdjustmentShadowFamily, ForecastAdjustmentMaintenanceRuntimePackage,
    ForecastAdjustmentTemperatureMaintenanceRuntimePackage,
    ForecastAdjustmentWindMaintenanceRuntimePackage, JsonValue,
};
use weather_database::{
    AdmittedApiMaintenanceRegistration, ApiMaintenanceRegistrationVersion, Sha256Digest,
};

use crate::api_maintenance_candidate_catalog::{ApiShadowCatalogEntry, select_api_shadow_catalog};
use crate::api_maintenance_candidate_members::validate_api_shadow_members;
use crate::api_maintenance_candidate_package::admit_api_shadow_package;
pub(crate) use crate::maintenance_canonical_json::{exact_keys, field, text, valid_hash};
use crate::{
    ApiMaintenanceCandidateFiles, InstalledApiTemperatureCapture, InstalledApiWindCapture,
    MaintenanceCanonicalJson, NativeApiMaintenanceCandidateFiles,
};

/// Value-free reasons installed shadow capture cannot be prepared at startup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApiMaintenanceCandidateError {
    File,
    Canonical,
    Catalog,
    Registration,
    Action,
    Projection,
    Parity,
    Package,
}

/// Admitted JSON, UTC and SHA-256 owners; model and graph validation remain in Rust.
pub trait ApiMaintenanceCandidateCodec: MaintenanceCanonicalJson {
    /// Admit an exact valid ISO UTC instant with milliseconds, without normalization.
    fn is_canonical_utc_instant(&mut self, value: &str) -> bool;

    /// Decode the exact generated TS package, rejecting unknown keys/literal or type changes.
    fn decode_maintenance_package(
        &mut self,
        value: &JsonValue,
    ) -> Option<ForecastAdjustmentMaintenanceRuntimePackage> {
        crate::decode_maintenance_runtime_package(value)
    }
}

/// The verified package and its already bound compact PostgreSQL registration payload.
struct InstalledApiCandidate<Package> {
    package: Package,
    registration: InstalledApiRegistration,
}

pub(crate) struct InstalledApiRegistration {
    family: AdjustmentShadowFamily,
    version: ApiMaintenanceRegistrationVersion,
    registration_sha256: String,
    candidate_sha256: String,
    source_sha256: String,
    canonical_json: String,
    catalog_sha256: Sha256Digest,
    has_capture_receipt_v1: bool,
}

impl InstalledApiRegistration {
    /// Borrow only the original compact values needed during a capture job.
    pub(crate) fn as_admitted(&self) -> AdmittedApiMaintenanceRegistration<'_> {
        AdmittedApiMaintenanceRegistration {
            family: self.family,
            version: self.version,
            registration_sha256: &self.registration_sha256,
            candidate_sha256: &self.candidate_sha256,
            source_sha256: &self.source_sha256,
            canonical_json: &self.canonical_json,
        }
    }

    /// Borrow the digest of the exact canonical catalog read by the owner.
    pub(crate) fn catalog_sha256(&self) -> &Sha256Digest {
        &self.catalog_sha256
    }
}

/// Loaded candidate choices are immutable for the private API listener's lifetime.
#[derive(Default)]
pub struct LoadedApiMaintenanceCandidates {
    temperature:
        Option<InstalledApiCandidate<ForecastAdjustmentTemperatureMaintenanceRuntimePackage>>,
    wind: Option<InstalledApiCandidate<ForecastAdjustmentWindMaintenanceRuntimePackage>>,
}

impl LoadedApiMaintenanceCandidates {
    /// Borrow the real selected temperature package and startup incumbent for capture.
    pub fn temperature_capture<'a>(
        &'a self,
        startup: &'a WeatherApiStartupSnapshots,
        active_incumbent: Option<(
            &'a AdmittedTemperatureCanaryRuntime,
            &'a AdmittedTemperatureMosModelOwner<'a, BorrowedTemperatureMosServingModel<'a>>,
        )>,
        calendar: &'a dyn ForecastTemperatureMosAdmissionBoundary,
    ) -> Option<InstalledApiTemperatureCapture<'a>> {
        let selected = self.temperature.as_ref()?;
        Some(InstalledApiTemperatureCapture {
            registration: selected.registration.as_admitted(),
            has_capture_receipt_v1: selected.registration.has_capture_receipt_v1,
            package: &selected.package,
            incumbent: &startup.temperature_adjustment.runtime,
            active_incumbent,
            calendar,
        })
    }

    /// Borrow the real selected wind package and startup incumbent for capture.
    pub fn wind_capture<'a>(
        &'a self,
        startup: &'a WeatherApiStartupSnapshots,
        calendar: &'a dyn MaintenanceWindModelCalendar,
    ) -> Option<InstalledApiWindCapture<'a>> {
        let selected = self.wind.as_ref()?;
        Some(InstalledApiWindCapture {
            registration: selected.registration.as_admitted(),
            has_capture_receipt_v1: selected.registration.has_capture_receipt_v1,
            package: &selected.package,
            incumbent: &startup.adjustment.runtime,
            calendar,
        })
    }

    /// Report the inactive candidate registration without losing the original loader result.
    pub fn registration_sha256(&self, family: AdjustmentShadowFamily) -> Option<&str> {
        match family {
            AdjustmentShadowFamily::Temperature => self
                .temperature
                .as_ref()
                .map(|entry| entry.registration.registration_sha256.as_str()),
            AdjustmentShadowFamily::Wind => self
                .wind
                .as_ref()
                .map(|entry| entry.registration.registration_sha256.as_str()),
            AdjustmentShadowFamily::Rain => None,
        }
    }

    /// Report the exact installed catalog address used to resolve the family.
    pub fn catalog_sha256(&self, family: AdjustmentShadowFamily) -> Option<&Sha256Digest> {
        match family {
            AdjustmentShadowFamily::Temperature => self
                .temperature
                .as_ref()
                .map(|entry| &entry.registration.catalog_sha256),
            AdjustmentShadowFamily::Wind => self
                .wind
                .as_ref()
                .map(|entry| &entry.registration.catalog_sha256),
            AdjustmentShadowFamily::Rain => None,
        }
    }
}

/// Load each original API family independently; an unavailable candidate never takes down public serving.
pub fn load_api_maintenance_candidates(
    files: &mut impl ApiMaintenanceCandidateFiles,
    codec: &mut impl ApiMaintenanceCandidateCodec,
) -> LoadedApiMaintenanceCandidates {
    let temperature = load_api_candidate(AdjustmentShadowFamily::Temperature, files, codec)
        .ok()
        .flatten()
        .and_then(|(registration, package)| match package {
            ForecastAdjustmentMaintenanceRuntimePackage::Temperature(package) => {
                Some(InstalledApiCandidate {
                    package,
                    registration,
                })
            }
            ForecastAdjustmentMaintenanceRuntimePackage::Wind(_) => None,
        });
    let wind = load_api_candidate(AdjustmentShadowFamily::Wind, files, codec)
        .ok()
        .flatten()
        .and_then(|(registration, package)| match package {
            ForecastAdjustmentMaintenanceRuntimePackage::Wind(package) => {
                Some(InstalledApiCandidate {
                    package,
                    registration,
                })
            }
            ForecastAdjustmentMaintenanceRuntimePackage::Temperature(_) => None,
        });
    LoadedApiMaintenanceCandidates { temperature, wind }
}

/// Load from the immutable application root only on the admitted private startup branch.
pub fn load_current_api_maintenance_candidates(
    _startup: &WeatherApiMaintenanceStartupPreparation,
    codec: &mut impl ApiMaintenanceCandidateCodec,
) -> LoadedApiMaintenanceCandidates {
    let Some(mut files) = std::env::current_dir()
        .ok()
        .and_then(|root| NativeApiMaintenanceCandidateFiles::new(root).ok())
    else {
        return LoadedApiMaintenanceCandidates::default();
    };
    load_api_maintenance_candidates(&mut files, codec)
}

#[cfg(test)]
#[path = "api_maintenance_candidate_test.rs"]
mod tests;

/// Read and cross-bind one catalog selection, and prepare the exact SQL registration object.
pub(crate) fn load_api_candidate(
    family: AdjustmentShadowFamily,
    files: &mut impl ApiMaintenanceCandidateFiles,
    codec: &mut impl ApiMaintenanceCandidateCodec,
) -> Result<
    Option<(
        InstalledApiRegistration,
        ForecastAdjustmentMaintenanceRuntimePackage,
    )>,
    ApiMaintenanceCandidateError,
> {
    let bytes = files.read_catalog()?;
    let catalog = codec
        .parse_canonical_object(&bytes)
        .ok_or(ApiMaintenanceCandidateError::Canonical)?;
    let Some(selected) = select_api_shadow_catalog(&catalog, family, codec)? else {
        return Ok(None);
    };
    let members = files.read_members(
        family,
        &selected.receipt.action_sha256,
        &selected.receipt.candidate_sha256,
        &selected.receipt.bundle_sha256,
    )?;
    validate_api_shadow_members(family, &selected, &members, codec)?;
    let package = admit_api_shadow_package(family, &selected.receipt, &members.bundle, codec)?;
    let registration = bind_installed_registration(family, &bytes, selected, codec)?;
    Ok(Some((registration, package)))
}

/// Keep catalog provenance and the exact canonical PostgreSQL bind together.
pub(crate) fn bind_installed_registration(
    family: AdjustmentShadowFamily,
    catalog_bytes: &[u8],
    selected: ApiShadowCatalogEntry<'_>,
    codec: &mut impl ApiMaintenanceCandidateCodec,
) -> Result<InstalledApiRegistration, ApiMaintenanceCandidateError> {
    let canonical = codec
        .encode_canonical_json(selected.registration)
        .ok_or(ApiMaintenanceCandidateError::Canonical)?;
    let canonical = canonical
        .strip_suffix(b"\n")
        .ok_or(ApiMaintenanceCandidateError::Canonical)?;
    let canonical_json = std::str::from_utf8(canonical)
        .map_err(|_| ApiMaintenanceCandidateError::Canonical)?
        .to_owned();
    let catalog_sha256 = codec
        .sha256(catalog_bytes)
        .filter(|digest| valid_hash(digest.as_str()))
        .ok_or(ApiMaintenanceCandidateError::Canonical)?;
    Ok(InstalledApiRegistration {
        family,
        version: selected.registration_version,
        registration_sha256: selected.receipt.registration_sha256,
        candidate_sha256: selected.receipt.candidate_sha256,
        source_sha256: selected.receipt.source_sha256,
        canonical_json,
        catalog_sha256,
        // the capture API's retained installed-candidate check explicitly requires v1
        has_capture_receipt_v1: selected.receipt.lifecycle.is_none(),
    })
}

/// Convert the generated family value without an alternate wire spelling.
pub(crate) const fn family_name(family: AdjustmentShadowFamily) -> &'static str {
    match family {
        AdjustmentShadowFamily::Temperature => "temperature",
        AdjustmentShadowFamily::Wind => "wind",
        AdjustmentShadowFamily::Rain => "rain",
    }
}
