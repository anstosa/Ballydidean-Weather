//! Construct a rain shadow publication from the worker's retained causal execution.

use weather_api::{AdmittedAdjustmentMaintenanceStage, AdmittedAdjustmentRevisionCaptureWitness};
use weather_contracts::{
    AdjustmentMaintenanceGapInput, AdjustmentMaintenanceGapInputFamilyEnum,
    AdjustmentMaintenanceStageInput, AdjustmentMaintenanceStageReceipt,
    AdjustmentRevisionCommitReceipt, FieldPresence, JsonValue,
    MaintenanceShadowComparatorAuthorityKind, MaintenanceShadowGapReason,
    MaintenanceShadowPredictionMetadata, MaintenanceShadowServingAuthority, Nullable,
    RainWindPerformanceHour, RainWindPredictionHourReasonCodeEnum,
};
use weather_database::{
    AdmittedApiMaintenanceRegistration, ApiMaintenancePredictionValues,
    RainMaintenancePredictionWritePort, append_rain_maintenance_shadow,
    register_rain_maintenance_shadow,
};
use weather_worker::{
    RainAdjustmentEvaluation, RainAdjustmentPendingInput, RainPortableReplayCalendar,
    RainShadowSourceHeader, project_rain_shadow_source,
};

use crate::api_maintenance_prediction::compact_prediction;
use crate::api_shadow_comparator_authority::admit_shadow_comparator_authority;
use crate::api_shadow_comparator_validation::validate_shadow_comparator_document;
use crate::api_shadow_epoch_projection::require_shadow_source_epoch;
use crate::api_shadow_metadata_projection::PreparedShadowSourceIdentity;
use crate::api_shadow_source_validation::validate_shadow_source;
use crate::api_shadow_wire_validation::{HOUR_MS, rows, validate_shadow_body};
use crate::maintenance_canonical_json::{field, text};
use crate::{
    ApiMaintenanceShadowEvidenceValues, LoadedWorkerRainMaintenanceCandidate,
    admit_api_shadow_stage_receipt,
};

type Gap = MaintenanceShadowGapReason;
const COMPARATOR_MAX_BYTES: usize = 512 * 1_024;
const DUE_OFFSET_MS: i64 = 35 * 60_000;

/// Previously selected installed candidate and causal worker inputs.
pub struct WorkerRainShadowInput<'a> {
    pub pending: &'a RainAdjustmentPendingInput,
    pub evaluation: &'a RainAdjustmentEvaluation,
    pub candidate: &'a LoadedWorkerRainMaintenanceCandidate,
    pub epoch: &'a AdmittedAdjustmentRevisionCaptureWitness,
    pub authority: Option<&'a MaintenanceShadowServingAuthority>,
    pub due_key: &'a str,
}

/// Borrow exact stage members; the private API derives its own metadata from them.
#[derive(Clone, Copy)]
pub struct WorkerRainShadowMembers<'a> {
    pub source: &'a [u8],
    pub body: &'a [u8],
    pub comparator: &'a [u8],
}

/// Call the original private API; it owns independent database admission before publication.
pub trait WorkerRainShadowPublicationClient {
    type Error;

    /// Stage the three immutable shadow members through the private API.
    fn stage_shadow(
        &mut self,
        members: WorkerRainShadowMembers<'_>,
    ) -> Result<AdjustmentMaintenanceStageReceipt, Self::Error>;

    /// Send the verified stage and ingest receipts back to the private API.
    fn publish_shadow(
        &mut self,
        members: WorkerRainShadowMembers<'_>,
        committed_at: &str,
        stage: &AdjustmentMaintenanceStageReceipt,
        revision: &AdjustmentRevisionCommitReceipt,
    ) -> Result<(), Self::Error>;

    /// Send one best-effort categorical shadow gap without its causal values.
    fn record_shadow_gap(&mut self, gap: &AdjustmentMaintenanceGapInput)
    -> Result<(), Self::Error>;
}

/// Local outcome of the independent shadow work; serving is not changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkerRainShadowOutcome {
    Published { prediction_sha256: String },
    Gap(Gap),
}

/// Stage through the API, append through the ingest role, then ask the API to publish.
pub fn run_worker_rain_shadow(
    values: &impl ApiMaintenanceShadowEvidenceValues,
    input: WorkerRainShadowInput<'_>,
    calendar: &mut impl RainPortableReplayCalendar,
    database: &mut impl RainMaintenancePredictionWritePort,
    database_values: &mut impl ApiMaintenancePredictionValues,
    client: &mut impl WorkerRainShadowPublicationClient,
) -> WorkerRainShadowOutcome {
    let registration = input.candidate.registration();
    let due_key = input.due_key;
    // do not send a gap or attempt stage with a noncanonical source due identity
    if require_original_rain_due(values, &input).is_none() {
        return WorkerRainShadowOutcome::Gap(Gap::SourceIncomplete);
    }
    let result = prepare_worker_rain_shadow(values, input, calendar).and_then(|stage| {
        stage_append_publish(
            values,
            stage,
            &registration,
            database,
            database_values,
            client,
        )
    });
    match result {
        Ok(prediction_sha256) => WorkerRainShadowOutcome::Published { prediction_sha256 },
        Err(reason) => {
            // preserve serving independence if the private gap relay is unavailable
            let _ = client.record_shadow_gap(&AdjustmentMaintenanceGapInput {
                family: AdjustmentMaintenanceGapInputFamilyEnum::Rain,
                registrationSha256: registration.registration_sha256.to_owned(),
                dueKey: due_key.to_owned(),
                reason,
            });
            WorkerRainShadowOutcome::Gap(reason)
        }
    }
}

/// Preserve the original ordering and classify each stage independently.
fn stage_append_publish(
    values: &impl ApiMaintenanceShadowEvidenceValues,
    stage: AdmittedAdjustmentMaintenanceStage,
    registration: &AdmittedApiMaintenanceRegistration<'_>,
    database: &mut impl RainMaintenancePredictionWritePort,
    database_values: &mut impl ApiMaintenancePredictionValues,
    client: &mut impl WorkerRainShadowPublicationClient,
) -> Result<String, Gap> {
    let input = &stage.relay_input;
    let FieldPresence::Value(comparator) = &input.comparator else {
        return Err(Gap::ComparatorUnavailable);
    };
    let members = WorkerRainShadowMembers {
        body: &input.body,
        comparator,
        source: &input.sourceProjection,
    };
    let staged = client
        .stage_shadow(members)
        .map_err(|_| Gap::ArchiveStageFailed)?;
    let staged = admit_api_shadow_stage_receipt(values, &stage, staged)
        .map_err(|_| Gap::ArchiveStageFailed)?;
    let prediction = compact_prediction(&input.metadata, &staged.stageReceiptSha256);
    register_rain_maintenance_shadow(database, registration)
        .map_err(|_| Gap::DatabaseAppendFailed)?;
    let appended = append_rain_maintenance_shadow(database, database_values, &prediction)
        .map_err(|_| Gap::DatabaseAppendFailed)?;
    client
        .publish_shadow(
            members,
            &appended.committedAt,
            &staged,
            &appended.revisionReceipt,
        )
        .map_err(|_| Gap::ArchivePublishFailed)?;
    Ok(prediction.predictionSha256)
}

/// Produce the exact original stage envelope before any archive or database call.
pub fn prepare_worker_rain_shadow(
    values: &impl ApiMaintenanceShadowEvidenceValues,
    input: WorkerRainShadowInput<'_>,
    calendar: &mut impl RainPortableReplayCalendar,
) -> Result<AdmittedAdjustmentMaintenanceStage, Gap> {
    // the scoring artifact may be v2 but the original writer still requires v1
    if !input.candidate.prepared().is_legacy_shadow_artifact() {
        return Err(Gap::CandidateUnavailable);
    }
    let (source_bytes, body_bytes, comparator, metadata) =
        prepare_documents(values, &input, calendar)?;
    Ok(AdmittedAdjustmentMaintenanceStage {
        relay_input: AdjustmentMaintenanceStageInput {
            body: body_bytes,
            comparator: FieldPresence::Value(comparator),
            sourceProjectionSha256: metadata.inputSha256.clone(),
            metadata,
            sourceProjection: source_bytes,
        },
    })
}

type ShadowDocuments = (
    Vec<u8>,
    Vec<u8>,
    Vec<u8>,
    MaintenanceShadowPredictionMetadata,
);

/// Validate local source, prediction and incumbent data with the retained relay invariants.
fn prepare_documents(
    values: &impl ApiMaintenanceShadowEvidenceValues,
    input: &WorkerRainShadowInput<'_>,
    calendar: &mut impl RainPortableReplayCalendar,
) -> Result<ShadowDocuments, Gap> {
    require_original_rain_due(values, input).ok_or(Gap::SourceIncomplete)?;
    let candidate = input
        .candidate
        .prepared()
        .replay(input.evaluation, calendar)
        .map_err(|_| Gap::SourceIncomplete)?;
    let registration = input.candidate.registration();
    let source = project_rain_shadow_source(
        input.pending,
        input.evaluation,
        RainShadowSourceHeader {
            registration_sha256: registration.registration_sha256,
            candidate_sha256: registration.candidate_sha256,
            source_sha256: registration.source_sha256,
            due_key: input.due_key,
            issued_at: &input.evaluation.run.generated_at,
        },
    )
    .ok_or(Gap::SourceIncomplete)?;
    let source_bytes = encode_original(values, &source).ok_or(Gap::SourceIncomplete)?;
    validate_shadow_source(values, &source, source_bytes.len()).ok_or(Gap::SourceIncomplete)?;
    require_shadow_source_epoch(values, &input.epoch.capture_epoch, &source)
        .map_err(|_| Gap::SourceIncomplete)?;
    let identity = PreparedShadowSourceIdentity::prepare(values, &source, &source_bytes)
        .ok_or(Gap::SourceIncomplete)?;
    let body =
        prediction_document(&source, &candidate.hours, &identity).ok_or(Gap::SourceIncomplete)?;
    let body_bytes = encode_original(values, &body).ok_or(Gap::SourceIncomplete)?;
    validate_shadow_body(values, &body, body_bytes.len()).ok_or(Gap::SourceIncomplete)?;
    let metadata = identity
        .bind_prediction(values, &body, &body_bytes)
        .ok_or(Gap::SourceIncomplete)?;

    let authority = input.authority.ok_or(Gap::ComparatorUnavailable)?;
    let comparator = comparator_document(
        &source,
        &input.evaluation.incumbent_performance.hours,
        &identity,
        &metadata,
        authority,
    )
    .ok_or(Gap::ComparatorUnavailable)?;
    let family = validate_shadow_comparator_document(&comparator, &body, &metadata)
        .ok_or(Gap::ComparatorUnavailable)?;
    let comparator_bytes =
        encode_original(values, &comparator).ok_or(Gap::ComparatorUnavailable)?;
    // enforce the original additive envelope before opening its embedded authority members
    if comparator_bytes.len() > COMPARATOR_MAX_BYTES {
        return Err(Gap::ComparatorUnavailable);
    }
    admit_shadow_comparator_authority(
        values,
        field(&comparator, "servingAuthority").ok_or(Gap::ComparatorUnavailable)?,
        family,
    )
    .ok_or(Gap::ComparatorUnavailable)?;
    Ok((source_bytes, body_bytes, comparator_bytes, metadata))
}

/// Bind the caller's due key to the actual six-hour source cycle plus 35 minutes.
fn require_original_rain_due(
    values: &impl ApiMaintenanceShadowEvidenceValues,
    input: &WorkerRainShadowInput<'_>,
) -> Option<()> {
    let initialized =
        values.canonical_utc_epoch_milliseconds(&input.evaluation.run.run_initialized_at)?;
    let due = values.canonical_utc_epoch_milliseconds(input.due_key.strip_prefix("capture/")?)?;
    (initialized.rem_euclid(6 * HOUR_MS) == 0 && initialized.checked_add(DUE_OFFSET_MS)? == due)
        .then_some(())
}

/// Build the closed rain prediction with its original candidate, amount and fallback fields.
fn prediction_document(
    source: &JsonValue,
    hours: &[RainWindPerformanceHour],
    identity: &PreparedShadowSourceIdentity<'_>,
) -> Option<JsonValue> {
    let source_rows = rows(source, "rows")?;
    // require the complete replay so no shortened candidate can be emitted
    if source_rows.len() != hours.len() {
        return None;
    }
    let body_rows = hours
        .iter()
        .zip(&identity.row_sha256)
        .enumerate()
        .map(|(index, (hour, address))| prediction_row(index, hour, address))
        .collect::<Option<Vec<_>>>()?;
    Some(object([
        (
            "contractVersion",
            string("adjustment-shadow-prediction-values/v3"),
        ),
        ("family", string("rain")),
        (
            "registrationSha256",
            field(source, "registrationSha256")?.clone(),
        ),
        ("candidateSha256", field(source, "candidateSha256")?.clone()),
        ("sourceSha256", field(source, "sourceSha256")?.clone()),
        ("dueKey", field(source, "dueKey")?.clone()),
        ("issuedAt", field(source, "issuedAt")?.clone()),
        (
            "sourceReceiptSha256",
            string(&identity.source_receipt_sha256),
        ),
        ("inputSha256", string(&identity.input_sha256)),
        ("rowCount", JsonValue::Number(hours.len() as f64)),
        ("rows", JsonValue::Array(body_rows)),
    ]))
}

/// Preserve explicit native probabilities and original raw fallback amounts on one candidate row.
fn prediction_row(
    index: usize,
    hour: &RainWindPerformanceHour,
    address: &str,
) -> Option<JsonValue> {
    let probabilities = probabilities(hour).unwrap_or([0.0; 3]);
    // retain the original raw clamp and reject invalid source numbers
    if !hour.rawPrecipitationMm.is_finite() {
        return None;
    }
    let fallback = hour.rawPrecipitationMm.min(30.0);
    let amount = match hour.positiveAmountMm {
        Nullable::Value(value) => value,
        Nullable::Null => fallback,
    };
    let selected = if hour.applied {
        hour.correctedPrecipitationMm
    } else {
        fallback
    };
    let reason = if hour.applied {
        "none"
    } else if hour.reasonCode
        == Nullable::Value(RainWindPredictionHourReasonCodeEnum::PhaseUnsupported)
    {
        "ineligible"
    } else {
        "model_unavailable"
    };
    Some(object([
        ("validAt", string(&hour.validAt)),
        ("leadHours", JsonValue::Number((index + 1) as f64)),
        ("sourceRowSha256", string(address)),
        ("occurrenceProbability64", binary64(probabilities[0])?),
        ("positiveAmountMm64", binary64(amount)?),
        ("candidatePrecipitationMm64", binary64(selected)?),
        ("atLeast1_0Probability64", binary64(probabilities[1])?),
        ("atLeast2_5Probability64", binary64(probabilities[2])?),
        ("wouldApply", JsonValue::Boolean(hour.applied)),
        ("fallbackCode", string(reason)),
    ]))
}

/// Capture the already executed incumbent or the original fully raw policy envelope.
fn comparator_document(
    source: &JsonValue,
    incumbent: &[RainWindPerformanceHour],
    identity: &PreparedShadowSourceIdentity<'_>,
    metadata: &MaintenanceShadowPredictionMetadata,
    authority: &MaintenanceShadowServingAuthority,
) -> Option<JsonValue> {
    let source_rows = rows(source, "rows")?;
    let policy_raw = authority.authorityKind == MaintenanceShadowComparatorAuthorityKind::PolicyRaw;
    // require one actual incumbent row per source lead unless serving policy explicitly stays raw
    if !policy_raw && incumbent.len() != source_rows.len() {
        return None;
    }
    let comparator_rows = source_rows
        .iter()
        .zip(&identity.row_sha256)
        .enumerate()
        .map(|(index, (row, digest))| {
            comparator_row(
                index,
                row,
                digest,
                if policy_raw {
                    None
                } else {
                    incumbent.get(index)
                },
            )
        })
        .collect::<Option<Vec<_>>>()?;
    Some(object([
        (
            "contractVersion",
            string("adjustment-shadow-incumbent-comparator/v1"),
        ),
        ("family", string("rain")),
        ("registrationSha256", string(&metadata.registrationSha256)),
        ("candidateSha256", string(&metadata.candidateSha256)),
        ("sourceSha256", string(&metadata.sourceSha256)),
        ("dueKey", string(&metadata.dueKey)),
        ("issuedAt", string(&metadata.issuedAt)),
        ("sourceProjectionSha256", string(&metadata.inputSha256)),
        (
            "predictionBodySha256",
            string(&metadata.predictionBodySha256),
        ),
        ("servingAuthority", authority_document(authority)?),
        ("rowCount", JsonValue::Number(source_rows.len() as f64)),
        ("rows", JsonValue::Array(comparator_rows)),
    ]))
}

/// Project one exact incumbent row from the admitted causal vector and retained model result.
fn comparator_row(
    index: usize,
    source: &JsonValue,
    digest: &str,
    incumbent: Option<&RainWindPerformanceHour>,
) -> Option<JsonValue> {
    let valid_at = text(source, "validAt")?;
    let (amount, probabilities, positive, applied, reason) = match incumbent {
        None => (
            field(source, "precipitationMm64")?.clone(),
            None,
            &Nullable::Null,
            false,
            string("policy_raw"),
        ),
        Some(hour) => {
            // bind the incumbent to the same target and underlying model lead
            if hour.validAt != valid_at
                || field(source, "modelLeadHours")? != &JsonValue::Number(hour.modelLeadHours)
            {
                return None;
            }
            let amount = if hour.applied {
                hour.correctedPrecipitationMm
            } else {
                hour.rawPrecipitationMm
            };
            let reason = match hour.reasonCode {
                Nullable::Null => JsonValue::Null,
                Nullable::Value(reason) => string(
                    RainWindPredictionHourReasonCodeEnum::WIRE_VALUES
                        .iter()
                        .find(|(_, value)| *value == reason)?
                        .0,
                ),
            };
            (
                binary64(amount)?,
                probabilities(hour),
                &hour.positiveAmountMm,
                hour.applied,
                reason,
            )
        }
    };
    let event = |index: usize| match probabilities {
        None => Some(JsonValue::Null),
        Some(values) => binary64(values[index]),
    };
    let positive = match positive {
        Nullable::Null => JsonValue::Null,
        Nullable::Value(value) => binary64(*value)?,
    };
    Some(object([
        ("validAt", string(&valid_at)),
        ("leadHours", JsonValue::Number((index + 1) as f64)),
        ("sourceRowSha256", string(digest)),
        ("incumbentPrecipitationMm64", amount),
        ("occurrenceProbability64", event(0)?),
        ("atLeast1_0Probability64", event(1)?),
        ("atLeast2_5Probability64", event(2)?),
        ("positiveAmountMm64", positive),
        ("applied", JsonValue::Boolean(applied)),
        ("reasonCode", reason),
    ]))
}

/// Borrow each original event probability in threshold order.
fn probabilities(hour: &RainWindPerformanceHour) -> Option<[f64; 3]> {
    let Nullable::Value(value) = &hour.occurrenceProbabilities else {
        return None;
    };
    Some([value.atLeast0_1, value.atLeast1_0, value.atLeast2_5])
}

/// Retain the original exact authority member order and nullable fields.
fn authority_document(authority: &MaintenanceShadowServingAuthority) -> Option<JsonValue> {
    let kind = MaintenanceShadowComparatorAuthorityKind::WIRE_VALUES
        .iter()
        .find(|(_, value)| *value == authority.authorityKind)?
        .0;
    let nullable = |value: &Nullable<String>| match value {
        Nullable::Value(value) => string(value),
        Nullable::Null => JsonValue::Null,
    };
    Some(object([
        ("artifactBase64", nullable(&authority.artifactBase64)),
        (
            "artifactIdentitySha256",
            nullable(&authority.artifactIdentitySha256),
        ),
        (
            "artifactMemberSha256",
            nullable(&authority.artifactMemberSha256),
        ),
        ("authorityKind", string(kind)),
        ("receiptBase64", string(&authority.receiptBase64)),
        (
            "receiptMemberSha256",
            string(&authority.receiptMemberSha256),
        ),
    ]))
}

/// Append the original LF after delegated compact ordered JSON.
pub(crate) fn encode_original(
    values: &impl ApiMaintenanceShadowEvidenceValues,
    doc: &JsonValue,
) -> Option<Vec<u8>> {
    let mut bytes = values.encode_shadow_ordered_json(doc)?;
    bytes.push(b'\n');
    Some(bytes)
}

/// Preserve the exact lowercase network-order binary64 wire value.
pub(crate) fn binary64(value: f64) -> Option<JsonValue> {
    value
        .is_finite()
        .then(|| string(&format!("{:016x}", value.to_bits())))
}

/// Build an object without changing supplied contract order.
pub(crate) fn object<const N: usize>(fields: [(&str, JsonValue); N]) -> JsonValue {
    JsonValue::Object(
        fields
            .into_iter()
            .map(|(key, value)| (key.encode_utf16().collect(), value))
            .collect(),
    )
}

/// Preserve original JSON's UTF-16 text representation.
pub(crate) fn string(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use weather_contracts::RainWindPerformanceHourOccurrenceProbabilities;

    /// Build a retained hour before selecting the serving policy.
    fn actual_hour() -> RainWindPerformanceHour {
        RainWindPerformanceHour {
            validAt: "2026-11-01T09:00:00.000Z".to_owned(),
            modelLeadHours: 9.0,
            rawPrecipitationMm: 35.0,
            correctedPrecipitationMm: 3.0,
            applied: true,
            reasonCode: Nullable::Null,
            occurrenceProbabilities: Nullable::Value(
                RainWindPerformanceHourOccurrenceProbabilities {
                    atLeast0_1: 0.75,
                    atLeast1_0: 0.5,
                    atLeast2_5: 0.25,
                },
            ),
            positiveAmountMm: Nullable::Value(4.0),
        }
    }

    /// Retain the actual heads and the original exact raw-policy null envelope.
    #[test]
    fn binds_actual_incumbent_rows_without_recomputing_and_preserves_policy_raw() {
        let hour = actual_hour();
        let raw = "4041800000000000";
        let source = object([
            ("validAt", string(&hour.validAt)),
            ("modelLeadHours", JsonValue::Number(hour.modelLeadHours)),
            ("precipitationMm64", string(raw)),
        ]);
        let active = comparator_row(0, &source, "address", Some(&hour)).expect("actual source row");
        assert_eq!(
            text(&active, "incumbentPrecipitationMm64").as_deref(),
            Some("4008000000000000")
        );
        assert_eq!(
            text(&active, "atLeast2_5Probability64").as_deref(),
            Some("3fd0000000000000")
        );
        let raw_row = comparator_row(0, &source, "address", None).expect("policy raw row");
        assert_eq!(
            text(&raw_row, "incumbentPrecipitationMm64").as_deref(),
            Some(raw)
        );
        assert_eq!(text(&raw_row, "reasonCode").as_deref(), Some("policy_raw"));
        assert!(matches!(
            field(&raw_row, "occurrenceProbability64"),
            Some(JsonValue::Null)
        ));
        let wrong = object([
            ("validAt", string(&hour.validAt)),
            ("modelLeadHours", JsonValue::Number(10.0)),
            ("precipitationMm64", string(raw)),
        ]);
        assert!(comparator_row(0, &wrong, "address", Some(&hour)).is_none());
    }

    /// Use zero probabilities and a capped raw amount for an ineligible candidate.
    #[test]
    fn preserves_the_compact_candidate_phase_and_fallback_amount() {
        let mut hour = actual_hour();
        hour.applied = false;
        hour.reasonCode = Nullable::Value(RainWindPredictionHourReasonCodeEnum::PhaseUnsupported);
        hour.occurrenceProbabilities = Nullable::Null;
        hour.positiveAmountMm = Nullable::Null;
        let row = prediction_row(0, &hour, "address").expect("retained fallback row");
        assert_eq!(text(&row, "fallbackCode").as_deref(), Some("ineligible"));
        assert_eq!(
            text(&row, "candidatePrecipitationMm64").as_deref(),
            Some("403e000000000000")
        );
        assert_eq!(
            text(&row, "positiveAmountMm64").as_deref(),
            Some("403e000000000000")
        );
        assert_eq!(
            text(&row, "occurrenceProbability64").as_deref(),
            Some("0000000000000000")
        );
    }
}
