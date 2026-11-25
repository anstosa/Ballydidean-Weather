//! Exact v3 daily evaluation chunk admission and source-backed row projection.

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

use weather_contracts::{AdjustmentRevisionCommitReceipt, JsonValue};
use weather_edge::{
    ArchiveWirePrimitives, PrivateShadowArchiveCodec, PrivateShadowReceiptCodec,
    RevisionArchiveCanonicalCodec,
};

use crate::maintenance_canonical_json::{exact_keys, field, text, valid_hash};
use crate::native_maintenance_policy_comparison::{
    NativeMaintenancePolicyRow, create_native_maintenance_policy_row, object, string,
};
use crate::native_maintenance_policy_rain::create_native_rain_maintenance_policy_row;
use crate::native_maintenance_retirement_files_v2::format_digest;
use crate::{
    NativeMaintenanceCaptureFamily, NativeModelEvidenceValues, NativePredecessorModelCalendar,
    NativeShadowProducerCodec, NativeWirePrimitives, VerifiedNativeMaintenanceCaptureEpochWitness,
};

pub const NATIVE_MAINTENANCE_EVALUATION_CHUNK_V3_VERSION: &str =
    "adjustment-maintenance-evaluation-chunk/v3";

const CONFIRMATION_PART_VERSION: &str = "adjustment-confirmation-chunk-part/v3";
const MAXIMUM_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;
const MAXIMUM_CHUNK_BYTES: usize = 8 * 1_024 * 1_024;
const MAXIMUM_CAPSULE_BYTES: usize = 2 * 1_024 * 1_024;
const CONFIRMATION_REGISTRATION_FIELDS: &[&str] = &[
    "accessState",
    "actionIdentitySha256",
    "actionState",
    "candidateKind",
    "candidateReportSha256",
    "candidateSha256",
    "cohortLineageSha256",
    "contractVersion",
    "family",
    "firstTargetAt",
    "gateManifestSha256",
    "inputHeadSha256",
    "intervalEndExclusiveLocalDate",
    "intervalStartLocalDate",
    "registrationSha256",
    "reservedKeySha256",
    "sourceLineageSha256",
    "terminalAccessAt",
];
const ACCESS_FIELDS: &[&str] = &[
    "accessState",
    "accessSha256",
    "accessedAt",
    "actionIdentitySha256",
    "actionState",
    "candidateKind",
    "candidateReportSha256",
    "candidateSha256",
    "cohortLineageSha256",
    "contractVersion",
    "expectedKeySetSha256",
    "family",
    "firstTargetAt",
    "gateManifestSha256",
    "inputHeadSha256",
    "intervalEndExclusiveLocalDate",
    "intervalStartLocalDate",
    "registrationSha256",
    "reservedKeySha256",
    "revisionCatalogWatermarkSha256",
    "sourceLineageSha256",
    "targetComparatorSnapshotRootSha256",
    "targetCutoffAt",
    "terminalAccessAt",
];
const SHADOW_REGISTRATION_FIELDS: &[&str] = &[
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
const PART_FIELDS: &[&str] = &[
    "captureLocalDate",
    "contractVersion",
    "eligiblePredictionSubsetSha256",
    "expectedKeyCount",
    "expectedKeySubsetSha256",
    "fromLocalDate",
    "logicalChunkIndex",
    "missingKeyCount",
    "missingKeySubsetSha256",
    "partCount",
    "partIndex",
    "partSha256",
    "recordCount",
    "toLocalDateExclusive",
];
const PAYLOAD_FIELDS: &[&str] = &[
    "capsules",
    "captureLocalDate",
    "confirmationRegistrationSha256",
    "contractVersion",
    "expectedKeys",
    "family",
    "logicalChunkIndex",
    "missingKeys",
    "partCount",
    "partIndex",
    "records",
    "shadowRegistrationSha256",
];
const RECORD_FIELDS: &[&str] = &[
    "capsuleMemberSha256",
    "comparison",
    "key",
    "metric",
    "rowIndex",
];
const CAPSULE_FIELDS: &[&str] = &["capsuleBase64", "capsuleMemberSha256"];
const COMPARISON_FIELDS: &[&str] = &[
    "farmTarget",
    "firstEdgeCommittedAt",
    "incumbentPrediction",
    "membership",
    "nearestThree",
    "providerFamily",
    "stationKey",
    "target",
];
const RAIN_COMPARISON_FIELDS: &[&str] = &[
    "actualBestMatchPrediction",
    "farmTarget",
    "firstEdgeCommittedAt",
    "incumbentPrediction",
    "incumbentProbability",
    "membership",
    "nativeSourceProbability",
    "nearestThree",
    "persistencePrediction",
    "providerFamily",
    "rawTargetHourTemperatureC",
    "recentVolumeScalePrediction",
    "runKey",
    "sameWindowVolumeScalePrediction",
    "stationKey",
    "target",
    "unchangedOrdinalPrediction",
    "volumeScalePrediction",
];
const MEMBERSHIP_FIELDS: &[&str] = &[
    "actualBestMatchProjectionSha256",
    "actualBestMatchReceiptSha256",
    "incumbentMemberSha256",
    "nativeSourceProjectionSha256",
    "nativeSourceReceiptSha256",
    "targetMemberSha256",
];
const RAIN_MEMBERSHIP_FIELDS: &[&str] = &[
    "actualBestMatchProjectionSha256",
    "actualBestMatchReceiptSha256",
    "incumbentMemberSha256",
    "nativeSourceProjectionSha256",
    "nativeSourceReceiptSha256",
    "rainGateProjectionSha256",
    "rainGateReceiptSha256",
    "targetMemberSha256",
];

/// Complete immutable inputs needed before any candidate model is evaluated.
pub struct NativeMaintenanceDailyChunkInput<'a> {
    pub clock_at: &'a str,
    pub candidate_bytes: &'a [u8],
    pub artifact_bytes: &'a [u8],
    pub confirmation_registration: &'a JsonValue,
    pub access: &'a JsonValue,
    pub shadow_registration: &'a JsonValue,
    pub chunks: &'a [JsonValue],
    pub full_manifest: &'a JsonValue,
    pub witness: &'a VerifiedNativeMaintenanceCaptureEpochWitness,
}

/// One parser-verified record ready for the independently owned model replay.
#[derive(Clone, Debug, PartialEq)]
pub struct NativeMaintenanceDailyAdmittedRecord {
    pub artifact_evaluator_input: JsonValue,
    pub body: JsonValue,
    pub capsule_member_sha256: String,
    pub chunk_metadata: JsonValue,
    pub comparison: JsonValue,
    pub family: NativeMaintenanceCaptureFamily,
    pub key: String,
    pub metric: Option<String>,
    pub record_index: usize,
    pub revision_receipt: AdjustmentRevisionCommitReceipt,
    pub row_index: usize,
    pub source: JsonValue,
}

/// Exact v3 physical metadata and replay-ready records from one burned lifecycle member.
#[derive(Clone, Debug, PartialEq)]
pub struct NativeMaintenanceDailyEvaluationMaterial {
    pub chunk_metadata: Vec<JsonValue>,
    pub family: NativeMaintenanceCaptureFamily,
    pub full_manifest: JsonValue,
    pub full_member_root_sha256: String,
    pub records: Vec<NativeMaintenanceDailyAdmittedRecord>,
}

#[derive(Clone)]
struct PartMetadata {
    capture_local_date: String,
    expected_key_count: usize,
    from_local_date: String,
    logical_chunk_index: usize,
    missing_key_count: usize,
    part_count: usize,
    part_index: usize,
    part_sha256: String,
    record_count: usize,
    to_local_date_exclusive: String,
    value: JsonValue,
}

struct LifecycleBinding {
    candidate_sha256: String,
    expected_key_set_sha256: String,
    family: NativeMaintenanceCaptureFamily,
    interval_end_exclusive_local_date: String,
    interval_start_local_date: String,
    registration_sha256: String,
    reserved_key_sha256: String,
    shadow_registration_sha256: String,
    source_sha256: String,
    target_cutoff_at: String,
}

/// Admit the exact current v3 chunk transport without evaluating model bytes.
pub fn validate_native_maintenance_daily_evaluation_input(
    input: NativeMaintenanceDailyChunkInput<'_>,
) -> Option<NativeMaintenanceDailyEvaluationMaterial> {
    let wire = NativeWirePrimitives;
    let calendar = NativePredecessorModelCalendar::new()?;
    let binding = validate_lifecycle_binding(&wire, &calendar, &input)?;
    let expected_parts = family_chunk_count(binding.family);
    // preserve the source minimum physical population under the additive part grammar
    if input.chunks.len() < expected_parts {
        return None;
    }
    let mut previous = None::<PartMetadata>;
    let mut chunk_metadata = Vec::with_capacity(input.chunks.len());
    let mut parts = Vec::with_capacity(input.chunks.len());
    let mut records = Vec::new();
    let mut record_keys = HashSet::new();
    // retain physical manifest order while binding every value-bearing payload
    for (physical_index, entry) in input.chunks.iter().enumerate() {
        let metadata_value = field(entry, "metadata")?;
        // close each outer entry before reading its bounded payload
        if !exact_keys(entry, &["metadata", "payloadBase64"]) {
            return None;
        }
        let metadata = decode_part_metadata(metadata_value, binding.family)?;
        let payload_bytes =
            decode_base64(&wire, &text(entry, "payloadBase64")?, MAXIMUM_CHUNK_BYTES)?;
        let payload = decode_canonical(&wire, &payload_bytes)?;
        // bind content bytes and physical coordinates before any capsule allocation
        if digest(&wire, &payload_bytes) != metadata.part_sha256
            || !part_follows(previous.as_ref(), &metadata, physical_index)
        {
            return None;
        }
        let admitted = validate_payload(
            &wire,
            &calendar,
            &payload,
            &metadata,
            &binding,
            input.witness,
        )?;
        // prohibit the same designated prediction across physical part boundaries
        for record in admitted {
            if !record_keys.insert(record.key.clone()) {
                return None;
            }
            records.push(record);
        }
        chunk_metadata.push(metadata.value.clone());
        parts.push(metadata.clone());
        previous = Some(metadata);
    }
    let full_manifest = build_complete_part_manifest(&wire, &parts, &binding, input.access)?;
    // bind scoring to the exact already-finalized full member
    if &full_manifest != input.full_manifest {
        return None;
    }
    let full_member_root_sha256 = text(&full_manifest, "fullMemberRootSha256")?;
    Some(NativeMaintenanceDailyEvaluationMaterial {
        chunk_metadata,
        family: binding.family,
        full_manifest,
        full_member_root_sha256,
        records,
    })
}

/// Project one replay only after raw-candidate and packaged decisions agree byte for byte.
pub fn project_native_maintenance_daily_policy_row(
    record: &NativeMaintenanceDailyAdmittedRecord,
    candidate_decision_bytes: &[u8],
    artifact_decision_bytes: &[u8],
) -> Option<NativeMaintenancePolicyRow> {
    let wire = NativeWirePrimitives;
    // preserve the source's two independent model paths before interpreting a decision
    if candidate_decision_bytes != artifact_decision_bytes {
        return None;
    }
    let decision = decode_canonical(&wire, candidate_decision_bytes)?;
    let JsonValue::Array(body_rows) = field(&record.body, "rows")? else {
        return None;
    };
    let JsonValue::Array(source_rows) = field(&record.source, "rows")? else {
        return None;
    };
    let body_row = body_rows.get(record.row_index)?;
    let source_row = source_rows.get(record.row_index)?;
    let replay = replay_projection(record, body_row, source_row, &decision)?;
    let calendar = NativePredecessorModelCalendar::new()?;
    let valid_at = text(body_row, "validAt")?;
    let common = object(vec![
        (
            "actualBestMatchPrediction",
            JsonValue::Number(replay.actual_best_match_prediction),
        ),
        ("applied", JsonValue::Boolean(replay.applied)),
        (
            "candidatePrediction",
            JsonValue::Number(replay.candidate_prediction),
        ),
        ("evidenceClass", string("prospective_receipt")),
        ("farmTarget", clone_field(&record.comparison, "farmTarget")?),
        (
            "firstEdgeCommittedAt",
            clone_field(&record.comparison, "firstEdgeCommittedAt")?,
        ),
        ("horizonHours", clone_field(body_row, "leadHours")?),
        (
            "incumbentPrediction",
            clone_field(&record.comparison, "incumbentPrediction")?,
        ),
        ("key", string(&record.key)),
        (
            "localDate",
            string(&calendar.features(&valid_at).ok()?.local_date),
        ),
        (
            "nativeSourcePrediction",
            JsonValue::Number(replay.native_source_prediction),
        ),
        (
            "nearestThree",
            clone_field(&record.comparison, "nearestThree")?,
        ),
        ("provenanceComplete", JsonValue::Boolean(true)),
        (
            "providerFamily",
            clone_field(&record.comparison, "providerFamily")?,
        ),
        (
            "sourceReceiptAt",
            string(&record.revision_receipt.archiveCommittedAt),
        ),
        ("sourceRowSha256", clone_field(body_row, "sourceRowSha256")?),
        ("stationKey", clone_field(&record.comparison, "stationKey")?),
        ("target", clone_field(&record.comparison, "target")?),
        (
            "targetRowSha256",
            clone_field(
                field(&record.comparison, "membership")?,
                "targetMemberSha256",
            )?,
        ),
        ("validAt", string(&valid_at)),
    ]);
    // retain rain's closed controls and probability heads under its owning row constructor
    if record.family == NativeMaintenanceCaptureFamily::Rain {
        return create_native_rain_maintenance_policy_row(
            &extend_object(
                common,
                vec![
                    ("candidateProbability", replay.candidate_probability?),
                    (
                        "incumbentProbability",
                        clone_field(&record.comparison, "incumbentProbability")?,
                    ),
                    (
                        "nativeSourceProbability",
                        clone_field(&record.comparison, "nativeSourceProbability")?,
                    ),
                    (
                        "persistencePrediction",
                        clone_field(&record.comparison, "persistencePrediction")?,
                    ),
                    (
                        "rawTargetHourTemperatureC",
                        clone_field(&record.comparison, "rawTargetHourTemperatureC")?,
                    ),
                    (
                        "recentVolumeScalePrediction",
                        clone_field(&record.comparison, "recentVolumeScalePrediction")?,
                    ),
                    ("runKey", clone_field(&record.comparison, "runKey")?),
                    (
                        "sameWindowVolumeScalePrediction",
                        clone_field(&record.comparison, "sameWindowVolumeScalePrediction")?,
                    ),
                    (
                        "unchangedOrdinalPrediction",
                        clone_field(&record.comparison, "unchangedOrdinalPrediction")?,
                    ),
                    (
                        "volumeScalePrediction",
                        clone_field(&record.comparison, "volumeScalePrediction")?,
                    ),
                ],
            )?,
            &calendar,
        );
    }
    let pair_key = if record.family == NativeMaintenanceCaptureFamily::Wind {
        let metric = record.metric.as_deref()?;
        let lead = integer(body_row, "leadHours")?;
        JsonValue::String(wind_pair_key(metric, lead)?.encode_utf16().collect())
    } else {
        JsonValue::Null
    };
    create_native_maintenance_policy_row(
        &extend_object(
            common,
            vec![
                ("family", string(record.family.wire())),
                ("pairKey", pair_key),
            ],
        )?,
        &calendar,
    )
}

struct ReplayProjection {
    actual_best_match_prediction: f64,
    applied: bool,
    candidate_prediction: f64,
    candidate_probability: Option<JsonValue>,
    native_source_prediction: f64,
}

/// Bind the closed lifecycle registration, burn record, public registration, and model bytes.
fn validate_lifecycle_binding(
    wire: &NativeWirePrimitives,
    calendar: &NativePredecessorModelCalendar,
    input: &NativeMaintenanceDailyChunkInput<'_>,
) -> Option<LifecycleBinding> {
    let registration = input.confirmation_registration;
    // close the unopened registration before recomputing its journal identity
    if !exact_keys(registration, CONFIRMATION_REGISTRATION_FIELDS)
        || text(registration, "contractVersion")? != "forecast-adjustment-lifecycle-ledger/v2"
        || text(registration, "accessState")? != "registered"
        || text(registration, "actionState")? != "none"
        || !matches!(
            field(registration, "actionIdentitySha256"),
            Some(JsonValue::Null)
        )
        || !matches!(
            field(registration, "candidateReportSha256"),
            Some(JsonValue::Null)
        )
    {
        return None;
    }
    let family = parse_family(&text(registration, "family")?)?;
    // retain the family-specific fitted candidate grammar
    if text(registration, "candidateKind")? != candidate_kind(family) {
        return None;
    }
    for name in [
        "candidateSha256",
        "cohortLineageSha256",
        "gateManifestSha256",
        "inputHeadSha256",
        "registrationSha256",
        "reservedKeySha256",
        "sourceLineageSha256",
    ] {
        // require every preregistered immutable identity
        if !valid_hash(&text(registration, name)?) {
            return None;
        }
    }
    let interval_start_local_date = local_date(registration, "intervalStartLocalDate")?;
    let interval_end_exclusive_local_date =
        local_date(registration, "intervalEndExclusiveLocalDate")?;
    let first_target_at = instant(wire, registration, "firstTargetAt")?;
    let terminal_access_at = instant(wire, registration, "terminalAccessAt")?;
    let registration_sha256 = text(registration, "registrationSha256")?;
    let unsigned = select_fields(
        registration,
        &[
            "contractVersion",
            "family",
            "candidateKind",
            "candidateSha256",
            "cohortLineageSha256",
            "sourceLineageSha256",
            "reservedKeySha256",
            "intervalStartLocalDate",
            "intervalEndExclusiveLocalDate",
            "firstTargetAt",
            "terminalAccessAt",
            "gateManifestSha256",
            "inputHeadSha256",
        ],
    )?;
    let interval_days = source_date_days(
        &interval_start_local_date,
        &interval_end_exclusive_local_date,
    )?;
    // bind the preregistration digest and exact family span
    if digest_json(wire, &unsigned)? != registration_sha256
        || interval_days != family_interval_days(family)
        || first_target_at >= terminal_access_at
    {
        return None;
    }
    validate_access(wire, input.access, registration, terminal_access_at)?;
    let shadow_registration_sha256 =
        validate_shadow_registration(wire, input.shadow_registration, family, input.witness)?;
    let target_cutoff_at = text(input.access, "targetCutoffAt")?;
    // cross-bind the private registration and public future-only registration
    if text(registration, "candidateSha256")? != text(input.shadow_registration, "candidateSha256")?
        || text(registration, "sourceLineageSha256")?
            != text(input.shadow_registration, "sourceSha256")?
        || text(registration, "reservedKeySha256")?
            != text(input.shadow_registration, "reservedKeySha256")?
        || interval_start_local_date
            != calendar
                .features(&text(input.shadow_registration, "intervalStartAt")?)
                .ok()?
                .local_date
        || interval_end_exclusive_local_date
            != calendar
                .features(&text(input.shadow_registration, "intervalEndAt")?)
                .ok()?
                .local_date
        || text(registration, "terminalAccessAt")? != text(input.shadow_registration, "terminalAt")?
        || target_cutoff_at != text(input.shadow_registration, "targetCutoffAt")?
    {
        return None;
    }
    let now = wire.canonical_utc_milliseconds(input.clock_at)?;
    let accessed = instant(wire, input.access, "accessedAt")?;
    let cutoff = instant(wire, input.access, "targetCutoffAt")?;
    // prevent value parsing before the terminal burn and frozen snapshot cutoff
    if now < terminal_access_at || now < accessed || cutoff > now {
        return None;
    }
    let candidate_sha256 = text(input.shadow_registration, "candidateSha256")?;
    let artifact_sha256 = text(input.shadow_registration, "artifactSha256")?;
    // bind raw candidate and portable artifact bytes before exposing replay inputs
    if !(2..=MAXIMUM_CHUNK_BYTES).contains(&input.candidate_bytes.len())
        || !(2..=MAXIMUM_CHUNK_BYTES).contains(&input.artifact_bytes.len())
        || digest(wire, input.candidate_bytes) != candidate_sha256
        || digest(wire, input.artifact_bytes) != artifact_sha256
    {
        return None;
    }
    Some(LifecycleBinding {
        candidate_sha256: text(input.shadow_registration, "candidateSha256")?,
        expected_key_set_sha256: text(input.access, "expectedKeySetSha256")?,
        family,
        interval_end_exclusive_local_date,
        interval_start_local_date,
        registration_sha256,
        reserved_key_sha256: text(registration, "reservedKeySha256")?,
        shadow_registration_sha256,
        source_sha256: text(input.shadow_registration, "sourceSha256")?,
        target_cutoff_at,
    })
}

/// Reproduce the v3 manifest's complete logical grouping, date coverage, and expected-key root.
fn build_complete_part_manifest(
    wire: &NativeWirePrimitives,
    parts: &[PartMetadata],
    binding: &LifecycleBinding,
    access: &JsonValue,
) -> Option<JsonValue> {
    let logical_count = family_chunk_count(binding.family);
    let mut expected_start = binding.interval_start_local_date.clone();
    let mut expected_part_roots = Vec::with_capacity(logical_count);
    let mut logical_chunks = Vec::with_capacity(logical_count);
    let mut cursor = 0usize;
    // require all fixed logical chunks and every declared physical part
    for logical_index in 0..logical_count {
        let first = parts.get(cursor)?;
        let part_count = first.part_count;
        let group = parts.get(cursor..cursor.checked_add(part_count)?)?;
        let expected_days = if logical_index + 1 == logical_count {
            family_interval_days(binding.family)
                .checked_sub(14 * i64::try_from(logical_count.checked_sub(1)?).ok()?)?
        } else {
            14
        };
        // bind one complete logical group to its exact consecutive date partition
        if group.len() != part_count
            || group.iter().enumerate().any(|(part_index, part)| {
                part.logical_chunk_index != logical_index
                    || part.part_index != part_index
                    || part.part_count != part_count
                    || part.from_local_date != expected_start
                    || source_date_days(&expected_start, &part.to_local_date_exclusive)
                        != Some(expected_days)
                    || part.to_local_date_exclusive != group[0].to_local_date_exclusive
            })
        {
            return None;
        }
        let expected_roots = group
            .iter()
            .map(|part| text(&part.value, "expectedKeySubsetSha256"))
            .collect::<Option<Vec<_>>>()?;
        let expected_root = digest_string_array(wire, &expected_roots)?;
        expected_part_roots.push(expected_root.clone());
        let eligible_roots = group
            .iter()
            .map(|part| text(&part.value, "eligiblePredictionSubsetSha256"))
            .collect::<Option<Vec<_>>>()?;
        let missing_roots = group
            .iter()
            .map(|part| text(&part.value, "missingKeySubsetSha256"))
            .collect::<Option<Vec<_>>>()?;
        let part_roots = group
            .iter()
            .map(|part| part.part_sha256.clone())
            .collect::<Vec<_>>();
        let record_count = group
            .iter()
            .try_fold(0usize, |sum, part| sum.checked_add(part.record_count))?;
        let expected_key_count = group
            .iter()
            .try_fold(0usize, |sum, part| sum.checked_add(part.expected_key_count))?;
        let missing_key_count = group
            .iter()
            .try_fold(0usize, |sum, part| sum.checked_add(part.missing_key_count))?;
        logical_chunks.push(object(vec![
            (
                "contractVersion",
                string("adjustment-confirmation-logical-chunk/v3"),
            ),
            (
                "eligiblePredictionSubsetSha256",
                string(&digest_string_array(wire, &eligible_roots)?),
            ),
            (
                "expectedKeyCount",
                JsonValue::Number(expected_key_count as f64),
            ),
            ("expectedKeySubsetSha256", string(&expected_root)),
            ("fromLocalDate", string(&expected_start)),
            ("logicalChunkIndex", JsonValue::Number(logical_index as f64)),
            (
                "missingKeyCount",
                JsonValue::Number(missing_key_count as f64),
            ),
            (
                "missingKeySubsetSha256",
                string(&digest_string_array(wire, &missing_roots)?),
            ),
            ("partCount", JsonValue::Number(part_count as f64)),
            (
                "partRootSha256",
                string(&digest_string_array(wire, &part_roots)?),
            ),
            ("recordCount", JsonValue::Number(record_count as f64)),
            (
                "toLocalDateExclusive",
                string(&group[0].to_local_date_exclusive),
            ),
        ]));
        expected_start = group[0].to_local_date_exclusive.clone();
        cursor = cursor.checked_add(part_count)?;
    }
    // require exact final coverage and the preregistered hierarchical expected population
    if cursor != parts.len()
        || expected_start != binding.interval_end_exclusive_local_date
        || digest_string_array(wire, &expected_part_roots)? != binding.expected_key_set_sha256
        || binding.expected_key_set_sha256 != binding.reserved_key_sha256
    {
        return None;
    }
    let eligible_roots = logical_chunks
        .iter()
        .map(|chunk| text(chunk, "eligiblePredictionSubsetSha256"))
        .collect::<Option<Vec<_>>>()?;
    let missing_roots = logical_chunks
        .iter()
        .map(|chunk| text(chunk, "missingKeySubsetSha256"))
        .collect::<Option<Vec<_>>>()?;
    let manifest_without_root = object(vec![
        ("accessSha256", clone_field(access, "accessSha256")?),
        ("chunkCount", JsonValue::Number(logical_count as f64)),
        ("chunks", JsonValue::Array(logical_chunks)),
        (
            "contractVersion",
            string("adjustment-confirmation-member/v3"),
        ),
        (
            "eligiblePredictionSetSha256",
            string(&digest_string_array(wire, &eligible_roots)?),
        ),
        (
            "expectedKeySetSha256",
            string(&binding.expected_key_set_sha256),
        ),
        ("family", string(binding.family.wire())),
        (
            "intervalEndExclusiveLocalDate",
            string(&binding.interval_end_exclusive_local_date),
        ),
        (
            "intervalStartLocalDate",
            string(&binding.interval_start_local_date),
        ),
        (
            "missingKeySetSha256",
            string(&digest_string_array(wire, &missing_roots)?),
        ),
        ("registrationSha256", string(&binding.registration_sha256)),
        (
            "revisionCatalogWatermarkSha256",
            clone_field(access, "revisionCatalogWatermarkSha256")?,
        ),
        (
            "targetComparatorSnapshotRootSha256",
            clone_field(access, "targetComparatorSnapshotRootSha256")?,
        ),
        ("targetCutoffAt", string(&binding.target_cutoff_at)),
    ]);
    extend_object(
        manifest_without_root.clone(),
        vec![(
            "fullMemberRootSha256",
            string(&digest_json(wire, &manifest_without_root)?),
        )],
    )
}

/// Validate the one burned access transition and its content identity.
fn validate_access(
    wire: &NativeWirePrimitives,
    access: &JsonValue,
    registration: &JsonValue,
    terminal_access_at: i64,
) -> Option<()> {
    // close the terminal burn record before comparing inherited fields
    if !exact_keys(access, ACCESS_FIELDS) || text(access, "accessState")? != "burned" {
        return None;
    }
    for name in CONFIRMATION_REGISTRATION_FIELDS {
        // preserve every registered member except the deliberate access transition
        if *name != "accessState" && field(access, name) != field(registration, name) {
            return None;
        }
    }
    for name in [
        "accessSha256",
        "expectedKeySetSha256",
        "revisionCatalogWatermarkSha256",
        "targetComparatorSnapshotRootSha256",
    ] {
        // retain each independently committed burn identity
        if !valid_hash(&text(access, name)?) {
            return None;
        }
    }
    let accessed_at = instant(wire, access, "accessedAt")?;
    instant(wire, access, "targetCutoffAt")?;
    let unsigned = without_field(access, "accessSha256")?;
    // bind the burn record and prohibit preterminal access
    if digest_json(wire, &unsigned)? != text(access, "accessSha256")?
        || accessed_at < terminal_access_at
    {
        return None;
    }
    Some(())
}

/// Validate one exact future-only public registration and return its address.
fn validate_shadow_registration(
    wire: &NativeWirePrimitives,
    registration: &JsonValue,
    family: NativeMaintenanceCaptureFamily,
    witness: &VerifiedNativeMaintenanceCaptureEpochWitness,
) -> Option<String> {
    // refuse legacy or extended registrations at the current daily boundary
    if !exact_keys(registration, SHADOW_REGISTRATION_FIELDS)
        || text(registration, "family")? != family.wire()
        || text(registration, "siteKey")? != "ballydidean"
        || text(registration, "epochWitnessSha256")? != witness.witness_sha256
    {
        return None;
    }
    for name in [
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
        // require each public lineage identity before hashing the registration preimage
        if !valid_hash(&text(registration, name)?) {
            return None;
        }
    }
    for name in [
        "intervalEndAt",
        "intervalStartAt",
        "targetCutoffAt",
        "terminalAt",
    ] {
        instant(wire, registration, name)?;
    }
    let predecessor = nullable_text(registration, "predecessorRegistrationSha256")?;
    // admit only null or one lowercase predecessor address
    if predecessor
        .as_deref()
        .is_some_and(|value| !valid_hash(value))
    {
        return None;
    }
    let components = [
        "adjustment-shadow-registration/v3".to_owned(),
        text(registration, "siteKey")?,
        text(registration, "family")?,
        text(registration, "candidateSha256")?,
        text(registration, "artifactSha256")?,
        text(registration, "policySha256")?,
        text(registration, "cohortSha256")?,
        text(registration, "reservedKeySha256")?,
        text(registration, "sourceSha256")?,
        text(registration, "epochWitnessSha256")?,
        text(registration, "scheduleContractSha256")?,
        predecessor.unwrap_or_else(|| "none".to_owned()),
        text(registration, "intervalStartAt")?,
        text(registration, "intervalEndAt")?,
        text(registration, "targetCutoffAt")?,
        text(registration, "terminalAt")?,
    ];
    let mut preimage = components.join("\n");
    preimage.push('\n');
    let identity = digest(wire, preimage.as_bytes());
    (identity == text(registration, "registrationSha256")?).then_some(identity)
}

/// Decode one blinded v3 part metadata record.
fn decode_part_metadata(
    value: &JsonValue,
    family: NativeMaintenanceCaptureFamily,
) -> Option<PartMetadata> {
    // close all part members before validating the bounded additive coordinates
    if !exact_keys(value, PART_FIELDS)
        || text(value, "contractVersion")? != CONFIRMATION_PART_VERSION
    {
        return None;
    }
    let logical_chunk_index = integer(value, "logicalChunkIndex")?;
    let part_index = integer(value, "partIndex")?;
    let part_count = integer(value, "partCount")?;
    let record_count = integer(value, "recordCount")?;
    let expected_key_count = integer(value, "expectedKeyCount")?;
    let missing_key_count = integer(value, "missingKeyCount")?;
    // retain the source physical ceilings and complete expected-key partition count
    if logical_chunk_index >= family_chunk_count(family)
        || part_count == 0
        || part_count > 24
        || part_index >= part_count
        || record_count > 8_192
        || expected_key_count == 0
        || expected_key_count > 8_192
        || missing_key_count > 8_192
        || record_count.checked_add(missing_key_count)? != expected_key_count
    {
        return None;
    }
    for name in [
        "partSha256",
        "expectedKeySubsetSha256",
        "eligiblePredictionSubsetSha256",
        "missingKeySubsetSha256",
    ] {
        // require all blinded roots before payload access
        if !valid_hash(&text(value, name)?) {
            return None;
        }
    }
    Some(PartMetadata {
        capture_local_date: local_date(value, "captureLocalDate")?,
        expected_key_count,
        from_local_date: local_date(value, "fromLocalDate")?,
        logical_chunk_index,
        missing_key_count,
        part_count,
        part_index,
        part_sha256: text(value, "partSha256")?,
        record_count,
        to_local_date_exclusive: local_date(value, "toLocalDateExclusive")?,
        value: value.clone(),
    })
}

/// Preserve the exact physical coordinate order selected by the source.
fn part_follows(
    previous: Option<&PartMetadata>,
    current: &PartMetadata,
    _physical_index: usize,
) -> bool {
    let Some(previous) = previous else {
        return current.logical_chunk_index == 0 && current.part_index == 0;
    };
    // continue one logical chunk only through its fixed part population
    if current.logical_chunk_index == previous.logical_chunk_index {
        return current.part_count == previous.part_count
            && current.part_index == previous.part_index + 1
            && current.capture_local_date > previous.capture_local_date
            && current.from_local_date == previous.from_local_date
            && current.to_local_date_exclusive == previous.to_local_date_exclusive;
    }
    previous.part_index == previous.part_count - 1
        && current.part_index == 0
        && current.logical_chunk_index == previous.logical_chunk_index + 1
}

/// Validate one value-bearing v3 payload and decode each source-backed record.
fn validate_payload(
    wire: &NativeWirePrimitives,
    calendar: &NativePredecessorModelCalendar,
    payload: &JsonValue,
    metadata: &PartMetadata,
    binding: &LifecycleBinding,
    witness: &VerifiedNativeMaintenanceCaptureEpochWitness,
) -> Option<Vec<NativeMaintenanceDailyAdmittedRecord>> {
    // close the current transport grammar before reading arrays
    if !exact_keys(payload, PAYLOAD_FIELDS)
        || text(payload, "contractVersion")? != NATIVE_MAINTENANCE_EVALUATION_CHUNK_V3_VERSION
        || text(payload, "family")? != binding.family.wire()
        || text(payload, "captureLocalDate")? != metadata.capture_local_date
        || integer(payload, "logicalChunkIndex")? != metadata.logical_chunk_index
        || integer(payload, "partCount")? != metadata.part_count
        || integer(payload, "partIndex")? != metadata.part_index
        || text(payload, "confirmationRegistrationSha256")? != binding.registration_sha256
        || text(payload, "shadowRegistrationSha256")? != binding.shadow_registration_sha256
    {
        return None;
    }
    let expected_keys = string_array(payload, "expectedKeys")?;
    let missing_keys = string_array(payload, "missingKeys")?;
    let JsonValue::Array(record_values) = field(payload, "records")? else {
        return None;
    };
    // bind declared counts before decoding any record or capsule
    if expected_keys.len() != metadata.expected_key_count
        || missing_keys.len() != metadata.missing_key_count
        || record_values.len() != metadata.record_count
        || !sorted_unique(&expected_keys)
        || !sorted_unique(&missing_keys)
    {
        return None;
    }
    let capsules = validate_capsules(wire, field(payload, "capsules")?)?;
    let mut record_keys = Vec::with_capacity(record_values.len());
    let mut referenced_capsules = HashSet::new();
    let mut admitted = Vec::with_capacity(record_values.len());
    // validate selectors and replay material in the source record order
    for (record_index, record) in record_values.iter().enumerate() {
        let capsule_identity = validate_record(record, binding.family)?;
        referenced_capsules.insert(capsule_identity.clone());
        let key = text(record, "key")?;
        record_keys.push(key.clone());
        let capsule_bytes = capsules.get(&capsule_identity)?;
        admitted.push(decode_admitted_record(
            wire,
            calendar,
            record,
            record_index,
            capsule_identity,
            capsule_bytes,
            metadata,
            binding,
            witness,
        )?);
    }
    // require one exact sorted deduplicated capsule table for the record references
    if capsules.len() != referenced_capsules.len()
        || referenced_capsules
            .iter()
            .any(|identity| !capsules.contains_key(identity))
        || !sorted_unique(&record_keys)
    {
        return None;
    }
    let mut partition = record_keys.clone();
    partition.extend(missing_keys.iter().cloned());
    partition.sort_by(|left, right| utf16_cmp(left, right));
    // bind the complete disjoint key partition and all three blinded subset roots
    if !sorted_unique(&partition)
        || partition != expected_keys
        || digest_string_array(wire, &expected_keys)?
            != text(&metadata.value, "expectedKeySubsetSha256")?
        || digest_string_array(wire, &record_keys)?
            != text(&metadata.value, "eligiblePredictionSubsetSha256")?
        || digest_string_array(wire, &missing_keys)?
            != text(&metadata.value, "missingKeySubsetSha256")?
    {
        return None;
    }
    Some(admitted)
}

/// Validate one sorted capsule table and retain each exact member byte string once.
fn validate_capsules(
    wire: &NativeWirePrimitives,
    value: &JsonValue,
) -> Option<HashMap<String, Vec<u8>>> {
    let JsonValue::Array(entries) = value else {
        return None;
    };
    // preserve the source record ceiling before allocating decoded members
    if entries.len() > 8_192 {
        return None;
    }
    let mut capsules = HashMap::with_capacity(entries.len());
    let mut identities = Vec::with_capacity(entries.len());
    // bind each exact capsule address before record selection
    for entry in entries {
        if !exact_keys(entry, CAPSULE_FIELDS) {
            return None;
        }
        let identity = text(entry, "capsuleMemberSha256")?;
        let bytes = decode_base64(wire, &text(entry, "capsuleBase64")?, MAXIMUM_CAPSULE_BYTES)?;
        // prohibit byte substitution and repeated table identities
        if !valid_hash(&identity)
            || digest(wire, &bytes) != identity
            || capsules.insert(identity.clone(), bytes).is_some()
        {
            return None;
        }
        identities.push(identity);
    }
    sorted_unique(&identities).then_some(capsules)
}

/// Validate one record selector and its closed comparison projection.
fn validate_record(record: &JsonValue, family: NativeMaintenanceCaptureFamily) -> Option<String> {
    // close the v2/v3 derived-target selector before reading its comparison
    if !exact_keys(record, RECORD_FIELDS) {
        return None;
    }
    let key = bounded_text(record, "key", 512)?;
    let identity = text(record, "capsuleMemberSha256")?;
    let row_index = integer(record, "rowIndex")?;
    let metric = nullable_text(record, "metric")?;
    // retain the family output geometry and fixed maximum capsule row count
    if key.is_empty()
        || !valid_hash(&identity)
        || row_index >= 168
        || match family {
            NativeMaintenanceCaptureFamily::Wind => {
                !matches!(metric.as_deref(), Some("windGustMps" | "windSpeedMps"))
            }
            NativeMaintenanceCaptureFamily::Temperature | NativeMaintenanceCaptureFamily::Rain => {
                metric.is_some()
            }
        }
    {
        return None;
    }
    validate_comparison(field(record, "comparison")?, family)?;
    Some(identity)
}

/// Validate the exact target/incumbent comparison and all immutable member identities.
fn validate_comparison(value: &JsonValue, family: NativeMaintenanceCaptureFamily) -> Option<()> {
    let comparison_fields = if family == NativeMaintenanceCaptureFamily::Rain {
        RAIN_COMPARISON_FIELDS
    } else {
        COMPARISON_FIELDS
    };
    // close the family comparison before decoding physical values
    if !exact_keys(value, comparison_fields) {
        return None;
    }
    let membership = field(value, "membership")?;
    let membership_fields = if family == NativeMaintenanceCaptureFamily::Rain {
        RAIN_MEMBERSHIP_FIELDS
    } else {
        MEMBERSHIP_FIELDS
    };
    // retain each source, comparator, target, and receipt member address
    if !exact_keys(membership, membership_fields)
        || membership_fields
            .iter()
            .any(|name| text(membership, name).is_none_or(|value| !valid_hash(&value)))
    {
        return None;
    }
    finite_number(value, "incumbentPrediction")?;
    finite_number(value, "target")?;
    nullable_finite_number(value, "farmTarget")?;
    nullable_boolean(value, "nearestThree")?;
    nullable_bounded_text(value, "providerFamily", 128)?;
    nullable_bounded_text(value, "stationKey", 128)?;
    NativeWirePrimitives.canonical_utc_milliseconds(&text(value, "firstEdgeCommittedAt")?)?;
    // retain the rain-only fixed controls and probability heads
    if family == NativeMaintenanceCaptureFamily::Rain {
        let actual = finite_number(value, "actualBestMatchPrediction")?;
        if !(0.0..=2_000.0).contains(&actual) {
            return None;
        }
        for name in [
            "persistencePrediction",
            "rawTargetHourTemperatureC",
            "recentVolumeScalePrediction",
            "sameWindowVolumeScalePrediction",
            "unchangedOrdinalPrediction",
            "volumeScalePrediction",
        ] {
            finite_number(value, name)?;
        }
        bounded_text(value, "runKey", 256)?;
        probability(value, "incumbentProbability")?;
        probability(value, "nativeSourceProbability")?;
    }
    Some(())
}

/// Parse and cross-bind one archived capsule to its record geometry and future-only epoch.
#[allow(clippy::too_many_arguments)]
fn decode_admitted_record(
    wire: &NativeWirePrimitives,
    calendar: &NativePredecessorModelCalendar,
    record: &JsonValue,
    record_index: usize,
    capsule_member_sha256: String,
    capsule_bytes: &[u8],
    metadata: &PartMetadata,
    binding: &LifecycleBinding,
    witness: &VerifiedNativeMaintenanceCaptureEpochWitness,
) -> Option<NativeMaintenanceDailyAdmittedRecord> {
    let codec = NativeShadowProducerCodec::new(NativeModelEvidenceValues::new());
    let capsule = PrivateShadowReceiptCodec::decode_capsule_document(&codec, capsule_bytes)?;
    // run the producer's complete source/body/comparator admission over retained exact bytes
    if !PrivateShadowArchiveCodec::admit_stage_input(&codec, &capsule.stage_input) {
        return None;
    }
    let body = wire.decode_json(&capsule.stage_input.body)?;
    let source = wire.decode_json(&capsule.stage_input.sourceProjection)?;
    // bind capsule material to the selected public registration
    if text(&body, "family")? != binding.family.wire()
        || text(&body, "registrationSha256")? != binding.shadow_registration_sha256
        || text(&body, "candidateSha256")? != binding.candidate_sha256
        || text(&source, "candidateSha256")? != binding.candidate_sha256
        || text(&body, "sourceSha256")? != binding.source_sha256
        || text(&source, "sourceSha256")? != binding.source_sha256
        || text(&body, "registrationSha256")? != text(&source, "registrationSha256")?
        || text(&body, "inputSha256")? != capsule.revision_receipt.projectionSha256
        || text(&body, "sourceReceiptSha256")? != capsule.revision_receipt.projectionIdentitySha256
    {
        return None;
    }
    validate_future_only_capsule(wire, &source, &capsule.revision_receipt, witness, binding)?;
    let row_index = integer(record, "rowIndex")?;
    let JsonValue::Array(body_rows) = field(&body, "rows")? else {
        return None;
    };
    let JsonValue::Array(source_rows) = field(&source, "rows")? else {
        return None;
    };
    let body_row = body_rows.get(row_index)?;
    let source_row = source_rows.get(row_index)?;
    let metric = nullable_text(record, "metric")?;
    let expected_key = evaluation_record_key(&body, body_row, metric.as_deref())?;
    let valid_at = text(body_row, "validAt")?;
    let record_local_date = calendar.features(&valid_at).ok()?.local_date;
    let committed_at = text(field(record, "comparison")?, "firstEdgeCommittedAt")?;
    let capture_at = text(&body, "dueKey")?.strip_prefix("capture/")?.to_owned();
    let capture_local_date = calendar.features(&capture_at).ok()?.local_date;
    // bind the selected row to the physical part, registered interval, and frozen cutoff
    if text(record, "key")? != expected_key
        || record_local_date < metadata.from_local_date
        || record_local_date >= metadata.to_local_date_exclusive
        || record_local_date < binding.interval_start_local_date
        || record_local_date >= binding.interval_end_exclusive_local_date
        || wire.canonical_utc_milliseconds(&valid_at)?
            > wire.canonical_utc_milliseconds(&binding.target_cutoff_at)?
        || wire.canonical_utc_milliseconds(&committed_at)?
            > wire.canonical_utc_milliseconds(&binding.target_cutoff_at)?
        || capture_local_date != metadata.capture_local_date
        || text(body_row, "validAt")? != text(source_row, "validAt")?
    {
        return None;
    }
    Some(NativeMaintenanceDailyAdmittedRecord {
        artifact_evaluator_input: candidate_evaluator_input(&source, source_row)?,
        body,
        capsule_member_sha256,
        chunk_metadata: metadata.value.clone(),
        comparison: field(record, "comparison")?.clone(),
        family: binding.family,
        key: expected_key,
        metric,
        record_index,
        revision_receipt: capsule.revision_receipt,
        row_index,
        source,
    })
}

/// Require every retained source clock and receipt to remain inside the future-only snapshot.
fn validate_future_only_capsule(
    wire: &NativeWirePrimitives,
    source: &JsonValue,
    receipt: &AdjustmentRevisionCommitReceipt,
    witness: &VerifiedNativeMaintenanceCaptureEpochWitness,
    binding: &LifecycleBinding,
) -> Option<()> {
    let ordinal = receipt.archiveCommitOrdinal.as_bytes();
    let epoch = wire.canonical_utc_milliseconds(&witness.epoch_at)?;
    let committed = wire.canonical_utc_milliseconds(&receipt.archiveCommittedAt)?;
    let cutoff = wire.canonical_utc_milliseconds(&binding.target_cutoff_at)?;
    // preserve the source decimal ordinal grammar without narrowing twenty-digit values to u64
    if ordinal.is_empty()
        || ordinal.len() > 20
        || ordinal[0] == b'0'
        || !ordinal.iter().all(u8::is_ascii_digit)
        || committed < epoch
        || committed > cutoff
    {
        return None;
    }
    let mut instants = Vec::new();
    let JsonValue::Array(rows) = field(source, "rows")? else {
        return None;
    };
    // retain every row receipt, run, and target clock separately
    for row in rows {
        instants.push(text(row, "receivedAt")?);
        instants.push(text(row, "referenceAt")?);
        instants.push(text(row, "validAt")?);
    }
    instants.push(text(source, "issuedAt")?);
    collect_family_causal_instants(source, &mut instants)?;
    // prohibit a postepoch receipt from laundering any older source clock
    for value in instants {
        if wire.canonical_utc_milliseconds(&value)? < epoch {
            return None;
        }
    }
    Some(())
}

/// Collect the closed family-specific causal clocks without scanning arbitrary members.
fn collect_family_causal_instants(source: &JsonValue, instants: &mut Vec<String>) -> Option<()> {
    match text(source, "family")?.as_str() {
        "temperature" => {
            let state = field(source, "recentErrorState")?;
            instants.push(text(state, "targetRunInitializedAt")?);
            instants.push(text(state, "windowEndValidAt")?);
            for name in ["maximumSourceRunInitializedAt", "maximumSourceValidAt"] {
                // omit only explicit absent causal maxima
                if let Some(value) = nullable_text(state, name)? {
                    instants.push(value);
                }
            }
        }
        "wind" => {}
        "rain" => {
            let causal = field(source, "causalInputs")?;
            let JsonValue::Array(captures) = field(causal, "captureSet")? else {
                return None;
            };
            // preserve every completed capture and present capture boundary
            for capture in captures {
                instants.push(text(capture, "completedAt")?);
                for name in ["runInitializedAt", "windowStart", "windowEndExclusive"] {
                    // omit only explicit missing boundaries
                    if let Some(value) = nullable_text(capture, name)? {
                        instants.push(value);
                    }
                }
            }
            let mut runs = vec![field(causal, "currentRun")?];
            let JsonValue::Array(prior_runs) = field(causal, "priorRuns")? else {
                return None;
            };
            runs.extend(prior_runs);
            // retain every selected current and prior run clock
            for run in runs {
                instants.push(text(run, "runInitializedAt")?);
                instants.push(text(run, "completedAt")?);
            }
            let JsonValue::Array(hours) = field(causal, "stationHours")? else {
                return None;
            };
            // retain every station observation and receipt clock
            for hour in hours {
                instants.push(text(hour, "hourAt")?);
                instants.push(text(hour, "receivedAt")?);
            }
        }
        _ => return None,
    }
    Some(())
}

/// Derive the exact evaluator input from one producer-verified source row.
fn candidate_evaluator_input(source: &JsonValue, row: &JsonValue) -> Option<JsonValue> {
    match text(source, "family")?.as_str() {
        "temperature" => Some(object(vec![
            ("evaluatedAt", clone_field(source, "issuedAt")?),
            (
                "rawBestMatchTemperatureC",
                decode_nullable_binary64(field(row, "bestMatchTemperatureC64")?)?,
            ),
            ("recentErrorState", clone_field(source, "recentErrorState")?),
            (
                "sourceForecast",
                object(vec![
                    ("adapterVersion", clone_field(row, "adapterVersion")?),
                    ("dataset", string("single_run")),
                    ("firstReceivedAt", clone_field(row, "receivedAt")?),
                    ("modelCycle", clone_field(row, "modelCycle")?),
                    ("modelLeadHours", clone_field(row, "modelLeadHours")?),
                    ("providerKey", string("open-meteo")),
                    (
                        "providerResponseSha256",
                        clone_field(row, "providerResponseSha256")?,
                    ),
                    (
                        "rawRelativeHumidityPercent",
                        decode_nullable_binary64(field(row, "rawRelativeHumidityPercent64")?)?,
                    ),
                    (
                        "rawTemperatureC",
                        JsonValue::Number(binary64(row, "rawTemperatureC64")?),
                    ),
                    (
                        "rawWindSpeedMps",
                        decode_nullable_binary64(field(row, "rawWindSpeedMps64")?)?,
                    ),
                    ("runInitializedAt", clone_field(row, "referenceAt")?),
                    ("upstreamModel", string("ecmwf_ifs")),
                    ("validAt", clone_field(row, "validAt")?),
                ]),
            ),
            ("validAt", clone_field(row, "validAt")?),
        ])),
        "wind" => Some(object(vec![
            ("evaluatedAt", clone_field(source, "issuedAt")?),
            (
                "metrics",
                object(vec![
                    ("apparentTemperatureC", JsonValue::Null),
                    ("blackGlobeTemperatureC", JsonValue::Null),
                    ("cloudCoverPercent", JsonValue::Null),
                    ("pm25MicrogramsPerCubicMeter", JsonValue::Null),
                    ("precipitationMm", JsonValue::Null),
                    ("precipitationRateMmPerHour", JsonValue::Null),
                    ("pressureHpa", JsonValue::Null),
                    ("relativeHumidityPercent", JsonValue::Null),
                    (
                        "soilElectricalConductivityMicrosiemensPerCm",
                        JsonValue::Null,
                    ),
                    ("soilMoisturePercent", JsonValue::Null),
                    ("solarRadiationWm2", JsonValue::Null),
                    ("temperatureC", JsonValue::Null),
                    ("uvIndex", JsonValue::Null),
                    ("waterLevelM", JsonValue::Null),
                    ("wetBulbGlobeTemperatureC", JsonValue::Null),
                    ("windDirectionDegrees", JsonValue::Null),
                    (
                        "windGustMps",
                        decode_nullable_binary64(field(row, "windGustMps64")?)?,
                    ),
                    (
                        "windSpeedMps",
                        JsonValue::Number(binary64(row, "windSpeedMps64")?),
                    ),
                ]),
            ),
            (
                "rawForecastProvenance",
                object(vec![
                    ("adapterVersion", clone_field(row, "adapterVersion")?),
                    ("cohort", string("legacy_v4_retrieval_snapshot")),
                    ("contractEpoch", clone_field(row, "contractEpoch")?),
                    ("dataset", clone_field(row, "dataset")?),
                    ("referenceAt", clone_field(row, "referenceAt")?),
                    ("referenceKind", string("retrieval_snapshot")),
                    (
                        "sourceConfigFingerprint",
                        clone_field(row, "sourceConfigFingerprint")?,
                    ),
                    ("sourceKey", clone_field(row, "sourceKey")?),
                    ("targetLeadHours", clone_field(row, "modelLeadHours")?),
                    ("upstreamModel", clone_field(row, "upstreamModel")?),
                    ("validAt", clone_field(row, "validAt")?),
                ]),
            ),
        ])),
        "rain" => Some(source.clone()),
        _ => None,
    }
}

/// Cross-check one canonical model decision against the archived public body row.
fn replay_projection(
    record: &NativeMaintenanceDailyAdmittedRecord,
    body_row: &JsonValue,
    source_row: &JsonValue,
    decision: &JsonValue,
) -> Option<ReplayProjection> {
    match record.family {
        NativeMaintenanceCaptureFamily::Temperature => {
            let raw_best = nullable_binary64(source_row, "bestMatchTemperatureC64")?;
            let raw = binary64(source_row, "rawTemperatureC64")?;
            let active = text(decision, "state")? == "active";
            let corrected = nullable_finite_number(decision, "correctedTemperatureC")?;
            let applied = active && corrected.is_some();
            let candidate = if applied {
                corrected?
            } else {
                raw_best.unwrap_or(raw)
            };
            // require the retained body to equal the replayed temperature decision
            if boolean(body_row, "wouldApply")? != applied
                || binary64(body_row, "candidateTemperatureC64")? != candidate
            {
                return None;
            }
            Some(ReplayProjection {
                actual_best_match_prediction: raw_best.unwrap_or(raw),
                applied,
                candidate_prediction: candidate,
                candidate_probability: None,
                native_source_prediction: raw,
            })
        }
        NativeMaintenanceCaptureFamily::Wind => {
            let metric = record.metric.as_deref()?;
            let gust = metric == "windGustMps";
            let raw = binary64(
                source_row,
                if gust {
                    "windGustMps64"
                } else {
                    "windSpeedMps64"
                },
            )?;
            let applied = text(decision, "state")? == "active"
                && string_array(decision, "appliedMetrics")?
                    .iter()
                    .any(|value| value == metric);
            let candidate = if applied {
                finite_number(field(decision, "adjustedMetrics")?, metric)?
            } else {
                raw
            };
            let body_applied = boolean(
                body_row,
                if gust {
                    "gustWouldApply"
                } else {
                    "speedWouldApply"
                },
            )?;
            let body_candidate = binary64(
                body_row,
                if gust {
                    "candidateGustMps64"
                } else {
                    "candidateSpeedMps64"
                },
            )?;
            // require the selected wind metric to equal the shared runtime decision
            if body_applied != applied || body_candidate != candidate {
                return None;
            }
            Some(ReplayProjection {
                actual_best_match_prediction: raw,
                applied,
                candidate_prediction: candidate,
                candidate_probability: None,
                native_source_prediction: raw,
            })
        }
        NativeMaintenanceCaptureFamily::Rain => {
            let JsonValue::Array(hours) = field(decision, "hours")? else {
                return None;
            };
            let lead = integer(body_row, "leadHours")?;
            let hour = hours.get(lead.checked_sub(1)?)?;
            let raw = binary64(source_row, "precipitationMm64")?;
            let candidate = binary64(body_row, "candidatePrecipitationMm64")?;
            let probability = object(vec![
                (
                    "atLeast0_1",
                    JsonValue::Number(binary64(body_row, "occurrenceProbability64")?),
                ),
                (
                    "atLeast1_0",
                    JsonValue::Number(binary64(body_row, "atLeast1_0Probability64")?),
                ),
                (
                    "atLeast2_5",
                    JsonValue::Number(binary64(body_row, "atLeast2_5Probability64")?),
                ),
            ]);
            // bind every retained rain output to the selected replay hour
            if text(hour, "validAt")? != text(body_row, "validAt")?
                || boolean(hour, "applied")? != boolean(body_row, "wouldApply")?
                || finite_number(hour, "correctedPrecipitationMm")? != candidate
                || finite_number(hour, "positiveAmountMm")?
                    != binary64(body_row, "positiveAmountMm64")?
                || field(hour, "occurrenceProbabilities")? != &probability
            {
                return None;
            }
            Some(ReplayProjection {
                actual_best_match_prediction: finite_number(
                    &record.comparison,
                    "actualBestMatchPrediction",
                )?,
                applied: boolean(body_row, "wouldApply")?,
                candidate_prediction: candidate,
                candidate_probability: Some(probability),
                native_source_prediction: raw,
            })
        }
    }
}

/// Derive the immutable v3 record key from one archived prediction row.
fn evaluation_record_key(
    body: &JsonValue,
    row: &JsonValue,
    metric: Option<&str>,
) -> Option<String> {
    let family = text(body, "family")?;
    Some(format!(
        "{}/{}/{}",
        text(body, "dueKey")?,
        integer(row, "leadHours")?,
        metric.unwrap_or(&family),
    ))
}

/// Project the seven fixed one-based wind lead bands.
fn wind_pair_key(metric: &str, lead: usize) -> Option<String> {
    let start = lead
        .checked_sub(1)?
        .checked_div(24)?
        .checked_mul(24)?
        .checked_add(1)?;
    let end = start.checked_add(23)?;
    Some(format!("{metric}:{start:03}-{end:03}"))
}

/// Decode one exact canonical newline-framed JSON document.
fn decode_canonical(wire: &NativeWirePrimitives, bytes: &[u8]) -> Option<JsonValue> {
    let value = wire.decode_json(bytes)?;
    (wire.encode_sorted_json(&value)?.as_slice() == bytes).then_some(value)
}

/// Decode padded standard base64 and prove its canonical spelling and bound.
fn decode_base64(
    wire: &NativeWirePrimitives,
    encoded: &str,
    maximum_bytes: usize,
) -> Option<Vec<u8>> {
    let bytes = wire.decode_standard_base64(encoded)?;
    (bytes.len() >= 2
        && bytes.len() <= maximum_bytes
        && wire.encode_standard_base64(&bytes)?.as_str() == encoded)
        .then_some(bytes)
}

/// Hash exact bytes into the lowercase wire address.
fn digest(wire: &NativeWirePrimitives, bytes: &[u8]) -> String {
    format_digest(RevisionArchiveCanonicalCodec::sha256(wire, bytes))
}

/// Hash one recursively sorted canonical JSON value.
fn digest_json(wire: &NativeWirePrimitives, value: &JsonValue) -> Option<String> {
    Some(digest(wire, &wire.encode_sorted_json(value)?))
}

/// Hash one string array under the source canonical JSON owner.
fn digest_string_array(wire: &NativeWirePrimitives, values: &[String]) -> Option<String> {
    digest_json(
        wire,
        &JsonValue::Array(values.iter().map(|value| string(value)).collect()),
    )
}

/// Select ordered source fields into a standalone hash preimage object.
fn select_fields(value: &JsonValue, names: &[&str]) -> Option<JsonValue> {
    Some(object(
        names
            .iter()
            .map(|name| Some((*name, clone_field(value, name)?)))
            .collect::<Option<Vec<_>>>()?,
    ))
}

/// Clone one object while omitting exactly one known field.
fn without_field(value: &JsonValue, omitted: &str) -> Option<JsonValue> {
    let JsonValue::Object(fields) = value else {
        return None;
    };
    Some(JsonValue::Object(
        fields
            .iter()
            .filter(|(name, _)| !name.iter().copied().eq(omitted.encode_utf16()))
            .cloned()
            .collect(),
    ))
}

/// Extend one object without replacing an existing source-owned field.
fn extend_object(value: JsonValue, additions: Vec<(&str, JsonValue)>) -> Option<JsonValue> {
    let JsonValue::Object(mut fields) = value else {
        return None;
    };
    // prevent an extension from silently replacing source material
    if additions.iter().any(|(name, _)| {
        fields
            .iter()
            .any(|(key, _)| key.iter().copied().eq(name.encode_utf16()))
    }) {
        return None;
    }
    fields.extend(
        additions
            .into_iter()
            .map(|(name, value)| (name.encode_utf16().collect(), value)),
    );
    Some(JsonValue::Object(fields))
}

/// Clone one required object member.
fn clone_field(value: &JsonValue, name: &str) -> Option<JsonValue> {
    field(value, name).cloned()
}

/// Decode one exact nonnegative JavaScript safe integer.
fn integer(value: &JsonValue, name: &str) -> Option<usize> {
    let JsonValue::Number(number) = field(value, name)? else {
        return None;
    };
    (*number >= 0.0 && *number <= MAXIMUM_SAFE_INTEGER && number.fract() == 0.0)
        .then_some(*number as usize)
}

/// Decode one finite number without normalizing negative zero.
fn finite_number(value: &JsonValue, name: &str) -> Option<f64> {
    let JsonValue::Number(number) = field(value, name)? else {
        return None;
    };
    number.is_finite().then_some(*number)
}

/// Decode one nullable finite number.
fn nullable_finite_number(value: &JsonValue, name: &str) -> Option<Option<f64>> {
    match field(value, name)? {
        JsonValue::Null => Some(None),
        JsonValue::Number(number) if number.is_finite() => Some(Some(*number)),
        _ => None,
    }
}

/// Decode one required boolean.
fn boolean(value: &JsonValue, name: &str) -> Option<bool> {
    let JsonValue::Boolean(value) = field(value, name)? else {
        return None;
    };
    Some(*value)
}

/// Decode one nullable boolean.
fn nullable_boolean(value: &JsonValue, name: &str) -> Option<Option<bool>> {
    match field(value, name)? {
        JsonValue::Null => Some(None),
        JsonValue::Boolean(value) => Some(Some(*value)),
        _ => None,
    }
}

/// Decode one nullable UTF-16 string.
fn nullable_text(value: &JsonValue, name: &str) -> Option<Option<String>> {
    match field(value, name)? {
        JsonValue::Null => Some(None),
        JsonValue::String(value) => String::from_utf16(value).ok().map(Some),
        _ => None,
    }
}

/// Decode one bounded required string using JavaScript UTF-16 length.
fn bounded_text(value: &JsonValue, name: &str, maximum: usize) -> Option<String> {
    let JsonValue::String(units) = field(value, name)? else {
        return None;
    };
    (!units.is_empty() && units.len() <= maximum).then(|| String::from_utf16(units).ok())?
}

/// Decode one bounded nullable string using JavaScript UTF-16 length.
fn nullable_bounded_text(value: &JsonValue, name: &str, maximum: usize) -> Option<Option<String>> {
    match field(value, name)? {
        JsonValue::Null => Some(None),
        JsonValue::String(units) if !units.is_empty() && units.len() <= maximum => {
            String::from_utf16(units).ok().map(Some)
        }
        _ => None,
    }
}

/// Decode one exact array of strings.
fn string_array(value: &JsonValue, name: &str) -> Option<Vec<String>> {
    let JsonValue::Array(values) = field(value, name)? else {
        return None;
    };
    values
        .iter()
        .map(|value| {
            let JsonValue::String(units) = value else {
                return None;
            };
            String::from_utf16(units).ok()
        })
        .collect()
}

/// Compare strings through JavaScript's default UTF-16 code-unit ordering.
fn utf16_cmp(left: &str, right: &str) -> Ordering {
    left.encode_utf16().cmp(right.encode_utf16())
}

/// Require one already sorted unique JavaScript string array.
fn sorted_unique(values: &[String]) -> bool {
    values
        .windows(2)
        .all(|pair| utf16_cmp(&pair[0], &pair[1]) == Ordering::Less)
}

/// Decode and validate one strict Gregorian source local date.
fn local_date(value: &JsonValue, name: &str) -> Option<String> {
    let value = text(value, name)?;
    let parsed = value.parse::<jiff::civil::Date>().ok()?;
    (value.len() == 10 && parsed.to_string() == value).then_some(value)
}

/// Calculate the source UTC-midnight interval in whole days.
fn source_date_days(start: &str, end: &str) -> Option<i64> {
    let start = crate::parse_predecessor_source_utc(&format!("{start}T00:00:00.000Z"))?;
    let end = crate::parse_predecessor_source_utc(&format!("{end}T00:00:00.000Z"))?;
    end.epoch_milliseconds
        .checked_sub(start.epoch_milliseconds)?
        .checked_div(86_400_000)
}

/// Decode one canonical UTC instant to epoch milliseconds.
fn instant(wire: &NativeWirePrimitives, value: &JsonValue, name: &str) -> Option<i64> {
    wire.canonical_utc_milliseconds(&text(value, name)?)
}

/// Decode one closed probability triplet.
fn probability(value: &JsonValue, name: &str) -> Option<()> {
    let value = field(value, name)?;
    // close and bound all three occurrence heads
    if !exact_keys(value, &["atLeast0_1", "atLeast1_0", "atLeast2_5"])
        || ["atLeast0_1", "atLeast1_0", "atLeast2_5"]
            .iter()
            .any(|name| {
                finite_number(value, name).is_none_or(|value| !(0.0..=1.0).contains(&value))
            })
    {
        return None;
    }
    Some(())
}

/// Decode one big-endian binary64 hex string.
fn binary64(value: &JsonValue, name: &str) -> Option<f64> {
    let encoded = text(value, name)?;
    decode_binary64(&encoded)
}

/// Decode one nullable big-endian binary64 member.
fn nullable_binary64(value: &JsonValue, name: &str) -> Option<Option<f64>> {
    match field(value, name)? {
        JsonValue::Null => Some(None),
        JsonValue::String(units) => {
            let encoded = String::from_utf16(units).ok()?;
            Some(Some(decode_binary64(&encoded)?))
        }
        _ => None,
    }
}

/// Decode one nullable binary64 field into its evaluator JSON representation.
fn decode_nullable_binary64(value: &JsonValue) -> Option<JsonValue> {
    match value {
        JsonValue::Null => Some(JsonValue::Null),
        JsonValue::String(units) => {
            let encoded = String::from_utf16(units).ok()?;
            Some(JsonValue::Number(decode_binary64(&encoded)?))
        }
        _ => None,
    }
}

/// Decode exactly sixteen lowercase hexadecimal digits as one finite f64.
fn decode_binary64(value: &str) -> Option<f64> {
    // reject alternate width, case, and alphabet before numeric decoding
    if value.len() != 16
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return None;
    }
    let bits = u64::from_str_radix(value, 16).ok()?;
    let decoded = f64::from_bits(bits);
    decoded.is_finite().then_some(decoded)
}

/// Parse one closed family spelling.
fn parse_family(value: &str) -> Option<NativeMaintenanceCaptureFamily> {
    match value {
        "temperature" => Some(NativeMaintenanceCaptureFamily::Temperature),
        "wind" => Some(NativeMaintenanceCaptureFamily::Wind),
        "rain" => Some(NativeMaintenanceCaptureFamily::Rain),
        _ => None,
    }
}

/// Return the original fitted candidate kind for one family.
const fn candidate_kind(family: NativeMaintenanceCaptureFamily) -> &'static str {
    match family {
        NativeMaintenanceCaptureFamily::Temperature => "temperature-delayed-mos/v1",
        NativeMaintenanceCaptureFamily::Wind => "wind-robust-hierarchical-median/v1",
        NativeMaintenanceCaptureFamily::Rain => "rain-hurdle-wind-occurrence-amount/v1",
    }
}

/// Return the source logical chunk population for one family.
const fn family_chunk_count(family: NativeMaintenanceCaptureFamily) -> usize {
    match family {
        NativeMaintenanceCaptureFamily::Temperature | NativeMaintenanceCaptureFamily::Wind => 27,
        NativeMaintenanceCaptureFamily::Rain => 24,
    }
}

/// Return the source registered interval length for one family.
const fn family_interval_days(family: NativeMaintenanceCaptureFamily) -> i64 {
    match family {
        NativeMaintenanceCaptureFamily::Temperature | NativeMaintenanceCaptureFamily::Wind => 366,
        NativeMaintenanceCaptureFamily::Rain => 334,
    }
}

#[cfg(test)]
#[path = "native_maintenance_daily_evaluation_input_test.rs"]
mod tests;

/// Admit one value-blind v3 physical part through the existing exact source parser.
pub(crate) fn validate_native_confirmation_part(
    value: &JsonValue,
    family: NativeMaintenanceCaptureFamily,
) -> bool {
    decode_part_metadata(value, family).is_some()
}

/// Reuse the source immutable sequence grammar when writing a blinded journal part.
pub(crate) fn native_confirmation_part_follows(
    previous: Option<&JsonValue>,
    current: &JsonValue,
    family: NativeMaintenanceCaptureFamily,
) -> bool {
    let Some(current) = decode_part_metadata(current, family) else {
        return false;
    };
    let previous = match previous {
        Some(value) => match decode_part_metadata(value, family) {
            Some(value) => Some(value),
            None => return false,
        },
        None => None,
    };
    part_follows(previous.as_ref(), &current, 0)
}

/// Assemble the identical v3 member manifest from its actual burned registration and immutable part metadata.
pub(crate) fn assemble_native_confirmation_part_manifest(
    registration: &JsonValue,
    access: &JsonValue,
    chunks: &[JsonValue],
) -> Option<JsonValue> {
    let wire = NativeWirePrimitives;
    let family = match text(registration, "family")?.as_str() {
        "temperature" => NativeMaintenanceCaptureFamily::Temperature,
        "wind" => NativeMaintenanceCaptureFamily::Wind,
        "rain" => NativeMaintenanceCaptureFamily::Rain,
        _ => return None,
    };
    validate_access(
        &wire,
        access,
        registration,
        instant(&wire, registration, "terminalAccessAt")?,
    )?;
    // use the same source grouping and reserved-key root as the independent value-bearing evaluator
    let binding = LifecycleBinding {
        candidate_sha256: text(registration, "candidateSha256")?,
        expected_key_set_sha256: text(access, "expectedKeySetSha256")?,
        family,
        interval_end_exclusive_local_date: text(registration, "intervalEndExclusiveLocalDate")?,
        interval_start_local_date: text(registration, "intervalStartLocalDate")?,
        registration_sha256: text(registration, "registrationSha256")?,
        reserved_key_sha256: text(registration, "reservedKeySha256")?,
        shadow_registration_sha256: String::new(),
        source_sha256: text(registration, "sourceLineageSha256")?,
        target_cutoff_at: text(access, "targetCutoffAt")?,
    };
    let parts = chunks
        .iter()
        .map(|chunk| decode_part_metadata(chunk, family))
        .collect::<Option<Vec<_>>>()?;
    for (index, part) in parts.iter().enumerate() {
        if !part_follows(
            index.checked_sub(1).and_then(|prior| parts.get(prior)),
            part,
            index,
        ) {
            return None;
        }
    }
    build_complete_part_manifest(&wire, &parts, &binding, access)
}
