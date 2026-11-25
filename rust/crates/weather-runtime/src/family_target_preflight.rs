//! Compose the original family action, target-image catalog and custody checks before mutation.

use std::path::PathBuf;

use weather_api::CurrentTimeResolver;
use weather_contracts::JsonValue;

use crate::capture_epoch_operator::{AUTHORITY_DIR, WITNESS_FILE, read_operator_file};
use crate::family_target_binding::{verify_development_target_binding, verify_rain_target_binding};
use crate::family_target_catalog::canonical_member;
use crate::family_target_catalog_validation::validate_family_target_catalog;
use crate::maintenance_canonical_json::text;
use crate::{
    FamilyActionAuthorityFiles, FamilyTargetCatalogInput, FamilyTargetImageRead,
    MaintenanceAnchorStatusError, MaintenanceAnchorStatusJson, authorize_family_action,
    build_family_target_catalog,
};

use MaintenanceAnchorStatusError::{Codec, Slot};

const MAXIMUM_CATALOG_BYTES: usize = 256 * 1024;
const RAIN_ACTION_VERSION: &str = "forecast-adjustment-rain-control-reference-action/v1";
const DEVELOPMENT_ANCHOR_VERSION: &str = "adjustment-development-custody-anchor/v1";

/// Exact installed release inputs already selected by the original operator.
pub struct FamilyTargetPreflightInput<'a> {
    pub action: &'a JsonValue,
    pub action_sha256: &'a str,
    pub family: &'a str,
    pub source_bytes: &'a [u8],
    pub environment: &'a JsonValue,
    pub image_commit: &'a str,
    pub settings_bytes: &'a [u8],
    pub clock: &'a dyn Fn() -> Result<String, MaintenanceAnchorStatusError>,
}

/// The independent retained capture-epoch observation used by development binding.
pub trait FamilyTargetWitnessRead {
    /// Read the exact canonical root-owned witness bytes.
    fn read_witness(&mut self) -> Result<Vec<u8>, MaintenanceAnchorStatusError>;
}

/// Actual file owner for the original separately selected capture epoch.
pub struct NativeFamilyTargetWitness {
    path: PathBuf,
    uid: u32,
    gid: u32,
}

impl NativeFamilyTargetWitness {
    /// Bind the fixed installed root-owned witness without reading it.
    pub fn native() -> Self {
        Self::for_path(PathBuf::from(AUTHORITY_DIR).join(WITNESS_FILE), 0, 0)
    }

    /// Bind an explicitly selected fixture witness and independent expected owner.
    pub fn for_path(path: PathBuf, uid: u32, gid: u32) -> Self {
        Self { path, uid, gid }
    }
}

impl FamilyTargetWitnessRead for NativeFamilyTargetWitness {
    /// Apply the original bounded root-owned canonical witness file boundary.
    fn read_witness(&mut self) -> Result<Vec<u8>, MaintenanceAnchorStatusError> {
        read_operator_file(&self.path, self.uid, self.gid).map_err(|_| Slot)
    }
}

/// Return the exact ready-for-image-verification catalog bytes without installing any state.
pub fn prepare_family_target_catalog(
    input: &FamilyTargetPreflightInput<'_>,
    files: &mut impl FamilyActionAuthorityFiles,
    image: &mut impl FamilyTargetImageRead,
    witness: &mut impl FamilyTargetWitnessRead,
    codec: &mut impl MaintenanceAnchorStatusJson,
    time: &dyn CurrentTimeResolver,
) -> Result<Vec<u8>, MaintenanceAnchorStatusError> {
    prepare_family_target_material(input, files, image, witness, codec, time)
        .map(|(_, bytes)| bytes)
}

// retain the exact authority together with the checked target catalog for the release proof
pub(crate) fn prepare_family_target_material(
    input: &FamilyTargetPreflightInput<'_>,
    files: &mut impl FamilyActionAuthorityFiles,
    image: &mut impl FamilyTargetImageRead,
    witness: &mut impl FamilyTargetWitnessRead,
    codec: &mut impl MaintenanceAnchorStatusJson,
    time: &dyn CurrentTimeResolver,
) -> Result<(JsonValue, Vec<u8>), MaintenanceAnchorStatusError> {
    // observe original installed action authority before interpreting the baseline catalog
    let authority = authorize_family_action(input.action, input.action_sha256, files, codec, time)?;
    let source = canonical_member(input.source_bytes, MAXIMUM_CATALOG_BYTES, codec)?;
    validate_family_target_catalog(&source, codec, time)?;

    let control = text(input.action, "contractVersion").as_deref() == Some(RAIN_ACTION_VERSION);
    // derive the receipt authority from the canonical original custody document only for control
    let custody_anchor = if control {
        let bytes = codec.encode_canonical_json(&authority).ok_or(Codec)?;
        Some(codec.sha256(&bytes).ok_or(Codec)?)
    } else {
        None
    };
    let target = build_family_target_catalog(
        &FamilyTargetCatalogInput {
            action: input.action,
            action_sha256: input.action_sha256,
            family: input.family,
            source: &source,
            environment: input.environment,
            image_commit: input.image_commit,
            settings_bytes: input.settings_bytes,
            clock: input.clock,
            custody_anchor_sha256: custody_anchor.as_ref().map(|digest| digest.as_str()),
        },
        image,
        codec,
        time,
    )?;
    // independently bind the new control slot to the exact original custody and action
    if control {
        verify_rain_target_binding(
            input.action,
            input.action_sha256,
            &authority,
            &target,
            codec,
        )?;
    }
    // only development authority requires the separately observed capture-epoch witness
    if text(&authority, "contractVersion").as_deref() == Some(DEVELOPMENT_ANCHOR_VERSION) {
        let witness = witness.read_witness()?;
        verify_development_target_binding(
            input.action,
            input.action_sha256,
            &authority,
            &target,
            &witness,
            codec,
            time,
        )?;
    }
    let bytes = codec.encode_canonical_json(&target).ok_or(Codec)?;
    Ok((authority, bytes))
}
