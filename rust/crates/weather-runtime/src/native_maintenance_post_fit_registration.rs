//! Source-exact post-fit rolling registration material built over a verified future-only epoch.

use weather_contracts::{AdjustmentShadowFamily, JsonValue};
use weather_edge::{ArchiveWirePrimitives, RevisionArchiveCanonicalCodec};

use crate::{
    NativeAdjustmentRegistrationCohortDescriptor, NativeAdjustmentRegistrationCohortInput,
    NativeAdjustmentRegistrationPolicyDescriptor, NativeAdjustmentRollingShadowRegistration,
    NativeAdjustmentRollingShadowRegistrationInput, NativeAdjustmentRollingWindow,
    NativeAdjustmentRollingWindowInput, NativeMaintenanceCaptureFamily,
    NativeMaintenanceExpectedKeyPlan, NativeWirePrimitives,
    VerifiedNativeMaintenanceCaptureEpochWitness, adjustment_rolling_local_date_at,
    build_adjustment_rolling_window, build_native_adjustment_registration_cohort_descriptor,
    build_native_adjustment_registration_policy_descriptor,
    build_native_adjustment_rolling_shadow_registration,
    build_native_maintenance_expected_key_plan,
    maintenance_canonical_json::valid_hash,
    native_maintenance_policy_comparison::{object, string},
    native_maintenance_retirement_files_v2::format_digest,
};

/// Immutable post-fit source inputs; no database or publication effects occur during assembly.
pub struct NativeMaintenancePostFitRegistrationInput<'a> {
    pub artifact_sha256: &'a str,
    pub candidate_sha256: &'a str,
    pub cutoff_at: &'a str,
    pub due_month: &'a str,
    pub witness: &'a VerifiedNativeMaintenanceCaptureEpochWitness,
    pub family: NativeMaintenanceCaptureFamily,
    pub historical_member_root_sha256: &'a str,
    pub input_manifest_sha256: &'a str,
    pub predecessor_registration_sha256: Option<&'a str>,
    pub predecessor_terminal_at: Option<&'a str>,
    pub requested_at: &'a str,
    pub source_identity_sha256: &'a str,
}

/// Byte-exact original component descriptors and their post-fit identity binding.
pub struct NativeMaintenancePostFitRegistration {
    pub cohort: NativeAdjustmentRegistrationCohortDescriptor,
    pub confirmation: JsonValue,
    pub expected_plan: NativeMaintenanceExpectedKeyPlan,
    pub policy: NativeAdjustmentRegistrationPolicyDescriptor,
    pub schedule: NativeAdjustmentRollingWindow,
    pub shadow: NativeAdjustmentRollingShadowRegistration,
    pub source_descriptor: JsonValue,
    pub source_bytes: Vec<u8>,
    pub source_sha256: String,
}

/// Complete the original registration schedule, key plan, cohort and source identity.
pub fn build_native_maintenance_post_fit_registration(
    input: NativeMaintenancePostFitRegistrationInput<'_>,
) -> Option<NativeMaintenancePostFitRegistration> {
    let wire = NativeWirePrimitives;
    // validate all independent source commitments before deriving a new registration
    if ![
        input.artifact_sha256,
        input.candidate_sha256,
        input.historical_member_root_sha256,
        input.input_manifest_sha256,
        input.source_identity_sha256,
        &input.witness.witness_sha256,
    ]
    .into_iter()
    .all(valid_hash)
        || input
            .predecessor_registration_sha256
            .is_some_and(|value| !valid_hash(value))
    {
        return None;
    }
    wire.canonical_utc_milliseconds(input.cutoff_at)?;
    let family = match input.family {
        NativeMaintenanceCaptureFamily::Temperature => AdjustmentShadowFamily::Temperature,
        NativeMaintenanceCaptureFamily::Wind => AdjustmentShadowFamily::Wind,
        NativeMaintenanceCaptureFamily::Rain => AdjustmentShadowFamily::Rain,
    };
    let schedule = build_adjustment_rolling_window(NativeAdjustmentRollingWindowInput {
        epoch_at: &input.witness.epoch_at,
        epoch_witness_sha256: &input.witness.witness_sha256,
        family: input.family.wire(),
        fit_month: input.due_month,
        predecessor_terminal_at: input.predecessor_terminal_at,
        requested_at: input.requested_at,
    })
    .ok()?;
    let expected_plan = build_native_maintenance_expected_key_plan(
        input.family,
        &schedule.interval_start_at,
        &schedule.interval_end_at,
    )?;
    let policy =
        build_native_adjustment_registration_policy_descriptor(family, &mut NativeWirePrimitives)
            .ok()?;
    let cohort = build_native_adjustment_registration_cohort_descriptor(
        NativeAdjustmentRegistrationCohortInput {
            cutoff_at: input.cutoff_at,
            due_month: input.due_month,
            epoch_witness_sha256: &input.witness.witness_sha256,
            family,
            historical_member_root_sha256: input.historical_member_root_sha256,
            input_manifest_sha256: input.input_manifest_sha256,
        },
        &mut NativeWirePrimitives,
        &NativeWirePrimitives,
    )
    .ok()?;
    let source_descriptor = object(vec![
        (
            "contractVersion",
            string("forecast-adjustment-future-only-source-lineage/v1"),
        ),
        ("epochWitnessSha256", string(&input.witness.witness_sha256)),
        ("family", string(input.family.wire())),
        ("sourceIdentitySha256", string(input.source_identity_sha256)),
    ]);
    let source_bytes = wire.encode_sorted_json(&source_descriptor)?;
    let source_sha256 = format_digest(wire.sha256(&source_bytes));
    let shadow = build_native_adjustment_rolling_shadow_registration(
        NativeAdjustmentRollingShadowRegistrationInput {
            artifact_sha256: input.artifact_sha256,
            candidate_sha256: input.candidate_sha256,
            cohort_sha256: &cohort.cohort_sha256,
            family,
            policy_sha256: &policy.policy_sha256,
            predecessor_registration_sha256: input.predecessor_registration_sha256,
            reserved_key_sha256: &expected_plan.reserved_key_sha256,
            source_sha256: &source_sha256,
        },
        &schedule,
        &mut NativeWirePrimitives,
        &NativeWirePrimitives,
    )
    .ok()?;
    let candidate_kind = match input.family {
        NativeMaintenanceCaptureFamily::Temperature => "temperature-delayed-mos/v1",
        NativeMaintenanceCaptureFamily::Wind => "wind-robust-hierarchical-median/v1",
        NativeMaintenanceCaptureFamily::Rain => "rain-hurdle-wind-occurrence-amount/v1",
    };
    let confirmation = object(vec![
        ("candidateKind", string(candidate_kind)),
        ("candidateSha256", string(input.candidate_sha256)),
        ("cohortLineageSha256", string(&cohort.cohort_sha256)),
        ("family", string(input.family.wire())),
        ("firstTargetAt", string(&schedule.interval_start_at)),
        ("gateManifestSha256", string(&policy.policy_sha256)),
        (
            "intervalEndExclusiveLocalDate",
            string(&adjustment_rolling_local_date_at(&schedule.interval_end_at).ok()?),
        ),
        (
            "intervalStartLocalDate",
            string(&adjustment_rolling_local_date_at(&schedule.interval_start_at).ok()?),
        ),
        (
            "reservedKeySha256",
            string(&expected_plan.reserved_key_sha256),
        ),
        ("sourceLineageSha256", string(&source_sha256)),
        ("terminalAccessAt", string(&schedule.terminal_at)),
    ]);
    Some(NativeMaintenancePostFitRegistration {
        cohort,
        confirmation,
        expected_plan,
        policy,
        schedule,
        shadow,
        source_descriptor,
        source_bytes,
        source_sha256,
    })
}
