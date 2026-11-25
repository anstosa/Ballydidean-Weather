//! Fixed installed serving selection for the original monthly temperature fit.

use weather_api::WeatherApiStartupRuntimeLoader;
use weather_contracts::{
    FieldPresence, LoadedForecastAdjustmentTemperatureCanaryRuntime as Loaded,
    LoadedForecastAdjustmentTemperatureCanaryRuntimeV1 as Legacy, Nullable,
};
use weather_edge::{ArchiveWirePrimitives, RevisionArchiveCanonicalCodec};

use crate::{
    NativeMaintenanceHistoricalFamily, NativeMaintenanceTemperatureIncumbent, NativeWirePrimitives,
    maintenance_canonical_json::{field, valid_hash},
    native_installed_api_runtime_loader,
    native_maintenance_retirement_files_v2::format_digest,
};

/// Read the exact active startup-selected model from its immutable verified authority.
pub fn load_native_maintenance_temperature_incumbent()
-> Option<NativeMaintenanceTemperatureIncumbent> {
    let mut loader = native_installed_api_runtime_loader().ok()?;
    let authority = match loader.load_temperature().ok()? {
        Loaded::Legacy {
            value: Legacy::Active(_),
            comparatorAuthority: FieldPresence::Value(authority),
        } => authority,
        Loaded::MaintenanceActive(active) => active.comparatorAuthority,
        _ => return None,
    };
    let Nullable::Value(artifact_base64) = authority.artifactBase64 else {
        return None;
    };
    let Nullable::Value(artifact_identity_sha256) = authority.artifactIdentitySha256 else {
        return None;
    };
    let Nullable::Value(artifact_member_sha256) = authority.artifactMemberSha256 else {
        return None;
    };
    let wire = NativeWirePrimitives;
    let artifact_bytes = wire.decode_standard_base64(&artifact_base64)?;
    // verify the actual serving member before exposing its original model JSON
    if !valid_hash(&artifact_identity_sha256)
        || format_digest(wire.sha256(&artifact_bytes)) != artifact_member_sha256
    {
        return None;
    }
    let document = wire.decode_json(&artifact_bytes)?;
    let model = field(&document, "model")?.clone();
    let authority_kind = weather_contracts::MaintenanceShadowComparatorAuthorityKind::WIRE_VALUES
        .iter()
        .find(|(_, kind)| *kind == authority.authorityKind)?
        .0
        .to_owned();
    Some(NativeMaintenanceTemperatureIncumbent {
        artifact_bytes,
        artifact_identity_sha256,
        authority_kind,
        family: NativeMaintenanceHistoricalFamily::Temperature,
        model,
        state: "active".to_owned(),
    })
}
