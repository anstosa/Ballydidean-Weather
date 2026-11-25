//! Source-specific archive grammar for rolling registration and independently derived descriptors.

use weather_contracts::{AdjustmentShadowFamily, JsonValue};
use weather_edge::RevisionArchiveCanonicalCodec;

use crate::{
    ADJUSTMENT_ROLLING_SCHEDULE_SHA256, MaintenanceCanonicalJson,
    NativeAdjustmentRegistrationCohortInput, NativeMaintenanceCaptureFamily, NativeWirePrimitives,
    build_native_adjustment_registration_cohort_descriptor,
    build_native_adjustment_registration_policy_descriptor,
    build_native_maintenance_expected_key_plan,
    maintenance_canonical_json::{exact_keys, field, text, valid_hash},
    native_maintenance_retirement_files_v2::format_digest,
};

/// Return the predecessor's exact member-specific size ceiling for each registration kind.
pub fn native_maintenance_archive_registration_maximum(
    kind: &str,
) -> Option<(&'static str, usize)> {
    match kind {
        "adjustment-shadow-registration/v3" => {
            Some(("adjustment-shadow-registration/v3", 16 * 1024))
        }
        "adjustment-registration-policy/v3" => {
            Some(("adjustment-registration-policy/v3", 16 * 1024))
        }
        "adjustment-registration-cohort/v3" => {
            Some(("adjustment-registration-cohort/v3", 16 * 1024))
        }
        "adjustment-registration-expected-key-plan/v3" => {
            Some(("adjustment-registration-expected-key-plan/v3", 256 * 1024))
        }
        _ => None,
    }
}

/// Recompute exact registration descriptors rather than relying on their claimed public digest.
pub fn validate_native_maintenance_archive_registration(kind: &str, bytes: &[u8]) -> Option<()> {
    let (_, maximum) = native_maintenance_archive_registration_maximum(kind)?;
    if bytes.is_empty() || bytes.len() > maximum {
        return None;
    }
    let value = NativeWirePrimitives.parse_canonical_object(bytes)?;
    let (family, capture_family) = match text(&value, "family")?.as_str() {
        "temperature" => (
            AdjustmentShadowFamily::Temperature,
            NativeMaintenanceCaptureFamily::Temperature,
        ),
        "wind" => (
            AdjustmentShadowFamily::Wind,
            NativeMaintenanceCaptureFamily::Wind,
        ),
        "rain" => (
            AdjustmentShadowFamily::Rain,
            NativeMaintenanceCaptureFamily::Rain,
        ),
        _ => return None,
    };
    match kind {
        "adjustment-shadow-registration/v3" => validate_rolling_shadow(&value),
        "adjustment-registration-policy/v3" => {
            let exact = build_native_adjustment_registration_policy_descriptor(
                family,
                &mut NativeWirePrimitives,
            )
            .ok()?;
            (exact.bytes == bytes).then_some(())
        }
        "adjustment-registration-cohort/v3" => {
            let exact = build_native_adjustment_registration_cohort_descriptor(
                NativeAdjustmentRegistrationCohortInput {
                    cutoff_at: &text(&value, "cutoffAt")?,
                    due_month: &text(&value, "dueMonth")?,
                    epoch_witness_sha256: &text(&value, "epochWitnessSha256")?,
                    family,
                    historical_member_root_sha256: &text(&value, "historicalMemberRootSha256")?,
                    input_manifest_sha256: &text(&value, "inputManifestSha256")?,
                },
                &mut NativeWirePrimitives,
                &NativeWirePrimitives,
            )
            .ok()?;
            (exact.bytes == bytes).then_some(())
        }
        "adjustment-registration-expected-key-plan/v3" => {
            let exact = build_native_maintenance_expected_key_plan(
                capture_family,
                &text(&value, "intervalStartAt")?,
                &text(&value, "intervalEndAt")?,
            )?;
            (exact.bytes == bytes).then_some(())
        }
        _ => None,
    }
}

/// Validate the exact registration's raw v3 identity and public source schedule.
fn validate_rolling_shadow(value: &JsonValue) -> Option<()> {
    const KEYS: &[&str] = &[
        "artifactSha256",
        "candidateSha256",
        "cohortSha256",
        "epochWitnessSha256",
        "family",
        "intervalEndAt",
        "intervalStartAt",
        "policySha256",
        "predecessorRegistrationSha256",
        "registrationSha256",
        "reservedKeySha256",
        "scheduleContractSha256",
        "siteKey",
        "sourceSha256",
        "targetCutoffAt",
        "terminalAt",
    ];
    let wire = NativeWirePrimitives;
    if !exact_keys(value, KEYS)
        || text(value, "siteKey")? != "ballydidean"
        || text(value, "scheduleContractSha256")? != ADJUSTMENT_ROLLING_SCHEDULE_SHA256
    {
        return None;
    }
    for key in [
        "artifactSha256",
        "candidateSha256",
        "cohortSha256",
        "epochWitnessSha256",
        "policySha256",
        "registrationSha256",
        "reservedKeySha256",
        "scheduleContractSha256",
        "sourceSha256",
    ] {
        if !valid_hash(&text(value, key)?) {
            return None;
        }
    }
    for key in [
        "intervalEndAt",
        "intervalStartAt",
        "targetCutoffAt",
        "terminalAt",
    ] {
        wire.canonical_utc_milliseconds(&text(value, key)?)?;
    }
    let predecessor = match field(value, "predecessorRegistrationSha256")? {
        JsonValue::Null => "none".to_owned(),
        _ => {
            let hash = text(value, "predecessorRegistrationSha256")?;
            if !valid_hash(&hash) {
                return None;
            }
            hash
        }
    };
    let preimage = [
        "adjustment-shadow-registration/v3".to_owned(),
        text(value, "siteKey")?,
        text(value, "family")?,
        text(value, "candidateSha256")?,
        text(value, "artifactSha256")?,
        text(value, "policySha256")?,
        text(value, "cohortSha256")?,
        text(value, "reservedKeySha256")?,
        text(value, "sourceSha256")?,
        text(value, "epochWitnessSha256")?,
        text(value, "scheduleContractSha256")?,
        predecessor,
        text(value, "intervalStartAt")?,
        text(value, "intervalEndAt")?,
        text(value, "targetCutoffAt")?,
        text(value, "terminalAt")?,
    ]
    .join("\n")
        + "\n";
    (format_digest(RevisionArchiveCanonicalCodec::sha256(
        &wire,
        preimage.as_bytes(),
    )) == text(value, "registrationSha256")?)
    .then_some(())
}
