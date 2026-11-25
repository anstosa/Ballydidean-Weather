//! Source daily family selection and pending evidence receipt.

use crate::{
    NativeMaintenanceCaptureFamily, NativeMaintenanceDailyEvaluation, NativeMaintenanceDue,
    NativeMaintenanceDueMode, NativeRegistrationLifecycleEntry, NativeRegistrationLifecycleStatus,
    NativeRegistrationSlotState, NativeWirePrimitives,
    VerifiedNativeMaintenanceCaptureEpochWitness,
    maintenance_canonical_json::valid_hash,
    native_maintenance_policy_comparison::{object, string},
    native_maintenance_retirement_files_v2::format_digest,
    verify_native_maintenance_due,
};
use weather_contracts::JsonValue;
use weather_edge::{ArchiveWirePrimitives, RevisionArchiveCanonicalCodec};

/// Select one authenticated occupied v3 registration with terminal eligibility and exact tie order.
pub fn select_native_maintenance_daily_lifecycle_entry<'a>(
    lifecycle: &'a NativeRegistrationLifecycleStatus,
    clock_at: &str,
) -> Option<Option<&'a NativeRegistrationLifecycleEntry>> {
    let wire = NativeWirePrimitives;
    let now = wire.canonical_utc_milliseconds(clock_at)?;
    let mut best = None::<((bool, i64, u8), &NativeRegistrationLifecycleEntry)>;
    // process eligible terminals first then earliest terminal then fixed family order
    for entry in &lifecycle.entries {
        if entry.slot.state != NativeRegistrationSlotState::BusyV3 {
            continue;
        }
        let Some(registration) = &entry.active_registration else {
            continue;
        };
        let terminal = wire.canonical_utc_milliseconds(&registration.terminal_at)?;
        let family = match entry.slot.family {
            NativeMaintenanceCaptureFamily::Temperature => 0,
            NativeMaintenanceCaptureFamily::Wind => 1,
            NativeMaintenanceCaptureFamily::Rain => 2,
        };
        let rank = (terminal > now, terminal, family);
        if best.as_ref().is_none_or(|(current, _)| rank < *current) {
            best = Some((rank, entry));
        }
    }
    Some(best.map(|(_, entry)| entry))
}

/// Construct the exact pending daily semantic receipt after the actual catalog and epoch admission.
pub fn evaluate_native_pending_maintenance_daily(
    catalog_input_manifest_sha256: &str,
    clock_at: &str,
    due: &NativeMaintenanceDue,
    witness: &VerifiedNativeMaintenanceCaptureEpochWitness,
    family: NativeMaintenanceCaptureFamily,
    registration_sha256: Option<&str>,
) -> Option<NativeMaintenanceDailyEvaluation> {
    let wire = NativeWirePrimitives;
    // bind the original future-only witness and exact daily cutoff projection
    if !valid_hash(catalog_input_manifest_sha256)
        || !valid_hash(&witness.witness_sha256)
        || registration_sha256.is_some_and(|hash| !valid_hash(hash))
        || due.mode != NativeMaintenanceDueMode::Daily
        || !verify_native_maintenance_due(due)
    {
        return None;
    }
    wire.canonical_utc_milliseconds(clock_at)?;
    let due_value = object(vec![
        ("dueKey", string(&due.due_key)),
        ("family", JsonValue::Null),
        ("mode", string("daily")),
        ("originalCutoffAt", string(&due.original_cutoff_at)),
        ("scope", string("daily")),
    ]);
    let material = object(vec![
        (
            "catalogInputManifestSha256",
            string(catalog_input_manifest_sha256),
        ),
        ("due", due_value),
        ("epochWitnessSha256", string(&witness.witness_sha256)),
        ("family", string(family.wire())),
        (
            "registrationSha256",
            registration_sha256.map_or(JsonValue::Null, string),
        ),
        ("state", string("pending")),
    ]);
    Some(NativeMaintenanceDailyEvaluation::Pending {
        semantic_input_sha256: format_digest(wire.sha256(&wire.encode_sorted_json(&material)?)),
    })
}
