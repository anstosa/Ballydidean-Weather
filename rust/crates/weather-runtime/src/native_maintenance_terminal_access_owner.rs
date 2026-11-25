//! Concrete daily terminal plan publication, local burn and owner-native access composition.

use weather_contracts::JsonValue;
use weather_edge::{ArchiveWirePrimitives, RevisionArchiveCanonicalCodec};

use crate::{
    NativeAdjustmentRollingShadowRegistration, NativeMaintenanceArchiveSegmentArchive,
    NativeMaintenanceArchiveSegmentClock, NativeMaintenanceConfirmationAccessInput,
    NativeMaintenanceConfirmationAccessPort, NativeMaintenanceDailyTerminalPreparation,
    NativeMaintenanceDailyTerminalPreparationInput, NativeMaintenanceDue,
    NativeMaintenanceHistoricalCatalogReady, NativeMaintenanceJournalStore,
    NativeMaintenanceRevisionSnapshotRequest, NativeRegistrationActiveRegistration,
    NativeRegistrationLifecycleEntry, NativeWirePrimitives,
    VerifiedNativeMaintenanceCaptureEpochWitness, archive_native_maintenance_confirmation_plan,
    build_native_maintenance_confirmation_access_request, burn_native_maintenance_confirmation,
    burn_native_maintenance_confirmation_access,
    maintenance_canonical_json::{field, text, valid_hash},
    native_maintenance_policy_comparison::{object, string},
    native_maintenance_retirement_files_v2::format_digest,
    prepare_native_maintenance_daily_terminal_material,
    read_native_maintenance_confirmation_custody,
    read_native_maintenance_confirmation_registration, record_native_maintenance_revision_snapshot,
};

/// Verified controller context for one genuine daily terminal access attempt.
pub struct NativeMaintenanceTerminalAccessInput<'a> {
    pub clock_at: &'a str,
    pub due: &'a NativeMaintenanceDue,
    pub catalog: &'a NativeMaintenanceHistoricalCatalogReady,
    pub witness: &'a VerifiedNativeMaintenanceCaptureEpochWitness,
    pub witness_bytes: &'a [u8],
    pub entry: &'a NativeRegistrationLifecycleEntry,
    pub source_commit: &'a str,
}

/// Exact local and external owner access receipts before post-burn value assembly.
pub struct NativeMaintenanceBurnedTerminalAccess {
    pub unsupported: bool,
    pub plan: JsonValue,
    pub confirmation_registration: JsonValue,
    pub shadow_registration: NativeAdjustmentRollingShadowRegistration,
    pub local_burn: JsonValue,
    pub native_access: JsonValue,
    pub plan_graph_manifest_sha256: String,
    pub source_commit: String,
}

/// Unopened source input or actually burned supported/unsupported material.
pub enum NativeMaintenanceTerminalAccessOutcome {
    Pending(JsonValue),
    Burned(Box<NativeMaintenanceBurnedTerminalAccess>),
}

/// Closed source terminal effect failures, never carrying unvalidated value-bearing data.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMaintenanceTerminalAccessError {
    Input,
    LocalRegistration,
    Plan,
    Archive,
    Journal,
    Custody,
    Remote,
}

/// Run original source order: blinded plan, verified archive, snapshot, local burn and native burn.
pub fn prepare_native_maintenance_terminal_access<Archive, Remote>(
    input: NativeMaintenanceTerminalAccessInput<'_>,
    journal: &mut NativeMaintenanceJournalStore,
    archive: &mut Archive,
    remote: &mut Remote,
) -> Result<NativeMaintenanceTerminalAccessOutcome, NativeMaintenanceTerminalAccessError>
where
    Archive: NativeMaintenanceArchiveSegmentArchive,
    Remote: NativeMaintenanceConfirmationAccessPort,
{
    use NativeMaintenanceTerminalAccessError as Error;
    let wire = NativeWirePrimitives;
    wire.canonical_utc_milliseconds(input.clock_at)
        .ok_or(Error::Input)?;
    let active = input
        .entry
        .active_registration
        .as_ref()
        .ok_or(Error::Input)?;
    let family = active.family;
    let local = read_native_maintenance_confirmation_registration(
        journal,
        family,
        &active.candidate_sha256,
    )
    .map_err(|_| Error::LocalRegistration)?;
    let witness_json = wire.decode_json(input.witness_bytes).ok_or(Error::Input)?;
    let shadow_registration = project_native_terminal_shadow_registration(active);
    let preparation = prepare_native_maintenance_daily_terminal_material(
        NativeMaintenanceDailyTerminalPreparationInput {
            catalog_input_manifest_sha256: &input.catalog.input_manifest_sha256,
            clock_at: input.clock_at,
            confirmation_registration: local.as_ref(),
            due: input.due,
            epoch_witness: &witness_json,
            history: &input.catalog.history,
            shadow_registration: &shadow_registration,
            source_commit: input.source_commit,
            verified_epoch_witness: input.witness,
        },
    )
    .map_err(|_| Error::Plan)?;
    let (unsupported, plan) = match preparation {
        NativeMaintenanceDailyTerminalPreparation::Pending {
            daily_evaluation_input,
        } => {
            return Ok(NativeMaintenanceTerminalAccessOutcome::Pending(
                daily_evaluation_input,
            ));
        }
        NativeMaintenanceDailyTerminalPreparation::Supported { plan } => (false, plan),
        NativeMaintenanceDailyTerminalPreparation::Unsupported { plan } => (true, plan),
    };
    let confirmation_registration = local.ok_or(Error::LocalRegistration)?;
    let registration_sha256 = text(&confirmation_registration, "registrationSha256")
        .filter(|hash| valid_hash(hash))
        .ok_or(Error::LocalRegistration)?;
    let plan_graph = field(&plan, "planGraph").ok_or(Error::Plan)?;
    // use the same authenticated controller snapshot clock for each source terminal custody mutation
    let mut clock = PinnedControllerClock(input.clock_at);
    let publication =
        archive_native_maintenance_confirmation_plan(plan_graph, archive, journal, &mut clock)
            .ok_or(Error::Archive)?;
    let plan_graph_manifest_sha256 = format_digest(publication.manifest_object_sha256);
    let entry_count = match field(&plan, "entryCount") {
        Some(JsonValue::Number(count))
            if *count >= 0.0 && *count <= 9_500_000.0 && count.fract() == 0.0 =>
        {
            *count as u64
        }
        _ => return Err(Error::Plan),
    };
    record_native_maintenance_revision_snapshot(
        journal,
        NativeMaintenanceRevisionSnapshotRequest {
            entry_count,
            expected_key_set_sha256: &text(&plan, "expectedKeySetSha256").ok_or(Error::Plan)?,
            family,
            now: input.clock_at,
            registration_sha256: &registration_sha256,
            revision_catalog_watermark_sha256: &text(&plan, "revisionCatalogWatermarkSha256")
                .ok_or(Error::Plan)?,
            snapshot_root_sha256: &text(&plan, "snapshotRootSha256").ok_or(Error::Plan)?,
            target_cutoff_at: &text(&plan, "targetCutoffAt").ok_or(Error::Plan)?,
        },
    )
    .map_err(|_| Error::Journal)?;
    let local_burn =
        burn_native_maintenance_confirmation(journal, family, &registration_sha256, input.clock_at)
            .map_err(|_| Error::Journal)?;
    let custody = read_native_maintenance_confirmation_custody(journal)
        .map_err(|_| Error::Custody)?
        .ok_or(Error::Custody)?;
    let request = build_native_maintenance_confirmation_access_request(
        NativeMaintenanceConfirmationAccessInput {
            custody: &custody,
            eligible_prediction_set_sha256: &text(&plan, "eligiblePredictionSetSha256")
                .ok_or(Error::Plan)?,
            full_graph_verified_at: input.clock_at,
            graph_manifest_sha256: &plan_graph_manifest_sha256,
            confirmation_registration: &confirmation_registration,
            epoch_witness_bytes: input.witness_bytes,
            local_burn: &local_burn,
            metadata: input.entry.metadata.as_ref().ok_or(Error::Custody)?,
            shadow_registration: &shadow_registration.registration,
            source_commit: input.source_commit,
        },
    )
    .ok_or(Error::Remote)?;
    let owner_access =
        burn_native_maintenance_confirmation_access(&request, remote).map_err(|_| Error::Remote)?;
    Ok(NativeMaintenanceTerminalAccessOutcome::Burned(Box::new(
        NativeMaintenanceBurnedTerminalAccess {
            unsupported,
            plan,
            confirmation_registration,
            shadow_registration,
            local_burn,
            native_access: owner_access.native_access,
            plan_graph_manifest_sha256,
            source_commit: input.source_commit.to_owned(),
        },
    )))
}

/// Reconstruct the exact public shadow registration from its separately verified database transaction.
pub fn project_native_terminal_shadow_registration(
    active: &NativeRegistrationActiveRegistration,
) -> NativeAdjustmentRollingShadowRegistration {
    let registration = object(vec![
        ("artifactSha256", string(&active.artifact_sha256)),
        ("candidateSha256", string(&active.candidate_sha256)),
        ("cohortSha256", string(&active.cohort_sha256)),
        ("epochWitnessSha256", string(&active.epoch_witness_sha256)),
        ("family", string(active.family.wire())),
        ("intervalEndAt", string(&active.interval_end_at)),
        ("intervalStartAt", string(&active.interval_start_at)),
        ("policySha256", string(&active.policy_sha256)),
        (
            "predecessorRegistrationSha256",
            active
                .predecessor_registration_sha256
                .as_deref()
                .map_or(JsonValue::Null, string),
        ),
        ("registrationSha256", string(&active.registration_sha256)),
        ("reservedKeySha256", string(&active.reserved_key_sha256)),
        (
            "scheduleContractSha256",
            string(&active.schedule_contract_sha256),
        ),
        ("siteKey", string(&active.site_key)),
        ("sourceSha256", string(&active.source_sha256)),
        ("targetCutoffAt", string(&active.target_cutoff_at)),
        ("terminalAt", string(&active.terminal_at)),
    ]);
    // the database returns the original public registration rather than a new rolling schedule derivation
    NativeAdjustmentRollingShadowRegistration {
        plan: JsonValue::Null,
        registration,
        registration_sha256: active.registration_sha256.clone(),
    }
}

struct PinnedControllerClock<'a>(&'a str);
impl NativeMaintenanceArchiveSegmentClock for PinnedControllerClock<'_> {
    /// Retain the original authenticated controller invocation clock throughout terminal effects.
    fn now(&mut self) -> Option<String> {
        Some(self.0.to_owned())
    }
}
