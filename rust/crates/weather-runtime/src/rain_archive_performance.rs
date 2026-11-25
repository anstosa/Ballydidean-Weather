//! Replay verified archived rain captures into the native performance report.

use std::collections::BTreeMap;

use weather_contracts::{
    JsonValue, Nullable, RainAdjustmentCapture, RainAdjustmentCaptureKindEnum, RainAdjustmentRun,
    RainWindPerformanceResult,
};
use weather_worker::{
    AdmittedRainEdgeEvidence, AdmittedRainPerformanceRun, ForecastAdjustmentScorecardCalendar,
    RainAdjustmentFeatureAdmission, RainAdjustmentTimeAdapter, RainPerformanceBestMatchError,
    RainPerformanceEvidence, RainPerformanceEvidenceError, RainPerformanceEvidenceValues,
    RainPerformanceReportCalendar, RainPerformanceReportError, RainPerformanceReportPackageInputs,
    RainPerformanceTarget, TaskFailure, build_rain_performance_best_match_diagnostic,
    build_rain_performance_evidence, create_rain_performance_report,
    replay_retained_rain_performance,
};

use crate::{
    RainArchiveTargetIdentityValues, WorkerRainAdjustmentCaptureValues,
    project_borrowed_worker_rain_adjustment_inputs, replay_rain_archive_target,
};

const MAXIMUM_PROVIDER_BODY_BYTES: usize = 2_000_000;
const HOUR_MILLISECONDS: i64 = 3_600_000;
const FORECAST_COHORT_LAGS_MS: [i64; 3] = [0, 6 * HOUR_MILLISECONDS, 12 * HOUR_MILLISECONDS];

/// Borrowed last-wins claim and collision-checked body indexes.
type ArchiveMaterialIndex<'a> = (
    BTreeMap<&'a str, &'a AdmittedRainArchiveClaim>,
    BTreeMap<&'a str, &'a AdmittedRainArchiveBody>,
);

/// One typed immutable claim admitted from the verified archive package.
#[derive(Debug, Clone, PartialEq)]
pub struct AdmittedRainArchiveClaim {
    pub id: String,
    pub kind: RainAdjustmentCaptureKindEnum,
    pub run_initialized_at: Nullable<String>,
    pub station_id: Nullable<u64>,
    pub window_end_exclusive: Nullable<String>,
    pub window_start: Nullable<String>,
}

/// One closed provider outcome admitted before archive replay.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdmittedRainArchiveReceiptOutcome {
    Invalid,
    Valid,
}

/// One typed receipt admitted from the verified archive package.
#[derive(Debug, Clone, PartialEq)]
pub struct AdmittedRainArchiveReceipt {
    pub body_member_path: String,
    pub body_sha256: String,
    pub claim_id: String,
    pub completed_at: String,
    pub outcome: AdmittedRainArchiveReceiptOutcome,
}

/// One verified compressed body member retained by archive path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdmittedRainArchiveBody {
    pub member_path: String,
    pub compressed: Vec<u8>,
}

/// One typed stored run and the package row identity that carried it.
#[derive(Debug, Clone, PartialEq)]
pub struct AdmittedRainArchiveRun {
    pub record_revision_identity: String,
    pub stored: RainAdjustmentRun,
}

/// Typed rain material emitted by the existing package verifier.
#[derive(Debug, Clone, PartialEq)]
pub struct AdmittedRainArchivePackage {
    pub bodies: Vec<AdmittedRainArchiveBody>,
    pub claims: Vec<AdmittedRainArchiveClaim>,
    pub edges: Vec<AdmittedRainEdgeEvidence>,
    pub receipts: Vec<AdmittedRainArchiveReceipt>,
    pub runs: Vec<AdmittedRainArchiveRun>,
}

/// Fully composed report plus its replay evidence for callers needing diagnostics.
#[derive(Debug, Clone, PartialEq)]
pub struct ArchivedRainPerformanceReport {
    pub document: JsonValue,
    pub evidence: RainPerformanceEvidence,
    pub replayed_run_count: usize,
    pub verified_capture_count: usize,
}

/// Approved body, capture, target, evidence, and calendar owners.
///
/// The retained model is fixed natively while feature admission and UTC formatting remain separate
/// explicit authorities on the composition function.
pub trait RainArchivePerformanceOwners:
    WorkerRainAdjustmentCaptureValues
    + RainArchiveTargetIdentityValues
    + RainPerformanceEvidenceValues
    + RainPerformanceReportCalendar
{
    /// Inflate one verified gzip member under the original two-megabyte limit.
    fn gunzip_bounded(
        &mut self,
        compressed: &[u8],
        maximum_output_bytes: usize,
    ) -> Result<Vec<u8>, TaskFailure>;

    /// Hash one exact decompressed provider body through the approved digest owner.
    fn body_sha256(&mut self, body: &[u8]) -> Result<String, TaskFailure>;
}

/// Closed failures from the post-verification archive replay pipeline.
#[derive(Debug, Clone, PartialEq)]
pub enum RainArchivePerformanceError {
    BodyCodec(TaskFailure),
    BodyHash(TaskFailure),
    CaptureProjection(TaskFailure),
    DuplicateBodyMember,
    Evidence(RainPerformanceEvidenceError),
    ForecastClock(TaskFailure),
    InvalidClaimShape,
    MissingBody,
    MissingClaim,
    MissingSourceForecast,
    BestMatch(RainPerformanceBestMatchError),
    Replay(TaskFailure),
    ReplayIdentity,
    Report(RainPerformanceReportError),
    Target(TaskFailure),
}

/// Native target adapter over one shared reconstructed capture slice.
struct ArchiveRainPerformanceEvidenceValues<'a, Owners> {
    captures: &'a [RainAdjustmentCapture],
    owners: &'a mut Owners,
    target_cache: BTreeMap<String, RainPerformanceTarget>,
    target_failure: Option<TaskFailure>,
}

impl<Owners> RainPerformanceEvidenceValues for ArchiveRainPerformanceEvidenceValues<'_, Owners>
where
    Owners: RainArchivePerformanceOwners,
{
    /// delegate the admitted local calendar.
    fn local_date(&mut self, valid_at: &str) -> Option<String> {
        RainPerformanceEvidenceValues::local_date(self.owners, valid_at)
    }

    /// replay and cache the native fixed-gauge target.
    fn rain_target(&mut self, valid_at: &str) -> Option<RainPerformanceTarget> {
        // stop replaying after the first target failure
        if self.target_failure.is_some() {
            return None;
        }
        // reuse one target across evidence classes
        if let Some(target) = self.target_cache.get(valid_at) {
            return Some(target.clone());
        }
        match replay_rain_archive_target(self.owners, self.captures, valid_at) {
            Ok(target) => {
                self.target_cache
                    .insert(valid_at.to_owned(), target.clone());
                Some(target)
            }
            Err(error) => {
                self.target_failure = Some(error);
                None
            }
        }
    }

    /// delegate the admitted receipt clock.
    fn receipt_clock(&mut self, value: &str) -> Option<String> {
        RainPerformanceEvidenceValues::receipt_clock(self.owners, value)
    }

    /// delegate the model-bound edge identity.
    fn edge_row_identity(
        &mut self,
        edge: &AdmittedRainEdgeEvidence,
        model_sha256: &str,
    ) -> Option<String> {
        RainPerformanceEvidenceValues::edge_row_identity(self.owners, edge, model_sha256)
    }

    /// delegate the model-free Best Match identity.
    fn best_match_edge_row_identity(&mut self, edge: &AdmittedRainEdgeEvidence) -> Option<String> {
        RainPerformanceEvidenceValues::best_match_edge_row_identity(self.owners, edge)
    }
}

impl<Owners> ForecastAdjustmentScorecardCalendar
    for ArchiveRainPerformanceEvidenceValues<'_, Owners>
where
    Owners: RainArchivePerformanceOwners,
{
    /// delegate canonical UTC parsing.
    fn canonical_utc_epoch_milliseconds(&mut self, value: &str) -> Option<i64> {
        ForecastAdjustmentScorecardCalendar::canonical_utc_epoch_milliseconds(self.owners, value)
    }

    /// delegate admitted Los Angeles dates.
    fn los_angeles_local_date(&mut self, value: &str, epoch_milliseconds: i64) -> Option<String> {
        ForecastAdjustmentScorecardCalendar::los_angeles_local_date(
            self.owners,
            value,
            epoch_milliseconds,
        )
    }
}

/// Reconstruct, replay, score, and compose one admitted rain archive report.
#[allow(clippy::too_many_arguments)]
pub fn create_archived_rain_performance_report(
    package: AdmittedRainArchivePackage,
    report_inputs: &RainPerformanceReportPackageInputs,
    generated_at: &str,
    source_revision: &str,
    owners: &mut impl RainArchivePerformanceOwners,
    admission: &mut impl RainAdjustmentFeatureAdmission,
    replay_time: &mut impl RainAdjustmentTimeAdapter,
) -> Result<ArchivedRainPerformanceReport, RainArchivePerformanceError> {
    let captures = reconstruct_rain_captures(&package, owners)?;
    let mut replays = Vec::with_capacity(package.runs.len());
    // replay stored runs in their admitted package order
    for run in &package.runs {
        replays.push(replay_saved_run(
            &captures,
            run,
            owners,
            admission,
            replay_time,
        )?);
    }
    let admitted = package
        .runs
        .iter()
        .zip(&replays)
        .map(|(run, replay)| AdmittedRainPerformanceRun {
            record_revision_identity: &run.record_revision_identity,
            replay,
            stored: &run.stored,
        })
        .collect::<Vec<_>>();
    let (evidence, best_match) = {
        let mut evidence_values = ArchiveRainPerformanceEvidenceValues {
            captures: &captures,
            owners,
            target_cache: BTreeMap::new(),
            target_failure: None,
        };
        let evidence_result =
            build_rain_performance_evidence(&admitted, &package.edges, &mut evidence_values);
        // preserve the target owner's concrete failure
        if let Some(error) = evidence_values.target_failure.take() {
            return Err(RainArchivePerformanceError::Target(error));
        }
        let evidence = evidence_result.map_err(RainArchivePerformanceError::Evidence)?;
        let best_match_result =
            build_rain_performance_best_match_diagnostic(&package.edges, &mut evidence_values);
        // preserve the target owner's concrete failure
        if let Some(error) = evidence_values.target_failure.take() {
            return Err(RainArchivePerformanceError::Target(error));
        }
        let best_match = best_match_result.map_err(RainArchivePerformanceError::BestMatch)?;
        (evidence, best_match)
    };
    let document = create_rain_performance_report(
        &evidence,
        report_inputs,
        best_match.as_ref(),
        generated_at,
        source_revision,
        owners,
    )
    .map_err(RainArchivePerformanceError::Report)?;
    Ok(ArchivedRainPerformanceReport {
        document,
        evidence,
        replayed_run_count: replays.len(),
        verified_capture_count: captures.len(),
    })
}

/// Reconstruct exact raw captures from typed claims, valid receipts, and verified members.
fn reconstruct_rain_captures(
    package: &AdmittedRainArchivePackage,
    owners: &mut impl RainArchivePerformanceOwners,
) -> Result<Vec<RainAdjustmentCapture>, RainArchivePerformanceError> {
    let (claims, bodies) = index_archive_material(package)?;
    let mut captures = Vec::new();
    // preserve receipt order while skipping admitted invalid provider outcomes
    for receipt in &package.receipts {
        if receipt.outcome != AdmittedRainArchiveReceiptOutcome::Valid {
            continue;
        }
        let claim = claims
            .get(receipt.claim_id.as_str())
            .copied()
            .ok_or(RainArchivePerformanceError::MissingClaim)?;
        require_claim_shape(claim)?;
        let member = bodies
            .get(receipt.body_member_path.as_str())
            .copied()
            .ok_or(RainArchivePerformanceError::MissingBody)?;
        let body = owners
            .gunzip_bounded(&member.compressed, MAXIMUM_PROVIDER_BODY_BYTES)
            .map_err(RainArchivePerformanceError::BodyCodec)?;
        // enforce the bound even when an injected codec returns an oversized member
        if body.len() > MAXIMUM_PROVIDER_BODY_BYTES {
            return Err(RainArchivePerformanceError::BodyCodec(TaskFailure {
                attempts: 0,
                code: "rain provider body exceeds replay bound".to_owned(),
            }));
        }
        let identity = owners
            .body_sha256(&body)
            .map_err(RainArchivePerformanceError::BodyHash)?;
        // bind replay bytes to the exact valid receipt after decompression
        if identity != receipt.body_sha256 {
            return Err(RainArchivePerformanceError::BodyHash(TaskFailure {
                attempts: 0,
                code: "rain provider body hash differs after verification".to_owned(),
            }));
        }
        captures.push(RainAdjustmentCapture {
            body,
            bodySha256: receipt.body_sha256.clone(),
            claimId: receipt.claim_id.clone(),
            completedAt: receipt.completed_at.clone(),
            kind: claim.kind,
            runInitializedAt: claim.run_initialized_at.clone(),
            stationId: claim.station_id.clone(),
            windowEndExclusive: claim.window_end_exclusive.clone(),
            windowStart: claim.window_start.clone(),
        });
    }
    Ok(captures)
}

/// Select the same station plus current, six-hour, and twelve-hour forecast cohort.
fn replay_saved_run(
    captures: &[RainAdjustmentCapture],
    run: &AdmittedRainArchiveRun,
    owners: &mut impl RainArchivePerformanceOwners,
    admission: &mut impl RainAdjustmentFeatureAdmission,
    replay_time: &mut impl RainAdjustmentTimeAdapter,
) -> Result<RainWindPerformanceResult, RainArchivePerformanceError> {
    let initialized_at_ms = owners
        .parse_utc_milliseconds(&run.stored.runInitializedAt)
        .map_err(RainArchivePerformanceError::ForecastClock)?;
    let decision_at_ms = owners
        .parse_utc_milliseconds(&run.stored.decisionAt)
        .map_err(RainArchivePerformanceError::ForecastClock)?;
    // bind saved outputs to the worker's frozen eight-hour decision boundary
    if initialized_at_ms.checked_add(8 * HOUR_MILLISECONDS) != Some(decision_at_ms) {
        return Err(RainArchivePerformanceError::ReplayIdentity);
    }
    let mut cohort = Vec::new();
    let mut exact_source_count = 0usize;
    // preserve capture order while retaining the original three forecast cycles
    for capture in captures {
        match capture.kind {
            RainAdjustmentCaptureKindEnum::Station => cohort.push(capture),
            RainAdjustmentCaptureKindEnum::Forecast => {
                let Nullable::Value(run_initialized_at) = &capture.runInitializedAt else {
                    return Err(RainArchivePerformanceError::InvalidClaimShape);
                };
                let capture_initialized_at_ms =
                    owners
                        .parse_utc_milliseconds(run_initialized_at)
                        .map_err(RainArchivePerformanceError::ForecastClock)?;
                let Some(lag_ms) = initialized_at_ms.checked_sub(capture_initialized_at_ms) else {
                    continue;
                };
                // retain only exact 0, 6, and 12 hour source cycles
                if !FORECAST_COHORT_LAGS_MS.contains(&lag_ms) {
                    continue;
                }
                // bind the saved run to its exact immutable source claim
                if lag_ms == 0 && capture.claimId == run.stored.forecastClaimId {
                    exact_source_count += 1;
                }
                cohort.push(capture);
            }
        }
    }
    // refuse a missing or ambiguous saved-run source before native decoding
    if exact_source_count != 1 {
        return Err(RainArchivePerformanceError::MissingSourceForecast);
    }
    let pending = project_borrowed_worker_rain_adjustment_inputs(owners, &cohort)
        .map_err(RainArchivePerformanceError::CaptureProjection)?;
    let replay = replay_retained_rain_performance(
        &pending,
        &run.stored.forecastClaimId,
        decision_at_ms,
        admission,
        replay_time,
    )
    .map_err(RainArchivePerformanceError::Replay)?;
    // bind replay output to the same saved decision and deterministic model
    if replay.modelSha256 != run.stored.modelSha256 || replay.decisionAt != run.stored.decisionAt {
        return Err(RainArchivePerformanceError::ReplayIdentity);
    }
    Ok(replay)
}

/// Index claims with original last-wins semantics and refuse conflicting body collisions.
fn index_archive_material(
    package: &AdmittedRainArchivePackage,
) -> Result<ArchiveMaterialIndex<'_>, RainArchivePerformanceError> {
    let mut claims = BTreeMap::new();
    // reproduce the original Map.set claim replacement order
    for claim in &package.claims {
        claims.insert(claim.id.as_str(), claim);
    }
    let mut bodies = BTreeMap::<&str, &AdmittedRainArchiveBody>::new();
    // accept repeated identical members but reject ambiguous member addresses
    for body in &package.bodies {
        if let Some(previous) = bodies.get(body.member_path.as_str())
            && previous.compressed != body.compressed
        {
            return Err(RainArchivePerformanceError::DuplicateBodyMember);
        }
        bodies.insert(body.member_path.as_str(), body);
    }
    Ok((claims, bodies))
}

/// Require claim metadata to match its closed forecast or station kind.
fn require_claim_shape(
    claim: &AdmittedRainArchiveClaim,
) -> Result<(), RainArchivePerformanceError> {
    let valid = match claim.kind {
        RainAdjustmentCaptureKindEnum::Forecast => {
            matches!(&claim.run_initialized_at, Nullable::Value(_))
                && matches!(&claim.station_id, Nullable::Null)
                && matches!(&claim.window_start, Nullable::Null)
                && matches!(&claim.window_end_exclusive, Nullable::Null)
        }
        RainAdjustmentCaptureKindEnum::Station => {
            matches!(&claim.run_initialized_at, Nullable::Null)
                && matches!(&claim.station_id, Nullable::Value(value) if *value > 0)
                && matches!(&claim.window_start, Nullable::Value(_))
                && matches!(&claim.window_end_exclusive, Nullable::Value(_))
        }
    };
    // preserve the frozen collector identity shape
    if !valid || claim.id.is_empty() {
        return Err(RainArchivePerformanceError::InvalidClaimShape);
    }
    Ok(())
}

#[cfg(test)]
#[path = "rain_archive_performance_test.rs"]
mod tests;
