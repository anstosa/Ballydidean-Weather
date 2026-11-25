//! Full native daily terminal replay from admitted chunks to canonical policy decisions.

use crate::{
    NativeMaintenanceCaptureFamily, NativeMaintenanceDailyCandidateReplayPair,
    NativeMaintenanceDailyChunkInput, NativeMaintenanceDailyEvaluation,
    NativeMaintenanceDailyScoringMaterial, NativeMaintenanceDue,
    finalize_native_maintenance_daily_scoring, maintenance_canonical_json::text,
    project_native_maintenance_daily_policy_row, replay_native_maintenance_daily_candidate_pair,
    validate_native_maintenance_daily_evaluation_input,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMaintenanceDailyEvaluatorError {
    Admission,
    Replay,
    PolicyRow,
    Scoring,
}

/// Replay each verified raw and packaged candidate independently and compute the original terminal policy.
pub fn evaluate_native_maintenance_daily_terminal(
    due: &NativeMaintenanceDue,
    catalog_input_manifest_sha256: &str,
    input: NativeMaintenanceDailyChunkInput<'_>,
) -> Result<NativeMaintenanceDailyEvaluation, NativeMaintenanceDailyEvaluatorError> {
    use NativeMaintenanceDailyEvaluatorError as Error;
    let material =
        validate_native_maintenance_daily_evaluation_input(NativeMaintenanceDailyChunkInput {
            clock_at: input.clock_at,
            candidate_bytes: input.candidate_bytes,
            artifact_bytes: input.artifact_bytes,
            confirmation_registration: input.confirmation_registration,
            access: input.access,
            shadow_registration: input.shadow_registration,
            chunks: input.chunks,
            full_manifest: input.full_manifest,
            witness: input.witness,
        })
        .ok_or(Error::Admission)?;
    let mut rows = Vec::with_capacity(material.records.len());
    let mut last_replay: Option<(&str, usize, NativeMaintenanceDailyCandidateReplayPair)> = None;
    // retain only the previous complete deterministic replay to avoid repeating a whole rain capsule
    for record in &material.records {
        let row_identity = match record.family {
            NativeMaintenanceCaptureFamily::Rain => 0,
            _ => record.row_index,
        };
        if last_replay.as_ref().is_none_or(|(capsule, row_index, _)| {
            *capsule != record.capsule_member_sha256 || *row_index != row_identity
        }) {
            let pair = replay_native_maintenance_daily_candidate_pair(
                record,
                input.candidate_bytes,
                input.artifact_bytes,
            )
            .map_err(|_| Error::Replay)?;
            last_replay = Some((&record.capsule_member_sha256, row_identity, pair));
        }
        let (_, _, pair) = last_replay.as_ref().ok_or(Error::Replay)?;
        // project raw and packaged byte agreement against the independently retained prediction body
        let row = project_native_maintenance_daily_policy_row(
            record,
            &pair.candidate_decision_bytes,
            &pair.packaged_decision_bytes,
        )
        .ok_or(Error::PolicyRow)?;
        rows.push(row.value);
    }
    finalize_native_maintenance_daily_scoring(NativeMaintenanceDailyScoringMaterial {
        catalog_input_manifest_sha256,
        due,
        witness: input.witness,
        family: material.family,
        candidate_sha256: &text(input.shadow_registration, "candidateSha256")
            .ok_or(Error::Admission)?,
        confirmation_registration_sha256: &text(
            input.confirmation_registration,
            "registrationSha256",
        )
        .ok_or(Error::Admission)?,
        registration_sha256: &text(input.shadow_registration, "registrationSha256")
            .ok_or(Error::Admission)?,
        full_member_root_sha256: &material.full_member_root_sha256,
        target_cutoff_at: &text(input.access, "targetCutoffAt").ok_or(Error::Admission)?,
        rows: &rows,
    })
    .ok_or(Error::Scoring)
}
