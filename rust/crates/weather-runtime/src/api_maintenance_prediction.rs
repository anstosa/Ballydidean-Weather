//! Publish one original private maintenance prediction without changing serving state.

use weather_api::{
    AdjustmentMaintenanceGapCallback, AdjustmentMaintenanceOperationError,
    AdjustmentMaintenancePublicationRelay, AdmittedAdjustmentMaintenanceGap,
    AdmittedAdjustmentMaintenancePublish, AdmittedAdjustmentMaintenanceStage,
    AdmittedTemperatureCanaryRuntime, AdmittedTemperatureMosModelOwner,
    BorrowedTemperatureMosServingModel, CurrentTimeResolver,
    DatabaseAdjustmentMaintenanceRepository, ForecastTemperatureMosAdmissionBoundary,
    MaintenanceCandidateRows, MaintenanceComparatorCanonicalCodec, MaintenanceIncumbentRows,
    MaintenancePredictionCanonicalCodec, MaintenanceWindModelCalendar,
    PreparedAdjustmentMaintenancePublish, PreparedMaintenanceSource, ShadowPublishAdmissionError,
    evaluate_temperature_maintenance_candidate, evaluate_temperature_maintenance_incumbent,
    evaluate_wind_maintenance_candidate, evaluate_wind_maintenance_incumbent,
    prepare_maintenance_comparator, prepare_maintenance_prediction,
};
use weather_contracts::{
    AdjustmentMaintenanceGapInput, AdjustmentMaintenanceGapInputFamilyEnum,
    AdjustmentMaintenancePublishInput, AdjustmentMaintenancePublishReceipt,
    AdjustmentMaintenanceStageInput, AdjustmentMaintenanceStageReceipt, AdjustmentShadowFamily,
    AdjustmentShadowPrediction, AdjustmentShadowPredictionReceipt,
    CaptureApiMaintenanceShadowResult, CaptureApiMaintenanceShadowResultGap,
    CaptureApiMaintenanceShadowResultGapStatusLiteral, CaptureApiMaintenanceShadowResultPublished,
    CaptureApiMaintenanceShadowResultPublishedStatusLiteral, FieldPresence,
    ForecastAdjustmentTemperatureMaintenanceRuntimePackage,
    ForecastAdjustmentWindMaintenanceRuntimePackage, LoadedApiForecastAdjustmentRuntime,
    LoadedForecastAdjustmentTemperatureCanaryRuntime, MaintenanceShadowGapReason,
    MaintenanceShadowPredictionMetadata,
};
use weather_database::{
    AdjustmentMaintenanceDatabasePort, AdmittedApiMaintenanceRegistration,
    ApiMaintenancePredictionValues, ApiMaintenancePredictionWritePort,
    append_api_maintenance_shadow, register_api_maintenance_shadow,
};

use crate::{
    ApiMaintenanceReceiptCanonicalValues, admit_api_shadow_publish_receipt,
    admit_api_shadow_stage_receipt, temperature_comparator_incumbent, wind_comparator_authority,
};

type OperationError = AdjustmentMaintenanceOperationError;
type GapReason = MaintenanceShadowGapReason;
type CaptureResult = CaptureApiMaintenanceShadowResult;

/// Already admitted package and startup incarnation for one original model family.
pub enum ApiMaintenanceReplay<'a> {
    Temperature {
        package: &'a ForecastAdjustmentTemperatureMaintenanceRuntimePackage,
        incumbent: &'a LoadedForecastAdjustmentTemperatureCanaryRuntime,
        active_incumbent: Option<(
            &'a AdmittedTemperatureCanaryRuntime,
            &'a AdmittedTemperatureMosModelOwner<'a, BorrowedTemperatureMosServingModel<'a>>,
        )>,
        calendar: &'a dyn ForecastTemperatureMosAdmissionBoundary,
        time: &'a dyn CurrentTimeResolver,
    },
    Wind {
        package: &'a ForecastAdjustmentWindMaintenanceRuntimePackage,
        incumbent: &'a LoadedApiForecastAdjustmentRuntime,
        calendar: &'a dyn MaintenanceWindModelCalendar,
        time: &'a dyn CurrentTimeResolver,
    },
}

/// Carry the verified installed registration and incumbent authority with the original source.
pub struct ApiMaintenancePredictionInput<'a> {
    pub source: PreparedMaintenanceSource<'a>,
    pub registration: &'a AdmittedApiMaintenanceRegistration<'a>,
    pub replay: ApiMaintenanceReplay<'a>,
}

/// Owner of the actual ordered private archive and retained database operations.
pub trait ApiMaintenancePredictionPorts {
    /// Durably stage the exact body, source and incumbent comparator.
    fn stage(
        &mut self,
        input: &AdmittedAdjustmentMaintenanceStage,
    ) -> Result<AdjustmentMaintenanceStageReceipt, OperationError>;

    /// Register the already admitted installed candidate and append compact metadata.
    fn register_and_append(
        &mut self,
        registration: &AdmittedApiMaintenanceRegistration<'_>,
        prediction: &AdjustmentShadowPrediction,
    ) -> Result<AdjustmentShadowPredictionReceipt, OperationError>;

    /// Re-read independent authoritative admission and bind it before private publication.
    fn admit(
        &mut self,
        prepared: PreparedAdjustmentMaintenancePublish,
    ) -> Result<AdmittedAdjustmentMaintenancePublish, ShadowPublishAdmissionError>;

    /// Publish the exact staged bytes only after independent database admission.
    fn publish(
        &mut self,
        admitted: &AdmittedAdjustmentMaintenancePublish,
    ) -> Result<AdjustmentMaintenancePublishReceipt, OperationError>;

    /// Record the original value-free gap through the archive owner.
    fn record_gap(&mut self, gap: AdmittedAdjustmentMaintenanceGap) -> Result<(), OperationError>;
}

/// Actual facade and relay adapter over the private API's one retained pool.
pub struct RetainedApiMaintenancePredictionPorts<'a, Database, Values, Relay> {
    pub repository: &'a mut DatabaseAdjustmentMaintenanceRepository<Database>,
    pub values: &'a mut Values,
    pub relay: &'a mut Relay,
}

impl<Database, Values, Relay> ApiMaintenancePredictionPorts
    for RetainedApiMaintenancePredictionPorts<'_, Database, Values, Relay>
where
    Database: AdjustmentMaintenanceDatabasePort + ApiMaintenancePredictionWritePort,
    Values: ApiMaintenancePredictionValues,
    Relay: AdjustmentMaintenancePublicationRelay + AdjustmentMaintenanceGapCallback,
{
    /// Call the existing archive relay without copying its material.
    fn stage(
        &mut self,
        input: &AdmittedAdjustmentMaintenanceStage,
    ) -> Result<AdjustmentMaintenanceStageReceipt, OperationError> {
        self.relay.stage_shadow(input)
    }

    /// Execute the original registration and append facades on the retained handle.
    fn register_and_append(
        &mut self,
        registration: &AdmittedApiMaintenanceRegistration<'_>,
        prediction: &AdjustmentShadowPrediction,
    ) -> Result<AdjustmentShadowPredictionReceipt, OperationError> {
        let database = self.repository.database_mut();
        register_api_maintenance_shadow(database, registration)
            .map_err(|_| OperationError::Rejected)?;
        append_api_maintenance_shadow(database, self.values, registration.family, prediction)
            .map_err(|_| OperationError::Rejected)
    }

    /// Retain the documented distinction between the first query and receipt rejection.
    fn admit(
        &mut self,
        prepared: PreparedAdjustmentMaintenancePublish,
    ) -> Result<AdmittedAdjustmentMaintenancePublish, ShadowPublishAdmissionError> {
        self.repository.admit_shadow_publish_classified(prepared)
    }

    /// Call the existing private relay only after exact persisted admission.
    fn publish(
        &mut self,
        admitted: &AdmittedAdjustmentMaintenancePublish,
    ) -> Result<AdjustmentMaintenancePublishReceipt, OperationError> {
        self.relay.publish_shadow(admitted)
    }

    /// Forward one value-free gap without logging or retaining the raw source.
    fn record_gap(&mut self, gap: AdmittedAdjustmentMaintenanceGap) -> Result<(), OperationError> {
        self.relay.record_gap(gap)
    }
}

/// Replay the original models and publish only through the admitted archive/database sequence.
pub fn run_api_maintenance_prediction(
    input: ApiMaintenancePredictionInput<'_>,
    codec: &mut (impl MaintenancePredictionCanonicalCodec + MaintenanceComparatorCanonicalCodec),
    ports: &mut impl ApiMaintenancePredictionPorts,
    receipt_values: &impl ApiMaintenanceReceiptCanonicalValues,
) -> Result<CaptureResult, OperationError> {
    let header = input.source.projection.header();
    let gap = GapContext::from_registration(input.registration, header.due_key)?;
    // a different installed registration may never stage or append this source
    if header.registration_sha256 != input.registration.registration_sha256
        || header.candidate_sha256 != input.registration.candidate_sha256
        || header.source_sha256 != input.registration.source_sha256
        || !gap.matches_family(input.source.projection.family())
    {
        return Ok(gap.record(ports, GapReason::SourceIncomplete));
    }
    let prepared = prepare_prediction_and_comparator(&input, codec);
    let (prediction, comparator) = match prepared {
        Ok(value) => value,
        Err(reason) => return Ok(gap.record(ports, reason)),
    };
    let stage = AdmittedAdjustmentMaintenanceStage {
        relay_input: AdjustmentMaintenanceStageInput {
            body: prediction.bytes,
            comparator: FieldPresence::Value(comparator),
            metadata: prediction.metadata,
            sourceProjection: input.source.bytes,
            sourceProjectionSha256: input.source.identity.inputSha256,
        },
    };
    match stage_append_publish(stage, input.registration, ports, receipt_values) {
        Ok(published) => Ok(CaptureResult::Published(published)),
        Err(reason) => Ok(gap.record(ports, reason)),
    }
}

/// Carry out candidate work before the startup incumbent, preserving distinct gap causes.
fn prepare_prediction_and_comparator(
    input: &ApiMaintenancePredictionInput<'_>,
    codec: &mut (impl MaintenancePredictionCanonicalCodec + MaintenanceComparatorCanonicalCodec),
) -> Result<(weather_api::PreparedMaintenancePrediction, Vec<u8>), GapReason> {
    let source = &input.source;
    match &input.replay {
        ApiMaintenanceReplay::Temperature {
            package,
            incumbent,
            active_incumbent,
            calendar,
            time,
        } => {
            // bind the installed model package to the selected registration
            if package.candidateSha256 != input.registration.candidate_sha256 {
                return Err(GapReason::CandidateUnavailable);
            }
            let candidates = evaluate_temperature_maintenance_candidate(
                package,
                &source.projection,
                *time,
                *calendar,
            )
            .map_err(|_| GapReason::CandidateUnavailable)?;
            let prediction = prepare_maintenance_prediction(
                source,
                MaintenanceCandidateRows::Temperature(&candidates),
                codec,
            )
            .map_err(|_| GapReason::CandidateUnavailable)?;
            let (incumbent, authority) =
                temperature_comparator_incumbent(incumbent, *active_incumbent)
                    .ok_or(GapReason::ComparatorUnavailable)?;
            let rows = evaluate_temperature_maintenance_incumbent(incumbent, &source.projection)
                .map_err(|_| GapReason::ComparatorUnavailable)?;
            let comparator = prepare_maintenance_comparator(
                source,
                &prediction,
                Some(authority),
                MaintenanceIncumbentRows::Temperature(&rows),
                codec,
            )
            .map_err(|_| GapReason::ComparatorUnavailable)?;
            Ok((prediction, comparator))
        }
        ApiMaintenanceReplay::Wind {
            package,
            incumbent,
            calendar,
            time,
        } => {
            // bind the installed model package to the selected registration
            if package.candidateSha256 != input.registration.candidate_sha256 {
                return Err(GapReason::CandidateUnavailable);
            }
            let candidates =
                evaluate_wind_maintenance_candidate(&source.projection, package, *calendar)
                    .map_err(|_| GapReason::CandidateUnavailable)?;
            let prediction = prepare_maintenance_prediction(
                source,
                MaintenanceCandidateRows::Wind(&candidates),
                codec,
            )
            .map_err(|_| GapReason::CandidateUnavailable)?;
            let authority =
                wind_comparator_authority(incumbent).ok_or(GapReason::ComparatorUnavailable)?;
            let rows = evaluate_wind_maintenance_incumbent(
                &source.projection,
                incumbent,
                *time,
                *calendar,
            )
            .map_err(|_| GapReason::ComparatorUnavailable)?;
            let comparator = prepare_maintenance_comparator(
                source,
                &prediction,
                Some(authority),
                MaintenanceIncumbentRows::Wind(&rows),
                codec,
            )
            .map_err(|_| GapReason::ComparatorUnavailable)?;
            Ok((prediction, comparator))
        }
    }
}

/// Preserve the original categorical result when source or candidate preparation cannot run.
pub(crate) fn report_api_maintenance_capture_gap(
    registration: &AdmittedApiMaintenanceRegistration<'_>,
    due_key: &str,
    ports: &mut impl ApiMaintenancePredictionPorts,
    reason: GapReason,
) -> Result<CaptureResult, OperationError> {
    Ok(GapContext::from_registration(registration, due_key)?.record(ports, reason))
}

/// Stage, append, independently rebind and publish one original future-only body.
fn stage_append_publish(
    stage: AdmittedAdjustmentMaintenanceStage,
    registration: &AdmittedApiMaintenanceRegistration<'_>,
    ports: &mut impl ApiMaintenancePredictionPorts,
    values: &impl ApiMaintenanceReceiptCanonicalValues,
) -> Result<CaptureApiMaintenanceShadowResultPublished, GapReason> {
    let receipt = ports
        .stage(&stage)
        .map_err(|_| GapReason::ArchiveStageFailed)?;
    let stage_receipt = admit_api_shadow_stage_receipt(values, &stage, receipt)
        .map_err(|_| GapReason::ArchiveStageFailed)?;
    let compact = compact_prediction(
        &stage.relay_input.metadata,
        &stage_receipt.stageReceiptSha256,
    );
    let appended = ports
        .register_and_append(registration, &compact)
        .map_err(|_| GapReason::DatabaseAppendFailed)?;
    admit_compact_commit_clock(values, &compact.issuedAt, &appended)?;
    let body = stage.relay_input;
    let prepared = PreparedAdjustmentMaintenancePublish {
        relay_input: AdjustmentMaintenancePublishInput {
            body: body.body,
            comparator: body.comparator,
            metadata: body.metadata,
            predictionCommittedAt: FieldPresence::Value(appended.committedAt),
            revisionReceipt: appended.revisionReceipt,
            sourceProjection: body.sourceProjection,
            sourceProjectionSha256: body.sourceProjectionSha256,
            stageReceipt: stage_receipt,
        },
    };
    let admitted = ports.admit(prepared).map_err(|failure| match failure {
        ShadowPublishAdmissionError::PreliminaryRead => GapReason::ArchivePublishFailed,
        ShadowPublishAdmissionError::NotAdmitted => GapReason::DatabaseAdmissionFailed,
    })?;
    let published = ports
        .publish(&admitted)
        .map_err(|_| GapReason::ArchivePublishFailed)?;
    let published = admit_api_shadow_publish_receipt(values, &admitted, published)
        .map_err(|_| GapReason::ArchivePublishFailed)?;
    Ok(CaptureApiMaintenanceShadowResultPublished {
        predictionSha256: compact.predictionSha256,
        publishReceiptSha256: published.publishReceiptSha256,
        status: CaptureApiMaintenanceShadowResultPublishedStatusLiteral,
    })
}

/// Move only the compact identities into the original TypeScript append contract.
pub(crate) fn compact_prediction(
    metadata: &MaintenanceShadowPredictionMetadata,
    stage_sha: &str,
) -> AdjustmentShadowPrediction {
    AdjustmentShadowPrediction {
        bodyByteCount: metadata.bodyByteCount,
        candidateSha256: metadata.candidateSha256.clone(),
        dueKey: metadata.dueKey.clone(),
        inputSha256: metadata.inputSha256.clone(),
        issuedAt: metadata.issuedAt.clone(),
        maxValidAt: metadata.maxValidAt.clone(),
        minValidAt: metadata.minValidAt.clone(),
        predictionBodySha256: metadata.predictionBodySha256.clone(),
        predictionSchemaSha256: metadata.predictionSchemaSha256.clone(),
        predictionSha256: metadata.predictionSha256.clone(),
        registrationSha256: metadata.registrationSha256.clone(),
        rowCount: metadata.rowCount,
        sourceReceiptSha256: metadata.sourceReceiptSha256.clone(),
        sourceSha256: metadata.sourceSha256.clone(),
        stageReceiptSha256: stage_sha.to_owned(),
    }
}

/// Preserve the compact row's actual commit clock before an additive comparator publish.
fn admit_compact_commit_clock(
    values: &impl ApiMaintenanceReceiptCanonicalValues,
    issued_at: &str,
    appended: &AdjustmentShadowPredictionReceipt,
) -> Result<(), GapReason> {
    let issued = values.canonical_utc_epoch_milliseconds(issued_at);
    let committed = values.canonical_utc_epoch_milliseconds(&appended.committedAt);
    let archived =
        values.canonical_utc_epoch_milliseconds(&appended.revisionReceipt.archiveCommittedAt);
    // the compact clock must not predate issue or occur after the revision receipt
    match (issued, committed, archived) {
        (Some(issued), Some(committed), Some(archived))
            if issued <= committed && committed <= archived =>
        {
            Ok(())
        }
        _ => Err(GapReason::DatabaseAdmissionFailed),
    }
}

/// Admission context for one value-free API gap without serialization or raw evidence.
struct GapContext<'a> {
    due_key: &'a str,
    family: AdjustmentMaintenanceGapInputFamilyEnum,
    registration_sha256: &'a str,
}

impl<'a> GapContext<'a> {
    /// Retain the installed API family's parsed registration identity.
    fn from_registration(
        registration: &'a AdmittedApiMaintenanceRegistration<'_>,
        due_key: &'a str,
    ) -> Result<Self, OperationError> {
        let family = match registration.family {
            AdjustmentShadowFamily::Temperature => {
                AdjustmentMaintenanceGapInputFamilyEnum::Temperature
            }
            AdjustmentShadowFamily::Wind => AdjustmentMaintenanceGapInputFamilyEnum::Wind,
            AdjustmentShadowFamily::Rain => return Err(OperationError::Rejected),
        };
        Ok(Self {
            due_key,
            family,
            registration_sha256: registration.registration_sha256,
        })
    }

    /// Reject another family's projection without substituting a registration.
    fn matches_family(&self, family: &str) -> bool {
        matches!(
            (self.family, family),
            (
                AdjustmentMaintenanceGapInputFamilyEnum::Temperature,
                "temperature"
            ) | (AdjustmentMaintenanceGapInputFamilyEnum::Wind, "wind")
        )
    }

    /// Attempt a value-free gap without coupling the disposition to archive availability.
    fn record(
        self,
        ports: &mut impl ApiMaintenancePredictionPorts,
        reason: GapReason,
    ) -> CaptureResult {
        let _ = ports.record_gap(AdmittedAdjustmentMaintenanceGap {
            request: AdjustmentMaintenanceGapInput {
                dueKey: self.due_key.to_owned(),
                family: self.family,
                reason,
                registrationSha256: self.registration_sha256.to_owned(),
            },
        });
        CaptureResult::Gap(CaptureApiMaintenanceShadowResultGap {
            reason,
            status: CaptureApiMaintenanceShadowResultGapStatusLiteral,
        })
    }
}

#[cfg(test)]
#[path = "api_maintenance_prediction_test.rs"]
mod tests;
