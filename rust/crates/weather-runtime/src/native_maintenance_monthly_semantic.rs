//! Installed monthly semantic inspection composing genuine historical rows, parity and readiness.

use weather_contracts::JsonValue;
use weather_edge::ArchiveWirePrimitives;

use crate::{
    NativeMaintenanceAdmittedFitRuntimeReadiness, NativeMaintenanceCaptureFamily,
    NativeMaintenanceDue, NativeMaintenanceDueMode, NativeMaintenanceDueRegistrationSlot,
    NativeMaintenanceDueRegistrationSlotState, NativeMaintenanceFitInput,
    NativeMaintenanceFitRuntimeReadiness, NativeMaintenanceHistoricalCatalogReady,
    NativeMaintenanceHistoricalFamily, NativeMaintenanceMonthlyFitOutcome,
    NativeMaintenanceMonthlyReadyInspection, NativeMaintenanceRainMonthlyFitOutcome,
    NativeMaintenanceRegistrationContext, NativeMaintenanceSemanticInspection,
    NativeRegistrationLifecycleStatus, NativeRegistrationScheduleStatus,
    NativeRegistrationSlotState, NativeWirePrimitives,
    VerifiedNativeMaintenanceCaptureEpochWitness,
    build_native_adjustment_historical_fit_projection,
    build_native_adjustment_monthly_fit_assembly,
    build_native_adjustment_rain_monthly_fit_assembly, build_native_maintenance_fit_parity_inputs,
    load_native_maintenance_temperature_incumbent,
    native_maintenance_policy_comparison::{object, string},
    verify_native_maintenance_due,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMaintenanceMonthlySemanticError {
    Input,
    History,
    Incumbent,
    Assembly,
    Parity,
    Runtime,
    Registration,
}

/// Assemble the exact original monthly source inspection using the actual installed model runtime.
pub fn inspect_native_maintenance_monthly_semantics(
    source_commit: &str,
    due: &NativeMaintenanceDue,
    witness: &VerifiedNativeMaintenanceCaptureEpochWitness,
    catalog: &NativeMaintenanceHistoricalCatalogReady,
    schedule: &NativeRegistrationScheduleStatus,
    lifecycle: &NativeRegistrationLifecycleStatus,
) -> Result<NativeMaintenanceSemanticInspection, NativeMaintenanceMonthlySemanticError> {
    use NativeMaintenanceMonthlySemanticError as Error;
    // require exactly one fixed original monthly due and its authenticated rolling lifecycle
    if due.mode != NativeMaintenanceDueMode::Monthly
        || !verify_native_maintenance_due(due)
        || schedule.epoch_witness_sha256 != witness.witness_sha256
        || lifecycle.epoch_witness_sha256 != witness.witness_sha256
    {
        return Err(Error::Input);
    }
    let family = due.family.ok_or(Error::Input)?;
    let fit_month = due.due_key.rsplit('/').next().ok_or(Error::Input)?;
    let historical_family = match family {
        NativeMaintenanceCaptureFamily::Temperature => {
            NativeMaintenanceHistoricalFamily::Temperature
        }
        NativeMaintenanceCaptureFamily::Wind => NativeMaintenanceHistoricalFamily::Wind,
        NativeMaintenanceCaptureFamily::Rain => NativeMaintenanceHistoricalFamily::Rain,
    };
    let history = build_native_adjustment_historical_fit_projection(
        witness,
        historical_family,
        fit_month,
        &catalog.history,
    )
    .map_err(|_| Error::History)?;
    let (fit_input, input_manifest, historical_root, cutoff) = match family {
        NativeMaintenanceCaptureFamily::Rain => {
            match build_native_adjustment_rain_monthly_fit_assembly(fit_month, witness, &history)
                .map_err(|_| Error::Assembly)?
            {
                NativeMaintenanceRainMonthlyFitOutcome::NoCandidate { common, .. } => {
                    return Ok(NativeMaintenanceSemanticInspection::MonthlyNoCandidate {
                        input_manifest_sha256: common.historical_member_root_sha256,
                    });
                }
                NativeMaintenanceRainMonthlyFitOutcome::Ready(assembly) => (
                    assembly.fit_input,
                    assembly.input_manifest_sha256,
                    assembly.common.historical_member_root_sha256,
                    assembly.common.cutoff_at,
                ),
            }
        }
        NativeMaintenanceCaptureFamily::Temperature | NativeMaintenanceCaptureFamily::Wind => {
            // complete missing source classes before loading the installed temperature comparator
            let counts = history.class_counts;
            let required = counts.actual_best_match > 0
                && counts.target_revision > 0
                && (family != NativeMaintenanceCaptureFamily::Temperature
                    || counts.native_source > 0);
            if !required {
                return Ok(NativeMaintenanceSemanticInspection::MonthlyNoCandidate {
                    input_manifest_sha256: history.member_root_sha256,
                });
            }
            let incumbent = if family == NativeMaintenanceCaptureFamily::Temperature {
                Some(load_native_maintenance_temperature_incumbent().ok_or(Error::Incumbent)?)
            } else {
                None
            };
            match build_native_adjustment_monthly_fit_assembly(
                fit_month,
                witness,
                historical_family,
                &history,
                incumbent.as_ref(),
            )
            .map_err(|_| Error::Assembly)?
            {
                NativeMaintenanceMonthlyFitOutcome::NoCandidate {
                    historical_member_root_sha256,
                    ..
                } => {
                    return Ok(NativeMaintenanceSemanticInspection::MonthlyNoCandidate {
                        input_manifest_sha256: historical_member_root_sha256,
                    });
                }
                NativeMaintenanceMonthlyFitOutcome::Ready(assembly) => (
                    assembly.fit_input,
                    assembly.input_manifest_sha256,
                    assembly.historical_member_root_sha256,
                    assembly.cutoff_at,
                ),
            }
        }
    };
    let parity = build_native_maintenance_fit_parity_inputs(family, &fit_input)
        .map_err(|_| Error::Parity)?
        .ok_or(Error::Parity)?;
    if parity.family != family {
        return Err(Error::Parity);
    }
    // recapture exact fixed installed numerical runtime before spending isolated fit capacity
    let readiness = NativeMaintenanceFitRuntimeReadiness::native_installed(source_commit)
        .and_then(|mut owner| owner.capture())
        .map_err(|_| Error::Runtime)?;
    let runtime_readiness = NativeWirePrimitives
        .decode_json(readiness.manifest.canonical_json().as_bytes())
        .ok_or(Error::Runtime)?;
    let entry = lifecycle
        .entries
        .iter()
        .find(|entry| entry.slot.family == family)
        .ok_or(Error::Registration)?;
    let slot = schedule
        .slots
        .iter()
        .find(|slot| slot.family == family)
        .ok_or(Error::Registration)?;
    let context = NativeMaintenanceRegistrationContext {
        value: object(vec![
            ("cutoffAt", string(&cutoff)),
            ("historicalMemberRootSha256", string(&historical_root)),
            ("inputManifestSha256", string(&input_manifest)),
            (
                "predecessorRegistrationSha256",
                entry
                    .predecessor
                    .as_ref()
                    .map_or(JsonValue::Null, |p| string(&p.registration_sha256)),
            ),
            (
                "predecessorTerminalAt",
                entry
                    .predecessor
                    .as_ref()
                    .map_or(JsonValue::Null, |p| string(&p.terminal_at)),
            ),
        ]),
    };
    let state = match slot.state {
        NativeRegistrationSlotState::BusyV2Legacy => {
            NativeMaintenanceDueRegistrationSlotState::BusyV2Legacy
        }
        NativeRegistrationSlotState::BusyV3 => NativeMaintenanceDueRegistrationSlotState::BusyV3,
        NativeRegistrationSlotState::Free => NativeMaintenanceDueRegistrationSlotState::Free,
    };
    Ok(NativeMaintenanceSemanticInspection::MonthlyReady(
        NativeMaintenanceMonthlyReadyInspection {
            fit_input: NativeMaintenanceFitInput { value: fit_input },
            input_manifest_sha256: input_manifest,
            registration_context: Some(context),
            registration_slot: Some(NativeMaintenanceDueRegistrationSlot { family, state }),
            retained_parity_input: Some(parity.retained_input),
            runtime_readiness: NativeMaintenanceAdmittedFitRuntimeReadiness {
                value: runtime_readiness,
            },
            runtime_readiness_sha256: readiness.readiness_sha256,
            synthetic_parity_input: Some(parity.synthetic_input),
        },
    ))
}
